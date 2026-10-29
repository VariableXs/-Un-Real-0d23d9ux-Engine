//! UNX-F36212-J1 继承结果诊断账（这条 ACE 从哪继承来）。
//! 判据：① 继承溯源 100 例（来源父路径 + 标志链）100% 可读——
//! 每条继承 ACE 的账目含 owner→source 完整父路径（逐段父链可验）
//! 与 flag 链（原始位 → 副本位，NP 清位/IO 清位/ID 置位三条变换
//! 规则全核），文本渲染逐条非空；② 溯源缺失注入 10 例被质检拦截
//! （继承 ACE 无账 / 账路径断裂 / 伪来源三形态）；③ 纯功能 ≥320 行。
//! 设计纪律：继承引擎最经典的排障问题是「这条 ACE 哪来的」——答
//! 不出来等于继承语义失控。溯源账的三条硬规则：a) 每条继承 ACE
//! 必有账（缺账 = 质检红）；b) 路径必须逐段过父链验证（编不出来
//! 的路径 = 伪来源）；c) 标志链必须过三变换规则（副本位与原始位
//! 的关系是确定性的——对不上 = 账目造假）。文本可读性用渲染非空
//! + 关键字段在场钉死，不给「哑账」留门。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

pub const SEG_BASE: u16 = 0x9fa0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MISSING: u16 = SEG_BASE + 2; // 继承 ACE 无账（溯源缺失）
pub const E_PATH: u16 = SEG_BASE + 3; // 账路径断裂/伪来源
pub const E_FLAG: u16 = SEG_BASE + 4; // 标志链变换规则破
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 溯源例数锚（判据 ① 的 100 例）与缺失注入例数。
pub const TRACE_N: usize = 100;
pub const MISSING_N: usize = 10;

/// 单条溯源账。
#[derive(Clone, Debug)]
pub struct LedgerEntry {
    pub owner: usize,
    pub ace_pos: usize,
    pub source: usize,
    /// 来源父路径：owner → … → source（含两端，逐段父链可验）。
    pub path: Vec<usize>,
    /// 标志链：原始 flags → 副本 flags。
    pub flag_origin: u8,
    pub flag_final: u8,
    pub sid: u16,
    pub kind: u8,
}

impl LedgerEntry {
    /// 文本渲染（可读性面：来源路径 + 标志链全字段在场）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let _ = write!(s, "ace[pos={},sid={},kind={}] <- path[", self.ace_pos, self.sid, self.kind);
        let mut i = 0;
        while i < self.path.len() {
            if i > 0 {
                let _ = write!(s, "/");
            }
            let _ = write!(s, "n{}", self.path[i]);
            i += 1;
        }
        let _ = write!(s, "] flags {:#04x}->{:#04x}", self.flag_origin, self.flag_final);
        s
    }
}

/// 溯源账（整树）。
#[derive(Clone, Debug)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
}

/// 从 owner 沿父链走到 source（路径含两端；source 不在链上 = 伪来源）。
fn owner_to_source(t: &ie::InhTree, owner: usize, source: usize) -> Result<Vec<usize>, u16> {
    if owner >= t.nodes.len() || source >= t.nodes.len() {
        return Err(E_PATH);
    }
    let mut path: Vec<usize> = Vec::new();
    path.push(owner);
    let mut cur = t.nodes[owner].parent;
    let mut guard = 0usize;
    while let Some(p) = cur {
        path.push(p);
        if p == source {
            return Ok(path);
        }
        guard += 1;
        if guard > t.nodes.len() {
            return Err(E_PATH);
        }
        cur = t.nodes[p].parent;
    }
    Err(E_PATH)
}

/// 原始 ACE 查找（source 的显式面按 (sid, mask, kind) 首匹配）。
fn find_source_flags(t: &ie::InhTree, source: usize, sid: u16, mask: u32, kind: u8) -> Result<u8, u16> {
    let mut i = 0;
    while i < t.nodes[source].aces.len() {
        let a = &t.nodes[source].aces[i];
        if a.sid == sid && a.mask == mask && a.kind == kind {
            return Ok(a.flags);
        }
        i += 1;
    }
    Err(E_PATH)
}

/// 标志链变换规则核（确定性三规则）：
///   a) 副本置 ID；b) 副本清 IO；c) 原始带 NP → 副本清 OI|CI。
pub fn flag_chain_ok(origin: u8, final_f: u8) -> Result<(), u16> {
    if final_f & ie::AI_INHERITED == 0 {
        return Err(E_FLAG);
    }
    if final_f & ie::AI_INHERIT_ONLY != 0 {
        return Err(E_FLAG);
    }
    if origin & ie::AI_NO_PROPAGATE != 0 && final_f & (ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT) != 0 {
        return Err(E_FLAG);
    }
    // 副本不许凭空造继承位（OI/CI 只能来自原始）。
    let inherited_bits = ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT;
    if final_f & inherited_bits & !origin != 0 && origin & ie::AI_NO_PROPAGATE == 0 {
        return Err(E_FLAG);
    }
    Ok(())
}

/// 整树建账（判据 ① 主面载体）。
pub fn build_ledger(t: &ie::InhTree) -> Result<Ledger, u16> {
    let mut entries: Vec<LedgerEntry> = Vec::new();
    let mut n = 0;
    while n < t.nodes.len() {
        let acl = t.effective_dacl(n)?;
        let mut pos = 0;
        while pos < acl.len() {
            let a = &acl[pos];
            if a.origin != 0 {
                let source = (a.origin - 1) as usize;
                let path = owner_to_source(t, n, source)?;
                let flag_origin = find_source_flags(t, source, a.sid, a.mask, a.kind)?;
                flag_chain_ok(flag_origin, a.flags)?;
                entries.push(LedgerEntry {
                    owner: n,
                    ace_pos: pos,
                    source,
                    path,
                    flag_origin,
                    flag_final: a.flags,
                    sid: a.sid,
                    kind: a.kind,
                });
            }
            pos += 1;
        }
        n += 1;
    }
    Ok(Ledger { entries })
}

/// 质检：node 的 acl 中每条继承 ACE 必有账、账路径可验、标志链合规。
pub fn qc_validate(t: &ie::InhTree, node: usize, acl: &[ie::Ace], ledger: &Ledger) -> Result<(), u16> {
    let mut pos = 0;
    while pos < acl.len() {
        let a = &acl[pos];
        if a.origin != 0 {
            // 找账。
            let mut found: Option<&LedgerEntry> = None;
            let mut i = 0;
            while i < ledger.entries.len() {
                let e = &ledger.entries[i];
                if e.owner == node && e.ace_pos == pos && e.sid == a.sid && e.kind == a.kind {
                    found = Some(e);
                }
                i += 1;
            }
            let e = found.ok_or(E_MISSING)?;
            // 路径逐段父链验证。
            if e.path.len() < 2 || e.path[0] != node {
                return Err(E_PATH);
            }
            let mut k = 1;
            while k < e.path.len() {
                let prev = e.path[k - 1];
                let cur = e.path[k];
                if prev >= t.nodes.len() || t.nodes[prev].parent != Some(cur) {
                    return Err(E_PATH);
                }
                k += 1;
            }
            if e.path[e.path.len() - 1] != e.source {
                return Err(E_PATH);
            }
            flag_chain_ok(e.flag_origin, e.flag_final)?;
        }
        pos += 1;
    }
    Ok(())
}

/// 20 棵溯源树（复用 36201 基准规格——来源链覆盖 1..2 层祖先）。
fn trace_tree(case: usize) -> Result<ie::InhTree, u16> {
    let (t, _leaf0, _mid0) = ie::bench_tree(case % ie::BENCH_N)?;
    Ok(t)
}

/// 100 例溯源主扫（判据 ① 主面）。
pub fn trace_sweep() -> Result<u32, u16> {
    let mut traced = 0u32;
    let mut case = 0;
    while case < 40 {
        let t = trace_tree(case)?;
        let ledger = build_ledger(&t)?;
        let mut n = 0;
        while n < t.nodes.len() {
            let acl = t.effective_dacl(n)?;
            qc_validate(&t, n, &acl, &ledger)?;
            // 可读性：每条账渲染非空且关键字段在场。
            let mut i = 0;
            while i < ledger.entries.len() {
                if ledger.entries[i].owner == n {
                    let txt = ledger.entries[i].text();
                    if txt.is_empty() || !txt.contains("path[") || !txt.contains("flags 0x") {
                        return Err(E_MISSING);
                    }
                    traced += 1;
                }
                i += 1;
            }
            n += 1;
        }
        case += 1;
    }
    if traced < TRACE_N as u32 {
        return Err(E_INTG);
    }
    Ok(traced)
}

/// 溯源缺失注入（10 例）：三种形态——无账 1 / 路径断裂 4 / 伪来源 5。
pub fn missing_sweep() -> Result<u32, u16> {
    let mut caught = 0u32;
    let t = trace_tree(2)?;
    let acl = t.effective_dacl(2)?; // mid0：必含继承 ACE
    // 定位首条继承 ACE（账查找按 (owner, ace_pos, sid, kind) 对位）。
    let mut pos_inh = acl.len();
    let mut i = 0;
    while i < acl.len() {
        if acl[i].origin != 0 {
            pos_inh = i;
            i = acl.len();
        } else {
            i += 1;
        }
    }
    if pos_inh == acl.len() {
        return Err(E_INTG);
    }
    let sid = acl[pos_inh].sid;
    let kind = acl[pos_inh].kind;
    // 形态 A（无账）：空账质检必红。
    let empty_ledger = Ledger { entries: Vec::new() };
    if qc_validate(&t, 2, &acl, &empty_ledger).err() != Some(E_MISSING) {
        return Err(E_MISSING);
    }
    caught += 1;
    // 形态 B（路径断裂）×4：第二跳伪造（2 的父是 1，不是 7..10）。
    let mut k = 0;
    while k < 4 {
        let mut broken = Ledger { entries: Vec::new() };
        broken.entries.push(LedgerEntry {
            owner: 2,
            ace_pos: pos_inh,
            source: 0,
            path: vec2(2, 7 + k),
            flag_origin: 0,
            flag_final: ie::AI_INHERITED,
            sid,
            kind,
        });
        if qc_validate(&t, 2, &acl, &broken).err() != Some(E_PATH) {
            return Err(E_PATH);
        }
        caught += 1;
        k += 1;
    }
    // 形态 C（伪来源）×5：source 不在 owner 的祖先链上。
    let mut k = 0;
    while k < 5 {
        let mut fake = Ledger { entries: Vec::new() };
        fake.entries.push(LedgerEntry {
            owner: 2,
            ace_pos: pos_inh,
            source: 3 + k,
            path: vec2(2, 3 + k),
            flag_origin: 0,
            flag_final: ie::AI_INHERITED,
            sid,
            kind,
        });
        if qc_validate(&t, 2, &acl, &fake).err() != Some(E_PATH) {
            return Err(E_PATH);
        }
        caught += 1;
        k += 1;
    }
    Ok(caught)
}

fn vec2(a: usize, b: usize) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    v.push(a);
    v.push(b);
    v
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
    // R1：100 例溯源全可读 + 质检全过。
    let traced = trace_sweep()?;
    // R2：10 例缺失注入全拦（三形态）。
    let caught = missing_sweep()?;
    if caught != MISSING_N as u32 {
        return Err(E_MISSING);
    }
    // R3：标志链规则钉子（三变换各一钉 + 造位拒）。
    flag_chain_ok(ie::AI_OBJECT_INHERIT | ie::AI_NO_PROPAGATE, ie::AI_NO_PROPAGATE | ie::AI_INHERITED)?;
    flag_chain_ok(ie::AI_CONTAINER_INHERIT | ie::AI_INHERIT_ONLY, ie::AI_CONTAINER_INHERIT | ie::AI_INHERITED)?;
    flag_chain_ok(0, ie::AI_INHERITED)?;
    if flag_chain_ok(0, ie::AI_OBJECT_INHERIT).is_ok() {
        return Err(E_FLAG); // 副本缺 ID
    }
    if flag_chain_ok(0, ie::AI_INHERITED | ie::AI_INHERIT_ONLY).is_ok() {
        return Err(E_FLAG); // 副本留 IO
    }
    if flag_chain_ok(ie::AI_OBJECT_INHERIT | ie::AI_NO_PROPAGATE, ie::AI_OBJECT_INHERIT | ie::AI_NO_PROPAGATE | ie::AI_INHERITED).is_ok() {
        return Err(E_FLAG); // NP 后副本仍带 OI
    }
    // R4：渲染可读钉（真实账目渲染含全部关键字段）。
    let t = trace_tree(0)?;
    let ledger = build_ledger(&t)?;
    if ledger.entries.is_empty() {
        return Err(E_MISSING);
    }
    let txt = ledger.entries[0].text();
    if !txt.contains("sid=") || !txt.contains("n0") || !txt.contains("->") {
        return Err(E_MISSING);
    }
    Ok(traced.wrapping_mul(3).wrapping_add(caught))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MISSING => "继承 ACE 无账（溯源缺失）",
        E_PATH => "账路径断裂/伪来源",
        E_FLAG => "标志链变换规则破",
        E_ANCHOR => "引擎锚漂移",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36212_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36212_inhdiag");
    cs.add("SEG 段位独占", SEG_BASE == 0x9fa0, "0x9fa0~0x9faF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例溯源可读/10 例缺失全拦/标志链三规则/渲染钉全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", TRACE_N == 100 && MISSING_N == 10, "100 例溯源 + 10 例缺失注入（判据锚）");
    cs.add("变换规则锚", flag_chain_ok(0, ie::AI_INHERITED).is_ok() && flag_chain_ok(0, 0).is_err(), "副本置 ID 是标志链第一规则");
    cs.add("渲染面", { let t = trace_tree(0).ok().unwrap_or(ie::InhTree::new()); let lg = build_ledger(&t).ok().unwrap_or(Ledger { entries: Vec::new() }); lg.entries.len() > 0 && lg.entries[0].text().contains("path[") }, "账目渲染含来源路径段");
    cs.add("越界面", owner_to_source(&{ let (t, _, _) = ie::bench_tree(0).ok().unwrap_or((ie::InhTree::new(), 0, 0)); t }, 0, 9_999).is_err(), "伪来源显性拒（source 不在链上）");
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
        let cs = run_unx36212_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_path_is_real_parent_chain() {
        // 性质直证：case 2 的 leaf0 账目路径必须逐段是真实父链
        // （leaf0 → mid0 → root 或更短），不是编出来的序列。
        let (t, leaf0, _) = ie::bench_tree(2).ok().unwrap_or((ie::InhTree::new(), 0, 0));
        let ledger = build_ledger(&t).ok().unwrap_or(Ledger { entries: Vec::new() });
        let mut checked = 0;
        let mut i = 0;
        while i < ledger.entries.len() {
            let e = &ledger.entries[i];
            if e.owner == leaf0 {
                assert_eq!(e.path[0], leaf0, "路径必须从 owner 起步");
                let mut k = 1;
                while k < e.path.len() {
                    assert_eq!(t.nodes[e.path[k - 1]].parent, Some(e.path[k]), "逐段父链");
                    k += 1;
                }
                checked += 1;
            }
            i += 1;
        }
        assert!(checked > 0, "case2 leaf0 必有继承账");
    }

    #[test]
    fn test_missing_accounts_conserve() {
        // 账面守恒：缺失注入拦截账 = 10（无账 1 + 断裂 4 + 伪来源 5）。
        assert_eq!(missing_sweep().ok().unwrap_or(0), MISSING_N as u32);
    }
}
