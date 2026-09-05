import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Folder, RotateCcw, Search, Trash2, X } from "lucide-react";
import { askConfirm } from "../../components/Modal";
import { errMessage, ipc } from "../../lib/ipc";
import type { RecItem } from "../../lib/ipc";
import { useI18n } from "../../i18n";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";

type RecSortKey = "time" | "type" | "name";

/**
 * 全局回收站视图（M6）：聚合 Write 数据库回收站（记录/文件夹/导图）、
 * Write 工作区 .trash、文件管理器删除的文件，支持还原 / 彻底删除 / 清空。
 * 数据 100% 来自本机真实查询，5s 轮询保持与其他窗口的删除操作同步。
 */
export function RecycleView(): React.ReactElement {
  const { t, lang } = useI18n();
  const [items, setItems] = useState<RecItem[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // 批次E（规格 6.x）：搜索 + 排序
  const [filter, setFilter] = useState("");
  const [sortKey, setSortKey] = useState<RecSortKey>("time");
  const alive = useRef(true);

  const load = useCallback(async (): Promise<void> => {
    try {
      const list = await ipc.recList();
      if (!alive.current) return;
      setItems(list);
      setSelected((sel) => (sel && list.some((i) => i.id === sel) ? sel : null));
    } catch (e) {
      if (alive.current) setItems([]);
      console.warn("[recycle] load failed", errMessage(e).message);
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void load();
    const id = window.setInterval(() => void load(), 5000);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [load]);

  const sel = useMemo(() => items?.find((i) => i.id === selected) ?? null, [items, selected]);

  // 批次E：过滤 + 排序（时间默认降序；类型/名称按当前语言不区分大小写）
  const visible = useMemo(() => {
    if (!items) return [];
    const q = filter.trim().toLowerCase();
    const base = q
      ? items.filter(
          (i) =>
            i.title.toLowerCase().includes(q) ||
            (i.origin ?? "").toLowerCase().includes(q) ||
            i.source.toLowerCase().includes(q),
        )
      : items;
    return [...base].sort((a, b) => {
      if (sortKey === "time") return b.deletedAt - a.deletedAt;
      if (sortKey === "type") return a.source.localeCompare(b.source) || b.deletedAt - a.deletedAt;
      return a.title.toLowerCase().localeCompare(b.title.toLowerCase());
    });
  }, [items, filter, sortKey]);

  // 批次E：占用容量（仅文件系统条目有真实字节数，数据库条目按 0 计）
  const totalBytes = useMemo(() => (items ?? []).reduce((n, i) => n + (i.size || 0), 0), [items]);

  const restore = async (): Promise<void> => {
    if (!sel || busy) return;
    setBusy(true);
    try {
      await ipc.recRestore(sel.id, sel.source);
      pushToast("success", t("restoredToast"), sel.title);
      await load();
    } catch (e) {
      pushToast("error", t("restoreSel"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  const purge = async (): Promise<void> => {
    if (!sel || busy) return;
    const ok = await askConfirm({
      title: t("purgeConfirmTitle"),
      body: t("purgeConfirmBody", { name: sel.title }),
      danger: true,
      okLabel: t("purgeSel"),
    });
    if (!ok) return;
    setBusy(true);
    try {
      await ipc.recPurge(sel.id, sel.source);
      await load();
    } catch (e) {
      pushToast("error", t("purgeSel"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  const emptyAll = async (): Promise<void> => {
    if (busy || !items || items.length === 0) return;
    const ok = await askConfirm({
      title: t("emptyRecycle"),
      body: t("emptyRecycleConfirmBody", { n: items.length }),
      danger: true,
      okLabel: t("emptyRecycle"),
    });
    if (!ok) return;
    setBusy(true);
    try {
      const n = await ipc.recEmpty();
      pushToast("success", t("emptiedToast", { n }));
      await load();
    } catch (e) {
      pushToast("error", t("emptyRecycle"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  const sourceLabel = (s: RecItem["source"]): string => {
    switch (s) {
      case "doc": return t("kindDocument");
      case "folder": return t("kindFolder");
      case "mindmap": return t("kindMindmap");
      case "ws-file": return t("srcWsFile");
      default: return t("srcFsItem");
    }
  };

  return (
    <div className="ex-recycle" data-busy={busy || undefined}>
      <div className="ex-toolbar">
        <button type="button" className="ex-tool-btn" disabled={!sel} onClick={() => void restore()}>
          <RotateCcw size={15} /> {t("restoreSel")}
        </button>
        <button type="button" className="ex-tool-btn danger" disabled={!sel} onClick={() => void purge()}>
          <Trash2 size={15} /> {t("purgeSel")}
        </button>
        <button
          type="button"
          className="ex-tool-btn danger"
          disabled={!items || items.length === 0}
          onClick={() => void emptyAll()}
        >
          <Trash2 size={15} /> {t("emptyRecycle")}
        </button>

        {/* 批次E：搜索 + 排序切换 + 容量显示 */}
        <span className="ex-search">
          <Search size={14} className="dim" />
          <input
            type="text"
            value={filter}
            placeholder={t("recSearchHint")}
            onChange={(e) => setFilter(e.target.value)}
          />
          {filter && (
            <button type="button" className="ex-side-btn" aria-label={t("copy")} onClick={() => setFilter("")}>
              <X size={12} />
            </button>
          )}
        </span>
        <button
          type="button"
          className="ex-tool-btn"
          title={t("recSortTime")}
          onClick={() => setSortKey((k) => (k === "time" ? "type" : k === "type" ? "name" : "time"))}
        >
          {sortKey === "time" ? t("recSortTime") : sortKey === "type" ? t("sortType") : t("sortName")}
        </button>
        <span className="dim small" style={{ marginLeft: "auto" }}>
          {items ? t("recOccupied", { size: formatBytes(totalBytes) }) : ""}
        </span>
      </div>

      <div className="ex-list">
        <div className="ex-head-row">
          <span>{t("colName")}</span>
          <span>{t("colSource")}</span>
          <span>{t("colOrigin")}</span>
          <span>{t("colDeletedAt")}</span>
        </div>
        {items === null ? (
          <div className="ex-hint dim" aria-busy="true" />
        ) : visible.length === 0 ? (
          <div className="ex-hint dim">{items.length === 0 ? t("emptyRecHint") : t("noResults")}</div>
        ) : (
          visible.map((it) => (
            <div
              key={`${it.source}:${it.id}`}
              className={`ex-row${selected === `${it.source}:${it.id}` ? " selected" : ""}`}
              onClick={() => setSelected(`${it.source}:${it.id}`)}
              onDoubleClick={() => void restore()}
            >
              <span className="ex-col-name">
                <Folder size={16} className="ex-ic" /> <span>{it.title}</span>
              </span>
              <span className="ex-col-src">{sourceLabel(it.source)}</span>
              <span className="ex-col-origin dim">{it.origin ?? "—"}</span>
              <span className="ex-col-date">{it.deletedAt ? new Date(it.deletedAt).toLocaleString(lang === "en" ? "en-US" : "zh-CN") : "—"}</span>
            </div>
          ))
        )}
      </div>

      <div className="ex-status">
        {items ? t("items", { n: items.length }) : ""}
      </div>
    </div>
  );
}
