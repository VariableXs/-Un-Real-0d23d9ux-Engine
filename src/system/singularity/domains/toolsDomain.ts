/**
 * SINGULARITY-100 · 域7 效率与工具中枢（Q-44…Q-50）行为层。
 *
 * 边界（全景 §7）：存量覆盖命令面板/剪贴板/截图/换算；本域补番茄、标尺、
 * 粘贴队列、划词条、轮盘、倒计时胶囊与收藏抽屉。计时器/抽屉在本层常驻，
 * 重 UI 在 overlay（pomodoro/ruler/countdown/stash 由 ai04 协议挂载）。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  chime,
  clamp,
  domReady,
  emitSingu,
  isTypingTarget,
  makeEl,
  on,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuNum } from "../registry";
import { pushNotify } from "../../../state/notifyStore";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-44：番茄阶段推进（返回下一阶段与剩余毫秒）。 */
export function pomodoroPhase(
  elapsedMs: number,
  focusMin: number,
  breakMin: number,
): { phase: "focus" | "break" | "done"; remainingMs: number; cycle: number } {
  const cycleMs = (focusMin + breakMin) * 60_000;
  const inCycle = elapsedMs % cycleMs;
  const cycle = Math.floor(elapsedMs / cycleMs);
  if (inCycle < focusMin * 60_000) {
    return { phase: "focus", remainingMs: focusMin * 60_000 - inCycle, cycle };
  }
  return { phase: "break", remainingMs: cycleMs - inCycle, cycle };
}

/** Q-46：粘贴队列 FIFO 出队。 */
export function dequeue<T>(q: readonly T[]): { item: T; rest: T[] } | null {
  if (q.length === 0) return null;
  return { item: q[0]!, rest: q.slice(1) };
}

/** Q-47：选区是否够格浮现工具条（≥10 字符）。 */
export function selectionQualified(text: string): boolean {
  return text.trim().length >= 10;
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-44 番茄剧场（计时内核 + 桌面聚光氛围 + 果实计数）----
function mountPomodoro(): void {
  interface PomoState {
    running: boolean;
    startedAt: number;
    pausedAt: number | null;
    fruits: number;
  }
  let state = singuStoreRead<PomoState>("pomodoro", { running: false, startedAt: 0, pausedAt: null, fruits: 0 });
  let ambience: HTMLElement | null = null;
  let stopRain: (() => void) | null = null;
  let tick: ReturnType<typeof setInterval> | null = null;
  const sync = (): void => singuStoreWrite("pomodoro", state);
  const apply = (): void => {
    const active = state.running && ctxRef?.on("Q-44");
    document.documentElement.classList.toggle("singu-pomo-focus", active);
    if (!ambience && active) {
      ambience = makeEl("div", "singu-pomo-glow");
      singuLayer().appendChild(ambience);
    } else if (ambience && !active) {
      ambience.remove();
      ambience = null;
    }
    emitSingu("pomodoro-tick", state);
  };
  const step = (): void => {
    if (!state.running) return;
    const focusMin = singuNum("Q-44", "focusMin") || 25;
    const breakMin = singuNum("Q-44", "breakMin") || 5;
    const elapsed = (state.pausedAt ?? Date.now()) - state.startedAt;
    const p = pomodoroPhase(elapsed, focusMin, breakMin);
    if (p.remainingMs < 1000) {
      // 完成一枚番茄（focus 段结束）
      state = { ...state, fruits: state.fruits + 1, startedAt: Date.now() };
      sync();
      chime();
      pushNotify("reminder", "番茄完成", `已积累 ${state.fruits} 枚果实`, undefined, { source: "singularity" });
    }
    apply();
  };
  tick = setInterval(step, 1000);
  bag.add(
    on(window, "singu:pomodoro", (e: Event) => {
      const detail = (e as CustomEvent).detail as { action: "start" | "pause" | "reset" } | undefined;
      if (!ctxRef?.on("Q-44")) return;
      if (detail?.action === "start") {
        state = { ...state, running: true, startedAt: state.pausedAt ? state.startedAt : Date.now(), pausedAt: null };
      } else if (detail?.action === "pause") {
        state = { ...state, running: false, pausedAt: Date.now() };
      } else {
        state = { running: false, startedAt: 0, pausedAt: null, fruits: state.fruits };
        stopRain?.();
        stopRain = null;
      }
      sync();
      apply();
    }),
  );
  // 休息期雨声（focus→break 切换时短促播放提示，雨声由 overlay 内手动开启长播）
  bag.add(() => {
    if (tick) clearInterval(tick);
    stopRain?.();
    ambience?.remove();
    document.documentElement.classList.remove("singu-pomo-focus");
  });
  apply();
}

// ---- Q-46 粘贴队列（Ctrl+Shift+C 收集 / Ctrl+Shift+V 依序出队）----
function mountPasteQueue(): void {
  interface QItem {
    text: string;
    ts: number;
  }
  // 隐私纪律：队列仅内存，跨会话不保留
  let queue: QItem[] = [];
  let hud: HTMLElement | null = null;
  const render = (): void => {
    if (queue.length === 0) {
      hud?.remove();
      hud = null;
      return;
    }
    if (!hud) {
      hud = makeEl("div", "singu-pastequeue");
      singuLayer().appendChild(hud);
    }
    hud.textContent = `QUEUE ${queue.length}`;
  };
  const copy = async (text: string): Promise<void> => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      ctxRef?.toast("error", "剪贴板不可用", "环境窗口需处于焦点状态");
    }
  };
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (!ke.ctrlKey || !ke.shiftKey) return;
      const k = ke.key.toLowerCase();
      if (k === "c" && ctxRef?.on("Q-46")) {
        const sel = window.getSelection()?.toString() ?? "";
        if (sel) {
          queue.push({ text: sel, ts: Date.now() });
          render();
          ke.preventDefault();
        }
      } else if (k === "v" && ctxRef?.on("Q-46")) {
        const out = dequeue(queue);
        if (out) {
          queue = out.rest;
          render();
          void copy(out.item.text);
          ke.preventDefault();
        }
      }
    }),
  );
  bag.add(
    on(window, "singu:paste-queue", (e: Event) => {
      const detail = (e as CustomEvent).detail as { action: "clear" | "peek" } | undefined;
      if (detail?.action === "clear") {
        queue = [];
        render();
      }
    }),
  );
  bag.add(() => {
    queue = [];
    hud?.remove();
  });
}

// ---- Q-47 划词工具箱（选中文本浮现工具条）----
function mountSelectionKit(): void {
  let bar: HTMLElement | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  const hide = (): void => {
    bar?.remove();
    bar = null;
  };
  bag.add(
    on(document, "selectionchange", () => {
      if (hideTimer) clearTimeout(hideTimer);
      hideTimer = setTimeout(() => {
        const sel = window.getSelection();
        const text = sel?.toString() ?? "";
        if (!ctxRef?.on("Q-47") || !selectionQualified(text)) return hide();
        const range = sel?.rangeCount ? sel.getRangeAt(0).getBoundingClientRect() : null;
        if (!range || (range.width === 0 && range.height === 0)) return hide();
        if (!bar) {
          bar = makeEl("div", "singu-selkit");
          for (const [label, act] of [
            ["字数", "count"],
            ["大写", "upper"],
            ["小写", "lower"],
            ["去空白", "trim"],
            ["存抽屉", "stash"],
          ] as Array<[string, string]>) {
            const b = makeEl("button", "singu-selkit-btn");
            b.textContent = label;
            b.dataset.act = act;
            b.addEventListener("pointerdown", (ev) => {
              ev.preventDefault();
              const s = window.getSelection()?.toString() ?? "";
              if (act === "count") {
                ctxRef?.toast("info", `共 ${s.length} 字符`, `${s.trim().split(/\s+/).filter(Boolean).length} 词`);
              } else if (act === "upper" || act === "lower") {
                void navigator.clipboard.writeText(act === "upper" ? s.toUpperCase() : s.toLowerCase());
                ctxRef?.toast("success", "已转换并复制");
              } else if (act === "trim") {
                void navigator.clipboard.writeText(s.replace(/\s+/g, " ").trim());
                ctxRef?.toast("success", "已去空白并复制");
              } else if (act === "stash") {
                window.dispatchEvent(new CustomEvent("singu:stash-add", { detail: { kind: "text", text: s } }));
                ctxRef?.toast("success", "已存入收藏抽屉");
              }
            });
            bar.appendChild(b);
          }
          singuLayer().appendChild(bar);
        }
        bar.style.left = `${clamp(range.left, 8, window.innerWidth - 320)}px`;
        bar.style.top = `${Math.max(8, range.top - 44)}px`;
      }, 220);
    }),
  );
  bag.add(hide);
}

// ---- Q-48 滚轮轮盘（Alt+滚轮径向应用轮盘）----
function mountWheelDial(): void {
  let host: HTMLElement | null = null;
  let sel = 0;
  const close = (): void => {
    host?.remove();
    host = null;
  };
  const apps = (): Array<{ id: string; name: string }> => {
    // 只读复用第三方应用注册表；无数据时如实空盘
    try {
      const raw = localStorage.getItem("variable.thirdApps");
      if (!raw) return [];
      return (JSON.parse(raw) as Array<{ id: string; name: string }>).slice(0, 8);
    } catch {
      return [];
    }
  };
  bag.add(
    on(window, "wheel", (e) => {
      const we = e as WheelEvent;
      if (!we.altKey || !ctxRef?.on("Q-48")) return;
      const list = apps();
      if (list.length === 0) return;
      we.preventDefault();
      if (!host) {
        host = makeEl("div", "singu-dial");
        list.forEach((a, i) => {
          const slot = makeEl("div", "singu-dial-slot");
          const angle = (i / list.length) * Math.PI * 2 - Math.PI / 2;
          slot.style.left = `${50 + 34 * Math.cos(angle)}%`;
          slot.style.top = `${50 + 34 * Math.sin(angle)}%`;
          slot.textContent = a.name.slice(0, 6);
          host!.appendChild(slot);
        });
        singuLayer().appendChild(host);
        sel = 0;
      }
      sel = (sel + (we.deltaY > 0 ? 1 : -1) + list.length) % list.length;
      host.querySelectorAll(".singu-dial-slot").forEach((s, i) => s.classList.toggle("on", i === sel));
    }, { passive: false }),
  );
  bag.add(
    on(window, "keyup", (e) => {
      if ((e as KeyboardEvent).key !== "Alt" || !host) return;
      const list = apps();
      const pick = list[sel];
      close();
      if (pick) {
        void import("../../launcher/thirdApps")
          .then(({ launchThirdApp }) => launchThirdApp(pick.id, pick.name))
          .catch(() => ctxRef?.toast("error", "启动失败", pick.name));
      }
    }),
  );
  bag.add(close);
}

// ---- Q-49 倒计时胶囊（任务栏旁常驻 + 到点通知；hub/overlay 创建）----
interface Capsule {
  id: number;
  label: string;
  dueAt: number;
  preAction: string | null;
}
function mountCountdown(): void {
  let capsules = singuStoreRead<Capsule[]>("capsules", []);
  let host: HTMLElement | null = null;
  const render = (): void => {
    const now = Date.now();
    capsules = capsules.filter((c) => c.dueAt > now - 60_000);
    singuStoreWrite("capsules", capsules);
    if (!ctxRef?.on("Q-49") || capsules.length === 0) {
      host?.remove();
      host = null;
      return;
    }
    if (!host) {
      host = makeEl("div", "singu-capsules");
      singuLayer().appendChild(host);
    }
    host.innerHTML = "";
    for (const c of capsules) {
      const remain = c.dueAt - now;
      const pill = makeEl("span", "singu-capsule");
      const min = Math.max(0, Math.round(remain / 60_000));
      pill.textContent = `${c.label} ${min}min`;
      pill.classList.toggle("soon", remain < 5 * 60_000 && remain > 0);
      pill.addEventListener("click", () => {
        capsules = capsules.filter((x) => x.id !== c.id);
        render();
      });
      host.appendChild(pill);
    }
  };
  bag.add(
    on(window, "singu:capsule-add", (e: Event) => {
      const d = (e as CustomEvent).detail as { label: string; minutes: number; preAction?: string };
      capsules = [
        ...capsules,
        { id: Date.now(), label: d.label, dueAt: Date.now() + d.minutes * 60_000, preAction: d.preAction ?? null },
      ];
      render();
    }),
  );
  let tick: ReturnType<typeof setInterval> | null = setInterval(() => {
    const now = Date.now();
    let changed = false;
    for (const c of capsules) {
      if (c.dueAt - now <= 5 * 60_000 && !c.preAction?.startsWith("notified5")) {
        pushNotify("reminder", `${c.label} · 5 分钟`, "倒计时即将到达", undefined, { source: "singularity" });
        c.preAction = `notified5:${c.id}`;
        changed = true;
      }
      if (c.dueAt - now <= 60_000 && !c.preAction?.startsWith("notified1")) {
        pushNotify("reminder", `${c.label} · 1 分钟`, "马上到达", undefined, { source: "singularity" });
        c.preAction = `notified1:${c.id}`;
        changed = true;
      }
      if (c.dueAt <= now && !c.preAction?.startsWith("fired")) {
        chime();
        pushNotify("reminder", `时间到 · ${c.label}`, "倒计时结束", undefined, { source: "singularity" });
        c.preAction = `fired:${c.id}`;
        changed = true;
      }
    }
    if (changed) render();
    else render();
  }, 30_000);
  render();
  bag.add(() => {
    if (tick) clearInterval(tick);
    tick = null;
    host?.remove();
  });
}

// ---- Q-50 收藏抽屉（侧滑中转；断电不丢）----
interface StashItem {
  id: number;
  kind: "file" | "text" | "link";
  value: string;
  ts: number;
}
const STASH_MAX = 200;
function mountStash(): void {
  let items = singuStoreRead<StashItem[]>("stash", []);
  let host: HTMLElement | null = null;
  const persist = (): void => {
    // LRU 200 配额
    if (items.length > STASH_MAX) {
      ctxRef?.toast("info", "抽屉已满", `超出 ${items.length - STASH_MAX} 条最早条目被淘汰`);
      items = items.slice(0, STASH_MAX);
    }
    singuStoreWrite("stash", items);
  };
  bag.add(
    on(window, "singu:stash-add", (e: Event) => {
      const d = (e as CustomEvent).detail as { kind: StashItem["kind"]; text?: string; path?: string };
      items = [
        { id: Date.now(), kind: d.kind, value: d.text ?? d.path ?? "", ts: Date.now() },
        ...items,
      ];
      persist();
    }),
  );
  bag.add(
    on(window, "singu:stash", (e: Event) => {
      if (!ctxRef?.on("Q-50")) return;
      const open = Boolean((e as CustomEvent).detail?.open);
      if (!open) {
        host?.remove();
        host = null;
        return;
      }
      if (host) return;
      host = makeEl("div", "singu-stash-drawer");
      host.innerHTML = `<div class="singu-panel-title">QUICK STASH · ${items.length}/${STASH_MAX}</div>`;
      for (const it of items.slice(0, 30)) {
        const row = makeEl("button", "singu-stash-item");
        row.innerHTML = `<i class="singu-stash-kind ${it.kind}"></i><span>${escapeHtml(it.value.slice(0, 42))}</span>`;
        row.addEventListener("click", () => {
          void navigator.clipboard.writeText(it.value);
          ctxRef?.toast("success", "已复制到剪贴板");
        });
        host!.appendChild(row);
      }
      const clearBtn = makeEl("button", "singu-btn danger");
      clearBtn.textContent = "清空抽屉";
      clearBtn.addEventListener("click", () => {
        items = [];
        persist();
        host?.remove();
        host = null;
      });
      host.appendChild(clearBtn);
      singuLayer().appendChild(host);
    }),
  );
  bag.add(() => {
    host?.remove();
    void isTypingTarget;
  });
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function toolsDomain(): DomainController {
  return {
    domain: "tools",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountPomodoro();
        mountPasteQueue();
        mountSelectionKit();
        mountWheelDial();
        mountCountdown();
        mountStash();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-pomo-glow,.singu-pastequeue,.singu-selkit,.singu-dial,.singu-capsules,.singu-stash-drawer").forEach((e) => e.remove());
      document.documentElement.classList.remove("singu-pomo-focus");
    },
  };
}
