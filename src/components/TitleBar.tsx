import { useEffect } from "react";
import {
  Map as MapIcon, PenLine, ListFilter, Search, FilePlus2, Languages, Settings2,
  FolderSearch, Sparkles,
} from "lucide-react";
import { useI18n } from "../i18n";
import { uiStore, setSaveStatus, useUi, type SaveStatus } from "../state/uiStore";
import { WindowControls } from "./WindowControls";

export type ClosePhase = "idle" | "flushing" | "failed";

export function TitleBar(props: {
  onCloseRequested: () => void;
  closePhase: ClosePhase;
}): React.ReactElement {
  const { t, lang, setLang } = useI18n();
  const mode = useUi((s) => s.mode);
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
          <WindowControls onCloseRequested={props.onCloseRequested} />
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

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-left" data-tauri-drag-region>
        <span className="logo" aria-hidden>V</span>
        <span className="app-name">Variable</span>
        <span className="mode-chip">
          {mode === "write"
            ? (lang === "zh" ? "写作" : "Write")
            : mode === "project"
              ? (lang === "zh" ? "项目" : "Project")
              : mode === "fate"
                ? (lang === "zh" ? "命运" : "Fate")
                : (lang === "zh" ? "导图" : "Map")}
        </span>
        {statusDot}
        {closePhase === "failed" && <span className="close-error">{t("closeSaveFailed")}</span>}
      </div>

      <div className="tb-right">
        <button
          type="button"
          className="icon-btn"
          data-tip={mode === "write" ? t("mindmapMode") : t("writingMode")}
          aria-label={mode === "write" ? t("mindmapMode") : t("writingMode")}
          onClick={() => uiStore.setState({ mode: mode === "write" ? "mindmap" : "write" })}
        >
          {mode === "write" ? <MapIcon size={17} /> : <PenLine size={17} />}
        </button>
        {/* 0.2 独立入口：项目分析空间（文件夹+放大镜，冰蓝色调） */}
        <button
          type="button"
          className={`icon-btn pv-entry ${mode === "project" ? "active" : ""}`}
          data-tip={lang === "zh" ? "项目分析 (Project)" : "Project analysis"}
          aria-label={lang === "zh" ? "项目分析" : "Project analysis"}
          onClick={() => uiStore.setState({ mode: mode === "project" ? "write" : "project" })}
        >
          <FolderSearch size={17} />
        </button>
        {/* 序章：命运推演空间入口（分叉星芒 · 冰蓝） */}
        <button
          type="button"
          className={`icon-btn fate-entry ${mode === "fate" ? "active" : ""}`}
          data-tip={lang === "zh" ? "命运推演 (Fate)" : "Fate tree"}
          aria-label={lang === "zh" ? "命运推演" : "Fate tree"}
          onClick={() => uiStore.setState({ mode: mode === "fate" ? "write" : "fate" })}
        >
          <Sparkles size={17} />
        </button>
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
          data-tip={t("newRecord")}
          aria-label={t("newRecord")}
          onClick={() => window.dispatchEvent(new CustomEvent("variable:new-doc"))}
        >
          <FilePlus2 size={17} />
        </button>
        <button
          type="button"
          className="icon-btn"
          data-tip={t("language")}
          aria-label={t("language")}
          onClick={() => setLang(lang === "zh" ? "en" : "zh")}
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
