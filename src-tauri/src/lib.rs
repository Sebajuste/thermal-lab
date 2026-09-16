mod autostart;
mod capabilities;
mod flyout;
mod i18n;
mod phases;
mod power;
mod profiles;
mod restore;
mod settings;
mod tools;
mod tray;
mod update;

/// Pipeline de mesure, expose : c'est la partie reutilisable de ce crate, independante
/// de Tauri comme de l'UI.
pub mod sensors;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use capabilities::Capabilities;
use flyout::Flyout;
use phases::{PhaseKey, PhaseRecorder, PhasesSnapshot};
use power::PowerState;
use profiles::{ProfileId, ProfileInfo};
use restore::{Journal, Restorer, SystemPower};
use sensors::{Reading, SensorHub};
use settings::Settings;

/// Le gardien du schema d'alimentation : toute modification passe par lui, et il est le
/// seul a savoir ce que la machine nous doit.
type PowerGuard = Restorer<SystemPower>;

/// Cadence panneau ouvert : c'est le rythme auquel les valeurs s'affichent, et un
/// affichage qui traine se remarque tout de suite.
const ACTIVE_PERIOD: Duration = Duration::from_millis(1000);

/// Cadence panneau replie. L'application passe l'essentiel de sa vie ici, et personne
/// ne lit les valeurs : les deux seuls consommateurs sont l'infobulle de l'icone — vue
/// au survol seulement — et les moyennes du comparatif, qui ne demandent pas de
/// resolution fine. Cinq fois moins de mesures, donc cinq fois moins de tout ce qu'elles
/// coutent : requetes WMI, lectures NVML, memoire partagee.
const IDLE_PERIOD: Duration = Duration::from_millis(5000);

/// Argument pose par le demarrage automatique : la session s'ouvre, l'application se
/// range dans la zone de notification sans reclamer l'ecran.
pub(crate) const SILENT_FLAG: &str = "--silent";

/// Ce que l'interface a besoin de savoir sur son propre chassis.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    pinned: bool,
    /// Etat d'ouverture au moment du montage. L'interface ne peut pas le deviner : elle
    /// est chargee des le lancement, y compris quand le panneau demarre replie.
    panel_visible: bool,
    autostart: bool,
    /// Mise a jour sans intervention : cochee, l'application se remplace elle-meme.
    auto_update: bool,
    /// Celle du paquet, pas celle du frontend : c'est elle que la mise a jour compare.
    version: String,
    /// Le profil que l'interrupteur applique : une preference, pas l'etat de la machine.
    profile: ProfileId,
    /// Les profils proposes, dans l'ordre de presentation. Tenus cote Rust : ce sont eux
    /// que l'interrupteur applique, et une copie dans le frontend finirait par diverger.
    profiles: Vec<ProfileInfo>,
}

/// La reponse a un changement de profil : ce qui a ete enregistre, et la machine telle
/// qu'elle est ensuite.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileChoice {
    profile: ProfileId,
    power: PowerState,
}

#[tauri::command]
fn read_sensors(hub: tauri::State<'_, SensorHub>) -> Reading {
    hub.latest()
}

#[tauri::command]
fn read_capabilities(hub: tauri::State<'_, SensorHub>) -> Capabilities {
    capabilities::collect(&hub)
}

/// Relit l'etat d'alimentation et le resynchronise partout : le schema peut aussi
/// changer depuis Windows, sans passer par nous.
#[tauri::command]
fn power_state(app: AppHandle) -> Result<PowerState, String> {
    let state = power::state()?;
    sync_power(&app, &state);
    Ok(state)
}

#[tauri::command]
fn set_optimization(app: AppHandle, on: bool) -> Result<PowerState, String> {
    apply_optimization(&app, on)
}

#[tauri::command]
fn read_phases(recorder: tauri::State<'_, Arc<PhaseRecorder>>) -> PhasesSnapshot {
    recorder.snapshot()
}

#[tauri::command]
fn reset_phases(recorder: tauri::State<'_, Arc<PhaseRecorder>>) {
    recorder.reset();
}

/// Lance un outil tiers avec elevation. Windows pose la question ; nous ne faisons que
/// la declencher, sur un geste explicite de l'utilisateur.
#[tauri::command]
fn launch_tool(id: String) -> Result<(), String> {
    tools::launch(&id)
}

/// Ouvre une page dans le navigateur du systeme : le webview ne sort pas de lui-meme.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    tools::open_url(&url)
}

#[tauri::command]
fn start_drag(app: AppHandle) -> Result<(), String> {
    flyout::start_drag(&app)
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    flyout::hide(&app);
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn ui_state(app: AppHandle) -> UiState {
    UiState {
        pinned: app.state::<Flyout>().is_pinned(),
        panel_visible: flyout::is_visible(&app),
        autostart: autostart::is_enabled(),
        auto_update: app.state::<Settings>().prefs().auto_update,
        version: app.package_info().version.to_string(),
        profile: app.state::<Settings>().prefs().profile,
        profiles: profiles::catalog(),
    }
}

/// Lue avant le premier rendu : les libelles sont figes a la construction de plusieurs
/// tables de module, qui ne peuvent pas attendre un etat React.
#[tauri::command]
fn ui_lang() -> i18n::Lang {
    i18n::lang()
}

#[tauri::command]
fn set_pinned(app: AppHandle, pinned: bool) {
    app.state::<Flyout>().set_pinned(pinned);
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    tray::set_autostart(&app, on)?;
    Ok(autostart::is_enabled())
}

/// Rend l'etat reellement enregistre, et non celui demande : une ecriture refusee doit
/// decocher la case plutot que promettre une mise a jour automatique qui n'aura pas lieu.
#[tauri::command]
fn set_auto_update(app: AppHandle, on: bool) -> Result<bool, String> {
    Ok(app.state::<Settings>().set_auto_update(on)?.auto_update)
}

/// Enregistre le profil choisi, et l'applique aussitot si l'interrupteur est allume :
/// changer de profil en cours de route ne doit pas demander d'eteindre puis de rallumer.
///
/// Eteint, on n'ecrit rien sur la machine — le choix attend le prochain allumage.
#[tauri::command]
fn set_profile(app: AppHandle, id: ProfileId) -> Result<ProfileChoice, String> {
    let profile = app.state::<Settings>().set_profile(id)?.profile;
    let current = power::state()?;
    let power = if current.optimized {
        let state = app.state::<PowerGuard>().engage(profiles::get(profile))?;
        sync_power(&app, &state);
        state
    } else {
        current
    };
    Ok(ProfileChoice { profile, power })
}

/// Bascule l'interrupteur principal, puis remet tout le monde d'accord : accumulateur de
/// phases, icone, menu, et interface si elle est ouverte.
///
/// Allume, il applique le profil choisi ; eteint, il rend la machine.
pub(crate) fn apply_optimization(app: &AppHandle, on: bool) -> Result<PowerState, String> {
    let guard = app.state::<PowerGuard>();
    let state = if on {
        let chosen = app.state::<Settings>().prefs().profile;
        guard.engage(profiles::get(chosen))?
    } else {
        guard.release()?
    };
    sync_power(app, &state);
    Ok(state)
}

/// Rend la machine telle qu'elle etait avant l'intervention, s'il y en a eu une.
///
/// Appele aux deux bouts : au demarrage, pour solder un arret qui n'a pas pu le faire
/// lui-meme — fin de session, plantage, arret de tache — et a la sortie, pour la notre.
/// Une erreur ici n'a plus d'interface ou s'afficher : le journal reste, et le lancement
/// suivant reessaie.
fn restore_machine(app: &AppHandle) {
    match app.state::<PowerGuard>().restore() {
        Ok(Some(state)) => sync_power(app, &state),
        Ok(None) => {}
        Err(e) => eprintln!("restauration du schema d'alimentation : {e}"),
    }
}

fn sync_power(app: &AppHandle, state: &PowerState) {
    app.state::<Arc<PhaseRecorder>>()
        .set_phase(PhaseKey::of(state));
    tray::set_optimized(app, state.optimized);
    tray::set_can_toggle(app, state.elevated);
    let _ = app.emit("power-changed", state);
}

/// Regle la cadence de mesure sur ce que le panneau demande. Appele par `flyout` a
/// chaque ouverture et chaque fermeture — les deux seuls moments ou le besoin change.
///
/// `try_state` plutot que `state` : le panneau peut s'ouvrir avant que le hub ne soit
/// enregistre, et un panneau qui refuse de s'ouvrir serait un bien mauvais prix a payer
/// pour une cadence.
pub(crate) fn set_sampling(app: &AppHandle, panel_visible: bool) {
    if let Some(hub) = app.try_state::<SensorHub>() {
        hub.set_period(if panel_visible {
            ACTIVE_PERIOD
        } else {
            IDLE_PERIOD
        });
    }
    // Le meme signal regle les deux cotes. Emis ailleurs, il pourrait dire « ouvert »
    // pendant que le hub mesure toutes les cinq secondes : l'interface interrogerait
    // quatre fois pour rien.
    let _ = app.emit("panel-visibility", panel_visible);
}

/// Une erreur nee dans le menu de l'icone n'a nulle part ou s'afficher : on la pousse
/// vers le panneau, et on l'ouvre pour qu'elle soit vue.
pub(crate) fn report_error(app: &AppHandle, message: &str) {
    let _ = app.emit("app-error", message);
    flyout::show(app);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Une seule instance, sinon deux icones et deux boucles de mesure pour un seul
        // etat systeme. Relancer l'executable revient a rappeler le panneau.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            flyout::show(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();

            // Reliquat de l'implementation precedente du demarrage automatique.
            autostart::clear_legacy();

            let silent = std::env::args().any(|a| a == SILENT_FLAG);

            let recorder = Arc::new(PhaseRecorder::new());
            app.manage(Arc::clone(&recorder));
            app.manage(Flyout::new());
            app.manage(update::Pending::default());
            app.manage(Settings::load(&handle));

            // Le journal de restauration vit dans le dossier de configuration du compte,
            // aux cotes des reglages. Sans dossier, pas de journal — et l'interrupteur
            // refusera de brider plutot que de promettre un retour en arriere.
            let journal = match handle.path().app_config_dir() {
                Ok(dir) => Journal::in_dir(&dir),
                Err(_) => Journal::nowhere(),
            };
            let guard = PowerGuard::new(SystemPower, journal);

            // Avant de lire quoi que ce soit : un journal encore rempli signe un arret
            // qui n'a pas pu rendre la machine. On solde la dette, et l'etat lu ensuite
            // est celui d'une machine rendue.
            if let Err(e) = guard.restore() {
                eprintln!("restauration du schema d'alimentation : {e}");
            }
            app.manage(guard);

            // L'etat d'alimentation precede tout le reste : il decide de l'icone posee,
            // de la coche du menu et de la phase qui commence a accumuler.
            let initial = power::state().ok();
            let optimized = initial.as_ref().is_some_and(|s| s.optimized);
            let elevated = initial.as_ref().is_some_and(|s| s.elevated);
            recorder.set_phase(initial.as_ref().map_or(PhaseKey::Free, PhaseKey::of));

            tray::build(&handle, optimized, elevated)?;

            let tick_handle = handle.clone();
            let sink = Arc::clone(&recorder);
            let initial = if silent { IDLE_PERIOD } else { ACTIVE_PERIOD };
            app.manage(SensorHub::start(initial, move |reading, dt| {
                sink.record(reading, dt.as_secs_f64());
                tray::refresh_tooltip(&tick_handle, reading, sink.is_capped());
            }));

            // La veille des mises a jour tourne quoi qu'il arrive : c'est elle qui lit
            // la preference a chaque battement, et non l'inverse. La cocher ou la
            // decocher n'a donc rien a demarrer ni a arreter.
            update::watch(handle.clone());

            if !silent {
                flyout::show(&handle);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Fermer le panneau ne ferme pas l'application : c'est tout le principe du
            // mode residant. La sortie est dans le menu de l'icone.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                flyout::hide(window.app_handle());
            }
            WindowEvent::Focused(true) => flyout::on_focus_gained(window.app_handle()),
            WindowEvent::Focused(false) => flyout::on_focus_lost(window.app_handle()),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            read_sensors,
            read_capabilities,
            launch_tool,
            open_url,
            read_phases,
            reset_phases,
            power_state,
            set_optimization,
            start_drag,
            hide_window,
            quit_app,
            ui_state,
            ui_lang,
            set_pinned,
            set_autostart,
            set_auto_update,
            set_profile,
            update::check_update,
            update::install_update
        ])
        .build(tauri::generate_context!())
        .expect("Tauri failed to start")
        // `run` plutot que la forme courte : c'est la seule qui donne la main sur la
        // sortie. `Exit` passe quelle que soit la porte empruntee — menu de l'icone,
        // commande du panneau, redemarrage de mise a jour — et c'est la que la machine
        // est rendue. Ce qui ne passe par aucune porte, la fin de session Windows ou un
        // plantage, est rattrape au lancement suivant par le journal.
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                restore_machine(app);
            }
        });
}
