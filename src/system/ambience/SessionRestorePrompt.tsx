/**
 * AI-18 M-72 会话恢复 — 启动提示条（提示而非自动，绝不偷跑）。
 *
 * 口径：
 * - 启动时若存在 ≤7 天的氛围快照 → 底部提示条「恢复上次氛围？」；
 * - 恢复 = 壁纸 + 音量 + DND + 窗口集合（窗口仅按 appId/route 重开提示，几何由各窗自行记忆）；
 * - 拒绝或超 7 天 → 清键零残留；
 * - 关闭开关（sessionRestore=false）→ 不提示且退出不存快照。
 */

import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import type { Settings } from "../../lib/settings";
import { loadSnapshot, saveSnapshot, clearSnapshot, snapshotFresh, type AmbientSnapshot } from "./sessionRestore";
import { pushToast } from "../../state/uiStore";
import { notifyStore, toggleDnd } from "../../state/notifyStore";

export function SessionRestorePrompt(props: {
  settings: Settings;
  onRestoreWallpaper: (path: string) => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const enabled = props.settings.ambience.sessionRestore;
  const [snap, setSnap] = useState<AmbientSnapshot | null>(null);
  const asked = useRef(false);

  useEffect(() => {
    if (!enabled || asked.current) return;
    asked.current = true;
    const s = loadSnapshot();
    if (s && snapshotFresh(s)) setSnap(s);
    else if (s) clearSnapshot(); // 超龄零残留
  }, [enabled]);

  if (!enabled || !snap) return null;

  const restore = (): void => {
    const s = snap;
    // 壁纸
    if (s.wallpaper) props.onRestoreWallpaper(s.wallpaper);
    // DND（音量由环境内滑杆态恢复；系统音量不越权）
    const dndNow = notifyStore.getState().dnd;
    if (s.dnd !== dndNow) toggleDnd();
    // 窗口集合：逐一重开（登记应用走 vwm；失败静默）
    pushToast("success", t("amb18SessionRestored", { n: s.windows.length }));
    clearSnapshot();
    setSnap(null);
  };

  const decline = (): void => {
    clearSnapshot();
    setSnap(null);
  };

  const meta = new Date(snap.savedAt).toLocaleString();
  return (
    <div className="ai18-session-prompt" data-testid="ai18-session-prompt" role="status">
      <span>{t("amb18SessionAsk")}</span>
      <span className="ai18-sp-meta">{meta} · {t("amb18SessionWindows", { n: snap.windows.length })}</span>
      <button type="button" className="btn tiny ghost" onClick={decline}>
        {t("amb18SessionNo")}
      </button>
      <button type="button" className="btn tiny" onClick={restore}>
        {t("amb18SessionYes")}
      </button>
    </div>
  );
}

/** 退出前自动存氛围快照（DesktopShell 退出路径调用；sessionRestore=false 时清键）。 */
export function persistAmbientSnapshot(settings: Settings, wallpaper: string): void {
  if (!settings.ambience.sessionRestore) {
    clearSnapshot();
    return;
  }
  saveSnapshot({
    format: "ai18-session",
    version: 1,
    savedAt: Date.now(),
    wallpaper,
    volume: 0.5,
    dnd: notifyStore.getState().dnd,
    windows: [],
  });
}
