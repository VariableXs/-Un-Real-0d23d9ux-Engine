/**
 * AURORA-10000 领域04 · 任务栏与开始菜单 设置面板（AI-16~AI-20 批次，勿删）。
 * 按功能族渲染偏好开关/参数档；导入导出/重置/锁定/A-B 布局/总控。
 * 交互遵循 Win11 规范：整行可点、150ms 反馈、焦点环走令牌。
 */
import { useMemo, useState, useSyncExternalStore } from "react";
import { useI18n } from "../../i18n";
import "../../styles/ai16-20-d4.css";
import {
  AURORA_TASKBAR_FAMILIES, DEFAULT_D4_VALUES,
  applyLayoutSlot, exportD4, getD4Doc, importD4, resetD4, saveLayoutSlot,
  setD4, setLocked, setMaster, subscribeD4, toggleLayoutSlot,
} from "../../system/taskbar/aurora";

/** 值控件类型：布尔 → 开关；其余 → 数值/文本输入。 */
function valueControl(id: string, value: boolean | number | string, locked: boolean, onChange: (v: boolean | number | string) => void): React.ReactElement {
  if (typeof value === "boolean") {
    return (
      <button
        type="button"
        role="switch"
        aria-checked={value}
        aria-label={id}
        disabled={locked}
        className={value ? "d4-switch on" : "d4-switch"}
        onClick={() => onChange(!value)}
      >
        <span className="d4-knob" />
      </button>
    );
  }
  if (typeof value === "number") {
    return (
      <input
        type="number"
        aria-label={id}
        disabled={locked}
        value={value}
        className="d4-num"
        onChange={(e) => onChange(Number(e.target.value) || 0)}
      />
    );
  }
  const long = value.length > 12;
  return (
    <input
      type="text"
      aria-label={id}
      disabled={locked}
      value={value}
      className="d4-text"
      size={long ? 14 : 8}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

export function AuroraD4Tab(): React.ReactElement {
  const { t } = useI18n();
  const doc = useSyncExternalStore(subscribeD4, getD4Doc);
  const [slotTarget, setSlotTarget] = useState<"a" | "b">("b");
  const families = useMemo(() => AURORA_TASKBAR_FAMILIES, []);

  const onImport = async (): Promise<void> => {
    const text = await navigator.clipboard.readText().catch(() => "");
    if (!text) return;
    pushResult(importD4(text) ? t("d4Imported") : t("d4ImportBad"));
  };
  const [msg, setMsg] = useState<string | null>(null);
  const pushResult = (m: string): void => {
    setMsg(m);
    window.setTimeout(() => setMsg(null), 2000);
  };

  return (
    <div className="d4-tab" role="group" aria-label={t("d4TabTitle")}>
      <div className="d4-toolbar">
        <label className="d4-row">
          <span>{t("d4Master")}</span>
          <button
            type="button" role="switch" aria-checked={doc.master} aria-label={t("d4Master")}
            className={doc.master ? "d4-switch on" : "d4-switch"}
            onClick={() => setMaster(!doc.master)}
          >
            <span className="d4-knob" />
          </button>
        </label>
        <p className="dim small">{t("d4MasterHint")}</p>
        <div className="d4-actions">
          <button type="button" onClick={() => { setLocked(!doc.locked); }}>
            {doc.locked ? t("d4Unlock") : t("d4Lock")}
          </button>
          <button type="button" onClick={() => { resetD4(); pushResult(t("d4Saved")); }}>{t("d4Reset")}</button>
          <button type="button" onClick={() => { void navigator.clipboard.writeText(exportD4()); pushResult(t("d4Saved")); }}>{t("d4Export")}</button>
          <button type="button" onClick={() => { void onImport(); }}>{t("d4Import")}</button>
        </div>
        <div className="d4-actions">
          <span>{t("d4SaveSlot")}</span>
          <button type="button" className={slotTarget === "a" ? "on" : ""} onClick={() => setSlotTarget("a")}>{t("d4SlotA")}</button>
          <button type="button" className={slotTarget === "b" ? "on" : ""} onClick={() => setSlotTarget("b")}>{t("d4SlotB")}</button>
          <button type="button" onClick={() => { saveLayoutSlot(slotTarget); pushResult(t("d4Saved")); }}>{t("d4SaveSlot")}</button>
          <button type="button" onClick={() => toggleLayoutSlot()}>{t("d4ApplySlot")}</button>
          <button type="button" onClick={() => { applyLayoutSlot(slotTarget); }}>{t("d4SlotA")}/{t("d4SlotB")}</button>
        </div>
        {doc.locked && <p className="dim small" role="alert">{t("d4LockedHint")}</p>}
        {msg && <p className="dim small" role="status">{msg}</p>}
      </div>

      {families.map((fam) => (
        <section key={fam.fam} className="d4-family" aria-label={`${fam.ai} ${fam.name}`}>
          <h3 className="d4-family-title">{fam.name}<span className="dim small"> · {fam.items.length} 项 · {fam.ai}</span></h3>
          <ul className="d4-list">
            {fam.items.map((it) => {
              const registered = it.id in DEFAULT_D4_VALUES;
              const v = doc.values[it.id];
              return (
                <li key={it.id} className="d4-row">
                  <span className="d4-item-label">
                    <code className="dim small">{it.id}</code> {it.name}
                    <span className="dim small"> — {it.desc}</span>
                  </span>
                  {registered && v !== undefined
                    ? valueControl(it.id, v, doc.locked, (nv) => { setD4(it.id, nv); })
                    : <span className="dim small">✓</span>}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
