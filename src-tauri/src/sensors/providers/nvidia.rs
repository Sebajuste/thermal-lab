//! Pilote GPU NVIDIA — `nvidia-smi`.
//!
//! L'outil est installe avec le pilote graphique : rien de plus a fournir, et aucun
//! privilege requis. On paie le cout d'un processus par sondage, negligeable a 1 Hz, en
//! echange d'une interface stable et documentee.

use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};

const ID: &str = "nvidia-smi";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const QUERY: &str = "--query-gpu=temperature.gpu,power.draw,clocks.sm,utilization.gpu";
const FORMAT: &str = "--format=csv,noheader,nounits";

/// L'ordre des colonnes suit celui de `QUERY`.
const COLUMNS: [Metric; 4] = [
    Metric::GpuTempC,
    Metric::GpuPowerW,
    Metric::GpuClockMhz,
    Metric::GpuUtilPct,
];

const PROVIDES: &[Metric] = &COLUMNS;

#[derive(Default)]
pub struct NvidiaProvider {
    available: bool,
}

impl NvidiaProvider {
    pub fn new() -> Self {
        Self::default()
    }

    fn query() -> Option<String> {
        let out = Command::new("nvidia-smi")
            .args([QUERY, FORMAT])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).to_string())
    }
}

impl Provider for NvidiaProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "NVIDIA (nvidia-smi)",
            kind: ProviderKind::Builtin,
            provides: PROVIDES,
            url: None,
        }
    }

    fn probe(&mut self, _ctx: &ProbeContext<'_>) -> ProbeState {
        match Self::query() {
            Some(text) if text.lines().next().is_some_and(|l| l.contains(',')) => {
                self.available = true;
                ProbeState::Ready
            }
            Some(_) => ProbeState::Failed {
                error: crate::t!(
                    "nvidia-smi answered in an unexpected format",
                    "nvidia-smi a repondu un format inattendu"
                )
                .into(),
            },
            None => {
                self.available = false;
                ProbeState::unavailable_only(crate::t!(
                    "no NVIDIA GPU, or driver missing",
                    "aucun GPU NVIDIA, ou pilote absent"
                ))
            }
        }
    }

    fn sample(&mut self, out: &mut Reading) -> Sampled {
        if !self.available {
            return Sampled::Lost;
        }
        let Some(text) = Self::query() else {
            return Sampled::Lost;
        };
        // Premier GPU uniquement : le POC ne gere pas le multi-carte.
        let Some(line) = text.lines().next() else {
            return Sampled::Lost;
        };

        for (metric, field) in COLUMNS.iter().zip(line.split(',')) {
            if let Ok(v) = field.trim().parse::<f64>() {
                out.offer(*metric, v, ID);
            }
        }

        Sampled::Answered
    }
}
