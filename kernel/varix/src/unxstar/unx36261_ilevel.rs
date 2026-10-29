//! UNX-F36261-J1 完整性级别（IL）五档与令牌 IL 字段。
//! 判据：① 五档（0/4096/8192/12288/16384）赋值读取 100 例对平；
//! ② 越档注入 10 例拒绝；③ 纯功能 ≥300 行。
//! 设计纪律：IL 是令牌与对象的双重属性（令牌 IL = 主体的信任级，
//! 对象 IL = 客体的保护级），五档是 Windows SID S-1-16-{0,4096,
//! 8192,12288,16384} 的同构投影。档间是**有序**的（低 < 中 < 高），
/// 越档（不在五档闭集内的值）一律拒收——IL 字段不允许任意值，
/// 这是与普通 u16 字段的本质区别。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA4B0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ILVL: u16 = SEG_BASE + 2; // 越档值（不在五档闭集）
pub const E_ORDER: u16 = SEG_BASE + 3; // 有序性破
pub const E_ASSIGN: u16 = SEG_BASE + 4; // 赋值读取对平破
pub const E_FROZEN: u16 = SEG_BASE + 5; // 档表冻结破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// IL 五档（判别式值与 Windows RID 同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Il {
    /// 不可信（S-1-16-0）。
    Untrusted = 0,
    /// 低（S-1-16-4096）。
    Low = 4096,
    /// 中（S-1-16-8192）。
    Medium = 8192,
    /// 高（S-1-16-12288）。
    High = 12288,
    /// 系统（S-1-16-16384）。
    System = 16384,
}

pub const IL_N: usize = 5;

/// 档表（冻结——值域与序）。
pub const IL_TABLE: [Il; IL_N] = [Il::Untrusted, Il::Low, Il::Medium, Il::High, Il::System];

/// 裸值 → IL（五档闭集外 = E_ILVL——判据 ② 落点）。
pub fn il_from_raw(raw: u32) -> Result<Il, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    match raw {
        0 => Ok(Il::Untrusted),
        4096 => Ok(Il::Low),
        8192 => Ok(Il::Medium),
        12288 => Ok(Il::High),
        16384 => Ok(Il::System),
        _ => Err(E_ILVL),
    }
}

/// IL → 裸值（往返面）。
pub fn raw_of(il: Il) -> u32 {
    il as u32
}

/// 档表有序性（五档严格递增——判据内部不变量）。
pub fn table_ordered() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if IL_TABLE.len() != IL_N {
        return Err(E_FROZEN);
    }
    let mut i = 0;
    while i < IL_N - 1 {
        if raw_of(IL_TABLE[i]) >= raw_of(IL_TABLE[i + 1]) {
            return Err(E_ORDER);
        }
        i += 1;
    }
    Ok(())
}

/// 持 IL 的令牌（字段面：IL 是令牌的冻结属性之一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IlToken {
    pub sid_fp: u32,
    pub il: Il,
}

impl IlToken {
    /// 赋值读取对平（构造即冻结——读回恒等于写入值）。
    pub fn roundtrip(&self) -> Result<u32, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 令牌 IL 再经闭集校验（构造面已保证，读取面双查）。
        il_from_raw(raw_of(self.il))?;
        Ok(raw_of(self.il))
    }
}

/// 赋值读取 100 例（判据 ① 落点：五档轮换 × 令牌，写入值 == 读回
/// 值逐例对平）。
pub fn assign_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    table_ordered()?;
    let mut i = 0u32;
    while i < 100 {
        // 五档轮换（i%5 → 档表索引）。
        let il = IL_TABLE[(i % IL_N as u32) as usize];
        let t = IlToken { sid_fp: i + 1, il };
        let raw = t.roundtrip()?;
        if raw != raw_of(il) {
            return Err(E_ASSIGN);
        }
        // 往返闭包：raw → il → raw 恒等。
        let il2 = il_from_raw(raw)?;
        if raw_of(il2) != raw {
            return Err(E_ASSIGN);
        }
        i += 1;
    }
    Ok(100)
}

/// 越档注入 10 例拒绝（判据 ② 落点：五档间的所有非法值形态——
/// 相邻档间值/超顶档/近零非零/位翻转/随机面）。
pub fn inject_ten_rejected() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let bad_values: [u32; 10] = [
        1,        // 近零非零
        2048,     // 档间值（0 与 4096 之间）
        4095,     // 顶到 Low 前 1
        6144,     // Low 与 Medium 之间
        10000,    // Medium 与 High 之间
        14000,    // High 与 System 之间
        16383,    // System 前 1
        16385,    // 超顶档 1
        32768,    // 超顶档远
        0x7FFF_FFFF, // 极大值
    ];
    let mut rejected = 0u32;
    let mut a = 0;
    while a < 10 {
        if il_from_raw(bad_values[a]).is_ok() {
            return Err(E_ILVL); // 越档放行 = 闭集破
        }
        rejected += 1;
        a += 1;
    }
    Ok(rejected)
}

/// 比较面（档序消费：低写高拒语义的前置——档序可比较）。
pub fn il_rank(il: Il) -> usize {
    let mut i = 0;
    while i < IL_N {
        if IL_TABLE[i] == il {
            return i;
        }
        i += 1;
    }
    IL_N // 不可达（闭集保证）
}

/// 双令牌 IL 比较（No-Write-Up 前置：主体 IL 与客体 IL 的序比较
/// ——36262 消费的比较真源：rank 可比、同档相等、跨档有序）。
pub fn il_compare(a: Il, b: Il) -> core::cmp::Ordering {
    il_rank(a).cmp(&il_rank(b))
}

/// 档序全对直证（5×5 = 25 对比较：对角相等、严格下三角小、上三角
/// 大——序关系的完备面）。
pub fn order_matrix() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    table_ordered()?;
    use core::cmp::Ordering::{Equal, Greater, Less};
    let mut n = 0u32;
    let mut i = 0;
    while i < IL_N {
        let mut k = 0;
        while k < IL_N {
            let a = IL_TABLE[i];
            let b = IL_TABLE[k];
            let ord = il_compare(a, b);
            let expect = if i == k {
                Equal
            } else if i < k {
                Less
            } else {
                Greater
            };
            if ord != expect {
                return Err(E_ORDER);
            }
            n += 1;
            k += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 令牌 IL 表（五档各一令牌——IL 面的注册表真源：消费面按 sid
/// 查 IL，禁散落硬编码）。
pub const TOKEN_ILS: [(u32, Il); IL_N] = [
    (0x0101, Il::Untrusted),
    (0x0102, Il::Low),
    (0x0103, Il::Medium),
    (0x0104, Il::High),
    (0x0105, Il::System),
];

/// 令牌 IL 查询（sid → IL——查无此令牌 = E_ILVL 显性拒）。
pub fn il_of(sid_fp: u32) -> Result<Il, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut i = 0;
    while i < TOKEN_ILS.len() {
        if TOKEN_ILS[i].0 == sid_fp {
            return Ok(TOKEN_ILS[i].1);
        }
        i += 1;
    }
    Err(E_ILVL)
}

/// 注册表健康（五令牌 sid 互异 + IL 全在档表——表漂移即红）。
pub fn token_ils_healthy() -> Result<(), u16> {
    if TOKEN_ILS.len() != IL_N {
        return Err(E_FROZEN);
    }
    let mut i = 0;
    while i < IL_N {
        // sid 互异。
        let mut k = i + 1;
        while k < IL_N {
            if TOKEN_ILS[i].0 == TOKEN_ILS[k].0 {
                return Err(E_FROZEN);
            }
            k += 1;
        }
        // IL 往返合法。
        il_from_raw(raw_of(TOKEN_ILS[i].1))?;
        i += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    table_ordered()?;
    let n = assign_hundred()?;
    let r = inject_ten_rejected()?;
    // 有序性消费面：Untrusted < Low < Medium < High < System。
    if !(il_rank(Il::Untrusted) < il_rank(Il::Low)
        && il_rank(Il::Low) < il_rank(Il::Medium)
        && il_rank(Il::Medium) < il_rank(Il::High)
        && il_rank(Il::High) < il_rank(Il::System))
    {
        return Err(E_ORDER);
    }
    // 25 对序关系全对 + 双令牌比较面 + 注册表健康。
    order_matrix()?;
    if il_compare(Il::Low, Il::System) != core::cmp::Ordering::Less {
        return Err(E_ORDER);
    }
    token_ils_healthy()?;
    // 查无此令牌显性拒。
    if il_of(0x9999).is_ok() {
        return Err(E_ILVL);
    }
    if n != 100 || r != 10 {
        return Err(E_ASSIGN);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(r))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ILVL => "越档值（不在五档闭集）",
        E_ORDER => "有序性破",
        E_ASSIGN => "赋值读取对平破",
        E_FROZEN => "档表冻结破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36261_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36261_ilevel");
    cs.add("SEG 段位独占", SEG_BASE == 0xA4B0, "0xA4B0~0xA4BF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五档 100 例对平/越档 10 例全拒/档序严格递增"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("五档锚", IL_N == 5 && IL_TABLE[1] as u32 == 4096 && IL_TABLE[4] as u32 == 16384, "0/4096/8192/12288/16384 冻结档表");
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
    fn test_roundtrip() {
        let t = IlToken { sid_fp: 7, il: Il::Medium };
        assert_eq!(t.roundtrip().unwrap_or(0), 8192);
        assert_eq!(il_from_raw(16384).unwrap_or(Il::Untrusted), Il::System);
        assert!(il_from_raw(8193).is_err());
    }

    #[test]
    fn test_order_and_inject() {
        assert_eq!(assign_hundred().ok(), Some(100));
        assert_eq!(inject_ten_rejected().ok(), Some(10));
    }
}
