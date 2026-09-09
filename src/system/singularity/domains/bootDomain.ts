/**
 * SINGULARITY-100 · 域1 启动与品牌剧场（Q-01…Q-07）行为层。
 *
 * 边界（全景 §1）：存量 U-01…U-06 管冷启动剧场；本域管唤醒、谢幕、
 * 剧本多样性、实况解说、品牌彩蛋与晨昏问候 —— 时间线与冷启动正交。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  domReady,
  emitSingu,
  makeEl,
  on,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuNum, singuMotionOK, subscribeSingu } from "../registry";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-07：时段分档（0-4 = 清晨/上午/下午/黄昏/深夜）。 */
export function daySegment(hour: number): number {
  if (hour < 6) return 4;
  if (hour < 11) return 0;
  if (hour < 14) return 1;
  if (hour < 18) return 2;
  if (hour < 23) return 3;
  return 4;
}

/** Q-07：是否该显示问候（每次会话一次）。 */
export function greetable(seenAt: number | null, now: number, sessionStart: number): boolean {
  if (seenAt !== null && seenAt >= sessionStart) return false;
  return now - sessionStart > 1500; // 桌面就绪后短暂延迟
}

/** Q-06：品牌时刻判定（全部离线日期规则；返回彩蛋 id 或 null）。 */
export function brandMoment(now: Date): "anniversary" | "newyear" | "birthday" | null {
  const m = now.getMonth();
  const d = now.getDate();
  if (m === 0 && d === 1) return "newyear";
  if (m === 11 && d === 31 && now.getHours() >= 23) return "newyear";
  if (m === 8 && d === 9) return "birthday"; // 项目诞辰（仓库首个 commit 纪念）
  return null;
}

/** Q-01：唤醒判定（距上次失焦超过阈值分钟才算唤醒）。 */
export function isWake(lastBlurTs: number | null, now: number, wakeMin: number): boolean {
  if (lastBlurTs === null) return false;
  return now - lastBlurTs >= wakeMin * 60_000;
}

/** Q-03：首跑巡礼判定（有旧版本戳且与当前不同 = 升级首启）。 */
export function paradeDue(lastVersionStamp: string | null, currentVersion: string): boolean {
  if (lastVersionStamp === null) return false; // 全新安装：走 U-57，不打扰
  return lastVersionStamp !== currentVersion;
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;
let greeted = false;

/** 全屏一次性剧场（问候/彩蛋/巡礼共用宿主）。 */
function theater(html: string, ms: number, cls: string, onDone?: () => void): void {
  const host = makeEl("div", `singu-theater ${cls}`, { pointerEvents: "none" });
  host.innerHTML = html;
  singuLayer().appendChild(host);
  const finish = (): void => {
    host.remove();
    onDone?.();
  };
  const timer = setTimeout(finish, ms);
  const skip = (e: Event): void => {
    if (e instanceof KeyboardEvent && e.key === "Escape") {
      clearTimeout(timer);
      finish();
    }
  };
  window.addEventListener("keydown", skip, { once: true });
  setTimeout(() => window.removeEventListener("keydown", skip), ms);
}

function mountWakeHandshake(): void {
  // Q-01：失焦/聚焦握手 —— 会话内唤醒剧场（冷启动 BootScreen 与之正交）
  let lastBlur: number | null = null;
  bag.add(on(window, "blur", () => (lastBlur = Date.now())));
  bag.add(
    on(window, "focus", () => {
      if (!ctxRef || !ctxRef.on("Q-01")) return;
      const wakeMin = singuNum("Q-01", "wakeMin");
      if (!isWake(lastBlur, Date.now(), wakeMin)) return;
      lastBlur = null;
      const hour = new Date().getHours();
      const hello = ["GOOD DAWN", "GOOD MORNING", "GOOD NOON", "GOOD DUSK", "GOOD NIGHT"][daySegment(hour)];
      if (!singuMotionOK()) {
        // 降级：静态问候 200ms
        theater(`<div class="singu-wake-static">${hello}</div>`, 200, "singu-wake");
        return;
      }
      theater(
        `<div class="singu-wake-light"></div><div class="singu-wake-clock">${hello}</div>`,
        900,
        "singu-wake",
      );
      emitSingu("wake-handshake");
    }),
  );
}

function mountCurtainCall(): void {
  // Q-02：谢幕剧场 —— 挂 ai04:curtain-call 总线（退出编排可派发）+ beforeunload 机会性播放
  const play = (): void => {
    if (!ctxRef || !ctxRef.on("Q-02") || !singuMotionOK()) return;
    theater(`<div class="singu-curtain-beam"></div>`, 650, "singu-curtain");
    emitSingu("curtain-call");
  };
  bag.add(on(window, "ai04:curtain-call", play));
  bag.add(on(window, "beforeunload", play));
}

function mountBootCommentary(): void {
  // Q-05：启动实况解说 —— 监听既有 boot://event 真实阶段，工程腔字幕 + 实测耗时
  void import("@tauri-apps/api/event")
    .then(({ listen }) => {
      const un = listen<{ stage?: string; label?: string; pct?: number }>("boot://event", (e) => {
        if (!ctxRef?.on("Q-05")) return;
        const stage = e.payload?.stage ?? e.payload?.label ?? "";
        if (!stage) return;
        const cap = String(stage).toUpperCase().replace(/[^A-Z0-9 :_-]/g, " ").trim();
        if (!cap) return;
        let bar = document.querySelector<HTMLElement>("#singu-bootcast");
        if (!bar) {
          bar = makeEl("div", "singu-bootcast");
          bar.id = "singu-bootcast";
          singuLayer().appendChild(bar);
        }
        bar.textContent = `${cap} · ${(e.payload?.pct ?? 0).toFixed(0)}%`;
      });
      bag.add(() => {
        void un.then((f) => f());
      });
      // 桌面就绪后 3s 淡出
      const fade = setTimeout(() => document.querySelector("#singu-bootcast")?.remove(), 30_000);
      bag.add(() => clearTimeout(fade));
    })
    .catch(() => {
      /* 非 Tauri：启动事件不可听，Q-05 如实静默 */
    });
}

function mountBrandMoments(): void {
  // Q-06：品牌时刻 —— 启动后一次判定，非彩蛋日零开销
  const t = setTimeout(() => {
    if (!ctxRef?.on("Q-06")) return;
    const kind = brandMoment(new Date());
    if (!kind) return;
    if (kind === "newyear") {
      theater(`<div class="singu-brand-stardust"></div>`, 2500, "singu-brand");
    } else {
      theater(`<div class="singu-brand-anniv">VARIABLE · ANNIVERSARY</div>`, 2200, "singu-brand");
    }
    emitSingu("brand-moment", kind);
  }, 2600);
  bag.add(() => clearTimeout(t));
}

function mountDayGreeting(): void {
  // Q-07：晨昏问候 —— 每会话一次，4s 即散，深夜最暗样式
  const t = setTimeout(() => {
    if (!ctxRef?.on("Q-07") || greeted) return;
    greeted = true;
    const seg = daySegment(new Date().getHours());
    const focus = singuStoreRead<string | null>("focus", null);
    const focusLine = focus ? `<div class="singu-greet-focus">${escapeHtml(focus)}</div>` : "";
    const words = [
      ["清晨好", "EARLY LIGHT"],
      ["上午好", "GOOD MORNING"],
      ["下午好", "GOOD AFTERNOON"],
      ["黄昏好", "GOOD EVENING"],
      ["夜深了", "LATE NIGHT"],
    ][seg] ?? ["你好", "HELLO"];
    const date = new Date().toLocaleDateString();
    theater(
      `<div class="singu-greet ${seg === 4 ? "night" : ""}">
        <div class="singu-greet-word">${words[0]}</div>
        <div class="singu-greet-sub">${words[1]} · ${escapeHtml(date)}</div>
        ${focusLine}
      </div>`,
      4000,
      "singu-greet-host",
    );
  }, 3200);
  bag.add(() => clearTimeout(t));
}

function mountFirstRunParade(): void {
  // Q-03：首跑巡礼 —— 升级后首启的增量能力卡（新装不打扰）
  const t = setTimeout(async () => {
    if (!ctxRef?.on("Q-03")) return;
    let ver = "";
    try {
      const { getVersion } = await import("@tauri-apps/api/app");
      ver = await getVersion();
    } catch {
      ver = "";
    }
    if (!ver) return; // 版本不可读：如实跳过
    const stamp = singuStoreRead<string | null>("version", null);
    if (!paradeDue(stamp, ver)) {
      if (stamp !== ver) singuStoreWrite("version", ver);
      return;
    }
    singuStoreWrite("version", ver);
    theater(
      `<div class="singu-parade">
        <div class="singu-parade-card">SINGULARITY-100<br/><span>100 项新能力已就位 · 中枢见 Q 面板</span></div>
        <div class="singu-parade-card">WAKE HANDSHAKE<br/><span>唤醒握手与谢幕剧场</span></div>
        <div class="singu-parade-card">TOOLS<br/><span>番茄剧场 · 屏幕标尺 · 收藏抽屉</span></div>
      </div>`,
      6000,
      "singu-parade-host",
    );
    emitSingu("first-run-parade", ver);
  }, 4000);
  bag.add(() => clearTimeout(t));
}

function mountBootScript(): void {
  // Q-04：启动剧本库 —— 以 data-singu-boot 标注剧本，CSS 层重皮肤 BootAtmosphere
  // （只覆盖既有 boot 视觉层参数，不碰进度真实性与胶囊条结构）
  const apply = (): void => {
    if (!ctxRef?.on("Q-04")) {
      delete document.documentElement.dataset.singuBoot;
      return;
    }
    let script = "aurora";
    try {
      script = ctxRef.str("Q-04", "script") || "aurora";
    } catch {
      script = "aurora";
    }
    const safe = ["aurora", "snowfield", "matrix", "paperplane"].includes(script) ? script : "aurora";
    document.documentElement.dataset.singuBoot = safe;
  };
  apply();
  // 注册表变化即时换装（hub 调参无需重启）
  bag.add(subscribeSingu(apply));
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function bootDomain(): DomainController {
  return {
    domain: "boot",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountWakeHandshake();
        mountCurtainCall();
        mountBootScript();
        mountBootCommentary();
        mountBrandMoments();
        mountDayGreeting();
        mountFirstRunParade();
      });
    },
    unmount() {
      bag.run();
      greeted = false;
      delete document.documentElement.dataset.singuBoot;
      document.querySelector("#singu-bootcast")?.remove();
    },
  };
}
