//! F174 诊断快照键 · 批次三深化（secstar · G-G-04）。
//!
//! 批次三功能面（主册判据「落盘 <2s / 帧率无感 / 脱敏三查」纵深）：
//! - [`SnapshotBuilder`]：快照组装器——寄存器组/栈摘要/日志尾三段定长
//!   组装 + 载荷字节数账（<2s 落盘的体积面：先算后写不盲写）；
//! - [`Redactor`]：脱敏引擎——路径/用户名/序列号三类模式扫描替换
//!   （脱敏三查的执行面：注入敏感样本 → 出口零残留）；
//! - [`DeltaRing`]：差分环——相邻快照同块跳过（重复 8MB 全量写是
//!   帧率杀手——差分把写量压到变化面）；
//! - [`payload_budget_ok`]：载荷预算判定——快照 ≤ SNAPSHOT_CAP_BYTES
//!   才可落盘（超限截断并标注，不静默膨胀）。
//!
//! 零堆纪律：定长段 + 定长环，无 alloc。

use super::diagsnap::{STACK_FRAMES, STORE_CAP, STORE_CAP_TIGHT, THROTTLE_WINDOW_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 快照组装器
// ---------------------------------------------------------------------------

/// 寄存器组段长（16 通用寄存器 × 8B）。
pub const REGS_LEN: usize = 16 * 8;
/// 日志尾段长。
pub const LOG_TAIL: usize = 128;
/// 组装段数。
pub const SEGMENTS: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct SnapshotBuilder {
    pub regs: [u8; REGS_LEN],
    pub stack: [u64; STACK_FRAMES],
    pub log_tail: [u8; LOG_TAIL],
    pub regs_used: usize,
    pub stack_used: usize,
    pub log_used: usize,
}

impl SnapshotBuilder {
    pub const fn new() -> SnapshotBuilder {
        SnapshotBuilder { regs: [0; REGS_LEN], stack: [0; STACK_FRAMES], log_tail: [0; LOG_TAIL], regs_used: 0, stack_used: 0, log_used: 0 }
    }

    pub fn set_regs(&mut self, src: &[u8]) {
        let n = src.len().min(REGS_LEN);
        self.regs[..n].copy_from_slice(&src[..n]);
        self.regs_used = n;
    }

    pub fn set_stack(&mut self, frames: &[u64]) {
        let n = frames.len().min(STACK_FRAMES);
        self.stack[..n].copy_from_slice(&frames[..n]);
        self.stack_used = n;
    }

    pub fn set_log(&mut self, src: &[u8]) {
        let n = src.len().min(LOG_TAIL);
        self.log_tail[..n].copy_from_slice(&src[..n]);
        self.log_used = n;
    }

    /// 载荷字节数账（三段实长和——写盘前的体积预估）。
    pub fn payload_bytes(&self) -> usize {
        self.regs_used + self.stack_used * 8 + self.log_used
    }
}

/// 载荷预算判定：≤ 8MB 才可落盘。
pub fn payload_budget_ok(bytes: usize) -> bool {
    bytes <= super::diagsnap::SNAPSHOT_CAP_BYTES
}

// ---------------------------------------------------------------------------
// 脱敏引擎（三类模式）
// ---------------------------------------------------------------------------

/// 脱敏替换符。
pub const REDACT_MARK: u8 = b'*';

/// 在缓冲中把 `pattern` 出现处替换为 `REDACT_MARK`（原位、返回替换次数）。
fn redact_pattern(buf: &mut [u8], pattern: &[u8]) -> usize {
    if pattern.is_empty() || pattern.len() > buf.len() {
        return 0;
    }
    let mut hits = 0;
    let mut i = 0;
    while i + pattern.len() <= buf.len() {
        if &buf[i..i + pattern.len()] == pattern {
            for b in &mut buf[i..i + pattern.len()] {
                *b = REDACT_MARK;
            }
            hits += 1;
            i += pattern.len();
        } else {
            i += 1;
        }
    }
    hits
}

/// 脱敏三查入口：路径（反斜杠绝对路径）、用户名、序列号三类一次扫净。
/// 返回 (替换总数, 缓冲)。三查口径：出口零残留。
pub fn redact_all(buf: &mut [u8], username: &[u8], serial: &[u8]) -> usize {
    let mut total = 0;
    // 路径类：所有 "X:\\" 盘符绝对路径头替换（C:\、D:\…）。
    for drive in b'A'..=b'Z' {
        let mut pat = [0u8; 3];
        pat[0] = drive;
        pat[1] = b':';
        pat[2] = b'\\';
        total += redact_pattern(buf, &pat);
    }
    total += redact_pattern(buf, username);
    total += redact_pattern(buf, serial);
    total
}

/// 零残留验证：三类模式在出口缓冲中均不可再寻获。
pub fn redaction_clean(buf: &[u8], username: &[u8], serial: &[u8]) -> bool {
    let contains = |hay: &[u8], needle: &[u8]| -> bool {
        needle.is_empty()
            || hay.windows(needle.len().max(1)).any(|w| w == needle)
    };
    let drive_leak = (b'A'..=b'Z').any(|d| {
        let pat = [d, b':', b'\\'];
        buf.windows(3).any(|w| w == pat)
    });
    !drive_leak && !contains(buf, username) && !contains(buf, serial)
}

// ---------------------------------------------------------------------------
// 差分环（相邻快照同块跳过）
// ---------------------------------------------------------------------------

/// 差分块粒度（1KB——变化面小于块则整块照写，简单可靠）。
pub const DELTA_BLOCK: usize = 1024;

/// 差分统计：新旧快照逐块比对 → 需写块数与节省千分比。
pub fn delta_stats(old: &[u8], new: &[u8]) -> (usize, u32) {
    let len = old.len().min(new.len()) / DELTA_BLOCK;
    let mut changed = 0;
    for b in 0..len {
        if old[b * DELTA_BLOCK..(b + 1) * DELTA_BLOCK] != new[b * DELTA_BLOCK..(b + 1) * DELTA_BLOCK] {
            changed += 1;
        }
    }
    let saved = if len == 0 { 0 } else { ((len - changed) * 1_000 / len) as u32 };
    (changed, saved)
}

// ---------------------------------------------------------------------------
// 差分环存储（继承主层容量纪律的批次三视角）
// ---------------------------------------------------------------------------

/// 差分环：只存差分统计行（日/写块数/节省 ‰）——全量快照归主层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeltaRow {
    pub snap_id: u64,
    pub blocks_written: usize,
    pub saved_permille: u32,
}

pub struct DeltaRing {
    rows: [Option<DeltaRow>; 16],
    pub n: usize,
}

impl DeltaRing {
    pub const fn new() -> DeltaRing {
        DeltaRing { rows: [const { None }; 16], n: 0 }
    }

    pub fn push(&mut self, row: DeltaRow) -> bool {
        if self.n >= 16 {
            return false;
        }
        self.rows[self.n] = Some(row);
        self.n += 1;
        true
    }

    /// 平均节省 ‡（首条无前照全量写——不计入节省均值）。
    pub fn mean_saved(&self) -> u32 {
        if self.n <= 1 {
            return 0;
        }
        let sum: u32 = self.rows[1..self.n].iter().flatten().map(|r| r.saved_permille).sum();
        sum / (self.n - 1) as u32
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diagsnap_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F174-b3");

    // 1) 组装器三段：寄存器/栈/日志尾各段截断语义（超长钳段长）。
    let mut b = SnapshotBuilder::new();
    let big = [0xAAu8; REGS_LEN + 10];
    b.set_regs(&big);
    b.set_stack(&[1u64; STACK_FRAMES + 4]);
    b.set_log(&[0xBBu8; LOG_TAIL + 10]);
    cs.add(
        "builder_clamps",
        b.regs_used == REGS_LEN && b.stack_used == STACK_FRAMES && b.log_used == LOG_TAIL && b.regs[REGS_LEN - 1] == 0xAA,
        "",
    );

    // 2) 载荷账：三段实长和（写盘前体积可预估——先算后写）。
    let mut b2 = SnapshotBuilder::new();
    b2.set_regs(&[1u8; 64]);
    b2.set_stack(&[2u64; 8]);
    b2.set_log(&[3u8; 32]);
    cs.add("payload_bytes_account", b2.payload_bytes() == 64 + 64 + 32, "");

    // 3) 预算线：0 与 8MB 恰好可落盘、超线拒。
    cs.add(
        "payload_budget",
        payload_budget_ok(0) && payload_budget_ok(super::diagsnap::SNAPSHOT_CAP_BYTES) && !payload_budget_ok(super::diagsnap::SNAPSHOT_CAP_BYTES + 1),
        "",
    );

    // 4) 脱敏三查：注入路径/用户名/序列号 → 全部替换且计数正确
    //   （C:\ ×1 + varia ×1 + SN-12345 ×1 = 3 处）。
    let mut buf = *b"C:\\Users\\varia\\report.docx SN-12345 done";
    let hits = redact_all(&mut buf, b"varia", b"SN-12345");
    cs.add("redact_three_kinds", hits == 3 && redaction_clean(&buf, b"varia", b"SN-12345"), "");

    // 5) 出口零残留：原始敏感串在出口缓冲不可寻获（三查的验收面）。
    let raw = core::str::from_utf8(&buf).unwrap_or("");
    cs.add("redact_zero_residual", !raw.contains("varia") && !raw.contains("SN-12345") && !raw.contains("C:\\"), "");

    // 6) 脱敏保结构：分隔符与文件名骨干保留（可读性不牺牲）。
    cs.add("redact_keeps_shape", raw.contains("Users") && raw.contains("report.docx") && raw.contains('*'), "");

    // 7) 空模式不炸：空用户名/序列号零替换（模式空=无靶）。
    let mut buf3 = *b"C:\\x";
    let h3 = redact_all(&mut buf3, b"", b"");
    cs.add("redact_empty_patterns", h3 == 1, "");

    // 8) 差分统计：单块变化 → 1 块写、其余省（99.9% 量级节省）。
    let old = [0u8; 4 * DELTA_BLOCK];
    let mut new = old;
    new[2 * DELTA_BLOCK + 5] = 0xFF;
    let (blocks, saved) = delta_stats(&old, &new);
    cs.add("delta_one_block", blocks == 1 && saved == 750, "");

    // 9) 差分零变化：全同快照 → 0 块写、节省 1000‰（最优面）。
    let same = [7u8; 2 * DELTA_BLOCK];
    let (blocks2, saved2) = delta_stats(&same, &same);
    cs.add("delta_identical", blocks2 == 0 && saved2 == 1_000, "");

    // 10) 差分环：首条全量不计均值、后续差分行入均值（统计口径诚实）。
    let mut ring = DeltaRing::new();
    ring.push(DeltaRow { snap_id: 1, blocks_written: 8192, saved_permille: 0 });
    ring.push(DeltaRow { snap_id: 2, blocks_written: 10, saved_permille: 900 });
    ring.push(DeltaRow { snap_id: 3, blocks_written: 20, saved_permille: 800 });
    cs.add("delta_ring_mean", ring.mean_saved() == 850, "");

    // 11) 差分环满容诚实拒（16 上限——与主层 STORE_CAP 纪律同源）。
    let mut ring2 = DeltaRing::new();
    let mut all = true;
    for i in 0..16 {
        all &= ring2.push(DeltaRow { snap_id: i, blocks_written: 0, saved_permille: 0 });
    }
    cs.add("delta_ring_cap", all && ring2.n == 16 && !ring2.push(DeltaRow { snap_id: 99, blocks_written: 0, saved_permille: 0 }), "");

    // 12) 主册常量贯通：栈 16 帧 / 节流 10s / 紧张档 5 槽一处一事实。
    cs.add(
        "consts_aligned",
        STACK_FRAMES == 16 && THROTTLE_WINDOW_MS == 10_000 && STORE_CAP == 20 && STORE_CAP_TIGHT == 5,
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
    fn redaction_survives_adjacent_hits() {
        // 相邻两次命中不漏（步进跳过已替换区——不回吞替换符）。
        let mut buf = *b"varia varia";
        let hits = redact_all(&mut buf, b"varia", b"");
        assert_eq!(hits, 2);
        assert_eq!(&buf, b"***** *****");
    }

    #[test]
    fn delta_partial_change_spectrum() {
        // 变化面谱系：0/1/2/4 块变化 → 写块数逐级对。
        for changed in [0usize, 1, 2, 4] {
            let mut a = [0u8; 4 * DELTA_BLOCK];
            let mut b2 = a;
            for c in 0..changed {
                b2[c * DELTA_BLOCK] = 1;
            }
            let (blocks, _) = delta_stats(&a, &b2);
            assert_eq!(blocks, changed, "changed={changed}");
        }
    }

    #[test]
    fn builder_is_idempotent_on_rewind() {
        // 重设更短内容后旧残留不进载荷（used 字段即真相）。
        let mut b = SnapshotBuilder::new();
        b.set_log(&[0xEEu8; 100]);
        b.set_log(&[0x11u8; 10]);
        assert_eq!(b.log_used, 10);
        assert_eq!(b.payload_bytes(), 10);
        assert_eq!(b.log_tail[..10], [0x11; 10], "新内容在位");
    }
}
