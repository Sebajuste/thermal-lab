//! Etat des outils tiers dont dependent les fournisseurs externes, et lancement.
//!
//! Le modele de capacites savait dire qu'une source n'etait pas etablie ; il ne savait
//! pas pourquoi. « Core Temp ne tourne pas » couvre trois situations qui n'appellent pas
//! la meme action : l'outil n'est pas installe, il est installe mais arrete, ou il tourne
//! sans les droits qui chargent son pilote. Ce module tranche entre les deux premieres et
//! permet d'agir sur la deuxieme.
//!
//! Rien ici n'est deduit du fournisseur : un outil peut tourner sans que sa mesure soit
//! lisible — HWiNFO demarre avec sa memoire partagee desactivee, Core Temp lance sans
//! elevation ne charge pas son pilote. C'est le croisement des deux qui renseigne.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::RegKey;

mod win {
    pub use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    pub use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    pub use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    pub use windows_sys::Win32::UI::Shell::ShellExecuteW;
    pub use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
}

/// La recherche dans la base de desinstallation parcourt quelques centaines de cles :
/// assez peu pour etre imperceptible, assez pour ne pas la refaire toutes les dix
/// secondes. L'etat « en cours d'execution », lui, est toujours relu.
const INSTALL_CACHE: Duration = Duration::from_secs(30);

/// Ce qu'il faut savoir d'un outil pour le trouver, le reconnaitre et le lancer.
struct Tool {
    id: &'static str,
    name: &'static str,
    /// Executable principal, tel qu'il apparait dans la liste des processus.
    exe: &'static str,
    /// Variantes acceptees : 32/64 bits, ou le projet dont l'outil est issu.
    aliases: &'static [&'static str],
    /// Fragment de `DisplayName` a chercher dans la base de desinstallation. Absent
    /// pour un outil portable, qui n'y figure pas.
    needle: Option<&'static str>,
    /// Emplacements usuels, en dernier recours. `%VAR%` est developpe.
    paths: &'static [&'static str],
}

const TOOLS: &[Tool] = &[
    Tool {
        id: "core-temp",
        name: "Core Temp",
        exe: "Core Temp.exe",
        aliases: &[],
        needle: Some("Core Temp"),
        paths: &[
            r"%ProgramFiles%\Core Temp\Core Temp.exe",
            r"%ProgramFiles(x86)%\Core Temp\Core Temp.exe",
        ],
    },
    Tool {
        id: "hwinfo",
        name: "HWiNFO",
        exe: "HWiNFO64.exe",
        aliases: &["HWiNFO32.exe", "HWiNFO.exe"],
        needle: Some("HWiNFO"),
        paths: &[
            r"%ProgramFiles%\HWiNFO64\HWiNFO64.exe",
            r"%ProgramFiles(x86)%\HWiNFO64\HWiNFO64.exe",
        ],
    },
    Tool {
        // Distribue en archive : aucune entree de desinstallation, donc introuvable
        // s'il n'est ni a un emplacement usuel ni en cours d'execution. On le dit
        // plutot que de le supposer absent.
        id: "libre-hw",
        name: "LibreHardwareMonitor",
        exe: "LibreHardwareMonitor.exe",
        aliases: &["OpenHardwareMonitor.exe"],
        needle: None,
        paths: &[
            r"%ProgramFiles%\LibreHardwareMonitor\LibreHardwareMonitor.exe",
            r"%LOCALAPPDATA%\Programs\LibreHardwareMonitor\LibreHardwareMonitor.exe",
        ],
    },
];

/// Quel outil fait vivre quel fournisseur. Deux fournisseurs peuvent dependre du meme :
/// `amd-gpu` lit les capteurs que LibreHardwareMonitor publie.
const BY_PROVIDER: &[(&str, &str)] = &[
    ("core-temp", "core-temp"),
    ("hwinfo", "hwinfo"),
    ("libre-hw", "libre-hw"),
    ("amd-gpu", "libre-hw"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: &'static str,
    pub name: &'static str,
    /// Vrai quand l'executable a ete localise. Faux ne prouve pas l'absence pour un
    /// outil portable — voir `certain`.
    pub installed: bool,
    /// Faux quand l'outil peut exister sans que nous sachions le trouver : l'interface
    /// doit alors proposer le telechargement sans affirmer qu'il manque.
    pub certain: bool,
    pub running: bool,
    pub path: Option<String>,
    /// Le lancer n'a de sens que si on sait ou il est et qu'il ne tourne pas deja.
    /// Calcule ici plutot que dans l'interface : la regle ne doit exister qu'une fois.
    pub launchable: bool,
}

impl ToolStatus {
    fn new(t: &'static Tool, path: Option<PathBuf>, running: bool) -> Self {
        Self {
            id: t.id,
            name: t.name,
            installed: path.is_some(),
            certain: t.needle.is_some(),
            running,
            launchable: path.is_some() && !running,
            path: path.map(|p| p.display().to_string()),
        }
    }
}

fn tool(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

/// Developpe les `%VAR%` d'un chemin. Une variable inconnue laisse le chemin
/// impossible a resoudre, ce qui le fait simplement echouer au test d'existence.
fn expand(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                out.push_str(&std::env::var(name).unwrap_or_else(|_| format!("%{name}%")));
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Base de desinstallation, dans ses trois emplacements : 64 bits, 32 bits, utilisateur.
fn from_uninstall(needle: &str, exe: &str) -> Option<PathBuf> {
    const ROOTS: [(isize, &str); 3] = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];

    let needle = needle.to_lowercase();
    for (hive, path) in ROOTS {
        let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ) else {
            continue;
        };
        for name in root.enum_keys().flatten() {
            let Ok(key) = root.open_subkey_with_flags(&name, KEY_READ) else {
                continue;
            };
            let display: String = key.get_value("DisplayName").unwrap_or_default();
            if !display.to_lowercase().contains(&needle) {
                continue;
            }
            if let Ok(dir) = key.get_value::<String, _>("InstallLocation") {
                let candidate = Path::new(dir.trim()).join(exe);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
            // `DisplayIcon` porte souvent l'executable lui-meme, suivi d'un index.
            if let Ok(icon) = key.get_value::<String, _>("DisplayIcon") {
                let raw = icon.split(',').next().unwrap_or_default();
                let candidate = Path::new(raw.trim().trim_matches('"'));
                if candidate.is_file()
                    && candidate
                        .file_name()
                        .is_some_and(|f| f.eq_ignore_ascii_case(exe))
                {
                    return Some(candidate.to_path_buf());
                }
            }
        }
    }
    None
}

/// `App Paths` : ce que Windows consulte quand on tape le nom d'un programme.
fn from_app_paths(exe: &str) -> Option<PathBuf> {
    let key = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(k) = RegKey::predef(hive).open_subkey_with_flags(&key, KEY_READ) else {
            continue;
        };
        if let Ok(value) = k.get_value::<String, _>("") {
            let path = PathBuf::from(value.trim().trim_matches('"'));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn locate(t: &Tool) -> Option<PathBuf> {
    t.needle
        .and_then(|n| from_uninstall(n, t.exe))
        .or_else(|| from_app_paths(t.exe))
        .or_else(|| {
            t.paths
                .iter()
                .map(|p| PathBuf::from(expand(p)))
                .find(|p| p.is_file())
        })
}

fn wide_to_string(raw: &[u16]) -> String {
    let len = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..len])
}

/// Noms d'image des processus en cours, en minuscules.
///
/// Le nom suffit : on ne cherche pas a savoir ou tourne l'outil, seulement s'il tourne.
/// C'est aussi la seule information qu'un processus eleve laisse lire a une application
/// qui ne l'est pas.
fn running_images() -> Vec<String> {
    let mut names = Vec::new();
    unsafe {
        let snapshot = win::CreateToolhelp32Snapshot(win::TH32CS_SNAPPROCESS, 0);
        if snapshot == win::INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry: win::PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<win::PROCESSENTRY32W>() as u32;
        if win::Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                names.push(wide_to_string(&entry.szExeFile).to_lowercase());
                if win::Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        win::CloseHandle(snapshot);
    }
    names
}

fn is_running(t: &Tool, images: &[String]) -> bool {
    std::iter::once(t.exe)
        .chain(t.aliases.iter().copied())
        .any(|exe| {
            let exe = exe.to_lowercase();
            images.iter().any(|i| *i == exe)
        })
}

struct Cache {
    at: Instant,
    paths: BTreeMap<&'static str, Option<PathBuf>>,
}

fn installs() -> BTreeMap<&'static str, Option<PathBuf>> {
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

    let mut slot = match CACHE.lock() {
        Ok(slot) => slot,
        Err(_) => return TOOLS.iter().map(|t| (t.id, locate(t))).collect(),
    };

    let fresh = slot
        .as_ref()
        .is_some_and(|c| c.at.elapsed() < INSTALL_CACHE);
    if !fresh {
        *slot = Some(Cache {
            at: Instant::now(),
            paths: TOOLS.iter().map(|t| (t.id, locate(t))).collect(),
        });
    }
    slot.as_ref().map(|c| c.paths.clone()).unwrap_or_default()
}

/// Etat de l'outil dont depend chaque fournisseur externe, indexe par identifiant de
/// fournisseur.
pub fn statuses() -> BTreeMap<&'static str, ToolStatus> {
    let paths = installs();
    let images = running_images();

    BY_PROVIDER
        .iter()
        .filter_map(|(provider, tool_id)| {
            let t = tool(tool_id)?;
            let path = paths.get(tool_id).cloned().flatten();
            Some((*provider, ToolStatus::new(t, path, is_running(t, &images))))
        })
        .collect()
}

/// Ouvre une adresse dans le navigateur par defaut.
///
/// Le webview ne sait pas sortir de lui-meme : un `target="_blank"` n'y fait rien. Le
/// meme `ShellExecute` que pour les outils s'en charge, sans verbe : Windows consulte
/// l'association du protocole. Seuls `http` et `https` passent — une adresse est une
/// chaine venue de l'interface, et `ShellExecute` ouvrirait aussi bien un executable.
pub fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("adresse refusée : {url}"));
    }

    let target = wide(url);
    let code = unsafe {
        let com = win::CoInitializeEx(std::ptr::null(), win::COINIT_APARTMENTTHREADED as u32);
        let result = win::ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            win::SW_SHOWNORMAL,
        );
        if com >= 0 {
            win::CoUninitialize();
        }
        result as isize
    };

    match code {
        c if c > 32 => Ok(()),
        c => Err(format!("ouverture impossible (code {c})")),
    }
}

/// Lance l'outil avec elevation.
///
/// Les trois outils chargent un pilote noyau pour lire les registres MSR : sans
/// elevation ils demarrent, mais ne publient rien — l'utilisateur verrait l'outil
/// tourner et la mesure rester vide. Le verbe `runas` fait poser la question par Windows,
/// qui est le seul a pouvoir la poser.
pub fn launch(tool_id: &str) -> Result<(), String> {
    let t = tool(tool_id).ok_or_else(|| format!("outil inconnu : {tool_id}"))?;
    let path = installs()
        .get(tool_id)
        .cloned()
        .flatten()
        .ok_or_else(|| format!("{} est introuvable sur cette machine", t.name))?;
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    let verb = wide("runas");
    let file = wide(&path.display().to_string());
    let cwd = wide(&dir.display().to_string());

    let code = unsafe {
        // ShellExecute attend un thread initialise pour COM ; celui d'une commande Tauri
        // ne l'est pas. L'echec de l'initialisation n'est pas bloquant, on tente quand meme.
        let com = win::CoInitializeEx(std::ptr::null(), win::COINIT_APARTMENTTHREADED as u32);
        let result = win::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            cwd.as_ptr(),
            win::SW_SHOWNORMAL,
        );
        if com >= 0 {
            win::CoUninitialize();
        }
        result as isize
    };

    // ShellExecute rend une pseudo-instance : au-dela de 32 c'est un succes, en deca un
    // code d'erreur historique.
    match code {
        c if c > 32 => Ok(()),
        5 => Err(format!("{} : élévation non accordée", t.name)),
        2 | 3 => Err(format!("{} : introuvable à l'emplacement connu", t.name)),
        c => Err(format!("{} : lancement impossible (code {c})", t.name)),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_maps_to_a_known_tool() {
        for (provider, tool_id) in BY_PROVIDER {
            assert!(
                tool(tool_id).is_some(),
                "{provider} renvoie vers l'outil inconnu {tool_id}"
            );
        }
    }

    /// Un outil introuvable par la base de desinstallation doit le dire, sans quoi
    /// l'interface affirmerait « pas installe » a propos d'un outil portable.
    #[test]
    fn portable_tools_are_not_claimed_absent() {
        let lhm = tool("libre-hw").expect("outil connu");
        assert!(lhm.needle.is_none());
        let statuses = statuses();
        let status = &statuses["libre-hw"];
        assert!(status.installed || !status.certain);
        assert!(!status.certain);
    }

    #[test]
    fn expands_environment_variables() {
        std::env::set_var("THERMAL_LAB_TEST_DIR", r"C:\ailleurs");
        assert_eq!(
            expand(r"%THERMAL_LAB_TEST_DIR%\outil.exe"),
            r"C:\ailleurs\outil.exe"
        );
        assert_eq!(expand(r"C:\sans\variable"), r"C:\sans\variable");
        assert_eq!(expand("%INCONNU_ICI%\\x"), "%INCONNU_ICI%\\x");
    }

    /// Le processus de test lui-meme doit etre vu : sans cela, la detection ne prouve
    /// rien quand elle ne trouve pas un outil.
    #[test]
    fn sees_the_current_process() {
        let images = running_images();
        assert!(
            images.iter().any(|i| i.contains("thermal")),
            "l'enumeration des processus ne voit pas le binaire de test"
        );
    }

    /// Coherence de ce que la detection rapporte sur la machine qui execute le test,
    /// et diagnostic lisible avec `--nocapture` : c'est le seul moyen de verifier la
    /// localisation d'un outil sans en installer un dans le test.
    #[test]
    fn reported_state_is_self_consistent() {
        for (provider, s) in statuses() {
            println!(
                "{provider:<10} installe={:<5} certain={:<5} tourne={:<5} lancable={:<5} {}",
                s.installed,
                s.certain,
                s.running,
                s.launchable,
                s.path.clone().unwrap_or_else(|| "-".into())
            );
            assert_eq!(s.installed, s.path.is_some());
            assert_eq!(s.launchable, s.installed && !s.running);
        }
    }

    /// `ShellExecute` ouvre aussi bien un programme qu'une page : le filtre est ce qui
    /// empeche une adresse venue de l'interface de devenir une execution.
    #[test]
    fn only_web_addresses_are_opened() {
        assert!(open_url(r"C:\Windows\System32\calc.exe").is_err());
        assert!(open_url("file:///C:/Windows/System32/calc.exe").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn launchable_requires_installed_and_stopped() {
        let t = tool("core-temp").expect("outil connu");
        let somewhere = Some(PathBuf::from(r"C:\Program Files\Core Temp\Core Temp.exe"));

        assert!(ToolStatus::new(t, somewhere.clone(), false).launchable);
        assert!(!ToolStatus::new(t, somewhere, true).launchable);
        assert!(!ToolStatus::new(t, None, false).launchable);
    }
}
