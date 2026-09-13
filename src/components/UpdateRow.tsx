import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  checkUpdate,
  installUpdate,
  type UpdateInfo,
  type UpdateProgress,
} from "../api";
import { t } from "../i18n";
import Icon from "./Icon";

interface Props {
  /** Version du paquet installé, telle que la connaît Rust. */
  version: string;
  /** Le mode automatique est coché : la mise à jour n'attend plus qu'un panneau fermé. */
  auto: boolean;
  update: UpdateInfo | null;
  onFound: (u: UpdateInfo | null) => void;
  onError: (message: string) => void;
}

/**
 * La mise à jour est proposée, jamais imposée : l'application tourne élevée et modifie
 * le schéma d'alimentation, la remplacer sous les pieds de quelqu'un en pleine mesure
 * serait au mieux impoli. Le bouton dit donc ce qu'il va faire — installer *et*
 * relancer — parce que le panneau disparaîtra.
 *
 * Cochée, l'option automatique ne retire pas le bouton : elle installe panneau fermé, et
 * quelqu'un qui vient de voir la version disponible n'a aucune raison d'attendre six
 * heures pour l'obtenir.
 */
export default function UpdateRow({
  version,
  auto,
  update,
  onFound,
  onError,
}: Props) {
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

  // Des octets qui arrivent sans qu'on ait cliqué : la veille installe. Le panneau vient
  // d'être rouvert au milieu du téléchargement — rare, mais afficher un bouton
  // « Installer » pendant ce temps proposerait un second téléchargement du même paquet,
  // que Rust refuserait.
  const background = busy === null && progress !== null;
  const installing = busy === "install" || background;

  const pct =
    progress && progress.total
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <div className="update">
      <div className="update-line">
        <span className="mono" title={t.installedVersion}>
          v{version}
        </span>
        {update ? (
          <button
            className="ghost"
            disabled={busy !== null || background}
            onClick={() => void install()}
            title={update.notes ?? undefined}
          >
            {!installing
              ? t.installAndRestart(update.version)
              : pct !== null
                ? t.downloadingPct(pct)
                : t.downloadingMb(
                    (progress ? progress.downloaded / 1e6 : 0).toFixed(1),
                  )}
          </button>
        ) : (
          <button
            className="ghost"
            disabled={busy !== null}
            onClick={() => void check()}
          >
            {busy === "check"
              ? t.checking
              : checked
                ? t.upToDate
                : t.checkForUpdate}
          </button>
        )}
      </div>

      {installing && (
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

      {update && !installing && (
        <div className="banner inline">
          <Icon name="launch" size={14} />
          <span>
            {t.updateNotice(update.version, update.current)}
            {auto && t.updateAutoNotice}
          </span>
        </div>
      )}
    </div>
  );
}
