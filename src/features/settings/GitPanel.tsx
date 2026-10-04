import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type GitCommitView, type GitStatusView, type SshKeyView } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * B-22：Git 深度面板（只读）——状态/分支/历史。
 * 职责边界（刻意设计）：面板只读（git2）；暂存/提交/push 一律在终端完成
 * （终端经执行档通道，SSH 密钥由金库 GIT_SSH_COMMAND 注入）。
 */


export function GitPanel(props: { repoDir: string }) {
  const { t } = useI18n();
  const [status, setStatus] = useState<GitStatusView | null>(null);
  const [log, setLog] = useState<GitCommitView[]>([]);
  const [branches, setBranches] = useState<string[]>([]);
  const [keys, setKeys] = useState<SshKeyView[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!props.repoDir) return;
    try {
      const [s, l, b, k] = await Promise.all([
        ipc.gitStatus(props.repoDir),
        ipc.gitLog(props.repoDir, 30),
        ipc.gitBranches(props.repoDir),
        ipc.sshKeys(),
      ]);
      setStatus(s);
      setLog(l);
      setBranches(b);
      setKeys(k);
      setError(null);
    } catch (e) {
      setError(errMessage(e).message);
      setStatus(null);
    }
  }, [props.repoDir]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(id);
  }, [refresh]);

  const genKey = async () => {
    try {
      await ipc.sshKeyGenerate("git");
      await refresh();
      pushToast("success", t("gitKeyDone"), "");
    } catch (e) {
      pushToast("error", t("gitKeyFail"), errMessage(e).message);
    }
  };

  return (
    <div className="git-panel">
      <div className="git-head">
        <h4>{t("gitTitle")}</h4>
        {error ? (
          <span className="dim small git-err">{error}</span>
        ) : status ? (
          <span className="dim small">
            {status.headBranch || "—"}
            {status.headCommit ? ` @ ${status.headCommit}` : ""}
            {status.ahead > 0 || status.behind > 0
              ? ` · ↑${status.ahead} ↓${status.behind}`
              : !status.hasUpstream
                ? ` · ${t("gitNoUpstream")}`
                : ""}
          </span>
        ) : null}
      </div>

      {status && (
        <div className="git-section">
          <h5>{t("gitChanges")}</h5>
          {status.entries.length === 0 && <p className="dim small">{t("gitClean")}</p>}
          <ul className="git-entries">
            {status.entries.slice(0, 50).map((e) => (
              <li key={`${e.state}:${e.path}`}>
                <span className={`git-state git-${e.state}`}>{e.state.replace(/^(wt|index)_/, "")}</span>
                <span className="git-path">{e.path}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {branches.length > 0 && (
        <div className="git-section">
          <h5>{t("gitBranches")}</h5>
          <div className="dim small">{branches.join(" · ")}</div>
        </div>
      )}

      {log.length > 0 && (
        <div className="git-section">
          <h5>{t("gitHistory")}</h5>
          <ul className="git-log">
            {log.map((c) => (
              <li key={c.id}>
                <code>{c.id}</code> {c.summary}
                <span className="dim small"> — {c.author}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="git-section">
        <h5>{t("gitSsh")}</h5>
        {keys.length === 0 && <p className="dim small">{t("gitSshEmpty")}</p>}
        {keys.map((k) => (
          <div key={k.id} className="dim small ellipsis">
            🔑 {k.label || k.id}
          </div>
        ))}
        <button type="button" className="git-key-btn" onClick={genKey}>
          {t("gitKeyGen")}
        </button>
        <p className="dim small">{t("gitWriteNote")}</p>
      </div>
    </div>
  );
}
