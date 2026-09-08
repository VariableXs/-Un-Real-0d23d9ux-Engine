import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Copy } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { formatBytes } from "../../lib/format";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import type { Shell } from "../../lib/ipc";

/**
 * M-21 校验和工具（VWM 虚拟窗口应用）：
 * - md5 / sha1 / sha256 / blake3 四种算法，1MB 分块流式计算
 * - 进度条实时显示（checksum://progress），大文件可取消
 * - 只读操作，不触碰文件内容
 */
const ALGOS = ["md5", "sha1", "sha256", "blake3"] as const;

export function ChecksumApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [path, setPath] = useState("");
  const [algo, setAlgo] = useState<(typeof ALGOS)[number]>("sha256");
  const [busy, setBusy] = useState(false);
  const [prog, setProg] = useState<{ done: number; total: number } | null>(null);
  const [result, setResult] = useState<Shell.ChecksumResult | null>(null);
  const [copied, setCopied] = useState(false);
  const opId = useRef(`cs-${Date.now().toString(36)}`);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<Shell.ChecksumProgress>("checksum://progress", (ev) => {
      if (ev.payload.opId === opId.current) setProg({ done: ev.payload.done, total: ev.payload.total });
    });
    void p
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  const run = (): void => {
    const p = path.trim();
    if (!p || busy) return;
    opId.current = `cs-${Date.now().toString(36)}`;
    setBusy(true);
    setProg(null);
    setResult(null);
    setCopied(false);
    ipc
      .checksum(p, algo, opId.current)
      .then((r) => {
        setResult(r);
        if (r.cancelled) pushToast("info", t("toolChecksum"), t("foCancelled"));
      })
      .catch((e: unknown) => pushToast("error", t("toolChecksum"), errMessage(e).message))
      .finally(() => {
        setBusy(false);
        setProg(null);
      });
  };

  const cancel = (): void => {
    void ipc.checksumCancel(opId.current).catch(() => {});
  };

  const copy = (): void => {
    if (!result) return;
    void navigator.clipboard.writeText(result.hex).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  };

  const pct = prog && prog.total > 0 ? Math.min(100, Math.round((prog.done / prog.total) * 100)) : null;

  return (
    <div className="fo-app">
      <div className="fo-bar">
        <input
          className="text-input fo-path"
          type="text"
          value={path}
          placeholder={t("foPathPlaceholder")}
          onChange={(e) => setPath(e.target.value)}
          aria-label={t("foPath")}
        />
        <select
          className="ex-sort-select"
          value={algo}
          onChange={(e) => setAlgo(e.target.value as (typeof ALGOS)[number])}
          aria-label={t("foAlgo")}
        >
          {ALGOS.map((a) => (
            <option key={a} value={a}>
              {a.toUpperCase()}
            </option>
          ))}
        </select>
        {busy ? (
          <button type="button" className="btn danger" onClick={cancel}>
            {t("foCancel")}
          </button>
        ) : (
          <button type="button" className="btn primary" onClick={run} disabled={!path.trim()}>
            {t("foStart")}
          </button>
        )}
      </div>

      {busy && prog && (
        <div className="fo-progress" role="status">
          <div className="fo-progress-bar">
            <div className="fo-progress-fill" style={{ width: `${pct ?? 0}%` }} />
          </div>
          <span className="dim small">
            {pct !== null ? `${pct}% · ` : ""}
            {formatBytes(prog.done)} / {formatBytes(prog.total)}
          </span>
        </div>
      )}

      {result && (
        <div className="fo-checksum-result">
          <div className="fo-checksum-meta dim small">
            {result.algo.toUpperCase()} · {formatBytes(result.bytes)}
            {result.cancelled ? ` · ${t("foCancelled")}` : ""}
          </div>
          <div className="fo-checksum-hex">{result.hex}</div>
          <button type="button" className="btn ghost tiny" onClick={copy}>
            <Copy size={12} /> {copied ? t("foCopied") : t("foCopy")}
          </button>
        </div>
      )}

      {!result && !busy && <p className="dim small fo-hint">{t("foChecksumHint")}</p>}
    </div>
  );
}
