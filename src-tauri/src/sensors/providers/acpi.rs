//! Pilote zone thermique ACPI — `MSAcpi_ThermalZoneTemperature`.
//!
//! Disponible sans rien installer, mais il faut savoir ce qu'on lit : ces zones mesurent
//! un point de la carte mere, pas le die du processeur. Sur un i9-14900K a 61 °C de
//! package, la zone ACPI en annonce 28. C'est un ordre de grandeur, pas une temperature
//! CPU — d'ou une metrique distincte, `BoardTempC`, qui ne peut pas se substituer a elle.

use wmi::WMIConnection;

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};
use crate::sensors::wmi_context::{variant_f64, Row};

const ID: &str = "acpi";
const NAMESPACE: &str = "root\\WMI";
const QUERY: &str = "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
const PROVIDES: &[Metric] = &[Metric::BoardTempC];

/// Le compteur ACPI est en dixiemes de kelvin.
const DECIKELVIN_TO_CELSIUS: f64 = 273.15;

#[derive(Default)]
pub struct AcpiProvider {
    con: Option<WMIConnection>,
}

impl AcpiProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for AcpiProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: crate::t!("ACPI thermal zone", "Zone thermique ACPI"),
            kind: ProviderKind::Builtin,
            provides: PROVIDES,
            url: None,
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState {
        let Some(wmi) = ctx.wmi else {
            return ProbeState::Failed {
                error: crate::t!("COM unavailable", "COM indisponible").into(),
            };
        };
        let Ok(con) = wmi.connect(NAMESPACE) else {
            return ProbeState::Failed {
                error: crate::t!(
                    format!("namespace {NAMESPACE} unreachable"),
                    format!("espace de noms {NAMESPACE} inaccessible")
                ),
            };
        };
        match con.raw_query::<Row>(QUERY) {
            Ok(rows) if !rows.is_empty() => {
                self.con = Some(con);
                ProbeState::Ready
            }
            _ => ProbeState::unavailable_only(crate::t!(
                "this motherboard exposes no thermal zone",
                "aucune zone thermique exposee par cette carte mere"
            )),
        }
    }

    fn sample(&mut self, out: &mut Reading) -> Sampled {
        let Some(con) = &self.con else {
            return Sampled::Lost;
        };
        let Ok(rows) = con.raw_query::<Row>(QUERY) else {
            return Sampled::Lost;
        };

        let hottest = rows
            .iter()
            .filter_map(|r| variant_f64(r.get("CurrentTemperature")))
            .map(|dk| dk / 10.0 - DECIKELVIN_TO_CELSIUS)
            .filter(|t| (0.0..150.0).contains(t))
            .fold(None::<f64>, |acc, t| Some(acc.map_or(t, |a| a.max(t))));

        if let Some(t) = hottest {
            out.offer(Metric::BoardTempC, t, ID);
        }

        Sampled::Answered
    }
}
