//! UNX-F11489 dd conv= 族与 status 档（0x20e60 段）
//!
//! dd 的 `conv=` 是位集合而不是枚举：大小写转换、`swab`、`sync`、`notrunc`、
//! `noerror`、`fsync`、`excl` 可以同时打开，但 `lcase` 与 `ucase` 互斥——同时
//! 出现必须拒绝，否则结果取决于实现顺序而不是规范。`sync` 把不足一块的记录
//! 补零到 `bs`，源块比 `bs` 还长是调用方违约，直接 ESYNC；`notrunc` 与 `noerror`
//! 不改变块内容，只影响调用方对输出文件与坏块的处理策略，所以模型把它们暴露为
//! 查询而不是变换。
//!
//! `status=` 只有三档：`none` 全静默、`noxfer` 只报速率、`progress` 逐块刷新。
//! 档位是解析期就固定的值，运行期不再变；未知档位 ESTATUS。`conv=excl` 则是
//! 打开输出前的存在性闸门：目标已存在就 EEXIST，不存在才允许创建。
//!
//! 账本十四格：八格成功覆盖三种 conv 组合、大小写各一、`swab` 首字节、`sync`
//! 补齐长度与 status 档；六格拒绝依次是大小写冲突、未知 conv、空列表、sync
//! 源超块、未知 status 与 excl 已存在。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e60;

/// 编号非法。
pub const DV_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const DV_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DV_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DV_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DV_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const DV_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DV_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const DV_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const DV_ESEAL: u32 = SEG_BASE + 0x09;
/// 大小写转换冲突。
pub const DV_ECONFLICT: u32 = SEG_BASE + 0x0a;
/// 未知 conv 名。
pub const DV_ECONV: u32 = SEG_BASE + 0x0b;
/// 空 conv 列表。
pub const DV_EEMPTY: u32 = SEG_BASE + 0x0c;
/// sync 源超块。
pub const DV_ESYNC: u32 = SEG_BASE + 0x0d;
/// 未知 status 档。
pub const DV_ESTATUS: u32 = SEG_BASE + 0x0e;
/// excl 目标已存在。
pub const DV_EEXIST: u32 = SEG_BASE + 0x0f;
/// 目的空间不足。
pub const DV_ENOSPC: u32 = SEG_BASE + 0x10;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：conv 解析。
pub const OP_CONV: u8 = 1;
/// 操作：块变换。
pub const OP_APPLY: u8 = 2;
/// 操作：status 解析。
pub const OP_STATUS: u8 = 3;
/// 操作：输出存在闸门。
pub const OP_OPEN: u8 = 4;

/// 小写转换。
pub const C_LCASE: u16 = 0x01;
/// 大写转换。
pub const C_UCASE: u16 = 0x02;
/// 字节对交换。
pub const C_SWAB: u16 = 0x04;
/// 补齐到块。
pub const C_SYNC: u16 = 0x08;
/// 不截断输出。
pub const C_NOTRUNC: u16 = 0x10;
/// 坏块继续。
pub const C_NOERROR: u16 = 0x20;
/// 同步落盘。
pub const C_FSYNC: u16 = 0x40;
/// 目标必须不存在。
pub const C_EXCL: u16 = 0x80;

/// status：静默。
pub const ST_NONE: u8 = 0;
/// status：只报速率。
pub const ST_NOXFER: u8 = 1;
/// status：逐块。
pub const ST_PROGRESS: u8 = 2;

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

fn bit_of(t: &[u8]) -> Result<u16, u32> {
    if t == b"lcase" {
        Ok(C_LCASE)
    } else if t == b"ucase" {
        Ok(C_UCASE)
    } else if t == b"swab" {
        Ok(C_SWAB)
    } else if t == b"sync" {
        Ok(C_SYNC)
    } else if t == b"notrunc" {
        Ok(C_NOTRUNC)
    } else if t == b"noerror" {
        Ok(C_NOERROR)
    } else if t == b"fsync" {
        Ok(C_FSYNC)
    } else if t == b"excl" {
        Ok(C_EXCL)
    } else {
        Err(DV_ECONV)
    }
}

/// conv 列表解析：并集后检查大小写冲突。
pub fn parse_conv(list: &[&[u8]]) -> Result<u16, u32> {
    if list.is_empty() {
        return Err(DV_EEMPTY);
    }
    let mut c = 0u16;
    let mut i = 0usize;
    while i < list.len() {
        let t = match list.get(i) {
            Some(v) => *v,
            None => return Err(DV_ECONV),
        };
        c |= bit_of(t)?;
        i += 1;
    }
    if c & C_LCASE != 0 && c & C_UCASE != 0 {
        return Err(DV_ECONFLICT);
    }
    Ok(c)
}

/// status 档解析。
pub fn parse_status(s: &[u8]) -> Result<u8, u32> {
    if s == b"none" {
        Ok(ST_NONE)
    } else if s == b"noxfer" {
        Ok(ST_NOXFER)
    } else if s == b"progress" {
        Ok(ST_PROGRESS)
    } else {
        Err(DV_ESTATUS)
    }
}

/// 是否保留输出尾部。
pub fn keep_tail(conv: u16) -> bool {
    conv & C_NOTRUNC != 0
}

/// 坏块是否继续。
pub fn retry_on_error(conv: u16) -> bool {
    conv & C_NOERROR != 0
}

/// 输出打开闸门：excl 且已存在则拒绝。
pub fn open_out(exists: bool, conv: u16) -> Result<u8, u32> {
    if exists && conv & C_EXCL != 0 {
        return Err(DV_EEXIST);
    }
    Ok(if exists { 1 } else { 0 })
}

/// 块变换：大小写 → swab → sync 补零。
pub fn apply(conv: u16, src: &[u8], dst: &mut [u8], bs: u32) -> Result<u32, u32> {
    if conv & C_LCASE != 0 && conv & C_UCASE != 0 {
        return Err(DV_ECONFLICT);
    }
    let out_len = if conv & C_SYNC != 0 {
        if src.len() > bs as usize {
            return Err(DV_ESYNC);
        }
        bs as usize
    } else {
        src.len()
    };
    if out_len > dst.len() {
        return Err(DV_ENOSPC);
    }
    let mut i = 0usize;
    while i < src.len() {
        let b = match src.get(i) {
            Some(&v) => v,
            None => break,
        };
        let v = if conv & C_UCASE != 0 {
            b.to_ascii_uppercase()
        } else if conv & C_LCASE != 0 {
            b.to_ascii_lowercase()
        } else {
            b
        };
        match dst.get_mut(i) {
            Some(slot) => *slot = v,
            None => return Err(DV_ENOSPC),
        }
        i += 1;
    }
    if conv & C_SWAB != 0 {
        let mut k = 0usize;
        while k + 1 < src.len() {
            match (dst.get(k).copied(), dst.get(k + 1).copied()) {
                (Some(a), Some(b)) => {
                    if let Some(slot) = dst.get_mut(k) {
                        *slot = b;
                    }
                    if let Some(slot) = dst.get_mut(k + 1) {
                        *slot = a;
                    }
                }
                _ => return Err(DV_ENOSPC),
            }
            k += 2;
        }
    }
    let mut k = src.len();
    while k < out_len {
        match dst.get_mut(k) {
            Some(slot) => *slot = 0,
            None => return Err(DV_ENOSPC),
        }
        k += 1;
    }
    Ok(out_len as u32)
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
            return Err(DV_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(DV_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(DV_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(DV_EORDER);
        }
        if self.len >= self.cap {
            return Err(DV_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(DV_EID),
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
            return Err(DV_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DV_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(DV_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(DV_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(DV_EMISMATCH);
                    }
                }
                None => return Err(DV_ENOBS),
            }
            i += 1;
        }
        let vals = [2u32, 6, 24, 3, 3, 98, 8, 2];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(DV_EMISMATCH);
                    }
                }
                _ => return Err(DV_EBAL),
            }
            k += 1;
        }
        let codes = [
            DV_ECONFLICT,
            DV_ECONV,
            DV_EEMPTY,
            DV_ESYNC,
            DV_ESTATUS,
            DV_EEXIST,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DV_EMISMATCH);
                        }
                    }
                    None => return Err(DV_EBAL),
                },
                None => return Err(DV_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(DV_EBAL);
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
            return Err(DV_EFROZEN);
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
            return Err(DV_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(DV_ESEAL);
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

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut dst = [0u8; 16];

    let _ = l.register(cell(0, OP_CONV, parse_conv(&[b"ucase"]).map(u32::from)));
    let _ = l.register(cell(
        1,
        OP_CONV,
        parse_conv(&[b"ucase", b"swab"]).map(u32::from),
    ));
    let _ = l.register(cell(
        2,
        OP_CONV,
        parse_conv(&[b"sync", b"notrunc"]).map(u32::from),
    ));
    let _ = l.register(cell(3, OP_APPLY, apply(C_UCASE, b"aBc", &mut dst, 8)));
    let _ = l.register(cell(4, OP_APPLY, apply(C_LCASE, b"AbC", &mut dst, 8)));
    let swab_len = apply(C_SWAB, b"abcd", &mut dst, 8);
    let first = match dst.get(0) {
        Some(&v) => u32::from(v),
        None => 0,
    };
    let _ = l.register(cell(5, OP_APPLY, swab_len.map(|_| first)));
    let _ = l.register(cell(6, OP_APPLY, apply(C_SYNC, b"abc", &mut dst, 8)));
    let _ = l.register(cell(7, OP_STATUS, parse_status(b"progress").map(u32::from)));

    let _ = l.register(cell(
        8,
        OP_CONV,
        parse_conv(&[b"ucase", b"lcase"]).map(u32::from),
    ));
    let _ = l.register(cell(9, OP_CONV, parse_conv(&[b"rot13"]).map(u32::from)));
    let _ = l.register(cell(10, OP_CONV, parse_conv(&[]).map(u32::from)));
    let _ = l.register(cell(11, OP_APPLY, apply(C_SYNC, b"abcde", &mut dst, 4)));
    let _ = l.register(cell(12, OP_STATUS, parse_status(b"loud").map(u32::from)));
    let _ = l.register(cell(13, OP_OPEN, open_out(true, C_EXCL).map(u32::from)));

    l
}

/// 四路注入：账本自洽、大小写、swab 与补齐、excl 恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(DV_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let mut a = [0u8; 8];
    let mut b = [0u8; 8];
    out[1] = apply(C_UCASE, b"aBc", &mut a, 8) == Ok(3)
        && a.get(0..3) == Some(&b"ABC"[..])
        && apply(C_LCASE, b"AbC", &mut b, 8) == Ok(3)
        && b.get(0..3) == Some(&b"abc"[..]);

    let mut c = [0u8; 16];
    out[2] = apply(C_SWAB | C_SYNC, b"abcd", &mut c, 8) == Ok(8)
        && c.get(0..4) == Some(&b"badc"[..])
        && c.get(4) == Some(&0)
        && c.get(7) == Some(&0);

    let free = open_out(false, C_EXCL);
    let taken = open_out(true, C_EXCL);
    let plain = open_out(true, 0);
    out[3] = free == Ok(0) && taken == Err(DV_EEXIST) && plain == Ok(1);
    out
}

/// 十三判据。
pub fn run_unx11489_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11489.ddconv");
    let l = build_ledger();

    s.add(
        "DV1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_CONV),
        "static",
    );
    s.add_dyn(
        "DV2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "DV3-conv 并集",
        parse_conv(&[b"ucase"]) == Ok(C_UCASE)
            && parse_conv(&[b"ucase", b"swab"]) == Ok(C_UCASE | C_SWAB)
            && parse_conv(&[b"sync", b"notrunc"]) == Ok(C_SYNC | C_NOTRUNC)
            && l.sample(2).map(|x| x.value) == Some(24),
        "static",
    );
    s.add(
        "DV4-大小写转换",
        {
            let mut a = [0u8; 8];
            let mut b = [0u8; 8];
            apply(C_UCASE, b"aBc", &mut a, 8) == Ok(3)
                && a.get(0..3) == Some(&b"ABC"[..])
                && apply(C_LCASE, b"AbC", &mut b, 8) == Ok(3)
                && b.get(0..3) == Some(&b"abc"[..])
        },
        "static",
    );
    s.add(
        "DV5-swab 成对交换",
        {
            let mut a = [0u8; 8];
            apply(C_SWAB, b"abcd", &mut a, 8) == Ok(4)
                && a.get(0..4) == Some(&b"badc"[..])
                && l.sample(5).map(|x| x.value) == Some(98)
        },
        "static",
    );
    s.add(
        "DV6-sync 补零",
        {
            let mut a = [0u8; 16];
            apply(C_SYNC, b"abc", &mut a, 8) == Ok(8)
                && a.get(3) == Some(&0)
                && a.get(7) == Some(&0)
                && l.sample(6).map(|x| x.value) == Some(8)
        },
        "static",
    );
    s.add(
        "DV7-status 三档",
        parse_status(b"none") == Ok(ST_NONE)
            && parse_status(b"noxfer") == Ok(ST_NOXFER)
            && parse_status(b"progress") == Ok(ST_PROGRESS)
            && l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );
    s.add(
        "DV8-大小写互斥",
        parse_conv(&[b"lcase", b"ucase"]) == Err(DV_ECONFLICT)
            && apply(C_LCASE | C_UCASE, b"x", &mut [0u8; 4], 4) == Err(DV_ECONFLICT)
            && l.sample(8).map(|x| x.code) == Some(DV_ECONFLICT),
        "static",
    );
    s.add(
        "DV9-未知与空列表",
        parse_conv(&[b"rot13"]) == Err(DV_ECONV)
            && parse_conv(&[]) == Err(DV_EEMPTY)
            && l.sample(9).map(|x| x.code) == Some(DV_ECONV)
            && l.sample(10).map(|x| x.code) == Some(DV_EEMPTY),
        "static",
    );
    s.add(
        "DV10-sync 源超块",
        apply(C_SYNC, b"abcde", &mut [0u8; 16], 4) == Err(DV_ESYNC)
            && l.sample(11).map(|x| x.code) == Some(DV_ESYNC),
        "static",
    );
    s.add(
        "DV11-未知 status",
        parse_status(b"loud") == Err(DV_ESTATUS)
            && l.sample(12).map(|x| x.code) == Some(DV_ESTATUS),
        "static",
    );
    s.add(
        "DV12-notrunc 与 excl",
        keep_tail(C_NOTRUNC)
            && !keep_tail(C_SYNC)
            && retry_on_error(C_NOERROR)
            && !retry_on_error(C_FSYNC)
            && open_out(true, C_EXCL) == Err(DV_EEXIST)
            && open_out(false, C_EXCL) == Ok(0)
            && l.sample(13).map(|x| x.code) == Some(DV_EEXIST),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DV_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "DV13-冻结重放与四路",
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
    fn conv_union() {
        assert_eq!(parse_conv(&[b"ucase"]), Ok(C_UCASE));
        assert_eq!(parse_conv(&[b"ucase", b"swab"]), Ok(C_UCASE | C_SWAB));
        assert_eq!(
            parse_conv(&[b"sync", b"notrunc"]),
            Ok(C_SYNC | C_NOTRUNC)
        );
    }

    #[test]
    fn case_conflict() {
        assert_eq!(parse_conv(&[b"ucase", b"lcase"]), Err(DV_ECONFLICT));
        assert_eq!(parse_conv(&[b"rot13"]), Err(DV_ECONV));
        assert_eq!(parse_conv(&[]), Err(DV_EEMPTY));
    }

    #[test]
    fn transformation_order() {
        let mut a = [0u8; 8];
        assert_eq!(apply(C_UCASE, b"aBc", &mut a, 8), Ok(3));
        assert_eq!(a.get(0..3), Some(&b"ABC"[..]));
        assert_eq!(apply(C_SWAB, b"abcd", &mut a, 8), Ok(4));
        assert_eq!(a.get(0..4), Some(&b"badc"[..]));
    }

    #[test]
    fn sync_pads_to_block() {
        let mut a = [0u8; 16];
        assert_eq!(apply(C_SYNC, b"abc", &mut a, 8), Ok(8));
        assert_eq!(a.get(3), Some(&0));
        assert_eq!(apply(C_SYNC, b"abcde", &mut a, 4), Err(DV_ESYNC));
    }

    #[test]
    fn status_tiers() {
        assert_eq!(parse_status(b"none"), Ok(ST_NONE));
        assert_eq!(parse_status(b"noxfer"), Ok(ST_NOXFER));
        assert_eq!(parse_status(b"progress"), Ok(ST_PROGRESS));
        assert_eq!(parse_status(b"loud"), Err(DV_ESTATUS));
    }

    #[test]
    fn excl_checks_existence() {
        assert_eq!(open_out(true, C_EXCL), Err(DV_EEXIST));
        assert_eq!(open_out(false, C_EXCL), Ok(0));
        assert_eq!(open_out(true, 0), Ok(1));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11489_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
