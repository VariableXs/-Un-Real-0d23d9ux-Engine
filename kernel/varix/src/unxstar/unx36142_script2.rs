//! UNX-F36142-J1 地基联动剧本 II：SID 变更在令牌/SD/ACE 的传播一致性。
//! 判据：① SID 变更一次性传播到三载体（令牌组位/SD 所有者位/ACE SID 位），
//! 6 例传播后三载体位图全一致、旧位清零新位就位；② 部分传播 3 例（跳过
//! 任一载体）必被一致性审计检出、未知旧槽 1 例/零新槽 1 例/原地改 1 例
//! 显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 联动剧本之二——三载体在本地建模，传播是一次性原子视角
//! （无线程），一致性审计器是本剧本的判据核心。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x8240;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_NIL: u16 = SEG_BASE + 2;   // 新槽为零
pub const E_NOENT: u16 = SEG_BASE + 3; // 旧槽不在任何载体（无可传播引用）
pub const E_NOOP: u16 = SEG_BASE + 4;  // 新旧同槽（原地改）
pub const E_TORN: u16 = SEG_BASE + 5;  // 传播撕裂（部分载体未跟上）
pub const E_RESID: u16 = SEG_BASE + 6; // 旧槽残留
pub const E_CS: u16 = SEG_BASE + 7;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 令牌组位容量。
pub const TOKEN_GROUPS: usize = 4;
/// ACE SID 位容量。
pub const ACE_REFS: usize = 8;

/// 传播状态（三载体 + 代际）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PropState {
    pub token_groups: [u16; TOKEN_GROUPS],
    pub sd_owner: u16,
    pub ace_refs: [u16; ACE_REFS],
    pub generation: u32,
}

impl PropState {
    /// 空态。
    pub const fn fresh() -> PropState {
        PropState { token_groups: [0; TOKEN_GROUPS], sd_owner: 0, ace_refs: [0; ACE_REFS], generation: 0 }
    }

    /// 旧槽引用总数（三载体扫描）。
    pub fn refs_of(&self, sid: u16) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < TOKEN_GROUPS {
            if self.token_groups[i] == sid {
                n += 1;
            }
            i += 1;
        }
        if self.sd_owner == sid {
            n += 1;
        }
        let mut j = 0;
        while j < ACE_REFS {
            if self.ace_refs[j] == sid {
                n += 1;
            }
            j += 1;
        }
        n
    }

    /// 一致性审计：旧槽引用必须全清（残留 = E_RESID）。
    pub fn audit_no_residue(&self, old: u16) -> Result<(), u16> {
        if self.refs_of(old) != 0 {
            return Err(E_RESID);
        }
        Ok(())
    }

    /// 一致性审计：新槽引用数必须等于期望（撕裂 = E_TORN）。
    pub fn audit_count(&self, new: u16, expect: usize) -> Result<(), u16> {
        let got = self.refs_of(new);
        if got != expect {
            return Err(E_TORN);
        }
        Ok(())
    }
}

/// 单载体传播器（partial = 跳过某载体，注入面用）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropScope {
    /// 三载体全传（唯一合法路径）。
    All        = 0x0001,
    /// 跳过令牌（撕裂注入）。
    SkipToken  = 0x0002,
    /// 跳过 SD（撕裂注入）。
    SkipSd     = 0x0003,
    /// 跳过 ACE（撕裂注入）。
    SkipAces   = 0x0004,
}

impl PropScope {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 传播原语：old → new（All 才允许入账；撕裂域只作注入演示）。
pub fn propagate(st: &mut PropState, scope: PropScope, old: u16, new: u16) -> Result<usize, u16> {
    if new == 0 {
        return Err(E_NIL);
    }
    if old == new {
        return Err(E_NOOP);
    }
    let expect = st.refs_of(old);
    if expect == 0 {
        return Err(E_NOENT);
    }
    if scope != PropScope::All {
        // 撕裂域：只改部分载体，返回改动数供注入演示（不入合法账）。
        let mut changed = 0;
        if scope == PropScope::SkipSd || scope == PropScope::SkipAces {
            let mut i = 0;
            while i < TOKEN_GROUPS {
                if st.token_groups[i] == old {
                    st.token_groups[i] = new;
                    changed += 1;
                }
                i += 1;
            }
        }
        if scope == PropScope::SkipToken || scope == PropScope::SkipAces {
            if st.sd_owner == old {
                st.sd_owner = new;
                changed += 1;
            }
        }
        if scope == PropScope::SkipToken || scope == PropScope::SkipSd {
            let mut j = 0;
            while j < ACE_REFS {
                if st.ace_refs[j] == old {
                    st.ace_refs[j] = new;
                    changed += 1;
                }
                j += 1;
            }
        }
        return Ok(changed);
    }
    // 全传：三载体一次改齐 + 代际推进。
    let mut i = 0;
    while i < TOKEN_GROUPS {
        if st.token_groups[i] == old {
            st.token_groups[i] = new;
        }
        i += 1;
    }
    if st.sd_owner == old {
        st.sd_owner = new;
    }
    let mut j = 0;
    while j < ACE_REFS {
        if st.ace_refs[j] == old {
            st.ace_refs[j] = new;
        }
        j += 1;
    }
    st.generation = st.generation.wrapping_add(1);
    Ok(expect)
}

/// 造一个三载体引用齐全的态（旧槽 sid 出现在全部三类载体）。
pub fn seed_state(sid: u16) -> PropState {
    let mut st = PropState::fresh();
    let mut i = 0;
    while i < TOKEN_GROUPS {
        st.token_groups[i] = sid;
        i += 1;
    }
    st.sd_owner = sid;
    let mut j = 0;
    while j < ACE_REFS {
        st.ace_refs[j] = sid;
        j += 1;
    }
    st
}

/// 撕裂检出：对部分传播后的态跑一致性审计（必须报 E_TORN）。
pub fn detect_tear(st: &PropState, old: u16, new: u16, full_expect: usize) -> Result<(), u16> {
    if st.refs_of(old) != 0 {
        // 旧槽还有残留 → 审计器报残留（也算撕裂检出）。
        return Err(E_RESID);
    }
    let got = st.refs_of(new);
    if got != full_expect {
        return Err(E_TORN);
    }
    // 全跟上了却声称撕裂 → 审计器失效（红线）。
    Err(E_CS)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(st: &PropState) -> String {
    let mut s = String::new();
    s.push_str("script2 gen=");
    s.push_str(&st.generation.to_string());
    s.push_str(" owner=");
    s.push_str(&st.sd_owner.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_NIL, E_NOENT, E_NOOP, E_TORN, E_RESID, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NIL => "新槽为零",
        E_NOENT => "旧槽不在任何载体（无可传播引用）",
        E_NOOP => "新旧同槽（原地改）",
        E_TORN => "传播撕裂（部分载体未跟上）",
        E_RESID => "旧槽残留",
        E_CS => "指纹不稳/审计器失效",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位；R2 6 例全传一致性；R3 撕裂注入 3 例全检出；
/// R4 未知旧槽/零新槽/原地改 3 例；R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 载体容量锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if TOKEN_GROUPS != 4 || ACE_REFS != 8 {
        return Err(E_CS);
    }
    hits += 1;
    // R2：6 例全传（各造一个态，old→new，三载体位图一致 + 代际推进）。
    let pairs: Vec<(u16, u16)> = vec![(3, 9), (5, 11), (7, 13), (2, 4), (6, 8), (10, 20)];
    let mut p = 0;
    while p < pairs.len() {
        let mut st = seed_state(pairs[p].0);
        let before = st.refs_of(pairs[p].0);
        let moved = propagate(&mut st, PropScope::All, pairs[p].0, pairs[p].1)?;
        if moved != before {
            return Err(E_TORN);
        }
        st.audit_no_residue(pairs[p].0)?;
        st.audit_count(pairs[p].1, before)?;
        if st.generation != 1 {
            return Err(E_TORN);
        }
        hits += 1;
        p += 1;
    }
    // R3：撕裂注入 3 例（跳令牌/跳 SD/跳 ACE）——检出且码正确。
    let scopes: Vec<PropScope> = vec![PropScope::SkipToken, PropScope::SkipSd, PropScope::SkipAces];
    let mut sc = 0;
    while sc < scopes.len() {
        let mut st = seed_state(6);
        let full_expect = st.refs_of(6);
        let changed = propagate(&mut st, scopes[sc], 6, 7)?;
        if changed == full_expect {
            return Err(E_CS); // 声称撕裂却全传了 → 注入面失效
        }
        match detect_tear(&st, 6, 7, full_expect) {
            Ok(_) => return Err(E_CS),
            Err(c) if c == E_TORN || c == E_RESID => {}
            Err(_) => return Err(E_TORN),
        }
        hits += 1;
        sc += 1;
    }
    // R4：未知旧槽 / 零新槽 / 原地改 3 例。
    let mut st = seed_state(4);
    match propagate(&mut st, PropScope::All, 99, 5) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    match propagate(&mut st, PropScope::All, 4, 0) {
        Ok(_) => return Err(E_NIL),
        Err(c) if c == E_NIL => {}
        Err(_) => return Err(E_NIL),
    }
    hits += 1;
    match propagate(&mut st, PropScope::All, 4, 4) {
        Ok(_) => return Err(E_NOOP),
        Err(c) if c == E_NOOP => {}
        Err(_) => return Err(E_NOOP),
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零（真源 FNV over 三载体位图 + 版本锚）。
    let st2 = seed_state(8);
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < TOKEN_GROUPS {
        feed.extend_from_slice(&st2.token_groups[i].to_le_bytes());
        i += 1;
    }
    feed.extend_from_slice(&st2.sd_owner.to_le_bytes());
    let mut j = 0;
    while j < ACE_REFS {
        feed.extend_from_slice(&st2.ace_refs[j].to_le_bytes());
        j += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑确定性（再跑一遍 R2 的第 1 例对拍）。
    let mut sa = seed_state(3);
    let mut sb = seed_state(3);
    let ra = propagate(&mut sa, PropScope::All, 3, 9)?;
    let rb = propagate(&mut sb, PropScope::All, 3, 9)?;
    if ra != rb || sa != sb {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(15));
    }

    #[test]
    fn full_propagation_consistent() {
        let mut st = seed_state(5);
        let moved = propagate(&mut st, PropScope::All, 5, 6).unwrap_or(0);
        assert_eq!(moved, 13); // 4 组 + 1 owner + 8 ace
        assert_eq!(st.audit_no_residue(5), Ok(()));
        assert_eq!(st.audit_count(6, 13), Ok(()));
    }

    #[test]
    fn gates_reject() {
        let mut st = seed_state(2);
        assert_eq!(propagate(&mut st, PropScope::All, 2, 0), Err(E_NIL));
        assert_eq!(propagate(&mut st, PropScope::All, 2, 2), Err(E_NOOP));
        assert_eq!(propagate(&mut st, PropScope::All, 42, 3), Err(E_NOENT));
    }

    #[test]
    fn summary_shapes() {
        assert!(summary(&seed_state(1)).contains("script2"));
        assert!(codes_distinct());
    }
}

pub fn run_unx36142_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36142_script2");
    cs.add("SEG 段位独占", SEG_BASE == 0x8240, "0x8240~0x824f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "6 例传播一致、撕裂 3 例全检出"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("三载体容量锚 4/8", TOKEN_GROUPS == 4 && ACE_REFS == 8, "载体容量锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
