import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import {
  FolderOpen, Download, Trash2, RotateCcw, HardDrive, ShieldCheck, Lock, Unlock, Plus, Monitor,
} from "lucide-react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import {
  errMessage, ipc,
  type AuditFinding, type FileCheck, type PackProgress, type UsbStatus,
  type VaultItem, type VaultStatus, type WpMonitor,
} from "../../lib/ipc";
import { DEFAULT_SETTINGS, type CustomBg, type MindDefaults, type Settings, type ThemeId } from "../../lib/settings";
import { formatBytes, clamp } from "../../lib/format";
import { pushToast, uiStore, useUi } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { Modal } from "../../components/Modal";
import type { BackupInfo, BootstrapInfo } from "../../lib/types";
import { wallpaperUsesMedia } from "../../system/wallpaper/WallpaperLayer";
import { SHORTCUT_ACTIONS, findConflicts, normalizeAccel } from "../../lib/shortcuts";

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
  const [usb, setUsb] = useState<UsbStatus | null>(null);
  const [usbProgress, setUsbProgress] = useState<PackProgress | null>(null);
  const [usbBad, setUsbBad] = useState<FileCheck[] | null>(null);
  // 批次E-6：多显示器 / 批次E-7：保险箱与自检 / U 盘向导
  const [monitors, setMonitors] = useState<WpMonitor[] | null>(null);
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [vaultItems, setVaultItems] = useState<VaultItem[]>([]);
  const [vaultPw, setVaultPw] = useState("");
  const [vaultPw2, setVaultPw2] = useState("");
  const [audit, setAudit] = useState<AuditFinding[] | null>(null);
  const [usbWizardDir, setUsbWizardDir] = useState<string | null>(null); // null=关闭, ""=待选择, 非空=打包中/校验
  const [wizardVerify, setWizardVerify] = useState<FileCheck[] | null>(null);
  const s = props.settings;

  useEffect(() => {
    if (isOpen && tab === "data") {
      void ipc.listBackups().then(setBackups).catch(() => setBackups([]));
      void ipc.usbStatus().then(setUsb).catch(() => setUsb(null));
      void reloadVault();
    }
    if (isOpen && tab === "appearance") {
      void ipc.wpMonitors().then(setMonitors).catch(() => setMonitors([]));
    }
  }, [isOpen, tab]);

  // M8：打包进度事件（真实文件计数，非时间线）
  useEffect(() => {
    const un = listen<PackProgress>("usb://progress", (e) => {
      setUsbProgress(e.payload.phase === "done" ? null : e.payload);
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, []);

  if (!isOpen) return null;

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => props.onChange({ [key]: value } as Partial<Settings>);
  const setBg = (patch: Partial<CustomBg>) => props.onChange({ customBg: { ...s.customBg, ...patch } });
  const setMind = (patch: Partial<MindDefaults>) => props.onChange({ mindDefaults: { ...s.mindDefaults, ...patch } });

  const tabs: { id: string; label: string }[] = [
    { id: "appearance", label: t("appearance") },
    { id: "editor", label: t("editorTab") },
    { id: "mindmap", label: t("mindmapTab") },
    { id: "general", label: t("general") },
    { id: "shortcuts", label: t("scTitle") },
    { id: "data", label: t("data") },
    { id: "about", label: t("aboutVariable") },
  ];

  // ---- 批次E（规格 4.7）快捷键自定义：编辑态（accel 文本）+ 冲突检测 + 导入/导出 ----
  const [bindDraft, setBindDraft] = useState<Record<string, string> | null>(null);
  // 批次E-8：关于页版本号（tauri.conf.json version，惰性读取）
  const [aboutVersion, setAboutVersion] = useState<string>("?");
  useEffect(() => {
    if (tab !== "about" || aboutVersion !== "?") return;
    void getVersion().then(setAboutVersion).catch(() => {});
  }, [tab, aboutVersion]);
  const binds = bindDraft ?? (s.shortcutBinds ?? {});
  const fullBinds = SHORTCUT_ACTIONS.map((a) => ({ action: a.id, accel: binds[a.id] ?? a.accel }));
  const conflicts = findConflicts(fullBinds);
  const invalidBinds = fullBinds.filter((b) => normalizeAccel(b.accel) === null).map((b) => b.action);

  const applyBinds = (next: Record<string, string>): void => {
    setBindDraft(null);
    set("shortcutBinds", next);
  };

  async function pickBackground(kind: "image" | "video"): Promise<void> {
    const p = await open({ multiple: false, filters: kind === "image" ? IMG_FILTERS : VID_FILTERS });
    if (typeof p !== "string") return;
    // Reference the ORIGINAL path (validated); user can relocate on failure.
    const check = await ipc.checkPaths([p]).catch(() => []);
    if (!check[0]?.exists) {
      pushToast("error", lang !== "en" ? "文件不可读" : "File not readable");
      return;
    }
    setBg(kind === "image" ? { type: "image", imagePath: p } : { type: "video", videoPath: p, playVideo: true });
  }

  // ---- M8 U 盘便携：打包向导（选目标 → 打包 → 校验） ----

  async function packToUsb(): Promise<void> {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    const ok = await askConfirm({
      title: t("usbPack"),
      body: t("usbPackConfirmBody", { dir }),
    });
    if (!ok) return;
    // 批次E-7：向导化 —— 选定目标后内联展示打包进度，完成后自动进入校验步骤
    setUsbBad(null);
    setWizardVerify(null);
    setUsbWizardDir(dir);
    try {
      await ipc.usbPack(dir);
      setUsbProgress(null);
      pushToast("success", t("usbPackDone"), dir);
      void ipc.usbStatus().then(setUsb).catch(() => {});
      // 打包完成 → 自动校验
      try {
        const checks = await ipc.usbVerify(dir);
        setWizardVerify(checks);
        if (checks.every((c) => c.ok)) pushToast("success", t("usbVerifyOk"), dir);
      } catch {
        setWizardVerify(null);
      }
    } catch (e) {
      setUsbProgress(null);
      setUsbWizardDir(null);
      pushToast("error", t("usbPack"), errMessage(e).message);
    }
  }

  async function verifyUsbBundle(): Promise<void> {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    try {
      const checks = await ipc.usbVerify(dir);
      const bad = checks.filter((c) => !c.ok);
      setUsbBad(bad.length ? bad : []);
      if (bad.length === 0) pushToast("success", t("usbVerifyOk"), dir);
    } catch (e) {
      pushToast("error", t("usbVerify"), errMessage(e).message);
    }
  }

  const usbPhaseLabel = (p: PackProgress): string => {
    if (p.phase === "copy") return t("usbPhaseCopy", { done: p.done, total: p.total });
    if (p.phase === "exe") return t("usbPhaseExe");
    if (p.phase === "manifest") return t("usbPhaseManifest");
    return t("usbPhaseCollect");
  };

  // ---- 批次E-7：隐私保险箱 ----

  async function reloadVault(): Promise<void> {
    try {
      const st = await ipc.vaultStatus();
      setVault(st);
      setVaultItems(st.unlocked ? await ipc.vaultList() : []);
    } catch {
      setVault(null);
    }
  }

  async function initVault(): Promise<void> {
    if (vaultPw.length < 4) return void pushToast("error", t("vaultTitle"), t("vaultPwShort"));
    if (vaultPw !== vaultPw2) return void pushToast("error", t("vaultTitle"), t("vaultPwMismatch"));
    try {
      await ipc.vaultInit(vaultPw);
      setVaultPw(""); setVaultPw2("");
      pushToast("success", t("vaultTitle"), t("vaultInitOk"));
      await reloadVault();
    } catch (e) {
      pushToast("error", t("vaultTitle"), errMessage(e).message);
    }
  }

  async function unlockVault(): Promise<void> {
    try {
      await ipc.vaultUnlock(vaultPw);
      setVaultPw("");
      pushToast("success", t("vaultTitle"), t("vaultUnlockOk"));
      await reloadVault();
    } catch (e) {
      pushToast("error", t("vaultTitle"), errMessage(e).message);
    }
  }

  async function importToVault(): Promise<void> {
    const p = await open({ multiple: false });
    if (typeof p !== "string") return;
    try {
      await ipc.vaultImport(p, false);
      pushToast("success", t("vaultImport"), p);
      await reloadVault();
    } catch (e) {
      pushToast("error", t("vaultImport"), errMessage(e).message);
    }
  }

  async function exportFromVault(name: string): Promise<void> {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    try {
      const dest = await ipc.vaultExport(name, dir);
      pushToast("success", t("vaultExport"), dest);
    } catch (e) {
      pushToast("error", t("vaultExport"), errMessage(e).message);
    }
  }

  async function destroyFromVault(name: string): Promise<void> {
    const ok = await askConfirm({ title: t("vaultDestroy"), body: name, danger: true, okLabel: t("vaultDestroy") });
    if (!ok) return;
    try {
      await ipc.vaultDestroy(name);
      pushToast("success", t("vaultDestroy"), name);
      await reloadVault();
    } catch (e) {
      pushToast("error", t("vaultDestroy"), errMessage(e).message);
    }
  }

  async function runAudit(): Promise<void> {
    setBusy(true);
    try {
      setAudit(await ipc.privacyAudit());
    } catch (e) {
      pushToast("error", t("privacyAudit"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  // ---- 批次E-6：多显示器壁纸 + 每日缓存目录 ----

  async function pickPoolDir(): Promise<void> {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return;
    set("wallpaperPoolDir", dir);
    if (!s.wallpaperDaily) set("wallpaperDaily", true);
  }

  async function setMonitorWallpaper(m: WpMonitor): Promise<void> {
    const p = await open({ multiple: false, filters: IMG_FILTERS });
    if (typeof p !== "string") return;
    try {
      await ipc.wpSetMonitor(m.id, p);
      pushToast("success", t("wpMonitorSet"), p);
    } catch (e) {
      pushToast("error", t("wpMonitorSet"), errMessage(e).message);
    }
  }

  async function applyAllMonitors(): Promise<void> {
    if (!s.customBg.imagePath) return void pushToast("info", t("wpMonitorAll"), t("wpNeedImage"));
    try {
      await ipc.wpSetMonitor("", s.customBg.imagePath);
      pushToast("success", t("wpMonitorAll"), s.customBg.imagePath);
    } catch (e) {
      pushToast("error", t("wpMonitorAll"), errMessage(e).message);
    }
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
              <Field label={t("wallpaperMode")}>
                <select
                  value={s.wallpaperMode}
                  onChange={(e) => set("wallpaperMode", e.target.value as Settings["wallpaperMode"])}
                >
                  <option value="gravity">{t("wpGravity")}</option>
                  <option value="solid">{t("wpSolid")}</option>
                  <option value="image">{t("wpImage")}</option>
                  <option value="video">{t("wpVideo")}</option>
                  <option value="hybrid">{t("wpHybrid")}</option>
                </select>
              </Field>
              <Field label={t("iconSize")}>
                <select
                  value={String(s.iconSize)}
                  onChange={(e) => set("iconSize", Number(e.target.value) as Settings["iconSize"])}
                >
                  <option value="32">{t("iconSmall")} · 32</option>
                  <option value="48">{t("iconMedium")} · 48</option>
                  <option value="64">{t("iconLarge")} · 64</option>
                </select>
              </Field>
              {/* 批次D（规格 4.3.5）：窗口控制按钮位置 */}
              <Field label={t("winControls")}>
                <div className="row gap8 wrap">
                  <select
                    value={s.winControls}
                    onChange={(e) => set("winControls", e.target.value as Settings["winControls"])}
                  >
                    <option value="mac">{t("winControlsMac")}</option>
                    <option value="windows">{t("winControlsWin")}</option>
                  </select>
                  <span className="dim small">{t("winControlsHint")}</span>
                </div>
              </Field>
              {/* 批次E（规格 4.4）：任务栏停靠位置四向 */}
              <Field label={t("taskbarPos")}>
                <select
                  value={s.taskbarPos}
                  onChange={(e) => set("taskbarPos", e.target.value as Settings["taskbarPos"])}
                >
                  <option value="bottom">{t("tbPosBottom")}</option>
                  <option value="left">{t("tbPosLeft")}</option>
                  <option value="right">{t("tbPosRight")}</option>
                  <option value="top">{t("tbPosTop")}</option>
                </select>
              </Field>
              <Field label={t("theme")}>
                <select value={s.theme} onChange={(e) => set("theme", e.target.value as ThemeId)}>
                  <option value="deep-space">{t("themeDeepSpace")}</option>
                  <option value="paper">{t("themePaper")}</option>
                  <option value="minimal-black">{t("themeMinimalBlack")}</option>
                  <option value="custom">{t("themeCustom")}</option>
                </select>
              </Field>
              {(s.theme === "custom" || wallpaperUsesMedia(s.wallpaperMode)) && (
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
              {/* 批次E-6：每日自动换（本地缓存池，零网络） */}
              <Field label={t("wpDaily")}>
                <div className="row gap8 wrap">
                  <Check label={t("wpDaily")} checked={s.wallpaperDaily} onChange={(v) => set("wallpaperDaily", v)} />
                  <input className="text-input flex-1" readOnly value={s.wallpaperPoolDir} placeholder={t("wpPoolDir")} />
                  <button type="button" className="btn ghost" onClick={() => void pickPoolDir()}>
                    <FolderOpen size={13} /> {t("chooseFile")}
                  </button>
                </div>
                <span className="dim small">{t("wpDailyHint")}</span>
              </Field>
              {/* 批次E-6：多显示器独立壁纸（IDesktopWallpaper，对 Variable 之外的真实桌面生效） */}
              {monitors !== null && monitors.length > 1 && (
                <Field label={t("wpMonitors")}>
                  {monitors.map((m) => (
                    <div key={m.id} className="row gap8" style={{ marginBottom: 4 }}>
                      <Monitor size={14} className="dim" />
                      <span className="small flex-1">
                        {m.primary ? t("wpMonitorPrimary") : t("wpMonitorN")} · {m.width}×{m.height}
                      </span>
                      <button type="button" className="btn ghost tiny" onClick={() => void setMonitorWallpaper(m)}>
                        {t("wpMonitorSet")}
                      </button>
                    </div>
                  ))}
                  <button type="button" className="btn ghost tiny" onClick={() => void applyAllMonitors()}>
                    {t("wpMonitorAll")}
                  </button>
                  <span className="dim small">{t("wpMonitorHint")}</span>
                </Field>
              )}
              {/* 批次E-6：Win+Tab 多窗口切换器（可选） */}
              <Field label={t("winTabTitle")}>
                <Check label={t("winTabTitle")} checked={s.winTabSwitcher} onChange={(v) => set("winTabSwitcher", v)} />
                <span className="dim small">{t("winTabHint")}</span>
              </Field>
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
                    <option key={i + 1} value={i + 1}>L{i + 1} · {(lang !== "en" ? TIER_LABELS : TIER_LABELS_EN)[i]}</option>
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
                  <option value="grid">{lang !== "en" ? "方格" : "Grid"}</option>
                  <option value="dot">{lang !== "en" ? "点阵" : "Dots"}</option>
                  <option value="iso">{lang !== "en" ? "等距" : "Isometric"}</option>
                  <option value="none">{lang !== "en" ? "无" : "None"}</option>
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
              <Check label={lang !== "en" ? "智能对齐辅助线" : "Smart alignment guides"} checked={s.mindDefaults.guidesEnabled} onChange={(v) => setMind({ guidesEnabled: v })} />
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
                <select value={lang} onChange={(e) => { const v = e.target.value as Lang; setLang(v); }}>
                  <option value="zh">简体中文</option>
                  <option value="zh-TW">繁體中文</option>
                  <option value="en">English</option>
                </select>
              </Field>
              <Field label={t("bootAnim")}>
                <select value={s.bootAnim} onChange={(e) => set("bootAnim", e.target.value as Settings["bootAnim"])}>
                  <option value="full">{t("bootAnimFull")}</option>
                  <option value="simple">{t("bootAnimSimple")}</option>
                  <option value="none">{t("bootAnimNone")}</option>
                </select>
              </Field>
              <p className="dim small" style={{ margin: "-6px 0 0" }}>{t("bootAnimHint")}</p>
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

          {/* 批次E（规格 4.7）：快捷键自定义 + 冲突检测 + 导入/导出 */}
          {tab === "shortcuts" && (
            <>
              <p className="dim small">{t("scHint")}</p>
              <div className="sc-list">
                {SHORTCUT_ACTIONS.map((a) => {
                  const value = binds[a.id] ?? a.accel;
                  const bad = invalidBinds.includes(a.id) || conflicts.has(value);
                  const label =
                    a.labelKey === "scActLaunchN"
                      ? t("scActLaunchN", { n: a.id.replace("launch", "") })
                      : t(a.labelKey);
                  return (
                    <div key={a.id} className={`sc-row${bad ? " bad" : ""}`}>
                      <span className="sc-label">{label}</span>
                      <input
                        type="text"
                        className="sc-input"
                        value={value}
                        spellCheck={false}
                        title={t("scInputHint")}
                        onChange={(e) => {
                          // 编辑态：以原始 accel 形式保存到草稿（如 ctrl+shift+k）
                          setBindDraft({ ...binds, [a.id]: e.target.value.toLowerCase().replace(/\s+/g, "") });
                        }}
                      />
                      {bad && <span className="sc-err">{conflicts.has(value) ? t("scConflict") : t("scInvalid")}</span>}
                    </div>
                  );
                })}
              </div>
              <div className="row gap8 wrap" style={{ marginTop: 10 }}>
                <button
                  type="button"
                  className="btn primary"
                  disabled={conflicts.size > 0 || invalidBinds.length > 0}
                  onClick={() => {
                    // 归一化后应用（非法项已被禁用保存拦截）
                    const next: Record<string, string> = {};
                    for (const [k, v] of Object.entries(binds)) {
                      const norm = normalizeAccel(v);
                      if (norm) next[k] = norm;
                    }
                    applyBinds(next);
                    pushToast("success", t("scTitle"), t("scApplied"));
                  }}
                >
                  {t("scApply")}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    applyBinds({});
                    pushToast("success", t("scTitle"), t("scReset"));
                  }}
                >
                  {t("scResetDefaults")}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    void save({ defaultPath: "variable-shortcuts.json", filters: [{ name: "JSON", extensions: ["json"] }] })
                      .then((p) => {
                        if (typeof p !== "string") return;
                        return ipc
                          .writeTextFile(p, JSON.stringify(binds, null, 2))
                          .then(() => pushToast("success", t("scExportOk"), p));
                      })
                      .catch((e) => pushToast("error", t("scTitle"), errMessage(e).message));
                  }}
                >
                  {t("scExport")}
                </button>
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    void open({ multiple: false, filters: [{ name: "JSON", extensions: ["json"] }] })
                      .then(async (p) => {
                        if (typeof p !== "string") return;
                        const text = await ipc.readTextFile(p);
                        const parsed = JSON.parse(text) as Record<string, string>;
                        const next: Record<string, string> = {};
                        for (const [k, v] of Object.entries(parsed)) {
                          if (SHORTCUT_ACTIONS.some((a) => a.id === k)) {
                            const norm = normalizeAccel(v);
                            if (norm) next[k] = norm;
                          }
                        }
                        const trial = findConflicts(
                          SHORTCUT_ACTIONS.map((a) => ({ action: a.id, accel: next[a.id] ?? a.accel })),
                        );
                        if (trial.size > 0) {
                          pushToast("error", t("scImportConflict"), [...trial].join(", "));
                          return;
                        }
                        applyBinds(next);
                        pushToast("success", t("scImportOk"), p);
                      })
                      .catch((e) => pushToast("error", t("scTitle"), errMessage(e).message));
                  }}
                >
                  {t("scImport")}
                </button>
              </div>
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
              <h4>{t("usbTitle")}</h4>
              <p className="dim small">{usb ? (usb.portable ? t("usbModePortable") : t("usbModeLocal")) : "…"}</p>
              <div className="row gap8">
                <button type="button" className="btn ghost" disabled={!!usbProgress} onClick={() => void packToUsb()}>
                  <HardDrive size={13} /> {t("usbPack")}
                </button>
                <button type="button" className="btn ghost" disabled={!!usbProgress} onClick={() => void verifyUsbBundle()}>
                  <ShieldCheck size={13} /> {t("usbVerify")}
                </button>
              </div>
              {usbProgress && (
                <div className="usb-progress">
                  <div className="usb-progress-bar">
                    <span
                      style={{
                        width: `${usbProgress.total > 0 ? Math.min(100, Math.round((usbProgress.done / usbProgress.total) * 100)) : 0}%`,
                      }}
                    />
                  </div>
                  <p className="dim small">{usbPhaseLabel(usbProgress)}</p>
                </div>
              )}
              {usbBad && usbBad.length === 0 && <p className="dim small">{t("usbVerifyOk")}</p>}
              {usbBad && usbBad.length > 0 && (
                <div className="usb-bad">
                  <p className="small">{t("usbVerifyBad", { n: usbBad.length })}</p>
                  {usbBad.slice(0, 8).map((c) => (
                    <p key={c.path} className="dim small ellipsis" title={c.path}>
                      {c.path} — {c.actual === "MISSING" ? t("usbFileMissing") : `${c.actual.slice(0, 12)}…`}
                    </p>
                  ))}
                </div>
              )}
              {/* 批次E-7：打包向导第 3 步 —— 打包完成后的自动校验结果 */}
              {usbWizardDir && wizardVerify !== null && (
                <div className="usb-bad">
                  <p className="small">{t("usbWizardVerifyTitle", { dir: usbWizardDir })}</p>
                  {wizardVerify.length === 0 || wizardVerify.every((c) => c.ok) ? (
                    <p className="dim small">{t("usbVerifyOk")}（{wizardVerify.length}）</p>
                  ) : (
                    wizardVerify.filter((c) => !c.ok).slice(0, 8).map((c) => (
                      <p key={c.path} className="dim small ellipsis" title={c.path}>
                        {c.path} — {c.actual === "MISSING" ? t("usbFileMissing") : `${c.actual.slice(0, 12)}…`}
                      </p>
                    ))
                  )}
                </div>
              )}

              {/* 批次E-7：隐私保险箱（AES-256-GCM；密钥仅驻留内存） */}
              <h4>{t("vaultTitle")}</h4>
              <p className="dim small">{t("vaultHint")}</p>
              {vault === null ? (
                <p className="dim small">…</p>
              ) : !vault.initialized ? (
                <>
                  <p className="dim small">{t("vaultNotInit")}</p>
                  <div className="row gap8 wrap">
                    <input
                      type="password" className="text-input" style={{ width: 160 }}
                      placeholder={t("vaultNewPw")} value={vaultPw}
                      onChange={(e) => setVaultPw(e.target.value)}
                    />
                    <input
                      type="password" className="text-input" style={{ width: 160 }}
                      placeholder={t("vaultNewPw2")} value={vaultPw2}
                      onChange={(e) => setVaultPw2(e.target.value)}
                    />
                    <button type="button" className="btn primary" onClick={() => void initVault()}>
                      <Lock size={13} /> {t("vaultInit")}
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div className="row gap8 wrap">
                    <span className="dim small">
                      {vault.unlocked
                        ? t("vaultStateUnlocked", { n: vault.count, size: formatBytes(vault.bytes) })
                        : t("vaultStateLocked", { n: vault.count })}
                    </span>
                    {vault.unlocked ? (
                      <>
                        <button type="button" className="btn ghost" onClick={() => void ipc.vaultLock().then(reloadVault).catch(() => {})}>
                          <Lock size={13} /> {t("vaultLock")}
                        </button>
                        <button type="button" className="btn ghost" onClick={() => void importToVault()}>
                          <Plus size={13} /> {t("vaultImport")}
                        </button>
                      </>
                    ) : (
                      <div className="row gap8">
                        <input
                          type="password" className="text-input" style={{ width: 160 }}
                          placeholder={t("vaultPwInput")} value={vaultPw}
                          onChange={(e) => setVaultPw(e.target.value)}
                        />
                        <button type="button" className="btn primary" onClick={() => void unlockVault()}>
                          <Unlock size={13} /> {t("vaultUnlock")}
                        </button>
                      </div>
                    )}
                  </div>
                  {vault.unlocked && (
                    <div className="backup-list">
                      {vaultItems.length === 0 && <p className="dim small">—</p>}
                      {vaultItems.map((it) => (
                        <div key={it.name} className="backup-row">
                          <span className="ellipsis" title={it.name}>{it.name}</span>
                          <span className="dim small">{formatBytes(it.size)}</span>
                          <span className="flex-1" />
                          <button type="button" className="icon-btn tiny" data-tip={t("vaultExport")} aria-label={t("vaultExport")}
                            onClick={() => void exportFromVault(it.name)}><Download size={13} /></button>
                          <button type="button" className="icon-btn tiny danger-hover" data-tip={t("vaultDestroy")} aria-label={t("vaultDestroy")}
                            onClick={() => void destroyFromVault(it.name)}><Trash2 size={13} /></button>
                        </div>
                      ))}
                    </div>
                  )}
                </>
              )}

              {/* 批次E-7：隐私自检报告 */}
              <h4>{t("privacyAudit")}</h4>
              <div className="row gap8">
                <button type="button" className="btn ghost" disabled={busy} onClick={() => void runAudit()}>
                  <ShieldCheck size={13} /> {t("privacyAuditRun")}
                </button>
              </div>
              {audit !== null && (
                <div className="audit-list">
                  {audit.map((f) => (
                    <p key={f.id} className={`small audit-row ${f.level}`}>
                      {f.level === "pass" ? "✓" : "⚠"} {f.detail}
                    </p>
                  ))}
                </div>
              )}
              <p className="dim small offline-note">{t("offlineNote")} · v{props.bootstrap?.version ?? "?"} · schema v{props.bootstrap?.schemaVersion ?? "?"}{props.bootstrap?.portable ? " · portable" : ""}</p>
            </>
          )}

          {/* 批次E-8：关于页 —— 版本信息 + 隐私承诺 */}
          {tab === "about" && (
            <>
              <h4>{t("aboutVariable")}</h4>
              <p className="dim small" style={{ whiteSpace: "pre-line" }}>{t("aboutBody")}</p>
              <div className="backup-list" style={{ marginTop: 12 }}>
                <div className="backup-row">
                  <span className="dim small">{t("version")}</span>
                  <span className="flex-1" />
                  <span className="small">v{aboutVersion}</span>
                </div>
                <div className="backup-row">
                  <span className="dim small">Tauri / React</span>
                  <span className="flex-1" />
                  <span className="small">2.x / 18</span>
                </div>
                <div className="backup-row">
                  <span className="dim small">Schema</span>
                  <span className="flex-1" />
                  <span className="small">v{props.bootstrap?.schemaVersion ?? "?"}{props.bootstrap?.portable ? " · portable" : ""}</span>
                </div>
              </div>
              <p className="dim small offline-note" style={{ marginTop: 12 }}>{t("offlineNote")}</p>
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
