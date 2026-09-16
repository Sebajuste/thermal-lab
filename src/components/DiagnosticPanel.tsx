import { useCallback, useState } from "react";
import {
  runDiagnosis,
  val,
  type Diagnosis,
  type LeverReading,
  type ProfileInfo,
} from "../api";
import { t } from "../i18n";
import { LEVERS } from "../levers";

interface Props {
  /** Le profil choisi : ses valeurs sont celles qu'on attend. */
  profile: ProfileInfo | undefined;
}

const num = (v: number | null | undefined) => (v === null || v === undefined ? "·" : String(v));

/** Secteur / batterie, ou une seule valeur quand les deux coïncident. */
const pair = (ac: number | null, dc: number | null) =>
  ac === null && dc === null ? "·" : ac === dc ? num(ac) : `${num(ac)}/${num(dc)}`;

const effective = (l: LeverReading) => l.policyAc ?? l.schemeAc ?? l.registryAc;

const yesNo = (v: number | null) => (v === null ? "?" : v === 1 ? t.yes : t.no);

/** Le journal sur une ligne : il est court, et un retour à la ligne se photographie mal. */
function compactJournal(raw: string | null): string {
  if (raw === null) return t.none;
  try {
    return JSON.stringify(JSON.parse(raw));
  } catch {
    return raw.replace(/\s+/g, " ").trim();
  }
}

/** Les lignes du relevé, en texte : ce que le bouton copie, et ce que le panneau affiche. */
function lines(d: Diagnosis, profile: ProfileInfo | undefined): string[] {
  const r = d.reading;
  const holders = r.gpuHolders
    .slice(0, 3)
    .map((h) => h.name ?? `pid ${h.pid}`)
    .join(", ");
  const clamp = d.gpuClamp;
  return [
    `${t.diagScheme} ${d.power.schemeName} ${d.power.schemeGuid}`,
    `${t.diagElevated} ${d.power.elevated ? t.yes : t.no} · ${t.diagPolicyScheme} ${d.power.policyScheme ?? t.none}`,
    `${t.diagProfile} ${profile ? t.profileName(profile.id) : "?"}`,
    ...d.power.levers.map((l) => {
      const spec = LEVERS.find((x) => x.name === l.name);
      const want = spec && profile ? profile[spec.key] : null;
      return `${l.name} ${t.diagWant}=${num(want)} ${t.diagSchemeCol}=${pair(l.schemeAc, l.schemeDc)} ${t.diagRegistryCol}=${pair(l.registryAc, l.registryDc)} ${t.diagPolicyCol}=${pair(l.policyAc, l.policyDc)}`;
    }),
    `${t.diagJournal} ${compactJournal(d.restoreJournal)}`,
    `GPU P${num(val(r, "gpuPerfStateIndex"))} · ${num(val(r, "gpuClockMhz"))}/${num(val(r, "gpuClockMaxMhz"))} MHz · ${num(val(r, "gpuPowerW"))} W · ${t.diagLoad} ${num(val(r, "gpuUtilPct"))} %`,
    `${t.diagDisplay} ${yesNo(val(r, "gpuDisplayActive"))} · ${t.diagDriverIdle} ${yesNo(val(r, "gpuDriverIdle"))} · ${t.diagClients} ${num(val(r, "gpuHolderCount"))}${holders ? ` (${holders})` : ""}`,
    `${t.diagClamp} ${clamp.enabled ? t.yes : t.no} · ${t.diagClamped} ${clamp.clamped ? t.yes : t.no} · ${t.diagClampJournal} ${d.gpuClampJournal ? t.yes : t.no}${clamp.unsupported ? ` · ${clamp.unsupported}` : ""}${clamp.error ? ` · ${clamp.error}` : ""}`,
  ];
}

/**
 * Tout ce que la machine dit d'elle-même, d'un geste. Pensé pour un poste géré où l'on
 * n'a ni console, ni copier-coller vers l'extérieur : le relevé doit se lire d'un coup
 * d'œil, et se photographier.
 *
 * Relevé à la demande : cinq appels à `powercfg`, une centaine de millisecondes.
 */
export default function DiagnosticPanel({ profile }: Props) {
  const [diag, setDiag] = useState<Diagnosis | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      setDiag(await runDiagnosis());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const copy = useCallback(async () => {
    if (!diag) return;
    try {
      await navigator.clipboard.writeText(lines(diag, profile).join("\n"));
      setCopied(true);
    } catch (e) {
      setError(String(e));
    }
  }, [diag, profile]);

  return (
    <section className="panel">
      <h2>{t.diagnostic}</h2>
      <p className="hint">{t.diagHint}</p>
      <div className="update-line">
        <button className="ghost" disabled={busy} onClick={() => void run()}>
          {busy ? t.diagRunning : t.diagRun}
        </button>
        {diag && (
          <button className="ghost" onClick={() => void copy()}>
            {copied ? t.diagCopied : t.diagCopy}
          </button>
        )}
      </div>

      {error && <p className="hint hint-line">{error}</p>}

      {diag && (
        <div className="diag">
          <p>
            {diag.power.schemeName} · <span className="diag-guid">{diag.power.schemeGuid}</span>
          </p>
          <p>
            {t.diagElevated} {diag.power.elevated ? t.yes : t.no} · {t.diagPolicyScheme}{" "}
            {diag.power.policyScheme ?? t.none}
          </p>
          <p>
            {t.diagProfile} {profile ? t.profileName(profile.id) : "?"}
          </p>

          <div className="diag-scroll">
            <table className="diag-table">
              <thead>
                <tr>
                  <th />
                  <th>{t.diagWant}</th>
                  <th>{t.diagSchemeCol}</th>
                  <th>{t.diagRegistryCol}</th>
                  <th>{t.diagPolicyCol}</th>
                </tr>
              </thead>
              <tbody>
                {diag.power.levers.map((l) => {
                  const spec = LEVERS.find((x) => x.name === l.name);
                  const want = spec && profile ? profile[spec.key] : null;
                  const off = want !== null && effective(l) !== want;
                  return (
                    <tr key={l.name} className={off ? "off" : undefined}>
                      <td title={l.name}>{spec ? spec.label() : l.name}</td>
                      <td>{num(want)}</td>
                      <td>{pair(l.schemeAc, l.schemeDc)}</td>
                      <td>{pair(l.registryAc, l.registryDc)}</td>
                      <td className={l.policyAc !== null ? "locked" : undefined}>
                        {pair(l.policyAc, l.policyDc)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>

          {lines(diag, profile)
            .slice(3 + diag.power.levers.length)
            .map((line) => (
              <p key={line}>{line}</p>
            ))}
        </div>
      )}
    </section>
  );
}
