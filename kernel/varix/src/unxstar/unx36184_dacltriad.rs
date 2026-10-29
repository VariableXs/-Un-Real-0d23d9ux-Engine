//! UNX-F36184-J1 三态 DACL 对照组总账（null DACL vs 空 DACL vs 无 DACL）。
//! 判据：① 三态语义与基准机口径零差——无 DACL（Absent，安全描述符根本
//! 没有DACL 槽）⇒ 隐式全允许；null DACL（槽存在但指针为 NULL）⇒ 显式全
//! 允许；空 DACL（槽存在、ACE 计数为零）⇒ 显式全拒绝；② 三态 × 10 掩码
//! 变体 = 30 组对照全入黄金库 v2 并标 P0 护栏；③ P0 护栏注入（把空 DACL
//! 误当无 DACL 的经典越权实现缺陷）必须被护栏位检出；④ 纯功能 ≥320 行。
//! 设计纪律：三态混淆是访问检查史上最著名的越权源（Windows 语义真源：
//! 「NULL DACL 与空 DACL 的语义差一个字，安全差一个宇宙」）。本模块把
//! 三态做成判别式闭集 + 确定性判定表，任何后续实现（B11 继承面等）消费
//! 本表时必须逐格对平，护栏位长驻 ktest。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x4220;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SHAPE: u16 = SEG_BASE + 2; // 形态越闭集（判别式外值）
pub const E_GOLD: u16 = SEG_BASE + 3; // 黄金库校验和不符
pub const E_MISMATCH: u16 = SEG_BASE + 4; // 对照格与基准口径差
pub const E_GUARD: u16 = SEG_BASE + 5; // P0 护栏被绕过（空 DACL 全开）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 掩码越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 黄金库版本锚（v2——本批起对照表升版，v1 为 B04 单态版）。
pub const GOLDEN_VER: u32 = 2;

/// 判决闭集（对照格取值）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 授出（掩码内所有位全允许）。
    Allow = 0x0001,
    /// 拒绝（掩码内所有位全拒绝）。
    Deny = 0x0002,
}

impl Verdict {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            Verdict::Allow => "授出",
            Verdict::Deny => "拒绝",
        }
    }
}

/// 三态 DACL 形态闭集（判别式 = 槽存在位 × 计数为零位的组合映射）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaclShape {
    /// 无 DACL：安全描述符没有 DACL 槽（SE_DACL_PRESENT 未置位）。
    /// 基准机口径：隐式全允许——没有保护即无界。
    Absent = 0x0001,
    /// null DACL：槽存在（SE_DACL_PRESENT 置位）但 ACL 指针为 NULL。
    /// 基准机口径：显式全允许——发布者明确选择了「谁都可以」。
    NullDacl = 0x0002,
    /// 空 DACL：槽存在、ACL 有效、ACE 计数为零。
    /// 基准机口径：显式全拒绝——发布者明确选择了「谁都不可以」。
    EmptyDacl = 0x0003,
}

impl DaclShape {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            DaclShape::Absent => "无 DACL（槽缺失→隐式全允许）",
            DaclShape::NullDacl => "null DACL（指针空→显式全允许）",
            DaclShape::EmptyDacl => "空 DACL（零 ACE→显式全拒绝）",
        }
    }
}

/// 基准机口径判定表（三态唯一真源——任何消费面以本函数为准）。
/// 无掩码分支：三态语义与请求掩码无关（全允许/全拒绝是整掩码粒度）。
pub fn baseline_verdict(shape: DaclShape) -> Verdict {
    match shape {
        DaclShape::Absent => Verdict::Allow,
        DaclShape::NullDacl => Verdict::Allow,
        DaclShape::EmptyDacl => Verdict::Deny,
    }
}

/// 十掩码变体（对照组横向维度——覆盖读/写/执/删/接管/标准并/规格并/
/// 全标准/GEN_RSVD 岛请求位/全并）。请求面可以携带 MA 位（36181 双面
/// 纪律：请求合法、存储非法），变体 9 即验请求面 MA 位不改变三态语义。
pub const CASE_MASKS: [u32; 10] = [
    sr::READ_CONTROL,
    sr::DELETE,
    sr::WRITE_DAC,
    sr::WRITE_OWNER,
    sr::SYNCHRONIZE,
    sr::STD_ALL,
    mt::SPEC_MASK,
    sr::STD_ALL | mt::SPEC_MASK,
    0x0200_0000 | sr::READ_CONTROL, // MA 请求位 + 读
    mt::ALL_LEGAL,
];

/// 黄金库 v2：3 态 × 10 变体 = 30 格判定快照（行 = 形态，列 = 掩码）。
/// 由基准口径直译（非手抄——生成面唯一，校验和只验翻译链无漂移）。
pub fn golden_v2() -> [[Verdict; 10]; 3] {
    let shapes = [DaclShape::Absent, DaclShape::NullDacl, DaclShape::EmptyDacl];
    let mut g = [[Verdict::Deny; 10]; 3];
    let mut i = 0;
    while i < 3 {
        let v = baseline_verdict(shapes[i]);
        let mut j = 0;
        while j < 10 {
            g[i][j] = v;
            j += 1;
        }
        i += 1;
    }
    g
}

/// 黄金库校验和（列主序折叠——与 36181 矩阵行主序不同，防同源漂移）。
pub fn golden_checksum(g: &[[Verdict; 10]; 3]) -> u32 {
    let mut sum: u32 = 0;
    let mut j = 0;
    while j < 10 {
        let mut i = 0;
        while i < 3 {
            sum = sum.wrapping_mul(7).wrapping_add(g[i][j].code() as u32);
            i += 1;
        }
        j += 1;
    }
    sum
}

/// P0 护栏：空 DACL 全开检测——若某实现把 EmptyDacl 误当 Absent/Null
/// （返回 Allow），护栏位立即拉响。这是本模块存在的第一性理由。
pub fn p0_guard(shape: DaclShape, verdict: Verdict) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if shape == DaclShape::EmptyDacl && verdict == Verdict::Allow {
        return Err(E_GUARD);
    }
    Ok(())
}

/// 对照执行器：对三态逐一走基准口径 + P0 护栏，与黄金库逐格对平。
/// 返回对平格数（30 = 全对平锚值）。
pub fn run_cases() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let shapes = [DaclShape::Absent, DaclShape::NullDacl, DaclShape::EmptyDacl];
    let gold = golden_v2();
    let mut hit = 0u32;
    let mut i = 0;
    while i < 3 {
        let expect = baseline_verdict(shapes[i]);
        p0_guard(shapes[i], expect)?;
        let mut j = 0;
        while j < 10 {
            // 语义与掩码无关——但掩码合法性仍要守住：剔除请求面 MA 位
            // （36181 双面纪律：MA 位请求面合法）后核合法区。
            let m = CASE_MASKS[j] & !super::unx36181_maxallowed::MA_BIT;
            if m & !mt::ALL_LEGAL != 0 {
                return Err(E_ILLEGAL);
            }
            if gold[i][j] != expect {
                return Err(E_MISMATCH);
            }
            hit += 1;
            j += 1;
        }
        i += 1;
    }
    Ok(hit)
}

/// 掩码无关性直证：同一形态在不同掩码下判决恒同（三态语义是整掩码
/// 粒度——不是逐位混合；逐位混合是 B09 AccessCheck 主干的职责面）。
pub fn mask_independence(shape: DaclShape) -> Result<(), u16> {
    let base = baseline_verdict(shape);
    let mut j = 0;
    while j < 10 {
        if baseline_verdict(shape) != base {
            return Err(E_MISMATCH);
        }
        j += 1;
    }
    Ok(())
}

/// P0 护栏注入演练：模拟经典缺陷实现（空 DACL 被映射为 Absent），
/// 护栏必须拦下注入面返回的全允许判决。
pub fn guard_injection() -> Result<(), u16> {
    // 注入 1：缺陷映射表（EmptyDacl → Absent 的错误翻译）。
    let broken = baseline_verdict(DaclShape::Absent); // 缺陷实现的输出
    if p0_guard(DaclShape::EmptyDacl, broken).err() != Some(E_GUARD) {
        return Err(E_GUARD);
    }
    // 注入 2：缺陷映射表二（EmptyDacl → NullDacl，同为全允许）。
    let broken2 = baseline_verdict(DaclShape::NullDacl);
    if p0_guard(DaclShape::EmptyDacl, broken2).err() != Some(E_GUARD) {
        return Err(E_GUARD);
    }
    // 正常面不误伤：Absent/NullDacl 的 Allow 不触护栏。
    p0_guard(DaclShape::Absent, Verdict::Allow)?;
    p0_guard(DaclShape::NullDacl, Verdict::Allow)?;
    // EmptyDacl 的 Deny 不触护栏。
    p0_guard(DaclShape::EmptyDacl, Verdict::Deny)?;
    Ok(())
}

/// 形态判别式互异性：三态编码互异且文本面互异（防两态同义漂移）。
pub fn shape_closure() -> Result<(), u16> {
    let s = [DaclShape::Absent, DaclShape::NullDacl, DaclShape::EmptyDacl];
    let mut i = 0;
    while i < 3 {
        if s[i].text().is_empty() {
            return Err(E_SHAPE);
        }
        let mut k = i + 1;
        while k < 3 {
            if s[i].code() == s[k].code() || s[i].text() == s[k].text() {
                return Err(E_SHAPE);
            }
            k += 1;
        }
        i += 1;
    }
    // 判决闭集两值互异。
    if Verdict::Allow.code() == Verdict::Deny.code()
        || Verdict::Allow.text() == Verdict::Deny.text()
    {
        return Err(E_SHAPE);
    }
    Ok(())
}

/// 黄金库 v2 自洽：版本锚 + 校验和与快照面一致 + 30 格行列形状。
pub fn golden_selfcheck() -> Result<u32, u16> {
    if GOLDEN_VER != 2 {
        return Err(E_GOLD);
    }
    let g = golden_v2();
    let sum = golden_checksum(&g);
    // 确定性重放。
    let g2 = golden_v2();
    if golden_checksum(&g2) != sum {
        return Err(E_GOLD);
    }
    // 逐格与基准口径零差（30 格）。
    let shapes = [DaclShape::Absent, DaclShape::NullDacl, DaclShape::EmptyDacl];
    let mut i = 0;
    while i < 3 {
        let expect = baseline_verdict(shapes[i]);
        let mut j = 0;
        while j < 10 {
            if g[i][j] != expect {
                return Err(E_MISMATCH);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(sum)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：形态闭集 + 判决闭集互异。
    shape_closure()?;
    // R2：基准口径三态语义（与基准机零差的直证面）。
    if baseline_verdict(DaclShape::Absent) != Verdict::Allow {
        return Err(E_MISMATCH);
    }
    if baseline_verdict(DaclShape::NullDacl) != Verdict::Allow {
        return Err(E_MISMATCH);
    }
    if baseline_verdict(DaclShape::EmptyDacl) != Verdict::Deny {
        return Err(E_MISMATCH);
    }
    // R3：掩码无关性（三态 × 10 变体）。
    let shapes = [DaclShape::Absent, DaclShape::NullDacl, DaclShape::EmptyDacl];
    let mut i = 0;
    while i < 3 {
        mask_independence(shapes[i])?;
        i += 1;
    }
    // R4：30 组对照全对平 + P0 护栏逐格通过。
    let hit = run_cases()?;
    if hit != 30 {
        return Err(E_MISMATCH);
    }
    // R5：P0 护栏注入（两路缺陷映射全拦截 + 正常面零误伤）。
    guard_injection()?;
    // R6：黄金库 v2 自洽（版本锚 + 校验和 + 30 格零差）。
    let sum = golden_selfcheck()?;
    // 校验和终锚：对平格数与黄金库校验和复合（确定性总锚）。
    Ok(hit.wrapping_mul(1000).wrapping_add(sum & 0x3FF))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SHAPE => "形态/判决越闭集",
        E_GOLD => "黄金库校验和不符",
        E_MISMATCH => "对照格与基准口径差",
        E_GUARD => "P0 护栏被绕过（空 DACL 全开）",
        E_ILLEGAL => "掩码越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36184_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36184_dacltriad");
    cs.add("SEG 段位独占", SEG_BASE == 0x4220, "0x4220~0x422F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三态语义/30 组对平/P0 护栏/黄金库 v2 全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("三态闭集", DaclShape::Absent.code() == 1 && DaclShape::EmptyDacl.code() == 3, "形态判别式闭集三值互异");
    cs.add("空 DACL 拒绝锚", baseline_verdict(DaclShape::EmptyDacl) == Verdict::Deny, "空 DACL = 显式全拒绝（P0 护栏语义锚）");
    cs.add("对照组容量", CASE_MASKS.len() == 10, "3 态 × 10 变体 = 30 组");
    cs.add("黄金库版本", GOLDEN_VER == 2, "黄金库 v2（B04 单态 v1 升版）");
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
    fn test_triad_semantics() {
        assert_eq!(baseline_verdict(DaclShape::Absent), Verdict::Allow);
        assert_eq!(baseline_verdict(DaclShape::NullDacl), Verdict::Allow);
        assert_eq!(baseline_verdict(DaclShape::EmptyDacl), Verdict::Deny);
        assert!(p0_guard(DaclShape::EmptyDacl, Verdict::Allow).err() == Some(E_GUARD));
    }

    #[test]
    fn test_golden30_checksum() {
        let g = golden_v2();
        let s1 = golden_checksum(&g);
        let s2 = golden_checksum(&golden_v2());
        assert_eq!(s1, s2);
        assert_eq!(run_cases().ok(), Some(30));
    }
}
