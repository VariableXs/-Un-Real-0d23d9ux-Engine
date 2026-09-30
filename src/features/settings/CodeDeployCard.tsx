import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * B-20：VS Code Portable 部署卡（设置页「编码」标签）。
 * 部署 → 登记（出现在启动器/任务栏）→ 启动即走 embed 嵌入通道。
 * 进度事件 code://progress（真实字节进度，与 B-8 Node 部署同款）。
 */
interface CodeProgress {
  phase: string;
  done: number;
  total: number;
  message: string;
}
interface CodeStatus {
  deployed: boolean;
  exe: string;
  registered: boolean;
  portableData: boolean;
}

export function CodeDeployCard() {
  const { t } = useI18n();
  const [status, setStatus] = useState<CodeStatus | null>(null);
  const [progress, setProgress] = useState<CodeProgress | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await ipc.codeStatus());
    } catch (e) {
      pushToast("error", t("cdStatusFail"), errMessage(e).message);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
    let alive = true;
    void listen<CodeProgress>("code://progress", (ev) => {
      if (!alive) return;
      setProgress(ev.payload);
      if (ev.payload.phase === "done" || ev.payload.phase === "error") {
        void refresh();
        if (ev.payload.phase === "done") {
          pushToast("success", t("cdDeployDone"), ev.payload.message);
        } else {
          pushToast("error", t("cdDeployFail"), ev.payload.message);
        }
      }
    }).then((un) => {
      unlistenRef.current = un;
    });
    return () => {
      alive = false;
      unlistenRef.current?.();
    };
  }, [refresh, t]);

  const deploy = async () => {
    try {
      await ipc.codeDeploy();
      pushToast("info", t("cdDeployStart"), t("cdDeployHint"));
    } catch (e) {
      pushToast("error", t("cdDeployFail"), errMessage(e).message);
    }
  };

  const launch = async () => {
    try {
      const r = await ipc.codeLaunch();
      pushToast(
        r.attached ? "success" : "info",
        r.attached ? t("cdLaunchEmbedded") : t("cdLaunchDetached"),
        r.reason,
      );
      void refresh();
    } catch (e) {
      pushToast("error", t("cdLaunchFail"), errMessage(e).message);
    }
  };

  const register = async () => {
    try {
      await ipc.codeRegister();
      await refresh();
      pushToast("success", t("cdRegisterDone"), "");
    } catch (e) {
      pushToast("error", t("cdRegisterFail"), errMessage(e).message);
    }
  };

  const pct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : null;

  return (
    <div className="cd-card">
      <p className="dim small">{t("cdHint")}</p>
      <div className="cd-status">
        <div>
          {t("cdDeployed")}: {status?.deployed ? "✅" : "❌"}
          {status?.portableData ? ` · ${t("cdPortable")}` : ""}
        </div>
        {status?.deployed && <div className="dim small ellipsis">{status.exe}</div>}
        <div>
          {t("cdRegistered")}:{" "}
          {status?.registered ? "✅" : status?.deployed ? "—（可登记）" : "—"}
        </div>
      </div>

      {pct !== null && progress?.phase !== "done" && (
        <div className="cd-progress">
          <div className="cd-progress-bar" style={{ width: `${pct}%` }} />
          <span className="dim small">
            {progress?.phase} · {pct}%
          </span>
        </div>
      )}

      <div className="cd-actions">
        <button type="button" onClick={deploy}>
          {status?.deployed ? t("cdRedeploy") : t("cdDeploy")}
        </button>
        <button type="button" disabled={!status?.deployed} onClick={register}>
          {t("cdRegister")}
        </button>
        <button type="button" disabled={!status?.deployed} onClick={launch}>
          {t("cdLaunch")}
        </button>
      </div>
      <p className="dim small">{t("cdOfflineNote")}</p>
    </div>
  );
}
