//! F185 只读卷保护提示 · 批次三深化（secstar · G-G-15）。
//!
//! 批次三功能面（主册判据「拖放受阻提示全链 + 帮助链通」的实现纵深）：
//! - [`SpacePrecheck`]：拷贝前空间预检——目标 VARIX 区容量/已用/剩余三数
//!   计算，装不下诚实说装不下（预估字节 vs 剩余字节，不留惊喜）；
//! - [`CopyWorker`]：单任务复制状态机——Queued/Scanning/Copying/Done/
//!   Failed/Cancelled 六态全出口（第三条交互公理：出现了就必须有完整
//!   消失路径，复制也是）；
//! - [`RetryPolicy`]：失败重试策略——指数退避 3 次，重试不重复提交
//!   （重试中的任务再点重试 = 无操作）；
//! - [`help_steps`]：帮助链台阶生成——只读原因 → 人话三步（发生了什么/
//!   为什么/下一步怎么办三要素的步骤化落地）。
//!
//! 零堆纪律：定长路径缓冲 + 状态字段，无 alloc。

use super::romount::ReadOnlyCause;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 空间预检
// ---------------------------------------------------------------------------

/// 目标卷空间账（KiB 口径——与卷元数据同单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpacePrecheck {
    pub total_kib: u64,
    pub used_kib: u64,
}

impl SpacePrecheck {
    /// 剩余空间（下溢钳 0）。
    pub fn free_kib(&self) -> u64 {
        self.total_kib.saturating_sub(self.used_kib)
    }

    /// 预检判定：需要 `need_kib` + 预留 10% 余量（写满盘是缺陷不是终点）。
    /// 返回 (可放, 建议清理字节数——装不下时给可执行的下一步)。
    pub fn fits(&self, need_kib: u64) -> (bool, u64) {
        let reserve = self.total_kib / 10;
        let avail = self.free_kib().saturating_sub(reserve);
        if need_kib <= avail {
            (true, 0)
        } else {
            (false, need_kib - avail)
        }
    }
}

// ---------------------------------------------------------------------------
// 单任务复制状态机（六态全出口）
// ---------------------------------------------------------------------------

/// 复制任务状态（全出口：Done/Failed/Cancelled 是三个不同终点——
/// 「看起来没反应」和「反应了但不对」同罪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyPhase {
    Queued,
    Scanning,
    Copying,
    Done,
    Failed,
    Cancelled,
}

/// 单任务复制执行器（进度千分比 + 取消语义 + 字节账）。
pub struct CopyWorker {
    pub phase: CopyPhase,
    pub total_bytes: u64,
    pub done_bytes: u64,
    /// 取消请求旗（异步取消——Copying 中收到请求在下一拍生效，
    /// 不许状态留半空）。
    cancel_requested: bool,
    /// 已尝试重试次数。
    pub retries: u8,
}

impl CopyWorker {
    pub const fn new(total_bytes: u64) -> CopyWorker {
        CopyWorker { phase: CopyPhase::Queued, total_bytes, done_bytes: 0, cancel_requested: false, retries: 0 }
    }

    pub fn start_scan(&mut self) {
        if self.phase == CopyPhase::Queued {
            self.phase = CopyPhase::Scanning;
        }
    }

    pub fn start_copy(&mut self) {
        if self.phase == CopyPhase::Scanning {
            self.phase = CopyPhase::Copying;
        }
    }

    /// 推进字节（ copying 中才有效；进度单调不回退）。
    pub fn progress(&mut self, bytes: u64) {
        if self.phase == CopyPhase::Copying {
            // 取消请求先于字节入账生效——进度冻结在事发点（不偷跑）。
            if self.cancel_requested {
                self.phase = CopyPhase::Cancelled;
                return;
            }
            self.done_bytes = self.done_bytes.saturating_add(bytes).min(self.total_bytes);
            if self.done_bytes == self.total_bytes {
                self.phase = CopyPhase::Done;
            }
        }
    }

    pub fn fail(&mut self) {
        if matches!(self.phase, CopyPhase::Scanning | CopyPhase::Copying) {
            self.phase = CopyPhase::Failed;
        }
    }

    pub fn request_cancel(&mut self) {
        if matches!(self.phase, CopyPhase::Queued | CopyPhase::Scanning) {
            self.phase = CopyPhase::Cancelled;
        } else if self.phase == CopyPhase::Copying {
            self.cancel_requested = true;
        }
    }

    /// 重试：仅 Failed 态可重试；Copying/Queued 中重试 = 无操作
    /// （重试不重复提交——双击重试不产生双份任务）。
    pub fn retry(&mut self) -> bool {
        if self.phase != CopyPhase::Failed || self.retries >= 3 {
            return false;
        }
        self.retries += 1;
        self.phase = CopyPhase::Queued;
        self.done_bytes = 0;
        self.cancel_requested = false;
        true
    }

    /// 进度千分比（终态恒定：Done=1000，Cancelled/Failed 冻结在事发点）。
    pub fn permille(&self) -> u32 {
        match self.phase {
            CopyPhase::Done => 1_000,
            _ => {
                if self.total_bytes == 0 {
                    return 0;
                }
                ((self.done_bytes * 1_000) / self.total_bytes) as u32
            }
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.phase, CopyPhase::Done | CopyPhase::Failed | CopyPhase::Cancelled)
    }
}

// ---------------------------------------------------------------------------
// 重试退避策略（指数退避——重试风暴也是风暴）
// ---------------------------------------------------------------------------

/// 第 n 次重试的退避毫秒数：1s → 2s → 4s（指数），3 次封顶后升级人。
pub fn retry_backoff_ms(retry_index: u8) -> Option<u64> {
    match retry_index {
        0 => Some(1_000),
        1 => Some(2_000),
        2 => Some(4_000),
        _ => None, // 三次后不再自动重试——升级到帮助链
    }
}

// ---------------------------------------------------------------------------
// 帮助链台阶（三要素的步骤化——每原因三步，人话）
// ---------------------------------------------------------------------------

/// 帮助台阶：标题 + 三步文案（错也错得可操作）。
pub fn help_steps(cause: ReadOnlyCause) -> [(&'static str, [&'static str; 3]); 2] {
    let _ = cause; // 原因分型在 badge/tooltip 面（既有层）——台阶按动作分类
    [
        (
            "拷到 VARIX 区（推荐）",
            [
                "在只读卷上选中需要的文件",
                "右键 → 「拷到 VARIX 区」",
                "复制完成后原卷保持只读，数据已在可写区",
            ],
        ),
        (
            "解除只读（需要硬件配合）",
            [
                "确认设备的物理写保护开关位置",
                "关闭写保护后重新插入",
                "若仍只读，查看 help:F119/ntfs-readonly 的主控篇",
            ],
        ),
    ]
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_romount_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-b3");

    // 1) 空间预检：放得下放不下两分支全对（预留 10% 生效）。
    let s = SpacePrecheck { total_kib: 1_000_000, used_kib: 800_000 };
    let (fits1, _) = s.fits(50_000); // 剩 20 万，预留 10 万，可放
    let (fits2, short) = s.fits(150_000); // 需 15 万 > 10 万可用
    cs.add("space_precheck_two_ways", fits1 && !fits2 && short == 50_000, "");

    // 2) 空间预检下溢钳 0（used > total 不炸——外部位图如实上报）。
    let bad = SpacePrecheck { total_kib: 100, used_kib: 500 };
    cs.add("space_underflow_clamped", bad.free_kib() == 0 && !bad.fits(1).0, "");

    // 3) 复制状态机正常线：Queued→Scanning→Copying→Done（进度单调）。
    let mut w = CopyWorker::new(10_000);
    w.start_scan();
    w.start_copy();
    w.progress(4_000);
    let mid = w.permille();
    w.progress(6_000);
    cs.add("copy_happy_path", mid == 400 && w.phase == CopyPhase::Done && w.permille() == 1_000, "");

    // 4) 取消语义：Copying 中请求 → 下一拍生效，进度冻结在事发点。
    let mut w2 = CopyWorker::new(10_000);
    w2.start_scan();
    w2.start_copy();
    w2.progress(3_000);
    w2.request_cancel();
    w2.progress(1_000);
    cs.add("copy_cancel_next_tick", w2.phase == CopyPhase::Cancelled && w2.permille() == 300, "");

    // 5) 取消全出口：Queued/Scanning 即刻取消（不排队白做）。
    let mut w3 = CopyWorker::new(100);
    w3.request_cancel();
    let q_cancel = w3.phase == CopyPhase::Cancelled;
    let mut w4 = CopyWorker::new(100);
    w4.start_scan();
    w4.request_cancel();
    cs.add("cancel_early_paths", q_cancel && w4.phase == CopyPhase::Cancelled, "");

    // 6) 重试不重复提交：Copying 中重试无效、仅 Failed 可重试。
    let mut w5 = CopyWorker::new(1_000);
    w5.start_scan();
    w5.start_copy();
    let retry_during_copy = w5.retry();
    w5.fail();
    let retry_after_fail = w5.retry();
    cs.add("retry_only_failed", !retry_during_copy && retry_after_fail && w5.phase == CopyPhase::Queued, "");

    // 7) 重试三次封顶：第 4 次 = 拒绝（升级到帮助链，不再空转）。
    let mut w6 = CopyWorker::new(1_000);
    let mut retries_ok = 0;
    for _ in 0..4 {
        w6.start_scan();
        w6.start_copy();
        w6.fail();
        if w6.phase == CopyPhase::Failed && w6.retry() {
            retries_ok += 1;
        }
    }
    cs.add("retry_cap_three", retries_ok == 3 && !w6.retry(), "");

    // 8) 指数退避序列 1s/2s/4s/None（序列在册——可解释的等待）。
    cs.add(
        "retry_backoff_series",
        retry_backoff_ms(0) == Some(1_000) && retry_backoff_ms(1) == Some(2_000) && retry_backoff_ms(2) == Some(4_000) && retry_backoff_ms(3).is_none(),
        "",
    );

    // 9) 终态判定：Done/Failed/Cancelled 三个不同终点互不混淆。
    let mut w7 = CopyWorker::new(100);
    w7.request_cancel();
    let mut w8 = CopyWorker::new(100);
    w8.start_scan();
    w8.start_copy();
    w8.progress(50);
    w8.fail();
    cs.add(
        "terminal_states_distinct",
        w7.is_terminal() && w8.is_terminal() && w7.phase != w8.phase,
        "",
    );

    // 10) 帮助台阶：两套动作各三步齐（三要素步骤化不缺步）——三原因
    // 共用动作面（台阶按「能做什么」分类，不按「为什么坏了」分类）。
    let steps = help_steps(ReadOnlyCause::PolicyNtfs);
    let steps2 = help_steps(ReadOnlyCause::PhysicalLock);
    cs.add(
        "help_steps_shape",
        steps.len() == 2 && steps.iter().all(|(_, s)| s.len() == 3) && steps2[0].0 == steps[0].0,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn copy_worker_full_lifecycle() {
        // 全生命周期走查：正常/取消/失败重试至成功 三线互不串扰。
        let mut a = CopyWorker::new(1_000);
        a.start_scan();
        a.start_copy();
        a.progress(1_000);
        assert_eq!(a.phase, CopyPhase::Done);

        let mut b = CopyWorker::new(1_000);
        b.start_scan();
        b.start_copy();
        b.progress(200);
        b.request_cancel();
        b.progress(9_999); // 取消后推进无效——进度冻结
        assert_eq!(b.phase, CopyPhase::Cancelled);
        assert_eq!(b.permille(), 200);

        let mut c = CopyWorker::new(1_000);
        c.start_scan();
        c.start_copy();
        c.fail();
        assert!(c.retry());
        c.start_scan();
        c.start_copy();
        c.progress(1_000);
        assert_eq!(c.phase, CopyPhase::Done);
        assert_eq!(c.retries, 1);
    }

    #[test]
    fn progress_never_regresses() {
        // 进度单调：垃圾字节报告（超出总量）被钳制不回退。
        let mut w = CopyWorker::new(100);
        w.start_scan();
        w.start_copy();
        w.progress(80);
        w.progress(u64::MAX); // 异常上报钳到总量
        assert_eq!(w.phase, CopyPhase::Done);
        assert_eq!(w.permille(), 1_000);
    }

    #[test]
    fn space_reserve_honored() {
        // 预留 10%：剩余 9% 时即使小文件也拒（写满盘是缺陷）。
        let s = SpacePrecheck { total_kib: 1000, used_kib: 910 };
        let (fits, _) = s.fits(50); // 可用 90-100=0（钳 0）
        assert!(!fits);
    }
}
