import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useI18n } from "../i18n";

/**
 * Contract (rev 4): the macOS-style traffic-light controls are ALWAYS visible
 * in the title bar — no reveal zones, no hover-to-show, no auto-hide. Each
 * control is a plain solid-color dot (a dedicated <span>, immune to
 * zoom/DPI/background-clip quirks) with NO glyph inside (-, □, ×): identity
 * comes from color + tooltip/aria-label only. Hover feedback is a pure
 * brightness/scale change on the dot.
 *
 * 批次D（规格 4.3.5）：新增 Windows 风格（右上角标准控件）——
 * 由设置 winControls 切换；mac 风格保持默认不变。
 */
export function WindowControls(props: {
  onCloseRequested: () => void;
  /** 批次D：控件风格（缺省 mac = 原红绿灯不变）。 */
  style?: "mac" | "windows";
}): React.ReactElement {
  const { t } = useI18n();
  const [maximized, setMaximized] = useState(false);
  const win = getCurrentWindow();

  useEffect(() => {
    let un: (() => void) | undefined;
    void win.isMaximized().then(setMaximized).catch(() => {});
    void win.onResized(async () => {
      try {
        setMaximized(await win.isMaximized());
      } catch {
        /* window gone */
      }
    }).then((u) => {
      un = u;
    });
    return () => un?.();
  }, [win]);

  if (props.style === "windows") {
    return (
      <div className="window-controls shown win-style">
        <button
          type="button"
          className="winctl-btn"
          aria-label={t("minimize")}
          title={t("minimize")}
          onClick={() => void win.minimize().catch(() => {})}
        >
          <Minus size={14} />
        </button>
        <button
          type="button"
          className="winctl-btn"
          aria-label={maximized ? t("restore") : t("maximize")}
          title={maximized ? t("restore") : t("maximize")}
          onClick={() => void win.toggleMaximize().catch(() => {})}
        >
          <Square size={11} />
        </button>
        <button
          type="button"
          className="winctl-btn winctl-close"
          aria-label={t("close")}
          title={t("close")}
          onClick={props.onCloseRequested}
        >
          <X size={14} />
        </button>
      </div>
    );
  }

  return (
    <div className="window-controls shown">
      <button
        type="button"
        className="win-btn"
        aria-label={t("minimize")}
        title={t("minimize")}
        onClick={() => void win.minimize().catch(() => {})}
      >
        <span className="win-dot yellow" />
      </button>
      <button
        type="button"
        className="win-btn"
        aria-label={maximized ? t("restore") : t("maximize")}
        title={maximized ? t("restore") : t("maximize")}
        onClick={() => void win.toggleMaximize().catch(() => {})}
      >
        <span className="win-dot green" />
      </button>
      <button
        type="button"
        className="win-btn"
        aria-label={t("close")}
        title={t("close")}
        onClick={props.onCloseRequested}
      >
        <span className="win-dot red" />
      </button>
    </div>
  );
}
