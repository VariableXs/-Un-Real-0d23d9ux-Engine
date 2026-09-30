import { useCallback, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * B-23：全库并行搜索 + 大文件分块查看器 + 行级跳转。
 * - 搜索：容器内工作区并行扫描（跳 node_modules/.git/target、二进制、>8MB）；
 * - 大文件查看：按 offset/len 分页读（>8MB 或点击"分块查看"的命中）；
 * - 行级跳转：命中行 → VS Code --goto file:line（未部署时如实报错）。
 */

interface SearchLine {
  lineNo: number;
  text: string;
}
interface SearchFileHits {
  path: string;
  lines: SearchLine[];
}
interface SearchReport {
  filesScanned: number;
  filesSkippedBinary: number;
  filesSkippedSize: number;
  truncated: boolean;
  hits: SearchFileHits[];
  elapsedMs: number;
}
interface FileSlice {
  offset: number;
  size: number;
  total: number;
  textLossy: string;
}

export function SearchPanel(props: { root: string }) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [report, setReport] = useState<SearchReport | null>(null);
  const [slice, setSlice] = useState<{ path: string; data: FileSlice } | null>(null);
  const [busy, setBusy] = useState(false);

  const run = useCallback(async () => {
    if (!props.root || !query.trim()) return;
    setBusy(true);
    try {
      setReport(await ipc.workspaceSearch(props.root, query.trim()));
    } catch (e) {
      pushToast("error", t("srFail"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }, [props.root, query, t]);

  const openSlice = async (path: string, offset: number) => {
    try {
      setSlice({ path, data: await ipc.bigfileSlice(path, offset, 4096) });
    } catch (e) {
      pushToast("error", t("srFail"), errMessage(e).message);
    }
  };

  const gotoLine = async (path: string, line: number) => {
    try {
      await ipc.editorGoto(path, line);
      pushToast("success", t("srGotoOk"), `${path}:${line}`);
    } catch (e) {
      pushToast("error", t("srGotoFail"), errMessage(e).message);
    }
  };

  return (
    <div className="sr-panel">
      <div className="sr-bar">
        <input
          placeholder={t("srPlaceholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void run()}
        />
        <button type="button" disabled={busy} onClick={() => void run()}>
          {t("srSearch")}
        </button>
      </div>

      {report && (
        <p className="dim small">
          {t("srSummary", {
            f: report.hits.length,
            n: report.filesScanned,
            ms: report.elapsedMs,
          })}
          {report.truncated ? ` · ${t("srTruncated")}` : ""}
        </p>
      )}

      {report?.hits.map((h) => (
        <div key={h.path} className="sr-file">
          <div className="sr-file-head">
            <code className="ellipsis">{h.path}</code>
            <span>
              <button
                type="button"
                className="icon-btn tiny"
                onClick={() => void openSlice(h.path, 0)}
                title={t("srView")}
              >
                {t("srView")}
              </button>
            </span>
          </div>
          <ul className="sr-lines">
            {h.lines.map((l) => (
              <li key={l.lineNo}>
                <code>{l.lineNo}</code> {l.text}
                <button
                  type="button"
                  className="icon-btn tiny"
                  title={t("srGoto")}
                  onClick={() => void gotoLine(h.path, l.lineNo)}
                >
                  →
                </button>
              </li>
            ))}
          </ul>
        </div>
      ))}

      {slice && (
        <div className="sr-slice card-pop">
          <div className="sr-slice-head">
            <code className="ellipsis">{slice.path}</code>
            <span className="dim small">
              @ {slice.data.offset} / {slice.data.total}
            </span>
            <button type="button" className="icon-btn tiny" onClick={() => setSlice(null)}>
              ✕
            </button>
          </div>
          <pre className="sr-slice-body">{slice.data.textLossy || "(empty)"}</pre>
          <div className="sr-slice-nav">
            <button
              type="button"
              disabled={slice.data.offset === 0}
              onClick={() => void openSlice(slice.path, Math.max(0, slice.data.offset - 4096))}
            >
              ← {t("srPrev")}
            </button>
            <button
              type="button"
              disabled={slice.data.offset + slice.data.size >= slice.data.total}
              onClick={() => void openSlice(slice.path, slice.data.offset + slice.data.size)}
            >
              {t("srNext")} →
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
