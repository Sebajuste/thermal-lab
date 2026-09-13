import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  checkUpdate,
  hideWindow,
  providerOf,
  quitApp,
  readCapabilities,
  readPhases,
  readPowerState,
  readSensors,
  readUiState,
  resetPhases,
  setAutostart,
  setAutoUpdate,
  setOptimization,
  setPinned,
  val,
  type Capabilities,
  type MetricKey,
  type PhasesSnapshot,
  type PowerState,
  type Reading,
  type UpdateInfo,
} from "./api";
import Icon from "./components/Icon";
import MetricCard, { type Badge } from "./components/MetricCard";
import PhaseTable from "./components/PhaseTable";
import ProvidersPanel from "./components/ProvidersPanel";
import Sparkline from "./components/Sparkline";
import SlideDeck from "./components/SlideDeck";
import Tabs, { TAB_IDS, type TabId } from "./components/Tabs";
import TitleBar from "./components/TitleBar";
import Toggle from "./components/Toggle";
import UpdateRow from "./components/UpdateRow";

const HISTORY = 120; // 2 minutes à 1 Hz
const POLL_MS = 1000;
const PHASES_MS = 2000;
const POWER_MS = 5000;
const CAPS_MS = 10000;

/**
 * En dessous de cette charge, l'absence de turbo ne prouve rien : au repos le CPU
 * boost par à-coups d'une fraction de seconde, et ne pas le voir booster ne veut pas
 * dire qu'il en est empêché. Mesuré au repos sur 14900K : cœur max entre 46 et 173 %.
 *
 * Deux seuils et non un : la charge au repos jitte de part et d'autre de 15 %, et un
 * seuil unique ferait alterner « repos » et « plafonné » à chaque seconde. On ne quitte
 * le repos qu'une fois la charge franchement installée, et on n'y retombe qu'une fois
 * franchement retombée.
 */
const IDLE_ENTER_PCT = 15;
const IDLE_LEAVE_PCT = 25;

/** Au-delà, un cœur dépasse franchement le nominal : bridé il plafonne vers 99. */
const TURBO_THRESHOLD_PCT = 105;

/** Ce que la mesure permet réellement de conclure sur le turbo. */
type TurboView = "boost" | "capped" | "idle";

const fmt = (v: number | null, digits = 0, unit = "") =>
  v === null || !Number.isFinite(v) ? "—" : `${v.toFixed(digits)}${unit}`;

/**
 * Le nom complet tient mal sur une ligne partagée avec le schéma d'alimentation, et sa
 * parenthèse — « (Raptor Lake-S) » — n'apprend rien à qui regarde ses températures. Elle
 * reste au survol.
 *
 * Renvoie `null` quand aucun fournisseur ne nomme le processeur : mieux vaut la seule
 * information connue qu'un « CPU » qui n'en est pas une.
 */
const shortCpu = (name: string | null) =>
  name ? name.replace(/\s*\([^)]*\)\s*$/, "") : null;

export default function App() {
  const [reading, setReading] = useState<Reading | null>(null);
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [power, setPower] = useState<PowerState | null>(null);
  const [phases, setPhases] = useState<PhasesSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [tab, setTab] = useState<TabId>("live");
  const [pinned, setPinnedState] = useState(false);
  const [autostart, setAutostartState] = useState(false);
  const [autoUpdate, setAutoUpdateState] = useState(false);
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  // La bascule passe par une tâche planifiée, donc par une invite UAC : le temps que
  // l'utilisateur y réponde, la case ne doit pas laisser croire qu'il ne s'est rien passé.
  const [autostartBusy, setAutostartBusy] = useState(false);

  const [history, setHistory] = useState<Reading[]>([]);

  // Le turbo peut s'engager sur un seul cœur pendant une fraction de seconde. Afficher
  // le relevé brut ferait clignoter le badge ; on exige deux mesures concordantes.
  const [turboStable, setTurboStable] = useState<boolean | null>(null);
  const [atRest, setAtRest] = useState(true);
  const pendingTurbo = useRef<{ value: boolean | null; count: number }>({
    value: null,
    count: 0,
  });

  useEffect(() => {
    readUiState()
      .then((s) => {
        setPinnedState(s.pinned);
        setAutostartState(s.autostart);
        setAutoUpdateState(s.autoUpdate);
        setVersion(s.version);
      })
      .catch(() => {});
  }, []);

  // Une recherche au lancement, silencieuse : ne pas joindre GitHub n'est pas un
  // événement dont l'utilisateur a quelque chose à faire. Celle du bouton, elle, a été
  // demandée — elle rend son erreur.
  useEffect(() => {
    void checkUpdate()
      .then(setUpdate)
      .catch(() => {});
  }, []);

  // L'état d'alimentation change aussi hors de l'application : depuis le menu de
  // l'icône, ou depuis Windows lui-même.
  useEffect(() => {
    const refresh = () =>
      void readPowerState()
        .then(setPower)
        .catch((e) => setError(String(e)));
    refresh();
    const id = setInterval(refresh, POWER_MS);
    const off = listen<PowerState>("power-changed", (e) => setPower(e.payload));
    return () => {
      clearInterval(id);
      void off.then((f) => f());
    };
  }, []);

  // Une erreur née dans le menu de l'icône n'a nulle part où s'afficher : elle arrive ici.
  useEffect(() => {
    const off = listen<string>("app-error", (e) => setError(e.payload));
    return () => void off.then((f) => f());
  }, []);

  // Les fournisseurs peuvent apparaître à chaud — lancer Core Temp ne doit pas imposer
  // de redémarrer l'application.
  const refreshCaps = useCallback(
    () =>
      void readCapabilities()
        .then(setCaps)
        .catch(() => {}),
    [],
  );

  useEffect(() => {
    refreshCaps();
    const id = setInterval(refreshCaps, CAPS_MS);
    return () => clearInterval(id);
  }, [refreshCaps]);

  // Les moyennes sont tenues côté Rust : on ne fait que lire un instantané.
  useEffect(() => {
    const refresh = () =>
      void readPhases()
        .then(setPhases)
        .catch(() => {});
    refresh();
    const id = setInterval(refresh, PHASES_MS);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const r = await readSensors();
        if (!alive) return;
        setReading(r);
        setHistory((h) => [...h, r].slice(-HISTORY));

        const util = val(r, "cpuUtilPct");
        if (util !== null) {
          setAtRest((rest) =>
            rest ? util <= IDLE_LEAVE_PCT : util < IDLE_ENTER_PCT,
          );
        }

        const maxCore = val(r, "cpuMaxCorePct");
        if (maxCore !== null) {
          const boosting = maxCore > TURBO_THRESHOLD_PCT;
          const p = pendingTurbo.current;
          pendingTurbo.current =
            boosting === p.value
              ? { value: p.value, count: p.count + 1 }
              : { value: boosting, count: 1 };
          if (pendingTurbo.current.count >= 2) setTurboStable(boosting);
        }
      } catch (e) {
        if (alive) setError(String(e));
      }
    };
    void tick();
    const id = setInterval(() => void tick(), POLL_MS);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // Échap referme le panneau : c'est ce que fait tout volet système.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void hideWindow();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toggle = useCallback(async () => {
    if (!power || busy) return;
    setBusy(true);
    setError(null);
    try {
      setPower(await setOptimization(!power.optimized));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [power, busy]);

  const togglePin = useCallback(() => {
    const next = !pinned;
    setPinnedState(next);
    void setPinned(next);
  }, [pinned]);

  const toggleAutostart = useCallback(async () => {
    if (autostartBusy) return;
    setAutostartBusy(true);
    setError(null);
    try {
      setAutostartState(await setAutostart(!autostart));
    } catch (e) {
      setError(String(e));
    } finally {
      setAutostartBusy(false);
    }
  }, [autostart, autostartBusy]);

  // Aucun état à attendre ici : la préférence est un fichier, et la veille la relit
  // d'elle-même au battement suivant.
  const toggleAutoUpdate = useCallback(async () => {
    setError(null);
    try {
      setAutoUpdateState(await setAutoUpdate(!autoUpdate));
    } catch (e) {
      setError(String(e));
    }
  }, [autoUpdate]);

  const reset = useCallback(() => {
    void resetPhases().then(() => readPhases().then(setPhases));
    setHistory([]);
  }, []);

  const on = power?.optimized ?? false;
  const canToggle = caps?.canControlPower ?? false;

  // La politique se lit dans le schéma ; ceci ne dit que ce que la mesure observe, et
  // se tait quand elle ne peut rien conclure.
  const turboView: TurboView | null = !reading
    ? null
    : atRest
      ? "idle"
      : turboStable === null
        ? null
        : turboStable
          ? "boost"
          : "capped";

  const turboBadge: Badge | null =
    turboView === "boost"
      ? { text: "TURBO", kind: "hot" }
      : turboView === "capped"
        ? { text: "plafonné", kind: "cool" }
        : turboView === "idle"
          ? { text: "repos", kind: "idle" }
          : null;

  const spark = (m: MetricKey) => history.map((h) => val(h, m));
  const nominal = val(reading, "cpuNominalMhz");
  const maxCore = val(reading, "cpuMaxCorePct");

  /**
   * Le contenu est produit à la demande par rang d'onglet : pendant la transition, le
   * deck monte deux vues à la fois — celle qui sort et celle qui entre.
   */
  const renderTab = (index: number) => {
    switch (TAB_IDS[index]) {
      case "compare":
        return <PhaseTable phases={phases} onReset={reset} />;

      case "system":
        return (
          <>
            <ProvidersPanel
              caps={caps}
              onChanged={refreshCaps}
              onError={setError}
            />

            <section className="panel">
              <h2>Application</h2>
              <label
                className="check"
                title="Tâche planifiée, exécutée en administrateur. Élévation demandée une fois. Elle vise cet exécutable : le déplacer décoche la case, la recocher inscrit le nouvel emplacement."
              >
                <input
                  type="checkbox"
                  checked={autostart}
                  disabled={autostartBusy}
                  onChange={() => void toggleAutostart()}
                />
                <span>
                  Démarrer avec Windows, en administrateur
                  {autostartBusy && " …"}
                </span>
              </label>

              <label
                className="check"
                title="Recherche toutes les six heures, installation sans confirmation. Elle attend que le panneau soit fermé : l'application se relance seule."
              >
                <input
                  type="checkbox"
                  checked={autoUpdate}
                  onChange={() => void toggleAutoUpdate()}
                />
                <span>Mettre à jour automatiquement</span>
              </label>

              <UpdateRow
                version={version}
                auto={autoUpdate}
                update={update}
                onFound={setUpdate}
                onError={setError}
              />

              <button
                className="ghost danger"
                onClick={() => void quitApp()}
                title="Le schéma d'alimentation reste en l'état"
              >
                Quitter
              </button>
              {power && (
                <div className="mono" title="Schéma actif">
                  PERFBOOSTMODE={power.boostMode} · PROCTHROTTLEMAX=
                  {power.throttleMax}%
                  <br />
                  {power.schemeGuid}
                </div>
              )}
            </section>
          </>
        );

      default:
        return (
          <section className="grid">
            <MetricCard
              title="Cœur max"
              value={fmt(maxCore, 0, " %")}
              provider={providerOf(reading, "cpuMaxCorePct")}
              badge={turboBadge}
              hint="Cœur le plus rapide, en % du nominal. Au-delà de 100 : turbo."
              note={
                maxCore !== null && nominal !== null
                  ? `≈ ${((nominal * maxCore) / 100).toFixed(0)} MHz`
                  : undefined
              }
            >
              <Sparkline values={spark("cpuMaxCorePct")} color="#ff9f43" />
            </MetricCard>

            <MetricCard
              title="Temp. CPU"
              value={fmt(val(reading, "cpuTempC"), 1, " °C")}
              provider={providerOf(reading, "cpuTempC")}
              hint="Température de package (max des cœurs)."
            >
              <Sparkline values={spark("cpuTempC")} color="#ff6b6b" />
            </MetricCard>

            <MetricCard
              title="Puis. CPU"
              value={fmt(val(reading, "cpuPowerW"), 1, " W")}
              provider={providerOf(reading, "cpuPowerW")}
              hint="Puissance du package."
            >
              <Sparkline values={spark("cpuPowerW")} color="#f78fb3" />
            </MetricCard>

            <MetricCard
              title="Charge CPU"
              value={fmt(val(reading, "cpuUtilPct"), 1, " %")}
              provider={providerOf(reading, "cpuUtilPct")}
              hint="Occupation totale des cœurs."
            >
              <Sparkline
                values={spark("cpuUtilPct")}
                min={0}
                max={100}
                color="#9aa7b8"
              />
            </MetricCard>

            <MetricCard
              title="Temp. GPU"
              value={fmt(val(reading, "gpuTempC"), 0, " °C")}
              provider={providerOf(reading, "gpuTempC")}
              hint="Température et charge GPU."
              note={fmt(val(reading, "gpuUtilPct"), 0, " % charge")}
            >
              <Sparkline values={spark("gpuTempC")} color="#4fc3ff" />
            </MetricCard>

            <MetricCard
              title="Puis. GPU"
              value={fmt(val(reading, "gpuPowerW"), 1, " W")}
              provider={providerOf(reading, "gpuPowerW")}
              hint="Puissance et fréquence GPU."
              note={fmt(val(reading, "gpuClockMhz"), 0, " MHz")}
            >
              <Sparkline values={spark("gpuPowerW")} color="#7ee787" />
            </MetricCard>
          </section>
        );
    }
  };

  return (
    <div className="app">
      <TitleBar
        optimized={on}
        pinned={pinned}
        onTogglePin={togglePin}
        onClose={() => void hideWindow()}
      />

      <div className="head">
        <span className="head-info" title={reading?.cpuName ?? undefined}>
          {[shortCpu(reading?.cpuName ?? null), power?.schemeName]
            .filter(Boolean)
            .join(" · ") || "…"}
        </span>
        <Toggle
          checked={on}
          disabled={!canToggle}
          busy={busy}
          onChange={() => void toggle()}
          title={
            canToggle
              ? on
                ? "Turbo bridé"
                : "Brider le turbo"
              : (caps?.powerBlockedReason ?? "indisponible")
          }
        />
      </div>

      {error && (
        <div
          className="banner err"
          onClick={() => setError(null)}
          title="Masquer"
        >
          <Icon name="alert" size={14} />
          <span>{error}</span>
        </div>
      )}

      <SlideDeck index={Math.max(0, TAB_IDS.indexOf(tab))} render={renderTab} />

      <Tabs active={tab} onSelect={setTab} />
    </div>
  );
}
