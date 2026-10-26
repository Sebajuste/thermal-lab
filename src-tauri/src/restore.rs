//! La garantie d'arret : la machine repart comme elle etait avant qu'on y touche.
//!
//! Le bridage est un reglage de Windows, pas un mode que l'application tiendrait ouvert.
//! Rien ne le defait tout seul : sans filet, une application fermee — ou tuee — laisse
//! un processeur plafonne a 99 % et personne pour dire pourquoi.
//!
//! Le filet est un journal sur le disque. Avant la premiere ecriture, les valeurs
//! d'origine du schema y sont posees ; elles n'en sortent qu'une fois rendues. Ce qui
//! couvre les trois familles d'arret :
//!
//!   - sortie normale — menu de l'icone, redemarrage de mise a jour : `restore()` est
//!     appele par la boucle d'evenements, et purge le journal ;
//!   - arret que nous ne voyons pas venir — fin de session Windows, arret de tache,
//!     plantage : le journal survit, et le lancement suivant restaure avant tout ;
//!   - echec de la restauration elle-meme : le journal reste, la dette aussi.
//!
//! La reference est *l'etat d'avant*, pas les valeurs par defaut de Windows : une
//! machine deja bridee par un outil tiers doit retrouver son bridage. Les defauts ne
//! servent que lorsque la reference est perdue.
//!
//! Corollaire assume : sans journal possible, pas de bridage. Une promesse de
//! restauration qu'on ne peut pas tenir vaut moins que l'interrupteur qu'elle protege.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::power::{self, PowerState, Targets, WINDOWS_DEFAULTS};
use crate::profiles::Profile;

const FILE: &str = "restore.json";

/// L'etat du schema avant notre intervention. Le GUID en fait partie : c'est ce
/// schema-la qu'il faudra remettre en etat, et non celui qui sera actif au moment de
/// l'arret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub scheme_guid: String,
    pub boost_mode: u32,
    pub throttle_max: u32,
    /// Absents d'un journal ecrit avant que ces leviers existent : la version qui l'a
    /// ecrit ne les avait pas touches, il n'y a donc rien a rendre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epp: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throttle_max_1: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epp_1: Option<u32>,
}

impl Baseline {
    fn of(state: &PowerState) -> Self {
        Self::with(state.scheme_guid.clone(), state.targets())
    }

    /// Complete une reference ecrite par une version qui ignorait certains leviers.
    ///
    /// Un levier absent du journal n'a jamais ete touche par la version qui l'a ecrit :
    /// sa valeur actuelle est donc sa valeur d'origine, et elle doit etre retenue avant
    /// que celle-ci le modifie. Seulement sur le schema de la reference : les valeurs d'un
    /// autre schema ne diraient rien de celui qu'il faudra rendre.
    fn completed(&self, state: &PowerState) -> Self {
        if state.scheme_guid != self.scheme_guid {
            return self.clone();
        }
        Self::with(self.scheme_guid.clone(), self.targets().or(state.targets()))
    }

    fn with(scheme_guid: String, t: Targets) -> Self {
        Self {
            scheme_guid,
            boost_mode: t.boost_mode,
            throttle_max: t.throttle_max,
            epp: t.epp,
            throttle_max_1: t.throttle_max_1,
            epp_1: t.epp_1,
        }
    }

    fn targets(&self) -> Targets {
        Targets {
            boost_mode: self.boost_mode,
            throttle_max: self.throttle_max,
            epp: self.epp,
            throttle_max_1: self.throttle_max_1,
            epp_1: self.epp_1,
        }
    }
}

/// Ce que le journal a retenir au moment ou on le lit.
#[derive(Debug)]
pub enum Pending {
    /// Aucune intervention en cours : il n'y a rien a rendre.
    None,
    /// Intervention en cours, valeurs d'origine connues.
    Baseline(Baseline),
    /// Intervention en cours, valeurs d'origine perdues : le fichier est la mais ne se
    /// relit pas. Les defauts de Windows valent mieux qu'un bridage laisse en place.
    Unknown,
}

/// Le journal lui-meme : un fichier, ou rien du tout quand le dossier de configuration
/// est introuvable.
#[derive(Clone)]
pub struct Journal {
    path: Option<PathBuf>,
}

impl Journal {
    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub fn in_dir(dir: &Path) -> Self {
        Self::at(dir.join(FILE))
    }

    /// Le journal tel qu'il est sur le disque, pour le diagnostic.
    pub fn raw(&self) -> Option<String> {
        fs::read_to_string(self.path.as_ref()?).ok()
    }

    /// Aucun endroit ou ecrire. L'interrupteur se refusera plutot que de promettre.
    pub fn nowhere() -> Self {
        Self { path: None }
    }

    #[cfg(test)]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn pending(&self) -> Pending {
        let Some(path) = self.path.as_ref() else {
            return Pending::None;
        };
        match fs::read_to_string(path) {
            Err(_) => Pending::None,
            Ok(raw) => serde_json::from_str(&raw)
                .map(Pending::Baseline)
                .unwrap_or(Pending::Unknown),
        }
    }

    /// Ecrit la reference. Echouer ici interdit l'intervention : c'est le seul moment ou
    /// l'on peut encore renoncer sans avoir rien change sur la machine.
    fn arm(&self, baseline: &Baseline) -> Result<(), String> {
        let path = self.path.as_ref().ok_or_else(|| {
            crate::t!(
                "configuration folder not found: the cap would not survive a crash",
                "dossier de configuration introuvable : le bridage ne survivrait pas a un plantage"
            )
            .to_string()
        })?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| {
                let dir = dir.display();
                crate::t!(format!("{dir}: {e}"), format!("{dir} : {e}"))
            })?;
        }
        let raw = serde_json::to_string_pretty(baseline).map_err(|e| e.to_string())?;
        fs::write(path, raw).map_err(|e| {
            let path = path.display();
            crate::t!(format!("{path}: {e}"), format!("{path} : {e}"))
        })
    }

    /// Appele une fois la machine reellement rendue, jamais avant.
    fn clear(&self) {
        if let Some(path) = self.path.as_ref() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Le schema d'alimentation, vu comme deux gestes. L'indirection n'existe que pour les
/// tests : la garantie d'arret se verifie sur des scenarios entiers, qu'on ne peut pas
/// jouer contre le vrai `powercfg` de la machine qui compile.
pub trait PowerControl {
    fn state(&self) -> Result<PowerState, String>;
    fn write(&self, scheme_guid: &str, targets: Targets) -> Result<(), String>;
}

/// Le vrai schema, celui de Windows.
pub struct SystemPower;

impl PowerControl for SystemPower {
    fn state(&self) -> Result<PowerState, String> {
        power::state()
    }

    fn write(&self, scheme_guid: &str, targets: Targets) -> Result<(), String> {
        power::write_values(scheme_guid, targets)
    }
}

/// Le seul point par ou passe une modification du schema.
pub struct Restorer<P: PowerControl = SystemPower> {
    power: P,
    journal: Journal,
    /// Une bascule a la fois : `powercfg` prend plusieurs centaines de millisecondes, et
    /// le menu de l'icone comme le panneau peuvent la demander en meme temps.
    ops: Mutex<()>,
}

impl<P: PowerControl> Restorer<P> {
    pub fn new(power: P, journal: Journal) -> Self {
        Self {
            power,
            journal,
            ops: Mutex::new(()),
        }
    }

    /// Applique un profil. La reference est prise avant la premiere modification, et un
    /// changement de profil en cours de route ne la touche pas.
    pub fn engage(&self, profile: &Profile) -> Result<PowerState, String> {
        let _ops = self.lock();
        self.cap(profile.targets)
    }

    /// Rend la machine. Ne pose pas des valeurs « libres » au hasard : fait exactement
    /// ce que fait l'arret.
    pub fn release(&self) -> Result<PowerState, String> {
        let _ops = self.lock();
        self.release_inner()
    }

    /// Rend la machine si nous lui devons quelque chose. `Ok(None)` quand il n'y a rien
    /// a rendre — c'est le cas courant a l'arret comme au demarrage.
    ///
    /// Appele aux deux bouts de la vie du process : au demarrage, un journal present
    /// signe un arret qui n'a pas pu restaurer ; a la sortie, il porte notre propre
    /// intervention.
    pub fn restore(&self) -> Result<Option<PowerState>, String> {
        let _ops = self.lock();
        self.restore_inner()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.ops.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cap(&self, targets: Targets) -> Result<PowerState, String> {
        let state = self.power.state()?;
        if !state.elevated {
            return Err(crate::t!(
                "administrator rights are required to change the power scheme",
                "droits administrateur requis pour modifier le schema d'alimentation"
            )
            .to_string());
        }

        // Deja arme : la reference date de la premiere intervention et ne bouge plus.
        // La reprendre ici enregistrerait l'etat bride comme etat d'origine.
        let baseline = match self.journal.pending() {
            Pending::None => {
                let baseline = Baseline::of(&state);
                self.journal.arm(&baseline)?;
                Some(baseline)
            }
            Pending::Baseline(baseline) => {
                let completed = baseline.completed(&state);
                if completed != baseline {
                    self.journal.arm(&completed)?;
                }
                Some(completed)
            }
            Pending::Unknown => None,
        };

        // Un levier que le profil ne fixe pas revient a sa valeur d'origine. Sans cela,
        // passer d'un profil qui le regle a un profil qui l'ignore le laisserait en place,
        // et la machine ne ressemblerait plus a aucun des deux.
        let targets = match baseline {
            Some(b) => targets.or(b.targets()),
            None => targets,
        };

        // L'ordre compte : le journal est ecrit avant la premiere modification, jamais
        // apres. Entre les deux, le pire cas est une dette sans intervention — une
        // restauration qui ne change rien.
        self.power.write(&state.scheme_guid, targets)?;
        self.power.state()
    }

    fn release_inner(&self) -> Result<PowerState, String> {
        match self.restore_inner()? {
            Some(state) => Ok(state),
            // Rien d'arme : le bridage vient d'ailleurs. On pose les defauts de Windows,
            // seule reference dont on dispose.
            None => {
                let state = self.power.state()?;
                self.power.write(&state.scheme_guid, WINDOWS_DEFAULTS)?;
                self.power.state()
            }
        }
    }

    fn restore_inner(&self) -> Result<Option<PowerState>, String> {
        let (guid, targets) = match self.journal.pending() {
            Pending::None => return Ok(None),
            Pending::Baseline(b) => (b.scheme_guid.clone(), b.targets()),
            Pending::Unknown => (self.power.state()?.scheme_guid, WINDOWS_DEFAULTS),
        };

        // Le `?` laisse volontairement le journal en place : une restauration ratee reste
        // due, et sera retentee au prochain lancement.
        self.power.write(&guid, targets)?;
        self.journal.clear();
        self.power.state().map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{self, ProfileId};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    const SCHEME: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
    const OTHER: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

    /// Un chemin de journal a nous seul, dans le dossier temporaire du compte.
    fn temp_journal() -> Journal {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "thermal-lab-restore-{}-{}.json",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&path);
        Journal::at(path)
    }

    /// Schema d'alimentation simule : retient les ecritures, et n'applique que celles
    /// qui visent le schema actif — comme Windows.
    struct FakePower {
        state: Mutex<PowerState>,
        writes: Mutex<Vec<(String, u32, u32)>>,
        fails: Mutex<bool>,
    }

    impl FakePower {
        /// Les autres leviers ont leur valeur d'un schema Equilibre.
        fn new(boost_mode: u32, throttle_max: u32) -> Self {
            let targets = Targets {
                epp: Some(33),
                throttle_max_1: Some(100),
                epp_1: Some(33),
                ..Targets::cpu(boost_mode, throttle_max)
            };
            Self {
                state: Mutex::new(PowerState::new(
                    SCHEME.into(),
                    "Balanced".into(),
                    targets,
                    true,
                )),
                writes: Mutex::new(Vec::new()),
                fails: Mutex::new(false),
            }
        }

        fn without_elevation(self) -> Self {
            self.state.lock().unwrap().elevated = false;
            self
        }

        fn switch_active_scheme(&self, guid: &str) {
            self.state.lock().unwrap().scheme_guid = guid.into();
        }

        fn fails(&self, on: bool) {
            *self.fails.lock().unwrap() = on;
        }

        fn writes(&self) -> Vec<(String, u32, u32)> {
            self.writes.lock().unwrap().clone()
        }
    }

    impl PowerControl for FakePower {
        fn state(&self) -> Result<PowerState, String> {
            Ok(self.state.lock().unwrap().clone())
        }

        fn write(&self, scheme_guid: &str, targets: Targets) -> Result<(), String> {
            if *self.fails.lock().unwrap() {
                return Err("powercfg a echoue".into());
            }
            self.writes.lock().unwrap().push((
                scheme_guid.into(),
                targets.boost_mode,
                targets.throttle_max,
            ));
            let mut state = self.state.lock().unwrap();
            if state.scheme_guid == scheme_guid {
                // Comme powercfg : un levier absent de l'ecriture n'est pas touche.
                let targets = targets.or(state.targets());
                *state = PowerState::new(
                    state.scheme_guid.clone(),
                    state.scheme_name.clone(),
                    targets,
                    state.elevated,
                );
            }
            Ok(())
        }
    }

    fn restorer(power: FakePower) -> Restorer<FakePower> {
        Restorer::new(power, temp_journal())
    }

    fn capped() -> &'static Profile {
        profiles::get(ProfileId::Capped)
    }

    fn aggressive() -> &'static Profile {
        profiles::get(ProfileId::Aggressive)
    }

    fn epp(power: &FakePower) -> Option<u32> {
        power.state().unwrap().epp
    }

    // --- Les profils ---

    #[test]
    fn un_profil_pose_tous_ses_leviers_et_l_arret_les_rend() {
        let r = restorer(FakePower::new(2, 100));

        let state = r.engage(aggressive()).expect("profil applique");
        assert_eq!(
            (state.boost_mode, state.throttle_max, state.epp),
            (0, 80, Some(60))
        );
        assert_eq!(state.profile, Some(ProfileId::Aggressive));

        let state = r.restore().expect("restauration").expect("etat rendu");
        assert_eq!(
            (state.boost_mode, state.throttle_max, state.epp),
            (2, 100, Some(33))
        );
    }

    /// Le cas reel qui a revele le defaut : un journal arme par la version qui ne
    /// connaissait pas les coeurs P, puis le profil agressif reapplique par la version
    /// corrigee. Les coeurs P sont plafonnes pour la premiere fois ; leur valeur d'origine
    /// doit entrer au journal avant, sans quoi l'arret les laisserait plafonnes.
    #[test]
    fn un_journal_ancien_est_complete_avant_de_toucher_un_nouveau_levier() {
        let journal = temp_journal();
        std::fs::write(
            journal.path().unwrap(),
            format!(r#"{{"schemeGuid":"{SCHEME}","boostMode":2,"throttleMax":100,"epp":33}}"#),
        )
        .unwrap();
        // La machine telle que la premiere version l'a laissee : coeurs E seuls bride.
        let power = FakePower::new(0, 80);
        let half = Targets {
            epp: Some(60),
            ..Targets::cpu(0, 80)
        };
        power.write(SCHEME, half).unwrap();
        let r = Restorer::new(power, journal);

        let state = r.engage(aggressive()).expect("profil reapplique");
        assert_eq!((state.throttle_max_1, state.epp_1), (Some(80), Some(60)));
        match r.journal.pending() {
            Pending::Baseline(b) => {
                assert_eq!((b.throttle_max_1, b.epp_1), (Some(100), Some(33)));
                assert_eq!(b.epp, Some(33), "une valeur deja retenue ne bouge pas");
            }
            other => panic!("journal perdu : {other:?}"),
        }

        let state = r.restore().expect("restauration").expect("etat rendu");
        assert_eq!((state.throttle_max, state.epp), (100, Some(33)));
        assert_eq!((state.throttle_max_1, state.epp_1), (Some(100), Some(33)));
    }

    /// Une reference ne se complete pas avec les valeurs d'un autre schema.
    #[test]
    fn une_reference_ne_se_complete_pas_depuis_un_autre_schema() {
        let journal = temp_journal();
        std::fs::write(
            journal.path().unwrap(),
            format!(r#"{{"schemeGuid":"{OTHER}","boostMode":2,"throttleMax":100}}"#),
        )
        .unwrap();
        let r = Restorer::new(FakePower::new(2, 100), journal);

        r.engage(aggressive()).expect("profil applique");
        match r.journal.pending() {
            Pending::Baseline(b) => {
                assert_eq!(b.scheme_guid, OTHER);
                assert_eq!((b.epp, b.throttle_max_1, b.epp_1), (None, None, None));
            }
            other => panic!("journal perdu : {other:?}"),
        }
    }

    /// Sur un processeur hybride, les coeurs P ont leurs propres reglages : le profil les
    /// pose, et l'arret les rend.
    #[test]
    fn le_profil_agressif_plafonne_aussi_les_coeurs_p() {
        let r = restorer(FakePower::new(2, 100));

        let state = r.engage(aggressive()).expect("profil applique");
        assert_eq!((state.throttle_max_1, state.epp_1), (Some(80), Some(60)));

        let state = r.restore().expect("restauration").expect("etat rendu");
        assert_eq!((state.throttle_max_1, state.epp_1), (Some(100), Some(33)));
    }

    /// Le levier qu'un profil ne fixe pas revient a sa valeur d'origine, et non a celle
    /// du profil precedent.
    #[test]
    fn changer_de_profil_rend_les_leviers_qu_il_ne_fixe_pas() {
        let r = restorer(FakePower::new(2, 100));

        r.engage(aggressive()).expect("agressif");
        let state = r.engage(capped()).expect("leger");

        assert_eq!(
            (state.boost_mode, state.throttle_max, state.epp),
            (0, 99, Some(33))
        );
        assert_eq!(state.profile, Some(ProfileId::Capped));
    }

    /// La reference reste celle d'avant le premier profil, quel que soit le nombre de
    /// changements ensuite.
    #[test]
    fn changer_de_profil_ne_reecrit_pas_la_reference() {
        let r = restorer(FakePower::new(2, 100));

        r.engage(capped()).expect("leger");
        r.engage(aggressive()).expect("agressif");
        r.engage(capped()).expect("leger encore");

        match r.journal.pending() {
            Pending::Baseline(b) => {
                assert_eq!((b.boost_mode, b.throttle_max, b.epp), (2, 100, Some(33)));
            }
            other => panic!("reference perdue : {other:?}"),
        }
        let state = r.restore().expect("restauration").expect("etat rendu");
        assert_eq!(epp(&r.power), Some(33));
        assert!(!state.optimized);
    }

    /// Un journal ecrit par une version sans EPP se relit, et sa restauration ne touche
    /// pas un levier que cette version n'avait pas modifie.
    #[test]
    fn un_journal_sans_epp_ne_touche_pas_l_epp() {
        let journal = temp_journal();
        std::fs::write(
            journal.path().unwrap(),
            format!(r#"{{"schemeGuid":"{SCHEME}","boostMode":2,"throttleMax":100}}"#),
        )
        .unwrap();
        // Des valeurs differentes de l'origine : si la restauration les ecrivait, on le
        // verrait.
        let power = FakePower::new(0, 99);
        let moved = Targets {
            epp: Some(70),
            throttle_max_1: Some(90),
            epp_1: Some(70),
            ..Targets::cpu(0, 99)
        };
        power.write(SCHEME, moved).unwrap();
        let r = Restorer::new(power, journal);

        let state = r.restore().expect("restauration").expect("etat rendu");
        assert_eq!((state.boost_mode, state.throttle_max), (2, 100));
        assert_eq!(
            (state.epp, state.throttle_max_1, state.epp_1),
            (Some(70), Some(90), Some(70))
        );
    }

    // --- La reference est prise avant l'intervention, jamais apres ---

    #[test]
    fn arme_le_journal_avant_de_brider() {
        let r = restorer(FakePower::new(2, 100));

        let state = r.engage(capped()).expect("bridage applique");

        assert!(state.optimized);
        assert_eq!(r.power.writes(), vec![(SCHEME.to_string(), 0, 99)]);
        match r.journal.pending() {
            Pending::Baseline(b) => {
                assert_eq!(b.scheme_guid, SCHEME);
                assert_eq!((b.boost_mode, b.throttle_max), (2, 100));
            }
            other => panic!("journal non arme : {other:?}"),
        }
    }

    #[test]
    fn n_ecrase_pas_la_reference_en_bridant_deux_fois() {
        let r = restorer(FakePower::new(2, 100));

        r.engage(capped()).expect("premier bridage");
        r.engage(capped()).expect("second bridage");

        match r.journal.pending() {
            Pending::Baseline(b) => assert_eq!((b.boost_mode, b.throttle_max), (2, 100)),
            other => panic!("reference perdue : {other:?}"),
        }
    }

    // --- L'arret restaure, quelle que soit sa cause ---

    #[test]
    fn restaure_les_valeurs_d_origine_a_l_arret() {
        let r = restorer(FakePower::new(2, 100));
        r.engage(capped()).expect("bridage applique");

        let state = r.restore().expect("restauration").expect("etat rendu");

        assert!(!state.optimized);
        assert_eq!((state.boost_mode, state.throttle_max), (2, 100));
        assert!(
            matches!(r.journal.pending(), Pending::None),
            "journal non purge"
        );
    }

    /// Le cas qui distingue « valeurs par defaut » de « valeurs d'avant » : une machine
    /// deja bridee par un outil tiers doit retrouver son bridage, pas nos defauts.
    #[test]
    fn restaure_un_etat_deja_bride_avant_l_intervention() {
        let r = restorer(FakePower::new(0, 99));

        r.engage(capped()).expect("bridage applique");
        let state = r.restore().expect("restauration").expect("etat rendu");

        assert_eq!((state.boost_mode, state.throttle_max), (0, 99));
    }

    /// Le schema actif peut changer depuis Windows entre le bridage et l'arret. C'est
    /// le schema que nous avons modifie qu'il faut remettre en etat, pas l'actif.
    #[test]
    fn restaure_le_schema_d_origine_meme_si_l_actif_a_change() {
        let r = restorer(FakePower::new(2, 100));
        r.engage(capped()).expect("bridage applique");

        r.power.switch_active_scheme(OTHER);
        r.restore().expect("restauration");

        assert_eq!(
            r.power.writes().last(),
            Some(&(SCHEME.to_string(), 2, 100)),
            "la restauration a vise le mauvais schema"
        );
    }

    #[test]
    fn un_arret_sans_intervention_ne_touche_a_rien() {
        let r = restorer(FakePower::new(2, 100));

        assert!(r.restore().expect("restauration").is_none());
        assert!(r.power.writes().is_empty());
    }

    /// La mort brutale — arret de session, plantage, fin de tache : plus aucun code a
    /// nous ne tourne. Le journal est sur le disque, et le lancement suivant restaure.
    #[test]
    fn le_journal_survit_a_la_mort_du_process() {
        let journal = temp_journal();
        let first = Restorer::new(FakePower::new(2, 100), journal.clone());
        first.engage(capped()).expect("bridage applique");
        drop(first); // aucune restauration : le process disparait

        let next = Restorer::new(FakePower::new(0, 99), journal);
        let state = next.restore().expect("restauration").expect("etat rendu");

        assert_eq!((state.boost_mode, state.throttle_max), (2, 100));
        assert!(matches!(next.journal.pending(), Pending::None));
    }

    /// Journal present mais illisible : la reference est perdue, l'intervention non.
    /// Les defauts de Windows valent mieux qu'un bridage laisse en place.
    #[test]
    fn un_journal_illisible_restaure_les_defauts_de_windows() {
        let journal = temp_journal();
        std::fs::write(journal.path().unwrap(), b"{ tronque").unwrap();
        let r = Restorer::new(FakePower::new(0, 99), journal);

        assert!(matches!(r.journal.pending(), Pending::Unknown));
        let state = r.restore().expect("restauration").expect("etat rendu");

        assert_eq!((state.boost_mode, state.throttle_max), (2, 100));
        assert!(matches!(r.journal.pending(), Pending::None));
    }

    // --- Un echec ne doit jamais effacer la dette ---

    #[test]
    fn un_bridage_qui_echoue_laisse_le_journal_arme() {
        let r = restorer(FakePower::new(2, 100));
        r.power.fails(true);

        assert!(r.engage(capped()).is_err());
        assert!(
            matches!(r.journal.pending(), Pending::Baseline(_)),
            "une ecriture partielle resterait sans dette"
        );
    }

    #[test]
    fn une_restauration_qui_echoue_garde_le_journal() {
        let r = restorer(FakePower::new(2, 100));
        r.engage(capped()).expect("bridage applique");
        r.power.fails(true);

        assert!(r.restore().is_err());
        assert!(matches!(r.journal.pending(), Pending::Baseline(_)));

        r.power.fails(false);
        r.restore().expect("seconde tentative");
        assert!(matches!(r.journal.pending(), Pending::None));
    }

    // --- Pas de journal, pas de bridage ---

    #[test]
    fn refuse_de_brider_sans_journal_possible() {
        let r = Restorer::new(FakePower::new(2, 100), Journal::nowhere());

        assert!(r.engage(capped()).is_err());
        assert!(
            r.power.writes().is_empty(),
            "bride sans filet de restauration"
        );
    }

    #[test]
    fn sans_elevation_rien_n_est_arme_ni_ecrit() {
        let r = restorer(FakePower::new(2, 100).without_elevation());

        assert!(r.engage(capped()).is_err());
        assert!(r.power.writes().is_empty());
        assert!(matches!(r.journal.pending(), Pending::None));
    }

    /// L'etat relu dit quel profil est applique : c'est ce que l'interface affiche, et
    /// il vient de la machine, pas d'un souvenir de l'application.
    #[test]
    fn l_etat_relu_nomme_le_profil_applique() {
        let r = restorer(FakePower::new(2, 100));
        assert_eq!(r.power.state().unwrap().profile, None);

        let state = r.engage(capped()).expect("bridage applique");
        assert_eq!(state.profile, Some(ProfileId::Capped));

        let state = r.release().expect("relachement");
        assert_eq!(state.profile, None);
    }

    /// Un bridage pose par un autre outil n'est pas un de nos profils : il reste bride,
    /// mais sans nom.
    #[test]
    fn un_bridage_tiers_n_a_pas_de_profil() {
        let state = FakePower::new(2, 80).state().unwrap();
        assert!(state.optimized);
        assert_eq!(state.profile, None);
    }

    /// Relacher a la main ce qu'un autre outil a bride : rien n'est arme, on pose les
    /// defauts de Windows.
    #[test]
    fn relacher_sans_journal_pose_les_defauts() {
        let r = restorer(FakePower::new(0, 99));

        let state = r.release().expect("relachement");

        assert_eq!((state.boost_mode, state.throttle_max), (2, 100));
        assert_eq!(r.power.writes(), vec![(SCHEME.to_string(), 2, 100)]);
    }

    /// La demonstration, bout a bout : les deux facons de s'arreter, sur une meme
    /// machine simulee. `cargo test restore:: -- --nocapture` en donne la trace.
    #[test]
    fn les_arrets_rendent_la_meme_machine() {
        let origine = (2u32, 100u32);
        let etats = |power: &FakePower| {
            let s = power.state().unwrap();
            (s.boost_mode, s.throttle_max)
        };

        for (cause, tuer) in [
            ("sortie par le menu de l'icone", false),
            ("fin de session Windows ou plantage", true),
        ] {
            let journal = temp_journal();
            let power = FakePower::new(origine.0, origine.1);

            // Le lancement : rien a rendre, la machine est intacte.
            let app = Restorer::new(power, journal.clone());
            assert!(app.restore().unwrap().is_none());
            assert_eq!(etats(&app.power), origine);

            // L'utilisateur bride.
            app.engage(capped()).unwrap();
            assert_eq!(etats(&app.power), (0, 99));
            println!("[{cause}] bride    : {:?}", etats(&app.power));

            // L'arret.
            let apres = if tuer {
                // Personne ne previent : le process disparait, et c'est le lancement
                // suivant qui trouve le journal.
                let survivant = FakePower::new(0, 99);
                drop(app);
                let relance = Restorer::new(survivant, journal.clone());
                relance.restore().unwrap();
                let vu = etats(&relance.power);
                assert!(matches!(relance.journal.pending(), Pending::None));
                vu
            } else {
                app.restore().unwrap();
                let vu = etats(&app.power);
                assert!(matches!(app.journal.pending(), Pending::None));
                vu
            };

            println!("[{cause}] restaure : {apres:?}");
            assert_eq!(apres, origine, "{cause} n'a pas rendu la machine");
        }
    }

    #[test]
    fn le_journal_se_relit_tel_qu_il_a_ete_ecrit() {
        let raw = serde_json::to_string(&Baseline {
            scheme_guid: SCHEME.into(),
            boost_mode: 2,
            throttle_max: 100,
            epp: Some(33),
            throttle_max_1: Some(100),
            epp_1: Some(50),
        })
        .expect("serialisable");

        assert!(raw.contains("schemeGuid"), "{raw}");
        let back: Baseline = serde_json::from_str(&raw).expect("relisible");
        assert_eq!(back.scheme_guid, SCHEME);
        assert_eq!(
            (back.boost_mode, back.throttle_max, back.epp),
            (2, 100, Some(33))
        );
    }
}
