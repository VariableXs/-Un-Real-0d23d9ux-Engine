import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import {
  RUN_ALIASES,
  RUN_NORECORD_KEY,
  addRunHistory,
  loadRunHistory,
  matchRunCandidates,
  classifyRunInput,
  saveRunHistory,
} from "./runParse";
import "../../styles/ai08-run.css";

/**
 * Z-28 运行对话框（全局浮层，类 Windows Win+R）：
 * - 居中浮层卡（modal 形态但非全屏遮罩，不挡桌面点击）
 * - 输入自动聚焦；Enter 执行、Esc 关闭、↑↓ 选补全、Tab/→ 确认补全
 * - 执行路由：内部别名 → onRunAlias(id)；路径/URI/可执行名 → ipc.shellExecute（默认 verb "open"）
 * - 历史 ≤20（localStorage variable:run:history:v1，可清空）；
 *   「不记录敏感路径」开关开启后本次执行不入历史
 * - 无法识别的输入如实报错（不瞎猜、不静默失败）
 * 红线：本组件不挂载任何写操作；别名到动作的映射由宿主（onRunAlias）落地。
 */

export function RunDialog(props: {
  open: boolean;
  onClose: () => void;
  onRunAlias: (id: string) => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(-1);
  const [history, setHistory] = useState<string[]>([]);
  const [noRecord, setNoRecord] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const candidates = useMemo(() => matchRunCandidates(q, history), [q, history]);

  // 打开时复位并聚焦（关闭态返回 null，输入框随开挂载）
  useEffect(() => {
    if (!props.open) return;
    setQ("");
    setSel(-1);
    setHistory(loadRunHistory());
    try {
      setNoRecord(localStorage.getItem(RUN_NORECORD_KEY) === "1");
    } catch {
      setNoRecord(false);
    }
    const raf = requestAnimationFrame(() => inputRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, [props.open]);

  /** 入历史（开关开启时本次执行不入历史）。 */
  const record = (entry: string): void => {
    if (noRecord) return;
    const next = addRunHistory(history, entry);
    setHistory(next);
    saveRunHistory(next);
  };

  const execute = (raw: string): void => {
    const s = raw.trim();
    if (!s) return;
    // 1) 内部别名 → VWM 工具/动作
    const alias = RUN_ALIASES[s.toLowerCase()];
    if (alias !== undefined) {
      record(s);
      props.onRunAlias(alias);
      props.onClose();
      return;
    }
    // 2) 路径 / URI / 可执行名 → 系统 ShellExecute（默认 verb "open"）
    const { kind, normalized } = classifyRunInput(s);
    if (kind === "unknown") {
      pushToast("error", t("rdTitle"), t("rdUnknown"));
      return;
    }
    if (!isTauriRuntime()) {
      pushToast("error", t("rdTitle"), t("rdNoTauri"));
      return;
    }
    record(s);
    ipc
      .shellExecute(normalized)
      .then((r) => {
        if (r.launched) {
          pushToast("success", t("rdTitle"), t("rdLaunched"));
          props.onClose();
        } else {
          pushToast("error", t("rdTitle"), t("rdFail", { code: String(r.errorCode ?? "?") }));
        }
      })
      .catch((e: unknown) => pushToast("error", t("rdTitle"), errMessage(e).message));
  };

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, candidates.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, -1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      execute(sel >= 0 && candidates[sel] ? candidates[sel].text : q);
    } else if (e.key === "Tab") {
      // Tab：确认补全（未选时取第一条）
      const c = sel >= 0 ? candidates[sel] : candidates[0];
      if (c) {
        e.preventDefault();
        setQ(c.text);
        setSel(-1);
      }
    } else if (e.key === "ArrowRight") {
      // →：仅已选中候选时确认补全（避免劫持光标移动）
      const c = sel >= 0 ? candidates[sel] : undefined;
      if (c) {
        e.preventDefault();
        setQ(c.text);
        setSel(-1);
      }
    }
  };

  const toggleNoRecord = (): void => {
    setNoRecord((v) => {
      const nv = !v;
      try {
        localStorage.setItem(RUN_NORECORD_KEY, nv ? "1" : "0");
      } catch {
        /* 存储受限 → 本次会话内生效，不持久 */
      }
      return nv;
    });
  };

  const clearHistory = (): void => {
    setHistory([]);
    saveRunHistory([]);
  };

  /** 卡片级兜底：焦点被复选框等控件夺走时 Esc 仍可关闭。 */
  const onCardKey = (e: React.KeyboardEvent): void => {
    if (e.key === "Escape") {
      e.preventDefault();
      props.onClose();
    }
  };

  if (!props.open) return null;

  return (
    <div className="rd-overlay">
      <div className="rd-card" role="dialog" aria-modal="false" aria-label={t("rdTitle")} onKeyDown={onCardKey}>
        <div className="rd-head">
          <span className="rd-title">{t("rdTitle")}</span>
          <button type="button" className="rd-close" onClick={props.onClose} aria-label={t("rdClose")}>
            ✕
          </button>
        </div>
        <input
          ref={inputRef}
          className="text-input rd-input"
          type="text"
          value={q}
          placeholder={t("rdPlaceholder")}
          aria-label={t("rdPlaceholder")}
          spellCheck={false}
          onChange={(e) => {
            setQ(e.target.value);
            setSel(-1);
          }}
          onKeyDown={onKey}
        />
        {candidates.length > 0 && (
          <ul className="rd-cands" role="listbox" aria-label={t("rdCandidates")}>
            {candidates.map((c, i) => (
              <li
                key={`${c.kind}:${c.text}`}
                className={`rd-cand${i === sel ? " active" : ""}`}
                role="option"
                aria-selected={i === sel}
                onMouseEnter={() => setSel(i)}
                // preventDefault：避免点击抢焦点导致键盘语义中断
                onMouseDown={(e) => {
                  e.preventDefault();
                  execute(c.text);
                }}
              >
                <span className={`rd-cand-kind ${c.kind}`}>{c.kind === "alias" ? t("rdCandAlias") : t("rdCandHistory")}</span>
                <span className="rd-cand-text">{c.text}</span>
              </li>
            ))}
          </ul>
        )}
        {history.length > 0 && (
          <div className="rd-history">
            <span className="dim small">{t("rdHistory")}</span>
            <div className="rd-chips">
              {history.slice(0, 8).map((h) => (
                <button
                  key={h}
                  type="button"
                  className="rd-chip"
                  title={h}
                  onClick={() => {
                    setQ(h);
                    setSel(-1);
                    inputRef.current?.focus();
                  }}
                >
                  {h}
                </button>
              ))}
            </div>
          </div>
        )}
        <div className="rd-foot">
          <label className="rd-norecord">
            <input type="checkbox" checked={noRecord} onChange={toggleNoRecord} />
            {t("rdNoRecord")}
          </label>
          <button type="button" className="btn ghost tiny" onClick={clearHistory} disabled={history.length === 0}>
            {t("rdClearHistory")}
          </button>
          <span className="flex-1" />
          <span className="dim small rd-hint">{t("rdHint")}</span>
        </div>
      </div>
    </div>
  );
}
