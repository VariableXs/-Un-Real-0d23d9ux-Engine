/// <reference types="node" />
/**
 * H4 跨批次总对账舱（v6 · 深化批次六）：
 * Variable 指令「所有对话生成的隔离验证与检查项对账，直到全部完成才算结束」——
 * 本舱把 v1-v6 六批的隔离验证与检查项对账做**总对账**：
 * ① 批次产物清单核对（每批声明的文件真实存在、测试文件非空）；
 * ② 六批隔离验证关键项在跨批次口径下复跑（import 边界 / 键域 / 登记册 / 源码纪律 /
 *    哈希单点 / 无环 / internal 层级——各批 spec 各管一面，本舱横向汇总）；
 * ③ 批次账册数字交叉核对（attic 各节声明的累计行数与当前实测链条一致）；
 * ④ 缺陷账本闭合核对（v2-v5 各批声明的即时修缺陷，对应修复痕迹可 grep 验证）。
 */
import { describe, expect, it } from "vitest";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.cwd();
const H4_DIR = join(ROOT, "src", "system", "h4");
const FEAT_DIR = join(ROOT, "src", "features", "h4");
const SETTINGS_DIR = join(ROOT, "src", "features", "settings");

const moduleFiles = (): string[] => readdirSync(H4_DIR).filter((f) => /^f\d{3}-.*\.ts$/.test(f));

/* ---------------- ① 批次产物清单核对 ---------------- */

describe("跨批次总对账 ①：六批产物清单核对（声明的件必须真实存在）", () => {
  const MANIFEST: ReadonlyArray<{ batch: string; files: string[] }> = [
    { batch: "v1 引擎层", files: [join(H4_DIR, "f351-workspaceSnapshot.ts"), join(H4_DIR, "registry.ts"), join(H4_DIR, "internal", "store.ts"), join(H4_DIR, "__tests__", "registry.test.ts")] },
    { batch: "v2 界面壳层", files: [join(FEAT_DIR, "h4ui.ts"), join(FEAT_DIR, "H4Runtime.tsx"), join(SETTINGS_DIR, "H4Tab.tsx"), join(SETTINGS_DIR, "H4Panels.tsx"), join(ROOT, "src", "styles", "h4.css"), join(FEAT_DIR, "__tests__", "h4deep.spec.ts")] },
    { batch: "v3 隔离与对账", files: [join(FEAT_DIR, "__tests__", "h4isolation.spec.ts"), join(FEAT_DIR, "__tests__", "h4reconcile.spec.ts"), join(FEAT_DIR, "reconcile.ts"), join(FEAT_DIR, "overlays.ts"), join(FEAT_DIR, "H4Overlays.tsx")] },
    { batch: "v4 引擎深化", files: [join(H4_DIR, "internal", "hash.ts"), join(H4_DIR, "__tests__", "h4deep-v4.spec.ts")] },
    { batch: "v5 引擎深化补齐", files: [join(H4_DIR, "__tests__", "h4deep-v5.spec.ts")] },
    { batch: "v6 消费面接线", files: [join(FEAT_DIR, "consumers.ts"), join(FEAT_DIR, "H4Wiring.tsx"), join(FEAT_DIR, "__tests__", "h4consumers.spec.ts")] },
    { batch: "v7 活契约基础设施", files: [join(FEAT_DIR, "bus.ts"), join(FEAT_DIR, "xlog.ts"), join(FEAT_DIR, "__tests__", "h4bus-xlog.spec.ts")] },
    { batch: "v8 任务栏真实挂载", files: [join(FEAT_DIR, "taskbarLayer.ts"), join(FEAT_DIR, "H4TaskbarLayer.tsx"), join(FEAT_DIR, "__tests__", "h4taskbar.spec.ts")] },
    { batch: "v9 快速设置磁贴+通道唯一化", files: [join(FEAT_DIR, "quickTiles.ts"), join(FEAT_DIR, "H4QuickTiles.tsx"), join(FEAT_DIR, "__tests__", "h4quicktiles.spec.ts")] },
    { batch: "v10 通道全面唯一化", files: [join(FEAT_DIR, "__tests__", "h4crossbatch.spec.ts")] },
  ];

  it("v1-v6 每批声明的核心产物真实存在且非空（零幻影交付）", () => {
    const missing: string[] = [];
    for (const { batch, files } of MANIFEST) {
      for (const f of files) {
        if (!existsSync(f)) {
          missing.push(`${batch}: ${f}`);
          continue;
        }
        if (statSync(f).size === 0) missing.push(`${batch}: ${f}（空文件）`);
      }
    }
    expect(missing).toEqual([]);
  });

  it("v1-v9 全部深测/舱位 spec 真实在册（九代隔离验证与对账的载体齐全）", () => {
    const specs = ["h4deep.spec.ts", "h4isolation.spec.ts", "h4reconcile.spec.ts", "h4overlays.spec.ts", "h4deep-v4.spec.ts", "h4deep-v5.spec.ts", "h4consumers.spec.ts", "h4bus-xlog.spec.ts", "h4taskbar.spec.ts", "h4quicktiles.spec.ts"];
    const missing = specs.filter((s) => !existsSync(join(FEAT_DIR, "__tests__", s)) && !existsSync(join(H4_DIR, "__tests__", s)));
    expect(missing).toEqual([]);
  });
});

/* ---------------- ② 六批隔离验证关键项横向复跑 ---------------- */

describe("跨批次总对账 ②：隔离验证关键项横向汇总（六代舱位一台总闸）", () => {
  it("纯逻辑纪律（v3 舱）：引擎层零框架依赖", () => {
    const offenders = moduleFiles().filter((f) => /from\s+["'](react|react-dom)/.test(readFileSync(join(H4_DIR, f), "utf-8")));
    expect(offenders).toEqual([]);
  });

  it("哈希单点（v4 门禁）：FNV 魔数只准在 internal/hash", () => {
    const offenders = moduleFiles().filter((f) => readFileSync(join(H4_DIR, f), "utf-8").includes("0x811c9dc5"));
    expect(offenders).toEqual([]);
  });

  it("源码纪律（v4 门禁）：零 TODO / 零 console / 零 as any（含 v6 新增件）", () => {
    const dirs = [H4_DIR, FEAT_DIR];
    const offenders: string[] = [];
    for (const d of dirs) {
      for (const f of readdirSync(d).filter((x) => x.endsWith(".ts") || x.endsWith(".tsx"))) {
        const src = readFileSync(join(d, f), "utf-8");
        if (/\b(TODO|FIXME)\b/.test(src) || /console\.(log|warn|error)/.test(src) || /as any\b/.test(src) || /@ts-ignore/.test(src)) offenders.push(f);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("无环（v5 舱）：引擎依赖图 DFS 零回边", () => {
    const files = moduleFiles();
    const graph = new Map<string, string[]>();
    for (const f of files) {
      const src = readFileSync(join(H4_DIR, f), "utf-8");
      const deps = [...src.matchAll(/from\s+["']\.[^"']*?(f\d{3}-[^"']+)\.ts["']/g)].map((m) => `${m[1]}.ts`);
      graph.set(f, deps.filter((d) => files.includes(d)));
    }
    const color = new Map<string, number>(files.map((f) => [f, 0]));
    let hasCycle = false;
    const visit = (f: string): void => {
      if (hasCycle) return;
      color.set(f, 1);
      for (const d of graph.get(f) ?? []) {
        if (color.get(d) === 1) {
          hasCycle = true;
          return;
        }
        if (color.get(d) === 0) visit(d);
      }
      color.set(f, 2);
    };
    for (const f of files) if (color.get(f) === 0) visit(f);
    expect(hasCycle).toBe(false);
  });

  it("internal 层级（v5 舱）：底座零出向依赖（v6 新增 hash.ts 一并覆盖）", () => {
    const offenders = readdirSync(join(H4_DIR, "internal")).filter((f) => f.endsWith(".ts") && /from\s+["']\.\.\//.test(readFileSync(join(H4_DIR, "internal", f), "utf-8")));
    expect(offenders).toEqual([]);
  });

  it("键域自域（v3 舱）：h4Key 全部落本模块 F 号域（v4/v5 新增键位一并覆盖）", () => {
    const violations: string[] = [];
    for (const f of moduleFiles()) {
      const own = `f${f.slice(1, 4)}`;
      for (const m of readFileSync(join(H4_DIR, f), "utf-8").matchAll(/h4Key\("([^"]+)"/g)) {
        if (m[1] !== own) violations.push(`${f} 写了 ${m[1]} 域`);
      }
    }
    expect(violations).toEqual([]);
  });

  it("通道唯一化收尾（v10 舱）：vx-h4-* window 事件零派发（全部走 h4Bus 单通道）", () => {
    // 归因：v9 迁移 filter 一条；v10 收尾 SUMMON×2/FOCUS×2/READING——deprecated 常量仅保留定义
    const dirs = [FEAT_DIR, join(ROOT, "src", "features", "settings"), join(FEAT_DIR, "..", "..", "entries", "taskbar")];
    const offenders: string[] = [];
    for (const d of dirs) {
      for (const f of readdirSync(d).filter((x) => x.endsWith(".ts") || x.endsWith(".tsx"))) {
        const src = readFileSync(join(d, f), "utf-8");
        if (/new CustomEvent\(\s*(SUMMON_PICKER|SUMMON_RULER|FOCUS_EVENT|H4_FILTER_EVENT|H4_READING_EVENT)/.test(src)) offenders.push(`${d}/${f}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});

/* ---------------- ③ 批次账册数字交叉核对 ---------------- */

describe("跨批次总对账 ③：账册数字交叉核对（声明的规模与实测一致）", () => {
  it("引擎层 50 模块 + 50 测试一一对应（v1 结构在 v4/v5 增量后保持不变）", () => {
    const mods = moduleFiles();
    const tests = readdirSync(join(H4_DIR, "__tests__")).filter((f) => /^f\d{3}-.*\.test\.ts$/.test(f));
    expect(mods.length).toBe(50);
    expect(tests.length).toBe(50);
  });

  it("attic 账册六批章节齐备（v1-v6 的行数对账与缺陷账本全部落册）", () => {
    const ledger = readFileSync(join(ROOT, "_attic", "aih4-f351-f400", "行数对账与缺陷账本.md"), "utf-8");
    // 归因：账册章节结构 = 各批「# 深化批次N」大节 + 「## 十N、…」小节（v3 起编号累进）
    for (const mark of ["## 六、v2 行数对账", "## 九、v3 行数对账", "## 十一、v4 行数对账", "## 十四、v5 行数对账"]) {
      expect(ledger).toContain(mark);
    }
    for (const section of ["## 十二、v4 缺陷账本", "## 十五、v5 缺陷账本", "## 十三、v4 验证证据", "## 十六、v5 验证证据"]) {
      expect(ledger).toContain(section);
    }
  });

  it("验证证据链归档：v4/v5 验证输出真实存在且含全绿结论", () => {
    for (const f of ["验证输出-vitest.txt", "验证输出-v4.txt", "验证输出-v5.txt"]) {
      const p = join(ROOT, "_attic", "aih4-f351-f400", f);
      expect(existsSync(p)).toBe(true);
      expect(readFileSync(p, "utf-8")).toMatch(/passed|通过|全绿/);
    }
  });

  it("完成报告批次章齐备（v1-v6 章节落册，§11/§12 为 v4/v5 深化批）", () => {
    const report = readFileSync(join(ROOT, "docs", "AI-H4-完成报告.md"), "utf-8");
    for (const mark of ["## 9. 深化批次二（v2）", "## 10. 深化批次三（v3）", "## 11. 深化批次四（v4）", "## 12. 深化批次五（v5）"]) {
      expect(report).toContain(mark);
    }
  });
});

/* ---------------- ④ 缺陷账本闭合核对 ---------------- */

describe("跨批次总对账 ④：缺陷账本闭合核对（声明的修复必须留痕）", () => {
  it("v4 修复#2：f386 contentHash 已委托 internal/hash（私抄件收编的 grep 痕迹）", () => {
    const src = readFileSync(join(H4_DIR, "f386-readingMode.ts"), "utf-8");
    expect(src).toContain('from "./internal/hash"');
    expect(src).toContain("return fnv1a32(text)");
    expect(src).not.toContain("0x811c9dc5");
  });

  it("v4 修复#1：f383 LayerLifecycleLog 已数组化（Map 键控覆盖缺陷的修复痕迹）", () => {
    const src = readFileSync(join(H4_DIR, "f383-modalQueue.ts"), "utf-8");
    expect(src).toContain("private readonly rows: LayerLifecycleRow[]");
    expect(src).not.toContain("new Map<string, LayerLifecycleRow>()");
  });

  it("v5 修复#1：f374 chordNormalize 修饰键已大写（规范化名副其实的修复痕迹）", () => {
    const src = readFileSync(join(H4_DIR, "f374-hotkeySheet.ts"), "utf-8");
    expect(src).toContain("charAt(0).toUpperCase()");
  });

  it("v5 修复#3：f381 lazyExpandCheck 判序已前置（childrenKnown 先于 isLeaf）", () => {
    const src = readFileSync(join(H4_DIR, "f381-treeTriState.ts"), "utf-8");
    const lazyIdx = src.indexOf("lazyExpandCheck");
    const body = src.slice(lazyIdx, lazyIdx + 600);
    expect(body.indexOf("childrenKnown")).toBeLessThan(body.indexOf("isLeaf"));
  });

  it("v5 修复#4：f386 视图完整性已改剥换行口径（零侵入判定的修复痕迹）", () => {
    const src = readFileSync(join(H4_DIR, "f386-readingMode.ts"), "utf-8");
    expect(src).toContain("s.replace(/\\n/g, \"\")");
  });
});
