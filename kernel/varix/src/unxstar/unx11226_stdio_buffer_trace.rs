//! UNX-F11226 glibc stdio 缓冲与 write 消费（0x1f826 段）
//!
//! 锚点：UNX-F11226-J1 stdio 行缓冲/全缓冲/无缓冲三态的 write 触发时序判据
//! 绿，与基准机输出字节序一致。
//! 域：CoRun·UNX-C5-B02——三种模式共用一本账，差异只在「何时落地」：
//! 行缓冲等换行或超容，全缓冲等超容或收尾，无缓冲每调必落。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f826;
pub const SB_MODE: u32 = SEG_BASE + 0x01; // 模式非法
pub const SB_ZERO: u32 = SEG_BASE + 0x02; // 空写
pub const SB_FLUSH: u32 = SEG_BASE + 0x03; // 空缓冲冲洗
pub const SB_SEQ: u32 = SEG_BASE + 0x04; // write 序不成立
pub const SB_PENDING: u32 = SEG_BASE + 0x05; // 收尾仍有滞留
pub const SB_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const SB_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 三种缓冲模式。
pub const MODE_LINE: u8 = 0;
pub const MODE_FULL: u8 = 1;
pub const MODE_NONE: u8 = 2;
pub const MODES: [&str; 3] = ["line", "full", "none"];
/// 缓冲容量与调用上限。
pub const CAP: u32 = 4096;
pub const MAX_CALLS: usize = 16;
pub const SB_TAG: u64 = 0x1f826d3;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一次 stdio 调用及其落地字节数。
#[derive(Clone, Debug)]
pub struct Emit {
    pub name: &'static str,
    pub want: u32,
    pub wrote: u32,
    pub newline: bool,
}

/// 单模式写入轨迹。
#[derive(Clone, Debug)]
pub struct StdioTrace {
    pub mode: u8,
    pub cap: u32,
    pub pending: u32,
    pub writes: u32,
    pub bytes: u32,
    pub flushed: u32,
    pub emits: Vec<Emit>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl StdioTrace {
    pub fn new(mode: u8, cap: u32) -> StdioTrace {
        StdioTrace {
            mode,
            cap,
            pending: 0,
            writes: 0,
            bytes: 0,
            flushed: 0,
            emits: Vec::new(),
            frozen: false,
            root: None,
        }
    }

    pub fn mode_name(&self) -> &'static str {
        if self.mode == MODE_LINE {
            MODES[0]
        } else if self.mode == MODE_FULL {
            MODES[1]
        } else if self.mode == MODE_NONE {
            MODES[2]
        } else {
            "?"
        }
    }

    /// 是否到落地时机。
    pub fn due(&self, len: u32, newline: bool) -> bool {
        let total = self.pending.saturating_add(len);
        if self.mode == MODE_LINE {
            newline || total > self.cap
        } else if self.mode == MODE_FULL {
            total > self.cap
        } else {
            true
        }
    }

    /// 一次写调用：空写/非法模式/冻结三路拒。
    pub fn emit(&mut self, name: &'static str, len: u32, newline: bool) -> Result<u32, u32> {
        if self.frozen {
            return Err(SB_FROZEN);
        }
        if len == 0 {
            return Err(SB_ZERO);
        }
        if self.mode > MODE_NONE {
            return Err(SB_MODE);
        }
        if self.emits.len() >= MAX_CALLS {
            return Err(SB_OVER);
        }
        let mut wrote = 0u32;
        if self.due(len, newline) {
            wrote = self.pending.saturating_add(len);
            self.pending = 0;
            self.writes = self.writes.saturating_add(1);
            self.bytes = self.bytes.saturating_add(wrote);
        } else {
            self.pending = self.pending.saturating_add(len);
        }
        self.emits.push(Emit { name, want: len, wrote, newline });
        Ok(wrote)
    }

    /// 显式冲洗：空缓冲拒。
    pub fn flush(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(SB_FROZEN);
        }
        if self.pending == 0 {
            return Err(SB_FLUSH);
        }
        let n = self.pending;
        self.pending = 0;
        self.writes = self.writes.saturating_add(1);
        self.bytes = self.bytes.saturating_add(n);
        self.flushed = self.flushed.saturating_add(n);
        Ok(n)
    }

    /// 收尾：全缓冲把滞留落一次；行/无缓冲要求本来无滞留。
    pub fn finish(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(SB_FROZEN);
        }
        if self.pending == 0 {
            return Ok(0);
        }
        if self.mode == MODE_FULL {
            return self.flush();
        }
        Err(SB_PENDING)
    }

    pub fn ordered(&self) -> bool {
        let mut k = 0usize;
        let mut landed = 0u32;
        while k < self.emits.len() {
            landed = landed.saturating_add(self.emits[k].wrote);
            k += 1;
        }
        landed.saturating_add(self.flushed) == self.bytes
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(SB_FROZEN);
        }
        if self.pending != 0 {
            return Err(SB_PENDING);
        }
        if self.writes == 0 {
            return Err(SB_SEQ);
        }
        if !self.ordered() {
            return Err(SB_SEQ);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("{} writes {} bytes {} pending {}", self.mode_name(), self.writes, self.bytes, self.pending)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = SB_TAG;
        h = (h ^ self.mode as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.cap as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.emits.len() {
            let b = self.emits[k].name.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.emits[k].want as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.emits[k].wrote as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.bytes as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 行缓冲：一次带换行的 printf。
pub fn line_trace() -> StdioTrace {
    let mut t = StdioTrace::new(MODE_LINE, CAP);
    let _ = t.emit("printf", 6, true);
    t
}

/// 全缓冲：三次无换行写 + 收尾冲洗。
pub fn full_trace() -> StdioTrace {
    let mut t = StdioTrace::new(MODE_FULL, CAP);
    let _ = t.emit("fwrite", 3, false);
    let _ = t.emit("fwrite", 3, false);
    let _ = t.emit("fwrite", 2, false);
    let _ = t.finish();
    t
}

/// 无缓冲：三次 puts 立即落地。
pub fn none_trace() -> StdioTrace {
    let mut t = StdioTrace::new(MODE_NONE, CAP);
    let _ = t.emit("puts", 3, false);
    let _ = t.emit("puts", 3, false);
    let _ = t.emit("puts", 2, false);
    t
}

/// 四路对抗：空写、非法模式、空缓冲冲洗、滞留即冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = StdioTrace::new(MODE_LINE, CAP);
    out[0] = a.emit("printf", 0, true) == Err(SB_ZERO);
    let mut b = StdioTrace::new(3, CAP);
    out[1] = b.emit("printf", 4, true) == Err(SB_MODE);
    let mut c = StdioTrace::new(MODE_FULL, CAP);
    out[2] = c.flush() == Err(SB_FLUSH);
    let mut d = StdioTrace::new(MODE_FULL, CAP);
    let _ = d.emit("fwrite", 3, false);
    out[3] = d.freeze() == Err(SB_PENDING);
    out
}

/// 判据入口。
pub fn run_unx11226_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11226");
    let mut l = line_trace();
    let mut f = full_trace();
    let mut n = none_trace();
    let rl = l.freeze().unwrap_or(0);
    let rf = f.freeze().unwrap_or(0);
    let rn = n.freeze().unwrap_or(0);

    s.add("J1-行缓冲换行", l.writes == 1 && l.bytes == 6, "带换行的一次 printf 落地一次六字节");
    s.add("J2-行缓冲滞留", {
        let mut x = StdioTrace::new(MODE_LINE, CAP);
        let _ = x.emit("printf", 5, false);
        x.pending == 5 && x.writes == 0
    }, "无换行的短写滞留缓冲、不产生 write");
    s.add("J3-行缓冲超容", {
        let mut x = StdioTrace::new(MODE_LINE, CAP);
        x.emit("printf", CAP + 1, false).unwrap_or(0) == CAP + 1 && x.writes == 1
    }, "超过容量的无换行写立即落地");
    s.add("J4-全缓冲滞留", f.writes == 1 && f.emits.len() == 3, "全缓冲三次写只落地一次");
    s.add("J5-全缓冲八字节", f.bytes == 8, "收尾冲洗把八字节一次写尽");
    s.add("J6-无缓冲立即", n.writes == 3 && n.bytes == 8, "无缓冲三次调用产生三次 write");
    s.add("J7-落地序", l.ordered() && f.ordered() && n.ordered(), "三本账的落地字节与写调用序自洽");
    s.add("J8-滞留归零", l.pending == 0 && f.pending == 0 && n.pending == 0, "三态收尾后滞留均为零");
    s.add("J9-冻存根", rl != 0 && rf != 0 && rn != 0, "三本账冻存根均非零");

    s.add("J10-冻后写拒", l.emit("printf", 1, true) == Err(SB_FROZEN), "冻存后写拒为 SB_FROZEN");
    s.add("J11-冻后冲拒", f.flush() == Err(SB_FROZEN), "冻存后冲洗拒为 SB_FROZEN");
    s.add("J12-空写拒", {
        let mut x = StdioTrace::new(MODE_NONE, CAP);
        x.emit("puts", 0, false) == Err(SB_ZERO)
    }, "零字节写拒为 SB_ZERO");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "空写/非法模式/空冲/滞留冻存四路全拒");
    s.add_dyn(
        "J14-三态明细",
        l.bytes == 6 && f.bytes == 8 && n.bytes == 8,
        &format!("line {}B/{}w，full {}B/{}w，none {}B/{}w", l.bytes, l.writes, f.bytes, f.writes, n.bytes, n.writes),
    );
    s.add_dyn(
        "J15-账根",
        rl == l.fold() && rf == f.fold() && rn == n.fold(),
        &format!("三根 0x{:x}/0x{:x}/0x{:x} 与重折叠一致", rl, rf, rn),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11226_stdio_buffer_flow() {
        let mut l = line_trace();
        let mut f = full_trace();
        assert_eq!(l.writes, 1);
        assert_eq!(f.writes, 1);
        assert!(l.freeze().unwrap_or(0) != 0);
        assert!(f.freeze().unwrap_or(0) != 0);
        assert_eq!(l.emit("printf", 1, true), Err(SB_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11226_checks_green() {
        let s = run_unx11226_checks();
        assert!(s.all_passed(), "UNX-F11226 判据未全绿");
    }
}
