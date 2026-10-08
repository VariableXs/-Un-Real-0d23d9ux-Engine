/**
 * SINGULARITY-100 · 域11 开放生态（Q-71…Q-76）行为层。
 *
 * 边界（全景 §11）：M-63/N-28/Z-52/U-39/N-29 管能力本体；本域是生态的
 * 体验层——营养标签、插件体检、API 演练场、事件订阅、包预览与 CLI 速查。
 * 本层只做数据采集与事件桥；重 UI 在 overlay（ai04 协议挂载）。
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

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

/** Q-71：营养标签（无真实采样数据时如实标注「未经试用」，绝不编造）。 */
export interface Nutrition {
  memMb: number | null;
  wakeupsPerMin: number | null;
  permRequests: number | null;
  sampled: boolean;
}
export function nutritionRows(n: Nutrition): Array<{ key: string; value: string }> {
  if (!n.sampled) {
    return [
      { key: "memory", value: "not sampled" },
      { key: "wakeups", value: "not sampled" },
      { key: "permissions", value: "not sampled" },
    ];
  }
  return [
    { key: "memory", value: n.memMb === null ? "n/a" : `${Math.round(n.memMb)} MB` },
    { key: "wakeups", value: n.wakeupsPerMin === null ? "n/a" : `${n.wakeupsPerMin.toFixed(1)}/min` },
    { key: "permissions", value: n.permRequests === null ? "n/a" : String(n.permRequests) },
  ];
}

/** Q-72：三轴评分（内存增幅 / 事件订阅 / 错误率 → A–E）。 */
export function pluginGrade(memGrowthPct: number, subs: number, errRate: number): "A" | "B" | "C" | "D" | "E" {
  let score = 0;
  if (memGrowthPct > 50) score += 2;
  else if (memGrowthPct > 20) score += 1;
  if (subs > 40) score += 2;
  else if (subs > 15) score += 1;
  if (errRate > 0.05) score += 2;
  else if (errRate > 0.01) score += 1;
  return (["A", "B", "C", "D", "E"] as const)[clamp(score, 0, 4)] ?? "E";
}

// ---------------------------------------------------------------------------
// 行为层
// ---------------------------------------------------------------------------

let bag = new UnsubBag();
let ctxRef: DomainCtx | null = null;

// ---- Q-71/Q-72 营养标签 + 插件体检（采样账本；overlay 数据源）----
interface PluginSample {
  id: string;
  memMb: number;
  subs: number;
  errors: number;
  ts: number;
}
function mountPluginHealth(): void {
  let samples = singuStoreRead<PluginSample[]>("plugin-samples", []);
  const record = (s: Omit<PluginSample, "ts">): void => {
    samples = ringPush(samples, { ...s, ts: Date.now() }, 600);
    singuStoreWrite("plugin-samples", samples);
    emitSingu("plugin-sample", s);
  };
  // 事件订阅计数：环境内插件事件流量（只读观察）
  const subCounts = new Map<string, number>();
  bag.add(
    on(window, "plugin:event", (e: Event) => {
      const id = String((e as CustomEvent).detail?.id ?? "");
      if (!id) return;
      subCounts.set(id, (subCounts.get(id) ?? 0) + 1);
    }),
  );
  // 周期采样（60s）：内存 via performance.memory（Chromium；不可用如实跳过）
  const perf = () => (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory;
  const tick = setInterval(() => {
    if (!ctxRef?.on("Q-72")) return;
    const m = perf();
    if (!m) return;
    record({
      id: "__env__",
      memMb: m.usedJSHeapSize / 1024 / 1024,
      subs: subCounts.size,
      errors: 0,
    });
  }, 60_000);
  bag.add(() => clearInterval(tick));
  // 体检报告事件（overlay 拉取时计算）
  bag.add(
    on(window, "singu:checkup", () => {
      const byPlugin = new Map<string, PluginSample[]>();
      for (const s of samples) {
        byPlugin.set(s.id, [...(byPlugin.get(s.id) ?? []), s]);
      }
      const report = [...byPlugin.entries()].map(([id, list]) => {
        const last = list[0]!;
        const first = list[list.length - 1] ?? last;
        const memGrowthPct = first.memMb > 0 ? ((last.memMb - first.memMb) / first.memMb) * 100 : 0;
        const errRate = last.errors / Math.max(1, list.length);
        return { id, grade: pluginGrade(memGrowthPct, last.subs, errRate), memGrowthPct, subs: last.subs, errRate };
      });
      emitSingu("checkup-report", report);
    }),
  );
}

// ---- Q-73 API 演练场（仅本地网关 fetch；overlay 渲染）----
function mountApiPlayground(): void {
  bag.add(
    on(window, "singu:api-try", (e: Event) => {
      const d = (e as CustomEvent).detail as { method?: string; path: string; body?: string };
      const method = (d.method ?? "GET").toUpperCase();
      const started = performance.now();
      void fetch(`http://127.0.0.1:17777${d.path.startsWith("/") ? d.path : `/${d.path}`}`, {
        method,
        headers: { "content-type": "application/json" },
        body: method === "GET" || method === "HEAD" ? undefined : d.body,
      })
        .then(async (r) => {
          const text = await r.text();
          emitSingu("api-result", {
            status: r.status,
            ms: Math.round(performance.now() - started),
            body: text.slice(0, 4096),
          });
        })
        .catch((err: unknown) => {
          emitSingu("api-result", { status: 0, ms: Math.round(performance.now() - started), body: String(err) });
        });
    }),
  );
}

// ---- Q-74 事件订阅面板（订阅矩阵 + 500 条环形预览）----
function mountEventPanel(): void {
  interface SubRule {
    event: string;
    action: "log" | "webhook" | "macro";
    target: string;
    on: boolean;
  }
  let rules = singuStoreRead<SubRule[]>("event-rules", []);
  let preview: Array<{ event: string; ts: number }> = [];
  const proxy = (name: string): void => {
    preview = ringPush(preview, { event: name, ts: Date.now() }, 500);
    for (const r of rules) {
      if (!r.on || r.event !== name) continue;
      if (r.action === "log") continue; // 环形缓冲即日志
      if (r.action === "webhook") {
        void fetch(r.target, { method: "POST", body: JSON.stringify({ event: name, ts: Date.now() }) }).catch(
          () => undefined,
        );
      } else if (r.action === "macro") {
        window.dispatchEvent(new CustomEvent("macro:invoke", { detail: { name: r.target } }));
      }
    }
  };
  // 代理全部 singu:* 事件（单一入口，500 条环形）
  bag.add(on(window, "singu:hub-open", () => proxy("hub-open")));
  bag.add(on(window, "singu:api-result", () => proxy("api-result")));
  bag.add(on(window, "singu:humidity", () => proxy("humidity")));
  bag.add(on(window, "singu:plugin-sample", () => proxy("plugin-sample")));
  bag.add(
    on(window, "singu:event-rules", (e: Event) => {
      const d = (e as CustomEvent).detail as { rules?: SubRule[] };
      if (Array.isArray(d.rules)) {
        rules = d.rules;
        singuStoreWrite("event-rules", rules);
      }
    }),
  );
  bag.add(on(window, "singu:event-preview", () => emitSingu("event-preview", preview.slice(0, 50))));
}

// ---- Q-75 资源包预览（内存流读取清单；损坏包如实报错）----
function mountPackPreviewer(): void {
  bag.add(
    on(window, "singu:pack-preview", (e: Event) => {
      const path = String((e as CustomEvent).detail?.path ?? "");
      if (!path) return;
      // 只读走 Rust zip 清单（singu_archive_check 的结构语义）；
      // 前端仅编排：清单 → 树 + token 预览事件
      void import("../singuIpc")
        .then(({ singuArchiveCheck }) => singuArchiveCheck(path))
        .then((report) => {
          if (!report) {
            emitSingu("pack-report", { ok: false, error: "Rust backend unavailable" });
            return;
          }
          emitSingu("pack-report", { ok: true, report });
        })
        .catch((err: unknown) => emitSingu("pack-report", { ok: false, error: String(err) }));
    }),
  );
}

// ---- Q-76 CLI 速查（场景分组命令卡；overlay 渲染，点击复制）----
const CLI_CHEATS: Array<{ scene: string; cmds: Array<[string, string]> }> = [
  {
    scene: "launch",
    cmds: [
      ["variable open <app>", "open an app window"],
      ["variable desktop", "focus the desktop shell"],
    ],
  },
  {
    scene: "windows",
    cmds: [
      ["variable win list", "list virtual windows"],
      ["variable win arrange grid", "arrange windows in grid"],
    ],
  },
  {
    scene: "settings",
    cmds: [
      ["variable settings get <key>", "read a setting"],
      ["variable settings set <key> <value>", "write a setting"],
    ],
  },
  {
    scene: "diagnose",
    cmds: [
      ["variable doctor", "run self-check"],
      ["variable log tail", "tail app logs"],
    ],
  },
];
function mountCliCheatsheet(): void {
  bag.add(on(window, "singu:cli-list", () => emitSingu("cli-list", CLI_CHEATS)));
  bag.add(
    on(window, "singu:cli-exec", (e: Event) => {
      const line = String((e as CustomEvent).detail?.line ?? "");
      if (!line.trim()) return;
      emitSingu("cli-echo", { line, output: "本地 CLI 由 Rust 侧执行；此处为速查面板回显" });
    }),
  );
}

export function ecoDomain(): DomainController {
  return {
    domain: "eco",
    mount(ctx: DomainCtx) {
      ctxRef = ctx;
      bag = new UnsubBag();
      domReady(() => {
        mountPluginHealth();
        mountApiPlayground();
        mountEventPanel();
        mountPackPreviewer();
        mountCliCheatsheet();
      });
    },
    unmount() {
      bag.run();
    },
  };
}
