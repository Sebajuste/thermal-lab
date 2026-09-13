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
//!
//! `data.json` ne porte les valeurs brutes (`RawValue`) que **depuis la 0.9.5** : avant,
//! un capteur n'expose que sa valeur mise en forme et localisee (`"52,4 °C"`). Les deux
//! sont lues, faute de quoi un LHM 0.9.4 repondait un arbre entier sans une seule mesure
//! exploitable — et le HTTP, joignable, privait du WMI qui aurait marche.

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
    /// Toujours finie : un capteur sans lecture ne produit pas de ligne.
    pub value: f64,
}

/// Ce qui manque quand la source n'est pas joignable, dans les termes de l'utilisateur.
pub fn unavailable_reason() -> &'static str {
    crate::t!(
        "LibreHardwareMonitor publishes no reading",
        "LibreHardwareMonitor ne publie aucune mesure"
    )
}

pub fn unavailable_hint() -> &'static str {
    crate::t!(
        "Run LibreHardwareMonitor as administrator, then turn on its web server \
         (Options → Remote Web Server → Run): since 0.9.5 that is the only way to read it.",
        "Lancer LibreHardwareMonitor en administrateur, puis activer son serveur web \
         (Options → Remote Web Server → Run) : depuis la version 0.9.5, c'est la seule voie \
         de lecture."
    )
}

/// Une voie de lecture etablie. Detenue par le fournisseur, elle survit aux cycles.
pub enum Source {
    Http { url: String, agent: ureq::Agent },
    Wmi(WMIConnection),
}

impl Source {
    /// Etablit la premiere voie qui publie des capteurs, ou rien. Le HTTP passe en
    /// premier : c'est le seul chemin qu'un LHM a jour propose encore.
    pub fn open(wmi: Option<&WmiContext>) -> Option<Self> {
        http_source().or_else(|| wmi_source(wmi?))
    }

    /// Les capteurs a cet instant. `None` quand la source a disparu en cours de route,
    /// **ou** qu'elle ne publie plus rien : une voie muette laisserait `actif` a
    /// l'ecran devant un releve vide, alors que l'autre voie reste a tenter.
    pub fn rows(&self) -> Option<Vec<SensorRow>> {
        let rows = match self {
            Source::Http { url, agent } => fetch_json(agent, url),
            Source::Wmi(con) => con.raw_query::<Row>(QUERY).ok().map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        let value = variant_f64(r.get("Value")).filter(|v| v.is_finite())?;
                        Some(SensorRow {
                            id: variant_string(r.get("Identifier"))?,
                            kind: variant_string(r.get("SensorType")).unwrap_or_default(),
                            name: variant_string(r.get("Name"))
                                .unwrap_or_default()
                                .to_lowercase(),
                            value,
                        })
                    })
                    .collect()
            }),
        }?;

        (!rows.is_empty()).then_some(rows)
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
    // `127.0.0.1` et non `localhost` : ce dernier se resout d'abord en `::1` sur Windows,
    // et l'attente qui suit coute un cycle d'echantillonnage entier. Le serveur de LHM
    // ecoute sur toutes les interfaces.
    let url = format!("http://127.0.0.1:{}/data.json", port());
    let source = Source::Http { url, agent };
    source.rows().map(|_| source)
}

fn wmi_source(wmi: &WmiContext) -> Option<Source> {
    NAMESPACES
        .into_iter()
        .filter_map(|ns| wmi.connect(ns).ok())
        .map(Source::Wmi)
        .find(|source| source.rows().is_some())
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
    /// Valeur brute, sans unite ni mise en forme. LHM ecrit `"NaN"` en chaine pour un
    /// capteur sans lecture. Absente avant la 0.9.5.
    #[serde(rename = "RawValue", default)]
    raw_value: Option<serde_json::Value>,
    /// Valeur mise en forme et localisee (`"52,4 °C"`, `"-"` sans lecture), seul repli
    /// pour les LHM anterieurs a la 0.9.5. La decimale perdue ne coute rien ici.
    #[serde(rename = "Value", default)]
    value: Option<String>,
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
    let value = node
        .raw_value
        .as_ref()
        .and_then(number)
        .or_else(|| node.value.as_deref().and_then(formatted_number))
        .filter(|v| v.is_finite());
    if let (Some(id), Some(value)) = (&node.sensor_id, value) {
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
/// ecrire (`"NaN"`) — un capteur sans lecture, que l'appelant ecarte.
fn number(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Le nombre en tete d'une valeur mise en forme : `"52,4 °C"` → `52.4`, `"-"` → rien.
/// La locale est celle de la machine qui fait tourner LHM, inconnue d'ici : separateur
/// decimal virgule ou point, milliers separes par une espace insecable.
fn formatted_number(text: &str) -> Option<f64> {
    let head: String = text
        .trim()
        .chars()
        .take_while(|c| {
            c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ',') || c.is_whitespace()
        })
        .filter(|c| !c.is_whitespace())
        .collect();

    // Un separateur n'est decimal que s'il ne reste qu'un ou deux chiffres derriere :
    // c'est ce qui distingue `"3.792 MHz"` (milliers) de `"52.4 °C"` (decimale).
    let point = head
        .rfind([',', '.'])
        .filter(|i| matches!(head.len() - i - 1, 1 | 2));
    let plain = match point {
        Some(i) => format!("{}.{}", head[..i].replace([',', '.'], ""), &head[i + 1..]),
        None => head.replace([',', '.'], ""),
    };

    plain.parse().ok()
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
        assert_eq!(rows[0].id, "/amdcpu/0/temperature/0");
        assert_eq!(rows[0].kind, "Temperature");
        assert_eq!(rows[0].name, "core (tctl/tdie)");
        assert_eq!(rows[0].value, 52.375);
    }

    /// Un capteur sans lecture vaut `"NaN"` : le laisser passer reviendrait a masquer
    /// le repli d'un fournisseur par une valeur qui ne mesure rien.
    #[test]
    fn drops_unreadable_sensors() {
        assert_eq!(parse(SAMPLE).len(), 1, "le capteur NaN doit etre ecarte");
    }

    /// Avant la 0.9.5, `data.json` ne porte pas de `RawValue` : sans ce repli, l'arbre
    /// entier d'un LHM 0.9.4 se lisait sans qu'une seule mesure en sorte.
    #[test]
    fn reads_the_formatted_value_when_the_raw_one_is_absent() {
        let rows = parse(
            r#"{"Text":"root","Children":[
                 {"Text":"Core (Tctl/Tdie)","SensorId":"/amdcpu/0/temperature/0",
                  "Type":"Temperature","Value":"52,4 °C"},
                 {"Text":"CPU Package","SensorId":"/amdcpu/0/power/0",
                  "Type":"Power","Value":"88.1 W"},
                 {"Text":"Core #1","SensorId":"/amdcpu/0/clock/1",
                  "Type":"Clock","Value":"3 792,0 MHz"},
                 {"Text":"Core #2","SensorId":"/amdcpu/0/temperature/9",
                  "Type":"Temperature","Value":"-"}]}"#,
        );

        let values: Vec<f64> = rows.iter().map(|r| r.value).collect();
        assert_eq!(
            values,
            vec![52.4, 88.1, 3792.0],
            "capteur sans lecture ou mal lu"
        );
    }

    /// Une valeur de milliers ne doit pas se lire comme une decimale : `3.792 MHz` est
    /// la meme frequence que `3 792 MHz`, pas 3,792.
    #[test]
    fn tells_a_thousands_separator_from_a_decimal_one() {
        assert_eq!(formatted_number("3.792 MHz"), Some(3792.0));
        assert_eq!(formatted_number("1.234,5 MHz"), Some(1234.5));
        assert_eq!(formatted_number("-5,0 °C"), Some(-5.0));
        assert_eq!(formatted_number("-"), None);
        assert_eq!(formatted_number(""), None);
    }

    /// Les noeuds de materiel n'ont pas de `SensorId` : ils ne valent que par leurs
    /// enfants, et ne doivent pas produire de ligne vide.
    #[test]
    fn ignores_nodes_without_a_sensor() {
        let rows = parse(r#"{"Text":"root","Children":[{"Text":"cpu","HardwareId":"/amdcpu/0"}]}"#);
        assert!(rows.is_empty());
    }
}
