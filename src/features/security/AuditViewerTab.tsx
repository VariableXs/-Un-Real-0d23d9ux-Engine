/**
 * 任务 36（AI-V）：审计查看页（设置页新标签）。
 *
 * 视觉：与通知中心视觉一致（复用同一设计系统的卡片/列表/时间线语义）。
 * 逻辑：时间线（按 seq 倒序）/ 按进程(PID)过滤 / 按路径过滤 / 导出 JSON+CSV。
 * 性能：虚拟列表 windowing（定高行 + 滚动窗口 ± 缓冲），万条记录滚动流畅。
 */
import { useEffect, useMemo, useRef, useState } from "react";
import "./security.css";
import { useI18n } from "../../i18n";
import { pushToast } from "../../state/uiStore";
import { EmptyState } from "../../components/EmptyState";
import { getDefaultBridge } from "./whitelistBridge";
import { auditToCsv, auditToJson } from "./csvExport";
import { computeWindow } from "./virtualList";
import { filterAudit, sortBySeqDesc } from "./auditFilter";
import { auditActionKind, type AuditEntry } from "./whitelistTypes";

const ROW_H = 56;

function download(filename: string, content: string, mime: string): void {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

function actionLabel(t: (k: string) => string, e: AuditEntry): string {
  const kind = auditActionKind(e);
  switch (kind) {
    case "allow-read":
      return t("avActionAllowRead");
    case "allow-write":
      return t("avActionAllowWrite");
    case "deny-read":
      return t("avActionDenyRead");
    case "deny-write":
      return t("avActionDenyWrite");
    case "add":
      return t("avActionAdd");
    case "remove":
      return t("avActionRemove");
  }
}

export function AuditViewerTab(): React.ReactElement {
  const { t } = useI18n();
  const bridge = getDefaultBridge();
  const [, force] = useState(0);
  const [pid, setPid] = useState("");
  const [path, setPath] = useState("");
  const [scrollTop, setScrollTop] = useState(0);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [viewport, setViewport] = useState(420);

  useEffect(() => {
    let alive = true;
    if (!bridge.isInitialized()) {
      void bridge.init().then(() => alive && force((n) => n + 1));
    }
    const off = bridge.onChange(() => alive && force((n) => n + 1));
    return () => {
      alive = false;
      off();
    };
  }, [bridge]);

  useEffect(() => {
    if (scrollRef.current) setViewport(scrollRef.current.clientHeight);
  }, []);

  const all = bridge.getAudit();
  const filtered = useMemo(
    () =>
      sortBySeqDesc(
        filterAudit(all, {
          pid: pid.trim() === "" ? undefined : Number(pid),
          pathSubstr: path,
        }),
      ),
    [all, pid, path],
  );

  const win = computeWindow({
    scrollTop,
    viewportHeight: viewport,
    rowHeight: ROW_H,
    total: filtered.length,
  });
  const slice = filtered.slice(win.start, win.end);

  const doExport = (fmt: "json" | "csv"): void => {
    try {
      const stamp = new Date().toISOString().slice(0, 19).replace(/[:]/g, "-");
      if (fmt === "csv") {
        download(`vfs-audit-${stamp}.csv`, auditToCsv(filtered), "text/csv;charset=utf-8");
      } else {
        download(`vfs-audit-${stamp}.json`, auditToJson(filtered), "application/json");
      }
      pushToast("success", t("avExportDone"), fmt.toUpperCase());
    } catch (e) {
      pushToast("error", t("avExportFail"), String(e));
    }
  };

  return (
    <div className="av-tab">
      <p className="dim small">{t("avDesc")}</p>

      <div className="av-filters st-actions">
        <label className="st-field-inline small">
          {t("avFilterProc")}
          <input
            value={pid}
            inputMode="numeric"
            placeholder={t("avFilterProcPh")}
            data-testid="av-filter-proc"
            onChange={(e) => setPid(e.target.value.replace(/[^0-9]/g, ""))}
          />
        </label>
        <label className="st-field-inline small">
          {t("avFilterPath")}
          <input
            value={path}
            placeholder={t("avFilterPathPh")}
            data-testid="av-filter-path"
            onChange={(e) => setPath(e.target.value)}
          />
        </label>
        <button type="button" onClick={() => doExport("csv")} data-testid="av-export-csv">
          {t("avBtnCsv")}
        </button>
        <button type="button" onClick={() => doExport("json")} data-testid="av-export-json">
          {t("avBtnJson")}
        </button>
      </div>

      <p className="dim small">{t("avCount", { n: filtered.length })}</p>

      {filtered.length === 0 ? (
        <EmptyState
          icon="notification"
          title={t("avEmptyTitle")}
          description={t("avEmptyDesc")}
        />
      ) : (
        <div
          className="av-scroll"
          ref={scrollRef}
          data-testid="av-scroll"
          style={{ height: 420, overflow: "auto", position: "relative" }}
          onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        >
          <div style={{ height: win.totalHeight, position: "relative" }}>
            <div style={{ transform: `translateY(${win.offsetY}px)` }}>
              {slice.map((e: AuditEntry) => {
                const allow = e.source === "ui-change" ? true : e.allow;
                const dot = allow ? "var(--ok, #3fb950)" : "var(--danger, #e5534b)";
                return (
                  <div className="av-row" key={e.seq} style={{ height: ROW_H }}>
                    <span className="av-dot" style={{ background: dot }} aria-hidden="true" />
                    <span className="av-seq mono">{e.seq}</span>
                    <span className="av-action">{actionLabel(t, e)}</span>
                    <span className="av-path mono ellipsis">{e.path}</span>
                    <span className="av-meta small dim">
                      {e.source === "ui-change"
                        ? `${t("avColOperator")}: ${e.operator ?? "local-ui"}`
                        : `${t("avColPid")}: ${e.pid}`}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
