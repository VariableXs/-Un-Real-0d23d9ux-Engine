import { useEffect, useState } from "react";
import { Music2, Pause, Play } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";

/**
 * F-5.4 媒体控制指示（任务栏）：
 * 轮询 GlobalSystemMediaTransportControls（media_status，2s），
 * 显示曲目/艺术家/播放状态与进度条。探测不到（null）时不渲染，
 * 控制按钮仅展示播放/暂停态（SMTC 只读探测，不向宿主会话注入按键）。
 */
function fmt(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

export function MediaControl(): React.ReactElement | null {
  const { t } = useI18n();
  const [media, setMedia] = useState<import("../../lib/ipc").Shell.MediaStatus | null>(null);
  const [gone, setGone] = useState(false);

  useEffect(() => {
    let alive = true;
    let miss = 0;
    const tick = (): void => {
      void ipc
        .mediaStatus()
        .then((m) => {
          if (!alive) return;
          miss = m ? 0 : miss + 1;
          // 连续 5 次探测不到则隐藏（避免空转渲染）
          if (miss >= 5) {
            setMedia(null);
            setGone(true);
          } else if (m) {
            setMedia(m);
            setGone(false);
          }
        })
        .catch(() => {});
    };
    tick();
    const id = window.setInterval(tick, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  if (gone || !media) return null;

  const pct =
    media.durationSec > 0
      ? Math.min(100, Math.round((media.positionSec / media.durationSec) * 100))
      : 0;
  const playing = media.status === "playing";

  return (
    <div className="tb-media" role="group" aria-label={t("mediaTitle")}>
      <span className={`tb-media-icon${playing ? " breathing" : ""}`} aria-hidden>
        {playing ? <Pause size={14} strokeWidth={1.8} /> : <Play size={14} strokeWidth={1.8} />}
      </span>
      <div className="tb-media-info">
        <span className="tb-media-title" title={media.title}>{media.title || t("mediaUnknown")}</span>
        <span className="tb-media-artist dim" title={media.artist}>{media.artist || t("mediaUnknown")}</span>
        <span className="tb-media-bar" aria-hidden>
          <span className="tb-media-fill" style={{ width: `${pct}%` }} />
        </span>
      </div>
      <span className="tb-media-time dim small">
        {fmt(media.positionSec)}/{fmt(media.durationSec)}
      </span>
      <Music2 size={13} strokeWidth={1.6} className="dim" aria-hidden />
    </div>
  );
}
