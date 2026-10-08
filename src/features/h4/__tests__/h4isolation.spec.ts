/// <reference types="node" />
/**
 * H4 隔离验证舱（v3 · 隔离验证与检查项对账）：
 * 引擎层隔离不变量的机检舱位——对齐 V2 isolation / U1 cabin 先例形态（TS 版）：
 * ① import 边界：引擎层纯逻辑纪律（零 react/零框架、零越境 import，
 *    跨模块依赖只允许登记在案的声明项）；
 * ② store 键域：每个模块只写自己的 F 号域（h4Key 自域纪律），
 *    全域键位零碰撞，统一挂 variable:h4: 命名域；
 * ③ 登记册对账：registry 的模块/测试文件必须真实存在于磁盘，
 *    50 模块 ↔ 50 测试一一对应，册与一行账（f400.H4_TITLES）同源。
 * 任何新增模块违反以上任一条，本舱红灯——隔离性是结构性质，不是口头承诺。
 */

import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { H4_REGISTRY } from "../../../system/h4/registry";
import { H4_TITLES } from "../../../system/h4/f400-hDomainClosure";

const H4_DIR = join(process.cwd(), "src", "system", "h4");

/** 已登记的跨模块依赖白名单（一处一事实：新增依赖必须先在此登记并说明理由）。 */
const ALLOWED_CROSS_IMPORTS: ReadonlyArray<{ from: string; to: string; reason: string }> = [
  { from: "f376-systemMenuMatrix.ts", to: "../windows/systemMenu", reason: "F376 复用 V-21 SystemMenuItemId 类型（批次一报告 §5 登记项）" },
  { from: "f393-storageTreemap.ts", to: "./f392-folderSize", reason: "F393 treemap 直接消费 F392 measureDir（同源计量，一处一事实）" },
  { from: "f391-textTranslate.ts", to: "./f390-wordLookup", reason: "F391 判据明文「浮卡同形制复用 F390」——re-export openCard/outsideClick（批次一报告 §5 登记项）" },
];

function moduleFiles(): string[] {
  return readdirSync(H4_DIR).filter((f) => /^f\d{3}-.*\.ts$/.test(f));
}

function testFiles(): string[] {
  return readdirSync(join(H4_DIR, "__tests__")).filter((f) => /^f\d{3}-.*\.test\.ts$/.test(f));
}

/* ---------------- ① import 边界 ---------------- */

describe("隔离舱 ①：引擎层 import 边界（纯逻辑纪律）", () => {
  it("引擎模块零框架依赖（不 import react/tsx/样式——纯逻辑层结构保证）", () => {
    const offenders: string[] = [];
    for (const f of moduleFiles()) {
      const src = readFileSync(join(H4_DIR, f), "utf-8");
      if (/\bfrom\s+["'](react|react-dom|.*\.css)["']/.test(src) || /\.tsx$/.test(f)) {
        offenders.push(f);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("引擎模块 import 全部在登记白名单内（内部底座 + 已声明跨模块依赖）", () => {
    const violations: string[] = [];
    for (const f of moduleFiles()) {
      const src = readFileSync(join(H4_DIR, f), "utf-8");
      const imports = [...src.matchAll(/(?:import|export)\s+(?:type\s+)?(?:\{[^}]*\}|\*\s+as\s+\w+|\w+)?\s*(?:type\s+)?from\s+["']([^"']+)["']/g)].map((m) => m[1]!);
      for (const imp of imports) {
        const internal = imp.startsWith("./internal/");
        const own = imp.startsWith("./") && imp.includes(f.slice(0, 6)); // 不应发生（自身不 import 自己）
        const allowed = ALLOWED_CROSS_IMPORTS.some((a) => a.from === f && a.to === imp);
        if (!internal && !allowed) violations.push(`${f} → ${imp}${own ? "（自引用）" : ""}`);
      }
    }
    expect(violations).toEqual([]);
  });

  it("白名单依赖均为 type-only 或已声明的同源消费（抽查语义）", () => {
    const f376 = readFileSync(join(H4_DIR, "f376-systemMenuMatrix.ts"), "utf-8");
    expect(f376).toContain('import type { SystemMenuItemId } from "../windows/systemMenu"');
    const f393 = readFileSync(join(H4_DIR, "f393-storageTreemap.ts"), "utf-8");
    expect(f393).toContain('from "./f392-folderSize"');
  });
});

/* ---------------- ② store 键域 ---------------- */

describe("隔离舱 ②：store 键域自域纪律与全域零碰撞", () => {
  it("每个 h4Key 都落在本模块自己的 F 号域（f3XX 文件只写 f3XX 键）", () => {
    const violations: string[] = [];
    for (const f of moduleFiles()) {
      const ownDomain = `f${f.slice(1, 4)}`;
      const src = readFileSync(join(H4_DIR, f), "utf-8");
      for (const m of src.matchAll(/h4Key\("([^"]+)"/g)) {
        if (m[1] !== ownDomain) violations.push(`${f} 写了 ${m[1]} 域`);
      }
    }
    expect(violations).toEqual([]);
  });

  it("全域键位零碰撞（module:slot 二元组唯一）", () => {
    const seen = new Map<string, string>();
    const collisions: string[] = [];
    for (const f of moduleFiles()) {
      const src = readFileSync(join(H4_DIR, f), "utf-8");
      for (const m of src.matchAll(/h4Key\("([^"]+)"(?:\s*,\s*"([^"]+)")?\)/g)) {
        const key = `${m[1]}:${m[2] ?? ""}`;
        const prev = seen.get(key);
        if (prev && prev !== f) collisions.push(`${key} 被 ${prev} 与 ${f} 同时占用`);
        seen.set(key, f);
      }
    }
    expect(collisions).toEqual([]);
    expect(seen.size).toBeGreaterThan(25); // 持久化模块的键位底数（纯状态机模块无键属正常）
  });

  it("键位统一挂 variable:h4: 命名域（h4Key 唯一入口——绕行者即隔离破口）", () => {
    const bypass: string[] = [];
    for (const f of moduleFiles()) {
      const src = readFileSync(join(H4_DIR, f), "utf-8");
      if (/["'`]variable:[^h]/.test(src) || /["'`]variable:h4:(?!)/.test(src.replace(/h4Key\(/g, ""))) {
        // 模块源码中出现裸 variable: 字符串键（不经 h4Key）即违规
        if (/["'`]variable:/i.test(src)) bypass.push(f);
      }
    }
    expect(bypass).toEqual([]);
  });
});

/* ---------------- ③ 登记册对账 ---------------- */

describe("隔离舱 ③：登记册 ↔ 磁盘 ↔ 一行账 三处对账", () => {
  it("registry 登记的 50 个模块与 50 个测试文件全部真实存在", () => {
    const missing: string[] = [];
    for (const e of H4_REGISTRY) {
      if (!existsSync(join(H4_DIR, e.module))) missing.push(e.module);
      if (!existsSync(join(H4_DIR, "__tests__", e.test))) missing.push(e.test);
    }
    expect(missing).toEqual([]);
  });

  it("磁盘上没有登记册之外的野模块（新增模块必须先进册——一处一事实）", () => {
    const registered = new Set(H4_REGISTRY.map((e) => e.module));
    const wild = moduleFiles().filter((f) => !registered.has(f));
    expect(wild).toEqual([]);
  });

  it("50 模块 ↔ 50 测试一一对应（编号挂接）", () => {
    expect(moduleFiles().length).toBe(50);
    expect(testFiles().length).toBe(50);
    for (const e of H4_REGISTRY) {
      expect(e.test.startsWith(e.item.toLowerCase())).toBe(true);
    }
  });

  it("登记册与一行账（f400.H4_TITLES）同源（项+标题逐字一致）", () => {
    expect(H4_TITLES.length).toBe(H4_REGISTRY.length);
    for (let i = 0; i < H4_REGISTRY.length; i++) {
      expect(H4_TITLES[i]).toEqual({ item: H4_REGISTRY[i]!.item, title: H4_REGISTRY[i]!.title });
    }
  });
});
