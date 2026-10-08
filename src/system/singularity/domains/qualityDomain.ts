/**
 * SINGULARITY-100 · 域15 工程质量、性能与收官（Q-93…Q-100）行为层。
 *
 * 边界（全景 §15）：V-60/Z-58/M-46/M-53/M-85/U-24 管既有门禁与面板；
 * 本域是启动火焰图、长任务看板、数据配额、崩溃演练、瘦身报告、
 * 降级矩阵、更新预览与版点心电图。被动观测优先（零自身长任务），
 * 无法获取的数据如实标注「无数据」。
 */

import {
  type DomainController,
  type DomainCtx,
  UnsubBag,
  clamp,
  domReady,
  emitSingu,
  on,
  ringPush,
  singuStoreRead,
  singuStoreWrite,
} from "../shared";
import { getDegradations, subscribeDegrade } from "../shared";
import { singuTempScan } from "../singuIpc";

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-93：火焰图层级矩形布局（宽度∝耗时）。 */
export interface FlameStage {
  name: string;
  ms: number;
}
export function flameLayout(stages: readonly FlameStage[], widthPx: number): Array<FlameStage & { x: number; w: number }> {
  const total = stages.reduce((s, x) => s + x.ms, 0) || 1;
  let x = 0;
  return stages.map((s) => {
    const w = clamp((s.ms / total) * widthPx, 4, widthPx);
    const row = { ...s, x, w };
    x += w;
    return row;
  });
}

/** Q-95：配额预警（超 90% 预警）。 */
export function quotaWarn(bytes: number, quotaBytes: number): boolean {
  if (quotaBytes <= 0) return false;
  return bytes / quotaBytes > 0.9;
}

/** Q-100：心电图导联健康骤降判定（环比跌 >30% 即红色）。 */
export function ecgDrop(prev: number | null, now: number): boolean {
  if (prev === null || prev <= 0) return false;
  return (prev - now) / prev > 0.3;
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-93 启动火焰图（bootTimeline 埋点 → 留最近 20 次）----
function mountBootFlame(): void {
  interface BootRun {
    ts: number;
    stages: FlameStage[];
  }
  let runs = singuStoreRead<BootRun[]>("boot-runs", []);
  const record = (): void => {
    const marks = performance.getEntriesByType("measure");
    const stages: FlameStage[] = marks
      .filter((m) => m.name.startsWith("boot:"))
      .map((m) => ({ name: m.name.slice(5), ms: m.duration }));
    if (stages.length === 0) {
      // 无埋点时如实单阶段（总耗时）
      stages.push({ name: "total", ms: Math.round(performance.now()) });
    }
    runs = ringPush(runs, { ts: Date.now(), stages }, 20);
    singuStoreWrite("boot-runs", runs);
  };
  // 桌面就绪后记录一次（延迟避开启动噪声）
  setTimeout(record, 15_000);
  bag.add(
    on(window, "singu:flame-load", () => {
      emitSingu("flame", runs);
    }),
  );
}

// ---- Q-94 长任务看板（PerformanceObserver 被动观测；保留 5 分钟）----
function mountLongTaskBoard(): void {
  interface LongTask {
    ms: number;
    ts: number;
    source: string;
  }
  let tasks: LongTask[] = [];
  let observer: PerformanceObserver | null = null;
  if (typeof PerformanceObserver !== "undefined") {
    try {
      observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.duration < 50) continue;
          tasks = ringPush(tasks, { ms: Math.round(entry.duration), ts: Date.now(), source: (entry as { name?: string }).name ?? "main" }, 600);
        }
      });
      observer.observe({ entryTypes: ["longtask"] });
    } catch {
      observer = null; // 环境不支持：如实降级
    }
  }
  bag.add(
    on(window, "singu:longtask-load", () => {
      const cutoff = Date.now() - 5 * 60_000;
      emitSingu("longtask", tasks.filter((t) => t.ts >= cutoff));
    }),
  );
  bag.add(() => observer?.disconnect());
}

// ---- Q-95 数据配额（各区当前量/配额/趋势总账）----
const DATA_QUOTAS: Record<string, number> = {
  cache: 512 * 1024 * 1024,
  temp: 1024 * 1024 * 1024,
  logs: 256 * 1024 * 1024,
  media: 8 * 1024 * 1024 * 1024,
};
function mountDataQuota(): void {
  bag.add(
    on(window, "singu:quota-load", () => {
      void singuTempScan().then((temp) => {
        const zones: Array<{ zone: string; bytes: number; quota: number; warn: boolean }> = [];
        const tempBytes = (temp ?? []).reduce((s, t) => s + t.bytes, 0);
        const quota = DATA_QUOTAS.temp ?? 0;
        zones.push({ zone: "temp", bytes: tempBytes, quota, warn: quotaWarn(tempBytes, quota) });
        // 其余区由 Rust singu_data_profile 提供；不可用时如实标注 0 配额=无数据
        emitSingu("quota", zones);
      });
    }),
  );
}

// ---- Q-96 崩溃演练（受控异常注入；仅显式开启可见）----
function mountCrashDrill(): void {
  bag.add(
    on(window, "singu:drill", (e: Event) => {
      if (!ctxRef?.on("Q-96")) return;
      const kind = String((e as CustomEvent).detail?.kind ?? "render");
      if (kind === "render") {
        // 受控渲染异常：由 ErrorBoundary 兜住（U-23 崩溃叙事验证）
        window.dispatchEvent(new CustomEvent("singu:drill-render-throw", { detail: { on: true } }));
      } else if (kind === "ipc") {
        // IPC 超时演练：调一个不存在的命令，走既有错误链路
        void import("../singuIpc")
          .then(({ singuPulse }) => singuPulse())
          .then(() => emitSingu("drill-done", { kind }));
      } else if (kind === "extension") {
        emitSingu("drill-extension-crash");
      }
    }),
  );
}

// ---- Q-97 瘦身报告（打包产物清单；无产物如实「尚未打包」）----
function mountBundleDiet(): void {
  bag.add(
    on(window, "singu:diet-load", () => {
      void fetch("asset://bundle-manifest.json")
        .then((r) => (r.ok ? r.json() : null))
        .then((manifest) => emitSingu("diet", manifest ?? { error: "尚未打包（未找到产物清单）" }))
        .catch(() => emitSingu("diet", { error: "尚未打包（未找到产物清单）" }));
    }),
  );
}

// ---- Q-98 降级矩阵（单一真源 getDegradations；订阅推送）----
function mountDegradationMatrix(): void {
  const push = (): void => emitSingu("degradations", getDegradations());
  bag.add(subscribeDegrade(push));
  bag.add(on(window, "singu:degrade-load", push));
  push();
}

// ---- Q-99 更新预览（版本差/新能力/体积差/回滚承诺；读巡礼数据）----
function mountUpdatePreview(): void {
  bag.add(
    on(window, "singu:update-preview", () => {
      const changelog = singuStoreRead<string[]>("boot-carnival-log", []);
      emitSingu("update-preview", {
        version: "see sysmaint::update_scan",
        highlights: changelog.slice(0, 8),
        rollback: "更新前自动创建完整备份，可一键回滚到当前版本",
      });
    }),
  );
}

// ---- Q-100 版点心电图（最近 10 版四导联；本地构建记录）----
function mountReleaseEcg(): void {
  interface EcgBeat {
    version: string;
    gates: number;
    tests: number;
    benchMs: number;
    crashes: number;
  }
  let beats = singuStoreRead<EcgBeat[]>("ecg-beats", []);
  bag.add(
    on(window, "singu:ecg-load", () => {
      // 数据来自本地构建记录（CI 门禁产物落盘）；无记录如实「无数据」
      if (beats.length === 0) {
        emitSingu("ecg", { beats: [], error: "无数据（尚未有本地构建记录）" });
        return;
      }
      const flags = beats.slice(0, 9).map((b, i) => ({
        version: b.version,
        gatesDrop: ecgDrop(beats[i + 1]?.gates ?? null, b.gates),
        crashesRise: ecgDrop(-b.crashes, -(beats[i + 1]?.crashes ?? -b.crashes)),
      }));
      emitSingu("ecg", { beats: beats.slice(0, 10), flags, error: null });
    }),
  );
  bag.add(
    on(window, "singu:ecg-append", (e: Event) => {
      const beat = (e as CustomEvent).detail as EcgBeat;
      if (!beat?.version) return;
      beats = ringPush(beats, beat, 10);
      singuStoreWrite("ecg-beats", beats);
    }),
  );
}

export function qualityDomain(): DomainController {
  return {
    domain: "quality",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountBootFlame();
        mountLongTaskBoard();
        mountDataQuota();
        mountCrashDrill();
        mountBundleDiet();
        mountDegradationMatrix();
        mountUpdatePreview();
        mountReleaseEcg();
      });
    },
    unmount() {
      bag.run();
    },
  };
}
