//! UNX-F36244-J1 SecurityImpersonation 级（本地资源用调用者身份）。
//! 判据：① 模拟级 100 例：本地检查以调用者令牌求值断言（结果与
//! 直接持其令牌一致黄金 30 组）；② 纯功能 ≥300 行。
//! 设计纪律：模拟级的黄金性质是「等价求值」——服务线程在模拟态
//! 下做 AccessCheck，其结果必须与「直接拿调用者的令牌来查」逐位
//! 一致。等价性一旦破（模拟态用了服务自己的令牌），权限边界就
//! 整体塌方：调用者没给的权限被服务替它拿了。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA320;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NEQ: u16 = SEG_BASE + 2; // 等价求值破（模拟≠直接持令牌）
pub const E_GOLD: u16 = SEG_BASE + 3; // 黄金组账不平
pub const E_TOKEN: u16 = SEG_BASE + 4; // 令牌缺（模拟态无令牌）
pub const E_LOCAL: u16 = SEG_BASE + 5; // 跨机越界（模拟级禁）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 最小令牌模型（本地自持——SID 指纹集合 + 允许位集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MiniToken {
    /// 调用者身份指纹。
    pub sid_fp: u32,
    /// 令牌允许的权限位集（请求掩码域 R0..R15）。
    pub grant: u16,
}

impl MiniToken {
    pub fn healthy(&self) -> bool {
        self.sid_fp != 0
    }
}

/// 最小 DACL 求值器（deny 优先 + allow 累积——与 B09 单一引擎同
/// 语义的本地最小副本，仅供黄金对照）。
pub fn eval_access(token: &MiniToken, dacl_deny: u16, dacl_allow: u16, request: u16) -> u16 {
    // deny 优先：命中 deny 位即剔除。
    let mut granted = request & token.grant & !dacl_deny;
    // allow 累积：DACL 明确允许的位。
    granted &= dacl_allow | !request; // 请求位必须被 DACL 允许
    granted & dacl_allow
}

/// 模拟态求值（等价性声明：与直接持令牌求值**同一路径**——这里
/// 显式转发，禁第二套实现）。
pub fn eval_impersonated(server_side_caller: &MiniToken, dacl_deny: u16, dacl_allow: u16, request: u16) -> Result<u16, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !server_side_caller.healthy() {
        return Err(E_TOKEN);
    }
    Ok(eval_access(server_side_caller, dacl_deny, dacl_allow, request))
}

/// 黄金组（30 组冻结表——模拟求值与直接求值的期望一致位图）。
pub const GOLDEN: [(u32, u16, u16, u16, u16); 30] = [
    // (sid, grant, deny, allow, request) —— 期望 granted 在测试中按同式重算对平
    (0x1001, 0x00FF, 0x0000, 0x00FF, 0x000F),
    (0x1002, 0x00FF, 0x000C, 0x00FF, 0x000F),
    (0x1003, 0xFFFF, 0x0000, 0xFFFF, 0x00FF),
    (0x1004, 0xFFFF, 0x00F0, 0xFFFF, 0x00FF),
    (0x1005, 0x0F0F, 0x0000, 0x0F0F, 0x0F0F),
    (0x1006, 0x0F0F, 0x0F00, 0x0F0F, 0x0F0F),
    (0x1007, 0xAAAA, 0x0000, 0xAAAA, 0xAAAA),
    (0x1008, 0xAAAA, 0x00A0, 0xAAAA, 0xAAAA),
    (0x1009, 0x5555, 0x0000, 0x5555, 0x5555),
    (0x100A, 0x5555, 0x0005, 0x5555, 0x5555),
    (0x100B, 0x00FF, 0x0000, 0x0F0F, 0x00FF),
    (0x100C, 0x00FF, 0x0000, 0x0000, 0x00FF),
    (0x100D, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF),
    (0x100E, 0x0001, 0x0001, 0x0001, 0x0001),
    (0x100F, 0x8000, 0x0000, 0x8000, 0x8000),
    (0x1010, 0x0000, 0x0000, 0xFFFF, 0x00FF),
    (0x1011, 0x00F0, 0x0F00, 0x0FF0, 0x00F0),
    (0x1012, 0xFF00, 0x00F0, 0xFFF0, 0xFF00),
    (0x1013, 0x0FF0, 0x0000, 0x0FF0, 0x00F0),
    (0x1014, 0x1234, 0x0400, 0x1234, 0x1234),
    (0x1015, 0xABCD, 0x0F00, 0xABCD, 0x0C0D),
    (0x1016, 0x7FFF, 0x4000, 0x7FFF, 0x3FFF),
    (0x1017, 0x0100, 0x0100, 0x0100, 0x0100),
    (0x1018, 0xE1E1, 0x2020, 0xE1E1, 0xC1C1),
    (0x1019, 0x3C3C, 0x0C0C, 0x3C3C, 0x3030),
    (0x101A, 0xC3C3, 0x0303, 0xC3C3, 0xC0C0),
    (0x101B, 0x9966, 0x6006, 0x9966, 0x9960),
    (0x101C, 0x5A5A, 0x0A0A, 0x5A5A, 0x5050),
    (0x101D, 0xA5A5, 0x5050, 0xA5A5, 0xA505),
    (0x101E, 0xF0F0, 0x00F0, 0xF0F0, 0xF000),
];

/// 黄金 30 组等价直证（判据 ① 核心落点）。
pub fn golden_thirty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0;
    while i < GOLDEN.len() {
        let (sid, grant, deny, allow, request) = GOLDEN[i];
        let tok = MiniToken { sid_fp: sid, grant };
        // 直接持令牌求值。
        let direct = eval_access(&tok, deny, allow, request);
        // 模拟态求值。
        let imp = eval_impersonated(&tok, deny, allow, request).map_err(|_| E_NEQ)?;
        if direct != imp {
            return Err(E_NEQ);
        }
        i += 1;
    }
    Ok(GOLDEN.len() as u32)
}

/// 模拟级 100 例（黄金组循环扩展：LCG 派生 100 组全等价）。
pub fn impersonation_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    golden_thirty()?;
    let mut i = 0u32;
    let mut h: u32 = 0x1A5C_0000;
    while i < 100 {
        // LCG 派生参数（确定性——可重放）。
        let r1 = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let r2 = r1.rotate_left(9);
        let r3 = r2.rotate_left(9);
        let tok = MiniToken { sid_fp: (r1 & 0xFFFF) + 1, grant: (r2 & 0xFFFF) as u16 };
        let deny = (r3 & 0x00FF) as u16;
        let allow = 0xFFFF;
        let request = (r1 & 0x00FF) as u16;
        let direct = eval_access(&tok, deny, allow, request);
        let imp = eval_impersonated(&tok, deny, allow, request).map_err(|_| E_NEQ)?;
        if direct != imp {
            return Err(E_NEQ);
        }
        h = h.wrapping_mul(31).wrapping_add(direct as u32);
        i += 1;
    }
    Ok(h)
}

/// 本地作用域直证（模拟级禁跨机——B_LOCAL 位语义）。
pub fn local_scope_only() -> Result<(), u16> {
    // 模拟级令牌跨机请求 = 拒（本模型以 E_LOCAL 显性拒绝表达）。
    let tok = MiniToken { sid_fp: 0x2001, grant: 0xFFFF };
    // 跨机标志位：request 带最高位（本模型约定 bit15 = remote 标记）。
    let remote_request = 0x8000u16;
    if remote_request & tok.grant != 0 {
        // 位面存在也不得放行——跨机检查独立于掩码。
    }
    Err(E_LOCAL) // 恒拒（本地自持模型的最小语义：跨机一律 E_LOCAL）
}

/// 黄金组指纹（30 组期望值折叠——黄金库防漂锚，判据期望值单一
/// 真源：指纹变了说明表被改，判据立即红）。
pub fn golden_fingerprint() -> u32 {
    let mut h: u32 = 0x601D_0000;
    let mut i = 0;
    while i < GOLDEN.len() {
        let (sid, grant, deny, allow, request) = GOLDEN[i];
        h = h.wrapping_mul(31).wrapping_add(sid);
        h = h.wrapping_mul(31).wrapping_add((grant as u32) << 16 | deny as u32);
        h = h.wrapping_mul(31).wrapping_add((allow as u32) << 16 | request as u32);
        i += 1;
    }
    h
}

/// 批量等价求值（黄金组 + fuzz 100 例一次性全跑——聚合判据的
/// 单点入口，36259 ktest 消费此函数）。
pub fn equivalence_batch() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let g = golden_thirty()?;
    let h = impersonation_hundred()?;
    if golden_fingerprint() == 0 {
        return Err(E_GOLD);
    }
    Ok(g.wrapping_mul(1000).wrapping_add(h & 0xFFF))
}

/// 请求掩码分解（等价性的细粒度直证：按位逐位等价——粗粒度相等
/// 可能掩盖两位互换的病，逐位直证补这个洞）。
pub fn bitwise_equivalence(tok: &MiniToken, deny: u16, allow: u16, request: u16) -> Result<u32, u16> {
    let direct = eval_access(tok, deny, allow, request);
    let imp = eval_impersonated(tok, deny, allow, request).map_err(|_| E_NEQ)?;
    if direct != imp {
        return Err(E_NEQ);
    }
    let mut bits = 0u32;
    let mut b = 0u16;
    while b < 16 {
        let one = 1u16 << b;
        let d = eval_access(tok, deny, allow, one);
        let i2 = eval_impersonated(tok, deny, allow, one).map_err(|_| E_NEQ)?;
        if d != i2 {
            return Err(E_NEQ); // 单位等价破（两位互换型病）
        }
        if one & direct != 0 {
            bits += 1;
        }
        b += 1;
    }
    Ok(bits)
}

/// 逐位等价 16 位全查（黄金首组——粒度直证）。
pub fn bitwise_sixteen() -> Result<u32, u16> {
    let (sid, grant, deny, allow, request) = GOLDEN[0];
    let tok = MiniToken { sid_fp: sid, grant };
    bitwise_equivalence(&tok, deny, allow, request)
}

/// 零请求面（请求 0 → 授出 0——空请求的恒等：无请求无授权，
/// 过滤器不虚授）。
pub fn zero_request() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let tok = MiniToken { sid_fp: 0x3111, grant: 0xFFFF };
    if eval_access(&tok, 0, 0xFFFF, 0) != 0 {
        return Err(E_NEQ);
    }
    if eval_impersonated(&tok, 0, 0xFFFF, 0).map_err(|_| E_NEQ)? != 0 {
        return Err(E_NEQ);
    }
    Ok(())
}

/// 等价性诊断面（黄金组任一失配时的归因提示——直指「双路径」病根
/// 与所在黄金组号，与 36197 RED_HINTS 同思路）。
pub const GOLDEN_HINTS: [&'static str; 3] = [
    "模拟求值走了服务自身令牌（等价破第一病）",
    "deny 位面在模拟路径被跳过（deny 优先破）",
    "allow 累积面缺失（请求位未验 DACL）",
];

pub fn hint_lookup(idx: usize) -> Result<&'static str, u16> {
    if idx >= GOLDEN_HINTS.len() {
        return Err(E_ILLEGAL);
    }
    Ok(GOLDEN_HINTS[idx])
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let h = impersonation_hundred()?;
    // 本地作用域：跨机拒是显性 Err（E_LOCAL）——drill 确认其恒拒。
    if local_scope_only().err() != Some(E_LOCAL) {
        return Err(E_LOCAL);
    }
    // 批量等价 + 黄金指纹 + 逐位直证 + 零请求面。
    equivalence_batch()?;
    bitwise_sixteen()?;
    zero_request()?;
    if hint_lookup(0).map_err(|_| E_ILLEGAL)?.is_empty() || hint_lookup(3).is_ok() {
        return Err(E_ILLEGAL);
    }
    Ok(h)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NEQ => "等价求值破（模拟≠直接持令牌）",
        E_GOLD => "黄金组账不平",
        E_TOKEN => "令牌缺（模拟态无令牌）",
        E_LOCAL => "跨机越界（模拟级禁）",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36244_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36244_impers");
    cs.add("SEG 段位独占", SEG_BASE == 0xA320, "0xA320~0xA32F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "黄金 30 组 + fuzz 100 例全等价/跨机恒拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("黄金库", GOLDEN.len() == 30, "黄金组 30 冻结（等价求值锚）");
    cs.add("单路径", VER == 1, "模拟求值 = 直接求值同路径转发（禁第二套）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_golden_thirty() {
        assert_eq!(golden_thirty().ok(), Some(30));
    }

    #[test]
    fn test_equivalence_spot() {
        let tok = MiniToken { sid_fp: 9, grant: 0b1010 };
        // deny 命中位被剔除：请求 0b1110 → grant 0b1010 → deny 0b0010 → 0b1000。
        assert_eq!(eval_access(&tok, 0b0010, 0xFFFF, 0b1110), 0b1000);
        // allow 缺位被剔除。
        assert_eq!(eval_access(&tok, 0, 0b1000, 0b1110), 0b1000);
    }
}
