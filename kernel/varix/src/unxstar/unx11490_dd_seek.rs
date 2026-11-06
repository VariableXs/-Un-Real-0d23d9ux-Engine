//! UNX-F11490 dd seek/skip 块寻址（0x20e70 段）
//!
//! dd 用块号而不是字节号定位：`skip=N` 让输入从第 N 块开始读，`seek=N` 让输出
//! 从第 N 块开始写，两者都乘以 `bs` 得到字节偏移。乘法必须做溢出检查——两个
//! 三十二位块号相乘很容易越过地址空间，溢出时返回 EOVF 而不是回绕到文件头。
//! `oflag=seek_bytes` 把单位从块改成字节，此时 `seek` 不再乘 `bs`，账本用一格
//! 专门盯这个单位切换。
//!
//! 读侧把偏移与源长度比较：偏移等于长度是合法的 EOF（读零字节），超过长度才是
//! EPOS；写侧要求 `偏移 + 数据长` 落在目的容量内，否则 ENOSPC。`span` 计算
//! 一个 `skip + count` 窗口实际能覆盖多少字节，窗口跨过 EOF 时按剩余长度截短。
//! `append` 与 `seek` 同时出现是自相矛盾的语义（追加会忽略定位），必须 EAPPEND；
//! `block_index` 则拒绝非整块的字节偏移（EMSALIGN）。
//!
//! 账本十四格：八格成功覆盖输入/输出偏移、EOF 短读、整块读、带偏移写、512 块
//! 定位、字节定位与窗口截短；六格拒绝依次是零块、乘法溢出、越界读、目的不足、
//! 追加冲突与偏移未对齐。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e70;

/// 编号非法。
pub const DK_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const DK_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DK_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DK_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DK_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const DK_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DK_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const DK_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const DK_ESEAL: u32 = SEG_BASE + 0x09;
/// 块尺寸为零。
pub const DK_EBS0: u32 = SEG_BASE + 0x0a;
/// 乘法溢出。
pub const DK_EOVF: u32 = SEG_BASE + 0x0b;
/// 读偏移越界。
pub const DK_EPOS: u32 = SEG_BASE + 0x0c;
/// 目的空间不足。
pub const DK_ENOSPC: u32 = SEG_BASE + 0x0d;
/// 追加与定位冲突。
pub const DK_EAPPEND: u32 = SEG_BASE + 0x0e;
/// 偏移未对齐。
pub const DK_EMSALIGN: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：输入偏移。
pub const OP_SKIP: u8 = 1;
/// 操作：输出偏移。
pub const OP_SEEK: u8 = 2;
/// 操作：定址读。
pub const OP_READ: u8 = 3;
/// 操作：定址写。
pub const OP_WRITE: u8 = 4;
/// 操作：窗口覆盖。
pub const OP_SPAN: u8 = 5;
/// 操作：字节定位。
pub const OP_BYTES: u8 = 6;

/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
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

/// 块地址换算：（输入偏移，输出偏移）。
pub fn offsets(bs: u32, skip: u32, seek: u32) -> Result<(u32, u32), u32> {
    if bs == 0 {
        return Err(DK_EBS0);
    }
    let in_off = skip.checked_mul(bs).ok_or(DK_EOVF)?;
    let out_off = seek.checked_mul(bs).ok_or(DK_EOVF)?;
    Ok((in_off, out_off))
}

/// 定址读：偏移等于长度是 EOF，超过才是越界。
pub fn read_at(src: &[u8], off: u32, want: u32) -> Result<u32, u32> {
    let o = off as usize;
    if o > src.len() {
        return Err(DK_EPOS);
    }
    let avail = src.len() - o;
    let take = if (want as usize) < avail {
        want as usize
    } else {
        avail
    };
    Ok(take as u32)
}

/// 定址写：整段必须落在容量内。
pub fn write_at(dst: &mut [u8], off: u32, data: &[u8]) -> Result<u32, u32> {
    let o = off as usize;
    let end = match o.checked_add(data.len()) {
        Some(v) => v,
        None => return Err(DK_ENOSPC),
    };
    if end > dst.len() {
        return Err(DK_ENOSPC);
    }
    let mut i = 0usize;
    while i < data.len() {
        match (data.get(i), dst.get_mut(o + i)) {
            (Some(&b), Some(slot)) => *slot = b,
            _ => return Err(DK_ENOSPC),
        }
        i += 1;
    }
    Ok(data.len() as u32)
}

/// 字节定位：oflag=seek_bytes 时 seek 不再乘块。
pub fn seek_bytes(seek: u32, bs: u32) -> Result<u32, u32> {
    if bs == 0 {
        return Err(DK_EBS0);
    }
    Ok(seek)
}

/// 窗口覆盖：从 skip 块起，count 块在源里实际覆盖多少字节。
pub fn span(bs: u32, skip: u32, count: u32, src_len: u32) -> Result<u32, u32> {
    if bs == 0 {
        return Err(DK_EBS0);
    }
    let start = skip.checked_mul(bs).ok_or(DK_EOVF)?;
    if start > src_len {
        return Err(DK_EPOS);
    }
    let avail = src_len - start;
    let want = count.checked_mul(bs).ok_or(DK_EOVF)?;
    Ok(if want < avail { want } else { avail })
}

/// 追加与定位的互斥闸门。
pub fn check_flags(append: bool, seek: u32) -> Result<u8, u32> {
    if append && seek != 0 {
        return Err(DK_EAPPEND);
    }
    Ok(if append { 1 } else { 0 })
}

/// 块号换算：偏移必须是整块。
pub fn block_index(off: u32, bs: u32) -> Result<u32, u32> {
    if bs == 0 {
        return Err(DK_EBS0);
    }
    if off % bs != 0 {
        return Err(DK_EMSALIGN);
    }
    Ok(off / bs)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DK_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(DK_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(DK_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(DK_EORDER);
        }
        if self.len >= self.cap {
            return Err(DK_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(DK_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
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

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(DK_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DK_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(DK_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(DK_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(DK_EMISMATCH);
                    }
                }
                None => return Err(DK_ENOBS),
            }
            i += 1;
        }
        let vals = [8u32, 12, 2, 4, 4, 512, 6, 8];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(DK_EMISMATCH);
                    }
                }
                _ => return Err(DK_EBAL),
            }
            k += 1;
        }
        let codes = [DK_EBS0, DK_EOVF, DK_EPOS, DK_ENOSPC, DK_EAPPEND, DK_EMSALIGN];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DK_EMISMATCH);
                        }
                    }
                    None => return Err(DK_EBAL),
                },
                None => return Err(DK_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(DK_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
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
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DK_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(DK_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(DK_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

const SRC10: [u8; 10] = [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut dst = [0u8; 16];

    let _ = l.register(cell(0, OP_SKIP, offsets(4, 2, 3).map(|(i, _)| i)));
    let _ = l.register(cell(1, OP_SEEK, offsets(4, 2, 3).map(|(_, o)| o)));
    let _ = l.register(cell(2, OP_READ, read_at(&SRC10, 8, 4)));
    let _ = l.register(cell(3, OP_READ, read_at(&SRC10, 0, 4)));
    let _ = l.register(cell(4, OP_WRITE, write_at(&mut dst, 12, b"wxyz")));
    let _ = l.register(cell(5, OP_SEEK, offsets(512, 1, 1).map(|(_, o)| o)));
    let _ = l.register(cell(6, OP_BYTES, seek_bytes(6, 4)));
    let _ = l.register(cell(7, OP_SPAN, span(4, 1, 2, 12)));

    let _ = l.register(cell(8, OP_SKIP, offsets(0, 1, 1).map(|(i, _)| i)));
    let _ = l.register(cell(9, OP_SKIP, offsets(4, 0xffff_ffff, 1).map(|(i, _)| i)));
    let _ = l.register(cell(10, OP_READ, read_at(&SRC10, 11, 1)));
    let _ = l.register(cell(11, OP_WRITE, write_at(&mut dst[..4], 2, b"wxyz")));
    let _ = l.register(cell(12, OP_BYTES, check_flags(true, 1).map(u32::from)));
    let _ = l.register(cell(13, OP_SKIP, block_index(6, 4)));

    l
}

/// 四路注入：账本自洽、块寻址、定址读写、追加冲突后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(DK_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    out[1] = offsets(4, 2, 3) == Ok((8, 12))
        && offsets(512, 1, 1) == Ok((512, 512))
        && offsets(1, 1, 1) == Ok((1, 1))
        && block_index(12, 4) == Ok(3);

    let mut dst = [0u8; 16];
    out[2] = read_at(&SRC10, 8, 4) == Ok(2)
        && read_at(&SRC10, 10, 4) == Ok(0)
        && write_at(&mut dst, 12, b"wxyz") == Ok(4)
        && dst.get(12) == Some(&b'w')
        && dst.get(15) == Some(&b'z');

    let blocked = check_flags(true, 1) == Err(DK_EAPPEND);
    let resumed = check_flags(false, 1) == Ok(0);
    let append_ok = check_flags(true, 0) == Ok(1);
    out[3] = blocked && resumed && append_ok;
    out
}

/// 十三判据。
pub fn run_unx11490_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11490.ddseek");
    let l = build_ledger();

    s.add(
        "DK1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_SKIP),
        "static",
    );
    s.add_dyn(
        "DK2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "DK3-双向偏移",
        offsets(4, 2, 3) == Ok((8, 12))
            && offsets(512, 1, 1) == Ok((512, 512))
            && l.sample(0).map(|x| x.value) == Some(8)
            && l.sample(1).map(|x| x.value) == Some(12),
        "static",
    );
    s.add(
        "DK4-EOF 短读",
        read_at(&SRC10, 8, 4) == Ok(2) && read_at(&SRC10, 10, 4) == Ok(0),
        "static",
    );
    s.add(
        "DK5-整块读",
        read_at(&SRC10, 0, 4) == Ok(4)
            && read_at(&SRC10, 0, 0) == Ok(0)
            && l.sample(3).map(|x| x.value) == Some(4),
        "static",
    );
    s.add(
        "DK6-定址写",
        {
            let mut dst = [0u8; 16];
            write_at(&mut dst, 12, b"wxyz") == Ok(4)
                && dst.get(11) == Some(&0)
                && dst.get(12) == Some(&b'w')
                && dst.get(15) == Some(&b'z')
        },
        "static",
    );
    s.add(
        "DK7-字节定位单位",
        seek_bytes(6, 4) == Ok(6)
            && seek_bytes(6, 512) == Ok(6)
            && l.sample(6).map(|x| x.value) == Some(6),
        "static",
    );
    s.add(
        "DK8-窗口截短",
        span(4, 1, 2, 12) == Ok(8)
            && span(4, 1, 2, 10) == Ok(6)
            && span(4, 0, 1, 10) == Ok(4)
            && l.sample(7).map(|x| x.value) == Some(8),
        "static",
    );
    s.add(
        "DK9-零块闸门",
        offsets(0, 1, 1) == Err(DK_EBS0)
            && l.sample(8).map(|x| x.code) == Some(DK_EBS0),
        "static",
    );
    s.add(
        "DK10-乘法溢出",
        offsets(4, 0xffff_ffff, 1) == Err(DK_EOVF)
            && span(65536, 65536, 1, 10) == Err(DK_EOVF)
            && l.sample(9).map(|x| x.code) == Some(DK_EOVF),
        "static",
    );
    s.add(
        "DK11-越界读",
        read_at(&SRC10, 11, 1) == Err(DK_EPOS)
            && span(4, 9, 1, 10) == Err(DK_EPOS)
            && l.sample(10).map(|x| x.code) == Some(DK_EPOS),
        "static",
    );
    s.add(
        "DK12-写越界与语义冲突",
        write_at(&mut [0u8; 4], 2, b"wxyz") == Err(DK_ENOSPC)
            && check_flags(true, 1) == Err(DK_EAPPEND)
            && block_index(6, 4) == Err(DK_EMSALIGN)
            && l.sample(11).map(|x| x.code) == Some(DK_ENOSPC)
            && l.sample(12).map(|x| x.code) == Some(DK_EAPPEND)
            && l.sample(13).map(|x| x.code) == Some(DK_EMSALIGN),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DK_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "DK13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && f.root() == root && inj == [true; 4],
        &format!("root={:016x} inj={:?}", root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_multiply() {
        assert_eq!(offsets(4, 2, 3), Ok((8, 12)));
        assert_eq!(offsets(512, 1, 1), Ok((512, 512)));
        assert_eq!(offsets(0, 1, 1), Err(DK_EBS0));
        assert_eq!(offsets(4, 0xffff_ffff, 1), Err(DK_EOVF));
    }

    #[test]
    fn read_short_at_eof() {
        assert_eq!(read_at(&SRC10, 8, 4), Ok(2));
        assert_eq!(read_at(&SRC10, 10, 4), Ok(0));
        assert_eq!(read_at(&SRC10, 11, 1), Err(DK_EPOS));
    }

    #[test]
    fn write_preserves_prefix() {
        let mut dst = [0u8; 8];
        assert_eq!(write_at(&mut dst, 4, b"ab"), Ok(2));
        assert_eq!(dst.get(3), Some(&0));
        assert_eq!(dst.get(4), Some(&b'a'));
        assert_eq!(write_at(&mut dst, 7, b"ab"), Err(DK_ENOSPC));
    }

    #[test]
    fn span_clamps_to_source() {
        assert_eq!(span(4, 1, 2, 12), Ok(8));
        assert_eq!(span(4, 1, 2, 10), Ok(6));
        assert_eq!(span(4, 9, 1, 10), Err(DK_EPOS));
    }

    #[test]
    fn append_conflicts_with_seek() {
        assert_eq!(check_flags(true, 1), Err(DK_EAPPEND));
        assert_eq!(check_flags(true, 0), Ok(1));
        assert_eq!(check_flags(false, 1), Ok(0));
    }

    #[test]
    fn block_index_requires_alignment() {
        assert_eq!(block_index(12, 4), Ok(3));
        assert_eq!(block_index(6, 4), Err(DK_EMSALIGN));
        assert_eq!(block_index(6, 0), Err(DK_EBS0));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11490_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
