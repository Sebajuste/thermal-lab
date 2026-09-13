//! Pilote LibreHardwareMonitor.
//!
//! LHM, et son ancetre OpenHardwareMonitor, publient leurs capteurs avec un `Identifier`
//! hierarchique (`/intelcpu/0/...`) qui permet de distinguer le CPU du reste sans se fier
//! au libelle. La voie d'acces — HTTP ou WMI selon la version de l'outil — est l'affaire
//! du module `lhm`.

use crate::sensors::lhm::{self, SensorRow, Source};
use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};

const ID: &str = "libre-hw";
const PROVIDES: &[Metric] = &[Metric::CpuTempC, Metric::CpuPowerW];

#[derive(Default)]
pub struct LibreHwProvider {
    source: Option<Source>,
}

impl LibreHwProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for LibreHwProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "LibreHardwareMonitor",
            kind: ProviderKind::External,
            provides: PROVIDES,
            url: Some("https://github.com/LibreHardwareMonitor/LibreHardwareMonitor"),
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState {
        self.source = Source::open(ctx.wmi);

        match self.source {
            Some(_) => ProbeState::Ready,
            None => ProbeState::unavailable(lhm::unavailable_reason(), lhm::unavailable_hint()),
        }
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
    // Le capteur « Package » est le bon ; a defaut on prend le plus chaud des coeurs,
    // ce qui revient au meme a une fraction de degre pres.
    let mut package_temp: Option<f64> = None;
    let mut hottest_core: Option<f64> = None;

    for r in rows {
        if !r.id.to_lowercase().contains("cpu") {
            continue;
        }

        match r.kind.as_str() {
            "Temperature" => {
                if r.name.contains("package") || r.name.contains("tdie") || r.name.contains("tctl")
                {
                    package_temp = Some(r.value);
                } else {
                    hottest_core = Some(hottest_core.map_or(r.value, |h: f64| h.max(r.value)));
                }
            }
            "Power" if r.name.contains("package") => {
                out.offer(Metric::CpuPowerW, r.value, ID);
            }
            _ => {}
        }
    }

    if let Some(t) = package_temp.or(hottest_core) {
        out.offer(Metric::CpuTempC, t, ID);
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

    /// Les libelles viennent tels quels de l'outil : c'est leur reconnaissance qui
    /// decide si la temperature publiee est celle du die ou celle d'un coeur.
    #[test]
    fn prefers_the_package_sensor() {
        let rows = [
            row("/amdcpu/0/temperature/2", "Temperature", "Core #1", 71.0),
            row(
                "/amdcpu/0/temperature/0",
                "Temperature",
                "Core (Tctl/Tdie)",
                64.25,
            ),
            row("/amdcpu/0/power/0", "Power", "Package", 88.125),
        ];
        let mut out = Reading::default();
        harvest(&rows, &mut out);

        assert_eq!(out.get(Metric::CpuTempC), Some(64.25));
        assert_eq!(out.get(Metric::CpuPowerW), Some(88.125));
    }

    /// Sans capteur de package, le plus chaud des coeurs en tient lieu.
    #[test]
    fn falls_back_to_the_hottest_core() {
        let rows = [
            row(
                "/intelcpu/0/temperature/1",
                "Temperature",
                "CPU Core #1",
                58.0,
            ),
            row(
                "/intelcpu/0/temperature/2",
                "Temperature",
                "CPU Core #2",
                66.5,
            ),
        ];
        let mut out = Reading::default();
        harvest(&rows, &mut out);

        assert_eq!(out.get(Metric::CpuTempC), Some(66.5));
    }

    /// L'identifiant, et non le libelle, borne ce pilote au CPU : le GPU a ses propres
    /// capteurs « Core », que revendiquer ici serait un contresens.
    #[test]
    fn ignores_sensors_outside_the_cpu() {
        let rows = [row(
            "/gpu-amd/0/temperature/0",
            "Temperature",
            "GPU Core",
            49.0,
        )];
        let mut out = Reading::default();
        harvest(&rows, &mut out);

        assert!(!out.has(Metric::CpuTempC));
    }
}
