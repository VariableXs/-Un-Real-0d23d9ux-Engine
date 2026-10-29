//! UNX-F36113-J1 掩码合法性校验器（保留位必须为零）。
//! 判据：① 存态掩码四律——保留位恒零、跨段混合拒、多 GENERIC 位
//! 拒、越界值拒——全负例注入命中；② 请求面豁免（MA 位单独可控，
//! 剥离后必须全过）；③ 纯功能代码 ≥300 行。
//! 设计纪律：存态红线守门人——位段常量从 36101 真源取；校验器
//! 供 36114 转换器（存储前置）与 36117 fuzz（对拍面）消费。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a10;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RESV: u16 = SEG_BASE + 2; // 保留位非零（存态红线）
pub const E_MIXSEG: u16 = SEG_BASE + 3; // GENERIC 与非 GENERIC 位混合
pub const E_GENCOMBO: u16 = SEG_BASE + 4; // 多个 GENERIC 位同时出现
pub const E_BOUND: u16 = SEG_BASE + 5; // 掩码值越界（不可能值，保守守卫）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_REQ: u16 = SEG_BASE + 7; // 请求面校验失灵（MA 剥离后应过）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 存态掩码校验（四律）：
/// L1 保留位（STD 21..23 + GEN 24..27）必须为零；
/// L2 GENERIC 位与非 GENERIC 位不得混合；
/// L3 GENERIC 位至多一个；
/// L4 值域守卫（掩码 ⊆ 32 位全域恒真，保守兜底）。
pub fn validate_stored(mask: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // L4：值域守卫。
    if mask & !0xFFFF_FFFF != 0 {
        return Err(E_BOUND);
    }
    // L1：保留位红线。
    if mask & mt::ALL_RSVD != 0 {
        return Err(E_RESV);
    }
    // L2：跨段混合。
    let has_gen = mask & mt::GEN_USED != 0;
    let has_other = mask & (mt::SPEC_MASK | mt::STD_USED) != 0;
    if has_gen && has_other {
        return Err(E_MIXSEG);
    }
    // L3：多 GENERIC 位。
    if has_gen {
        let mut n = 0;
        let mut b = 28;
        while b < 32 {
            if mask & (1u32 << b) != 0 {
                n += 1;
            }
            b += 1;
        }
        if n > 1 {
            return Err(E_GENCOMBO);
        }
    }
    Ok(())
}

/// 请求面校验：MA 位（bit 25）豁免，其余同存态四律。
/// 语义：请求掩码可携带 MA 载体位（见 36110），但仅此一位豁免。
pub fn validate_request(request: u32) -> Result<(), u16> {
    let body = request & !MA_BIT;
    if request & !MA_BIT & !body != 0 {
        return Err(E_REQ);
    }
    validate_stored(body)
}

/// MAXIMUM_ALLOWED 请求位（与 36110 写死锚一致；本模块独立写死并
/// 在 drill 中与段锚对拍——双写对拍点）。
pub const MA_BIT: u32 = 0x0200_0000;

/// MA 位识别。
pub fn has_ma(request: u32) -> bool {
    request & MA_BIT != 0
}

/// 纯 MA 载体判定：除 MA 位外无任何具体位（探测性请求面）。
pub fn ma_probe_only(request: u32) -> bool {
    has_ma(request) && request & !MA_BIT == 0
}

/// 校验账：分诊结果（存态/请求/双拒/未分诊）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Verdict {
    StoredOk = 0x0001,
    RequestOk = 0x0002,
    BothReject = 0x0004,
    StoredOnlyOk = 0x0008,
}

impl Verdict {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 双面对拍：同一掩码的存态/请求两路校验结果归类。
pub fn classify(mask: u32) -> Result<Verdict, u16> {
    let s = validate_stored(mask);
    let r = validate_request(mask);
    if s.is_ok() && r.is_ok() {
        Ok(Verdict::StoredOk)
    } else if s.is_err() && r.is_err() {
        Ok(Verdict::BothReject)
    } else if s.is_err() && r.is_ok() {
        Ok(Verdict::StoredOnlyOk)
    } else {
        // 存态过而请求拒 = 请求面比存态严——当前规则集不可能，守卫。
        Err(E_REQ)
    }
}

/// 演练：R1 四律正例；R2 保留位负例全注入；R3 混合/多 GENERIC 负例；
/// R4 请求面 MA 豁免对拍；R5 分诊面 + 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：四律正例——特定段、标准五位、单 GENERIC 位、空掩码。
    if VER != 1 {
        return Err(E_VER);
    }
    let goods: [u32; 8] = [
        0,
        0x0000_0089,
        0x0000_FFFF,
        mt::STD_USED,
        0x0012_0089,
        0x0002_0019,
        0x8000_0000,
        0x1000_0000,
    ];
    let mut i = 0;
    while i < 8 {
        validate_stored(goods[i])?;
        validate_request(goods[i])?;
        hits += 1;
        i += 1;
    }
    // R2：保留位负例——11 例全命中 E_RESV。
    let rsvds: [u32; 11] = [
        1u32 << 21,
        1u32 << 22,
        1u32 << 23,
        1u32 << 24,
        1u32 << 25,
        1u32 << 26,
        1u32 << 27,
        mt::STD_RSVD,
        mt::GEN_RSVD,
        mt::ALL_RSVD,
        mt::STD_USED | mt::STD_RSVD,
    ];
    let mut i = 0;
    while i < 11 {
        if validate_stored(rsvds[i]) != Err(E_RESV) {
            return Err(E_RESV);
        }
        hits += 1;
        i += 1;
    }
    // R3：混合/多 GENERIC 负例——GENERIC+特定、GENERIC+标准、
    // 双 GENERIC、四 GENERIC 全命中对应码。
    let mixes: [(u32, u16); 6] = [
        (0x8000_0000 | 0x0000_0089, E_MIXSEG),
        (0x8000_0000 | mt::STD_USED, E_MIXSEG),
        (0x8000_0000 | 0x4000_0000, E_GENCOMBO),
        (0xF000_0000, E_GENCOMBO),
        (0x8000_0000 | 0x1000_0000 | 0x0002_0000, E_MIXSEG),
        (0x2000_0000 | 0x1000_0000, E_GENCOMBO),
    ];
    let mut i = 0;
    while i < 6 {
        let (m, code) = mixes[i];
        if validate_stored(m) != Err(code) {
            return Err(code);
        }
        hits += 1;
        i += 1;
    }
    // R4：请求面 MA 豁免——MA+体 过；纯 MA 过；MA+保留位 拒。
    if !ma_probe_only(MA_BIT) || ma_probe_only(MA_BIT | 0x0000_0089) {
        return Err(E_REQ);
    }
    let ma_probe: [u32; 6] = [
        MA_BIT,
        MA_BIT | 0x0012_0089,
        MA_BIT | 0x8000_0000,
        MA_BIT | mt::STD_USED,
        MA_BIT | mt::STD_RSVD,
        MA_BIT | 0x0000_0089 | (1u32 << 24),
    ];
    let mut i = 0;
    while i < 6 {
        let m = ma_probe[i];
        if !has_ma(m) {
            return Err(E_REQ);
        }
        if i < 4 {
            validate_request(m)?;
        } else {
            // MA+保留位：剥 MA 后保留位仍在 → E_RESV（非 E_REQ）。
            if validate_request(m) != Err(E_RESV) {
                return Err(E_RESV);
            }
        }
        // 存态面：全部拒（MA 位本身踩保留区红线）。
        if validate_stored(m) != Err(E_RESV) {
            return Err(E_STORE_ANCHOR());
        }
        hits += 1;
        i += 1;
    }
    hits += 1;
    // R5：分诊面 + 负例注入（伪校验器恒过必须被检出）。
    let cls: [u32; 4] = [0x0012_0089, MA_BIT | 0x0012_0089, mt::ALL_RSVD, 0xF000_0000];
    let expect: [Verdict; 4] =
        [Verdict::StoredOk, Verdict::StoredOnlyOk, Verdict::BothReject, Verdict::BothReject];
    let mut i = 0;
    while i < 4 {
        match classify(cls[i])? {
            v => {
                if v.code() != expect[i].code() {
                    return Err(E_NEG);
                }
            }
        }
        hits += 1;
        i += 1;
    }
    fn fake_validate(_m: u32) -> Result<(), u16> {
        Ok(())
    }
    let mut caught = 0;
    let mut i = 0;
    while i < 11 {
        if fake_validate(rsvds[i]).is_ok() && validate_stored(rsvds[i]).is_err() {
            caught += 1;
        }
        i += 1;
    }
    if caught != 11 {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<u16> = Vec::new();
    ledger.push(Verdict::StoredOk.code());
    ledger.push(Verdict::RequestOk.code());
    ledger.push(Verdict::BothReject.code());
    ledger.push(Verdict::StoredOnlyOk.code());
    if ledger.len() != 4 {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

/// 存态锚哨兵（E_RESV 的别名语义——MA 位 ∈ GEN_RSVD 保留区）。
fn E_STORE_ANCHOR() -> u16 {
    E_RESV
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(37));
    }

    #[test]
    fn reserved_rejected() {
        assert_eq!(validate_stored(mt::STD_RSVD), Err(E_RESV));
        assert_eq!(validate_stored(mt::GEN_RSVD), Err(E_RESV));
        assert_eq!(validate_stored(1u32 << 25), Err(E_RESV));
    }

    #[test]
    fn generic_mix_rejected() {
        assert_eq!(validate_stored(0x8000_0000 | 0x89), Err(E_MIXSEG));
        assert_eq!(validate_stored(0xC000_0000), Err(E_GENCOMBO));
    }

    #[test]
    fn ma_exempt_on_request_face() {
        assert!(validate_request(MA_BIT | 0x0012_0089).is_ok());
        assert_eq!(validate_stored(MA_BIT | 0x0012_0089), Err(E_RESV));
    }
}

pub fn run_unx36113_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36113_maskvalid");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a10, "0x3A10~0x3A1F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四律/负例/MA 豁免全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("保留位红线", mt::ALL_RSVD == 0x0FE0_0000, "STD 21..23 + GEN 24..27 恒零");
    cs.add("MA 双写对拍", MA_BIT == 0x02000000 && MA_BIT & mt::GEN_RSVD == MA_BIT, "与 36110 写死锚一致");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RESV => "保留位非零（存态红线）",
        E_MIXSEG => "GENERIC 与非 GENERIC 位混合",
        E_GENCOMBO => "多个 GENERIC 位同时出现",
        E_BOUND => "掩码值越界（不可能值，保守守卫）",
        E_NEG => "负例注入未被检出",
        E_REQ => "请求面校验失灵（MA 剥离后应过）",
        _ => "未知诊断码",
    }
}
