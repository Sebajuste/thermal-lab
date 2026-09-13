import type { ReactNode } from "react";
import StateBadge, { type Badge } from "./StateBadge";

export type { Badge };

interface Props {
  title: string;
  value: string;
  /** Ce que la carte ne dit pas d'elle-même : au survol, pas à l'écran. */
  hint?: string;
  /** Complément chiffré seulement : l'explication appartient à `hint`. */
  note?: string;
  /** Fournisseur ayant produit la valeur : l'utilisateur doit pouvoir en tracer l'origine. */
  provider?: string | null;
  badge?: Badge | null;
  children?: ReactNode;
}

export default function MetricCard({
  title,
  value,
  hint,
  note,
  provider,
  badge,
  children,
}: Props) {
  return (
    <div className="card" title={hint}>
      <div className="card-top">
        <span className="card-title">{title}</span>
        <StateBadge badge={badge ?? null} />
      </div>
      <div className="card-value">{value}</div>
      <div className="card-note">{note ?? ""}</div>
      {children}
      {provider && (
        <div className="card-source" title={`Mesure fournie par ${provider}`}>
          {provider}
        </div>
      )}
    </div>
  );
}
