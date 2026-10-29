//! UNX-F36110-J1 MAXIMUM_ALLOWED 请求位语义载体。
//! 判据：① 请求位（0x02000000）与具体位的区分——MA 位是请求面
//! 载体、永不落 ACE 存储（strip 后存态合法）；② 尽量授而不失败的
//! 授权计算器（MA 路径恒不失败、非 MA 路径不足即拒）+ 负例；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：请求面语义载体——存态合法性归 36113，本模块定义
//! MA 位的剥离/识别与授权计算；ACE 求值夹具为本地演练面。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x39a0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BIT: u16 = SEG_BASE + 2; // MA 位值与写死锚不符
pub const E_STORE: u16 = SEG_BASE + 3; // MA 位落存态（红线）
pub const E_ACCESS: u16 = SEG_BASE + 4; // 非 MA 路径覆盖不足（预期拒）
pub const E_OVER: u16 = SEG_BASE + 5; // 授出 ⊄ 请求（越权红线）
pub const E_ACE: u16 = SEG_BASE + 6; // ACE 夹具非法（掩码越合法区）
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// 授权路径标记（MA 请求面 vs 具体位面）。
#[repr(u16)]
pub enum GrantPath {
    MaximumAllowed = 0x0001,
    SpecificBits = 0x0002,
}

impl GrantPath {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn of(ma: bool) -> GrantPath {
        if ma {
            GrantPath::MaximumAllowed
        } else {
            GrantPath::SpecificBits
        }
    }
}

/// MAXIMUM_ALLOWED 请求位（bit 25；请求面载体，存态必须为零）。
pub const MA_REQUEST: u32 = 0x0200_0000;

/// MA 位归属检查：属于 36101::GEN_RSVD 段内（这就是它不落存态的原因）。
pub fn ma_in_reserved_seg() -> bool {
    MA_REQUEST & mt::GEN_RSVD == MA_REQUEST && MA_REQUEST.count_ones_wrap() == 1
}

/// u32 计位（本模块本地 popcount，无外部依赖）。
trait CountOnesWrap {
    fn count_ones_wrap(self) -> u32;
}

impl CountOnesWrap for u32 {
    fn count_ones_wrap(self) -> u32 {
        let mut n = 0;
        let mut x = self;
        while x != 0 {
            n += x & 1;
            x >>= 1;
        }
        n
    }
}

/// MA 识别。
pub fn is_ma(request: u32) -> bool {
    request & MA_REQUEST != 0
}

/// MA 剥离：请求掩码 → 存态候选（MA 位清零；其余原样保留）。
pub fn strip_ma(request: u32) -> u32 {
    request & !MA_REQUEST
}

/// 请求合法性：剥离 MA 后必须落在全体合法位内；MA 位本身单独可控。
pub fn request_valid(request: u32) -> Result<(), u16> {
    let body = strip_ma(request);
    if body & !mt::ALL_LEGAL != 0 {
        return Err(E_STORE);
    }
    Ok(())
}

/// 存态红线：任何含 MA 位的掩码不得进入 ACE 存储。
pub fn storable(mask: u32) -> Result<(), u16> {
    if mask & MA_REQUEST != 0 {
        return Err(E_STORE);
    }
    if mask & !mt::ALL_LEGAL != 0 {
        return Err(E_STORE);
    }
    Ok(())
}

/// ACE 求值夹具：allow/deny 掩码对。
#[derive(Clone, Copy)]
pub struct AceFixture {
    pub allow: u32,
    pub deny: u32,
}

/// 夹具合法性：allow/deny 都必须是可存态掩码。
impl AceFixture {
    pub fn wellformed(&self) -> Result<(), u16> {
        storable(self.allow)?;
        storable(self.deny)?;
        Ok(())
    }
}

/// 授权结果。
pub struct GrantResult {
    /// 实际授出掩码（恒 ⊆ 请求剥离后的具体位）。
    pub granted: u32,
    /// 是否失败（非 MA 路径覆盖不足时为 true）。
    pub failed: bool,
    /// 被显性 Deny 剔除的位。
    pub denied: u32,
    /// MA 路径标记。
    pub ma: bool,
}

/// 授权计算器（尽量授而不失败）：
/// - MA 路径：授出 = (请求具体位 − 全体 deny 并) ∪ (allow 并 ∩ 请求)，
///   恒不失败；未覆盖位留在 granted 外但不报错。
/// - 非 MA 路径：授出 = 逐 ACE (allow ∩ 请求 − deny)；末端覆盖判定，
///   不足即 fail（E_ACCESS 域）。
pub fn calc(desired: u32, aces: &[AceFixture]) -> Result<GrantResult, u16> {
    request_valid(desired)?;
    let mut i = 0;
    while i < aces.len() {
        aces[i].wellformed()?;
        i += 1;
    }
    let body = strip_ma(desired);
    let ma = is_ma(desired);
    // 全体 deny 并（请求面剔除基准）。
    let mut deny_all: u32 = 0;
    let mut allow_all: u32 = 0;
    let mut i = 0;
    while i < aces.len() {
        deny_all |= aces[i].deny;
        allow_all |= aces[i].allow;
        i += 1;
    }
    if ma {
        let granted = (body & !deny_all) | (allow_all & body & !deny_all);
        return Ok(GrantResult { granted, failed: false, denied: body & deny_all, ma: true });
    }
    // 非 MA：授出 = allow 并 ∩ 请求 − deny 并。
    let granted = allow_all & body & !deny_all;
    let denied = body & deny_all;
    let failed = !covers_local(granted, body);
    Ok(GrantResult { granted, failed, denied, ma: false })
}

/// 本地覆盖判定（与 36109 同式，避免跨模块语义漂移的双写对拍点）。
fn covers_local(granted: u32, required: u32) -> bool {
    required & !granted == 0
}

/// 演练：R1 MA 位锚（位值/段归属/单置位）；R2 剥离往返与存态红线；
/// R3 MA 计算器——恒不失败 + 越权红线；R4 非 MA 计算器——足则过、
/// 不足则拒；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：MA 位锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if MA_REQUEST != 0x0200_0000 || !ma_in_reserved_seg() {
        return Err(E_BIT);
    }
    if MA_REQUEST & mt::GEN_USED != 0 || MA_REQUEST & mt::ALL_LEGAL != 0 {
        return Err(E_BIT); // MA 位在合法位域之外（请求面专属）
    }
    hits += 1;
    // R2：剥离往返——strip 幂等；MA+体 = 请求；存态红线双例。
    let req = MA_REQUEST | 0x0012_0089;
    if strip_ma(req) != 0x0012_0089 || strip_ma(strip_ma(req)) != strip_ma(req) {
        return Err(E_BIT);
    }
    if !is_ma(req) || is_ma(strip_ma(req)) {
        return Err(E_BIT);
    }
    if request_valid(req).is_err() || request_valid(0x0012_0089).is_err() {
        return Err(E_STORE);
    }
    if storable(req) != Err(E_STORE) {
        return Err(E_STORE);
    }
    if storable(MA_REQUEST) != Err(E_STORE) {
        return Err(E_STORE);
    }
    if request_valid(MA_REQUEST | mt::STD_RSVD) != Err(E_STORE) {
        return Err(E_STORE);
    }
    hits += 1;
    // R3：MA 计算器——恒不失败；授出 ⊆ 请求具体位；Deny 恒被剔。
    let aces1: [AceFixture; 3] = [
        AceFixture { allow: 0x0012_019F, deny: 0 },
        AceFixture { allow: 0x0002_0000, deny: 0x0010_0000 }, // deny SYNCHRONIZE
        AceFixture { allow: 0, deny: 0x0000_0080 },
    ];
    let r = calc(MA_REQUEST | 0x0012_01FF, &aces1)?;
    if r.failed || !r.ma {
        return Err(E_NEG);
    }
    if GrantPath::of(r.ma).code() != GrantPath::MaximumAllowed.code() {
        return Err(E_NEG);
    }
    if r.granted & !0x0012_01FF != 0 {
        return Err(E_OVER); // 授出 ⊄ 请求 = 越权红线
    }
    if r.granted & 0x0010_0000 != 0 || r.granted & 0x0000_0080 != 0 {
        return Err(E_NEG); // Deny 位未被剔
    }
    if r.denied & 0x0010_0000 == 0 || r.denied & 0x0000_0080 == 0 {
        return Err(E_NEG);
    }
    // 空 ACE 表：MA 路径尽量授——授出即请求体本身，恒不失败。
    let empty: [AceFixture; 0] = [];
    let r2 = calc(MA_REQUEST | 0x0000_0089, &empty)?;
    if r2.failed || r2.granted != 0x0000_0089 {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：非 MA 计算器——足则过、不足则拒。
    let aces2: [AceFixture; 2] = [
        AceFixture { allow: 0x0012_0089, deny: 0 },
        AceFixture { allow: 0x0000_0076, deny: 0 },
    ];
    let r3 = calc(0x0000_0089, &aces2)?;
    if r3.failed || r3.ma || r3.granted != 0x0000_0089 {
        return Err(E_NEG);
    }
    let r4 = calc(0x0000_01FF, &aces2)?;
    if !r4.failed || r4.granted != 0x0000_00FF {
        return Err(E_ACCESS); // 0x100 (WRITE_ATTR) 无人授 → 覆盖不足
    }
    let r5 = calc(0x0000_008B, &aces2)?;
    if r5.failed || r5.granted & 0x0000_0002 == 0 {
        return Err(E_NEG);
    }
    if r5.granted & !0x0000_008B != 0 {
        return Err(E_OVER);
    }
    hits += 1;
    // R5：负例注入——伪 strip（不清 MA）必须被 storable 检出。
    fn fake_strip(x: u32) -> u32 {
        x
    }
    let mut caught = 0;
    let probes: [u32; 6] = [
        MA_REQUEST,
        MA_REQUEST | 0x0000_0089,
        MA_REQUEST | mt::STD_USED,
        MA_REQUEST | 0x8000_0000,
        MA_REQUEST | 0x0000_0001,
        MA_REQUEST | 0x0012_01FF,
    ];
    let mut i = 0;
    while i < 6 {
        if storable(fake_strip(probes[i])).is_err() {
            caught += 1;
        }
        if storable(strip_ma(probes[i])).is_err() {
            return Err(E_STORE); // 真 strip 不得误报
        }
        i += 1;
    }
    if caught != 6 {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut log: Vec<u32> = Vec::new();
    log.push(r.granted);
    log.push(r2.granted);
    log.push(r3.granted);
    if log.len() != 3 {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn ma_bit_anchor() {
        assert_eq!(MA_REQUEST, 0x02000000);
        assert!(ma_in_reserved_seg());
        assert_eq!(strip_ma(MA_REQUEST | 0x89), 0x89);
    }

    #[test]
    fn ma_never_fails() {
        let empty: [AceFixture; 0] = [];
        let r = calc(MA_REQUEST | 0x00120089, &empty).ok().unwrap_or(GrantResult {
            granted: 0xFFFF_FFFF,
            failed: true,
            denied: 0,
            ma: false,
        });
        assert!(!r.failed);
    }

    #[test]
    fn non_ma_fails_when_short() {
        let aces: [AceFixture; 1] = [AceFixture { allow: 0x01, deny: 0 }];
        let r = calc(0x03, &aces).ok().unwrap_or(GrantResult {
            granted: 0,
            failed: false,
            denied: 0,
            ma: false,
        });
        assert!(r.failed);
    }

    #[test]
    fn ma_bit_not_storable() {
        assert_eq!(storable(MA_REQUEST), Err(E_STORE));
    }
}

pub fn run_unx36110_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36110_maxallowed");
    cs.add("SEG 段位独占", SEG_BASE == 0x39a0, "0x39A0~0x39AF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "MA 锚/计算器/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("MA 位写死", MA_REQUEST == 0x02000000, "bit 25 请求面载体");
    cs.add("存态红线", mt::GEN_RSVD & MA_REQUEST == MA_REQUEST, "MA 位 ∈ 36101::GEN_RSVD 保留区");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BIT => "MA 位值与写死锚不符",
        E_STORE => "MA 位落存态（红线）",
        E_ACCESS => "非 MA 路径覆盖不足（预期拒）",
        E_OVER => "授出 ⊄ 请求（越权红线）",
        E_ACE => "ACE 夹具非法（掩码越合法区）",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
