/**
 * SINGULARITY-100 · 域4 任务栏与开始菜单（Q-23…Q-29）行为层。
 *
 * 边界（全景 §4）：存量覆盖跳转列表/托盘/时钟/便签；本域补资源温度、
 * 键击流直达、隐形模式、时钟微卡、律动条、布局编辑与分页坞。
 * 全部为任务栏 DOM 之上的只读/视觉层，不改 Taskbar/StartMenu 内部。
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
import { singuMotionOK } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-23：CPU 占用 → 温度档（0 绿 / 1 黄 / 2 红）。 */
export function thermoLevel(cpu: number): 0 | 1 | 2 {
  return cpu > 75 ? 2 : cpu >= 40 ? 1 : 0;
}

/** Q-24：拼音首字母键击流命中（子序列匹配）。 */
export function flowMatch(keystrokes: string, target: string, pinyinInitials: string): boolean {
  if (!keystrokes) return false;
  const low = target.toLowerCase();
  if (low.includes(keystrokes)) return true;
  if (pinyinInitials && pinyinInitials.toLowerCase().startsWith(keystrokes.toLowerCase())) return true;
  return false;
}

/** Q-53：温度 → 色相（40–95℃ 蓝绿黄红）。 */
export function thermalHue(tempC: number): number {
  const t = clamp((tempC - 40) / 55, 0, 1);
  return 240 - t * 240; // 240(蓝) → 0(红)
}

/** Q-56：速率 → 可见度 0..1。 */
export function vaneIntensity(bps: number): number {
  return clamp(Math.log10(1 + bps / 1024) / 5, 0.05, 1);
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-23 任务栏温度计（运行图标下 2px 色条）----
function mountThermometer(): void {
  let timer: ReturnType<typeof setInterval> | null = null;
  const tick = (): void => {
    if (!ctxRef?.on("Q-23")) {
      document.querySelectorAll(".singu-thermo").forEach((e) => e.remove());
      return;
    }
    const pulse = ctxRef.pulse();
    if (!pulse) return; // 无采样：如实隐藏而非假绿
    const level = thermoLevel(pulse.cpu_usage);
    const running = document.querySelectorAll<HTMLElement>(".tb-btn.running");
    running.forEach((btn) => {
      let bar = btn.querySelector<HTMLElement>(".singu-thermo");
      if (!bar) {
        bar = makeEl("span", "singu-thermo");
        btn.appendChild(bar);
      }
      bar.dataset.level = String(level);
      bar.title = `CPU ${pulse.cpu_usage.toFixed(0)}%`;
    });
  };
  timer = setInterval(tick, 2000);
  bag.add(() => {
    if (timer) clearInterval(timer);
    document.querySelectorAll(".singu-thermo").forEach((e) => e.remove());
  });
}

// ---- Q-24 开始菜单键盘流（拼音首字母键击流直达）----
function mountMenuFlow(): void {
  let buffer = "";
  let resetTimer: ReturnType<typeof setTimeout> | null = null;
  const PINYIN: Record<string, string> = {
    文件: "wj", 文件管理器: "wjglq", 记录: "jl", 计算器: "jsq", 日历: "rl",
    回收站: "hsz", 任务管理: "rwgl", 设置: "sz", 网络: "wl", 便签: "bq",
    终端: "zd", 思维导图: "swdt", 代码: "dm", 快照: "kz", 剪贴板: "jtb",
  };
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ctxRef?.on("Q-24") || ke.ctrlKey || ke.altKey || ke.metaKey) return;
      const menu = document.querySelector<HTMLElement>(".start-menu");
      if (!menu) {
        buffer = "";
        return;
      }
      if ((ke.target as HTMLElement).matches("input, textarea")) return;
      if (ke.key === "Escape") {
        buffer = "";
        return;
      }
      if (ke.key.length !== 1 || !/[a-zA-Z]/.test(ke.key)) return;
      buffer += ke.key.toLowerCase();
      if (resetTimer) clearTimeout(resetTimer);
      resetTimer = setTimeout(() => (buffer = ""), 1200);
      const apps = Array.from(menu.querySelectorAll<HTMLElement>(".start-app"));
      let hitIdx = -1;
      apps.forEach((app, i) => {
        const name = app.querySelector(".start-app-name")?.textContent ?? "";
        const initials = PINYIN[name] ?? "";
        app.classList.remove("singu-flow-hit");
        if (hitIdx < 0 && flowMatch(buffer, name, initials)) hitIdx = i;
      });
      if (hitIdx >= 0) {
        const hit = apps[hitIdx]!;
        hit.classList.add("singu-flow-hit");
        if (ke.key === "Enter" || buffer.length >= 3) {
          // 三键内命中即高亮；Enter 启动
          if (ke.key === "Enter") (hit as HTMLElement).click();
        }
      }
    }),
  );
  bag.add(
    on(window, "keydown", (e) => {
      if ((e as KeyboardEvent).key === "Enter") {
        const hit = document.querySelector<HTMLElement>(".start-app.singu-flow-hit");
        if (hit) hit.click();
      }
    }),
  );
}

// ---- Q-25 任务栏隐形模式（全屏自动 / hub 手动锁定）----
function mountGhostBar(): void {
  let manual = false;
  bag.add(
    on(window, "singu:ghost-bar", (e: Event) => {
      manual = Boolean((e as CustomEvent).detail?.on);
    }),
  );
  const apply = (fullscreen: boolean): void => {
    if (!ctxRef?.on("Q-25")) return;
    const bar = document.querySelector<HTMLElement>(".taskbar");
    if (!bar) return;
    const ghost = manual || fullscreen;
    bar.classList.toggle("singu-ghost", ghost);
  };
  void import("@tauri-apps/api/event")
    .then(({ listen }) => {
      const un = listen<boolean>("sys://fullscreen", (e) => apply(Boolean(e.payload)));
      bag.add(() => {
        void un.then((f) => f());
      });
    })
    .catch(() => {
      /* 非 Tauri：全屏信号不可听，手动模式仍可用 */
    });
  bag.add(() => document.querySelector(".taskbar")?.classList.remove("singu-ghost"));
}

// ---- Q-26 时钟微卡片（悬停任务栏时钟三联微卡）----
function mountClockCards(): void {
  let host: HTMLElement | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  const hide = (): void => {
    host?.remove();
    host = null;
  };
  bag.add(
    on(window, "pointerover", (e) => {
      if (!ctxRef?.on("Q-26")) return;
      const clock = (e.target as HTMLElement).closest<HTMLElement>(".tb-clock, .taskbar-clock, [data-tb-clock]");
      if (!clock) return;
      if (hideTimer) clearTimeout(hideTimer);
      if (host) return;
      host = makeEl("div", "singu-cards");
      const now = new Date();
      // 月历
      const y = now.getFullYear();
      const m = now.getMonth();
      const firstDow = new Date(y, m, 1).getDay();
      const days = new Date(y, m + 1, 0).getDate();
      let cal = "";
      for (let i = 0; i < firstDow; i++) cal += "<i></i>";
      for (let d = 1; d <= days; d++) {
        cal += d === now.getDate() ? `<b>${d}</b>` : `<i>${d}</i>`;
      }
      const todos = singuTodos();
      host.innerHTML = `
        <div class="singu-card singu-cal"><div class="singu-card-h">${y}·${m + 1}</div><div class="singu-cal-grid">${cal}</div></div>
        <div class="singu-card singu-weather"><div class="singu-card-h">WEATHER</div><div class="singu-card-b">本地数据源未接入 · 如实留白</div></div>
        <div class="singu-card singu-todo"><div class="singu-card-h">TODAY</div><div class="singu-card-b">${todos}</div></div>`;
      const r = clock.getBoundingClientRect();
      host.style.left = `${clamp(r.left - 120, 8, window.innerWidth - 380)}px`;
      host.style.bottom = `${window.innerHeight - r.top + 10}px`;
      singuLayer().appendChild(host);
      host.addEventListener("pointerleave", hide);
    }),
  );
  bag.add(
    on(window, "pointerout", (e) => {
      const clock = (e.target as HTMLElement).closest(".tb-clock, .taskbar-clock, [data-tb-clock]");
      if (clock && host) {
        if (hideTimer) clearTimeout(hideTimer);
        hideTimer = setTimeout(hide, 200);
      }
    }),
  );
  bag.add(hide);
}

function singuTodos(): string {
  try {
    const raw = localStorage.getItem("variable.singu.stash");
    if (!raw) return "无待办数据";
    const items = JSON.parse(raw) as Array<{ kind: string; text: string }>;
    const first = items.find((i) => i.kind === "text");
    return first ? escapeHtml(first.text.slice(0, 40)) : "无待办数据";
  } catch {
    return "无待办数据";
  }
}

// ---- Q-27 任务栏律动条（环境内媒体元素频谱）----
function mountPulseBar(): void {
  let host: HTMLElement | null = null;
  let raf = 0;
  let analyser: AnalyserNode | null = null;
  let data: Uint8Array | null = null;
  const stop = (): void => {
    cancelAnimationFrame(raf);
    host?.remove();
    host = null;
    analyser = null;
  };
  const tickLoop = (): void => {
    raf = requestAnimationFrame(tickLoop);
    if (!ctxRef?.on("Q-27")) return;
    // 懒绑定：查找环境内正在播放的媒体元素
    const media = document.querySelector<HTMLMediaElement>("audio, video");
    if (!media || media.paused) {
      // 第十一轮大检查：原写法 querySelector("audio:not([paused]), video:not([paused])")
      // 永远命中自身——媒体元素没有 paused 内容属性（那是 JS 属性），
      // 导致 idle 呼吸态从未进入过。改为按属性真值判定。
      const anyPlaying = Array.from(
        document.querySelectorAll<HTMLMediaElement>("audio, video"),
      ).some((m) => !m.paused);
      if (host && !anyPlaying) {
        // 无播放：呼吸慢闪态
        host.classList.add("idle");
        host.classList.toggle("breath", singuMotionOK());
      }
      return;
    }
    if (!analyser) {
      try {
        const AC = window.AudioContext;
        if (!AC) return;
        const ctxA = new AC();
        const src = ctxA.createMediaElementSource(media);
        analyser = ctxA.createAnalyser();
        analyser.fftSize = 32;
        src.connect(analyser).connect(ctxA.destination);
        data = new Uint8Array(analyser.frequencyBinCount);
      } catch {
        return; // 已绑定过（重复 createMediaElementSource 会抛）——如实跳过
      }
    }
    if (!host) {
      host = makeEl("div", "singu-pulsebar");
      // 第十一轮大检查：子元素必须是 <i>——全部 CSS 规则（.singu-pulsebar i）
      // 按标签选择器编写，此前生成的 <span> 与样式完全不匹配，律动条不可见。
      host.innerHTML = "<i></i><i></i><i></i><i></i>";
      singuLayer().appendChild(host);
    }
    host.classList.remove("idle");
    if (analyser && data) {
      analyser.getByteFrequencyData(data as unknown as Uint8Array<ArrayBuffer>);
      const bars = host.children;
      for (let i = 0; i < 4 && i < bars.length; i++) {
        (bars[i] as HTMLElement).style.height = `${clamp((data[i * 2] ?? 0) / 255, 0.08, 1) * 16}px`;
      }
    }
  };
  raf = requestAnimationFrame(tickLoop);
  bag.add(stop);
}

// ---- Q-28 开始菜单布局编辑（CSS order/flex 比例，持久化）----
function mountMenuLayout(): void {
  bag.add(
    on(window, "singu:menu-layout", (e: Event) => {
      if (!ctxRef?.on("Q-28")) return;
      const show = Boolean((e as CustomEvent).detail?.show);
      let panel = document.querySelector<HTMLElement>("#singu-menu-layout");
      if (!show) {
        panel?.remove();
        return;
      }
      if (panel) return;
      panel = makeEl("div", "singu-panel singu-menu-layout");
      panel.id = "singu-menu-layout";
      const saved = singuLayoutPref();
      panel.innerHTML = `
        <div class="singu-panel-title">MENU LAYOUT</div>
        <label>固定区占比 <input type="range" min="30" max="70" value="${saved.ratio}" data-k="ratio"> <span class="v">${saved.ratio}%</span></label>
        <button class="singu-btn" data-act="swap">固定区 ↔ 最近区 互换</button>
        <button class="singu-btn" data-act="reset">重置默认</button>`;
      singuLayer().appendChild(panel);
      const apply = (p: { ratio: number; swap: boolean }): void => {
        document.documentElement.style.setProperty("--singu-menu-fixed", `${p.ratio}%`);
        document.documentElement.classList.toggle("singu-menu-swap", p.swap);
        localStorage.setItem("variable.singu.menulayout", JSON.stringify(p));
      };
      const cur = { ...saved };
      apply(cur);
      const slider = panel.querySelector<HTMLInputElement>("input[type=range]");
      slider?.addEventListener("input", () => {
        cur.ratio = Number(slider.value);
        (panel!.querySelector(".v") as HTMLElement).textContent = `${cur.ratio}%`;
        apply(cur);
      });
      (panel.querySelector('[data-act="swap"]') as HTMLElement).addEventListener("click", () => {
        cur.swap = !cur.swap;
        apply(cur);
      });
      (panel.querySelector('[data-act="reset"]') as HTMLElement).addEventListener("click", () => {
        cur.ratio = 50;
        cur.swap = false;
        if (slider) slider.value = "50";
        apply(cur);
      });
    }),
  );
}

function singuLayoutPref(): { ratio: number; swap: boolean } {
  try {
    const raw = localStorage.getItem("variable.singu.menulayout");
    if (raw) return JSON.parse(raw) as { ratio: number; swap: boolean };
  } catch {
    /* 损坏回默认 */
  }
  return { ratio: 50, swap: false };
}

// ---- Q-29 分页坞（钉选容器变换分页 + 页点）----
function mountBarPages(): void {
  let host: HTMLElement | null = null;
  let page = 0;
  const PAGES = 3;
  const apply = (): void => {
    const list = document.querySelector<HTMLElement>(".taskbar-center");
    if (!list) return;
    list.style.transition = "transform .15s ease";
    list.style.transform = `translateY(${-page * 100}%)`;
    if (host) {
      host.querySelectorAll(".singu-page-dot").forEach((d, i) => d.classList.toggle("on", i === page));
    }
  };
  const ensure = (): void => {
    if (host || !ctxRef?.on("Q-29")) return;
    const bar = document.querySelector<HTMLElement>(".taskbar");
    if (!bar) return;
    host = makeEl("div", "singu-pages");
    for (let i = 0; i < PAGES; i++) {
      const dot = makeEl("span", "singu-page-dot");
      dot.addEventListener("click", () => {
        page = i;
        apply();
      });
      host.appendChild(dot);
    }
    bar.appendChild(host);
  };
  const mo = new MutationObserver(() => ensure());
  mo.observe(document.body, { childList: true, subtree: true });
  let wheelLatch = 0;
  bag.add(
    on(window, "wheel", (e) => {
      const we = e as WheelEvent;
      if (!we.ctrlKey || !ctxRef?.on("Q-29")) return;
      const bar = (we.target as HTMLElement).closest(".taskbar");
      if (!bar) return;
      const now = Date.now();
      if (now - wheelLatch < 180) return;
      wheelLatch = now;
      page = (page + (we.deltaY > 0 ? 1 : -1) + PAGES) % PAGES;
      apply();
    }),
  );
  bag.add(() => {
    mo.disconnect();
    host?.remove();
    const list = document.querySelector<HTMLElement>(".taskbar-center");
    if (list) list.style.transform = "";
  });
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function taskbarDomain(): DomainController {
  return {
    domain: "taskbar",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountThermometer();
        mountMenuFlow();
        mountGhostBar();
        mountClockCards();
        mountPulseBar();
        mountMenuLayout();
        mountBarPages();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-thermo,.singu-cards,.singu-pulsebar,.singu-pages,#singu-menu-layout").forEach((e) => e.remove());
      document.querySelector(".taskbar")?.classList.remove("singu-ghost");
      document.documentElement.classList.remove("singu-menu-swap");
    },
  };
}
