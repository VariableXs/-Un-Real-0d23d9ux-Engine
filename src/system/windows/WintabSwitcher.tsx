import { useEffect, useRef, useState } from "react";
import { getAllWindows, Window } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { AppWindow } from "lucide-react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";

/**
 * Win+Tab 多窗口切换器（批次E-6，规格 4.3.6，可选功能）：
 * - 后端 super+tab 全局快捷键 → `sys://wintab` 事件 → 唤起本切换器；
 * - 列出全部 Variable 窗口（桌面 + 四软件 + 文件管理器等），↑/↓ 选择、Enter/点击聚焦、Esc 关闭；
 * - 设置 `winTabSwitcher=false` 时事件被忽略（Windows 自己的 Task View 不受影响 ——
 *   全局键已被 Variable 注册，此时 Win+Tab 仅切换 Variable 窗口，如实降级）。
 */

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
  const [wins, setWins] = useState<Window[]>([]);
  const [sel, setSel] = useState(0);
  const listRef = useRef<Window[]>([]);

  useEffect(() => {
    const un = listen("sys://wintab", () => {
      void (async () => {
        const all = await getAllWindows().catch(() => [] as Window[]);
        if (all.length === 0) return;
        listRef.current = all;
        setWins(all);
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
  }, []);

  const focus = (w: Window): void => {
    setOpen(false);
    void w.show().catch(() => {});
    void w.unminimize().catch(() => {});
    void w.setFocus().catch(() => {});
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
        <p className="wintab-head">{t("wintabTitle")}</p>
        {wins.map((w, i) => (
          <button
            key={w.label}
            type="button"
            className={`wintab-row${i === sel ? " selected" : ""}`}
            onMouseEnter={() => setSel(i)}
            onClick={() => focus(w)}
          >
            <AppWindow size={16} strokeWidth={1.7} />
            <span>{labelFor(w.label, lang)}</span>
          </button>
        ))}
        <p className="dim small wintab-hint">{t("wintabHint")}</p>
      </div>
    </div>
  );
}
