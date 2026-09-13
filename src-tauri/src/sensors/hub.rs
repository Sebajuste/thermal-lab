//! Boucle d'echantillonnage et etat des fournisseurs.
//!
//! Un thread dedie detient COM, les connexions WMI et les fournisseurs — rien de tout
//! cela ne traverse les threads. Les commandes Tauri se contentent de lire le dernier
//! releve publie.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::metric::Reading;
use super::provider::{ProbeContext, ProbeState, ProviderInfo};
use super::registry;
use super::wmi_context::WmiContext;

/// Nombre de cycles entre deux tentatives sur un fournisseur non etabli. Permet de
/// brancher un outil a chaud sans redemarrer l'application.
const REPROBE_EVERY: u32 = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    #[serde(flatten)]
    pub info: ProviderInfo,
    #[serde(flatten)]
    pub state: ProbeState,
}

pub struct SensorHub {
    latest: Arc<Mutex<Reading>>,
    statuses: Arc<Mutex<Vec<ProviderStatus>>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl SensorHub {
    /// `on_reading` est appele sur le thread d'echantillonnage, a chaque cycle, avec le
    /// releve qui vient d'etre publie. C'est le seul point d'accroche du pipeline vers
    /// l'exterieur : il permet d'accumuler ou de notifier au rythme de la mesure, sans
    /// dependre de la presence d'une interface.
    pub fn start<F>(period: Duration, on_reading: F) -> Self
    where
        F: Fn(&Reading) + Send + 'static,
    {
        let latest = Arc::new(Mutex::new(Reading::default()));
        let statuses = Arc::new(Mutex::new(Vec::new()));

        let reading_sink = Arc::clone(&latest);
        let status_sink = Arc::clone(&statuses);

        thread::spawn(move || {
            // L'echec de COM n'est pas fatal : les fournisseurs qui n'en dependent pas
            // (memoire partagee, nvidia-smi) restent operationnels.
            let wmi = WmiContext::new().ok();
            let ctx = ProbeContext { wmi: wmi.as_ref() };

            let mut providers = registry::build();
            let mut states: Vec<ProbeState> =
                providers.iter_mut().map(|p| p.probe(&ctx)).collect();
            publish_statuses(&status_sink, &providers, &states);

            let mut tick: u32 = 0;
            loop {
                tick = tick.wrapping_add(1);
                let retry = tick % REPROBE_EVERY == 0;
                let mut changed = false;

                let mut reading = Reading::default();
                for (provider, state) in providers.iter_mut().zip(states.iter_mut()) {
                    if !state.is_ready() && retry {
                        let fresh = provider.probe(&ctx);
                        changed |= fresh.is_ready() != state.is_ready();
                        *state = fresh;
                    }
                    if state.is_ready() {
                        provider.sample(&mut reading);
                    }
                }
                reading.ts_ms = now_ms();

                on_reading(&reading);
                if let Ok(mut slot) = reading_sink.lock() {
                    *slot = reading;
                }
                if changed {
                    publish_statuses(&status_sink, &providers, &states);
                }

                thread::sleep(period);
            }
        });

        Self { latest, statuses }
    }

    pub fn latest(&self) -> Reading {
        self.latest.lock().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn statuses(&self) -> Vec<ProviderStatus> {
        self.statuses.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn publish_statuses(
    sink: &Arc<Mutex<Vec<ProviderStatus>>>,
    providers: &[Box<dyn super::provider::Provider>],
    states: &[ProbeState],
) {
    let snapshot: Vec<ProviderStatus> = providers
        .iter()
        .zip(states.iter())
        .map(|(p, s)| ProviderStatus {
            info: p.info(),
            state: s.clone(),
        })
        .collect();

    if let Ok(mut slot) = sink.lock() {
        *slot = snapshot;
    }
}
