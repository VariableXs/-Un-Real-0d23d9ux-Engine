import { useEffect, useRef, useState } from "react";
import { getAllWindows, Window } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { AppWindow } from "lucide-react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import { vwmWindowTitle } from "./vwm";
import { focusVwmWin, vwmStore } from "./vwm";
import { CloseLight } from "../../components/CloseLight";

/**
 * Win+Tab 多窗口切换器（批次E-6，规格 4.3.6，可选功能）：
 * - 后端 super+tab 全局快捷键 → `sys://wintab` 事件 → 唤起本切换器；
 * - 列出全部 Variable 窗口（桌面 + 四软件 + 文件管理器等），
 *   VWM 化后并入桌面层内托管的虚拟窗口（含同软件多开实例），
 *   ↑/↓ 选择、Enter/点击聚焦、Esc 关闭；
 * - 设置 `winTabSwitcher=false` 时事件被忽略（Windows 自己的 Task View 不受影响 ——
 *   全局键已被 Variable 注册，此时 Win+Tab 仅切换 Variable 窗口，如实降级）。
 */

interface WinEntry {
  key: string;
  name: string;
  /** OS 级窗口（桌面 / 文件管理器等） */
  os?: Window;
  /** VWM 虚拟窗口实例 id */
  vwmId?: string;
}

const KNOWN_LABELS: Record<string, { zh: string; en: string }> = {
  desktop: { zh: "Variable 桌面", en: "Variable Desktop" },
  write: { zh: "Variable Write", en: "Variable Write" },
  mindmap: { zh: "Variable Mind", en: "Variable Mind" },
  project: { zh: "Variable Code", en: "Variable Code" },
  fate: { zh: "Variable Fate", en: "Variable Fate" },
  explorer: { zh: "文件管理器", en: "File Manager" },
};

function labelFor(label: string, lang: Lang): string {
  const known = KNOWN_LABELS[label];
  if (known) return known[lang === "zh-TW" ? "zh" : lang];
  return label;
}

export function WintabSwitcher(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const [open, setOpen] = useState(false);
  const [wins, setWins] = useState<WinEntry[]>([]);
  const [sel, setSel] = useState(0);
  const listRef = useRef<WinEntry[]>([]);

  useEffect(() => {
    const un = listen("sys://wintab", () => {
      void (async () => {
        const all = await getAllWindows().catch(() => [] as Window[]);
        const entries: WinEntry[] = all.map((w) => ({
          key: w.label,
          name: labelFor(w.label, lang),
          os: w,
        }));
        // VWM 虚拟窗口并入（Z 序从高到低，最近聚焦的排前面）
        const vwins = [...vwmStore.getState().wins].sort((a, b) => b.z - a.z);
        for (const w of vwins) {
          entries.push({ key: w.id, name: vwmWindowTitle(w.app), vwmId: w.id });
        }
        if (entries.length === 0) return;
        listRef.current = entries;
        setWins(entries);
        setSel(0);
        setOpen((o) => {
          if (o) {
            // 再次 Win+Tab = 关闭（toggle）
            return false;
          }
          return true;
        });
      })();
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lang]);

  const focus = (w: WinEntry): void => {
    setOpen(false);
    if (w.vwmId) {
      focusVwmWin(w.vwmId);
      return;
    }
    if (w.os) {
      void w.os.show().catch(() => {});
      void w.os.unminimize().catch(() => {});
      void w.os.setFocus().catch(() => {});
    }
  };

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
      } else if (e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey)) {
        e.preventDefault();
        setSel((s) => (wins.length === 0 ? 0 : (s + 1) % wins.length));
      } else if (e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey)) {
        e.preventDefault();
        setSel((s) => (wins.length === 0 ? 0 : (s - 1 + wins.length) % wins.length));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const w = wins[sel];
        if (w) focus(w);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, wins, sel]);

  if (!open) return null;

  return (
    <div className="wintab-overlay" role="dialog" aria-label={t("wintabTitle")} onClick={() => setOpen(false)}>
      <div className="wintab-panel" onClick={(e) => e.stopPropagation()}>
        <div className="wintab-head-row">
          <p className="wintab-head">{t("wintabTitle")}</p>
          <CloseLight onClose={() => setOpen(false)} />
        </div>
        {wins.map((w, i) => (
          <button
            key={w.key}
            type="button"
            className={`wintab-row${i === sel ? " selected" : ""}`}
            onMouseEnter={() => setSel(i)}
            onClick={() => focus(w)}
          >
            <AppWindow size={16} strokeWidth={1.7} />
            <span>{w.name}</span>
          </button>
        ))}
        <p className="dim small wintab-hint">{t("wintabHint")}</p>
      </div>
    </div>
  );
}
