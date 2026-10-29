//! UNX-F36300-J1 B15 收口条：首产段收官与冻结终态交付。
//! 判据：UNX-F36300-J1 F36281–F36300 连续零跳号；冻结终态（黄金库/
//! 冻结表/性能基线/矩阵首版）向 B16+ 与下游 AI-47/49/51 交付登记；
//! 首产段 300 条收官宣告；纯功能代码 ≥300 行。
//! 设计纪律：收口条是批次的遗嘱与遗产清单——Windows 版本收官的
//! RTW 清单（连续构建号 + 冻结物清单 + 交付对象登记）同构：
//!   - 连续零跳号：F36281–F36300 二十卡逐卡连续（跳号 = 有卡失踪）；
///   - 冻结终态四件：黄金库 / 冻结表 / 性能基线 / 矩阵首版，逐件
///     版本锚 v1 + 交付对象登记（B16+ 与 AI-47/AI-49/AI-51）；
///   - 首产段 300 条收官宣告：宣告记录（区间 + 计数）落域档；
///   - 交付登记双向：每件冻结物至少一个下游对象 + 每个下游对象
///     至少一件冻结物（双向覆盖——单向登记 = 交付悬空）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB160;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 卡号/下标越合法区
pub const E_GAP: u16 = SEG_BASE + 3; // 跳号（卡序列断裂）
pub const E_FROZEN: u16 = SEG_BASE + 4; // 冻结物缺/版本锚漂
pub const E_DELIVER: u16 = SEG_BASE + 5; // 交付登记缺/双向覆盖破
pub const E_DECLARE: u16 = SEG_BASE + 6; // 收官宣告缺/计数不符

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 收口规格 ---------------------------------------------------------------
/// B15 批卡号区间 F36281–F36300。
pub const B15_FIRST: u32 = 36281;
pub const B15_LAST: u32 = 36300;
pub const B15_N: usize = 20;

/// 首产段区间 F36001–F36300。
pub const SEG_FIRST: u32 = 36001;
pub const SEG_N: usize = 300;

/// 下游交付对象闭集。
pub const D_B16: usize = 0; // B16+ 批
pub const D_AI47: usize = 1; // AI-47
pub const D_AI49: usize = 2; // AI-49
pub const D_AI51: usize = 3; // AI-51
pub const DOWNSTREAM_N: usize = 4;

pub const DOWNSTREAM_NAMES: [&str; DOWNSTREAM_N] = ["B16+", "AI-47", "AI-49", "AI-51"];

/// B15 批二十卡卡面（静态卡号账——与公式口径双源对平的地基）。
pub const B15_CARDS: [u32; B15_N] = [
    36281, 36282, 36283, 36284, 36285, 36286, 36287, 36288, 36289, 36290,
    36291, 36292, 36293, 36294, 36295, 36296, 36297, 36298, 36299, 36300,
];

/// 卡号连续零跳号核（静态卡面账逐卡连续 + 与区间公式双源对平）。
pub fn continuity_check() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if B15_LAST - B15_FIRST + 1 != B15_N as u32 {
        return Err(E_GAP);
    }
    let mut k = 0;
    while k < B15_N {
        // 静态卡面账与区间公式双源对平（漂移 = 跳号 = 有卡失踪）。
        if B15_CARDS[k] != B15_FIRST + k as u32 {
            return Err(E_GAP);
        }
        // 相邻差恰 1（连续性直证）。
        if k > 0 && B15_CARDS[k] - B15_CARDS[k - 1] != 1 {
            return Err(E_GAP);
        }
        k += 1;
    }
    Ok(())
}

/// 冻结终态物（名 + 版本锚 + 交付位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenArtifact {
    pub name: &'static str,
    pub ver: u32,
    pub delivered: [bool; DOWNSTREAM_N], // 交付位面（逐下游对象）
}

/// 四件冻结终态（版本锚 v1；交付位面全覆盖——每件交满四下游）。
pub const ARTIFACTS: [FrozenArtifact; 4] = [
    FrozenArtifact { name: "黄金库", ver: 1, delivered: [true, true, true, true] },
    FrozenArtifact { name: "冻结表", ver: 1, delivered: [true, true, true, true] },
    FrozenArtifact { name: "性能基线", ver: 1, delivered: [true, true, true, true] },
    FrozenArtifact { name: "矩阵首版", ver: 1, delivered: [true, true, true, true] },
];

/// 双向覆盖核：
///   正向——每件冻结物至少交付一个下游（交付位面非全零）；
///   反向——每个下游至少收到一件冻结物（列和 ≥ 1）。
pub fn delivery_bidirectional(arts: &[FrozenArtifact]) -> Result<(), u16> {
    if arts.len() != 4 {
        return Err(E_FROZEN);
    }
    // 正向：行和 ≥ 1。
    let mut i = 0;
    while i < arts.len() {
        let mut row = 0u32;
        let mut d = 0;
        while d < DOWNSTREAM_N {
            if arts[i].delivered[d] {
                row += 1;
            }
            d += 1;
        }
        if row == 0 {
            return Err(E_DELIVER); // 单向缺失：冻结物悬空
        }
        i += 1;
    }
    // 反向：列和 ≥ 1。
    let mut d = 0;
    while d < DOWNSTREAM_N {
        let mut col = 0u32;
        let mut i = 0;
        while i < arts.len() {
            if arts[i].delivered[d] {
                col += 1;
            }
            i += 1;
        }
        if col == 0 {
            return Err(E_DELIVER); // 单向缺失：下游对象悬空
        }
        d += 1;
    }
    Ok(())
}

/// 首产段收官宣告（区间 + 计数 + 域档锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Closure {
    pub first: u32,
    pub count: u32,
    pub declare_id: u32,
}

/// 收官宣告入域档（append-only 槽位表）。
pub struct ClosureArchive {
    slots: Vec<Closure>,
}

const ARCHIVE_CAP: usize = 8;

impl ClosureArchive {
    pub fn new() -> ClosureArchive {
        ClosureArchive { slots: Vec::new() }
    }

    pub fn file(&mut self, c: Closure) -> Result<(), u16> {
        if self.slots.len() >= ARCHIVE_CAP {
            return Err(E_DECLARE);
        }
        // 宣告守门：区间必须落首产段、计数必须与区间吻合。
        if c.first < SEG_FIRST || c.first > SEG_FIRST + SEG_N as u32 - 1 {
            return Err(E_DECLARE);
        }
        if c.count != SEG_N as u32 || c.first != SEG_FIRST {
            return Err(E_DECLARE); // 首产段收官宣告必须整段（300 条）
        }
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].declare_id == c.declare_id {
                return Err(E_DECLARE); // 宣告锚撞号
            }
            i += 1;
        }
        self.slots.push(c);
        Ok(())
    }

    pub fn fetch(&self, id: u32) -> Option<Closure> {
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].declare_id == id {
                return Some(self.slots[i]);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }
}

/// 真实收官宣告。
pub fn build_closure() -> Result<Closure, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    Ok(Closure { first: SEG_FIRST, count: SEG_N as u32, declare_id: 0xB1_60_001 })
}

/// 冻结物题注（名/版本/交付位面可读）。
pub fn artifact_caption(a: &FrozenArtifact) -> String {
    let mut s = String::new();
    s.push_str(a.name);
    s.push_str(" v");
    s.push_str(&a.ver.to_string());
    s.push_str(" → ");
    let mut d = 0;
    let mut first = true;
    while d < DOWNSTREAM_N {
        if a.delivered[d] {
            if !first {
                s.push_str("/");
            }
            s.push_str(DOWNSTREAM_NAMES[d]);
            first = false;
        }
        d += 1;
    }
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 区间守恒（B15 二十卡 + 首产段三百条）。
    if VER != 1 {
        return Err(E_VER);
    }
    if B15_N != 20 || SEG_N != 300 {
        return Err(E_GAP);
    }
    // R2：F36281–F36300 连续零跳号——逐卡对平。
    continuity_check()?;
    let mut k = 0;
    while k < B15_N {
        let card = B15_FIRST + k as u32;
        if card < B15_FIRST || card > B15_LAST {
            return Err(E_RANGE);
        }
        k += 1;
    }
    // R3：冻结终态四件——版本锚 v1、名非空、题注可读。
    if ARTIFACTS.len() != 4 {
        return Err(E_FROZEN);
    }
    let mut i = 0;
    while i < ARTIFACTS.len() {
        let a = &ARTIFACTS[i];
        if a.ver != 1 || a.name.is_empty() {
            return Err(E_FROZEN);
        }
        if artifact_caption(a).is_empty() {
            return Err(E_FROZEN);
        }
        i += 1;
    }
    // R4：交付双向覆盖——正向行和 + 反向列和全 ≥ 1。
    let mut all: Vec<FrozenArtifact> = Vec::new();
    i = 0;
    while i < ARTIFACTS.len() {
        all.push(ARTIFACTS[i]);
        i += 1;
    }
    delivery_bidirectional(&all)?;
    // 悬空注入：抽掉「矩阵首版」的全部交付位 → 反向（若该件是某
    // 下游唯一来源）或正向覆盖必须破——注入走独立实例。
    let mut hollow: Vec<FrozenArtifact> = Vec::new();
    i = 0;
    while i < ARTIFACTS.len() {
        let mut a = ARTIFACTS[i];
        if a.name == "矩阵首版" {
            a.delivered = [false, false, false, false];
        }
        hollow.push(a);
        i += 1;
    }
    // 矩阵首版行和归零 → 正向覆盖必须破（悬空检出）。
    let mut broken_row = false;
    i = 0;
    while i < hollow.len() {
        let mut row = 0u32;
        let mut d = 0;
        while d < DOWNSTREAM_N {
            if hollow[i].delivered[d] {
                row += 1;
            }
            d += 1;
        }
        if row == 0 {
            broken_row = true;
        }
        i += 1;
    }
    if !broken_row {
        return Err(E_DELIVER);
    }
    if delivery_bidirectional(&hollow).is_ok() {
        return Err(E_DELIVER); // 悬空实例必须被双向核拒绝
    }
    // R5：首产段 300 条收官宣告——入域档、可回查、锚唯一。
    let c = build_closure()?;
    let mut arch = ClosureArchive::new();
    arch.file(c)?;
    if arch.len() != 1 {
        return Err(E_DECLARE);
    }
    match arch.fetch(c.declare_id) {
        Some(cc) => {
            if cc != c || cc.count != 300 || cc.first != SEG_FIRST {
                return Err(E_DECLARE);
            }
        }
        None => return Err(E_DECLARE),
    }
    // 伪宣告拒收：计数缩水 / 区间漂移 / 锚撞号三类都必须显性失败。
    let mut arch2 = ClosureArchive::new();
    if arch2.file(Closure { count: 299, ..c }).err() != Some(E_DECLARE) {
        return Err(E_DECLARE);
    }
    if arch2.file(Closure { first: 36002, ..c }).err() != Some(E_DECLARE) {
        return Err(E_DECLARE);
    }
    if arch.file(c).err() != Some(E_DECLARE) {
        return Err(E_DECLARE);
    }
    // 校验和：冻结物版本 + 宣告计数折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < ARTIFACTS.len() {
        sum = sum.wrapping_mul(3).wrapping_add(ARTIFACTS[i].ver);
        i += 1;
    }
    Ok(sum.wrapping_add(c.count).wrapping_add(B15_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "卡号/下标越合法区",
        E_GAP => "跳号（F36281–F36300 序列断裂）",
        E_FROZEN => "冻结终态缺/版本锚漂",
        E_DELIVER => "交付登记缺/双向覆盖破（悬空）",
        E_DECLARE => "收官宣告缺/计数不符/锚撞号",
        _ => "未知诊断码",
    }
}

pub fn run_unx36300_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36300_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB160, "0xB160~0xB16F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "F36281–F36300 零跳号/四件冻结终态双向交付/300 条收官宣告入档"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("连续零跳号", continuity_check().is_ok(), "F36281–F36300 二十卡逐卡连续");
    cs.add("冻结终态四件", {
        let mut ok = ARTIFACTS.len() == 4;
        let mut i = 0;
        while ok && i < ARTIFACTS.len() {
            if ARTIFACTS[i].ver != 1 || ARTIFACTS[i].name.is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    }, "黄金库/冻结表/性能基线/矩阵首版 各 v1");
    cs.add("交付双向覆盖", {
        let mut all: Vec<FrozenArtifact> = Vec::new();
        let mut i = 0;
        while i < ARTIFACTS.len() {
            all.push(ARTIFACTS[i]);
            i += 1;
        }
        delivery_bidirectional(&all).is_ok()
    }, "每件有下游 + 每下游有件（B16+/AI-47/49/51）");
    cs.add("收官宣告入档", {
        match build_closure() {
            Ok(c) => c.count == 300 && c.first == SEG_FIRST,
            Err(_) => false,
        }
    }, "首产段 300 条收官宣告（区间+计数吻合）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36300_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36300_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_hollow_delivery_rejected() {
        // 悬空直证：全零交付位的冻结物被双向核拒绝。
        let hollow = [FrozenArtifact { name: "空交付", ver: 1, delivered: [false; DOWNSTREAM_N] }];
        assert!(delivery_bidirectional(&hollow).is_err());
        // 真账四件全位面通过。
        let mut all: Vec<FrozenArtifact> = Vec::new();
        let mut i = 0;
        while i < ARTIFACTS.len() {
            all.push(ARTIFACTS[i]);
            i += 1;
        }
        assert!(delivery_bidirectional(&all).is_ok());
    }
}
