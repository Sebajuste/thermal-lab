//! Pipeline de mesure : des fournisseurs interchangeables alimentent un releve commun.
//!
//! - `metric`    : ce qui peut etre mesure, et le releve agrege
//! - `provider`  : le contrat que respecte toute source
//! - `registry`  : quels fournisseurs, dans quel ordre de priorite
//! - `hub`       : la boucle d'echantillonnage
//! - `providers` : les pilotes concrets
//!
//! Les deux modules restants sont des supports techniques partages par plusieurs
//! pilotes : `wmi_context` pour l'acces WMI, `shared_memory` pour les sections nommees.

pub mod hub;
pub mod metric;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod shared_memory;
pub mod wmi_context;

pub use hub::{ProviderStatus, SensorHub};
pub use metric::{Metric, Reading};
