import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Printer } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { isTauriRuntime } from "../../entries/runtime";
import type { Shell } from "../../lib/ipc";

/**
 * V-98 打印队列查看器（AI-08 · VWM 虚拟窗口应用）：
 * - 当前打印机队列列表（文档名 / 状态 / 页数 / 提交时间），多打印机可切换查看
 * - 暂停 / 恢复 / 取消单任务 —— 全部显式操作；「取消全部」需确认（不可撤销如实警告）
 * - 队列变更轮询刷新（1s；规格验收 <1s）+ 手动刷新；只读消费 winspool 数据
 * - 无打印机环境：空态如实提示（不伪造）
 * 红线：不做打印机属性/首选项设置；不做默认打印机切换（设备中心领地）。
 */

/** 刷新间隔（ms）——规格「事件刷新 <1s」，winspool 无异步通知，用 1s 轮询近似。 */
const POLL_MS = 1_000;

export function PrintQueueApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [printers, setPrinters] = useState<Shell.PrinterInfo[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Shell.PrintJob[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const timer = useRef<number | null>(null);
  // 操作去重（同一 job 连点）
  const acting = useRef<Set<number>>(new Set());

  const refresh = useCallback(
    async (printer: string | null): Promise<void> => {
      if (!isTauriRuntime()) return;
      try {
        const list = await ipc.printList();
        setPrinters(list);
        setLoadErr(null);
        // 当前打印机失效（被删）→ 回落到第一台（默认优先）
        let target = printer && list.some((p) => p.name === printer) ? printer : null;
        if (!target) target = list.find((p) => p.isDefault)?.name ?? list[0]?.name ?? null;
        setCurrent((prev) => (prev === target ? prev : target));
        setJobs(target ? await ipc.printJobs(target) : []);
      } catch (e) {
        setLoadErr(errMessage(e).message);
      } finally {
        setLoaded(true);
      }
    },
    [],
  );

  useEffect(() => {
    if (!isTauriRuntime()) {
      setLoaded(true);
      return;
    }
    void refresh(null);
    const tick = (): void => {
      timer.current = window.setTimeout(() => {
        void refresh(currentRef.current);
        tick();
      }, POLL_MS);
    };
    tick();
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 轮询回调里拿最新打印机名（避免闭包旧值）
  const currentRef = useRef<string | null>(null);
  useEffect(() => {
    currentRef.current = current;
  }, [current]);

  const act = useCallback(
    (job: Shell.PrintJob, action: "pause" | "resume" | "cancel"): void => {
      if (!current || acting.current.has(job.jobId)) return;
      acting.current.add(job.jobId);
      ipc
        .printJobSet(current, job.jobId, action)
        .then(() => {
          pushToast("success", t("pqTitle"), t("pqDone", { action: t(`pqAct_${action}`) }));
        })
        .catch((e: unknown) => {
          // 失败（脱机/缺纸/权限）透传系统错误，如实提示
          pushToast("error", t("pqTitle"), errMessage(e).message);
        })
        .finally(() => {
          acting.current.delete(job.jobId);
          void refresh(current);
        });
    },
    [current, refresh, t],
  );

  const cancelAll = useCallback(async (): Promise<void> => {
    if (!current || jobs.length === 0) return;
    const ok = await askConfirm({
      title: t("pqCancelAllTitle"),
      body: t("pqCancelAllBody", { n: jobs.length, name: current }),
      danger: true,
    });
    if (!ok) return;
    // 逐任务取消（winspool 无批量 API）；失败不中断，最后统一刷新
    const results = await Promise.allSettled(jobs.map((j) => ipc.printJobSet(current, j.jobId, "cancel")));
    const failed = results.filter((r) => r.status === "rejected").length;
    if (failed > 0) pushToast("error", t("pqTitle"), t("pqCancelAllPartial", { failed }));
    else pushToast("success", t("pqTitle"), t("pqCancelAllDone"));
    void refresh(current);
  }, [current, jobs, refresh, t]);

  const sorted = useMemo(
    () => [...jobs].sort((a, b) => (a.submitted < b.submitted ? 1 : a.submitted > b.submitted ? -1 : b.jobId - a.jobId)),
    [jobs],
  );

  return (
    <div className="fo-app pq-app">
      <div className="fo-bar">
        <Printer size={16} aria-hidden />
        {printers.length > 0 ? (
          <select
            className="ex-sort-select pq-printer-select"
            value={current ?? ""}
            onChange={(e) => {
              setCurrent(e.target.value || null);
              setJobs([]);
              void refresh(e.target.value || null);
            }}
            aria-label={t("pqPrinter")}
          >
            {printers.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
                {p.isDefault ? ` · ${t("pqDefault")}` : ""} · {t(`pqStatus_${p.status.split("/")[0]}`) !== `pqStatus_${p.status.split("/")[0]}` ? t(`pqStatus_${p.status.split("/")[0]}`) : p.status}
              </option>
            ))}
          </select>
        ) : (
          <span className="dim small">{loaded ? t("pqNoPrinter") : "…"}</span>
        )}
        <span className="dim small">{t("pqCount", { n: jobs.length })}</span>
        <span className="pq-spacer" />
        <button type="button" className="btn ghost tiny" onClick={() => void refresh(current)}>
          {t("pqRefresh")}
        </button>
        <button
          type="button"
          className="btn danger tiny"
          disabled={jobs.length === 0}
          onClick={() => void cancelAll()}
        >
          {t("pqCancelAll")}
        </button>
      </div>

      {loadErr && <p className="dim small pq-err" role="status">{loadErr}</p>}

      {loaded && printers.length === 0 && !loadErr && (
        <p className="dim small pq-empty">{t("pqNoPrinterHint")}</p>
      )}

      {printers.length > 0 && sorted.length === 0 && !loadErr && (
        <p className="dim small pq-empty">{t("pqEmpty")}</p>
      )}

      {sorted.length > 0 && (
        <div className="pq-table" role="table" aria-label={t("pqTitle")}>
          <div className="pq-row pq-head" role="row">
            <span role="columnheader">{t("pqDoc")}</span>
            <span role="columnheader">{t("pqStatusCol")}</span>
            <span role="columnheader">{t("pqPages")}</span>
            <span role="columnheader">{t("pqSubmitted")}</span>
            <span role="columnheader" aria-hidden />
          </div>
          {sorted.map((j) => (
            <div key={j.jobId} className="pq-row" role="row">
              <span className="pq-doc" title={j.document}>{j.document}</span>
              <span className="pq-status">
                <span className={`pq-dot pq-dot-${j.status.split("/")[0]}`} aria-hidden />
                {t(`pqJob_${j.status.split("/")[0]}`) !== `pqJob_${j.status.split("/")[0]}`
                  ? t(`pqJob_${j.status.split("/")[0]}`)
                  : j.status}
                {j.status.includes("paused") && j.pagesPrinted > 0 && j.pagesPrinted < j.totalPages
                  ? ` · ${j.pagesPrinted}/${j.totalPages}`
                  : ""}
              </span>
              <span className="pq-pages">
                {j.totalPages > 0 ? (j.pagesPrinted > 0 ? `${j.pagesPrinted}/${j.totalPages}` : `${j.totalPages}`) : "—"}
              </span>
              <span className="pq-time">{j.submitted}</span>
              <span className="pq-actions">
                {j.status.includes("paused") ? (
                  <button type="button" className="btn ghost tiny" onClick={() => act(j, "resume")}>
                    {t("pqAct_resume")}
                  </button>
                ) : (
                  <button type="button" className="btn ghost tiny" onClick={() => act(j, "pause")}>
                    {t("pqAct_pause")}
                  </button>
                )}
                <button type="button" className="btn ghost tiny" onClick={() => act(j, "cancel")}>
                  {t("pqAct_cancel")}
                </button>
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
