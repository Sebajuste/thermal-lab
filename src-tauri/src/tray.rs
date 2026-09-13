//! L'icone de la zone de notification : le vrai point d'entree de l'application.
//!
//! C'est elle qui porte l'etat en permanence — sa couleur dit si le bridage est actif,
//! son infobulle donne les dernieres mesures sans rien ouvrir — et c'est son menu
//! contextuel qui detient la seule commande de sortie.

use std::sync::Mutex;

use crate::flyout::{self, Anchor};
use crate::sensors::{Metric, Reading};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "thermal-lab";

/// L'icone du bundle, en 32 px : Windows la redescend a la taille de la zone de
/// notification selon la mise a l'echelle du poste.
const BASE_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// Vert d'etat, identique a `--good` dans la feuille de style : l'icone teintee et la
/// pastille du panneau doivent designer la meme chose.
const OPTIMIZED_RGB: (f32, f32, f32) = (126.0, 231.0, 135.0);

pub struct Tray {
    free: Image<'static>,
    optimized: Image<'static>,
    toggle_item: CheckMenuItem<Wry>,
    autostart_item: CheckMenuItem<Wry>,
    tooltip: Mutex<String>,
}

/// Reduit l'icone a sa luminance puis la reteinte. La silhouette survit a 16 px, la
/// couleur devient lisible d'un coup d'oeil — c'est tout ce qu'on demande a une icone
/// de zone de notification.
fn tinted(src: &Image<'_>, rgb: (f32, f32, f32)) -> Image<'static> {
    let mut rgba = src.rgba().to_vec();
    // `as_chunks_mut` plutot que `chunks_exact_mut` : la taille etant constante, le
    // compilateur rend un tableau de 4 et non une tranche, donc sans borne a verifier.
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    for px in pixels {
        let lum = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
        // Racine quatrieme plutot que lineaire : l'icone d'origine est sombre, une
        // teinte proportionnelle a sa luminance la rendrait presque noire.
        let l = (lum / 255.0).powf(0.45);
        px[0] = (rgb.0 * l).min(255.0) as u8;
        px[1] = (rgb.1 * l).min(255.0) as u8;
        px[2] = (rgb.2 * l).min(255.0) as u8;
    }
    Image::new_owned(rgba, src.width(), src.height())
}

pub fn build(app: &AppHandle, optimized: bool, can_toggle: bool) -> tauri::Result<()> {
    let base = Image::from_bytes(BASE_ICON)?;
    let free = Image::new_owned(base.rgba().to_vec(), base.width(), base.height());
    let optimized_icon = tinted(&base, OPTIMIZED_RGB);

    let open = MenuItem::with_id(
        app,
        "open",
        crate::t!("Open Thermal Lab", "Ouvrir Thermal Lab"),
        true,
        None::<&str>,
    )?;
    let toggle_item = CheckMenuItem::with_id(
        app,
        "toggle",
        crate::t!("Optimization (turbo capped)", "Optimisation (turbo bridé)"),
        can_toggle,
        optimized,
        None::<&str>,
    )?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        "autostart",
        crate::t!("Start with Windows", "Démarrer avec Windows"),
        true,
        crate::autostart::is_enabled(),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        crate::t!("Quit Thermal Lab", "Quitter Thermal Lab"),
        true,
        None::<&str>,
    )?;

    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &toggle_item,
            &autostart_item,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let icon = if optimized {
        optimized_icon.clone()
    } else {
        free.clone()
    };

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Thermal Lab")
        .menu(&menu)
        // Le clic gauche ouvre le panneau ; le menu reste sur le clic droit, comme
        // partout ailleurs dans la zone de notification.
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(on_icon)
        .build(app)?;

    app.manage(Tray {
        free,
        optimized: optimized_icon,
        toggle_item,
        autostart_item,
        tooltip: Mutex::new(String::new()),
    });

    Ok(())
}

fn on_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "open" => flyout::show(app),
        "toggle" => {
            let state = app.state::<Tray>();
            let wanted = state.toggle_item.is_checked().unwrap_or(false);
            if let Err(e) = crate::apply_optimization(app, wanted) {
                // Le menu a deja bascule sa coche : la remettre sur l'etat reel.
                let _ = state.toggle_item.set_checked(!wanted);
                crate::report_error(app, &e);
            }
        }
        "autostart" => {
            let state = app.state::<Tray>();
            let wanted = state.autostart_item.is_checked().unwrap_or(false);
            if let Err(e) = set_autostart(app, wanted) {
                let _ = state.autostart_item.set_checked(!wanted);
                crate::report_error(app, &e);
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn on_icon(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        rect,
        ..
    } = event
    {
        let position = rect.position.to_physical::<f64>(1.0);
        let size = rect.size.to_physical::<f64>(1.0);
        flyout::toggle(
            tray.app_handle(),
            Some(Anchor {
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
            }),
        );
    }
}

pub fn set_autostart(_app: &AppHandle, on: bool) -> Result<(), String> {
    crate::autostart::set(on).map_err(|e| {
        crate::t!(
            format!("autostart: {e}"),
            format!("démarrage automatique : {e}")
        )
    })
}

/// Repercute l'etat du bridage sur l'icone et sur la coche du menu.
pub fn set_optimized(app: &AppHandle, optimized: bool) {
    let state = app.state::<Tray>();
    let _ = state.toggle_item.set_checked(optimized);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let icon = if optimized {
            state.optimized.clone()
        } else {
            state.free.clone()
        };
        let _ = tray.set_icon(Some(icon));
    }
}

pub fn set_can_toggle(app: &AppHandle, can: bool) {
    let _ = app.state::<Tray>().toggle_item.set_enabled(can);
}

/// Infobulle : l'essentiel sans rien ouvrir. Windows la limite a 127 caracteres.
pub fn refresh_tooltip(app: &AppHandle, reading: &Reading, optimized: bool) {
    let cpu = line(
        "CPU",
        reading.get(Metric::CpuTempC),
        reading.get(Metric::CpuPowerW),
    );
    let gpu = line(
        "GPU",
        reading.get(Metric::GpuTempC),
        reading.get(Metric::GpuPowerW),
    );
    let head = if optimized {
        crate::t!(
            "Thermal Lab — optimization on",
            "Thermal Lab — optimisation active"
        )
    } else {
        crate::t!(
            "Thermal Lab — optimization off",
            "Thermal Lab — optimisation inactive"
        )
    };
    let text = format!("{head}\n{cpu}{gpu}");

    let state = app.state::<Tray>();
    // Les appels Win32 d'infobulle ne valent pas une ecriture par seconde pour un texte
    // identique.
    match state.tooltip.lock() {
        Ok(mut last) if *last != text => *last = text.clone(),
        _ => return,
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(&text));
    }
}

fn line(label: &str, temp: Option<f64>, power: Option<f64>) -> String {
    match (temp, power) {
        (None, None) => String::new(),
        (t, p) => {
            let t = t.map(|v| format!("{v:.0} °C")).unwrap_or_default();
            let p = p.map(|v| format!("{v:.0} W")).unwrap_or_default();
            let sep = if t.is_empty() || p.is_empty() {
                ""
            } else {
                " · "
            };
            format!("{label} {t}{sep}{p}\n")
        }
    }
}
