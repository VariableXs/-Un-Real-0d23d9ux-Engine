//! UNX-F48690 —— I35 错误码族 I 段（8 条）
//! 判据原文：UNX-F48690-J1 错误码 ×8 零漂移；判据锚定 D1 同源体系。
//! 判据落地（drill 场景）：R1 八码在册；R2 连续零跳号；R3 零重复；
//! R4 段窗不越界；R5 名称互异非空；R6 漂移检出；R7 码查询；
//! R8 指纹稳定；R9 呈现行；R10 总数守恒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0DA0;

// ── 错误码族 I 段（八条：更新链专用，SEG_BASE+1..+8）────────────
pub const E_USIG: u16 = SEG_BASE + 1; // 更新包签名失败
pub const E_USWAP: u16 = SEG_BASE + 2; // 原子换装失败
pub const E_UWINDOW: u16 = SEG_BASE + 3; // 换装空窗
pub const E_UROLL: u16 = SEG_BASE + 4; // 回滚失败
pub const E_UCHAN: u16 = SEG_BASE + 5; // 更新通道非法
pub const U_EEXTERNAL: u16 = SEG_BASE + 6; // 直连外网
pub const E_UDUP: u16 = SEG_BASE + 7; // 重复更新包
pub const E_UALARM: u16 = SEG_BASE + 8; // 告警缺失
/// 族内条数。
pub const FAMILY_N: usize = 8;

// ── 本卡内部诊断码（族码之后连续编号）─────────────────────────
const E_XDUP: u16 = SEG_BASE + 9; // 码或名重复
const E_XCOUNT: u16 = SEG_BASE + 10; // 族条数不足
const E_XFP: u16 = SEG_BASE + 11; // 指纹漂移
const E_XLINE: u16 = SEG_BASE + 12; // 呈现行缺段

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条错误码登记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ErrCode {
    pub code: u16,
    pub name: String,
}

impl ErrCode {
    pub fn name_ok(&self) -> bool {
        self.name.len() >= 3
    }
}

/// 错误码族账。
pub struct ErrFamily {
    pub items: Vec<ErrCode>,
}

impl ErrFamily {
    pub fn new() -> ErrFamily {
        ErrFamily { items: vec![] }
    }

    /// 登记：连续编号 + 名非空 + 码名唯一。
    pub fn register(&mut self, code: u16, name: &str) -> Result<(), u16> {
        let want = SEG_BASE + self.items.len() as u16 + 1;
        if code != want {
            return Err(E_USWAP);
        }
        let e = ErrCode { code, name: name.to_string() };
        if !e.name_ok() {
            return Err(E_UALARM);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == e.code || self.items[i].name == e.name {
                return Err(E_XDUP);
            }
            i += 1;
        }
        self.items.push(e);
        Ok(())
    }

    /// 族校验：八条齐、连续、唯一、段窗不越、名互异非空。
    pub fn family_check(&self) -> Result<(), u16> {
        if self.items.len() != FAMILY_N {
            return Err(E_XCOUNT);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code != SEG_BASE + i as u16 + 1 {
                return Err(E_USIG);
            }
            if self.items[i].code > SEG_BASE + 0x1F {
                return Err(E_UWINDOW);
            }
            if !self.items[i].name_ok() {
                return Err(E_UALARM);
            }
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].code == self.items[j].code
                    || self.items[i].name == self.items[j].name
                {
                    return Err(E_XDUP);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 按名查码。
    pub fn by_name(&self, name: &str) -> Option<u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].name == name {
                return Some(self.items[i].code);
            }
            i += 1;
        }
        None
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.items.len() {
            buf.extend_from_slice(&self.items[i].code.to_le_bytes());
            buf.extend_from_slice(self.items[i].name.as_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造本族八条（冻结名表）。
pub fn build_family() -> Result<ErrFamily, u16> {
    let mut f = ErrFamily::new();
    f.register(E_USIG, "upd-sig")?;
    f.register(E_USWAP, "upd-swap")?;
    f.register(E_UWINDOW, "upd-window")?;
    f.register(E_UROLL, "upd-rollback")?;
    f.register(E_UCHAN, "upd-channel")?;
    f.register(U_EEXTERNAL, "upd-external")?;
    f.register(E_UDUP, "upd-dup")?;
    f.register(E_UALARM, "upd-alarm")?;
    Ok(f)
}

/// 呈现行：n=<条数> lo=<首码> hi=<末码>。
pub fn family_line(f: &ErrFamily) -> String {
    let lo = if f.items.is_empty() { 0 } else { f.items[0].code };
    let hi = if f.items.is_empty() { 0 } else { f.items[f.items.len() - 1].code };
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&f.items.len().to_string());
    out.push_str(" lo=");
    out.push_str(&lo.to_string());
    out.push_str(" hi=");
    out.push_str(&hi.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// u16 十进制串。
pub fn concat_u16(v: u16) -> String {
    let mut out = String::new();
    out.push_str(&v.to_string());
    out
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_90);

    // R1 八码在册。
    let fam = build_family()?;
    if fam.items.len() != FAMILY_N {
        return Err(E_XCOUNT);
    }
    hits += 1;

    // R2-R4 连续零跳号 + 零重复 + 段窗不越 + 名互异非空。
    fam.family_check()?;
    hits += 1;

    // R5 跳号检出。
    let mut gap = ErrFamily::new();
    gap.register(E_USIG, "upd-sig")?;
    gap.register(E_USWAP, "upd-swap")?;
    match gap.register(E_UCHAN, "upd-channel") {
        Err(E_USWAP) => hits += 1,
        _ => return Err(E_USWAP),
    }

    // R6 名称重复检出。
    let mut dup = ErrFamily::new();
    dup.register(E_USIG, "same")?;
    match dup.register(E_USWAP, "same") {
        Err(E_XDUP) => hits += 1,
        _ => return Err(E_XDUP),
    }

    // R7 码查询：按名查回冻结码。
    let q1 = fam.by_name("upd-channel");
    let q2 = fam.by_name("upd-external");
    let q3 = fam.by_name("none");
    if q1 != Some(E_UCHAN) || q2 != Some(U_EEXTERNAL) || q3.is_some() {
        return Err(E_UROLL);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = fam.fp();
    let f2 = fam.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_XFP);
    }
    hits += 1;

    // R9 呈现行：n=8 lo=<+1> hi=<+8>。
    let line = family_line(&fam);
    if !has_kw(line.as_bytes(), b"n=8")
        || !has_kw(line.as_bytes(), concat_u16(SEG_BASE + 1).as_bytes())
        || !has_kw(line.as_bytes(), concat_u16(SEG_BASE + 8).as_bytes())
    {
        return Err(E_XLINE);
    }
    hits += 1;

    // R10 总数守恒：族外再登必红；漂移族检出。
    let mut over = build_family()?;
    match over.register(SEG_BASE + 9, "over") {
        Err(E_USWAP) => {}
        _ => return Err(E_USWAP),
    }
    let mut drift = ErrFamily::new();
    drift.register(SEG_BASE + 1, "a")?;
    drift.register(SEG_BASE + 3, "c")?;
    match drift.family_check() {
        Err(E_USIG) => hits += 1,
        _ => return Err(E_USIG),
    }
    let _ = g.below(10);

    Ok(hits)
}
