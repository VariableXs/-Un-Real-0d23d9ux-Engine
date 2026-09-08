import { useEffect, useRef } from "react";
import { useI18n } from "../../i18n";
import { appAccent } from "../../components/AppGlyphs";
import { focusVwmWin, minimizedOrder, vwmStore, vwmWindowTitle, isTpApp, tpIdOf } from "./vwm";
import { useStore } from "../../lib/store";
import { getThirdApps } from "../launcher/thirdApps";

/**
 * AI-01 M-03 最小化窗口抽屉：
 * - 任务栏上方一枚抽屉图标（徽标 = 最小化数量；无最小化窗口时图标自动隐藏）
 * - 列表按 minimizedAt 降序（最近的最先），含应用色点 + 标题 + 相对时间
 * - 点击还原；Ctrl+Alt+` 呼出（键盘链路）、Esc 关闭、↑↓ 选择、Enter 还原
 * - 只列当前会话仍存活窗口（环境退出即清空，不做历史记录）
 */

function relTime(ms: number, lang: string): string {
  const s = Math.max(1, Math.round((Date.now() - ms) / 1000));
  if (lang === "en") {
    if (s < 60) return `${s}s ago`;
    const m = Math.round(s / 60);
    if (m < 60) return `${m}m ago`;
    return `${Math.round(m / 60)}h ago`;
  }
  if (s < 60) return `${s} 秒前`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} 分钟前`;
  return `${Math.round(m / 60)} 小时前`;
}

export function MinimizedDrawer(props: { open: boolean; onToggle: () => void; onClose: () => void }): React.ReactElement | null {
  const { t, lang } = useI18n();
  const wins = useStore(vwmStore, (s) => s.wins);
  const items = minimizedOrder(wins);
  const listRef = useRef<HTMLDivElement | null>(null);
  const selRef = useRef(-1);

  useEffect(() => {
    if (!props.open) {
      selRef.current = -1;
      return;
    }
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.preventDefault();
        props.onClose();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (items.length === 0) return;
        selRef.current =
          e.key === "ArrowDown"
            ? Math.min(items.length - 1, selRef.current + 1)
            : Math.max(0, selRef.current <= 0 ? items.length - 1 : selRef.current - 1);
        listRef.current?.querySelectorAll(".wf-drawer-item").forEach((el, i) => {
          el.classList.toggle("sel", i === selRef.current);
        });
        return;
      }
      if (e.key === "Enter" && selRef.current >= 0 && items[selRef.current]) {
        e.preventDefault();
        focusVwmWin(items[selRef.current]!.id);
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [props, items]);

  return (
    <div className="wf-drawer-root" data-open={props.open ? "1" : "0"}>
      {items.length > 0 && (
        <button
          type="button"
          className="wf-drawer-trigger"
          aria-label={t("wfDrawerOpen")}
          title={t("wfDrawerOpen")}
          onClick={props.onToggle}
        >
          <span aria-hidden>▦</span>
          <span className="wf-drawer-badge" aria-hidden>
            {items.length}
          </span>
        </button>
      )}
      {props.open && (
        <div className="wf-drawer-panel" role="dialog" aria-label={t("wfDrawerOpen")}>
          <div className="wf-drawer-cap">{t("wfDrawerTitle")}</div>
          {items.length === 0 ? (
            <div className="wf-drawer-empty">{t("wfDrawerEmpty")}</div>
          ) : (
            <div ref={listRef} className="wf-drawer-list">
              {items.map((w) => (
                <button
                  key={w.id}
                  type="button"
                  className="wf-drawer-item"
                  onClick={() => {
                    focusVwmWin(w.id);
                    props.onClose();
                  }}
                >
                  <span className="vwm-app-dot" aria-hidden style={{ background: appAccent(w.app) }} />
                  <span className="wf-drawer-item-title">
                    {isTpApp(w.app)
                      ? getThirdApps().find((a) => a.id === tpIdOf(w.app))?.name ?? vwmWindowTitle(w.app)
                      : vwmWindowTitle(w.app)}
                  </span>
                  <span className="wf-drawer-item-time">{relTime(w.minimizedAt ?? Date.now(), lang)}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
