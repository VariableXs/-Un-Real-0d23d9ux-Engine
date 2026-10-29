//! unx36011_groupsid — UNX-F36011 组 SID 与成员属性位图预留位
//! （B02 令牌消费锚）。
//!
//! 判据原文：
//!   J1 属性位（DISABLED/DENY_ONLY/INTEGRITY 等）位定义表与版本锚一致；
//!      位冲突注入（双主属性同置）10 例拒绝。
//! 设计纪律：
//!   1. 位定义锚（Windows SE_GROUP_* 语义）：MANDATORY=0x1、
//!      ENABLED_BY_DEFAULT=0x2、ENABLED=0x4、OWNER=0x8、
//!      DENY_ONLY=0x10、INTEGRITY=0x20、INTEGRITY_ENABLED=0x40、
//!      RESOURCE=0x2000_0000、LOGON_ID=0xC000_0000。
//!   2. 冲突矩阵（构造期全量校验，命中即拒）：
//!      OWNER×DENY_ONLY / ENABLED×DENY_ONLY / LOGON_ID×RESOURCE /
//!      ENABLED×(非 ENABLED_BY_DEFAULT) / INTEGRITY 与 INTEGRITY_ENABLED
//!      必须成对 / MANDATORY 必须 ENABLED。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x73B0;

// ── 诊断码闭集（0x73B0~0x73CF，独占）─────────────────────────
const E_OWN_DNY: u16 = SEG_BASE + 1; // OWNER × DENY_ONLY 冲突
const E_ENB_DNY: u16 = SEG_BASE + 2; // ENABLED × DENY_ONLY 冲突
const E_LOG_RES: u16 = SEG_BASE + 3; // LOGON_ID × RESOURCE 冲突
const E_ENB_DEF: u16 = SEG_BASE + 4; // ENABLED 未随 ENABLED_BY_DEFAULT
const E_IL_PAIR: u16 = SEG_BASE + 5; // INTEGRITY 位不成对
const E_MND_ENB: u16 = SEG_BASE + 6; // MANDATORY 未 ENABLED
const E_ANCHOR: u16 = SEG_BASE + 7; // 位定义表与版本锚失配
const E_BAL: u16 = SEG_BASE + 8; // 判例账守恒破
const E_CAP: u16 = SEG_BASE + 9; // 组册满（显性拒）

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// 位定义（版本锚值）。
pub const B_MANDATORY: u32 = 0x0000_0001;
pub const B_ENABLED_BY_DEFAULT: u32 = 0x0000_0002;
pub const B_ENABLED: u32 = 0x0000_0004;
pub const B_OWNER: u32 = 0x0000_0008;
pub const B_DENY_ONLY: u32 = 0x0000_0010;
pub const B_INTEGRITY: u32 = 0x0000_0020;
pub const B_INTEGRITY_ENABLED: u32 = 0x0000_0040;
pub const B_RESOURCE: u32 = 0x2000_0000;
pub const B_LOGON_ID: u32 = 0xC000_0000;

/// 位定义表：(位值, 助记名)。判据 J1 版本锚对照面。
pub const BIT_TAB: [(u32, &'static str); 9] = [
    (B_MANDATORY, "MANDATORY"),
    (B_ENABLED_BY_DEFAULT, "ENABLED_BY_DEFAULT"),
    (B_ENABLED, "ENABLED"),
    (B_OWNER, "OWNER"),
    (B_DENY_ONLY, "DENY_ONLY"),
    (B_INTEGRITY, "INTEGRITY"),
    (B_INTEGRITY_ENABLED, "INTEGRITY_ENABLED"),
    (B_RESOURCE, "RESOURCE"),
    (B_LOGON_ID, "LOGON_ID"),
];

/// 属性位校验：冲突矩阵逐条判，命中即拒（判据 J1 注入面）。
pub fn validate_attrs(attrs: u32) -> Result<(), u16> {
    if attrs & B_OWNER != 0 && attrs & B_DENY_ONLY != 0 {
        return Err(E_OWN_DNY);
    }
    if attrs & B_ENABLED != 0 && attrs & B_DENY_ONLY != 0 {
        return Err(E_ENB_DNY);
    }
    let logon = attrs & B_LOGON_ID == B_LOGON_ID;
    if logon && attrs & B_RESOURCE != 0 {
        return Err(E_LOG_RES);
    }
    if attrs & B_ENABLED != 0 && attrs & B_ENABLED_BY_DEFAULT == 0 {
        return Err(E_ENB_DEF);
    }
    let il = attrs & B_INTEGRITY != 0;
    let ile = attrs & B_INTEGRITY_ENABLED != 0;
    if il != ile {
        return Err(E_IL_PAIR);
    }
    if attrs & B_MANDATORY != 0 && attrs & B_ENABLED == 0 {
        return Err(E_MND_ENB);
    }
    Ok(())
}

/// 组册条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupEntry {
    pub fp: u64,   // 组 SID 指纹
    pub rid: u32,  // 组 RID
    pub attrs: u32, // 属性位
}

/// 组册：B02 令牌消费锚（预留位面）。
pub struct GroupBook {
    entries: [Option<GroupEntry>; 128],
    pub added: u32,
    pub rejected: u32,
    pub req_total: u32,
}

impl GroupBook {
    pub fn new() -> GroupBook {
        GroupBook {
            entries: [None; 128],
            added: 0,
            rejected: 0,
            req_total: 0,
        }
    }

    /// 添加组：属性校验通过才入册（同指纹幂等更新属性）；校验拒入拒绝账。
    pub fn add(&mut self, s: &Sid, attrs: u32) -> Result<bool, u16> {
        self.req_total += 1;
        if let Err(c) = validate_attrs(attrs) {
            self.rejected += 1;
            return Err(c);
        }
        let fp = s.fingerprint();
        let mut i = 0;
        while i < 128 {
            if let Some(e) = self.entries[i] {
                if e.fp == fp {
                    self.entries[i] = Some(GroupEntry { fp, rid: s.rid(), attrs });
                    self.added += 1;
                    return Ok(false);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < 128 {
            if self.entries[j].is_none() {
                self.entries[j] = Some(GroupEntry { fp, rid: s.rid(), attrs });
                self.added += 1;
                return Ok(true);
            }
            j += 1;
        }
        Err(E_CAP)
    }

    /// 属性查询（按指纹）。
    pub fn attrs_of(&self, fp: u64) -> Option<u32> {
        let mut i = 0;
        while i < 128 {
            if let Some(e) = self.entries[i] {
                if e.fp == fp {
                    return Some(e.attrs);
                }
            }
            i += 1;
        }
        None
    }

    /// 启用组清点（令牌消费面：仅 ENABLED 组参与访问判定）。
    pub fn enabled_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < 128 {
            if let Some(e) = self.entries[i] {
                if e.attrs & B_ENABLED != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// DENY_ONLY 组清点（过滤令牌消费面）。
    pub fn deny_only_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < 128 {
            if let Some(e) = self.entries[i] {
                if e.attrs & B_DENY_ONLY != 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 守恒：入册 + 拒绝 == 总请求。
    pub fn conserve(&self) -> bool {
        self.added + self.rejected == self.req_total
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_OWN_DNY => "OWNER × DENY_ONLY 位冲突",
        E_ENB_DNY => "ENABLED × DENY_ONLY 位冲突",
        E_LOG_RES => "LOGON_ID × RESOURCE 位冲突",
        E_ENB_DEF => "ENABLED 未随 ENABLED_BY_DEFAULT",
        E_IL_PAIR => "INTEGRITY 与 INTEGRITY_ENABLED 不成对",
        E_MND_ENB => "MANDATORY 未随 ENABLED",
        E_ANCHOR => "位定义表与版本锚失配",
        E_BAL => "判例账守恒破",
        E_CAP => "组册满（显性拒绝）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：位定义锚对平 + 100 合法组合入册 + 10 冲突注入全拒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：位定义表版本锚——9 位逐项值对平、助记名非空、位值互异。
    if BIT_TAB.len() != 9 {
        return Err(E_ANCHOR);
    }
    let expect: [u32; 9] = [
        0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x40, 0x2000_0000, 0xC000_0000,
    ];
    let mut i = 0;
    while i < 9 {
        if BIT_TAB[i].0 != expect[i] || BIT_TAB[i].1.is_empty() {
            return Err(E_ANCHOR);
        }
        let mut j = i + 1;
        while j < 9 {
            if BIT_TAB[i].0 == BIT_TAB[j].0 {
                return Err(E_ANCHOR);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R2：100 例合法组合入册——LCG 在合法空间铺（基准位 + 可选 OWNER/
    // RESOURCE/INTEGRITY 对），全部 validate 通过并入册。
    let mut seed: u32 = 0x360B_0001;
    let mut book = GroupBook::new();
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut attrs = B_ENABLED_BY_DEFAULT | B_ENABLED | B_MANDATORY;
        let pick = (seed >> 8) % 4;
        if pick == 1 {
            attrs |= B_INTEGRITY | B_INTEGRITY_ENABLED;
        } else if pick == 2 {
            attrs |= B_RESOURCE;
        } else if pick == 3 {
            attrs |= B_OWNER;
        }
        validate_attrs(attrs)?;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[32, 5000 + (n as u32)])?;
        book.add(&s, attrs)?;
        n += 1;
    }
    if book.added != 100 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：位冲突注入 10 例——六条冲突规则族各铺，构造期全拒。
    let s = Sid::new(1, Sid::ia_from(5), &[32, 544])?;
    let conflicts: [u32; 10] = [
        B_OWNER | B_DENY_ONLY,                       // 规则1
        B_ENABLED | B_DENY_ONLY,                     // 规则2
        B_LOGON_ID | B_RESOURCE,                     // 规则3
        B_ENABLED,                                   // 规则4（缺 BY_DEFAULT）
        B_INTEGRITY,                                 // 规则5a（缺 ENABLED 位）
        B_INTEGRITY_ENABLED,                         // 规则5b（缺 INTEGRITY 位）
        B_MANDATORY,                                 // 规则6（缺 ENABLED）
        B_OWNER | B_DENY_ONLY | B_ENABLED_BY_DEFAULT, // 规则1 变体
        B_LOGON_ID | B_RESOURCE | B_ENABLED_BY_DEFAULT | B_ENABLED, // 规则3 变体
        B_MANDATORY | B_INTEGRITY_ENABLED,           // 规则6+5b 复合
    ];
    let mut k = 0;
    while k < 10 {
        if book.add(&s, conflicts[k]) != Err(conflict_code(conflicts[k])) {
            return Err(conflict_code(conflicts[k]));
        }
        k += 1;
    }
    if book.rejected != 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：消费面清点——ENABLED/DENY_ONLY 计数与入册内容对平；守恒。
    // 册内 100 组全为 ENABLED（合法空间锚定）。
    if book.enabled_count() != 100 {
        return Err(E_BAL);
    }
    if book.deny_only_count() != 0 {
        return Err(E_BAL);
    }
    if !book.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

/// 冲突样本 → 期望诊断码（独立映射面，与 validate_attrs 互为对账）。
fn conflict_code(attrs: u32) -> u16 {
    if attrs & B_OWNER != 0 && attrs & B_DENY_ONLY != 0 {
        return E_OWN_DNY;
    }
    if attrs & B_ENABLED != 0 && attrs & B_DENY_ONLY != 0 {
        return E_ENB_DNY;
    }
    if attrs & B_LOGON_ID == B_LOGON_ID && attrs & B_RESOURCE != 0 {
        return E_LOG_RES;
    }
    if attrs & B_ENABLED != 0 && attrs & B_ENABLED_BY_DEFAULT == 0 {
        return E_ENB_DEF;
    }
    if (attrs & B_INTEGRITY != 0) != (attrs & B_INTEGRITY_ENABLED != 0) {
        return E_IL_PAIR;
    }
    if attrs & B_MANDATORY != 0 && attrs & B_ENABLED == 0 {
        return E_MND_ENB;
    }
    0
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36011_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36011_groupsid");
    cs.add("SEG 段位独占", SEG_BASE == 0x73B0, "0x73B0~0x73CF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
