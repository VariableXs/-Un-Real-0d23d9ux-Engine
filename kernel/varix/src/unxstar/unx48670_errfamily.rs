//! UNX-F48670 —— I34 错误码族 I 段（6 条）
//! 判据原文：UNX-F48670-J1 错误码 ×6 零漂移；判据锚定 D1 同源体系。
//! 判据落地（drill 场景）：R1 六码在册；R2 连续零跳号；R3 零重复；
//! R4 段窗不越界；R5 名称互异非空；R6 漂移检出；R7 码查询；
//! R8 指纹稳定；R9 呈现行；R10 总数守恒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0B20;

// ── 诊断码闭集（本卡主题即错误码族：六条，SEG_BASE+1..+6）───────
pub const E_ACC: u16 = SEG_BASE + 1; // 打包账缺体
pub const E_LAT: u16 = SEG_BASE + 2; // 延迟段缺位
pub const E_REG: u16 = SEG_BASE + 3; // 收录登记失败
pub const E_READY: u16 = SEG_BASE + 4; // 就绪位自检失败
pub const E_RATIO: u16 = SEG_BASE + 5; // 占比机检不达
pub const E_NOTIFY: u16 = SEG_BASE + 6; // 通知链不可达
/// 族内条数。
pub const FAMILY_N: usize = 6;

// ── 本卡内部诊断码（族码之后连续编号）─────────────────────────
const E_DUP: u16 = SEG_BASE + 7; // 码或名重复
const E_COUNT: u16 = SEG_BASE + 8; // 族条数不足
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

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
    /// 名称合法：非空 ≥3 字符。
    pub fn name_ok(&self) -> bool {
        self.name.len() >= 3
    }
}

/// 错误码族账：登记-校验-查询。
pub struct ErrFamily {
    pub items: Vec<ErrCode>,
}

impl ErrFamily {
    pub fn new() -> ErrFamily {
        ErrFamily { items: vec![] }
    }

    /// 登记：码必须等于 SEG_BASE + 已有条数 + 1（连续编号），名非空，码唯一。
    pub fn register(&mut self, code: u16, name: &str) -> Result<(), u16> {
        let want = SEG_BASE + self.items.len() as u16 + 1;
        if code != want {
            return Err(E_RATIO);
        }
        let e = ErrCode { code, name: name.to_string() };
        if !e.name_ok() {
            return Err(E_REG);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == e.code || self.items[i].name == e.name {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(e);
        Ok(())
    }

    /// 族校验：六条齐、连续、唯一、段窗不越界、名互异非空。
    pub fn family_check(&self) -> Result<(), u16> {
        if self.items.len() != FAMILY_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code != SEG_BASE + i as u16 + 1 {
                return Err(E_ACC);
            }
            if self.items[i].code > SEG_BASE + 0x1F {
                return Err(E_LAT);
            }
            if !self.items[i].name_ok() {
                return Err(E_REG);
            }
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].code == self.items[j].code
                    || self.items[i].name == self.items[j].name
                {
                    return Err(E_DUP);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 码查询：按名查码 / 按码查名。
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

/// 构造本族六条（冻结名表）。
pub fn build_family() -> Result<ErrFamily, u16> {
    let mut f = ErrFamily::new();
    f.register(E_ACC, "acc-miss")?;
    f.register(E_LAT, "lat-miss")?;
    f.register(E_REG, "reg-deny")?;
    f.register(E_READY, "ready-red")?;
    f.register(E_RATIO, "ratio-low")?;
    f.register(E_NOTIFY, "notify-dead")?;
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_70);

    // R1 六码在册。
    let fam = build_family()?;
    if fam.items.len() != FAMILY_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 连续零跳号 + 零重复 + 段窗不越界 + 名互异非空。
    fam.family_check()?;
    hits += 1;

    // R3 跳号检出：漏登中间码 → 下一码断言红。
    let mut gap = ErrFamily::new();
    gap.register(E_ACC, "acc-miss")?;
    gap.register(E_LAT, "lat-miss")?;
    gap.register(E_NOTIFY, "notify-dead")?; // 第 3 条必须是 +4，红
    match gap.register(E_NOTIFY, "notify-dead") {
        Err(E_RATIO) => hits += 1,
        _ => return Err(E_RATIO),
    }

    // R4 名称重复检出。
    let mut dup = ErrFamily::new();
    dup.register(E_ACC, "same-name")?;
    match dup.register(E_LAT, "same-name") {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R5 空名拒收。
    let mut noname = ErrFamily::new();
    match noname.register(E_ACC, "") {
        Err(E_REG) => hits += 1,
        _ => return Err(E_REG),
    }

    // R6 码查询：按名查回冻结码。
    let q1 = fam.by_name("ratio-low");
    let q2 = fam.by_name("acc-miss");
    let q3 = fam.by_name("no-such");
    if q1 != Some(E_RATIO) || q2 != Some(E_ACC) || q3.is_some() {
        return Err(E_READY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = fam.fp();
    let f2 = fam.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：n=6 lo=<SEG_BASE+1> hi=<SEG_BASE+6>。
    let line = family_line(&fam);
    let lo_want = concat_u16(SEG_BASE + 1);
    let hi_want = concat_u16(SEG_BASE + 6);
    if !has_kw(line.as_bytes(), b"n=6") || !has_kw(line.as_bytes(), lo_want.as_bytes())
        || !has_kw(line.as_bytes(), hi_want.as_bytes())
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 总数守恒：六条之外再登必红（连续编号越族）。
    let mut over = build_family()?;
    match over.register(SEG_BASE + 7, "over-family") {
        Err(E_RATIO) => hits += 1,
        _ => return Err(E_RATIO),
    }

    // R10 漂移检出：构造错位族（跳过 +2）→ family_check 红。
    let mut drift = ErrFamily::new();
    drift.register(SEG_BASE + 1, "a-code")?;
    drift.register(SEG_BASE + 3, "c-code")?;
    match drift.family_check() {
        Err(E_ACC) => hits += 1,
        _ => return Err(E_ACC),
    }
    let _ = g.below(10);

    Ok(hits)
}

/// u16 十进制字符串拼装（呈现断言辅助）。
pub fn concat_u16(v: u16) -> String {
    let mut out = String::new();
    out.push_str(&v.to_string());
    out
}
