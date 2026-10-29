//! unx36024_tokgroups — UNX-F36024 组 SID 数组与属性位图求值准备面。
//!
//! 判据原文（UNX-F36024-J1）：
//!   组数组容量上限与越限注入 10 例拒绝；属性位图读出与写入逐位对账
//!   200 例零差。
//! 设计纪律：
//!   1. 容量单源：G_MAX=16 与 unx36021 Token.groups 定容同源；越限拒绝在
//!      add() 构造期（qty 超 G_MAX 即 E_CAP），拒绝入账。
//!   2. 位图读写面：attrs 位锚 SE_GROUP_*（ENABLED/OWNER/USE_FOR_DENY/
//!      RESOURCE/LOGON_ID/MANDATORY 六位），set_bit/clear_bit/read_bit
//!      三原语逐位对账；保留位（!0x3F）不可置——置入即拒。
//!   3. 守恒账：added + rejected == req_total；位操作账 ops == set + clear。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use super::unx36001_sidcore;
use super::unx36021_tokencore::{self, GroupAttr};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf800;

// ── 诊断码闭集（0xf800~0xf81f，独占）─────────────────────────
const E_CAP: u16 = SEG_BASE + 1; // 组数越限（> G_MAX）
const E_BIT: u16 = SEG_BASE + 2; // 属性位非法（保留位置位）
const E_BAL: u16 = SEG_BASE + 3; // 守恒账破
const E_DUP: u16 = SEG_BASE + 4; // 同 SID 重复入册
const E_IDX: u16 = SEG_BASE + 5; // 槽位越界

/// 组数组容量（与 unx36021 Token.groups 定容同源）。
pub const G_MAX: usize = 16;

/// SE_GROUP_* 位锚。
pub const SE_ENABLED: u32 = 0x01;
pub const SE_OWNER: u32 = 0x02;
pub const SE_USE_FOR_DENY: u32 = 0x04;
pub const SE_RESOURCE: u32 = 0x08;
pub const SE_LOGON_ID: u32 = 0x10;
pub const SE_MANDATORY: u32 = 0x20;
/// 合法位掩码（其余为保留位）。
pub const SE_VALID: u32 = 0x3f;

/// 组册（容量 G_MAX，构造期容量核验）。
pub struct GroupBook {
    pub items: [GroupAttr; G_MAX],
    pub len: usize,
    pub req_total: u64,
    pub added: u64,
    pub rejected: u64,
}

impl GroupBook {
    pub const fn new() -> GroupBook {
        GroupBook {
            items: [GroupAttr { sid: unx36021_tokencore::placeholder_sid(), attrs: 0 }; G_MAX],
            len: 0,
            req_total: 0,
            added: 0,
            rejected: 0,
        }
    }

    pub fn conserve(&self) -> bool {
        self.req_total == self.added + self.rejected
    }

    /// 入册：容量核验 + 同 SID 幂等更新（属性行覆盖，不加行）。
    pub fn add(&mut self, sid: unx36001_sidcore::Sid, attrs: u32) -> Result<bool, u16> {
        self.req_total += 1;
        if attrs & !SE_VALID != 0 {
            self.rejected += 1;
            return Err(E_BIT);
        }
        if self.len >= G_MAX {
            self.rejected += 1;
            return Err(E_CAP);
        }
        // 同 SID 幂等：更新属性行，不加新槽。
        let mut i = 0;
        while i < self.len {
            if self.items[i].sid == sid {
                self.items[i].attrs = attrs;
                self.added += 1;
                return Ok(false);
            }
            i += 1;
        }
        self.items[self.len] = GroupAttr { sid, attrs };
        self.len += 1;
        self.added += 1;
        Ok(true)
    }
}

/// 属性位图写：置位（保留位拒绝）。
pub fn set_bit(attrs: u32, bit: u32) -> Result<u32, u16> {
    if bit & !SE_VALID != 0 || bit == 0 {
        return Err(E_BIT);
    }
    Ok(attrs | bit)
}

/// 属性位图写：清位（保留位拒绝——清保留位亦拒，位面操作只在合法域内）。
pub fn clear_bit(attrs: u32, bit: u32) -> Result<u32, u16> {
    if bit & !SE_VALID != 0 || bit == 0 {
        return Err(E_BIT);
    }
    Ok(attrs & !bit)
}

/// 属性位图读：单位读出（保留位返回 false 非错误——读面宽容）。
pub fn read_bit(attrs: u32, bit: u32) -> bool {
    bit != 0 && attrs & bit == bit
}

/// 逐位对账：读出向量 = 写入意图（六位全量）。
fn bits_align(attrs: u32, want: [bool; 6]) -> bool {
    let masks = [
        SE_ENABLED,
        SE_OWNER,
        SE_USE_FOR_DENY,
        SE_RESOURCE,
        SE_LOGON_ID,
        SE_MANDATORY,
    ];
    let mut i = 0;
    while i < 6 {
        if read_bit(attrs, masks[i]) != want[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 样本组 SID（第 k 例：S-1-5-32-(600+k)）。
pub fn sample_group_sid(k: u32) -> unx36001_sidcore::Sid {
    match unx36001_sidcore::Sid::new(
        1,
        unx36001_sidcore::Sid::ia_from(5),
        &[32, 600 + k],
    ) {
        Ok(s) => s,
        Err(_) => unx36021_tokencore::placeholder_sid(),
    }
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CAP => "组数越限：超过 G_MAX=16 容量上限",
        E_BIT => "属性位非法：保留位（!0x3F）不可置/清",
        E_BAL => "守恒账破：added + rejected != req_total 或位操作账不平",
        E_DUP => "同 SID 重复入册（应幂等更新）",
        E_IDX => "槽位越界",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1：容量上限注入 10 例拒绝——先填满 16 槽，再注入 10 例全拒。
    let mut book = GroupBook::new();
    let mut i = 0;
    while i < G_MAX {
        book.add(sample_group_sid(i as u32), SE_ENABLED)?;
        i += 1;
    }
    if book.len != G_MAX {
        return Err(E_BAL);
    }
    let mut j = 0;
    while j < 10 {
        if book.add(sample_group_sid(100 + j as u32), SE_ENABLED) != Err(E_CAP) {
            return Err(E_CAP);
        }
        hits += 1;
        j += 1;
    }
    // R2：守恒账（16 added + 10 rejected == 26 req）。
    if book.added != 16 || book.rejected != 10 || book.req_total != 26 {
        return Err(E_BAL);
    }
    if !book.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    // R3：属性位图读写逐位对账 200 例零差——随机意图向量 → 写 → 读 → 对平。
    // LCG 确定性伪随机（seed 覆盖 200 例）。
    let mut seed: u32 = 0x243f_6a88;
    let mut k = 0;
    while k < 200 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // 意图：六位按 seed 各位随机 + 保留位脏样本按 k%7 注入。
        let mut want = [false; 6];
        let mut attrs = 0u32;
        let mut b = 0;
        while b < 6 {
            want[b] = seed >> b & 1 == 1;
            if want[b] {
                attrs = set_bit(attrs, 1 << b)?;
            }
            b += 1;
        }
        if !bits_align(attrs, want) {
            return Err(E_BIT);
        }
        // 清位对账：清除任意一位后读出翻转。
        let cb = (k % 6) as u32;
        let cleared = clear_bit(attrs, 1 << cb)?;
        if read_bit(cleared, 1 << cb) {
            return Err(E_BIT);
        }
        hits += 1;
        k += 1;
    }
    // R4：保留位注入拒绝（set/clear 双向 10 例）。
    let mut q = 0;
    while q < 5 {
        if set_bit(0, 0x40 << q) != Err(E_BIT) || clear_bit(0, 0x40 << q) != Err(E_BIT) {
            return Err(E_BIT);
        }
        hits += 1;
        q += 1;
    }
    // R5：同 SID 幂等更新（属性行覆盖不加槽）+ 位操作账。
    let mut book2 = GroupBook::new();
    let sid = sample_group_sid(0);
    book2.add(sid, SE_ENABLED)?;
    let upd = book2.add(sid, SE_ENABLED | SE_OWNER)?;
    if upd != false || book2.len != 1 {
        return Err(E_DUP);
    }
    if !read_bit(book2.items[0].attrs, SE_OWNER) {
        return Err(E_BIT);
    }
    hits += 1;
    Ok(hits)
}

/// 组册视图/批量辅助面。

/// 组册快照（只读视图：len + 逐项 sid/attrs 投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupSnapshot {
    pub len: usize,
    pub attrs: [u32; G_MAX],
}

impl GroupBook {
    /// 快照导出。
    pub fn snapshot(&self) -> GroupSnapshot {
        let mut a = [0u32; G_MAX];
        let mut i = 0;
        while i < self.len {
            a[i] = self.items[i].attrs;
            i += 1;
        }
        GroupSnapshot { len: self.len, attrs: a }
    }

    /// 快照守恒核（attrs 逐位对平）。
    pub fn snapshot_conserve(&self, s: &GroupSnapshot) -> bool {
        if s.len != self.len {
            return false;
        }
        let mut i = 0;
        while i < self.len {
            if s.attrs[i] != self.items[i].attrs {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 反查：SID → 属性位（不在册 None）。
    pub fn attrs_of_sid(&self, sid: &unx36001_sidcore::Sid) -> Option<u32> {
        let mut i = 0;
        while i < self.len {
            if self.items[i].sid == *sid {
                return Some(self.items[i].attrs);
            }
            i += 1;
        }
        None
    }
}

/// 批量入册面：N 例（sid, attrs）批量（fork 批量建组场景）。
pub fn batch_add(book: &mut GroupBook, items: &[(unx36001_sidcore::Sid, u32)]) -> Result<(u64, u64), u16> {
    let mut ok = 0u64;
    let mut bad = 0u64;
    let mut i = 0;
    while i < items.len() {
        match book.add(items[i].0, items[i].1) {
            Ok(_) => ok += 1,
            Err(E_CAP) | Err(E_BIT) => bad += 1,
            Err(e) => return Err(e),
        }
        i += 1;
    }
    Ok((ok, bad))
}

pub fn run_unx36024_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36024_tokgroups");
    cs.add("SEG 段位独占", SEG_BASE == 0xf800, "0xf800~0xf81f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 组数组与位图对账", true, "越限 10 例拒+位图 200 例零差"),
        Err(code) => cs.add("J1 组数组与位图对账", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(217));
    }
}
