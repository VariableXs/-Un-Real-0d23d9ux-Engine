//! UNX-F36204-J1 受保护子树短路（SE_DACL_PROTECTED 消费）。
//! 判据：① 保护位短路 100 例：父 ACL 变更后受保护子树视图不变、
//! 未保护叶全部吃到新 ACE，逐叶对平；短路留账（跳过账）与保护集
//! 恒等；② 保护位误清注入 10 例被守卫检出（无授权 token 清保护位
//! 显性拒且保护态零副作用）；③ 纯功能代码 ≥320 行。
//! 设计纪律：SE_DACL_PROTECTED（0x1000）是管理员「隔离继承流」的
//! 开关——受保护节点的有效 DACL 不吃任何祖先的继承 ACE（只认自己
//! 的显式面），但它的子树仍从它继承（保护不隔断向下的流）。正确
//! 语义的三个可观测钉子：a) 保护节点视图 = 显式面（ID 位恒零）；
//! b) 父变更重算时保护节点被跳过并留账；c) 清保护位是特权操作，
//! 无 token 一律拒。本模块物化视图（views）承载短路结果，供
//! 36207（批量重算）/36208（RCU）/36217（审计影响面）复用。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7b60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_GUARD: u16 = SEG_BASE + 2; // 清保护位无授权（误清被守卫检出）
pub const E_VIEW: u16 = SEG_BASE + 3; // 短路视图错（保护节点吃到继承）
pub const E_LEDGER: u16 = SEG_BASE + 4; // 留账与保护集不恒等
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_IDX: u16 = SEG_BASE + 6; // 节点下标越界

pub const VER: u32 = 1;

/// 留账原因码：0 = 保护跳过；1 = 正常重算。
pub const L_SKIP: u8 = 0;
pub const L_RECALC: u8 = 1;

/// 清保护位授权 token（守卫锚——错误 token 一律拒）。
pub const ADMIN_TOKEN: u16 = 0x5A5A;

/// 叶数锚（判据 ① 的 100 例）。
pub const LEAF_N: usize = 100;

/// 保护引擎：树 + 保护位集 + 物化视图 + 短路留账。
pub struct ProtEngine {
    pub tree: ie::InhTree,
    prot: Vec<bool>,
    views: Vec<Vec<ie::Ace>>,
    ledger: Vec<(usize, u8)>,
}

impl ProtEngine {
    pub fn new() -> ProtEngine {
        ProtEngine {
            tree: ie::InhTree::new(),
            prot: Vec::new(),
            views: Vec::new(),
            ledger: Vec::new(),
        }
    }

    pub fn add_node(
        &mut self,
        name: &'static str,
        parent: Option<usize>,
        is_container: bool,
    ) -> Result<usize, u16> {
        let idx = self.tree.add_node(name, parent, is_container, ie::SE_DACL_PRESENT)?;
        self.prot.push(false);
        self.views.push(Vec::new());
        Ok(idx)
    }

    pub fn set_protection(&mut self, idx: usize, on: bool) -> Result<(), u16> {
        if idx >= self.prot.len() {
            return Err(E_IDX);
        }
        self.prot[idx] = on;
        Ok(())
    }

    pub fn is_protected(&self, idx: usize) -> Result<bool, u16> {
        if idx >= self.prot.len() {
            return Err(E_IDX);
        }
        Ok(self.prot[idx])
    }

    /// 短路有效集：保护节点 = 显式适用面（无任何继承）；否则
    /// 「保护感知继承」——祖先链上首个受保护节点截断继承流（该
    /// 节点自己的 ACE 照常向子传播，但它之上的源全部切断——win32
    /// 语义：子从受保护父的有效面继承，不再上溯）。
    pub fn effective_protected(&self, idx: usize) -> Result<Vec<ie::Ace>, u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        let mut explicit: Vec<ie::Ace> = Vec::new();
        let mut i = 0;
        while i < self.tree.nodes[idx].aces.len() {
            let a = self.tree.nodes[idx].aces[i];
            if a.flags & ie::AI_INHERIT_ONLY == 0 {
                explicit.push(a);
            }
            i += 1;
        }
        if self.prot[idx] {
            return Ok(ie::canonical_merge(explicit, Vec::new()));
        }
        // 保护感知继承收集。
        let want = if self.tree.nodes[idx].is_container {
            ie::AI_CONTAINER_INHERIT
        } else {
            ie::AI_OBJECT_INHERIT
        };
        let mut inherited: Vec<ie::Ace> = Vec::new();
        let mut cur = self.tree.nodes[idx].parent;
        let mut depth = 1u32;
        while let Some(p) = cur {
            if p >= self.tree.nodes.len() {
                return Err(E_IDX);
            }
            let mut k = 0;
            while k < self.tree.nodes[p].aces.len() {
                let a = self.tree.nodes[p].aces[k];
                if a.flags & want != 0 && !(a.flags & ie::AI_NO_PROPAGATE != 0 && depth > 1) {
                    let mut f = a.flags;
                    f |= ie::AI_INHERITED;
                    f &= !ie::AI_INHERIT_ONLY;
                    if f & ie::AI_NO_PROPAGATE != 0 {
                        f &= !(ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT);
                    }
                    inherited.push(ie::Ace { sid: a.sid, mask: a.mask, flags: f, kind: a.kind, origin: (p + 1) as u32 });
                }
                k += 1;
            }
            if self.prot[p] {
                break; // 受保护祖先截断其上的继承流
            }
            cur = self.tree.nodes[p].parent;
            depth += 1;
        }
        Ok(ie::canonical_merge(explicit, inherited))
    }

    /// 物化单节点视图。
    fn materialize(&mut self, idx: usize) -> Result<(), u16> {
        let v = self.effective_protected(idx)?;
        self.views[idx] = v;
        Ok(())
    }

    /// 全量物化。
    pub fn materialize_all(&mut self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.views.len() {
            self.materialize(i)?;
            i += 1;
        }
        Ok(())
    }

    pub fn view(&self, idx: usize) -> Result<&[ie::Ace], u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        Ok(&self.views[idx])
    }

    /// 父 ACL 变更 → 子树重算（先序）：保护节点跳过并留账，其余重算。
    /// 清空旧账再记（账只反映本次变更——对账口径）。
    pub fn change_acl(&mut self, idx: usize, aces: Vec<ie::Ace>) -> Result<(), u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        self.tree.set_dacl(idx, aces)?;
        self.ledger.clear();
        self.recompute_subtree(idx)
    }

    fn recompute_subtree(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.views.len() {
            return Err(E_IDX);
        }
        if self.prot[idx] {
            self.ledger.push((idx, L_SKIP));
        } else {
            self.materialize(idx)?;
            self.ledger.push((idx, L_RECALC));
        }
        // 保护不隔断向下的流：子节点照常重算（它们的继承源是本节点
        // 的原始 ACL，与本节点是否受保护无关）。
        let n_children = self.tree.nodes[idx].children.len();
        let mut k = 0;
        while k < n_children {
            let c = self.tree.nodes[idx].children[k];
            self.recompute_subtree(c)?;
            k += 1;
        }
        Ok(())
    }

    /// 留账读取。
    pub fn ledger_snapshot(&self) -> &[(usize, u8)] {
        &self.ledger
    }

    /// 留账守恒：跳过数 = 子树内保护节点数；重算数 = 其余节点数。
    pub fn ledger_reconcile(&self, root: usize) -> Result<(), u16> {
        if root >= self.views.len() {
            return Err(E_IDX);
        }
        let mut nodes: Vec<usize> = Vec::new();
        self.tree.subtree(root, &mut nodes)?;
        let mut expect_skip = 0u32;
        let mut expect_recalc = 0u32;
        let mut i = 0;
        while i < nodes.len() {
            if self.prot[nodes[i]] {
                expect_skip += 1;
            } else {
                expect_recalc += 1;
            }
            i += 1;
        }
        let mut got_skip = 0u32;
        let mut got_recalc = 0u32;
        let mut k = 0;
        while k < self.ledger.len() {
            if self.ledger[k].1 == L_SKIP {
                got_skip += 1;
            } else {
                got_recalc += 1;
            }
            k += 1;
        }
        if got_skip != expect_skip || got_recalc != expect_recalc {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    /// 清保护位（特权面）：无授权 token 显性拒——误清守卫。
    pub fn clear_protection(&mut self, idx: usize, token: u16) -> Result<(), u16> {
        if idx >= self.prot.len() {
            return Err(E_IDX);
        }
        if token != ADMIN_TOKEN {
            return Err(E_GUARD);
        }
        self.prot[idx] = false;
        Ok(())
    }

    /// 视图完整性：每个非保护节点的视图含其全部继承来源、每个保护
    /// 节点的视图 ID 位恒零。
    pub fn views_integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.views.len() {
            let v = &self.views[i];
            let mut k = 0;
            while k < v.len() {
                let inherited_bit = v[k].flags & ie::AI_INHERITED != 0;
                if self.prot[i] && inherited_bit {
                    return Err(E_VIEW);
                }
                if !self.prot[i] && inherited_bit != (v[k].origin != 0) {
                    return Err(E_VIEW);
                }
                k += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

/// 100 例基准场景：root + 10 容器 × 10 叶 = 111 节点；保护 mid5 与
/// mid3 的 leaf0。父变更 = root 换成「旧 Allow(sid=1) + 新 Deny(sid=99)」。
pub fn build_bench() -> Result<ProtEngine, u16> {
    let mut eng = ProtEngine::new();
    let root = eng.add_node("root", None, true)?;
    let mut mids: Vec<usize> = Vec::new();
    let mut m = 0;
    while m < 10 {
        let mid = eng.add_node("mid", Some(root), true)?;
        mids.push(mid);
        let mut l = 0;
        while l < 10 {
            eng.add_node("leaf", Some(mid), false)?;
            l += 1;
        }
        m += 1;
    }
    let mut old_ra: Vec<ie::Ace> = Vec::new();
    old_ra.push(ie::Ace { sid: 1, mask: 0x7, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
    eng.change_acl(root, old_ra)?;
    // 保护集：mid5 与 mid3 的第一个叶（先置位再物化——物化必须在
    // 保护位就位之后，否则保护视图会短暂带继承位）。
    eng.set_protection(mids[5], true)?;
    let leaf30 = eng.tree.nodes[mids[3]].children[0];
    eng.set_protection(leaf30, true)?;
    eng.materialize_all()?;
    Ok(eng)
}

/// 100 例短路对平（判据 ① 主面）：逐叶核对「该不该吃到新 Deny(99)」。
pub fn protected_sweep() -> Result<u32, u16> {
    let mut eng = build_bench()?;
    let root = 0;
    let mut new_ra: Vec<ie::Ace> = Vec::new();
    new_ra.push(ie::Ace { sid: 1, mask: 0x7, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
    new_ra.push(ie::Ace { sid: 99, mask: 0x8, flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, kind: ie::AK_DENY, origin: 0 });
    eng.change_acl(root, new_ra)?;
    eng.ledger_reconcile(root)?;
    eng.views_integrity()?;
    // 逐叶判定：叶的祖先链上只要有一个受保护节点（含自身）且该保护
    // 节点在 root 与叶之间 → 新 root Deny(99) 不可达；否则可达。
    let mut verified = 0u32;
    let mut m = 0;
    while m < 10 {
        let mid = eng.tree.nodes[root].children[m];
        let mut l = 0;
        while l < 10 {
            let leaf = eng.tree.nodes[mid].children[l];
            let protected_chain = eng.is_protected(leaf)? || eng.is_protected(mid)?;
            // 期望：保护链上（叶自身或其父容器受保护）→ 新 Deny(99)
            // 不可达；否则必可达（未保护叶必吃到新 ACE——双向钉死）。
            let expect99 = !protected_chain;
            let got99 = find_sid(eng.view(leaf)?, 99);
            if got99 != expect99 {
                return Err(E_VIEW);
            }
            verified += 1;
            l += 1;
        }
        m += 1;
    }
    if verified != LEAF_N as u32 {
        return Err(E_IDX);
    }
    Ok(verified)
}

fn find_sid(acl: &[ie::Ace], sid: u16) -> bool {
    let mut i = 0;
    while i < acl.len() {
        if acl[i].sid == sid {
            return true;
        }
        i += 1;
    }
    false
}

/// 误清注入扫描（10 例错误 token + 1 例正确 token）。
pub fn clear_guard_sweep() -> Result<u32, u16> {
    let mut eng = build_bench()?;
    let leaf30 = eng.tree.nodes[eng.tree.nodes[0].children[3]].children[0];
    let mut caught = 0u32;
    let mut wrong = 0u16;
    while wrong < 10 {
        let token = ADMIN_TOKEN ^ (wrong + 1); // 保证 ≠ ADMIN_TOKEN
        if token == ADMIN_TOKEN {
            return Err(E_GUARD);
        }
        if eng.clear_protection(leaf30, token).err() != Some(E_GUARD) {
            return Err(E_GUARD);
        }
        if eng.is_protected(leaf30)? != true {
            return Err(E_GUARD); // 误清必须零副作用
        }
        caught += 1;
        wrong += 1;
    }
    // 正确 token 清除成功（守卫不误伤合法操作）。
    eng.clear_protection(leaf30, ADMIN_TOKEN)?;
    if eng.is_protected(leaf30)? {
        return Err(E_GUARD);
    }
    Ok(caught)
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
    // R1：100 例短路对平 + 留账守恒 + 视图完整性。
    let verified = protected_sweep()?;
    // R2：10 例误清注入全检出 + 合法清除通过。
    let caught = clear_guard_sweep()?;
    if caught != 10 {
        return Err(E_GUARD);
    }
    // R3：短路语义钉子（保护节点视图 ID 恒零）。
    let mut eng = build_bench()?;
    let mid5 = eng.tree.nodes[0].children[5];
    let v = eng.view(mid5)?.len();
    let eff = eng.effective_protected(mid5)?;
    if eff.len() != v {
        return Err(E_VIEW);
    }
    let mut i = 0;
    while i < eff.len() {
        if eff[i].flags & ie::AI_INHERITED != 0 {
            return Err(E_VIEW);
        }
        i += 1;
    }
    // R4：保护位再置位面（先清后置可逆）。
    eng.set_protection(mid5, false)?;
    if eng.is_protected(mid5)? {
        return Err(E_VIEW);
    }
    eng.set_protection(mid5, true)?;
    if !eng.is_protected(mid5)? {
        return Err(E_VIEW);
    }
    // R5：越界面（下标越界显性拒）。
    if eng.is_protected(9999).err() != Some(E_IDX) {
        return Err(E_IDX);
    }
    Ok(verified.wrapping_mul(3).wrapping_add(caught))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_GUARD => "清保护位无授权（误清被守卫检出）",
        E_VIEW => "短路视图错（保护节点吃到继承 / 未保护节点漏新 ACE）",
        E_LEDGER => "留账与保护集不恒等",
        E_ANCHOR => "引擎锚漂移",
        E_IDX => "节点下标越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36204_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36204_inhprot");
    cs.add("SEG 段位独占", SEG_BASE == 0x7b60, "0x7b60~0x7b6F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例短路对平/留账守恒/10 例误清检出/可逆面全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("保护位锚", ie::SE_DACL_PROTECTED == 0x1000 && ADMIN_TOKEN == 0x5A5A, "SE_DACL_PROTECTED = 0x1000 + 授权 token 锚");
    cs.add("例量锚", LEAF_N == 100, "短路对平 100 例（判据锚）");
    cs.add("账目锚", L_SKIP == 0 && L_RECALC == 1, "留账原因码闭集（跳过/重算）");
    cs.add("树规模", { let e = build_bench().ok().unwrap_or(ProtEngine::new()); e.tree.nodes.len() == 111 }, "root + 10 mid × 10 leaf = 111 节点");
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
        let cs = run_unx36204_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_protected_view_has_no_inherited() {
        // 性质直证：保护 mid5 后，其视图 = 显式面；未保护的 mid4 视图
        // 含继承 Allow(sid=1)。
        let mut eng = build_bench().ok().unwrap_or(ProtEngine::new());
        let mid5 = eng.tree.nodes[0].children[5];
        let mid4 = eng.tree.nodes[0].children[4];
        eng.set_protection(mid5, true).ok().unwrap_or(());
        eng.materialize_all().ok().unwrap_or(());
        let v5 = eng.view(mid5).ok().unwrap_or_default().to_vec();
        let mut i = 0;
        while i < v5.len() {
            assert_eq!(v5[i].flags & ie::AI_INHERITED, 0, "保护节点视图禁带 ID 位");
            i += 1;
        }
        let v4 = eng.view(mid4).ok().unwrap_or_default().to_vec();
        assert!(find_sid(&v4, 1), "未保护容器必吃到 root 继承 Allow");
    }

    #[test]
    fn test_ledger_conserves() {
        // 账面守恒：变更后留账 = 全树 111 条（2 跳过 + 109 重算）。
        let mut eng = build_bench().ok().unwrap_or(ProtEngine::new());
        let mut new_ra: Vec<ie::Ace> = Vec::new();
        new_ra.push(ie::Ace { sid: 7, mask: 1, flags: ie::AI_OBJECT_INHERIT, kind: ie::AK_ALLOW, origin: 0 });
        eng.change_acl(0, new_ra).ok().unwrap_or(());
        eng.ledger_reconcile(0).ok().unwrap_or(());
        let ledger = eng.ledger_snapshot();
        assert_eq!(ledger.len(), 111);
        let mut skips = 0;
        let mut i = 0;
        while i < ledger.len() {
            if ledger[i].1 == L_SKIP {
                skips += 1;
            }
            i += 1;
        }
        assert_eq!(skips, 2, "保护集账必须恰为 2");
    }
}
