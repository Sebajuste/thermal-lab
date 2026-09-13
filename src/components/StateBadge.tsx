import { useEffect, useLayoutEffect, useRef, useState } from "react";

export interface Badge {
  text: string;
  kind: "hot" | "cool" | "idle";
}

/** Doit rester égal à `--state` dans la feuille de style. */
const SWAP_MS = 240;

const same = (a: Badge | null, b: Badge | null) =>
  a?.text === b?.text && a?.kind === b?.kind;

/**
 * Pastille d'état — turbo, plafonné, repos.
 *
 * L'élément ne bouge jamais d'un état à l'autre : c'est le même nœud qui change de
 * classe, donc sa couleur s'interpole au lieu de sauter. Restait la largeur, que
 * « plafonné » et « repos » ne partagent pas ; elle est posée en pixels après mesure
 * pour être interpolable à son tour — sans cela, la pastille se rétracte d'un coup et
 * le changement se lit comme un clignotement.
 *
 * Le mot lui-même ne peut pas se transformer : les deux libellés se superposent le
 * temps du fondu croisé, l'ancien hors du flux pour ne pas peser sur la mesure.
 */
export default function StateBadge({ badge }: { badge: Badge | null }) {
  const [shown, setShown] = useState<Badge | null>(badge);
  const [ghost, setGhost] = useState<Badge | null>(null);
  const [width, setWidth] = useState<number | null>(null);
  const label = useRef<HTMLSpanElement>(null);

  useLayoutEffect(() => {
    if (same(badge, shown)) return;
    setGhost(shown);
    setShown(badge);
  }, [badge, shown]);

  // Mesure avant peinture : la largeur cible doit être connue dans la même image que
  // le nouveau libellé, sinon la transition part d'une valeur déjà appliquée.
  useLayoutEffect(() => {
    setWidth(shown && label.current ? label.current.scrollWidth : 0);
  }, [shown]);

  useEffect(() => {
    if (!ghost) return;
    const id = window.setTimeout(() => setGhost(null), SWAP_MS);
    return () => window.clearTimeout(id);
  }, [ghost]);

  if (!shown && !ghost) return null;

  return (
    <span
      className={`badge ${(shown ?? ghost)!.kind}`}
      style={{ opacity: shown ? 1 : 0 }}
    >
      <span className="badge-swap" style={{ width: width ?? undefined }}>
        {shown && (
          <span key={shown.text} className="badge-label" ref={label}>
            {shown.text}
          </span>
        )}
        {ghost && <span className="badge-label ghost">{ghost.text}</span>}
      </span>
    </span>
  );
}
