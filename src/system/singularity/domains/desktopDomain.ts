/**
 * SINGULARITY-100 · 域3 桌面与图标表达（Q-16…Q-22）行为层。
 *
 * 边界（全景 §3）：存量覆盖排列/密度/锁定/模板；本域补单图标交互物理、
 * 堆叠视图、框选反馈、贴地感、缩放舞台与网格微调。全部为只读 DOM 观察 +
 * 视觉层（CSS 类/内联视觉变换），框选与点击逻辑零改变。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  clamp,
  domReady,
  makeEl,
  on,
  singuLayer,
} from "../shared";
import { singuBool, singuMotionOK } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-16：悬停倾斜角（朝向鼠标来向，±3°）。 */
export function hoverTilt(_iconCx: number, mouseFromLeft: boolean): number {
  return mouseFromLeft ? 3 : -3;
}

/** Q-20：缩放钳制与回正判定。 */
export function zoomStageClamp(next: number): number {
  return clamp(next, 0.6, 1.6);
}

/** Q-21：波次延迟（按距圆心距离，每 30ms 一波）。 */
export function rippleDelay(dx: number, dy: number, perWaveMs = 30): number {
  return Math.round(Math.hypot(dx, dy) / 120) * perWaveMs;
}

/** Q-39 同族：图标分类（堆叠分组用）。 */
export function iconCategory(name: string): string {
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "none";
  if (["exe", "lnk", "app"].includes(ext)) return "app";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"].includes(ext)) return "image";
  if (["mp4", "mkv", "avi", "mov", "webm"].includes(ext)) return "video";
  if (["mp3", "flac", "wav", "ogg"].includes(ext)) return "audio";
  if (["zip", "rar", "7z", "tar", "gz"].includes(ext)) return "archive";
  if (["doc", "docx", "pdf", "txt", "md", "xls", "xlsx", "ppt", "pptx"].includes(ext)) return "doc";
  if (["js", "ts", "tsx", "py", "rs", "c", "cpp", "go", "json"].includes(ext)) return "code";
  return "other";
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

function iconRoot(): HTMLElement | null {
  return document.querySelector<HTMLElement>(".desktop-icons, #desktop-icons, [data-desktop-icons]");
}

// ---- Q-16 图标情绪微动效 + Q-19 地面接触 ----
function mountIconMood(): void {
  bag.add(
    on(window, "pointerover", (e) => {
      if (!ctxRef?.on("Q-16")) return;
      const pe = e as PointerEvent;
      const icon = (pe.target as HTMLElement).closest<HTMLElement>(".desktop-icon");
      if (!icon) return;
      if (!ctxRef.on("Q-19")) return;
      const r = icon.getBoundingClientRect();
      icon.style.setProperty("--singu-tilt", `${hoverTilt(r.left + r.width / 2, pe.clientX < r.left + r.width / 2).toFixed(1)}deg`);
    }),
  );
}

// ---- Q-18 框选橡皮筋（增强既有 .marquee 视觉 + 框角计数）----
function mountLasso(): void {
  let badge: HTMLElement | null = null;
  bag.add(
    on(window, "pointerup", () => {
      badge?.remove();
      badge = null;
    }),
  );
  // marquee 元素由 DesktopIcons 渲染；我们只贴角标 + 张力类
  const mo = new MutationObserver(() => {
    if (!ctxRef?.on("Q-18")) return;
    const marquee = document.querySelector<HTMLElement>(".marquee");
    if (!marquee) {
      badge?.remove();
      badge = null;
      return;
    }
    marquee.classList.add("singu-lasso");
    const count = document.querySelectorAll(".desktop-icon.selected").length;
    if (!badge) {
      badge = makeEl("div", "singu-lasso-count");
      singuLayer().appendChild(badge);
    }
    const mr = marquee.getBoundingClientRect();
    badge.style.left = `${mr.right + 6}px`;
    badge.style.top = `${mr.top - 8}px`;
    badge.textContent = String(count);
  });
  mo.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class"] });
  bag.add(() => {
    mo.disconnect();
    badge?.remove();
  });
}

// ---- Q-20 桌面缩放舞台（Ctrl+滚轮缩放图标层，1.5s 自动回正）----
function mountZoomStage(): void {
  let scale = 1;
  let returnTimer: ReturnType<typeof setTimeout> | null = null;
  const apply = (): void => {
    const root = iconRoot();
    if (!root) return;
    root.style.transformOrigin = "center top";
    root.style.transition = "transform .25s cubic-bezier(.2,.8,.2,1)";
    root.style.transform = `scale(${scale.toFixed(3)})`;
  };
  const reset = (): void => {
    scale = 1;
    apply();
  };
  bag.add(
    on(window, "wheel", (e) => {
      const we = e as WheelEvent;
      if (!we.ctrlKey || !ctxRef?.on("Q-20")) return;
      const root = iconRoot();
      if (!root) return;
      // 仅桌面空白（目标不在图标/面板上）
      if ((we.target as HTMLElement).closest(".desktop-icon, .vwm-window, .taskbar, .start-menu")) return;
      we.preventDefault();
      scale = zoomStageClamp(scale - Math.sign(we.deltaY) * 0.08);
      apply();
      if (returnTimer) clearTimeout(returnTimer);
      if (singuBool("Q-20", "autoReturn")) {
        returnTimer = setTimeout(reset, 1500);
      }
    }, { passive: false }),
  );
  bag.add(() => {
    if (returnTimer) clearTimeout(returnTimer);
    const root = iconRoot();
    if (root) {
      root.style.transform = "";
      root.style.transition = "";
    }
  });
}

// ---- Q-21 涟漪重排（观察图标位移 → 波次交错）----
function mountRippleRearrange(): void {
  const pending = new WeakMap<HTMLElement, number>();
  let lastPositions = new Map<HTMLElement, { left: string; top: string }>();
  const collect = (): void => {
    lastPositions = new Map();
    for (const icon of Array.from(document.querySelectorAll<HTMLElement>(".desktop-icon"))) {
      lastPositions.set(icon, { left: icon.style.left, top: icon.style.top });
    }
  };
  collect();
  let waveTimer: ReturnType<typeof setTimeout> | null = null;
  const mo = new MutationObserver(() => {
    if (waveTimer) return;
    waveTimer = setTimeout(() => {
      waveTimer = null;
      if (!ctxRef?.on("Q-21") || !singuMotionOK()) {
        collect();
        return;
      }
      const cx = window.innerWidth / 2;
      const cy = window.innerHeight / 2;
      let moved = false;
      for (const icon of Array.from(document.querySelectorAll<HTMLElement>(".desktop-icon"))) {
        const prev = lastPositions.get(icon);
        if (!prev || prev.left === icon.style.left && prev.top === icon.style.top) continue;
        moved = true;
        const r = icon.getBoundingClientRect();
        icon.style.transitionDelay = `${rippleDelay(r.left - cx, r.top - cy)}ms`;
        pending.set(icon, Date.now());
      }
      collect();
      if (moved) {
        setTimeout(() => {
          for (const icon of Array.from(document.querySelectorAll<HTMLElement>(".desktop-icon"))) {
            icon.style.transitionDelay = "";
          }
        }, 1400);
      }
    }, 60);
  });
  mo.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["style"] });
  bag.add(() => {
    mo.disconnect();
    if (waveTimer) clearTimeout(waveTimer);
    void pending;
  });
}

// ---- Q-22 网格微调器（hub 触发；调整 icon 层缩放间距 + 网格线预览）----
function mountGridTuner(): void {
  bag.add(
    on(window, "singu:grid-tuner", (e: Event) => {
      if (!ctxRef?.on("Q-22")) return;
      const detail = (e as CustomEvent).detail as { show: boolean } | undefined;
      let panel = document.querySelector<HTMLElement>("#singu-grid-tuner");
      if (!detail?.show) {
        panel?.remove();
        return;
      }
      if (panel) return;
      panel = makeEl("div", "singu-panel singu-grid-tuner");
      panel.id = "singu-grid-tuner";
      const saved = { gx: Number(localStorage.getItem("variable.singu.grid.gx") ?? 0), gy: Number(localStorage.getItem("variable.singu.grid.gy") ?? 0) };
      panel.innerHTML = `
        <div class="singu-panel-title">GRID TUNER</div>
        <label>横向间距 <input type="range" min="-24" max="48" step="8" value="${saved.gx}" data-k="gx"><span class="v"></span></label>
        <label>纵向间距 <input type="range" min="-24" max="48" step="8" value="${saved.gy}" data-k="gy"><span class="v"></span></label>
        <button class="singu-btn" data-act="grid-lines">网格线预览</button>
        <button class="singu-btn" data-act="reset">重置</button>`;
      singuLayer().appendChild(panel);
      const applyGap = (gx: number, gy: number): void => {
        const root = iconRoot();
        if (root) {
          root.style.setProperty("--singu-grid-gx", `${gx}px`);
          root.style.setProperty("--singu-grid-gy", `${gy}px`);
        }
      };
      panel.querySelectorAll<HTMLInputElement>("input[type=range]").forEach((inp) => {
        inp.addEventListener("input", () => {
          const gx = Number((panel!.querySelector('[data-k="gx"]') as HTMLInputElement).value);
          const gy = Number((panel!.querySelector('[data-k="gy"]') as HTMLInputElement).value);
          localStorage.setItem("variable.singu.grid.gx", String(gx));
          localStorage.setItem("variable.singu.grid.gy", String(gy));
          applyGap(gx, gy);
        });
      });
      (panel.querySelector('[data-act="grid-lines"]') as HTMLElement).addEventListener("click", () => {
        document.documentElement.classList.toggle("singu-grid-preview");
      });
      (panel.querySelector('[data-act="reset"]') as HTMLElement).addEventListener("click", () => {
        localStorage.removeItem("variable.singu.grid.gx");
        localStorage.removeItem("variable.singu.grid.gy");
        applyGap(0, 0);
        panel!.remove();
      });
    }),
  );
}

// ---- Q-17 桌面快速堆叠（只读分组视图 overlay；hub 触发）----
function mountDeskStacks(): void {
  bag.add(
    on(window, "singu:stacks", () => {
      if (!ctxRef?.on("Q-17")) return;
      let host = document.querySelector<HTMLElement>("#singu-stacks");
      if (host) {
        host.remove();
        return;
      }
      const groups = new Map<string, HTMLElement[]>();
      for (const icon of Array.from(document.querySelectorAll<HTMLElement>(".desktop-icon"))) {
        const name = icon.querySelector(".desktop-icon-label, .icon-label, .label")?.textContent ?? icon.textContent ?? "";
        const cat = iconCategory(name.trim().split("\n")[0] ?? "");
        const list = groups.get(cat) ?? [];
        list.push(icon);
        groups.set(cat, list);
      }
      host = makeEl("div", "singu-panel singu-stacks");
      host.id = "singu-stacks";
      const title = makeEl("div", "singu-panel-title");
      title.textContent = "DESK STACKS · 按类型";
      host.appendChild(title);
      for (const [cat, icons] of groups) {
        if (icons.length < 2) continue; // 单件不成堆
        const stack = makeEl("button", "singu-stack");
        stack.innerHTML = `<span class="singu-stack-count">${icons.length}</span><span class="singu-stack-cat">${cat}</span>`;
        stack.addEventListener("pointerenter", () => {
          stack.innerHTML = icons
            .slice(0, 5)
            .map((ic) => `<span class="singu-stack-item">${(ic.textContent ?? "").trim().slice(0, 18)}</span>`)
            .join("");
        });
        stack.addEventListener("pointerleave", () => {
          stack.innerHTML = `<span class="singu-stack-count">${icons.length}</span><span class="singu-stack-cat">${cat}</span>`;
        });
        stack.addEventListener("click", () => {
          // 定位：滚动/高亮首件原位图标（堆叠不移动原位，取消即还原 —— 视图零残留）
          const first = icons[0];
          if (!first) return;
          first.scrollIntoView({ block: "center", behavior: singuMotionOK() ? "smooth" : "auto" });
          first.classList.add("singu-flash");
          setTimeout(() => first.classList.remove("singu-flash"), 900);
        });
        host.appendChild(stack);
      }
      if (host.children.length === 1) {
        const empty = makeEl("div", "singu-empty");
        empty.textContent = "桌面元素不足 2 件同类 · 无可堆叠";
        host.appendChild(empty);
      }
      singuLayer().appendChild(host);
      setTimeout(() => host?.remove(), 20_000); // 20s 自动退场
    }),
  );
}

export function desktopDomain(): DomainController {
  return {
    domain: "desktop",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountIconMood();
        mountLasso();
        mountZoomStage();
        mountRippleRearrange();
        mountGridTuner();
        mountDeskStacks();
        // Q-19 贴地感与 Q-16 弹簧回落为纯 CSS 层（singularity.css .singu-grounding）
        const root = iconRoot();
        if (root) root.classList.add("singu-grounding");
      });
    },
    unmount() {
      bag.run();
      iconRoot()?.classList.remove("singu-grounding");
      document.querySelectorAll("#singu-stacks,#singu-grid-tuner,.singu-lasso-count").forEach((e) => e.remove());
    },
  };
}
