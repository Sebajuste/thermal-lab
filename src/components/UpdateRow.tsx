import { useCallback, useState } from "react";
import { checkUpdate, installUpdate, type UpdateInfo } from "../api";
import Icon from "./Icon";

interface Props {
  /** Version du paquet installé, telle que la connaît Rust. */
  version: string;
  update: UpdateInfo | null;
  onFound: (u: UpdateInfo | null) => void;
  onError: (message: string) => void;
}

/**
 * La mise à jour est proposée, jamais imposée : l'application tourne élevée et modifie
 * le schéma d'alimentation, la remplacer sous les pieds de quelqu'un en pleine mesure
 * serait au mieux impoli. Le bouton dit donc ce qu'il va faire — installer *et*
 * relancer — parce que le panneau disparaîtra.
 */
export default function UpdateRow({ version, update, onFound, onError }: Props) {
  const [busy, setBusy] = useState<"check" | "install" | null>(null);
  // Distinct de `update === null` : au premier rendu on ne sait rien, après une
  // recherche infructueuse on sait que rien n'attend. Les deux méritent un mot différent.
  const [checked, setChecked] = useState(false);

  const check = useCallback(async () => {
    if (busy) return;
    setBusy("check");
    try {
      onFound(await checkUpdate());
      setChecked(true);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  }, [busy, onFound, onError]);

  const install = useCallback(async () => {
    if (busy) return;
    setBusy("install");
    try {
      // Ne rend la main qu'en cas d'échec : au succès, le processus est remplacé.
      await installUpdate();
    } catch (e) {
      onError(String(e));
      setBusy(null);
    }
  }, [busy, onError]);

  return (
    <div className="update">
      <div className="update-line">
        <span className="mono" title="Version installée">
          v{version}
        </span>
        {update ? (
          <button
            className="ghost"
            disabled={busy !== null}
            onClick={() => void install()}
            title={update.notes ?? undefined}
          >
            {busy === "install"
              ? "Installation…"
              : `Installer la v${update.version} et relancer`}
          </button>
        ) : (
          <button
            className="ghost"
            disabled={busy !== null}
            onClick={() => void check()}
          >
            {busy === "check"
              ? "Recherche…"
              : checked
                ? "À jour — chercher encore"
                : "Rechercher une mise à jour"}
          </button>
        )}
      </div>

      {update && (
        <div className="banner inline">
          <Icon name="launch" size={14} />
          <span>
            La v{update.version} remplacera la v{update.current}. L'application se
            ferme et se rouvre ; le schéma d'alimentation reste en l'état.
          </span>
        </div>
      )}
    </div>
  );
}
