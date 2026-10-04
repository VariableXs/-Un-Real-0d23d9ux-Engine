import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { ArcCard, ArcNode } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize, fmtTime, splitPaths } from "./shared";

/**
 * U-29 存档柜：zstd 归档卡片墙（创建 / 只读浏览 / SHA-256 审计 / 修复 / 提取 / 删除）。
 * 浏览支持归档内虚拟目录逐级下钻。
 */
export function ArchivePanel(): React.ReactElement {
  const { t } = useI18n();
  const [cards, setCards] = useState<ArcCard[]>([]);
  const [name, setName] = useState("");
  const [srcs, setSrcs] = useState("");
  const [busy, setBusy] = useState(false);
  const [browse, setBrowse] = useState<{ id: string; sub: string; nodes: ArcNode[] } | null>(null);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.archList().then(setCards).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const create = async (): Promise<void> => {
    const list = splitPaths(srcs);
    if (!name.trim() || list.length === 0) return;
    setBusy(true);
    try {
      await ipc.archCreate(name.trim(), list);
      pushToast("success", t("archCreateTitle"));
      setName("");
      setSrcs("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const openBrowse = async (id: string, sub: string): Promise<void> => {
    try {
      const nodes = await ipc.archBrowse(id, sub);
      setBrowse({ id, sub, nodes });
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const audit = async (id: string): Promise<void> => {
    try {
      const r = await ipc.archAudit(id);
      if (r.ok) {
        pushToast("success", t("archOk", { n: r.checked }));
      } else {
        pushToast("error", t("archCorrupt", { n: r.corrupt.length }), r.corrupt.slice(0, 5).join("\n"));
      }
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const repair = async (id: string): Promise<void> => {
    try {
      const r = await ipc.archRepair(id);
      pushToast(r.ok ? "success" : "info", t("archRepaired", { n: r.checked - r.corrupt.length, m: r.corrupt.length }));
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const extract = async (id: string): Promise<void> => {
    const dest = window.prompt(t("archExtract"));
    if (!dest) return;
    try {
      const n = await ipc.archExtract(id, dest);
      pushToast("success", t("archExtracted", { n }));
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const remove = async (id: string): Promise<void> => {
    if (!window.confirm(t("archRemove") + "?")) return;
    try {
      await ipc.archRemove(id);
      setBrowse(null);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  return (
    <section>
      <h2>{t("dvTabArchive")}</h2>
      <h3>{t("archCreateTitle")}</h3>
      <div className="dv-row">
        <input className="dv-input" placeholder={t("archNameLabel")} value={name} onChange={(e) => setName(e.target.value)} />
        <button className="dv-btn primary" disabled={busy} onClick={() => void create()}>{t("archCreateTitle")}</button>
      </div>
      <textarea
        className="dv-input wide"
        rows={3}
        style={{ width: "100%", resize: "vertical" }}
        placeholder={t("archSrcLabel")}
        value={srcs}
        onChange={(e) => setSrcs(e.target.value)}
      />

      <h3>{t("archCards")}</h3>
      {cards.length === 0 ? (
        <div className="dv-empty">{t("archEmpty")}</div>
      ) : (
        <div className="dv-cards">
          {cards.map((c) => {
            const ratio = c.bytes > 0 ? Math.round((c.storedBytes / c.bytes) * 100) : 0;
            return (
              <div key={c.id} className="dv-card">
                <div className="dv-card-title">{c.name}</div>
                <div className="dv-card-meta">
                  {fmtTime(c.createdAt)} · {t("archFilesN", { n: c.files })} · {fmtSize(c.bytes)} · {t("archRatio", { p: ratio })}
                </div>
                <div className="dv-card-actions">
                  <button className="dv-btn" onClick={() => void openBrowse(c.id, "")}>{t("archBrowse")}</button>
                  <button className="dv-btn" onClick={() => void audit(c.id)}>{t("archAudit")}</button>
                  <button className="dv-btn" onClick={() => void repair(c.id)}>{t("archRepair")}</button>
                  <button className="dv-btn" onClick={() => void extract(c.id)}>{t("archExtract")}</button>
                  <button className="dv-btn danger" onClick={() => void remove(c.id)}>{t("archRemove")}</button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {browse && (
        <div className="dv-list" style={{ marginTop: 14 }}>
          <div className="dv-item">
            <button className="dv-btn" onClick={() => {
              const parent = browse.sub.includes("/")
                ? browse.sub.slice(0, browse.sub.lastIndexOf("/"))
                : "";
              void openBrowse(browse.id, parent);
            }}>{t("archBrowseBack")}</button>
            <span className="dv-hint">/{browse.sub}</span>
            <div style={{ flex: 1 }} />
            <button className="dv-btn" onClick={() => setBrowse(null)}>✕</button>
          </div>
          {browse.nodes.map((n) => (
            <div key={n.path} className="dv-item">
              <span className="dv-chip">{n.kind === "dir" ? "DIR" : fmtSize(n.size)}</span>
              <span
                className="grow"
                style={{ cursor: n.kind === "dir" ? "pointer" : "default" }}
                onClick={() => {
                  if (n.kind === "dir") void openBrowse(browse.id, n.path);
                }}
              >
                {n.name}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

