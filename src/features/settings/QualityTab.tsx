import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import { aggregateErrors, boardRecords, buildErrReport, clearErrBoard } from "../../lib/errBoard";
import { formatSmartTime } from "../../lib/formatUnits";
import { ipc, type DepAuditStateView } from "../../lib/ipc";

/**
 * AI-20 质量门禁与收官组 — M-79 前端错误聚合看板（设置→质量与诊断）。
 *
 * 近 7 天错误 Top10（组件、消息、次数、首末时间）+ 一键复制 markdown 报告
 * + 手动清空。PII 已在采集层清洗（errBoard.scrubPII）。
 * M-89 联动：首末时间走统一智能时间口径（相对/绝对切换规则）。
 */
export function QualityTab(props: { appVersion?: string }): React.ReactElement {
  const { t } = useI18n();
  const [nonce, setNonce] = useState(0);
  const [copied, setCopied] = useState(false);
  const [depAudit, setDepAudit] = useState<DepAuditStateView | null>(null);

  useEffect(() => {
    // M-85：只读状态；后端不可达（开发档未起）时如实隐藏该区块
    void ipc.depAuditStatus().then(setDepAudit).catch(() => setDepAudit(null));
  }, []);

  const entries = useMemo(() => aggregateErrors(boardRecords(), 7, 10), [nonce]);

  const copyReport = async (): Promise<void> => {
    const report = buildErrReport(entries, props.appVersion ?? "?");
    try {
      await navigator.clipboard.writeText(report);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // 剪贴板不可用（权限/非安全上下文）→ 退避下载
      try {
        const blob = new Blob([report], { type: "text/markdown" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = "variable-error-report.md";
        a.click();
        URL.revokeObjectURL(url);
      } catch {
        /* 如实静默 */
      }
    }
  };

  return (
    <div className="quality-tab" data-testid="quality-tab">
      <h4>{t("q20TabTitle")}</h4>
      <p className="dim small" style={{ whiteSpace: "pre-line" }}>{t("q20ErrBoardIntro")}</p>

      <div className="row gap8" style={{ margin: "12px 0" }}>
        <button type="button" className="btn" data-testid="err-copy-btn" onClick={() => void copyReport()} disabled={entries.length === 0}>
          {copied ? t("q20ErrCopied") : t("q20ErrCopy")}
        </button>
        <button type="button" className="btn ghost" onClick={() => { clearErrBoard(); setNonce((n) => n + 1); }} disabled={entries.length === 0}>
          {t("q20ErrClear")}
        </button>
      </div>

      {entries.length === 0 ? (
        <p className="dim small" data-testid="err-empty">{t("q20ErrEmpty")}</p>
      ) : (
        <div className="backup-list" data-testid="err-board">
          {entries.map((e, i) => (
            <div key={`${e.component}-${i}`} className="backup-row" style={{ alignItems: "flex-start" }}>
              <span className="chip err-count">×{e.count}</span>
              <div style={{ minWidth: 0, flex: 1 }}>
                <div className="small ellipsis"><strong>{e.component}</strong> · {e.message}</div>
                <div className="dim small">
                  {e.stackTop ?? "—"} · {t("q20ErrFirst")}: {formatSmartTime(e.firstAt)} · {t("q20ErrLast")}: {formatSmartTime(e.lastAt)}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* M-80 联动说明：IPC 追踪面板仅 dev 构建（Ctrl+Alt+F12），此处如实提示 */}
      <p className="dim small" style={{ marginTop: 16 }}>
        {t("q20DevPanelNote")}
      </p>

      {/* M-85 依赖审计周任务状态（只读；后端不可达时如实隐藏） */}
      {depAudit && (
        <div style={{ marginTop: 8 }} data-testid="dep-audit-status">
          <strong className="small">{t("q20DepAuditTitle")}</strong>
          <p className="dim small">
            {depAudit.lastRunMs <= 0
              ? t("q20DepAuditNever")
              : t("q20DepAuditLast", {
                  time: formatSmartTime(depAudit.lastRunMs),
                  state: depAudit.lastOk ? "OK" : depAudit.summary,
                })}
          </p>
        </div>
      )}
    </div>
  );
}
