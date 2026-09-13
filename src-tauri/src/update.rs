//! Mise a jour : verification, telechargement, relance — a la demande, ou toute seule.
//!
//! Le plugin expose aussi des commandes au frontend, mais les cabler demanderait
//! d'ouvrir des permissions a un webview qui tourne elevee. Tout passe donc par les
//! commandes de l'application, comme le reste du chassis : le frontend demande et
//! affiche, la decision reste ici.
//!
//! La chaine de confiance tient a une signature : le paquet telecharge est verifie
//! contre la cle publique inscrite dans `tauri.conf.json`. Sans elle, l'application
//! refuse tout — c'est ce qui empeche un tiers de servir un faux `latest.json`.
//!
//! Le mode automatique n'est pas un second chemin d'installation : c'est une boucle de
//! veille qui appelle exactement la verification et l'installation du mode manuel. Ce
//! qui lui est propre tient a deux questions — l'utilisateur l'a-t-il autorise, et le
//! moment est-il acceptable.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::flyout;
use crate::settings::Settings;

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

/// Battement de la boucle automatique. Court, parce qu'il ne decide de rien : il ne fait
/// que reposer les deux questions du moment — la case est-elle cochee, le panneau est-il
/// ouvert. C'est aussi le delai maximal entre le geste de cocher et la premiere
/// recherche, la boucle repartant de zero des que l'option est reactivee.
const TICK: Duration = Duration::from_secs(5 * 60);

/// Espacement de deux interrogations de GitHub. Une application residante tourne des
/// semaines : six heures suffisent a ne pas rater une publication, et evitent de
/// transformer un outil de mesure en client de sondage.
const PERIOD: Duration = Duration::from_secs(6 * 3600);

/// La mise a jour reperee, gardee entre la verification et l'installation : telecharger
/// demande l'objet rendu par `check`, pas seulement son numero de version.
#[derive(Default)]
pub struct Pending {
    slot: Mutex<Option<Update>>,
    /// La boucle et le bouton visent le meme paquet. Sans ce drapeau, ouvrir le panneau
    /// pendant un telechargement de fond offrirait un bouton qui lancerait un second
    /// telechargement du meme fichier, puis un second installateur.
    installing: AtomicBool,
}

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

    if let Ok(mut slot) = app.state::<Pending>().slot.lock() {
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
    install_pending(&app).await
}

/// Prend la mise a jour en attente et l'installe, pour le bouton comme pour la boucle.
///
/// Ne rend la main qu'en cas d'echec : sous Windows le plugin lance l'installateur puis
/// termine le processus lui-meme, et l'installateur relance l'application avec les
/// arguments du lancement courant. `--silent` en fait partie, donc une mise a jour
/// partie d'une session ouverte en fond ne ramene pas le panneau a l'ecran.
async fn install_pending(app: &AppHandle) -> Result<(), String> {
    let update = {
        let state = app.state::<Pending>();
        if state.installing.swap(true, Ordering::SeqCst) {
            return Err(crate::t!(
                "an installation is already running",
                "une installation est déjà en cours"
            )
            .into());
        }
        let taken = state.slot.lock().ok().and_then(|mut slot| slot.take());
        match taken {
            Some(update) => update,
            None => {
                state.installing.store(false, Ordering::SeqCst);
                return Err(crate::t!("no pending update", "aucune mise à jour en attente").into());
            }
        }
    };

    let outcome = download_and_install(app, update).await;
    if outcome.is_err() {
        // Le paquet n'est pas installe et le processus est toujours la : rouvrir la
        // porte, sans quoi une coupure reseau condamnerait le bouton jusqu'au prochain
        // lancement.
        app.state::<Pending>()
            .installing
            .store(false, Ordering::SeqCst);
    }
    outcome
}

async fn download_and_install(app: &AppHandle, update: Update) -> Result<(), String> {
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

/// Vrai quand le panneau est sous les yeux de quelqu'un.
fn panel_open(app: &AppHandle) -> bool {
    app.get_webview_window(flyout::MAIN)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// La boucle du mode automatique.
///
/// Elle ne touche a rien tant que la case n'est pas cochee, et differe tant que le
/// panneau est ouvert : installer veut dire ici remplacer l'executable et relancer le
/// processus, ce qui ferait disparaitre la fenetre au milieu d'une lecture. Le panneau
/// se referme des qu'il perd le focus, l'attente se resout donc d'elle-meme au battement
/// suivant ; elle ne dure que pour un panneau epingle, ou personne n'est surpris de le
/// voir rester.
///
/// Un fil et non une tache asynchrone : la seule chose a faire entre deux battements est
/// d'attendre, et `tauri::async_runtime` n'offre pas de minuterie sans ajouter tokio aux
/// dependances pour cinq lignes.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<Instant> = None;
        loop {
            std::thread::sleep(TICK);

            if !app.state::<Settings>().prefs().auto_update {
                // Decocher puis recocher redemande une recherche au battement suivant :
                // sans cela, reactiver l'option pourrait n'avoir aucun effet pendant six
                // heures, et ressemblerait a une case sans effet.
                last = None;
                continue;
            }
            if panel_open(&app) {
                continue;
            }
            if last.is_some_and(|t| t.elapsed() < PERIOD) {
                continue;
            }
            last = Some(Instant::now());

            // Silencieuse des deux cotes : ne pas joindre GitHub n'est pas un evenement,
            // et une installation qui echoue sera retentee au tour suivant. Le bouton du
            // panneau reste la pour obtenir une erreur lisible, puisqu'on la lui a
            // demandee.
            tauri::async_runtime::block_on(async {
                if matches!(fetch(&app).await, Ok(Some(_))) {
                    let _ = install_pending(&app).await;
                }
            });
        }
    });
}
