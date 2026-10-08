import { useEffect, useMemo, useState } from "react";
import { Monitor, Palette, SunMoon, Globe2, Clock3, Image as ImageIcon } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { Modal } from "../../components/Modal";

/**
 * AI-20 质量门禁与收官组 — V-93 Windows 偏好搬家向导。
 *
 * 只读读取当前 Windows 用户偏好（壁纸/强调色/深浅色/区域格式/24h 制），
 * 逐项预览、勾选导入（跳过全部项也能正常完成）。
 *
 * 红线：只读读取（注册表用户级）；不导入任何系统隐私数据（只搬偏好）；
 * 不做反向导出（写回系统 = 越权）；读取失败项如实显示「不可用」。
 */

const ACCENT_STORE_KEY = "variable:prefs:accent:v1";

/** 强调色轻量应用（--accent/--accent-soft 内联覆盖；退出持久）。 */
export function applyAccentOverride(hex: string | null): void {
  const root = document.documentElement;
  if (!hex) {
    root.style.removeProperty("--accent");
    root.style.removeProperty("--accent-soft");
    try {
      localStorage.removeItem(ACCENT_STORE_KEY);
    } catch {
      /* ignore */
    }
    return;
  }
  root.style.setProperty("--accent", hex);
  root.style.setProperty("--accent-soft", `${hex}29`);
  try {
    localStorage.setItem(ACCENT_STORE_KEY, hex);
  } catch {
    /* ignore */
  }
}

/** 启动时恢复已导入的强调色（App 调用一次）。 */
export function restoreAccentOverride(): void {
  try {
    const hex = localStorage.getItem(ACCENT_STORE_KEY);
    if (hex && /^#[0-9a-fA-F]{6}$/.test(hex)) applyAccentOverride(hex);
  } catch {
    /* ignore */
  }
}

interface PrefsRow {
  id: "wallpaper" | "accent" | "theme" | "locale" | "hour24";
  icon: React.ReactNode;
  label: string;
  value: string;
  /** 应用该行的设置补丁（返回 null = 本行不可应用） */
  preview: string | null;
}

export function PrefsImportGate(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement | null {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState<Record<string, boolean>>({});
  const [prefs, setPrefs] = useState<Awaited<ReturnType<typeof ipc.sysPrefsRead>> | null>(null);

  const firstRun = !props.settings.prefsImportDone;

  useEffect(() => {
    if (firstRun) setOpen(true);
  }, [firstRun]);

  const read = async (): Promise<void> => {
    setLoading(true);
    setError(null);
    try {
      const p = await ipc.sysPrefsRead();
      setPrefs(p);
      setChecked({
        wallpaper: !!p.wallpaperPath,
        accent: !!p.accentColor,
        theme: p.lightTheme !== null,
        locale: !!p.localeName,
        hour24: p.hour24 !== null,
      });
    } catch (e) {
      setError(errMessage(e).message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (open) void read();
  }, [open]);

  const rows: PrefsRow[] = useMemo(() => {
    if (!prefs) return [];
    const out: PrefsRow[] = [];
    out.push({
      id: "wallpaper",
      icon: <ImageIcon size={16} />,
      label: t("v93RowWallpaper"),
      value: prefs.wallpaperPath || t("v93Unavailable"),
      preview: prefs.wallpaperPath || null,
    });
    out.push({
      id: "accent",
      icon: <Palette size={16} />,
      label: t("v93RowAccent"),
      value: prefs.accentColor ?? t("v93Unavailable"),
      preview: prefs.accentColor,
    });
    out.push({
      id: "theme",
      icon: <SunMoon size={16} />,
      label: t("v93RowTheme"),
      value: prefs.lightTheme === null ? t("v93Unavailable") : prefs.lightTheme ? t("v93Light") : t("v93Dark"),
      preview: prefs.lightTheme === null ? null : (prefs.lightTheme ? "paper" : "deep-space"),
    });
    out.push({
      id: "locale",
      icon: <Globe2 size={16} />,
      label: t("v93RowLocale"),
      value: prefs.localeName ?? t("v93Unavailable"),
      preview: prefs.localeName,
    });
    out.push({
      id: "hour24",
      icon: <Clock3 size={16} />,
      label: t("v93RowHour24"),
      value: prefs.hour24 === null ? t("v93Unavailable") : prefs.hour24 ? "24h" : "12h",
      preview: prefs.hour24 === null ? null : "follow-locale",
    });
    return out;
  }, [prefs, t]);

  const apply = async (): Promise<void> => {
    const anyChecked = rows.some((r) => checked[r.id]);
    if (anyChecked) {
      const s = props.settings;
      const patch: Partial<Settings> = {};
      if (checked.theme) {
        const themePreview = rows.find((r) => r.id === "theme")?.preview;
        if (themePreview) patch.theme = themePreview as Settings["theme"];
      }
      if (checked.wallpaper) {
        const wp = rows.find((r) => r.id === "wallpaper")?.preview;
        if (wp) {
          // 壁纸路径存在性校验（只读 check）；失败如实 toast 跳过该项
          const chk = await ipc.checkPaths([wp]).catch(() => []);
          if (chk[0]?.exists) {
            // living 活化模式：Windows 桌面动态壁纸（Wallpaper Engine 等）读到的
            // 是静态落盘图，活化层（粒子 + Ken Burns）让导入后依旧有呼吸感。
            patch.wallpaperMode = "living";
            patch.customBg = { ...s.customBg, type: "image", imagePath: wp };
          } else {
            pushToast("error", t("v93WallpaperTitle"), t("v93WallpaperMissing"));
          }
        }
      }
      if (checked.accent) {
        const ac = rows.find((r) => r.id === "accent")?.preview;
        if (ac) applyAccentOverride(ac);
      }
      if (checked.locale || checked.hour24) {
        // 区域格式跟随（M-78）：12/24h 与日期序一并随系统口径
        patch.localeFormatFollow = true;
      }
      if (Object.keys(patch).length > 0) props.onPatch(patch);
      pushToast("success", t("v93DoneTitle"), t("v93DoneBody"));
    }
    props.onPatch({ prefsImportDone: true });
    setOpen(false);
  };

  if (!open) return null;
  return (
    <Modal open onClose={() => { props.onPatch({ prefsImportDone: true }); setOpen(false); }} title={t("v93Title")} width={520}>
      <div className="col gap8" data-testid="prefs-wizard">
        <p className="dim small" style={{ whiteSpace: "pre-line" }}>{t("v93Intro")}</p>
        {loading && <p className="dim small" aria-busy="true">{t("v93Reading")}</p>}
        {error && <p className="small" style={{ color: "var(--danger)" }}>{t("v93ProbeFail")}: {error}</p>}
        {rows.map((r) => (
          <label key={r.id} className={`check-line ${r.preview ? "" : "disabled"}`} data-testid={`prefs-row-${r.id}`}>
            <input
              type="checkbox"
              checked={!!checked[r.id] && !!r.preview}
              disabled={!r.preview}
              onChange={(e) => setChecked((c) => ({ ...c, [r.id]: e.target.checked }))}
            />
            <span className="row gap8" style={{ minWidth: 0 }}>
              {r.icon}
              <span className="small">{r.label}</span>
              <span className="dim small ellipsis" style={{ flex: 1 }}>{r.value}</span>
              {r.id === "accent" && r.preview && (
                <i style={{ width: 14, height: 14, borderRadius: 3, background: r.preview, border: "1px solid var(--stroke)" }} />
              )}
            </span>
          </label>
        ))}
        <p className="dim small">{t("v93PrivacyNote")}</p>
        <div className="row gap8" style={{ justifyContent: "flex-end" }}>
          <button type="button" className="btn ghost" onClick={() => { props.onPatch({ prefsImportDone: true }); setOpen(false); }}>
            {t("v93SkipAll")}
          </button>
          <button type="button" className="btn primary" data-testid="prefs-apply" onClick={() => void apply()} disabled={loading}>
            {t("v93Apply")}
          </button>
        </div>
      </div>
    </Modal>
  );
}

/** 设置页手动触发入口（双入口之二：关于页按钮）。 */
export function PrefsImportEntry(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const [gate, setGate] = useState(false);
  return (
    <>
      <button type="button" className="btn" data-testid="prefs-import-entry" onClick={() => setGate(true)}>
        <Monitor size={14} style={{ marginRight: 4, verticalAlign: -2 }} />
        {t("v93EntryBtn")}
      </button>
      {gate && (
        <PrefsImportGateModal
          settings={props.settings}
          onPatch={props.onPatch}
          onClose={() => setGate(false)}
        />
      )}
    </>
  );
}

/** 手动入口用包装（绕过 firstRun 门控直接打开：向导本体按「未导入」态渲染）。 */
function PrefsImportGateModal(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
  onClose: () => void;
}): React.ReactElement | null {
  return (
    <PrefsImportGate
      settings={{ ...props.settings, prefsImportDone: false }}
      onPatch={(patch) => {
        props.onPatch(patch);
        if ("prefsImportDone" in patch) props.onClose();
      }}
    />
  );
}
