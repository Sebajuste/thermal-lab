import type { PowerState, ProfileId, ProfileInfo } from "../api";
import { t } from "../i18n";

interface Props {
  profiles: ProfileInfo[];
  /** Le profil que l'interrupteur applique : une préférence. */
  selected: ProfileId;
  /** La machine telle qu'elle est relue : ce qui est réellement appliqué. */
  power: PowerState | null;
  busy: boolean;
  onSelect: (id: ProfileId) => void;
}

/**
 * Ce qu'un profil fait, levier par levier, en une ligne. Le plafond à 99 % n'est pas
 * mentionné : il ne sert qu'à interdire le turbo, que « turbo coupé » dit déjà.
 */
function summary(p: ProfileInfo): string {
  const parts: string[] = [];
  if (p.boostMode === 0) parts.push(t.turboOff);
  if (p.throttleMax < 99) parts.push(t.capAt(p.throttleMax));
  parts.push(p.epp === null ? t.eppKept : t.eppAt(p.epp));
  return parts.join(" · ");
}

/**
 * Le choix du profil, à côté de l'interrupteur et non à sa place : l'interrupteur dit si
 * l'on intervient, ce sélecteur dit comment.
 *
 * Deux informations distinctes s'y lisent. La sélection est l'intention ; la couleur
 * « appliqué » vient de l'état relu dans le schéma, jamais de la sélection — un outil
 * tiers peut avoir changé la machine depuis. Un bridage qu'aucun profil ne décrit est
 * signalé comme tel plutôt que rangé sous le profil choisi.
 */
export default function ProfilePicker({ profiles, selected, power, busy, onSelect }: Props) {
  const current = profiles.find((p) => p.id === selected);
  const external = power?.optimized === true && power.profile === null;

  return (
    <div className="profiles" title={t.profileHint}>
      <div className="seg" role="radiogroup" aria-label={t.profileHint}>
        {profiles.map((p) => {
          const applied = power?.profile === p.id;
          return (
            <button
              key={p.id}
              type="button"
              role="radio"
              aria-checked={p.id === selected}
              className={[
                p.id === selected ? "active" : "",
                applied ? "applied" : "",
              ].join(" ")}
              disabled={busy}
              title={summary(p)}
              onClick={() => p.id !== selected && onSelect(p.id)}
            >
              {t.profileName(p.id)}
            </button>
          );
        })}
      </div>
      <span className={`profile-note ${external ? "warn" : ""}`}>
        {external ? t.externalCap : current ? summary(current) : ""}
      </span>
    </div>
  );
}
