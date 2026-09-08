import type { VwmWin, VwmRect } from "./vwm";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { appAccent } from "../../components/AppGlyphs";
import { isTpApp } from "./vwm";
import { ipc } from "../../lib/ipc";
import { isDragStart, liveDragThreshold } from "../../lib/inputFeel";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { askChoice, askConfirm, askPrompt } from "../../components/Modal";
import { useStore } from "../../lib/store";
import { embedStateStore } from "./embedState";
// AI-01 窗口手感组：M-01 摇晃 / Z-41 手势 / M-07 参考线（纯函数层）
import { detectGesture, detectShake, computeGuide } from "./winfeel";
// AI-01：Z-36 菜单模型 + 逐应用透明度记忆 + M-06 挂起登记
import { appOpacityOf, rememberAppOpacity, setSuspended, suspensionStore, winFeelMenuItems } from "./winfeelMenu";
import {
  activateVwmTab,
  applyLayoutSnapshot,
  closeVwmTab,
  ferryVwmWin,
  groupMembersOf,
  groupVwmWins,
  listLayoutSnapshots,
  minimizeVwmWin,
  moveVwmWin,
  pointerFocusVwm,
  resizeVwmWin,
  rollVwmWin,
  saveLayoutSnapshot,
  setVwmGuides,
  setVwmOpacity,
  setVwmSnapPreview,
  setVwmTopmost,
  settleVwmWin,
  shakeMinimizeOthers,
  snapVwmRect,
  snapZoneForVwm,
  tabsEnabled,
  toggleMaxVwmWin,
  ungroupVwmWin,
  unmaxVwmTo,
  vwmStore,
  vwmWindowTitle,
  closeVwmWin,
} from "./vwm";

/**
 * 虚拟窗口框架：
 * - 标题栏拖拽移动（最大化态拖动 → Windows 习惯还原跟随）+ 双击最大化/还原
 * - 拖到屏幕边缘 → 贴靠预览（左右半屏 / 四角 1/4 / 顶部最大化），松手应用
 * - 八向边缘缩放（min 820×540，与既有系统窗口一致）
 * - 右上角 Mac 风格红绿灯：🟢 退出（关闭）/ 🟡 全屏（最大化-还原）/ 🔴 最小化
 * - AI-01 窗口手感：M-01 摇一摇最小化 / Z-41 手势 / M-07 参考线 / M-05 边缘摆渡 /
 *   Z-36 右键菜单（透明度+置顶）/ M-02 卷帘 / M-06 挂起 / M-04 未响应徽标 / Z-40 布局快照
 *
 * 壳层只做几何与层级调度；children（软件视图）零触碰。
 */

const MIN_W = 820;
const MIN_H = 540;
const EDGE = 12; // 贴靠判定边距（CSS 像素）

type SnapDir = "left" | "right" | "up" | "tl" | "tr" | "bl" | "br";

function zoneFromPointer(px: number, py: number, wa: VwmRect): SnapDir | null {
  const nearTop = py <= wa.y + EDGE;
  const nearLeft = px <= wa.x + EDGE;
  const nearRight = px >= wa.x + wa.w - EDGE;
  const nearBottom = py >= wa.y + wa.h - EDGE;
  if (nearTop && nearLeft) return "tl";
  if (nearTop && nearRight) return "tr";
  if (nearBottom && nearLeft) return "bl";
  if (nearBottom && nearRight) return "br";
  if (nearTop) return "up";
  if (nearLeft) return "left";
  if (nearRight) return "right";
  return null;
}

const APP_TITLES = vwmWindowTitle;

export function VirtualWindowFrame(props: {
  win: VwmWin;
  focused: boolean;
  zIndex: number;
  /** 批次E-14 动效：关闭仪式中（缩小淡出）/ 最小化飞行中（飞向任务栏）。 */
  closing?: boolean;
  flying?: boolean;
  /** AI-01：窗口手感设置（透明度/摇一摇/参考线/手势开关；缺省 = 全部最保守关闭）。 */
  settings?: Settings;
  /** AI-01 M-04：无响应（IsHungAppWindow 命中）徽标。 */
  hung?: boolean;
  children: React.ReactNode;
}): React.ReactElement {
  const { t } = useI18n();
  const win = props.win;
  const title = APP_TITLES(win.app);
  // 拖拽中半透明 + 抬起阴影；贴靠/最大化时平滑滑入
  const [dragging, setDragging] = useState(false);
  const [snapping, setSnapping] = useState(false);
  // AI-01 Z-36：标题栏右键菜单（模型与透明度记忆在 winfeelMenu）
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const feel = props.settings;
  const suspVersion = useStore(suspensionStore, (s) => (s.suspended[win.id] === true ? 1 : 0));
  const suspendedNow = suspVersion === 1;
  const menuRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (ev: MouseEvent): void => {
      if (menuRef.current?.contains(ev.target as Node)) return;
      setMenu(null);
    };
    const onKey = (ev: KeyboardEvent): void => {
      if (ev.key === "Escape") setMenu(null);
    };
    window.addEventListener("pointerdown", close, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", close, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [menu]);
  const playSnap = (): void => {
    setSnapping(true);
    window.setTimeout(() => setSnapping(false), 240);
  };

  // ---------- 标题栏拖拽 ----------
  const onTitlePointerDown = (e: React.PointerEvent): void => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest(".vwm-lights")) return;
    pointerFocusVwm(win.id);
    const s0 = vwmStore.getState();
    const wa = s0.workArea;
    let offX = e.clientX - win.x;
    const offY = e.clientY - win.y;
    if (win.state === "max") {
      // Windows 习惯：拖动即还原，指针保持在标题栏内的相对横向位置
      const r = win.restore ?? {
        x: wa.x + 60,
        y: wa.y + 40,
        w: Math.min(1180, wa.w - 120),
        h: Math.min(760, wa.h - 80),
      };
      offX = Math.round(((e.clientX - wa.x) / Math.max(1, wa.w)) * r.w);
      unmaxVwmTo(win.id, { x: e.clientX - offX, y: e.clientY - offY, w: r.w, h: r.h });
    }
    const drag = { offX, offY };
    let pendingZone: SnapDir | null = null;
    let lastZone: SnapDir | null = null;
    // V-70 拖拽阈值防手滑：阈值内按住移动不算拖拽（不影响贴靠判定距离）
    const startXY = { x: e.clientX, y: e.clientY };
    let dragStarted = false;
    setDragging(true);
    // AI-01 M-01：拖拽路径采样（摇晃检测）
    const shakeSamples: Array<{ x: number; t: number }> = [{ x: e.clientX, t: Date.now() }];
    let shook = false;
    // AI-01 M-05：边缘摆渡节流（左右缘停留 700ms 触发一次）
    let ferryTimer: number | null = null;
    let ferryDir: "left" | "right" | null = null;

    const onMove = (ev: PointerEvent): void => {
      if (!dragStarted && !isDragStart(ev.clientX - startXY.x, ev.clientY - startXY.y, liveDragThreshold(), ev.pointerType)) {
        return;
      }
      dragStarted = true;
      const s = vwmStore.getState();
      const w = s.wins.find((x) => x.id === win.id);
      if (!w) return void cleanup();
      const nx = ev.clientX - drag.offX;
      const ny = Math.max(s.workArea.y - 8, ev.clientY - drag.offY);
      // 保守钳制：至少保留 120px 可见，避免窗口被完全拖出屏幕
      const cx = Math.min(
        Math.max(nx, s.workArea.x - w.w + 120),
        Math.max(s.workArea.x, s.workArea.x + s.workArea.w - 120),
      );
      let mx = cx;
      let my = ny;
      // AI-01 M-07：对齐参考线与轻吸附（Alt 按住临时禁用）
      if (feel?.winGuides && !ev.altKey) {
        const candidates = s.wins
          .filter((o) => o.id !== win.id && !o.minimized && !s.closing.includes(o.id))
          .map((o) => ({ x: o.x, y: o.y, w: o.w, h: o.h }));
        const g = computeGuide(candidates, { x: mx, y: my, w: w.w, h: w.h });
        if (g.snapX !== null) mx = g.snapX;
        if (g.snapY !== null) my = g.snapY;
        setVwmGuides(g.snapX !== null || g.snapY !== null ? { xs: g.guideXs, ys: g.guideYs } : null);
      } else {
        setVwmGuides(null);
      }
      moveVwmWin(win.id, mx, my);
      // AI-01 M-01：摇晃采样（≤96 个样本）
      shakeSamples.push({ x: ev.clientX, t: Date.now() });
      if (shakeSamples.length > 96) shakeSamples.shift();
      pendingZone = zoneFromPointer(ev.clientX, ev.clientY, s.workArea);
      if (pendingZone !== lastZone) {
        lastZone = pendingZone;
        setVwmSnapPreview(pendingZone ? snapZoneForVwm(pendingZone, s.workArea) : null);
      }
      // AI-01 M-05：拖到工作区左右缘停住 → 摆渡到相邻屏（仅多屏时 ferryVwmWin 生效）
      {
        const nearL = ev.clientX <= s.workArea.x + 6;
        const nearR = ev.clientX >= s.workArea.x + s.workArea.w - 6;
        const dir = nearL ? "left" : nearR ? "right" : null;
        if (dir && dir !== ferryDir && ferryTimer === null && !pendingZone) {
          ferryDir = dir;
          ferryTimer = window.setTimeout(() => {
            ferryTimer = null;
            if (ferryDir === "left" || ferryDir === "right") void ferryVwmWin(win.id, ferryDir);
          }, 700);
        } else if (!dir && ferryTimer !== null) {
          window.clearTimeout(ferryTimer);
          ferryTimer = null;
          ferryDir = null;
        }
      }
    };
    const cleanup = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    };
    const onUp = (ev?: PointerEvent): void => {
      if (ferryTimer !== null) {
        window.clearTimeout(ferryTimer);
        ferryTimer = null;
      }
      setVwmGuides(null);
      // AI-01 M-01：摇晃触发（默认关；只最小化其他窗口，可 Ctrl+Alt+D 恢复）
      if (!shook && feel?.winShake && dragStarted && detectShake(shakeSamples)) {
        shook = true;
        shakeMinimizeOthers(win.id);
        pushToast("info", t("wfShakeDone"), t("wfShakeRestoreHint"));
      }
      if (pendingZone) {
        playSnap();
        if (pendingZone === "up") toggleMaxVwmWin(win.id);
        else snapVwmRect(win.id, snapZoneForVwm(pendingZone, vwmStore.getState().workArea));
      } else if (ev && tabsEnabled()) {
        // 批次W-5 标签页化：拖到同应用另一窗口标题栏上松手 → 合并为标签组
        const el = document.elementFromPoint(ev.clientX, ev.clientY);
        const bar = el?.closest?.(".vwm-titlebar") as HTMLElement | null;
        const targetId = bar?.dataset?.winid;
        if (targetId && targetId !== win.id) {
          const s = vwmStore.getState();
          const target = s.wins.find((x) => x.id === targetId);
          if (target && target.app === win.app) groupVwmWins(win.id, targetId);
        }
      }
      settleVwmWin(win.id);
      setVwmSnapPreview(null);
      setDragging(false);
      cleanup();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  // ---------- 八向缩放 ----------
  const beginResize = (e: React.PointerEvent, dir: string): void => {
    if (e.button !== 0 || win.state === "max") return;
    e.stopPropagation();
    pointerFocusVwm(win.id);
    const start = { x: e.clientX, y: e.clientY, wx: win.x, wy: win.y, ww: win.w, wh: win.h };
    const onMove = (ev: PointerEvent): void => {
      const dx = ev.clientX - start.x;
      const dy = ev.clientY - start.y;
      let { wx, wy, ww, wh } = start;
      if (dir.includes("e")) ww = Math.max(MIN_W, start.ww + dx);
      if (dir.includes("s")) wh = Math.max(MIN_H, start.wh + dy);
      if (dir.includes("w")) {
        ww = Math.max(MIN_W, start.ww - dx);
        wx = start.wx + (start.ww - ww);
      }
      if (dir.includes("n")) {
        wh = Math.max(MIN_H, start.wh - dy);
        wy = start.wy + (start.wh - wh);
      }
      resizeVwmWin(win.id, { x: wx, y: wy, w: ww, h: wh });
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      settleVwmWin(win.id);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const maximized = win.state === "max";

  // ---------- AI-01 Z-41 鼠标手势最小集（默认关；右键拖 下=关窗 / 上=最小化） ----------
  const gestureFired = useRef(false);
  const onTitleGestureStart = (e: React.PointerEvent): void => {
    if (e.button !== 2 || !feel?.winGestures) return;
    if (win.state === "max" || win.rolledUp) return;
    const path: Array<{ x: number; y: number }> = [{ x: e.clientX, y: e.clientY }];
    const onMove = (ev: PointerEvent): void => {
      path.push({ x: ev.clientX, y: ev.clientY });
      if (path.length > 96) path.shift();
    };
    const onUp = (ev: PointerEvent): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      path.push({ x: ev.clientX, y: ev.clientY });
      // 24px 内判定失败 → 还原为正常右键菜单（无感回退）
      const d = detectGesture(path, 24);
      if (!d) return;
      gestureFired.current = true;
      window.setTimeout(() => { gestureFired.current = false; }, 350);
      if (d === "down") closeVwmWin(win.id);
      else if (d === "up") minimizeVwmWin(win.id);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  // ---------- AI-01 M-06 挂起 / 恢复（仅嵌入登记的第三方进程） ----------
  const toggleSuspend = (): void => {
    setMenu(null);
    const pid = embedStateStore.getState().meta[win.id]?.rootPid ?? 0;
    if (!pid) {
      pushToast("info", title, t("wfSuspendFail"));
      return;
    }
    if (suspendedNow) {
      void ipc
        .procResume(pid)
        .then(() => {
          setSuspended(win.id, false);
          pushToast("success", title, t("wfResumeDone"));
        })
        .catch(() => pushToast("error", title, t("wfSuspendFail")));
    } else {
      void ipc
        .procSuspend(pid)
        .then(() => {
          setSuspended(win.id, true);
          pushToast("info", title, t("wfSuspendDone"));
        })
        .catch(() => pushToast("error", title, t("wfSuspendFail")));
    }
  };

  // ---------- AI-01 M-04 无响应处置（环境绝不自动杀进程） ----------
  const hungAction = (): void => {
    void (async () => {
      const c = await askChoice({
        title: t("wfHungTitle"),
        body: t("wfHungBody"),
        options: [
          { value: "wait", label: t("wfWait") },
          { value: "kill", label: t("wfKill") },
        ],
      });
      if (c !== "kill") return;
      const ok = await askConfirm({ title: t("wfKillConfirmTitle"), body: t("wfKillConfirmBody"), danger: true });
      if (!ok) return;
      const pid = embedStateStore.getState().meta[win.id]?.rootPid ?? 0;
      if (!pid) return;
      try {
        await ipc.procKill(pid, true);
      } catch {
        pushToast("error", title, t("wfSuspendFail"));
      }
    })();
  };

  // ---------- AI-01 Z-40 布局快照（保存 / 应用） ----------
  const saveLayoutUi = (): void => {
    setMenu(null);
    void askPrompt({ title: t("wfSnapSave"), initial: "" }).then((name) => {
      if (!name || !name.trim()) return;
      saveLayoutSnapshot(name.trim());
      pushToast("success", t("wfSnapSaved"), name.trim());
    });
  };
  const applyLayoutUi = (): void => {
    setMenu(null);
    const list = listLayoutSnapshots();
    if (list.length === 0) {
      pushToast("info", t("wfSnapApply"), t("wfSnapNone"));
      return;
    }
    void (async () => {
      const name = await askPrompt({ title: t("wfSnapApply"), initial: list[0]?.name ?? "" });
      if (!name || !name.trim()) return;
      if (applyLayoutSnapshot(name.trim())) pushToast("success", t("wfSnapApplied"), name.trim());
      else pushToast("info", t("wfSnapApply"), t("wfSnapNone"));
    })();
  };

  // ---------- AI-01 Z-36 右键菜单动作 ----------
  const onFeelMenuItem = (id: string): void => {
    if (id === "roll") rollVwmWin(win.id, true);
    else if (id === "unroll") rollVwmWin(win.id, false);
    else if (id === "topmost") setVwmTopmost(win.id, true);
    else if (id === "untopmost") setVwmTopmost(win.id, false);
    else if (id === "suspend" || id === "resume") toggleSuspend();
    else if (id === "saveLayout") saveLayoutUi();
    else if (id === "applyLayout") applyLayoutUi();
    if (id !== "suspend" && id !== "resume" && id !== "saveLayout" && id !== "applyLayout") setMenu(null);
  };

  // Z-36：开启透明度微控时，新窗口应用该应用的记忆透明度（与 1 一致不记忆）
  useEffect(() => {
    if (!feel?.winFeelOpacity) return;
    const mem = appOpacityOf(win.app);
    if (mem !== null && win.opacity === 1) setVwmOpacity(win.id, mem);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      className={`vwm-window${props.focused ? " focused" : ""}${win.minimized && !props.flying ? " minimized" : ""}${maximized ? " maximized" : ""}${dragging ? " dragging" : ""}${snapping ? " snapping" : ""}${props.closing ? " closing" : ""}${props.flying ? " flying" : ""}`}
      style={{ left: win.x, top: win.y, width: win.w, height: win.h, zIndex: props.zIndex, opacity: win.opacity < 1 ? win.opacity : undefined }}
      onPointerDown={() => {
        pointerFocusVwm(win.id);
        if (isTpApp(win.app)) void ipc.embedFocus(win.id).catch(() => {});
      }}
      role="dialog"
      aria-label={title}
      data-winid={win.id}
    >
      <div
        className="vwm-titlebar"
        data-winid={win.id}
        onPointerDown={(e) => {
          onTitlePointerDown(e);
          onTitleGestureStart(e);
        }}
        onDoubleClick={() => { playSnap(); toggleMaxVwmWin(win.id); }}
        onContextMenu={(e) => {
          e.preventDefault();
          // AI-01 Z-41：手势已触发 → 本次抑制菜单
          if (gestureFired.current) return;
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <span className="vwm-app-dot" aria-hidden style={{ background: appAccent(win.app) }} />
        <span className="vwm-title">{title}</span>
        {/* AI-01 M-04：未响应琥珀色徽标（环境不替应用做决定，仅提供选项） */}
        {props.hung && (
          <button type="button" className="vwm-hung-badge" title={t("wfHungHint")} onClick={hungAction}>
            {t("wfHungBadge")}
          </button>
        )}
        {suspendedNow && (
          <span className="vwm-suspended-badge" title={t("wfSuspendedHint")}>
            {t("wfSuspendedBadge")}
          </span>
        )}
        <span className="vwm-titlebar-space" />
        {/* 右上角 Mac 风格红绿灯（需求指定顺序：左绿 中黄 右红）：
            🟢 退出（关闭窗口）/ 🟡 全屏（最大化-还原）/ 🔴 最小化 */}
        <div className="vwm-lights" role="group" aria-label="Window controls">
          <button
            type="button"
            className="win-btn"
            aria-label={t("close")}
            title={t("close")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => {
              if (isTpApp(win.app)) void ipc.embedClose(win.id).catch(() => {});
              // 批次W-5：组内红绿灯只关当前标签（其它成员保活），未分组原样关窗
              closeVwmTab(win.id);
            }}
          >
            <span className="win-dot green" />
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label={maximized ? t("restore") : t("maximize")}
            title={maximized ? t("restore") : t("maximize")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => { playSnap(); toggleMaxVwmWin(win.id); }}
          >
            <span className="win-dot yellow" />
          </button>
          <button
            type="button"
            className="win-btn"
            aria-label={t("minimize")}
            title={t("minimize")}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => minimizeVwmWin(win.id)}
          >
            <span className="win-dot red" />
          </button>
        </div>
      </div>

      {/* 批次W-5 标签组：标题栏下沿 TabStrip（仅组内窗口渲染）。
          点击切换显示；按住拖出 24px = 拆分（脱离标签组）。 */}
      {win.group && (
        <div className="vwm-tabstrip" role="tablist">
          {groupMembersOf(vwmStore.getState().wins, win.group).map((m) => (
            <button
              key={m.id}
              type="button"
              role="tab"
              aria-selected={m.id === win.id}
              className={`vwm-tab${m.id === win.id ? " on" : ""}`}
              onClick={() => activateVwmTab(m.id)}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                const sx = e.clientX;
                const sy = e.clientY;
                let split = false;
                const onMove = (ev: PointerEvent): void => {
                  if (split) return;
                  if (Math.hypot(ev.clientX - sx, ev.clientY - sy) > 24) {
                    split = true;
                    ungroupVwmWin(m.id);
                  }
                };
                const onUp = (): void => {
                  window.removeEventListener("pointermove", onMove);
                  window.removeEventListener("pointerup", onUp);
                  window.removeEventListener("pointercancel", onUp);
                };
                window.addEventListener("pointermove", onMove);
                window.addEventListener("pointerup", onUp);
                window.addEventListener("pointercancel", onUp);
              }}
            >
              <span className="vwm-tab-dot" aria-hidden style={{ background: appAccent(m.app) }} />
              {vwmWindowTitle(m.app)}
            </button>
          ))}
        </div>
      )}

      <div className="vwm-content">{props.children}</div>

      {/* AI-01 Z-36 标题栏右键菜单：卷帘 / 置顶 / 挂起 / 透明度 / 布局快照 */}
      {menu && (
        <div
          ref={menuRef}
          className="vwm-sysmenu"
          role="menu"
          style={{ left: menu.x, top: menu.y }}
          onContextMenu={(e) => e.preventDefault()}
        >
          {winFeelMenuItems(win, suspendedNow).map((it) => (
            <button
              key={it.id}
              type="button"
              role="menuitem"
              className="vwm-sysmenu-item"
              disabled={it.disabled}
              title={it.hintKey ? t(it.hintKey) : undefined}
              onClick={() => onFeelMenuItem(it.id)}
            >
              {t(it.labelKey)}
            </button>
          ))}
          {feel?.winFeelOpacity && (
            <label className="vwm-sysmenu-item vwm-feel-opacity">
              <span className="vwm-sysmenu-cap">{t("wfOpacity")}</span>
              <input
                type="range"
                min={0.2}
                max={1}
                step={0.05}
                value={win.opacity}
                onChange={(e) => setVwmOpacity(win.id, Number(e.target.value))}
                onPointerDown={(e) => e.stopPropagation()}
                onMouseUp={() => rememberAppOpacity(win.app, win.opacity)}
                onTouchEnd={() => rememberAppOpacity(win.app, win.opacity)}
              />
              <span>{Math.round(win.opacity * 100)}%</span>
            </label>
          )}
          <button type="button" role="menuitem" className="vwm-sysmenu-item" onClick={saveLayoutUi}>
            {t("wfMenuSaveLayout")}
          </button>
          <button type="button" role="menuitem" className="vwm-sysmenu-item" onClick={applyLayoutUi}>
            {t("wfMenuApplyLayout")}
          </button>
        </div>
      )}

      {!maximized && (
        <>
          {(["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const).map((dir) => (
            <div key={dir} className={`vwm-rz ${dir}`} data-dir={dir} onPointerDown={(e) => beginResize(e, dir)} />
          ))}
        </>
      )}
    </div>
  );
}
