/**
 * SINGULARITY-100 · 域9 兼容性防线（Q-58…Q-63）行为层。
 *
 * 边界（全景 §9）：Z-17/Z-19 管既有兼容矩阵；本域是变更确认协议、
 * HDR 认知、低电量防线、IME 瞬间缓冲、会话静默与断屏救援。
 * 全部「只拦变更瞬间」，不碰稳态行为（守护既有手感）。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  domReady,
  isTypingTarget,
  makeEl,
  on,
  recordDegrade,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuBool, singuNum } from "../registry";
import { pushNotify } from "../../../state/notifyStore";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-58：确认条剩余时间 → 按钮文案（≤0 = 应回滚）。 */
export function escortLabel(remainSec: number): string {
  if (remainSec <= 0) return "Reverting…";
  return `Keep new display settings? ${remainSec}s`;
}

/** Q-60：电量跨档判定（返回本次跨越到的最低档；null = 未跨档）。 */
export function batteryTierCrossed(
  prev: number | null,
  now: number,
  low1: number,
  low2: number,
): 1 | 2 | null {
  if (prev === null) return null;
  if (prev > low1 && now <= low1 && now > low2) return 1;
  if (prev > low2 && now <= low2) return 2;
  return null;
}

/** Q-61：缓冲窗口判定（150ms 内的按键纳入缓冲）。 */
export function inBufferWindow(bufferSince: number, now: number, windowMs = 150): boolean {
  return now - bufferSince <= windowMs;
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-58 分辨率护送（DPR/屏幕几何变更 → 10s 确认条，超时回滚应用内补偿）----
function mountResolutionEscort(): void {
  let bar: HTMLElement | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  let remain = 0;
  let prevSnapshot = "";
  const snapshot = (): string => `${window.devicePixelRatio.toFixed(3)}@${screen.width}x${screen.height}`;
  const close = (): void => {
    if (timer) clearInterval(timer);
    timer = null;
    bar?.remove();
    bar = null;
  };
  const rollback = (): void => {
    // 诚实边界：应用无法改写系统分辨率；回滚 = 通知 + 派发恢复事件，
    // 由显示设置面板（sysenv_display_set）接收真实 API 回滚请求。
    window.dispatchEvent(new CustomEvent("singu:display-rollback", { detail: { from: prevSnapshot, to: snapshot() } }));
    ctxRef?.toast("info", "已发起显示设置回滚", "如系统未响应请到显示设置手动确认");
    recordDegrade(
      "display-escort",
      "分辨率护送回滚",
      false,
      "变更超时未确认，已派发回滚请求",
      "在显示设置中确认最终状态",
    );
    close();
  };
  const open = (): void => {
    if (bar) return;
    remain = Math.round(singuNum("Q-58", "confirmSec") || 10);
    bar = makeEl("div", "singu-escort-bar");
    const label = makeEl("span", "singu-escort-label");
    const keep = makeEl("button", "singu-btn");
    keep.textContent = "Keep";
    const revert = makeEl("button", "singu-btn danger");
    revert.textContent = "Revert";
    bar.append(label, keep, revert);
    keep.addEventListener("click", () => {
      ctxRef?.toast("success", "已保持新显示设置");
      close();
    });
    revert.addEventListener("click", rollback);
    singuLayer().appendChild(bar);
    const paint = (): void => {
      label.textContent = escortLabel(remain);
    };
    paint();
    timer = setInterval(() => {
      remain -= 1;
      paint();
      if (remain <= 0) rollback();
    }, 1000);
  };
  const check = (): void => {
    if (!ctxRef?.on("Q-58")) return;
    const cur = snapshot();
    if (prevSnapshot === "") {
      prevSnapshot = cur;
      return;
    }
    if (cur !== prevSnapshot) {
      prevSnapshot = cur;
      open();
    }
  };
  bag.add(on(window, "resize", check));
  bag.add(close);
  check();
}

// ---- Q-59 HDR 感知（dynamic-range 检测 → 每显示状态一次性提示）----
function mountHdrAware(): void {
  const seenKey = "singu.hdr-seen";
  let mq: MediaQueryList | null = null;
  const handler = (e: MediaQueryListEvent | MediaQueryList): void => {
    if (!ctxRef?.on("Q-59")) return;
    const hdr = e.matches;
    if (!hdr) return;
    if (singuStoreRead<boolean>(seenKey, false)) return;
    singuStoreWrite(seenKey, true);
    pushNotify(
      "system",
      "HDR 已开启",
      "环境界面按 SDR 语义渲染，壁纸将获得更广亮度（预期行为，不是颜色坏了）",
      undefined,
      { source: "singularity" },
    );
  };
  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    mq = window.matchMedia("(dynamic-range: high)");
    handler(mq);
    mq.addEventListener?.("change", handler);
    bag.add(() => mq?.removeEventListener?.("change", handler));
  }
}

// ---- Q-60 低电量防线（两档跨域：提示保存 + 暂停活化；自动 static 降级）----
function mountLowBattery(): void {
  let prev: number | null = null;
  let tier2Engaged = false;
  const tick = setInterval(() => {
    const p = ctxRef?.pulse();
    if (!p || !ctxRef?.on("Q-60") || p.battery_percent === null || p.battery_ac) {
      if (tier2Engaged && p?.battery_ac) {
        tier2Engaged = false;
        document.documentElement.classList.remove("singu-battery-static");
        recordDegrade("battery-defense", "低电量防线", false, "已接入电源", "恢复动画观感");
      }
      prev = p?.battery_percent ?? null;
      return;
    }
    const low1 = singuNum("Q-60", "low1") || 15;
    const low2 = singuNum("Q-60", "low2") || 7;
    const now = p.battery_percent;
    const crossed = batteryTierCrossed(prev, now, low1, low2);
    prev = now;
    if (crossed === 1) {
      pushNotify("hardware", "电量低（第一档）", `剩余 ${now}%，建议保存工作 · 壁纸活化已暂停（可撤销）`, undefined, {
        source: "singularity",
      });
      window.dispatchEvent(new CustomEvent("ai04:wallpaper-apply", { detail: { mode: "image" } }));
    } else if (crossed === 2) {
      tier2Engaged = true;
      document.documentElement.classList.add("singu-battery-static");
      window.dispatchEvent(new CustomEvent("ai04:wallpaper-apply", { detail: { mode: "image" } }));
      pushNotify(
        "hardware",
        "电量临界（第二档）",
        `剩余 ${now}%，动画观感已降级到 static · 接入电源后自动恢复`,
        undefined,
        { source: "singularity" },
      );
      recordDegrade(
        "battery-defense",
        "低电量防线",
        true,
        "电量跨过第二档阈值，视频壁纸与全环境动画已降级",
        "接入电源即恢复（动作全部可撤销）",
      );
    }
  }, 5000);
  bag.add(() => {
    clearInterval(tick);
    document.documentElement.classList.remove("singu-battery-static");
  });
}

// ---- Q-61 IME 缓冲（切换瞬间 150ms 按键暂存 + 语义重放）----
function mountImeBuffer(): void {
  let composing = false;
  let switchAt = 0;
  const pending: KeyboardEvent[] = [];
  bag.add(
    on(window, "keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (ke.isComposing || ke.key === "Process") {
        if (pending.length === 0) switchAt = Date.now();
        return;
      }
      // 非 Process 按键落在切换窗口内 → 暂存（不消费，让既有输入正常处理；
      // 缓冲的价值是 compositionend 后对「丢字」的补救重放）
      if (pending.length > 0 && inBufferWindow(switchAt, Date.now())) {
        pending.push(ke);
        if (pending.length > 8) pending.shift();
      }
    }, true),
  );
  bag.add(
    on(window, "compositionstart", () => {
      composing = true;
      pending.length = 0;
    }),
  );
  bag.add(
    on(window, "compositionend", () => {
      composing = false;
      if (!ctxRef?.on("Q-61")) return;
      switchAt = Date.now();
    }),
  );
  // 缓冲重放：compositionend 后 150ms 若目标输入框未见内容变化 → 重放暂存键
  bag.add(
    on(window, "compositionend", () => {
      setTimeout(() => {
        if (pending.length === 0 || !ctxRef?.on("Q-61")) return;
        const target = document.activeElement;
        if (!isTypingTarget(target)) return;
        const el = target as HTMLInputElement;
        const before = el.value;
        setTimeout(() => {
          const after = el.value;
          if (before !== after) return; // 输入法已正确上屏，无需补救
          for (const ke of pending) {
            el.dispatchEvent(
              new KeyboardEvent("keydown", { key: ke.key, code: ke.code, bubbles: true }),
            );
          }
        }, 60);
      }, 150);
    }),
  );
  bag.add(() => {
    pending.length = 0;
    void composing;
  });
}

// ---- Q-62 会话静默（锁屏/失焦会话 → 媒体静默 + 通知暂存；恢复后询问）----
function mountSessionHush(): void {
  let hushed = false;
  let mutedBefore: number | null = null;
  const stash: Array<{ kind: "info" | "success" | "error"; title: string; body?: string }> = [];
  const hush = (): void => {
    if (hushed || !ctxRef?.on("Q-62")) return;
    hushed = true;
    window.dispatchEvent(new CustomEvent("singu:media-hush", { detail: { on: true } }));
    window.dispatchEvent(new CustomEvent("singu:notify-hush", { detail: { on: true } }));
    recordDegrade(
      "session-hush",
      "会话静默",
      true,
      "检测到会话锁定/切换，媒体已暂停、通知已暂存",
      "解锁后询问是否恢复",
    );
  };
  const resume = (): void => {
    if (!hushed) return;
    hushed = false;
    window.dispatchEvent(new CustomEvent("singu:media-hush", { detail: { on: false } }));
    window.dispatchEvent(new CustomEvent("singu:notify-hush", { detail: { on: false } }));
    recordDegrade("session-hush", "会话静默", false, "会话已恢复", "已询问媒体恢复");
    if (stash.length > 0) {
      // Q-86 迟到合流：暂存通知合流出卡
      window.dispatchEvent(new CustomEvent("singu:late-merge", { detail: { count: stash.length } }));
      stash.length = 0;
    }
    if (singuBool("Q-62", "askResume") && mutedBefore !== null) {
      pushNotify("system", "会话已恢复", "是否恢复媒体播放？到通知中心点击恢复（不自动恢复，尊重你）", undefined, {
        source: "singularity",
      });
    }
  };
  bag.add(on(document, "visibilitychange", () => (document.hidden ? hush() : resume())));
  bag.add(() => {
    resume();
    void mutedBefore;
  });
}

// ---- Q-63 显示器守护（屏幕数减少 → VWM 窗口救回主屏按面积排布）----
function mountDisplayGuard(): void {
  let prevCount = 0;
  const check = (): void => {
    if (!ctxRef?.on("Q-63")) return;
    const infos = (window as unknown as { __variableScreens?: unknown[] }).__variableScreens;
    const count = Array.isArray(infos) ? infos.length : 1;
    if (prevCount === 0) {
      prevCount = count;
      return;
    }
    if (count < prevCount) {
      window.dispatchEvent(new CustomEvent("singu:screen-rescue", { detail: { lost: prevCount, now: count } }));
      ctxRef.toast("info", "显示器已断开", "其上的窗口已搬回主屏 · 重连后可一键复位到原屏原位");
    } else if (count > prevCount) {
      window.dispatchEvent(new CustomEvent("singu:screen-restored", { detail: { now: count } }));
    }
    prevCount = count;
  };
  bag.add(on(window, "resize", check));
  bag.add(check);
}

export function compatDomain(): DomainController {
  return {
    domain: "compat",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountResolutionEscort();
        mountHdrAware();
        mountLowBattery();
        mountImeBuffer();
        mountSessionHush();
        mountDisplayGuard();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-escort-bar").forEach((e) => e.remove());
      document.documentElement.classList.remove("singu-battery-static");
    },
  };
}
