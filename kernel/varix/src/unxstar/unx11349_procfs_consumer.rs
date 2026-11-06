//! UNX-F11349 诊断工具高频消费（上游语义）（0xebd0 段）
//!
//! `ps`、`top`、`strace` 这类诊断工具的读取模式非常固定：**打开一次、反复短读、
//! 偶尔回头 seek**。`/proc` 文件不是普通文件——每次 `open` 会拿到一份快照，同一
//! 句柄后续的多次读取必须自洽地落在这一份快照上；而中途别处刷新了上游数据，
//! 旧句柄不该突然「长出」新内容，否则工具拼出来的记录就会半新半旧。
//!
//! 本模块把上游内容压成「一个长度 + 一个版本号」，句柄在打开时钉住当时的长度：
//!
//! - 短读合法，游标照常前进，多次短读拼起来必须恰好等于可用字节；
//! - 刷新后旧句柄仍在旧长度上读，读到尽头返回 0 字节（EOF 不是错误）；
//! - 若旧句柄的游标已经越过新快照的末端，说明它引用的数据已被回收，读取必须
//!   报 `EPIN`，而不是静默返回 0 让工具以为文件变短了。
//!
//! 闸门次序：
//! - `open`：句柄表满；
//! - `read`：零缓冲 → 无此句柄 → 已关闭 → 钉定越界；
//! - `seek`：无此句柄 → 已关闭 → 越过句柄末端；
//! - `close`：无此句柄 → 已关闭；
//! - `refresh`：零长度。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xebd0;

/// 零缓冲或零长度。
pub const DC_EINVAL: u32 = SEG_BASE + 0x01;
/// 无此句柄。
pub const DC_EBADH: u32 = SEG_BASE + 0x02;
/// 定位越过句柄末端。
pub const DC_ESEEK: u32 = SEG_BASE + 0x03;
/// 句柄表满。
pub const DC_EFULL: u32 = SEG_BASE + 0x04;
/// 句柄已关闭。
pub const DC_ECLOSED: u32 = SEG_BASE + 0x05;
/// 钉定数据已被回收。
pub const DC_EPIN: u32 = SEG_BASE + 0x06;
/// 账本形状不符。
pub const DC_EBAL: u32 = SEG_BASE + 0x07;
/// 槽位乱序。
pub const DC_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const DC_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const DC_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观察下标越界。
pub const DC_ENOBS: u32 = SEG_BASE + 0x0b;
/// 游标与字节账不符。
pub const DC_ETORN: u32 = SEG_BASE + 0x0c;
/// 版本号回退。
pub const DC_EVER: u32 = SEG_BASE + 0x0d;

/// 句柄容量。
pub const HCAP: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：打开。
pub const OP_OPEN: u8 = 1;
/// 操作码：读取。
pub const OP_READ: u8 = 2;
/// 操作码：定位。
pub const OP_SEEK: u8 = 3;
/// 操作码：刷新。
pub const OP_REFRESH: u8 = 4;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一个读取句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    /// 句柄号。
    pub id: u32,
    /// 游标。
    pub cur: u32,
    /// 打开时钉住的长度。
    pub len: u32,
    /// 是否仍然打开。
    pub open: bool,
}

impl Handle {
    /// 空句柄。
    pub const fn blank() -> Handle {
        Handle { id: 0, cur: 0, len: 0, open: false }
    }

    /// 剩余可读字节。
    pub fn remaining(&self) -> u32 {
        self.len.saturating_sub(self.cur)
    }
}

/// 上游数据 + 句柄表。
#[derive(Clone, Copy, Debug)]
pub struct Feed {
    cur_len: u32,
    ver: u32,
    h: [Handle; HCAP],
    hn: usize,
    next_id: u32,
    reads: u32,
    bytes: u32,
    seeks: u32,
    closes: u32,
    refreshes: u32,
}

impl Feed {
    /// 以给定长度开局。
    pub const fn new(len: u32) -> Feed {
        Feed {
            cur_len: len,
            ver: 1,
            h: [Handle::blank(); HCAP],
            hn: 0,
            next_id: 1,
            reads: 0,
            bytes: 0,
            seeks: 0,
            closes: 0,
            refreshes: 0,
        }
    }

    /// 当前快照长度。
    pub fn cur_len(&self) -> u32 {
        self.cur_len
    }

    /// 当前版本号。
    pub fn version(&self) -> u32 {
        self.ver
    }

    /// 句柄槽数（含已关闭）。
    pub fn handle_count(&self) -> usize {
        self.hn
    }

    /// 累计读取次数。
    pub fn read_count(&self) -> u32 {
        self.reads
    }

    /// 累计读取字节。
    pub fn byte_count(&self) -> u32 {
        self.bytes
    }

    /// 累计定位次数。
    pub fn seek_count(&self) -> u32 {
        self.seeks
    }

    /// 累计关闭次数。
    pub fn close_count(&self) -> u32 {
        self.closes
    }

    /// 累计刷新次数。
    pub fn refresh_count(&self) -> u32 {
        self.refreshes
    }

    /// 第 i 个句柄。
    pub fn handle_at(&self, i: usize) -> Option<Handle> {
        if i >= self.hn {
            return None;
        }
        self.h.get(i).copied()
    }

    /// 按号查槽位。
    fn slot(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.hn {
            match self.h.get(i) {
                Some(x) => {
                    if x.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 打开：钉住当前长度。
    pub fn open(&mut self) -> Result<u32, u32> {
        if self.hn >= HCAP {
            return Err(DC_EFULL);
        }
        let id = self.next_id;
        if let Some(slot) = self.h.get_mut(self.hn) {
            *slot = Handle { id, cur: 0, len: self.cur_len, open: true };
        }
        self.hn += 1;
        self.next_id = self.next_id.saturating_add(1);
        Ok(id)
    }

    /// 读取：最多 `cap` 字节，返回实际字节数。
    pub fn read(&mut self, id: u32, cap: u32) -> Result<u32, u32> {
        if cap == 0 {
            return Err(DC_EINVAL);
        }
        let i = match self.slot(id) {
            Some(i) => i,
            None => return Err(DC_EBADH),
        };
        let h = match self.h.get(i) {
            Some(h) => *h,
            None => return Err(DC_EBADH),
        };
        if !h.open {
            return Err(DC_ECLOSED);
        }
        if h.cur > self.cur_len {
            return Err(DC_EPIN);
        }
        let n = if h.remaining() < cap { h.remaining() } else { cap };
        if let Some(slot) = self.h.get_mut(i) {
            slot.cur = slot.cur.saturating_add(n);
        }
        self.reads = self.reads.saturating_add(1);
        self.bytes = self.bytes.saturating_add(n);
        Ok(n)
    }

    /// 定位：游标可落在 `[0, len]`。
    pub fn seek(&mut self, id: u32, off: u32) -> Result<u32, u32> {
        let i = match self.slot(id) {
            Some(i) => i,
            None => return Err(DC_EBADH),
        };
        let h = match self.h.get(i) {
            Some(h) => *h,
            None => return Err(DC_EBADH),
        };
        if !h.open {
            return Err(DC_ECLOSED);
        }
        if off > h.len {
            return Err(DC_ESEEK);
        }
        if let Some(slot) = self.h.get_mut(i) {
            slot.cur = off;
        }
        self.seeks = self.seeks.saturating_add(1);
        Ok(off)
    }

    /// 关闭句柄。
    pub fn close(&mut self, id: u32) -> Result<u32, u32> {
        let i = match self.slot(id) {
            Some(i) => i,
            None => return Err(DC_EBADH),
        };
        let h = match self.h.get(i) {
            Some(h) => *h,
            None => return Err(DC_EBADH),
        };
        if !h.open {
            return Err(DC_ECLOSED);
        }
        if let Some(slot) = self.h.get_mut(i) {
            slot.open = false;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(0)
    }

    /// 刷新上游：长度不得为零，版本号单调递增。
    pub fn refresh(&mut self, len: u32) -> Result<u32, u32> {
        if len == 0 {
            return Err(DC_EINVAL);
        }
        let next = match self.ver.checked_add(1) {
            Some(v) => v,
            None => return Err(DC_EVER),
        };
        self.ver = next;
        self.cur_len = len;
        self.refreshes = self.refreshes.saturating_add(1);
        Ok(next)
    }

    /// 字节账与句柄游标是否自洽：每个句柄的游标都不超过它钉住的长度。
    pub fn consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.hn {
            match self.h.get(i) {
                Some(h) => {
                    if h.cur > h.len {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DC_EORDER);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DC_EBAL);
        }
        const VALS: [u32; 8] = [1, 256, 256, 0, 950, 2, 2, 1100];
        const CODES: [u32; 6] =
            [DC_EFULL, DC_EBADH, DC_ESEEK, DC_EINVAL, DC_ECLOSED, DC_EPIN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DC_EBAL),
            };
            if s.slot as usize != i {
                return Err(DC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DC_EMISMATCH);
                        }
                    }
                    None => return Err(DC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DC_EMISMATCH);
                        }
                    }
                    None => return Err(DC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DC_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DC_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(DC_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：短读两次 → 回头 seek → 大读 → 刷新上游 → 新句柄见新长度 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = Feed::new(1000);

    let _ = l.register(word_cell(0, OP_OPEN, f.open()));
    let _ = l.register(word_cell(1, OP_READ, f.read(1, 256)));
    let _ = l.register(word_cell(2, OP_READ, f.read(1, 256)));
    let _ = l.register(word_cell(3, OP_SEEK, f.seek(1, 0)));
    let _ = l.register(word_cell(4, OP_READ, f.read(1, 950)));
    let _ = l.register(word_cell(5, OP_REFRESH, f.refresh(1200)));
    let _ = l.register(word_cell(6, OP_OPEN, f.open()));
    let _ = l.register(word_cell(7, OP_READ, f.read(2, 1100)));

    let mut full = Feed::new(64);
    let mut i = 0usize;
    while i < HCAP {
        let _ = full.open();
        i += 1;
    }
    let _ = l.register(word_cell(8, OP_OPEN, full.open()));

    let _ = l.register(word_cell(9, OP_READ, f.read(99, 16)));
    let _ = l.register(word_cell(10, OP_SEEK, f.seek(1, 5000)));
    let _ = l.register(word_cell(11, OP_READ, f.read(1, 0)));
    let _ = f.close(1);
    let _ = l.register(word_cell(12, OP_READ, f.read(1, 16)));

    let mut p = Feed::new(1200);
    let _ = p.open();
    let _ = p.read(1, 1100);
    let _ = p.refresh(300);
    let _ = l.register(word_cell(13, OP_READ, p.read(1, 16)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：多次短读拼起来不漏字节，读到尽头给 0。
    let mut a = Feed::new(700);
    let h = a.open();
    let r1 = a.read(1, 200);
    let r2 = a.read(1, 200);
    let r3 = a.read(1, 400);
    let r4 = a.read(1, 400);
    out[1] = h == Ok(1)
        && r1 == Ok(200)
        && r2 == Ok(200)
        && r3 == Ok(300)
        && r4 == Ok(0)
        && a.byte_count() == 700
        && a.read_count() == 4
        && a.consistent();

    // 二路：快照钉定——刷新后旧句柄仍停在旧长度，新句柄见新长度。
    let mut b = Feed::new(500);
    let old = b.open();
    let _ = b.refresh(900);
    let new = b.open();
    let old_tail = b.read(1, 900);
    let new_tail = b.read(2, 900);
    let seek_old = b.seek(1, 900);
    out[2] = old == Ok(1)
        && new == Ok(2)
        && old_tail == Ok(500)
        && new_tail == Ok(900)
        && seek_old == Err(DC_ESEEK)
        && b.version() == 2
        && b.cur_len() == 900
        && b.handle_at(0) == Some(Handle { id: 1, cur: 500, len: 500, open: true })
        && b.consistent();

    // 三路：闸门（零缓冲 / 无句柄 / 越界 seek / 重复关闭 / 钉定越界）。
    let mut g = Feed::new(100);
    let g1 = g.read(1, 0) == Err(DC_EINVAL);
    let g2 = g.read(7, 8) == Err(DC_EBADH);
    let g3 = g.open() == Ok(1);
    let g4 = g.seek(1, 101) == Err(DC_ESEEK);
    let g5 = g.close(1) == Ok(0);
    let g6 = g.read(1, 8) == Err(DC_ECLOSED);
    let g7 = g.close(1) == Err(DC_ECLOSED);
    let g8 = g.refresh(0) == Err(DC_EINVAL);
    let g9 = g.handle_count() == 1 && g.close_count() == 1;
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9;

    out
}

/// 十三项判据。
pub fn run_unx11349_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11349.procfs_consumer");
    let l = build_ledger();

    s.add_dyn(
        "DC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DC2-首句柄号为一",
        l.sample(0).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DC3-两次短读各二百五十六",
        l.sample(1).map(|x| x.value) == Some(256) && l.sample(2).map(|x| x.value) == Some(256),
        "static",
    );

    s.add(
        "DC4-回头定位归零",
        l.sample(3).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "DC5-大读按可用字节截断",
        l.sample(4).map(|x| x.value) == Some(950),
        "static",
    );

    s.add(
        "DC6-刷新版本递增",
        l.sample(5).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DC7-新句柄见新长度",
        l.sample(6).map(|x| x.value) == Some(2) && l.sample(7).map(|x| x.value) == Some(1100),
        "static",
    );

    s.add(
        "DC8-句柄表满被拒",
        l.sample(8).map(|x| x.code) == Some(DC_EFULL),
        "static",
    );

    s.add(
        "DC9-无句柄与越界定位",
        l.sample(9).map(|x| x.code) == Some(DC_EBADH)
            && l.sample(10).map(|x| x.code) == Some(DC_ESEEK),
        "static",
    );

    s.add(
        "DC10-零缓冲与关闭后读",
        l.sample(11).map(|x| x.code) == Some(DC_EINVAL)
            && l.sample(12).map(|x| x.code) == Some(DC_ECLOSED),
        "static",
    );

    s.add(
        "DC11-钉定数据被回收",
        l.sample(13).map(|x| x.code) == Some(DC_EPIN),
        "static",
    );

    s.add(
        "DC12-短读拼接与快照钉定",
        {
            let mut a = Feed::new(300);
            let _ = a.open();
            let mut total = 0u32;
            let mut guard = 0u32;
            let mut bad = false;
            while guard < 8 {
                match a.read(1, 100) {
                    Ok(0) => break,
                    Ok(n) => total = total.saturating_add(n),
                    Err(_) => {
                        bad = true;
                        break;
                    }
                }
                guard += 1;
            }
            let _ = a.refresh(10);
            let old_eof = a.read(1, 100);
            !bad
                && a.byte_count() == 300
                && total == 300
                && a.read_count() == 4
                && old_eof == Err(DC_EPIN)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "DC13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11349_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn short_reads_concatenate() {
        let mut f = Feed::new(250);
        let _ = f.open();
        assert_eq!(f.read(1, 100), Ok(100));
        assert_eq!(f.read(1, 100), Ok(100));
        assert_eq!(f.read(1, 100), Ok(50));
        assert_eq!(f.read(1, 100), Ok(0));
        assert_eq!(f.byte_count(), 250);
    }

    #[test]
    fn handles_pin_their_snapshot() {
        let mut f = Feed::new(400);
        let _ = f.open();
        let _ = f.refresh(800);
        assert_eq!(f.read(1, 800), Ok(400));
        let _ = f.open();
        assert_eq!(f.read(2, 800), Ok(800));
        assert_eq!(f.version(), 2);
    }

    #[test]
    fn seek_is_bounded_by_pinned_length() {
        let mut f = Feed::new(120);
        let _ = f.open();
        assert_eq!(f.seek(1, 120), Ok(120));
        assert_eq!(f.seek(1, 121), Err(DC_ESEEK));
        assert_eq!(f.read(1, 8), Ok(0));
        assert_eq!(f.seek(1, 0), Ok(0));
        assert_eq!(f.read(1, 8), Ok(8));
    }

    #[test]
    fn pinned_cursor_beyond_new_snapshot() {
        let mut f = Feed::new(1000);
        let _ = f.open();
        let _ = f.read(1, 900);
        let _ = f.refresh(200);
        assert_eq!(f.read(1, 8), Err(DC_EPIN));
        assert_eq!(f.read(2, 8), Err(DC_EBADH));
    }

    #[test]
    fn gates() {
        let mut f = Feed::new(50);
        assert_eq!(f.read(1, 8), Err(DC_EBADH));
        assert_eq!(f.seek(1, 0), Err(DC_EBADH));
        assert_eq!(f.close(1), Err(DC_EBADH));
        assert_eq!(f.refresh(0), Err(DC_EINVAL));
        let _ = f.open();
        assert_eq!(f.read(1, 0), Err(DC_EINVAL));
        assert_eq!(f.close(1), Ok(0));
        assert_eq!(f.close(1), Err(DC_ECLOSED));
        assert_eq!(f.seek(1, 0), Err(DC_ECLOSED));
    }

    #[test]
    fn capacity_is_exact() {
        let mut f = Feed::new(10);
        let mut i = 0usize;
        while i < HCAP {
            assert!(f.open().is_ok());
            i += 1;
        }
        assert_eq!(f.open(), Err(DC_EFULL));
        assert_eq!(f.handle_count(), HCAP);
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
