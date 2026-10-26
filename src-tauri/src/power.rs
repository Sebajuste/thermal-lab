//! Pilotage du bridage CPU via le schema d'alimentation actif de Windows.
//!
//! Le levier est exactement celui qu'utilisent les "optimiseurs" du commerce, et il est
//! entierement natif :
//!   - PERFBOOSTMODE   = 0  -> Turbo Boost desactive
//!   - PROCTHROTTLEMAX = 99 -> plafond a 99 % du nominal, ce qui interdit le turbo aussi
//!
//! Sur un Intel, le second suffit ; on pose les deux, comme le fait Windows lui-meme, pour
//! rester coherent quel que soit le pilote de performance (legacy ou Intel Speed Shift).
//!
//! Un troisieme levier, PERFEPP, oriente Speed Shift vers la performance ou l'economie.
//! Quelles valeurs poser, et quand, est l'affaire de `profiles` : ce module ne fait que
//! lire et ecrire.
//!
//! Lecture par le registre, car `powercfg /query` n'affiche rien pour ces reglages quand
//! leur attribut est masque — sauf PERFEPP, lue par `powercfg /qh`, voir `read_epp`.
//! Ecriture par `powercfg`, qui gere la propagation au systeme.

use serde::{Deserialize, Serialize};

use crate::profiles::{self, ProfileId};
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const SCHEMES_PATH: &str = r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes";
const SUB_PROCESSOR: &str = "54533251-82be-4824-96c1-47b60b740d00";
const PERFBOOSTMODE: &str = "be337238-0d82-4146-a960-4f3749d470c7";
const PROCTHROTTLEMAX: &str = "bc5038f7-23e0-4960-96da-33abaf5935ec";
/// Preference d'energie de Speed Shift, 0 a 100 : plus haut, plus econome. Ignoree par
/// un processeur sans HWP, mais ecrite quand meme — le schema la garde.
const PERFEPP: &str = "36687f9e-e3a5-4dbf-b1dc-15eb381c6863";

// Les memes, pour la classe d'efficacite 1 : les coeurs P d'un processeur hybride.
// Windows les regle a part — PROCTHROTTLEMAX et PERFEPP ne touchent alors que la classe 0,
// les coeurs E. Releve sur i9-14900K, plafond a 80 % pose sur la seule classe 0 : coeurs
// E a 59 %, coeurs P a 96 %. Sur un processeur homogene, ils existent et sont ignores.
const PROCTHROTTLEMAX1: &str = "bc5038f7-23e0-4960-96da-33abaf5935ed";
const PERFEPP1: &str = "36687f9e-e3a5-4dbf-b1dc-15eb381c6864";

/// Les leviers, par leur alias `powercfg` : c'est le nom que le diagnostic affiche.
const LEVERS: [(&str, &str); 5] = [
    ("PERFBOOSTMODE", PERFBOOSTMODE),
    ("PROCTHROTTLEMAX", PROCTHROTTLEMAX),
    ("PERFEPP", PERFEPP),
    ("PROCTHROTTLEMAX1", PROCTHROTTLEMAX1),
    ("PERFEPP1", PERFEPP1),
];

/// Ou une strategie de groupe depose ses reglages d'alimentation. Une valeur presente
/// ici s'applique quoi que dise le schema, et `powercfg` ne la change pas.
const POLICY_PATH: &str = r"SOFTWARE\Policies\Microsoft\Power\PowerSettings";

/// Les valeurs des leviers CPU, telles que le schema les porte.
///
/// Les leviers optionnels n'ont pas la meme absence selon l'endroit : dans un etat relu,
/// la valeur n'a pas pu etre lue ; dans un profil, le profil ne fixe pas ce levier ; dans
/// une ecriture, il n'est pas touche.
///
/// Le suffixe `_1` designe la classe d'efficacite 1 — les coeurs P d'un processeur
/// hybride. Sans suffixe, la classe 0 : les coeurs E, ou tous les coeurs ailleurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    pub boost_mode: u32,
    pub throttle_max: u32,
    pub epp: Option<u32>,
    pub throttle_max_1: Option<u32>,
    pub epp_1: Option<u32>,
}

impl Targets {
    /// Les deux leviers historiques seuls, les autres non fixes.
    pub const fn cpu(boost_mode: u32, throttle_max: u32) -> Self {
        Self {
            boost_mode,
            throttle_max,
            epp: None,
            throttle_max_1: None,
            epp_1: None,
        }
    }

    /// Chaque levier optionnel absent d'ici est pris dans `fallback`.
    pub fn or(self, fallback: Targets) -> Targets {
        Targets {
            epp: self.epp.or(fallback.epp),
            throttle_max_1: self.throttle_max_1.or(fallback.throttle_max_1),
            epp_1: self.epp_1.or(fallback.epp_1),
            ..self
        }
    }

    /// Le turbo est interdit, ou une classe de coeurs est plafonnee sous son nominal.
    pub fn caps_turbo(self) -> bool {
        self.boost_mode == 0
            || self.throttle_max < 100
            || self.throttle_max_1.is_some_and(|t| t < 100)
    }
}

/// Valeurs par defaut de Windows quand la cle n'existe pas dans le schema.
///
/// Pas de defaut pour l'EPP : elle depend du schema — 33 pour Equilibre, 60 pour
/// Economie d'energie — et `powercfg /qh` la resout toute seule a la lecture. Le plafond
/// des coeurs P, lui, vaut 100 partout.
pub(crate) const WINDOWS_DEFAULTS: Targets = Targets {
    throttle_max_1: Some(100),
    ..Targets::cpu(2 /* aggressive */, 100)
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerState {
    pub scheme_guid: String,
    pub scheme_name: String,
    pub boost_mode: u32,
    pub throttle_max: u32,
    /// `None` quand la valeur n'a pas pu etre lue.
    pub epp: Option<u32>,
    pub throttle_max_1: Option<u32>,
    pub epp_1: Option<u32>,
    /// Vrai quand le turbo est interdit, ou qu'un de nos profils est applique. C'est
    /// l'etat de l'interrupteur principal, quel que soit ce qui bride.
    pub optimized: bool,
    /// Le profil connu dont les valeurs sont exactement celles-ci. `None` pour une machine
    /// rendue, et pour un bridage qu'aucun profil ne decrit — pose par un outil tiers.
    pub profile: Option<ProfileId>,
    pub elevated: bool,
    /// Les leviers qu'une strategie de groupe impose : ecrire dans le schema n'y change
    /// rien. Vide hors poste gere.
    pub policy_locked: Vec<&'static str>,
}

impl PowerState {
    /// Tout ce qui se deduit des valeurs est deduit ici, et nulle part ailleurs.
    pub fn new(scheme_guid: String, scheme_name: String, targets: Targets, elevated: bool) -> Self {
        let profile = profiles::identify(targets);
        Self {
            scheme_guid,
            scheme_name,
            boost_mode: targets.boost_mode,
            throttle_max: targets.throttle_max,
            epp: targets.epp,
            throttle_max_1: targets.throttle_max_1,
            epp_1: targets.epp_1,
            optimized: targets.caps_turbo() || profile.is_some(),
            profile,
            elevated,
            policy_locked: Vec::new(),
        }
    }

    pub fn targets(&self) -> Targets {
        Targets {
            boost_mode: self.boost_mode,
            throttle_max: self.throttle_max,
            epp: self.epp,
            throttle_max_1: self.throttle_max_1,
            epp_1: self.epp_1,
        }
    }
}

/// Extrait le premier GUID canonique d'une chaine, sans dependre de la langue de Windows.
fn extract_guid(text: &str) -> Option<String> {
    const SHAPE: [usize; 5] = [8, 4, 4, 4, 12];
    for token in text.split(|c: char| !(c.is_ascii_hexdigit() || c == '-')) {
        let parts: Vec<&str> = token.split('-').collect();
        if parts.len() != 5 {
            continue;
        }
        let ok = parts
            .iter()
            .zip(SHAPE.iter())
            .all(|(p, n)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()));
        if ok {
            return Some(token.to_ascii_lowercase());
        }
    }
    None
}

/// Nom affiche du schema : Windows le place entre parentheses en fin de ligne.
fn extract_name(text: &str) -> String {
    text.rsplit_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(name, _)| name.trim().to_string())
        .unwrap_or_else(|| crate::t!("(unnamed)", "(sans nom)").to_string())
}

fn powercfg(args: &[&str]) -> Result<String, String> {
    let out = Command::new("powercfg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| {
            crate::t!(
                format!("powercfg not found: {e}"),
                format!("powercfg introuvable : {e}")
            )
        })?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let std = String::from_utf8_lossy(&out.stdout);
        let msg = if err.trim().is_empty() { std } else { err };
        let (cmd, msg) = (args.join(" "), msg.trim());
        return Err(crate::t!(
            format!("powercfg {cmd} failed: {msg}"),
            format!("powercfg {cmd} a echoue : {msg}")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn active_scheme() -> Result<(String, String), String> {
    let out = powercfg(&["/getactivescheme"])?;
    let guid = extract_guid(&out).ok_or_else(|| {
        crate::t!(
            "active scheme GUID not found",
            "GUID du schema actif introuvable"
        )
    })?;
    Ok((guid, extract_name(&out)))
}

/// Les index secteur et batterie d'une cle, s'ils y sont.
fn registry_indices(path: &str) -> (Option<u32>, Option<u32>) {
    match RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(path, KEY_READ) {
        Ok(k) => (
            k.get_value::<u32, _>("ACSettingIndex").ok(),
            k.get_value::<u32, _>("DCSettingIndex").ok(),
        ),
        Err(_) => (None, None),
    }
}

/// Un levier tel que chaque source le voit. C'est ce qui explique qu'une machine ne
/// ressemble pas au profil qu'on vient de lui appliquer : une strategie qui impose sa
/// valeur, un logiciel qui reecrit derriere nous, une cle absente.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeverReading {
    pub name: &'static str,
    /// `powercfg /qh` : la valeur du schema, defauts resolus.
    pub scheme_ac: Option<u32>,
    pub scheme_dc: Option<u32>,
    /// La cle du schema dans le registre : absente tant que personne ne l'a ecrite.
    pub registry_ac: Option<u32>,
    pub registry_dc: Option<u32>,
    /// Une strategie de groupe : si elle est la, c'est elle qui s'applique.
    pub policy_ac: Option<u32>,
    pub policy_dc: Option<u32>,
}

impl LeverReading {
    /// Ce qui s'applique en secteur : la strategie, sinon le schema, sinon sa cle.
    pub fn effective(&self) -> Option<u32> {
        self.policy_ac.or(self.scheme_ac).or(self.registry_ac)
    }
}

fn read_lever(guid: &str, name: &'static str, setting: &str) -> LeverReading {
    let (scheme_ac, scheme_dc) = powercfg(&["/qh", guid, SUB_PROCESSOR, setting])
        .ok()
        .and_then(|out| indices(&out))
        .map_or((None, None), |(ac, dc)| (Some(ac), Some(dc)));
    let (registry_ac, registry_dc) = registry_indices(&format!(
        "{SCHEMES_PATH}\\{guid}\\{SUB_PROCESSOR}\\{setting}"
    ));
    let (policy_ac, policy_dc) = registry_indices(&format!("{POLICY_PATH}\\{setting}"));
    LeverReading {
        name,
        scheme_ac,
        scheme_dc,
        registry_ac,
        registry_dc,
        policy_ac,
        policy_dc,
    }
}

fn read_levers(guid: &str) -> Vec<LeverReading> {
    LEVERS
        .iter()
        .map(|(name, setting)| read_lever(guid, name, setting))
        .collect()
}

/// Tout ce que la machine dit de ses leviers, d'un coup. Plus lent que `state` : une
/// action de l'utilisateur, pas un sondage.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerDiagnosis {
    pub scheme_guid: String,
    pub scheme_name: String,
    pub elevated: bool,
    /// Le schema qu'une strategie impose comme actif, s'il y en a un.
    pub policy_scheme: Option<String>,
    pub levers: Vec<LeverReading>,
}

pub fn diagnose() -> Result<PowerDiagnosis, String> {
    let (guid, name) = active_scheme()?;
    let policy_scheme = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(POLICY_PATH, KEY_READ)
        .and_then(|k| k.get_value::<String, _>("ActivePowerScheme"))
        .ok();
    Ok(PowerDiagnosis {
        levers: read_levers(&guid),
        scheme_guid: guid,
        scheme_name: name,
        elevated: is_elevated(),
        policy_scheme,
    })
}

/// Elevation du process, lue sur le jeton.
///
/// Surtout pas un test d'ecriture sur `SCHEMES_PATH` : cette cle n'accorde Full Control
/// qu'a SYSTEM, les Administrateurs y sont en lecture seule. Un tel test repondrait non
/// meme dans un terminal eleve. L'ecriture passe de toute facon par `powercfg`, qui
/// utilise l'API power privilegiee et non le registre en direct.
pub(crate) fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Ce qu'il faut pour savoir si l'on peut ecrire : un schema lisible, et l'elevation.
/// Sans relire les leviers, qui coutent cinq appels a `powercfg`.
pub fn access() -> Result<bool, String> {
    active_scheme().map(|_| is_elevated())
}

/// Les index secteur et batterie d'une sortie de `powercfg /qh`.
///
/// La sortie est localisee, mais ses valeurs ne le sont pas : minimum, maximum,
/// increment pour un reglage a plage, puis index secteur et index batterie, tous en
/// `0x` sur huit chiffres. Les deux derniers sont ceux qu'on cherche.
fn indices(text: &str) -> Option<(u32, u32)> {
    let values: Vec<u32> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|t| t.strip_prefix("0x"))
        .filter(|h| h.len() == 8)
        .filter_map(|h| u32::from_str_radix(h, 16).ok())
        .collect();
    match values.as_slice() {
        [.., ac, dc] => Some((*ac, *dc)),
        _ => None,
    }
}

/// Les cinq leviers par la meme voie. `powercfg /qh` plutot que le registre : la cle du
/// schema est absente tant que personne ne l'a ecrite, la valeur vient alors de defauts
/// propres a chaque schema. Une strategie de groupe prime sur les deux — c'est elle qui
/// s'applique. Une vingtaine de millisecondes par levier ; le sous-groupe entier en une
/// fois en coute 800.
pub fn state() -> Result<PowerState, String> {
    let (guid, name) = active_scheme()?;
    let levers = read_levers(&guid);
    let value = |i: usize| levers[i].effective();
    let targets = Targets {
        boost_mode: value(0).unwrap_or(WINDOWS_DEFAULTS.boost_mode),
        throttle_max: value(1).unwrap_or(WINDOWS_DEFAULTS.throttle_max),
        epp: value(2),
        throttle_max_1: value(3),
        epp_1: value(4),
    };
    let mut state = PowerState::new(guid, name, targets, is_elevated());
    state.policy_locked = levers
        .iter()
        .filter(|l| l.policy_ac.is_some())
        .map(|l| l.name)
        .collect();
    Ok(state)
}

/// Pose des valeurs sur un schema donne, et les applique.
///
/// Le schema vise est passe en parametre plutot que relu : une restauration doit
/// remettre en etat le schema qu'on a modifie, meme si Windows en a active un autre
/// entre-temps.
///
/// On ecrit les valeurs secteur *et* batterie : sur une tour la seconde ne sert a rien,
/// mais laisser les deux coherentes evite un comportement different sur onduleur.
pub fn write_values(scheme_guid: &str, targets: Targets) -> Result<(), String> {
    if !is_elevated() {
        return Err(crate::t!(
            "administrator rights are required to change the power scheme",
            "droits administrateur requis pour modifier le schema d'alimentation"
        )
        .to_string());
    }

    let boost = targets.boost_mode.to_string();
    let throttle = targets.throttle_max.to_string();

    for (verb, value, setting) in [
        ("/setacvalueindex", &boost, PERFBOOSTMODE),
        ("/setdcvalueindex", &boost, PERFBOOSTMODE),
        ("/setacvalueindex", &throttle, PROCTHROTTLEMAX),
        ("/setdcvalueindex", &throttle, PROCTHROTTLEMAX),
    ] {
        powercfg(&[verb, scheme_guid, SUB_PROCESSOR, setting, value])?;
    }
    for (setting, value) in [
        (PERFEPP, targets.epp),
        (PROCTHROTTLEMAX1, targets.throttle_max_1),
        (PERFEPP1, targets.epp_1),
    ] {
        let Some(value) = value else { continue };
        let value = value.to_string();
        for verb in ["/setacvalueindex", "/setdcvalueindex"] {
            powercfg(&[verb, scheme_guid, SUB_PROCESSOR, setting, &value])?;
        }
    }

    // Sans /setactive, les valeurs sont ecrites mais pas appliquees au systeme.
    // Le schema reactive est l'actif, qui n'est pas forcement celui qu'on vient
    // d'ecrire : reactiver un autre schema changerait le reglage de la machine.
    let (active, _) = active_scheme()?;
    powercfg(&["/setactive", &active])?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_guid_from_localized_output() {
        let fr = "GUID du mode de gestion de l'alimentation : 4e2a2b94-6646-493e-9f10-64f712e088aa  (Usage normal)";
        assert_eq!(
            extract_guid(fr).as_deref(),
            Some("4e2a2b94-6646-493e-9f10-64f712e088aa")
        );
        assert_eq!(extract_name(fr), "Usage normal");
    }

    #[test]
    fn parses_guid_from_english_output() {
        let en = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)";
        assert_eq!(
            extract_guid(en).as_deref(),
            Some("381b4222-f694-41f0-9685-ff5bb260df2e")
        );
        assert_eq!(extract_name(en), "Balanced");
    }

    /// Sortie reelle de `powercfg /qh` sur le schema Equilibre : 33 sur secteur.
    #[test]
    fn reads_the_indices_from_localized_output() {
        let fr = "GUID du mode de gestion de l'alimentation : 381b4222-f694-41f0-9685-ff5bb260df2e  (Utilisation normale)
  GUID du sous-groupe : 54533251-82be-4824-96c1-47b60b740d00  (Gestion de l'alimentation du processeur)
    GUID du parametre d'alimentation : 36687f9e-e3a5-4dbf-b1dc-15eb381c6863
      Valeur minimale possible : 0x00000000
      Valeur maximale possible : 0x00000064
      Increment possible des parametres : 0x00000001
      Unites possibles des parametres :  %
    Index actuel du parametre de courant alternatif : 0x00000021
    Index actuel du parametre de courant continu : 0x00000032";
        assert_eq!(indices(fr), Some((33, 50)));

        let en = "    Current AC Power Setting Index: 0x0000003c
    Current DC Power Setting Index: 0x00000050";
        assert_eq!(indices(en), Some((60, 80)));
    }

    #[test]
    fn no_index_means_no_value() {
        assert_eq!(indices("Le parametre n'existe pas."), None);
        assert_eq!(indices("Index : 0x00000021"), None);
    }

    /// Lit la machine reelle, sans rien ecrire : `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn reads_the_real_scheme() {
        let s = state().expect("schema lisible");
        println!("{s:#?}");
        assert!(s.epp.is_some(), "EPP illisible sur cette machine");
        assert!(s.throttle_max_1.is_some(), "plafond de classe 1 illisible");
    }

    #[test]
    fn rejects_malformed_guid() {
        assert_eq!(extract_guid("pas de guid ici 1234-56"), None);
    }

    /// Une strategie de groupe s'applique quoi que dise le schema ; le schema, quoi que
    /// dise une cle absente ou perimee.
    #[test]
    fn a_policy_wins_over_the_scheme() {
        let lever = |scheme, registry, policy| LeverReading {
            name: "PROCTHROTTLEMAX",
            scheme_ac: scheme,
            scheme_dc: scheme,
            registry_ac: registry,
            registry_dc: registry,
            policy_ac: policy,
            policy_dc: policy,
        };
        assert_eq!(lever(Some(80), Some(80), Some(100)).effective(), Some(100));
        assert_eq!(lever(Some(80), None, None).effective(), Some(80));
        assert_eq!(lever(None, Some(90), None).effective(), Some(90));
        assert_eq!(lever(None, None, None).effective(), None);
    }

    /// Lit la machine reelle, sans rien ecrire.
    #[test]
    #[ignore]
    fn diagnoses_the_real_scheme() {
        println!("{:#?}", diagnose().expect("schema lisible"));
    }
}
