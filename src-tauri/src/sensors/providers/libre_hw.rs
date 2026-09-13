//! Pilote LibreHardwareMonitor — espace de noms WMI `root\LibreHardwareMonitor`.
//!
//! LHM, et son ancetre OpenHardwareMonitor, publient leurs capteurs sous forme de classe
//! WMI `Sensor`. Chaque capteur porte un `Identifier` hierarchique (`/intelcpu/0/...`)
//! qui permet de distinguer le CPU du reste sans se fier au libelle.

use wmi::WMIConnection;

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind};
use crate::sensors::wmi_context::{variant_f64, variant_string, Row};

const ID: &str = "libre-hw";
const NAMESPACES: [&str; 2] = ["root\\LibreHardwareMonitor", "root\\OpenHardwareMonitor"];
const QUERY: &str = "SELECT Name, Value, SensorType, Identifier FROM Sensor";
const PROVIDES: &[Metric] = &[Metric::CpuTempC, Metric::CpuPowerW];

#[derive(Default)]
pub struct LibreHwProvider {
    con: Option<WMIConnection>,
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
        let Some(wmi) = ctx.wmi else {
            return ProbeState::Failed {
                error: "COM indisponible".into(),
            };
        };

        self.con = NAMESPACES
            .into_iter()
            .find_map(|ns| wmi.connect(ns).ok())
            .filter(|con| con.raw_query::<Row>(QUERY).is_ok());

        match self.con {
            Some(_) => ProbeState::Ready,
            None => ProbeState::unavailable(
                "LibreHardwareMonitor ne tourne pas",
                "Lancer LibreHardwareMonitor en administrateur.",
            ),
        }
    }

    fn sample(&mut self, out: &mut Reading) {
        let Some(con) = &self.con else { return };
        let Ok(rows) = con.raw_query::<Row>(QUERY) else {
            return;
        };

        // Le capteur « Package » est le bon ; a defaut on prend le plus chaud des coeurs,
        // ce qui revient au meme a une fraction de degre pres.
        let mut package_temp: Option<f64> = None;
        let mut hottest_core: Option<f64> = None;

        for r in &rows {
            let id = variant_string(r.get("Identifier"))
                .unwrap_or_default()
                .to_lowercase();
            if !id.contains("cpu") {
                continue;
            }
            let Some(value) = variant_f64(r.get("Value")) else {
                continue;
            };
            let kind = variant_string(r.get("SensorType")).unwrap_or_default();
            let name = variant_string(r.get("Name")).unwrap_or_default().to_lowercase();

            match kind.as_str() {
                "Temperature" => {
                    if name.contains("package") || name.contains("tdie") || name.contains("tctl") {
                        package_temp = Some(value);
                    } else {
                        hottest_core = Some(hottest_core.map_or(value, |h: f64| h.max(value)));
                    }
                }
                "Power" if name.contains("package") => {
                    out.offer(Metric::CpuPowerW, value, ID);
                }
                _ => {}
            }
        }

        if let Some(t) = package_temp.or(hottest_core) {
            out.offer(Metric::CpuTempC, t, ID);
        }
    }
}
