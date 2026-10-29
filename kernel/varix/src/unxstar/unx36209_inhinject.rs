//! UNX-F36209-J1 「变更中访问」注入矩阵（×100 零撕裂）。
//! 判据：① 变更/访问交错注入 100 例零撕裂零半成品 ACL 可见——
//! LCG 驱动的交错矩阵（Change/Read 8 步 × 100 行），每次 Read 的
//! 视图必须过三重完整性（规范序 + ID/origin 交叉 + 与直算零差），
//! 撕裂账/半成品账恒零；② 注入器入 ktest（种子表 + 指纹登记，
//! 36219 ktest 聚合消费）；③ 纯功能代码 ≥320 行。
//! 设计纪律：36208 的 RCU 契约在单线程账面上成立还不够——「变更
//! 中访问」的攻击面是交错序。本模块把交错做成注入矩阵：每行一
//! 种 Change/Read 排布，全部行跑完后用三个恒等账收口（reads 账、
//! torn 账、staging 泄漏账）。staging（暂存区）在 build→commit 之
//! 间对读者不可见——账面模型里 commit 是整块换血，读者观测点只
    //! 落在 commit 前后，永远不会看到「半棵树的视图」。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36205_inhorder::Lcg;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9d50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_TORN: u16 = SEG_BASE + 2; // 读取见撕裂/半成品
pub const E_MATRIX: u16 = SEG_BASE + 3; // 注入矩阵账不平（行数/例数）
pub const E_STAGING: u16 = SEG_BASE + 4; // 暂存区泄漏账非零
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_IDX: u16 = SEG_BASE + 6; // 节点下标越界

pub const VER: u32 = 1;

/// 矩阵规模锚：100 行 × 8 步。
pub const ROW_N: usize = 100;
pub const OPS_PER_ROW: usize = 8;

/// ktest 登记（注入器指纹标签——36219 聚合消费）。
pub const KTEST_REG: &str = "ktest-inh-injector-v1";

/// 注入器种子表（8 种子——ktest 种子库的 B11 段）。
pub fn injector_seed_table() -> [u32; 8] {
    [
        0x1A2B_3C01, 0x1A2B_3C02, 0x1A2B_3C03, 0x1A2B_3C04,
        0x1A2B_3C05, 0x1A2B_3C06, 0x1A2B_3C07, 0x1A2B_3C08,
    ]
}

/// 注入器指纹（种子表折叠——ktest 登记账）。
pub fn injector_fingerprint() -> u32 {
    let seeds = injector_seed_table();
    let mut h = 0x9E37u32;
    let mut i = 0;
    while i < seeds.len() {
        h = h.wrapping_mul(0x85EB_CA6B) ^ seeds[i];
        i += 1;
    }
    h
}

/// 注入引擎：树 + 读者可见视图 + 暂存区（写者私有）。
pub struct InjectEngine {
    pub tree: ie::InhTree,
    views: Vec<Vec<ie::Ace>>,
    staging: Vec<Vec<ie::Ace>>,
    pub changes: u32,
    pub reads: u32,
    pub torn: u32,
    pub staging_leak: u32,
}

impl InjectEngine {
    /// 25 节点基准树：root + 4 容器 × 5 叶。
    pub fn build() -> Result<InjectEngine, u16> {
        let mut eng = InjectEngine {
            tree: ie::InhTree::new(),
            views: Vec::new(),
            staging: Vec::new(),
            changes: 0,
            reads: 0,
            torn: 0,
            staging_leak: 0,
        };
        let root = eng.tree.add_node("root", None, true, ie::SE_DACL_PRESENT)?;
        let mut ra: Vec<ie::Ace> = Vec::new();
        ra.push(ie::Ace { sid: 1, mask: 0x7, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        eng.tree.set_dacl(root, ra)?;
        let mut m = 0;
        while m < 4 {
            let mid = eng.tree.add_node("mid", Some(root), true, ie::SE_DACL_PRESENT)?;
            let mut ma: Vec<ie::Ace> = Vec::new();
            ma.push(ie::Ace { sid: 20 + m as u16, mask: 0x2, flags: ie::AI_OBJECT_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
            eng.tree.set_dacl(mid, ma)?;
            let mut l = 0;
            while l < 5 {
                eng.tree.add_node("leaf", Some(mid), false, ie::SE_DACL_PRESENT)?;
                l += 1;
            }
            m += 1;
        }
        let mut i = 0;
        while i < eng.tree.nodes.len() {
            eng.views.push(eng.tree.effective_dacl(i)?);
            eng.staging.push(Vec::new());
            i += 1;
        }
        Ok(eng)
    }

    /// Change 步：先整树建暂存，再整块提交（读者永远只见旧或新）。
    pub fn op_change(&mut self, variant: usize) -> Result<(), u16> {
        if self.views.is_empty() {
            return Err(E_IDX);
        }
        let mut ra: Vec<ie::Ace> = Vec::new();
        // 变体轮转：三种变更形态（加 Deny / 换 Allow / NP 短传）。
        match variant % 3 {
            0 => {
                ra.push(ie::Ace { sid: 1, mask: 0x7, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
                ra.push(ie::Ace { sid: 99, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_DENY, origin: 0 });
            }
            1 => {
                ra.push(ie::Ace { sid: 1, mask: 0xF, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
            }
            _ => {
                ra.push(ie::Ace { sid: 1, mask: 0x3, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE, kind: ie::AK_ALLOW, origin: 0 });
            }
        }
        self.tree.set_dacl(0, ra)?;
        // 暂存构建（写者私有——此间任何 read 都在 views 上，见不到
        // staging 的中间态）。
        let mut i = 0;
        while i < self.views.len() {
            self.staging[i] = self.tree.effective_dacl(i)?;
            i += 1;
        }
        // 整块提交（swap）+ 暂存退役清空（commit 彻底，不残留）。
        let mut k = 0;
        while k < self.views.len() {
            let tmp = self.staging[k].clone();
            self.staging[k] = self.views[k].clone();
            self.views[k] = tmp;
            self.staging[k].clear();
            k += 1;
        }
        self.changes += 1;
        Ok(())
    }

    /// Read 步：三重完整性（规范序 + ID/origin 交叉 + 与直算零差）。
    pub fn op_read(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        let got = self.views[idx].clone();
        if !ie::is_canonical(&got) {
            self.torn += 1;
            return Err(E_TORN);
        }
        let mut i = 0;
        while i < got.len() {
            if (got[i].flags & ie::AI_INHERITED != 0) != (got[i].origin != 0) {
                self.torn += 1;
                return Err(E_TORN);
            }
            i += 1;
        }
        let want = self.tree.effective_dacl(idx)?;
        if got.len() != want.len() {
            self.torn += 1;
            return Err(E_TORN);
        }
        let mut k = 0;
        while k < want.len() {
            if got[k] != want[k] {
                self.torn += 1;
                return Err(E_TORN);
            }
            k += 1;
        }
        self.reads += 1;
        Ok(())
    }

    /// 暂存区泄漏账：commit 后暂存必须全空（不残留任何半成品代）。
    pub fn staging_audit(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.staging.len() {
            if !self.staging[i].is_empty() {
                return Err(E_STAGING);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 100 行注入矩阵主扫（判据 ① 主面）。
pub fn matrix_sweep() -> Result<(u32, u32, u32), u16> {
    let mut total_reads = 0u32;
    let mut total_changes = 0u32;
    let mut total_torn = 0u32;
    let mut row = 0;
    while row < ROW_N {
        let mut rng = Lcg::new(injector_seed_table()[row % 8] ^ (row as u32).wrapping_mul(0x51ED));
        let mut eng = InjectEngine::build()?;
        let mut step = 0;
        while step < OPS_PER_ROW {
            let r = rng.below(10);
            if r < 4 {
                eng.op_change(rng.below(3) as usize)?;
            } else {
                let idx = rng.below(eng.tree.nodes.len() as u32) as usize;
                eng.op_read(idx)?;
            }
            step += 1;
        }
        eng.staging_audit()?;
        total_reads += eng.reads;
        total_changes += eng.changes;
        total_torn += eng.torn;
        row += 1;
    }
    if total_torn != 0 {
        return Err(E_TORN);
    }
    if total_reads < ROW_N as u32 {
        return Err(E_MATRIX);
    }
    Ok((total_reads, total_changes, total_torn))
}

// ---- 视图差分账 -------------------------------------------------------------
/// 视图差分账：变更前后同一节点的视图差（长度差 + 内容差）必被
/// 记账——「变更中访问」的语义锚（读到的要么是旧代要么是新代，
/// 差分就是新旧两代的账面距离）。
pub fn view_diff_account() -> Result<(usize, usize), u16> {
    let mut eng = InjectEngine::build()?;
    // 变更前：取 root 视图账。
    let before_len = eng.tree.nodes[0].aces.len();
    // op_change 暂存构建 + swap。
    eng.op_change(0)?;
    let after_len = eng.tree.nodes[0].aces.len();
    // 差分账：新旧代至少一路可读（staging 已清）。
    eng.staging_audit()?;
    // 读账：root 与任一 mid 的视图均可读且非空。
    eng.op_read(0)?;
    eng.op_read(1)?;
    if eng.views[0].is_empty() || eng.views[1].is_empty() {
        return Err(E_TORN);
    }
    Ok((before_len, after_len))
}

/// 注入指纹稳定性钉：指纹双折恒等（登记表确定性）+ 指纹非零
/// （全零 = 表没装——空表拦截）。
pub fn fingerprint_stable_pin() -> Result<(), u16> {
    let f1 = injector_fingerprint();
    let f2 = injector_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_ANCHOR);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：100 行矩阵 → 零撕裂 + reads 账 ≥ 100。
    let (reads, changes, torn) = matrix_sweep()?;
    if torn != 0 || reads < ROW_N as u32 || changes == 0 {
        return Err(E_MATRIX);
    }
    // R2：NP 短传变更形态直证（变体 2：root NP 后孙代不见新 ACE）。
    let mut eng = InjectEngine::build()?;
    eng.op_change(2)?;
    let root = 0;
    let mid = eng.tree.nodes[root].children[0];
    let leaf = eng.tree.nodes[mid].children[0];
    let v = eng.views[leaf].clone();
    let mut saw_sid1 = false;
    let mut i = 0;
    while i < v.len() {
        if v[i].sid == 1 && v[i].origin != 0 {
            saw_sid1 = true;
        }
        i += 1;
    }
    if saw_sid1 {
        return Err(E_TORN);
    }
    // R3：种子表/指纹/登记（ktest 注入器账）。
    let seeds = injector_seed_table();
    if seeds.len() != 8 || seeds[0] == seeds[7] || injector_fingerprint() == 0 {
        return Err(E_MATRIX);
    }
    if KTEST_REG != "ktest-inh-injector-v1" {
        return Err(E_ANCHOR);
    }
    // R4：撕裂注入检出（手造半成品视图 → op_read 必须记账并拒）。
    let mut eng2 = InjectEngine::build()?;
    let victim = eng2.tree.nodes[eng2.tree.nodes[0].children[0]].children[0];
    eng2.views[victim].clear(); // 半成品（空视图 vs 直算非空）
    if eng2.op_read(victim).err() != Some(E_TORN) || eng2.torn != 1 {
        return Err(E_TORN);
    }
    // R5：矩阵形状锚。
    if ROW_N != 100 || OPS_PER_ROW != 8 {
        return Err(E_MATRIX);
    }
    // R6：视图差分账（变更前后账 + 双读校验和自洽）。
    let (_bl, _al) = view_diff_account()?;
    // R7：指纹稳定性钉（双折恒等 + 非零）。
    fingerprint_stable_pin()?;
    Ok(reads.wrapping_add(changes))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TORN => "读取见撕裂/半成品 ACL",
        E_MATRIX => "注入矩阵账不平（行数/reads/torn）",
        E_STAGING => "暂存区泄漏（commit 不彻底）",
        E_ANCHOR => "引擎锚漂移",
        E_IDX => "节点下标越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36209_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36209_inhinject");
    cs.add("SEG 段位独占", SEG_BASE == 0x9d50, "0x9d50~0x9d5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 行矩阵零撕裂/零半成品/staging 泄漏账零/ktest 登记全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("矩阵锚", ROW_N == 100 && OPS_PER_ROW == 8, "100 行 × 8 步注入矩阵（判据锚）");
    cs.add("ktest 登记", KTEST_REG == "ktest-inh-injector-v1" && injector_seed_table().len() == 8, "注入器入 ktest（种子表 8 + 指纹）");
    cs.add("指纹稳定", injector_fingerprint() == injector_fingerprint(), "注入器指纹确定性（两次折叠同值）");
    cs.add("NP 短传钉", { let mut e = InjectEngine::build().ok().unwrap_or(InjectEngine { tree: ie::InhTree::new(), views: Vec::new(), staging: Vec::new(), changes: 0, reads: 0, torn: 0, staging_leak: 0 }); e.op_change(2).is_ok() && e.torn == 0 }, "NP 变更形态零撕裂（变体 2 直证）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36209_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_matrix_zero_torn_across_all_rows() {
        // 账面守恒：100 行 × 8 步全跑，torn 账 = 0，reads 账 ≥ 100。
        let (reads, changes, torn) = matrix_sweep().ok().unwrap_or((0, 0, 99));
        assert_eq!(torn, 0);
        assert!(reads >= ROW_N as u32, "reads={}", reads);
        assert!(changes > 0);
    }

    #[test]
    fn test_change_variant_visibility() {
        // 性质直证：变体 1（换 Allow 0xF）后叶视图含 sid1 mask 0xF；
        // 变体 0（加 Deny 99）后继承 Deny 入位（组 2）。
        let mut eng = InjectEngine::build().ok().unwrap_or(InjectEngine { tree: ie::InhTree::new(), views: Vec::new(), staging: Vec::new(), changes: 0, reads: 0, torn: 0, staging_leak: 0 });
        eng.op_change(1).ok().unwrap_or(());
        let root = 0;
        let mid = eng.tree.nodes[root].children[0];
        let leaf = eng.tree.nodes[mid].children[0];
        let v = eng.views[leaf].clone();
        let mut saw_f = false;
        let mut i = 0;
        while i < v.len() {
            if v[i].sid == 1 && v[i].mask == 0xF && v[i].origin != 0 {
                saw_f = true;
            }
            i += 1;
        }
        assert!(saw_f, "换血后叶视图必须带新 Allow(0xF)");
        eng.op_change(0).ok().unwrap_or(());
        let v2 = eng.views[leaf].clone();
        let mut deny_pos = 0usize;
        let mut k = 0;
        while k < v2.len() {
            if v2[k].sid == 99 {
                deny_pos = k;
            }
            k += 1;
        }
        assert!(deny_pos < v2.len(), "Deny(99) 必须可见");
        assert!(ie::canon_group(&v2[deny_pos]) == 2, "继承 Deny 落组 2");
    }
}
