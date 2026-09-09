/**
 * NOVA-200 · S0 地基 · NovaRuntime（全局运行时，AI-01 路代建，十六路共享）。
 *
 * 职责（实施总步骤 §0.6）：
 * - 订阅 registry，按域激活 16 个域模块（模块文件各自拥有，本运行时零越界）；
 * - 统一 reduce-motion / safeMode / static 降级链：
 *   · data-reduce-motion 由 App.tsx 既有链路写入（不重复接管）；
 *   · data-safe-mode / data-static-mode 由本运行时按 settings.safeMode /
 *     settings.perfMode==="static" 写入 <html>（nova.css 与 registry.novaMotionOK 消费）；
 * - 模块激活幂等；全部关闭时所有模块卸载（运行时自身仅保留 1 条 registry 订阅，
 *   供随时复活 —— 零常驻开销的诚实口径）。
 *
 * 模块发现：显式装配表（file→domain），每域一行；AI-06…AI-16 模块交付后由
 * S18 收官统一追加（接线收敛纪律：运行时是 S0 工件，各路不自行改写）。
 * 每个模块经动态 import 懒加载 —— 未激活的域零代码加载。
 */

import {
  NOVA_DOMAINS,
  novaDomainActive,
  novaStats,
  novaOn,
  novaNum,
  novaStr,
  novaBool,
  novaMotionOK,
  subscribeNova,
  type NovaDomainId,
} from "./registry";

/** 域模块统一运行时句柄（各模块的真实 activate/deactivate 的最小接口）。 */
interface DomainModuleHandle {
  domain: NovaDomainId;
  /** 模块文件名（观测/日志用）。 */
  file: string;
  activate: () => void | Promise<void>;
  deactivate: () => void;
}

type NovaModuleNamespace = Record<string, unknown>;

/** 二代工厂模块的 ctx 注入契约（deskNova 等共享）。 */
interface NovaModuleCtx {
  on: (id: string) => boolean;
  num: (id: string, key: string) => number;
  str: (id: string, key: string) => string;
  bool: (id: string, key: string) => boolean;
  motionOK: () => boolean;
}

function isFn(v: unknown): v is (...args: never[]) => unknown {
  return typeof v === "function";
}

/**
 * 从模块命名空间解析 activate/deactivate（两代契约兼容）：
 * - 一代（本仓库主流）：具名导出 activateXxxNova / deactivateXxxNova；
 * - 二代（deskNova）：createXxxNova() 工厂返回 { activate(ctx), deactivate() }。
 */
function resolveHandle(domain: NovaDomainId, file: string, mod: NovaModuleNamespace): DomainModuleHandle | null {
  const activateKey = Object.keys(mod).find((k) => /^activate\w+Nova$/.test(k) && isFn(mod[k]));
  if (activateKey) {
    const deactivateKey = Object.keys(mod).find((k) => /^deactivate\w+Nova$/.test(k) && isFn(mod[k]));
    const activate = mod[activateKey] as () => void;
    const deactivate = deactivateKey ? (mod[deactivateKey] as () => void) : () => {};
    return { domain, file, activate: () => activate(), deactivate };
  }
  const factoryKey = Object.keys(mod).find((k) => /^create\w+Nova$/.test(k) && isFn(mod[k]));
  if (factoryKey) {
    type FactoryHandle = { activate: (ctx: NovaModuleCtx) => void; deactivate: () => void };
    let handle: FactoryHandle | null = null;
    const ctx: NovaModuleCtx = {
      on: (id: string): boolean => novaOn(id),
      num: (id: string, key: string): number => novaNum(id, key),
      str: (id: string, key: string): string => novaStr(id, key),
      bool: (id: string, key: string): boolean => novaBool(id, key),
      motionOK: (): boolean => novaMotionOK(),
    };
    return {
      domain,
      file,
      activate: () => {
        const factory = mod[factoryKey] as () => FactoryHandle;
        const h = handle ?? (handle = factory());
        h.activate(ctx);
      },
      deactivate: () => handle?.deactivate(),
    };
  }
  return null;
}

// ---------------------------------------------------------------------------
// 显式装配表（file → domain；S18 收官按同样式追加 AI-06…AI-16 五个文件行）
// ---------------------------------------------------------------------------

type ModuleEntry = {
  domain: NovaDomainId;
  file: string;
  load: () => Promise<NovaModuleNamespace>;
};

const MODULE_TABLE: ModuleEntry[] = [
  { domain: "boot", file: "bootNova", load: () => import("./modules/bootNova") },
  { domain: "windows", file: "windowNova", load: () => import("./modules/windowNova") },
  { domain: "desktop", file: "deskNova", load: () => import("./modules/deskNova") },
  { domain: "dock", file: "dockNova", load: () => import("./modules/dockNova") },
  { domain: "input", file: "inputNova", load: () => import("./modules/inputNova") },
  { domain: "files", file: "filesNova", load: () => import("./modules/filesNova") },
  { domain: "tools", file: "toolsNova", load: () => import("./modules/toolsNova") },
  { domain: "hardware", file: "hwNova", load: () => import("./modules/hwNova") },
  { domain: "compat", file: "compatNova", load: () => import("./modules/compatNova") },
  { domain: "privacy", file: "privacyNova", load: () => import("./modules/privacyNova") },
];

// ---------------------------------------------------------------------------
// 纯逻辑（单测消费）
// ---------------------------------------------------------------------------

/** 降级数据集映射（settings → <html> dataset 布尔；reduce-motion 归 App 不在此列）。 */
export function degradeFromSettings(s: { safeMode?: boolean; perfMode?: string }): {
  safe: boolean;
  static: boolean;
} {
  return { safe: s.safeMode === true, static: s.perfMode === "static" };
}

/** 给定装配表与当前开关，返回应激活的域集合（novaDomainActive 语义）。 */
export function activeDomainsOf(entries: ReadonlyArray<{ domain: NovaDomainId }>): NovaDomainId[] {
  return entries.map((e) => e.domain).filter((d) => novaDomainActive(d));
}

/**
 * 模块同步差分（激活幂等的核心）：
 * - 应激活 且 未激活 → activate；
 * - 不应激活 且 已激活 → deactivate；
 * - 其余不动（重复激活零调用）。
 */
export function diffModuleSync(
  wanted: ReadonlySet<NovaDomainId>,
  live: ReadonlyMap<NovaDomainId, { domain: NovaDomainId }>,
): { toActivate: NovaDomainId[]; toDeactivate: NovaDomainId[] } {
  const toActivate: NovaDomainId[] = [];
  const toDeactivate: NovaDomainId[] = [];
  for (const d of wanted) if (!live.has(d)) toActivate.push(d);
  for (const d of live.keys()) if (!wanted.has(d)) toDeactivate.push(d);
  return { toActivate, toDeactivate };
}

/** 装配表覆盖校验（S18 门禁用：十六域全部有主）。 */
export function missingDomainModules(entries: ReadonlyArray<{ domain: NovaDomainId }>): NovaDomainId[] {
  const have = new Set(entries.map((e) => e.domain));
  return NOVA_DOMAINS.map((d) => d.id).filter((id) => !have.has(id));
}

// ---------------------------------------------------------------------------
// 运行时单例
// ---------------------------------------------------------------------------

interface RuntimeState {
  un: (() => void) | null;
  live: Map<NovaDomainId, DomainModuleHandle>;
  pending: Set<NovaDomainId>;
  degradeTimer: number | null;
}

const rt: RuntimeState = { un: null, live: new Map(), pending: new Set(), degradeTimer: null };

/** 降级数据集落盘（真实 settings 读取；失败如实跳过不猜测）。 */
async function refreshDegradeDatasets(): Promise<void> {
  if (typeof document === "undefined") return;
  try {
    const { loadSettings } = await import("../../lib/settings");
    const s = await loadSettings();
    if (!s) return;
    const d = degradeFromSettings(s);
    document.documentElement.dataset.safeMode = String(d.safe);
    document.documentElement.dataset.staticMode = String(d.static);
  } catch {
    /* 浏览器 dev / 设置不可读：保持现状（诚实不降级猜测） */
  }
}

async function syncModules(): Promise<void> {
  const wanted = new Set(activeDomainsOf(MODULE_TABLE));
  const { toActivate, toDeactivate } = diffModuleSync(wanted, rt.live);
  for (const d of toDeactivate) {
    const h = rt.live.get(d);
    if (h) {
      try {
        h.deactivate();
      } catch {
        /* 单模块卸载失败不阻塞其余 */
      }
      rt.live.delete(d);
    }
  }
  for (const d of toActivate) {
    if (rt.pending.has(d)) continue;
    const entry = MODULE_TABLE.find((e) => e.domain === d);
    if (!entry) continue;
    rt.pending.add(d);
    void entry
      .load()
      .then((mod) => {
        rt.pending.delete(d);
        // 激活前复查（订阅风暴/快速切换下防止僵尸激活）
        if (!novaDomainActive(d)) return;
        const handle = resolveHandle(d, entry.file, mod);
        if (!handle) return; // 模块未导出可识别契约：如实跳过
        rt.live.set(d, handle);
        void handle.activate();
      })
      .catch(() => {
        rt.pending.delete(d);
        /* 模块加载失败：诚实跳过（hub 可重试），不阻塞其余域 */
      });
  }
}

/** 全部关闭 → 模块全卸载（运行时自身保留订阅以复活；轻量至可忽略）。 */
function teardownIfAllOff(): void {
  if (novaStats().on > 0) return;
  for (const [, h] of rt.live) {
    try {
      h.deactivate();
    } catch {
      /* 同上 */
    }
  }
  rt.live.clear();
}

let started = false;

/**
 * 启动运行时（幂等；activate.ts 模块加载时调用一次）：
 * 降级数据集 → registry 订阅 → 首轮同步。
 */
export function startNovaRuntime(): void {
  if (started) return;
  started = true;
  void refreshDegradeDatasets();
  rt.un = subscribeNova(() => {
    void syncModules();
    teardownIfAllOff();
  });
  void syncModules();
  teardownIfAllOff();
}

/** 停止运行时（测试/热重载）：卸载全部模块 + 退订。 */
export function stopNovaRuntime(): void {
  rt.un?.();
  rt.un = null;
  for (const [, h] of rt.live) {
    try {
      h.deactivate();
    } catch {
      /* 尽力而为 */
    }
  }
  rt.live.clear();
  started = false;
}

/** 运行时观测快照（hub 状态页/测试用）。 */
export function novaRuntimeStatus(): {
  started: boolean;
  liveDomains: NovaDomainId[];
  pendingDomains: NovaDomainId[];
  missingModules: NovaDomainId[];
} {
  return {
    started,
    liveDomains: [...rt.live.keys()],
    pendingDomains: [...rt.pending],
    missingModules: missingDomainModules(MODULE_TABLE),
  };
}

/** hub 打开时复读降级设置（设置无事件总线，打开动作即刷新点）。 */
export function refreshNovaDegrade(): void {
  void refreshDegradeDatasets();
}
