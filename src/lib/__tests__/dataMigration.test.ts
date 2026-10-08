import { describe, expect, it } from "vitest";
import {
  DoubleConfirm,
  MIGRATION_STEPS,
  advanceMigration,
  fnv1a64,
  newMigration,
  restoreMigration,
  retireReset,
  retireWipeBlock,
  saveMigration,
  verifyChecksums,
} from "../dataMigration";

describe("任务78 · I3 跨 U 盘搬家状态机", () => {
  it("四步顺序推进，全 done 才签发完成", () => {
    let s = newMigration("usbA->usbB");
    expect(s.done).toBe(false);
    for (const step of MIGRATION_STEPS) {
      s = advanceMigration(s, step, () => `${step}-ok`);
    }
    expect(MIGRATION_STEPS.every((k) => s.step[k] === "done")).toBe(true);
    expect(s.done).toBe(true);
  });

  it("幂等：已完成步重跑不重做（execute 不被调用）", () => {
    let s = newMigration("p");
    s = advanceMigration(s, "copy-shared", () => "hash-abc");
    let calls = 0;
    s = advanceMigration(s, "copy-shared", () => {
      calls += 1;
      return "other";
    });
    expect(calls).toBe(0);
    expect(s.note["copy-shared"]).toBe("hash-abc");
  });

  it("断点续走：序列化→恢复→从断点继续；错 planId 拒绝续跑", () => {
    let s = newMigration("usbA->usbB");
    s = advanceMigration(s, "remount-diff-chain", () => "ok");
    const raw = saveMigration(s);
    const back = restoreMigration(raw, "usbA->usbB");
    expect(back).not.toBeNull();
    expect(back!.step["remount-diff-chain"]).toBe("done");
    let s2 = advanceMigration(back!, "copy-shared", () => "ok2");
    expect(s2.step["copy-shared"]).toBe("done");
    // 换盘（不同 planId）= 拒绝续走
    expect(restoreMigration(raw, "usbC->usbD")).toBeNull();
    // 损坏 JSON = 拒绝
    expect(restoreMigration("{oops", "usbA->usbB")).toBeNull();
  });

  it("失败步标 failed 且可重试同一步；失败不阻断后续步推进", () => {
    let s = newMigration("p");
    s = advanceMigration(s, "remount-diff-chain", () => {
      throw new Error("diff chain missing");
    });
    expect(s.step["remount-diff-chain"]).toBe("failed");
    expect(s.note["remount-diff-chain"]).toContain("diff chain missing");
    s = advanceMigration(s, "copy-shared", () => "ok");
    expect(s.step["copy-shared"]).toBe("done");
    // 重试成功后转 done
    s = advanceMigration(s, "remount-diff-chain", () => "recovered");
    expect(s.step["remount-diff-chain"]).toBe("done");
  });

  it("一致性校验：逐键相等才 ok，缺失/异值如实列出", () => {
    expect(verifyChecksums({ a: "1", b: "2" }, { a: "1", b: "2" })).toEqual({
      ok: true,
      mismatch: [],
    });
    const r = verifyChecksums({ a: "1", b: "2" }, { a: "9", c: "3" });
    expect(r.ok).toBe(false);
    expect(r.mismatch.sort()).toEqual(["a", "b", "c"]);
  });
});

describe("任务78 · I4 退役安全清空", () => {
  it("reset 档：系统态清空、用户数据保留", () => {
    const r = retireReset(["kv/settings", "whitelist", "audit"], ["docs", "saves"]);
    expect(r.cleared.sort()).toEqual(["audit", "kv/settings", "whitelist"]);
    expect(r.kept).toEqual(["docs", "saves"]);
  });

  it("wipe 档：三轮覆写 0x00→0xFF→0xA5，轮轮摘要互异且确定性可复现", () => {
    const buf = new Uint8Array(128);
    buf.fill(0x5a);
    const w1 = retireWipeBlock(buf);
    expect(w1.passes.map((p) => p.pattern)).toEqual([0x00, 0xff, 0xa5]);
    expect(new Set(w1.passes.map((p) => p.digest)).size).toBe(3);
    // 确定性：同初值区块重跑摘要一致
    const buf2 = new Uint8Array(128);
    buf2.fill(0x5a);
    const w2 = retireWipeBlock(buf2);
    expect(w1.finalDigest).toBe(w2.finalDigest);
    // 覆写后内容确实全 A5（末轮模式）
    expect(buf.every((b) => b === 0xa5)).toBe(true);
  });

  it("fnv1a64 已知向量（与内核同族校验算法对齐）", () => {
    // fnv1a64("") = cbf29ce484222325
    expect(fnv1a64(new Uint8Array(0))).toBe("cbf29ce484222325");
    // fnv1a64("a") 标准向量
    expect(fnv1a64(new Uint8Array([0x61]))).toBe("af63dc4c8601ec8c");
  });

  it("双确认闸：一次确认不放行，两次显式确认才 granted，口令不符重置", () => {
    const dc = new DoubleConfirm("全部清空");
    expect(dc.confirm("wrong")).toBe("rejected");
    expect(dc.confirm("全部清空")).toBe("need-second");
    expect(dc.confirm("全部清空")).toBe("granted");
    // 全新闸：第二次先于第一次 → 仍需两步
    const dc2 = new DoubleConfirm("全部清空");
    expect(dc2.confirm("全部清空")).toBe("need-second");
    expect(dc2.confirm("wrong")).toBe("rejected");
    expect(dc2.confirm("全部清空")).toBe("need-second"); // 中途撤销重置
    expect(dc2.confirm("全部清空")).toBe("granted");
  });

  it("退役证书：双确认通过 + 三轮摘要齐备才签发", () => {
    const w = retireWipeBlock(new Uint8Array(32));
    expect(
      DoubleConfirm.certificate("usbX", w.passes.map((p) => p.digest), false),
    ).toBeNull();
    expect(DoubleConfirm.certificate("usbX", w.passes.slice(0, 2).map((p) => p.digest), true)).toBeNull();
    const cert = DoubleConfirm.certificate("usbX", w.passes.map((p) => p.digest), true);
    expect(cert).toEqual({
      subject: "usbX",
      passes: w.passes.map((p) => p.digest),
      granted: true,
    });
  });
});
