import { useEffect, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n";
import { createStore, useStore } from "../lib/store";
import { CloseLight } from "./CloseLight";

export function Modal(props: {
  open: boolean;
  title?: string;
  onClose: () => void;
  children: ReactNode;
  width?: number;
  footer?: ReactNode;
}) {
  const { t } = useI18n();
  useEffect(() => {
    if (!props.open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  if (!props.open) return null;
  return (
    <div className="modal-overlay" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
      <div role="dialog" aria-modal="true" aria-label={props.title} className="modal" style={{ maxWidth: props.width ?? 520 }}>
        {props.title !== undefined && (
          <div className="modal-head">
            <h3>{props.title}</h3>
            <span className="flex-1" />
            {/* 批次E-12：所有弹窗统一补绿灯（关闭），方便退出 */}
            <CloseLight onClose={props.onClose} />
            <button type="button" className="icon-btn small" aria-label={t("close")} data-tip={t("close")} onClick={props.onClose}>
              <X size={15} />
            </button>
          </div>
        )}
        <div className="modal-body">{props.children}</div>
        {props.footer && <div className="modal-foot">{props.footer}</div>}
      </div>
    </div>
  );
}

// ---------- global confirm ----------

interface ConfirmState {
  title: string;
  body: string;
  danger: boolean;
  okLabel?: string;
  resolve: (ok: boolean) => void;
}

const confirmStore = createStore<{ current: ConfirmState | null }>({ current: null });

export function askConfirm(opts: {
  title: string;
  body: string;
  danger?: boolean;
  okLabel?: string;
}): Promise<boolean> {
  const existing = confirmStore.getState().current;
  existing?.resolve(false);
  return new Promise<boolean>((resolve) => {
    confirmStore.setState({ current: { ...opts, danger: opts.danger ?? false, resolve } });
  });
}

export function ConfirmHost(): ReactNode {
  const current = useStore(confirmStore, (s) => s.current);
  const done = (ok: boolean) => {
    current?.resolve(ok);
    confirmStore.setState({ current: null });
  };
  if (!current) return null;
  return (
    <Modal open title={current.title} onClose={() => done(false)} width={460}>
      <p className="confirm-body" style={{ whiteSpace: "pre-wrap" }}>{current.body}</p>
      <div className="row end gap8">
        <button type="button" className="btn ghost" onClick={() => done(false)}>
          取消 / Cancel
        </button>
        <button type="button" autoFocus className={`btn ${current.danger ? "danger" : "primary"}`} onClick={() => done(true)}>
          {current.okLabel ?? "确定 / OK"}
        </button>
      </div>
    </Modal>
  );
}

// ---------- choice（批次D：红灯选择框等多选项确认） ----------

export interface ChoiceOption {
  value: string;
  label: string;
  danger?: boolean;
}

interface ChoiceState {
  title: string;
  body?: string;
  options: ChoiceOption[];
  resolve: (v: string | null) => void;
}

const choiceStore = createStore<{ current: ChoiceState | null }>({ current: null });

export function askChoice(opts: { title: string; body?: string; options: ChoiceOption[] }): Promise<string | null> {
  const existing = choiceStore.getState().current;
  existing?.resolve(null);
  return new Promise<string | null>((resolve) => {
    choiceStore.setState({ current: { ...opts, resolve } });
  });
}

export function ChoiceHost(): ReactNode {
  const current = useStore(choiceStore, (s) => s.current);
  const done = (v: string | null) => {
    current?.resolve(v);
    choiceStore.setState({ current: null });
  };
  if (!current) return null;
  return (
    <Modal open title={current.title} onClose={() => done(null)} width={440}>
      {current.body && <p className="confirm-body">{current.body}</p>}
      <div className="col gap8">
        {current.options.map((o) => (
          <button
            key={o.value}
            type="button"
            autoFocus={o.danger}
            className={`btn ${o.danger ? "danger" : "ghost"}`}
            onClick={() => done(o.value)}
          >
            {o.label}
          </button>
        ))}
      </div>
    </Modal>
  );
}

// ---------- net consent（批次0：任何联网前必须用户明确授权，规格 12.2.2） ----------

export type NetConsentDecision = "once" | "always" | "deny";

interface NetConsentState {
  target: string;
  purpose: string;
  resolve: (d: NetConsentDecision) => void;
}

const netConsentStore = createStore<{ current: NetConsentState | null }>({ current: null });

export function askNetConsent(opts: { target: string; purpose: string }): Promise<NetConsentDecision> {
  const existing = netConsentStore.getState().current;
  existing?.resolve("deny");
  return new Promise<NetConsentDecision>((resolve) => {
    netConsentStore.setState({ current: { ...opts, resolve } });
  });
}

export function NetConsentHost(): ReactNode {
  const current = useStore(netConsentStore, (s) => s.current);
  const { t } = useI18n();
  const done = (d: NetConsentDecision) => {
    current?.resolve(d);
    netConsentStore.setState({ current: null });
  };
  if (!current) return null;
  return (
    <Modal open title={t("netConsentTitle")} onClose={() => done("deny")} width={480}>
      <p className="small" style={{ wordBreak: "break-all" }}>
        <span className="dim">{t("netConsentTarget")}</span> {current.target}
      </p>
      <p className="small">
        <span className="dim">{t("netConsentPurpose")}</span> {current.purpose}
      </p>
      <p className="dim small">{t("netConsentNote")}</p>
      <div className="row end gap8">
        <button type="button" className="btn ghost" onClick={() => done("deny")}>
          {t("netDeny")}
        </button>
        <button type="button" className="btn ghost" onClick={() => done("once")}>
          {t("netOnce")}
        </button>
        <button type="button" autoFocus className="btn primary" onClick={() => done("always")}>
          {t("netAlways")}
        </button>
      </div>
    </Modal>
  );
}

// ---------- non-modal confirm bubble ----------

interface BubbleState {
  x: number;
  y: number;
  message: string;
  resolve: (ok: boolean) => void;
}

const bubbleStore = createStore<{ current: BubbleState | null }>({ current: null });

export function askConfirmBubble(opts: { x: number; y: number; message: string }): Promise<boolean> {
  bubbleStore.getState().current?.resolve(false);
  return new Promise<boolean>((resolve) => {
    const x = Math.min(Math.max(8, opts.x), window.innerWidth - 280);
    const y = Math.min(Math.max(8, opts.y), window.innerHeight - 90);
    bubbleStore.setState({ current: { x, y, message: opts.message, resolve } });
  });
}

export function ConfirmBubbleHost(): ReactNode {
  const cur = useStore(bubbleStore, (s) => s.current);
  useEffect(() => {
    if (!cur) return;
    const onDown = (e: MouseEvent): void => {
      const t = e.target as HTMLElement;
      if (t.closest(".confirm-bubble")) return;
      done(false);
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") done(false);
    };
    const t = setTimeout(() => window.addEventListener("mousedown", onDown), 0);
    window.addEventListener("keydown", onKey);
    return () => {
      clearTimeout(t);
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cur]);
  if (!cur) return null;
  function done(ok: boolean): void {
    cur?.resolve(ok);
    bubbleStore.setState({ current: null });
  }
  return (
    <div className="confirm-bubble card-pop" style={{ left: cur.x, top: cur.y }} role="alertdialog" aria-label={cur.message}>
      <span className="bubble-msg">{cur.message}</span>
      <button type="button" autoFocus className="btn tiny danger" onClick={() => done(true)}>OK</button>
      <button type="button" className="btn tiny ghost" onClick={() => done(false)}>✕</button>
    </div>
  );
}

// ---------- prompt (single-line input) ----------

interface PromptState {
  title: string;
  initial: string;
  validate: (v: string) => string | null;
  resolve: (v: string | null) => void;
}

const promptStore = createStore<{ current: PromptState | null }>({ current: null });

export function askPrompt(opts: {
  title: string;
  initial?: string;
  validate?: (v: string) => string | null;
}): Promise<string | null> {
  promptStore.getState().current?.resolve(null);
  return new Promise<string | null>((resolve) => {
    promptStore.setState({ current: { title: opts.title, initial: opts.initial ?? "", validate: opts.validate ?? (() => null), resolve } });
  });
}

export function PromptHost(): ReactNode {
  const current = useStore(promptStore, (s) => s.current);
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (current) setValue(current.initial);
  }, [current]);
  if (!current) return null;
  const done = (v: string | null) => {
    current.resolve(v);
    promptStore.setState({ current: null });
  };
  const submit = () => {
    const err = current.validate(value);
    if (err) {
      setError(err);
      return;
    }
    done(value);
  };
  return (
    <Modal open title={current.title} onClose={() => done(null)} width={420}
      footer={
        <div className="row end gap8">
          <button type="button" className="btn ghost" onClick={() => done(null)}>取消 / Cancel</button>
          <button type="button" className="btn primary" onClick={submit}>确定 / OK</button>
        </div>
      }
    >
      <input
        className={`text-input ${error ? "invalid" : ""}`}
        value={value}
        autoFocus
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
          e.stopPropagation();
        }}
        onFocus={(e) => e.target.select()}
      />
      {error && <div className="field-error">{error}</div>}
    </Modal>
  );
}
