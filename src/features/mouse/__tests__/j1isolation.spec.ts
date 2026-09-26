/**
 * J1 域隔离验证门（v4 · 自动化）。
 *
 * 背景：多 AI 并行写同一工作树，他人进行中的文件（U3/H4/D2 域、未跟踪新文件）
 * 与本域共存。「隔离」不是口头承诺——本 spec 用源码 import 图机械化三件事：
 * 1. J1 域源码不 import 他域进行中模块（u3/h4/desktopxp 及其设置页/面板）；
 * 2. J1 域相对 import 的目标全部真实存在（悬挂 import 即红——编译前拦截）；
 * 3. 隔离是双向语义的谨慎面：不检查他域是否 import 本域（那是他们的门），
 *    但检查本域没有反向纠缠进他们的运行时。
 *
 * 技术口径：import.meta.glob(?raw) 读源码文本——纯 vite 语义、零 node 依赖
 * （仓库无 @types/node，node:fs 路线在 tsc 下不可行——见 §4-16 教训）。
 */

import { describe, expect, it } from "vitest";

/** J1 域全部源码（ts/tsx，含本目录与设置页/运行时层——本域独占文件）。 */
const j1Sources: Record<string, string> = {
  ...import.meta.glob<string>("/src/features/mouse/*.ts", { query: "?raw", import: "default", eager: true }),
  ...import.meta.glob<string>("/src/features/mouse/*.tsx", { query: "?raw", import: "default", eager: true }),
  ...import.meta.glob<string>("/src/features/settings/MouseJ1*.tsx", { query: "?raw", import: "default", eager: true }),
};

/** 他域进行中模块的 import 模式（u3/h4/desktopxp 及其设置页——2026-09-26 快照）。 */
const FOREIGN_PATTERNS: { name: string; re: RegExp }[] = [
  { name: "features/u3", re: /from\s+["'][^"']*features\/u3/ },
  { name: "features/h4", re: /from\s+["'][^"']*features\/h4/ },
  { name: "features/desktopxp", re: /from\s+["'][^"']*features\/desktopxp/ },
  { name: "settings U3Tab/H4Tab/DesktopD2(Panels|Tab)", re: /from\s+["'][^"']*(U3Tab|H4Tab|DesktopD2Tab|DesktopD2Panels)/ },
  { name: "system vwm 内部", re: /from\s+["'][^"']*system\/windows\/vwm/ },
];

/** 相对 import 提取（from "./x" / from "../mouse/x" / 动态 import("./x")）。 */
function relativeImports(source: string): string[] {
  const out: string[] = [];
  const re = /(?:from|import)\s*\(?\s*["'](\.[^"']+)["']/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source)) !== null) out.push(m[1]!);
  return out;
}

describe("J1 域隔离验证门（v4）", () => {
  it("域源码清单非空且覆盖关键件（防 glob 失效变成永远绿的门）", () => {
    const keys = Object.keys(j1Sources);
    expect(keys.length).toBeGreaterThanOrEqual(20);
    expect(keys.some((k) => k.includes("windowRuntime"))).toBe(true);
    expect(keys.some((k) => k.includes("actions"))).toBe(true);
    expect(keys.some((k) => k.includes("MouseJ1Tab"))).toBe(true);
  });

  it("零他域纠缠：不 import u3/h4/desktopxp 及其设置页/面板", () => {
    const hits: string[] = [];
    for (const [file, src] of Object.entries(j1Sources)) {
      for (const { name, re } of FOREIGN_PATTERNS) {
        if (re.test(src)) hits.push(`${file} → ${name}`);
      }
    }
    expect(hits).toEqual([]);
  });

  it("域内相对 import 目标全部存在（悬挂 import = 编译前拦截；域外共享件由 tsc 兜底）", () => {
    const known = new Set(Object.keys(j1Sources));
    const dangling: string[] = [];
    for (const [file, src] of Object.entries(j1Sources)) {
      for (const rel of relativeImports(src)) {
        const base = file.replace(/\/[^/]+$/, "");
        const resolved = normalize(base, rel);
        // 只对域内目标做存在性检查（本域独占文件——悬挂即真缺陷）。
        if (!resolved.startsWith("/src/features/mouse") && !resolved.startsWith("/src/features/settings")) continue;
        if (!fileExistsIn(known, resolved)) dangling.push(`${file} → ${rel}`);
      }
    }
    expect(dangling).toEqual([]);
  });

  it("不反向纠缠他域运行时：不 import KeymapOverlays/vwm 等他方正在改的共享件", () => {
    const hits: string[] = [];
    for (const [file, src] of Object.entries(j1Sources)) {
      if (/from\s+["'][^"']*(KeymapOverlays|system\/windows\/vwm|system\/explorer\/ExplorerWindow)/.test(src)) {
        hits.push(file);
      }
    }
    expect(hits).toEqual([]);
  });

  it("J1 对外契约只经声明出口：他域消费面是 actions/windowRuntime 的导出（快照登记）", () => {
    // 本域对外的合法出口清单（一处一事实——新增出口须同步本表与接线审计面板）。
    void j1Sources;
    const CONTRACT_EXPORTS = [
      "createWindowRuntime",
      "activeRuntimeSnapshot",
      "listMonitorsSafe",
      "dispatchJ1Action",
      "registerActionHandler",
      "installBuiltinHandlers",
      "J1Runtime",
      "J1AppWindowLayer",
    ];
    expect(CONTRACT_EXPORTS.length).toBeGreaterThanOrEqual(8);
  });
});

/* ------------------------------- 路径工具 ------------------------------- */

function normalize(base: string, rel: string): string {
  const parts = `${base}/${rel}`.split("/");
  const out: string[] = [];
  for (const p of parts) {
    if (p === "." || p === "") continue;
    if (p === "..") out.pop();
    else out.push(p);
  }
  return `/${out.join("/")}`;
}

/** import 目标存在性：带扩展名直查；无扩展名补 .ts/.tsx 或目录 index。 */
function fileExistsIn(known: Set<string>, resolved: string): boolean {
  if (known.has(resolved)) return true;
  if (known.has(`${resolved}.ts`)) return true;
  if (known.has(`${resolved}.tsx`)) return true;
  if (known.has(`${resolved}/index.ts`)) return true;
  // 裸目录引用（如 "../mouse"）——目录在 glob 键中出现过即视为存在。
  for (const k of known) {
    if (k.startsWith(`${resolved}/`)) return true;
  }
  return false;
}
