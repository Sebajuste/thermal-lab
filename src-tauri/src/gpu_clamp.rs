//! Brider une carte graphique qui consomme sans rien produire.
//!
//! Le cas qui motive ce module : une RTX 2000 Ada de portable qui tire 18 W en continu,
//! sans ecran, sans client, a 0 % d'utilisation, et que le pilote ne ramene jamais a son
//! etat de repos. Aucune API ne force une carte a s'eteindre ; on peut en revanche
//! verrouiller ses horloges sur la plage de son etat le plus repose, que la carte declare
//! elle-meme. C'est un levier de suppression de gaspillage : il n'agit que quand il n'y a
//! rien a perdre.
//!
//! Trois garde-fous, dans l'ordre ou ils comptent :
//!
//! - **ne brider que l'anomalie etablie** : pilote qui declare la carte inoccupee, aucun
//!   ecran attache, etat de performance eleve, charge nulle — plusieurs releves de suite.
//!   Une carte qui affiche n'est jamais touchee : le verrou de sa memoire ferait des
//!   artefacts sur l'ecran qu'elle balaie ;
//! - **relacher au premier signe de travail** : un jeu qui demarre ne doit pas attendre.
//!   L'asymetrie est voulue — brider trop tard coute quelques watts, relacher trop tard
//!   fait saccader. Replie, la mesure bat toutes les cinq secondes : c'est le delai
//!   maximal, couvert par n'importe quel ecran de chargement ;
//! - **ne jamais laisser une carte bridee derriere soi** : un journal est pose avant le
//!   verrou et retire apres. La sortie relache ; un arret que l'application ne voit pas
//!   venir est solde au lancement suivant. Les verrous tombent de toute facon au
//!   redemarrage du pilote, mais une carte bridee a l'etat de repos pendant qu'on joue
//!   n'est pas un defaut qu'on laisse au hasard.
//!
//! Desactive par defaut : c'est une ecriture dans le pilote graphique, qu'on a demandee.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};
use nvml_wrapper::enums::device::GpuLockedClocksSetting;
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::Nvml;
use serde::Serialize;

use crate::sensors::{Metric, Reading};

const FILE: &str = "gpu-clamp.json";

/// Releves concordants avant de brider : quinze secondes panneau replie, trois ouvert.
const ENGAGE_AFTER: u32 = 3;

/// Memes seuils que le badge : sous le premier la carte est inoccupee, au-dessus du
/// second elle travaille. L'ecart evite de relacher sur le jitter du repos.
const IDLE_ENTER_PCT: f64 = 10.0;
const IDLE_LEAVE_PCT: f64 = 20.0;

/// Au-dela, la carte est deja dans un etat de repos : rien a brider.
const PINNED_PSTATE: f64 = 5.0;

/// Ce qu'un releve dit, du point de vue du bridage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    /// Inoccupee, sans ecran, et pourtant en etat de performance.
    Waste,
    /// Rien a faire : deja au repos.
    Quiet,
    /// Elle travaille, ou elle affiche : ne pas y toucher.
    Busy,
    /// Une grandeur manque : on ne conclut pas, et dans le doute on relache.
    Unknown,
}

/// Le meme enonce que le badge, lu du point de vue d'un actionneur.
///
/// Bridee, la carte est redescendue par notre fait : son etat de performance ne dit plus
/// rien et n'est pas consulte. Seuls comptent alors le travail et l'ecran.
fn assess(r: &Reading, clamped: bool) -> Sign {
    let (Some(util), Some(driver_idle), Some(display)) = (
        r.get(Metric::GpuUtilPct),
        r.get(Metric::GpuDriverIdle),
        r.get(Metric::GpuDisplayActive),
    ) else {
        return Sign::Unknown;
    };
    let load = util
        .max(r.get(Metric::GpuDecodeUtilPct).unwrap_or(0.0))
        .max(r.get(Metric::GpuEncodeUtilPct).unwrap_or(0.0));

    let working = if clamped {
        load > IDLE_LEAVE_PCT
    } else {
        load >= IDLE_ENTER_PCT
    };
    if working || driver_idle != 1.0 || display != 0.0 {
        return Sign::Busy;
    }
    if clamped {
        return Sign::Waste;
    }
    match r.get(Metric::GpuPerfStateIndex) {
        None => Sign::Unknown,
        Some(p) if p > PINNED_PSTATE => Sign::Quiet,
        Some(_) => Sign::Waste,
    }
}

/// Pourquoi un verrou n'a pas pu etre pose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClampError {
    /// La carte, le pilote ou les droits ne le permettent pas : inutile de reessayer.
    Unsupported(String),
    /// Un echec qui peut ne pas se reproduire.
    Failed(String),
}

/// La carte, vue comme deux gestes. L'indirection n'existe que pour les tests : les
/// scenarios se verifient sans materiel, et sans rien ecrire dans le pilote de la machine
/// qui compile.
pub trait GpuWriter: Send {
    fn clamp(&mut self) -> Result<(), ClampError>;
    fn release(&mut self) -> Result<(), String>;
}

/// La vraie carte, par NVML. Premiere carte seulement, comme le reste de l'application.
#[derive(Default)]
pub struct NvmlWriter {
    nvml: Option<Nvml>,
}

impl NvmlWriter {
    fn nvml(&mut self) -> Result<&Nvml, NvmlError> {
        if self.nvml.is_none() {
            self.nvml = Some(Nvml::init()?);
        }
        Ok(self.nvml.as_ref().expect("initialise juste au-dessus"))
    }
}

fn classify(e: NvmlError) -> ClampError {
    match e {
        NvmlError::NotSupported => ClampError::Unsupported(
            crate::t!(
                "this card does not accept clock locks",
                "cette carte n'accepte pas le verrouillage d'horloge"
            )
            .into(),
        ),
        NvmlError::NoPermission => ClampError::Unsupported(
            crate::t!(
                "administrator rights are required to lock the GPU clocks",
                "droits administrateur requis pour verrouiller les horloges du GPU"
            )
            .into(),
        ),
        e => ClampError::Failed(e.to_string()),
    }
}

impl GpuWriter for NvmlWriter {
    /// Verrouille les horloges sur la plage de l'etat le plus repose que la carte declare.
    /// Rien n'est code en dur : P8 vaut 210-405 MHz sur une RTX 4080 SUPER, et une autre
    /// carte a ses propres valeurs, voire un autre etat de repos.
    fn clamp(&mut self) -> Result<(), ClampError> {
        let nvml = self
            .nvml()
            .map_err(|e| ClampError::Unsupported(e.to_string()))?;
        let mut gpu = nvml.device_by_index(0).map_err(classify)?;

        let deepest = gpu
            .supported_performance_states()
            .map_err(classify)?
            .into_iter()
            .filter(|p| *p != PerformanceState::Unknown)
            .max_by_key(|p| p.as_c())
            .ok_or_else(|| {
                ClampError::Unsupported(
                    crate::t!(
                        "the card declares no performance state",
                        "la carte ne declare aucun etat de performance"
                    )
                    .into(),
                )
            })?;

        let (min, max) = gpu
            .min_max_clock_of_pstate(Clock::Graphics, deepest)
            .map_err(classify)?;
        gpu.set_gpu_locked_clocks(GpuLockedClocksSetting::Numeric {
            min_clock_mhz: min,
            max_clock_mhz: max,
        })
        .map_err(classify)?;

        // La memoire ne se verrouille que depuis Ampere. Refusee, le verrou graphique
        // reste utile seul ; echouee autrement, on defait tout plutot que de laisser un
        // etat que personne n'a voulu.
        if let Ok((min, max)) = gpu.min_max_clock_of_pstate(Clock::Memory, deepest) {
            match gpu.set_mem_locked_clocks(min, max) {
                Ok(()) | Err(NvmlError::NotSupported) => {}
                Err(e) => {
                    let _ = gpu.reset_gpu_locked_clocks();
                    return Err(ClampError::Failed(e.to_string()));
                }
            }
        }
        Ok(())
    }

    fn release(&mut self) -> Result<(), String> {
        let nvml = self.nvml().map_err(|e| e.to_string())?;
        let mut gpu = nvml.device_by_index(0).map_err(|e| e.to_string())?;
        let ignore_unsupported = |r: Result<(), NvmlError>| match r {
            Ok(()) | Err(NvmlError::NotSupported) => Ok(()),
            Err(e) => Err(e.to_string()),
        };
        ignore_unsupported(gpu.reset_gpu_locked_clocks())?;
        ignore_unsupported(gpu.reset_mem_locked_clocks())
    }
}

/// La trace d'un verrou pose : un fichier, present tant qu'un verrou peut etre en place.
#[derive(Clone)]
pub struct ClampJournal {
    path: Option<PathBuf>,
}

impl ClampJournal {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            path: Some(dir.join(FILE)),
        }
    }

    /// Aucun endroit ou ecrire : le bridage se refusera plutot que de promettre.
    pub fn nowhere() -> Self {
        Self { path: None }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn pending(&self) -> bool {
        self.path.as_ref().is_some_and(|p| p.exists())
    }

    fn arm(&self) -> Result<(), String> {
        let path = self.path.as_ref().ok_or_else(|| {
            crate::t!(
                "configuration folder not found: a GPU lock would not survive a crash",
                "dossier de configuration introuvable : un verrou GPU ne survivrait pas a un plantage"
            )
            .to_string()
        })?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(path, "{\"locked\":true}").map_err(|e| e.to_string())
    }

    fn clear(&self) {
        if let Some(path) = self.path.as_ref() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Ce que l'interface affiche du bridage.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClampStatus {
    /// L'utilisateur l'a demande.
    pub enabled: bool,
    /// Un verrou est en place en ce moment.
    pub clamped: bool,
    /// Pourquoi il ne sera pas pose, tant que l'option n'est pas recochee.
    pub unsupported: Option<String>,
    /// Le dernier echec, s'il y en a eu un.
    pub error: Option<String>,
}

struct Inner<W> {
    writer: W,
    status: ClampStatus,
    streak: u32,
}

/// Le seul point par ou passe un verrou d'horloge.
pub struct GpuClamp<W: GpuWriter = NvmlWriter> {
    inner: Mutex<Inner<W>>,
    journal: ClampJournal,
}

impl<W: GpuWriter> GpuClamp<W> {
    pub fn new(writer: W, journal: ClampJournal, enabled: bool) -> Self {
        Self {
            inner: Mutex::new(Inner {
                writer,
                status: ClampStatus {
                    enabled,
                    ..ClampStatus::default()
                },
                streak: 0,
            }),
            journal,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<W>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> ClampStatus {
        self.lock().status.clone()
    }

    /// Solde un verrou laisse par un arret que l'application n'a pas vu venir. A appeler
    /// au lancement, avant toute mesure.
    pub fn restore_at_startup(&self) {
        if !self.journal.pending() {
            return;
        }
        let mut inner = self.lock();
        match inner.writer.release() {
            Ok(()) => self.journal.clear(),
            Err(e) => inner.status.error = Some(e),
        }
    }

    /// Relache a la sortie. Le journal disparait avec le verrou, pas avant.
    pub fn shutdown(&self) {
        let mut inner = self.lock();
        if inner.status.clamped {
            self.release(&mut inner);
        }
    }

    /// Cocher efface un refus precedent : les droits ou le pilote ont pu changer depuis.
    /// Decocher relache sur-le-champ.
    pub fn set_enabled(&self, on: bool) -> ClampStatus {
        let mut inner = self.lock();
        inner.status.enabled = on;
        inner.streak = 0;
        if on {
            inner.status.unsupported = None;
            inner.status.error = None;
        } else if inner.status.clamped {
            self.release(&mut inner);
        }
        inner.status.clone()
    }

    /// Appele a chaque releve, sur le thread d'echantillonnage. Rend vrai quand l'etat
    /// affichable a change.
    pub fn on_reading(&self, r: &Reading) -> bool {
        let mut inner = self.lock();
        let before = inner.status.clone();

        let allowed = inner.status.enabled && inner.status.unsupported.is_none();
        let sign = assess(r, inner.status.clamped);

        if inner.status.clamped {
            if !allowed || sign != Sign::Waste {
                self.release(&mut inner);
            }
        } else if allowed && sign == Sign::Waste {
            inner.streak += 1;
            if inner.streak >= ENGAGE_AFTER {
                inner.streak = 0;
                self.engage(&mut inner);
            }
        } else {
            inner.streak = 0;
        }

        inner.status != before
    }

    fn engage(&self, inner: &mut Inner<W>) {
        // Le journal avant le verrou, jamais apres : entre les deux, le pire cas est un
        // relachement qui ne relache rien.
        if let Err(e) = self.journal.arm() {
            inner.status.unsupported = Some(e);
            return;
        }
        match inner.writer.clamp() {
            Ok(()) => {
                inner.status.clamped = true;
                inner.status.error = None;
            }
            Err(e) => {
                // Un echec de pose peut avoir laisse un verrou partiel : on le defait
                // avant d'effacer la trace.
                if inner.writer.release().is_ok() {
                    self.journal.clear();
                }
                match e {
                    ClampError::Unsupported(why) => inner.status.unsupported = Some(why),
                    ClampError::Failed(why) => inner.status.error = Some(why),
                }
            }
        }
    }

    fn release(&self, inner: &mut Inner<W>) {
        match inner.writer.release() {
            Ok(()) => {
                inner.status.clamped = false;
                self.journal.clear();
            }
            // Le verrou est peut-etre toujours la : on reste bride aux yeux de la
            // boucle, qui retentera au releve suivant, et le journal reste pour le
            // lancement d'apres.
            Err(e) => inner.status.error = Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    struct Log {
        clamps: u32,
        releases: u32,
        locked: bool,
        clamp_fails: Option<ClampError>,
        release_fails: bool,
        journal_seen_at_clamp: Option<bool>,
    }

    /// Carte simulee. Le journal est observe au moment du verrou : c'est l'ordre des
    /// deux gestes qui fait la garantie.
    struct FakeWriter {
        log: Arc<Mutex<Log>>,
        journal: Option<PathBuf>,
    }

    impl GpuWriter for FakeWriter {
        fn clamp(&mut self) -> Result<(), ClampError> {
            let mut log = self.log.lock().unwrap();
            log.journal_seen_at_clamp = Some(self.journal.as_ref().is_some_and(|p| p.exists()));
            if let Some(e) = log.clamp_fails.clone() {
                return Err(e);
            }
            log.clamps += 1;
            log.locked = true;
            Ok(())
        }

        fn release(&mut self) -> Result<(), String> {
            let mut log = self.log.lock().unwrap();
            if log.release_fails {
                return Err("NVML a echoue".into());
            }
            log.releases += 1;
            log.locked = false;
            Ok(())
        }
    }

    fn temp_path() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "thermal-lab-gpu-clamp-{}-{}.json",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&path);
        path
    }

    fn setup(enabled: bool) -> (GpuClamp<FakeWriter>, Arc<Mutex<Log>>, PathBuf) {
        let path = temp_path();
        let log = Arc::new(Mutex::new(Log::default()));
        let writer = FakeWriter {
            log: Arc::clone(&log),
            journal: Some(path.clone()),
        };
        let clamp = GpuClamp::new(writer, ClampJournal::at(path.clone()), enabled);
        (clamp, log, path)
    }

    /// Un releve de la carte de contrepoint : P3, sans ecran, pilote inoccupe.
    fn reading(util: f64, pstate: f64, display: f64, driver_idle: f64) -> Reading {
        let mut r = Reading::default();
        r.offer(Metric::GpuUtilPct, util, "test");
        r.offer(Metric::GpuPerfStateIndex, pstate, "test");
        r.offer(Metric::GpuDisplayActive, display, "test");
        r.offer(Metric::GpuDriverIdle, driver_idle, "test");
        r
    }

    fn wasting() -> Reading {
        reading(0.0, 3.0, 0.0, 1.0)
    }

    fn feed(clamp: &GpuClamp<FakeWriter>, r: &Reading, n: u32) {
        for _ in 0..n {
            clamp.on_reading(r);
        }
    }

    #[test]
    fn bride_apres_plusieurs_releves_concordants() {
        let (clamp, log, _) = setup(true);

        feed(&clamp, &wasting(), ENGAGE_AFTER - 1);
        assert_eq!(log.lock().unwrap().clamps, 0, "trop tot");

        clamp.on_reading(&wasting());
        assert_eq!(log.lock().unwrap().clamps, 1);
        assert!(clamp.status().clamped);
    }

    #[test]
    fn un_releve_different_remet_le_compte_a_zero() {
        let (clamp, log, _) = setup(true);

        feed(&clamp, &wasting(), ENGAGE_AFTER - 1);
        clamp.on_reading(&reading(40.0, 0.0, 0.0, 0.0));
        feed(&clamp, &wasting(), ENGAGE_AFTER - 1);

        assert_eq!(log.lock().unwrap().clamps, 0);
    }

    #[test]
    fn relache_au_premier_signe_de_travail() {
        let (clamp, log, _) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);

        // Bridee, la carte redescend : son etat de performance ne compte plus.
        clamp.on_reading(&reading(0.0, 8.0, 0.0, 1.0));
        assert!(clamp.status().clamped, "relache sur son propre effet");

        clamp.on_reading(&reading(35.0, 8.0, 0.0, 1.0));
        assert!(!clamp.status().clamped);
        assert_eq!(log.lock().unwrap().releases, 1);
    }

    /// Le decodage video occupe la carte sans passer par `utilization.gpu`.
    #[test]
    fn une_video_empeche_et_leve_le_bridage() {
        let (clamp, _, _) = setup(true);
        let mut video = wasting();
        video.offer(Metric::GpuDecodeUtilPct, 30.0, "test");

        feed(&clamp, &video, ENGAGE_AFTER * 2);
        assert!(!clamp.status().clamped);

        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert!(clamp.status().clamped);
        clamp.on_reading(&video);
        assert!(!clamp.status().clamped);
    }

    #[test]
    fn relache_quand_le_pilote_reprend_la_carte() {
        let (clamp, _, _) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);

        clamp.on_reading(&reading(0.0, 0.0, 0.0, 0.0));
        assert!(!clamp.status().clamped);
    }

    /// Une carte qui affiche n'est jamais touchee : c'est le cas d'une tour.
    #[test]
    fn ne_touche_jamais_une_carte_qui_affiche() {
        let (clamp, log, _) = setup(true);

        feed(&clamp, &reading(0.0, 0.0, 1.0, 1.0), ENGAGE_AFTER * 3);
        assert_eq!(log.lock().unwrap().clamps, 0);
    }

    #[test]
    fn ne_touche_pas_une_carte_deja_au_repos() {
        let (clamp, log, _) = setup(true);

        feed(&clamp, &reading(0.0, 8.0, 0.0, 1.0), ENGAGE_AFTER * 3);
        assert_eq!(log.lock().unwrap().clamps, 0);
    }

    /// Une grandeur perdue en cours de route : dans le doute, on relache.
    #[test]
    fn une_mesure_perdue_relache() {
        let (clamp, _, _) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);

        clamp.on_reading(&Reading::default());
        assert!(!clamp.status().clamped);
    }

    #[test]
    fn desactive_ne_bride_rien() {
        let (clamp, log, _) = setup(false);

        feed(&clamp, &wasting(), ENGAGE_AFTER * 3);
        assert_eq!(log.lock().unwrap().clamps, 0);
    }

    #[test]
    fn decocher_relache_sur_le_champ() {
        let (clamp, log, path) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);

        let status = clamp.set_enabled(false);
        assert!(!status.clamped);
        assert!(!log.lock().unwrap().locked);
        assert!(!path.exists());
    }

    // --- Le journal ---

    #[test]
    fn le_journal_precede_le_verrou_et_le_suit_de_pres() {
        let (clamp, log, path) = setup(true);

        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert_eq!(log.lock().unwrap().journal_seen_at_clamp, Some(true));
        assert!(path.exists(), "verrou pose sans trace");

        clamp.on_reading(&reading(50.0, 0.0, 0.0, 0.0));
        assert!(!path.exists(), "trace restee apres relachement");
    }

    #[test]
    fn la_sortie_relache() {
        let (clamp, log, path) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);

        clamp.shutdown();
        assert!(!log.lock().unwrap().locked);
        assert!(!path.exists());
    }

    /// La mort brutale : le verrou reste, la trace aussi, le lancement suivant relache.
    #[test]
    fn le_lancement_suivant_solde_un_verrou_orphelin() {
        let (clamp, _, path) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);
        drop(clamp);
        assert!(path.exists());

        let log = Arc::new(Mutex::new(Log {
            locked: true,
            ..Log::default()
        }));
        let writer = FakeWriter {
            log: Arc::clone(&log),
            journal: Some(path.clone()),
        };
        let next = GpuClamp::new(writer, ClampJournal::at(path.clone()), false);
        next.restore_at_startup();

        assert!(!log.lock().unwrap().locked);
        assert!(!path.exists());
    }

    #[test]
    fn un_lancement_sans_trace_ne_touche_a_rien() {
        let (clamp, log, _) = setup(true);
        clamp.restore_at_startup();
        assert_eq!(log.lock().unwrap().releases, 0);
    }

    #[test]
    fn un_relachement_rate_garde_la_trace_et_reessaie() {
        let (clamp, log, path) = setup(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);
        log.lock().unwrap().release_fails = true;

        clamp.on_reading(&reading(50.0, 0.0, 0.0, 0.0));
        assert!(clamp.status().clamped);
        assert!(clamp.status().error.is_some());
        assert!(path.exists());

        log.lock().unwrap().release_fails = false;
        clamp.on_reading(&reading(50.0, 0.0, 0.0, 0.0));
        assert!(!clamp.status().clamped);
        assert!(!path.exists());
    }

    #[test]
    fn sans_journal_possible_pas_de_verrou() {
        let log = Arc::new(Mutex::new(Log::default()));
        let writer = FakeWriter {
            log: Arc::clone(&log),
            journal: None,
        };
        let clamp = GpuClamp::new(writer, ClampJournal::nowhere(), true);

        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert_eq!(log.lock().unwrap().clamps, 0);
        assert!(clamp.status().unsupported.is_some());
    }

    // --- Les refus ---

    #[test]
    fn un_refus_de_la_carte_arrete_les_tentatives() {
        let (clamp, log, path) = setup(true);
        log.lock().unwrap().clamp_fails = Some(ClampError::Unsupported("non".into()));

        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert!(clamp.status().unsupported.is_some());
        assert!(!path.exists(), "trace d'un verrou jamais pose");

        log.lock().unwrap().clamp_fails = None;
        feed(&clamp, &wasting(), ENGAGE_AFTER * 3);
        assert_eq!(log.lock().unwrap().clamps, 0, "reessaie malgre le refus");

        // Recocher efface le refus : les droits ont pu changer.
        clamp.set_enabled(true);
        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert_eq!(log.lock().unwrap().clamps, 1);
    }

    #[test]
    fn un_echec_passager_laisse_reessayer() {
        let (clamp, log, _) = setup(true);
        log.lock().unwrap().clamp_fails = Some(ClampError::Failed("GpuLost".into()));

        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert!(clamp.status().error.is_some());
        assert!(clamp.status().unsupported.is_none());

        log.lock().unwrap().clamp_fails = None;
        feed(&clamp, &wasting(), ENGAGE_AFTER);
        assert!(clamp.status().clamped);
        assert!(clamp.status().error.is_none());
    }

    /// **Ecrit dans le pilote.** Verrouille la vraie carte, relit ses horloges, relache.
    /// A lancer dans une console elevee, aucun jeu en cours :
    /// `cargo test verrouille_et_relache_la_vraie_carte -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn verrouille_et_relache_la_vraie_carte() {
        use nvml_wrapper::enum_wrappers::device::Clock;
        let read = || {
            let nvml = Nvml::init().expect("NVML");
            let gpu = nvml.device_by_index(0).expect("carte 0");
            (
                gpu.clock_info(Clock::SM).ok(),
                gpu.clock_info(Clock::Memory).ok(),
                gpu.power_usage().ok(),
                gpu.performance_state().ok(),
            )
        };
        println!("avant   : {:?}", read());

        let mut writer = NvmlWriter::default();
        let clamped = writer.clamp();
        println!("verrou  : {clamped:?}");
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!("bridee  : {:?}", read());

        writer.release().expect("relachement");
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!("relachee: {:?}", read());
        clamped.expect("verrou refuse");
    }

    #[test]
    fn signale_un_changement_d_etat_et_seulement_lui() {
        let (clamp, _, _) = setup(true);
        assert!(!clamp.on_reading(&wasting()));
        assert!(!clamp.on_reading(&wasting()));
        assert!(
            clamp.on_reading(&wasting()),
            "le verrou pose doit se signaler"
        );
        assert!(!clamp.on_reading(&wasting()));
    }
}
