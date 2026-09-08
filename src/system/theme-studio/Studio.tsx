/**
 * N-08 主题工坊 Studio（车道 E 自挂载 overlay）：
 * - 监听 window 事件 "ai04:open-feature"（detail.feature === "theme-studio"）由
 *   src/system/widgets/bus.ts 统一接线，本组件以 createPortal(document.body) 渲染。
 * - 三变体（亮/暗/高对比）页签同时编辑；每个颜色 token 一个拾色器 + OKLCH 展示
 *   + WCAG AA 对比度实时校验（不达标红牌且保存禁用，可显式勾选豁免并标注）。
 * - 导出/导入 .vtheme；应用到桌面走 applied.ts（跟随 data-theme 信号换变体）。
 * - 右侧实时预览：桌面缩样（任务栏条 + 窗口框 + 文字样例），用当前变体 token 渲染。
 */
import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { pushOverlay, popOverlay } from "../../state/uiStore";
import {
  COLOR_TOKENS, DENSITY_KINDS, FONT_SCALE_KINDS, RADIUS_KINDS,
  compositeOver, contrastRatio, currentVariant, defaultVariant, formatOklch,
  hexToOklch, hexToRgba, validateVariantContrast,
  type DensityKind, type FontScaleKind, type RadiusKind, type VariantKind,
} from "./tokens";
import {
  buildThumbnail, parseVtheme, serializeVtheme,
  type VthemeFile, type VthemeIssue, type VthemeVariant,
} from "./vtheme";
import { applyVthemeNow, clearAppliedVtheme, storeAppliedVtheme } from "./applied";
import { LABELS, useLaneLang, type LaneT } from "./labels";

const LIB_LS_KEY = "variable:theme-studio:library:v1";

interface StudioProps {
  onClose: () => void;
}

interface LibraryEntry extends VthemeFile {
  savedAt: number;
}

function loadLibrary(): LibraryEntry[] {
  try {
    const raw = localStorage.getItem(LIB_LS_KEY);
    const arr = raw ? (JSON.parse(raw) as LibraryEntry[]) : [];
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function saveLibrary(list: LibraryEntry[]): void {
  try {
    localStorage.setItem(LIB_LS_KEY, JSON.stringify(list.slice(-20)));
  } catch {
    /* storage full */
  }
}

/** 拾色器只接受 6 位 hex；半透明 token 去掉 alpha 两位，提交时拼回。 */
function rgbPart(hex: string): string {
  return hex.length === 9 ? hex.slice(0, 7) : hex;
}

function withAlpha(orig: string, rgb6: string): string {
  return orig.length === 9 ? `${rgb6}${orig.slice(7)}` : rgb6;
}

function defaultVariants(): Record<VariantKind, VthemeVariant> {
  return {
    light: defaultVariant("light"),
    dark: defaultVariant("dark"),
    hc: defaultVariant("hc"),
  };
}

function download(name: string, content: string): void {
  const blob = new Blob([content], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  void a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export default function ThemeStudio({ onClose }: StudioProps): React.ReactPortal {
  const lang = useLaneLang();
  const t = LABELS[lang] as LaneT;
  const [name, setName] = useState("My Variable Theme");
  const [variants, setVariants] = useState<Record<VariantKind, VthemeVariant>>(defaultVariants);
  const [tab, setTab] = useState<VariantKind>(() => currentVariant());
  const [exempt, setExempt] = useState<Record<string, boolean>>({});
  const [library, setLibrary] = useState<LibraryEntry[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [conflicts, setConflicts] = useState<VthemeIssue[]>([]);

  useEffect(() => {
    pushOverlay("theme-studio");
    setLibrary(loadLibrary());
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popOverlay("theme-studio");
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const current = variants[tab];
  const pairs = useMemo(() => validateVariantContrast(current.colors), [current.colors]);
  const blocking = pairs.filter((p) => !p.ok && !exempt[p.id]);
  const saveDisabled = blocking.length > 0 || name.trim().length === 0;

  const buildFile = (): VthemeFile => ({
    format: "vtheme",
    version: 1,
    name: name.trim() || "Untitled",
    variants,
    thumbnail: buildThumbnail(current, name.trim() || "Untitled"),
  });

  const setColor = (key: string, rgb6: string): void => {
    setVariants((v) => ({
      ...v,
      [tab]: {
        ...v[tab],
        colors: { ...v[tab].colors, [key]: withAlpha(v[tab].colors[key] ?? rgb6, rgb6) },
      },
    }));
  };

  const setShape = (patch: Partial<VthemeVariant["shape"]>): void => {
    setVariants((v) => ({ ...v, [tab]: { ...v[tab], shape: { ...v[tab].shape, ...patch } } }));
  };

  const doSave = (): void => {
    const file = buildFile();
    const next = [...loadLibrary().filter((e) => e.name !== file.name), { ...file, savedAt: Date.now() }];
    saveLibrary(next);
    setLibrary(next);
    setStatus(t.saved);
  };

  const doApply = (): void => {
    const file = buildFile();
    storeAppliedVtheme(file);
    applyVthemeNow(file);
    setStatus(t.applied);
  };

  const doRestore = (): void => {
    clearAppliedVtheme();
    setStatus(t.restored);
  };

  const doExport = (): void => {
    download(`${(name.trim() || "theme").replace(/[\\/:*?"<>|]/g, "_")}.vtheme`, serializeVtheme(buildFile()));
  };

  const doImport = async (file: File | undefined): Promise<void> => {
    if (!file) return;
    try {
      const text = await file.text();
      const r = parseVtheme(text);
      setConflicts(r.issues);
      if (!r.ok || !r.data) {
        setStatus(`${t.importFail}: ${r.issues.find((i) => i.level === "error")?.message ?? ""}`);
        return;
      }
      setName(r.data.name);
      const merged = defaultVariants();
      for (const k of ["light", "dark", "hc"] as VariantKind[]) {
        const v = r.data.variants[k];
        if (v) merged[k] = v;
      }
      setVariants(merged);
      setStatus(t.importOk);
    } catch (e) {
      setStatus(`${t.importFail}: ${String(e)}`);
    }
  };

  // ---- 实时预览缩样：直接用当前变体值（所见即所得） ----
  const pv = current.colors;
  const previewStyle: CSSProperties = {
    background: pv["--bg-canvas"],
    color: pv["--text-primary"],
    borderRadius: `${({ sharp: 2, soft: 16, round: 22 } as Record<RadiusKind, number>)[current.shape.radius]}px`,
    fontSize: `${({ small: 11, standard: 12, large: 13 } as Record<FontScaleKind, number>)[current.shape.fontScale]}px`,
  };

  return createPortal(
    <div className="theme-studio-overlay" role="dialog" aria-label={t.title}>
      <div className="theme-studio-win">
        <header className="theme-studio-head">
          <h3>{t.title}</h3>
          <span className="theme-studio-note">{t.storageNote}</span>
          <button type="button" className="iconpk-btn" aria-label={t.close} onClick={onClose}>
            <X size={16} />
          </button>
        </header>

        <div className="theme-studio-body">
          {/* ---------- 左：编辑区 ---------- */}
          <div className="theme-studio-edit">
            <div className="theme-studio-tabs">
              {(["light", "dark", "hc"] as VariantKind[]).map((k) => (
                <button
                  key={k}
                  type="button"
                  className={`theme-studio-tab ${tab === k ? "on" : ""}`}
                  onClick={() => setTab(k)}
                >
                  {k === "light" ? t.tabLight : k === "dark" ? t.tabDark : t.tabHC}
                </button>
              ))}
              <input
                className="theme-studio-name"
                value={name}
                maxLength={60}
                aria-label={t.name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>

            {(["bg", "text", "brand", "state"] as const).map((group) => (
              <div key={group} className="theme-studio-group">
                <div className="theme-studio-group-title">
                  {group === "bg" ? t.groupBg : group === "text" ? t.groupText : group === "brand" ? t.groupBrand : t.groupState}
                </div>
                {COLOR_TOKENS.filter((d) => d.group === group).map((def) => {
                  const val = current.colors[def.key] ?? "#000000";
                  const oklch = formatOklch(hexToOklch(rgbPart(val)));
                  return (
                    <label key={def.key} className="theme-studio-token">
                      <input
                        type="color"
                        value={rgbPart(val)}
                        onChange={(e) => setColor(def.key, e.target.value)}
                      />
                      <span className="theme-studio-token-name">
                        {lang === "en" ? def.en : def.zh}
                        <code>{def.key}</code>
                      </span>
                      <span className="theme-studio-token-val">
                        <code>{val}</code>
                        <code className="dim">{oklch}</code>
                        {def.translucent && <em className="theme-studio-alpha">{t.translucentNote}</em>}
                      </span>
                    </label>
                  );
                })}
              </div>
            ))}

            <div className="theme-studio-group">
              <div className="theme-studio-group-title">{t.shape}</div>
              <div className="theme-studio-shape">
                <span>{t.radius}</span>
                {RADIUS_KINDS.map((k) => (
                  <button key={k} type="button" className={`theme-studio-pill ${current.shape.radius === k ? "on" : ""}`} onClick={() => setShape({ radius: k as RadiusKind })}>
                    {k === "sharp" ? t.radiusSharp : k === "soft" ? t.radiusSoft : t.radiusRound}
                  </button>
                ))}
                <span>{t.density}</span>
                {DENSITY_KINDS.map((k) => (
                  <button key={k} type="button" className={`theme-studio-pill ${current.shape.density === k ? "on" : ""}`} onClick={() => setShape({ density: k as DensityKind })}>
                    {k === "compact" ? t.densityCompact : k === "regular" ? t.densityRegular : t.densityRelaxed}
                  </button>
                ))}
                <span>{t.fontScale}</span>
                {FONT_SCALE_KINDS.map((k) => (
                  <button key={k} type="button" className={`theme-studio-pill ${current.shape.fontScale === k ? "on" : ""}`} onClick={() => setShape({ fontScale: k as FontScaleKind })}>
                    {k === "small" ? t.fontSmall : k === "standard" ? t.fontStandard : t.fontLarge}
                  </button>
                ))}
              </div>
            </div>

            <div className="theme-studio-group">
              <div className="theme-studio-group-title">{t.contrastPairs}</div>
              {pairs.map((p) => {
                const fg = compositeOver(current.colors[p.fgKey] ?? "#000", current.colors["--bg-canvas"] ?? "#000") ?? current.colors[p.fgKey] ?? "#000";
                const bg = compositeOver(current.colors[p.bgKey] ?? "#000", current.colors["--bg-canvas"] ?? "#000") ?? current.colors[p.bgKey] ?? "#000";
                const ratio = contrastRatio(fg, bg);
                const isExempt = !!exempt[p.id];
                return (
                  <div key={p.id} className={`theme-studio-pair ${p.ok ? "ok" : isExempt ? "exempt" : "bad"}`}>
                    <span className="theme-studio-pair-demo" style={{ background: bg, color: fg }}>
                      Aa
                    </span>
                    <code>{p.fgKey}</code>
                    <span>× {p.bgKey}</span>
                    <strong>{ratio.toFixed(2)}:1</strong>
                    {p.ok ? (
                      <em className="ok">{t.contrastOk}</em>
                    ) : (
                      <label className="theme-studio-exempt">
                        <input
                          type="checkbox"
                          checked={isExempt}
                          onChange={(e) => setExempt((x) => ({ ...x, [p.id]: e.target.checked }))}
                        />
                        {t.exempt}
                      </label>
                    )}
                    {!p.ok && isExempt && <em className="theme-studio-exempt-note">{t.exemptNote}</em>}
                  </div>
                );
              })}
            </div>
          </div>

          {/* ---------- 右：实时预览 + 动作 ---------- */}
          <div className="theme-studio-side">
            <div className="theme-studio-preview" style={previewStyle}>
              <div className="theme-studio-pv-win">
                <div className="theme-studio-pv-titlebar">
                  <i /><i /><i />
                  <span style={{ background: pv["--text-secondary"] }} />
                </div>
                <div className="theme-studio-pv-line" style={{ background: pv["--text-primary"] }} />
                <div className="theme-studio-pv-line short" style={{ background: pv["--text-secondary"] }} />
                <button type="button" className="theme-studio-pv-btn" style={{ background: pv["--accent"], color: pv["--bg-canvas"] }}>
                  {pv["--accent"] ? "Aa" : ""}
                </button>
              </div>
              <div className="theme-studio-pv-taskbar" style={{ background: pv["--bg-raised"], borderColor: pv["--stroke"] }}>
                <i style={{ background: pv["--accent"] }} />
                <i style={{ background: pv["--success"] }} />
                <i style={{ background: pv["--danger"] }} />
              </div>
            </div>

            <div className="theme-studio-actions">
              <button type="button" className="iconpk-btn primary" disabled={saveDisabled} onClick={doSave}>
                {t.save}
              </button>
              <button type="button" className="iconpk-btn" onClick={doApply}>{t.apply}</button>
              <button type="button" className="iconpk-btn" onClick={doRestore}>{t.restore}</button>
              <button type="button" className="iconpk-btn" onClick={doExport}>{t.export}</button>
              <label className="iconpk-btn">
                {t.import}
                <input type="file" accept=".vtheme,.json,application/json" hidden onChange={(e) => void doImport(e.target.files?.[0])} />
              </label>
            </div>

            {blocking.length > 0 && (
              <div className="theme-studio-block-note">
                {t.contrastFail} × {blocking.length} → {t.exempt}
              </div>
            )}
            {conflicts.length > 0 && (
              <div className="theme-studio-conflicts">
                <div className="theme-studio-group-title">{t.conflicts}</div>
                {conflicts.slice(0, 8).map((i, idx) => (
                  <div key={`${i.field}-${idx}`} className={`lvl-${i.level}`}>
                    [{i.level}] {i.field}: {i.message}
                  </div>
                ))}
              </div>
            )}
            {status && <div className="theme-studio-status">{status}</div>}

            <div className="theme-studio-group">
              <div className="theme-studio-group-title">{t.library}</div>
              {library.length === 0 && <div className="dim">{t.noLibrary}</div>}
              {library.map((e) => (
                <button key={e.name} type="button" className="theme-studio-librow" onClick={() => { setName(e.name); setVariants({ ...defaultVariants(), ...e.variants } as Record<VariantKind, VthemeVariant>); }}>
                  {e.name}
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

/** 校验辅助（暴露给测试/外部工具；Studio 内部未直接使用 rgba 完整路径）。 */
export const __internal = { rgbPart, withAlpha, hexToRgba };