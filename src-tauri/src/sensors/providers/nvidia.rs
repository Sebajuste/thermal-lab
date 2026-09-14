//! Pilote GPU NVIDIA — NVML.
//!
//! `nvml.dll` est livree avec le pilote graphique : rien de plus a fournir, et aucun
//! privilege requis. La bibliotheque est chargee une fois, a l'etablissement, puis
//! interrogee par appel direct — un releve coute quelques microsecondes.
//!
//! La version precedente lancait `nvidia-smi` a chaque cycle. Un processus par seconde,
//! ~16 ms de CPU chacun pour initialiser NVML, le charger, l'afficher et mourir : le
//! poste dominant de l'application au repos, pour la meme mesure.

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};

const ID: &str = "nvidia";

const PROVIDES: &[Metric] = &[
    Metric::GpuTempC,
    Metric::GpuPowerW,
    Metric::GpuClockMhz,
    Metric::GpuClockMaxMhz,
    Metric::GpuUtilPct,
    Metric::GpuDecodeUtilPct,
    Metric::GpuEncodeUtilPct,
];

/// NVML rend la puissance en milliwatts.
const MILLIWATTS_PER_WATT: f64 = 1000.0;

#[derive(Default)]
pub struct NvidiaProvider {
    nvml: Option<Nvml>,
}

impl NvidiaProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for NvidiaProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "NVIDIA (NVML)",
            kind: ProviderKind::Builtin,
            provides: PROVIDES,
            url: None,
        }
    }

    fn probe(&mut self, _ctx: &ProbeContext<'_>) -> ProbeState {
        self.nvml = None;

        let Ok(nvml) = Nvml::init() else {
            return ProbeState::unavailable_only(crate::t!(
                "no NVIDIA GPU, or driver missing",
                "aucun GPU NVIDIA, ou pilote absent"
            ));
        };

        // Une bibliotheque chargee ne garantit pas une carte : un pilote laisse en place
        // apres retrait de la carte s'initialise encore, et ne compte aucun peripherique.
        match nvml.device_count() {
            Ok(0) | Err(_) => ProbeState::unavailable_only(crate::t!(
                "no NVIDIA GPU, or driver missing",
                "aucun GPU NVIDIA, ou pilote absent"
            )),
            Ok(_) => {
                self.nvml = Some(nvml);
                ProbeState::Ready
            }
        }
    }

    fn sample(&mut self, out: &mut Reading) -> Sampled {
        let Some(nvml) = &self.nvml else {
            return Sampled::Lost;
        };
        // Premier GPU uniquement : le POC ne gere pas le multi-carte. La poignee n'est
        // pas retenue entre deux cycles — elle emprunte a `nvml`, et la resoudre par
        // index est une recherche en table, pas un appel au pilote.
        let Ok(gpu) = nvml.device_by_index(0) else {
            return Sampled::Lost;
        };

        // Chaque grandeur est facultative : une carte sans capteur de puissance repond
        // quand meme, et une seule absence ne vaut pas une source perdue.
        if let Ok(t) = gpu.temperature(TemperatureSensor::Gpu) {
            out.offer(Metric::GpuTempC, t as f64, ID);
        }
        if let Ok(mw) = gpu.power_usage() {
            out.offer(Metric::GpuPowerW, mw as f64 / MILLIWATTS_PER_WATT, ID);
        }
        if let Ok(mhz) = gpu.clock_info(Clock::SM) {
            out.offer(Metric::GpuClockMhz, mhz as f64, ID);
        }
        if let Ok(mhz) = gpu.max_clock_info(Clock::SM) {
            out.offer(Metric::GpuClockMaxMhz, mhz as f64, ID);
        }
        if let Ok(u) = gpu.utilization_rates() {
            out.offer(Metric::GpuUtilPct, u.gpu as f64, ID);
        }
        if let Ok(u) = gpu.decoder_utilization() {
            out.offer(Metric::GpuDecodeUtilPct, u.utilization as f64, ID);
        }
        if let Ok(u) = gpu.encoder_utilization() {
            out.offer(Metric::GpuEncodeUtilPct, u.utilization as f64, ID);
        }

        Sampled::Answered
    }
}
