/**
 * 壁纸中心（Wallpaper Center）：Variable 界面内的本地壁纸管理器 —— 不打开 Steam、
 * 不弹 Windows 窗口（实机需求重申：必须在 Variable 里打开）。
 *
 * 三区布局：库网格（WE 扫描 + 本地目录 + 手动图片三源合并）/ 预览 + 属性面板
 * （静态图活化滑杆：粒子密度 / Ken Burns 漂移幅度 / 粒子风格）/ 播放列表底栏
 * （间隔轮换 + 随机，runner 在 DesktopShell）。
 *
 * 自挂载：模块加载即监听 `ai04:open-feature` {feature:"wallpaper-center"}（mount.ts
 * 协议，与壁纸工坊同款）；主控 `void import("<本模块>")` 一次即激活。
 * 应用壁纸 = 派发 `ai04:wallpaper-apply`（DesktopShell 消费 → onPatchSettings）。
 * GPU 自律：挂载/卸载广播 `wpcenter://open-state`（DesktopShell 抑制壁纸动画）。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { toAssetUrl } from "../../../features/background/CosmicBackground";
import { errMessage, ipc } from "../../../lib/ipc";
import { loadSettings, type CustomBg } from "../../../lib/settings";
import { mountOnEvent, installCloseHandler, dispatchClose } from "../mount";
import { cT } from "./centerLabels";
import {
  currentItem,
  dispatchCenterOpen,
  dispatchWpApply,
  fromEngineItem,
  fromImageFile,
  loadDirs,
  loadManualImages,
  loadPlaylist,
  loadPlaylistState,
  mergeLibrary,
  saveDirs,
  saveManualImages,
  savePlaylist,
  savePlaylistState,
  nextIndex,
  entryPatch,
  dispatchPlaylistChanged,
  type LibraryItem,
  type PlaylistEntry,
  type PlaylistState,
} from "./centerCore";

type TabId = "installed" | "create";

export function WallpaperCenter(): React.ReactElement {
  const t = useMemo(() => cT(), []);
  const [tab, setTab] = useState<TabId>("installed");
  const [settings, setSettings] = useState<Awaited<ReturnType<typeof loadSettings>> | null>(null);
  const [weItems, setWeItems] = useState<Parameters<typeof fromEngineItem>[0][]>([]);
  const [localItems, setLocalItems] = useState<LibraryItem[]>([]);
  const [selected, setSelected] = useState<LibraryItem | null>(null);
  const [busy, setBusy] = useState(false);
  const [weError, setWeError] = useState<string | null>(null);
  const [toast, setToast] = useState("");
  // 属性面板草稿（应用时随 customBg 一起提交）
  const [draft, setDraft] = useState<Partial<CustomBg>>({});

  // GPU 自律广播：挂载 open=true / 卸载 open=false
  useEffect(() => {
    dispatchCenterOpen(true);
    return () => dispatchCenterOpen(false);
  }, []);

  // 载入 settings（只读）+ WE 自动扫描 + 本地目录枚举
  useEffect(() => {
    let alive = true;
    void loadSettings().then((s) => {
      if (!alive) return;
      setSettings(s);
      const cur = currentItem(s.customBg);
      if (cur) setSelected(cur);
      setDraft({
        livingIntensity: s.customBg.livingIntensity,
        livingDrift: s.customBg.livingDrift,
        particleStyle: s.customBg.particleStyle,
      });
    });
    void ipc.wpEngineScan("")
      .then((list) => { if (alive) setWeItems(list); })
      .catch(() => { if (alive) setWeError(null); }); // WE 未安装是常态，非错误
    void (async () => {
      const acc: LibraryItem[] = [];
      for (const d of loadDirs()) {
        try {
          const files = await ipc.wpListImages(d);
          acc.push(...files.map((f) => fromImageFile(f.path, f.name)));
        } catch { /* 目录失效（盘符漂移）：如实跳过 */ }
      }
      acc.push(...loadManualImages().map((p) => fromImageFile(p)));
      if (alive) setLocalItems(acc);
    })();
    return () => { alive = false; };
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(""), 4000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const items = useMemo(() => {
    const we = weItems.map(fromEngineItem);
    const cur = settings ? currentItem(settings.customBg) : null;
    return cur
      ? mergeLibrary([cur, ...localItems, ...we])
      : mergeLibrary([...localItems, ...we]);
  }, [settings, weItems, localItems]);

  /** 应用壁纸：合并当前 customBg + 活化草稿 + 选中项路径 → 桌面立即生效。 */
  function apply(item: LibraryItem): void {
    const base: Record<string, unknown> = {
      ...settings?.customBg,
      ...draft,
      type: "image",
    };
    let patch: { wallpaperMode: string; customBg: Record<string, unknown> };
    switch (item.kind) {
      case "video":
        patch = { wallpaperMode: "video", customBg: { ...base, videoPath: item.path, playVideo: true } };
        break;
      case "web":
        patch = { wallpaperMode: "web", customBg: { ...base, htmlPath: item.path } };
        break;
      case "shader":
        patch = {
          wallpaperMode: "shader",
          customBg: {
            ...base,
            shaderPath: item.path,
            imagePath: item.preview ?? settings?.customBg.imagePath ?? "",
          },
        };
        break;
      default:
        patch = { wallpaperMode: "living", customBg: { ...base, imagePath: item.path } };
    }
    dispatchWpApply(patch);
    setToast(t("applied"));
    // 同步本地选中态（当前壁纸即时刷新）
    if (settings) {
      setSettings({
        ...settings,
        wallpaperMode: patch.wallpaperMode as typeof settings.wallpaperMode,
        customBg: { ...settings.customBg, ...patch.customBg } as CustomBg,
      });
      const cur = currentItem({
        imagePath: (patch.customBg.imagePath as string) ?? "",
        videoPath: (patch.customBg.videoPath as string) ?? "",
        htmlPath: (patch.customBg.htmlPath as string) ?? "",
        shaderPath: (patch.customBg.shaderPath as string) ?? "",
      });
      if (cur) setSelected(cur);
    }
  }

  async function scanWePick(): Promise<void> {
    setBusy(true);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const dir = await open({ directory: true, multiple: false });
      if (typeof dir === "string") {
        setWeItems(await ipc.wpEngineScan(dir));
        setToast(t("weScanned"));
      }
    } catch (e) {
      setWeError(errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function addDir(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const dir = await open({ directory: true, multiple: false });
      if (typeof dir !== "string") return;
      saveDirs([...new Set([...loadDirs(), dir])]);
      const files = await ipc.wpListImages(dir);
      setLocalItems((prev) => {
        const known = new Set(prev.map((p) => p.path));
        return [...prev, ...files.map((f) => fromImageFile(f.path, f.name)).filter((x) => !known.has(x.path))];
      });
      setToast(t("dirAdded"));
    } catch (e) {
      setToast(errMessage(e).message);
    }
  }

  async function addManual(): Promise<void> {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const sel = await open({
        multiple: true,
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
      });
      const list = Array.isArray(sel) ? sel : sel ? [sel] : [];
      if (list.length === 0) return;
      saveManualImages([...new Set([...loadManualImages(), ...list])]);
      setLocalItems((prev) => {
        const known = new Set(prev.map((p) => p.path));
        return [...prev, ...list.map((p) => fromImageFile(p)).filter((x) => !known.has(x.path))];
      });
      setToast(t("imgAdded"));
    } catch (e) {
      setToast(errMessage(e).message);
    }
  }

  const sel = selected;

  return (
    <div
      className="wp-center-root"
      role="dialog"
      aria-label={t("title")}
      tabIndex={-1}
      onKeyDown={(e) => {
        if (e.key === "Escape") dispatchClose("wallpaper-center");
      }}
    >
      <header className="wp-center-header">
        <h2>{t("title")}</h2>
        <nav className="wp-center-tabs">
          <button className={tab === "installed" ? "on" : ""} onClick={() => setTab("installed")}>
            {t("tabInstalled")}
          </button>
          <button className={tab === "create" ? "on" : ""} onClick={() => setTab("create")}>
            {t("tabCreate")}
          </button>
        </nav>
        <div className="wp-center-actions">
          <button disabled={busy} onClick={() => void scanWePick()}>{t("scanWe")}</button>
          <button onClick={() => void addDir()}>{t("addDir")}</button>
          <button onClick={() => void addManual()}>{t("addImage")}</button>
          <button className="wp-center-close" onClick={() => dispatchClose("wallpaper-center")}>✕</button>
        </div>
      </header>

      {weError && <p className="wp-center-error" role="alert">{weError}</p>}

      {tab === "create" ? (
        <div className="wp-center-create">
          <p>{t("createHint")}</p>
          <button
            className="wp-center-open-studio"
            onClick={() => window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "wallpaper-studio" } }))}
          >
            {t("openStudio")}
          </button>
        </div>
      ) : (
        <div className="wp-center-main">
          <div className="wp-center-lib" role="listbox" aria-label={t("tabInstalled")}>
            {items.length === 0 && <p className="wp-center-empty">{t("empty")}</p>}
            {items.map((it) => (
              <button
                key={it.id}
                role="option"
                aria-selected={sel?.id === it.id}
                className={`wp-center-card ${sel?.id === it.id ? "on" : ""} ${it.supported ? "" : "off"}`}
                onClick={() => setSelected(it)}
                onDoubleClick={() => it.supported && apply(it)}
              >
                <span className="wp-center-thumb">
                  {it.preview ? (
                    <img src={toAssetUrl(it.preview)} alt="" loading="lazy" />
                  ) : (
                    <span className="wp-center-thumb-none">{kindGlyph(it.kind)}</span>
                  )}
                </span>
                <span className="wp-center-card-title" title={it.title}>{it.title}</span>
                <span className="wp-center-kind">{t(`kind_${it.kind}` as never)}</span>
              </button>
            ))}
          </div>

          <aside className="wp-center-side">
            <div className="wp-center-preview">
              {sel?.preview ? (
                <img src={toAssetUrl(sel.preview)} alt="" />
              ) : sel?.kind === "video" && sel.path ? (
                <video src={toAssetUrl(sel.path)} autoPlay loop muted playsInline />
              ) : (
                <span className="wp-center-empty">{t("noPreview")}</span>
              )}
            </div>
            {sel && (
              <div className="wp-center-props">
                <h3 title={sel.path}>{sel.title}</h3>
                {(sel.kind === "image" || (sel.kind === "current" && settings?.customBg.imagePath)) && (
                  <>
                    <label className="wp-center-slider">
                      <span>{t("propIntensity")}</span>
                      <input
                        type="range" min={0} max={1.5} step={0.05}
                        value={draft.livingIntensity ?? 0.8}
                        onChange={(e) => setDraft((d) => ({ ...d, livingIntensity: Number(e.target.value) }))}
                      />
                      <code>{(draft.livingIntensity ?? 0.8).toFixed(2)}</code>
                    </label>
                    <label className="wp-center-slider">
                      <span>{t("propDrift")}</span>
                      <input
                        type="range" min={0} max={1} step={0.05}
                        value={draft.livingDrift ?? 0.6}
                        onChange={(e) => setDraft((d) => ({ ...d, livingDrift: Number(e.target.value) }))}
                      />
                      <code>{(draft.livingDrift ?? 0.6).toFixed(2)}</code>
                    </label>
                    <label className="wp-center-select">
                      <span>{t("propStyle")}</span>
                      <select
                        value={draft.particleStyle ?? "mixed"}
                        onChange={(e) => setDraft((d) => ({ ...d, particleStyle: e.target.value as CustomBg["particleStyle"] }))}
                      >
                        <option value="dust">{t("styleDust")}</option>
                        <option value="bokeh">{t("styleBokeh")}</option>
                        <option value="mixed">{t("styleMixed")}</option>
                      </select>
                    </label>
                  </>
                )}
                {sel.kind === "video" && <p className="wp-center-note">{t("videoNote")}</p>}
                <button className="wp-center-apply" disabled={!sel.supported || !sel.path} onClick={() => apply(sel)}>
                  {t("apply")}
                </button>
                <button
                  className="wp-center-playlist-add"
                  disabled={!sel.supported || !sel.path || sel.kind === "current"}
                  onClick={() => {
                    const list = loadPlaylist();
                    const entry: PlaylistEntry = {
                      title: sel.title,
                      kind: sel.kind === "current" ? "image" : sel.kind,
                      path: sel.path,
                    };
                    if (list.some((x) => x.path === entry.path)) {
                      setToast(t("playlistDup"));
                      return;
                    }
                    savePlaylist([...list, entry]);
                    setToast(t("playlistAdded"));
                  }}
                >
                  + {t("playlist")}
                </button>
              </div>
            )}
          </aside>
        </div>
      )}

      <PlaylistBar onToast={setToast} />
      {toast && <div className="wp-center-toast" role="status">{toast}</div>}
    </div>
  );
}

function kindGlyph(kind: LibraryItem["kind"]): string {
  switch (kind) {
    case "video": return "▶";
    case "web": return "⌘";
    case "shader": return "✦";
    default: return "▤";
  }
}

/** 底栏播放列表：成员/间隔/顺序/开关/立即换一张（轮换 runner 在 DesktopShell）。 */
function PlaylistBar(props: { onToast: (msg: string) => void }): React.ReactElement {
  const t = useMemo(() => cT(), []);
  const [entries, setEntries] = useState<PlaylistEntry[]>(() => loadPlaylist());
  const [st, setSt] = useState<PlaylistState>(() => loadPlaylistState());
  const stateRef = useRef(st);
  stateRef.current = st;

  // 中心内变更（本组件 save*）即时刷新；runner 换壁纸后 cursor 变化 → 事件回来同步
  useEffect(() => {
    const onPl = (): void => {
      setEntries(loadPlaylist());
      setSt(loadPlaylistState());
    };
    window.addEventListener("wpcenter://playlist-changed", onPl);
    return () => window.removeEventListener("wpcenter://playlist-changed", onPl);
  }, []);

  const save = (list: PlaylistEntry[]): void => {
    setEntries(list);
    savePlaylist(list);
  };
  const saveState = (s: PlaylistState): void => {
    setSt(s);
    savePlaylistState(s);
  };

  const next = (): void => {
    if (entries.length === 0) return;
    const i = nextIndex(entries.length, stateRef.current.cursor, stateRef.current.shuffle);
    const entry = entries[i];
    if (!entry) return;
    saveState({ ...stateRef.current, cursor: i });
    dispatchWpApply(entryPatch(entry));
    props.onToast(t("applied"));
  };

  return (
    <footer className="wp-center-playlist">
      <label className="wp-center-toggle">
        <input
          type="checkbox"
          checked={st.enabled}
          onChange={(e) => saveState({ ...st, enabled: e.target.checked })}
        />
        <span>{t("playlist")}</span>
      </label>
      <span className="wp-center-pl-count">{entries.length}</span>
      <label className="wp-center-num">
        <span>{t("interval")}</span>
        <input
          type="number" min={1} max={720}
          value={st.intervalMin}
          onChange={(e) => saveState({ ...st, intervalMin: Math.max(1, Math.round(Number(e.target.value) || 1)) })}
        />
      </label>
      <label className="wp-center-toggle">
        <input type="checkbox" checked={st.shuffle} onChange={(e) => saveState({ ...st, shuffle: e.target.checked })} />
        <span>{t("shuffle")}</span>
      </label>
      <button onClick={next} disabled={entries.length === 0}>{t("nextNow")}</button>
      <div className="wp-center-pl-items">
        {entries.map((e, i) => (
          <span key={`${e.path}:${i}`} className={`wp-center-pl-item ${i === st.cursor ? "on" : ""}`} title={e.title}>
            {e.title}
            <button
              className="wp-center-pl-remove"
              aria-label={t("playlistRemove")}
              onClick={() => save(entries.filter((_, j) => j !== i))}
            >✕</button>
          </span>
        ))}
      </div>
      {entries.length > 0 && (
        <button className="wp-center-pl-clear" onClick={() => { save([]); dispatchPlaylistChanged(); }}>
          {t("playlistClear")}
        </button>
      )}
    </footer>
  );
}

// ---- 自挂载（mount.ts 协议，与壁纸工坊同款） ----
if (typeof window !== "undefined") {
  mountOnEvent(window, "wallpaper-center", async () => ({ Overlay: WallpaperCenter }));
  installCloseHandler(window, "wallpaper-center");
}
