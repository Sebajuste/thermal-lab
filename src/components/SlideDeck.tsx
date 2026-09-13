import { useEffect, useRef, useState, type ReactNode } from "react";

/** Doit rester égal à la durée de `view-in`/`view-out` dans la feuille de style. */
const SLIDE_MS = 260;

interface Props {
  /** Rang de la vue affichée : son signe par rapport au précédent donne le sens du glissement. */
  index: number;
  render: (index: number) => ReactNode;
}

/**
 * Glissement latéral d'une vue à l'autre, dans le sens de la navigation : l'onglet de
 * droite entre par la droite. La vue sortante reste montée le temps de l'animation, ce
 * qui lui laisse afficher ses dernières mesures plutôt que de disparaître d'un coup.
 *
 * Chaque vue porte son propre défilement : revenir sur un onglet le retrouve où il
 * était plutôt qu'en haut.
 */
export default function SlideDeck({ index, render }: Props) {
  const [current, setCurrent] = useState(index);
  const [leaving, setLeaving] = useState<number | null>(null);
  const [dir, setDir] = useState(1);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (index === current) return;
    setDir(index > current ? 1 : -1);
    setLeaving(current);
    setCurrent(index);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setLeaving(null), SLIDE_MS);
  }, [index, current]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const side = dir > 0 ? "right" : "left";

  return (
    <div className="deck">
      {leaving !== null && (
        <div key={`out-${leaving}`} className={`view out-${side}`} aria-hidden>
          {render(leaving)}
        </div>
      )}
      <div key={`in-${current}`} className={`view in-${side}`}>
        {render(current)}
      </div>
    </div>
  );
}
