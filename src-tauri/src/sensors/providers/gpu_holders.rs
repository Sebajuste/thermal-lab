//! Qui tient la carte NVIDIA eveillee — memoire dediee par process.
//!
//! NVML ne voit que les contextes compute : sous WDDM, une application qui garde un
//! device D3D ouvert, et qui suffit a interdire l'extinction de la carte, n'y apparait
//! jamais. Les compteurs `GPU Process Memory` de Windows comptent chaque allocation par
//! process et par adaptateur, sans reveiller la carte et sans privilege.
//!
//! C'est la memoire allouee qui designe un client, pas la charge : un process peut
//! tenir 180 Mo sur la carte a 0,00 % d'utilisation, et c'est exactement le cas cherche.
//!
//! Le compteur par process peut mentir sur la taille : releve sur la machine de
//! developpement, 2,66 To de memoire dediee pour un seul process, quand l'adaptateur
//! entier en utilisait 1,79 Go — WMI et PDH d'accord sur ce chiffre. La presence reste
//! vraie, pas la quantite : une taille qui depasse celle de l'adaptateur est declaree
//! inconnue, et le process reste compte.
//!
//! Les instances portent la LUID de leur adaptateur. Sur un portable Optimus, presque
//! tous les process tiennent de la memoire sur l'iGPU : sans filtrer sur la LUID
//! NVIDIA, on accuserait le bureau entier. NVML ne donne pas cette LUID, le registre
//! DirectX si — et elle change au redemarrage du pilote, d'ou une relecture par cycle.

use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;
use wmi::WMIConnection;

use crate::sensors::metric::{GpuHolder, Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};
use crate::sensors::wmi_context::{variant_f64, variant_string, Row};

const ID: &str = "gpu-holders";
const NAMESPACE: &str = "root\\cimv2";
const QUERY: &str = "SELECT Name, DedicatedUsage \
                     FROM Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory";
const ADAPTER_QUERY: &str = "SELECT Name, DedicatedUsage \
                     FROM Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory";

/// Marge sur le total de l'adaptateur : les deux requetes ne sont pas simultanees, et
/// un gros client peut s'en approcher. La valeur fautive observee le depassait 1500 fois.
const PLAUSIBLE_FACTOR: f64 = 2.0;

const DIRECTX_PATH: &str = r"SOFTWARE\Microsoft\DirectX";
const VENDOR_NVIDIA: u32 = 0x10DE;

const PROVIDES: &[Metric] = &[Metric::GpuHolderCount];

/// Le compte porte sur tous les clients ; la liste ne garde que les plus gros, ceux
/// qu'on peut raisonnablement nommer a l'ecran.
const MAX_LISTED: usize = 5;
const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

#[derive(Default)]
pub struct GpuHoldersProvider {
    con: Option<WMIConnection>,
}

impl GpuHoldersProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for GpuHoldersProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: crate::t!(
                "GPU clients (performance counters)",
                "Clients GPU (compteurs de performance)"
            ),
            kind: ProviderKind::Builtin,
            provides: PROVIDES,
            url: None,
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState {
        self.con = None;

        if nvidia_luids().is_empty() {
            return ProbeState::unavailable_only(crate::t!(
                "no NVIDIA adapter declared to DirectX",
                "aucun adaptateur NVIDIA déclaré à DirectX"
            ));
        }
        let Some(wmi) = ctx.wmi else {
            return ProbeState::Failed {
                error: crate::t!("COM unavailable", "COM indisponible").into(),
            };
        };
        match wmi.connect(NAMESPACE) {
            Err(e) => ProbeState::Failed { error: e },
            Ok(con) => match con.raw_query::<Row>(QUERY) {
                Err(e) => ProbeState::Failed {
                    error: crate::t!(
                        format!("GPU memory counters unreadable: {e}"),
                        format!("compteurs de mémoire GPU illisibles : {e}")
                    ),
                },
                Ok(_) => {
                    self.con = Some(con);
                    ProbeState::Ready
                }
            },
        }
    }

    fn sample(&mut self, out: &mut Reading) -> Sampled {
        let Some(con) = &self.con else {
            return Sampled::Lost;
        };
        let luids = nvidia_luids();
        if luids.is_empty() {
            return Sampled::Lost;
        }
        let Ok(rows) = con.raw_query::<Row>(QUERY) else {
            return Sampled::Lost;
        };

        // Un process peut apparaitre sous plusieurs instances — une par segment physique :
        // on additionne.
        let mut by_pid: BTreeMap<u32, f64> = BTreeMap::new();
        for r in &rows {
            let Some((pid, luid)) = variant_string(r.get("Name"))
                .as_deref()
                .and_then(parse_instance)
            else {
                continue;
            };
            let bytes = variant_f64(r.get("DedicatedUsage")).unwrap_or(0.0);
            if bytes > 0.0 && luids.contains(&luid) {
                *by_pid.entry(pid).or_default() += bytes;
            }
        }

        out.offer(Metric::GpuHolderCount, by_pid.len() as f64, ID);

        // Sans total d'adaptateur, pas de borne : on garde les tailles telles quelles.
        let bound = con.raw_query::<Row>(ADAPTER_QUERY).ok().map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let luid = variant_string(r.get("Name"))
                        .as_deref()
                        .and_then(parse_adapter)?;
                    luids
                        .contains(&luid)
                        .then(|| variant_f64(r.get("DedicatedUsage")))
                        .flatten()
                })
                .sum::<f64>()
        });

        let mut holders: Vec<(u32, Option<f64>)> = by_pid
            .into_iter()
            .map(|(pid, bytes)| (pid, plausible(bytes, bound)))
            .collect();
        // Les tailles connues d'abord, de la plus grosse a la plus petite : une taille
        // inconnue ne doit pas passer en tete sur la foi d'un chiffre faux.
        holders.sort_by(|a, b| match (a.1, b.1) {
            (Some(x), Some(y)) => y.total_cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });
        if out.gpu_holders.is_empty() {
            out.gpu_holders = holders
                .into_iter()
                .take(MAX_LISTED)
                .map(|(pid, bytes)| GpuHolder {
                    pid,
                    name: process_name(pid),
                    dedicated_mb: bytes.map(|b| b / BYTES_PER_MB),
                })
                .collect();
        }

        Sampled::Answered
    }
}

/// Les LUID que DirectX attribue aux adaptateurs NVIDIA.
fn nvidia_luids() -> BTreeSet<u64> {
    let Ok(root) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(DIRECTX_PATH, KEY_READ)
    else {
        return BTreeSet::new();
    };
    let adapters: Vec<(u32, u64)> = root
        .enum_keys()
        .flatten()
        .filter_map(|name| {
            let key = root.open_subkey_with_flags(&name, KEY_READ).ok()?;
            let vendor: u32 = key.get_value("VendorId").ok()?;
            let luid: u64 = key.get_value("AdapterLuid").ok()?;
            Some((vendor, luid))
        })
        .collect();
    select_luids(adapters, VENDOR_NVIDIA)
}

/// Les LUID du fournisseur voulu, moins celles qu'un autre adaptateur revendique aussi.
///
/// Une entree du registre survit a son adaptateur. Si sa vieille LUID a ete reattribuee
/// a l'iGPU, la garder ferait accuser les clients de l'iGPU : une LUID ambigue est
/// ecartee, l'entree vivante de l'autre adaptateur la revendiquant forcement.
fn select_luids(adapters: impl IntoIterator<Item = (u32, u64)>, vendor: u32) -> BTreeSet<u64> {
    let mut mine = BTreeSet::new();
    let mut others = BTreeSet::new();
    for (v, luid) in adapters {
        if luid == 0 {
            continue;
        }
        if v == vendor {
            mine.insert(luid);
        } else {
            others.insert(luid);
        }
    }
    mine.difference(&others).copied().collect()
}

/// Une taille au-dela de ce que l'adaptateur porte en tout n'est pas une mesure.
fn plausible(bytes: f64, adapter_total: Option<f64>) -> Option<f64> {
    match adapter_total {
        Some(total) if bytes > total * PLAUSIBLE_FACTOR => None,
        _ => Some(bytes),
    }
}

/// `pid_1288_luid_0x00000000_0x00016CE1_phys_0` → `(1288, 0x16CE1)`.
fn parse_instance(name: &str) -> Option<(u32, u64)> {
    let rest = name.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once('_')?;
    Some((pid.parse().ok()?, parse_adapter(rest)?))
}

/// `luid_0x00000000_0x00016CE1_phys_0` → `0x16CE1`.
fn parse_adapter(name: &str) -> Option<u64> {
    let mut parts = name.strip_prefix("luid_")?.split('_');
    let high = u64::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
    let low = u64::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
    Some((high << 32) | low)
}

/// Le nom de l'executable, ou rien. Un process protege, ou deja termine, ne se laisse
/// pas ouvrir : l'interface affiche alors son seul identifiant.
fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_counter_instances() {
        assert_eq!(
            parse_instance("pid_1288_luid_0x00000000_0x00016CE1_phys_0"),
            Some((1288, 0x16CE1))
        );
        assert_eq!(
            parse_instance("pid_7_luid_0x00000001_0x00000002_phys_1"),
            Some((7, (1 << 32) | 2))
        );
        assert_eq!(parse_instance("luid_0x00000000_0x00016CE1_phys_0"), None);
        assert_eq!(parse_instance("pid_x_luid_0x0_0x0_phys_0"), None);
        assert_eq!(
            parse_adapter("luid_0x00000000_0x00015485_phys_0"),
            Some(0x15485)
        );
    }

    /// Relevé réel : 2,66 To annoncés pour un process, 1,79 Go pour l'adaptateur.
    #[test]
    fn rejects_a_size_the_adapter_cannot_hold() {
        let adapter = Some(1_788_026_880.0);
        assert_eq!(plausible(2_658_178_019_328.0, adapter), None);
        assert_eq!(plausible(273_633_280.0, adapter), Some(273_633_280.0));
        // Les deux requêtes ne sont pas simultanées : un gros client peut dépasser
        // un instantané de l'adaptateur sans être faux.
        assert_eq!(plausible(1_900_000_000.0, adapter), Some(1_900_000_000.0));
        assert_eq!(plausible(5e12, None), Some(5e12));
    }

    /// Relevé réel : NVIDIA et le rendu de base de Microsoft, plus l'entrée
    /// `ShaderCache` sans LUID.
    #[test]
    fn keeps_only_the_vendor_luids() {
        let adapters = [(0x1414, 0x16CE1), (VENDOR_NVIDIA, 0x15485), (0, 0)];
        assert_eq!(
            select_luids(adapters, VENDOR_NVIDIA),
            BTreeSet::from([0x15485])
        );
    }

    /// Une entrée NVIDIA périmée dont la LUID appartient désormais à l'iGPU ne doit
    /// pas faire accuser les clients de l'iGPU.
    #[test]
    fn drops_a_luid_claimed_by_another_adapter() {
        let adapters = [
            (VENDOR_NVIDIA, 0x100),
            (VENDOR_NVIDIA, 0x200),
            (0x8086, 0x100),
        ];
        assert_eq!(
            select_luids(adapters, VENDOR_NVIDIA),
            BTreeSet::from([0x200])
        );
    }

    /// Lit la machine réelle : `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_real_holders() {
        println!("LUID NVIDIA : {:x?}", nvidia_luids());
        let wmi = crate::sensors::wmi_context::WmiContext::new().unwrap();
        let mut p = GpuHoldersProvider::new();
        let state = p.probe(&ProbeContext { wmi: Some(&wmi) });
        println!("probe : {state:?}");
        let mut r = Reading::default();
        p.sample(&mut r);
        println!("clients : {:?}", r.get(Metric::GpuHolderCount));
        for h in &r.gpu_holders {
            println!("  {:>6} {:>10?} Mo  {:?}", h.pid, h.dedicated_mb, h.name);
        }
    }
}
