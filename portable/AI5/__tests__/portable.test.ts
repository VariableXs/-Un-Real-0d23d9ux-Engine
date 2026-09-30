/**
 * AI-5 交付核 / portable 套件的回归测试
 *
 * 为什么放在这里：仓库 CI 的 frontend 作业已经在 windows-latest 上跑 `npm test`，
 * 而本会话的 GitHub App 令牌没有 `workflows` 权限、无法新增 CI 作业。
 * 把自检挂进 vitest，就能借现有 CI 在真 PowerShell 上执行 portable/tests/Run-PortableTests.ps1。
 *
 * 两组测试：
 *   A. 数据不变量 —— 任何平台都跑，直接读仓库里的 JSON 断言（不是替身，读的是交付物本身）。
 *   B. PowerShell 自检 —— 仅在 Windows 上真跑 pwsh/powershell；非 Windows 明确 skip 并说明原因。
 */
import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const HERE = dirname(fileURLToPath(import.meta.url));
const AI5 = resolve(HERE, "..");
const PORTABLE = resolve(AI5, "..");
const MATRIX = join(AI5, "Data", "compat-matrix.json");
const CHAOS = join(AI5, "Data", "chaos-scenarios.json");

type Verdict = "pass" | "warn" | "fail" | "todo";
interface MatrixApp {
  id: string;
  category: string;
  name: string;
  aMode: Verdict;
  bMode: Verdict;
}
interface Matrix {
  budget: Record<string, number>;
  summary: { total: number };
  apps: MatrixApp[];
}
interface Scenario {
  id: string;
  name: string;
  automatable: "auto" | "manual";
  dangerous: boolean;
  steps: string[];
  expect: string;
}

const readJson = <T,>(p: string): T => JSON.parse(readFileSync(p, "utf8")) as T;

describe("AI-5 兼容矩阵数据 (主计划 11.1)", () => {
  const m = readJson<Matrix>(MATRIX);
  const VERDICTS: Verdict[] = ["pass", "warn", "fail", "todo"];

  it("共 200 条，且 summary 与实际一致", () => {
    expect(m.apps).toHaveLength(200);
    expect(m.summary.total).toBe(m.apps.length);
  });

  it("5 类 × 40 条", () => {
    const byCat = new Map<string, number>();
    for (const a of m.apps) byCat.set(a.category, (byCat.get(a.category) ?? 0) + 1);
    expect(byCat.size).toBe(5);
    for (const [cat, n] of byCat) expect(n, `类别 ${cat}`).toBe(40);
  });

  it("应用名与 id 全局唯一", () => {
    expect(new Set(m.apps.map((a) => a.name)).size).toBe(200);
    expect(new Set(m.apps.map((a) => a.id)).size).toBe(200);
  });

  it("A/B 判定只允许 pass/warn/fail/todo", () => {
    for (const a of m.apps) {
      expect(VERDICTS, `${a.name} aMode=${a.aMode}`).toContain(a.aMode);
      expect(VERDICTS, `${a.name} bMode=${a.bMode}`).toContain(a.bMode);
    }
  });

  it("主计划 11.1 点名的条目都在列", () => {
    const names = new Set(m.apps.map((a) => a.name));
    for (const must of ["WPS Office", "微信", "Blender", "Adobe Photoshop", "Visual Studio 2022", "Trae CN", "Steam"]) {
      expect(names.has(must), must).toBe(true);
    }
  });

  it("预算字段齐全（供 Bench-Perf/Accept-Gate 判定）", () => {
    for (const k of ["coldStartSec", "hotStartSec", "systemBootSec", "crashRecoverSec", "pnpSwitchSec"]) {
      expect(typeof m.budget[k], k).toBe("number");
    }
    expect(m.budget.hotStartSec).toBe(6); // 主计划 1.3：大软件 6 秒
  });

  it("未实测的条目不得被标成 pass（防假勾）", () => {
    // 只有主计划 11.1 / 16.2 点名过的 14 条允许带既有结论，其余必须 todo
    const measured = m.apps.filter((a) => a.aMode !== "todo");
    expect(measured.length).toBe(14);
  });
});

describe("AI-5 混沌场景数据 (主计划 11.2 / 扩充 21)", () => {
  const doc = readJson<{ scenarios: Scenario[] }>(CHAOS);
  const sc = doc.scenarios;

  it("至少覆盖扩充 21 的 10 个必测场景", () => {
    expect(sc.length).toBeGreaterThanOrEqual(10);
  });

  it("场景 id 唯一且字段完整", () => {
    expect(new Set(sc.map((s) => s.id)).size).toBe(sc.length);
    for (const s of sc) {
      expect(["auto", "manual"], s.id).toContain(s.automatable);
      expect(s.steps.length, s.id).toBeGreaterThan(0);
      expect(s.expect, s.id).toBeTruthy();
    }
  });

  it("危险场景一律 manual —— 脚本绝不代为执行", () => {
    for (const s of sc) {
      if (s.dangerous) expect(s.automatable, `${s.id} ${s.name}`).toBe("manual");
    }
  });

  it("包含主计划 11.2 要求的 0x80000003 注入场景", () => {
    const raw = readFileSync(CHAOS, "utf8");
    expect(raw).toContain("0x80000003");
    expect(sc.some((s) => s.automatable === "auto" && /0x80000003/.test(s.name + s.expect))).toBe(true);
  });
});

describe("AI-5 PowerShell 套件自检 (真 pwsh 执行)", () => {
  const isWindows = process.platform === "win32";
  const selfTest = join(PORTABLE, "tests", "Run-PortableTests.ps1");

  it("自检脚本存在", () => {
    expect(existsSync(selfTest)).toBe(true);
  });

  // 非 Windows 上 PowerShell 不存在，如实 skip；CI 的 windows-latest 会真跑。
  const run = isWindows ? it : it.skip;

  run(
    "Run-PortableTests.ps1 在真 PowerShell 上通过（AST 语法 + 数据不变量 + 只读动作）",
    () => {
      const shells = ["pwsh", "powershell"];
      // 逐个 shell 记录失败原因。旧实现只用 lastErr 覆盖，导致 pwsh(7) 的真实报错
      // 被 powershell(5.1) 的报错顶掉——排障时看到的是最后一个 shell，而不是根因。
      const failures: string[] = [];
      for (const sh of shells) {
        try {
          // -ExecutionPolicy Bypass：Windows 客户端默认 Restricted，不带此参数时
          // 脚本会被执行策略拒绝（UnauthorizedAccess）。仅对本子进程生效。
          execFileSync(sh, ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", selfTest], {
            stdio: "pipe",
            timeout: 600_000,
            encoding: "utf8",
          });
          return; // 任一 PowerShell 跑通即通过
        } catch (e) {
          const err = e as { stdout?: string; stderr?: string; message?: string };
          const detail = [err.stdout, err.stderr, err.message].filter(Boolean).join("\n");
          failures.push(`--- ${sh} 失败 ---\n${detail}`);
        }
      }
      throw new Error(
        `PowerShell 自检在所有可用 shell 上均未通过（${shells.length} 个）：\n\n` +
          failures.join("\n\n").slice(-6000),
      );
    },
    600_000,
  );

  it.skipIf(isWindows)("非 Windows 环境已跳过 PowerShell 执行（无 pwsh 可用）", () => {
    expect(isWindows).toBe(false);
  });
});
