import type { ReactElement } from "react";

export type IconName =
  | "live"
  | "compare"
  | "system"
  | "pin"
  | "close"
  | "reset"
  | "alert"
  | "launch";

/**
 * Jeu d'icônes en traits, dessiné ici plutôt qu'importé : sept glyphes ne justifient pas
 * une dépendance, et `currentColor` suffit à les faire suivre l'état du composant qui
 * les porte.
 */
const PATHS: Record<IconName, ReactElement> = {
  // Courbe de mesure : la vue temps réel.
  live: <polyline points="2 12 6 12 9 4 15 20 18 12 22 12" />,
  // Barres inégales : la comparaison de deux états.
  compare: (
    <>
      <line x1="6" y1="21" x2="6" y2="13" />
      <line x1="12" y1="21" x2="12" y2="3" />
      <line x1="18" y1="21" x2="18" y2="9" />
    </>
  ),
  // Puce : ce que la machine offre, et les réglages de l'application.
  system: (
    <>
      <rect x="5" y="5" width="14" height="14" rx="2" />
      <rect x="9.5" y="9.5" width="5" height="5" />
      <line x1="9" y1="2" x2="9" y2="5" />
      <line x1="15" y1="2" x2="15" y2="5" />
      <line x1="9" y1="19" x2="9" y2="22" />
      <line x1="15" y1="19" x2="15" y2="22" />
      <line x1="2" y1="9" x2="5" y2="9" />
      <line x1="2" y1="15" x2="5" y2="15" />
      <line x1="19" y1="9" x2="22" y2="9" />
      <line x1="19" y1="15" x2="22" y2="15" />
    </>
  ),
  pin: (
    <>
      <line x1="9" y1="3" x2="15" y2="3" />
      <line x1="12" y1="3" x2="12" y2="9" />
      <path d="M8 9h8l1.5 5h-11z" />
      <line x1="12" y1="14" x2="12" y2="21" />
    </>
  ),
  close: (
    <>
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </>
  ),
  reset: (
    <>
      <polyline points="2 5 2 11 8 11" />
      <path d="M4.2 15a9 9 0 1 0 1.9-9.3L2 11" />
    </>
  ),
  // Lecture : démarrer l'outil tiers.
  launch: <path d="M7 4.5v15l13-7.5z" />,
  alert: (
    <>
      <path d="M12 4 2.5 20h19z" />
      <line x1="12" y1="10" x2="12" y2="14" />
      <line x1="12" y1="17" x2="12" y2="17" />
    </>
  ),
};

export default function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}
