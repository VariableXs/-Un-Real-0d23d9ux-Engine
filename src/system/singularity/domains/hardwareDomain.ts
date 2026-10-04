/**
 * SINGULARITY-100 · 域8 系统集成与硬件（Q-51…Q-57）行为层。
 *
 * 边界（全景 §8）：N-19..N-23 是数据面板/管理中心；本域是环境织入——
 * GPU/电池/温度/磁盘/网络全部化为无数字的氛围表达，风扇高速时主动
 * 降载（Q-57）并如实记账到降级矩阵（Q-98）。
 * 采样统一走共享 pulse（2s 缓存）；不可读的能力如实隐藏（不伪造）。
 */

import {
  type DomainController,
  type DomainCtx,
  type SinguPulseLike,
  UnsubBag,
  clamp,
  domReady,
  emitSingu,
  lerp,
  makeEl,
  on,
  recordDegrade,
  singuLayer,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { singuBool } from "../registry";
import { pushNotify } from "../../../state/notifyStore";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-51：GPU 负载 → 极光强度 0..1（null GPU 驱动：CPU 占用率替代并注明）。 */
export function auroraIntensity(p: SinguPulseLike | null): { v: number; byCpu: boolean } {
  if (!p) return { v: 0, byCpu: false };
  if (typeof p.gpu_usage === "number") return { v: clamp(p.gpu_usage / 100, 0, 1), byCpu: false };
  return { v: clamp(p.cpu_usage / 100, 0, 1) * 0.6, byCpu: true };
}

/** Q-52：电量 → 呼吸周期 ms 与暖色权重（插电常亮：period=0）。 */
export function breathProfile(percent: number | null, ac: boolean): { periodMs: number; warm: number } {
  if (percent === null) return { periodMs: 0, warm: 0 };
  if (ac) return { periodMs: 0, warm: 0 };
  const t = clamp(percent / 100, 0, 1);
  return { periodMs: lerp(1200, 4000, t), warm: 1 - t };
}

/** Q-53：CPU 温度 → 色温色相（40℃ 蓝 → 95℃ 红）。 */
export function tempHue(c: number): number {
  return lerp(210, 0, clamp((c - 40) / 55, 0, 1));
}

/** Q-53：最近采样斜率 → 趋势箭头（+1 升 / -1 降 / 0 平）。 */
export function tempTrend(samples: readonly number[]): number {
  if (samples.length < 3) return 0;
  const n = Math.min(5, samples.length);
  const recent = samples.slice(0, n);
  const slope = (recent[0]! - recent[recent.length - 1]!) / (recent.length - 1);
  if (slope > 0.6) return 1;
  if (slope < -0.6) return -1;
  return 0;
}

/** Q-54：占用率 → 湿度四态。 */
export function humidityState(usagePct: number): "dry" | "moist" | "damp" | "tide" {
  if (usagePct > 90) return "tide";
  if (usagePct >= 70) return "damp";
  if (usagePct >= 30) return "moist";
  return "dry";
}

/** Q-56：速率 → 风向标强度 0..1（对数感知：小流量也可见，满速拉满）。 */
export function vaneIntensity(bps: number): number {
  if (bps <= 0) return 0;
  return clamp(Math.log10(1 + bps / 1024) / Math.log10(1 + 100 * 1024 * 1024 / 1024), 0, 1);
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

/** 共享 UI 宿主（右下角指示器列：温度点 + 风向标；固定宿主避免反复插拔）。 */
function trayHost(): HTMLElement {
  let host = document.getElementById("singu-hw-tray");
  if (!host) {
    host = makeEl("div", "singu-hw-tray");
    host.id = "singu-hw-tray";
    singuLayer().appendChild(host);
  }
  return host;
}

// ---- Q-51 GPU 画布 + Q-52 电池呼吸 + Q-53 热点 + Q-56 风向标（共用脉搏步进）----
function mountPulseDisplays(): void {
  let aurora: HTMLElement | null = null;
  let breath: HTMLElement | null = null;
  let dot: HTMLElement | null = null;
  let vane: HTMLElement | null = null;
  let tempSamples: number[] = [];

  const step = (): void => {
    const p = ctxRef?.pulse() ?? null;

    // Q-51 GPU 画布：极光强度（仅壁纸活化态叠加；关闭则隐藏）
    const wantAurora = ctxRef?.on("Q-51") === true && p !== null;
    if (wantAurora) {
      if (!aurora) {
        aurora = makeEl("div", "singu-gpu-aurora");
        singuLayer().appendChild(aurora);
      }
      const { v, byCpu } = auroraIntensity(p);
      aurora.style.setProperty("--singu-aurora", String(v));
      aurora.classList.toggle("by-cpu", byCpu);
      aurora.title = byCpu ? "GPU unavailable · CPU-driven aurora" : "GPU aurora";
    } else if (aurora) {
      aurora.remove();
      aurora = null;
    }

    // Q-52 电池呼吸：左下角微光源
    const wantBreath = ctxRef?.on("Q-52") === true && p !== null && p.battery_percent !== null;
    if (wantBreath) {
      if (!breath) {
        breath = makeEl("div", "singu-battery-breath");
        singuLayer().appendChild(breath);
      }
      const bp = breathProfile(p!.battery_percent, p!.battery_ac);
      breath.style.setProperty("--singu-breath-ms", String(bp.periodMs));
      breath.style.setProperty("--singu-breath-warm", String(bp.warm));
      breath.classList.toggle("steady", bp.periodMs === 0);
    } else if (breath) {
      breath.remove();
      breath = null;
    }

    // Q-53 热成像角标（右下角 8px 色温点 + 悬停精确值/趋势）
    const temp = p?.cpu_temp_c ?? null;
    if (ctxRef?.on("Q-53") === true && temp !== null) {
      if (!dot) {
        dot = makeEl("div", "singu-temp-dot");
        dot.addEventListener("pointerenter", () => {
          if (!dot || tempSamples.length === 0) return;
          const latest = tempSamples[0]!;
          const t = tempTrend(tempSamples);
          dot.title = `${Math.round(latest)}°C ${t > 0 ? "↑" : t < 0 ? "↓" : "→"}`;
        });
        trayHost().appendChild(dot);
      }
      tempSamples = [temp, ...tempSamples].slice(0, 5);
      dot.style.setProperty("--singu-temp-hue", String(tempHue(temp)));
    } else if (dot) {
      dot.remove();
      dot = null;
      tempSamples = [];
    }

    // Q-56 网络风向标（上下行双三角）
    if (ctxRef?.on("Q-56") === true && p !== null) {
      if (!vane) {
        vane = makeEl("div", "singu-net-vane");
        vane.innerHTML = `<span class="up"></span><span class="down"></span>`;
        trayHost().appendChild(vane);
      }
      const up = vane.querySelector(".up") as HTMLElement | null;
      const down = vane.querySelector(".down") as HTMLElement | null;
      if (up) up.style.setProperty("--singu-vane", String(vaneIntensity(p.net_up_bps)));
      if (down) down.style.setProperty("--singu-vane", String(vaneIntensity(p.net_down_bps)));
      vane.title =
        p.net_down_bps <= 0 && p.net_up_bps <= 0
          ? "offline"
          : `↓ ${fmtBps(p.net_down_bps)} · ↑ ${fmtBps(p.net_up_bps)}`;
      vane.classList.toggle("offline", p.net_down_bps <= 0 && p.net_up_bps <= 0);
    } else if (vane) {
      vane.remove();
      vane = null;
    }
  };

  const tick = setInterval(step, 2000);
  bag.add(() => {
    clearInterval(tick);
    aurora?.remove();
    breath?.remove();
    dot?.remove();
    vane?.remove();
  });
  step();
}

function fmtBps(bps: number): string {
  if (bps < 1024) return `${Math.round(bps)} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
  return `${(bps / 1024 / 1024).toFixed(1)} MB/s`;
}

// ---- Q-54 存储湿度计（占用四态 + 涨潮通知；冰山/配额共用其账本）----
function mountHumidity(): void {
  const zoneKey = "singu.humidity";
  let lastState = singuStoreRead<string>(zoneKey, "");
  const tick = setInterval(() => {
    if (!ctxRef?.on("Q-54")) return;
    const p = ctxRef.pulse();
    if (!p || p.disks.length === 0) return;
    // 系统盘（容量最大者）作为环境湿度代表
    const main = [...p.disks].sort((a, b) => b.total_bytes - a.total_bytes)[0];
    if (!main) return;
    const usage = 1 - main.available_bytes / Math.max(1, main.total_bytes);
    const st = humidityState(usage * 100);
    if (st !== lastState) {
      lastState = st;
      singuStoreWrite(zoneKey, st);
      emitSingu("humidity", { state: st, usage: usage * 100, mount: main.mount });
      if (st === "tide" && singuBool("Q-54", "tideNotify")) {
        pushNotify("hardware", "存储涨潮", `${main.mount} 已用 ${Math.round(usage * 100)}% · 打开空间分析器可治理`, undefined, {
          source: "singularity",
        });
      }
    }
  }, 5000);
  bag.add(() => clearInterval(tick));
}

// ---- Q-55 外设画廊（usb:// 事件 → 设备卡片滑入/滑出，3s 退场）----
function mountDeviceGallery(): void {
  interface Card {
    el: HTMLElement;
    timer: ReturnType<typeof setTimeout>;
  }
  const cards = new Map<string, Card>();
  const show = (id: string, name: string, detail: string, leaving: boolean): void => {
    const old = cards.get(id);
    if (old) {
      clearTimeout(old.timer);
      old.el.remove();
    }
    const el = makeEl("div", `singu-device-card${leaving ? " leaving" : ""}`);
    el.innerHTML = `<i class="plug"></i><div class="body"><b>${escapeHtml(name)}</b><span>${escapeHtml(detail)}</span></div>`;
    el.addEventListener("click", () => {
      if (leaving) return;
      window.dispatchEvent(new CustomEvent("explorer:navigate", { detail: { path: detail } }));
    });
    singuLayer().appendChild(el);
    const timer = setTimeout(() => {
      el.remove();
      cards.delete(id);
    }, leaving ? 1200 : 3000);
    cards.set(id, { el, timer });
  };
  bag.add(
    on(window, "usb://arrived", (e: Event) => {
      if (!ctxRef?.on("Q-55")) return;
      const d = (e as CustomEvent).detail as { id?: string; name?: string; path?: string; label?: string };
      show(d.id ?? String(Date.now()), d.name ?? d.label ?? "USB Device", d.path ?? "", false);
    }),
  );
  bag.add(
    on(window, "usb://removed", (e: Event) => {
      if (!ctxRef?.on("Q-55")) return;
      const d = (e as CustomEvent).detail as { id?: string; name?: string; label?: string };
      show(d.id ?? String(Date.now()), d.name ?? d.label ?? "USB Device", "", true);
    }),
  );
  bag.add(() => {
    for (const c of cards.values()) {
      clearTimeout(c.timer);
      c.el.remove();
    }
    cards.clear();
  });
}

// ---- Q-57 风扇感知（fan_high → 全局动画降载；30s 滞回恢复）----
function mountFanAware(): void {
  let engaged = false;
  let recoverTimer: ReturnType<typeof setTimeout> | null = null;
  const tick = setInterval(() => {
    const p = ctxRef?.pulse();
    if (!p || !ctxRef?.on("Q-57")) {
      if (engaged) disengage("feature off");
      return;
    }
    if (p.fan_high && !engaged) {
      engage();
    } else if (!p.fan_high && engaged) {
      // 30s 滞回防抖：风扇回落后等满 30s 再恢复
      if (!recoverTimer) {
        recoverTimer = setTimeout(() => {
          recoverTimer = null;
          if (engaged) disengage("fan recovered 30s");
        }, 30_000);
      }
    } else if (p.fan_high && engaged && recoverTimer) {
      clearTimeout(recoverTimer);
      recoverTimer = null;
    }
  }, 3000);
  const engage = (): void => {
    engaged = true;
    document.documentElement.classList.add("singu-fan-hush");
    recordDegrade(
      "fan-hush",
      "风扇感知降载",
      true,
      "CPU 风扇高速，壁纸粒子密度与动画帧率已降低",
      "风扇回落后 30 秒自动恢复",
    );
  };
  const disengage = (why: string): void => {
    engaged = false;
    document.documentElement.classList.remove("singu-fan-hush");
    recordDegrade("fan-hush", "风扇感知降载", false, why, "已恢复");
  };
  bag.add(() => {
    clearInterval(tick);
    if (recoverTimer) clearTimeout(recoverTimer);
    disengage("unmount");
  });
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

export function hardwareDomain(): DomainController {
  return {
    domain: "hardware",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountPulseDisplays();
        mountHumidity();
        mountDeviceGallery();
        mountFanAware();
      });
    },
    unmount() {
      bag.run();
      document.getElementById("singu-hw-tray")?.remove();
      document.documentElement.classList.remove("singu-fan-hush");
    },
  };
}
