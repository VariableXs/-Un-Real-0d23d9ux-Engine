//! unx4170_pathmax — UNX-F4170 路径总长限制（PATH_MAX）（诊断段 0x96E0）。
//!
//! 判据原文：
//!   J1 总长超 PATH_MAX（4,096）注入返回 ENAMETOOLONG。
//!   J2 符号链接展开后超限场景（B10 联动预锚）行为账齐备。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. PATH_MAX = 4096 常量写死（POSIX/Linux 全路径上限），判定
//!      看整路径字节数（含分隔符），单分量上限归 F4169 分账。
//!   2. 拒收口径：总长 > PATH_MAX 一律 ENAMETOOLONG，拒收零副作用
//!      （解析账不变）。
//!   3. 符号链接展开预锚：每遇到链接分量把目标长度并入有效总长
//!      （B10 联动口径），展开后超限同样 ENAMETOOLONG 并入账。
//!   4. 展开预算：链接展开链深有界（防链接环），超预算判 ELOOP
//!      族错误（本模块闭集内独立码）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x96E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PATH_LONG: u16 = SEG_BASE + 1;  // 总长超 PATH_MAX（ENAMETOOLONG 口径）
const E_EMPTY_PATH: u16 = SEG_BASE + 2; // 空路径拒收
const E_LNK_LOOP: u16 = SEG_BASE + 3;   // 链接展开链深超预算（ELOOP 族）
const E_EXPAND_MISS: u16 = SEG_BASE + 4;// 展开超限场景账缺验（判据 J2 失守）
const E_BAL: u16 = SEG_BASE + 5;        // 拒收零副作用失守
const E_BAD_STATE: u16 = SEG_BASE + 6;  // 裁定状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// PATH_MAX = 4096（POSIX/Linux 全路径上限，常量写死）。
pub const PATH_MAX: usize = 4096;
/// 链接展开链深预算（Linux SYMLOOP_MAX 口径裁剪）。
pub const SYMLOOP_BUDGET: usize = 8;

/// 裁定状态机：Idle 待命 / Scanning 扫描中 / Expanding 链接展开 / Passed 通过 / Rejected 已拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathState {
    Idle,
    Scanning,
    Expanding,
    Passed,
    Rejected,
}

/// 链接表条目：链接名 → 目标字节长（长度口径，不存真字节）。
pub struct LinkEntry {
    pub comp_fp: u32,   // 分量指纹（FNV-1a）
    pub target_len: u32, // 目标字节长（展开并入口径）
}

/// PATH_MAX 裁定枢纽：总长账 + 链接展开预锚 + 拒收零副作用。
pub struct PathMaxRuler {
    links: Vec<LinkEntry>,
    pub state: PathState,
    pub pass_total: u32,     // 通过总数
    pub reject_long: u32,    // 超长拒收总数（判据 J1 面）
    pub expand_over: u32,    // 展开后超限账（判据 J2 面）
    pub loop_intercepts: u32,// 链接环拦截账
    pub last_effective: u32, // 最近一次有效总长（含展开）
}

fn fp_of(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x01000193);
        i += 1;
    }
    h
}

impl PathMaxRuler {
    pub fn new() -> PathMaxRuler {
        PathMaxRuler {
            links: Vec::new(),
            state: PathState::Idle,
            pass_total: 0,
            reject_long: 0,
            expand_over: 0,
            loop_intercepts: 0,
            last_effective: 0,
        }
    }

    /// 登记链接分量（分量指纹 + 目标长）。
    pub fn register_link(&mut self, comp: &[u8], target_len: u32) -> Result<(), u16> {
        if comp.is_empty() {
            return Err(E_EMPTY_PATH);
        }
        self.links.push(LinkEntry { comp_fp: fp_of(comp), target_len });
        Ok(())
    }

    fn link_target_len(&self, comp: &[u8]) -> Option<u32> {
        let f = fp_of(comp);
        let mut i = 0usize;
        while i < self.links.len() {
            if self.links[i].comp_fp == f {
                return Some(self.links[i].target_len);
            }
            i += 1;
        }
        None
    }

    /// 总长裁定：路径字节长 <= PATH_MAX 才通过（判据 J1）。
    pub fn rule(&mut self, path: &[u8]) -> Result<u32, u16> {
        if path.is_empty() {
            self.state = PathState::Rejected;
            return Err(E_EMPTY_PATH);
        }
        self.state = PathState::Scanning;
        let total = path.len() as u32;
        self.last_effective = total;
        if total > PATH_MAX as u32 {
            self.reject_long += 1;
            self.state = PathState::Rejected;
            return Err(E_PATH_LONG);
        }
        self.pass_total += 1;
        self.state = PathState::Passed;
        Ok(total)
    }

    /// 展开裁定：链接分量目标长并入有效总长后重裁（判据 J2 预锚）。
    /// 链深超 SYMLOOP_BUDGET 判 ELOOP 族（E_LNK_LOOP）。
    pub fn rule_with_expand(&mut self, path: &[u8], link_depth: usize) -> Result<u32, u16> {
        if path.is_empty() {
            self.state = PathState::Rejected;
            return Err(E_EMPTY_PATH);
        }
        if link_depth > SYMLOOP_BUDGET {
            self.loop_intercepts += 1;
            self.state = PathState::Rejected;
            return Err(E_LNK_LOOP);
        }
        self.state = PathState::Expanding;
        let mut total = path.len() as u32;
        // 分量扫描：链接分量目标长并入（预锚口径：每链接展开一次）。
        let mut i = 0usize;
        let mut comp_start = 0usize;
        while i <= path.len() {
            if i == path.len() || path[i] == b'/' {
                if i > comp_start {
                    let mut comp: Vec<u8> = Vec::new();
                    let mut k = comp_start;
                    while k < i {
                        comp.push(path[k]);
                        k += 1;
                    }
                    if let Some(t) = self.link_target_len(&comp) {
                        total += t;
                    }
                }
                comp_start = i + 1;
            }
            i += 1;
        }
        self.last_effective = total;
        if total > PATH_MAX as u32 {
            // 展开后超限：同样 ENAMETOOLONG，展开超限账单列（判据 J2）。
            self.expand_over += 1;
            self.state = PathState::Rejected;
            return Err(E_PATH_LONG);
        }
        self.pass_total += 1;
        self.state = PathState::Passed;
        Ok(total)
    }

    /// 拒收零副作用核验：通过账在拒收前后不变。
    pub fn verify_no_side_effect(&self, pass_before: u32) -> Result<(), u16> {
        if self.pass_total != pass_before {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 展开超限场景账核验：expand_over 非零（判据 J2 账面齐备）。
    pub fn expand_ledger(&self, want: u32) -> Result<(), u16> {
        if self.expand_over < want {
            return Err(E_EXPAND_MISS);
        }
        Ok(())
    }

    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// 守恒对账：状态机合法，通过账与拒收账互斥事件。
    pub fn audit(&self) -> Result<(), u16> {
        match self.state {
            PathState::Idle | PathState::Passed | PathState::Rejected => {}
            PathState::Scanning | PathState::Expanding => return Err(E_BAD_STATE),
        }
        if self.last_effective > PATH_MAX as u32 && self.state == PathState::Passed {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_BAD_STATE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 总长边界 → R2 边界扩展 → R3 展开超限账 → R4 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：恰 4096 通过 / 恰 4097 拒收 / 拒收零副作用。
    let mut r = PathMaxRuler::new();
    let ok_path = [b'a'; PATH_MAX];
    let t = r.rule(&ok_path)?;
    if t != PATH_MAX as u32 {
        return Err(E_BAL);
    }
    let pass_before = r.pass_total;
    let over_path = [b'b'; PATH_MAX + 1];
    if r.rule(&over_path).is_ok() {
        return Err(E_PATH_LONG);
    }
    r.verify_no_side_effect(pass_before)?;
    hits += 1;

    // R2 边界：空路径 / 链深预算拦截。
    let mut r = PathMaxRuler::new();
    if r.rule(b"").is_ok() {
        return Err(E_EMPTY_PATH);
    }
    if r.rule_with_expand(b"a/b", SYMLOOP_BUDGET + 1).is_ok() {
        return Err(E_LNK_LOOP);
    }
    if r.loop_intercepts != 1 {
        return Err(E_LNK_LOOP);
    }
    hits += 1;

    // R3 展开超限账：链接目标长并入后越过 4096 → ENAMETOOLONG + 账齐备。
    let mut r = PathMaxRuler::new();
    // 链接名 "L" 目标长 4090：路径 "/L/x" 原长 4，展开后 4094 <= 4096 通过。
    r.register_link(b"L", 4090)?;
    let t = r.rule_with_expand(b"/L/x", 1)?;
    if t != 4094 {
        return Err(E_EXPAND_MISS);
    }
    // 再深一层并入：展开后 4097 > 4096 → 拒收入账。
    if r.rule_with_expand(b"/L/L/x", 1).is_ok() {
        return Err(E_PATH_LONG);
    }
    if r.expand_over != 1 {
        return Err(E_EXPAND_MISS);
    }
    r.expand_ledger(1)?;
    r.writeback(true, 0x40)?;
    hits += 1;

    // R4 守恒对账：通过账与槽位账平 + 状态机合法。
    let mut r = PathMaxRuler::new();
    r.register_link(b"l1", 8)?;
    r.register_link(b"l2", 16)?;
    r.rule(b"/p/q")?;
    r.rule_with_expand(b"/l1/l2/end", 2)?;
    r.audit()?;
    if r.link_count() != 2 {
        return Err(E_BAL);
    }
    if r.pass_total != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 追加回归：链深恰预算放行 + 超预算拦截 + 拦截账读数。
    let mut r = PathMaxRuler::new();
    r.register_link(b"l", 4)?;
    // 恰预算（8 层）不判环：路径短小可过。
    let t = r.rule_with_expand(b"/l", SYMLOOP_BUDGET)?;
    if t != 6 {
        return Err(E_EXPAND_MISS);
    }
    // 超预算一层即拦（ELOOP 族）。
    if r.rule_with_expand(b"/l", SYMLOOP_BUDGET + 1).is_ok() {
        return Err(E_LNK_LOOP);
    }
    if r.loop_intercepts != 1 {
        return Err(E_LNK_LOOP);
    }
    r.audit()?;
    r.writeback(true, 0x28)?;
    hits += 1;

    Ok(hits)
}
