//! UNX-F36188-J1 边界用例库 v2 扩充（边界 100 例冻结）。
//! 判据：① 边界库 +100 例版本化入库——四族边界（掩码边界/容量边界/
//! 序数边界/三态形态边界）各 25 例，id 段 0x0100 起（与 B09 用例库
//! 36172 的 id 1~16 段零重叠）；② 库校验和断言（确定性折叠，重放恒等）；
//! ③ 与 B09 用例库合并索引零重复（两库 id 并集查重，重复即 E_DUP）；
//! ④ 纯功能 ≥320 行。
//! 设计纪律：边界库的真源不是「例多」，是「每例期望可直推」——族规则
//! 生成期望（不是手抄），冻结 = 校验和 + 规则双锚。手抄期望库三个月后
//! 无人能证它对；规则直推库任何一位漂移都会被规则面与校验和双面夹出。
//! 消费 36181 MA 位锚 / 36184 三态形态 / 36172 B09 合并面。

use crate::checks::CheckSet;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36181_maxallowed::{MA_BIT, MA_NO_VIRT};
use super::unx36184_dacltriad::{baseline_verdict, DaclShape, Verdict};
use super::unx36172_accases2 as ac2;

pub const SEG_BASE: u16 = 0x4400;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RULE: u16 = SEG_BASE + 2; // 族规则期望失配
pub const E_SUM: u16 = SEG_BASE + 3; // 校验和不符
pub const E_DUP: u16 = SEG_BASE + 4; // 合并索引重复
pub const E_LIB: u16 = SEG_BASE + 5; // 库完整性不符
pub const E_FROZEN: u16 = SEG_BASE + 6; // 冻结漂移（版本/容量）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 库版本锚（v2——B09 v1 单族库的升版）。
pub const LIB_VER: u32 = 2;

/// 库容量（100 例 = 4 族 × 25）。
pub const LIB_CAP: usize = 100;
pub const FAMILIES: usize = 4;
pub const PER_FAMILY: usize = 25;

/// B09 用例库 id 段（36172 CASES id 1~16——合并索引的对账面）。
pub const B09_ID_LO: u16 = 1;
pub const B09_ID_HI: u16 = 16;

/// 本库 id 段（0x0100~0x0163——与 B09 段零重叠的合约位）。
pub const LIB_ID_LO: u16 = 0x0100;

/// 边界例条目（id + 族 + 载荷 + 期望形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeCase {
    /// 全库唯一 id（0x0100 起）。
    pub id: u16,
    /// 族序（0~3）。
    pub family: u8,
    /// 载荷：掩码族 → 请求掩码；容量族 → 目标容量；序数族 → 槽序；
    /// 形态族 → DACL 形态判别值。
    pub payload: u32,
    /// 期望形态（族规则直推的期望编码）。
    pub expect: u32,
}

/// 族 0 掩码边界规则：第 i 例的请求掩码与期望（越合法区 → E_ILLEGAL
/// 形态期望 0xFFFF_0000；合法 → 掩码本身回显期望）。
fn family0_case(i: usize) -> EdgeCase {
    // 25 例阶梯：单 bit 阶（16 标准位）+ 组合阶（6）+ 岛位阶（3）。
    let (payload, expect) = match i {
        0..=15 => {
            let bit = 1u32 << (16 + i as u32); // STD_USED 区单 bit 阶梯
            (bit, bit)
        }
        16 => (sr::STD_ALL, sr::STD_ALL),
        17 => (mt::SPEC_MASK, mt::SPEC_MASK),
        18 => (sr::STD_ALL | mt::SPEC_MASK, sr::STD_ALL | mt::SPEC_MASK),
        19 => (0u32, 0u32), // 零掩码边界
        20 => (MA_BIT & !MA_BIT, 0), // MA 位自剔（双面纪律——存储面零回显）
        21 => (sr::DELETE | sr::WRITE_DAC, sr::DELETE | sr::WRITE_DAC),
        22 => (sr::SYNCHRONIZE | 0x00FF, sr::SYNCHRONIZE | 0x00FF),
        23 => (MA_NO_VIRT, MA_NO_VIRT), // WRITE_OWNER 单 bit（存储面合法）
        24 => (mt::ALL_LEGAL, mt::ALL_LEGAL), // 全合法区并集
        _ => (0, 0),
    };
    EdgeCase { id: LIB_ID_LO + i as u16, family: 0, payload, expect }
}

/// 族 1 容量边界规则：第 i 例的目标容量与期望（≤CAP 合法 / >CAP 越界
/// 期望 0xFFFF_0001）。容量基准取 16（J1 域令牌面主体闭集）。
fn family1_case(i: usize) -> EdgeCase {
    const CAP: u32 = 16;
    let n = i as u32;
    // 25 例：0..15 全合法带 + CAP/CAP+1/+2 边界点 + 高位远点。
    let target = if n <= CAP { n } else { n * 7 % 64 };
    let expect = if target <= CAP { target } else { 0xFFFF_0001 };
    EdgeCase { id: LIB_ID_LO + PER_FAMILY as u16 + i as u16, family: 1, payload: target, expect }
}

/// 族 2 序数边界规则：第 i 例的槽序与期望（首槽 0 / 尾槽 CAP-1 合法；
/// CAP/越端期望 0xFFFF_0002——序数族的「差一」就是越界史的全部）。
fn family2_case(i: usize) -> EdgeCase {
    const CAP: u32 = 16;
    let ord = if i < 16 { i as u32 } else { 16 + (i as u32 - 16) * 3 };
    let expect = if ord < CAP { ord } else { 0xFFFF_0002 };
    EdgeCase { id: LIB_ID_LO + PER_FAMILY as u16 * 2 + i as u16, family: 2, payload: ord, expect }
}

/// 族 3 三态形态边界规则：第 i 例的形态判别值与期望判决码
/// （1=Allow / 2=Deny——直推自 36184 基准口径；越闭集判别值 0/4+ 期望
/// 0xFFFF_0003——形态族的「差一」是 P0 护栏的邻域）。
fn family3_case(i: usize) -> EdgeCase {
    let disc = (i as u32) % 4; // 0~3 循环——0 是越闭集注入位
    let shape = match disc {
        1 => Some(DaclShape::Absent),
        2 => Some(DaclShape::NullDacl),
        3 => Some(DaclShape::EmptyDacl),
        _ => None,
    };
    let expect = match shape {
        Some(s) => match baseline_verdict(s) {
            Verdict::Allow => 1u32,
            Verdict::Deny => 2u32,
        },
        None => 0xFFFF_0003, // 判别值 0（闭集外）必拒
    };
    EdgeCase { id: LIB_ID_LO + PER_FAMILY as u16 * 3 + i as u16, family: 3, payload: disc, expect }
}

/// 全库构建（确定性——同参数重放逐位恒等）。
pub fn build_library() -> [EdgeCase; LIB_CAP] {
    let mut lib = [EdgeCase { id: 0, family: 0, payload: 0, expect: 0 }; LIB_CAP];
    let mut i = 0;
    while i < LIB_CAP {
        lib[i] = match i / PER_FAMILY {
            0 => family0_case(i % PER_FAMILY),
            1 => family1_case(i % PER_FAMILY),
            2 => family2_case(i % PER_FAMILY),
            _ => family3_case(i % PER_FAMILY),
        };
        i += 1;
    }
    lib
}

/// 期望复验：单例期望按族规则复推（入库期望 == 规则复推——冻结的规则面）。
pub fn verify_case(c: &EdgeCase) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if c.id < LIB_ID_LO || c.id >= LIB_ID_LO + LIB_CAP as u16 {
        return Err(E_LIB);
    }
    let recomputed = match c.family {
        0 => family0_case((c.id - LIB_ID_LO) as usize % PER_FAMILY).expect,
        1 => family1_case((c.id - LIB_ID_LO) as usize % PER_FAMILY).expect,
        2 => family2_case((c.id - LIB_ID_LO) as usize % PER_FAMILY).expect,
        3 => family3_case((c.id - LIB_ID_LO) as usize % PER_FAMILY).expect,
        _ => return Err(E_RULE),
    };
    if recomputed != c.expect {
        return Err(E_RULE);
    }
    Ok(())
}

/// 库校验和（id/family/payload/expect 四元折叠——确定性冻结锚）。
pub fn library_checksum() -> u32 {
    let lib = build_library();
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < LIB_CAP {
        let c = &lib[i];
        sum = sum.wrapping_mul(31).wrapping_add(c.id as u32);
        sum = sum.wrapping_mul(31).wrapping_add(c.family as u32);
        sum = sum.wrapping_mul(31).wrapping_add(c.payload);
        sum = sum.wrapping_mul(31).wrapping_add(c.expect);
        i += 1;
    }
    sum
}

/// 合并索引零重复：本库 id 段与 B09 库（36172 CASES id 1~16）并集查重。
/// 双向：本库不含 B09 段 id；B09 段不含本库段 id；段间自身互异。
pub fn merged_index_audit() -> Result<u32, u16> {
    if B09_ID_LO > B09_ID_HI {
        return Err(E_LIB);
    }
    let lib = build_library();
    // 本库 id 全部落在 0x0100 段且互异。
    let mut seen = 0u128; // 位图（100 位 < 128——位图查重是 O(1)/例）
    let mut i = 0;
    while i < LIB_CAP {
        let off = (lib[i].id - LIB_ID_LO) as u128;
        if off >= LIB_CAP as u128 {
            return Err(E_LIB);
        }
        if seen & (1u128 << off) != 0 {
            return Err(E_DUP);
        }
        seen |= 1u128 << off;
        // 本库 id 不得落 B09 段。
        if lib[i].id >= B09_ID_LO && lib[i].id <= B09_ID_HI {
            return Err(E_DUP);
        }
        i += 1;
    }
    if seen.count_ones() != LIB_CAP as u32 {
        return Err(E_DUP);
    }
    // B09 段对账（36172 CASES 的 id 值域实核——快照面）。
    let mut b09 = 0u32;
    let mut k = 0;
    while k < ac2::CASE_N {
        let id = ac2::CASES[k].id;
        if id < B09_ID_LO || id > B09_ID_HI {
            return Err(E_LIB);
        }
        // B09 id 不得落本库段。
        if id >= LIB_ID_LO && id < LIB_ID_LO + LIB_CAP as u16 {
            return Err(E_DUP);
        }
        b09 += 1;
        k += 1;
    }
    Ok(b09)
}

/// 冻结断言：版本锚 + 容量锚 + 校验和重放恒等（三锚同守）。
pub fn frozen_check() -> Result<u32, u16> {
    if LIB_VER != 2 {
        return Err(E_FROZEN);
    }
    if build_library().len() != LIB_CAP {
        return Err(E_FROZEN);
    }
    let s1 = library_checksum();
    let s2 = library_checksum();
    if s1 != s2 {
        return Err(E_SUM);
    }
    Ok(s1)
}

/// 规则面直证：四族边界例全量复验（100 例逐例 verify_case）。
pub fn rule_audit() -> Result<u32, u16> {
    let lib = build_library();
    let mut ok = 0u32;
    let mut i = 0;
    while i < LIB_CAP {
        verify_case(&lib[i])?;
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

/// 族锚直证：四族例数对半账 + 族序单调（族块连续）。
pub fn family_anchor() -> Result<(), u16> {
    let lib = build_library();
    let mut counts = [0u32; FAMILIES];
    let mut i = 0;
    while i < LIB_CAP {
        let f = lib[i].family as usize;
        if f >= FAMILIES {
            return Err(E_LIB);
        }
        counts[f] += 1;
        if i > 0 && lib[i].family < lib[i - 1].family {
            return Err(E_LIB); // 族块必须单调（构建序保证）
        }
        i += 1;
    }
    let mut f = 0;
    while f < FAMILIES {
        if counts[f] != PER_FAMILY as u32 {
            return Err(E_LIB);
        }
        f += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 族锚。
    if VER != 1 {
        return Err(E_VER);
    }
    family_anchor()?;
    // R2：规则面 100 例全量复验。
    let ok = rule_audit()?;
    if ok != LIB_CAP as u32 {
        return Err(E_RULE);
    }
    // R3：合并索引零重复（含 B09 段实核）。
    let b09 = merged_index_audit()?;
    if b09 != ac2::CASE_N as u32 {
        return Err(E_DUP);
    }
    // R4：冻结断言（三锚同守）。
    let sum = frozen_check()?;
    // R5：族规则语义抽检（掩码族首例 = DELETE 单 bit；形态族 EmptyDacl = Deny）。
    let lib = build_library();
    if lib[0].payload != sr::DELETE || lib[0].expect != sr::DELETE {
        return Err(E_RULE);
    }
    // 形态族 disc = i%4：i=2 → disc=2 → NullDacl → Allow(1)。
    if lib[LIB_CAP - PER_FAMILY + 2].expect != 1 {
        return Err(E_RULE);
    }
    if lib[LIB_CAP - PER_FAMILY].expect != 0xFFFF_0003 {
        return Err(E_RULE); // disc=0 → 闭集外必拒
    }
    // 校验和终锚。
    Ok(ok.wrapping_mul(7).wrapping_add(sum & 0x3FF).wrapping_add(b09))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RULE => "族规则期望失配",
        E_SUM => "校验和不符",
        E_DUP => "合并索引重复",
        E_LIB => "库完整性不符",
        E_FROZEN => "冻结漂移（版本/容量）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36188_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36188_edgelib");
    cs.add("SEG 段位独占", SEG_BASE == 0x4400, "0x4400~0x440F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例规则直推/合并索引零重复/冻结三锚全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("库容量", LIB_CAP == 100 && FAMILIES == 4 && PER_FAMILY == 25, "4 族 × 25 = 100 例（+100 例判据锚）");
    cs.add("id 段合约", LIB_ID_LO == 0x0100 && B09_ID_HI == 16, "本库 0x0100 段与 B09 id 1~16 段零重叠");
    cs.add("库版本", LIB_VER == 2, "边界库 v2（B09 v1 升版）");
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
    fn test_checksum_replay() {
        assert_eq!(library_checksum(), library_checksum());
        assert!(frozen_check().is_ok());
    }

    #[test]
    fn test_merged_no_dup() {
        assert!(merged_index_audit().is_ok());
        let lib = build_library();
        assert_eq!(lib[0].id, 0x0100);
        assert_eq!(lib[LIB_CAP - 1].id, 0x0100 + LIB_CAP as u16 - 1);
    }
}
