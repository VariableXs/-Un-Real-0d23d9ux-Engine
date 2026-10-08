import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Pin, PinOff, Search, Terminal, Settings2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { uiStore } from "../../state/uiStore";
import { openVwmApp } from "../windows/vwm";
import { applySnap } from "../windows/snap";
import {
  executeCommand,
  loadPinned,
  loadStats,
  pinCommand,
  queryCommands,
  serializePinned,
  serializeStats,
  STORAGE_KEYS,
  unpinCommand,
} from "../../lib/commands/registry";
import type { CommandHit } from "../../lib/commands/types";
import { ensureBuiltinRegistered, setCommandHandler } from "../../lib/commands/builtin";
import { inlineCalc } from "../../lib/quick/inlineCalc";
import { isTimestampQuery, timestampCard } from "../../lib/quick/timestamps";
import { pushToast } from "../../state/uiStore";

/**
 * AI-07 · N-13 命令面板（Ctrl+K，三类一框）：
 * - 无前缀：全部（应用优先）；`>` 命令模式；`?` 设置模式
 * - 学习排序（本地频次+新近度）；钉选置顶（V-45 同款 8 位）
 * - 结果卡：V-41 内联计算、V-49 时间戳速插（now/ts）
 * - V-47 联动：隐私「不记录」模式下统计仅存内存（不落 localStorage）
 * - 全局热键（winman "commandPalette" → sys://open-palette）+ 应用内 Ctrl+K
 */
export function CommandPalette(): React.ReactElement | null {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [input, setInput] = useState("");
  const [sel, setSel] = useState(0);
  const [pinnedVersion, setPinnedVersion] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // ---- 初始化：注册内置命令 + 载入统计/钉选（一次）----
  const initedRef = useRef(false);
  if (!initedRef.current) {
    initedRef.current = true;
    ensureBuiltinRegistered();
    try {
      loadStats(localStorage.getItem(STORAGE_KEYS.stats));
      loadPinned(localStorage.getItem(STORAGE_KEYS.pinned));
    } catch {
      /* 无 localStorage（测试环境）如实跳过 */
    }
    installHandlers();
  }

  // ---- 查询解析（前缀模式）----
  const mode = input.startsWith(">") ? "commands" : input.startsWith("?") ? "settings" : "all";
  const query = mode === "all" ? input : input.slice(1);

  const hits = useMemo<CommandHit[]>(() => {
    if (!open) return [];
    return queryCommands(query, { mode, resolveTitle: (c) => t(c.titleKey) }).slice(0, 12);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, mode, open, pinnedVersion, t]);

  // 结果卡：内联计算 / 时间戳（无前缀且无命令命中时让位给卡片）
  const calcResult = useMemo(() => (mode === "all" && !input.startsWith(">") ? inlineCalc(input) : null), [input, mode]);
  const tsCard = useMemo(() => (mode === "all" && isTimestampQuery(input) ? timestampCard() : null), [input, mode]);

  // ---- 持久化（V-47：设置不记录模式时跳过——settings 键由 DesktopShell 注入）----
  const persistStats = useCallback(() => {
    try {
      if (localStorage.getItem("variable.historyPolicy") === "off") return;
      localStorage.setItem(STORAGE_KEYS.stats, serializeStats());
      localStorage.setItem(STORAGE_KEYS.pinned, serializePinned());
    } catch {
      /* 忽略 */
    }
  }, []);

  const close = useCallback(() => {
    setOpen(false);
    setInput("");
    setSel(0);
    persistStats();
  }, [persistStats]);

  const runHit = useCallback(
    async (hit: CommandHit) => {
      const chained = await executeCommand(hit.command.id);
      if (chained === false) return; // 链式参数：面板停留
      close();
    },
    [close],
  );

  // ---- 全局事件 + 应用内 Ctrl+K ----
  useEffect(() => {
    const un = listen("sys://open-palette", () => {
      setOpen((o) => !o); // toggle（再按关闭）
      setInput("");
      setSel(0);
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
        setInput("");
        setSel(0);
      }
      if (e.key === "Escape" && open) {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      void un.then((f) => f()).catch(() => {});
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open, close]);

  useEffect(() => {
    if (open) {
      setSel(0);
      window.setTimeout(() => inputRef.current?.focus(), 30);
    }
  }, [open]);

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-idx="${sel}"]`)?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  if (!open) return null;

  const onInputKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, hits.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (calcResult) {
        void navigator.clipboard.writeText(calcResult.formatted).catch(() => {});
        pushToast("info", t("calcCopied"), calcResult.formatted);
        close();
      } else if (tsCard) {
        void navigator.clipboard.writeText(tsCard.iso).catch(() => {});
        pushToast("info", t("calcCopied"), tsCard.iso);
        close();
      } else if (hits[sel]) {
        void runHit(hits[sel]);
      }
    }
  };

  return (
    <div
      className="var-palette-scrim"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="var-palette" role="dialog" aria-label={t("cmdPalette")}>
        <div className="var-palette-input-row">
          {mode === "settings" ? <Settings2 size={16} /> : mode === "commands" ? <Terminal size={16} /> : <Search size={16} />}
          <input
            ref={inputRef}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              setSel(0);
            }}
            onKeyDown={onInputKey}
            placeholder={t("palettePlaceholder")}
            spellCheck={false}
            data-testid="palette-input"
          />
          <span className="var-palette-hint">{t("paletteHint")}</span>
        </div>

        {/* V-41 内联计算结果卡 */}
        {calcResult && (
          <button
            type="button"
            className="var-palette-card"
            onClick={() => {
              void navigator.clipboard.writeText(calcResult.formatted).catch(() => {});
              pushToast("info", t("calcCopied"), calcResult.formatted);
              close();
            }}
          >
            <span className="var-palette-expr">{calcResult.expr}</span>
            <span className="var-palette-value">= {calcResult.formatted}</span>
          </button>
        )}

        {/* V-49 时间戳结果卡 */}
        {tsCard && (
          <div className="var-palette-card var-palette-ts" data-testid="ts-card">
            <span className="var-palette-expr">{tsCard.human}</span>
            <span className="var-palette-value">{tsCard.iso}</span>
            <span>Unix {tsCard.unixSeconds} / {tsCard.unixMillis}ms</span>
            <span>{tsCard.relative}</span>
          </div>
        )}

        {/* 命令结果 */}
        <div className="var-palette-list" ref={listRef}>
          {hits.length === 0 && !calcResult && !tsCard && <div className="var-palette-empty">{t("paletteEmpty")}</div>}
          {hits.map((h, i) => (
            <div
              key={h.command.id}
              data-idx={i}
              className={`var-palette-item${i === sel ? " sel" : ""}${h.pinned ? " pinned" : ""}`}
              onMouseMove={() => setSel(i)}
              onClick={() => void runHit(h)}
            >
              <span className="var-palette-item-title">{t(h.command.titleKey)}</span>
              <span className="var-palette-item-id">{h.command.id}</span>
              <button
                type="button"
                className="var-palette-pin"
                title={h.pinned ? t("paletteUnpin") : t("palettePin")}
                onClick={(e) => {
                  e.stopPropagation();
                  if (h.pinned) unpinCommand(h.command.id);
                  else {
                    const ok = pinCommand(h.command.id);
                    if (!ok) pushToast("info", t("cmdPalette"), t("palettePinFull"));
                  }
                  setPinnedVersion((v) => v + 1);
                }}
              >
                {h.pinned ? <Pin size={13} /> : <PinOff size={13} />}
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/** 安装命令动作处理器（应用/动作直达——UI 层上下文）。 */
function installHandlers(): void {
  const apps: Record<string, () => void> = {
    "app.notes": () => openVwmApp("notes"),
    "app.calc": () => openVwmApp("calc"),
    "app.calendar": () => openVwmApp("calendar"),
    "app.snapshot": () => openVwmApp("snapshot"),
    "app.clipboard": () => openVwmApp("clipboard"),
    "app.explorer": () => openVwmApp("explorer"),
    "app.recycle": () => openVwmApp("recycle"),
    "app.taskman": () => openVwmApp("taskman"),
    "app.write": () => window.dispatchEvent(new CustomEvent("variable:open-app", { detail: "write" })),
    "app.mind": () => window.dispatchEvent(new CustomEvent("variable:open-app", { detail: "mind" })),
    "app.code": () => window.dispatchEvent(new CustomEvent("variable:open-app", { detail: "code" })),
    "app.fate": () => window.dispatchEvent(new CustomEvent("variable:open-app", { detail: "fate" })),
  };
  for (const [id, fn] of Object.entries(apps)) setCommandHandler(id, fn);

  const actions: Record<string, () => void> = {
    "act.snapLeft": () => void applySnap("left"),
    "act.snapRight": () => void applySnap("right"),
    "act.snapUp": () => void applySnap("up"),
    "act.snapDown": () => void applySnap("down"),
    "act.showDesktop": () => uiStore.setState({ startOpen: false }),
    "act.minimizeAll": () => window.dispatchEvent(new Event("variable:minimize-all")),
    "act.dnd": () => window.dispatchEvent(new Event("variable:toggle-dnd")),
    "act.quickPanel": () => uiStore.setState({ quickOpen: true, startOpen: false }),
    "act.settings": () => uiStore.setState({ settingsOpen: true, startOpen: false }),
    // 效率中枢动作
    "eff.clipboardHistory": () => openVwmApp("clipboard"),
    "eff.searchAll": () => uiStore.setState({ startOpen: true }),
    "eff.inlineCalc": () => openVwmApp("calc"),
    "eff.timestamp": () => {
      const card = timestampCard();
      void navigator.clipboard.writeText(card.iso).catch(() => {});
      pushToast("info", "Timestamp", card.iso);
    },
    "eff.snipRegion": () => window.dispatchEvent(new Event("variable:snip-region")),
    "eff.ocrScreen": () => window.dispatchEvent(new Event("variable:snip-ocr")),
    "eff.qrCode": () => window.dispatchEvent(new Event("variable:qr-open")),
    "eff.purePaste": () => window.dispatchEvent(new Event("variable:pure-paste")),
    "eff.quickNote": () => window.dispatchEvent(new Event("variable:quick-note")),
    "eff.shortcutsHub": () => uiStore.setState({ settingsOpen: true }),
    "eff.macroEngine": () => window.dispatchEvent(new Event("variable:macro-open")),
    "eff.cheatsheetMd": () => window.dispatchEvent(new CustomEvent("variable:cheatsheet", { detail: "md" })),
    "eff.cheatsheetHtml": () => window.dispatchEvent(new CustomEvent("variable:cheatsheet", { detail: "html" })),
    "eff.historyPrivacy": () => uiStore.setState({ settingsOpen: true }),
    // SINGULARITY-100 奇点中枢
    "singu.hub": () =>
      window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "singu-hub" } })),
    // NOVA-200 新星中枢
    "nova.hub": () =>
      window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "nova-hub" } })),
  };
  for (const [id, fn] of Object.entries(actions)) setCommandHandler(id, fn);
}
