import { useEffect, useRef, useState } from "react";
import { Volume1, Volume2, VolumeX } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { useDnd } from "../../state/notifyStore";

/**
 * M-18 外设音量滚轮规范（AI-03 任务栏与托盘组）：
 * 前端捕获多媒体键 VK_VOLUME_*（不 preventDefault，系统音量该变就变，我们只做显示与联动），
 * 显示 QuickPanel 同款音量浮标，1s 淡出；DND 下照常显示（音量操作不是打扰）。
 */

interface Badge {
  kind: "up" | "down" | "mute";
  at: number;
}

export function VolumeBadge(): React.ReactElement | null {
  const { t } = useI18n();
  const dnd = useDnd();
  const [badge, setBadge] = useState<Badge | null>(null);
  const [volume, setVolume] = useState<number | null>(null);
  const fadeTimer = useRef<number | null>(null);

  useEffect(() => {
    const show = (kind: Badge["kind"]): void => {
      setBadge({ kind, at: Date.now() });
      if (fadeTimer.current !== null) window.clearTimeout(fadeTimer.current);
      fadeTimer.current = window.setTimeout(() => setBadge(null), 1000);
    };
    const onKey = (e: KeyboardEvent): void => {
      // VK_VOLUME_UP 0xAF / DOWN 0xAE / MUTE 0xAD；只监听显示，不改系统行为
      if (e.key === "VolumeUp") show("up");
      else if (e.key === "VolumeDown") show("down");
      else if (e.key === "VolumeMute") show("mute");
      else return;
      void ipc
        .audioGet()
        .then((a) => a.kind === "ok" && setVolume(a.value.volume))
        .catch((err) => console.warn("[volume-badge] read failed", errMessage(err).message));
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      if (fadeTimer.current !== null) window.clearTimeout(fadeTimer.current);
    };
  }, []);

  if (!badge || dnd === undefined) return null;
  const muted = badge.kind === "mute";
  const Icon = muted ? VolumeX : volume !== null && volume < 30 ? Volume1 : Volume2;
  const label = muted ? t("tbVolMuted") : volume !== null ? `${t("tbVol")} ${Math.round(volume)}%` : t("tbVol");

  return (
    <div className="volume-badge card-pop" role="status" aria-live="polite">
      <Icon size={18} strokeWidth={1.7} aria-hidden />
      <span>{label}</span>
      {volume !== null && !muted && (
        <span className="tb-mem" aria-hidden>
          <span style={{ width: `${Math.round(volume)}%` }} />
        </span>
      )}
    </div>
  );
}
