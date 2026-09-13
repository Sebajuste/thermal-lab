//! Pilote compteurs de performance Windows — frequence et charge CPU.
//!
//! C'est la seule source disponible partout : ni pilote a installer, ni privilege, ni
//! outil tiers. Et c'est la source decisive pour ce POC, car
//! `PercentProcessorPerformance` exprime la frequence en pourcentage du nominal : au-dela
//! de 100, le turbo est engage.
//!
//! On retient le **maximum par coeur**, pas la moyenne `_Total` : cette derniere melange
//! tous les processeurs logiques, reste vers 50 au repos, et ne franchit 100 que par
//! a-coups quand un seul coeur boost.

use wmi::WMIConnection;

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind};
use crate::sensors::wmi_context::{variant_f64, variant_string, Row};

const ID: &str = "perf-counters";
const NAMESPACE: &str = "root\\cimv2";
const QUERY: &str = "SELECT Name, PercentProcessorPerformance, PercentProcessorTime, \
                     ProcessorFrequency FROM Win32_PerfFormattedData_Counters_ProcessorInformation";

const PROVIDES: &[Metric] = &[
    Metric::CpuMaxCorePct,
    Metric::CpuAvgPerfPct,
    Metric::CpuUtilPct,
    Metric::CpuNominalMhz,
];

#[derive(Default)]
pub struct PerfCountersProvider {
    con: Option<WMIConnection>,
}

impl PerfCountersProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for PerfCountersProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "Compteurs de performance Windows",
            kind: ProviderKind::Builtin,
            provides: PROVIDES,
            url: None,
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState {
        let Some(wmi) = ctx.wmi else {
            return ProbeState::Failed {
                error: "COM indisponible".into(),
            };
        };
        match wmi.connect(NAMESPACE) {
            Err(e) => ProbeState::Failed { error: e },
            Ok(con) => match con.raw_query::<Row>(QUERY) {
                Err(e) => ProbeState::Failed {
                    error: format!("classe de compteurs illisible : {e}"),
                },
                Ok(_) => {
                    self.con = Some(con);
                    ProbeState::Ready
                }
            },
        }
    }

    fn sample(&mut self, out: &mut Reading) {
        let Some(con) = &self.con else { return };
        let Ok(rows) = con.raw_query::<Row>(QUERY) else {
            return;
        };

        let mut max_core: Option<f64> = None;
        for r in &rows {
            let name = variant_string(r.get("Name")).unwrap_or_default();
            let perf = variant_f64(r.get("PercentProcessorPerformance"));

            if name.contains("_Total") {
                if let Some(p) = perf {
                    out.offer(Metric::CpuAvgPerfPct, p, ID);
                }
                if let Some(u) = variant_f64(r.get("PercentProcessorTime")) {
                    out.offer(Metric::CpuUtilPct, u, ID);
                }
                if let Some(f) = variant_f64(r.get("ProcessorFrequency")) {
                    out.offer(Metric::CpuNominalMhz, f, ID);
                }
            } else if let Some(p) = perf {
                max_core = Some(max_core.map_or(p, |m: f64| m.max(p)));
            }
        }

        if let Some(m) = max_core {
            out.offer(Metric::CpuMaxCorePct, m, ID);
        }
    }
}
