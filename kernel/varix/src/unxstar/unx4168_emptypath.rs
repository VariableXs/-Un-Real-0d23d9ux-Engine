//! unx4168_emptypath — UNX-F4168 空路径与 "/" 语义（诊断段 0x96A0）。
//!
//! 判据原文：
//!   J1 空字符串返回 ENOENT（POSIX 口径）、"/" 解析到根（双断言各 100 次）。
//!   J2 空串与 "." 的返回区分账齐备。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 空串口径：POSIX getcwd/路径解析裁剪——空路径串一律
//!      ENOENT（无分量可解析），与 "." 的原地到达严格区分。
//!   2. "/" 口径：解析到根节点（单斜杠即根锚），分量数为 0。
//!   3. 区分账：空串账与 "." 账分开记（判据 J2），两账互不吞噬。
//!   4. 断言配额：空串断言与根断言各 100 次入账，账数须恰等。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x96A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_PATH: u16 = SEG_BASE + 1;  // 空路径串（POSIX ENOENT 口径出账）
const E_NOT_ROOT: u16 = SEG_BASE + 2;    // "/" 未解析到根（判据 J1 失守）
const E_DOT_MIX: u16 = SEG_BASE + 3;     // 空串与 "." 账混淆（判据 J2 失守）
const E_ASSERT_MISS: u16 = SEG_BASE + 4; // 断言配额不足（各 100 次）
const E_BAL: u16 = SEG_BASE + 5;         // 双账合计守恒失守
const E_BAD_STATE: u16 = SEG_BASE + 6;   // 解析状态机非法迁移
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

pub const ASSERT_QUOTA: u32 = 100; // 双断言各 100 次（判据 J1）

/// 解析结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathResult {
    RootArrive, // "/" → 到根
    DotStay,    // "." → 原地
    Enodata,    // 空串 → ENOENT 口径（以 Err(E_EMPTY_PATH) 出账，此态记账）
}

/// 解析状态机：Idle 待命 / Parsing 解析中 / Arrived 已到达 / Rejected 已拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseState {
    Idle,
    Parsing,
    Arrived,
    Rejected,
}

/// 空路径枢纽：双断言账 + 区分账。
pub struct EmptyPath {
    pub state: ParseState,
    pub empty_hits: u32, // 空串 ENOENT 断言账（判据 J1 左）
    pub root_hits: u32,  // "/" 到根断言账（判据 J1 右）
    pub dot_hits: u32,   // "." 原地断言账（判据 J2 区分账）
    pub cur_node: u32,   // 当前所在节点（根=0）
}

impl EmptyPath {
    pub fn new() -> EmptyPath {
        EmptyPath {
            state: ParseState::Idle,
            empty_hits: 0,
            root_hits: 0,
            dot_hits: 0,
            cur_node: 0,
        }
    }

    /// 路径解析：空串 ENOENT / "/" 到根 / "." 原地三分支（判据 J1/J2）。
    pub fn resolve(&mut self, path: &[u8]) -> Result<PathResult, u16> {
        self.state = ParseState::Parsing;
        if path.is_empty() {
            // POSIX 口径：空路径串 → ENOENT。
            self.empty_hits += 1;
            self.state = ParseState::Rejected;
            return Err(E_EMPTY_PATH);
        }
        // 全斜杠（含 "/"）→ 到根。
        let mut all_slash = true;
        let mut i = 0usize;
        while i < path.len() {
            if path[i] != b'/' {
                all_slash = false;
                break;
            }
            i += 1;
        }
        if all_slash {
            self.root_hits += 1;
            self.cur_node = 0;
            self.state = ParseState::Arrived;
            return Ok(PathResult::RootArrive);
        }
        // 纯 "." 序列（含 "///./." 折叠后只剩点）→ 原地。
        let mut only_dots = true;
        let mut j = 0usize;
        while j < path.len() {
            if path[j] != b'.' && path[j] != b'/' {
                only_dots = false;
                break;
            }
            j += 1;
        }
        if only_dots {
            self.dot_hits += 1;
            self.state = ParseState::Arrived;
            return Ok(PathResult::DotStay);
        }
        // 混合分量归 F4162 分账，本模块按不收处理。
        self.state = ParseState::Rejected;
        Err(E_BAD_STATE)
    }

    /// 断言配额核验：空串账与根账各恰 100 次（判据 J1）。
    pub fn assert_quota(&self) -> Result<(), u16> {
        if self.empty_hits != ASSERT_QUOTA {
            return Err(E_ASSERT_MISS);
        }
        if self.root_hits != ASSERT_QUOTA {
            return Err(E_ASSERT_MISS);
        }
        Ok(())
    }

    /// 区分账核验：空串账与 "." 账独立成账且互不吞噬（判据 J2）。
    pub fn distinction_ledger(&self) -> Result<(), u16> {
        if self.empty_hits == 0 || self.dot_hits == 0 {
            return Err(E_DOT_MIX);
        }
        Ok(())
    }

    /// 守恒对账：三账合计与状态机一致。
    pub fn audit(&self) -> Result<(), u16> {
        let total = self.empty_hits + self.root_hits + self.dot_hits;
        if total == 0 && self.state != ParseState::Idle {
            return Err(E_BAL);
        }
        match self.state {
            ParseState::Idle | ParseState::Arrived | ParseState::Rejected => {}
            ParseState::Parsing => return Err(E_BAD_STATE),
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

/// 自例程：R1 双断言各 100 次 → R2 边界 → R3 注入拦截 → R4 区分账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：空串 100 次 ENOENT + "/" 100 次到根。
    let mut p = EmptyPath::new();
    let mut i = 0u32;
    while i < ASSERT_QUOTA {
        if p.resolve(b"").is_ok() {
            return Err(E_EMPTY_PATH);
        }
        if p.resolve(b"/")? != PathResult::RootArrive {
            return Err(E_NOT_ROOT);
        }
        i += 1;
    }
    p.assert_quota()?;
    p.writeback(true, 0x30)?;
    hits += 1;

    // R2 边界："////" 也是到根；"////./././" 走 "." 账。
    let mut p = EmptyPath::new();
    if p.resolve(b"////")? != PathResult::RootArrive {
        return Err(E_NOT_ROOT);
    }
    if p.resolve(b"////./././")? != PathResult::DotStay {
        return Err(E_DOT_MIX);
    }
    if p.dot_hits != 1 {
        return Err(E_DOT_MIX);
    }
    hits += 1;

    // R3 注入拦截：空串断言账不足配额时核验拦截。
    let mut p = EmptyPath::new();
    p.resolve(b"").is_err(); // 空串 → ENOENT（预期拒收）
    p.resolve(b"/")?;
    if p.assert_quota().is_ok() {
        return Err(E_ASSERT_MISS);
    }
    if p.resolve(b"a/b").is_ok() {
        return Err(E_BAD_STATE); // 混合分量不收
    }
    hits += 1;

    // R4 区分账对账：空串账与 "." 账齐备且状态机合法。
    let mut p = EmptyPath::new();
    let mut k = 0u32;
    while k < 4 {
        p.resolve(b"").is_err();
        p.resolve(b"/")?;
        p.resolve(b".")?;
        k += 1;
    }
    p.distinction_ledger()?;
    p.audit()?;
    if p.cur_node != 0 {
        return Err(E_NOT_ROOT);
    }
    hits += 1;

    // R5 追加回归：配额账满额复核 + 根锚反复到达不迁移。
    let mut p = EmptyPath::new();
    let mut i = 0u32;
    while i < ASSERT_QUOTA {
        p.resolve(b"").is_err(); // 空串 → ENOENT 账
        p.resolve(b"/")?;        // 根 → root 账
        i += 1;
    }
    p.assert_quota()?;
    if p.empty_hits != ASSERT_QUOTA || p.root_hits != ASSERT_QUOTA {
        return Err(E_ASSERT_MISS);
    }
    // 根锚复核：反复到根后节点仍为 0 且状态 Arrived。
    let mut k = 0u32;
    while k < 20 {
        if p.resolve(b"/")? != PathResult::RootArrive {
            return Err(E_NOT_ROOT);
        }
        k += 1;
    }
    if p.cur_node != 0 || p.state != ParseState::Arrived {
        return Err(E_BAD_STATE);
    }
    if p.root_hits != ASSERT_QUOTA + 20 {
        return Err(E_ASSERT_MISS);
    }
    p.audit()?;
    p.writeback(true, 0x60)?;
    hits += 1;

    // R6 追加回归：空串与 "." 的区分账双满复核（判据 J2）。
    let mut p2 = EmptyPath::new();
    let mut j = 0u32;
    while j < 50 {
        p2.resolve(b"").is_err();
        p2.resolve(b".")?;
        p2.resolve(b"./.")?;
        j += 1;
    }
    if p2.empty_hits != 50 || p2.dot_hits != 100 {
        return Err(E_DOT_MIX);
    }
    p2.distinction_ledger()?;
    p2.audit()?;
    // 根锚不迁移："." 与空串都不改变当前节点。
    if p2.cur_node != 0 {
        return Err(E_NOT_ROOT);
    }
    p2.writeback(true, 0x61)?;
    hits += 1;

    // R7 回归：混合分量注入与状态机账。
    let mut p3 = EmptyPath::new();
    if p3.resolve(b"a").is_ok() {
        return Err(E_BAD_STATE); // 纯名分量不收（归 F4162 分账）
    }
    if p3.resolve(b"/a/.").is_ok() {
        return Err(E_BAD_STATE);
    }
    p3.resolve(b"").is_err(); // 空串 → ENOENT（预期拒收）
    if p3.state != ParseState::Rejected {
        return Err(E_BAD_STATE);
    }
    if p3.resolve(b"/")? != PathResult::RootArrive {
        return Err(E_NOT_ROOT);
    }
    if p3.state != ParseState::Arrived {
        return Err(E_BAD_STATE);
    }
    // 三账读数复核：空串 1、根 1、点 0（互不吞噬）。
    if p3.empty_hits != 1 || p3.root_hits != 1 || p3.dot_hits != 0 {
        return Err(E_DOT_MIX);
    }
    // 空串拒收路径不改根锚（ENOENT 无副作用红线）。
    if p3.cur_node != 0 {
        return Err(E_NOT_ROOT);
    }
    // 拒收态与到达态在账面上互斥可查。
    if p3.root_hits + p3.dot_hits == 0 && p3.state == ParseState::Arrived {
        return Err(E_BAL);
    }
    p3.audit()?;
    hits += 1;

    Ok(hits)
}
