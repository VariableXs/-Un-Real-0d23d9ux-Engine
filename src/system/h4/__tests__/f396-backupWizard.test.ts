import { describe, expect, it } from "vitest";
import { getReminder, markVerified, pauseTransfer, reminderDue, restoreCompare, resumeTransfer, resumedFrom, setReminder, startChain, appendIncrement, chainReadyForRestore, validatePlan, type BackupScope, type TransferState } from "../f396-backupWizard";
import { __clearMem, memStore } from "../internal/store";

const SCOPE: BackupScope = { systemPartition: true, userFiles: true };
const DAY = 24 * 3600 * 1000;

describe("F396 备份向导", () => {
  it("三步计划校验：目标合法、系统分区必选（判据）", () => {
    expect(validatePlan("second-media", SCOPE).ok).toBe(true);
    expect(validatePlan("floppy" as never, SCOPE).problems[0]).toContain("目标");
    const noSys = validatePlan("network", { systemPartition: false, userFiles: true });
    expect(noSys.ok).toBe(false);
    expect(noSys.problems[0]).toContain("必选");
  });

  it("增量链：首次全量 seq=0；增量只收差集；空差集拒绝（判据）", () => {
    const { chain } = startChain("another-usb", SCOPE, 0, ["f1", "f2", "f3"], 1000);
    const inc1 = appendIncrement(chain!, DAY, ["f1", "f2", "f3", "f4"], 100);
    expect(inc1.error).toBeNull();
    expect(inc1.chain.entries[1]!.fingerprints).toEqual(["f4"]);
    expect(inc1.chain.entries[1]!.seq).toBe(1);
    const inc2 = appendIncrement(inc1.chain, 2 * DAY, ["f1", "f2", "f3", "f4"], 0);
    expect(inc2.error).toContain("无变化");
  });

  it("可恢复性校验（判据）：全部 verified 才 ready；未验的如实列出", () => {
    let { chain } = startChain("network", SCOPE, 0, ["a"], 10);
    chain = appendIncrement(chain!, DAY, ["a", "b"], 5).chain;
    expect(chainReadyForRestore(chain)).toEqual({ ready: false, unverified: [0, 1] });
    chain = markVerified(chain, 0)!;
    expect(chainReadyForRestore(chain)).toEqual({ ready: false, unverified: [1] });
    chain = markVerified(chain, 1)!;
    expect(chainReadyForRestore(chain).ready).toBe(true);
  });

  it("三次增量后整体还原比对（判据）：当前文件全部可恢复（完备性）；已删旧文件如实标注 staleExtra", () => {
    let { chain } = startChain("second-media", SCOPE, 0, ["f1", "f2"], 100);
    chain = appendIncrement(chain!, DAY, ["f1", "f2", "f3"], 10).chain;
    chain = appendIncrement(chain, 2 * DAY, ["f1", "f2", "f3", "f4"], 10).chain;
    chain = appendIncrement(chain, 3 * DAY, ["f2", "f3", "f4", "f5"], 10).chain;
    const r = restoreCompare(chain, ["f2", "f3", "f4", "f5"]);
    expect(r.identical).toBe(true); // 完备性：current ⊆ 合并集
    expect(r.missing).toEqual([]);
    expect(r.staleExtra).toEqual(["f1"]); // f1 已删但备份里有——如实标注
    const loss = restoreCompare(chain, ["f2", "f9"]);
    expect(loss.identical).toBe(false);
    expect(loss.missing).toEqual(["f9"]);
  });

  it("暂停续传（F269 同源）：断点续传不重传", () => {
    let t: TransferState = { phase: "running", doneBytes: 700, totalBytes: 1000 };
    t = pauseTransfer(t);
    expect(t.phase).toBe("paused");
    t = pauseTransfer(t); // 幂等
    expect(t.phase).toBe("paused");
    t = resumeTransfer(t);
    expect(t.phase).toBe("running");
    expect(resumedFrom(t)).toBe(700);
  });

  it("提醒周期设置与到期判定：不自动跑（判据红线 autoRun=false）", () => {
    __clearMem();
    const s = memStore();
    expect(getReminder(s)).toBe("off");
    setReminder("monthly", s);
    expect(getReminder(s)).toBe("monthly");
    expect(reminderDue("off", null, DAY * 400).due).toBe(false);
    expect(reminderDue("monthly", null, 1).due).toBe(true); // 从未备份
    expect(reminderDue("quarterly", 0, 89 * DAY).due).toBe(false);
    expect(reminderDue("quarterly", 0, 91 * DAY).due).toBe(true);
    expect(reminderDue("monthly", null, 1).autoRun).toBe(false);
  });
});
