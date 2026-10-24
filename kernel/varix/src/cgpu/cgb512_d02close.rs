//! CGPU-F0512 · D02 组收口与 D03 移交（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - D02 组收口：16 项自检入树/抑制效果验证矩阵全绿/组双签/台账更新/
//!   向 D03 卡顿归因组移交（抖动可测可抑——归因组有了抑制手段与检测
//!   证据）。判据：入树、矩阵报告、双签、移交、判据。
//!
//! 设计纪律：
//! - 16 项自检入树：D02 组（F0497~F0512）16 单的自检结论逐条登记——
//!   每条（任务号/自检结论/证据指纹）；入树即纳入组收口树。
//! - 抑制效果验证矩阵：五源 × 三模式的 15 格矩阵——每格（抑制率，
//!   达标）；全绿才可收口。
//! - 组双签：两组员独立签核（角色互异 + 指纹为当次收口内容哈希 +
//!   无阻断项）——签核必须对当前内容哈希做比对（签的是"这一版"）。
//! - 移交包：D03 需要的四件（检测证据接口清单/抑制手段面/五源指纹
//!   表/开放缺陷清单）——交接完整性校验。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5270 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x5270，批十独占）。
pub const SEG_BASE: u16 = 0x5270;

pub const E_CL_TREE: u16 = SEG_BASE + 1; // 入树项数不足
pub const E_CL_MATRIX: u16 = SEG_BASE + 2; // 验证矩阵有红格
pub const E_CL_SIGN: u16 = SEG_BASE + 3; // 双签校验失败
pub const E_CL_HANDOVER: u16 = SEG_BASE + 4; // 移交件缺失
pub const E_CL_ROLE: u16 = SEG_BASE + 5; // 签核角色重复

/// D02 组任务号区间（F0497~F0512）。
pub const D02_RANGE: (u32, u32) = (497, 512);

/// 五源 × 三模式 = 15 格矩阵。
pub const FIVE_SOURCES: [&str; 5] = ["sched", "vblank", "power", "thermal", "driver"];
pub const THREE_MODES: [u8; 3] = [1, 2, 3];

/// FNV-1a 64（内容指纹）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 自检入树条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeItem {
    /// 任务号（如 497 表示 F0497）。
    pub task: u32,
    /// 自检结论（true=过）。
    pub ok: bool,
    /// 证据指纹。
    pub evid: u64,
}

/// 组收口树。
#[derive(Clone, Debug)]
pub struct CloseTree {
    items: Vec<TreeItem>,
}

impl CloseTree {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 入树：任务号必须落在 D02 区间且不得重号。
    pub fn enter(&mut self, item: TreeItem) -> Result<(), u16> {
        if item.task < D02_RANGE.0 || item.task > D02_RANGE.1 {
            return Err(E_CL_TREE);
        }
        if self.items.iter().any(|x| x.task == item.task) {
            return Err(E_CL_TREE);
        }
        self.items.push(item);
        Ok(())
    }

    /// 收口门槛：区间 16 单全部入树且全过。
    pub fn complete(&self) -> bool {
        if self.items.len() != 16 {
            return false;
        }
        for n in D02_RANGE.0..=D02_RANGE.1 {
            match self.items.iter().find(|x| x.task == n) {
                Some(x) if x.ok => {}
                _ => return false,
            }
        }
        true
    }

    /// 收口内容哈希（双签对象）。
    pub fn content_hash(&self) -> u64 {
        let mut bytes = Vec::new();
        for it in &self.items {
            bytes.extend_from_slice(&it.task.to_le_bytes());
            bytes.push(it.ok as u8);
            bytes.extend_from_slice(&it.evid.to_le_bytes());
        }
        fnv1a64(&bytes)
    }
}

/// 验证矩阵：15 格抑制率。
#[derive(Clone, Debug)]
pub struct VerifyMatrix {
    cells: Vec<(u8, usize, u64)>, // (mode, source_idx, 抑制率‰)
    target_pm: u64,
}

impl VerifyMatrix {
    pub fn new(target_pm: u64) -> Self {
        Self {
            cells: Vec::new(),
            target_pm,
        }
    }

    /// 填格：模式+源定位。
    pub fn put(&mut self, mode: u8, source: usize, rate_pm: u64) -> Result<(), u16> {
        if !THREE_MODES.contains(&mode) || source >= 5 {
            return Err(E_CL_MATRIX);
        }
        if let Some(slot) = self.cells.iter_mut().find(|(m, s, _)| *m == mode && *s == source) {
            slot.2 = rate_pm;
        } else {
            self.cells.push((mode, source, rate_pm));
        }
        Ok(())
    }

    /// 矩阵全绿：15 格齐且全部达标。
    pub fn all_green(&self) -> bool {
        if self.cells.len() != 15 {
            return false;
        }
        self.cells.iter().all(|(_, _, r)| *r >= self.target_pm)
    }
}

/// 双签账本。
#[derive(Clone, Debug)]
pub struct SignBook {
    signs: Vec<(&'static str, u64)>, // (角色, 签的收口内容哈希)
}

impl SignBook {
    pub fn new() -> Self {
        Self { signs: Vec::new() }
    }

    /// 签核：角色不得重复；指纹必须等于当次收口内容哈希（签"这一版"）。
    pub fn sign(&mut self, role: &'static str, content: u64, presented: u64) -> Result<(), u16> {
        if self.signs.iter().any(|(r, _)| *r == role) {
            return Err(E_CL_ROLE);
        }
        if content != presented {
            return Err(E_CL_SIGN);
        }
        self.signs.push((role, presented));
        Ok(())
    }

    /// 双签成立：两个不同角色且都签的当前哈希。
    pub fn double_signed(&self, content: u64) -> bool {
        self.signs.iter().filter(|(_, h)| *h == content).count() >= 2
            && self.signs.len() >= 2
    }
}

/// 移交包（D03 四件）。
#[derive(Clone, Debug, Default)]
pub struct HandoverPack {
    /// 检测证据接口清单（接口名）。
    pub detect_apis: Vec<&'static str>,
    /// 抑制手段面（策略名）。
    pub supp_strats: Vec<&'static str>,
    /// 五源指纹表（源→指纹）。
    pub source_fps: Vec<(&'static str, u64)>,
    /// 开放缺陷清单（缺陷号）。
    pub open_defects: Vec<u32>,
}

impl HandoverPack {
    /// 交接完整性：四件齐（接口/手段非空，指纹表恰 5 源全命名，缺陷清单非空）。
    pub fn handover_ok(&self) -> bool {
        !self.detect_apis.is_empty()
            && !self.supp_strats.is_empty()
            && self.source_fps.len() == 5
            && FIVE_SOURCES
                .iter()
                .all(|s| self.source_fps.iter().any(|(n, _)| n == s))
            && !self.open_defects.is_empty()
    }
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb512_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 1. 16 项自检入树：区间全号入树全过
    let mut tree = CloseTree::new();
    let mut ok_tree = true;
    for n in D02_RANGE.0..=D02_RANGE.1 {
        ok_tree &= tree.enter(TreeItem { task: n, ok: true, evid: 0x5212 + n as u64 }) == Ok(());
    }
    v.push(("入树-16项全号入树", ok_tree && tree.complete()));

    // 2. 区间外任务号拒
    v.push((
        "入树-区间外拒",
        tree.enter(TreeItem { task: 600, ok: true, evid: 0 }) == Err(E_CL_TREE)
            && tree.enter(TreeItem { task: 497, ok: true, evid: 1 }) == Err(E_CL_TREE),
    ));

    // 3. 验证矩阵全绿：15 格填满达标
    let mut mx = VerifyMatrix::new(600);
    for m in THREE_MODES {
        for (i, _) in FIVE_SOURCES.iter().enumerate() {
            let _ = mx.put(m, i, 700);
        }
    }
    v.push(("矩阵-15格全绿", mx.all_green()));

    // 4. 一格红即不可收口
    let mut mx2 = VerifyMatrix::new(600);
    for m in THREE_MODES {
        for (i, _) in FIVE_SOURCES.iter().enumerate() {
            let _ = mx2.put(m, i, 700);
        }
    }
    let _ = mx2.put(2, 3, 500); // 一格掉线
    v.push(("矩阵-红格阻断收口", !mx2.all_green()));

    // 5. 双签：两角色对同一内容哈希
    let h = tree.content_hash();
    let mut sb = SignBook::new();
    let s1 = sb.sign("组长", h, h) == Ok(());
    let s2 = sb.sign("QA", h, h) == Ok(());
    v.push(("双签-两角色同版本", s1 && s2 && sb.double_signed(h)));

    // 6. 签的必须当版：指纹对不上即拒
    let mut sb2 = SignBook::new();
    v.push((
        "双签-旧版指纹拒",
        sb2.sign("组长", h, h ^ 1) == Err(E_CL_SIGN),
    ));

    // 7. 角色互异：同角色二签拒
    let mut sb3 = SignBook::new();
    let _ = sb3.sign("组长", h, h);
    v.push(("双签-角色重复拒", sb3.sign("组长", h, h) == Err(E_CL_ROLE)));

    // 8. 移交包四件齐：D03 拿到检测证据/抑制手段/五源指纹/缺陷清单
    let hp = HandoverPack {
        detect_apis: vec!["supp_subscribe", "supp_verify"],
        supp_strats: vec!["deepbuf", "earlyren"],
        source_fps: FIVE_SOURCES.iter().map(|s| (*s, fnv1a64(s.as_bytes()))).collect(),
        open_defects: vec![2101],
    };
    v.push(("移交-四件齐备", hp.handover_ok()));

    v
}

pub fn run_cgb512_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb512");
    for (name, ok) in cgb512_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb512_raw_checks as raw;
    #[test]
    fn t_cgb512() {
        for (n, ok) in raw() {
            assert!(ok, "cgb512 check failed: {}", n);
        }
    }
}
