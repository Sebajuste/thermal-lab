import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  checkUpdate,
  chooseProfile,
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
  type GpuHolder,
  type MetricKey,
  type PhasesSnapshot,
  type ProfileId,
  type ProfileInfo,
  type PowerState,
  type Reading,
  type UpdateInfo,
} from "./api";
import { t } from "./i18n";
import Icon from "./components/Icon";
import MetricCard, { type Badge } from "./components/MetricCard";
import PhaseTable from "./components/PhaseTable";
import ProfilePicker from "./components/ProfilePicker";
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

/** Au-delà, la carte est redescendue dans un état de repos. */
const GPU_PINNED_PSTATE = 5;

/**
 * Les issues de l'algorithme de décision de `docs/gpu-power-control.md`, plus une :
 * `pinned`, l'anomalie établie dont la cause reste indécidable faute de compteurs par
 * process. La taire serait cacher une anomalie qu'on sait réelle.
 *
 * La charge n'y figure pas : elle a sa propre hystérésis, et prime sur tout le reste.
 */
type GpuVerdict = "mute" | "busy" | "rest" | "display" | "software" | "policy" | "pinned";

/**
 * L'algorithme de décision, étapes ① à ⑥, sur un relevé.
 *
 * L'ordre n'est pas indifférent. ② avant l'anomalie : une carte que le pilote déclare
 * occupée travaille, même quand `utilization.gpu` affiche 0. ③ avant ④ : une carte
 * redescendue va bien, écran ou pas — et c'est ce qui sépare une carte épinglée d'une
 * carte seulement réveillée, y compris par notre propre échantillonnage. ④ avant ⑤ : une
 * carte qui affiche est tenue par le compositeur, qu'on accuserait à tort. ⑥ ne se prouve
 * pas, il se conclut par élimination : sans ⑤ décidable, il ne l'est pas non plus.
 *
 * La fréquence n'y entre pas. Sur la carte de contrepoint, `clocks.sm` renvoie 2115 MHz
 * au MHz près en toutes circonstances : une valeur nominale recopiée par le pilote, pas
 * une mesure. Elle a figuré dans ce verdict et y disait « épinglé » quoi qu'il arrive.
 */
function gpuVerdict(r: Reading): GpuVerdict {
  const pstate = val(r, "gpuPerfStateIndex");
  const display = val(r, "gpuDisplayActive");
  const driverIdle = val(r, "gpuDriverIdle");
  // ①
  if (pstate === null || display === null || driverIdle === null) return "mute";
  // ②
  if (driverIdle === 0) return "busy";
  // ③
  if (pstate > GPU_PINNED_PSTATE) return "rest";
  // ④
  if (display === 1) return "display";
  // ⑤ et ⑥
  const holders = val(r, "gpuHolderCount");
  if (holders === null) return "pinned";
  return holders > 0 ? "software" : "policy";
}

/**
 * Une grandeur continue qui n'a pas bougé pendant que la charge, elle, a franchi les deux
 * régimes, n'est pas un capteur. Sans variation de charge on ne conclut rien : une carte
 * au repos a le droit de rester à 210 MHz tout du long.
 */
const MIN_FROZEN_SAMPLES = 10;

function isFrozen(history: Reading[], m: MetricKey): boolean {
  const values = history.map((h) => val(h, m)).filter((v): v is number => v !== null);
  if (values.length < MIN_FROZEN_SAMPLES) return false;
  const loads = history
    .map((h) => val(h, "gpuUtilPct"))
    .filter((v): v is number => v !== null);
  const sawIdle = loads.some((l) => l < GPU_IDLE_ENTER_PCT);
  const sawBusy = loads.some((l) => l > GPU_IDLE_LEAVE_PCT);
  return sawIdle && sawBusy && values.every((v) => v === values[0]);
}

const holderLabel = (h: GpuHolder) => h.name ?? `pid ${h.pid}`;

/**
 * Charge en dessous de laquelle le GPU est considéré inoccupé. Deux seuils comme pour le
 * CPU : un seul ferait osciller le badge sur le jitter du repos.
 */
const GPU_IDLE_ENTER_PCT = 10;
const GPU_IDLE_LEAVE_PCT = 20;

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

/**
 * L'ouverture du panneau, telle que Rust la connaît.
 *
 * Le même signal règle la cadence de mesure côté Rust : les deux côtés ne peuvent pas
 * diverger, et l'interface ne sonde jamais plus vite que le hub ne mesure.
 *
 * On part de « replié » plutôt que de l'inverse : lancée au démarrage de session,
 * l'application charge son interface sans jamais montrer le panneau, et supposer
 * l'ouverture ferait tourner une salve de sondages pour rien.
 */
function usePanelVisible() {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const off = listen<boolean>("panel-visibility", (e) => setVisible(e.payload));
    void readUiState()
      .then((s) => setVisible(s.panelVisible))
      .catch(() => {});
    return () => void off.then((f) => f());
  }, []);

  return visible;
}

/**
 * Un sondage qui ne tourne que panneau ouvert, et qui repart d'une lecture immédiate à
 * la réouverture — attendre le premier intervalle montrerait des valeurs périmées.
 *
 * Replié, l'application n'a personne à informer : l'infobulle de l'icône et les moyennes
 * du comparatif sont tenues côté Rust, précisément pour ne dépendre d'aucune interface.
 * Ce qui tournait ici était du travail pur pour le webview — appels, rendu React,
 * réallocation de l'historique — devant une fenêtre que personne ne regardait.
 *
 * `alive` dit si le sondage court toujours : une réponse arrivée après la fermeture n'a
 * plus rien à écrire.
 */
function usePollWhileVisible(
  poll: (alive: () => boolean) => void,
  ms: number,
  visible: boolean,
) {
  useEffect(() => {
    if (!visible) return;
    let live = true;
    const run = () => poll(() => live);
    run();
    const id = setInterval(run, ms);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [poll, ms, visible]);
}

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
  const [profile, setProfileState] = useState<ProfileId>("capped");
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  // La bascule passe par une tâche planifiée, donc par une invite UAC : le temps que
  // l'utilisateur y réponde, la case ne doit pas laisser croire qu'il ne s'est rien passé.
  const [autostartBusy, setAutostartBusy] = useState(false);

  const [history, setHistory] = useState<Reading[]>([]);

  const visible = usePanelVisible();

  // Le turbo peut s'engager sur un seul cœur pendant une fraction de seconde. Afficher
  // le relevé brut ferait clignoter le badge ; on exige deux mesures concordantes.
  const [turboStable, setTurboStable] = useState<boolean | null>(null);
  const [atRest, setAtRest] = useState(true);
  const pendingTurbo = useRef<{ value: boolean | null; count: number }>({
    value: null,
    count: 0,
  });

  // Même précaution côté GPU : un état transitoire ne doit pas faire clignoter le badge.
  const [gpuVerdictStable, setGpuVerdictStable] = useState<GpuVerdict | null>(null);
  const [gpuAtRest, setGpuAtRest] = useState(true);
  const pendingGpuVerdict = useRef<{ value: GpuVerdict | null; count: number }>({
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
        setProfileState(s.profile);
        setProfiles(s.profiles);
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
  const refreshPower = useCallback(
    (alive: () => boolean) =>
      void readPowerState()
        .then((p) => alive() && setPower(p))
        .catch((e) => alive() && setError(String(e))),
    [],
  );

  usePollWhileVisible(refreshPower, POWER_MS, visible);

  // La bascule depuis le menu de l'icône se signale d'elle-même : cet abonnement tient
  // panneau replié, il ne coûte rien tant que rien ne change.
  useEffect(() => {
    const off = listen<PowerState>("power-changed", (e) => setPower(e.payload));
    return () => void off.then((f) => f());
  }, []);

  // Une erreur née dans le menu de l'icône n'a nulle part où s'afficher : elle arrive ici.
  useEffect(() => {
    const off = listen<string>("app-error", (e) => setError(e.payload));
    return () => void off.then((f) => f());
  }, []);

  // Les fournisseurs peuvent apparaître à chaud — lancer Core Temp ne doit pas imposer
  // de redémarrer l'application.
  // `alive` a une valeur par défaut : `ProvidersPanel` rappelle cette fonction après
  // avoir lancé un outil, et `setTimeout` ne lui passe aucun argument.
  const refreshCaps = useCallback(
    (alive: () => boolean = () => true) =>
      void readCapabilities()
        .then((c) => alive() && setCaps(c))
        .catch(() => {}),
    [],
  );

  usePollWhileVisible(refreshCaps, CAPS_MS, visible);

  // Les moyennes sont tenues côté Rust et continuent de s'accumuler panneau replié :
  // il n'y a rien à lire tant que personne ne regarde le comparatif.
  const refreshPhases = useCallback(
    (alive: () => boolean) =>
      void readPhases()
        .then((p) => alive() && setPhases(p))
        .catch(() => {}),
    [],
  );

  usePollWhileVisible(refreshPhases, PHASES_MS, visible);

  const tickSensors = useCallback((alive: () => boolean) => {
    void (async () => {
      try {
        const r = await readSensors();
        if (!alive()) return;
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

        // Le décodage vidéo laisse `gpuUtilPct` bas tout en occupant la carte : une
        // lecture en plein écran passerait pour un repos, et un GPU qui décode n'a rien
        // d'épinglé. Les moteurs dédiés comptent donc dans la charge.
        const gpuUtil = val(r, "gpuUtilPct");
        if (gpuUtil !== null) {
          const load = Math.max(
            gpuUtil,
            val(r, "gpuDecodeUtilPct") ?? 0,
            val(r, "gpuEncodeUtilPct") ?? 0,
          );
          setGpuAtRest((rest) =>
            rest ? load <= GPU_IDLE_LEAVE_PCT : load < GPU_IDLE_ENTER_PCT,
          );
        }

        const verdict = gpuVerdict(r);
        const g = pendingGpuVerdict.current;
        pendingGpuVerdict.current =
          verdict === g.value
            ? { value: g.value, count: g.count + 1 }
            : { value: verdict, count: 1 };
        if (pendingGpuVerdict.current.count >= 2) setGpuVerdictStable(verdict);
      } catch (e) {
        if (alive()) setError(String(e));
      }
    })();
  }, []);

  usePollWhileVisible(tickSensors, POLL_MS, visible);

  // L'historique du graphe est vidé au repli. Le conserver recollerait la courbe d'avant
  // la fermeture sur celle d'après, sans discontinuité visible, alors qu'une heure a pu
  // s'écouler entre les deux points voisins.
  useEffect(() => {
    if (!visible) setHistory([]);
  }, [visible]);

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

  // Même verrou que l'interrupteur : les deux peuvent écrire le schéma, et powercfg
  // prend plusieurs centaines de millisecondes.
  const selectProfile = useCallback(
    async (id: ProfileId) => {
      if (busy) return;
      setBusy(true);
      setError(null);
      try {
        const choice = await chooseProfile(id);
        setProfileState(choice.profile);
        setPower(choice.power);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [busy],
  );

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
        ? { text: t.badgeCapped, kind: "cool" }
        : turboView === "idle"
          ? { text: t.badgeIdle, kind: "idle" }
          : null;

  // La charge prime : une carte qui calcule explique tout, quel que soit le reste.
  const gpuView: GpuVerdict | null =
    !reading || val(reading, "gpuUtilPct") === null
      ? null
      : !gpuAtRest
        ? "busy"
        : gpuVerdictStable;

  const gpuAnomaly = gpuView === "software" || gpuView === "policy" || gpuView === "pinned";

  // Les couleurs gardent le sens qu'elles ont pour le CPU : « hot » l'état coûteux,
  // « cool » l'état économe, « idle » celui où la mesure ne permet pas de conclure. Une
  // carte qui affiche coûte, mais légitimement : rien à conclure sur un gaspillage.
  const gpuBadge: Badge | null = gpuAnomaly
    ? { text: t.badgeGpuPinned, kind: "hot" }
    : gpuView === "rest"
      ? { text: t.badgeIdle, kind: "cool" }
      : gpuView === "display"
        ? { text: t.badgeGpuDisplay, kind: "idle" }
        : gpuView === "busy"
          ? { text: t.badgeGpuBusy, kind: "idle" }
          : null;

  const holders = reading?.gpuHolders ?? [];
  const holderCount = val(reading, "gpuHolderCount") ?? holders.length;

  const spark = (m: MetricKey) => history.map((h) => val(h, m));
  const nominal = val(reading, "cpuNominalMhz");
  const maxCore = val(reading, "cpuMaxCorePct");

  // La fréquence seule ne se juge pas : c'est son rapport au plafond de la carte qui
  // dit s'il s'agit d'un repos ou d'un plein régime.
  const gpuClock = val(reading, "gpuClockMhz");
  const gpuClockMax = val(reading, "gpuClockMaxMhz");
  const gpuClockFrozen = isFrozen(history, "gpuClockMhz");
  const gpuClockText =
    gpuClock === null
      ? ""
      : gpuClockMax === null
        ? fmt(gpuClock, 0, " MHz")
        : `${fmt(gpuClock, 0)} / ${fmt(gpuClockMax, 0, " MHz")}`;
  const gpuClockNote =
    gpuClockFrozen && gpuClockText ? `${gpuClockText} · ${t.gpuFrozen}` : gpuClockText;

  // Épinglée, la cause remplace la fréquence : c'est ce que l'utilisateur doit lire, et
  // la fréquence n'est pas toujours une mesure sur les cartes concernées.
  const gpuPowerNote =
    gpuView === "software" && holders.length > 0
      ? t.gpuHeldBy(holderLabel(holders[0]), Math.max(0, holderCount - 1))
      : gpuView === "policy"
        ? t.gpuNoClient
        : gpuView === "pinned"
          ? t.gpuCauseUnknown
          : gpuClockNote;

  const gpuPowerHint =
    gpuView === "software" && holders.length > 0
      ? `${t.gpuPowerHint}\n${t.gpuClients} ${holders
          .map((h) =>
            h.dedicatedMb === null
              ? holderLabel(h)
              : `${holderLabel(h)} (${fmt(h.dedicatedMb, 0, t.mbUnit)})`,
          )
          .join(", ")}`
      : t.gpuPowerHint;

  /**
   * Le contenu est produit à la demande par rang d'onglet : pendant la transition, le
   * deck monte deux vues à la fois — celle qui sort et celle qui entre.
   */
  const renderTab = (index: number) => {
    switch (TAB_IDS[index]) {
      case "compare":
        return <PhaseTable phases={phases} selected={profile} onReset={reset} />;

      case "system":
        return (
          <>
            <ProvidersPanel
              caps={caps}
              onChanged={refreshCaps}
              onError={setError}
            />

            <section className="panel">
              <h2>{t.application}</h2>
              <label className="check" title={t.autostartHint}>
                <input
                  type="checkbox"
                  checked={autostart}
                  disabled={autostartBusy}
                  onChange={() => void toggleAutostart()}
                />
                <span>
                  {t.autostart}
                  {autostartBusy && " …"}
                </span>
              </label>

              <label className="check" title={t.autoUpdateHint}>
                <input
                  type="checkbox"
                  checked={autoUpdate}
                  onChange={() => void toggleAutoUpdate()}
                />
                <span>{t.autoUpdate}</span>
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
                title={t.quitHint}
              >
                {t.quit}
              </button>
              {power && (
                <div className="mono" title={t.activeScheme}>
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
              title={t.maxCore}
              value={fmt(maxCore, 0, " %")}
              provider={providerOf(reading, "cpuMaxCorePct")}
              badge={turboBadge}
              hint={t.maxCoreHint}
              note={
                maxCore !== null && nominal !== null
                  ? `≈ ${((nominal * maxCore) / 100).toFixed(0)} MHz`
                  : undefined
              }
            >
              <Sparkline values={spark("cpuMaxCorePct")} color="#ff9f43" />
            </MetricCard>

            <MetricCard
              title={t.cpuTemp}
              value={fmt(val(reading, "cpuTempC"), 1, " °C")}
              provider={providerOf(reading, "cpuTempC")}
              hint={t.cpuTempHint}
            >
              <Sparkline values={spark("cpuTempC")} color="#ff6b6b" />
            </MetricCard>

            <MetricCard
              title={t.cpuPower}
              value={fmt(val(reading, "cpuPowerW"), 1, " W")}
              provider={providerOf(reading, "cpuPowerW")}
              hint={t.cpuPowerHint}
            >
              <Sparkline values={spark("cpuPowerW")} color="#f78fb3" />
            </MetricCard>

            <MetricCard
              title={t.cpuLoad}
              value={fmt(val(reading, "cpuUtilPct"), 1, " %")}
              provider={providerOf(reading, "cpuUtilPct")}
              hint={t.cpuLoadHint}
            >
              <Sparkline
                values={spark("cpuUtilPct")}
                min={0}
                max={100}
                color="#9aa7b8"
              />
            </MetricCard>

            <MetricCard
              title={t.gpuTemp}
              value={fmt(val(reading, "gpuTempC"), 0, " °C")}
              provider={providerOf(reading, "gpuTempC")}
              hint={t.gpuTempHint}
              note={fmt(val(reading, "gpuUtilPct"), 0, t.gpuLoadUnit)}
            >
              <Sparkline values={spark("gpuTempC")} color="#4fc3ff" />
            </MetricCard>

            <MetricCard
              title={t.gpuPower}
              value={fmt(val(reading, "gpuPowerW"), 1, " W")}
              provider={providerOf(reading, "gpuPowerW")}
              hint={gpuPowerHint}
              badge={gpuBadge}
              note={gpuPowerNote}
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
                ? t.turboCapped
                : t.capTurbo
              : (caps?.powerBlockedReason ?? t.powerUnavailable)
          }
        />
      </div>

      {profiles.length > 0 && (
        <ProfilePicker
          profiles={profiles}
          selected={profile}
          power={power}
          busy={busy}
          onSelect={(id) => void selectProfile(id)}
        />
      )}

      {error && (
        <div
          className="banner err"
          onClick={() => setError(null)}
          title={t.hide}
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
