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
//! Lecture par le registre, car `powercfg /query` n'affiche rien pour ces reglages quand
//! leur attribut est masque. Ecriture par `powercfg`, qui gere la propagation au systeme.

use serde::Serialize;
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

/// Valeurs par defaut de Windows quand la cle n'existe pas dans le schema.
const DEFAULT_BOOST_MODE: u32 = 2; // aggressive
const DEFAULT_THROTTLE_MAX: u32 = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerState {
    pub scheme_guid: String,
    pub scheme_name: String,
    pub boost_mode: u32,
    pub throttle_max: u32,
    /// Vrai quand le turbo est effectivement interdit, par l'un ou l'autre des deux leviers.
    pub optimized: bool,
    pub elevated: bool,
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
        .unwrap_or_else(|| "(sans nom)".to_string())
}

fn powercfg(args: &[&str]) -> Result<String, String> {
    let out = Command::new("powercfg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powercfg introuvable : {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let std = String::from_utf8_lossy(&out.stdout);
        let msg = if err.trim().is_empty() { std } else { err };
        return Err(format!(
            "powercfg {} a echoue : {}",
            args.join(" "),
            msg.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn active_scheme() -> Result<(String, String), String> {
    let out = powercfg(&["/getactivescheme"])?;
    let guid = extract_guid(&out).ok_or("GUID du schema actif introuvable")?;
    Ok((guid, extract_name(&out)))
}

fn read_setting(guid: &str, setting: &str, default: u32) -> u32 {
    let path = format!("{SCHEMES_PATH}\\{guid}\\{SUB_PROCESSOR}\\{setting}");
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(path, KEY_READ)
        .and_then(|k| k.get_value::<u32, _>("ACSettingIndex"))
        .unwrap_or(default)
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

pub fn state() -> Result<PowerState, String> {
    let (guid, name) = active_scheme()?;
    let boost_mode = read_setting(&guid, PERFBOOSTMODE, DEFAULT_BOOST_MODE);
    let throttle_max = read_setting(&guid, PROCTHROTTLEMAX, DEFAULT_THROTTLE_MAX);

    Ok(PowerState {
        optimized: boost_mode == 0 || throttle_max < 100,
        scheme_guid: guid,
        scheme_name: name,
        boost_mode,
        throttle_max,
        elevated: is_elevated(),
    })
}

/// Active ou desactive le bridage sur le schema courant.
///
/// On ecrit les valeurs secteur *et* batterie : sur une tour la seconde ne sert a rien,
/// mais laisser les deux coherentes evite un comportement different sur onduleur.
pub fn set_optimized(on: bool) -> Result<PowerState, String> {
    if !is_elevated() {
        return Err(
            "droits administrateur requis pour modifier le schema d'alimentation".to_string(),
        );
    }

    let (guid, _) = active_scheme()?;
    let boost = if on { "0" } else { "2" };
    let throttle = if on { "99" } else { "100" };

    for (verb, value, setting) in [
        ("/setacvalueindex", boost, PERFBOOSTMODE),
        ("/setdcvalueindex", boost, PERFBOOSTMODE),
        ("/setacvalueindex", throttle, PROCTHROTTLEMAX),
        ("/setdcvalueindex", throttle, PROCTHROTTLEMAX),
    ] {
        powercfg(&[verb, &guid, SUB_PROCESSOR, setting, value])?;
    }

    // Sans /setactive, les valeurs sont ecrites mais pas appliquees au systeme.
    powercfg(&["/setactive", &guid])?;

    state()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_guid_from_localized_output() {
        let fr = "GUID du mode de gestion de l'alimentation : 4e2a2b94-6646-493e-9f10-64f712e088aa  (Camomile)";
        assert_eq!(
            extract_guid(fr).as_deref(),
            Some("4e2a2b94-6646-493e-9f10-64f712e088aa")
        );
        assert_eq!(extract_name(fr), "Camomile");
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

    #[test]
    fn rejects_malformed_guid() {
        assert_eq!(extract_guid("pas de guid ici 1234-56"), None);
    }
}
