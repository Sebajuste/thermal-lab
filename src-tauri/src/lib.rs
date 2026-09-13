mod autostart;
mod capabilities;
mod flyout;
mod phases;
mod power;
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
use phases::{PhaseRecorder, PhasesSnapshot};
use power::PowerState;
use sensors::{Reading, SensorHub};

const SAMPLE_PERIOD: Duration = Duration::from_millis(1000);

/// Argument pose par le demarrage automatique : la session s'ouvre, l'application se
/// range dans la zone de notification sans reclamer l'ecran.
pub(crate) const SILENT_FLAG: &str = "--silent";

/// Ce que l'interface a besoin de savoir sur son propre chassis.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    pinned: bool,
    autostart: bool,
    /// Celle du paquet, pas celle du frontend : c'est elle que la mise a jour compare.
    version: String,
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
        autostart: autostart::is_enabled(),
        version: app.package_info().version.to_string(),
    }
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

/// Bascule le bridage, puis remet tout le monde d'accord : accumulateur de phases,
/// icone, menu, et interface si elle est ouverte.
pub(crate) fn apply_optimization(app: &AppHandle, on: bool) -> Result<PowerState, String> {
    let state = power::set_optimized(on)?;
    sync_power(app, &state);
    Ok(state)
}

fn sync_power(app: &AppHandle, state: &PowerState) {
    app.state::<Arc<PhaseRecorder>>()
        .set_optimized(state.optimized);
    tray::set_optimized(app, state.optimized);
    tray::set_can_toggle(app, state.elevated);
    let _ = app.emit("power-changed", state);
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

            let recorder = Arc::new(PhaseRecorder::new(SAMPLE_PERIOD.as_secs_f64()));
            app.manage(Arc::clone(&recorder));
            app.manage(Flyout::new());
            app.manage(update::Pending::default());

            // L'etat d'alimentation precede tout le reste : il decide de l'icone posee,
            // de la coche du menu et de la phase qui commence a accumuler.
            let initial = power::state().ok();
            let optimized = initial.as_ref().is_some_and(|s| s.optimized);
            let elevated = initial.as_ref().is_some_and(|s| s.elevated);
            recorder.set_optimized(optimized);

            tray::build(&handle, optimized, elevated)?;

            let tick_handle = handle.clone();
            let sink = Arc::clone(&recorder);
            app.manage(SensorHub::start(SAMPLE_PERIOD, move |reading| {
                sink.record(reading);
                tray::refresh_tooltip(&tick_handle, reading, sink.is_optimized());
            }));

            if !std::env::args().any(|a| a == SILENT_FLAG) {
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
            set_pinned,
            set_autostart,
            update::check_update,
            update::install_update
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application Tauri");
}
