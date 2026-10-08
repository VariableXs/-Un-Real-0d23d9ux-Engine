/**
 * AURORA-10000 · AI-11~AI-15 车道 · 设计中心（feature: design-center）。
 * 领域03 全 25 族 × 25 项的总控台：
 * - preset 族单选 / switch 独立启停 / reserved 展示诚实边界；
 * - 族0051/0053 实时图标预览网格（同一套 --w2-icon-* 变量驱动）；
 * - 族0056 选中即经 ai04:wallpaper-apply 驱动真实壁纸引擎；
 * - 令牌调试（F01646）、档案导入导出（F01800）、剧场入口（族0074）。
 */
import { useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { FAMILIES, findEntry } from "./catalog";
import {
  activeEntries, designStore, exportProfile, importProfile, resetAll, resetFamily,
  selectPreset, setTokenOverride, clearTokenOverride, toggleSwitch, useDesignState,
} from "./state";
import { applyEngineEntry } from "./runtime";
import { LABELS, useLaneLang } from "./labels";
import { dispatchClose } from "../wallpaper/mount";
import "./design-center.css";

const GROUPS: Array<{ key: string; labelKey: keyof typeof LABELS.zh; familyIds: string[] }> = [
  { key: "icons", labelKey: "groupIcons", familyIds: ["f0051", "f0052", "f0053", "f0054", "f0055"] },
  { key: "wallpaper", labelKey: "groupWallpaper", familyIds: ["f0056", "f0057", "f0058", "f0059", "f0060"] },
  { key: "widgets", labelKey: "groupWidgets", familyIds: ["f0061", "f0062", "f0063", "f0064", "f0065"] },
  { key: "visual", labelKey: "groupVisual", familyIds: ["f0066", "f0067", "f0068", "f0069", "f0070"] },
  { key: "personality", labelKey: "groupPersonality", familyIds: ["f0071", "f0072", "f0073", "f0074", "f0075"] },
];

function familyProgress(familyId: string, selections: Record<string, string | null>, enabled: Record<string, boolean>): number {
  const f = FAMILIES.find((x) => x.id === familyId);
  if (!f) return 0;
  let n = selections[familyId] ? 1 : 0;
  for (const e of f.entries) if (e.kind !== "preset" && enabled[e.id]) n++;
  return n;
}

export function DesignCenter(): React.JSX.Element {
  const lang = useLaneLang();
  const L = LABELS[lang];
  const s = useDesignState();
  const [current, setCurrent] = useState("f0051");
  const [msg, setMsg] = useState("");
  const fileRef = useRef<HTMLInputElement | null>(null);

  const family = FAMILIES.find((f) => f.id === current) ?? FAMILIES[0]!;
  const activeCount = useMemo(() => activeEntries(s).length, [s]);

  const onEntryClick = (entryId: string): void => {
    const ent = findEntry(entryId);
    if (!ent) return;
    if (ent.kind === "preset") {
      selectPreset(entryId);
      if (ent.family === "f0056") {
        applyEngineEntry(ent);
        setMsg(L.engineApplied);
      }
    } else {
      toggleSwitch(entryId);
    }
  };

  return (
    <div className="w2-center-backdrop" onPointerDown={(e) => { if (e.target === e.currentTarget) dispatchClose("design-center"); }}>
      <section className="w2-center" role="dialog" aria-label={L.title}>
        <header className="w2-center-head">
          <h2>{L.title}</h2>
          <span className="w2-lane-note">{L.lane} · {activeCount} {L.entryApplied}</span>
          <div className="w2-head-actions">
            <button type="button" className="w2-btn w2-btn-ghost" onClick={() => { void navigator.clipboard?.writeText(exportProfile()); setMsg(L.exportProfile + " ✓"); }}>{L.exportProfile}</button>
            <button type="button" className="w2-btn w2-btn-ghost" onClick={() => fileRef.current?.click()}>{L.importProfile}</button>
            <button type="button" className="w2-btn w2-btn-ghost" onClick={() => { resetAll(); setMsg(L.resetAll + " ✓"); }}>{L.resetAll}</button>
            <button type="button" className="w2-btn w2-btn-ghost" aria-label={L.close} onClick={() => dispatchClose("design-center")}>
              <X size={14} aria-hidden="true" />
            </button>
          </div>
          <input
            ref={fileRef}
            type="file"
            accept="application/json"
            hidden
            onChange={async () => {
              const f = fileRef.current?.files?.[0];
              if (!f) return;
              const r = importProfile(await f.text());
              setMsg(r.ok ? L.profileOk : `${L.profileBad}: ${r.error ?? ""}`);
              if (fileRef.current) fileRef.current.value = "";
            }}
          />
        </header>

        <nav className="w2-center-nav" aria-label={L.title}>
          {GROUPS.map((g) => (
            <div key={g.key}>
              <p className="w2-nav-group">{L[g.labelKey] as string}</p>
              {FAMILIES.filter((f) => g.familyIds.includes(f.id)).map((f) => (
                <button
                  key={f.id}
                  type="button"
                  className="w2-nav-item"
                  aria-current={f.id === current}
                  onClick={() => setCurrent(f.id)}
                >
                  <span>族{f.no} {f.title[lang]}</span>
                  <span className="w2-count">{L.count(familyProgress(f.id, s.selections, s.enabled), 25)}</span>
                </button>
              ))}
            </div>
          ))}
        </nav>

        <div className="w2-center-body">
          <h3 className="w2-family-title">{`族${family.no} ${family.title[lang]}`}</h3>
          <p className="w2-family-range">
            F{String(family.range[0]).padStart(5, "0")}~F{String(family.range[1]).padStart(5, "0")} · {family.ai}
            {family.id === "f0073" ? ` · ${L.lunarNote}` : ""}
            <button type="button" className="w2-btn w2-btn-ghost" style={{ marginLeft: "var(--sp-3)" }} onClick={() => resetFamily(family.id)}>{L.resetFamily}</button>
            {family.id === "f0074" && (
              <button
                type="button"
                className="w2-btn"
                style={{ marginLeft: "var(--sp-3)" }}
                onClick={() => window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "design-theater" } }))}
              >
                {L.theaterOpen}
              </button>
            )}
            {family.id === "f0075" && (
              <button
                type="button"
                className="w2-btn"
                style={{ marginLeft: "var(--sp-3)" }}
                onClick={() => window.dispatchEvent(new CustomEvent("aurora-w2:ritual", { detail: { entryId: "F01851" } }))}
              >
                {L.ritualTest}
              </button>
            )}
          </p>
          {msg && <p className="w2-family-range" role="status">{msg}</p>}

          {(family.id === "f0051" || family.id === "f0053") && (
            <div className="w2-icon-preview" aria-hidden="true">
              {Array.from({ length: 16 }, (_, i) => (
                <span
                  key={i}
                  className="w2-icon-cell"
                  style={{
                    width: "72%",
                    height: "72%",
                    margin: "auto",
                    borderRadius: "var(--w2-icon-frame, 50%)",
                    filter: `saturate(var(--w2-icon-sat, 1)) brightness(var(--w2-icon-bright, 1)) blur(${i % 5 === 0 ? "var(--w2-icon-plate-blur, 0px)" : "0"})`,
                    boxShadow: "var(--w2-icon-shadow, none)",
                  }}
                />
              ))}
            </div>
          )}

          {family.entries.map((ent) => {
            const active = ent.kind === "preset" ? s.selections[family.id] === ent.id : s.enabled[ent.id] === true;
            return (
              <div className="w2-entry" key={ent.id} data-active={active}>
                <span className="w2-entry-id">{ent.id}</span>
                <div className="w2-entry-main">
                  <span>{ent.label[lang]}</span>
                  <span className="w2-entry-kind">
                    {ent.kind === "preset" ? L.preset : ent.kind === "switch" ? L.switch : L.reserved}
                  </span>
                  {ent.note && <p className="w2-entry-note">{ent.note[lang]}</p>}
                </div>
                {ent.kind !== "preset" && (
                  <button
                    type="button"
                    className="w2-btn"
                    aria-pressed={active}
                    onClick={() => onEntryClick(ent.id)}
                  >
                    {active ? L.on : L.off}
                  </button>
                )}
                {ent.kind === "preset" && (
                  <button
                    type="button"
                    className="w2-btn"
                    aria-pressed={active}
                    onClick={() => onEntryClick(ent.id)}
                  >
                    {active ? L.selected : L.preset}
                  </button>
                )}
              </div>
            );
          })}

          {family.id === "f0066" && (
            <div className="w2-token-debug">
              <p>{L.tokenDebug}</p>
              {Object.entries(s.tokenOverrides).map(([k, v]) => (
                <div className="w2-token-row" key={k}>
                  <input value={k} readOnly aria-label={L.tokenName} style={{ width: 220 }} />
                  <input value={v} readOnly aria-label={L.tokenValue} style={{ width: 220 }} />
                  <button type="button" className="w2-btn w2-btn-ghost" onClick={() => clearTokenOverride(k)}>{L.tokenClear}</button>
                </div>
              ))}
              <div className="w2-token-row">
                <input
                  id="w2-token-name"
                  placeholder="--w2-elev-6"
                  aria-label={L.tokenName}
                  style={{ width: 220 }}
                  onKeyDown={(e) => {
                    if (e.key !== "Enter") return;
                    const name = (e.target as HTMLInputElement).value.trim();
                    const valEl = document.getElementById("w2-token-value") as HTMLInputElement | null;
                    if (name.startsWith("--") && valEl?.value) {
                      setTokenOverride(name, valEl.value);
                      valEl.value = "";
                      (e.target as HTMLInputElement).value = "";
                    }
                  }}
                />
                <input id="w2-token-value" placeholder="0 24px 64px oklch(0.15 0.02 260 / 0.45)" aria-label={L.tokenValue} style={{ width: 280 }} />
                <button
                  type="button"
                  className="w2-btn"
                  onClick={() => {
                    const name = (document.getElementById("w2-token-name") as HTMLInputElement | null)?.value.trim() ?? "";
                    const valEl = document.getElementById("w2-token-value") as HTMLInputElement | null;
                    if (name.startsWith("--") && valEl?.value) {
                      setTokenOverride(name, valEl.value);
                      valEl.value = "";
                    }
                  }}
                >
                  {L.tokenSet}
                </button>
              </div>
            </div>
          )}

          {family.id === "f0065" && s.enabled["F01623"] && (
            <p className="w2-family-range">{L.healthRunning}</p>
          )}
        </div>
      </section>
    </div>
  );
}

// 保持 designStore 引用（防止 tree-shake 误删订阅装配；runtime.initDesignRuntime 负责订阅）
void designStore;
