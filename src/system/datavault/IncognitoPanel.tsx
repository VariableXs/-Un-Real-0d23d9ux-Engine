import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { IncStatus } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize, fmtTime } from "./shared";

/**
 * U-36 隐身会话：临时文件写入 → 结束即逐文件覆写焚毁（零残留）。
 * 隐身期间版本快照 / 血缘 / 洞察全部后端侧静默停用（红线在 Rust 侧，UI 只呈现）。
 */
export function IncognitoPanel(props: { onChange?: (s: IncStatus | null) => void }): React.ReactElement {
  const { t } = useI18n();
  const [status, setStatus] = useState<IncStatus | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [contents, setContents] = useState("");

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.incStatus().then((s) => {
      setStatus(s);
      props.onChange?.(s);
      if (!s.active) setFiles([]);
      else void ipc.incList().then(setFiles).catch(() => {});
    }).catch(() => {
      setStatus(null);
      props.onChange?.(null);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const start = async (): Promise<void> => {
    try {
      await ipc.incStart();
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const end = async (): Promise<void> => {
    try {
      const n = await ipc.incEnd();
      pushToast("success", t("incEndLabel") + `: ${n}`);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const write = async (): Promise<void> => {
    if (!name || !contents) return;
    try {
      await ipc.incWrite(name, contents);
      setName("");
      setContents("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const s = status?.session;

  return (
    <section>
      <h2>{t("dvTabIncognito")}</h2>
      <p className="dv-hint">{t("incHint")}</p>
      {status?.active && s ? (
        <div className="dv-banner incognito">
          {t("incActiveLabel")} · {fmtTime(s.startedAt)} · {s.fileCount} files · {fmtSize(s.fileBytes)}
        </div>
      ) : (
        <div className="dv-row">
          <button className="dv-btn primary" onClick={() => void start()}>{t("incStartLabel")}</button>
        </div>
      )}

      {status?.active && (
        <>
          <h3>{t("incWriteLabel")}</h3>
          <div className="dv-row">
            <input className="dv-input" placeholder={t("incFileName")}
              value={name} onChange={(e) => setName(e.target.value)} />
            <button className="dv-btn" onClick={() => void write()}>{t("incWriteLabel")}</button>
          </div>
          <textarea
            className="dv-input wide"
            rows={4}
            style={{ width: "100%", resize: "vertical" }}
            placeholder={t("incContents")}
            value={contents}
            onChange={(e) => setContents(e.target.value)}
          />
          <h3>{t("incSessionFiles", { n: files.length })}</h3>
          <div className="dv-list">
            {files.map((f) => (
              <div key={f} className="dv-item"><span className="grow">{f}</span></div>
            ))}
          </div>
          <div className="dv-row" style={{ marginTop: 12 }}>
            <button className="dv-btn danger" onClick={() => void end()}>{t("incEndLabel")}</button>
          </div>
        </>
      )}

      {status && status.sessionsTotal > 0 && (
        <p className="dv-hint" style={{ marginTop: 10 }}>
          {t("incHistoryLine", { n: status.sessionsTotal })}
          {status.lastBurnedAt > 0 ? ` · ${fmtTime(status.lastBurnedAt)}` : ""}
        </p>
      )}
    </section>
  );
}

