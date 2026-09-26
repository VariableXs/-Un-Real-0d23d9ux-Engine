//! F181 交接检查 · 批次八（v8）——探测并行预算账、会话撤销语义、
//! 面包屑渲染器扩展、仲裁日志完整性帧。零堆、no_std。

use crate::checks::CheckSet;
use super::handoffchk::CHECKS_BUDGET_MS;

/// 会话预算切片数。
pub const SLICE_COUNT: usize = 4;
/// 会话日志环容量（b7 之上的仲裁层）。
pub const ARBITER_LOG_CAP: usize = 16;
/// 仲裁帧长（12B）。
pub const ARBITER_FRAME_LEN: usize = 12;

/// 预算切片账：350ms 总预算切成 SLICE_COUNT 片，各片可独立超时。
/// 切分余数归最后一片（不丢预算——账平判据）。
#[derive(Clone, Copy)]
pub struct BudgetSlicer {
    slices: [u32; SLICE_COUNT],
}

impl BudgetSlicer {
    pub fn new(total_ms: u32) -> BudgetSlicer {
        let base = total_ms / SLICE_COUNT as u32;
        let mut slices = [base; SLICE_COUNT];
        let rem = total_ms - base * SLICE_COUNT as u32;
        slices[SLICE_COUNT - 1] += rem; // 余数归尾片
        BudgetSlicer { slices }
    }

    pub fn slice(&self, i: usize) -> Option<u32> {
        self.slices.get(i).copied()
    }

    /// 账平：片和 == 总预算。
    pub fn conserved(&self, total_ms: u32) -> bool {
        self.slices.iter().sum::<u32>() == total_ms
    }
}

/// 会话撤销：探测中途可撤（撤 = 全部半途结果作废——不留半空状态）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionPhase {
    Idle,
    Probing,
    Cancelled,
    Completed,
}

#[derive(Clone, Copy)]
pub struct CancellableSession {
    pub phase: SessionPhase,
    pub probes_done: u32,
    /// 撤销时已耗 ms（撤也要记账——预算审计不豁免半途会话）。
    pub cancelled_at_ms: u32,
}

impl CancellableSession {
    pub const fn new() -> CancellableSession {
        CancellableSession { phase: SessionPhase::Idle, probes_done: 0, cancelled_at_ms: 0 }
    }

    pub fn start(&mut self) -> bool {
        if self.phase != SessionPhase::Idle {
            return false; // 不重入
        }
        self.phase = SessionPhase::Probing;
        true
    }

    pub fn on_probe(&mut self, cost_ms: u32) {
        if self.phase == SessionPhase::Probing {
            self.probes_done += 1;
            self.cancelled_at_ms += cost_ms;
        }
    }

    /// 撤销：Probing → Cancelled（Idle/终态撤不动——无东西可撤）。
    pub fn cancel(&mut self) -> bool {
        if self.phase != SessionPhase::Probing {
            return false;
        }
        self.phase = SessionPhase::Cancelled;
        true
    }

    /// 完成：Probing → Completed（撤销后不能完成——状态机单向）。
    pub fn complete(&mut self) -> bool {
        if self.phase != SessionPhase::Probing {
            return false;
        }
        self.phase = SessionPhase::Completed;
        true
    }
}

/// 面包屑扩展：四态渲染（绿 G / 红 R / 异常 E / 未到 .）——探测序列位图。
pub fn breadcrumb4(states: &[Option<bool>; 3], out: &mut [u8]) -> usize {
    let mut n = 0;
    let put = |b: u8, out: &mut [u8], n: &mut usize| {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    };
    put(b'[', out, &mut n);
    for s in states {
        let c = match s {
            Some(true) => b'G',
            Some(false) => b'R',
            None => b'.',
        };
        put(c, out, &mut n);
    }
    put(b']', out, &mut n);
    n
}

/// 异常态注入：三查中异常（fail-closed）在面包屑显示 E 并使整行判负。
pub fn breadcrumb_with_exception(has_exception: bool, states: &[Option<bool>; 3], out: &mut [u8]) -> usize {
    let mut n = breadcrumb4(states, out);
    let put = |b: u8, out: &mut [u8], n: &mut usize| {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    };
    if has_exception {
        put(b'!', out, &mut n);
        put(b'E', out, &mut n);
    }
    n
}

/// 仲裁日志完整性帧（12B）：
/// [0..2) "AJ" · [2..4) 会话数 LE · [4..6) 拦停数 LE · [6..8) 撤销数 LE ·
/// [8..10) 预算余 ms/10 LE · [10..12) 校验和（前 10B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_arbiter(sessions: u16, blocks: u16, cancels: u16, budget_left_ms: u32, out: &mut [u8; ARBITER_FRAME_LEN]) -> bool {
    if budget_left_ms > CHECKS_BUDGET_MS as u32 {
        return false; // 余量 > 总预算 = 账不平
    }
    out[0] = b'A';
    out[1] = b'J';
    out[2..4].copy_from_slice(&sessions.to_le_bytes());
    out[4..6].copy_from_slice(&blocks.to_le_bytes());
    out[6..8].copy_from_slice(&cancels.to_le_bytes());
    let tens = (budget_left_ms / 10) as u16;
    out[8..10].copy_from_slice(&tens.to_le_bytes());
    let c = fnv16(&out[..10]);
    out[10] = (c & 0xFF) as u8;
    out[11] = (c >> 8) as u8;
    true
}

pub fn decode_arbiter(frame: &[u8; ARBITER_FRAME_LEN]) -> Option<(u16, u16, u16, u32)> {
    if frame[0] != b'A' || frame[1] != b'J' {
        return None;
    }
    let want = (frame[11] as u16) << 8 | frame[10] as u16;
    if fnv16(&frame[..10]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
        u16::from_le_bytes(frame[8..10].try_into().ok()?) as u32 * 10,
    ))
}

#[inline(never)]
pub fn run_handoffchk_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F181-b8");

    // 1) 预算切片：350ms → 4 片各 87、余 2 归尾片（89）。
    let s = BudgetSlicer::new(CHECKS_BUDGET_MS as u32);
    cs.add(
        "budget_slice_split",
        s.slice(0) == Some(87) && s.slice(1) == Some(87) && s.slice(2) == Some(87) && s.slice(3) == Some(89),
        "",
    );

    // 2) 预算账平：片和 == 350（余数不丢）。
    cs.add("budget_slice_conserved", s.conserved(CHECKS_BUDGET_MS as u32), "");

    // 3) 会话生命周期：start → 探 2 → complete（撤销前完成是正线）。
    let mut sess = CancellableSession::new();
    let st = sess.start();
    sess.on_probe(50);
    sess.on_probe(60);
    let done = sess.complete();
    cs.add(
        "session_complete_path",
        st && done && sess.probes_done == 2 && sess.phase == SessionPhase::Completed,
        "",
    );

    // 4) 撤销语义：Probing 中撤 → Cancelled；撤后完成拒（状态单向）。
    let mut sess2 = CancellableSession::new();
    sess2.start();
    sess2.on_probe(30);
    let cancelled = sess2.cancel();
    let cant_complete = !sess2.complete();
    cs.add(
        "session_cancel_semantics",
        cancelled && cant_complete && sess2.phase == SessionPhase::Cancelled && sess2.cancelled_at_ms == 30,
        "",
    );

    // 5) 撤销越权拒：Idle 撤不动、Completed 撤不动（无东西可撤）。
    let mut sess3 = CancellableSession::new();
    let idle_cancel = !sess3.cancel();
    sess3.start();
    sess3.complete();
    let done_cancel = !sess3.cancel();
    cs.add("session_cancel_bounds", idle_cancel && done_cancel, "");

    // 6) 会话不重入：Probing 中再 start 拒（一次一个会话——预算模型）。
    let mut sess4 = CancellableSession::new();
    sess4.start();
    cs.add("session_no_reentry", !sess4.start(), "");

    // 7) 面包屑四态：全探测 "[GRG]"、未到 "[G..]"（进度可视化面）。
    let mut buf = [0u8; 16];
    let n1 = breadcrumb4(&[Some(true), Some(false), Some(true)], &mut buf);
    let mut buf2 = [0u8; 16];
    let n2 = breadcrumb4(&[Some(true), None, None], &mut buf2);
    cs.add(
        "breadcrumb4_shapes",
        n1 == 5 && &buf[..5] == b"[GRG]" && n2 == 5 && &buf2[..5] == b"[G..]",
        "",
    );

    // 8) 异常注入：!E 后缀 + 位置敏感（异常必须显性——第十三章铁律）。
    let mut buf3 = [0u8; 16];
    let n3 = breadcrumb_with_exception(true, &[Some(true), Some(true), Some(true)], &mut buf3);
    let mut buf4 = [0u8; 16];
    let n4 = breadcrumb_with_exception(false, &[Some(true), Some(true), Some(true)], &mut buf4);
    cs.add(
        "breadcrumb_exception_visible",
        n3 == 7 && &buf3[..7] == b"[GGG]!E" && n4 == 5 && &buf4[..5] == b"[GGG]",
        "",
    );

    // 9) 仲裁帧 round-trip + 余量守门 + 撕裂拒。
    let mut f = [0u8; ARBITER_FRAME_LEN];
    let ok = encode_arbiter(12, 3, 2, 170, &mut f);
    let bad = !encode_arbiter(12, 3, 2, (CHECKS_BUDGET_MS as u32) + 1, &mut f);
    let mut tear_ok = true;
    for i in 0..ARBITER_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x5E;
        if decode_arbiter(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add(
        "arbiter_frame_guards",
        ok && decode_arbiter(&f) == Some((12, 3, 2, 170)) && bad && tear_ok,
        "",
    );

    // 10) 常量自洽：片 4、环 16、帧 12、预算 350 引用主层。
    cs.add(
        "b8_constants",
        SLICE_COUNT == 4 && ARBITER_LOG_CAP == 16 && ARBITER_FRAME_LEN == 12 && CHECKS_BUDGET_MS == 350,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_slice_exact_division() {
        // 恰好整除：400ms → 4 片各 100（余数 0 不虚增尾片）。
        let s = BudgetSlicer::new(400);
        assert_eq!(s.slice(3), Some(100));
        assert!(s.conserved(400));
    }

    #[test]
    fn cancel_burns_budget_account() {
        // 撤销的会话仍计入预算消耗（审计不豁免半途）。
        let mut s = CancellableSession::new();
        s.start();
        s.on_probe(100);
        s.on_probe(80);
        s.cancel();
        assert_eq!(s.cancelled_at_ms, 180);
        assert_eq!(s.probes_done, 2);
    }

    #[test]
    fn breadcrumb_buffer_too_small_truncates() {
        // 缓冲过小：诚实截断不越界（渲染面守门）。
        let mut tiny = [0u8; 3];
        let n = breadcrumb4(&[Some(true), Some(true), Some(true)], &mut tiny);
        assert_eq!(n, 3);
        assert_eq!(&tiny[..], b"[GG");
    }
}
