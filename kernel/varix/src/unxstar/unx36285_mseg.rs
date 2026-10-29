//! UNX-F36285-J1 跨批冻结总表（B09–B14 引擎面 ≥18 项汇总）。
//! 判据：UNX-F36285-J1 冻结表逐项可检索版本化；变更走 ADR 断言；
//! 与 B08 F 型冻结表合并索引零冲突；纯功能代码 ≥300 行。
//! 设计纪律：冻结表是语义防漂移的宪法——Windows 驱动契约一旦冻结
//! （如 SSDT 序号、Ioctl 码）就不许静默改；本域把 B09–B14 引擎面
//! ≥18 项冻结锚汇总成总表：
//!   - 逐项可检索：id → (名, 批次, 版本) 恒可查，缺项即账破；
//!   - 版本化：每项 ver ≥ 1，变更必须 ver 单调 +1（禁跳版禁回退）；
//!   - 变更走 ADR：无 ADR 号的变更一律拒收（E_ADR）——ADR 记录与
//!     表项同账落簿，变更历史可回放；
//!   - 合并索引：与 B08 F 型冻结表（8 项）合并成一维索引，id 全域
//!     唯一零冲突——撞号即两套宪法打架，必须显性失败。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB070;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MISS: u16 = SEG_BASE + 2; // 冻结项缺册（不可检索）
pub const E_ADR: u16 = SEG_BASE + 3; // 变更无 ADR（拒收）
pub const E_VORDER: u16 = SEG_BASE + 4; // 版本不单调（跳版/回退）
pub const E_CLASH: u16 = SEG_BASE + 5; // 合并索引 id 撞号
pub const E_RANGE: u16 = SEG_BASE + 6; // 下标越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 冻结表规格 -------------------------------------------------------------
/// 单条冻结项（id 全域唯一锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeItem {
    pub id: u32,
    pub name: &'static str,
    pub batch: u8,  // 来源批（9..14）
    pub ver: u32,   // 冻结版本（≥1）
}

/// B09–B14 引擎面 18 项（每批 3 项——批次面均匀覆盖）。
pub const FREEZE_ITEMS: [FreezeItem; 18] = [
    FreezeItem { id: 9001, name: "B09 引擎核锚", batch: 9, ver: 1 },
    FreezeItem { id: 9002, name: "B09 掩码位面", batch: 9, ver: 1 },
    FreezeItem { id: 9003, name: "B09 求值口径", batch: 9, ver: 2 },
    FreezeItem { id: 9101, name: "B10 边界闭集", batch: 10, ver: 1 },
    FreezeItem { id: 9102, name: "B10 拒绝序", batch: 10, ver: 1 },
    FreezeItem { id: 9103, name: "B10 容量账", batch: 10, ver: 1 },
    FreezeItem { id: 9201, name: "B11 继承树锚", batch: 11, ver: 1 },
    FreezeItem { id: 9202, name: "B11 规范序", batch: 11, ver: 1 },
    FreezeItem { id: 9203, name: "B11 传播位", batch: 11, ver: 3 },
    FreezeItem { id: 9301, name: "B12 句柄面", batch: 12, ver: 1 },
    FreezeItem { id: 9302, name: "B12 生命周期", batch: 12, ver: 1 },
    FreezeItem { id: 9303, name: "B12 泄漏账", batch: 12, ver: 2 },
    FreezeItem { id: 9401, name: "B13 模拟四级", batch: 13, ver: 1 },
    FreezeItem { id: 9402, name: "B13 诚实三态", batch: 13, ver: 1 },
    FreezeItem { id: 9403, name: "B13 身份可见位", batch: 13, ver: 1 },
    FreezeItem { id: 9501, name: "B14 UAC 门", batch: 14, ver: 1 },
    FreezeItem { id: 9502, name: "B14 静默模式", batch: 14, ver: 2 },
    FreezeItem { id: 9503, name: "B14 提权账", batch: 14, ver: 1 },
];

/// B08 F 型冻结表 8 项（合并索引的另一半）。
pub const F_TYPE_ITEMS: [FreezeItem; 8] = [
    FreezeItem { id: 8001, name: "B08 F 门锚", batch: 8, ver: 1 },
    FreezeItem { id: 8002, name: "B08 F 序号面", batch: 8, ver: 1 },
    FreezeItem { id: 8003, name: "B08 F 容量", batch: 8, ver: 1 },
    FreezeItem { id: 8004, name: "B08 F 溢出账", batch: 8, ver: 1 },
    FreezeItem { id: 8005, name: "B08 F 幂等", batch: 8, ver: 2 },
    FreezeItem { id: 8006, name: "B08 F 快照", batch: 8, ver: 1 },
    FreezeItem { id: 8007, name: "B08 F 重放", batch: 8, ver: 1 },
    FreezeItem { id: 8008, name: "B08 F 收口", batch: 8, ver: 1 },
];

/// ADR 变更记录（变更走 ADR 的账面形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AdrRec {
    pub adr_no: u32,
    pub item: u32,
    pub old_ver: u32,
    pub new_ver: u32,
}

/// 跨批冻结总表（可变版本面 + ADR 账）。
pub struct FreezeTable {
    items: Vec<FreezeItem>,
    adrs: Vec<AdrRec>,
}

const FREEZE_CAP: usize = 64;

impl FreezeTable {
    pub fn new() -> FreezeTable {
        let mut items: Vec<FreezeItem> = Vec::new();
        let mut i = 0;
        while i < FREEZE_ITEMS.len() {
            items.push(FREEZE_ITEMS[i]);
            i += 1;
        }
        FreezeTable { items, adrs: Vec::new() }
    }

    /// 逐项可检索：id → 冻结项（缺项即 E_MISS）。
    pub fn lookup(&self, id: u32) -> Result<FreezeItem, u16> {
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].id == id {
                return Ok(self.items[i]);
            }
            i += 1;
        }
        Err(E_MISS)
    }

    /// 变更走 ADR：无 ADR 号拒收；版本必须 +1 单调；ADR 记录同账落簿。
    pub fn change(&mut self, id: u32, adr_no: u32) -> Result<u32, u16> {
        if adr_no == 0 {
            return Err(E_ADR); // 无 ADR 的变更一律拒收
        }
        let mut idx = self.items.len();
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].id == id {
                idx = i;
                break;
            }
            i += 1;
        }
        if idx == self.items.len() {
            return Err(E_MISS);
        }
        let old = self.items[idx].ver;
        let new = old + 1;
        if self.adrs.len() >= FREEZE_CAP {
            return Err(E_RANGE);
        }
        self.items[idx].ver = new;
        self.adrs.push(AdrRec { adr_no, item: id, old_ver: old, new_ver: new });
        Ok(new)
    }

    /// ADR 账回放：每条记录版本链连续（old+1 == new）且表内版本一致。
    pub fn adr_replay(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.adrs.len() {
            let r = &self.adrs[i];
            if r.new_ver != r.old_ver + 1 {
                return Err(E_VORDER);
            }
            let cur = self.lookup(r.item)?;
            if cur.ver != r.new_ver {
                return Err(E_VORDER);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn adr_count(&self) -> usize {
        self.adrs.len()
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

/// 合并索引：B08 F 型 + B09–B14 总表拼成一维索引（id 升序）。
/// 返回 (排序后 id 序列, 来源标记序列)；撞号即 E_CLASH。
pub fn merged_index(extra: &[FreezeItem]) -> Result<(Vec<u32>, Vec<u8>), u16> {
    let mut ids: Vec<(u32, u8)> = Vec::new();
    let mut i = 0;
    while i < F_TYPE_ITEMS.len() {
        ids.push((F_TYPE_ITEMS[i].id, 0)); // 来源 0 = B08 F 型
        i += 1;
    }
    i = 0;
    while i < extra.len() {
        ids.push((extra[i].id, 1)); // 来源 1 = 跨批总表
        i += 1;
    }
    ids.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out_ids: Vec<u32> = Vec::new();
    let mut out_src: Vec<u8> = Vec::new();
    i = 0;
    while i < ids.len() {
        if i > 0 && ids[i].0 == ids[i - 1].0 {
            return Err(E_CLASH); // 撞号 = 两套宪法打架
        }
        out_ids.push(ids[i].0);
        out_src.push(ids[i].1);
        i += 1;
    }
    Ok((out_ids, out_src))
}

/// 冻结项题注（可检索呈现面）。
pub fn item_caption(it: FreezeItem) -> String {
    let mut s = String::new();
    s.push_str("#");
    s.push_str(&it.id.to_string());
    s.push_str(" ");
    s.push_str(it.name);
    s.push_str(" v");
    s.push_str(&it.ver.to_string());
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 规格守恒（18 项 + 8 项容量账）。
    if VER != 1 {
        return Err(E_VER);
    }
    if FREEZE_ITEMS.len() < 18 || F_TYPE_ITEMS.len() != 8 {
        return Err(E_MISS);
    }
    // R2：逐项可检索 + 版本化（18 项全查得、ver ≥ 1、题注可读）。
    let t = FreezeTable::new();
    let mut i = 0;
    while i < FREEZE_ITEMS.len() {
        let it = t.lookup(FREEZE_ITEMS[i].id)?;
        if it.ver < 1 || it.batch < 9 || it.batch > 14 {
            return Err(E_MISS);
        }
        if item_caption(it).is_empty() {
            return Err(E_MISS);
        }
        i += 1;
    }
    if t.item_count() != 18 {
        return Err(E_MISS);
    }
    // R3：变更走 ADR——无 ADR 拒收；有 ADR 版本 +1 落账。
    let mut t2 = FreezeTable::new();
    if t2.change(9001, 0).err() != Some(E_ADR) {
        return Err(E_ADR); // 无 ADR 必须拒收
    }
    let v = t2.change(9001, 2026)?;
    if v != 2 || t2.lookup(9001)?.ver != 2 {
        return Err(E_VORDER);
    }
    if t2.adr_count() != 1 || t2.adr_replay().is_err() {
        return Err(E_VORDER);
    }
    // 再走一轮 ADR（版本链 1→2→3 连续）。
    let v3 = t2.change(9001, 2027)?;
    if v3 != 3 || t2.adr_replay().is_err() {
        return Err(E_VORDER);
    }
    // 缺册项不可检索也不可变更。
    if t2.lookup(9999).err() != Some(E_MISS) || t2.change(9999, 1).err() != Some(E_MISS) {
        return Err(E_MISS);
    }
    // R4：合并索引零冲突——B08 F 型 8 项 + 总表 18 项 = 26 条，升序唯一。
    let mut extra: Vec<FreezeItem> = Vec::new();
    i = 0;
    while i < FREEZE_ITEMS.len() {
        extra.push(FREEZE_ITEMS[i]);
        i += 1;
    }
    let (ids, srcs) = merged_index(&extra)?;
    if ids.len() != 26 || srcs.len() != 26 {
        return Err(E_CLASH);
    }
    i = 1;
    while i < ids.len() {
        if ids[i] <= ids[i - 1] {
            return Err(E_CLASH);
        }
        i += 1;
    }
    // 撞号注入：把 F 型 id 8001 复制进总表侧——必须显性 E_CLASH。
    let mut clash: Vec<FreezeItem> = Vec::new();
    clash.push(FreezeItem { id: 8001, name: "撞号样本", batch: 9, ver: 1 });
    if merged_index(&clash).err() != Some(E_CLASH) {
        return Err(E_CLASH);
    }
    // 校验和：索引 id 折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < ids.len() {
        sum = sum.wrapping_mul(7).wrapping_add(ids[i].wrapping_mul(srcs[i] as u32 + 1));
        i += 1;
    }
    Ok(sum.wrapping_add(t2.lookup(9001)?.ver).wrapping_add(26))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MISS => "冻结项缺册（不可检索）",
        E_ADR => "变更无 ADR（一律拒收）",
        E_VORDER => "版本不单调（跳版/回退/ADR 链断）",
        E_CLASH => "合并索引 id 撞号",
        E_RANGE => "下标/容量越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36285_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36285_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB070, "0xB070~0xB07F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 项全可检索版本化/ADR 链 1→2→3/合并索引 26 条零冲突"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("冻结项容量", FREEZE_ITEMS.len() == 18 && F_TYPE_ITEMS.len() == 8, "B09–B14 十八项 + B08 F 型八项");
    cs.add("ADR 拒收", FreezeTable::new().change(9001, 0).err() == Some(E_ADR), "无 ADR 变更一律拒收");
    cs.add("合并索引零冲突", {
        let mut extra: Vec<FreezeItem> = Vec::new();
        let mut i = 0;
        while i < FREEZE_ITEMS.len() {
            extra.push(FREEZE_ITEMS[i]);
            i += 1;
        }
        merged_index(&extra).map(|x| x.0.len() == 26).unwrap_or(false)
    }, "B08+ 本表 26 条升序唯一");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36285_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36285_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_version_chain_monotonic() {
        // 版本链直证：两次 ADR 后 1→2→3 连续，回放恒一致。
        let mut t = FreezeTable::new();
        let a = t.change(9203, 11).ok().unwrap_or(0);
        let b = t.change(9203, 12).ok().unwrap_or(0);
        assert_eq!(a, 2);
        assert_eq!(b, 3);
        assert!(t.adr_replay().is_ok());
        assert_eq!(t.lookup(9203).ok().unwrap_or(FREEZE_ITEMS[8]).ver, 3);
        assert_eq!(t.adr_count(), 2);
    }
}
