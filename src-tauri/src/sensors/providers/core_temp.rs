//! Pilote Core Temp — memoire partagee `CoreTempMappingObjectEx`.
//!
//! Core Temp embarque son propre pilote noyau signe et publie ses releves dans une
//! section nommee, selon une structure documentee par son auteur. Il doit tourner, et
//! avoir ete lance en administrateur pour que son pilote soit charge.

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind};
use crate::sensors::shared_memory::{c_string, MappedView};

const ID: &str = "core-temp";
const SECTIONS: [&str; 2] = ["CoreTempMappingObjectEx", "CoreTempMappingObject"];
const PROVIDES: &[Metric] = &[Metric::CpuTempC, Metric::CpuPowerW];

/// `CoreTempSharedDataEx`, alignement C naturel.
///
/// Les quatre `u8` consecutifs comblent exactement le mot precedant `struct_version`,
/// donc aucun `packed` n'est requis.
#[repr(C)]
#[derive(Clone, Copy)]
struct SharedDataEx {
    load: [u32; 256],
    tj_max: [u32; 128],
    core_count: u32,
    cpu_count: u32,
    temp: [f32; 256],
    vid: f32,
    cpu_speed: f32,
    fsb_speed: f32,
    multiplier: f32,
    cpu_name: [u8; 100],
    fahrenheit: u8,
    delta_to_tjmax: u8,
    tdp_supported: u8,
    power_supported: u8,
    struct_version: u32,
    tdp: [u32; 128],
    power: [f32; 128],
    multipliers: [f32; 256],
}

#[derive(Default)]
pub struct CoreTempProvider {
    section: Option<&'static str>,
}

impl CoreTempProvider {
    pub fn new() -> Self {
        Self::default()
    }

    fn read(&self) -> Option<SharedDataEx> {
        let name = self.section?;
        MappedView::open(name)?.read_struct::<SharedDataEx>(0)
    }
}

impl Provider for CoreTempProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "Core Temp",
            kind: ProviderKind::External,
            provides: PROVIDES,
            url: Some("https://www.alcpu.com/CoreTemp/"),
        }
    }

    fn probe(&mut self, _ctx: &ProbeContext<'_>) -> ProbeState {
        self.section = SECTIONS
            .into_iter()
            .find(|name| MappedView::open(name).is_some());

        match self.section {
            None => ProbeState::unavailable(
                "Core Temp ne tourne pas",
                "Lancer Core Temp en administrateur.",
            ),
            Some(_) => match self.read() {
                // Section presente mais illisible : disposition inattendue, pas une absence.
                None => ProbeState::Failed {
                    error: "section presente mais structure illisible".into(),
                },
                Some(d) if c_string(&d.cpu_name).is_empty() => ProbeState::Failed {
                    error: "nom de CPU vide : disposition memoire inattendue".into(),
                },
                Some(_) => ProbeState::Ready,
            },
        }
    }

    fn sample(&mut self, out: &mut Reading) {
        let Some(d) = self.read() else { return };

        let cores = (d.core_count as usize)
            .saturating_mul(d.cpu_count.max(1) as usize)
            .min(d.temp.len());
        let tj_max = d.tj_max.first().copied().unwrap_or(100) as f64;

        let mut package: Option<f64> = None;
        for i in 0..cores {
            let raw = d.temp[i] as f64;
            if !raw.is_finite() {
                continue;
            }
            // Core Temp publie soit la temperature absolue, soit l'ecart au TjMax.
            let mut celsius = if d.delta_to_tjmax != 0 { tj_max - raw } else { raw };
            if d.fahrenheit != 0 {
                celsius = (celsius - 32.0) * 5.0 / 9.0;
            }
            if (-50.0..150.0).contains(&celsius) {
                package = Some(package.map_or(celsius, |p: f64| p.max(celsius)));
            }
        }

        if let Some(t) = package {
            out.offer(Metric::CpuTempC, t, ID);
        }
        if d.power_supported != 0 {
            if let Some(p) = d.power.first().map(|p| *p as f64).filter(|p| *p > 0.0) {
                out.offer(Metric::CpuPowerW, p, ID);
            }
        }

        let name = c_string(&d.cpu_name);
        if !name.is_empty() && out.cpu_name.is_none() {
            out.cpu_name = Some(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::provider::ProbeState;

    /// Validation contre la vraie section partagee. Sans Core Temp lance il n'y a rien a
    /// valider ; quand il repond, les valeurs doivent etre physiquement plausibles et le
    /// nom du CPU lisible — un decalage d'un octet suffirait a les rendre aberrantes.
    #[test]
    fn decodes_plausible_values_when_running() {
        let mut p = CoreTempProvider::new();
        let state = p.probe(&ProbeContext { wmi: None });

        match state {
            ProbeState::Unavailable { .. } => println!("Core Temp non lance : rien a valider"),
            ProbeState::Failed { error } => panic!("decodage casse : {error}"),
            ProbeState::Ready => {
                let mut r = Reading::default();
                p.sample(&mut r);
                println!("CPU       : {:?}", r.cpu_name);
                println!("Package   : {:?} °C", r.get(Metric::CpuTempC));
                println!("Puissance : {:?} W", r.get(Metric::CpuPowerW));

                let t = r.get(Metric::CpuTempC).expect("aucune temperature");
                assert!((10.0..=110.0).contains(&t), "temperature hors plage : {t}");
                assert!(r.cpu_name.as_deref().is_some_and(|n| !n.is_empty()));
                if let Some(w) = r.get(Metric::CpuPowerW) {
                    assert!((1.0..=400.0).contains(&w), "puissance hors plage : {w}");
                }
            }
        }
    }
}
