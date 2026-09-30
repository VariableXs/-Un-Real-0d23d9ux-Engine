import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Pin, PinOff, X } from "lucide-react";
import { useI18n } from "../../i18n";
import {
  addSticky, clearUnpinned, pinSticky, removeSticky, setInputOpen,
  restoreStickies, tickStickies, useStickies, moveSticky, type StickyCorner,
} from "./stickies";

/**
 * M-17 任务栏便签速贴（AI-03 任务栏与托盘组）：
 * Ctrl+Alt+S（后端 sys://quick-sticky）或任务栏空区菜单唤出单行输入，
 * 回车生成桌面角便签：纯文本、10 分钟淡隐（500ms 动画）、点击钉住。
 */

const CORNERS: StickyCorner[] = ["tl", "tr", "bl", "br"];

export function StickyNotes(): React.ReactElement {
  const { t } = useI18n();
  const { items, inputOpen } = useStickies();
  const [text, setText] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);
  const inputWrapRef = useRef<HTMLDivElement | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);

  // 快捷键（后端注册 ctrl+alt+s → sys://quick-sticky）
  useEffect(() => {
    const un = listen("sys://quick-sticky", () => setInputOpen(true));
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, []);

  // 重启恢复钉住项 + 淡隐心跳（500ms 粒度足够）
  useEffect(() => {
    restoreStickies();
    const id = window.setInterval(() => tickStickies(), 500);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    if (inputOpen) inputRef.current?.focus();
  }, [inputOpen]);

  useEffect(() => {
    if (!confirmClear) return;
    const id = window.setTimeout(() => setConfirmClear(false), 3000);
    return () => window.clearTimeout(id);
  }, [confirmClear]);

  useEffect(() => {
    if (!inputOpen) return;
    const onDown = (e: MouseEvent): void => {
      if (!inputWrapRef.current?.contains(e.target as Node)) setInputOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [inputOpen]);

  const submit = (): void => {
    const r = addSticky(text);
    if (r) setText("");
    setInputOpen(false);
  };

  return (
    <>
      {inputOpen && (
        <div className="sticky-input card-pop" ref={inputWrapRef} role="dialog" aria-label={t("tbStickyTitle")}>
          <input
            ref={inputRef}
            value={text}
            maxLength={500}
            placeholder={t("tbStickyHint")}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") submit();
              if (e.key === "Escape") setInputOpen(false);
            }}
          />
          <button type="button" className="btn primary small" onClick={submit}>{t("tbStickyAdd")}</button>
        </div>
      )}
      {items.map((s) => (
        <div
          key={s.id}
          className={`sticky-note${s.fading ? " fading" : ""}${s.pinned ? " pinned" : ""}`}
          data-corner={s.corner}
          role="note"
          aria-label={s.text}
        >
          <div className="sticky-note-bar">
            <button
              type="button"
              className="icon-btn tiny"
              aria-label={s.pinned ? t("tbStickyUnpin") : t("tbStickyPin")}
              title={s.pinned ? t("tbStickyUnpin") : t("tbStickyPin")}
              onClick={() => pinSticky(s.id)}
            >
              {s.pinned ? <Pin size={11} /> : <PinOff size={11} />}
            </button>
            <div className="sticky-corner-picker">
              {CORNERS.map((c) => (
                <button key={c} type="button" aria-label={c} onClick={() => moveSticky(s.id, c)}>{""}</button>
              ))}
            </div>
            <button
              type="button"
              className="icon-btn tiny"
              aria-label={t("close")}
              onClick={() => removeSticky(s.id)}
            >
              <X size={11} />
            </button>
          </div>
          <p className="sticky-note-text">{s.text}</p>
        </div>
      ))}
      {items.some((s) => !s.pinned) && (
        <button
          type="button"
          className="sticky-clear icon-btn small"
          aria-label={t("tbStickyClearAll")}
          title={confirmClear ? t("tbStickyClearConfirm") : t("tbStickyClearAll")}
          onClick={() => {
            if (confirmClear) {
              clearUnpinned();
              setConfirmClear(false);
            } else {
              setConfirmClear(true);
            }
          }}
        >
          <X size={13} />
        </button>
      )}
    </>
  );
}
