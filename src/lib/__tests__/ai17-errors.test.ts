import { describe, it, expect } from "vitest";
import { classifyError, narrateError, backoffRetry } from "../errors";

describe("AI-17 errors（U-56）", () => {
  it("十类常见错误全部命中词典（不落入 unknown）", () => {
    const cases: [string, string][] = [
      ["文件正被另一个程序使用", "file_in_use"],
      ["Access is denied", "access_denied"],
      ["There is not enough space on the disk", "disk_full"],
      ["路径过长 path too long", "path_too_long"],
      ["network unreachable 断网了", "network_unreachable"],
      ["The system cannot find the file specified", "not_found"],
      ["data is corrupted 数据已损坏", "corrupted"],
      ["operation timed out 超时", "timeout"],
      ["media is write protected 只读", "read_only"],
      ["401 unauthorized 未授权", "unauthorized"],
    ];
    for (const [raw, code] of cases) {
      expect(classifyError(raw), raw).toBe(code);
    }
  });

  it("未知错误走通用模板并标记未收录（不裸抛 error code）", () => {
    const n = narrateError("zzz-weird-0x8712", "zh");
    expect(n.unknown).toBe(true);
    expect(n.narrative.what).toBe("发生了一个问题");
    expect(n.detail.raw).toBe("zzz-weird-0x8712");
  });

  it("叙事四段式完整（what/impact/actions/detail）且中英双语可切换", () => {
    const zh = narrateError("disk full 空间不足", "zh");
    expect(zh.narrative.what).toContain("空间");
    expect(zh.narrative.actions.length).toBeGreaterThanOrEqual(2);
    const en = narrateError("disk full 空间不足", "en");
    expect(en.narrative.whatEn).toMatch(/space/i);
  });

  it("退避重试：默认 1s/3s/9s 三次退避后成功", async () => {
    const delays: number[] = [];
    let attempts = 0;
    const sleep = async (ms: number): Promise<void> => {
      delays.push(ms);
    };
    const result = await backoffRetry(
      async () => {
        attempts++;
        if (attempts < 4) throw new Error("timeout");
        return "ok";
      },
      { sleep, onProgress: () => {} },
    );
    expect(result).toBe("ok");
    expect(attempts).toBe(4);
    expect(delays).toEqual([1000, 3000, 9000]);
  });

  it("不可重试错误立即抛出（shouldRetry=false）", async () => {
    let attempts = 0;
    await expect(
      backoffRetry(
        async () => {
          attempts++;
          throw new Error("path too long");
        },
        { shouldRetry: () => false, sleep: async () => {} },
      ),
    ).rejects.toThrow("path too long");
    expect(attempts).toBe(1);
  });

  it("重试耗尽后抛出最后一次错误", async () => {
    await expect(
      backoffRetry(async () => { throw new Error("always fails"); }, { delays: [1, 1, 1], sleep: async () => {} }),
    ).rejects.toThrow("always fails");
  });
});
