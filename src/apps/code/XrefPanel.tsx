/**
 * 批次C（规格 5.7.3）：Variable Code 的引用面板 —— Code 引用 Variable Write 的技术文档。
 * - 添加引用：选择 Write 文档，记录添加时的版本（updatedAt）
 * - 源文档更新后显示"内容已更新"；删除后显示"源文档已删除"
 * - 点击"打开"跳转 Write 并定位文档；"复制引用"可把引用粘进 Write 正文
 * 引用清单仅存本机（localStorage），零网络。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { BookOpen, Copy, ExternalLink, Plus, RefreshCw, Trash2, X } from "lucide-react";
import { ipc, errMessage } from "../../lib/ipc";
import type { DocumentMeta } from "../../lib/types";
import { pushToast } from "../../state/uiStore";
import { useI18n } from "../../i18n";
import { copyXrefToClipboard, openXref, type XrefKind } from "../../lib/xref";

interface RefEntry {
  kind: Extract<XrefKind, "write-doc">;
  id: string;
  ver: number;
  title: string;
  addedAt: number;
}

const LS_KEY = "code.xrefs.v1";

function loadRefs(): RefEntry[] {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const arr = JSON.parse(raw) as RefEntry[];
      if (Array.isArray(arr)) return arr.filter((r) => r && r.kind === "write-doc" && r.id);
    }
  } catch { /* corrupt → empty */ }
  return [];
}

/** 单条引用的当前状态。 */
interface RefStatus {
  /** 源当前版本；-1 = 文档缺失（已删除/回收） */
  sourceVer: number;
}

export function CodeXrefPanel(): React.ReactElement {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [refs, setRefs] = useState<RefEntry[]>(loadRefs);
  const [docs, setDocs] = useState<DocumentMeta[]>([]);
  const [docsLoaded, setDocsLoaded] = useState(false);
  const [pickId, setPickId] = useState("");
  const [status, setStatus] = useState<Record<string, RefStatus>>({});
  const [checking, setChecking] = useState(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const persist = useCallback((next: RefEntry[]): void => {
    setRefs(next);
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(next));
    } catch { /* quota */ }
  }, []);

  const loadDocs = useCallback(async (): Promise<void> => {
    try {
      const list = await ipc.listDocuments({ view: "all", sort: "updated" });
      if (!mountedRef.current) return;
      setDocs(list);
      setDocsLoaded(true);
    } catch {
      if (mountedRef.current) setDocsLoaded(true);
    }
  }, []);

  const checkAll = useCallback(async (list: RefEntry[]): Promise<void> => {
    if (list.length === 0) {
      setStatus({});
      return;
    }
    setChecking(true);
    const next: Record<string, RefStatus> = {};
    for (const r of list) {
      try {
        const d = await ipc.getDocument(r.id);
        next[r.id] = { sourceVer: d.updatedAt };
      } catch {
        next[r.id] = { sourceVer: -1 };
      }
    }
    if (mountedRef.current) {
      setStatus(next);
      setChecking(false);
    }
  }, []);

  // 打开面板时拉取文档列表 + 校验引用状态
  useEffect(() => {
    if (!open) return;
    void loadDocs();
    void checkAll(refs);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const addRef = useCallback((): void => {
    const meta = docs.find((d) => d.id === pickId);
    if (!meta) return;
    if (refs.some((r) => r.id === meta.id)) {
      pushToast("info", t("refsDup"));
      return;
    }
    const entry: RefEntry = { kind: "write-doc", id: meta.id, ver: meta.updatedAt, title: meta.title, addedAt: Date.now() };
    persist([...refs, entry]);
    setStatus((s) => ({ ...s, [meta.id]: { sourceVer: meta.updatedAt } }));
    setPickId("");
    pushToast("success", t("refsAdded"));
  }, [docs, pickId, refs, persist, t]);

  const removeRef = useCallback((id: string): void => {
    persist(refs.filter((r) => r.id !== id));
    setStatus((s) => {
      const { [id]: _drop, ...rest } = s;
      return rest;
    });
  }, [refs, persist]);

  const openRef = useCallback(async (r: RefEntry): Promise<void> => {
    try {
      await openXref({ kind: r.kind, id: r.id, ver: r.ver });
    } catch (e) {
      pushToast("error", t("refsOpenFailed"), errMessage(e).message);
    }
  }, [t]);

  const copyRef = useCallback(async (r: RefEntry): Promise<void> => {
    await copyXrefToClipboard(r.kind, r.id, r.ver, r.title);
    pushToast("success", t("refsCopied"));
  }, [t]);

  return (
    <>
      {!open && (
        <button type="button" className="xp-fab" title={t("refsTitle")} onClick={() => setOpen(true)}>
          <BookOpen size={17} />
        </button>
      )}
      {open && (
        <div className="xp-panel" role="dialog" aria-label={t("refsTitle")}>
          <div className="xp-head">
            <span className="xp-title"><BookOpen size={15} /> {t("refsTitle")}</span>
            <span className="xp-spacer" />
            <button
              type="button"
              className="xp-iconbtn"
              title={t("exRefresh")}
              disabled={checking}
              onClick={() => void checkAll(refs)}
            >
              <RefreshCw size={14} className={checking ? "spin" : undefined} />
            </button>
            <button type="button" className="xp-iconbtn" title={t("close")} onClick={() => setOpen(false)}>
              <X size={15} />
            </button>
          </div>

          <div className="xp-add">
            <select
              className="xp-select"
              value={pickId}
              onChange={(e) => setPickId(e.target.value)}
            >
              <option value="">{docsLoaded ? t("refsPick") : t("refsLoading")}</option>
              {docs.map((d) => (
                <option key={d.id} value={d.id}>{d.title}</option>
              ))}
            </select>
            <button type="button" className="xp-addbtn" disabled={!pickId} onClick={addRef}>
              <Plus size={14} /> {t("refsAdd")}
            </button>
          </div>

          {refs.length === 0 ? (
            <div className="xp-empty">{t("refsEmpty")}</div>
          ) : (
            <div className="xp-list">
              {refs.map((r) => {
                const st = status[r.id];
                const stale = st !== undefined && st.sourceVer > r.ver;
                const missing = st !== undefined && st.sourceVer < 0;
                return (
                  <div key={r.id} className="xp-row">
                    <button
                      type="button"
                      className="xp-main"
                      title={t("refsOpen")}
                      onClick={() => void openRef(r)}
                    >
                      <ExternalLink size={13} />
                      <span className="xp-name">{r.title || r.id}</span>
                      {missing ? (
                        <span className="xp-badge missing">{t("refsMissing")}</span>
                      ) : stale ? (
                        <span className="xp-badge stale">{t("refsUpdated")}</span>
                      ) : null}
                    </button>
                    <span className="xp-actions">
                      <button
                        type="button"
                        className="xp-iconbtn"
                        title={t("refsCopy")}
                        onClick={() => void copyRef(r)}
                      >
                        <Copy size={13} />
                      </button>
                      <button
                        type="button"
                        className="xp-iconbtn danger"
                        title={t("refsRemove")}
                        onClick={() => removeRef(r.id)}
                      >
                        <Trash2 size={13} />
                      </button>
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </>
  );
}
