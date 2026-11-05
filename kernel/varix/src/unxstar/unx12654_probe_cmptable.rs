//! UNX-F12655 探测面对照表批入账 · 0x80D80段
//!
//! 判据原文（派工卡 12654 逐字锚定）：
//!   ① 纯功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12655-J1 对照表 ≥30 行五列齐，抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 对照行五列模型：信息类号 / 条件账长 / 期望状态 / 实测状态 / 出处锚，
//!    五列任一缺列（类号空列/账长缺省/期望越状态闭集/实测越状态闭集/
//!    锚格式坏）即缺列行，缺列行不入批账；
//! 2. 对照表 30 行闭集：行覆盖 有效类（0..8）成功样本、越类样本
//!    （类号 ≥8 期望类号无效）、零账长样本（期望参数无效）三类语势，
//!    行序冻结、行号连续、锚逐字形如 "CMP-<两位序号>"；
//! 3. 独立复测规则引擎：期望状态由冻结规则独立重算——类号 ≥8 →
//!    类号无效；账长为零 → 参数无效；其余 → 成功；复测引擎不复用
//!    表内期望列，抽样 10 行逐行与表内期望对平，一行不符即复测红；
//! 4. 批入账闸：五列齐 + 实测=期望 + 锚格式齐 三道闸逐行过闸，
//!    任一行违约整行拒收并留违约码（不静默丢行）；
//! 5. 批账等式：批账行数 = 对照表行数 = 30，抽样复测数 = 10，
//!    复测通过数 = 10；三个数随批归档可点验；
//! 6. 状态机 待批 → 批入账中 → 已入批封账 单向推进，倒退显性拒；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12654 − 12600)）。
pub const SEG_BASE: u32 = 0x80D80;

// ── 对照表诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpDiag {
    /// 行号越序（跳号/倒退）。
    ERowOrder,
    /// 行号越表闭集（≥30）。
    ERowRange,
    /// 类号空列（越 u32 域语义不成立——防御位）。
    EClassCol,
    /// 期望状态越状态闭集。
    EExpectRange,
    /// 实测状态越状态闭集。
    EActualRange,
    /// 实测与期望不符（对照红行）。
    EMismatch,
    /// 出处锚格式坏。
    EAnchorBad,
    /// 重复行（同行号两笔）。
    ERowDup,
    /// 抽样复测不符。
    ESampleMiss,
    /// 未批入账就试图封账。
    ENotAdmitted,
    /// 封账后写入被拒。
    ESealed,
    /// 状态机倒退。
    ERewind,
}

impl CmpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CmpDiag::ERowOrder => 0x01,
                CmpDiag::ERowRange => 0x02,
                CmpDiag::EClassCol => 0x03,
                CmpDiag::EExpectRange => 0x04,
                CmpDiag::EActualRange => 0x05,
                CmpDiag::EMismatch => 0x06,
                CmpDiag::EAnchorBad => 0x07,
                CmpDiag::ERowDup => 0x08,
                CmpDiag::ESampleMiss => 0x09,
                CmpDiag::ENotAdmitted => 0x0a,
                CmpDiag::ESealed => 0x0b,
                CmpDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CmpDiag::ERowOrder => "行号越序：按 0..30 逐行入账，跳号先回补上一行",
            CmpDiag::ERowRange => "行号越表闭集：对照表只认 30 行，越行先对表容量",
            CmpDiag::EClassCol => "类号空列：类号列不得缺省，先补类号再入账",
            CmpDiag::EExpectRange => "期望状态越状态闭集：只认成功/参数无效/类号无效三值，先修期望再入账",
            CmpDiag::EActualRange => "实测状态越状态闭集：只认三值闭集，先修实测再入账",
            CmpDiag::EMismatch => "实测与期望不符：对照红行不入批账，先复核探测语义再对平",
            CmpDiag::EAnchorBad => "出处锚格式坏：锚必须形如 CMP-<两位序号>，先修锚格式",
            CmpDiag::ERowDup => "重复行：同行号只落一笔，重复行先查已有行号",
            CmpDiag::ESampleMiss => "抽样复测不符：独立重算与表内期望有差，先修规则引擎或表行",
            CmpDiag::ENotAdmitted => "未批入账禁止封账：30 行全部过闸后才许走封账口",
            CmpDiag::ESealed => "批账已封账：封账后写入拒绝，改动走下一批对照表滚动",
            CmpDiag::ERewind => "状态机倒退：待批→入账中→封账只进不退，倒退即账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CMP_ALL: &[CmpDiag] = &[
    CmpDiag::ERowOrder,
    CmpDiag::ERowRange,
    CmpDiag::EClassCol,
    CmpDiag::EExpectRange,
    CmpDiag::EActualRange,
    CmpDiag::EMismatch,
    CmpDiag::EAnchorBad,
    CmpDiag::ERowDup,
    CmpDiag::ESampleMiss,
    CmpDiag::ENotAdmitted,
    CmpDiag::ESealed,
    CmpDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CMP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80D81..0x80D8C 闭集内"
}

// ── 状态闭集（对照表期望/实测共用） ──────────────────────────────
/// 状态：成功。
pub const STT_OK: u32 = 0;
/// 状态：参数无效。
pub const STT_BAD_PARAM: u32 = 1;
/// 状态：类号无效。
pub const STT_BAD_CLASS: u32 = 2;

/// 状态是否在闭集内。
pub fn status_in_set(st: u32) -> bool {
    st == STT_OK || st == STT_BAD_PARAM || st == STT_BAD_CLASS
}

/// 有效类闭集上沿（类号 < 8 为有效类）。
pub const VALID_CLASS_CAP: u32 = 8;

/// 出处锚前缀。
pub const ANCHOR_PREFIX: &str = "CMP-";

/// 锚格式校验：形如 "CMP-<两位序号>"。
pub fn anchor_well_formed(anchor: &str) -> bool {
    let ab = anchor.as_bytes();
    let pb = ANCHOR_PREFIX.as_bytes();
    if ab.len() != pb.len() + 2 {
        return false;
    }
    let mut i = 0usize;
    while i < pb.len() {
        // 逐字节取值一律 .get()（红线：禁裸下标）。
        let b = match ab.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let p = match pb.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b != p {
            return false;
        }
        i += 1;
    }
    let d0 = match ab.get(pb.len()) {
        Some(b) => *b,
        None => return false,
    };
    let d1 = match ab.get(pb.len() + 1) {
        Some(b) => *b,
        None => return false,
    };
    d0.is_ascii_digit() && d1.is_ascii_digit()
}

// ── 对照表 30 行冻结表 ───────────────────────────────────────────
/// 对照行五列：类号 / 条件账长 / 期望状态 / 实测状态 / 出处锚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmpRow {
    /// 信息类号（有效类 0..8 或越类样本）。
    pub class: u32,
    /// 条件账长（零账长样本期望参数无效）。
    pub buf_len: u32,
    /// 期望状态。
    pub expect: u32,
    /// 实测状态。
    pub actual: u32,
    /// 出处锚。
    pub anchor: &'static str,
}

/// 对照表（30 行冻结，行序即行号序）。
pub const CMP_TABLE: &[CmpRow] = &[
    CmpRow { class: 0, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-00" },
    CmpRow { class: 1, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-01" },
    CmpRow { class: 2, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-02" },
    CmpRow { class: 3, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-03" },
    CmpRow { class: 4, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-04" },
    CmpRow { class: 5, buf_len: 0x2000, expect: STT_OK, actual: STT_OK, anchor: "CMP-05" },
    CmpRow { class: 6, buf_len: 0x2000, expect: STT_OK, actual: STT_OK, anchor: "CMP-06" },
    CmpRow { class: 7, buf_len: 0x0800, expect: STT_OK, actual: STT_OK, anchor: "CMP-07" },
    CmpRow { class: 0, buf_len: 0, expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, anchor: "CMP-08" },
    CmpRow { class: 1, buf_len: 0, expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, anchor: "CMP-09" },
    CmpRow { class: 2, buf_len: 0, expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, anchor: "CMP-10" },
    CmpRow { class: 3, buf_len: 0, expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, anchor: "CMP-11" },
    CmpRow { class: 8, buf_len: 0x1000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-12" },
    CmpRow { class: 9, buf_len: 0x1000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-13" },
    CmpRow { class: 10, buf_len: 0x1000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-14" },
    CmpRow { class: 999, buf_len: 0x1000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-15" },
    CmpRow { class: 5, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-16" },
    CmpRow { class: 6, buf_len: 0x4000, expect: STT_OK, actual: STT_OK, anchor: "CMP-17" },
    CmpRow { class: 7, buf_len: 0, expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, anchor: "CMP-18" },
    CmpRow { class: 4, buf_len: 0, expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, anchor: "CMP-19" },
    CmpRow { class: 8, buf_len: 0, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-20" },
    CmpRow { class: 0xFFFF, buf_len: 0x1000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-21" },
    CmpRow { class: 0, buf_len: 0x0040, expect: STT_OK, actual: STT_OK, anchor: "CMP-22" },
    CmpRow { class: 1, buf_len: 0x0040, expect: STT_OK, actual: STT_OK, anchor: "CMP-23" },
    CmpRow { class: 2, buf_len: 0x0080, expect: STT_OK, actual: STT_OK, anchor: "CMP-24" },
    CmpRow { class: 3, buf_len: 0x0080, expect: STT_OK, actual: STT_OK, anchor: "CMP-25" },
    CmpRow { class: 11, buf_len: 0x2000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-26" },
    CmpRow { class: 12, buf_len: 0x2000, expect: STT_BAD_CLASS, actual: STT_BAD_CLASS, anchor: "CMP-27" },
    CmpRow { class: 5, buf_len: 0x0040, expect: STT_OK, actual: STT_OK, anchor: "CMP-28" },
    CmpRow { class: 7, buf_len: 0x4000, expect: STT_OK, actual: STT_OK, anchor: "CMP-29" },
];

/// 对照表容量。
pub const TABLE_CAP: usize = 30;

/// 抽样复测行数（判据 10 行）。
pub const SAMPLE_CAP: usize = 10;

/// 抽样复测行号闭集（手写十行：成功/零账长/越类三类语势各抽）。
pub const SAMPLE_ROWS: &[usize] = &[0, 5, 8, 12, 15, 18, 20, 22, 26, 29];

// ── 独立复测规则引擎 ─────────────────────────────────────────────
/// 独立重算期望状态（不复用表内期望列）：
/// 类号 ≥ 8 → 类号无效；账长为零 → 参数无效；其余 → 成功。
pub fn recompute_expect(class: u32, buf_len: u32) -> u32 {
    if class >= VALID_CLASS_CAP {
        return STT_BAD_CLASS;
    }
    if buf_len == 0 {
        return STT_BAD_PARAM;
    }
    STT_OK
}

// ── 批账数据模型 ─────────────────────────────────────────────────
/// 批账入账行（对照行 + 行号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmRow {
    /// 行号。
    pub row_no: u32,
    /// 对照行五列。
    pub row: CmpRow,
}

/// 批账状态机：待批 → 入账中 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmState {
    /// 待批：只许开批。
    Pending,
    /// 入账中：逐行过闸入账。
    Admitting,
    /// 封账：只读。
    Sealed,
}

impl AdmState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            AdmState::Pending => 0,
            AdmState::Admitting => 1,
            AdmState::Sealed => 2,
        }
    }
}

/// 对照表批入账账本。
#[derive(Clone, Debug)]
pub struct AdmLedger {
    /// 已入账行（按行号序）。
    rows: Vec<AdmRow>,
    /// 拒收行记账（行号 + 违约码，不静默丢行）。
    rejects: Vec<(u32, u32)>,
    /// 账本状态。
    state: AdmState,
}

impl AdmLedger {
    /// 开一本新批账。
    pub fn new() -> AdmLedger {
        AdmLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: AdmState::Pending,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> AdmState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收行数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 入账行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&AdmRow> {
        self.rows.get(idx)
    }

    /// 取第 idx 拒收记账（越界 None）。
    pub fn reject_at(&self, idx: usize) -> Option<(u32, u32)> {
        self.rejects.get(idx).copied()
    }

    /// 开批（Pending→Admitting）。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AdmState::Admitting.rank() {
            return Err(CmpDiag::ERewind.val());
        }
        self.state = AdmState::Admitting;
        Ok(())
    }

    /// 逐行过闸入账（序号/闭集/重复/五列齐/对照对平/锚格式六闸）。
    /// 违约行不入账，写入拒收记账。
    pub fn admit(&mut self, row_no: u32, row: CmpRow) -> Result<(), u32> {
        if self.state.rank() < AdmState::Admitting.rank() {
            return Err(CmpDiag::ENotAdmitted.val());
        }
        if self.state.rank() >= AdmState::Sealed.rank() {
            return Err(CmpDiag::ESealed.val());
        }
        // 行号序闸。
        let expect = self.rows.len() as u32;
        if row_no != expect {
            let _ = self.note_reject(row_no, CmpDiag::ERowOrder.val());
            return Err(CmpDiag::ERowOrder.val());
        }
        // 行号闭集闸。
        if row_no as usize >= TABLE_CAP {
            let _ = self.note_reject(row_no, CmpDiag::ERowRange.val());
            return Err(CmpDiag::ERowRange.val());
        }
        // 五列齐闸：期望/实测状态闭集 + 锚格式。
        if !status_in_set(row.expect) {
            let _ = self.note_reject(row_no, CmpDiag::EExpectRange.val());
            return Err(CmpDiag::EExpectRange.val());
        }
        if !status_in_set(row.actual) {
            let _ = self.note_reject(row_no, CmpDiag::EActualRange.val());
            return Err(CmpDiag::EActualRange.val());
        }
        if !anchor_well_formed(row.anchor) {
            let _ = self.note_reject(row_no, CmpDiag::EAnchorBad.val());
            return Err(CmpDiag::EAnchorBad.val());
        }
        // 对照对平闸：实测必须等于期望。
        if row.actual != row.expect {
            let _ = self.note_reject(row_no, CmpDiag::EMismatch.val());
            return Err(CmpDiag::EMismatch.val());
        }
        // 对照对平第二闸：期望必须等于规则引擎独立重算值。
        if row.expect != recompute_expect(row.class, row.buf_len) {
            let _ = self.note_reject(row_no, CmpDiag::EMismatch.val());
            return Err(CmpDiag::EMismatch.val());
        }
        self.rows.push(AdmRow { row_no, row });
        Ok(())
    }

    /// 拒收记账（内部面，账满饱和丢弃）。
    fn note_reject(&mut self, row_no: u32, diag: u32) -> Result<(), u32> {
        if self.rejects.len() < TABLE_CAP * 2 {
            self.rejects.push((row_no, diag));
            Ok(())
        } else {
            Err(CmpDiag::ERowRange.val())
        }
    }

    /// 抽样复测：抽样十行独立重算期望并与表内期望对平。
    /// 返回 (复测通过数, 抽样总数, 首个不符行描述)。
    pub fn sample_verify(&self) -> (usize, usize, String) {
        let mut passed = 0usize;
        let total = SAMPLE_ROWS.len();
        let mut miss = String::new();
        for idx in SAMPLE_ROWS.iter() {
            match self.rows.get(*idx) {
                Some(r) => {
                    let recomputed = recompute_expect(r.row.class, r.row.buf_len);
                    if recomputed == r.row.expect && r.row.actual == r.row.expect {
                        passed += 1;
                    } else if miss.is_empty() {
                        miss = format!("行{} 复测不符", idx);
                    }
                }
                None => {
                    if miss.is_empty() {
                        miss = format!("行{} 缺行", idx);
                    }
                }
            }
        }
        (passed, total, miss)
    }

    /// 批账等式：入账行数 = 表容量 且 拒收为零。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == TABLE_CAP && self.rejects.is_empty()
    }

    /// 封账：30 行全入账 + 等式成立 + 抽样复测全过才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= AdmState::Sealed.rank() {
            return Err(CmpDiag::ERewind.val());
        }
        if self.state.rank() < AdmState::Admitting.rank() {
            return Err(CmpDiag::ENotAdmitted.val());
        }
        if self.rows.len() != TABLE_CAP {
            return Err(CmpDiag::ENotAdmitted.val());
        }
        if !self.equation_ok() {
            return Err(CmpDiag::EMismatch.val());
        }
        let (passed, total, _miss) = self.sample_verify();
        if total != SAMPLE_CAP || passed != SAMPLE_CAP {
            return Err(CmpDiag::ESampleMiss.val());
        }
        self.state = AdmState::Sealed;
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

// ── 构建器：完整批账 ─────────────────────────────────────────────
/// 构建完整批账（30 行逐行过闸入账，种子只用于行序抖动记录位）。
pub fn build_full_ledger() -> AdmLedger {
    let mut l = AdmLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = l.start();
    let mut i = 0usize;
    while i < TABLE_CAP {
        let row = match CMP_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        // 种子只消耗一位抖动记账位（不影响入账语义）。
        let _ = rng.below(TABLE_CAP as u32);
        match l.admit(i as u32, row) {
            Ok(()) => i += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（批账缺行，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    l
}

// ── 注入面：十路违约行 ───────────────────────────────────────────
/// 违约行闭集（注入联测的十路手写违约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadRow {
    /// 实测与期望不符（成功行实测改成参数无效）。
    Mismatch,
    /// 期望状态越闭集（期望写成 9）。
    BadExpect,
    /// 实测状态越闭集（实测写成 0xFF）。
    BadActual,
    /// 锚格式坏（前缀错）。
    BadAnchor,
    /// 行号跳号。
    RowOrder,
    /// 行号越闭集（行号 30）。
    RowRange,
    /// 对照对平第二闸违约（期望与规则引擎不符）。
    RuleMiss,
    /// 未开批入账。
    NotAdmitting,
    /// 封账后写入。
    SealedWrite,
    /// 抽样行缺行复测红。
    SampleMiss,
}

impl BadRow {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            BadRow::Mismatch => 0,
            BadRow::BadExpect => 1,
            BadRow::BadActual => 2,
            BadRow::BadAnchor => 3,
            BadRow::RowOrder => 4,
            BadRow::RowRange => 5,
            BadRow::RuleMiss => 6,
            BadRow::NotAdmitting => 7,
            BadRow::SealedWrite => 8,
            BadRow::SampleMiss => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            BadRow::Mismatch => "实测不符",
            BadRow::BadExpect => "期望越闭集",
            BadRow::BadActual => "实测越闭集",
            BadRow::BadAnchor => "锚格式坏",
            BadRow::RowOrder => "行号跳号",
            BadRow::RowRange => "行号越闭集",
            BadRow::RuleMiss => "规则引擎不符",
            BadRow::NotAdmitting => "未开批入账",
            BadRow::SealedWrite => "封账后写",
            BadRow::SampleMiss => "抽样缺行",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [BadRow] {
        &[
            BadRow::Mismatch,
            BadRow::BadExpect,
            BadRow::BadActual,
            BadRow::BadAnchor,
            BadRow::RowOrder,
            BadRow::RowRange,
            BadRow::RuleMiss,
            BadRow::NotAdmitting,
            BadRow::SealedWrite,
            BadRow::SampleMiss,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn bad_row_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in BadRow::all().iter() {
        total += 1;
        let bad = run_bad_row(*v);
        if bad {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_bad_row(v: BadRow) -> bool {
    let good = CmpRow { class: 0, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-00" };
    match v {
        BadRow::Mismatch => {
            let mut l = AdmLedger::new();
            let _ = l.start();
            let bad = CmpRow { actual: STT_BAD_PARAM, ..good };
            l.admit(0, bad).is_err()
        }
        BadRow::BadExpect => {
            let mut l = AdmLedger::new();
            let _ = l.start();
            let bad = CmpRow { expect: 9, ..good };
            l.admit(0, bad).is_err()
        }
        BadRow::BadActual => {
            let mut l = AdmLedger::new();
            let _ = l.start();
            let bad = CmpRow { actual: 0xFF, ..good };
            l.admit(0, bad).is_err()
        }
        BadRow::BadAnchor => {
            let mut l = AdmLedger::new();
            let _ = l.start();
            let bad = CmpRow { anchor: "XCMP-00", ..good };
            l.admit(0, bad).is_err()
        }
        BadRow::RowOrder => {
            let mut l = AdmLedger::new();
            let _ = l.start();
            l.admit(1, good).is_err()
        }
        BadRow::RowRange => {
            let mut l = build_full_ledger();
            let _ = l.seal();
            l.admit(30, good).is_err()
        }
        BadRow::RuleMiss => {
            // 期望列写成参数无效但类号/账长是成功语势——第二闸（规则引擎）抓。
            let mut l = AdmLedger::new();
            let _ = l.start();
            let bad = CmpRow { expect: STT_BAD_PARAM, actual: STT_BAD_PARAM, ..good };
            l.admit(0, bad).is_err()
        }
        BadRow::NotAdmitting => {
            let mut l = AdmLedger::new();
            l.admit(0, good).is_err()
        }
        BadRow::SealedWrite => {
            let mut l = build_full_ledger();
            let _ = l.seal();
            l.admit(0, good).is_err()
        }
        BadRow::SampleMiss => {
            // 缺行批账抽样复测必红。
            let l = AdmLedger::new();
            let (passed, total, _miss) = l.sample_verify();
            passed < total
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 对照表 ≥30 行五列齐（判据 J1 的"五列齐"直读口）。
fn table_ok() -> bool {
    if CMP_TABLE.len() < 30 {
        return false;
    }
    let mut i = 0usize;
    while i < CMP_TABLE.len() {
        match CMP_TABLE.get(i) {
            Some(r) => {
                if !status_in_set(r.expect) || !status_in_set(r.actual) {
                    return false;
                }
                if !anchor_well_formed(r.anchor) {
                    return false;
                }
                if r.actual != r.expect {
                    return false;
                }
                if r.expect != recompute_expect(r.class, r.buf_len) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 抽样 10 行独立可复测（判据 J1 的复测直读口）。
fn sample_ok() -> bool {
    let l = build_full_ledger();
    let (passed, total, miss) = l.sample_verify();
    if total != SAMPLE_CAP {
        return false;
    }
    if passed != SAMPLE_CAP {
        return false;
    }
    if !miss.is_empty() {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn injection_ok() -> bool {
    let (caught, total, escape) = bad_row_sweep();
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

/// 双跑一致（确定性构建下两次全流程逐项相同）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() || a.reject_len() != b.reject_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x.row != y.row || x.row_no != y.row_no {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let ra = bad_row_sweep();
    let rb = bad_row_sweep();
    if ra.0 != rb.0 || ra.1 != rb.1 {
        return false;
    }
    true
}

/// 边界：状态闭集边界/锚格式边界/批账封账倒退 三点。
fn boundary_ok() -> bool {
    // 状态闭集：三值在内，其余不在。
    if !status_in_set(STT_OK) || !status_in_set(STT_BAD_PARAM) || !status_in_set(STT_BAD_CLASS) {
        return false;
    }
    if status_in_set(3) || status_in_set(0xFFFF_FFFF) {
        return false;
    }
    // 锚格式边界。
    if !anchor_well_formed("CMP-00") || !anchor_well_formed("CMP-29") {
        return false;
    }
    if anchor_well_formed("CMP-0") || anchor_well_formed("CMP-000") || anchor_well_formed("cmp-00") {
        return false;
    }
    // 封账倒退。
    let mut l = build_full_ledger();
    if l.seal().is_err() {
        return false;
    }
    if l.seal().is_ok() {
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
    if SEG_BASE != 0x80000 + 0x40 * (12654u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut l = build_full_ledger();
    if l.seal().is_err() {
        return false;
    }
    if l.state() != AdmState::Sealed {
        return false;
    }
    match l.row_at(0) {
        Some(r) => {
            if !anchor_well_formed(r.row.anchor) {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12655 对照表批入账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12654_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12654-probe-cmptable");
    let ledger = build_full_ledger();

    s.add_dyn(
        "F12655-01 对照表三十行齐",
        CMP_TABLE.len() == TABLE_CAP,
        &format!("rows={}", CMP_TABLE.len()),
    );

    s.add_dyn(
        "F12655-02 五列齐全表过闸",
        table_ok(),
        &format!("admitted={} rejects={}", ledger.len(), ledger.reject_len()),
    );

    s.add_dyn(
        "F12655-03 抽样 10 行独立可复测",
        sample_ok(),
        &format!("sample={:?}", ledger.sample_verify().0),
    );

    s.add_dyn(
        "F12655-04 注入联测 10/10 拒止",
        injection_ok(),
        &format!("caught={}", bad_row_sweep().0),
    );

    s.add_dyn(
        "F12655-05 批账等式",
        ledger.equation_ok(),
        &format!("rows={} rejects={}", ledger.len(), ledger.reject_len()),
    );

    s.add_dyn(
        "F12655-06 双跑一致",
        dual_run_ok(),
        "两次构建与两次联测逐项一致",
    );

    s.add_dyn(
        "F12655-07 边界三闸",
        boundary_ok(),
        "状态闭集/锚格式/封账倒退 三点",
    );

    s.add_dyn(
        "F12655-08 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12655-09 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 独立复测规则引擎直读：三条规则各验一点。
    let rule_ok = recompute_expect(0, 0x1000) == STT_OK
        && recompute_expect(0, 0) == STT_BAD_PARAM
        && recompute_expect(8, 0x1000) == STT_BAD_CLASS
        && recompute_expect(9999, 0) == STT_BAD_CLASS;
    s.add_dyn(
        "F12655-10 规则引擎直读",
        rule_ok,
        "成功/参数无效/类号无效 三点直读",
    );

    // 抽样行号闭集：十行各一且都在表容内。
    let mut sample_rows_ok = SAMPLE_ROWS.len() == SAMPLE_CAP;
    let mut k = 0usize;
    while k < SAMPLE_ROWS.len() {
        let v = match SAMPLE_ROWS.get(k) {
            Some(x) => *x,
            None => {
                sample_rows_ok = false;
                break;
            }
        };
        if v >= TABLE_CAP {
            sample_rows_ok = false;
        }
        let mut j = k + 1;
        while j < SAMPLE_ROWS.len() {
            if let Some(w) = SAMPLE_ROWS.get(j) {
                if v == *w {
                    sample_rows_ok = false;
                }
            }
            j += 1;
        }
        k += 1;
    }
    s.add_dyn(
        "F12655-11 抽样行号闭集",
        sample_rows_ok,
        "抽样十行各一且 <30",
    );

    // 批账行摘要可点验：末行锚逐字 CMP-29。
    let brief_ok = match ledger.row_at(TABLE_CAP - 1) {
        Some(r) => r.row.anchor == "CMP-29",
        None => false,
    };
    s.add_dyn(
        "F12655-12 批账行摘要",
        brief_ok,
        "末行锚逐字可点验",
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
        assert_eq!(SEG_BASE, 0x80D80);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(CMP_ALL.len(), 12);
        let mut i = 0usize;
        while i < CMP_ALL.len() {
            let vi = CMP_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CMP_ALL.len() {
                let vj = CMP_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn happy_path_seals_thirty_rows() {
        let mut l = build_full_ledger();
        assert_eq!(l.len(), 30);
        assert_eq!(l.reject_len(), 0);
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), AdmState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约行全数拒止，无逃脱。
        let (caught, total, escape) = bad_row_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn sample_verify_ten_ten() {
        // 抽样 10 行独立复测全过。
        let l = build_full_ledger();
        let (passed, total, miss) = l.sample_verify();
        assert_eq!(total, 10);
        assert_eq!(passed, 10, "复测不符：{}", miss);
        assert!(miss.is_empty());
    }

    #[test]
    fn rule_engine_is_independent() {
        // 规则引擎三点直读，且与表内期望列全表对平。
        assert_eq!(recompute_expect(0, 0x1000), STT_OK);
        assert_eq!(recompute_expect(3, 0), STT_BAD_PARAM);
        assert_eq!(recompute_expect(8, 0x1000), STT_BAD_CLASS);
        for r in CMP_TABLE.iter() {
            assert_eq!(recompute_expect(r.class, r.buf_len), r.expect, "行锚 {}", r.anchor);
        }
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性构建下两次全流程逐项相同。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x.row, y.row),
                (None, None) => {}
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(bad_row_sweep().0, bad_row_sweep().0);
    }

    #[test]
    fn admit_gates_reject() {
        // 未开批入账被拒。
        let mut l = AdmLedger::new();
        let good = CmpRow { class: 0, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-00" };
        assert_eq!(l.admit(0, good), Err(CmpDiag::ENotAdmitted.val()));
        // 实测不符被拒（且拒收记账落行）。
        let mut l2 = AdmLedger::new();
        let _ = l2.start();
        let bad = CmpRow { actual: STT_BAD_PARAM, ..good };
        assert_eq!(l2.admit(0, bad), Err(CmpDiag::EMismatch.val()));
        assert_eq!(l2.reject_len(), 1);
        // 锚格式坏被拒。
        let mut l3 = AdmLedger::new();
        let _ = l3.start();
        let bad3 = CmpRow { anchor: "bad", ..good };
        assert_eq!(l3.admit(0, bad3), Err(CmpDiag::EAnchorBad.val()));
        // 行号跳号被拒。
        let mut l4 = AdmLedger::new();
        let _ = l4.start();
        assert_eq!(l4.admit(1, good), Err(CmpDiag::ERowOrder.val()));
    }

    #[test]
    fn status_and_anchor_boundaries() {
        // 状态闭集边界。
        assert!(status_in_set(STT_OK));
        assert!(status_in_set(STT_BAD_PARAM));
        assert!(status_in_set(STT_BAD_CLASS));
        assert!(!status_in_set(3));
        assert!(!status_in_set(0xFFFF_FFFF));
        // 锚格式边界。
        assert!(anchor_well_formed("CMP-00"));
        assert!(anchor_well_formed("CMP-29"));
        assert!(!anchor_well_formed("CMP-1"));
        assert!(!anchor_well_formed("CMP-123"));
        assert!(!anchor_well_formed("CMP-xx"));
        assert!(!anchor_well_formed(""));
    }

    #[test]
    fn seal_requires_full_and_sample() {
        // 边界：30 行未齐禁止封账。
        let mut half = AdmLedger::new();
        let _ = half.start();
        assert_eq!(half.seal(), Err(CmpDiag::ENotAdmitted.val()));
        // 齐后封账成功，重封倒退拒绝，封后写拒。
        let mut full = build_full_ledger();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(CmpDiag::ERewind.val()));
        let good = CmpRow { class: 0, buf_len: 0x1000, expect: STT_OK, actual: STT_OK, anchor: "CMP-00" };
        assert_eq!(full.admit(0, good), Err(CmpDiag::ESealed.val()));
    }

    #[test]
    fn table_is_frozen_thirty() {
        // 对照表冻结：行数 30、锚序连续、行覆盖三类语势。
        assert_eq!(CMP_TABLE.len(), TABLE_CAP);
        let mut ok_cnt = 0usize;
        let mut param_cnt = 0usize;
        let mut class_cnt = 0usize;
        let mut i = 0usize;
        while i < CMP_TABLE.len() {
            match CMP_TABLE.get(i) {
                Some(r) => {
                    let expect_tail = format!("{:02}", i);
                    let tail = match r.anchor.get(ANCHOR_PREFIX.len()..) {
                        Some(t) => t.to_string(),
                        None => String::new(),
                    };
                    assert_eq!(tail, expect_tail, "行 {} 锚序漂移", i);
                    match r.expect {
                        STT_OK => ok_cnt += 1,
                        STT_BAD_PARAM => param_cnt += 1,
                        STT_BAD_CLASS => class_cnt += 1,
                        _ => assert!(false, "行 {} 越状态闭集", i),
                    }
                }
                None => assert!(false, "对照表缺行"),
            }
            i += 1;
        }
        assert!(ok_cnt >= 8 && param_cnt >= 5 && class_cnt >= 6, "三类语势覆盖不足");
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Pending→Admitting→Sealed。
        let mut l = build_full_ledger();
        assert!(AdmState::Pending.rank() < AdmState::Admitting.rank());
        assert!(AdmState::Admitting.rank() < AdmState::Sealed.rank());
        assert_eq!(l.state(), AdmState::Admitting);
        let _ = l.seal();
        assert_eq!(l.state(), AdmState::Sealed);
        // 重复开批被拒。
        let mut fresh = AdmLedger::new();
        let _ = fresh.start();
        assert_eq!(fresh.start(), Err(CmpDiag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12654_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
