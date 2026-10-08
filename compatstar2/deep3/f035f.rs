//! F035 深化批次四 · 修复动作执行面（compatstar2/deep3 · G-A-35）。
//!
//! 批次一/二/三已覆盖 F035 的向导流程面、执行治理面与日志归因面；本批补
//! 主册【功能定义】「全语义对齐」的序列化/账本/容错面：修复动作表
//! （download/install/regrant/rerun 四动作 × 前置检查位图：磁盘/网络/
//! 权限/哈希四检）、动作幂等账（重复执行检测：同指纹动作已完成 → 跳过
//! 并记账，rerun 动作例外幂等可重跑）、部分失败回滚（多步动作定长 8 步，
//! 第 N 步失败 → 已完成步逆序回滚）、重试上限与放弃账（每动作重试计数，
//! 超限归档待人工）。
//!
//! 判据对账：深化以主册【状态与异常】「包管理器下载失败 → 三要素错误 +
//! 重试」（F035 视角 = 修复动作的重试出口）、「归因置信度低 → 如实
//! 『未知原因』（不硬编原因，超限动作归档待人工不静默重抛）」未落地面
//! 为源，一处一事实（MS 修复重试上限与事务逆序回滚语义对拍）。
//!
//! 零堆纪律：定长动作表/指纹账/步账/放弃档案，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 动作码：下载运行时。
pub const ACT_DOWNLOAD: u8 = 0;
/// 动作码：安装。
pub const ACT_INSTALL: u8 = 1;
/// 动作码：重新授权。
pub const ACT_REGRANT: u8 = 2;
/// 动作码：重跑（判例重验）。
pub const ACT_RERUN: u8 = 3;
/// 动作数。
pub const ACTION_N: usize = 4;
/// 前置检查位：磁盘。
pub const CHK_DISK: u32 = 1 << 0;
/// 前置检查位：网络。
pub const CHK_NET: u32 = 1 << 1;
/// 前置检查位：权限。
pub const CHK_PERM: u32 = 1 << 2;
/// 前置检查位：哈希。
pub const CHK_HASH: u32 = 1 << 3;
/// 动作名表（账面/诊断显示用）。
pub const ACTION_NAMES: [&'static str; ACTION_N] = ["download", "install", "regrant", "rerun"];
/// 动作前置检查位图（主册归因分支映射：缺运行时→下载、权限→提权、
/// 损坏→重跑；install 组合磁盘+权限+哈希三检）。
pub const ACTION_REQ: [u32; ACTION_N] = [
    CHK_DISK | CHK_NET,             // download：磁盘 + 网络
    CHK_DISK | CHK_PERM | CHK_HASH, // install：磁盘 + 权限 + 哈希
    CHK_PERM,                       // regrant：权限
    CHK_DISK,                       // rerun：磁盘
];
/// 多步动作步数（定长 8）。
pub const STEPS: usize = 8;
/// 重试上限（超过 → 归档待人工）。
pub const RETRY_CAP: u32 = 3;
/// 指纹账/重试账/放弃档案容量。
pub const LEDGER_N: usize = 8;

/// 前置缺位提取（诊断面）：have 相对动作要求的缺失位集。
pub fn missing_bits(action: u8, have: u32) -> u32 {
    if action as usize >= ACTION_N {
        return u32::MAX;
    }
    ACTION_REQ[action as usize] & !have
}

// ---------------------------------------------------------------------------
// 修复动作执行器（幂等账 + 回滚 + 重试上限）
// ---------------------------------------------------------------------------

pub struct RepairRunner {
    done_keys: [u32; LEDGER_N],
    done_n: usize,
    retry_keys: [u32; LEDGER_N],
    retry_counts: [u32; LEDGER_N],
    retry_n: usize,
    abandoned: [u32; LEDGER_N],
    pub abandoned_n: usize,
    /// 实际执行计数（含 rerun 重跑）。
    pub executed: u32,
    /// 幂等跳过计数。
    pub skipped: u32,
    /// rerun 例外重跑计数。
    pub reruns: u32,
    /// 前置不满足拦截计数。
    pub blocked: u32,
    /// 回滚发生次数。
    pub rollbacks: u32,
    /// 逆序回滚步数累计。
    pub rolled_back_steps: u32,
    /// 完成步数累计（含后被回滚的步，账面如实）。
    pub completed_steps: u32,
    /// 重试计数累计。
    pub retries: u32,
}

impl RepairRunner {
    pub const fn new() -> Self {
        RepairRunner {
            done_keys: [0; LEDGER_N],
            done_n: 0,
            retry_keys: [0; LEDGER_N],
            retry_counts: [0; LEDGER_N],
            retry_n: 0,
            abandoned: [0; LEDGER_N],
            abandoned_n: 0,
            executed: 0,
            skipped: 0,
            reruns: 0,
            blocked: 0,
            rollbacks: 0,
            rolled_back_steps: 0,
            completed_steps: 0,
            retries: 0,
        }
    }

    /// 前置检查：have 覆盖动作要求位 → Ok；缺位 → 拦截计数 + 显性 Err。
    pub fn precondition(&mut self, action: u8, have: u32) -> Result<(), &'static str> {
        if action as usize >= ACTION_N {
            return Err("bad-action");
        }
        if have & ACTION_REQ[action as usize] == ACTION_REQ[action as usize] {
            Ok(())
        } else {
            self.blocked += 1;
            Err("precondition-fail")
        }
    }

    /// 执行动作：同指纹非 rerun 已完成 → 跳过（Ok(false)）并记账；
    /// rerun 例外可重跑（reruns 计数）；真执行返回 Ok(true)。
    pub fn execute(&mut self, action: u8, fp: u32) -> Result<bool, &'static str> {
        if action as usize >= ACTION_N {
            return Err("bad-action");
        }
        let mut done = false;
        for i in 0..self.done_n {
            if self.done_keys[i] == fp {
                done = true;
                break;
            }
        }
        if done && action != ACT_RERUN {
            self.skipped += 1;
            return Ok(false);
        }
        if done {
            self.reruns += 1;
        } else {
            if self.done_n >= LEDGER_N {
                return Err("done-ledger-full");
            }
            self.done_keys[self.done_n] = fp;
            self.done_n += 1;
        }
        self.executed += 1;
        Ok(true)
    }

    /// 多步动作（定长 8 步）顺序执行：第 fail_at 步失败 → 已完成步逆序
    /// 回滚并显性 Err；fail_at ≥ STEPS → 全步成功 Ok(STEPS)。
    pub fn run_steps(&mut self, fail_at: usize) -> Result<usize, &'static str> {
        let mut done_steps = [false; STEPS];
        for i in 0..STEPS {
            if i == fail_at {
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    if done_steps[j] {
                        done_steps[j] = false;
                        self.rolled_back_steps += 1;
                    }
                }
                self.rollbacks += 1;
                return Err("step-failed");
            }
            done_steps[i] = true;
            self.completed_steps += 1;
        }
        Ok(STEPS)
    }

    /// 失败记账：同指纹重试计数；超过 RETRY_CAP → 归档放弃（待人工，
    /// 去重）并显性 Err；Ok(n) = 第 n 次重试仍可继续。
    pub fn record_failure(&mut self, fp: u32) -> Result<u32, &'static str> {
        let mut idx = None;
        for i in 0..self.retry_n {
            if self.retry_keys[i] == fp {
                idx = Some(i);
                break;
            }
        }
        let i = match idx {
            Some(i) => i,
            None => {
                if self.retry_n >= LEDGER_N {
                    return Err("retry-ledger-full");
                }
                self.retry_keys[self.retry_n] = fp;
                self.retry_counts[self.retry_n] = 0;
                self.retry_n += 1;
                self.retry_n - 1
            }
        };
        self.retry_counts[i] += 1;
        self.retries += 1;
        if self.retry_counts[i] > RETRY_CAP {
            if !self.is_abandoned(fp) {
                if self.abandoned_n >= LEDGER_N {
                    return Err("abandon-ledger-full");
                }
                self.abandoned[self.abandoned_n] = fp;
                self.abandoned_n += 1;
            }
            return Err("retries-exhausted");
        }
        Ok(self.retry_counts[i])
    }

    /// 指纹当前重试计数。
    pub fn retry_count(&self, fp: u32) -> u32 {
        for i in 0..self.retry_n {
            if self.retry_keys[i] == fp {
                return self.retry_counts[i];
            }
        }
        0
    }

    /// 指纹是否已归档放弃。
    pub fn is_abandoned(&self, fp: u32) -> bool {
        for i in 0..self.abandoned_n {
            if self.abandoned[i] == fp {
                return true;
            }
        }
        false
    }

    /// 已完成指纹账条数。
    pub fn done_count(&self) -> usize {
        self.done_n
    }
}

/// 域自检（深化批次四）。
pub fn run_f035f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F035-repair-action-d4");
    // 1) 前置位图：满验零缺位；缺位提取如实（download 需磁盘+网络）。
    let mut req_ok = true;
    for a in 0..ACTION_N as u8 {
        req_ok &= missing_bits(a, ACTION_REQ[a as usize]) == 0;
    }
    req_ok &= missing_bits(ACT_DOWNLOAD, 0) == (CHK_DISK | CHK_NET);
    req_ok &= missing_bits(ACT_INSTALL, CHK_DISK) == (CHK_PERM | CHK_HASH);
    cs.add("action_req_bitmap", req_ok, "");
    // 2) 前置拦截记账 + 补齐后放行。
    let mut r = RepairRunner::new();
    let blocked = r.precondition(ACT_DOWNLOAD, CHK_DISK) == Err("precondition-fail") && r.blocked == 1;
    let pass = r.precondition(ACT_DOWNLOAD, CHK_DISK | CHK_NET).is_ok();
    cs.add("precondition_block_counted", blocked && pass && r.blocked == 1, "");
    // 3) 幂等账：同指纹已完成 → 跳过并记账，不再执行。
    let first = r.execute(ACT_INSTALL, 0xA11CE);
    let again = r.execute(ACT_INSTALL, 0xA11CE);
    cs.add("idempotent_skip", first == Ok(true) && again == Ok(false) && r.skipped == 1 && r.executed == 1, "");
    // 4) rerun 例外：同指纹仍可重跑（reruns 计数）。
    cs.add("rerun_exception", r.execute(ACT_RERUN, 0xA11CE) == Ok(true) && r.reruns == 1 && r.executed == 2, "");
    // 5) 新指纹独立执行（指纹账两条）。
    cs.add("fresh_fingerprint_executes", r.execute(ACT_DOWNLOAD, 0xB0B) == Ok(true) && r.done_count() == 2, "");
    // 6) 多步动作：全成 Ok(8)；第 5 步失败 → 5 步逆序回滚。
    let mut r2 = RepairRunner::new();
    let full = r2.run_steps(usize::MAX);
    let partial = r2.run_steps(5);
    cs.add("rollback_reverse", full == Ok(8) && partial == Err("step-failed") && r2.rollbacks == 1 && r2.rolled_back_steps == 5 && r2.completed_steps == 13, "");
    // 7) 首步即败：零回滚步、回滚事件仍记账。
    let mut r3 = RepairRunner::new();
    cs.add("rollback_at_first_step", r3.run_steps(0) == Err("step-failed") && r3.rolled_back_steps == 0 && r3.rollbacks == 1, "");
    // 8) 重试上限：3 次内放行，第 4 次归档放弃。
    let mut r4 = RepairRunner::new();
    let mut seq_ok = r4.record_failure(0xFEED) == Ok(1)
        && r4.record_failure(0xFEED) == Ok(2)
        && r4.record_failure(0xFEED) == Ok(3);
    seq_ok &= r4.record_failure(0xFEED) == Err("retries-exhausted") && r4.is_abandoned(0xFEED) && r4.abandoned_n == 1;
    cs.add("retry_cap_abandon", seq_ok && r4.retries == 4, "");
    // 9) 放弃档案去重：同指纹重复失败不再追加档案。
    cs.add("abandon_dedup", r4.record_failure(0xFEED) == Err("retries-exhausted") && r4.abandoned_n == 1, "");
    // 10) 重试计数按指纹独立。
    cs.add("independent_retry_counters", r4.record_failure(0xD00D) == Ok(1) && r4.retry_count(0xD00D) == 1 && !r4.is_abandoned(0xD00D), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rerun_never_blocks_fresh_work() {
        let mut r = RepairRunner::new();
        assert_eq!(r.execute(ACT_DOWNLOAD, 1), Ok(true));
        assert_eq!(r.execute(ACT_DOWNLOAD, 1), Ok(false), "同指纹重复执行幂等跳过");
        assert_eq!(r.execute(ACT_RERUN, 1), Ok(true), "rerun 例外可重跑");
        assert_eq!(r.execute(ACT_RERUN, 1), Ok(true), "rerun 可再重跑");
        assert_eq!(r.executed, 3);
        assert_eq!(r.skipped, 1);
        assert_eq!(r.reruns, 2);
        assert_eq!(r.done_count(), 1, "重跑不重复入账");
    }

    #[test]
    fn retry_and_abandon_are_exact() {
        let mut r = RepairRunner::new();
        for expect in 1..=RETRY_CAP {
            assert_eq!(r.record_failure(7), Ok(expect), "上限内重试放行");
        }
        assert_eq!(r.record_failure(7), Err("retries-exhausted"), "超限显性放弃");
        assert!(r.is_abandoned(7));
        assert!(!r.is_abandoned(8));
        assert_eq!(r.retry_count(7), 4);
        assert_eq!(r.abandoned_n, 1);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f035f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
