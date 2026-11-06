//! UNX-F11479 statfs 字段溢出与钳制（0x20dc0 段）
//!
//! `statfs` 用文件系统块为单位汇报容量，块大小由 `f_frsize` 决定；当
//! `f_frsize` 为零时必须回落到 `f_bsize`，否则容量计算会退化成零。块数、
//! 空闲块、可用块、文件数与空闲文件数在 64 位视图里都是 `u64`，但旧的
//! 32 位兼容结构只有 `u32` 字段，超出范围时只能饱和到 `u32::MAX` 并记录
//! 钳制次数；严格模式下同样的溢出必须返回 `EOVERFLOW`，否则调用方会把
//! 饱和值当成真实容量。
//!
//! 账本把所有计数变化都压缩进一行结果：块数、空闲块、可用块、使用率、
//! 总字节数以及钳制次数。使用率以块数之差为分子、总块数为分母做整数
//! 除法，零块文件系统直接给零，绝不触发除零；总字节数用 `checked_mul`
//! 计算，溢出时饱和到 `u64::MAX` 并把 `ovf` 置真。`bavail` 大于 `bfree`
//! 属于损坏的超级块，读取时按 `bfree` 钳制而不是报错；`bfree` 大于
//! `blocks` 或 `ffree` 大于 `files` 则是硬错误，直接返回 `EBAL`。
//!
//! 不变量：`bfree <= blocks`、`bavail <= bfree`、`used_pct <= 100`、
//! `namelen <= 255`；`clamped` 只统计真正发生饱和的字段；`ovf` 为真时
//! `total_bytes` 必为 `u64::MAX`。冻结后的账本拒绝任何追加，重放必须
//! 重建出同一签名。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20dc0;
pub const SF_EID: u32 = SEG_BASE + 0x01;
pub const SF_EORDER: u32 = SEG_BASE + 0x02;
pub const SF_EDUP: u32 = SEG_BASE + 0x03;
pub const SF_EFROZEN: u32 = SEG_BASE + 0x04;
pub const SF_ENOBS: u32 = SEG_BASE + 0x05;
pub const SF_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const SF_EBAL: u32 = SEG_BASE + 0x07;
pub const SF_ESEAL: u32 = SEG_BASE + 0x08;
pub const SF_EINVAL: u32 = SEG_BASE + 0x09;
pub const SF_EOVERFLOW: u32 = SEG_BASE + 0x0a;
pub const SF_ENOSPC: u32 = SEG_BASE + 0x0b;

pub const SAMPLES: usize = 14;
pub const NAME_MAX: u32 = 255;
pub const HUGE_BLOCKS: u64 = (u64::MAX / 4096) + 1;

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

/// 文件系统超级块计数（64 位原始视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fs {
    pub bsize: u32,
    pub frsize: u32,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub namemax: u32,
}

/// 一次 statfs 查询的结果视图（32 位兼容结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub bsize: u32,
    pub frsize: u32,
    pub blocks: u32,
    pub bfree: u32,
    pub bavail: u32,
    pub files: u32,
    pub ffree: u32,
    pub namelen: u32,
    pub used_pct: u32,
    pub total_bytes: u64,
    pub clamped: u32,
    pub ovf: bool,
}

/// 基准超级块。
pub fn base_fs() -> Fs {
    Fs {
        bsize: 4096,
        frsize: 0,
        blocks: 1000,
        bfree: 400,
        bavail: 300,
        files: 200,
        ffree: 50,
        namemax: NAME_MAX,
    }
}

fn clamp32(v: u64, strict: bool, clamped: &mut u32) -> Result<u32, u32> {
    if v > (u32::MAX as u64) {
        if strict {
            return Err(SF_EOVERFLOW);
        }
        *clamped += 1;
        Ok(u32::MAX)
    } else {
        Ok(v as u32)
    }
}

/// 把 64 位计数压进 32 位兼容视图。
pub fn stat(fs: &Fs, strict: bool) -> Result<View, u32> {
    if fs.bsize == 0 {
        return Err(SF_EINVAL);
    }
    if fs.bfree > fs.blocks || fs.ffree > fs.files {
        return Err(SF_EBAL);
    }
    let frsize = if fs.frsize == 0 { fs.bsize } else { fs.frsize };
    let mut clamped = 0u32;
    let mut ovf = false;
    let total_bytes = match fs.blocks.checked_mul(frsize as u64) {
        Some(v) => v,
        None => {
            if strict {
                return Err(SF_EOVERFLOW);
            }
            clamped += 1;
            ovf = true;
            u64::MAX
        }
    };
    let bavail = if fs.bavail > fs.bfree {
        clamped += 1;
        fs.bfree
    } else {
        fs.bavail
    };
    let blocks = clamp32(fs.blocks, strict, &mut clamped)?;
    let bfree = clamp32(fs.bfree, strict, &mut clamped)?;
    let bavail32 = clamp32(bavail, strict, &mut clamped)?;
    let files = clamp32(fs.files, strict, &mut clamped)?;
    let ffree = clamp32(fs.ffree, strict, &mut clamped)?;
    let namelen = if fs.namemax > NAME_MAX {
        clamped += 1;
        NAME_MAX
    } else {
        fs.namemax
    };
    let used = fs.blocks.saturating_sub(fs.bfree);
    let used_pct = if fs.blocks == 0 {
        0
    } else {
        (used.saturating_mul(100) / fs.blocks) as u32
    };
    Ok(View {
        bsize: fs.bsize,
        frsize,
        blocks,
        bfree,
        bavail: bavail32,
        files,
        ffree,
        namelen,
        used_pct,
        total_bytes,
        clamped,
        ovf,
    })
}

/// 按场景编号构造查询。
pub fn run_scen(id: u8) -> Result<View, u32> {
    let mut fs = base_fs();
    match id {
        0 => stat(&fs, false),
        1 => {
            fs.blocks = 5_000_000_000;
            fs.bfree = 1_000_000_000;
            fs.bavail = 900_000_000;
            fs.files = 300;
            fs.ffree = 200;
            stat(&fs, false)
        }
        2 => {
            fs.bfree = 1200;
            stat(&fs, false)
        }
        3 => {
            fs.ffree = 250;
            stat(&fs, false)
        }
        4 => {
            fs.bavail = 500;
            stat(&fs, false)
        }
        5 => {
            fs.frsize = 512;
            stat(&fs, false)
        }
        6 => {
            fs.namemax = 300;
            stat(&fs, false)
        }
        7 => {
            fs.blocks = 0;
            fs.bfree = 0;
            fs.bavail = 0;
            fs.files = 0;
            fs.ffree = 0;
            stat(&fs, false)
        }
        8 => {
            fs.bfree = 1000;
            fs.bavail = 1000;
            stat(&fs, false)
        }
        9 => {
            fs.bfree = 0;
            fs.bavail = 0;
            stat(&fs, false)
        }
        10 => {
            fs.blocks = HUGE_BLOCKS;
            fs.bfree = 1_000_000_000;
            fs.bavail = 900_000_000;
            fs.files = 300;
            fs.ffree = 200;
            stat(&fs, false)
        }
        11 => {
            fs.blocks = HUGE_BLOCKS;
            fs.bfree = 1_000_000_000;
            fs.bavail = 900_000_000;
            stat(&fs, true)
        }
        12 => {
            fs.files = u32::MAX as u64;
            fs.ffree = 0;
            stat(&fs, false)
        }
        13 => {
            fs.blocks = 3;
            fs.bfree = 2;
            fs.bavail = 1;
            fs.files = 10;
            fs.ffree = 9;
            stat(&fs, false)
        }
        _ => Err(SF_EID),
    }
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub blocks: u32,
    pub bfree: u32,
    pub bavail: u32,
    pub used_pct: u32,
    pub total_bytes: u64,
    pub clamped: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub blocks: u32,
    pub bfree: u32,
    pub bavail: u32,
    pub used_pct: u32,
    pub total_bytes: u64,
    pub clamped: u32,
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
        blocks: u32,
        bfree: u32,
        bavail: u32,
        used_pct: u32,
        total_bytes: u64,
        clamped: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SF_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SF_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(SF_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(SF_EORDER);
        }
        let s = Sample {
            scen,
            expect_ok: ok,
            blocks,
            bfree,
            bavail,
            used_pct,
            total_bytes,
            clamped,
            code,
        };
        let out = match run_scen(scen) {
            Ok(v) => {
                if !ok
                    || v.blocks != blocks
                    || v.bfree != bfree
                    || v.bavail != bavail
                    || v.used_pct != used_pct
                    || v.total_bytes != total_bytes
                    || v.clamped != clamped
                {
                    return Err(SF_EMISMATCH);
                }
                Outcome {
                    ok: true,
                    blocks: v.blocks,
                    bfree: v.bfree,
                    bavail: v.bavail,
                    used_pct: v.used_pct,
                    total_bytes: v.total_bytes,
                    clamped: v.clamped,
                    code: 0,
                }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(SF_EMISMATCH);
                }
                Outcome {
                    ok: false,
                    blocks: 0,
                    bfree: 0,
                    bavail: 0,
                    used_pct: 0,
                    total_bytes: 0,
                    clamped: 0,
                    code: low,
                }
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
            return Err(SF_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(SF_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(SF_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(SF_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok
                        || v.blocks != o.blocks
                        || v.bfree != o.bfree
                        || v.bavail != o.bavail
                        || v.used_pct != o.used_pct
                        || v.total_bytes != o.total_bytes
                        || v.clamped != o.clamped
                    {
                        return Err(SF_EMISMATCH);
                    }
                    if v.bfree > v.blocks || v.bavail > v.bfree {
                        return Err(SF_EBAL);
                    }
                    if v.used_pct > 100 || v.namelen > NAME_MAX {
                        return Err(SF_EBAL);
                    }
                    if v.ovf && v.total_bytes != u64::MAX {
                        return Err(SF_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(SF_EMISMATCH);
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
            return Err(SF_EFROZEN);
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
                h = fold(h, o.blocks as u64);
                h = fold(h, o.bfree as u64);
                h = fold(h, o.bavail as u64);
                h = fold(h, o.used_pct as u64);
                h = fold(h, o.total_bytes);
                h = fold(h, o.clamped as u64);
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
            Err(SF_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let ebal = (SF_EBAL & 0xffff) as u16;
    let eovf = (SF_EOVERFLOW & 0xffff) as u16;
    let _ = l.register(0, 0, true, 1000, 400, 300, 60, 4_096_000, 0, 0);
    let _ = l.register(1, 1, true, u32::MAX, 1_000_000_000, 900_000_000, 80, 20_480_000_000_000, 1, 0);
    let _ = l.register(2, 2, false, 0, 0, 0, 0, 0, 0, ebal);
    let _ = l.register(3, 3, false, 0, 0, 0, 0, 0, 0, ebal);
    let _ = l.register(4, 4, true, 1000, 400, 400, 60, 4_096_000, 1, 0);
    let _ = l.register(5, 5, true, 1000, 400, 300, 60, 512_000, 0, 0);
    let _ = l.register(6, 6, true, 1000, 400, 300, 60, 4_096_000, 1, 0);
    let _ = l.register(7, 7, true, 0, 0, 0, 0, 0, 0, 0);
    let _ = l.register(8, 8, true, 1000, 1000, 1000, 0, 4_096_000, 0, 0);
    let _ = l.register(9, 9, true, 1000, 0, 0, 100, 4_096_000, 0, 0);
    let _ = l.register(10, 10, true, u32::MAX, 1_000_000_000, 900_000_000, 99, u64::MAX, 2, 0);
    let _ = l.register(11, 11, false, 0, 0, 0, 0, 0, 0, eovf);
    let _ = l.register(12, 12, true, 1000, 400, 300, 60, 4_096_000, 0, 0);
    let _ = l.register(13, 13, true, 3, 2, 1, 33, 12_288, 0, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(SF_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut fs = base_fs();
    fs.blocks = 5_000_000_000;
    fs.bfree = 1_000_000_000;
    fs.bavail = 900_000_000;
    let compat = stat(&fs, false);
    let strict = stat(&fs, true);
    out[1] = compat.map(|v| (v.blocks, v.clamped)) == Ok((u32::MAX, 1))
        && strict == Err(SF_EOVERFLOW);

    let mut fs = base_fs();
    fs.bavail = 500;
    let clamp_avail = stat(&fs, false);
    let mut nm = base_fs();
    nm.namemax = 300;
    let clamp_name = stat(&nm, false);
    let mut z = base_fs();
    z.blocks = 0;
    z.bfree = 0;
    z.bavail = 0;
    let empty = stat(&z, false);
    out[2] = clamp_avail.map(|v| (v.bavail, v.clamped)) == Ok((400, 1))
        && clamp_name.map(|v| (v.namelen, v.clamped)) == Ok((255, 1))
        && empty.map(|v| (v.used_pct, v.total_bytes)) == Ok((0, 0));

    let mut big = base_fs();
    big.blocks = HUGE_BLOCKS;
    big.bfree = 1_000_000_000;
    big.bavail = 900_000_000;
    let sat = stat(&big, false);
    let mut small_fr = base_fs();
    small_fr.frsize = 512;
    let scaled = stat(&small_fr, false);
    let mut three = base_fs();
    three.blocks = 3;
    three.bfree = 2;
    three.bavail = 1;
    let frac = stat(&three, false);
    out[3] = sat.map(|v| (v.total_bytes, v.ovf, v.clamped)) == Ok((u64::MAX, true, 2))
        && scaled.map(|v| v.total_bytes) == Ok(512_000)
        && frac.map(|v| v.used_pct) == Ok(33);
    out
}

/// 十三项判据。
pub fn run_unx11479_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11479_statfs_clamp");
    let l = build_ledger();
    s.add(
        "SF1-十四行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "SF2-十一应三拒",
        l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "SF3-frsize 回落 bsize",
        run_scen(0).map(|v| (v.frsize, v.bsize)) == Ok((4096, 4096)),
        "static",
    );
    s.add(
        "SF4-可用块按空闲块钳制",
        run_scen(4).map(|v| (v.bavail, v.clamped)) == Ok((400, 1)),
        "static",
    );
    s.add(
        "SF5-32 位块数饱和",
        run_scen(1).map(|v| (v.blocks, v.clamped)) == Ok((u32::MAX, 1)),
        "static",
    );
    s.add(
        "SF6-严格模式溢出",
        run_scen(11) == Err(SF_EOVERFLOW),
        "static",
    );
    s.add(
        "SF7-非法空闲块拒绝",
        run_scen(2) == Err(SF_EBAL),
        "static",
    );
    s.add(
        "SF8-零块除零保护",
        run_scen(7).map(|v| (v.used_pct, v.total_bytes)) == Ok((0, 0)),
        "static",
    );
    s.add(
        "SF9-全满使用率",
        run_scen(9).map(|v| v.used_pct) == Ok(100),
        "static",
    );
    s.add(
        "SF10-名字上限钳制",
        run_scen(6).map(|v| (v.namelen, v.clamped)) == Ok((255, 1)),
        "static",
    );
    s.add(
        "SF11-总字节饱和",
        run_scen(10).map(|v| (v.total_bytes, v.ovf)) == Ok((u64::MAX, true)),
        "static",
    );
    s.add(
        "SF12-frsize 缩放容量",
        run_scen(5).map(|v| v.total_bytes) == Ok(512_000),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(SF_EFROZEN);
    s.add_dyn(
        "SF13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!(
        "ok={} err={} rows={} pct={}",
        l.ok_count(),
        l.err_count(),
        l.len(),
        l.sample(0).map(|x| x.used_pct).unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frsize_falls_back_to_bsize() {
        let v = stat(&base_fs(), false);
        assert_eq!(v.map(|x| x.frsize), Ok(4096));
    }

    #[test]
    fn compat_saturates_block_count() {
        let mut fs = base_fs();
        fs.blocks = 5_000_000_000;
        fs.bfree = 1_000_000_000;
        fs.bavail = 900_000_000;
        let v = stat(&fs, false);
        assert_eq!(v.map(|x| (x.blocks, x.clamped)), Ok((u32::MAX, 1)));
        assert_eq!(stat(&fs, true), Err(SF_EOVERFLOW));
    }

    #[test]
    fn broken_superblock_rejected() {
        let mut fs = base_fs();
        fs.bfree = 1200;
        assert_eq!(stat(&fs, false), Err(SF_EBAL));
        let mut fs = base_fs();
        fs.ffree = 250;
        assert_eq!(stat(&fs, false), Err(SF_EBAL));
    }

    #[test]
    fn empty_filesystem_has_zero_usage() {
        let mut fs = base_fs();
        fs.blocks = 0;
        fs.bfree = 0;
        fs.bavail = 0;
        assert_eq!(stat(&fs, false).map(|x| x.used_pct), Ok(0));
    }

    #[test]
    fn usage_percent_truncates() {
        let mut fs = base_fs();
        fs.blocks = 3;
        fs.bfree = 2;
        fs.bavail = 1;
        assert_eq!(stat(&fs, false).map(|x| x.used_pct), Ok(33));
    }

    #[test]
    fn byte_total_saturates() {
        let mut fs = base_fs();
        fs.blocks = HUGE_BLOCKS;
        fs.bfree = 1_000_000_000;
        fs.bavail = 900_000_000;
        let v = stat(&fs, false);
        assert_eq!(v.map(|x| (x.total_bytes, x.ovf)), Ok((u64::MAX, true)));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 3);
        assert!(l.audit().is_ok());
        let s = run_unx11479_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
