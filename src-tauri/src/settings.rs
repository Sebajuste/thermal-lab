//! Les reglages qui n'appartiennent qu'a l'application, conserves d'un lancement a
//! l'autre dans un JSON du dossier de configuration du compte.
//!
//! Ce qui vit ailleurs n'y figure pas : le demarrage automatique est une tache
//! planifiee, le schema d'alimentation appartient a Windows, la langue se lit dans la
//! session. Les relire a leur source evite de promettre un etat que le systeme a change
//! dans notre dos — un fichier qui doublerait ces reponses finirait par mentir.
//!
//! Un fichier illisible ou absent n'est pas une erreur : on repart des valeurs par
//! defaut. Perdre une preference est moins grave que refuser de demarrer pour elle.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE: &str = "settings.json";

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// Faux par defaut : une application qui se remplace toute seule, elevee, doit avoir
    /// ete autorisee a le faire. La coche est le consentement.
    pub auto_update: bool,
}

pub struct Settings {
    /// `None` quand le dossier de configuration est introuvable : l'application tourne
    /// alors sans memoire, la coche retombant a chaque lancement.
    path: Option<PathBuf>,
    prefs: Mutex<Prefs>,
}

impl Settings {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_config_dir().ok().map(|dir| dir.join(FILE));
        let prefs = path
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            path,
            prefs: Mutex::new(prefs),
        }
    }

    pub fn prefs(&self) -> Prefs {
        self.prefs.lock().map(|p| *p).unwrap_or_default()
    }

    /// Ecrit avant de rendre la main : la reponse rendue a l'interface est celle du
    /// fichier, pas une intention. Une ecriture qui echoue ne doit pas laisser une coche
    /// cochee qui redeviendrait vide au prochain lancement.
    pub fn set_auto_update(&self, on: bool) -> Result<Prefs, String> {
        let next = {
            let mut prefs = self
                .prefs
                .lock()
                .map_err(|_| "reglages inutilisables".to_string())?;
            prefs.auto_update = on;
            *prefs
        };
        self.save(&next)?;
        Ok(next)
    }

    fn save(&self, prefs: &Prefs) -> Result<(), String> {
        let Some(path) = self.path.as_ref() else {
            return Err("dossier de configuration introuvable".into());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
        }
        let raw = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
        fs::write(path, raw).map_err(|e| format!("{} : {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une preference ajoutee plus tard ne doit pas rendre illisible un fichier ecrit
    /// avant elle : `#[serde(default)]` comble les champs absents.
    #[test]
    fn reads_a_file_written_by_an_older_version() {
        let prefs: Prefs = serde_json::from_str("{}").expect("objet vide accepte");
        assert!(!prefs.auto_update);
    }

    #[test]
    fn round_trips_through_json() {
        let raw = serde_json::to_string(&Prefs { auto_update: true }).expect("serialisable");
        assert!(raw.contains("autoUpdate"), "{raw}");
        let back: Prefs = serde_json::from_str(&raw).expect("relisible");
        assert!(back.auto_update);
    }
}
