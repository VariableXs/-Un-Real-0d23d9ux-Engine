import { afterEach, describe, expect, it } from "vitest";
import {
  degradeCapMissing,
  degradeMessage,
  loadPerfBaseline,
  MISSING_AS_CODE,
  perfP95,
  savePerfBaseline,
  type PerfBaseline,
} from "../perfBaseline";

describe("任务 29 · 降级提示 i18n（zh/en 全量，禁裸错误码）", () => {
  it("七个 ShimErrorCode 全部有词条映射（无遗漏分支）", () => {
    const codes = [
      "SHIM_BACKEND_DOWN",
      "SHIM_UNSUPPORTED",
      "SHIM_MISSING",
      "SHIM_TIMEOUT",
      "SHIM_VERSION_MISMATCH",
      "SHIM_PERM_DENIED",
      "SHIM_INTERNAL",
      "SHIM_INVALID_ARGS",
    ] as const;
    for (const code of codes) {
      const zh = degradeMessage("zh", code, { cmd: "kv_get", front: 1, back: 2 });
      const en = degradeMessage("en", code, { cmd: "kv_get", front: 1, back: 2 });
      expect(zh, `${code} zh`).not.toMatch(/^degrade/);
      expect(en, `${code} en`).not.toMatch(/^degrade/);
      expect(zh.length).toBeGreaterThan(6);
      expect(en.length).toBeGreaterThan(6);
    }
    // {cmd} 插值只在「命令级」词条出现（unsupported/missing）；全局级词条不含 cmd
    expect(degradeMessage("zh", "SHIM_UNSUPPORTED", { cmd: "kv_get" })).toContain("kv_get");
    expect(degradeMessage("en", "SHIM_MISSING", { cmd: "kv_get" })).toContain("kv_get");
  });

  it("zh 与 en 双语落词（同 key 两语非空且不等价于 key 本身）", () => {
    expect(degradeMessage("zh", MISSING_AS_CODE)).toContain("回退");
    expect(degradeMessage("en", MISSING_AS_CODE)).toContain("fallback");
  });

  it("能力位缺失提示带能力名", () => {
    expect(degradeCapMissing("zh", "kvStore")).toContain("kvStore");
    expect(degradeCapMissing("en", "kvStore")).toContain("kvStore");
  });
});

describe("任务 29 · 性能采样基线（首帧/交互延迟，localStorage 持久化）", () => {
  afterEach(() => localStorage.clear());

  it("空基线：无持久化数据时 firstFrameMs=null、样本空", () => {
    expect(loadPerfBaseline()).toEqual({ firstFrameMs: null, interactionSamples: [], recordedAt: null });
  });

  it("保存/读回 + 坏数据回落空基线（绝不带病持久化）", () => {
    const b: PerfBaseline = { firstFrameMs: 321.5, interactionSamples: [10, 20, 30], recordedAt: 1234 };
    savePerfBaseline(b);
    expect(loadPerfBaseline().firstFrameMs).toBe(321.5);
    localStorage.setItem("variable:perf:baseline:v1", "{broken json");
    expect(loadPerfBaseline()).toEqual({ firstFrameMs: null, interactionSamples: [], recordedAt: null });
    localStorage.setItem("variable:perf:baseline:v1", JSON.stringify({ firstFrameMs: "x", interactionSamples: [1, "y", -5, NaN] }));
    const healed = loadPerfBaseline();
    expect(healed.firstFrameMs).toBeNull();
    expect(healed.interactionSamples).toEqual([1]);
  });

  it("P95 口径与样本钳制一致（线性插值）", () => {
    expect(perfP95([])).toBe(0);
    expect(perfP95([40])).toBe(40);
    expect(perfP95([10, 20, 30, 40])).toBe(38.5);
  });
});
