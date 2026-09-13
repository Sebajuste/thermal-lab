import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  checkUpdate,
  installUpdate,
  type UpdateInfo,
  type UpdateProgress,
} from "../api";
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
  const [progress, setProgress] = useState<UpdateProgress | null>(null);

  useEffect(() => {
    const off = listen<UpdateProgress>("update-progress", (e) =>
      setProgress(e.payload),
    );
    return () => void off.then((f) => f());
  }, []);

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
      setProgress(null);
    }
  }, [busy, onError]);

  const pct =
    progress && progress.total
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

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
            {busy !== "install"
              ? `Installer la v${update.version} et relancer`
              : pct !== null
                ? `Téléchargement ${pct} %`
                : `Téléchargement ${(progress ? progress.downloaded / 1e6 : 0).toFixed(1)} Mo`}
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

      {busy === "install" && (
        // Barre déterminée quand la taille est annoncée, rayures animées sinon : une
        // barre pleine à 100 % qui ne bouge plus ressemble à un blocage.
        <div
          className={pct === null ? "update-bar unknown" : "update-bar"}
          role="progressbar"
          aria-valuenow={pct ?? undefined}
        >
          <span style={pct === null ? undefined : { width: `${pct}%` }} />
        </div>
      )}

      {update && busy !== "install" && (
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
