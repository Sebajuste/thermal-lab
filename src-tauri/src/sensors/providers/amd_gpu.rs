//! Pilote GPU AMD/Radeon — capteurs de LibreHardwareMonitor.
//!
//! AMD n'a pas d'equivalent de `nvidia-smi` livre avec le pilote graphique : la seule
//! voie sans SDK natif passe par LibreHardwareMonitor, qui publie les capteurs de la
//! carte sous les identifiants `/gpu-amd/<n>/...` (`/amdgpu/<n>/...` sur
//! OpenHardwareMonitor). Meme source que `libre_hw`, donc, mais fournisseur distinct :
//! `provides` doit rester exact, et l'utilisateur doit pouvoir lire dans l'UI que LHM
//! lui apporterait *ces* grandeurs-la.
//!
//! Cote utilisateur, LHM doit tourner en administrateur — sans quoi il ne charge pas son
//! pilote noyau et ne publie rien — et exposer ses capteurs : voir `lhm` pour les deux
//! voies de lecture.
//!
//! **Une seule carte** : celle de plus petit index. Sur une machine ou un iGPU Radeon
//! cotoie une carte dediee, c'est l'iGPU qui peut sortir gagnant, et le rang du registre
//! ne permet pas d'arbitrer plus finement — le POC ne gere pas le multi-carte, pas plus
//! ici que dans `nvidia`.

use crate::sensors::lhm::{self, SensorRow, Source};
use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};

const ID: &str = "amd-gpu";

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
    source: Option<Source>,
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
            name: crate::t!(
                "AMD GPU (LibreHardwareMonitor)",
                "GPU AMD (LibreHardwareMonitor)"
            ),
            kind: ProviderKind::External,
            provides: PROVIDES,
            url: Some("https://github.com/LibreHardwareMonitor/LibreHardwareMonitor"),
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState {
        self.source = None;

        let Some(source) = Source::open(ctx.wmi) else {
            return ProbeState::unavailable(lhm::unavailable_reason(), lhm::unavailable_hint());
        };

        // LHM present ne veut pas dire carte AMD presente : sans capteur `/gpu-amd/`,
        // rien ne sera jamais mesure et le dire vaut mieux que se declarer Ready.
        let has_amd = source
            .rows()
            .is_some_and(|rows| rows.iter().any(|r| instance_prefix(&r.id).is_some()));

        if !has_amd {
            return ProbeState::unavailable_only(crate::t!(
                "no AMD GPU among the LibreHardwareMonitor sensors",
                "aucun GPU AMD parmi les capteurs de LibreHardwareMonitor"
            ));
        }

        self.source = Some(source);
        ProbeState::Ready
    }

    fn sample(&mut self, out: &mut Reading) -> Sampled {
        let Some(rows) = self.source.as_ref().and_then(Source::rows) else {
            return Sampled::Lost;
        };
        harvest(&rows, out);
        Sampled::Answered
    }
}

/// Le tri des capteurs, separe de leur lecture : c'est la seule partie qui merite d'etre
/// eprouvee, et elle ne depend pas de la voie par laquelle les lignes sont arrivees.
fn harvest(rows: &[SensorRow], out: &mut Reading) {
    // Les identifiants n'arrivent pas ordonnes : on determine d'abord la carte retenue,
    // puis on ne lit que ses capteurs.
    let mut target: Option<&str> = None;
    for r in rows {
        let Some(prefix) = instance_prefix(&r.id) else {
            continue;
        };
        if target.is_none_or(|t| prefix < t) {
            target = Some(prefix);
        }
    }
    // Le separateur final evite que `/gpu-amd/1` capte les capteurs de `/gpu-amd/10`.
    let Some(target) = target.map(|t| format!("{t}/")) else {
        return;
    };

    // « GPU Core » est la mesure de reference. Les replis existent parce qu'AMD ne publie
    // pas la meme liste selon la generation : hot spot sans core sur certaines RDNA, PPT
    // au lieu de Package ailleurs.
    let mut core_temp: Option<f64> = None;
    let mut any_temp: Option<f64> = None;
    let mut board_power: Option<f64> = None;
    let mut core_power: Option<f64> = None;

    for r in rows {
        if !r.id.starts_with(&target) {
            continue;
        }

        match r.kind.as_str() {
            "Temperature" => {
                if r.name.contains("core") {
                    core_temp = Some(r.value);
                } else {
                    any_temp.get_or_insert(r.value);
                }
            }
            "Power" => {
                if r.name.contains("package") || r.name.contains("ppt") {
                    board_power = Some(r.value);
                } else if r.name.contains("core") {
                    core_power = Some(r.value);
                }
            }
            "Clock" if r.name.contains("core") => out.offer(Metric::GpuClockMhz, r.value, ID),
            "Load" if r.name.contains("core") => out.offer(Metric::GpuUtilPct, r.value, ID),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, kind: &str, name: &str, value: f64) -> SensorRow {
        SensorRow {
            id: id.into(),
            kind: kind.into(),
            name: name.to_lowercase(),
            value,
        }
    }

    /// Les quatre grandeurs de la carte, telles qu'un LHM a jour les publie.
    #[test]
    fn reads_the_four_gpu_metrics() {
        let rows = [
            row("/gpu-amd/0/temperature/0", "Temperature", "GPU Core", 49.0),
            row("/gpu-amd/0/power/0", "Power", "GPU Package", 142.0),
            row("/gpu-amd/0/clock/0", "Clock", "GPU Core", 2105.0),
            row("/gpu-amd/0/load/0", "Load", "GPU Core", 37.0),
        ];
        let mut out = Reading::default();
        harvest(&rows, &mut out);

        assert_eq!(out.get(Metric::GpuTempC), Some(49.0));
        assert_eq!(out.get(Metric::GpuPowerW), Some(142.0));
        assert_eq!(out.get(Metric::GpuClockMhz), Some(2105.0));
        assert_eq!(out.get(Metric::GpuUtilPct), Some(37.0));
    }

    /// Une seule carte, celle de plus petit index : melanger deux GPU donnerait une
    /// temperature et une puissance qui ne decrivent aucune piece reelle.
    #[test]
    fn keeps_a_single_card() {
        let rows = [
            row("/gpu-amd/1/temperature/0", "Temperature", "GPU Core", 70.0),
            row("/gpu-amd/0/temperature/0", "Temperature", "GPU Core", 49.0),
        ];
        let mut out = Reading::default();
        harvest(&rows, &mut out);

        assert_eq!(out.get(Metric::GpuTempC), Some(49.0));
    }

    #[test]
    fn keeps_the_hardware_instance_only() {
        assert_eq!(
            instance_prefix("/gpu-amd/0/temperature/0"),
            Some("/gpu-amd/0")
        );
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
