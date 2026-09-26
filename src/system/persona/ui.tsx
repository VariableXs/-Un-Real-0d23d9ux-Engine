/**
 * PersonaStudio 共享 UI 件（E 域二十页共用）：
 * - 密度对标专业工具（Bloomberg/Blender 的"每一格都有用"），层级靠分区卡递进；
 * - 质感走 tokens.css 既有变量 + persona --p-* 令牌（毛玻璃/光带/按压反馈齐全）；
 * - 全键盘可达：控件原生 input/button + 焦点环继承 tokens.css --focus-ring。
 */
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { personaStore, type PersonaSection } from "./store";
import { useLaneLang, makeT, type LaneT } from "./labels";

/** 订阅 personaStore 分节：数据变 → 组件重渲（订阅制不轮询，F155 同源）。 */
export function usePersonaSection<S extends PersonaSection>(section: S): Record<string, unknown> {
  const [, bump] = useState(0);
  useEffect(() => {
    const off = personaStore.subscribe((s) => {
      if (s === section) bump((n) => n + 1);
    });
    return off;
  }, [section]);
  return personaStore.get(section);
}

export function useT(): LaneT {
  const lang = useLaneLang();
  return makeT(lang);
}

// ---------- 布局件 ----------

export function PageHeader(props: { title: string; hint?: string; right?: ReactNode }): ReactNode {
  return (
    <div style={styles.pageHeader}>
      <div>
        <h2 style={styles.pageTitle}>{props.title}</h2>
        {props.hint ? <p style={styles.pageHint}>{props.hint}</p> : null}
      </div>
      {props.right ? <div style={styles.pageHeaderRight}>{props.right}</div> : null}
    </div>
  );
}

export function Card(props: { title?: string; children: ReactNode; style?: CSSProperties }): ReactNode {
  return (
    <section style={{ ...styles.card, ...props.style }}>
      {props.title ? <h3 style={styles.cardTitle}>{props.title}</h3> : null}
      {props.children}
    </section>
  );
}

export function Row(props: { label: string; sub?: string; children: ReactNode }): ReactNode {
  return (
    <label style={styles.row}>
      <span style={styles.rowLabel}>
        <b style={styles.rowLabelText}>{props.label}</b>
        {props.sub ? <small style={styles.rowSub}>{props.sub}</small> : null}
      </span>
      <span style={styles.rowControl}>{props.children}</span>
    </label>
  );
}

// ---------- 控件件 ----------

export function Toggle(props: { checked: boolean; onChange: (v: boolean) => void; ariaLabel: string }): ReactNode {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.ariaLabel}
      style={{ ...styles.toggle, ...(props.checked ? styles.toggleOn : {}) }}
      onClick={() => props.onChange(!props.checked)}
    >
      <span style={{ ...styles.toggleBall, ...(props.checked ? styles.toggleBallOn : {}) }} />
    </button>
  );
}

export function Slider(props: { value: number; min: number; max: number; step: number; onChange: (v: number) => void; ariaLabel: string; format?: (v: number) => string }): ReactNode {
  return (
    <span style={styles.sliderWrap}>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value}
        aria-label={props.ariaLabel}
        style={styles.slider}
        onChange={(e) => props.onChange(Number(e.target.value))}
      />
      {props.format ? <small style={styles.sliderValue}>{props.format(props.value)}</small> : null}
    </span>
  );
}

export function Segmented<T extends string>(props: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; ariaLabel: string }): ReactNode {
  return (
    <span role="radiogroup" aria-label={props.ariaLabel} style={styles.segmented}>
      {props.options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={props.value === o.value}
          style={{ ...styles.segBtn, ...(props.value === o.value ? styles.segBtnOn : {}) }}
          onClick={() => props.onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </span>
  );
}

export function PButton(props: { children: ReactNode; onClick: () => void; kind?: "primary" | "ghost" | "danger"; disabled?: boolean; ariaLabel?: string }): ReactNode {
  const base = props.kind === "primary" ? styles.btnPrimary : props.kind === "danger" ? styles.btnDanger : styles.btnGhost;
  return (
    <button type="button" style={{ ...styles.btn, ...base }} disabled={props.disabled} aria-label={props.ariaLabel} onClick={props.onClick}>
      {props.children}
    </button>
  );
}

export function Notice(props: { tone: "info" | "warn" | "danger" | "ok"; children: ReactNode }): ReactNode {
  const tone = styles[`notice${props.tone[0]?.toUpperCase() ?? "I"}${props.tone.slice(1)}` as keyof typeof styles] as CSSProperties | undefined;
  return <div style={{ ...styles.notice, ...tone }}>{props.children}</div>;
}

/** 色值拾色器行：6 位 hex + alpha 保留（theme-studio rgbPart 同法）。 */
export function ColorChip(props: { value: string; onChange: (hex: string) => void; ariaLabel: string }): ReactNode {
  const rgb = props.value.length === 9 ? props.value.slice(0, 7) : props.value;
  const alpha = props.value.length === 9 ? props.value.slice(7) : "";
  return (
    <span style={styles.chipWrap}>
      <input
        type="color"
        value={/^#[0-9a-fA-F]{6}$/.test(rgb) ? rgb : "#808080"}
        aria-label={props.ariaLabel}
        style={styles.colorInput}
        onChange={(e) => props.onChange(`${e.target.value}${alpha}`)}
      />
      <code style={styles.chipHex}>{props.value}</code>
    </span>
  );
}

/** 迷你桌面预览（F152/F168/F164 共用——0.5x 缩放真渲染，非贴图）。 */
export function MiniDesktop(props: { width?: number; accent?: string; children?: ReactNode; scene?: string }): ReactNode {
  const w = props.width ?? 400;
  return (
    <div style={{ ...styles.miniDesktop, width: w, height: Math.round(w * 0.6) }} data-scene={props.scene}>
      <div style={styles.miniWall} />
      <div style={{ ...styles.miniWindow, borderColor: props.accent }}>
        <span style={{ ...styles.miniTitlebarDot, background: props.accent }} />
        <span style={styles.miniLine} />
        <span style={{ ...styles.miniLine, width: "60%" }} />
      </div>
      {props.children}
      <div style={styles.miniTaskbar}>
        <span style={{ ...styles.miniTaskIcon, background: props.accent }} />
        <span style={styles.miniTaskIcon} />
        <span style={styles.miniTaskIcon} />
      </div>
    </div>
  );
}

// ---------- 样式（密集但有呼吸：12px 分区距 / 8px 行距 / 1px 淡描边） ----------

export const styles: Record<string, CSSProperties> = {
  pageHeader: { display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 16, marginBottom: 12 },
  pageTitle: { margin: 0, fontSize: "var(--p-fs-title, 20px)", fontWeight: 650, letterSpacing: 0.2 },
  pageHint: { margin: "4px 0 0", fontSize: "var(--p-fs-caption, 12px)", color: "var(--p-fg-secondary, var(--text-secondary, #a0a0b4))", lineHeight: 1.6, maxWidth: 640 },
  pageHeaderRight: { flexShrink: 0 },
  card: {
    background: "var(--p-bg-surface, var(--bg-surface, rgba(28,28,38,0.72)))",
    border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.12))",
    borderRadius: "var(--p-r-card, 12px)",
    padding: 14,
    marginBottom: 12,
    backdropFilter: "blur(14px)",
  },
  cardTitle: { margin: "0 0 10px", fontSize: "var(--p-fs-body, 15px)", fontWeight: 600 },
  row: { display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, padding: "7px 0", borderBottom: "1px solid var(--p-border-subtle, rgba(140,140,160,0.1))" },
  rowLabel: { display: "flex", flexDirection: "column", gap: 2, minWidth: 0 },
  rowLabelText: { fontSize: "var(--p-fs-body, 15px)", fontWeight: 500 },
  rowSub: { fontSize: "var(--p-fs-caption, 12px)", color: "var(--p-fg-secondary, var(--text-secondary, #a0a0b4))" },
  rowControl: { display: "flex", alignItems: "center", gap: 8, flexShrink: 0 },
  toggle: { width: 40, height: 20, borderRadius: 10, border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", background: "var(--p-bg-disabled, rgba(44,44,56,0.7))", position: "relative", cursor: "pointer", padding: 0, transition: "background var(--p-motion-micro, 120ms) var(--p-ease-enter, ease-out)" },
  toggleOn: { background: "var(--p-accent, var(--accent, #6e7fd4))", borderColor: "transparent" },
  toggleBall: { position: "absolute", top: 2, left: 2, width: 14, height: 14, borderRadius: "50%", background: "#fff", transition: "transform var(--p-motion-micro, 120ms) var(--p-ease-spring, cubic-bezier(0.34,1.56,0.64,1))" },
  toggleBallOn: { transform: "translateX(20px)" },
  sliderWrap: { display: "flex", alignItems: "center", gap: 8 },
  slider: { accentColor: "var(--p-accent, var(--accent, #6e7fd4))", width: 160 },
  sliderValue: { fontVariantNumeric: "tabular-nums", minWidth: 44, textAlign: "right" },
  segmented: { display: "inline-flex", borderRadius: "var(--p-r-control, 8px)", border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", overflow: "hidden" },
  segBtn: { padding: "5px 12px", fontSize: "var(--p-fs-caption, 12px)", background: "transparent", color: "inherit", border: "none", cursor: "pointer", transition: "background var(--p-motion-micro, 120ms) var(--p-ease-enter, ease-out)" },
  segBtnOn: { background: "var(--p-accent, var(--accent, #6e7fd4))", color: "var(--p-on-accent, #fff)" },
  btn: { padding: "6px 14px", borderRadius: "var(--p-r-control, 8px)", fontSize: "var(--p-fs-caption, 12px)", border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", cursor: "pointer", transition: "transform var(--p-motion-micro, 120ms) var(--p-ease-enter, ease-out), filter var(--p-motion-micro, 120ms)" },
  btnPrimary: { background: "var(--p-accent, var(--accent, #6e7fd4))", color: "var(--p-on-accent, #fff)", borderColor: "transparent" },
  btnGhost: { background: "transparent", color: "inherit" },
  btnDanger: { background: "var(--p-danger-soft, rgba(212,104,95,0.15))", color: "var(--p-danger, #d4685f)", borderColor: "var(--p-danger, #d4685f)" },
  notice: { padding: "8px 12px", borderRadius: "var(--p-r-control, 8px)", fontSize: "var(--p-fs-caption, 12px)", marginBottom: 10, lineHeight: 1.6 },
  noticeInfo: { background: "var(--p-accent-soft, rgba(110,127,212,0.16))" },
  noticeWarn: { background: "var(--p-warn-soft, rgba(212,180,95,0.16))", color: "var(--p-warn, #d4b45f)" },
  noticeDanger: { background: "var(--p-danger-soft, rgba(212,104,95,0.16))", color: "var(--p-danger, #d4685f)" },
  noticeOk: { background: "var(--p-success-soft, rgba(95,191,138,0.16))", color: "var(--p-success, #5fbf8a)" },
  chipWrap: { display: "inline-flex", alignItems: "center", gap: 6 },
  colorInput: { width: 26, height: 26, padding: 0, border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", borderRadius: 6, background: "transparent", cursor: "pointer" },
  chipHex: { fontSize: 11, opacity: 0.8 },
  miniDesktop: { position: "relative", borderRadius: "var(--p-r-card, 12px)", overflow: "hidden", border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", background: "var(--p-bg-canvas, var(--bg-canvas, #14141c))" },
  miniWall: { position: "absolute", inset: 0, background: "linear-gradient(135deg, var(--p-accent-soft, rgba(110,127,212,0.2)), transparent 55%), radial-gradient(circle at 75% 25%, var(--p-selection, rgba(110,127,212,0.3)), transparent 45%)" },
  miniWindow: { position: "absolute", left: "12%", top: "14%", width: "58%", height: "56%", background: "var(--p-bg-raised, rgba(34,34,46,0.9))", border: "1px solid var(--p-border-regular, rgba(140,140,160,0.3))", borderRadius: "var(--p-r-window, 16px)", padding: 10, display: "flex", flexDirection: "column", gap: 8, boxShadow: "0 8px 24px var(--p-shadow, rgba(0,0,0,0.4))" },
  miniTitlebarDot: { width: 8, height: 8, borderRadius: "50%" },
  miniLine: { height: 6, width: "85%", borderRadius: 3, background: "var(--p-border-regular, rgba(140,140,160,0.25))" },
  miniTaskbar: { position: "absolute", left: 0, right: 0, bottom: 0, height: 28, display: "flex", alignItems: "center", justifyContent: "center", gap: 8, background: "var(--p-bg-surface, rgba(28,28,38,0.8))", borderTop: "1px solid var(--p-border-subtle, rgba(140,140,160,0.12))" },
  miniTaskIcon: { width: 14, height: 14, borderRadius: 4, background: "var(--p-border-regular, rgba(140,140,160,0.4))" },
};
