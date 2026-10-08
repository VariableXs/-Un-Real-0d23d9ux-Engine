import { useMemo, useState } from "react";
import { Plus, Trash2, Undo2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import type { Shell } from "../../lib/ipc";

/**
 * Z-32 批量重命名工具（VWM 虚拟窗口应用）：
 * - 规则管线：替换 / 序号 / 大小写 / 扩展名（预览 → 显式确认 → 应用 → 可撤销）
 * - 红线：写操作必须显式确认 + 可撤销（后端保留 undo 映射）
 */
type Rule = Shell.RenameRule;

function basename(p: string): string {
  const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
  return i >= 0 ? p.slice(i + 1) : p;
}

function ruleSummary(r: Rule): string {
  if (r.type === "replace") return `${r.find} → ${r.replace}`;
  if (r.type === "number") return `${r.start} +${r.step} (${r.pad})`;
  if (r.type === "case") return r.mode;
  return `.${r.from} → .${r.to}`;
}

export function RenameApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [rawPaths, setRawPaths] = useState("");
  const [rules, setRules] = useState<Rule[]>([]);
  const [preview, setPreview] = useState<Shell.RenamePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastUndoId, setLastUndoId] = useState<string | null>(null);

  const items = useMemo<Shell.RenameItem[]>(
    () =>
      rawPaths
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean)
        .map((p) => ({ path: p, name: basename(p) })),
    [rawPaths],
  );

  const addRule = (r: Rule): void => {
    setRules((rs) => [...rs, r]);
    setPreview(null);
  };

  const removeRule = (i: number): void => {
    setRules((rs) => rs.filter((_, j) => j !== i));
    setPreview(null);
  };

  const doPreview = (): void => {
    if (items.length === 0 || busy) return;
    setBusy(true);
    ipc
      .batchRenamePreview(items, rules)
      .then(setPreview)
      .catch((e: unknown) => pushToast("error", t("toolRename"), errMessage(e).message))
      .finally(() => setBusy(false));
  };

  const conflicts = preview ? preview.rows.filter((r) => r.conflict).length : 0;

  const apply = async (): Promise<void> => {
    if (!preview || busy || conflicts > 0) return;
    const ok = await askConfirm({
      title: t("foRenameConfirmTitle"),
      body: t("foRenameConfirmBody", { n: preview.rows.length }),
      okLabel: t("foApply"),
    });
    if (!ok) return;
    setBusy(true);
    try {
      const r = await ipc.batchRenameApply(items, rules);
      setLastUndoId(r.undoId);
      pushToast("success", t("toolRename"), t("foRenamed", { n: r.renamed }));
      setPreview(null);
    } catch (e: unknown) {
      pushToast("error", t("toolRename"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  const undo = async (): Promise<void> => {
    if (!lastUndoId || busy) return;
    setBusy(true);
    try {
      const n = await ipc.batchRenameUndo(lastUndoId);
      pushToast("success", t("toolRename"), t("foUndoDone", { n }));
      setLastUndoId(null);
      setRawPaths("");
    } catch (e: unknown) {
      pushToast("error", t("toolRename"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fo-app fo-rename">
      <label className="fo-label" htmlFor="fo-rename-paths">
        {t("foRenameFilesLabel")}
      </label>
      <textarea
        id="fo-rename-paths"
        className="text-input fo-rename-paths"
        value={rawPaths}
        placeholder={t("foRenameFilesPlaceholder")}
        rows={5}
        onChange={(e) => {
          setRawPaths(e.target.value);
          setPreview(null);
        }}
      />

      <div className="fo-rules-head">
        <span className="fo-label">{t("foRules")}</span>
        <div className="row gap4">
          <button type="button" className="btn ghost tiny" onClick={() => addRule({ type: "replace", find: "", replace: "" })}>
            <Plus size={12} /> {t("foRuleReplace")}
          </button>
          <button type="button" className="btn ghost tiny" onClick={() => addRule({ type: "number", start: 1, step: 1, pad: 3 })}>
            <Plus size={12} /> {t("foRuleNumber")}
          </button>
          <button type="button" className="btn ghost tiny" onClick={() => addRule({ type: "case", mode: "lower" })}>
            <Plus size={12} /> {t("foRuleCase")}
          </button>
          <button type="button" className="btn ghost tiny" onClick={() => addRule({ type: "ext", from: "", to: "" })}>
            <Plus size={12} /> {t("foRuleExt")}
          </button>
        </div>
      </div>

      <div className="fo-rule-list">
        {rules.length === 0 && <p className="dim small">{t("foNoRules")}</p>}
        {rules.map((r, i) => (
          <div key={i} className="fo-rule-row">
            <span className="fo-rule-kind">{t(`foRuleKind_${r.type}`)}</span>
            {r.type === "replace" && (
              <>
                <input
                  className="text-input tiny"
                  type="text"
                  placeholder={t("foFind")}
                  value={r.find}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, find: e.target.value } : x)))}
                />
                <input
                  className="text-input tiny"
                  type="text"
                  placeholder={t("foReplace")}
                  value={r.replace}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, replace: e.target.value } : x)))}
                />
              </>
            )}
            {r.type === "number" && (
              <>
                <label className="dim small">{t("foStartNo")}</label>
                <input
                  className="text-input tiny fo-num"
                  type="number"
                  value={r.start}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, start: Number(e.target.value) || 0 } : x)))}
                />
                <label className="dim small">{t("foStep")}</label>
                <input
                  className="text-input tiny fo-num"
                  type="number"
                  value={r.step}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, step: Number(e.target.value) || 1 } : x)))}
                />
                <label className="dim small">{t("foPad")}</label>
                <input
                  className="text-input tiny fo-num"
                  type="number"
                  min={0}
                  max={9}
                  value={r.pad}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, pad: Number(e.target.value) || 0 } : x)))}
                />
              </>
            )}
            {r.type === "case" && (
              <select
                className="ex-sort-select"
                value={r.mode}
                onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, mode: e.target.value } : x)))}
              >
                <option value="lower">{t("foCaseLower")}</option>
                <option value="upper">{t("foCaseUpper")}</option>
                <option value="title">{t("foCaseTitle")}</option>
              </select>
            )}
            {r.type === "ext" && (
              <>
                <input
                  className="text-input tiny fo-ext"
                  type="text"
                  placeholder="txt"
                  value={r.from}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, from: e.target.value } : x)))}
                />
                <span className="dim">→</span>
                <input
                  className="text-input tiny fo-ext"
                  type="text"
                  placeholder="md"
                  value={r.to}
                  onChange={(e) => setRules((rs) => rs.map((x, j) => (j === i ? { ...x, to: e.target.value } : x)))}
                />
              </>
            )}
            <span className="dim small fo-rule-summary" title={ruleSummary(r)}>
              {ruleSummary(r)}
            </span>
            <button type="button" className="icon-btn tiny danger-hover" onClick={() => removeRule(i)} aria-label={t("foRemove")}>
              <Trash2 size={12} />
            </button>
          </div>
        ))}
      </div>

      <div className="fo-bar">
        <button type="button" className="btn" onClick={doPreview} disabled={busy || items.length === 0}>
          {t("foPreview")}
        </button>
        <button
          type="button"
          className="btn primary"
          onClick={() => void apply()}
          disabled={busy || !preview || conflicts > 0}
          title={conflicts > 0 ? t("foConflictHint") : ""}
        >
          {t("foApply")}
        </button>
        {lastUndoId && (
          <button type="button" className="btn ghost" onClick={() => void undo()} disabled={busy}>
            <Undo2 size={13} /> {t("foUndo")}
          </button>
        )}
        {conflicts > 0 && (
          <span className="fo-conflict-count">
            {t("foConflicts", { n: conflicts })}
          </span>
        )}
      </div>

      {preview && (
        <div className="fo-preview-table" role="region" aria-label={t("foPreview")}>
          <table className="fo-table">
            <thead>
              <tr>
                <th>{t("foOldName")}</th>
                <th>{t("foNewName")}</th>
                <th>{t("foStatus")}</th>
              </tr>
            </thead>
            <tbody>
              {preview.rows.map((r) => (
                <tr key={r.path} className={r.conflict ? "fo-row-conflict" : ""} title={r.path}>
                  <td>{r.oldName}</td>
                  <td>{r.newName}</td>
                  <td className={r.conflict ? "fo-conflict" : "dim"}>
                    {r.conflict ? `${t("foConflict")}：${r.reason}` : "OK"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
