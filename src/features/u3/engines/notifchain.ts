/**
 * U3-v8 引擎三：notifchain —— 通知中心 → 横幅 → 锁屏三链路体验日志贯通
 * （AI-U3 · 批次八工单③）。
 *
 * 判据唯一源（主册摘文 + 工单）：
 * - 十三章「浮层从打开到关闭的完整生命线（谁打开的、怎么关的、开关之间
 *   用户做了什么）」——通知类浮层有三层落点（通知中心常驻、横幅瞬时、
 *   锁屏态接力），此前三层各自记日志、事件串不成线；本引擎用关联 id
 *   （correlation id）把一条通知在三层之间的每一次转手串成一条可回放
 *   链路，任何一层的孤儿事件（缺上游）都是显性缺陷。
 * - 十三章「每个事件带体验结论字段」——链路每跳都推导结论（顺畅/被打断/
 *   报错），链尾给出整链结论。
 * - F516 横幅位置 / F507 锁屏接力：横幅在锁屏态不显示（锁屏防截红线），
 *   通知改走锁屏接力层——层路由是有判据的，不是随手 if。
 * - 隐私红线：链路只记层转手与结果，不记通知正文。
 */

import { u3ExpLog, type ExpEvent } from "./explog";

/* ------------------------------- 层模型 ------------------------------- */

/** 三层落点（链路节点形制）。 */
export type NotifLayer = "center" | "banner" | "lockscreen";

/** 层路由判据（F507 红线：锁屏态横幅不显示，改走锁屏接力）。 */
export function routeFor(lockedScreen: boolean): NotifLayer[] {
  return lockedScreen ? ["center", "lockscreen"] : ["center", "banner"];
}

/** 链路节点（脱敏：只存 id 与结果，不存通知正文）。 */
export interface ChainNode {
  layer: NotifLayer;
  action: "enter" | "show" | "dismiss" | "expire" | "click" | "escalate";
  atMs: number;
  latencyMs: number;
  verdict: "smooth" | "laggy" | "no-response" | "interrupted" | "error";
  /** 出路（dismiss/click/expire 必带——十三章「完整消失路径」）。 */
  exit: string | null;
}

/** 一条通知的完整链路。 */
export interface NotifChain {
  /** 关联 id（三层串线的唯一凭据）。 */
  chainId: string;
  /** 预期路由（routeFor 产物——实际节点必须落在预期层内）。 */
  expected: ReadonlyArray<NotifLayer>;
  nodes: ChainNode[];
  /** 整链结论（链尾推导）。 */
  finalVerdict: "smooth" | "laggy" | "interrupted" | "error" | "open";
}

/* ------------------------------- 链路记录器 ------------------------------- */

export class NotifChainLog {
  private chains = new Map<string, NotifChain>();
  private nextId = 1;

  /** 开链（通知入中心——一切链路的唯一起点；孤儿横幅由此显性化）。 */
  open(lockedScreen: boolean, atMs: number): NotifChain {
    const chainId = `n${this.nextId++}`;
    const chain: NotifChain = { chainId, expected: routeFor(lockedScreen), nodes: [], finalVerdict: "open" };
    chain.nodes.push({ layer: "center", action: "enter", atMs, latencyMs: 0, verdict: "smooth", exit: null });
    this.chains.set(chainId, chain);
    return chain;
  }

  /** 记节点（非 center 层必须已有链——孤儿检测的事实基础）。 */
  node(chainId: string, n: Omit<ChainNode, "verdict"> ): NotifChain {
    const chain = this.chains.get(chainId);
    if (!chain) throw new Error(`[u3:notifchain] 孤儿事件：链 ${chainId} 不存在——通知未经中心入链`);
    if (!chain.expected.includes(n.layer)) {
      throw new Error(`[u3:notifchain] 层路由违规：链 ${chainId} 预期 [${chain.expected.join("/")}]，收到 ${n.layer}`);
    }
    const verdict = n.action === "expire" ? "interrupted" : n.latencyMs >= 100 ? "laggy" : "smooth";
    chain.nodes.push({ ...n, verdict });
    this.finalize(chain);
    return chain;
  }

  /** 孤儿检测（十三章显性化：不经中心的层事件必须被抓到）。 */
  orphanRejected(layer: NotifLayer, action: ChainNode["action"]): boolean {
    try {
      this.node(`ghost-${this.nextId + 999}`, { layer, action, atMs: 0, latencyMs: 0, exit: null });
      return false;
    } catch {
      return true;
    }
  }

  private finalize(c: NotifChain): void {
    const last = c.nodes[c.nodes.length - 1];
    if (!last) return;
    const closed = last.action === "dismiss" || last.action === "click" || last.action === "expire";
    c.finalVerdict = !closed ? "open"
      : last.verdict === "error" ? "error"
      : c.nodes.some((n) => n.verdict === "interrupted") ? "interrupted"
      : c.nodes.some((n) => n.verdict === "laggy") ? "laggy"
      : "smooth";
  }

  /** 全链导出（可回放：按开链顺序）。 */
  exportAll(): NotifChain[] {
    return [...this.chains.values()].sort((a, b) => a.chainId.localeCompare(b.chainId));
  }

  /** 链路审计（完整性：每链都有 center 起点、closed 链有出路、open 链显性）。 */
  audit(): { total: number; closed: number; open: number; violations: string[] } {
    const violations: string[] = [];
    let closed = 0;
    let open = 0;
    for (const c of this.exportAll()) {
      if (!c.nodes.some((n) => n.layer === "center" && n.action === "enter")) violations.push(`${c.chainId}: 缺中心起点`);
      const last = c.nodes[c.nodes.length - 1];
      if (c.finalVerdict === "open") {
        open++;
        if (last && last.action !== "show" && last.action !== "escalate" && last.action !== "enter") {
          violations.push(`${c.chainId}: open 链尾态可疑（${last.action}）`);
        }
      } else {
        closed++;
        const lastNode = last;
        if (!lastNode?.exit && lastNode?.action === "dismiss") violations.push(`${c.chainId}: dismiss 无出路登记`);
      }
    }
    return { total: this.chains.size, closed, open, violations };
  }

  size(): number { return this.chains.size; }
}

/* ------------------------------- 体验日志埋点 ------------------------------- */

/** 链路节点 → 十三章体验日志（u3ExpLog 统一时间轴——三链路贯通的落点）。 */
export function chainNodeToExpLog(chain: NotifChain, n: ChainNode): ExpEvent {
  return u3ExpLog.record({
    atMs: n.atMs,
    domain: "f516",
    action: `notif-${n.layer}-${n.action}`,
    targetId: ExpLogTarget.sanitize(chain.chainId),
    latencyMs: n.latencyMs,
    verdict: n.verdict,
  });
}

/** 链 id 脱敏（十三章：目标 id 不含用户内容——id 是系统生成的，仍过同一闸门）。 */
export const ExpLogTarget = { sanitize: (raw: string): string => (/(password|secret|密码)/i.test(raw) ? "target:redacted" : raw) };

/* ------------------------------- 自检 ------------------------------- */

export function notifchainSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 层路由判据：常态横幅、锁屏接力
  checks.push({ name: "F516 常态路由中心+横幅", pass: routeFor(false).join("/") === "center/banner" });
  checks.push({ name: "F507 锁屏路由改接力", pass: routeFor(true).join("/") === "center/lockscreen" });

  // 完整链路：中心入 → 横幅 show → 用户 click 关
  const log = new NotifChainLog();
  const c1 = log.open(false, 1000);
  log.node(c1.chainId, { layer: "banner", action: "show", atMs: 1050, latencyMs: 50, exit: null });
  log.node(c1.chainId, { layer: "banner", action: "click", atMs: 2000, latencyMs: 10, exit: "user-click" });
  checks.push({ name: "十三章 完整链路整链顺畅", pass: c1.finalVerdict === "smooth" && c1.nodes.length === 3 });

  // 超时过期 = interrupted（链尾结论）
  const log2 = new NotifChainLog();
  const c2 = log2.open(false, 0);
  log2.node(c2.chainId, { layer: "banner", action: "expire", atMs: 6000, latencyMs: 0, exit: "timeout" });
  checks.push({ name: "十三章 过期链被打断结论", pass: c2.finalVerdict === "interrupted" });

  // 孤儿显性化：不经中心的横幅事件被拒
  const log3 = new NotifChainLog();
  checks.push({ name: "十三章 孤儿事件显性拒绝", pass: log3.orphanRejected("banner", "show") });

  // 层路由违规显性化：锁屏链进横幅层即红
  const log4 = new NotifChainLog();
  const c4 = log4.open(true, 0);
  let rejected = false;
  try { log4.node(c4.chainId, { layer: "banner", action: "show", atMs: 1, latencyMs: 0, exit: null }); } catch { rejected = true; }
  checks.push({ name: "F507 锁屏链横幅违规被拒", pass: rejected && c4.expected.includes("lockscreen") && !c4.expected.includes("banner") });

  // 审计：closed/open 计数与出路登记
  const log5 = new NotifChainLog();
  const c5 = log5.open(false, 0);
  log5.node(c5.chainId, { layer: "banner", action: "show", atMs: 10, latencyMs: 10, exit: null });
  const c6 = log5.open(false, 20);
  log5.node(c6.chainId, { layer: "banner", action: "show", atMs: 30, latencyMs: 10, exit: null });
  log5.node(c6.chainId, { layer: "banner", action: "dismiss", atMs: 50, latencyMs: 5, exit: "user-click" });
  const audit = log5.audit();
  checks.push({ name: "十三章 审计 closed/open 计数", pass: audit.total === 2 && audit.closed === 1 && audit.open === 1 && audit.violations.length === 0 });

  // 埋点贯通：节点落到 u3ExpLog 统一时间轴（domain=f516、action 带层）
  const before = u3ExpLog.size();
  chainNodeToExpLog(c1, c1.nodes[1]!);
  const after = u3ExpLog.exportAll();
  checks.push({ name: "十三章 埋点入统一时间轴", pass: u3ExpLog.size() === before + 1 && after[after.length - 1]?.action === "notif-banner-show" && after[after.length - 1]?.domain === "f516" });

  // 隐私：可疑 id 打码
  checks.push({ name: "十三章 链 id 脱敏", pass: ExpLogTarget.sanitize("n1-password") === "target:redacted" });

  return checks;
}
