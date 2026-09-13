//! Acces aux capteurs de LibreHardwareMonitor, par HTTP puis par WMI.
//!
//! LHM a publie ses capteurs dans l'espace de noms WMI `root\LibreHardwareMonitor`
//! jusqu'a la version 0.9.4. La 0.9.5 a **supprime** ce fournisseur — .NET 10 ne le
//! supporte plus — et l'outil ne laisse que son serveur HTTP, dont `data.json` porte
//! exactement les memes champs sous une autre forme. Un outil a jour tournait donc sans
//! qu'aucune mesure n'arrive, et aucune elevation n'y pouvait rien.
//!
//! Les deux chemins sont gardes : le HTTP d'abord, puis le WMI pour OpenHardwareMonitor
//! et les LHM anterieurs. Les fournisseurs qui lisent cette source
//! (`libre_hw`, `amd_gpu`) ne voient qu'une liste de lignes, identique dans les deux cas.

use std::time::Duration;

use serde::Deserialize;
use wmi::WMIConnection;

use super::wmi_context::{variant_f64, variant_string, Row, WmiContext};

/// Port d'ecoute par defaut du serveur web de LHM. L'utilisateur peut en choisir un
/// autre dans ses options ; `THERMAL_LAB_LHM_PORT` permet alors de nous le dire.
const DEFAULT_PORT: u16 = 8085;
const PORT_ENV: &str = "THERMAL_LAB_LHM_PORT";

/// Le serveur repond en quelques millisecondes sur la boucle locale. Au-dela, il n'est
/// pas la : mieux vaut rendre la main que retarder tout le cycle d'echantillonnage.
const TIMEOUT: Duration = Duration::from_millis(800);

const NAMESPACES: [&str; 2] = ["root\\LibreHardwareMonitor", "root\\OpenHardwareMonitor"];
const QUERY: &str = "SELECT Name, Value, SensorType, Identifier FROM Sensor";

/// Ce qu'il faut savoir d'un capteur, quelle que soit la voie d'acces.
pub struct SensorRow {
    /// Identifiant hierarchique : `/intelcpu/0/temperature/0`, `/gpu-amd/0/load/0`.
    pub id: String,
    /// `Temperature`, `Power`, `Clock`, `Load`…
    pub kind: String,
    /// Libelle affiche, deja en minuscules — les comparaisons se font toutes ainsi.
    pub name: String,
    pub value: f64,
}

/// Ce qui manque quand la source n'est pas joignable, dans les termes de l'utilisateur.
pub const UNAVAILABLE_REASON: &str = "LibreHardwareMonitor ne publie aucune mesure";
pub const UNAVAILABLE_HINT: &str = "Lancer LibreHardwareMonitor en administrateur, puis \
     activer son serveur web (Options → Remote Web Server → Run) : depuis la version \
     0.9.5, c'est la seule voie de lecture.";

/// Une voie de lecture etablie. Detenue par le fournisseur, elle survit aux cycles.
pub enum Source {
    Http { url: String, agent: ureq::Agent },
    Wmi(WMIConnection),
}

impl Source {
    /// Etablit la premiere voie qui repond, ou rien. Le HTTP passe en premier : c'est
    /// le seul chemin qu'un LHM a jour propose encore.
    pub fn open(wmi: Option<&WmiContext>) -> Option<Self> {
        http_source().or_else(|| wmi_source(wmi?))
    }

    /// Les capteurs a cet instant. `None` quand la source a disparu en cours de route.
    pub fn rows(&self) -> Option<Vec<SensorRow>> {
        match self {
            Source::Http { url, agent } => fetch_json(agent, url),
            Source::Wmi(con) => con.raw_query::<Row>(QUERY).ok().map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        Some(SensorRow {
                            id: variant_string(r.get("Identifier"))?,
                            kind: variant_string(r.get("SensorType")).unwrap_or_default(),
                            name: variant_string(r.get("Name"))
                                .unwrap_or_default()
                                .to_lowercase(),
                            value: variant_f64(r.get("Value"))?,
                        })
                    })
                    .collect()
            }),
        }
    }
}

fn port() -> u16 {
    std::env::var(PORT_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn http_source() -> Option<Source> {
    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .max_idle_connections_per_host(1)
        .build();
    let url = format!("http://localhost:{}/data.json", port());
    fetch_json(&agent, &url).map(|_| Source::Http { url, agent })
}

fn wmi_source(wmi: &WmiContext) -> Option<Source> {
    NAMESPACES
        .into_iter()
        .filter_map(|ns| wmi.connect(ns).ok())
        .find(|con| con.raw_query::<Row>(QUERY).is_ok())
        .map(Source::Wmi)
}

/// L'arborescence renvoyee par `data.json`. Seuls les noeuds de capteur portent un
/// `SensorId` ; les autres ne sont la que pour leurs enfants.
#[derive(Deserialize)]
struct Node {
    #[serde(rename = "Text", default)]
    text: String,
    #[serde(rename = "SensorId", default)]
    sensor_id: Option<String>,
    #[serde(rename = "Type", default)]
    kind: Option<String>,
    /// Valeur brute, sans unite ni mise en forme — `Value` est une chaine localisee,
    /// donc inexploitable. LHM ecrit `"NaN"` en chaine pour un capteur sans lecture.
    #[serde(rename = "RawValue", default)]
    raw_value: Option<serde_json::Value>,
    #[serde(rename = "Children", default)]
    children: Vec<Node>,
}

fn fetch_json(agent: &ureq::Agent, url: &str) -> Option<Vec<SensorRow>> {
    let root: Node = agent.get(url).call().ok()?.into_json().ok()?;
    let mut out = Vec::new();
    collect(&root, &mut out);
    Some(out)
}

fn collect(node: &Node, out: &mut Vec<SensorRow>) {
    if let (Some(id), Some(value)) = (&node.sensor_id, node.raw_value.as_ref().and_then(number)) {
        out.push(SensorRow {
            id: id.clone(),
            kind: node.kind.clone().unwrap_or_default(),
            name: node.text.to_lowercase(),
            value,
        });
    }
    for child in &node.children {
        collect(child, out);
    }
}

/// `RawValue` arrive en nombre, ou en chaine pour les valeurs que JSON ne sait pas
/// ecrire (`"NaN"`). Un `NaN` traverse sans dommage : `Reading::offer` le rejette.
fn number(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "id": 0, "Text": "Sensor", "Children": [{
        "id": 1, "Text": "DESKTOP", "Children": [{
          "id": 2, "Text": "AMD Ryzen 7", "HardwareId": "/amdcpu/0", "Children": [{
            "id": 3, "Text": "Temperatures", "Children": [
              { "id": 4, "Text": "Core (Tctl/Tdie)", "SensorId": "/amdcpu/0/temperature/0",
                "Type": "Temperature", "Value": "52,4 °C", "RawValue": 52.375 },
              { "id": 5, "Text": "Core (Tdie)", "SensorId": "/amdcpu/0/temperature/1",
                "Type": "Temperature", "Value": "-", "RawValue": "NaN" }
            ]}
          ]}
        ]}
      ]}"#;

    fn parse(json: &str) -> Vec<SensorRow> {
        let root: Node = serde_json::from_str(json).expect("arborescence illisible");
        let mut out = Vec::new();
        collect(&root, &mut out);
        out
    }

    /// Les champs de `data.json` doivent arriver sous la meme forme que ceux du WMI,
    /// sans quoi les fournisseurs devraient connaitre leur voie d'acces.
    #[test]
    fn flattens_the_tree_into_sensor_rows() {
        let rows = parse(SAMPLE);
        assert_eq!(rows.len(), 2, "les deux capteurs doivent ressortir");
        assert_eq!(rows[0].id, "/amdcpu/0/temperature/0");
        assert_eq!(rows[0].kind, "Temperature");
        assert_eq!(rows[0].name, "core (tctl/tdie)");
        assert_eq!(rows[0].value, 52.375);
    }

    /// Un capteur sans lecture sort en `NaN` plutot que d'etre tu : c'est `offer` qui
    /// tranche, et lui seul.
    #[test]
    fn keeps_unreadable_sensors_as_nan() {
        assert!(parse(SAMPLE)[1].value.is_nan());
    }

    /// Les noeuds de materiel n'ont pas de `SensorId` : ils ne valent que par leurs
    /// enfants, et ne doivent pas produire de ligne vide.
    #[test]
    fn ignores_nodes_without_a_sensor() {
        let rows = parse(r#"{"Text":"root","Children":[{"Text":"cpu","HardwareId":"/amdcpu/0"}]}"#);
        assert!(rows.is_empty());
    }
}
