/**
 * N-09 小组件 Board（车道 E 自挂载 overlay）：
 * - 两形态：桌面常驻（自由拖放 + 32px 网格吸附 + 互不遮挡判定）/ 组件板（右缘侧滑）；
 * - 布局持久化 localStorage variable:widgets:layout:v1；默认无卡片（默认行为 = 现状）；
 * - 内置 8 组件 + 第三方 manifest+html 导入（iframe 沙箱 + 白名单消息 + 失败 3 次自动收起）；
 * - 全屏（sys://fullscreen）/低电（Battery API）经 useWidgetGate 暂停各组件刷新回调；
 * - reduce-motion 时关闭进出场动画。
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ComponentType, type ReactElement } from "react";
import { createPortal } from "react-dom";
import { LayoutGrid, Monitor, Pencil, Plus, Trash2, X } from "lucide-react";
import { pushOverlay, popOverlay, pushToast } from "../../state/uiStore";
import {
  clampToViewport, loadLayout, saveLayout, snap, SIZE_PX,
  resolvePlacement, type WgtLayoutDoc, type WgtLayoutItem, type WgtSize,
} from "./layout";
import {
  validateManifest, WIDGET_HTML_MAX_BYTES, type WidgetFailureDoc, type WidgetManifest,
} from "./manifest";
import { useWidgetGate } from "./gate";
import { LABELS, useLaneLang } from "./labels";
import WorldClock from "./widgets/WorldClock";
import MonthCalendar from "./widgets/MonthCalendar";
import TodoList from "./widgets/TodoList";
import QuickLaunch from "./widgets/QuickLaunch";
import FocusTimer from "./widgets/FocusTimer";
import NetStatus from "./widgets/NetStatus";
import InspireCard from "./widgets/InspireCard";
import PerfMini from "./widgets/PerfMini";
import ThirdPartyHost from "./widgets/ThirdPartyHost";

interface BoardProps {
  onClose: () => void;
}

interface ThirdWidget {
  manifest: WidgetManifest;
  html: string;
}

const THIRD_LS_KEY = "variable:widgets:third:v1";

const BUILTINS: { id: string; size: WgtSize; render: ComponentType<{ paused: boolean }> }[] = [
  { id: "builtin:clock", size: "2x1", render: WorldClock },
  { id: "builtin:calendar", size: "2x2", render: MonthCalendar },
  { id: "builtin:todos", size: "2x2", render: TodoList },
  { id: "builtin:quicklaunch", size: "1x1", render: QuickLaunch },
  { id: "builtin:focus", size: "1x1", render: FocusTimer },
  { id: "builtin:net", size: "1x1", render: NetStatus },
  { id: "builtin:inspire", size: "2x1", render: InspireCard },
  { id: "builtin:perf", size: "2x1", render: PerfMini },
];

function loadThird(): ThirdWidget[] {
  try {
    const raw = localStorage.getItem(THIRD_LS_KEY);
    const arr = raw ? (JSON.parse(raw) as ThirdWidget[]) : [];
    return Array.isArray(arr) ? arr.filter((w) => w && w.manifest && typeof w.html === "string") : [];
  } catch {
    return [];
  }
}

export default function WidgetBoard({ onClose }: BoardProps): ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const gate = useWidgetGate();
  const [doc, setDoc] = useState<WgtLayoutDoc>(loadLayout);
  const [edit, setEdit] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [third, setThird] = useState<ThirdWidget[]>(loadThird);
  const [failures, setFailures] = useState<WidgetFailureDoc>({ counts: {} });
  const dragRef = useRef<{ id: string; dx: number; dy: number } | null>(null);
  const layerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    pushOverlay("widget-board");
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popOverlay("widget-board");
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const update = useCallback((next: WgtLayoutDoc): void => {
    setDoc(next);
    saveLayout(next);
  }, []);

  const visible = useMemo(() => doc.items.filter((i) => !i.collapsed), [doc.items]);

  const addBuiltin = (id: string): void => {
    if (doc.items.some((i) => i.id === id)) {
      update({ ...doc, items: doc.items.map((i) => (i.id === id ? { ...i, collapsed: false } : i)) });
      setAddOpen(false);
      return;
    }
    const size = BUILTINS.find((b) => b.id === id)?.size ?? "1x1";
    const { w } = SIZE_PX[size];
    const vw = window.innerWidth;
    const item: WgtLayoutItem = {
      id,
      size,
      x: snap(Math.max(0, vw - w - 80)),
      y: snap(80 + doc.items.length * 24),
    };
    update({ ...doc, items: [...doc.items, item] });
    setAddOpen(false);
  };

  const removeItem = (id: string): void => {
    update({ ...doc, items: doc.items.filter((i) => i.id !== id) });
  };

  // ---------- 拖放（仅桌面常驻 + 编辑布局模式） ----------
  const onPointerDown = (e: React.PointerEvent, item: WgtLayoutItem): void => {
    if (!edit || doc.mode !== "desktop") return;
    const layer = layerRef.current;
    if (!layer) return;
    const rect = layer.getBoundingClientRect();
    dragRef.current = { id: item.id, dx: e.clientX - rect.left - item.x, dy: e.clientY - rect.top - item.y };
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent): void => {
    const drag = dragRef.current;
    const layer = layerRef.current;
    if (!drag || !layer) return;
    const rect = layer.getBoundingClientRect();
    const item = doc.items.find((i) => i.id === drag.id);
    if (!item) return;
    const p = clampToViewport(
      e.clientX - rect.left - drag.dx,
      e.clientY - rect.top - drag.dy,
      item.size,
      layer.clientWidth,
      layer.clientHeight,
    );
    setDoc((d) => ({ ...d, items: d.items.map((i) => (i.id === drag.id ? { ...i, x: p.x, y: p.y } : i)) }));
  };

  const onPointerUp = (): void => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag) return;
    const item = doc.items.find((i) => i.id === drag.id);
    if (!item) return;
    const layer = layerRef.current;
    update({
      ...doc,
      items: resolvePlacement(doc.items, drag.id, item.x, item.y, layer?.clientWidth ?? window.innerWidth, layer?.clientHeight ?? window.innerHeight),
    });
  };

  // ---------- 第三方导入（本地文件，零网络） ----------
  const importManifest = async (file: File | undefined): Promise<void> => {
    if (!file) return;
    try {
      const r = validateManifest(JSON.parse(await file.text()) as unknown);
      if (!r.ok || !r.data) {
        pushToast("error", t.thirdBad, r.errors.join("; "));
        return;
      }
      const m = r.data;
      const others = third.filter((w) => w.manifest.id !== m.id);
      setThird([...others, { manifest: m, html: "__PENDING__" }]);
      // 记住待配 html 的 manifest，等第二个文件
      (window as unknown as { __wgtPendingManifest?: WidgetManifest }).__wgtPendingManifest = m;
      pushToast("info", t.thirdPickHtml, m.name);
    } catch (err) {
      pushToast("error", t.thirdBad, String(err));
    }
  };

  const importHtml = async (file: File | undefined): Promise<void> => {
    if (!file) return;
    if (file.size > WIDGET_HTML_MAX_BYTES) {
      pushToast("error", t.thirdTooBig);
      return;
    }
    const m = (window as unknown as { __wgtPendingManifest?: WidgetManifest }).__wgtPendingManifest;
    if (!m) {
      pushToast("info", t.thirdPickManifest);
      return;
    }
    const html = await file.text();
    const others = third.filter((w) => w.manifest.id !== m.id);
    const next = [...others, { manifest: m, html }];
    setThird(next);
    try {
      localStorage.setItem(THIRD_LS_KEY, JSON.stringify(next));
    } catch {
      /* storage full —— 会话内可用 */
    }
    (window as unknown as { __wgtPendingManifest?: WidgetManifest }).__wgtPendingManifest = undefined;
    if (!doc.items.some((i) => i.id === `tp:${m.id}`)) {
      const item: WgtLayoutItem = { id: `tp:${m.id}`, size: m.size, x: snap(64), y: snap(64) };
      update({ ...doc, items: [...doc.items, item] });
    }
    pushToast("success", t.thirdAdded, m.name);
    setAddOpen(false);
  };

  const onCollapse = (id: string): void => {
    update({ ...doc, items: doc.items.map((i) => (i.id === id ? { ...i, collapsed: true } : i)) });
    pushToast("info", t.collapsed, id);
  };

  const renderBody = (item: WgtLayoutItem): ReactElement => {
    if (item.id.startsWith("tp:")) {
      const w = third.find((x) => x.manifest.id === item.id.slice(3));
      if (!w || w.html === "__PENDING__") return <div className="wgt-dim">{t.thirdPickHtml}</div>;
      return (
        <ThirdPartyHost
          manifest={w.manifest}
          html={w.html}
          paused={gate.paused}
          failures={failures}
          onFailures={setFailures}
          onCollapse={onCollapse}
        />
      );
    }
    const b = BUILTINS.find((x) => x.id === item.id);
    if (!b) return <div className="wgt-dim">?</div>;
    const Comp = b.render;
    return <Comp paused={gate.paused} />;
  };

  const isDesktop = doc.mode === "desktop";

  return createPortal(
    <div className={`wgt-root ${gate.reduceMotion ? "wgt-noanim" : ""}`}>
      {/* 右缘把手：形态切换 / 编辑 / 添加 */}
      <div className="wgt-handle">
        <button type="button" className="wgt-handle-btn" title={isDesktop ? t.boardMode : t.desktopMode} onClick={() => update({ ...doc, mode: isDesktop ? "board" : "desktop" })}>
          {isDesktop ? <LayoutGrid size={14} /> : <Monitor size={14} />}
        </button>
        <button type="button" className={`wgt-handle-btn ${edit ? "on" : ""}`} title={t.editLayout} onClick={() => setEdit((v) => !v)}>
          <Pencil size={14} />
        </button>
        <button type="button" className="wgt-handle-btn" title={t.add} onClick={() => setAddOpen((v) => !v)}>
          <Plus size={14} />
        </button>
        <button type="button" className="wgt-handle-btn" title={t.board} aria-label={t.board} onClick={onClose}>
          <X size={14} />
        </button>
      </div>

      {addOpen && (
        <div className="wgt-addmenu">
          <div className="wgt-addmenu-title">{t.add}</div>
          {BUILTINS.filter((b) => !doc.items.some((i) => i.id === b.id && !i.collapsed)).map((b) => (
            <button key={b.id} type="button" className="wgt-addmenu-item" onClick={() => addBuiltin(b.id)}>
              {b.id.replace("builtin:", "")} · {b.size}
            </button>
          ))}
          <div className="wgt-addmenu-sep" />
          <label className="wgt-addmenu-item file">
            {t.thirdPickManifest}
            <input type="file" accept=".json,application/json" hidden onChange={(e) => void importManifest(e.target.files?.[0])} />
          </label>
          <label className="wgt-addmenu-item file">
            {t.thirdPickHtml}
            <input type="file" accept=".html,.htm,text/html" hidden onChange={(e) => void importHtml(e.target.files?.[0])} />
          </label>
          <div className="wgt-addmenu-note">{t.sdkNote}</div>
        </div>
      )}

      {gate.paused && (
        <div className="wgt-paused">{gate.reason === "fullscreen" ? t.pausedFs : t.pausedBattery}</div>
      )}

      {isDesktop ? (
        <div
          ref={layerRef}
          className={`wgt-layer ${edit ? "editing" : ""}`}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          {visible.map((item) => {
            const { w, h } = SIZE_PX[item.size];
            return (
              <div
                key={item.id}
                className="wgt-card"
                style={{ left: item.x, top: item.y, width: w, height: h, touchAction: "none" }}
                onPointerDown={(e) => onPointerDown(e, item)}
              >
                <div className="wgt-card-head">
                  <span>{item.id.replace(/^(builtin:|tp:)/, "")}</span>
                  {edit && (
                    <button type="button" className="wgt-x" aria-label={t.remove} onPointerDown={(e) => e.stopPropagation()} onClick={() => removeItem(item.id)}>
                      <Trash2 size={11} />
                    </button>
                  )}
                </div>
                <div className="wgt-card-body">{renderBody(item)}</div>
              </div>
            );
          })}
        </div>
      ) : (
        <div className="wgt-drawer">
          <div className="wgt-drawer-head">
            <strong>{t.board}</strong>
            {edit && <span className="wgt-dim tiny">{t.editLayout} · {t.remove}</span>}
          </div>
          <div className="wgt-drawer-list">
            {visible.length === 0 && <div className="wgt-dim">{t.add}</div>}
            {visible.map((item) => (
              <div key={item.id} className="wgt-card static">
                <div className="wgt-card-head">
                  <span>{item.id.replace(/^(builtin:|tp:)/, "")}</span>
                  <button type="button" className="wgt-x" aria-label={t.remove} onClick={() => removeItem(item.id)}>
                    <Trash2 size={11} />
                  </button>
                </div>
                <div className="wgt-card-body">{renderBody(item)}</div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>,
    document.body,
  );
}