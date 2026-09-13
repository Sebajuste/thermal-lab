import { useState } from "react";
import {
  launchTool,
  openUrl,
  type Capabilities,
  type ProviderStatus,
  type ToolStatus,
} from "../api";
import { t } from "../i18n";
import Icon from "./Icon";

const kindLabel = (kind: ProviderStatus["kind"]) =>
  kind === "builtin" ? t.kindBuiltin : t.kindExternal;

/**
 * L'état affiché croise deux informations qui ne disent pas la même chose : le
 * fournisseur sait si la mesure arrive, l'outil sait s'il est là. « Ne tourne pas »
 * recouvrait trois situations qui n'appellent pas la même action.
 */
type Shown = "ready" | "failed" | "started" | "stopped" | "missing" | "unknown";

const pill = (state: Shown): { text: string; kind: string; why: string } => {
  switch (state) {
    case "ready":
      return { text: t.pillReady, kind: "ok", why: t.pillReadyWhy };
    case "failed":
      return { text: t.pillFailed, kind: "err", why: t.pillFailedWhy };
    case "started":
      return { text: t.pillStarted, kind: "warn", why: t.pillStartedWhy };
    case "stopped":
      return { text: t.pillStopped, kind: "off", why: t.pillStoppedWhy };
    case "missing":
      return { text: t.pillMissing, kind: "off", why: t.pillMissingWhy };
    case "unknown":
      return { text: t.pillUnknown, kind: "off", why: t.pillUnknownWhy };
  }
};

function shown(p: ProviderStatus, tool?: ToolStatus): Shown {
  if (p.state === "ready") return "ready";
  if (p.state === "failed") return "failed";
  if (!tool) return "missing";
  if (tool.running) return "started";
  if (tool.installed) return "stopped";
  return tool.certain ? "missing" : "unknown";
}

/**
 * Ce qui reste à l'écran quand l'état ne suffit pas : la marche à suivre.
 *
 * Elle est superflue là où un bouton ou un lien fait déjà le travail, et indispensable
 * partout ailleurs — quand l'outil tourne sans rien publier, c'est là que se trouve le
 * réglage à cocher ; quand une source native manque, c'est là qu'on lit qu'il n'y a
 * rien à y faire.
 */
function detail(p: ProviderStatus, state: Shown, hasTool: boolean) {
  if (p.state === "failed") return p.detail.error;
  if (p.state !== "unavailable") return null;
  if (state === "stopped") return null; // le bouton de lancement le dit
  if (state === "missing" && hasTool) return null; // le lien de téléchargement le dit
  return p.detail.hint ?? p.detail.reason;
}

interface Props {
  caps: Capabilities | null;
  /** Relire les capacités : l'outil vient de démarrer, son état a changé. */
  onChanged: () => void;
  onError: (message: string) => void;
}

/**
 * Aucune capacité manquante n'est laissée sans explication : pour chaque fournisseur
 * absent, l'état de son outil et l'action qui le corrige ; pour chaque grandeur non
 * mesurée, qui saurait la fournir.
 */
export default function ProvidersPanel({ caps, onChanged, onError }: Props) {
  const [launching, setLaunching] = useState<string | null>(null);

  if (!caps) return null;

  const missing = caps.metrics.filter((m) => !m.available);

  /**
   * Le webview n'a pas de navigation sortante : `target="_blank"` n'y produit rien.
   * Le `href` reste pour ce qu'il apporte encore — l'adresse au survol, le curseur —
   * et l'ouverture passe par le systeme.
   */
  const open = (e: React.MouseEvent, url: string) => {
    e.preventDefault();
    openUrl(url).catch((err) => onError(String(err)));
  };

  const launch = async (tool: ToolStatus) => {
    setLaunching(tool.id);
    try {
      await launchTool(tool.id);
      onChanged();
      // Le pilote de l'outil met quelques secondes a se charger, et le hub ne
      // retente une source qu'un cycle sur cinq.
      setTimeout(onChanged, 4000);
    } catch (e) {
      onError(String(e));
    } finally {
      setLaunching(null);
    }
  };

  return (
    <section className="panel">
      <h2>{t.providers}</h2>

      <ul className="prov">
        {caps.providers.map((p) => {
          const tool = caps.tools[p.id];
          const state = shown(p, tool);
          const badge = pill(state);
          const note = detail(p, state, tool !== undefined);
          return (
            <li key={p.id}>
              <div className="prov-line">
                <span
                  className="prov-name"
                  title={`${kindLabel(p.kind)} · ${t.metricCount(p.provides.length)}${
                    tool?.path ? `\n${tool.path}` : ""
                  }`}
                >
                  {p.url ? (
                    <a href={p.url} onClick={(e) => open(e, p.url!)}>
                      {p.name}
                    </a>
                  ) : (
                    p.name
                  )}
                </span>
                {tool?.launchable && (
                  <button
                    className="chrome"
                    disabled={launching === tool.id}
                    onClick={() => void launch(tool)}
                    aria-label={t.launch(tool.name)}
                    title={t.launchAdmin(tool.name)}
                  >
                    <Icon name="launch" size={13} />
                  </button>
                )}
                <span className={`pill ${badge.kind}`} title={badge.why}>
                  {badge.text}
                </span>
              </div>
              {note && <div className="pill-note">{note}</div>}
            </li>
          );
        })}
      </ul>

      {missing.length > 0 && (
        <div className="missing">
          <strong>{t.notMeasured}</strong>
          <ul>
            {missing.map((m) => (
              <li key={m.metric}>
                <code>{m.metric}</code>
                {m.providedBy.length > 0 && <> — {m.providedBy.join(", ")}</>}
              </li>
            ))}
          </ul>
        </div>
      )}

      {!caps.canControlPower && caps.powerBlockedReason && (
        <div className="banner warn inline">{caps.powerBlockedReason}</div>
      )}
    </section>
  );
}
