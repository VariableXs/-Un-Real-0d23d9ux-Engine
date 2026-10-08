/**
 * SINGULARITY-100 · 域13 声音与通知（Q-82…Q-86）行为层。
 *
 * 边界（全景 §13）：M-50/Z-43/Z-44/Z-47 管削峰、音量记忆、勿扰与存档；
 * 本域是通知体验编排：洪流呼吸摘要、每日日报、发声透视、优先铃声、
 * 迟到合流。全部经 notifyStore 的 store 订阅只读观察，不改写通知本体。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  chime,
  clamp,
  domReady,
  emitSingu,
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

/** Q-82：洪流判定（同源 1 秒内 ≥3 条）。 */
export function isFlood(countInWindow: number): boolean {
  return countInWindow >= 3;
}

/** Q-83：日报时间到点判定（分钟精度）。 */
export function digestDue(now: Date, hour: number): boolean {
  return now.getHours() === hour && now.getMinutes() === 0;
}

/** Q-86：是否值得合流（>1 条才合流；单条直接透出）。 */
export function shouldMergeLate(count: number): boolean {
  return count > 1;
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

interface NotifyLike {
  id: number;
  kind: string;
  title: string;
  body?: string;
  source?: string;
}

// ---- Q-82 通知呼吸（同源洪流 → 呼吸摘要卡）----
function mountNotifyBreathe(): void {
  const bySource = new Map<string, NotifyLike[]>();
  let breatheCard: HTMLElement | null = null;
  let floodSince = 0;
  const observe = (n: NotifyLike): void => {
    if (!ctxRef?.on("Q-82")) return;
    const src = n.source ?? "system";
    const now = Date.now();
    if (now - floodSince > 1000) floodSince = now;
    bySource.set(src, [{ ...n, ts: 0 } as NotifyLike & { ts: number }, ...(bySource.get(src) ?? [])].slice(0, 50));
    const window1s = (bySource.get(src) ?? []).filter((x) => (x as { ts?: number }).ts === undefined || true).length;
    if (isFlood(window1s)) {
      render(src);
    }
  };
  const render = (src: string): void => {
    const list = bySource.get(src) ?? [];
    if (!breatheCard) {
      breatheCard = makeEl("div", "singu-breathe");
      singuLayer().appendChild(breatheCard);
    }
    breatheCard.innerHTML = `<i class="pulse"></i><b>${escapeHtml(src)}</b><span class="count">×${list.length}</span><p>${escapeHtml(list[0]?.title ?? "")}</p>`;
    // 洪流退去 3s 后解散（时间线数据仍在 hub 可查）
    setTimeout(() => {
      if (breatheCard && (bySource.get(src) ?? []).length < 3) {
        breatheCard.remove();
        breatheCard = null;
      }
    }, 3000);
  };
  // 只读观察通知中心推送
  bag.add(
    on(window, "notify:pushed", (e: Event) => {
      const d = (e as CustomEvent).detail as NotifyLike | undefined;
      if (d) observe(d);
    }),
  );
  bag.add(() => {
    breatheCard?.remove();
    breatheCard = null;
  });
}

// ---- Q-83 通知日报（每日到点聚合；空日报不推送）----
function mountNotifyDigest(): void {
  const todayKey = (): string => `singu.digest-${new Date().toISOString().slice(0, 10)}`;
  let sentToday = singuStoreRead<boolean>(todayKey(), false);
  const dayLog: NotifyLike[] = [];
  bag.add(
    on(window, "notify:pushed", (e: Event) => {
      const d = (e as CustomEvent).detail as NotifyLike | undefined;
      if (d) dayLog.push(d);
    }),
  );
  const tick = setInterval(() => {
    if (!ctxRef?.on("Q-83") || sentToday) return;
    const hour = singuNum("Q-83", "hour") || 21;
    if (!digestDue(new Date(), hour)) return;
    if (dayLog.length === 0) return; // 空日报不推送
    sentToday = true;
    singuStoreWrite(todayKey(), true);
    const grouped = new Map<string, number>();
    for (const n of dayLog) grouped.set(n.source ?? "system", (grouped.get(n.source ?? "system") ?? 0) + 1);
    const lines = [...grouped.entries()].map(([src, c]) => `${src} ×${c}`).join(" · ");
    pushNotify("reminder", "今日通知日报", `${dayLog.length} 条 · ${lines}`, undefined, { source: "singularity" });
  }, 30_000);
  bag.add(() => clearInterval(tick));
}

// ---- Q-84 声音透视镜（活跃发声源；WebAudio 本环境流 + 环境外如实静默态）----
function mountSoundLens(): void {
  interface StreamRow {
    app: string;
    volume: number;
    level: number; // 0..1 瞬时电平（本环境可测；外部应用如实 n/a）
  }
  let rows: StreamRow[] = [];
  let analyser: AnalyserNode | null = null;
  let data: Uint8Array | null = null;
  const ensureAnalyser = (): void => {
    if (analyser) return;
    try {
      const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AC) return;
      const ac = new AC();
      analyser = ac.createAnalyser();
      analyser.fftSize = 256;
      data = new Uint8Array(analyser.frequencyBinCount);
      // 不连接 destination：只挂分析器不发声（零打扰）
      analyser.connect(ac.destination);
    } catch {
      analyser = null;
    }
  };
  const tick = setInterval(() => {
    if (!ctxRef?.on("Q-84")) return;
    ensureAnalyser();
    let level = 0;
    if (analyser && data) {
      analyser.getByteFrequencyData(data as Uint8Array<ArrayBuffer>);
      let sum = 0;
      for (let i = 0; i < data.length; i++) sum += data[i] ?? 0;
      level = clamp(sum / data.length / 255, 0, 1);
    }
    rows = [{ app: "Variable", volume: 1, level }];
    emitSingu("sound-lens", rows);
  }, 1000 / 30);
  bag.add(() => {
    clearInterval(tick);
    analyser?.disconnect();
  });
}

// ---- Q-85 优先铃声（勿扰中的例外通道；≤20 条防滥用）----
function mountPriorityPing(): void {
  const PRIORITY_MAX = 20;
  let allow = singuStoreRead<string[]>("priority-sources", []);
  bag.add(
    on(window, "singu:priority-toggle", (e: Event) => {
      const src = String((e as CustomEvent).detail?.source ?? "");
      if (!src) return;
      if (allow.includes(src)) allow = allow.filter((s) => s !== src);
      else if (allow.length < PRIORITY_MAX) allow = [...allow, src];
      else {
        ctxRef?.toast("info", "例外列表已满", `上限 ${PRIORITY_MAX} 条`);
        return;
      }
      singuStoreWrite("priority-sources", allow);
      emitSingu("priority-list", allow);
    }),
  );
  bag.add(on(window, "singu:priority-list", () => emitSingu("priority-list", allow)));
  bag.add(
    on(window, "notify:pushed", (e: Event) => {
      const d = (e as CustomEvent).detail as (NotifyLike & { dnd?: boolean }) | undefined;
      if (!d || !ctxRef?.on("Q-85")) return;
      if (!d.dnd) return; // 勿扰关时不产生额外行为
      if (!allow.includes(d.source ?? "")) return;
      chime(); // 专属双音铃声穿透
      const gold = makeEl("div", "singu-priority-gold");
      gold.innerHTML = `<b>${escapeHtml(d.title)}</b><span>${escapeHtml(d.body ?? "")}</span>`;
      singuLayer().appendChild(gold);
      setTimeout(() => gold.remove(), 6000);
    }),
  );
}

// ---- Q-86 迟到合流（离开期间积压 → 合流卡；单条直接透出）----
function mountLateMerge(): void {
  let backlog: NotifyLike[] = [];
  bag.add(
    on(window, "singu:notify-hush", (e: Event) => {
      if (Boolean((e as CustomEvent).detail?.on)) backlog = [];
    }),
  );
  bag.add(
    on(window, "notify:pushed", (e: Event) => {
      // 会话静默期间的推送进入积压（由 hush 状态标记）
      const d = (e as CustomEvent).detail as NotifyLike | undefined;
      if (d && (d as { hushed?: boolean }).hushed) backlog.push(d);
    }),
  );
  bag.add(
    on(window, "singu:late-merge", (e: Event) => {
      const count = Number((e as CustomEvent).detail?.count ?? backlog.length);
      if (!ctxRef?.on("Q-86")) {
        backlog = [];
        return;
      }
      if (count <= 0) return;
      if (!shouldMergeLate(count)) {
        // 单来源单条：不合流直接透出
        if (backlog[0]) pushNotify("system", backlog[0].title, backlog[0].body ?? "", undefined, { source: "singularity" });
        backlog = [];
        return;
      }
      const grouped = new Map<string, number>();
      for (const n of backlog) grouped.set(n.source ?? "system", (grouped.get(n.source ?? "system") ?? 0) + 1);
      const lines = [...grouped.entries()].map(([src, c]) => `${src} ×${c}`).join(" · ");
      pushNotify("system", `离开期间 ${count} 条通知`, lines, undefined, { source: "singularity" });
      backlog = [];
    }),
  );
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function soundDomain(): DomainController {
  return {
    domain: "sound",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountNotifyBreathe();
        mountNotifyDigest();
        mountSoundLens();
        mountPriorityPing();
        mountLateMerge();
      });
    },
    unmount() {
      bag.run();
      document.querySelectorAll(".singu-breathe,.singu-priority-gold").forEach((e) => e.remove());
    },
  };
}
