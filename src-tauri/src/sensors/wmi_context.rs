//! Acces WMI : initialisation COM, ouverture de connexions, decodage des variants.
//!
//! COM s'initialise une fois par thread. Le hub detient donc un unique contexte, cree
//! sur son thread d'echantillonnage, et les fournisseurs WMI lui demandent leurs
//! connexions plutot que d'appeler `CoInitializeEx` chacun de leur cote.

use std::collections::HashMap;
use wmi::{COMLibrary, Variant, WMIConnection};

/// Une ligne de resultat WMI, colonnes non typees.
pub type Row = HashMap<String, Variant>;

pub struct WmiContext {
    com: COMLibrary,
}

impl WmiContext {
    pub fn new() -> Result<Self, String> {
        COMLibrary::new()
            .map(|com| Self { com })
            .map_err(|e| format!("initialisation COM impossible : {e}"))
    }

    pub fn connect(&self, namespace: &str) -> Result<WMIConnection, String> {
        WMIConnection::with_namespace_path(namespace, self.com)
            .map_err(|e| format!("connexion a {namespace} impossible : {e}"))
    }
}

/// WMI type ses colonnes selon la classe interrogee, et pas toujours comme la
/// documentation le laisse croire : on accepte tout ce qui se ramene a un nombre.
pub fn variant_f64(v: Option<&Variant>) -> Option<f64> {
    match v? {
        Variant::UI8(n) => Some(*n as f64),
        Variant::UI4(n) => Some(*n as f64),
        Variant::UI2(n) => Some(*n as f64),
        Variant::UI1(n) => Some(*n as f64),
        Variant::I8(n) => Some(*n as f64),
        Variant::I4(n) => Some(*n as f64),
        Variant::I2(n) => Some(*n as f64),
        Variant::I1(n) => Some(*n as f64),
        Variant::R8(n) => Some(*n),
        Variant::R4(n) => Some(*n as f64),
        Variant::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

pub fn variant_string(v: Option<&Variant>) -> Option<String> {
    match v? {
        Variant::String(s) => Some(s.clone()),
        _ => None,
    }
}
