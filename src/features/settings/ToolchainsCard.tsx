import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * B-21：工具链便携部署卡（Python / Go / Rust；Node 在 AI Hub）。
 * 部署到容器 runtime/ 下，PATH 由 spawn_profiled 统一前置（受管进程天然继承）。
 * 进度事件 toolchain://progress。
 */
interface TcStatus {
  id: string;
  deployed: boolean;
  home: string;
}
interface TcProgress {
  id: string;
  phase: string;
  done: number;
  total: number;
  message: string;
}

export function ToolchainsCard() {
  const { t } = useI18n();
  const [list, setList] = useState<TcStatus[]>([]);
  const [progress, setProgress] = useState<TcProgress | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const refresh = useCallback(async () => {
    try {
      setList(await ipc.toolchainStatus());
    } catch (e) {
      pushToast("error", t("tcStatusFail"), errMessage(e).message);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
    let alive = true;
    void listen<TcProgress>("toolchain://progress", (ev) => {
      if (!alive) return;
      setProgress(ev.payload);
      if (ev.payload.phase === "done") {
        pushToast("success", t("tcDeployDone"), `${ev.payload.id}: ${ev.payload.message}`);
        void refresh();
      } else if (ev.payload.phase === "error") {
        pushToast("error", t("tcDeployFail"), ev.payload.message);
        void refresh();
      }
    }).then((un) => {
      unlistenRef.current = un;
    });
    return () => {
      alive = false;
      unlistenRef.current?.();
    };
  }, [refresh, t]);

  const deploy = async (id: string) => {
    try {
      await ipc.toolchainDeploy(id);
      pushToast("info", t("tcDeployStart"), id);
    } catch (e) {
      pushToast("error", t("tcDeployFail"), errMessage(e).message);
    }
  };

  return (
    <div className="tc-card">
      <p className="dim small">{t("tcHint")}</p>
      <div className="tc-list">
        {list.map((tc) => (
          <div key={tc.id} className="tc-row">
            <span className="tc-name">
              {tc.id} {tc.deployed ? "✅" : "—"}
            </span>
            {!tc.deployed && (
              <button type="button" onClick={() => deploy(tc.id)}>
                {t("tcDeploy")}
              </button>
            )}
          </div>
        ))}
      </div>
      {progress && progress.phase !== "done" && progress.phase !== "error" && (
        <div className="dim small">
          {progress.id} · {progress.phase} · {progress.message}
        </div>
      )}
      <p className="dim small">{t("tcOfflineNote")}</p>
    </div>
  );
}
