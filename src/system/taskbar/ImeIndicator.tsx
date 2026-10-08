import { useEffect, useRef, useState } from "react";
import { Languages } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * F-5.3 输入法指示器（任务栏）：当前键盘布局 + 中英态（1s 只读轮询）。
 * 点击弹语言列表；切换仅作用于 Variable 内输入（KLF_SETFORPROCESS），
 * 不改变宿主全局布局——弹层文案如实标注（规格 21.5）。
 */
export function ImeIndicator(): React.ReactElement {
  const { t } = useI18n();
  const [status, setStatus] = useState<import("../../lib/ipc").Shell.ImeStatus | null>(null);
  const [layouts, setLayouts] = useState<import("../../lib/ipc").Shell.ImeLayout[] | null>(null);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    const tick = (): void => {
      void ipc
        .imeStatus()
        .then((s) => {
          if (!alive) return;
          // 变更检测：中英态与布局不变则保持原引用（避免每秒空转重渲染）
          setStatus((prev) =>
            prev !== null && s !== null && prev.chinese === s.chinese && prev.langId === s.langId ? prev : s,
          );
        })
        .catch(() => {
          if (alive) setStatus((prev) => (prev === null ? prev : null));
        });
    };
    tick();
    const id = window.setInterval(tick, 1000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    void ipc
      .imeList()
      .then(setLayouts)
      .catch(() => setLayouts([]));
    const onDown = (e: MouseEvent): void => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  // 指示文案：中文态显示「中」，英文态显示「英」，未知显示语言 ID
  const label =
    status === null
      ? "?"
      : status.chinese === true
        ? "中"
        : status.chinese === false
          ? "英"
          : status.langId.slice(0, 2).toUpperCase();

  return (
    <div ref={ref} className="ime-ind">
      <button
        type="button"
        className={`tb-btn ime-btn${open ? " active" : ""}`}
        aria-label={t("imeTitle")}
        title={status ? `${t("imeTitle")} · ${status.langId}` : t("imeTitle")}
        onClick={() => setOpen(!open)}
      >
        <Languages size={15} strokeWidth={1.7} />
        <span className="ime-label">{label}</span>
      </button>
      {open && (
        <div className="ime-pop card-pop" role="dialog" aria-label={t("imeTitle")}>
          <p className="dim small ime-note">{t("imeScopeNote")}</p>
          {layouts === null && <p className="dim small">…</p>}
          {layouts !== null && layouts.length === 0 && <p className="dim small">{t("imeNone")}</p>}
          {(layouts ?? []).map((l) => (
            <button
              key={l.langId}
              type="button"
              className={`ime-row${status?.langId === l.langId ? " sel" : ""}`}
              onClick={() => {
                void ipc
                  .imeSwitch(l.langId)
                  .then(() => setOpen(false))
                  .catch((e) => pushToast("error", t("imeTitle"), errMessage(e).message));
              }}
            >
              <span>{l.name}</span>
              <span className="dim small">{l.langId}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
