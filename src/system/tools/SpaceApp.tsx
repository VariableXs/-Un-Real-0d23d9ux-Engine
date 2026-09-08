import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { formatBytes } from "../../lib/format";
import { pushToast } from "../../state/uiStore";
import type { Shell } from "../../lib/ipc";

/**
 * Z-34 磁盘空间分析（VWM 虚拟窗口应用）：
 * - 目录树递归统计（大小 / 文件数 / 目录数），子项按占用降序
 * - 只读分析；上限 20 万项截断如实显示
 */
function SpaceNodeRow(props: { node: Shell.SpaceNode; depth: number }): React.ReactElement {
  const { node, depth } = props;
  const [open, setOpen] = useState(props.depth < 1);
  const children = [...node.children].sort((a, b) => b.size - a.size);
  return (
    <>
      <button
        type="button"
        className="fo-tree-row"
        style={{ paddingLeft: `${6 + depth * 14}px` }}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        title={node.path}
      >
        {children.length > 0 ? (
          open ? <ChevronDown size={12} /> : <ChevronRight size={12} />
        ) : (
          <span className="fo-tree-leaf" />
        )}
        <span className="fo-tree-name">{node.name}</span>
        <span className="fo-tree-size">{formatBytes(node.size)}</span>
        <span className="dim small fo-tree-counts">
          {props.node ? `${node.fileCount}F / ${node.dirCount}D` : ""}
        </span>
      </button>
      {open &&
        children.map((c) => <SpaceNodeRow key={c.path} node={c} depth={depth + 1} />)}
    </>
  );
}

export function SpaceApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<Shell.SpaceReport | null>(null);

  const scan = (): void => {
    const p = path.trim();
    if (!p || busy) return;
    setBusy(true);
    setReport(null);
    ipc
      .spaceScan(p)
      .then(setReport)
      .catch((e: unknown) => pushToast("error", t("toolSpace"), errMessage(e).message))
      .finally(() => setBusy(false));
  };

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
        <button type="button" className="btn primary" onClick={scan} disabled={busy || !path.trim()}>
          {busy ? t("foScanning") : t("foScan")}
        </button>
      </div>

      {report && (
        <>
          <div className="fo-summary">
            <span>{t("foTotal", { size: formatBytes(report.root.size) })}</span>
            <span className="dim">
              {t("foFiles")} {report.root.fileCount} · {t("foDirs")} {report.root.dirCount} ·{" "}
              {t("foScanned", { n: report.scanned })}
            </span>
            {report.truncated && <span className="fo-truncated">{t("foTruncated")}</span>}
          </div>
          <div className="fo-tree">
            <SpaceNodeRow node={report.root} depth={0} />
          </div>
        </>
      )}

      {!report && !busy && <p className="dim small fo-hint">{t("foSpaceHint")}</p>}
    </div>
  );
}
