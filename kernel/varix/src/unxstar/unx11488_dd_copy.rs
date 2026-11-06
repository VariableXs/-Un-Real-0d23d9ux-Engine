//! UNX-F11488 dd 基本复制（0x20e50 段）
//!
//! dd 的核心是“块”而不是字节：`bs` 定一次读写多大，`count` 定最多搬几块，
//! 最后一块可以短于 `bs`，这在统计里记成 `+1` 的 partial 记录。模型把源与目的
//! 都做成切片，复制器逐块推进：块大小、剩余源长、目的余量三者先算清楚再落字节，
//! 目的放不下整块就直接 ENOSPC——本模型不写“半块”，因为 dd 在短写上本来就要
//! 单独记账，模型把这层留给了 conv 模块。
//!
//! `bs` 的解析支持 dd 的后缀族：`c`=1、`w`=2、`b`=512、`k/K`=1024、`M`=1MiB，
//! 以及乘法形式 `2x512`。语法是“数 [后缀] [x 数 [后缀]]”，解析器对每个因子
//! 单独做溢出检查，乘积再检查一次；空串、后缀非法、数值溢出、零块各有一档错误。
//!
//! 账本十四格：八格成功覆盖十字节三块、十二字节三块、完整块计数、短块计数与
//! 三种块尺寸解析；六格拒绝依次是零块、目的不足、空串、非法后缀、数值溢出与
//! 块尺寸超上限（1MiB）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e50;

/// 编号非法。
pub const DC_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const DC_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DC_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DC_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const DC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DC_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const DC_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const DC_ESEAL: u32 = SEG_BASE + 0x09;
/// 块尺寸为零。
pub const DC_EBS0: u32 = SEG_BASE + 0x0a;
/// 目的空间不足。
pub const DC_ENOSPC: u32 = SEG_BASE + 0x0b;
/// 尺寸串为空。
pub const DC_EEMPTY: u32 = SEG_BASE + 0x0c;
/// 后缀非法。
pub const DC_EBSUF: u32 = SEG_BASE + 0x0d;
/// 数值溢出。
pub const DC_EOVF: u32 = SEG_BASE + 0x0e;
/// 块尺寸超上限。
pub const DC_EBLK: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：整段复制。
pub const OP_COPY: u8 = 1;
/// 操作：完整块数。
pub const OP_BLOCKS: u8 = 2;
/// 操作：短块数。
pub const OP_PARTIAL: u8 = 3;
/// 操作：块尺寸解析。
pub const OP_PARSE: u8 = 4;

/// 块尺寸上限（1MiB）。
pub const MAX_BS: u32 = 1 << 20;
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

/// 复制统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub full: u32,
    pub part: u32,
    pub bytes: u32,
    pub bs: u32,
}

impl Stats {
    /// 空统计。
    pub const fn blank() -> Stats {
        Stats {
            full: 0,
            part: 0,
            bytes: 0,
            bs: 0,
        }
    }
}

/// 单个尺寸因子：数字加可选后缀。
fn term(s: &[u8], i: &mut usize) -> Result<u32, u32> {
    let start = *i;
    let mut v = 0u32;
    while let Some(&c) = s.get(*i) {
        if !c.is_ascii_digit() {
            break;
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add(u32::from(c - b'0')))
        {
            Some(x) => x,
            None => return Err(DC_EOVF),
        };
        *i += 1;
    }
    if *i == start {
        return Err(DC_EBSUF);
    }
    let m = match s.get(*i) {
        Some(&b'c') => {
            *i += 1;
            1u32
        }
        Some(&b'w') => {
            *i += 1;
            2
        }
        Some(&b'b') => {
            *i += 1;
            512
        }
        Some(&b'k') | Some(&b'K') => {
            *i += 1;
            1024
        }
        Some(&b'M') => {
            *i += 1;
            1024 * 1024
        }
        Some(&b'x') => 1,
        Some(_) => return Err(DC_EBSUF),
        None => 1,
    };
    v.checked_mul(m).ok_or(DC_EOVF)
}

/// 块尺寸解析：数 [后缀] [x 数 [后缀]]。
pub fn parse_bs(s: &[u8]) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(DC_EEMPTY);
    }
    let mut i = 0usize;
    let a = term(s, &mut i)?;
    let mut v = a;
    if s.get(i) == Some(&b'x') {
        i += 1;
        let b = term(s, &mut i)?;
        v = a.checked_mul(b).ok_or(DC_EOVF)?;
    }
    if i != s.len() {
        return Err(DC_EBSUF);
    }
    if v == 0 {
        return Err(DC_EBS0);
    }
    Ok(v)
}

/// 基本复制：按 bs 分块，最多 count 块。
pub fn copy(src: &[u8], dst: &mut [u8], bs: u32, count: u32) -> Result<Stats, u32> {
    if bs == 0 {
        return Err(DC_EBS0);
    }
    if bs > MAX_BS {
        return Err(DC_EBLK);
    }
    let bs_u = bs as usize;
    let mut s = 0usize;
    let mut d = 0usize;
    let mut full = 0u32;
    let mut part = 0u32;
    let mut n = 0u32;
    while n < count && s < src.len() {
        let avail = src.len() - s;
        let take = if bs_u < avail { bs_u } else { avail };
        let end = match d.checked_add(take) {
            Some(v) => v,
            None => return Err(DC_ENOSPC),
        };
        if end > dst.len() {
            return Err(DC_ENOSPC);
        }
        let mut k = 0usize;
        while k < take {
            match (src.get(s + k), dst.get_mut(d + k)) {
                (Some(&b), Some(slot)) => *slot = b,
                _ => return Err(DC_ENOSPC),
            }
            k += 1;
        }
        s += take;
        d = end;
        if take == bs_u {
            full += 1;
        } else {
            part += 1;
        }
        n += 1;
    }
    Ok(Stats {
        full,
        part,
        bytes: d as u32,
        bs,
    })
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
            return Err(DC_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(DC_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(DC_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(DC_EORDER);
        }
        if self.len >= self.cap {
            return Err(DC_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(DC_EID),
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
            return Err(DC_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DC_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(DC_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(DC_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(DC_EMISMATCH);
                    }
                }
                None => return Err(DC_ENOBS),
            }
            i += 1;
        }
        let vals = [10u32, 2, 1, 12, 3, 8192, 1024, 1048576];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(DC_EMISMATCH);
                    }
                }
                _ => return Err(DC_EBAL),
            }
            k += 1;
        }
        let codes = [DC_EBS0, DC_ENOSPC, DC_EEMPTY, DC_EBSUF, DC_EOVF, DC_EBLK];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DC_EMISMATCH);
                        }
                    }
                    None => return Err(DC_EBAL),
                },
                None => return Err(DC_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(DC_EBAL);
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
            return Err(DC_EFROZEN);
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
            return Err(DC_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(DC_ESEAL);
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
const SRC12: [u8; 12] = [
    b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', b'j', b'k', b'l',
];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut dst = [0u8; 32];

    let _ = l.register(cell(0, OP_COPY, copy(&SRC10, &mut dst, 4, 3).map(|s| s.bytes)));
    let _ = l.register(cell(1, OP_BLOCKS, copy(&SRC10, &mut dst, 4, 3).map(|s| s.full)));
    let _ = l.register(cell(2, OP_PARTIAL, copy(&SRC10, &mut dst, 4, 3).map(|s| s.part)));
    let _ = l.register(cell(3, OP_COPY, copy(&SRC12, &mut dst, 4, 3).map(|s| s.bytes)));
    let _ = l.register(cell(4, OP_BLOCKS, copy(&SRC12, &mut dst, 4, 3).map(|s| s.full)));
    let _ = l.register(cell(5, OP_PARSE, parse_bs(b"8k")));
    let _ = l.register(cell(6, OP_PARSE, parse_bs(b"2x512")));
    let _ = l.register(cell(7, OP_PARSE, parse_bs(b"1M")));

    let _ = l.register(cell(8, OP_COPY, copy(&SRC10, &mut dst, 0, 1).map(|s| s.bytes)));
    let _ = l.register(cell(9, OP_COPY, copy(&SRC10, &mut dst[..6], 4, 3).map(|s| s.bytes)));
    let _ = l.register(cell(10, OP_PARSE, parse_bs(b"")));
    let _ = l.register(cell(11, OP_PARSE, parse_bs(b"8q")));
    let _ = l.register(cell(12, OP_PARSE, parse_bs(b"999999999999")));
    let _ = l.register(cell(13, OP_COPY, copy(&SRC10, &mut dst, MAX_BS + 1, 1).map(|s| s.bytes)));

    l
}

/// 四路注入：账本自洽、统计分解、后缀族、目的不足后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(DC_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let mut dst = [0u8; 32];
    out[1] = match copy(&SRC10, &mut dst, 4, 3) {
        Ok(s) => {
            s.full == 2
                && s.part == 1
                && s.bytes == 10
                && s.bytes == s.full * s.bs + 2
                && dst.get(9) == Some(&b'9')
        }
        Err(_) => false,
    };

    out[2] = parse_bs(b"512") == Ok(512)
        && parse_bs(b"1w") == Ok(2)
        && parse_bs(b"2x512") == Ok(1024)
        && parse_bs(b"1k") == Ok(1024)
        && parse_bs(b"2b") == Ok(1024);

    let mut tiny = [0u8; 6];
    let over = copy(&SRC10, &mut tiny, 4, 3);
    let mut roomy = [0u8; 16];
    let fits = copy(&SRC10, &mut roomy, 4, 3);
    out[3] = over == Err(DC_ENOSPC)
        && fits.map(|s| s.bytes) == Ok(10)
        && roomy.get(10) == Some(&0);
    out
}

/// 十三判据。
pub fn run_unx11488_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11488.ddcopy");
    let l = build_ledger();

    s.add(
        "DC1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_COPY),
        "static",
    );
    s.add_dyn(
        "DC2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "DC3-十字节三块",
        {
            let mut dst = [0u8; 32];
            match copy(&SRC10, &mut dst, 4, 3) {
                Ok(s) => s.bytes == 10 && s.full == 2 && s.part == 1 && s.bs == 4,
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "DC4-十二字节整齐",
        {
            let mut dst = [0u8; 32];
            match copy(&SRC12, &mut dst, 4, 3) {
                Ok(s) => s.bytes == 12 && s.full == 3 && s.part == 0 && dst.get(11) == Some(&b'l'),
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "DC5-count 截断",
        {
            let mut dst = [0u8; 32];
            copy(&SRC12, &mut dst, 4, 2).map(|s| s.bytes) == Ok(8)
                && l.sample(3).map(|x| x.value) == Some(12)
        },
        "static",
    );
    s.add(
        "DC6-后缀族",
        parse_bs(b"512") == Ok(512)
            && parse_bs(b"1k") == Ok(1024)
            && parse_bs(b"1M") == Ok(1048576)
            && parse_bs(b"2b") == Ok(1024)
            && parse_bs(b"1w") == Ok(2),
        "static",
    );
    s.add(
        "DC7-乘法形式",
        parse_bs(b"2x512") == Ok(1024)
            && parse_bs(b"8x8k") == Ok(65536)
            && parse_bs(b"1x1x1") == Err(DC_EBSUF),
        "static",
    );
    s.add(
        "DC8-统计自洽",
        {
            let mut dst = [0u8; 32];
            match copy(&SRC10, &mut dst, 4, 3) {
                Ok(s) => s.bytes == s.full * s.bs + 2 && s.full + s.part == 3,
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "DC9-零块闸门",
        copy(&SRC10, &mut [0u8; 32], 0, 1) == Err(DC_EBS0)
            && l.sample(8).map(|x| x.code) == Some(DC_EBS0),
        "static",
    );
    s.add(
        "DC10-目的不足",
        copy(&SRC10, &mut [0u8; 6], 4, 3) == Err(DC_ENOSPC)
            && l.sample(9).map(|x| x.code) == Some(DC_ENOSPC),
        "static",
    );
    s.add(
        "DC11-空串与非法后缀",
        parse_bs(b"") == Err(DC_EEMPTY)
            && parse_bs(b"8q") == Err(DC_EBSUF)
            && parse_bs(b"q8") == Err(DC_EBSUF)
            && l.sample(10).map(|x| x.code) == Some(DC_EEMPTY)
            && l.sample(11).map(|x| x.code) == Some(DC_EBSUF),
        "static",
    );
    s.add(
        "DC12-溢出与块上限",
        parse_bs(b"999999999999") == Err(DC_EOVF)
            && parse_bs(b"99999x99999") == Err(DC_EOVF)
            && copy(&SRC10, &mut [0u8; 32], MAX_BS + 1, 1) == Err(DC_EBLK)
            && l.sample(12).map(|x| x.code) == Some(DC_EOVF)
            && l.sample(13).map(|x| x.code) == Some(DC_EBLK),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DC_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "DC13-冻结重放与四路",
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
    fn blocks_and_partial() {
        let mut dst = [0u8; 32];
        let s = copy(&SRC10, &mut dst, 4, 3).expect("copy");
        assert_eq!(s.full, 2);
        assert_eq!(s.part, 1);
        assert_eq!(s.bytes, 10);
        assert_eq!(dst.get(0), Some(&b'0'));
    }

    #[test]
    fn count_stops_early() {
        let mut dst = [0u8; 32];
        assert_eq!(copy(&SRC12, &mut dst, 4, 2).map(|s| s.bytes), Ok(8));
        assert_eq!(copy(&SRC12, &mut dst, 4, 0).map(|s| s.bytes), Ok(0));
    }

    #[test]
    fn suffix_family() {
        assert_eq!(parse_bs(b"1c"), Ok(1));
        assert_eq!(parse_bs(b"1w"), Ok(2));
        assert_eq!(parse_bs(b"1b"), Ok(512));
        assert_eq!(parse_bs(b"1K"), Ok(1024));
        assert_eq!(parse_bs(b"1M"), Ok(1048576));
        assert_eq!(parse_bs(b"2x512"), Ok(1024));
    }

    #[test]
    fn parse_rejects() {
        assert_eq!(parse_bs(b""), Err(DC_EEMPTY));
        assert_eq!(parse_bs(b"0"), Err(DC_EBS0));
        assert_eq!(parse_bs(b"8q"), Err(DC_EBSUF));
        assert_eq!(parse_bs(b"x8"), Err(DC_EBSUF));
        assert_eq!(parse_bs(b"999999999999"), Err(DC_EOVF));
    }

    #[test]
    fn short_destination_fails_whole_block() {
        assert_eq!(copy(&SRC10, &mut [0u8; 6], 4, 3), Err(DC_ENOSPC));
        assert_eq!(copy(&SRC10, &mut [0u8; 32], 0, 1), Err(DC_EBS0));
        assert_eq!(copy(&SRC10, &mut [0u8; 32], MAX_BS + 1, 1), Err(DC_EBLK));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11488_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
