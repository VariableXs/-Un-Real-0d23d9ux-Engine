//! unx2222_rollver —— UNX-F2222 冻结账批级滚动升版（B32 段滚动条款）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2222）：
//!   UNX-F2222-J1 滚动升版全过（B32 的 20 条判据随批入总账：
//!   占位段补真指纹、族链延展、全域链重算、账版本 v1→v2 单调升），
//!   升版前后未涉及行指纹零变化断言过，跳步升版注入 10/10 拒绝
//!
//! ## 滚动升版到底在解决什么
//!
//! F2221 把全域冻结总账钉死在 v1：800 行、四列格式 v1、一条哈希链。
//! 但内核不可能永远停在 v1——B32 之后还有 B33..F2400 的判据要入账。
//! 于是问题来了：**账变了，怎么证明"只该变的地方变了"**？
//!
//! Windows 的做法是打新版本号（`major.minor.build`），
//! 装一个补丁写进文件属性。做完你只能知道"版本号从 1 变成 2 了"，
//! 至于 v2 到底动了哪几行、动的那些行是不是本来就该动——它不回答。
//!
//! 内核侧要回答的是四个问题，本模块逐个落成机制：
//!
//! 1. **能不能升**——版本只能 `+1`（`v1→v2`）。跳步（`v1→v3`）与
//!    降级（`v2→v1`）都是账目造假，必须**注入 10/10 全拒**。
//! 2. **补不补真指纹**——B33–F2400 的段位在v1 里只是"占位声明"，
//!    等某段真正被实现时，占位行必须**原地升级**成真指纹行。
//!    这是升版里唯一允许"改已有行内容"的路径，且改完kind 会变。
//! 3. **族链延展**——总链是一条平链，重算便宜但定位差：断在哪一行
//!    只能逐行对比。**族链**把 800 行按域切成 33 个族（B01..B32
//!    各一族 + F 段一族），每个族一条独立链。族链的价值是
//!    **未涉及族必须逐位不变**——这是廉价的局部性判据。
//! 4. **全域链重算**——族链管局部，总链管全局。两者都必须重算，
//!    且重算值 == 落账值。
//!
//! ## 为什么"未涉及行指纹零变化"是最强的一条
//!
//! 它是**局部性**的形式化：一次升版只准动被显式列入计划的那几行，
//! 剩下的行必须**逐位相同**。这条比"总数没变"强得多——
//! 有人删一行 A 又补一行 A（内容不同），总数不变、加法链不破、
//! 甚至总链重算后也能自洽（因为他自己重算过），但只要那一行
//! 不在计划里，零变化断言就红。
//!
//! 换句话说：**篡改必须同时骗过"计划"和"零变化"两道闸才可能藏住**，
//! 而这两道闸的判据完全不同（一道查集合归属，一道查逐位相等）。

#![allow(clippy::too_many_arguments)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::unx2221_freeledge::{
    chain_of, render_row_v1, B01, B32, CHAIN_SEED, F2400, FNV_PRIME, Row, RowKind, N_SLOT, N_TRUE,
};
use crate::checks::CheckSet;

/// 模块版本。
pub const F2222_VERSION: &str = "f2222-rollver-v1";

/// 诊断段基址（本单独占）。
pub const F2222_SEG_BASE: u16 = 0xD300;

/// 源账版本。
pub const VER_FROM: u32 = 1;
/// 目标账版本（滚动**只允许**到这一版）。
pub const VER_TO: u32 = 2;
/// 允许的单步跨度。
pub const VER_STEP: u32 = 1;

/// B32 段随批入账的判据条数（冻结常量·20）。
pub const B32_BATCH: usize = 20;
/// 其中"新增真指纹行"条数。
pub const B32_ADD: usize = 16;
/// 其中"占位段补真指纹"条数。
pub const B32_PROMOTE: usize = 4;

/// v1 账行数（620真 + 180 占位）。
pub const N_V1: usize = N_TRUE + N_SLOT;
/// v2 账行数。
///
/// **注意**：补指纹是**原地**升级（占位行 kind:S->T），**不增行**；
/// 真正新增行的只有随批新增的 16 条。所以 v2 = 800 + 16 = 816，
/// 而不是 800 + 20 = 820。批次构成式另立：
/// `B32_ADD + B32_PROMOTE == B32_BATCH`。
pub const N_V2: usize = N_V1 + B32_ADD;

/// 族数（B01..B32 共 32 族 + F 段 1 族）。
pub const FAM_N: usize = 33;
/// F 段族的族号（不属于 B01..B32）。
pub const FAM_F: u16 = 0xFFFF;
/// F 段族在族表里的下标。
pub const FAM_F_IDX: usize = 32;

/// 跳步注入的样本数（判据要求 10/10 全拒）。
pub const JUMP_N: usize = 10;

// --------------------------------------------------------------------------
// 诊断码
// --------------------------------------------------------------------------

/// 滚动升版诊断码（**12 码闭集**·独占 `0xD300` 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RollCode {
    /// 01版本跳步（跨度 > 1）。
    VerSkip = 0,
    /// 02 版本降级（to < from）。
    VerDowngrade,
    /// 03 版本越界（to > VER_TO）。
    VerOverflow,
    /// 04 补指纹目标不是占位行。
    SlotNotSlot,
    /// 05 补指纹目标已被补过。
    SlotAlreadyPromoted,
    /// 06 补指纹目标不在本批计划内。
    SlotNotInPlan,
    /// 07 族种子非法。
    FamSeedBad,
    /// 08 族内行数与族表登记不符。
    FamLenMismatch,
    /// 09 族链断裂（重算 != 落账）。
    FamChainBroken,
    /// 10 未涉及行指纹发生变化。
    UntouchedRowChanged,
    /// 11 跳步注入被接受（**永不成立**·存在即账破）。
    JumpInjectAccepted,
    /// 12 批次构成非法（add + promote != 20）。
    BatchShapeBad,
}

impl RollCode {
    /// 全部码（闭集·顺序即码序）。
    pub const ALL: [RollCode; 12] = [
        RollCode::VerSkip,
        RollCode::VerDowngrade,
        RollCode::VerOverflow,
        RollCode::SlotNotSlot,
        RollCode::SlotAlreadyPromoted,
        RollCode::SlotNotInPlan,
        RollCode::FamSeedBad,
        RollCode::FamLenMismatch,
        RollCode::FamChainBroken,
        RollCode::UntouchedRowChanged,
        RollCode::JumpInjectAccepted,
        RollCode::BatchShapeBad,
    ];

    /// 诊断码值（`0xD301` 起）。
    pub fn val(self) -> u16 {
        F2222_SEG_BASE + 1 + (self as u16)
    }

    /// 可操作修复建议（每条 >= 10 字节·不许有空洞）。
    pub fn remedy(self) -> &'static str {
        match self {
            RollCode::VerSkip => "版本跨度必须恰为 1：把 v1->v3 拆成两次连续升版，中间补齐 v2 落账",
            RollCode::VerDowngrade => "禁止降级：若要回退须新建并行账并标注取代关系，不得改低既有 ver",
            RollCode::VerOverflow => "目标版本越界：本次目标只能是 v2，超出部分另起批次处理",
            RollCode::SlotNotSlot => "补真指纹只能打在段位占位行上：检查该 seq 的 kind 是否被误改为 T",
            RollCode::SlotAlreadyPromoted => "该段位已补过真指纹：删除重复补录行，或为新段位重新分配 seg 值",
            RollCode::SlotNotInPlan => "补指纹目标不在本批计划内：先把该 seq 列入 RollPlan 再执行升版",
            RollCode::FamSeedBad => "族种子非法：族种子须由 CHAIN_SEED 与族号派生，不可外部注入",
            RollCode::FamLenMismatch => "族内行数与族表登记不符：重跑族表重建，勿手工改len 字段",
            RollCode::FamChainBroken => "族链断裂：族内必有行内容被改而族链未重算，重算后再落账",
            RollCode::UntouchedRowChanged => "出现未涉及行的指纹变化：把改动回滚，或将其显式列入本批计划",
            RollCode::JumpInjectAccepted => "跳步注入竟被接受：版本闸已失效，立即停止升版并复核 check_bump 实现",
            RollCode::BatchShapeBad => "批次构成非法：B32 随批判据须恰为 16 新增 + 4 补指纹 = 20 条",
        }
    }

    /// 渲染行。
    pub fn line(self) -> String {
        format!("0x{:04X} {}", self.val(), self.remedy())
    }
}

/// 诊断段守卫（本段位只能被本模块占用）。
#[must_use]
pub fn segment_guard(c: u16) -> bool {
    (F2222_SEG_BASE + 1..=F2222_SEG_BASE + 12).contains(&c)
}

// --------------------------------------------------------------------------
// 族链
// --------------------------------------------------------------------------

/// 族的派生种子（**唯一真源**·外部不得注入）。
#[must_use]
pub fn fam_seed(fam: u16) -> u64 {
    CHAIN_SEED ^ (fam as u64).wrapping_mul(FNV_PRIME)
}

/// 行归属的族号（B01..B32 为本族·B33..F2400 归 F 段族）。
#[must_use]
pub fn fam_of(r: &Row) -> u16 {
    if r.domain >= B01 && r.domain <= B32 {
        r.domain as u16
    } else {
        FAM_F
    }
}

/// 族号在族表里的下标。
#[must_use]
pub fn fam_idx(fam: u16) -> usize {
    if fam == FAM_F {
        FAM_F_IDX
    } else {
        (fam as usize).saturating_sub(B01 as usize)
    }
}

/// 单个族的延展链。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamChain {
    /// 族号。
    pub fam: u16,
    /// 族种子（由 `fam_seed` 派生）。
    pub seed: u64,
    /// 族内行数。
    pub len: u32,
    /// 落账链值。
    pub chain: u64,
}

impl FamChain {
    /// 造空族链。
    pub fn new(fam: u16) -> FamChain {
        FamChain {
            fam,
            seed: fam_seed(fam),
            len: 0,
            chain: fam_seed(fam),
        }
    }

    /// 族链延展一步。
    pub fn step(&self, row_fp: u64) -> u64 {
        (self.chain ^ row_fp).wrapping_mul(FNV_PRIME)
    }

    /// 种子自检（外部注入的种子会被这条抓住）。
    pub fn seed_ok(&self) -> bool {
        self.seed == fam_seed(self.fam) && self.chain != 0 || self.len == 0
    }
}

/// 族表（33 族·定容）。
#[derive(Clone, Debug)]
pub struct FamTable {
    fams: Vec<FamChain>,
}

impl FamTable {
    /// 造空族表。
    pub fn new() -> FamTable {
        let mut v: Vec<FamChain> = Vec::with_capacity(FAM_N);
        for i in 0..FAM_N {
            let fam = if i == FAM_F_IDX {
                FAM_F
            } else {
                (B01 as usize + i) as u16
            };
            v.push(FamChain::new(fam));
        }
        FamTable { fams: v }
    }

    /// 只读视图。
    pub fn fams_ref(&self) -> &[FamChain] {
        &self.fams
    }

    /// 族链读出。
    pub fn chain(&self, fam: u16) -> u64 {
        match self.fams.get(fam_idx(fam)) {
            Some(f) => f.chain,
            None => 0,
        }
    }

    /// 族内行数读出。
    pub fn len_of(&self, fam: u16) -> u32 {
        match self.fams.get(fam_idx(fam)) {
            Some(f) => f.len,
            None => 0,
        }
    }

    /// 全族表重建（重算每族链）。
    pub fn rebuild(rows: &[Row]) -> FamTable {
        let mut t = FamTable::new();
        for r in rows.iter() {
            let i = fam_idx(fam_of(r));
            if let Some(f) = t.fams.get_mut(i) {
                f.chain = f.step(r.compute_fp());
                f.len = f.len.wrapping_add(1);
            }
        }
        t
    }

    /// 族链全闭合（重算逐族 == 落账）。
    pub fn all_closed(&self, rows: &[Row]) -> bool {
        let fresh = FamTable::rebuild(rows);
        for (a, b) in self.fams.iter().zip(fresh.fams.iter()) {
            if a.chain != b.chain || a.len != b.len {
                return false;
            }
        }
        true
    }

    /// 族内行数与实际行数一致。
    pub fn len_closed(&self, rows: &[Row]) -> bool {
        let fresh = FamTable::rebuild(rows);
        self.fams.iter().zip(fresh.fams.iter()).all(|(a, b)| a.len == b.len)
    }

    /// 未涉及族的链值逐位不变（**局部性判据**·参��旧表比对）。
    pub fn untouched_intact(&self, old: &FamTable, touched: &[u16]) -> bool {
        for (a, b) in old.fams.iter().zip(self.fams.iter()) {
            if touched.contains(&a.fam) {
                continue;
            }
            if a.chain != b.chain || a.len != b.len {
                return false;
            }
        }
        true
    }
}

// --------------------------------------------------------------------------
// 版本闸
// --------------------------------------------------------------------------

/// 版本闸：**只允许 `to == from + 1`**且不越界。
///
/// 这条闸是滚动升版的**第一道也是最廉价的一道**：它 O(1)，
/// 却在第一秒就挡掉"一���跳到 v5"这种最常见的账目造假。
/// 注意它**不查内容**——跳步升版注入 10/10 全拒，正是靠它。
pub fn check_bump(from: u32, to: u32) -> Result<u32, RollCode> {
    if to < from {
        return Err(RollCode::VerDowngrade);
    }
    if to > VER_TO {
        return Err(RollCode::VerOverflow);
    }
    if to - from != VER_STEP {
        return Err(RollCode::VerSkip);
    }
    Ok(to)
}

/// 跳步注入样本（**10 条**·覆盖跳步/降级/越界三类）。
#[must_use]
pub fn jump_injections() -> [(u32, u32); JUMP_N] {
    [
        (1, 3),
        (1, 4),
        (1, 11),
        (1, u32::MAX),
        (2, 1),
        (2, 0),
        (2, 5),
        (0, 2),
        (0, 7),
        (1, 0),
    ]
}

// --------------------------------------------------------------------------
// 批次计划
// --------------------------------------------------------------------------

/// 一批（B32 段随批判据）的升版计划。
#[derive(Clone, Debug)]
pub struct RollPlan {
    /// 源版本。
    pub from: u32,
    /// 目标版本。
    pub to: u32,
    /// 随批入账的 20 条判据编号（B32-J01..B32-J20）。
    pub batch: Vec<u32>,
    /// 新增真指纹行的 `seq`（16 条）。
    pub add: Vec<u32>,
    /// 占位段补真指纹的 `seq`（4 条）。
    pub promote: Vec<u32>,
}

impl RollPlan {
    /// 批次构成自检（恰为 16 新增 + 4 补指纹 = 20）。
    pub fn shape_ok(&self) -> bool {
        self.batch.len() == B32_BATCH
            && self.add.len() == B32_ADD
            && self.promote.len() == B32_PROMOTE
            && self.add.len() + self.promote.len() == B32_BATCH
    }

    /// 本批涉及的全部 `seq`（零变化断言的比对集合）。
    pub fn touched_seqs(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::with_capacity(B32_BATCH);
        for s in self.add.iter() {
            v.push(*s);
        }
        for s in self.promote.iter() {
            v.push(*s);
        }
        v
    }

    /// 本批涉及的全部族号（族链局部性断言的比对集合）。
    pub fn touched_fams(&self, rows: &[Row]) -> Vec<u16> {
        let seqs = self.touched_seqs();
        let mut v: Vec<u16> = Vec::new();
        for s in seqs.iter() {
            if let Some(r) = rows.iter().find(|x| x.seq == *s) {
                let f = fam_of(r);
                if !v.contains(&f) {
                    v.push(f);
                }
            }
        }
        v
    }

    /// 计划渲染（零浮点·十六进制域号）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "rollplan v{}->v{} batch={} add={} promote={} shape_ok={}\n",
            self.from,
            self.to,
            self.batch.len(),
            self.add.len(),
            self.promote.len(),
            self.shape_ok()
        ));
        s.push_str("batch:");
        for j in self.batch.iter() {
            s.push_str(&format!(" B32-J{:02}", j));
        }
        s.push('\n');
        s.push_str("add_seq:");
        for q in self.add.iter() {
            s.push_str(&format!(" {}", q));
        }
        s.push_str("\npromote_seq:");
        for q in self.promote.iter() {
            s.push_str(&format!(" {}", q));
        }
        s.push('\n');
        s
    }
}

// --------------------------------------------------------------------------
// 滚动账
// --------------------------------------------------------------------------

/// 冻结账的滚动升版器。
#[derive(Clone, Debug)]
pub struct Roller {
    rows: Vec<Row>,
    /// 当前账版本。
    pub ver: u32,
    /// 族表。
    pub fams: FamTable,
    /// 全域链落账值。
    pub global: u64,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
    /// **接管时的行快照**（`(seq, fp, kind, value)`）。
    ///
    /// 这是零变化判据的**唯一真源**：行内自洽（每行 fp 等于自己算的）
    /// 只能证明"账没算错"，证明不了"账没被改"——篡改者把行内容改了
    /// 再重算 fp，两边照样自洽。必须拿一份**跨版本的旁证**来比。
    base_snap: Vec<(u32, u64, RowKind, u64)>,
    /// 本次升版已登记的涉及行（提交时写入）。
    touched: Vec<u32>,
}

impl Roller {
    /// 造滚动账（从既有 v1 行集接管）。
    pub fn new(rows: Vec<Row>, ver: u32) -> Roller {
        let fams = FamTable::rebuild(&rows);
        let global = chain_of(&rows);
        let mut r = Roller {
            rows,
            ver,
            fams,
            global,
            stats: [0; 12],
            base_snap: Vec::new(),
            touched: Vec::new(),
        };
        r.base_snap = r.snapshot();
        r
    }

    /// 未涉及行的指纹/种类/值偏离列表（**零变化闸**）。
    ///
    /// 返回偏离行的 `seq`。空列表 == 未涉及行逐位未变。
    pub fn zero_change_violations(&self) -> Vec<u32> {
        let mut bad: Vec<u32> = Vec::new();
        for (seq, fp, kind, value) in self.base_snap.iter() {
            if self.touched.contains(seq) {
                continue;
            }
            match self.rows.iter().find(|r| r.seq == *seq) {
                Some(r) => {
                    if r.fp != *fp || r.kind != *kind || r.value != *value {
                        bad.push(*seq);
                    }
                }
                None => bad.push(*seq),
            }
        }
        bad
    }

    /// 立案计数自增。
    fn bump(&mut self, c: RollCode) {
        let i = (c.val() - F2222_SEG_BASE - 1) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: RollCode) -> u64 {
        let i = (c.val() - F2222_SEG_BASE - 1) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 只读行视图。
    pub fn rows_ref(&self) -> &[Row] {
        &self.rows
    }

    /// 可变行视图（**仅供对账/注入测试构造反例**·
    /// 正常升版路径一律走 `promote` / `add_row` 以保证留痕）。
    pub fn rows_mut(&mut self) -> &mut Vec<Row> {
        &mut self.rows
    }

    /// 改内容前先算旧行指纹快照（零变化断言用）。
    pub fn snapshot(&self) -> Vec<(u32, u64, RowKind, u64)> {
        let mut v: Vec<(u32, u64, RowKind, u64)> = Vec::with_capacity(self.rows.len());
        for r in self.rows.iter() {
            v.push((r.seq, r.fp, r.kind, r.value));
        }
        v
    }

    /// 占位段补真指纹（**升版唯一允许的改已有行路径**）。
    ///
    /// 三个前置条件缺一不可：
    /// - 目标必须在计划里（否则等于偷偷改账）
    /// - 目标必须是占位行（真指纹行补指纹是空操作）
    /// - 目标不能已被补过（防重复补录）
    ///
    /// 注意补完之后 `kind` 从 `S` 变`T`——**行指纹必变**。
    /// 这正是"补真指纹"要有痕迹的原因：段位从"声明占用"
    /// 变成"内容可验"，账上必须看得出这个状态跃迁。
    pub fn promote(&mut self, seq: u32, real_fp: u64, plan: &RollPlan) -> Result<u64, RollCode> {
        let idx = match self.rows.iter().position(|r| r.seq == seq) {
            Some(i) => i,
            None => {
                self.bump(RollCode::SlotNotInPlan);
                return Err(RollCode::SlotNotInPlan);
            }
        };
        // 顺序敏感：**先判是不是占位行**，再判在不在计划内。
        // 反过来写的话，"对真指纹行补指纹"会先被"计划外"拦下，
        // 报出 SlotNotInPlan——诊断码答非所问，排障时被带偏。
        let kind = match self.rows.get(idx) {
            Some(r) => r.kind,
            None => {
                self.bump(RollCode::SlotNotInPlan);
                return Err(RollCode::SlotNotInPlan);
            }
        };
        if kind == RowKind::TrueFp {
            // 分不清是"本来就是真指纹行"还是"已补过"时，
            // 统一按"已补过"处理：两种情况都不该再补。
            self.bump(RollCode::SlotAlreadyPromoted);
            return Err(RollCode::SlotAlreadyPromoted);
        }
        if !plan.promote.contains(&seq) {
            self.bump(RollCode::SlotNotInPlan);
            return Err(RollCode::SlotNotInPlan);
        }
        // 原地改 kind + value，并重算行指纹。
        if let Some(r) = self.rows.get_mut(idx) {
            r.kind = RowKind::TrueFp;
            r.value = real_fp;
            r.fp = r.compute_fp();
        }
        Ok(self.rows.get(idx).map(|r| r.fp).unwrap_or(0))
    }

    /// 随批新增真指纹行（序号必须接续）。
    pub fn add_row(&mut self, r: Row, plan: &RollPlan) -> Result<usize, RollCode> {
        if !plan.add.contains(&r.seq) {
            self.bump(RollCode::SlotNotInPlan);
            return Err(RollCode::SlotNotInPlan);
        }
        let want = self.rows.len() as u32 + 1;
        if r.seq != want {
            self.bump(RollCode::SlotNotInPlan);
            return Err(RollCode::SlotNotInPlan);
        }
        self.rows.push(r);
        Ok(self.rows.len() - 1)
    }

    /// 全域链重算（重算并落账）。
    pub fn reglobal(&mut self) -> u64 {
        self.global = chain_of(&self.rows);
        self.global
    }

    /// 族链重算（重建族表）。
    pub fn refams(&mut self) {
        self.fams = FamTable::rebuild(&self.rows);
    }

    /// 升版提交（**顺序敏感**：先内容、再族链、后总链、最后 ver**）。
    ///
    /// 为什么 ver 必须最后写：ver 是"这笔升版发生过"的唯一标记，
    /// 如果先写ver 而后内容失败，账就会停在一个"声称已升版但内容没跟上"
    /// 的自相矛盾状态——而这正是段位重排最常见的伪装形态。
    pub fn commit(&mut self, plan: &RollPlan) -> Result<u32, RollCode> {
        match check_bump(plan.from, plan.to) {
            Ok(_) => {}
            Err(e) => {
                self.bump(e);
                return Err(e);
            }
        }
        if plan.from != self.ver {
            self.bump(RollCode::VerSkip);
            return Err(RollCode::VerSkip);
        }
        if !plan.shape_ok() {
            self.bump(RollCode::BatchShapeBad);
            return Err(RollCode::BatchShapeBad);
        }
        for s in plan.fams_broken() {
            self.bump(s);
        }
        if !self.fams.all_closed(&self.rows) {
            self.bump(RollCode::FamChainBroken);
            return Err(RollCode::FamChainBroken);
        }
        if !self.fams.len_closed(&self.rows) {
            self.bump(RollCode::FamLenMismatch);
            return Err(RollCode::FamLenMismatch);
        }
        if self.global != chain_of(&self.rows) {
            self.bump(RollCode::FamChainBroken);
            return Err(RollCode::FamChainBroken);
        }
        self.touched = plan.touched_seqs();
        self.ver = plan.to;
        Ok(self.ver)
    }

    /// 全量升版断言（判据 J1 主入口）。
    pub fn assert_all(&mut self, base_rows: usize) -> bool {
        let mut ok = true;
        if self.ver < VER_FROM || self.ver > VER_TO {
            self.bump(RollCode::VerOverflow);
            ok = false;
        }
        if self.len() != N_V2 {
            self.bump(RollCode::VerSkip);
            ok = false;
        }
        if base_rows != N_V1 {
            self.bump(RollCode::VerSkip);
            ok = false;
        }
        if !self.fams.all_closed(&self.rows) {
            self.bump(RollCode::FamChainBroken);
            ok = false;
        }
        if !self.fams.len_closed(&self.rows) {
            self.bump(RollCode::FamLenMismatch);
            ok = false;
        }
        if self.global != chain_of(&self.rows) {
            self.bump(RollCode::FamChainBroken);
            ok = false;
        }
        for r in self.rows.iter() {
            if !r.fp_intact() {
                self.bump(RollCode::UntouchedRowChanged);
                ok = false;
                break;
            }
        }
        // 零变化闸（**独立于链**）：逐条比接管快照。
        let bad = self.zero_change_violations();
        if !bad.is_empty() {
            for _ in bad.iter() {
                self.bump(RollCode::UntouchedRowChanged);
            }
            ok = false;
        }
        ok
    }

    /// 渲染（看板真源·零浮点·头尾各三行）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "rollver v{} rows={} fams={} global=0x{:016X} seg=0x{:04X}\n",
            self.ver,
            self.rows.len(),
            FAM_N,
            self.global,
            F2222_SEG_BASE
        ));
        s.push_str(&format!(
            "fam_b01=0x{:016X} fam_b32=0x{:016X} fam_f=0x{:016X}\n",
            self.fams.chain(1),
            self.fams.chain(32),
            self.fams.chain(FAM_F)
        ));
        s.push_str("seq|domain|kind|value\n");
        let n = self.rows.len();
        for i in 0..n {
            if i >= 3 && i + 3 < n {
                if i == 3 {
                    s.push_str("...\n");
                }
                continue;
            }
            match self.rows.get(i) {
                Some(r) => {
                    s.push_str(&render_row_v1(r));
                    s.push('\n');
                }
                None => {}
            }
        }
        s
    }
}

impl RollPlan {
    /// 计划自身可能的形态缺陷（提交时统一立案）。
    fn fams_broken(&self) -> Vec<RollCode> {
        let mut v: Vec<RollCode> = Vec::new();
        if self.add.len() != B32_ADD || self.promote.len() != B32_PROMOTE {
            v.push(RollCode::BatchShapeBad);
        }
        for s in self.promote.iter() {
            for t in self.add.iter() {
                if s == t {
                    v.push(RollCode::BatchShapeBad);
                }
            }
        }
        v
    }
}

// --------------------------------------------------------------------------
// 造样
// --------------------------------------------------------------------------

/// 造 v1 基线账（620 真指纹 + 180 占位= 800 行·确定性派生·零墙钟）。
#[must_use]
pub fn build_v1(seed: u64) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::with_capacity(N_V1);
    for i in 0..N_TRUE {
        let seq = (i + 1) as u32;
        let dom = B01 + (i as u32) % (B32 - B01 + 1);
        let fp = (seed
            .wrapping_add(i as u64)
            .wrapping_mul(FNV_PRIME))
            ^ (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        rows.push(Row::true_row(seq, dom, fp));
    }
    for i in 0..N_SLOT {
        let seq = (N_TRUE + i + 1) as u32;
        let dom = F2400 - (i as u32);
        // 段位**先乘后加**：`(base + i) * step` 会在 base*step 溢出 u16
        // 时把整段段位折叠成 0，而 0 段位是非法载荷 → 占位行被静默全丢。
        let seg = 0x2000u16.wrapping_add((i as u16).wrapping_mul(0x10));
        rows.push(Row::slot_row(seq, dom, seg, i as u32));
    }
    rows
}

/// 造 B32 随批计划（20 条= 16 新增 + 4 补指纹）。
#[must_use]
pub fn plan_b32() -> RollPlan {
    let mut add: Vec<u32> = Vec::with_capacity(B32_ADD);
    let mut promote: Vec<u32> = Vec::with_capacity(B32_PROMOTE);
    // 补指纹挑 v1 账末尾 4 条占位行（它们域号属于 F 段族）。
    for i in 0..B32_PROMOTE {
        promote.push((N_TRUE + N_SLOT - B32_PROMOTE + i + 1) as u32);
    }
    // 新增 16 行接在 801..816，域号全落 B32 族。
    for i in 0..B32_ADD {
        add.push((N_V1 + i + 1) as u32);
    }
    let mut batch: Vec<u32> = Vec::with_capacity(B32_BATCH);
    for i in 0..B32_BATCH {
        batch.push((i + 1) as u32);
    }
    RollPlan {
        from: VER_FROM,
        to: VER_TO,
        batch,
        add,
        promote,
    }
}

/// 判据指纹派生（B32-J## 的内容指纹·零浮点）。
#[must_use]
pub fn judge_fp(j: u32) -> u64 {
    (CHAIN_SEED ^ (j as u64).wrapping_mul(FNV_PRIME)).wrapping_add(0x1000_0000_0000_0000)
}

// --------------------------------------------------------------------------
// 判据（12 条）
// --------------------------------------------------------------------------

/// UNX-F2222 判据全集（12 条）。
pub fn run_f2222_checks() -> CheckSet {
    let mut cs = CheckSet::new("f2222");

    // ---- F2222-01 诊断段闭集与建议完备 ----
    let seg_ok = RollCode::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == F2222_SEG_BASE + 1 + i as u16)
        && RollCode::ALL.iter().all(|c| segment_guard(c.val()));
    let remedy_ok = RollCode::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("F2222-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续落段+建议完备");

    // ---- F2222-02 版本闸：+1 通过 ----
    let bump_ok = check_bump(1, 2).is_ok() && check_bump(0, 1).is_ok();
    cs.add("F2222-02 单步升版放行", bump_ok, "v1->v2 / v0->v1 恰一步均放行");

    // ---- F2222-03 跳步注入 10/10 全拒 ----
    let mut rejected = 0u64;
    for (a, b) in jump_injections().iter() {
        if check_bump(*a, *b).is_err() {
            rejected = rejected.wrapping_add(1);
        }
    }
    cs.add(
        "F2222-03 跳步注入 10/10 拒绝",
        rejected == JUMP_N as u64,
        "跳步/降级/越界三类注入共 10 条全拒",
    );

    // ---- F2222-04 跳步注入未被记账 ----
    let mut r04 = Roller::new(build_v1(0x1234_5678), VER_FROM);
    for (a, b) in jump_injections().iter() {
        let mut p = plan_b32();
        p.from = *a;
        p.to = *b;
        let _ = r04.commit(&p);
    }
    let accepted_bad = r04.stat(RollCode::JumpInjectAccepted);
    cs.add(
        "F2222-04 注入零放行",
        accepted_bad == 0 && r04.ver == VER_FROM,
        "10 次注入后账版本仍为 v1·未推进",
    );

    // ---- F2222-05 批次构成 16+4=20 ----
    let plan = plan_b32();
    let shape = plan.shape_ok()
        && plan.batch.len() == B32_BATCH
        && plan.add.len() + plan.promote.len() == B32_BATCH;
    cs.add("F2222-05 批次构成闭合", shape, "16 新增 + 4 补指纹 = 20 条随批判据");

    // ---- F2222-06 正常升版成功 ----
    let base = build_v1(0x1234_5678);
    let _snap_v1 = {
        let tmp = Roller::new(base.clone(), VER_FROM);
        tmp.snapshot()
    };
    let mut r06 = Roller::new(base.clone(), VER_FROM);
    // 先补 4 条占位行的真指纹。
    let mut promoted_fps: Vec<u64> = Vec::new();
    for (i, s) in plan.promote.iter().enumerate() {
        if let Ok(f) = r06.promote(*s, judge_fp(900 + i as u32), &plan) {
            promoted_fps.push(f);
        }
    }
    // 再随批新增 16 行（域号全落 B32）。
    let mut added = 0usize;
    for (i, s) in plan.add.iter().enumerate() {
        let row = Row::true_row(*s, B32, judge_fp((i + 1) as u32));
        if r06.add_row(row, &plan).is_ok() {
            added = added.wrapping_add(1);
        }
    }
    r06.refams();
    r06.reglobal();
    let committed = r06.commit(&plan);
    let len_ok = r06.len() == N_V2 && added == B32_ADD && promoted_fps.len() == B32_PROMOTE;
    cs.add(
        "F2222-06 升版成功",
        committed == Ok(VER_TO) && len_ok && r06.ver == VER_TO,
        "v1(800)->v2(820)·补 4 增 16·ver 单调升至 2",
    );

    // ---- F2222-07 占位段补真指纹生效 ----
    let promoted_true = r06
        .rows_ref()
        .iter()
        .filter(|r| plan.promote.contains(&r.seq) && r.kind == RowKind::TrueFp)
        .count();
    let promoted_fp_ok = r06
        .rows_ref()
        .iter()
        .filter(|r| plan.promote.contains(&r.seq) && r.fp_intact())
        .count();
    cs.add(
        "F2222-07 占位补真指纹",
        promoted_true == B32_PROMOTE && promoted_fp_ok == B32_PROMOTE,
        "4 条占位行 kind:S->T 且行指纹自洽",
    );

    // ---- F2222-08 补指纹越权被拒（三道闸各拒一次） ----
    let mut r08 = Roller::new(base.clone(), VER_FROM);
    // ① 目标不在计划内（取一条未被列入promote 的占位行）。
    // 注意不能取末行：末行 seq=800 恰好在 promote 计划里。
    let outside = (N_TRUE + N_SLOT - B32_PROMOTE - 1) as u32;
    let e1 = r08.promote(outside, 1, &plan);
    // ② 目标本来就是真指纹行。
    let true_row_seq = 1u32;
    let e2 = r08.promote(true_row_seq, 1, &plan);
    // ③ 目标已被补过（重复补）。
    let mut p2 = plan.clone();
    p2.promote.push(true_row_seq);
    let e3 = r08.promote(true_row_seq, 1, &p2);
    // ④ 新增行走计划外通道（seq 取计划末尾之后·不在 add 列表内）。
    let e4 = r08.add_row(Row::true_row(N_V1 as u32 + B32_ADD as u32 + 1, B32, 1), &plan);
    cs.add(
        "F2222-08 补指纹越权全拒",
        e1 == Err(RollCode::SlotNotInPlan)
            && e2 == Err(RollCode::SlotAlreadyPromoted)
            && e3 == Err(RollCode::SlotAlreadyPromoted)
            && e4 == Err(RollCode::SlotNotInPlan)
            && r08.stat(RollCode::SlotNotInPlan) == 2
            && r08.stat(RollCode::SlotAlreadyPromoted) == 2
            && r08.len() == N_V1,
        "计划外/真指纹行/重复补/计划外新增 四条路径全拒且行集未变",
    );

    // ---- F2222-09 未涉及行指纹零变化 ----
    let touched = plan.touched_seqs();
    let zero_bad = r06.zero_change_violations();
    // 快照里 800 行 − 4 条补指纹 = 796 条未涉及行必须逐位相同。
    let count_ok = r06.len() - touched.len() == N_V1 - B32_PROMOTE;
    cs.add(
        "F2222-09 未涉及行零变化",
        zero_bad.is_empty() && count_ok,
        "796 条未涉及行指纹/种类/值逐位相同",
    );

    // ---- F2222-10 族链延展 + 未涉及族不变 ----
    // 族号必须在**升版后**的行集上算：新增的 801..816 在 v1 里根本不存在，
    // 若拿 v1 行集去查，这16 行会被漏掉，B32 族链已变却没被标为"涉及"，
    // 局部性断言就会误判成"未涉及族发生了变化"（假红）。
    let fams_touched = plan.touched_fams(r06.rows_ref());
    let fams_local = r06.fams.untouched_intact(&Roller::new(base.clone(), VER_FROM).fams, &fams_touched);
    let fams_closed = r06.fams.all_closed(r06.rows_ref()) && r06.fams.len_closed(r06.rows_ref());
    let b32_chain_moved =
        r06.fams.chain(B32 as u16) != Roller::new(base.clone(), VER_FROM).fams.chain(B32 as u16);
    cs.add(
        "F2222-10 族链延展",
        fams_local && fams_closed && b32_chain_moved,
        "B32 族链已延展·未涉及族链逐位不变·族内行数自洽",
    );

    // ---- F2222-11 全域链重算 + 落账快照 ----
    let g_recalc = chain_of(r06.rows_ref());
    let snapshot_open = Roller::new(base.clone(), VER_FROM).reglobal_check_only();
    let global_ok = r06.global == g_recalc && g_recalc != snapshot_open;
    let assert_ok = {
        let mut chk = r06.clone();
        chk.assert_all(N_V1)
    };
    cs.add(
        "F2222-11 全域链重算",
        global_ok && assert_ok,
        "重算==落账且与 v1 快照不同·全量断言通过",
    );

    // ---- F2222-12 篡改必破（零变化闸 vs 链闸 双轨） ----
    // 反例构造：改一行**不在计划内**的真指纹内容并重算该行 fp。
    // 这会让行集自洽、族链与总链重算后也自洽——但零变化闸独立于
    // 链，所以只要比对 v1 快照就一定红。这正是"两道闸判据不同"的
    // 价值：链能查"改了什么"，快照能查"该不该改"。
    let mut r12 = Roller::new(base.clone(), VER_FROM);
    for (i, s) in plan.promote.iter().enumerate() {
        let _ = r12.promote(*s, judge_fp(900 + i as u32), &plan);
    }
    for (i, s) in plan.add.iter().enumerate() {
        let row = Row::true_row(*s, B32, judge_fp((i + 1) as u32));
        let _ = r12.add_row(row, &plan);
    }
    r12.refams();
    r12.reglobal();
    let committed12 = r12.commit(&plan);
    let victim_seq = 7u32; // 计划外的真指纹行
    if let Some(idx) = r12.rows_ref().iter().position(|r| r.seq == victim_seq) {
        if let Some(r) = r12.rows_mut().get_mut(idx) {
            r.value ^= 0xFFFF;
            r.fp = r.compute_fp();
        }
    }
    // 篡改者顺手把族链与总链也重算一遍——这是最强反例：
    // 此时链闸、族链闸、行内自洽闸**全部绿**，只有零变化闸还红着。
    r12.refams();
    r12.reglobal();
    // 链侧：重算后链与落账一致（说明链闸被"骗过"）。
    let chain_fooled = r12.global == chain_of(r12.rows_ref());
    // 快照侧：行集自洽（每行 fp 正确）但内容已偏离 v1。
    let rows_self_ok = r12.rows_ref().iter().all(|r| r.fp_intact());
    let zero_bad2 = r12.zero_change_violations();
    let fams_fooled = r12.fams.all_closed(r12.rows_ref());
    let detected = committed12 == Ok(VER_TO)
        && chain_fooled
        && fams_fooled
        && rows_self_ok
        && !zero_bad2.is_empty();
    r12.assert_all(N_V1);
    let flagged = r12.stat(RollCode::UntouchedRowChanged);
    cs.add(
        "F2222-12 篡改双轨检出",
        detected && zero_bad2.len() == 1 && flagged == 1,
        "改计划外行并重算行fp/族链/总链：三闸全绿·仅零变化闸独立捕获",
    );

    cs
}

impl Roller {
    /// 只读视图下重算全域链（用于快照比对·不落账）。
    pub fn reglobal_check_only(&self) -> u64 {
        chain_of(&self.rows)
    }
}

/// UNX-F2222 锚点串（跨批引用防漂移）。
#[must_use]
pub fn f2222_anchor() -> &'static str {
    F2222_VERSION
}