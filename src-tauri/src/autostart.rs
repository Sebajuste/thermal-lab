//! Demarrage a l'ouverture de session, par tache planifiee.
//!
//! La voie evidente — une valeur sous `HKCU\...\CurrentVersion\Run` — lance le programme
//! avec le jeton filtre de la session, donc **sans elevation**, et Windows ne pose aucune
//! question UAC au logon : un programme qui la reclamerait depuis cette cle serait
//! simplement bloque. L'application demarrerait en lecture seule, mesurant tout et ne
//! pouvant rien basculer, ce qui est exactement l'inverse de ce qu'on attend d'un
//! demarrage automatique ici.
//!
//! Une tache planifiee declenchee au logon avec « executer avec les autorisations
//! maximales » n'a pas ce defaut : elle demarre elevee, silencieusement. C'est ce que
//! font Core Temp et HWiNFO pour leur propre demarrage automatique. Le prix est une
//! invite UAC **au moment ou on coche la case**, une fois, et non a chaque session.

use std::os::windows::process::CommandExt;
use std::process::Command;

use crate::power;
use crate::SILENT_FLAG;

mod win {
    pub use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    pub use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    pub use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
    pub use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    /// Garder la main sur le processus lance, pour lire son code de sortie.
    pub const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
    /// L'utilisateur a ferme l'invite UAC.
    pub const ERROR_CANCELLED: u32 = 1223;
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Nom de la tache, tel qu'il apparait dans le planificateur de taches de Windows.
const TASK: &str = "Thermal Lab";

/// Nom sous lequel l'implementation precedente ecrivait dans la cle `Run`. Conserve le
/// temps que les postes qui l'ont connue soient nettoyes.
const LEGACY_RUN_VALUE: &str = "thermal-lab";

/// Delai d'attente de `schtasks` lance par elevation : au-dela, l'invite UAC est restee
/// sans reponse et mieux vaut rendre la main que bloquer une commande Tauri.
const ELEVATED_TIMEOUT_MS: u32 = 120_000;

/// Vrai quand une tache existe **et** vise cet executable-ci.
///
/// Se contenter de son existence serait un piege : deplacer le binaire laisse la tache
/// pointer sur l'ancien emplacement, ou plus rien ne repond. La case resterait cochee en
/// annoncant un demarrage automatique qui echouerait en silence a la session suivante, et
/// la decocher pour la recocher demanderait deux gestes dont le premier detruit. Ainsi
/// posee, la question devient « cet executable-ci demarre-t-il avec Windows ? », a
/// laquelle un seul clic repond — `/F` remplace la tache homonyme.
pub fn is_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    query_xml().is_some_and(|xml| task_targets(&xml, &exe.display().to_string()))
}

/// Le planificateur range la commande et ses arguments dans deux elements distincts : le
/// chemin se trouve donc seul dans `<Command>`, et le reconnaitre ne demande pas
/// d'analyser le document.
fn task_targets(xml: &str, exe: &str) -> bool {
    xml.to_lowercase().contains(&exe.to_lowercase())
}

fn query_xml() -> Option<String> {
    let out = Command::new("schtasks")
        .args(["/Query", "/TN", TASK, "/XML"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn set(on: bool) -> Result<(), String> {
    let args = if on { create_args()? } else { delete_args() };
    run(&args)
}

/// Supprime la valeur laissee dans `Run` par l'implementation precedente. Deux
/// demarrages automatiques pour une application a instance unique ne produiraient rien
/// d'utile, seulement un lancement non eleve qui se referme aussitot.
pub fn clear_legacy() {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        KEY_SET_VALUE,
    ) {
        let _ = key.delete_value(LEGACY_RUN_VALUE);
    }
}

fn create_args() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("chemin de l'executable introuvable : {e}"))?
        .display()
        .to_string();

    // `/RU` avec `/IT` : la tache s'execute sous le compte courant, uniquement quand il
    // est ouvert. Sans `/IT`, `schtasks` reclamerait un mot de passe.
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(name)) => format!("{domain}\\{name}"),
        (_, Ok(name)) => name,
        _ => return Err("compte utilisateur courant indeterminable".into()),
    };

    Ok(vec![
        "/Create".into(),
        "/TN".into(),
        TASK.into(),
        "/TR".into(),
        format!("\"{exe}\" {SILENT_FLAG}"),
        "/SC".into(),
        "ONLOGON".into(),
        "/RL".into(),
        "HIGHEST".into(),
        "/RU".into(),
        user,
        "/IT".into(),
        "/F".into(),
    ])
}

fn delete_args() -> Vec<String> {
    vec!["/Delete".into(), "/TN".into(), TASK.into(), "/F".into()]
}

/// Creer ou supprimer une tache `HIGHEST` exige l'elevation. Quand nous l'avons deja,
/// autant appeler `schtasks` directement : c'est synchrone et sa sortie est lisible.
/// Sinon, Windows pose la question.
fn run(args: &[String]) -> Result<(), String> {
    if power::is_elevated() {
        run_direct(args)
    } else {
        run_elevated(args)
    }
}

fn run_direct(args: &[String]) -> Result<(), String> {
    let out = Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("schtasks introuvable : {e}"))?;

    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let std = String::from_utf8_lossy(&out.stdout);
    let message = if err.trim().is_empty() { std } else { err };
    Err(format!("schtasks a echoue : {}", message.trim()))
}

fn quote(arg: &str) -> String {
    if !arg.contains(' ') && !arg.contains('"') {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\\\""))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `ShellExecuteEx` plutot que `ShellExecute` : lui seul rend une poignee sur le
/// processus, donc un code de sortie. Sans cela on ne saurait pas distinguer une tache
/// creee d'une commande refusee.
fn run_elevated(args: &[String]) -> Result<(), String> {
    let params: Vec<String> = args.iter().map(|a| quote(a)).collect();

    let verb = wide("runas");
    let file = wide("schtasks.exe");
    let parameters = wide(&params.join(" "));

    let mut info: win::SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<win::SHELLEXECUTEINFOW>() as u32;
    info.fMask = win::SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = win::SW_HIDE;

    unsafe {
        if win::ShellExecuteExW(&mut info) == 0 {
            return Err(match win::GetLastError() {
                win::ERROR_CANCELLED => "élévation non accordée".to_string(),
                code => format!("élévation impossible (code {code})"),
            });
        }

        let mut status = 0u32;
        let waited = win::WaitForSingleObject(info.hProcess, ELEVATED_TIMEOUT_MS);
        let read = win::GetExitCodeProcess(info.hProcess, &mut status);
        win::CloseHandle(info.hProcess);

        if waited != win::WAIT_OBJECT_0 {
            return Err("schtasks n'a pas rendu la main".into());
        }
        if read == 0 {
            return Err("code de sortie de schtasks illisible".into());
        }
        if status != 0 {
            return Err(format!("schtasks a echoue (code {status})"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_what_needs_it() {
        assert_eq!(quote("/Create"), "/Create");
        assert_eq!(quote("Thermal Lab"), "\"Thermal Lab\"");
    }

    /// L'action de la tache est une ligne de commande dans une ligne de commande : le
    /// chemin de l'executable garde ses guillemets une fois l'argument lui-meme entoure.
    #[test]
    fn nests_the_quotes_of_the_task_action() {
        let action = "\"C:\\Program Files\\Thermal Lab\\thermal-lab.exe\" --silent";
        assert_eq!(
            quote(action),
            "\"\\\"C:\\Program Files\\Thermal Lab\\thermal-lab.exe\\\" --silent\""
        );
    }

    /// La tache doit demander l'elevation, viser l'ouverture de session, et demarrer
    /// l'application sans reclamer l'ecran.
    #[test]
    fn the_task_is_elevated_silent_and_bound_to_logon() {
        let args = create_args().expect("arguments constructibles");
        let joined = args.join(" ");
        assert!(joined.contains("/RL HIGHEST"), "{joined}");
        assert!(joined.contains("/SC ONLOGON"), "{joined}");
        assert!(joined.contains("/IT"), "{joined}");
        assert!(joined.contains(SILENT_FLAG), "{joined}");
        assert!(args.contains(&TASK.to_string()));
    }

    /// Echantillon reduit de ce que rend `schtasks /Query /XML` : la commande et ses
    /// arguments y sont separes.
    const SAMPLE: &str = r#"<Task version="1.4">
  <Principals><Principal id="Author"><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Actions Context="Author">
    <Exec>
      <Command>D:\Outils\Thermal Lab\thermal-lab.exe</Command>
      <Arguments>--silent</Arguments>
    </Exec>
  </Actions>
</Task>"#;

    #[test]
    fn recognises_the_executable_it_points_at() {
        assert!(task_targets(
            SAMPLE,
            r"D:\Outils\Thermal Lab\thermal-lab.exe"
        ));
    }

    /// Windows ne distingue pas la casse des chemins, et `schtasks` restitue celle de la
    /// creation : comparer strictement ferait croire a un deplacement inexistant.
    #[test]
    fn ignores_case() {
        assert!(task_targets(
            SAMPLE,
            r"d:\outils\thermal lab\THERMAL-LAB.EXE"
        ));
    }

    /// Le coeur du correctif : une tache laissee sur l'ancien emplacement ne compte pas
    /// comme un demarrage automatique de ce binaire.
    #[test]
    fn a_task_left_behind_elsewhere_does_not_count() {
        assert!(!task_targets(SAMPLE, r"C:\Ailleurs\thermal-lab.exe"));
    }

    #[test]
    fn deletion_does_not_prompt() {
        assert!(delete_args().contains(&"/F".to_string()));
    }
}
