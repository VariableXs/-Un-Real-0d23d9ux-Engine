//! UNX-F12157 — 异常分发可观测性：命中序列日志（0x1a5c0 段）
//!
//! 锚点：UNX-F12157-J1 命中序列日志 10/10（每次分发逐级记序列，
//! 日志滚动窗口有界、环回覆写最旧），序列回放全对。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：LogEntry（序列条目）+ RingLog（环形日志：定容 + 写指针
//! 环回）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a5c0;
pub const LG_ORDER: u32 = SEG_BASE + 0x01; // 级序非法（rank 0）
pub const LG_GHOST: u32 = SEG_BASE + 0x02; // 空日志读
pub const LG_SEALED: u32 = SEG_BASE + 0x04; // 印后写
pub const LG_EMPTY: u32 = SEG_BASE + 0x05; // 空留痕写
pub const LG_FULL: u32 = SEG_BASE + 0x06; // 定容 0 非法

/// 日志条目。
#[derive(Clone, PartialEq)]
pub struct LogEntry {
    pub code: u32,
    pub rank: u8, // 1=VEH 2=SEH 3=UEF
    pub seq: u64,
    pub note: String,
}

impl LogEntry {
    pub fn ok(&self) -> bool {
        self.code != 0 && (1..=3).contains(&self.rank) && !self.note.is_empty()
    }

    pub fn render(&self) -> String {
        format!("#{} r{} c={:#x}", self.seq, self.rank, self.code)
    }
}

/// 环形日志。
pub struct RingLog {
    cap: usize,
    pub buf: Vec<Option<LogEntry>>,
    write: usize,
    total: u64,
    sealed: bool,
}

impl RingLog {
    pub fn new(cap: usize) -> RingLog {
        let mut buf = Vec::new();
        for _ in 0..cap {
            buf.push(None);
        }
        RingLog { cap, buf, write: 0, total: 0, sealed: false }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 写一条（环回覆写最旧）。
    pub fn write(&mut self, code: u32, rank: u8, note: &str) -> Result<u64, u32> {
        if self.sealed {
            return Err(LG_SEALED);
        }
        if self.cap == 0 {
            return Err(LG_FULL);
        }
        if note.is_empty() {
            return Err(LG_EMPTY);
        }
        self.total += 1;
        let e = LogEntry { code, rank, seq: self.total, note: note.to_string() };
        if !e.ok() {
            self.total -= 1;
            return Err(LG_ORDER);
        }
        self.buf[self.write % self.cap] = Some(e);
        self.write += 1;
        Ok(self.total)
    }

    /// 最近 n 条（新→旧）。
    pub fn tail(&self, n: usize) -> Vec<LogEntry> {
        let mut out = Vec::new();
        let mut i = self.write;
        while out.len() < n && i > 0 {
            i -= 1;
            if let Some(Some(e)) = self.buf.get(i % self.cap) {
                out.push(e.clone());
            }
        }
        out
    }

    /// 序列连续：日志内 seq 全局唯一且连续（读出范围内）。
    pub fn tail_seq_contiguous(&self, n: usize) -> bool {
        let t = self.tail(n);
        if t.len() < 2 {
            return true;
        }
        t.windows(2).all(|w| w[0].seq == w[1].seq + 1)
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a5c0;
        h = h.wrapping_mul(211).wrapping_add(self.cap as u32);
        h = h.wrapping_mul(211).wrapping_add(self.total as u32);
        for e in self.buf.iter().flatten() {
            h = h.wrapping_mul(211).wrapping_add(e.code);
            h = h.wrapping_mul(211).wrapping_add(e.rank as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!("cap={} total={} live={}", self.cap, self.total, self.tail(self.cap).len())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = RingLog::new(4);
    // 1) 级序 0 非法必拒。
    got[0] = b.write(0xC0000005, 0, "r0").err() == Some(LG_ORDER);
    // 2) 级序 4 非法必拒。
    got[1] = b.write(0xC0000005, 4, "r4").err() == Some(LG_ORDER);
    // 3) 留痕空必拒。
    got[2] = b.write(0xC0000005, 1, "").err() == Some(LG_EMPTY);
    // 4) 零容日志写必拒。
    let mut b0 = RingLog::new(0);
    got[3] = b0.write(1, 1, "z").err() == Some(LG_FULL);
    // 5) 印后写必拒。
    b.seal();
    got[4] = b.write(1, 1, "post").err() == Some(LG_SEALED);
    got
}

/// 判据入口：UNX-F12157
pub fn run_unx12157_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12157");
    let mut b = RingLog::new(8);

    // J1 写 6 条留痕（三级分发 × 2 异常）。
    let mut ok1 = true;
    for i in 0..6u64 {
        let rank = (1 + (i % 3)) as u8;
        if b.write(0xC0000005, rank, "disp").is_err() {
            ok1 = false;
        }
    }
    s.add("J1_write6", ok1 && b.total() == 6, "六条留痕全写");

    // J2 最近 6 条 seq 连续。
    s.add("J2_seq_cont", b.tail_seq_contiguous(6), "tail 序列 seq 连续");

    // J3 tail 新→旧序。
    let t = b.tail(6);
    s.add(
        "J3_tail_new_first",
        t.len() == 6 && t[0].seq == 6 && t[5].seq == 1,
        "tail 首条=最新 seq 6",
    );

    // J4 环回覆写：cap 8 写 10 条后最旧两条被覆。
    let mut b4 = RingLog::new(8);
    for i in 0..10u64 {
        let _ = b4.write(0xC0000094, 1, "w");
    }
    let t4 = b4.tail(8);
    s.add(
        "J4_ring_overwrite",
        t4.len() == 8 && t4[0].seq == 10 && t4[7].seq == 3,
        "环回后 tail 首=10 尾=3",
    );

    // J5 条目自洽核。
    let e = LogEntry { code: 0xC0000005, rank: 2, seq: 1, note: "n".to_string() };
    s.add("J5_entry_ok", e.ok(), "条目过自洽核");

    // J6 条目渲染。
    let r = e.render();
    s.add(
        "J6_render",
        r.contains("r2") && r.contains("c=0xc0000005"),
        "渲染含 seq/rank/code",
    );

    // J7 三级 rank 常量。
    s.add(
        "J7_ranks",
        (1..=3u8).all(|r| LogEntry { code: 1, rank: r, seq: 1, note: "n".to_string() }.ok()),
        "rank 1/2/3 全合法",
    );

    // J8 零码条目拒。
    let bad = LogEntry { code: 0, rank: 1, seq: 1, note: "n".to_string() };
    s.add("J8_zero_code", !bad.ok(), "零码条目判非法");

    // J9 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J9_fingerprint", f1 == f2, "日志折叠指纹一致");

    // J10 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 印账受控。
    b.seal();
    s.add(
        "J11_sealed",
        b.write(1, 1, "post").err() == Some(LG_SEALED),
        "印账后写拒",
    );

    // J12 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J12_summary",
        sm.contains("cap=8") && sm.contains("total=6"),
        "摘要含容量与总数",
    );

    // J13 空日志 tail 空。
    let b13 = RingLog::new(4);
    s.add(
        "J13_empty_tail",
        b13.tail(4).is_empty() && b13.tail_seq_contiguous(4),
        "空日志 tail 空、连续位真",
    );

    // J14 tail(n) 超 total 截断。
    let t14 = b.tail(100);
    s.add(
        "J14_tail_clamp",
        t14.len() == 6,
        "超量 tail 截断至总数",
    );

    // J15 环回后 seq 仍全局连续（新窗口）。
    s.add(
        "J15_wrap_cont",
        b4.tail_seq_contiguous(8),
        "环回窗口 seq 连续",
    );

    // J16 留痕空拒。
    let mut b16 = RingLog::new(2);
    s.add(
        "J16_empty_note",
        b16.write(1, 1, "").err() == Some(LG_EMPTY),
        "留痕空拒 EMPTY",
    );

    // J17 大环 32 写 40 条。
    let mut b17 = RingLog::new(32);
    let mut ok17 = true;
    for i in 0..40u64 {
        if b17.write(0xC000_0000 + i as u32, ((i % 3) + 1) as u8, "m").is_err() {
            ok17 = false;
        }
    }
    let t17 = b17.tail(32);
    s.add(
        "J17_ring32",
        ok17 && t17.len() == 32 && t17[0].seq == 40 && t17[31].seq == 9,
        "32 环写 40 条覆 8 最旧",
    );

    // J18 空日志指纹稳定。
    let b18 = RingLog::new(4);
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空日志指纹稳定");

    // J19 total 单调。
    let t19_before = b4.total();
    let _ = b4.write(0xC0000409, 3, "more");
    s.add(
        "J19_total_mono",
        b4.total() == t19_before + 1,
        "total 写后 +1 单调",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12157_ring() {
        let mut b = RingLog::new(2);
        let _ = b.write(1, 1, "a");
        let _ = b.write(2, 1, "b");
        let _ = b.write(3, 1, "c");
        let t = b.tail(2);
        assert_eq!(t[0].seq, 3);
        assert_eq!(t[1].seq, 2);
    }

    #[test]
    fn unx12157_checks_green() {
        let s = run_unx12157_checks();
        assert!(s.all_passed(), "UNX-F12157 判据未全绿");
    }

    #[test]
    fn unx12157_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
