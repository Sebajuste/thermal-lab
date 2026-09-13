//! Le panneau : une fenetre sans bordure, ancree sur l'icone de la zone de notification.
//!
//! Comportement calque sur les volets systeme de Windows 11 (volume, reseau) : un clic
//! sur l'icone ouvre le panneau juste au-dessus d'elle, un clic ailleurs le referme,
//! et la fenetre n'apparait pas dans la barre des taches. La fermer ne ferme jamais
//! l'application : seul le menu contextuel de l'icone le fait.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

/// Label de l'unique fenetre, tel que declare dans `tauri.conf.json`.
pub const MAIN: &str = "main";

/// Marge entre le panneau et le bord de l'ecran ou l'icone, en pixels physiques a 100 %.
const GAP: f64 = 12.0;

/// Hauteur supposee de la barre des taches quand on n'a pas d'ancre — uniquement pour
/// le repli « ouvrir depuis le menu avant tout clic sur l'icone ».
const TASKBAR_GUESS: f64 = 56.0;

/// Cliquer sur l'icone alors que le panneau est ouvert fait d'abord perdre le focus a
/// celui-ci — il se masque — puis declenche notre gestionnaire de clic, qui le verrait
/// masque et le rouvrirait aussitot. Le panneau deviendrait impossible a refermer par
/// son icone. On ignore donc une demande d'ouverture qui suit de trop pres un
/// masquage par perte de focus.
const REOPEN_GUARD: Duration = Duration::from_millis(350);

/// Delai de grace apres une ouverture. Windows n'accorde pas toujours le focus a une
/// fenetre qui se montre : lancee depuis un terminal, ou rappelee pendant qu'un jeu tient
/// le premier plan, elle peut le recevoir puis le reperdre dans la foulee. Sans ce delai
/// le panneau disparaitrait avant d'avoir ete vu. Constate au lancement depuis
/// PowerShell : ouverture, focus, perte de focus immediate.
const SETTLE: Duration = Duration::from_millis(600);

/// Rectangle de l'icone dans la zone de notification, en pixels physiques.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct Flyout {
    /// Epinglage : le panneau reste ouvert malgre la perte de focus, et cesse d'etre
    /// repositionne. Indispensable des qu'on veut le regarder pendant qu'on joue,
    /// c'est-a-dire pendant que le focus est ailleurs par construction.
    pinned: AtomicBool,
    /// Echappatoire de developpement : le masquage automatique rend les devtools
    /// inutilisables, puisque leur ouverture vole le focus au panneau.
    autohide: bool,
    /// Le masquage sur perte de focus ne s'arme qu'une fois le focus obtenu. Sans cela,
    /// un panneau affiche alors qu'une autre fenetre garde le focus — au lancement
    /// depuis un terminal, ou depuis le menu de l'icone pendant qu'un jeu a la main —
    /// recevrait un `Focused(false)` immediat et disparaitrait avant d'avoir ete vu.
    armed: AtomicBool,
    anchor: Mutex<Option<Anchor>>,
    hidden_at: Mutex<Option<Instant>>,
    shown_at: Mutex<Option<Instant>>,
}

impl Flyout {
    pub fn new() -> Self {
        Self {
            pinned: AtomicBool::new(false),
            autohide: std::env::var_os("THERMAL_LAB_NO_AUTOHIDE").is_none(),
            armed: AtomicBool::new(false),
            anchor: Mutex::new(None),
            hidden_at: Mutex::new(None),
            shown_at: Mutex::new(None),
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.load(Ordering::Relaxed)
    }

    pub fn set_pinned(&self, on: bool) {
        self.pinned.store(on, Ordering::Relaxed);
    }

    fn remember(&self, anchor: Anchor) {
        if let Ok(mut slot) = self.anchor.lock() {
            *slot = Some(anchor);
        }
    }

    fn anchor(&self) -> Option<Anchor> {
        self.anchor.lock().ok().and_then(|a| *a)
    }

    /// Consomme le verrou de reouverture : vrai si le panneau vient d'etre masque par
    /// perte de focus, auquel cas le clic en cours *est* celui qui l'a masque.
    fn take_reopen_guard(&self) -> bool {
        match self.hidden_at.lock() {
            Ok(mut slot) => {
                let recent = slot.is_some_and(|t| t.elapsed() < REOPEN_GUARD);
                *slot = None;
                recent
            }
            Err(_) => false,
        }
    }

    fn within_settle(&self) -> bool {
        self.shown_at
            .lock()
            .ok()
            .and_then(|t| *t)
            .is_some_and(|t| t.elapsed() < SETTLE)
    }

    fn arm_reopen_guard(&self) {
        if let Ok(mut slot) = self.hidden_at.lock() {
            *slot = Some(Instant::now());
        }
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN)
}

pub fn show(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let state = app.state::<Flyout>();
    state.armed.store(false, Ordering::Relaxed);
    if let Ok(mut slot) = state.shown_at.lock() {
        *slot = Some(Instant::now());
    }

    // Epingle, le panneau appartient a l'utilisateur : il l'a peut-etre deplace, et le
    // rappeler contre l'icone a chaque ouverture annulerait ce geste. Masquer puis
    // reafficher conserve la position, il suffit donc de ne pas y toucher.
    if !state.is_pinned() {
        place(app, &win);
    }

    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

/// Saisit la fenetre pour la deplacer.
///
/// Passe par une commande plutot que par `data-tauri-drag-region` : l'attribut exige la
/// permission `core:window:allow-start-dragging`, absente du jeu par defaut, et son
/// absence ne se voit nulle part — le panneau etait simplement impossible a deplacer,
/// sans le moindre message. Une commande echoue bruyamment, ce qui vaut mieux.
pub fn start_drag(app: &AppHandle) -> Result<(), String> {
    window(app)
        .ok_or_else(|| crate::t!("window missing", "fenetre absente").to_string())?
        .start_dragging()
        .map_err(|e| format!("deplacement impossible : {e}"))
}

pub fn hide(app: &AppHandle) {
    if let Some(win) = window(app) {
        let _ = win.hide();
    }
}

/// Clic gauche sur l'icone : ouvre ou referme.
pub fn toggle(app: &AppHandle, anchor: Option<Anchor>) {
    let state = app.state::<Flyout>();
    if let Some(a) = anchor {
        state.remember(a);
    }

    if state.take_reopen_guard() {
        return;
    }

    let visible = window(app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if visible {
        hide(app);
    } else {
        show(app);
    }
}

/// Focus obtenu : a partir de maintenant, le perdre veut dire quelque chose.
pub fn on_focus_gained(app: &AppHandle) {
    app.state::<Flyout>().armed.store(true, Ordering::Relaxed);
}

/// Perte de focus : on se referme, sauf si l'utilisateur a epingle le panneau.
pub fn on_focus_lost(app: &AppHandle) {
    let state = app.state::<Flyout>();
    if !state.autohide || state.is_pinned() || state.within_settle() {
        return;
    }
    if !state.armed.swap(false, Ordering::Relaxed) {
        return;
    }
    state.arm_reopen_guard();
    hide(app);
}

/// Place le panneau contre l'icone, du bon cote de la barre des taches, sans deborder
/// de l'ecran qui porte cette icone.
fn place(app: &AppHandle, win: &WebviewWindow) {
    let Ok(size) = win.outer_size() else { return };
    let (w, h) = (size.width as f64, size.height as f64);

    let anchor = app.state::<Flyout>().anchor();
    let point = anchor.map(|a| (a.x + a.width / 2.0, a.y + a.height / 2.0));

    let monitor = point
        .and_then(|(px, py)| pick_monitor(app, px, py))
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };

    let scale = monitor.scale_factor();
    let gap = GAP * scale;
    let mx = monitor.position().x as f64;
    let my = monitor.position().y as f64;
    let mw = monitor.size().width as f64;
    let mh = monitor.size().height as f64;

    let (x, y) = match anchor {
        Some(a) => {
            // La barre des taches est du cote ou se trouve l'icone : si celle-ci est
            // dans la moitie basse de l'ecran, le panneau s'ouvre au-dessus.
            let below = a.y + a.height / 2.0 < my + mh / 2.0;
            let y = if below {
                a.y + a.height + gap
            } else {
                a.y - h - gap
            };
            (a.x + a.width / 2.0 - w / 2.0, y)
        }
        None => (mx + mw - w - gap, my + mh - h - TASKBAR_GUESS * scale - gap),
    };

    let x = x.clamp(mx + gap, (mx + mw - w - gap).max(mx));
    let y = y.clamp(my + gap, (my + mh - h - gap).max(my));

    let _ = win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn pick_monitor(app: &AppHandle, x: f64, y: f64) -> Option<tauri::window::Monitor> {
    app.available_monitors().ok()?.into_iter().find(|m| {
        let p = m.position();
        let s = m.size();
        x >= p.x as f64
            && x < p.x as f64 + s.width as f64
            && y >= p.y as f64
            && y < p.y as f64 + s.height as f64
    })
}
