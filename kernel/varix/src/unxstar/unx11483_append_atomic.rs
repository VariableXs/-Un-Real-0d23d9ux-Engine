//! UNX-F11483 O_APPEND 原子追加（0x20e00 段）
//!
//! `O_APPEND` 的语义不是“先读文件大小、再写到该偏移”，而是把定位与写入
//! 合并成一个不可分割的动作：每次写都从当时的文件末尾开始，两个并发
//! 追加者不会互相覆盖。若调用方自己缓存了旧的大小并据此普通写入，就会
//! 出现经典的丢失更新：后写的数据覆盖先写的数据，文件长度也不再等于
//! 两次写入之和。本模块用带标签的字节块模拟这两个过程并对比其结果。
//!
//! 普通写入允许落在文件内部（覆盖既有字节并计数）、落在末尾之外（形成
//! 零填充的空洞），也允许零长度写入；任何写入只要结束偏移超过上限就
//! 返回 `EFBIG`。追加写入在“当前大小 + 长度”超过上限时返回 `ENOSPC`，
//! 恰好填满上限则成功。零长度追加是合法的空操作，仍返回当前末尾作为
//! 起始偏移。
//!
//! 不变量：`data.len() == size` 且 `size <= MAX_FILE`；追加写入的 `start`
//! 等于写入前的大小，`clobbered` 恒为零；普通写入的 `clobbered` 只统计
//! 被覆盖的、原本位于文件长度之内的非零字节；空洞区域一律清零。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e00;
pub const AP_EID: u32 = SEG_BASE + 0x01;
pub const AP_EORDER: u32 = SEG_BASE + 0x02;
pub const AP_EDUP: u32 = SEG_BASE + 0x03;
pub const AP_EFROZEN: u32 = SEG_BASE + 0x04;
pub const AP_ENOBS: u32 = SEG_BASE + 0x05;
pub const AP_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const AP_EBAL: u32 = SEG_BASE + 0x07;
pub const AP_ESEAL: u32 = SEG_BASE + 0x08;
pub const AP_ENOSPC: u32 = SEG_BASE + 0x09;
pub const AP_EFBIG: u32 = SEG_BASE + 0x0a;
pub const AP_EINVAL: u32 = SEG_BASE + 0x0b;

pub const SAMPLES: usize = 15;
pub const MAX_FILE: u32 = 4096;

pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 写入结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wr {
    pub start: u32,
    pub len: u32,
    pub size: u32,
    pub clobbered: u32,
}

/// 文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub data: Vec<u8>,
    pub size: u32,
}

impl File {
    pub fn empty() -> File {
        File { data: Vec::new(), size: 0 }
    }

    pub fn filled(size: u32, val: u8) -> File {
        let n = if size > MAX_FILE { MAX_FILE } else { size };
        let mut data = Vec::new();
        let mut i = 0u32;
        while i < n {
            data.push(val);
            i += 1;
        }
        File { data, size: n }
    }

    pub fn byte(&self, i: u32) -> Option<u8> {
        self.data.get(i as usize).copied()
    }

    /// 追加：定位与增长一步完成。
    pub fn append(&mut self, tag: u8, len: u32) -> Result<Wr, u32> {
        let start = self.size;
        if len > MAX_FILE.saturating_sub(start) {
            return Err(AP_ENOSPC);
        }
        let mut i = 0u32;
        while i < len {
            self.data.push(tag);
            i += 1;
        }
        self.size = start + len;
        Ok(Wr { start, len, size: self.size, clobbered: 0 })
    }

    /// 普通写入：使用调用方给出的偏移，可能覆盖或形成空洞。
    pub fn write_at(&mut self, off: u32, tag: u8, len: u32) -> Result<Wr, u32> {
        let end = match off.checked_add(len) {
            Some(v) => v,
            None => return Err(AP_EFBIG),
        };
        if end > MAX_FILE {
            return Err(AP_EFBIG);
        }
        while self.data.len() < off as usize {
            self.data.push(0);
        }
        let old_size = self.size;
        let mut clobbered = 0u32;
        let mut i = 0u32;
        while i < len {
            let pos = off + i;
            if pos < old_size {
                if self.data.get(pos as usize).copied().unwrap_or(0) != 0 {
                    clobbered += 1;
                }
            }
            if (self.data.len() as u32) <= pos {
                self.data.push(tag);
            } else if let Some(slot) = self.data.get_mut(pos as usize) {
                *slot = tag;
            }
            i += 1;
        }
        if end > self.size {
            self.size = end;
        }
        Ok(Wr { start: off, len, size: self.size, clobbered })
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.data.len() != (self.size as usize) {
            return Err(AP_EBAL);
        }
        if self.size > MAX_FILE {
            return Err(AP_EBAL);
        }
        if self.data.len() > (MAX_FILE as usize) {
            return Err(AP_EBAL);
        }
        Ok(())
    }
}

/// 基础文件：100 个 0x11 字节。
pub fn base_file() -> File {
    File::filled(100, 0x11)
}

/// 两个追加者交错写入六块，每块 100 字节。
pub fn interleave() -> (File, Wr) {
    let mut f = base_file();
    let tags = [b'A', b'B', b'A', b'B', b'A', b'B'];
    let mut last = Wr { start: 0, len: 0, size: f.size, clobbered: 0 };
    let mut i = 0usize;
    while i < tags.len() {
        if let Some(&t) = tags.get(i) {
            if let Ok(w) = f.append(t, 100) {
                last = w;
            }
        }
        i += 1;
    }
    (f, last)
}

/// 执行场景。
pub fn run_scen(id: u8) -> Result<Wr, u32> {
    match id {
        0 => {
            let mut f = base_file();
            f.append(b'A', 50)
        }
        1 => {
            let mut f = base_file();
            let _ = f.append(b'A', 100)?;
            let _ = f.append(b'B', 100)?;
            f.append(b'A', 100)
        }
        2 => {
            let mut f = base_file();
            let stale = f.size;
            let _ = f.write_at(stale, b'A', 50)?;
            f.write_at(stale, b'B', 50)
        }
        3 => {
            let mut f = base_file();
            let _ = f.append(b'A', 50)?;
            f.append(b'B', 50)
        }
        4 => {
            let mut f = base_file();
            f.write_at(50, b'Z', 20)
        }
        5 => {
            let mut f = base_file();
            f.write_at(150, b'Z', 50)
        }
        6 => {
            let mut f = File::filled(200, 0x22);
            f.append(b'Z', 20)
        }
        7 => {
            let mut f = File::filled(4000, 0x33);
            f.append(b'Z', 200)
        }
        8 => {
            let mut f = base_file();
            f.append(b'Z', 0)
        }
        9 => {
            let mut f = base_file();
            f.write_at(4000, b'Z', 200)
        }
        10 => {
            let (_, w) = interleave();
            Ok(w)
        }
        11 => {
            let mut f = base_file();
            f.write_at(90, b'Z', 20)
        }
        12 => {
            let mut f = File::empty();
            f.append(b'Z', 5)
        }
        13 => {
            let mut f = base_file();
            f.write_at(0, b'Z', 100)
        }
        14 => {
            let mut f = File::filled(4000, 0x44);
            f.append(b'Z', 96)
        }
        _ => Err(AP_EID),
    }
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub start: u32,
    pub len: u32,
    pub size: u32,
    pub clobbered: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub start: u32,
    pub len: u32,
    pub size: u32,
    pub clobbered: u32,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    pub fn register(
        &mut self,
        slot: u8,
        scen: u8,
        ok: bool,
        start: u32,
        len: u32,
        size: u32,
        clobbered: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AP_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(AP_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(AP_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(AP_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, start, len, size, clobbered, code };
        let out = match run_scen(scen) {
            Ok(v) => {
                if !ok || v.start != start || v.len != len || v.size != size || v.clobbered != clobbered {
                    return Err(AP_EMISMATCH);
                }
                Outcome { ok: true, start: v.start, len: v.len, size: v.size, clobbered: v.clobbered, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(AP_EMISMATCH);
                }
                Outcome { ok: false, start: 0, len: 0, size: 0, clobbered: 0, code: low }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }

    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(AP_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(AP_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(AP_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(AP_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok
                        || v.start != o.start
                        || v.len != o.len
                        || v.size != o.size
                        || v.clobbered != o.clobbered
                    {
                        return Err(AP_EMISMATCH);
                    }
                    if v.size > MAX_FILE || v.start > v.size {
                        return Err(AP_EBAL);
                    }
                    if v.clobbered > v.len {
                        return Err(AP_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(AP_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AP_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, u64::from(o.ok));
                h = fold(h, o.start as u64);
                h = fold(h, o.len as u64);
                h = fold(h, o.size as u64);
                h = fold(h, o.clobbered as u64);
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.scen as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal()
            && fresh.ok_count() == self.ok_count()
            && fresh.err_count() == self.err_count()
        {
            Ok(fresh.seal())
        } else {
            Err(AP_ESEAL)
        }
    }
}

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enospc = (AP_ENOSPC & 0xffff) as u16;
    let efbig = (AP_EFBIG & 0xffff) as u16;
    let _ = l.register(0, 0, true, 100, 50, 150, 0, 0);
    let _ = l.register(1, 1, true, 300, 100, 400, 0, 0);
    let _ = l.register(2, 2, true, 100, 50, 150, 50, 0);
    let _ = l.register(3, 3, true, 150, 50, 200, 0, 0);
    let _ = l.register(4, 4, true, 50, 20, 100, 20, 0);
    let _ = l.register(5, 5, true, 150, 50, 200, 0, 0);
    let _ = l.register(6, 6, true, 200, 20, 220, 0, 0);
    let _ = l.register(7, 7, false, 0, 0, 0, 0, enospc);
    let _ = l.register(8, 8, true, 100, 0, 100, 0, 0);
    let _ = l.register(9, 9, false, 0, 0, 0, 0, efbig);
    let _ = l.register(10, 10, true, 600, 100, 700, 0, 0);
    let _ = l.register(11, 11, true, 90, 20, 110, 10, 0);
    let _ = l.register(12, 12, true, 0, 5, 5, 0, 0);
    let _ = l.register(13, 13, true, 0, 100, 100, 100, 0);
    let _ = l.register(14, 14, true, 4000, 96, 4096, 0, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 13 && l.err_count() == 2 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(AP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut f = base_file();
    let a = f.append(b'A', 50);
    let b = f.append(b'B', 25);
    out[1] = a.map(|w| (w.start, w.size)) == Ok((100, 150))
        && b.map(|w| (w.start, w.size)) == Ok((150, 175))
        && f.byte(149) == Some(b'A')
        && f.byte(150) == Some(b'B')
        && f.audit().is_ok();

    let mut stale = base_file();
    let snap = stale.size;
    let _ = stale.write_at(snap, b'A', 50);
    let lost = stale.write_at(snap, b'B', 50);
    let mut atomic = base_file();
    let _ = atomic.append(b'A', 50);
    let keep = atomic.append(b'B', 50);
    out[2] = lost.map(|w| (w.size, w.clobbered)) == Ok((150, 50))
        && keep.map(|w| (w.start, w.size, w.clobbered)) == Ok((150, 200, 0))
        && atomic.byte(149) == Some(b'A')
        && atomic.byte(150) == Some(b'B');

    let mut f = File::filled(4000, 0x44);
    let full = f.append(b'Z', 96);
    let over = f.append(b'Z', 1);
    let zero = f.append(b'Z', 0);
    let big = f.write_at(4090, b'Z', 10);
    out[3] = full.map(|w| (w.start, w.size)) == Ok((4000, 4096))
        && over == Err(AP_ENOSPC)
        && zero.map(|w| (w.start, w.size)) == Ok((4096, 4096))
        && big == Err(AP_EFBIG)
        && f.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11483_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11483_append_atomic");
    let l = build_ledger();
    s.add(
        "AP1-十五行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "AP2-十三应两拒",
        l.ok_count() == 13 && l.err_count() == 2 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "AP3-追加定位末尾",
        run_scen(0).map(|w| (w.start, w.size, w.clobbered)) == Ok((100, 150, 0)),
        "static",
    );
    s.add(
        "AP4-连续追加累加",
        run_scen(1).map(|w| (w.start, w.size)) == Ok((300, 400)),
        "static",
    );
    s.add(
        "AP5-陈旧偏移丢失更新",
        run_scen(2).map(|w| (w.start, w.size, w.clobbered)) == Ok((100, 150, 50)),
        "static",
    );
    s.add(
        "AP6-原子追加不覆盖",
        run_scen(3).map(|w| (w.start, w.size, w.clobbered)) == Ok((150, 200, 0)),
        "static",
    );
    s.add(
        "AP7-交错块边界完整",
        {
            let (f, w) = interleave();
            let starts_ok = f.byte(100) == Some(b'A')
                && f.byte(200) == Some(b'B')
                && f.byte(300) == Some(b'A')
                && f.byte(400) == Some(b'B')
                && f.byte(500) == Some(b'A')
                && f.byte(600) == Some(b'B');
            w.start == 600 && w.size == 700 && starts_ok && f.audit().is_ok()
        },
        "static",
    );
    s.add(
        "AP8-内部覆盖计数",
        run_scen(4).map(|w| (w.start, w.size, w.clobbered)) == Ok((50, 100, 20)),
        "static",
    );
    s.add(
        "AP9-末尾外写入形成空洞",
        {
            let mut f = base_file();
            let w = f.write_at(150, b'Z', 50);
            w.map(|v| (v.size, v.clobbered)) == Ok((200, 0))
                && f.byte(120) == Some(0)
                && f.byte(160) == Some(b'Z')
                && f.audit().is_ok()
        },
        "static",
    );
    s.add(
        "AP10-稀疏后追加与空文件",
        run_scen(6).map(|w| (w.start, w.size)) == Ok((200, 220))
            && run_scen(12).map(|w| (w.start, w.size)) == Ok((0, 5)),
        "static",
    );
    s.add(
        "AP11-追加溢出",
        run_scen(7) == Err(AP_ENOSPC),
        "static",
    );
    s.add(
        "AP12-越界与零长度",
        run_scen(9) == Err(AP_EFBIG)
            && run_scen(8).map(|w| (w.start, w.len, w.size)) == Ok((100, 0, 100))
            && run_scen(14).map(|w| w.size) == Ok(4096),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(AP_EFROZEN);
    s.add_dyn(
        "AP13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_starts_at_end() {
        let mut f = base_file();
        assert_eq!(f.append(b'A', 50).map(|w| (w.start, w.size)), Ok((100, 150)));
    }

    #[test]
    fn atomic_appends_do_not_clobber() {
        let mut f = base_file();
        let _ = f.append(b'A', 50);
        let w = f.append(b'B', 50);
        assert_eq!(w.map(|v| (v.start, v.clobbered)), Ok((150, 0)));
        assert_eq!(f.byte(149), Some(b'A'));
        assert_eq!(f.byte(150), Some(b'B'));
    }

    #[test]
    fn stale_offset_loses_update() {
        let mut f = base_file();
        let snap = f.size;
        let _ = f.write_at(snap, b'A', 50);
        let w = f.write_at(snap, b'B', 50);
        assert_eq!(w.map(|v| (v.size, v.clobbered)), Ok((150, 50)));
    }

    #[test]
    fn write_past_end_leaves_hole() {
        let mut f = base_file();
        let w = f.write_at(150, b'Z', 50);
        assert_eq!(w.map(|v| v.size), Ok(200));
        assert_eq!(f.byte(120), Some(0));
        assert_eq!(f.byte(199), Some(b'Z'));
    }

    #[test]
    fn limits_are_enforced() {
        let mut f = File::filled(4000, 0x44);
        assert_eq!(f.append(b'Z', 96).map(|w| w.size), Ok(4096));
        assert_eq!(f.append(b'Z', 1), Err(AP_ENOSPC));
        assert_eq!(f.write_at(4090, b'Z', 10), Err(AP_EFBIG));
    }

    #[test]
    fn zero_length_append_is_noop() {
        let mut f = base_file();
        assert_eq!(f.append(b'Z', 0).map(|w| (w.start, w.size)), Ok((100, 100)));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 13);
        assert_eq!(l.err_count(), 2);
        assert!(l.audit().is_ok());
        let s = run_unx11483_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
