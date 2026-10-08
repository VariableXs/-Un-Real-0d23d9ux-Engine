import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ShieldCheck } from "lucide-react";
import { WindowControls } from "../../components/WindowControls";
import { ToastHost } from "../../components/ToastHost";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import type { IncStatus } from "../../lib/ipc";
import { isTauriRuntime } from "../../entries/runtime";
import { VersionsPanel } from "./VersionsPanel";
import { TransferPanel } from "./TransferPanel";
import { ArchivePanel } from "./ArchivePanel";
import { TagsPanel } from "./TagsPanel";
import { LineagePanel } from "./LineagePanel";
import { PrivacyPanel } from "./PrivacyPanel";
import { TrustPanel } from "./TrustPanel";
import { IncognitoPanel } from "./IncognitoPanel";
import { InsightsPanel } from "./InsightsPanel";
import { RecyclePolicyPanel } from "./RecyclePolicyPanel";
import { FirewallPanel } from "./FirewallPanel";
import { PanicPanel } from "./PanicPanel";

/**
 * AI-10 数据安全中心（datavault.html）：
 * 版本时光机 / 传输指挥台 / 存档柜 / 文件标签 / 数据血缘 /
 * 隐私仪表盘 / 应用防火墙 / 信任链 / 紧急擦拭 / 隐身会话 / 使用洞察 / 回收站策略。
 * 浏览器 dev 模式（无 IPC）如实显示"后端不可用"，不伪造数据。
 */
export type DvTab =
  | "versions" | "transfer" | "archive" | "tags" | "lineage"
  | "privacy" | "firewall" | "trust" | "panic" | "incognito" | "insights" | "recycle";

const TABS: { id: DvTab; key: string }[] = [
  { id: "versions", key: "dvTabVersions" },
  { id: "transfer", key: "dvTabTransfer" },
  { id: "archive", key: "dvTabArchive" },
  { id: "tags", key: "dvTabTags" },
  { id: "lineage", key: "dvTabLineage" },
  { id: "privacy", key: "dvTabPrivacy" },
  { id: "firewall", key: "dvTabFirewall" },
  { id: "trust", key: "dvTabTrust" },
  { id: "panic", key: "dvTabPanic" },
  { id: "incognito", key: "dvTabIncognito" },
  { id: "insights", key: "dvTabInsights" },
  { id: "recycle", key: "dvTabRecycle" },
];

export function DataVaultWindow(): React.ReactElement {
  const { t } = useI18n();
  const [tab, setTab] = useState<DvTab>(() => {
    const q = new URLSearchParams(window.location.search).get("tab");
    return (TABS.some((x) => x.id === q) ? q : "versions") as DvTab;
  });
  const [inc, setInc] = useState<IncStatus | null>(null);

  // 隐身状态带轮询（所有面板共享的红色边界提示）
  useEffect(() => {
    let alive = true;
    const tick = (): void => {
      if (!isTauriRuntime()) return;
      void ipc.incStatus().then((s) => {
        if (alive) setInc(s);
      }).catch(() => {
        if (alive) setInc(null);
      });
    };
    tick();
    const timer = window.setInterval(tick, 3000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  const close = useCallback((): void => {
    void getCurrentWindow().close().catch(() => {});
  }, []);

  const panel: React.ReactNode =
    tab === "versions" ? <VersionsPanel /> :
    tab === "transfer" ? <TransferPanel /> :
    tab === "archive" ? <ArchivePanel /> :
    tab === "tags" ? <TagsPanel /> :
    tab === "lineage" ? <LineagePanel /> :
    tab === "privacy" ? <PrivacyPanel /> :
    tab === "firewall" ? <FirewallPanel /> :
    tab === "trust" ? <TrustPanel /> :
    tab === "panic" ? <PanicPanel /> :
    tab === "incognito" ? <IncognitoPanel onChange={(s) => setInc(s)} /> :
    tab === "insights" ? <InsightsPanel /> :
    <RecyclePolicyPanel />;

  return (
    <div className="dv-window">
      <div className="dv-titlebar">
        <ShieldCheck size={16} opacity={0.8} />
        <span className="dv-title">{t("dvTitle")}</span>
        <div style={{ flex: 1 }} />
        <WindowControls onCloseRequested={close} />
      </div>
      {inc?.active && (
        <div className="dv-banner incognito" data-tauri-drag-dragging="false">
          {t("incActiveLabel")} — {t("incHint")}
        </div>
      )}
      <div className="dv-body">
        <nav className="dv-nav">
          {TABS.map((x) => (
            <button
              key={x.id}
              className={`dv-nav-btn${tab === x.id ? " on" : ""}`}
              onClick={() => setTab(x.id)}
            >
              {t(x.key)}
            </button>
          ))}
        </nav>
        <main className="dv-main">
          {!isTauriRuntime() && (
            <p className="dv-hint" style={{ marginBottom: 10 }}>
              [dev] no Tauri backend — data unavailable in browser mode
            </p>
          )}
          {panel}
        </main>
      </div>
      <ToastHost />
    </div>
  );
}

/** 面板通用错误提示（如实呈现后端错误，不吞）。 */
export function dvError(e: unknown): string {
  return errMessage(e).message;
}
