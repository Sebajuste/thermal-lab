import { t } from "../i18n";
import Icon, { type IconName } from "./Icon";

export type TabId = "live" | "compare" | "system";

const TABS: { id: TabId; icon: IconName; label: () => string }[] = [
  { id: "live", icon: "live", label: () => t.tabLive },
  { id: "compare", icon: "compare", label: () => t.tabCompare },
  { id: "system", icon: "system", label: () => t.tabSystem },
];

/** L'ordre de la barre : c'est lui qui donne le sens du glissement d'une vue à l'autre. */
export const TAB_IDS: TabId[] = TABS.map((t) => t.id);

/**
 * Navigation en bas de fenêtre, sur icônes seules — trois destinations tiennent dans la
 * largeur sans étiquette, et le libellé reste accessible au survol comme au lecteur
 * d'écran. Le panneau étant ouvert puis refermé en quelques secondes, chaque ligne de
 * texte économisée est une ligne de mesure gagnée.
 */
export default function Tabs({
  active,
  onSelect,
}: {
  active: TabId;
  onSelect: (t: TabId) => void;
}) {
  return (
    <nav className="tabbar">
      {TABS.map(({ id, icon, label }) => {
        const text = label();
        return (
          <button
            key={id}
            className={`tab ${active === id ? "active" : ""}`}
            onClick={() => onSelect(id)}
            title={text}
            aria-label={text}
            aria-current={active === id}
          >
            <Icon name={icon} size={20} />
          </button>
        );
      })}
    </nav>
  );
}
