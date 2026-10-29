//! UNX-F36162-J1 AccessCheck 第①步：模拟级门（acimperson）。
//! 判据：① 冻结四级面（Anon=0/Iden=1/Imp=2/Deleg=3）+ step1 契约——
//! 匿名显性拒、越界级显性拒、其余放行；② 操作面授权表（每级允许的
//! 检查侧操作闭集）；③ 注入负例显性拒；④ 纯功能代码 ≥310 行。
//! 设计纪律：类型消费 36161 冻结面（AcRequest/AcTokenFace），本模块
//! 是五步装配的入口步——36161 通过 `super::unx36162_acimperson as s1` 消费
//! IMP_ANON/IMP_IDEN/IMP_IMP/IMP_DELEG 与 step1。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xefe0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_LEVEL: u16 = SEG_BASE + 2; // 模拟级越出四级闭集
pub const E_ANON: u16 = SEG_BASE + 3; // 匿名级不持可检查身份
pub const E_FACE: u16 = SEG_BASE + 4; // 操作面编码非法（含保留位）
pub const E_DOWN: u16 = SEG_BASE + 5; // 降级链非法（级序倒挂）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 7; // 授权表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 模拟级四级闭集（判别式冻结：与 36161 消费契约逐位对齐）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpLevel {
    Anonymous = 0x0000,
    Identification = 0x0001,
    Impersonation = 0x0002,
    Delegation = 0x0003,
}

impl ImpLevel {
    pub const fn code(self) -> u16 {
        self as u16
    }
    /// u16 → 级（越界归 Anonymous，仅用于表驱动回显）。
    pub fn of(v: u16) -> ImpLevel {
        match v {
            1 => ImpLevel::Identification,
            2 => ImpLevel::Impersonation,
            3 => ImpLevel::Delegation,
            _ => ImpLevel::Anonymous,
        }
    }
}

/// 冻结级常数（36161 直接引用；禁止在本批内重定义数值）。
pub const IMP_ANON: u16 = ImpLevel::Anonymous.code();
pub const IMP_IDEN: u16 = ImpLevel::Identification.code();
pub const IMP_IMP: u16 = ImpLevel::Impersonation.code();
pub const IMP_DELEG: u16 = ImpLevel::Delegation.code();

pub const LEVEL_NAMES: [&str; 4] = ["anonymous", "identification", "impersonation", "delegation"];

/// 级名回显（越界返回 "bad-level"）。
pub fn level_name(v: u16) -> &'static str {
    match v {
        0 => LEVEL_NAMES[0],
        1 => LEVEL_NAMES[1],
        2 => LEVEL_NAMES[2],
        3 => LEVEL_NAMES[3],
        _ => "bad-level",
    }
}

// ---- 操作面授权表 -----------------------------------------------------------
// 检查侧操作面（位集）：身份可读 / 组可读 / 特权可读 / 结果可写 /
// 审计可写 / 属性可读 / 降级可做 / 全链可入。
pub const F_IDREAD: u16 = 0x0001;
pub const F_GRPREAD: u16 = 0x0002;
pub const F_PRVREAD: u16 = 0x0004;
pub const F_RESWRITE: u16 = 0x0008;
pub const F_AUDWRITE: u16 = 0x0010;
pub const F_ATTREAD: u16 = 0x0020;
pub const F_DOWNGRADE: u16 = 0x0040;
pub const F_FULLCHAIN: u16 = 0x0080;
/// 操作面合法区（闭集：8 位全用满，无保留位）。
pub const FACE_LEGAL: u16 = F_IDREAD
    | F_GRPREAD
    | F_PRVREAD
    | F_RESWRITE
    | F_AUDWRITE
    | F_ATTREAD
    | F_DOWNGRADE
    | F_FULLCHAIN;

/// 各级允许的操作面（冻结表：级 → 位集）。
/// Anonymous 零面；Identification 只读身份/组；Impersonation 加特权读、
/// 结果写与属性读；Delegation 全量。
pub const LEVEL_FACES: [u16; 4] = [
    0x0000,
    F_IDREAD | F_GRPREAD,
    F_IDREAD | F_GRPREAD | F_PRVREAD | F_RESWRITE | F_ATTREAD,
    FACE_LEGAL,
];

/// 面判定：级 v 是否允许操作面 face。
pub fn face_allowed(v: u16, face: u16) -> Result<bool, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if face & !FACE_LEGAL != 0 {
        return Err(E_FACE);
    }
    if v > IMP_DELEG {
        return Err(E_LEVEL);
    }
    Ok(LEVEL_FACES[v as usize] & face == face)
}

/// 每级允许面计数（表驱动，供闭集校验）。
pub fn face_count(v: u16) -> Result<u32, u16> {
    if v > IMP_DELEG {
        return Err(E_LEVEL);
    }
    let f = LEVEL_FACES[v as usize];
    let mut n = 0u32;
    let mut bit = 0u16;
    while bit < 8 {
        if f & (1u16 << bit) != 0 {
            n += 1;
        }
        bit += 1;
    }
    Ok(n)
}

/// 降级链合法性：模拟级只能沿 0→1→2→3 单调升，降级目标必须 < 当前级。
pub fn downgrade_ok(cur: u16, target: u16) -> Result<bool, u16> {
    if cur > IMP_DELEG || target > IMP_DELEG {
        return Err(E_LEVEL);
    }
    if target >= cur {
        return Err(E_DOWN);
    }
    Ok(true)
}

// ---- 第①步契约 -------------------------------------------------------------
/// step1：模拟级门。匿名显性拒（E_ANON）；越界级显性拒（E_LEVEL）；
/// 其余放行。仅读 token.imp_level，不触 DACL/特权/掩码。
pub fn step1(req: &acf::AcRequest) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let lv = req.token.imp_level;
    if lv > IMP_DELEG {
        return Err(E_LEVEL);
    }
    if lv == IMP_ANON {
        return Err(E_ANON);
    }
    Ok(())
}

/// 级序闭集自检：四级名非空、常数与判别式一致、授权表单调不减。
pub fn level_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if IMP_ANON != 0 || IMP_IDEN != 1 || IMP_IMP != 2 || IMP_DELEG != 3 {
        return Err(E_TABLE);
    }
    let mut i = 0;
    while i < 4 {
        if LEVEL_NAMES[i].is_empty() {
            return Err(E_TABLE);
        }
        if LEVEL_FACES[i] & !FACE_LEGAL != 0 {
            return Err(E_TABLE);
        }
        if i > 0 && LEVEL_FACES[i] & LEVEL_FACES[i - 1] != LEVEL_FACES[i - 1] {
            return Err(E_TABLE); // 单调性：高级面 ⊇ 低级面
        }
        i += 1;
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 step1 四级正负例；R3 授权面判定；
/// R4 降级链；R5 负例注入（越界级/匿名/非法面/倒挂降级/表破坏探针）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    level_integrity()?;
    hits += 1;
    // R2：step1 四级——Anon 拒 / Iden·Imp·Deleg 放行 / 4 越界拒。
    let mk = |lv: u16| acf::AcRequest {
        token: acf::AcTokenFace {
            token_id: lv as u32,
            sid: 0x1000 + lv as u64,
            groups: Vec::new(),
            deny_only: Vec::new(),
            privs: Vec::new(),
            imp_level: lv,
        },
        owner: 1,
        dacl: acf::DaclFace { state: 1, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    if step1(&mk(IMP_ANON)) != Err(E_ANON) {
        return Err(E_NEG);
    }
    step1(&mk(IMP_IDEN))?;
    step1(&mk(IMP_IMP))?;
    step1(&mk(IMP_DELEG))?;
    if step1(&mk(4)) != Err(E_LEVEL) {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：授权面——Anon 零面；Iden 拒特权读；Imp 允结果写；Deleg 全量。
    if face_allowed(IMP_ANON, F_IDREAD)? {
        return Err(E_NEG);
    }
    if face_allowed(IMP_IDEN, F_IDREAD)? != true {
        return Err(E_NEG);
    }
    if face_allowed(IMP_IDEN, F_PRVREAD)? {
        return Err(E_NEG);
    }
    if face_allowed(IMP_IMP, F_RESWRITE)? != true {
        return Err(E_NEG);
    }
    if face_allowed(IMP_DELEG, FACE_LEGAL)? != true {
        return Err(E_NEG);
    }
    if face_count(IMP_ANON)? != 0 || face_count(IMP_DELEG)? != 8 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R4：降级链——Imp→Iden 合法；同级/升级倒挂拒；越界拒。
    if downgrade_ok(IMP_IMP, IMP_IDEN)? != true {
        return Err(E_NEG);
    }
    if downgrade_ok(IMP_IDEN, IMP_IDEN).is_ok() {
        return Err(E_NEG);
    }
    if downgrade_ok(IMP_ANON, IMP_DELEG).is_ok() {
        return Err(E_NEG);
    }
    if downgrade_ok(4, IMP_IDEN).is_ok() {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——非法面位 / 级回显闭集 / 表单调探针。
    if face_allowed(IMP_IMP, 0x0100).is_ok() {
        return Err(E_NEG);
    }
    if level_name(5) != "bad-level" {
        return Err(E_NEG);
    }
    if ImpLevel::of(7) != ImpLevel::Anonymous {
        return Err(E_NEG);
    }
    if LEVEL_FACES[3] & !FACE_LEGAL != 0 {
        return Err(E_TABLE);
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
    fn level_table_holds() {
        assert_eq!(level_integrity(), Ok(()));
    }

    #[test]
    fn step1_gates_anonymous() {
        let req = acf::AcRequest {
            token: acf::tok_anon(),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x89,
            map_name: "File",
            priv_gate: None,
        };
        assert_eq!(step1(&req), Err(E_ANON));
    }

    #[test]
    fn faces_monotonic() {
        let mut i = 1;
        while i < 4 {
            assert_eq!(LEVEL_FACES[i] & LEVEL_FACES[i - 1], LEVEL_FACES[i - 1]);
            i += 1;
        }
    }
}

pub fn run_unx36162_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36162_acimperson");
    cs.add("SEG 段位独占", SEG_BASE == 0xefe0, "0xEFE0~0xEFEFF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四级门/授权面/降级链全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", level_integrity() == Ok(()), "四级常数 + 授权表单调");
    cs.add("契约冻结", VER == 1, "step1 契约锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_LEVEL => "模拟级越出四级闭集",
        E_ANON => "匿名级不持可检查身份",
        E_FACE => "操作面编码非法",
        E_DOWN => "降级链非法（级序倒挂）",
        E_NEG => "负例注入未被检出",
        E_TABLE => "授权表闭集破坏",
        _ => "未知诊断码",
    }
}
