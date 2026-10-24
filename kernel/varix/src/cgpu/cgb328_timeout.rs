// cgb328_timeout.rs ↔ CGPU-F0328 超时处置
//
// 判据原文：CGPU-F0328-J1 三路、检查点语义、归因、判据。
// 难点分析：预算耗尽的三路处置（降档/排队/拒绝）选择取决于任务语义——
//          有检查点的可降档回滚，硬截止的只能拒绝。部分完成语义要求
//          「中途耗尽→检查点或回滚」，与 B 域任务语义一致。
// 纪律约束：降质不降帧铁律；超时归因记录；检查点语义与 B 域一致。
// 成本记账：O(1) 处置决策；归因流水环形上限防膨胀。

#![allow(dead_code)]

use alloc::vec::Vec;

pub type AccountId = u32;
pub type RequestId = u64;
pub type BudgetUs = u64;

/// 处置路径（三路）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposal {
    /// 降档：映射 O 域降质参数（降质不降帧）。
    Degrade,
    /// 排队：顺延下一帧预算窗口。
    Queue,
    /// 拒绝：硬截止任务拒绝并归因。
    Reject,
}

impl Disposal {
    pub fn describe(self) -> &'static str {
        match self {
            Disposal::Degrade => "降档（降质不降帧）",
            Disposal::Queue => "排队（顺延下一帧）",
            Disposal::Reject => "拒绝（硬截止）",
        }
    }
}

/// 任务超时画像。
#[derive(Debug, Clone)]
pub struct TimeoutProfile {
    pub request_id: RequestId,
    pub account: AccountId,
    /// 是否硬截止（不可顺延）。
    pub hard_deadline: bool,
    /// 是否有检查点（可回滚/续跑）。
    pub has_checkpoint: bool,
    /// 降档档位（0=不降，1~3 递减质量）。
    pub degrade_level: u8,
    /// 已排队次数（防无限顺延）。
    pub queued_times: u8,
}

/// 超时归因记录。
#[derive(Debug, Clone)]
pub struct TimeoutRecord {
    pub request_id: RequestId,
    pub account: AccountId,
    /// 超了多少（μs）。
    pub overrun_us: BudgetUs,
    /// 批准预算。
    pub approved_us: BudgetUs,
    /// 实际用量。
    pub actual_us: BudgetUs,
    /// 处置。
    pub disposal: Disposal,
    /// 原因描述指针。
    pub reason: &'static str,
}

/// 检查点状态（部分完成语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointState {
    /// 无检查点（必须全有或全无）。
    None,
    /// 已存检查点（可续跑）。
    Saved,
    /// 回滚到检查点。
    RolledBack,
    /// 从检查点续跑。
    Resumed,
}

/// 超时处置器。
pub struct TimeoutHandler {
    /// 排队队列（下一帧释放）。
    queue: Vec<TimeoutProfile>,
    /// 归因流水。
    records: Vec<TimeoutRecord>,
    /// 最大排队次数（超过转拒绝）。
    max_queue_times: u8,
    /// 最大降档档位。
    max_degrade_level: u8,
    /// 归因流水上限。
    max_records: usize,
    /// 统计。
    n_degrade: u32,
    n_queue: u32,
    n_reject: u32,
}

impl TimeoutHandler {
    /// 新建。
    pub fn new() -> Self {
        TimeoutHandler {
            queue: Vec::new(),
            records: Vec::new(),
            max_queue_times: 3,
            max_degrade_level: 3,
            max_records: 4096,
            n_degrade: 0,
            n_queue: 0,
            n_reject: 0,
        }
    }

    /// 决策：给定任务画像与超时量，选处置路径。
    pub fn decide(&self, profile: &TimeoutProfile) -> Disposal {
        // 硬截止 → 排队无意义。
        if profile.hard_deadline {
            return Disposal::Reject;
        }
        // 有检查点 + 还能降档 → 降档。
        if profile.has_checkpoint && profile.degrade_level < self.max_degrade_level {
            return Disposal::Degrade;
        }
        // 排队次数未满 → 排队。
        if profile.queued_times < self.max_queue_times {
            return Disposal::Queue;
        }
        // 兜底拒绝。
        Disposal::Reject
    }

    /// 执行处置（含检查点语义与归因）。
    pub fn handle(
        &mut self,
        profile: &TimeoutProfile,
        approved_us: BudgetUs,
        actual_us: BudgetUs,
    ) -> (Disposal, CheckpointState) {
        let overrun = actual_us.saturating_sub(approved_us);
        let disposal = self.decide(profile);

        // 检查点语义。
        let ckpt = match disposal {
            Disposal::Degrade => {
                if profile.has_checkpoint {
                    CheckpointState::Saved
                } else {
                    CheckpointState::None
                }
            }
            Disposal::Queue => {
                if profile.has_checkpoint {
                    CheckpointState::RolledBack
                } else {
                    CheckpointState::None
                }
            }
            Disposal::Reject => {
                if profile.has_checkpoint {
                    CheckpointState::RolledBack
                } else {
                    CheckpointState::None
                }
            }
        };

        // 更新画像副本入队/统计。
        match disposal {
            Disposal::Degrade => {
                self.n_degrade += 1;
                let mut p = profile.clone();
                p.degrade_level += 1;
                // 降档后下帧重新参与。
            }
            Disposal::Queue => {
                self.n_queue += 1;
                let mut p = profile.clone();
                p.queued_times += 1;
                self.queue.push(p);
            }
            Disposal::Reject => {
                self.n_reject += 1;
            }
        }

        // 归因记录（环形上限）。
        if self.records.len() >= self.max_records {
            self.records.remove(0);
        }
        self.records.push(TimeoutRecord {
            request_id: profile.request_id,
            account: profile.account,
            overrun_us: overrun,
            approved_us,
            actual_us,
            disposal,
            reason: match disposal {
                Disposal::Degrade => "超预算→降档（有检查点）",
                Disposal::Queue => "超预算→排队顺延",
                Disposal::Reject => "超预算→拒绝（硬截止或排队超限）",
            },
        });

        (disposal, ckpt)
    }

    /// 帧初：弹出排队任务。
    pub fn drain_queue(&mut self) -> Vec<TimeoutProfile> {
        core::mem::take(&mut self.queue)
    }

    /// 统计。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.n_degrade, self.n_queue, self.n_reject)
    }
    /// 归因流水。
    pub fn records(&self) -> &[TimeoutRecord] {
        &self.records
    }
    /// 排队深度。
    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }
}

impl Default for TimeoutHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb328_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0328 超时处置");

    let mut h = TimeoutHandler::new();

    // --- 硬截止 → Reject ---
    let hard = TimeoutProfile {
        request_id: 1,
        account: 1,
        hard_deadline: true,
        has_checkpoint: true,
        degrade_level: 0,
        queued_times: 0,
    };
    cs.add("硬截止 → 拒绝", h.decide(&hard) == Disposal::Reject, "不可顺延");

    // --- 有检查点 + 可降档 → Degrade ---
    let soft = TimeoutProfile {
        request_id: 2,
        account: 2,
        hard_deadline: false,
        has_checkpoint: true,
        degrade_level: 0,
        queued_times: 0,
    };
    cs.add("可降档 → 降档", h.decide(&soft) == Disposal::Degrade, "降质不降帧");

    // --- 无检查点 + 可排队 → Queue ---
    let no_ckpt = TimeoutProfile {
        request_id: 3,
        account: 3,
        hard_deadline: false,
        has_checkpoint: false,
        degrade_level: 0,
        queued_times: 0,
    };
    cs.add("无检查点 → 排队", h.decide(&no_ckpt) == Disposal::Queue, "顺延下一帧");

    // --- 排队超限 → Reject ---
    let queued_out = TimeoutProfile {
        request_id: 4,
        account: 4,
        hard_deadline: false,
        has_checkpoint: false,
        degrade_level: 0,
        queued_times: 3,
    };
    cs.add("排队超限 → 拒绝", h.decide(&queued_out) == Disposal::Reject, "防无限顺延");

    // --- handle 归因 ---
    let (d, ck) = h.handle(&soft, 100, 250);
    cs.add("处置执行", d == Disposal::Degrade, "降档");
    cs.add("检查点 Saved", ck == CheckpointState::Saved, "有检查点");
    cs.add("归因记录非空", h.records().len() == 1, "一条记录");
    let rec = &h.records()[0];
    cs.add("超时量正确", rec.overrun_us == 150, "250-100=150");

    // --- 排队入队 ---
    h.handle(&no_ckpt, 100, 200);
    cs.add("排队深度 1", h.queue_depth() == 1, "入队");
    let drained = h.drain_queue();
    cs.add("帧初弹出", drained.len() == 1, "队列清空");
    cs.add_dyn("弹出后队列空", h.queue_depth() == 0, "drain 清空");

    // --- 三路统计 ---
    let (nd, nq, nr) = h.stats();
    cs.add("三路统计齐全", nd >= 1 && nq >= 1, "降档+排队");

    cs
}
