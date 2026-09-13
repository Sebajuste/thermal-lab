import type { PointerEvent } from "react";
import { startDrag } from "../api";
import Icon from "./Icon";

interface Props {
  /** Pastille d'état : la même couleur que l'icône de la zone de notification. */
  optimized: boolean;
  pinned: boolean;
  onTogglePin: () => void;
  onClose: () => void;
}

/**
 * La fenêtre est sans bordure : cette barre lui rend ce que Windows ne dessine plus,
 * c'est-à-dire une prise pour la déplacer et une croix.
 *
 * La croix masque le panneau, elle ne quitte pas l'application — le libellé au survol
 * le dit, parce que rien dans un bouton de fermeture ne le laisse deviner.
 */
export default function TitleBar({ optimized, pinned, onTogglePin, onClose }: Props) {
  // Bouton gauche seulement, et pas quand le geste commence sur une commande : la barre
  // est aussi le parent des deux boutons.
  const grab = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    void startDrag();
  };

  return (
    <div
      className="titlebar"
      onPointerDown={grab}
      title="Glisser pour déplacer"
    >
      <span
        className={`state-dot ${optimized ? "on" : "off"}`}
        title={optimized ? "Turbo bridé" : "Turbo libre"}
      />
      <span className="titlebar-name">Thermal Lab</span>
      <button
        className={`chrome ${pinned ? "active" : ""}`}
        onClick={onTogglePin}
        aria-pressed={pinned}
        aria-label="Épingler le panneau"
        title={
          pinned
            ? "Détacher"
            : "Épingler"
        }
      >
        <Icon name="pin" size={15} />
      </button>
      <button
        className="chrome close"
        onClick={onClose}
        aria-label="Masquer le panneau"
        title="Masquer le panneau"
      >
        <Icon name="close" size={15} />
      </button>
    </div>
  );
}
