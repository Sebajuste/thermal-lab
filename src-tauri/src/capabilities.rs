//! Ce que l'application peut reellement faire sur cette machine, et pourquoi pas le reste.
//!
//! Rien n'est grise sans explication : chaque grandeur absente est rattachee aux
//! fournisseurs qui la fourniraient et a la marche a suivre. C'est la reponse au fait
//! qu'un poste d'entreprise, une machine sans GPU NVIDIA ou un PC sans outil de
//! monitoring n'offrent pas les memes capacites — et qu'aucun de ces cas n'est une erreur.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::power;
use crate::sensors::{Metric, ProviderStatus, SensorHub};
use crate::tools::{self, ToolStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricInfo {
    pub metric: Metric,
    pub unit: &'static str,
    /// Vrai si au moins un fournisseur etabli la mesure.
    pub available: bool,
    /// Tous les fournisseurs capables de la mesurer, etablis ou non : c'est ce qui
    /// permet a l'UI de dire *quoi installer* pour combler un manque.
    pub provided_by: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub providers: Vec<ProviderStatus>,
    /// Etat de l'outil tiers dont depend chaque fournisseur externe, indexe par
    /// identifiant de fournisseur. Le fournisseur dit si sa mesure arrive ; ceci dit si
    /// l'outil est la, et ce qu'on peut y faire.
    pub tools: BTreeMap<&'static str, ToolStatus>,
    pub metrics: Vec<MetricInfo>,
    /// Faux sur un poste sans droits administrateur, ou dont les schemas
    /// d'alimentation sont verrouilles par strategie de groupe.
    pub can_control_power: bool,
    pub power_blocked_reason: Option<String>,
}

pub fn collect(hub: &SensorHub) -> Capabilities {
    let providers = hub.statuses();

    let available: BTreeSet<Metric> = providers
        .iter()
        .filter(|p| p.state.is_ready())
        .flat_map(|p| p.info.provides.iter().copied())
        .collect();

    let metrics = Metric::ALL
        .iter()
        .map(|m| MetricInfo {
            metric: *m,
            unit: m.unit(),
            available: available.contains(m),
            provided_by: providers
                .iter()
                .filter(|p| p.info.provides.contains(m))
                .map(|p| p.info.name)
                .collect(),
        })
        .collect();

    let (can_control_power, power_blocked_reason) = match power::state() {
        Ok(s) if s.elevated => (true, None),
        Ok(_) => (
            false,
            Some(
                crate::t!(
                    "administrator rights are required to change the power scheme",
                    "droits administrateur requis pour modifier le schema d'alimentation"
                )
                .into(),
            ),
        ),
        Err(e) => (false, Some(e)),
    };

    Capabilities {
        providers,
        tools: tools::statuses(),
        metrics,
        can_control_power,
        power_blocked_reason,
    }
}
