//! unx4143_dopsfreeze — UNX-F4143 dentry_operations 语义表逐函数冻结（诊断段 0x93A0）。
//!
//! 判据原文：UNX-F4143-J1 d_op 全函数清单（d_revalidate/d_hash/d_compare/d_delete/
//!           d_release/d_iput 6 函数）逐函数断言通过，返回值三态约定（信任/失效/错误）
//!           账齐。
//!   J2 允许失效态的行恰 3 行（revalidate/compare/delete）。
//!   J3 void 函数（release/iput）只允许信任态。
//!   J4 冻结后篡改元数必令哈希失配。
//! 设计纪律：
//!   1. 三态是本模块的唯一裁决语言：Trust（信任，缓存可用）、Invalid（失效，须重解析）、
//!      Error（错误，带回负 errno）。每个函数允许哪几态由冻结表以位掩码写死，
//!      越界态一律拒，不做「按语义猜」的宽容。
//!   2. 不可返回失效态的函数（d_hash/d_release/d_iput）若返回 Invalid，属语义错位：
//!      调用方会据此丢弃一个本来合法的 dentry，代价是解析风暴。故此类注入必须红。
//!   3. void 型函数（d_release/d_iput）只允许 Trust，其「返回」在账上折算为「已完成」，
//!      不得因无返回值就跳过记账。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x93A0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x93A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TRI: u16 = SEG_BASE + 1; // 返回态不在该函数允许的掩码内
const E_ARGC: u16 = SEG_BASE + 2; // 元数与冻结值不符
const E_COUNT: u16 = SEG_BASE + 3; // 函数数不是 6
const E_HASH: u16 = SEG_BASE + 4; // 快照哈希不匹配
const E_FROZEN: u16 = SEG_BASE + 5; // 冻结后改写或未断言即冻结
const E_IDX: u16 = SEG_BASE + 6; // 函数序号越界
const E_ACCT: u16 = SEG_BASE + 7; // 三态账不平
const E_VOID: u16 = SEG_BASE + 8; // void 函数被当作有返回值使用

/// 冻结函数数（判据原文 6）。
pub const FN_COUNT: usize = 6;
/// 三态掩码：Trust=1，Invalid=2，Error=4。
pub const M_TRUST: u8 = 0x1;
pub const M_INVALID: u8 = 0x2;
pub const M_ERROR: u8 = 0x4;

/// 返回三态（判据原文「信任/失效/错误」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriState {
    Trust = 0,
    Invalid = 1,
    Error = 2,
}

impl TriState {
    /// 态 → 掩码位。
    pub fn mask(self) -> u8 {
        match self {
            TriState::Trust => M_TRUST,
            TriState::Invalid => M_INVALID,
            TriState::Error => M_ERROR,
        }
    }

    pub fn from_code(c: u8) -> Option<TriState> {
        match c {
            0 => Some(TriState::Trust),
            1 => Some(TriState::Invalid),
            2 => Some(TriState::Error),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TriState::Trust => "trust",
            TriState::Invalid => "invalid",
            TriState::Error => "error",
        }
    }
}

/// d_op 编号（6 元闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DopId {
    Revalidate = 0,
    Hash = 1,
    Compare = 2,
    Delete = 3,
    Release = 4,
    Iput = 5,
}

/// 冻结相位机：Draft → Asserted → Frozen；Broken 为断言失败终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    Draft,
    Asserted,
    Frozen,
    Broken,
}

/// 冻结行：名 + 元数 + 允许态掩码 + 是否 void。
#[derive(Clone, Copy)]
pub struct DopRow {
    /// 函数名
    pub name: &'static str,
    /// 元数（文件外有效参数）
    pub argc: u8,
    /// 允许的返回态掩码
    pub allow: u8,
    /// 是否 void 型（只允许 Trust）
    pub is_void: bool,
}

/// 冻结清单（来源：Linux v5.10 include/linux/dcache.h struct dentry_operations）。
pub const CANON: [DopRow; FN_COUNT] = [
    // d_revalidate 可答「信任/失效/错误」三态
    DopRow { name: "d_revalidate", argc: 2, allow: M_TRUST | M_INVALID | M_ERROR, is_void: false },
    // d_hash 只能成功或报错（不允许「失效」——哈希不出失效语义）
    DopRow { name: "d_hash", argc: 3, allow: M_TRUST | M_ERROR, is_void: false },
    // d_compare 用「信任=相等、失效=不等」表达比较结果，错误态另立
    DopRow { name: "d_compare", argc: 4, allow: M_TRUST | M_INVALID | M_ERROR, is_void: false },
    // d_delete 用「信任=保留、失效=删除」
    DopRow { name: "d_delete", argc: 2, allow: M_TRUST | M_INVALID, is_void: false },
    // d_release / d_iput 为 void：只允许「已完成」
    DopRow { name: "d_release", argc: 2, allow: M_TRUST, is_void: true },
    DopRow { name: "d_iput", argc: 3, allow: M_TRUST, is_void: true },
];

/// 三态账：每态计数 + 越界拒收。
pub struct TriLedger {
    /// Trust 出现次数
    pub trust: u32,
    /// Invalid 出现次数
    pub invalid: u32,
    /// Error 出现次数
    pub error: u32,
    /// 越界态拒收次数
    pub illegal: u32,
}

impl TriLedger {
    pub fn new() -> TriLedger {
        TriLedger {
            trust: 0,
            invalid: 0,
            error: 0,
            illegal: 0,
        }
    }

    /// 记账：合法态 +1，非法态只记拒收账。
    pub fn note(&mut self, s: TriState, allowed: bool) {
        if !allowed {
            self.illegal += 1;
            return;
        }
        match s {
            TriState::Trust => self.trust += 1,
            TriState::Invalid => self.invalid += 1,
            TriState::Error => self.error += 1,
        }
    }

    /// 三态守恒：三数之和 == 合法裁决次数。
    pub fn total_legal(&self) -> u32 {
        self.trust + self.invalid + self.error
    }
}

/// d_op 语义表：规格表 + 断言账 + 三态账 + 快照。
pub struct DopsFreeze {
    /// 规格表
    pub rows: Vec<DopRow>,
    /// 相位
    pub state: FreezeState,
    /// 断言通过次数
    pub asserts_ok: u32,
    /// 三态账
    pub tri: TriLedger,
    /// 快照哈希
    pub hash: u32,
    /// 快照条数
    pub snap_count: u8,
    /// 是否已封存
    pub frozen: bool,
}

impl DopsFreeze {
    pub fn new() -> DopsFreeze {
        let mut v = Vec::new();
        let mut i = 0usize;
        while i < FN_COUNT {
            v.push(CANON[i]);
            i += 1;
        }
        DopsFreeze {
            rows: v,
            state: FreezeState::Draft,
            asserts_ok: 0,
            tri: TriLedger::new(),
            hash: 0,
            snap_count: 0,
            frozen: false,
        }
    }

    /// FNV-1a 32 位。
    pub fn fnv1a(bytes: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < bytes.len() {
            h ^= bytes[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        h
    }

    /// 单函数断言：名、元数、掩码、void 标记四项比对；void 行必须恰为 M_TRUST。
    pub fn assert_one(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.rows.len() || idx >= FN_COUNT {
            return Err(E_IDX);
        }
        let got = self.rows[idx];
        let want = CANON[idx];
        if got.name != want.name || got.is_void != want.is_void {
            return Err(E_IDX);
        }
        if got.argc != want.argc {
            return Err(E_ARGC);
        }
        if got.allow != want.allow {
            return Err(E_TRI);
        }
        if got.is_void && got.allow != M_TRUST {
            return Err(E_VOID); // void 函数不得声称可返回失效/错误
        }
        if (got.allow & !(M_TRUST | M_INVALID | M_ERROR)) != 0 {
            return Err(E_TRI); // 掩码出现闭集外位
        }
        self.asserts_ok += 1;
        if self.state == FreezeState::Draft {
            self.state = FreezeState::Asserted;
        }
        Ok(())
    }

    /// 全表断言。
    pub fn assert_all(&mut self) -> Result<u32, u16> {
        if self.rows.len() != FN_COUNT {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < FN_COUNT {
            if let Err(e) = self.assert_one(i) {
                self.state = FreezeState::Broken;
                return Err(e);
            }
            i += 1;
        }
        Ok(self.asserts_ok)
    }

    /// 裁决：给定函数与返回态，校验是否在允许掩码内并入账。
    pub fn rule(&mut self, idx: usize, s: TriState) -> Result<TriState, u16> {
        if idx >= self.rows.len() {
            return Err(E_IDX);
        }
        let allow = self.rows[idx].allow;
        let ok = (allow & s.mask()) != 0;
        self.tri.note(s, ok);
        if !ok {
            return Err(E_TRI);
        }
        Ok(s)
    }

    /// 快照哈希：名折叠 + 元数 + 掩码 + void 标记。
    pub fn compute_hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            let mut fold = 0u8;
            let mut k = 0usize;
            while k < r.name.len() {
                fold ^= r.name.as_bytes()[k];
                k += 1;
            }
            buf.push(fold);
            buf.push(r.argc);
            buf.push(r.allow);
            buf.push(if r.is_void { 1u8 } else { 0u8 });
            i += 1;
        }
        Self::fnv1a(&buf)
    }

    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.state != FreezeState::Asserted {
            return Err(E_FROZEN);
        }
        let h = self.compute_hash();
        if h == 0 {
            return Err(E_HASH);
        }
        self.hash = h;
        self.snap_count = self.rows.len() as u8;
        self.frozen = true;
        self.state = FreezeState::Frozen;
        Ok(h)
    }

    pub fn verify(&self) -> Result<u32, u16> {
        if !self.frozen || self.snap_count as usize != FN_COUNT {
            return Err(E_FROZEN);
        }
        let h = self.compute_hash();
        if h != self.hash {
            return Err(E_HASH);
        }
        Ok(h)
    }

    /// void 型函数的调用记账：只允许 Trust（折算为「已完成」）。
    pub fn call_void(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.rows.len() {
            return Err(E_IDX);
        }
        if !self.rows[idx].is_void {
            return Err(E_VOID); // 非 void 函数不得走 void 通路
        }
        self.rule(idx, TriState::Trust)?;
        Ok(())
    }

    /// 三态覆盖：6 行中允许 Invalid 的恰 3 行（revalidate/compare/delete）。
    pub fn invalid_rows(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if (self.rows[i].allow & M_INVALID) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TRI => "返回态不在该函数允许的掩码内",
        E_ARGC => "元数与冻结值不符",
        E_COUNT => "函数数不是 6",
        E_HASH => "快照哈希不匹配",
        E_FROZEN => "冻结后改写或未断言即冻结",
        E_IDX => "函数序号越界或函数名不符",
        E_ACCT => "三态账不平",
        E_VOID => "void 函数语义被误用",
        _ => "未知诊断码",
    }
}

/// 确定性演练：逐函数断言 → 三态裁决 → 越界态注入 → 冻结 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：6 条逐函数断言，断言账 6/0，允许失效态的行恰 3 条。
    let mut df = DopsFreeze::new();
    let mut i = 0usize;
    while i < FN_COUNT {
        df.assert_one(i)?;
        i += 1;
    }
    if df.asserts_ok != FN_COUNT as u32 {
        return Err(E_ARGC);
    }
    if df.invalid_rows() != 3 {
        return Err(E_TRI);
    }
    if df.state != FreezeState::Asserted {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R2：d_revalidate 三态全通——信任/失效/错误各裁决一次，账齐。
    df.rule(DopId::Revalidate as usize, TriState::Trust)?;
    df.rule(DopId::Revalidate as usize, TriState::Invalid)?;
    df.rule(DopId::Revalidate as usize, TriState::Error)?;
    if df.tri.trust != 1 || df.tri.invalid != 1 || df.tri.error != 1 {
        return Err(E_ACCT);
    }
    if df.tri.total_legal() != 3 || df.tri.illegal != 0 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：d_hash 返回失效态 10/10 次全部拒收（判据「三态约定」的反向断言）。
    let mut bad = 0u32;
    let mut k = 0;
    while k < 10 {
        if df.rule(DopId::Hash as usize, TriState::Invalid) != Err(E_TRI) {
            return Err(E_TRI);
        }
        bad += 1;
        k += 1;
    }
    if bad != 10 || df.tri.illegal != 10 {
        return Err(E_ACCT);
    }
    // 合法态不受影响：d_hash 信任 + 错误仍可裁决
    df.rule(DopId::Hash as usize, TriState::Trust)?;
    df.rule(DopId::Hash as usize, TriState::Error)?;
    if df.tri.trust != 2 || df.tri.error != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：d_delete 允许信任/失效，拒绝错误态。
    df.rule(DopId::Delete as usize, TriState::Trust)?;
    df.rule(DopId::Delete as usize, TriState::Invalid)?;
    if df.rule(DopId::Delete as usize, TriState::Error) != Err(E_TRI) {
        return Err(E_TRI);
    }
    hits += 1;

    // R5：void 型函数走专用通路——d_release/d_iput 各记一次完成。
    df.call_void(DopId::Release as usize)?;
    df.call_void(DopId::Iput as usize)?;
    if df.tri.trust != 5 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R6：冻结——哈希非零、条数 6、可复验。
    let h = df.freeze()?;
    if h == 0 || !df.frozen || df.snap_count != 6 {
        return Err(E_HASH);
    }
    if df.verify()? != h {
        return Err(E_HASH);
    }
    hits += 1;

    // I1：掩码注入——把 d_delete 的允许集改成含错误态，断言必报 E_TRI。
    let mut f2 = DopsFreeze::new();
    f2.rows[3].allow = M_TRUST | M_INVALID | M_ERROR;
    if f2.assert_one(3) != Err(E_TRI) {
        return Err(E_TRI);
    }
    hits += 1;

    // I2：void 函数声称可失效 → E_VOID（语义错位的专门码）。
    let mut f3 = DopsFreeze::new();
    f3.rows[4].allow = M_TRUST | M_INVALID;
    if f3.assert_one(4) != Err(E_VOID) {
        return Err(E_VOID);
    }
    hits += 1;

    // I3：非 void 函数走 void 通路 → E_VOID；序号越界 → E_IDX。
    if df.call_void(DopId::Revalidate as usize) != Err(E_VOID) {
        return Err(E_VOID);
    }
    if df.rule(FN_COUNT, TriState::Trust) != Err(E_IDX) {
        return Err(E_IDX);
    }
    hits += 1;

    // I4：冻结后改写 → E_FROZEN；篡改规格后 verify → E_HASH。
    let mut f4 = DopsFreeze::new();
    f4.assert_all()?;
    f4.freeze()?;
    if f4.freeze() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    f4.rows[1].argc = 9;
    if f4.verify() != Err(E_HASH) {
        return Err(E_HASH);
    }
    hits += 1;

    // I5：函数数不足 → E_COUNT（先比条数再断言）。
    let mut f5 = DopsFreeze::new();
    f5.rows.pop();
    if f5.assert_all().is_ok() {
        return Err(E_COUNT);
    }
    hits += 1;

    // C1：三态账终审——trust 5（R2 1 + R3 1 + R4 1 + R5 2）、invalid 2、error 2、非法 11。
    if df.tri.trust != 5 || df.tri.invalid != 2 || df.tri.error != 2 {
        return Err(E_ACCT);
    }
    if df.tri.illegal != 11 {
        return Err(E_ACCT);
    }
    if df.tri.total_legal() != 9 {
        return Err(E_ACCT);
    }
    if df.verify()? != h {
        return Err(E_HASH);
    }
    hits += 1;

    Ok(hits)
}
