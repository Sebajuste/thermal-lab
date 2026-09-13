import { useState } from "react";
import {
  launchTool,
  openUrl,
  type Capabilities,
  type ProviderStatus,
  type ToolStatus,
} from "../api";
import Icon from "./Icon";

const KIND_LABEL: Record<ProviderStatus["kind"], string> = {
  builtin: "natif",
  external: "outil tiers",
};

/**
 * L'état affiché croise deux informations qui ne disent pas la même chose : le
 * fournisseur sait si la mesure arrive, l'outil sait s'il est là. « Ne tourne pas »
 * recouvrait trois situations qui n'appellent pas la même action.
 */
type Shown = "ready" | "failed" | "started" | "stopped" | "missing" | "unknown";

const PILL: Record<Shown, { text: string; kind: string; why: string }> = {
  ready: { text: "actif", kind: "ok", why: "Mesure reçue." },
  failed: { text: "erreur", kind: "err", why: "Réponse inattendue." },
  started: {
    text: "lancé",
    kind: "warn",
    why: "Tourne, mais ne publie aucune mesure.",
  },
  stopped: { text: "arrêté", kind: "off", why: "Installé, non lancé." },
  missing: { text: "absent", kind: "off", why: "Introuvable." },
  unknown: {
    text: "introuvable",
    kind: "off",
    why: "Outil portable : détectable seulement s'il tourne.",
  },
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
      <h2>Fournisseurs</h2>

      <ul className="prov">
        {caps.providers.map((p) => {
          const tool = caps.tools[p.id];
          const state = shown(p, tool);
          const pill = PILL[state];
          const note = detail(p, state, tool !== undefined);
          return (
            <li key={p.id}>
              <div className="prov-line">
                <span
                  className="prov-name"
                  title={`${KIND_LABEL[p.kind]} · ${p.provides.length} grandeurs${
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
                    aria-label={`Lancer ${tool.name}`}
                    title={`Lancer ${tool.name} en administrateur`}
                  >
                    <Icon name="launch" size={13} />
                  </button>
                )}
                <span className={`pill ${pill.kind}`} title={pill.why}>
                  {pill.text}
                </span>
              </div>
              {note && <div className="pill-note">{note}</div>}
            </li>
          );
        })}
      </ul>

      {missing.length > 0 && (
        <div className="missing">
          <strong>Non mesuré ici :</strong>
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
