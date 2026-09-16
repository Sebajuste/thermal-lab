import {
  DEFAULT_PROFILE,
  phaseAvg,
  phaseOf,
  type MetricKey,
  type PhaseKey,
  type PhasesSnapshot,
} from "../api";
import { t } from "../i18n";
import Icon from "./Icon";

interface RowSpec {
  label: () => string;
  metric: MetricKey;
  unit: string;
  digits: number;
}

const ROWS: RowSpec[] = [
  { label: () => t.maxCore, metric: "cpuMaxCorePct", unit: " %", digits: 0 },
  { label: () => t.cpuTemp, metric: "cpuTempC", unit: " °C", digits: 1 },
  { label: () => t.cpuPower, metric: "cpuPowerW", unit: " W", digits: 1 },
  { label: () => t.gpuTemp, metric: "gpuTempC", unit: " °C", digits: 1 },
  { label: () => t.gpuPower, metric: "gpuPowerW", unit: " W", digits: 1 },
];

const fmt = (v: number | null, digits: number, unit: string) =>
  v === null || !Number.isFinite(v) ? "—" : `${v.toFixed(digits)}${unit}`;

const duration = (s: number) =>
  s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${String(s % 60).padStart(2, "0")}`;

interface Props {
  phases: PhasesSnapshot | null;
  onReset: () => void;
}

/**
 * La colonne comparée à la référence : la phase bridée en cours, ou le profil par défaut
 * quand la machine est libre — c'est celui que l'interrupteur rallumerait. Un bridage
 * tiers a sa propre colonne, plutôt que de se mêler aux moyennes d'un profil.
 */
function compared(phases: PhasesSnapshot | null): PhaseKey {
  return phases && phases.current !== "free" ? phases.current : DEFAULT_PROFILE;
}

export default function PhaseTable({ phases, onReset }: Props) {
  const key = compared(phases);
  const optimized = phaseOf(phases, key);
  const free = phaseOf(phases, "free");

  return (
    <section className="panel">
      <div className="panel-head">
        <p className="hint">{t.compareHint}</p>
        <button
          className="chrome"
          onClick={onReset}
          aria-label={t.resetAverages}
          title={t.resetAverages}
        >
          <Icon name="reset" size={15} />
        </button>
      </div>
      <table className="cmp">
        <thead>
          <tr>
            <th />
            <th title={t.turboCapped}>
              {key === "custom" ? t.phaseCustom : t.phaseCapped} ·{" "}
              {duration(optimized?.seconds ?? 0)}
            </th>
            <th title={t.turboFree}>
              {t.phaseFree} · {duration(free?.seconds ?? 0)}
            </th>
            <th title={t.deltaHint}>Δ</th>
          </tr>
        </thead>
        <tbody>
          {ROWS.map((row) => {
            const on = phaseAvg(optimized, row.metric);
            const off = phaseAvg(free, row.metric);
            const delta = on !== null && off !== null ? on - off : null;
            return (
              <tr key={row.metric}>
                <td>{row.label()}</td>
                <td className="num">{fmt(on, row.digits, row.unit)}</td>
                <td className="num">{fmt(off, row.digits, row.unit)}</td>
                <td className={`num ${delta !== null && delta < 0 ? "good" : ""}`}>
                  {delta === null
                    ? "—"
                    : `${delta > 0 ? "+" : ""}${delta.toFixed(row.digits)}${row.unit}`}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
