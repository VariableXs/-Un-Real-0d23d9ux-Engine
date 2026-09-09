import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { askConfirm, askPrompt } from "../../components/Modal";
import { pushToast } from "../../state/uiStore";
import { useI18n } from "../../i18n";
import type { Settings } from "../../lib/settings";
import { loadDesktopLayout } from "../desktop-icons/layout";
import { loadIconPx } from "../desktop-icons/density";
import { desktopLabel } from "../desktop-icons/labels";
import {
  APPLY_EVENT,
  OPEN_EVENT,
  deleteProfile,
  listProfiles,
  saveProfile,
  snapshotLayout,
  type DesktopProfile,
} from "./profiles";

/**
 * U-13 桌面配置管理面板（简易 overlay：列表卡片 + 删除确认 + 快捷键登记位）。
 * 入口：桌面右键「桌面配置」→ 保存当前 / 管理（经 ai04:open-profiles 事件，
 * 由 DesktopShell 渲染本面板）。应用走 ai04:apply-profile 事件交 DesktopShell
 * 执行（壁纸 patch + 布局回写 + 图标 px 记忆）。
 * 诚实边界：快捷键仅登记在配置内，系统热键通道未接入 —— 面板内明示提示。
 */
export function DesktopProfiles(props: {
  mode: "save" | "manage";
  settings: Settings;
  onClose: () => void;
}): React.ReactElement {
  const { t, lang } = useI18n();
  const [profiles, setProfiles] = useState<DesktopProfile[]>(() => listProfiles());
  const savedOnce = useRef(false);

  const saveCurrent = (): void => {
    void (async () => {
      const initial = `${desktopLabel(lang, "profileMenu")} ${new Date().toLocaleDateString()}`;
      const name = await askPrompt({ title: desktopLabel(lang, "profileNamePrompt"), initial });
      if (!name || !name.trim()) return;
      saveProfile({
        name: name.trim(),
        wallpaper: { mode: props.settings.wallpaperMode, customBg: props.settings.customBg },
        iconPx: loadIconPx(),
        iconLayout: snapshotLayout(loadDesktopLayout()),
      });
      setProfiles(listProfiles());
      pushToast("success", desktopLabel(lang, "profileMenu"), desktopLabel(lang, "profileSaved"));
    })();
  };

  // mode="save"：面板打开即引导命名保存（右键「保存当前配置…」直达）；每挂载只引导一次
  useEffect(() => {
    if (props.mode !== "save" || savedOnce.current) return;
    savedOnce.current = true;
    saveCurrent();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.mode]);

  const apply = (p: DesktopProfile): void => {
    window.dispatchEvent(new CustomEvent(APPLY_EVENT, { detail: p }));
  };

  return (
    <div className="props-overlay" onPointerDown={props.onClose}>
      <div
        className="props-card profiles-card"
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => e.stopPropagation()}
      >
        <header className="props-head">
          <span className="props-title">{desktopLabel(lang, "profileMenu")}</span>
          <button type="button" className="shf-x" onClick={props.onClose} aria-label={t("close")}>
            <X size={14} />
          </button>
        </header>
        <div className="profiles-body">
          {profiles.length === 0 && <p className="dim small">{desktopLabel(lang, "profileEmpty")}</p>}
          {profiles.map((p) => (
            <div key={p.id} className="profiles-item">
              <div className="profiles-meta">
                <b>{p.name}</b>
                <span className="dim small">{new Date(p.ts).toLocaleString()}</span>
              </div>
              <div className="profiles-ops">
                <button type="button" className="btn ghost" onClick={() => apply(p)}>
                  {desktopLabel(lang, "profileApply")}
                </button>
                <button
                  type="button"
                  className="btn ghost"
                  onClick={() => {
                    void askConfirm({
                      title: desktopLabel(lang, "profileDeleteTitle"),
                      body: p.name,
                      danger: true,
                      okLabel: desktopLabel(lang, "profileDelete"),
                    }).then((ok) => {
                      if (!ok) return;
                      deleteProfile(p.id);
                      setProfiles(listProfiles());
                    });
                  }}
                >
                  {desktopLabel(lang, "profileDelete")}
                </button>
              </div>
            </div>
          ))}
        </div>
        <footer className="profiles-foot">
          <button
            type="button"
            className="btn ghost"
            onClick={() => {
              // 关闭后以 save 模式重开 → 触发保存引导（挂载 effect）
              props.onClose();
              window.setTimeout(() => {
                window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: { mode: "save" } }));
              }, 0);
            }}
          >
            {desktopLabel(lang, "profileSave")}
          </button>
        </footer>
        <p className="dim small profiles-hint">{desktopLabel(lang, "profileHotkeyHint")}</p>
      </div>
    </div>
  );
}