// AURORA-10000：AI-01~AI-05 批次，勿删。
// ceremonyFx.ts — 族0019 关机仪式 / 族0020 睡眠唤醒剧场 / 族0025 自检报告卡
// / 族0022 无障碍开机播报 / 族0024 彩蛋触发器的运行时入口。
// 全部 DOM 直挂（不依赖 React 宿主），样式走 boot-theater.css 令牌类；
// 参数档由 params.ts 提供，档位 → 动画差异在 buildCeremonyStyle 内派生。

import { findTheaterItem } from "./registry";
import { itemParams, reportStyle, a11yBootFlags } from "./params";

const CEREMONY_MS = 1400;

function buildCeremonyStyle(id: string): Partial<CSSStyleDeclaration> {
  const p = itemParams(id);
  const color = `hsl(${p.hue} 80% 62%)`;
  return {
    borderColor: color,
    boxShadow: `0 0 ${Math.round(24 * p.intensity)}px ${color}`,
    animationDuration: `${(CEREMONY_MS * p.durScale / 1000).toFixed(2)}s`,
  };
}

function mountCeremony(id: string, wake: boolean): () => void {
  const item = findTheaterItem(id);
  if (!item) return () => undefined;
  const root = document.createElement("div");
  root.className = `bt-ceremony${wake ? " bt-ceremony--wake" : ""}`;
  root.setAttribute("role", "status");
  root.setAttribute("aria-label", wake ? `唤醒仪式：${item.name}` : `关机仪式：${item.name}`);
  const core = document.createElement("div");
  core.className = "bt-ceremony-core";
  Object.assign(core.style, buildCeremonyStyle(id));
  root.appendChild(core);
  document.body.appendChild(root);
  const timer = window.setTimeout(() => root.remove(), CEREMONY_MS * 2);
  return () => {
    window.clearTimeout(timer);
    root.remove();
  };
}

/** 族0019 关机/重启仪式：展示一次所选档位的关机动画。 */
export function showShutdownCeremony(id: string): () => void {
  return mountCeremony(id, false);
}

/** 族0020 睡眠唤醒剧场：展示一次所选档位的唤醒动画。 */
export function showWakeCeremony(id: string): () => void {
  return mountCeremony(id, true);
}

/* ---------- 族0025 自检报告卡 ---------- */

export interface ReportCardData {
  records: number;
  mindmaps: number;
  mediaFiles: number;
  nodes: number;
  elapsedMs: number;
  version: string;
}

/** 组装报告卡行数据（档位只影响版式与题头，行内容为真实统计）。 */
export function buildReportCard(id: string, data: ReportCardData): { title: string; layout: string; rows: [string, string][] } {
  const style = reportStyle(id);
  const secs = (data.elapsedMs / 1000).toFixed(1);
  return {
    title: style.masthead,
    layout: style.layout,
    rows: [
      ["记录", String(data.records)],
      ["思维导图", String(data.mindmaps)],
      ["媒体文件", String(data.mediaFiles)],
      ["节点", String(data.nodes)],
      ["耗时", `${secs}s`],
      ["版本", data.version],
    ],
  };
}

/* ---------- 族0022 无障碍开机：aria 播报 ---------- */

/** 按 a11y 档位向读屏播报阶段完成（announce 档才发；plainLanguage 档用大白话）。 */
export function announceBootStage(id: string, stage: string, done: boolean): void {
  const flags = a11yBootFlags(id);
  if (!flags.announce) return;
  const text = flags.plainLanguage ? `${stage}好了` : `${stage}${done ? "完成" : "进行中"}`;
  const el = document.createElement("div");
  el.className = "visually-hidden";
  el.setAttribute("aria-live", "polite");
  el.textContent = text;
  document.body.appendChild(el);
  window.setTimeout(() => el.remove(), 1000);
}

/* ---------- 族0024 彩蛋：秘技（上上下下）触发器 ---------- */

/** Konami 序列（F00591）：宿主在既有按键处理中调用 advanceKonami 推进状态，
 *  零新增裸 keydown 监听（Z-08 键位纪律）；命中后由宿主调用 showEggOverlay。 */
export const KONAMI_SEQ: readonly string[] = [
  "arrowup", "arrowup", "arrowdown", "arrowdown",
  "arrowleft", "arrowright", "arrowleft", "arrowright",
];

export const KONAMI_LEN = KONAMI_SEQ.length;

/** 推进秘技状态：返回新序号（命中后归零）。key 须为小写化的 e.key。 */
export function advanceKonami(idx: number, key: string): number {
  const k = key.toLowerCase();
  if (k === KONAMI_SEQ[idx]) {
    return idx + 1 === KONAMI_LEN ? 0 : idx + 1;
  }
  return k === KONAMI_SEQ[0] ? 1 : 0;
}

export function isKonamiHit(prevIdx: number, nextIdx: number): boolean {
  return prevIdx === KONAMI_LEN - 1 && nextIdx === 0;
}

/** 彩蛋命中的一次性装饰浮层（自动淡出，不阻塞交互）。 */
export function showEggOverlay(id: string): () => void {
  const item = findTheaterItem(id);
  if (!item) return () => undefined;
  const p = itemParams(id);
  const root = document.createElement("div");
  root.className = "bt-egg bt-egg--pop";
  root.style.color = `hsl(${p.hue} 85% 70%)`;
  root.style.animationDuration = `${(2 * p.durScale).toFixed(2)}s`;
  root.setAttribute("role", "status");
  root.setAttribute("aria-label", `彩蛋：${item.name}`);
  root.textContent = "✦";
  document.body.appendChild(root);
  const timer = window.setTimeout(() => root.remove(), 2600);
  return () => {
    window.clearTimeout(timer);
    root.remove();
  };
}
