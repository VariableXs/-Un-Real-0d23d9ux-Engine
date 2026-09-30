﻿import { useEffect } from "react";
import {
  ListFilter, Search, FilePlus2, Languages, Settings2, House,
} from "lucide-react";
import { useI18n } from "../i18n";
import { uiStore, setSaveStatus, useUi, type AppMode, type SaveStatus } from "../state/uiStore";
import { focusDesktop } from "../system/windows/appWindows";
import { beginDragSnap } from "../system/windows/snap";
import { WindowControls } from "./WindowControls";

export type ClosePhase = "idle" | "flushing" | "failed";

export function TitleBar(props: {
  onCloseRequested: () => void;
  closePhase: ClosePhase;
  /** M4 拆窗：本窗口承载的软件（缺省 desktop = 桌面环境窗口，过渡期兼容）。 */
  appType?: "desktop" | AppMode;
  /** 批次D（规格 4.3.5）：窗口控制按钮风格（缺省 mac）。 */
  controlStyle?: "mac" | "windows";
}): React.ReactElement {
  const { t, lang, setLang } = useI18n();
  const appType = props.appType ?? "desktop";
  const standalone = appType !== "desktop";
  const focusMode = useUi((s) => s.focusMode);
  const currentDocId = useUi((s) => s.currentDocId);
  const sidebarOpen = useUi((s) => s.sidebarOpen);
  const closePhase = props.closePhase;
  const status: SaveStatus | undefined =
    useUi((s) => (currentDocId ? s.saveStatuses[currentDocId] : undefined)) ?? "saved";

  useEffect(() => {
    if (!currentDocId) setSaveStatus(currentDocId ?? "", "saved");
  }, [currentDocId]);

  if (focusMode && closePhase === "idle") {
    // Immersive mode: the title bar itself disappears, but a fixed host keeps
    // the traffic-light controls alive so the top-corner reveal zones still
    // work (minimize / maximize / close are never out of reach).
    return (
      <>
        <div className="titlebar hidden" />
        <div className="focus-controls">
          <WindowControls onCloseRequested={props.onCloseRequested} style={props.controlStyle} />
        </div>
      </>
    );
  }

  const statusDot = (
    <span
      className={`save-dot ${status} ${closePhase !== "idle" ? "busy" : ""}`}
      role="status"
      aria-label={
        closePhase === "flushing"
          ? t("closeUnsaved")
          : closePhase === "failed"
            ? t("closeSaveFailed")
            : status === "saved"
              ? t("savedLocally")
              : status === "saving"
                ? t("saving")
                : status === "error"
                  ? t("saveFailed")
                  : t("unsaved")
      }
      data-tip={
        closePhase === "flushing"
          ? t("closeUnsaved")
          : closePhase === "failed"
            ? t("closeSaveFailed")
            : status === "saved"
              ? t("savedLocally")
              : status === "saving"
                ? t("saving")
                : status === "error"
                  ? t("saveFailed")
                  : t("unsaved")
      }
    />
  );

  const chipLabel =
    appType === "write"
      ? (lang !== "en" ? "写作" : "Write")
      : appType === "mindmap"
        ? (lang !== "en" ? "导图" : "Mind")
        : appType === "project"
          ? (lang !== "en" ? "项目" : "Code")
          : (lang !== "en" ? "命运" : "Fate");

  return (
    <header
      className="titlebar"
      data-tauri-drag-region
      onPointerDown={(e) => {
        // 批次D（规格 4.5）：标题栏拖动 → 贴靠跟踪（左键、非交互控件区）
        if (e.button !== 0) return;
        const t = e.target as HTMLElement | null;
        if (t?.closest("button, input, select, a")) return;
        beginDragSnap();
      }}
    >
      <div className="tb-left" data-tauri-drag-region>
        <button
          type="button"
          className="icon-btn"
          data-tip={t("backDesktop")}
          aria-label={t("backDesktop")}
          onClick={() => (standalone ? focusDesktop() : uiStore.setState({ mode: "desktop" }))}
        >
          <House size={16} />
        </button>
        <span className="logo" aria-hidden>V</span>
        <span className="app-name">Variable</span>
        <span className="mode-chip">{chipLabel}</span>
        {statusDot}
        {closePhase === "failed" && <span className="close-error">{t("closeSaveFailed")}</span>}
      </div>

      <div className="tb-right">
        {/* M4 拆窗：跨软件切换按钮已移除 —— 每个窗口就是一个独立软件。 */}
        {appType === "write" && (
          <button
            type="button"
            className="icon-btn"
            data-tip={t("newRecord")}
            aria-label={t("newRecord")}
            onClick={() => window.dispatchEvent(new CustomEvent("variable:new-doc"))}
          >
            <FilePlus2 size={17} />
          </button>
        )}
        <button
          type="button"
          className={`icon-btn ${sidebarOpen ? "active" : ""}`}
          data-tip={t("recordList")}
          aria-label={t("recordList")}
          onClick={() => uiStore.setState((s) => ({ sidebarOpen: !s.sidebarOpen }))}
        >
          <ListFilter size={17} />
        </button>
        <button
          type="button"
          className="icon-btn"
          data-tip={t("globalSearch")}
          aria-label={t("search")}
          onClick={() => uiStore.setState({ searchOpen: true })}
        >
          <Search size={17} />
        </button>
        <button
          type="button"
          className="icon-btn"
          data-tip={t("language")}
          aria-label={t("language")}
          onClick={() => setLang(lang !== "en" ? "en" : "zh")}
        >
          <Languages size={17} />
        </button>
        <button
          type="button"
          className="icon-btn"
          data-tip={t("settings")}
          aria-label={t("settings")}
          onClick={() => uiStore.setState({ settingsOpen: true })}
        >
          <Settings2 size={17} />
        </button>
        <span className="tb-divider" aria-hidden />
        <WindowControls onCloseRequested={props.onCloseRequested} />
      </div>
    </header>
  );
}
