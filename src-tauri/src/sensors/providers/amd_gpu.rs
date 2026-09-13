//! Pilote GPU AMD/Radeon — espace de noms WMI `root\LibreHardwareMonitor`.
//!
//! AMD n'a pas d'equivalent de `nvidia-smi` livre avec le pilote graphique : la seule
//! voie sans SDK natif passe par LibreHardwareMonitor, qui publie les capteurs de la
//! carte sous les identifiants `/gpu-amd/<n>/...` (`/amdgpu/<n>/...` sur
//! OpenHardwareMonitor). Meme source que `libre_hw`, donc, mais fournisseur distinct :
//! `provides` doit rester exact, et l'utilisateur doit pouvoir lire dans l'UI que LHM
//! lui apporterait *ces* grandeurs-la.
//!
//! Cote utilisateur, LHM doit tourner en administrateur — sans quoi il ne charge pas son
//! pilote noyau et ne publie rien.
//!
//! **Une seule carte** : celle de plus petit index. Sur une machine ou un iGPU Radeon
//! cotoie une carte dediee, c'est l'iGPU qui peut sortir gagnant, et le rang du registre
//! ne permet pas d'arbitrer plus finement — le POC ne gere pas le multi-carte, pas plus
//! ici que dans `nvidia`.

use wmi::WMIConnection;

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind};
use crate::sensors::wmi_context::{variant_f64, variant_string, Row};

const ID: &str = "amd-gpu";
const NAMESPACES: [&str; 2] = ["root\\LibreHardwareMonitor", "root\\OpenHardwareMonitor"];
const QUERY: &str = "SELECT Name, Value, SensorType, Identifier FROM Sensor";

/// Les deux graphies rencontrees : LHM a renomme le type materiel, OHM garde l'ancienne.
const MARKERS: [&str; 2] = ["/gpu-amd/", "/amdgpu/"];

const PROVIDES: &[Metric] = &[
    Metric::GpuTempC,
    Metric::GpuPowerW,
    Metric::GpuClockMhz,
    Metric::GpuUtilPct,
];

/// `/gpu-amd/0/temperature/0` → `/gpu-amd/0`. Sert a ne retenir qu'une carte.
fn instance_prefix(identifier: &str) -> Option<&str> {
    if !MARKERS.iter().any(|m| identifier.starts_with(m)) {
        return None;
    }
    // Segments : ["", "gpu-amd", "0", ...] — le troisieme '/' borne le prefixe, et son
    // absence signale un identifiant tronque, sans capteur derriere.
    let (end, _) = identifier.match_indices('/').nth(2)?;
    Some(&identifier[..end])
}

#[derive(Default)]
pub struct AmdGpuProvider {
    con: Option<WMIConnection>,
}

impl AmdGpuProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for AmdGpuProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "GPU AMD (LibreHardwareMonitor)",
            kind: ProviderKind::External,
            provides: PROVIDES,
            url: Some("https://github.com/LibreHardwareMonitor/LibreHardwareMonitor"),
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState {
        let Some(wmi) = ctx.wmi else {
            return ProbeState::Failed {
                error: "COM indisponible".into(),
            };
        };

        let con = NAMESPACES
            .into_iter()
            .find_map(|ns| wmi.connect(ns).ok())
            .filter(|con| con.raw_query::<Row>(QUERY).is_ok());

        let Some(con) = con else {
            self.con = None;
            return ProbeState::unavailable(
                "LibreHardwareMonitor ne tourne pas",
                "Lancer LibreHardwareMonitor en administrateur.",
            );
        };

        // LHM present ne veut pas dire carte AMD presente : sans capteur `/gpu-amd/`,
        // rien ne sera jamais mesure et le dire vaut mieux que se declarer Ready.
        let has_amd = con.raw_query::<Row>(QUERY).is_ok_and(|rows| {
            rows.iter().any(|r| {
                variant_string(r.get("Identifier"))
                    .as_deref()
                    .and_then(instance_prefix)
                    .is_some()
            })
        });

        if !has_amd {
            self.con = None;
            return ProbeState::unavailable_only(
                "aucun GPU AMD parmi les capteurs de LibreHardwareMonitor",
            );
        }

        self.con = Some(con);
        ProbeState::Ready
    }

    fn sample(&mut self, out: &mut Reading) {
        let Some(con) = &self.con else { return };
        let Ok(rows) = con.raw_query::<Row>(QUERY) else {
            return;
        };

        // Les identifiants ne sont pas ordonnes par la requete : on determine d'abord la
        // carte retenue, puis on ne lit que ses capteurs.
        let mut target: Option<String> = None;
        for r in &rows {
            let Some(id) = variant_string(r.get("Identifier")) else {
                continue;
            };
            let Some(prefix) = instance_prefix(&id) else {
                continue;
            };
            if target.as_deref().is_none_or(|t| prefix < t) {
                target = Some(prefix.to_string());
            }
        }
        // Le separateur final evite que `/gpu-amd/1` capte les capteurs de `/gpu-amd/10`.
        let Some(target) = target.map(|t| format!("{t}/")) else { return };

        // « GPU Core » est la mesure de reference. Les replis existent parce qu'AMD ne
        // publie pas la meme liste selon la generation : hot spot sans core sur certaines
        // RDNA, PPT au lieu de Package ailleurs.
        let mut core_temp: Option<f64> = None;
        let mut any_temp: Option<f64> = None;
        let mut board_power: Option<f64> = None;
        let mut core_power: Option<f64> = None;

        for r in &rows {
            let Some(id) = variant_string(r.get("Identifier")) else {
                continue;
            };
            if !id.starts_with(&target) {
                continue;
            }
            let Some(value) = variant_f64(r.get("Value")) else {
                continue;
            };
            let kind = variant_string(r.get("SensorType")).unwrap_or_default();
            let name = variant_string(r.get("Name")).unwrap_or_default().to_lowercase();

            match kind.as_str() {
                "Temperature" => {
                    if name.contains("core") {
                        core_temp = Some(value);
                    } else {
                        any_temp.get_or_insert(value);
                    }
                }
                "Power" => {
                    if name.contains("package") || name.contains("ppt") {
                        board_power = Some(value);
                    } else if name.contains("core") {
                        core_power = Some(value);
                    }
                }
                "Clock" if name.contains("core") => out.offer(Metric::GpuClockMhz, value, ID),
                "Load" if name.contains("core") => out.offer(Metric::GpuUtilPct, value, ID),
                _ => {}
            }
        }

        if let Some(t) = core_temp.or(any_temp) {
            out.offer(Metric::GpuTempC, t, ID);
        }
        if let Some(p) = board_power.or(core_power) {
            out.offer(Metric::GpuPowerW, p, ID);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::instance_prefix;

    #[test]
    fn keeps_the_hardware_instance_only() {
        assert_eq!(instance_prefix("/gpu-amd/0/temperature/0"), Some("/gpu-amd/0"));
        assert_eq!(instance_prefix("/amdgpu/1/load/0"), Some("/amdgpu/1"));
    }

    /// Le prefixe doit exclure les autres materiels, GPU NVIDIA compris : c'est lui qui
    /// empeche ce pilote de revendiquer une carte qu'il ne mesure pas.
    #[test]
    fn ignores_other_hardware() {
        assert_eq!(instance_prefix("/gpu-nvidia/0/temperature/0"), None);
        assert_eq!(instance_prefix("/amdcpu/0/temperature/0"), None);
        assert_eq!(instance_prefix("/intelcpu/0/load/0"), None);
    }

    /// Un identifiant tronque ne doit pas produire un prefixe qui capterait tout.
    #[test]
    fn rejects_truncated_identifiers() {
        assert_eq!(instance_prefix("/gpu-amd/"), None);
        assert_eq!(instance_prefix("/gpu-amd/0"), None);
    }
}
