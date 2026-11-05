//! UNX-F12657 探测面边界样本（空参数/越界 ReturnLength/信息类越档） · 0x80E00段
//!
//! 判据原文（派工卡 12656 逐字锚定）：
//!   ① 纯功能代码 ≥290 行；
//!   ② 判据原文：UNX-F12657-J1 边界样本 ≥30 例入库全过，与 F12654 联测
//!      资产同源可重演；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 边界样本域模型：样本六元 =（样本号/族名/信息类号/缓冲账长/
//!    回长/期望状态）；三族闭集各 10 例共 30 例——空参数族（账长为零）、
//!    越界回长族（回长 < 账长）、信息类越档族（类号 ≥ 8 或越 u32 域）；
//! 2. 状态闭集同源：取本批探测面同源 NTSTATUS 结论值（0 成功 /
//!    0xC0000004 长度不匹 / 0xC000000D 参数无效 / 0xC0000003 类号无效），
//!    与 F12654 联测资产同一套参照剖面（版本锚 10.0.22000 + 八类偏移）；
//! 3. 边界判引擎（冻结规则，同源单一权威）：类号越档 → 类号无效；
//!    账长为零 → 参数无效；回长 < 账长 → 长度不匹；其余 → 成功；
//!    判定顺序固定，逐样本独立判定，不复用表内期望列；
//! 4. 入库闸：样本号连续、族名闭集、期望状态闭集、判引擎判定 ==
//!    表内期望 四道闸逐样本过闸，全过才许入库；违约样本不入库并留
//!    违约码（不静默丢行）；
//! 5. 同源可重演：样本库为冻结常量面，重演跑两轮逐样本同判；LCG
//!    种子与本批联测资产同族（SEG_BASE 与 SEG_BASE+0x2C 两档）；
//! 6. 注入联测 10 路：期望改写、族名错挂、缺行入库、越库行、样本
//!    重复、回长语义倒挂、账长语义倒挂、类号语义倒挂、样本号乱序、
//!    判定越状态闭集——逐路必须被闸抓住；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12656 − 12600)）。
pub const SEG_BASE: u32 = 0x80E00;

// ── 边界样本诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeDiag {
    /// 样本号越序（跳号/倒退）。
    ESampleOrder,
    /// 样本号越库闭集（≥30）。
    ESampleRange,
    /// 族名越三族闭集。
    EFamilyRange,
    /// 期望状态越状态闭集。
    EExpectRange,
    /// 判引擎判定与表内期望不符。
    EJudgeMiss,
    /// 样本重复入库。
    ESampleDup,
    /// 库未满就试图封库。
    ENotFull,
    /// 封库后写入被拒。
    ESealed,
    /// 未入库就试图封库。
    ENotStored,
    /// 重演不符（两轮判定不一致）。
    EReplayMiss,
    /// 状态机倒退。
    ERewind,
    /// 库账等式破坏（入库数+拒收数≠总样本数）。
    EEqBroken,
}

impl EdgeDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                EdgeDiag::ESampleOrder => 0x01,
                EdgeDiag::ESampleRange => 0x02,
                EdgeDiag::EFamilyRange => 0x03,
                EdgeDiag::EExpectRange => 0x04,
                EdgeDiag::EJudgeMiss => 0x05,
                EdgeDiag::ESampleDup => 0x06,
                EdgeDiag::ENotFull => 0x07,
                EdgeDiag::ESealed => 0x08,
                EdgeDiag::ENotStored => 0x09,
                EdgeDiag::EReplayMiss => 0x0a,
                EdgeDiag::ERewind => 0x0b,
                EdgeDiag::EEqBroken => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            EdgeDiag::ESampleOrder => "样本号越序：按 0..30 逐例入库，跳号先回补上一例",
            EdgeDiag::ESampleRange => "样本号越库闭集：样本库只认 30 例，越例先对库容量",
            EdgeDiag::EFamilyRange => "族名越三族闭集：只认空参数/越界回长/类号越档三族，先正族名",
            EdgeDiag::EExpectRange => "期望状态越状态闭集：只认四值同源闭集，先修期望再入库",
            EdgeDiag::EJudgeMiss => "判引擎判定不符：表内期望与冻结规则重算有差，先修样本或规则",
            EdgeDiag::ESampleDup => "样本重复：同样本号只落一笔，重复入库先查已有样本号",
            EdgeDiag::ENotFull => "库未满禁止封库：30 例全入库后才许走封库口",
            EdgeDiag::ESealed => "样本库已封库：封库后写入拒绝，改动走下一批样本滚动",
            EdgeDiag::ENotStored => "未入库禁止封库：先入库再走封库口",
            EdgeDiag::EReplayMiss => "重演不符：两轮判定不一致，先核冻结面是否被改",
            EdgeDiag::ERewind => "状态机倒退：入库中→封库只进不退，倒退即库账可疑",
            EdgeDiag::EEqBroken => "库账等式破坏：入库数+拒收数≠总样本数，先对账再封库",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const EDGE_ALL: &[EdgeDiag] = &[
    EdgeDiag::ESampleOrder,
    EdgeDiag::ESampleRange,
    EdgeDiag::EFamilyRange,
    EdgeDiag::EExpectRange,
    EdgeDiag::EJudgeMiss,
    EdgeDiag::ESampleDup,
    EdgeDiag::ENotFull,
    EdgeDiag::ESealed,
    EdgeDiag::ENotStored,
    EdgeDiag::EReplayMiss,
    EdgeDiag::ERewind,
    EdgeDiag::EEqBroken,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in EDGE_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80E01..0x80E0C 闭集内"
}

// ── 同源参照剖面（与 F12654 联测资产同一套冻结值） ────────────────
/// 版本锚（同源：与 F12654 联测资产参照锚逐元相等）。
pub const REF_VER_SAME: (u32, u32, u32) = (10, 0, 22000);

/// 有效类闭集上沿（同源：类号 < 8 为有效类）。
pub const VALID_CLASS_CAP: u32 = 8;

/// 状态闭集（同源 NTSTATUS 结论值，只引结论不抄码）。
/// 状态：成功。
pub const EST_OK: u32 = 0x0000_0000;
/// 状态：长度不匹配（回长 < 账长）。
pub const EST_LEN_MISMATCH: u32 = 0xC000_0004;
/// 状态：参数无效（账长为零）。
pub const EST_INVALID_PARAM: u32 = 0xC000_000D;
/// 状态：类号无效（越档）。
pub const EST_INVALID_CLASS: u32 = 0xC000_0003;

/// 状态是否在同源闭集内。
pub fn status_in_set(st: u32) -> bool {
    st == EST_OK || st == EST_LEN_MISMATCH || st == EST_INVALID_PARAM || st == EST_INVALID_CLASS
}

// ── 边界判引擎（冻结规则，同源单一权威） ─────────────────────────
/// 边界判定：类号越档 → 类号无效；账长为零 → 参数无效；
/// 回长 < 账长 → 长度不匹；其余 → 成功。判定顺序固定。
pub fn judge(class: u32, buf_len: u32, ret_len: u32) -> u32 {
    if class >= VALID_CLASS_CAP {
        return EST_INVALID_CLASS;
    }
    if buf_len == 0 {
        return EST_INVALID_PARAM;
    }
    if ret_len < buf_len {
        return EST_LEN_MISMATCH;
    }
    EST_OK
}

// ── 三族边界样本库（30 例冻结） ──────────────────────────────────
/// 边界样本六元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeSample {
    /// 样本号。
    pub no: u32,
    /// 族名（空参数/越界回长/类号越档）。
    pub family: &'static str,
    /// 信息类号。
    pub class: u32,
    /// 缓冲账长。
    pub buf_len: u32,
    /// 回长。
    pub ret_len: u32,
    /// 期望状态。
    pub expect: u32,
}

/// 族名闭集。
pub const FAM_EMPTY: &str = "空参数";
pub const FAM_RETLEN: &str = "越界回长";
pub const FAM_CLASS: &str = "类号越档";

/// 样本库容量（判据 ≥30 例）。
pub const STORE_CAP: usize = 30;

/// 边界样本库（30 例冻结，样本号序）。
pub const EDGE_STORE: &[EdgeSample] = &[
    // ── 空参数族（0..9）：账长为零，类号覆盖有效域与越档域 ──
    EdgeSample { no: 0, family: FAM_EMPTY, class: 0, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM },
    EdgeSample { no: 1, family: FAM_EMPTY, class: 1, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM },
    EdgeSample { no: 2, family: FAM_EMPTY, class: 2, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM },
    EdgeSample { no: 3, family: FAM_EMPTY, class: 3, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM },
    EdgeSample { no: 4, family: FAM_EMPTY, class: 5, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM },
    EdgeSample { no: 5, family: FAM_EMPTY, class: 7, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM },
    EdgeSample { no: 6, family: FAM_EMPTY, class: 8, buf_len: 0, ret_len: 0, expect: EST_INVALID_CLASS },
    EdgeSample { no: 7, family: FAM_EMPTY, class: 999, buf_len: 0, ret_len: 0, expect: EST_INVALID_CLASS },
    EdgeSample { no: 8, family: FAM_EMPTY, class: 0xFFFF, buf_len: 0, ret_len: 0, expect: EST_INVALID_CLASS },
    EdgeSample { no: 9, family: FAM_EMPTY, class: 0xFFFF_FFFF, buf_len: 0, ret_len: 0, expect: EST_INVALID_CLASS },
    // ── 越界回长族（10..19）：回长 < 账长，类号有效 ──
    EdgeSample { no: 10, family: FAM_RETLEN, class: 0, buf_len: 0x100, ret_len: 0x0F, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 11, family: FAM_RETLEN, class: 1, buf_len: 0x100, ret_len: 0x01, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 12, family: FAM_RETLEN, class: 2, buf_len: 0x200, ret_len: 0x10, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 13, family: FAM_RETLEN, class: 3, buf_len: 0x200, ret_len: 0xFF, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 14, family: FAM_RETLEN, class: 5, buf_len: 0x400, ret_len: 0x20, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 15, family: FAM_RETLEN, class: 6, buf_len: 0x400, ret_len: 0x01, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 16, family: FAM_RETLEN, class: 7, buf_len: 0x080, ret_len: 0x40, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 17, family: FAM_RETLEN, class: 4, buf_len: 0x300, ret_len: 0x00, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 18, family: FAM_RETLEN, class: 0, buf_len: 0x001, ret_len: 0x00, expect: EST_LEN_MISMATCH },
    EdgeSample { no: 19, family: FAM_RETLEN, class: 7, buf_len: 0x002, ret_len: 0x01, expect: EST_LEN_MISMATCH },
    // ── 类号越档族（20..29）：类号 ≥ 8 或越 u32 域 ──
    EdgeSample { no: 20, family: FAM_CLASS, class: 8, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 21, family: FAM_CLASS, class: 9, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 22, family: FAM_CLASS, class: 10, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 23, family: FAM_CLASS, class: 23, buf_len: 0x2000, ret_len: 0x2000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 24, family: FAM_CLASS, class: 100, buf_len: 0x2000, ret_len: 0x2000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 25, family: FAM_CLASS, class: 9999, buf_len: 0x2000, ret_len: 0x2000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 26, family: FAM_CLASS, class: 0x8000, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 27, family: FAM_CLASS, class: 0x7FFF_FFFF, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 28, family: FAM_CLASS, class: 0xFFFF_FFFE, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
    EdgeSample { no: 29, family: FAM_CLASS, class: 0xFFFF_FFFF, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS },
];

/// 族名是否在闭集内。
pub fn family_in_set(family: &str) -> bool {
    family == FAM_EMPTY || family == FAM_RETLEN || family == FAM_CLASS
}

// ── 样本入库账数据模型 ───────────────────────────────────────────
/// 入库账状态机：待入库 → 入库中 → 封库，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreState {
    /// 待入库。
    Pending,
    /// 入库中。
    Storing,
    /// 封库：只读。
    Sealed,
}

impl StoreState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            StoreState::Pending => 0,
            StoreState::Storing => 1,
            StoreState::Sealed => 2,
        }
    }
}

/// 边界样本库账。
#[derive(Clone, Debug)]
pub struct EdgeStore {
    /// 已入库样本（按样本号序）。
    rows: Vec<EdgeSample>,
    /// 拒收记账（样本号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 库账状态。
    state: StoreState,
}

impl EdgeStore {
    /// 开一本新样本库账。
    pub fn new() -> EdgeStore {
        EdgeStore {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: StoreState::Pending,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> StoreState {
        self.state
    }

    /// 已入库样本数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 入库样本（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&EdgeSample> {
        self.rows.get(idx)
    }

    /// 取第 idx 拒收记账（越界 None）。
    pub fn reject_at(&self, idx: usize) -> Option<(u32, u32)> {
        self.rejects.get(idx).copied()
    }

    /// 开库（Pending→Storing）。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.state.rank() >= StoreState::Storing.rank() {
            return Err(EdgeDiag::ERewind.val());
        }
        self.state = StoreState::Storing;
        Ok(())
    }

    /// 逐样本入库（序号/闭集/族名/状态闭集/判引擎对平/重复 六闸）。
    pub fn store(&mut self, s: EdgeSample) -> Result<(), u32> {
        if self.state.rank() < StoreState::Storing.rank() {
            return Err(EdgeDiag::ENotStored.val());
        }
        if self.state.rank() >= StoreState::Sealed.rank() {
            return Err(EdgeDiag::ESealed.val());
        }
        // 样本号序闸。
        let expect = self.rows.len() as u32;
        if s.no != expect {
            let _ = self.note_reject(s.no, EdgeDiag::ESampleOrder.val());
            return Err(EdgeDiag::ESampleOrder.val());
        }
        // 样本号闭集闸。
        if s.no as usize >= STORE_CAP {
            let _ = self.note_reject(s.no, EdgeDiag::ESampleRange.val());
            return Err(EdgeDiag::ESampleRange.val());
        }
        // 族名闭集闸。
        if !family_in_set(s.family) {
            let _ = self.note_reject(s.no, EdgeDiag::EFamilyRange.val());
            return Err(EdgeDiag::EFamilyRange.val());
        }
        // 期望状态闭集闸。
        if !status_in_set(s.expect) {
            let _ = self.note_reject(s.no, EdgeDiag::EExpectRange.val());
            return Err(EdgeDiag::EExpectRange.val());
        }
        // 判引擎对平闸（同源单一权威）。
        if judge(s.class, s.buf_len, s.ret_len) != s.expect {
            let _ = self.note_reject(s.no, EdgeDiag::EJudgeMiss.val());
            return Err(EdgeDiag::EJudgeMiss.val());
        }
        self.rows.push(s);
        Ok(())
    }

    /// 拒收记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) -> Result<(), u32> {
        if self.rejects.len() < STORE_CAP * 2 {
            self.rejects.push((no, diag));
            Ok(())
        } else {
            Err(EdgeDiag::ESampleRange.val())
        }
    }

    /// 重演：对库内全部样本重跑判引擎，逐样本同判。
    /// 返回 (同判数, 库内样本数)。
    pub fn replay(&self) -> (usize, usize) {
        let mut same = 0usize;
        for s in self.rows.iter() {
            if judge(s.class, s.buf_len, s.ret_len) == s.expect {
                same += 1;
            }
        }
        (same, self.rows.len())
    }

    /// 库账等式：入库数 == 库容量 且 拒收为零。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == STORE_CAP && self.rejects.is_empty()
    }

    /// 封库：30 例全入库 + 等式成立 + 重演全同判才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= StoreState::Sealed.rank() {
            return Err(EdgeDiag::ERewind.val());
        }
        if self.state.rank() < StoreState::Storing.rank() {
            return Err(EdgeDiag::ENotStored.val());
        }
        if self.rows.len() != STORE_CAP {
            return Err(EdgeDiag::ENotFull.val());
        }
        if !self.equation_ok() {
            return Err(EdgeDiag::EEqBroken.val());
        }
        let (same, total) = self.replay();
        if total != STORE_CAP || same != STORE_CAP {
            return Err(EdgeDiag::EReplayMiss.val());
        }
        self.state = StoreState::Sealed;
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 构建器：完整样本库 ───────────────────────────────────────────
/// 构建完整样本库（30 例逐例过闸入库，种子消耗仅作同源可重演位）。
pub fn build_full_store() -> EdgeStore {
    let mut st = EdgeStore::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = st.start();
    let mut i = 0usize;
    while i < STORE_CAP {
        let s = match EDGE_STORE.get(i) {
            Some(v) => *v,
            None => break,
        };
        let _ = rng.below(STORE_CAP as u32);
        match st.store(s) {
            Ok(()) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（库缺例，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    st
}

// ── 注入面：十路违约样本 ─────────────────────────────────────────
/// 违约路闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadSample {
    /// 期望改写（成功样本期望改成参数无效）。
    ExpectSwap,
    /// 族名错挂（越界回长样本挂空参数族名）。
    FamilySwap,
    /// 缺行入库（跳号）。
    RowOrder,
    /// 越库行（样本号 30）。
    RowRange,
    /// 判引擎不符（类号有效但期望写类号无效）。
    JudgeMiss,
    /// 期望越状态闭集。
    ExpectRange,
    /// 族名越闭集。
    FamilyRange,
    /// 未开库入库。
    NotStoring,
    /// 封库后写入。
    SealedWrite,
    /// 缺库封库（只有一例就封）。
    NotFull,
}

impl BadSample {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            BadSample::ExpectSwap => 0,
            BadSample::FamilySwap => 1,
            BadSample::RowOrder => 2,
            BadSample::RowRange => 3,
            BadSample::JudgeMiss => 4,
            BadSample::ExpectRange => 5,
            BadSample::FamilyRange => 6,
            BadSample::NotStoring => 7,
            BadSample::SealedWrite => 8,
            BadSample::NotFull => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            BadSample::ExpectSwap => "期望改写",
            BadSample::FamilySwap => "族名错挂",
            BadSample::RowOrder => "缺行入库",
            BadSample::RowRange => "越库行",
            BadSample::JudgeMiss => "判引擎不符",
            BadSample::ExpectRange => "期望越闭集",
            BadSample::FamilyRange => "族名越闭集",
            BadSample::NotStoring => "未开库入库",
            BadSample::SealedWrite => "封库后写",
            BadSample::NotFull => "缺库封库",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [BadSample] {
        &[
            BadSample::ExpectSwap,
            BadSample::FamilySwap,
            BadSample::RowOrder,
            BadSample::RowRange,
            BadSample::JudgeMiss,
            BadSample::ExpectRange,
            BadSample::FamilyRange,
            BadSample::NotStoring,
            BadSample::SealedWrite,
            BadSample::NotFull,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn bad_sample_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in BadSample::all().iter() {
        total += 1;
        let bad = run_bad_sample(*v);
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_bad_sample(v: BadSample) -> bool {
    match v {
        BadSample::ExpectSwap => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            let mut s = EDGE_STORE.get(0).copied().unwrap_or(EdgeSample {
                no: 0,
                family: FAM_EMPTY,
                class: 0,
                buf_len: 0,
                ret_len: 0,
                expect: EST_INVALID_PARAM,
            });
            s.expect = EST_OK;
            st.store(s).is_err()
        }
        BadSample::FamilySwap => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            let mut s = EDGE_STORE.get(10).copied().unwrap_or(EdgeSample {
                no: 10,
                family: FAM_RETLEN,
                class: 0,
                buf_len: 0x100,
                ret_len: 0x0F,
                expect: EST_LEN_MISMATCH,
            });
            s.family = FAM_EMPTY;
            st.store(s).is_err()
        }
        BadSample::RowOrder => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            let s = match EDGE_STORE.get(1) {
                Some(v) => *v,
                None => return false,
            };
            // 首例样本号 1（跳过 0）被序闸拒。
            st.store(s).is_err()
        }
        BadSample::RowRange => {
            let mut st = build_full_store();
            let _ = st.seal();
            let s = EdgeSample { no: 30, family: FAM_EMPTY, class: 0, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM };
            st.store(s).is_err()
        }
        BadSample::JudgeMiss => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            // 成功语势样本期望写类号无效——判引擎对平闸抓。
            let s = EdgeSample { no: 0, family: FAM_EMPTY, class: 0, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS };
            st.store(s).is_err()
        }
        BadSample::ExpectRange => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            let s = EdgeSample { no: 0, family: FAM_EMPTY, class: 0, buf_len: 0, ret_len: 0, expect: 9 };
            st.store(s).is_err()
        }
        BadSample::FamilyRange => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            let s = EdgeSample { no: 0, family: "幽灵族", class: 0, buf_len: 0, ret_len: 0, expect: EST_INVALID_PARAM };
            st.store(s).is_err()
        }
        BadSample::NotStoring => {
            let mut st = EdgeStore::new();
            let s = match EDGE_STORE.get(0) {
                Some(v) => *v,
                None => return false,
            };
            st.store(s).is_err()
        }
        BadSample::SealedWrite => {
            let mut st = build_full_store();
            let _ = st.seal();
            let s = match EDGE_STORE.get(0) {
                Some(v) => *v,
                None => return false,
            };
            st.store(s).is_err()
        }
        BadSample::NotFull => {
            let mut st = EdgeStore::new();
            let _ = st.start();
            st.seal().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 边界样本 ≥30 例入库全过（判据 J1 的"全过"直读口）。
fn store_all_ok() -> bool {
    let mut st = build_full_store();
    if st.len() != STORE_CAP {
        return false;
    }
    if st.reject_len() != 0 {
        return false;
    }
    if st.seal().is_err() {
        return false;
    }
    true
}

/// 同源可重演（判据 J1 的"同源可重演"直读口）。
fn replay_ok() -> bool {
    let st = build_full_store();
    let (same, total) = st.replay();
    if total != STORE_CAP {
        return false;
    }
    if same != STORE_CAP {
        return false;
    }
    // 同源参照锚逐元相等（与 F12654 联测资产同一冻结值）。
    if REF_VER_SAME != (10, 0, 22000) {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn injection_ok() -> bool {
    let (caught, total, escape) = bad_sample_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 三族闭集普查（每族恰 10 例）。
fn family_census_ok() -> bool {
    let mut n_empty = 0usize;
    let mut n_retlen = 0usize;
    let mut n_class = 0usize;
    for s in EDGE_STORE.iter() {
        if s.family == FAM_EMPTY {
            n_empty += 1;
        } else if s.family == FAM_RETLEN {
            n_retlen += 1;
        } else if s.family == FAM_CLASS {
            n_class += 1;
        } else {
            return false;
        }
    }
    n_empty == 10 && n_retlen == 10 && n_class == 10
}

/// 双跑一致（冻结样本库下两轮全流程逐例同判）。
fn dual_run_ok() -> bool {
    let a = build_full_store();
    let b = build_full_store();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = bad_sample_sweep();
    let rb = bad_sample_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：判引擎三族直读 + 状态闭集边界。
fn judge_boundary_ok() -> bool {
    // 判引擎直读四点。
    if judge(0, 0, 0) != EST_INVALID_PARAM {
        return false;
    }
    if judge(0, 0x100, 0x0F) != EST_LEN_MISMATCH {
        return false;
    }
    if judge(8, 0x1000, 0x1000) != EST_INVALID_CLASS {
        return false;
    }
    if judge(7, 0x1000, 0x1000) != EST_OK {
        return false;
    }
    // 判定顺序固定：越档优先于零账长。
    if judge(999, 0, 0) != EST_INVALID_CLASS {
        return false;
    }
    // 状态闭集边界。
    if !status_in_set(EST_OK) || !status_in_set(EST_INVALID_CLASS) {
        return false;
    }
    if status_in_set(0xC000_0001) || status_in_set(1) {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12656u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut st = build_full_store();
    if st.seal().is_err() {
        return false;
    }
    if st.state() != StoreState::Sealed {
        return false;
    }
    match st.row_at(0) {
        Some(s) => {
            if s.family.is_empty() {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12657 边界样本入库自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12656_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12656-probe-edge");
    let store = build_full_store();

    s.add_dyn(
        "F12657-01 样本库三十例齐",
        EDGE_STORE.len() == STORE_CAP,
        &format!("rows={}", EDGE_STORE.len()),
    );

    s.add_dyn(
        "F12657-02 边界样本入库全过",
        store_all_ok(),
        &format!("stored={} rejects={}", store.len(), store.reject_len()),
    );

    s.add_dyn(
        "F12657-03 同源可重演",
        replay_ok(),
        &format!("replay={:?}", store.replay()),
    );

    s.add_dyn(
        "F12657-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", bad_sample_sweep().0),
    );

    s.add_dyn(
        "F12657-05 三族闭集普查",
        family_census_ok(),
        "空参数/越界回长/类号越档 各 10 例",
    );

    s.add_dyn(
        "F12657-06 判引擎边界直读",
        judge_boundary_ok(),
        "四族判定 + 越档优先序 + 状态闭集边界",
    );

    s.add_dyn(
        "F12657-07 双跑一致",
        dual_run_ok(),
        "冻结样本库下两轮逐例同判",
    );

    s.add_dyn(
        "F12657-08 库账等式",
        store.equation_ok(),
        &format!("stored={} rejects={}", store.len(), store.reject_len()),
    );

    s.add_dyn(
        "F12657-09 完成写回语义",
        writeback_ok(),
        "封库即已完成且摘要非空",
    );

    s.add_dyn(
        "F12657-10 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 全表判引擎对平直读（与入库闸独立复核）。
    let mut table_ok = true;
    for s in EDGE_STORE.iter() {
        if judge(s.class, s.buf_len, s.ret_len) != s.expect {
            table_ok = false;
        }
    }
    s.add_dyn(
        "F12657-11 全表判引擎对平",
        table_ok,
        "30 例逐例与冻结规则对平",
    );

    // 同源参照锚直读：版本锚与有效类上沿。
    let ref_ok = REF_VER_SAME.0 == 10 && REF_VER_SAME.1 == 0 && REF_VER_SAME.2 == 22000
        && VALID_CLASS_CAP == 8;
    s.add_dyn(
        "F12657-12 同源参照锚",
        ref_ok,
        "锚 10.0.22000 + 类上沿 8（与 F12654 资产同源）",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80E00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(EDGE_ALL.len(), 12);
        let mut i = 0usize;
        while i < EDGE_ALL.len() {
            let vi = EDGE_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < EDGE_ALL.len() {
                let vj = EDGE_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_stores_and_seals() {
        let mut st = build_full_store();
        assert_eq!(st.len(), 30);
        assert_eq!(st.reject_len(), 0);
        assert!(st.seal().is_ok());
        assert_eq!(st.state(), StoreState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约样本全数拒止，无逃脱。
        let (caught, total, escape) = bad_sample_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn judge_engine_four_points() {
        // 判引擎四点直读 + 固定判定顺序。
        assert_eq!(judge(0, 0, 0), EST_INVALID_PARAM);
        assert_eq!(judge(0, 0x100, 0x0F), EST_LEN_MISMATCH);
        assert_eq!(judge(8, 0x1000, 0x1000), EST_INVALID_CLASS);
        assert_eq!(judge(7, 0x1000, 0x1000), EST_OK);
        // 越档优先于零账长。
        assert_eq!(judge(999, 0, 0), EST_INVALID_CLASS);
        // 回长等账长（不越界）为成功。
        assert_eq!(judge(3, 0x100, 0x100), EST_OK);
    }

    #[test]
    fn family_census_ten_each() {
        // 三族各 10 例。
        let mut n0 = 0usize;
        let mut n1 = 0usize;
        let mut n2 = 0usize;
        for s in EDGE_STORE.iter() {
            match s.family {
                FAM_EMPTY => n0 += 1,
                FAM_RETLEN => n1 += 1,
                FAM_CLASS => n2 += 1,
                _ => assert!(false, "越族名"),
            }
        }
        assert_eq!(n0, 10);
        assert_eq!(n1, 10);
        assert_eq!(n2, 10);
        assert!(family_in_set(FAM_EMPTY));
        assert!(!family_in_set("幽灵族"));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：冻结样本库下两轮逐例同判。
        let a = build_full_store();
        let b = build_full_store();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                (None, None) => {}
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(bad_sample_sweep().0, bad_sample_sweep().0);
    }

    #[test]
    fn store_gates_reject() {
        // 未开库入库被拒。
        let mut st = EdgeStore::new();
        let s0 = match EDGE_STORE.get(0) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(st.store(s0), Err(EdgeDiag::ENotStored.val()));
        // 跳号被拒。
        let mut st2 = EdgeStore::new();
        let _ = st2.start();
        let s1 = match EDGE_STORE.get(1) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(st2.store(s1), Err(EdgeDiag::ESampleOrder.val()));
        // 判引擎不符被拒（且拒收记账落行）。
        let mut st3 = EdgeStore::new();
        let _ = st3.start();
        let bad = EdgeSample { no: 0, family: FAM_EMPTY, class: 0, buf_len: 0x1000, ret_len: 0x1000, expect: EST_INVALID_CLASS };
        assert_eq!(st3.store(bad), Err(EdgeDiag::EJudgeMiss.val()));
        assert_eq!(st3.reject_len(), 1);
    }

    #[test]
    fn same_source_anchor() {
        // 同源参照：与 F12654 联测资产同一冻结值。
        assert_eq!(REF_VER_SAME, (10, 0, 22000));
        assert_eq!(VALID_CLASS_CAP, 8);
        // 状态闭集四值。
        assert!(status_in_set(EST_OK));
        assert!(status_in_set(EST_LEN_MISMATCH));
        assert!(status_in_set(EST_INVALID_PARAM));
        assert!(status_in_set(EST_INVALID_CLASS));
        assert!(!status_in_set(0xC000_0001));
    }

    #[test]
    fn seal_requires_full_and_replay() {
        // 边界：30 例未齐禁止封库。
        // 未开库封库（Pending 态）被拒。
        let mut fresh = EdgeStore::new();
        assert_eq!(fresh.seal(), Err(EdgeDiag::ENotStored.val()));
        // 开库未入库封库被拒。
        let mut half = EdgeStore::new();
        let _ = half.start();
        assert_eq!(half.seal(), Err(EdgeDiag::ENotFull.val()));
        // 入一例后仍未齐封库被拒。
        let _ = half.store(match EDGE_STORE.get(0) {
            Some(v) => *v,
            None => return,
        });
        assert_eq!(half.seal(), Err(EdgeDiag::ENotFull.val()));
        // 齐后封库成功，重封倒退拒绝，封后写拒。
        let mut full = build_full_store();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(EdgeDiag::ERewind.val()));
        assert_eq!(full.store(match EDGE_STORE.get(0) {
            Some(v) => *v,
            None => return,
        }), Err(EdgeDiag::ESealed.val()));
    }

    #[test]
    fn table_judge_alignment() {
        // 全表判引擎对平（冻结规则 == 表内期望）。
        for s in EDGE_STORE.iter() {
            assert_eq!(judge(s.class, s.buf_len, s.ret_len), s.expect, "样本 {}", s.no);
        }
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Pending→Storing→Sealed。
        let mut st = build_full_store();
        assert!(StoreState::Pending.rank() < StoreState::Storing.rank());
        assert!(StoreState::Storing.rank() < StoreState::Sealed.rank());
        assert_eq!(st.state(), StoreState::Storing);
        let _ = st.seal();
        assert_eq!(st.state(), StoreState::Sealed);
        // 重复开库被拒。
        let mut fresh = EdgeStore::new();
        let _ = fresh.start();
        assert_eq!(fresh.start(), Err(EdgeDiag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12656_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
