import { invoke } from "@tauri-apps/api/core";

/**
 * Les libellés de l'interface, en anglais par défaut.
 *
 * La langue n'est pas un réglage : Rust lit celle de l'affichage de Windows une fois
 * pour toutes et la rend ici. Le français n'apparaît donc que sur un Windows affiché en
 * français — un poste anglais au format régional français reste en anglais, ce qui est
 * bien ce qu'on veut : c'est la langue lue qui compte, pas le séparateur décimal.
 *
 * Deux dictionnaires plutôt qu'une bibliothèque : deux langues et une centaine de
 * chaînes ne paient pas une dépendance, et `fr` étant typé sur `en`, un libellé oublié
 * est une erreur de compilation.
 */
export type Lang = "en" | "fr";

const en = {
  // Barre de titre
  drag: "Drag to move",
  turboCapped: "Turbo capped",
  turboFree: "Turbo free",
  pin: "Pin",
  unpin: "Unpin",
  pinPanel: "Pin the panel",
  hidePanel: "Hide the panel",
  hide: "Hide",

  // Onglets
  tabLive: "Live readings",
  tabCompare: "Phase comparison",
  tabSystem: "Capabilities and settings",

  // Cartes de mesure
  maxCore: "Max core",
  maxCoreHint: "Fastest core, as a % of nominal. Above 100: turbo.",
  cpuTemp: "CPU temp.",
  cpuTempHint: "Package temperature (hottest core).",
  cpuPower: "CPU power",
  cpuPowerHint: "Package power draw.",
  cpuLoad: "CPU load",
  cpuLoadHint: "Total core occupancy.",
  gpuTemp: "GPU temp.",
  gpuTempHint: "GPU temperature and load.",
  gpuPower: "GPU power",
  gpuPowerHint: "GPU power draw and clock.",
  gpuLoadUnit: " % load",
  badgeCapped: "capped",
  badgeIdle: "idle",
  measuredBy: (provider: string) => `Measured by ${provider}`,

  // Comparaison des phases
  compareHint: "Compare under a steady load.",
  resetAverages: "Reset the averages",
  phaseCapped: "Capped",
  phaseFree: "Free",
  deltaHint: "Capped minus free",

  // Fournisseurs
  providers: "Providers",
  kindBuiltin: "built-in",
  kindExternal: "third-party tool",
  pillReady: "live",
  pillReadyWhy: "Reading received.",
  pillFailed: "error",
  pillFailedWhy: "Unexpected answer.",
  pillStarted: "started",
  pillStartedWhy: "Running, but publishes no reading.",
  pillStopped: "stopped",
  pillStoppedWhy: "Installed, not started.",
  pillMissing: "missing",
  pillMissingWhy: "Not found.",
  pillUnknown: "not found",
  pillUnknownWhy: "Portable tool: detectable only while it runs.",
  metricCount: (n: number) => `${n} metrics`,
  launch: (tool: string) => `Launch ${tool}`,
  launchAdmin: (tool: string) => `Launch ${tool} as administrator`,
  notMeasured: "Not measured here:",

  // Réglages de l'application
  application: "Application",
  autostart: "Start with Windows, as administrator",
  autostartHint:
    "Scheduled task, run as administrator. Elevation is asked once. It targets this executable: moving it clears the box, ticking it again records the new location.",
  autoUpdate: "Update automatically",
  autoUpdateHint:
    "Checks every six hours, installs without asking. It waits for the panel to be closed: the application restarts on its own.",
  quit: "Quit",
  quitHint: "The power scheme is left as it is",
  activeScheme: "Active scheme",
  capTurbo: "Cap the turbo",
  powerUnavailable: "unavailable",

  // Mise à jour
  installedVersion: "Installed version",
  installAndRestart: (version: string) => `Install v${version} and restart`,
  downloadingPct: (pct: number) => `Downloading ${pct} %`,
  downloadingMb: (mb: string) => `Downloading ${mb} MB`,
  checking: "Checking…",
  upToDate: "Up to date — check again",
  checkForUpdate: "Check for an update",
  updateNotice: (next: string, current: string) =>
    `v${next} will replace v${current}. The application closes and reopens; the power scheme is left as it is.`,
  updateAutoNotice: " Left alone, it will install once this panel is closed.",
};

type Strings = typeof en;

const fr: Strings = {
  drag: "Glisser pour déplacer",
  turboCapped: "Turbo bridé",
  turboFree: "Turbo libre",
  pin: "Épingler",
  unpin: "Détacher",
  pinPanel: "Épingler le panneau",
  hidePanel: "Masquer le panneau",
  hide: "Masquer",

  tabLive: "Mesures en direct",
  tabCompare: "Comparaison des phases",
  tabSystem: "Capacités et réglages",

  maxCore: "Cœur max",
  maxCoreHint: "Cœur le plus rapide, en % du nominal. Au-delà de 100 : turbo.",
  cpuTemp: "Temp. CPU",
  cpuTempHint: "Température de package (max des cœurs).",
  cpuPower: "Puis. CPU",
  cpuPowerHint: "Puissance du package.",
  cpuLoad: "Charge CPU",
  cpuLoadHint: "Occupation totale des cœurs.",
  gpuTemp: "Temp. GPU",
  gpuTempHint: "Température et charge GPU.",
  gpuPower: "Puis. GPU",
  gpuPowerHint: "Puissance et fréquence GPU.",
  gpuLoadUnit: " % charge",
  badgeCapped: "plafonné",
  badgeIdle: "repos",
  measuredBy: (provider) => `Mesure fournie par ${provider}`,

  compareHint: "Comparer sous charge stable.",
  resetAverages: "Réinitialiser les moyennes",
  phaseCapped: "Bridé",
  phaseFree: "Libre",
  deltaHint: "Bridé moins libre",

  providers: "Fournisseurs",
  kindBuiltin: "natif",
  kindExternal: "outil tiers",
  pillReady: "actif",
  pillReadyWhy: "Mesure reçue.",
  pillFailed: "erreur",
  pillFailedWhy: "Réponse inattendue.",
  pillStarted: "lancé",
  pillStartedWhy: "Tourne, mais ne publie aucune mesure.",
  pillStopped: "arrêté",
  pillStoppedWhy: "Installé, non lancé.",
  pillMissing: "absent",
  pillMissingWhy: "Introuvable.",
  pillUnknown: "introuvable",
  pillUnknownWhy: "Outil portable : détectable seulement s'il tourne.",
  metricCount: (n) => `${n} grandeurs`,
  launch: (tool) => `Lancer ${tool}`,
  launchAdmin: (tool) => `Lancer ${tool} en administrateur`,
  notMeasured: "Non mesuré ici :",

  application: "Application",
  autostart: "Démarrer avec Windows, en administrateur",
  autostartHint:
    "Tâche planifiée, exécutée en administrateur. Élévation demandée une fois. Elle vise cet exécutable : le déplacer décoche la case, la recocher inscrit le nouvel emplacement.",
  autoUpdate: "Mettre à jour automatiquement",
  autoUpdateHint:
    "Recherche toutes les six heures, installation sans confirmation. Elle attend que le panneau soit fermé : l'application se relance seule.",
  quit: "Quitter",
  quitHint: "Le schéma d'alimentation reste en l'état",
  activeScheme: "Schéma actif",
  capTurbo: "Brider le turbo",
  powerUnavailable: "indisponible",

  installedVersion: "Version installée",
  installAndRestart: (version) => `Installer la v${version} et relancer`,
  downloadingPct: (pct) => `Téléchargement ${pct} %`,
  downloadingMb: (mb) => `Téléchargement ${mb} Mo`,
  checking: "Recherche…",
  upToDate: "À jour — chercher encore",
  checkForUpdate: "Rechercher une mise à jour",
  updateNotice: (next, current) =>
    `La v${next} remplacera la v${current}. L'application se ferme et se rouvre ; le schéma d'alimentation reste en l'état.`,
  updateAutoNotice: " Sans clic, elle s'installera une fois ce panneau fermé.",
};

/**
 * Liaison vivante : les modules qui l'importent voient la substitution faite par
 * `initLang`. Elle doit donc être lue au rendu, jamais figée dans une table de module —
 * les imports sont évalués avant que Rust ait répondu.
 */
export let t: Strings = en;

export let lang: Lang = "en";

/**
 * À appeler avant le premier rendu. Un échec laisse l'anglais en place : ne pas savoir
 * la langue du poste n'est pas une raison de ne rien afficher.
 */
export async function initLang(): Promise<Lang> {
  try {
    lang = await invoke<Lang>("ui_lang");
    t = lang === "fr" ? fr : en;
  } catch {
    /* anglais */
  }
  document.documentElement.lang = lang;
  return lang;
}
