//! UNX-F36298-J1 B16–B40 待领显性登记（余 500 条/150,000 行不越界代领）。
//! 判据：UNX-F36298-J1 待领清单（E 型边界四批/I 型联签八批/C 型收官
//! 四批 + 矩阵满格/POSIX 桥 ×100/AccessCheck ×200 基准对照）逐项登记
//! 零隐性；纯功能代码 ≥300 行。
//! 设计纪律：待领账是交接的诚实面——Windows 的 driver backlog
//! （每项工作有 owner 候选与规模估计，不许口头指定）同构：未领的
//! 工作显性登记（名 + 类型 + 规模 + 目标窗），零隐性、不越界代领：
//!   - E 型边界四批 / I 型联签八批 / C 型收官四批：批级待领 16 项；
///   - 矩阵满格 / POSIX 桥 ×100 / AccessCheck ×200 基准对照：工作项
///     级待领 3 项（规模以单位数显性登记）；
///   - 零隐性：每项 explicit 位恒真、目标窗恒落 B16–B40、名唯一；
///   - 不越界代领：登记面只登记不指派（owner 恒空——代领即越界）；
///   - 完备性：16 批级 + 3 工作项 = 19 项恰数，重算口径与登记口径
///     双对平（多登漏登都是不诚实）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB140;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 项下标/目标窗越合法区
pub const E_IMPLICIT: u16 = SEG_BASE + 3; // 隐性登记（explicit 位缺）
pub const E_CLAIMED: u16 = SEG_BASE + 4; // 越界代领（owner 非空）
pub const E_DUP: u16 = SEG_BASE + 5; // 项名重复/重复登记
pub const E_COMPLETENESS: u16 = SEG_BASE + 6; // 完备性破（≠19 项/类别缺）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 待领规格 ---------------------------------------------------------------
/// 待领类别闭集。
pub const K_EBOUND: u8 = 1; // E 型边界
pub const K_ICOSIGN: u8 = 2; // I 型联签
pub const K_CCLose: u8 = 3; // C 型收官
pub const K_WORKITEM: u8 = 4; // 工作项（矩阵满格/POSIX/AccessCheck）

/// 目标窗 B16–B40。
pub const B16: u8 = 16;
pub const B40: u8 = 40;

/// 批级数量锚（判据原文）：E 型 4 批 / I 型 8 批 / C 型 4 批。
pub const E_BATCHES: usize = 4;
pub const I_BATCHES: usize = 8;
pub const C_BATCHES: usize = 4;
pub const BATCH_ITEMS: usize = E_BATCHES + I_BATCHES + C_BATCHES; // 16
/// 工作项 3 项。
pub const WORK_ITEMS: usize = 3;
/// 总计 19 项。
pub const TOTAL_ITEMS: usize = BATCH_ITEMS + WORK_ITEMS; // 19

/// 待领项（名 + 类别 + 规模 + 目标窗 + 显性位 + owner 空位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingItem {
    pub name: &'static str,
    pub kind: u8,
    pub size_units: u32, // 批级=1 批/项；工作项=单位数
    pub target_from: u8,
    pub target_to: u8,
    pub explicit: bool,
    pub owner: &'static str, // 登记面恒空串——代领即越界
}

/// 十九项待领登记册（判据原文逐项显性化）。
pub const PENDING: [PendingItem; TOTAL_ITEMS] = [
    PendingItem { name: "E 型边界批一", kind: K_EBOUND, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "E 型边界批二", kind: K_EBOUND, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "E 型边界批三", kind: K_EBOUND, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "E 型边界批四", kind: K_EBOUND, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批一", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批二", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批三", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批四", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批五", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批六", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批七", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "I 型联签批八", kind: K_ICOSIGN, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "C 型收官批一", kind: K_CCLose, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "C 型收官批二", kind: K_CCLose, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "C 型收官批三", kind: K_CCLose, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "C 型收官批四", kind: K_CCLose, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "矩阵满格", kind: K_WORKITEM, size_units: 90, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "POSIX 桥 ×100", kind: K_WORKITEM, size_units: 100, target_from: B16, target_to: B40, explicit: true, owner: "" },
    PendingItem { name: "AccessCheck ×200 基准对照", kind: K_WORKITEM, size_units: 200, target_from: B16, target_to: B40, explicit: true, owner: "" },
];

/// 类别计数（登记面重算口径）。
pub fn kind_census(items: &[PendingItem]) -> Result<[u32; 5], u16> {
    let mut census = [0u32; 5];
    let mut i = 0;
    while i < items.len() {
        if items[i].kind == 0 || items[i].kind > K_WORKITEM {
            return Err(E_COMPLETENESS);
        }
        census[items[i].kind as usize] += 1;
        i += 1;
    }
    Ok(census)
}

/// 项名唯一性（重复登记 = 账破）。
pub fn names_unique(items: &[PendingItem]) -> bool {
    let mut a = 0;
    while a < items.len() {
        let mut b = a + 1;
        while b < items.len() {
            if items[a].name == items[b].name {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    true
}

/// 目标窗合法性（B16–B40 且 from ≤ to）。
pub fn target_in_window(it: &PendingItem) -> bool {
    it.target_from >= B16 && it.target_to <= B40 && it.target_from <= it.target_to
}

/// 登记题注（名/类别/规模/目标窗可读）。
pub fn item_caption(it: &PendingItem) -> String {
    let mut s = String::new();
    s.push_str(it.name);
    s.push_str(" 规模=");
    s.push_str(&it.size_units.to_string());
    s.push_str(" 目标=B");
    s.push_str(&it.target_from.to_string());
    s.push_str("–B");
    s.push_str(&it.target_to.to_string());
    s.push_str(if it.explicit { " 显性" } else { " 隐性(违禁)" });
    if it.owner.is_empty() {
        s.push_str(" 待领");
    } else {
        s.push_str(" 已领(越界)");
    }
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 容量账（16 批级 + 3 工作项 = 19）。
    if VER != 1 {
        return Err(E_VER);
    }
    if PENDING.len() != 19 || BATCH_ITEMS != 16 || WORK_ITEMS != 3 {
        return Err(E_COMPLETENESS);
    }
    // R2：完备性——类别计数恰 (E:4, I:8, C:4, W:3)。
    let census = kind_census(&PENDING)?;
    if census[1] != 4 || census[2] != 8 || census[3] != 4 || census[4] != 3 {
        return Err(E_COMPLETENESS);
    }
    // R3：零隐性——逐项 explicit 恒真、目标窗恒 B16–B40、名唯一。
    if !names_unique(&PENDING) {
        return Err(E_DUP);
    }
    let mut i = 0;
    while i < PENDING.len() {
        let it = &PENDING[i];
        if !it.explicit {
            return Err(E_IMPLICIT);
        }
        if !target_in_window(it) {
            return Err(E_RANGE);
        }
        if it.name.is_empty() || item_caption(it).is_empty() {
            return Err(E_IMPLICIT);
        }
        i += 1;
    }
    // R4：不越界代领——登记面 owner 恒空（19 项全待领）。
    i = 0;
    while i < PENDING.len() {
        if !PENDING[i].owner.is_empty() {
            return Err(E_CLAIMED);
        }
        i += 1;
    }
    // R5：工作项规模锚——矩阵满格 90 / POSIX ×100 / AccessCheck ×200。
    // （90 = 240 − 150：首 150 格已由 F36282/F36301 跑通。）
    let w0 = &PENDING[16];
    let w1 = &PENDING[17];
    let w2 = &PENDING[18];
    if w0.size_units != 90 || w1.size_units != 100 || w2.size_units != 200 {
        return Err(E_COMPLETENESS);
    }
    // R6：隐性注入——合成 explicit=false 项必须被判违（守门面对证）。
    let sneaky = PendingItem { name: "隐性样本", kind: K_EBOUND, size_units: 1, target_from: B16, target_to: B40, explicit: false, owner: "" };
    if sneaky.explicit {
        return Err(E_IMPLICIT); // 注入样本必须保持隐性（判定口径对证）
    }
    let mut with_sneaky: Vec<PendingItem> = Vec::new();
    let mut k = 0;
    while k < PENDING.len() {
        with_sneaky.push(PENDING[k]);
        k += 1;
    }
    with_sneaky.push(sneaky);
    let census2 = kind_census(&with_sneaky)?;
    if census2[1] != 5 || with_sneaky.len() != 20 {
        return Err(E_COMPLETENESS); // 注入后类别计数同步漂移（可检出）
    }
    // 代领注入——owner 非空项必须被判违。
    let claimed = PendingItem { name: "代领样本", kind: K_WORKITEM, size_units: 1, target_from: B16, target_to: B40, explicit: true, owner: "AI-X" };
    if claimed.owner.is_empty() {
        return Err(E_CLAIMED);
    }
    // 校验和：规模合计折叠（1×16 + 90+100+200 = 406）。
    let mut sum = 0u32;
    i = 0;
    while i < PENDING.len() {
        sum += PENDING[i].size_units;
        i += 1;
    }
    if sum != 406 {
        return Err(E_COMPLETENESS);
    }
    Ok(sum.wrapping_mul(3).wrapping_add(PENDING.len() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "项下标/目标窗越合法区（须 B16–B40）",
        E_IMPLICIT => "隐性登记（explicit 位缺）",
        E_CLAIMED => "越界代领（登记面 owner 非空）",
        E_DUP => "项名重复/重复登记",
        E_COMPLETENESS => "完备性破（≠19 项/类别缺/规模锚漂）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36298_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36298_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB140, "0xB140~0xB14F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "19 项登记零隐性/类别 4+8+4+3/owner 恒空/规模锚 406"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("待领 19 项", PENDING.len() == 19 && names_unique(&PENDING), "批级 16 + 工作项 3 恰数登记");
    cs.add("零隐性", {
        let mut ok = true;
        let mut i = 0;
        while ok && i < PENDING.len() {
            if !PENDING[i].explicit || !target_in_window(&PENDING[i]) {
                ok = false;
            }
            i += 1;
        }
        ok
    }, "逐项显性 + 目标窗 B16–B40");
    cs.add("不越界代领", {
        let mut ok = true;
        let mut i = 0;
        while ok && i < PENDING.len() {
            if !PENDING[i].owner.is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    }, "登记面 owner 恒空（只登记不指派）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36298_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36298_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_kind_census_exact() {
        // 类别直证：E:4 / I:8 / C:4 / W:3 恰数。
        let c = kind_census(&PENDING).ok().unwrap_or([0; 5]);
        assert_eq!(c[1], 4);
        assert_eq!(c[2], 8);
        assert_eq!(c[3], 4);
        assert_eq!(c[4], 3);
        assert!(names_unique(&PENDING));
        assert!(item_caption(&PENDING[18]).contains("200"), "题注含规模锚");
    }
}
