import { invoke } from "@tauri-apps/api/core";

/** Miroir de `sensors::Metric`. */
export type MetricKey =
  | "cpuTempC"
  | "cpuPowerW"
  | "cpuMaxCorePct"
  | "cpuAvgPerfPct"
  | "cpuUtilPct"
  | "cpuNominalMhz"
  | "boardTempC"
  | "gpuTempC"
  | "gpuPowerW"
  | "gpuClockMhz"
  | "gpuUtilPct";

/** Une valeur et le fournisseur qui l'a produite. */
export interface Sample {
  value: number;
  provider: string;
}

/** Miroir de `sensors::Reading`. */
export interface Reading {
  values: Partial<Record<MetricKey, Sample>>;
  cpuName: string | null;
  tsMs: number;
}

export type ProviderKind = "builtin" | "external";

/** Miroir de `sensors::hub::ProviderStatus`, dont l'état est aplati par serde. */
export type ProviderStatus = {
  id: string;
  name: string;
  kind: ProviderKind;
  provides: MetricKey[];
  url: string | null;
} & (
  | { state: "ready" }
  | { state: "unavailable"; detail: { reason: string; hint: string | null } }
  | { state: "failed"; detail: { error: string } }
);

export interface MetricInfo {
  metric: MetricKey;
  unit: string;
  available: boolean;
  providedBy: string[];
}

/** Miroir de `tools::ToolStatus`. */
export interface ToolStatus {
  id: string;
  name: string;
  /** L'exécutable a été localisé. */
  installed: boolean;
  /** Faux quand l'outil est portable : ne pas le trouver ne prouve pas son absence. */
  certain: boolean;
  running: boolean;
  path: string | null;
  /** Règle tenue côté Rust : installé et arrêté. */
  launchable: boolean;
}

export interface Capabilities {
  providers: ProviderStatus[];
  /** État de l'outil tiers de chaque fournisseur externe, par identifiant de fournisseur. */
  tools: Partial<Record<string, ToolStatus>>;
  metrics: MetricInfo[];
  canControlPower: boolean;
  powerBlockedReason: string | null;
}

/** Miroir de `power::PowerState`. */
export interface PowerState {
  schemeGuid: string;
  schemeName: string;
  boostMode: number;
  throttleMax: number;
  optimized: boolean;
  elevated: boolean;
}

/** Accès à une mesure, indifférent au fournisseur qui l'a produite. */
export const val = (r: Reading | null, m: MetricKey): number | null =>
  r?.values[m]?.value ?? null;

/** Provenance d'une mesure, pour que l'utilisateur sache d'où sort un chiffre. */
export const providerOf = (r: Reading | null, m: MetricKey): string | null =>
  r?.values[m]?.provider ?? null;

/** Miroir de `phases::Stat`. */
export interface Stat {
  avg: number;
  min: number;
  max: number;
  /** Nombre d'échantillons : une moyenne sur trois points n'en est pas une. */
  n: number;
}

/** Miroir de `phases::PhaseSnapshot`. */
export interface PhaseSnapshot {
  metrics: Partial<Record<MetricKey, Stat>>;
  seconds: number;
}

/** Miroir de `phases::PhasesSnapshot`. */
export interface PhasesSnapshot {
  optimized: PhaseSnapshot;
  free: PhaseSnapshot;
}

/** Ce que l'interface sait de son propre châssis. */
export interface UiState {
  pinned: boolean;
  autostart: boolean;
  /** Celle du paquet installé, celle que la mise à jour compare. */
  version: string;
}

/** Miroir de `update::UpdateInfo`. */
export interface UpdateInfo {
  version: string;
  current: string;
  /** Corps de la release GitHub, quand elle en porte un. */
  notes: string | null;
}

export const readSensors = () => invoke<Reading>("read_sensors");
export const readCapabilities = () => invoke<Capabilities>("read_capabilities");
export const readPowerState = () => invoke<PowerState>("power_state");
export const setOptimization = (on: boolean) =>
  invoke<PowerState>("set_optimization", { on });

/** Lance un outil tiers avec élévation : Windows pose la question. */
export const launchTool = (id: string) => invoke<void>("launch_tool", { id });

/** Ouvre une page dans le navigateur par défaut : le webview ne sait pas le faire. */
export const openUrl = (url: string) => invoke<void>("open_url", { url });

export const readPhases = () => invoke<PhasesSnapshot>("read_phases");
export const resetPhases = () => invoke<void>("reset_phases");

export const readUiState = () => invoke<UiState>("ui_state");
export const setPinned = (pinned: boolean) => invoke<void>("set_pinned", { pinned });
export const setAutostart = (on: boolean) => invoke<boolean>("set_autostart", { on });

/** `null` quand l'application est à jour. Remonte l'erreur : la recherche a été demandée. */
export const checkUpdate = () => invoke<UpdateInfo | null>("check_update");

/** Télécharge, installe, relance : l'appel ne rend la main que s'il échoue. */
export const installUpdate = () => invoke<void>("install_update");

// Le chassis passe par des commandes plutot que par l'API fenetre du frontend : tout
// le pilotage du panneau reste au meme endroit, cote Rust. Le deplacement y gagne en
// plus de remonter ses erreurs : `data-tauri-drag-region` echoue sans rien dire quand
// la permission manque.
export const startDrag = () => invoke<void>("start_drag");
export const hideWindow = () => invoke<void>("hide_window");
export const quitApp = () => invoke<void>("quit_app");

/** Moyenne d'une phase, ou `null` si rien n'a été mesuré. */
export const phaseAvg = (p: PhaseSnapshot | undefined, m: MetricKey): number | null =>
  p?.metrics[m]?.avg ?? null;

