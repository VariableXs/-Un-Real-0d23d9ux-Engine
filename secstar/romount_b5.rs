//! F185 只读卷保护提示 · 批次五深化（secstar · G-G-15）。
//!
//! 批次五功能面（与 b3「单任务状态机」、b4「队列治理」互补，本批管
//! 「批量与回执」）：
//! - [`BatchPicker`]：批量选择队列化——多选文件逐个入队，总量预估
//!   （「全选拷贝」的入口面：一次操作 N 个任务）；
//! - [`ConflictPolicy`]：目标区冲突三分支——跳过/覆盖/重命名逐任务
//!   决策（用户选策略，不替用户猜）；
//! - [`CopyReceipt`]：拷贝回执——成功清单/失败清单/字节账三段
//!   （做完给回执：哪些到了、哪些没到、一共多少——可对账）；
//! - [`receipt_frame`]：回执持久帧 encode/decode 带校验和（回执可
//!   跨重启查——撕裂必拒）。
//!
//! 零堆纪律：定长清单 + 定长帧，无 alloc。

use super::romount::CopyFallbackJob;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 批量选择队列化
// ---------------------------------------------------------------------------

/// 批量上限（一次操作最多 16 个文件——超限分批）。
pub const BATCH_CAP: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct BatchPicker {
    pub count: usize,
    pub total_bytes: u64,
    pub rejected_over: usize,
}

impl BatchPicker {
    pub const fn new() -> BatchPicker {
        BatchPicker { count: 0, total_bytes: 0, rejected_over: 0 }
    }

    /// 选中一个文件（超限诚实拒——分批提示的依据）。
    pub fn pick(&mut self, size_bytes: u64) -> bool {
        if self.count >= BATCH_CAP {
            self.rejected_over += 1;
            return false;
        }
        self.count += 1;
        self.total_bytes += size_bytes;
        true
    }

    /// 清空重选（取消选择是安全出路）。
    pub fn reset(&mut self) {
        self.count = 0;
        self.total_bytes = 0;
        self.rejected_over = 0;
    }
}

// ---------------------------------------------------------------------------
// 冲突三分支策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictOutcome {
    Skipped,
    Overwritten,
    Renamed,
    /// 策略为 Rename 但重命名失败（名字池满）——诚实失败。
    RenameFailed,
}

/// 执行分支决策（重命名走批次四 dedupe 规则——此处只裁决归类）。
pub fn resolve_conflict(choice: ConflictChoice, rename_ok: bool) -> ConflictOutcome {
    match choice {
        ConflictChoice::Skip => ConflictOutcome::Skipped,
        ConflictChoice::Overwrite => ConflictOutcome::Overwritten,
        ConflictChoice::Rename => {
            if rename_ok {
                ConflictOutcome::Renamed
            } else {
                ConflictOutcome::RenameFailed
            }
        }
    }
}

/// 用户级默认策略：重命名（不丢旧文件也不丢新文件——两全默认）。
pub const DEFAULT_CONFLICT: ConflictChoice = ConflictChoice::Rename;

// ---------------------------------------------------------------------------
// 拷贝回执
// ---------------------------------------------------------------------------

/// 回执清单容量。
pub const RECEIPT_CAP: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct CopyReceipt {
    succeeded: [u32; RECEIPT_CAP], // 任务 idx
    succeeded_n: usize,
    failed: [u32; RECEIPT_CAP],
    failed_n: usize,
    pub bytes_done: u64,
    pub bytes_planned: u64,
}

impl CopyReceipt {
    pub const fn new() -> CopyReceipt {
        CopyReceipt { succeeded: [0; RECEIPT_CAP], succeeded_n: 0, failed: [0; RECEIPT_CAP], failed_n: 0, bytes_done: 0, bytes_planned: 0 }
    }

    pub fn plan(&mut self, idx: usize, bytes: u64) -> bool {
        if idx >= RECEIPT_CAP {
            return false;
        }
        self.bytes_planned += bytes;
        true
    }

    pub fn on_success(&mut self, idx: usize, bytes: u64) -> bool {
        if self.succeeded_n >= RECEIPT_CAP {
            return false;
        }
        self.succeeded[self.succeeded_n] = idx as u32;
        self.succeeded_n += 1;
        self.bytes_done += bytes;
        true
    }

    pub fn on_failure(&mut self, idx: usize) -> bool {
        if self.failed_n >= RECEIPT_CAP {
            return false;
        }
        self.failed[self.failed_n] = idx as u32;
        self.failed_n += 1;
        true
    }

    /// 收口判定：计划中每任务都有结局（成功或失败——没有「没了下文」）。
    pub fn settled(&self, planned_tasks: usize) -> bool {
        self.succeeded_n + self.failed_n == planned_tasks
    }

    /// 成功率 ‰。
    pub fn success_permille(&self) -> u32 {
        let total = self.succeeded_n + self.failed_n;
        if total == 0 {
            return 0;
        }
        (self.succeeded_n * 1_000 / total) as u32
    }
}

// ---------------------------------------------------------------------------
// 回执持久帧（带校验和）
// ---------------------------------------------------------------------------

/// 帧：[0..2) 魔数 "XR" · [2..4) 成功数 LE · [4..6) 失败数 LE ·
/// [6..14) bytes_done u64 LE · [14..22) bytes_planned u64 LE ·
/// [22..24) 校验和（前 22B FNV-16）。
pub const RECEIPT_FRAME_LEN: usize = 24;

fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811c9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    ((h >> 16) ^ h) as u16
}

pub fn encode_receipt_frame(r: &CopyReceipt, out: &mut [u8; RECEIPT_FRAME_LEN]) -> bool {
    if r.bytes_planned == 0 && (r.succeeded_n > 0 || r.failed_n > 0) {
        return false; // 有结局却无计划 = 账目矛盾，不出门
    }
    out[0] = b'X';
    out[1] = b'R';
    out[2..4].copy_from_slice(&(r.succeeded_n as u16).to_le_bytes());
    out[4..6].copy_from_slice(&(r.failed_n as u16).to_le_bytes());
    out[6..14].copy_from_slice(&r.bytes_done.to_le_bytes());
    out[14..22].copy_from_slice(&r.bytes_planned.to_le_bytes());
    let c = fnv16(&out[..22]);
    out[22] = (c & 0xFF) as u8;
    out[23] = (c >> 8) as u8;
    true
}

pub fn decode_receipt_frame(frame: &[u8; RECEIPT_FRAME_LEN]) -> Option<(u16, u16, u64, u64)> {
    if frame[0] != b'X' || frame[1] != b'R' {
        return None;
    }
    let want = (frame[23] as u16) << 8 | frame[22] as u16;
    if fnv16(&frame[..22]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u64::from_le_bytes(frame[6..14].try_into().ok()?),
        u64::from_le_bytes(frame[14..22].try_into().ok()?),
    ))
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_romount_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F185-b5");

    // 1) 批量选择：16 内全收、第 17 拒并计数（分批提示依据）。
    let mut b = BatchPicker::new();
    let mut all = true;
    for _ in 0..BATCH_CAP {
        all &= b.pick(1_000);
    }
    cs.add(
        "batch_cap",
        all && b.count == BATCH_CAP && !b.pick(1) && b.rejected_over == 1 && b.total_bytes == 16_000,
        "",
    );

    // 2) 批量清空重选：取消选择零残留（安全出路）。
    b.reset();
    cs.add("batch_reset", b.count == 0 && b.total_bytes == 0 && b.rejected_over == 0, "");

    // 3) 冲突三分支：跳过/覆盖/重命名逐任务决策（不替用户猜）。
    cs.add(
        "conflict_three_branches",
        resolve_conflict(ConflictChoice::Skip, true) == ConflictOutcome::Skipped
            && resolve_conflict(ConflictChoice::Overwrite, false) == ConflictOutcome::Overwritten
            && resolve_conflict(ConflictChoice::Rename, true) == ConflictOutcome::Renamed,
        "",
    );

    // 4) 重命名失败诚实：rename_ok=false → RenameFailed（不冒充成功）。
    cs.add("conflict_rename_failed", resolve_conflict(ConflictChoice::Rename, false) == ConflictOutcome::RenameFailed, "");

    // 5) 默认策略：重命名（两全默认在册）。
    cs.add("conflict_default_rename", DEFAULT_CONFLICT == ConflictChoice::Rename, "");

    // 6) 回执收口：3 计划 2 成 1 败 → settled（没有没下文的任务）。
    let mut r = CopyReceipt::new();
    r.plan(0, 1_000);
    r.plan(1, 2_000);
    r.plan(2, 500);
    r.on_success(0, 1_000);
    r.on_success(1, 2_000);
    r.on_failure(2);
    cs.add(
        "receipt_settled",
        r.settled(3) && r.success_permille() == 666 && r.bytes_done == 3_000 && r.bytes_planned == 3_500,
        "",
    );

    // 7) 回执未收口：漏一任务 → settled 假（「没了下文」可检出）。
    let mut r2 = CopyReceipt::new();
    r2.plan(0, 100);
    r2.plan(1, 100);
    r2.on_success(0, 100);
    cs.add("receipt_unsettled_visible", !r2.settled(2), "");

    // 8) 回执零任务诚实：0 → 成功率 0（不冒充 1000）。
    cs.add("receipt_empty_zero", CopyReceipt::new().success_permille() == 0, "");

    // 9) 回执帧 round-trip：四字段全保真（跨重启可查）。
    let mut frame = [0u8; RECEIPT_FRAME_LEN];
    let enc = encode_receipt_frame(&r, &mut frame);
    cs.add(
        "receipt_frame_roundtrip",
        enc && decode_receipt_frame(&frame) == Some((2, 1, 3_000, 3_500)),
        "",
    );

    // 10) 回执帧撕裂必拒：任一字节翻转 → 校验和关拦。
    let mut torn_all = true;
    for i in 0..RECEIPT_FRAME_LEN {
        let mut t = frame;
        t[i] ^= 0x33;
        torn_all &= decode_receipt_frame(&t).is_none();
    }
    cs.add("receipt_frame_tears", torn_all, "");

    // 11) 账目矛盾帧拒：有结局无计划不出门（帧面账目自洽）。
    let mut bad = CopyReceipt::new();
    bad.on_success(0, 100);
    cs.add("receipt_contradiction_rejected", !encode_receipt_frame(&bad, &mut frame), "");

    // 12) 批次四队列贯通：批量入队走 CopyQueue（容量 8 与批量 16 的关系：
    //     16 选件分两批入队——接口消费面）。
    let mut q = super::romount::CopyQueue::new();
    let mut enqueued = 0;
    for _ in 0..10u32 {
        if q.enqueue(CopyFallbackJob::new(b'E', b"batch-item")) {
            enqueued += 1;
        }
    }
    cs.add("batch_to_queue_handoff", enqueued == super::romount::COPY_QUEUE_CAP && q.n == super::romount::COPY_QUEUE_CAP, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn receipt_all_fail_honest() {
        // 全败回执：成功率 0、收口真（失败也是结局——不悬空）。
        let mut r = CopyReceipt::new();
        r.plan(0, 100);
        r.plan(1, 100);
        r.on_failure(0);
        r.on_failure(1);
        assert!(r.settled(2));
        assert_eq!(r.success_permille(), 0);
        assert_eq!(r.bytes_done, 0);
    }

    #[test]
    fn receipt_frame_zero_state() {
        // 零计划零结局：合法空回执可编解码（空回执也是回执）。
        let r = CopyReceipt::new();
        let mut frame = [0u8; RECEIPT_FRAME_LEN];
        assert!(encode_receipt_frame(&r, &mut frame));
        assert_eq!(decode_receipt_frame(&frame), Some((0, 0, 0, 0)));
    }

    #[test]
    fn batch_reset_allows_repick() {
        // 清空后可重选满额（复用语义）。
        let mut b = BatchPicker::new();
        for _ in 0..BATCH_CAP {
            b.pick(1);
        }
        b.reset();
        assert!(b.pick(2));
        assert_eq!(b.count, 1);
    }
}
