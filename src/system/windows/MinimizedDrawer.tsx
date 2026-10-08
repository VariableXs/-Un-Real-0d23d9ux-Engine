import { useEffect, useRef } from "react";
import { EyeOff } from "lucide-react";
import { useI18n } from "../../i18n";
import { appAccent } from "../../components/AppGlyphs";
import { focusVwmWin, minimizedOrder, unhideVwmWin, vwmStore, vwmWindowTitle, isTpApp, tpIdOf } from "./vwm";
import { useStore } from "../../lib/store";
import { getThirdApps } from "../launcher/thirdApps";

/**
 * AI-01 M-03 最小化窗口抽屉 + 批次F 隐藏窗口分区：
 * - 任务栏上方一枚抽屉图标（徽标 = 最小化+隐藏数量；两者皆无时图标自动隐藏）
 * - 列表按 minimizedAt 降序（最近的最先），含应用色点 + 标题 + 相对时间
 * - 隐藏窗口独立分区（EyeOff 徽标），点击恢复；「全部恢复」一键召回
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
  const hidden = wins.filter((w) => w.hidden);
  // 键盘导航统一序列：最小化在前、隐藏在后（与渲染顺序一致）
  const navList = [...items, ...hidden];
  const total = navList.length;
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
        if (navList.length === 0) return;
        selRef.current =
          e.key === "ArrowDown"
            ? Math.min(navList.length - 1, selRef.current + 1)
            : Math.max(0, selRef.current <= 0 ? navList.length - 1 : selRef.current - 1);
        listRef.current?.querySelectorAll(".wf-drawer-item").forEach((el, i) => {
          el.classList.toggle("sel", i === selRef.current);
        });
        return;
      }
      if (e.key === "Enter" && selRef.current >= 0 && navList[selRef.current]) {
        e.preventDefault();
        const pick = navList[selRef.current]!;
        if (pick.hidden) unhideVwmWin(pick.id);
        else focusVwmWin(pick.id);
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [props, navList]);

  return (
    <div className="wf-drawer-root" data-open={props.open ? "1" : "0"}>
      {total > 0 && (
        <button
          type="button"
          className="wf-drawer-trigger"
          aria-label={t("wfDrawerOpen")}
          title={t("wfDrawerOpen")}
          onClick={props.onToggle}
        >
          <span aria-hidden>{hidden.length > 0 && items.length === 0 ? <EyeOff size={15} /> : "▦"}</span>
          <span className="wf-drawer-badge" aria-hidden>
            {total}
          </span>
        </button>
      )}
      {props.open && (
        <div className="wf-drawer-panel" role="dialog" aria-label={t("wfDrawerOpen")}>
          <div className="wf-drawer-cap">{t("wfDrawerTitle")}</div>
          {items.length === 0 && hidden.length === 0 ? (
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
              {hidden.length > 0 && (
                <div className="wf-drawer-hidden-sec">
                  <div className="wf-drawer-cap">
                    {t("wfHiddenRestore")}
                    <button
                      type="button"
                      className="wf-drawer-restore-all"
                      onClick={() => {
                        for (const h of hidden) unhideVwmWin(h.id);
                        props.onClose();
                      }}
                    >
                      {t("wfHiddenRestoreAll")}
                    </button>
                  </div>
                  {hidden.map((w) => (
                    <button
                      key={w.id}
                      type="button"
                      className="wf-drawer-item"
                      onClick={() => {
                        unhideVwmWin(w.id);
                        props.onClose();
                      }}
                    >
                      <span className="vwm-app-dot" aria-hidden style={{ background: appAccent(w.app) }} />
                      <span className="wf-drawer-item-title">
                        {isTpApp(w.app)
                          ? getThirdApps().find((a) => a.id === tpIdOf(w.app))?.name ?? vwmWindowTitle(w.app)
                          : vwmWindowTitle(w.app)}
                      </span>
                      <span className="wf-drawer-item-time" aria-hidden>
                        <EyeOff size={13} />
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
