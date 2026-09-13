//! Mise a jour automatique : verification, telechargement, relance.
//!
//! Le plugin expose aussi des commandes au frontend, mais les cabler demanderait
//! d'ouvrir des permissions a un webview qui tourne elevee. Tout passe donc par les
//! commandes de l'application, comme le reste du chassis : le frontend demande et
//! affiche, la decision reste ici.
//!
//! La chaine de confiance tient a une signature : le paquet telecharge est verifie
//! contre la cle publique inscrite dans `tauri.conf.json`. Sans elle, l'application
//! refuse tout — c'est ce qui empeche un tiers de servir un faux `latest.json`.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Ce qu'il faut a l'interface pour proposer la mise a jour sans la subir : les deux
/// numeros de version, et les notes de publication si la release en porte.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
}

/// Avancement du telechargement. `total` reste `None` quand le serveur ne l'annonce
/// pas : la barre se rabat alors sur les octets recus, qui disent au moins que ca avance.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Un evenement par morceau recu saturerait le pont pour rien : a 3 Mo, le webview en
/// recevrait plusieurs milliers pour dessiner cent positions de barre. On n'emet qu'au
/// changement de pourcent entier, et faute de taille annoncee, tous les 256 Ko.
const PROGRESS_STEP_BYTES: u64 = 256 * 1024;

/// La mise a jour reperee, gardee entre la verification et l'installation : telecharger
/// demande l'objet rendu par `check`, pas seulement son numero de version.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

async fn fetch(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;

    let Some(update) = found else {
        return Ok(None);
    };

    let info = UpdateInfo {
        version: update.version.clone(),
        current: update.current_version.clone(),
        notes: update.body.clone(),
    };

    if let Ok(mut slot) = app.state::<Pending>().0.lock() {
        *slot = Some(update);
    }
    Ok(Some(info))
}

/// La verification est declenchee par le frontend, au montage puis a la demande, et non
/// ici au demarrage : un evenement emis pendant le `setup` n'aurait aucun auditeur, le
/// webview n'etant pas encore charge. Le panneau existe des le lancement, y compris en
/// mode silencieux, donc rien n'est perdu a le laisser demander.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    fetch(&app).await
}

/// Telecharge, installe, relance. L'installateur NSIS tourne en mode passif : il montre
/// sa progression sans rien demander, l'utilisateur ayant deja repondu en cliquant.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Pending>();
    let pending = state
        .0
        .lock()
        .map_err(|_| "etat de mise a jour inutilisable")?
        .take();
    let update = pending.ok_or("aucune mise a jour en attente")?;

    let mut downloaded = 0u64;
    let mut last_mark = 0u64;

    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let mark = match total {
                    Some(total) if total > 0 => downloaded * 100 / total,
                    _ => downloaded / PROGRESS_STEP_BYTES,
                };
                if mark != last_mark {
                    last_mark = mark;
                    let _ = app.emit("update-progress", Progress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    app.restart()
}
