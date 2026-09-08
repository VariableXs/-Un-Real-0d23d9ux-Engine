import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ChevronDown, ChevronRight, FileWarning } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { formatBytes } from "../../lib/format";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import type { Shell } from "../../lib/ipc";

/**
 * Z-33 重复文件侦探（VWM 虚拟窗口应用）：
 * - 目录树扫描 → 按大小分组 → 抽样哈希 → 全量哈希确认
 * - 红线：只报告，不提供任何删除按钮
 */
export function DupeApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [path, setPath] = useState("");
  const [minSize, setMinSize] = useState(1024);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState("");
  const [report, setReport] = useState<Shell.DupeReport | null>(null);
  const [open, setOpen] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<Shell.DupeProgress>("dupe://progress", (ev) => {
      const p = ev.payload;
      setPhase(
        p.phase === "collect"
          ? t("foPhaseCollect", { done: p.done, total: p.total })
          : p.phase === "hash"
            ? t("foPhaseHash", { done: p.done, total: p.total })
            : t("foPhaseConfirm", { done: p.done, total: p.total }),
      );
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
  }, [t]);

  const scan = (): void => {
    const p = path.trim();
    if (!p || busy) return;
    setBusy(true);
    setPhase(t("foPhaseCollect", { done: 0, total: 0 }));
    setReport(null);
    setOpen({});
    ipc
      .dupeScan(p, minSize > 0 ? minSize : null)
      .then(setReport)
      .catch((e: unknown) => pushToast("error", t("toolDupe"), errMessage(e).message))
      .finally(() => {
        setBusy(false);
        setPhase("");
      });
  };

  const wasted = report ? report.groups.reduce((s, g) => s + g.wasted, 0) : 0;

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
        <input
          className="text-input tiny fo-minsize"
          type="number"
          min={1}
          value={minSize}
          onChange={(e) => setMinSize(Number(e.target.value) || 0)}
          aria-label={t("foMinSize")}
          title={t("foMinSize")}
        />
        <button type="button" className="btn primary" onClick={scan} disabled={busy || !path.trim()}>
          {busy ? t("foScanning") : t("foScan")}
        </button>
      </div>

      {busy && (
        <p className="dim small fo-progress-text" role="status">
          {phase}
        </p>
      )}

      {report && (
        <>
          <div className="fo-summary">
            <span>
              {t("foGroups", { n: report.groups.length })} · {t("foScanned", { n: report.scanned })}
            </span>
            <span className="fo-wasted">
              {t("foWasted")}：{formatBytes(wasted)}
            </span>
            {report.truncated && (
              <span className="fo-truncated" role="status">
                <FileWarning size={13} /> {t("foTruncated")}
              </span>
            )}
          </div>
          <p className="dim small fo-readonly-note">{t("foReadOnlyNote")}</p>
          <div className="fo-dupe-list">
            {report.groups.length === 0 && <p className="dim small">{t("foDupeEmpty")}</p>}
            {report.groups.map((g, i) => {
              const key = `${g.hash.slice(0, 12)}-${i}`;
              const isOpen = open[key] ?? true;
              return (
                <div key={key} className="fo-dupe-group">
                  <button
                    type="button"
                    className="fo-dupe-head"
                    onClick={() => setOpen((o) => ({ ...o, [key]: !isOpen }))}
                    aria-expanded={isOpen}
                  >
                    {isOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                    <span className="fo-dupe-size">{formatBytes(g.size)}</span>
                    <span className="dim">× {g.files.length}</span>
                    <span className="dim small fo-dupe-wasted">{t("foWastedShort")} {formatBytes(g.wasted)}</span>
                  </button>
                  {isOpen && (
                    <ul className="fo-dupe-files">
                      {g.files.map((f) => (
                        <li key={f.path} title={f.path}>
                          <span className="fo-dupe-path">{f.path}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}

      {!report && !busy && <p className="dim small fo-hint">{t("foDupeHint")}</p>}
    </div>
  );
}
