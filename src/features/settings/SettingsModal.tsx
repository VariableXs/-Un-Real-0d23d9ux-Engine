import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Download, Trash2, RotateCcw } from "lucide-react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { DEFAULT_SETTINGS, type CustomBg, type MindDefaults, type Settings, type ThemeId } from "../../lib/settings";
import { formatBytes, clamp } from "../../lib/format";
import { pushToast, uiStore, useUi } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { Modal } from "../../components/Modal";
import type { BackupInfo, BootstrapInfo } from "../../lib/types";

const IMG_FILTERS = [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }];
const VID_FILTERS = [{ name: "Videos", extensions: ["mp4", "webm", "ogv", "mov", "m4v"] }];
/** Ten-tier starfield labels (spec chapter 4) — zh / en picked at render. */
const TIER_LABELS: readonly string[] = [
  "冰蓝静谧", "心跳脉动", "双层视差", "独立闪烁", "漫画网点",
  "四角星芒", "景深色散", "极光流淌", "真实散射", "终极光影",
];
const TIER_LABELS_EN: readonly string[] = [
  "Ice Static", "Global Pulse", "Dual Parallax", "Indep. Twinkle", "Screentone",
  "4-Point Flare", "Bokeh & Dispersion", "Aurora Flow", "Real Scattering", "Light Master",
];
const FONT_STACKS = [
  { label: "Segoe UI / 微软雅黑", value: `"Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif` },
  { label: "Serif (Georgia)", value: `Georgia, "Times New Roman", serif` },
  { label: "Mono (Consolas)", value: `Consolas, "Courier New", monospace` },
];

export function SettingsModal(props: {
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
  bootstrap: BootstrapInfo | null;
}): React.ReactElement | null {
  const { t, lang, setLang } = useI18n();
  const isOpen = useUi((s) => s.settingsOpen);
  const tab = useUi((s) => s.settingsTab);
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const s = props.settings;

  useEffect(() => {
    if (isOpen && tab === "data") {
      void ipc.listBackups().then(setBackups).catch(() => setBackups([]));
    }
  }, [isOpen, tab]);

  if (!isOpen) return null;

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => props.onChange({ [key]: value } as Partial<Settings>);
  const setBg = (patch: Partial<CustomBg>) => props.onChange({ customBg: { ...s.customBg, ...patch } });
  const setMind = (patch: Partial<MindDefaults>) => props.onChange({ mindDefaults: { ...s.mindDefaults, ...patch } });

  const tabs: { id: string; label: string }[] = [
    { id: "appearance", label: t("appearance") },
    { id: "editor", label: t("editorTab") },
    { id: "mindmap", label: t("mindmapTab") },
    { id: "general", label: t("general") },
    { id: "data", label: t("data") },
  ];

  async function pickBackground(kind: "image" | "video"): Promise<void> {
    const p = await open({ multiple: false, filters: kind === "image" ? IMG_FILTERS : VID_FILTERS });
    if (typeof p !== "string") return;
    // Reference the ORIGINAL path (validated); user can relocate on failure.
    const check = await ipc.checkPaths([p]).catch(() => []);
    if (!check[0]?.exists) {
      pushToast("error", lang === "zh" ? "文件不可读" : "File not readable");
      return;
    }
    setBg(kind === "image" ? { type: "image", imagePath: p } : { type: "video", videoPath: p, playVideo: true });
  }

  return (
    <Modal open onClose={() => uiStore.setState({ settingsOpen: false })} title={t("settings")} width={760}>
      <div className="settings-layout">
        <nav className="settings-nav">
          {tabs.map((tb) => (
            <button key={tb.id} type="button" className={tab === tb.id ? "on" : ""} onClick={() => uiStore.setState({ settingsTab: tb.id })}>
              {tb.label}
            </button>
          ))}
        </nav>
        <div className="settings-body">
          {tab === "appearance" && (
            <>
              <Field label={t("theme")}>
                <select value={s.theme} onChange={(e) => set("theme", e.target.value as ThemeId)}>
                  <option value="deep-space">{t("themeDeepSpace")}</option>
                  <option value="paper">{t("themePaper")}</option>
                  <option value="minimal-black">{t("themeMinimalBlack")}</option>
                  <option value="custom">{t("themeCustom")}</option>
                </select>
              </Field>
              {s.theme === "custom" && (
                <>
                  <Field label={t("backgroundType")}>
                    <select value={s.customBg.type} onChange={(e) => setBg({ type: e.target.value as CustomBg["type"] })}>
                      <option value="nebula">{t("themeDeepSpace")}</option>
                      <option value="color">{t("bgPureColor")}</option>
                      <option value="gradient">{t("bgGradient")}</option>
                      <option value="image">{t("bgImage")}</option>
                      <option value="video">{t("bgVideo")}</option>
                    </select>
                  </Field>
                  {(s.customBg.type === "image" || s.customBg.type === "video") && (
                    <Field label={s.customBg.type === "image" ? t("bgImage") : t("bgVideo")}>
                      <div className="row gap8">
                        <input className="text-input flex-1" readOnly value={s.customBg.type === "image" ? s.customBg.imagePath : s.customBg.videoPath} />
                        <button type="button" className="btn ghost" onClick={() => void pickBackground(s.customBg.type === "image" ? "image" : "video")}>{t("chooseFile")}</button>
                      </div>
                    </Field>
                  )}
                  {s.customBg.type === "color" && (
                    <Field label={t("bgPureColor")}>
                      <input type="color" value={s.customBg.color} onChange={(e) => setBg({ color: e.target.value })} />
                    </Field>
                  )}
                  {s.customBg.type === "gradient" && (
                    <Field label={`${t("bgGradient")} A → B`}>
                      <div className="row gap8">
                        <input type="color" value={s.customBg.gradientFrom} onChange={(e) => setBg({ gradientFrom: e.target.value })} />
                        <input type="color" value={s.customBg.gradientTo} onChange={(e) => setBg({ gradientTo: e.target.value })} />
                      </div>
                    </Field>
                  )}
                  <Slider label={t("brightness")} min={20} max={140} value={Math.round(s.customBg.brightness * 100)} suffix="%" onChange={(v) => setBg({ brightness: v / 100 })} />
                  <Slider label={t("blur")} min={0} max={24} value={s.customBg.blur} suffix="px" onChange={(v) => setBg({ blur: v })} />
                  <Slider label={t("vignette")} min={0} max={100} value={Math.round(s.customBg.vignette * 100)} suffix="%" onChange={(v) => setBg({ vignette: v / 100 })} />
                  <Slider label={t("saturation")} min={0} max={200} value={Math.round(s.customBg.saturation * 100)} suffix="%" onChange={(v) => setBg({ saturation: v / 100 })} />
                  <Slider label={t("maskOpacity")} min={0} max={90} value={Math.round(s.customBg.maskOpacity * 100)} suffix="%" onChange={(v) => setBg({ maskOpacity: v / 100 })} />
                  <Slider label={t("dynamicStrength")} min={0} max={100} value={Math.round(s.customBg.dynamicStrength * 100)} suffix="%" onChange={(v) => setBg({ dynamicStrength: v / 100 })} />
                  <Slider label={t("parallaxStrength")} min={0} max={100} value={Math.round(s.customBg.parallaxStrength * 100)} suffix="%" onChange={(v) => setBg({ parallaxStrength: v / 100 })} />
                  {s.customBg.type === "video" && (
                    <Check label={t("playVideoBg")} checked={s.customBg.playVideo} disabled={s.safeMode}
                      onChange={(v) => setBg({ playVideo: v })} />
                  )}
                </>
              )}
              <Field label={t("perfMode")}>
                <select value={s.perfMode} onChange={(e) => set("perfMode", e.target.value as Settings["perfMode"])}>
                  <option value="high">{t("perfHigh")}</option>
                  <option value="balanced">{t("perfBalanced")}</option>
                  <option value="eco">{t("perfEco")}</option>
                  <option value="static">{t("perfStatic")}</option>
                  <option value="auto">{t("perfAuto")}</option>
                </select>
              </Field>
              <Field label={t("bgTier")}>
                <select value={String(s.bgTier)} onChange={(e) => set("bgTier", Number(e.target.value))}>
                  <option value="0">{t("bgTierAuto")}</option>
                  {TIER_LABELS.map((_, i) => (
                    <option key={i + 1} value={i + 1}>L{i + 1} · {(lang === "zh" ? TIER_LABELS : TIER_LABELS_EN)[i]}</option>
                  ))}
                </select>
              </Field>
            </>
          )}

          {tab === "editor" && (
            <>
              <Slider label={t("editorWidth")} min={58} max={72} value={s.editorWidthPct} suffix="%" onChange={(v) => set("editorWidthPct", v)} />
              <Field label={t("alignEditor")}>
                <select value={s.editorAlign} onChange={(e) => set("editorAlign", e.target.value as Settings["editorAlign"])}>
                  <option value="center">{t("posCenter")}</option>
                  <option value="left">{t("posLeft")}</option>
                  <option value="right">{t("posRight")}</option>
                </select>
              </Field>
              <Field label={t("fontFamily")}>
                <select value={s.fontFamily} onChange={(e) => set("fontFamily", e.target.value)}>
                  {FONT_STACKS.map((f) => <option key={f.value} value={f.value}>{f.label}</option>)}
                </select>
              </Field>
              <Slider label={t("baseFontSize")} min={12} max={26} value={s.fontSize} suffix="px" onChange={(v) => set("fontSize", v)} />
              <Slider label={t("lineHeight")} min={130} max={240} value={Math.round(s.lineHeight * 100)} suffix="%" onChange={(v) => set("lineHeight", v / 100)} />
              <Slider label={t("autosaveDelay")} min={300} max={3000} step={100} value={s.autosaveDelayMs} suffix="ms" onChange={(v) => set("autosaveDelayMs", v)} />
              <Check label={t("statusBar")} checked={s.showStatusBar} onChange={(v) => set("showStatusBar", v)} />
            </>
          )}

          {tab === "mindmap" && (
            <>
              <Check label={t("gridDefault")} checked={s.mindDefaults.gridEnabled} onChange={(v) => setMind({ gridEnabled: v })} />
              <Field label={t("gridMode")}>
                <select value={s.mindDefaults.gridMode} onChange={(e) => setMind({ gridMode: e.target.value as import("../../lib/settings").GridMode })}>
                  <option value="grid">{lang === "zh" ? "方格" : "Grid"}</option>
                  <option value="dot">{lang === "zh" ? "点阵" : "Dots"}</option>
                  <option value="iso">{lang === "zh" ? "等距" : "Isometric"}</option>
                  <option value="none">{lang === "zh" ? "无" : "None"}</option>
                </select>
              </Field>
              <Field label={`${t("gridMode")} · ${t("color")}`}>
                <div className="row gap8">
                  <input type="color" value={s.mindDefaults.gridColor} onChange={(e) => setMind({ gridColor: e.target.value })} />
                  <input
                    type="range" min={4} max={60} value={Math.round(s.mindDefaults.gridOpacity * 100)}
                    onChange={(e) => setMind({ gridOpacity: Number(e.target.value) / 100 })}
                  />
                  <span className="dim small">{Math.round(s.mindDefaults.gridOpacity * 100)}%</span>
                </div>
              </Field>
              <Check label={lang === "zh" ? "智能对齐辅助线" : "Smart alignment guides"} checked={s.mindDefaults.guidesEnabled} onChange={(v) => setMind({ guidesEnabled: v })} />
              <Check label={t("snapDefault")} checked={s.mindDefaults.snapEnabled} onChange={(v) => setMind({ snapEnabled: v })} />
              <Field label={t("defaultShape")}>
                <select value={s.mindDefaults.defaultShape} onChange={(e) => setMind({ defaultShape: e.target.value as MindDefaults["defaultShape"] })}>
                  {(["rect", "rounded", "circle", "triangle", "diamond", "pentagon", "hexagon", "heptagon"] as const).map((sh) => (
                    <option key={sh} value={sh}>{t(`shape${sh.charAt(0).toUpperCase()}${sh.slice(1)}`)}</option>
                  ))}
                </select>
              </Field>
              <Slider label={t("resizeSensitivity")} min={4} max={24} value={s.mindDefaults.resizeSensitivity} suffix="px" onChange={(v) => setMind({ resizeSensitivity: v })} />
              <Field label={t("edgeStyleDefault")}>
                <select value={s.mindDefaults.edgeStyle} onChange={(e) => setMind({ edgeStyle: e.target.value as MindDefaults["edgeStyle"] })}>
                  <option value="solid">{t("lsSolid")}</option>
                  <option value="dashed">{t("lsDashed")}</option>
                  <option value="dotted">{t("lsDotted")}</option>
                </select>
              </Field>
              <Check label={t("edgeAnimDefault")} checked={s.mindDefaults.edgeAnim} onChange={(v) => setMind({ edgeAnim: v })} />
              <Slider label={t("wasdSpeed")} min={200} max={1200} step={40} value={s.mindDefaults.wasdSpeed} suffix="px/s" onChange={(v) => setMind({ wasdSpeed: v })} />
            </>
          )}

          {tab === "general" && (
            <>
              <Field label={t("language")}>
                <select value={lang} onChange={(e) => { const v = e.target.value as "zh" | "en"; setLang(v); }}>
                  <option value="zh">中文</option>
                  <option value="en">English</option>
                </select>
              </Field>
              <Check label={t("launchAnim")} checked={s.launchAnim} onChange={(v) => set("launchAnim", v)} />
              <Check label={t("reduceMotion")} checked={s.reduceMotion} onChange={(v) => set("reduceMotion", v)} />
              <Check label={t("safeMode")} checked={s.safeMode} onChange={(v) => set("safeMode", v)} />
              <Slider label={t("uiZoom")} min={85} max={130} value={Math.round(s.uiZoom * 100)} suffix="%" onChange={(v) => set("uiZoom", v / 100)} />
              <hr />
              <button
                type="button"
                className="btn ghost"
                onClick={() =>
                  void askConfirm({ title: t("resetUiSettings"), body: t("resetUiConfirm"), danger: false }).then(async (ok) => {
                    if (!ok) return;
                    try {
                      await ipc.resetUiSettings();
                      props.onChange(structuredClone(DEFAULT_SETTINGS));
                      pushToast("success", t("resetUiSettings"));
                    } catch (e) {
                      pushToast("error", t("resetUiSettings"), errMessage(e).message);
                    }
                  })
                }
              >
                <RotateCcw size={13} /> {t("resetUiSettings")}
              </button>
            </>
          )}

          {tab === "data" && (
            <>
              <Field label={t("dataDir")}>
                <code className="path-code">{props.bootstrap?.dataDir ?? "…"}</code>
              </Field>
              <div className="row gap8">
                <button type="button" className="btn ghost" onClick={() => void ipc.openPath(props.bootstrap?.dataDir ?? "").catch((e) => pushToast("error", t("openDataDir"), errMessage(e).message))}>
                  <FolderOpen size={13} /> {t("openDataDir")}
                </button>
                <button
                  type="button"
                  className="btn primary"
                  disabled={busy}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      await ipc.createBackup("manual");
                      pushToast("success", t("backupOk"));
                      setBackups(await ipc.listBackups());
                    } catch (e) {
                      pushToast("error", t("backupsTitle"), errMessage(e).message);
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  {t("createBackupNow")}
                </button>
              </div>
              <h4>{t("backupsTitle")}</h4>
              <div className="backup-list">
                {backups.length === 0 && <p className="dim small">—</p>}
                {backups.map((b) => (
                  <div key={b.id} className={`backup-row ${b.status !== "ok" ? "missing" : ""}`}>
                    <span className="ellipsis" title={b.fileName}>{b.fileName}</span>
                    <span className="dim small">{formatBytes(b.size)}</span>
                    <span className="flex-1" />
                    {b.status !== "ok" ? (
                      <span className="dim small">{t("backupMissing")}</span>
                    ) : (
                      <>
                        <button type="button" className="icon-btn tiny" data-tip={t("export")} aria-label={t("export")}
                          onClick={async () => {
                            const p = await save({ defaultPath: b.fileName, filters: [{ name: "SQLite backup", extensions: ["db"] }] });
                            if (typeof p !== "string") return;
                            await ipc.exportBackup(b.fileName, p).then(() => pushToast("success", t("exportedOk"), p)).catch((e) => pushToast("error", t("export"), errMessage(e).message));
                          }}
                        ><Download size={13} /></button>
                        <button type="button" className="icon-btn tiny" data-tip={t("restoreBackupAction")} aria-label={t("restoreBackupAction")}
                          onClick={() =>
                            void askConfirm({ title: t("restoreBackupAction"), body: t("restoreBackupConfirm"), danger: true }).then(async (ok) => {
                              if (!ok) return;
                              await ipc.restoreBackup(b.fileName)
                                .then(() => pushToast("success", t("restoredRestart")))
                                .catch((e) => pushToast("error", t("restoreBackupAction"), errMessage(e).message));
                            })
                          }
                        ><RotateCcw size={13} /></button>
                        <button type="button" className="icon-btn tiny danger-hover" data-tip={t("deleteBackupAction")} aria-label={t("deleteBackupAction")}
                          onClick={() =>
                            void askConfirm({ title: t("deleteBackupAction"), body: b.fileName, danger: true }).then(async (ok) => {
                              if (!ok) return;
                              await ipc.deleteBackup(b.fileName).then(async () => setBackups(await ipc.listBackups())).catch((e) => pushToast("error", t("deleteBackupAction"), errMessage(e).message));
                            })
                          }
                        ><Trash2 size={13} /></button>
                      </>
                    )}
                  </div>
                ))}
              </div>
              <p className="dim small offline-note">{t("offlineNote")} · v{props.bootstrap?.version ?? "?"} · schema v{props.bootstrap?.schemaVersion ?? "?"}{props.bootstrap?.portable ? " · portable" : ""}</p>
            </>
          )}
        </div>
      </div>
    </Modal>
  );
}

function Field(props: { label: string; children: React.ReactNode }): React.ReactElement {
  return (
    <label className="field">
      <span className="field-label">{props.label}</span>
      {props.children}
    </label>
  );
}

function Slider(props: { label: string; min: number; max: number; step?: number; value: number; suffix?: string; onChange: (v: number) => void }): React.ReactElement {
  return (
    <Field label={`${props.label}: ${props.value}${props.suffix ?? ""}`}>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step ?? 1}
        value={clamp(props.value, props.min, props.max)}
        onChange={(e) => props.onChange(Number(e.target.value))}
      />
    </Field>
  );
}

function Check(props: { label: string; checked: boolean; disabled?: boolean; onChange: (v: boolean) => void }): React.ReactElement {
  return (
    <label className={`check-line ${props.disabled ? "disabled" : ""}`}>
      <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={(e) => props.onChange(e.target.checked)} />
      {props.label}
    </label>
  );
}
