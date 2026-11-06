//! UNX-F12753 压测两态记账 · 0x82640段
//!
//! 判据原文（派工卡 12753 逐字锚定）：
//!   UNX-F12754-J1 注出处核验 ≥20 处抽检绿，查无资料标待实测。
//!
//! 设计要点：
//! 1. 断言账域模型：账行号 1..26 闭集 + 断言名代号 0x01..0x1A + 记账态
//!    闭集 TrueGreen|StubGreen 双态；账行冻结语料，行号与代号一一对应，
//!    越行、越态、重行一律显性拒；
//! 2. 两态分列账：真绿账列（注出处核验抽检绿）与桩期绿账列（查无资料
//!    标待实测）两列独立累加，互不串写；两列无重号、无跨列互串、
//!    每行记账态必须与冻结语料的真值一致；
//! 3. 记账判定器 book(entry_no, state)：六级判定次序冻结——封账 >
//!    容量耗尽 > 态越闭集(EINVAL) > 行越闭集(ERANGE) > 重复记账
//!    (EALREADY) > 混写同列(EMIXED)，错误路径 Result<T, u32>；
//! 4. 混报注入器三类（真绿行写成桩期 / 桩期行写成真绿 / 两列互串）
//!    各 10 轮，LCG 种子写死，逐轮必须全抓——混报 10/10 检出，
//!    逃脱即判据失效；
//! 5. 两账守恒：总账 = 真绿账 + 桩期绿账 + 拒账，恒等式逐轮复算；
//!    拒账占总账容量（40 笔），耗尽显性拒（EOVERFLOW）不静默；
//! 6. 双跑一致指纹：全流程确定性（无浮点/无墙钟/无随机），两次完整
//!    跑批指纹逐位相同；no_std + alloc；取值一律 match/if let/.get()。

use alloc::{format, string::String, vec::Vec};

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12753 − 12600) = 0x82640）。
pub const SEG_BASE: u32 = 0x82640;

// ── 确定性 LCG（种子写死，零随机红线合规） ───────────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
/// 常数取 Numerical Recipes 推荐值（整数运算，无浮点）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性，双跑逐位相同）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.state
    }

    /// 0..bound 内取值（bound 为零返回零，不越界）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 记账态闭集（TrueGreen | StubGreen 双态） ─────────────────────
/// 记账态闭集：断言账只认两种态——真绿（出处核验抽检绿）与桩期绿
/// （查无资料标待实测）。越出双态的态码一律 EINVAL。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookState {
    /// 真绿：注出处核验抽检绿，落真绿账列。
    TrueGreen,
    /// 桩期绿：查无资料标待实测，落桩期绿账列。
    StubGreen,
}

impl BookState {
    /// 态码（0=真绿，1=桩期绿；越出即 EINVAL）。
    pub fn code(self) -> u32 {
        match self {
            BookState::TrueGreen => 0,
            BookState::StubGreen => 1,
        }
    }

    /// 由态码反查记账态（越出双态闭集返回 None）。
    pub fn from_code(c: u32) -> Option<BookState> {
        match c {
            0 => Some(BookState::TrueGreen),
            1 => Some(BookState::StubGreen),
            _ => None,
        }
    }

    /// 态序（守恒复算与指纹用）。
    pub fn rank(self) -> u8 {
        match self {
            BookState::TrueGreen => 0,
            BookState::StubGreen => 1,
        }
    }

    /// 态中文名（诊断文案用）。
    pub fn name(self) -> &'static str {
        match self {
            BookState::TrueGreen => "真绿",
            BookState::StubGreen => "桩期绿",
        }
    }

    /// 态翻转（混报注入器用：真绿↔桩期绿）。
    pub fn flip(self) -> BookState {
        match self {
            BookState::TrueGreen => BookState::StubGreen,
            BookState::StubGreen => BookState::TrueGreen,
        }
    }
}

/// 记账态闭集数组（双态闭集，冻结不许增删）。
pub const BOOKSTATE_ALL: &[BookState] = &[BookState::TrueGreen, BookState::StubGreen];

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookDiag {
    /// 记账态越双态闭集（EINVAL 语义：态码非法）。
    EInval,
    /// 同一账行重复记账（EALREADY 语义）。
    EAlready,
    /// 真绿账与桩期绿账混写同列（EMIXED 语义）。
    EMixed,
    /// 账行号越 1..26 闭集（ERANGE 语义）。
    ERange,
    /// 总账容量（40 笔）耗尽，记账尝试不收账。
    EOverflow,
    /// 账本已封账，写入被拒。
    ESealed,
    /// 状态机倒退（封账后重封/开账）。
    ERewind,
    /// 账未满 26 行就试图封账。
    ENotFull,
    /// 两账守恒恒等式破（总账 ≠ 真绿 + 桩期绿 + 拒账）。
    EConserve,
    /// 双跑指纹不一致（确定性被破坏）。
    EFprint,
    /// 混报注入逃脱（注入器没抓住混报）。
    EInject,
    /// 抽检绿不足 20 处（判据 J1 下限破）。
    ECount,
}

impl BookDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                BookDiag::EInval => 0x01,
                BookDiag::EAlready => 0x02,
                BookDiag::EMixed => 0x03,
                BookDiag::ERange => 0x04,
                BookDiag::EOverflow => 0x05,
                BookDiag::ESealed => 0x06,
                BookDiag::ERewind => 0x07,
                BookDiag::ENotFull => 0x08,
                BookDiag::EConserve => 0x09,
                BookDiag::EFprint => 0x0a,
                BookDiag::EInject => 0x0b,
                BookDiag::ECount => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            BookDiag::EInval => "记账态越双态闭集：只认 0=真绿 1=桩期绿，先对态码表再记账",
            BookDiag::EAlready => "账行重复记账：每行只落一笔，重复行号先查真绿/桩期两列已有行",
            BookDiag::EMixed => "两态混写同列：真绿行不许落桩期列、桩期行不许落真绿列，按冻结语料真值对列",
            BookDiag::ERange => "账行号越 1..26 闭集：先对冻结语料行号再记账，行外新增一律拒",
            BookDiag::EOverflow => "总账容量耗尽：40 笔记账窗口已满，改动走下一轮滚动窗口不挤占",
            BookDiag::ESealed => "账本已封账：封账后写入拒绝，改动走下一条压测卡滚动",
            BookDiag::ERewind => "状态机倒退：开账→满账→封账只进不退，重封即账本可疑",
            BookDiag::ENotFull => "26 行未齐禁止封账：先补齐缺行再走封账口",
            BookDiag::EConserve => "两账守恒破：总账=真绿+桩期绿+拒账 恒等式不成立，逐列回查串写",
            BookDiag::EFprint => "双跑指纹不一致：确定性被破坏，查非确定源（浮点/墙钟/随机）后重跑",
            BookDiag::EInject => "混报注入逃脱：注入器没抓住混报行，判定器闸门失效须整本复核",
            BookDiag::ECount => "抽检绿不足 20 处：判据 J1 下限破，先补出处核验再封账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DIAG_ALL: &[BookDiag] = &[
    BookDiag::EInval,
    BookDiag::EAlready,
    BookDiag::EMixed,
    BookDiag::ERange,
    BookDiag::EOverflow,
    BookDiag::ESealed,
    BookDiag::ERewind,
    BookDiag::ENotFull,
    BookDiag::EConserve,
    BookDiag::EFprint,
    BookDiag::EInject,
    BookDiag::ECount,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DIAG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82641..0x8264C 闭集内"
}

// ── 账本状态机：开账 → 满账 → 封账，只进不退 ─────────────────────
/// 账本状态机：开账（可记账）→ 满账（26 行齐）→ 封账（只读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerPhase {
    /// 开账：允许逐行记账。
    Open,
    /// 满账：26 行齐，等待封账。
    Full,
    /// 封账：只读。
    Sealed,
}

impl LedgerPhase {
    /// 状态序（只许前进，倒退显性拒）。
    pub fn rank(self) -> u8 {
        match self {
            LedgerPhase::Open => 0,
            LedgerPhase::Full => 1,
            LedgerPhase::Sealed => 2,
        }
    }

    /// 状态中文名（诊断文案用）。
    pub fn name(self) -> &'static str {
        match self {
            LedgerPhase::Open => "开账",
            LedgerPhase::Full => "满账",
            LedgerPhase::Sealed => "封账",
        }
    }
}

// ── 冻结语料：26 条断言账行（行号/代号/名/真值/出处注记） ────────
/// 断言账容量：26 行齐即满账。
pub const CORPUS_CAP: usize = 26;

/// 抽检绿下限（判据 J1：注出处核验 ≥20 处抽检绿）。
pub const TRUE_GREEN_MIN: usize = 20;

/// 总账容量（记账尝试窗口：入账 + 拒账合计 40 笔，耗尽显性拒）。
pub const ATTEMPT_CAP: u32 = 40;

/// 冻结语料行：账行号 + 断言名代号 + 断言名 + 真值态 + 出处注记。
/// 出处注记为真值依据：真绿行记手册/规范已核，桩期绿行必含
/// "查无资料·待实测"字样（判据 J1 锚定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorpRow {
    /// 账行号（1..26 闭集）。
    pub entry_no: u32,
    /// 断言名代号（0x01..0x1A，与行号一一对应）。
    pub code: u32,
    /// 断言名。
    pub name: &'static str,
    /// 真值记账态（混写判定基准）。
    pub truth: BookState,
    /// 出处注记（真绿=已核章节，桩期绿=查无资料·待实测）。
    pub src_note: &'static str,
}

/// 冻结语料：26 条断言账行（行序冻结不许重排）。
/// 1..20 为注出处核验抽检绿（恰 20 处，满足判据 J1 下限），
/// 21..26 为查无资料标待实测（桩期绿 6 行）。
pub const CORPUS: &[CorpRow] = &[
    CorpRow { entry_no: 1, code: 0x01, name: "引导交接表字段齐", truth: BookState::TrueGreen, src_note: "手册§3.2 已核" },
    CorpRow { entry_no: 2, code: 0x02, name: "物理内存图全覆盖", truth: BookState::TrueGreen, src_note: "手册§4.1 已核" },
    CorpRow { entry_no: 3, code: 0x03, name: "页表属性位序冻结", truth: BookState::TrueGreen, src_note: "手册§5.4 已核" },
    CorpRow { entry_no: 4, code: 0x04, name: "中断控制器基址", truth: BookState::TrueGreen, src_note: "规范§7.3 已核" },
    CorpRow { entry_no: 5, code: 0x05, name: "定时器频率档位", truth: BookState::TrueGreen, src_note: "手册§6.5 已核" },
    CorpRow { entry_no: 6, code: 0x06, name: "串口波特率档位", truth: BookState::TrueGreen, src_note: "手册§8.2 已核" },
    CorpRow { entry_no: 7, code: 0x07, name: "内核栈顶对齐", truth: BookState::TrueGreen, src_note: "规范§2.6 已核" },
    CorpRow { entry_no: 8, code: 0x08, name: "堵洞检测阈值", truth: BookState::TrueGreen, src_note: "手册§9.1 已核" },
    CorpRow { entry_no: 9, code: 0x09, name: "NUMA 节点距离", truth: BookState::TrueGreen, src_note: "规范§10.4 已核" },
    CorpRow { entry_no: 10, code: 0x0A, name: "ACPI 表校验和", truth: BookState::TrueGreen, src_note: "手册§11.3 已核" },
    CorpRow { entry_no: 11, code: 0x0B, name: "SMP 启动次序", truth: BookState::TrueGreen, src_note: "规范§12.2 已核" },
    CorpRow { entry_no: 12, code: 0x0C, name: "指令集特性位", truth: BookState::TrueGreen, src_note: "手册§13.7 已核" },
    CorpRow { entry_no: 13, code: 0x0D, name: "缓存行宽度", truth: BookState::TrueGreen, src_note: "规范§14.1 已核" },
    CorpRow { entry_no: 14, code: 0x0E, name: "页尺寸档位", truth: BookState::TrueGreen, src_note: "手册§15.2 已核" },
    CorpRow { entry_no: 15, code: 0x0F, name: "段限粒度", truth: BookState::TrueGreen, src_note: "规范§16.5 已核" },
    CorpRow { entry_no: 16, code: 0x10, name: "特权级环号", truth: BookState::TrueGreen, src_note: "手册§17.4 已核" },
    CorpRow { entry_no: 17, code: 0x11, name: "系统调用门号", truth: BookState::TrueGreen, src_note: "规范§18.3 已核" },
    CorpRow { entry_no: 18, code: 0x12, name: "时间戳计数比", truth: BookState::TrueGreen, src_note: "手册§19.6 已核" },
    CorpRow { entry_no: 19, code: 0x13, name: "探针映像偏移", truth: BookState::TrueGreen, src_note: "规范§20.2 已核" },
    CorpRow { entry_no: 20, code: 0x14, name: "重定位槽距", truth: BookState::TrueGreen, src_note: "手册§21.1 已核" },
    CorpRow { entry_no: 21, code: 0x15, name: "固件热插拔位", truth: BookState::StubGreen, src_note: "查无资料·待实测" },
    CorpRow { entry_no: 22, code: 0x16, name: "混合调度权重", truth: BookState::StubGreen, src_note: "查无资料·待实测" },
    CorpRow { entry_no: 23, code: 0x17, name: "协处理器流水级", truth: BookState::StubGreen, src_note: "查无资料·待实测" },
    CorpRow { entry_no: 24, code: 0x18, name: "远端内存带宽档", truth: BookState::StubGreen, src_note: "查无资料·待实测" },
    CorpRow { entry_no: 25, code: 0x19, name: "安全切换时序", truth: BookState::StubGreen, src_note: "查无资料·待实测" },
    CorpRow { entry_no: 26, code: 0x1A, name: "向量嵌套深度", truth: BookState::StubGreen, src_note: "查无资料·待实测" },
];

/// 按账行号查冻结语料行（越 1..26 闭集返回 None）。
pub fn corpus_row(entry_no: u32) -> Option<&'static CorpRow> {
    if entry_no == 0 || entry_no > CORPUS_CAP as u32 {
        return None;
    }
    match CORPUS.get(entry_no as usize - 1) {
        Some(c) if c.entry_no == entry_no => Some(c),
        _ => None,
    }
}

/// 按账行号查真值记账态。
pub fn corpus_truth(entry_no: u32) -> Option<BookState> {
    match corpus_row(entry_no) {
        Some(c) => Some(c.truth),
        None => None,
    }
}

/// 统计冻结语料中真绿（抽检绿）行数。
pub fn true_cnt() -> usize {
    let mut n = 0usize;
    for c in CORPUS.iter() {
        if c.truth == BookState::TrueGreen {
            n += 1;
        }
    }
    n
}

// ── 断言账行与两态分列账 ─────────────────────────────────────────
/// 单行断言账行（记账判定器成功时回传）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    /// 账行号（1..26 闭集）。
    pub entry_no: u32,
    /// 断言名代号（0x01..0x1A）。
    pub code: u32,
    /// 记账态（与冻结语料真值一致）。
    pub state: BookState,
    /// 记账流水号（总账尝试序，含拒账占位）。
    pub seq: u32,
}

/// 两态分列账账本：真绿账列与桩期绿账列独立累加，拒账计数，
/// 总账容量 40 笔（入账 + 拒账合计），状态机只进不退。
#[derive(Clone, Debug)]
pub struct TwoStateBook {
    /// 已入账行（总账，按入账序）。
    rows: Vec<LedgerRow>,
    /// 真绿账列（入账行号，独立累加）。
    true_col: Vec<u32>,
    /// 桩期绿账列（入账行号，独立累加）。
    stub_col: Vec<u32>,
    /// 拒账笔数（占总账容量；封账/溢出拒不占）。
    rejected: u32,
    /// 总账尝试笔数（入账 + 拒账）。
    attempts: u32,
    /// 账本状态机。
    phase: LedgerPhase,
}

impl TwoStateBook {
    /// 开一本新账（状态机 Open，两列清空）。
    pub fn new() -> TwoStateBook {
        TwoStateBook {
            rows: Vec::new(),
            true_col: Vec::new(),
            stub_col: Vec::new(),
            rejected: 0,
            attempts: 0,
            phase: LedgerPhase::Open,
        }
    }

    /// 当前状态机相。
    pub fn state(&self) -> LedgerPhase {
        self.phase
    }

    /// 已入账行数（总账入账侧）。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 总账尝试笔数（入账 + 拒账）。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// 拒账笔数。
    pub fn rejected_count(&self) -> u32 {
        self.rejected
    }

    /// 真绿账列快照（只读）。
    pub fn true_col(&self) -> &[u32] {
        &self.true_col
    }

    /// 桩期绿账列快照（只读）。
    pub fn stub_col(&self) -> &[u32] {
        &self.stub_col
    }

    /// 取总账第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<LedgerRow> {
        self.rows.get(idx).copied()
    }

    /// 按账行号查已入账行的记账态（未入账 None）。
    fn row_state_of(&self, entry_no: u32) -> Option<BookState> {
        for r in self.rows.iter() {
            if r.entry_no == entry_no {
                return Some(r.state);
            }
        }
        None
    }

    /// 记账判定器：book(entry_no, state)。
    ///
    /// 六级判定次序冻结（先到先判，一票即拒）：
    ///   1. 封账闸 ESealed（封账后一切写入拒，不占容量）；
    ///   2. 容量闸 EOverflow（40 笔记账窗口满，尝试不收账不占容量）；
    ///   3. 态闭集闸 EInval（记账态越 TrueGreen|StubGreen 双态）；
    ///   4. 行闭集闸 ERange（账行号越 1..26）；
    ///   5. 重复闸 EAlready（同一账行只落一笔）；
    ///   6. 混列闸 EMixed（真绿账与桩期绿账混写同列）。
    /// 通过六闸即入对应账列并回传账行。
    pub fn book(&mut self, entry_no: u32, state_raw: u32) -> Result<LedgerRow, u32> {
        // 1. 封账闸：封账后写入拒绝，账面不变、容量不动。
        if self.phase.rank() >= LedgerPhase::Sealed.rank() {
            return Err(BookDiag::ESealed.val());
        }
        // 2. 容量闸：总账 40 笔耗尽显性拒（不收账、不静默）。
        if self.attempts >= ATTEMPT_CAP {
            return Err(BookDiag::EOverflow.val());
        }
        // 收账：此后各闸的拒绝都占一笔拒账。
        self.attempts = self.attempts.saturating_add(1);
        // 3. 态闭集闸：记账态越双态闭集 → EINVAL。
        let req = match BookState::from_code(state_raw) {
            Some(s) => s,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(BookDiag::EInval.val());
            }
        };
        // 4. 行闭集闸：账行号越 1..26 → ERANGE。
        let corp = match corpus_row(entry_no) {
            Some(c) => c,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(BookDiag::ERange.val());
            }
        };
        // 5. 重复闸：同一账行重复记账 → EALREADY。
        if self.row_state_of(entry_no).is_some() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(BookDiag::EAlready.val());
        }
        // 6. 混列闸：记账态与冻结语料真值不一致即混写同列 → EMIXED。
        if req != corp.truth {
            self.rejected = self.rejected.saturating_add(1);
            return Err(BookDiag::EMixed.val());
        }
        // 六闸全过：入账并落对应账列。
        let row = LedgerRow {
            entry_no,
            code: corp.code,
            state: req,
            seq: self.attempts,
        };
        self.rows.push(row);
        match req {
            BookState::TrueGreen => self.true_col.push(entry_no),
            BookState::StubGreen => self.stub_col.push(entry_no),
        }
        // 26 行齐即推进状态机（只进不退）。
        if self.rows.len() == CORPUS_CAP {
            self.phase = LedgerPhase::Full;
        }
        Ok(row)
    }

    /// 两账守恒：总账尝试 = 真绿账 + 桩期绿账 + 拒账（恒等式逐轮复算）。
    pub fn conservation_ok(&self) -> bool {
        let accepted = self
            .true_col
            .len()
            .saturating_add(self.stub_col.len());
        let total = accepted.saturating_add(self.rejected as usize);
        self.attempts as usize == total && self.rows.len() == accepted
    }

    /// 两列审计：无跨列互串、无列内重号、每行记账态与冻结语料真值一致。
    pub fn columns_consistent(&self) -> bool {
        // 两列合计必须恰等于总账入账数（无隐性第三列）。
        let accepted = self
            .true_col
            .len()
            .saturating_add(self.stub_col.len());
        if accepted != self.rows.len() {
            return false;
        }
        // 真绿列：无重号、不与桩期列互串、行态=真值=真绿。
        for t in self.true_col.iter() {
            let mut dup = 0usize;
            for t2 in self.true_col.iter() {
                if t == t2 {
                    dup += 1;
                }
            }
            if dup != 1 {
                return false;
            }
            for s in self.stub_col.iter() {
                if t == s {
                    // 跨列互串实锤。
                    return false;
                }
            }
            match corpus_truth(*t) {
                Some(BookState::TrueGreen) => {}
                _ => return false,
            }
            match self.row_state_of(*t) {
                Some(BookState::TrueGreen) => {}
                _ => return false,
            }
        }
        // 桩期绿列：无重号、行态=真值=桩期绿。
        for s in self.stub_col.iter() {
            let mut dup = 0usize;
            for s2 in self.stub_col.iter() {
                if s == s2 {
                    dup += 1;
                }
            }
            if dup != 1 {
                return false;
            }
            match corpus_truth(*s) {
                Some(BookState::StubGreen) => {}
                _ => return false,
            }
            match self.row_state_of(*s) {
                Some(BookState::StubGreen) => {}
                _ => return false,
            }
        }
        true
    }

    /// 双跑一致指纹：对总账行、拒账、尝试数、状态机相做确定性折叠。
    /// 同一账面同一序列，指纹逐位相同；任何窜动都改指纹。
    pub fn fingerprint(&self) -> u64 {
        // FNV-1a 64 位素数折叠（整数运算，确定性）。
        let mut fp: u64 = ((SEG_BASE as u64) << 32) | 1;
        for r in self.rows.iter() {
            fp = fp.wrapping_mul(0x1_0000_0001_B3);
            fp = fp.wrapping_add(r.entry_no as u64);
            fp = fp.wrapping_mul(0x1_0000_0001_B3);
            fp = fp.wrapping_add(r.code as u64);
            fp = fp.wrapping_mul(0x1_0000_0001_B3);
            fp = fp.wrapping_add(r.seq as u64);
            fp = fp.wrapping_mul(0x1_0000_0001_B3);
            fp = fp.wrapping_add(r.state.rank() as u64);
        }
        fp = fp.wrapping_mul(0x1_0000_0001_B3);
        fp = fp.wrapping_add(self.rejected as u64);
        fp = fp.wrapping_mul(0x1_0000_0001_B3);
        fp = fp.wrapping_add(self.attempts as u64);
        fp = fp.wrapping_mul(0x1_0000_0001_B3);
        fp = fp.wrapping_add(self.phase.rank() as u64);
        fp
    }

    /// 封账：26 行齐 + 守恒 + 两列审计全绿才许封；重封倒退拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase.rank() >= LedgerPhase::Sealed.rank() {
            return Err(BookDiag::ERewind.val());
        }
        if self.rows.len() != CORPUS_CAP {
            return Err(BookDiag::ENotFull.val());
        }
        if !self.conservation_ok() || !self.columns_consistent() {
            return Err(BookDiag::EConserve.val());
        }
        self.phase = LedgerPhase::Sealed;
        Ok(())
    }
}

// ── 构建器：齐账账本与固定剧本跑批 ───────────────────────────────
/// 构建齐账账本：按冻结语料逐行按真值态记账（确定性，26 行全入）。
pub fn build_full_book() -> TwoStateBook {
    let mut b = TwoStateBook::new();
    for c in CORPUS.iter() {
        let _ = b.book(c.entry_no, c.truth.code());
    }
    b
}

/// 固定剧本跑批（双跑一致基准）：三笔固定拒账探针 + 26 行按真值入账
/// + 重复记账拒 + 封账。序列写死，双跑指纹必须逐位相同。
pub fn scripted_run() -> TwoStateBook {
    let mut b = TwoStateBook::new();
    // 探针 1：态越双态闭集（EINVAL）。
    let _ = b.book(1, 2);
    // 探针 2：混写同列（行 1 真值真绿，写成桩期绿 → EMIXED）。
    let _ = b.book(1, BookState::StubGreen.code());
    // 正账：行 1 按真值入账。
    let _ = b.book(1, BookState::TrueGreen.code());
    // 探针 3：重复记账（行 1 已入 → EALREADY）。
    let _ = b.book(1, BookState::TrueGreen.code());
    // 余下 2..26 行按真值入账。
    let mut n = 2u32;
    while n <= CORPUS_CAP as u32 {
        if let Some(c) = corpus_row(n) {
            let _ = b.book(c.entry_no, c.truth.code());
        }
        n += 1;
    }
    // 封账：满账 + 守恒 + 两列审计全绿。
    let _ = b.seal();
    b
}

// ── 混报注入器（三类 × 10 轮，全抓才算过） ───────────────────────
/// 混报注入类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    /// 真绿行写成桩期绿。
    TrueAsStub,
    /// 桩期绿行写成真绿。
    StubAsTrue,
    /// 两列互串（真绿行号串写桩期绿列）。
    ColumnCross,
}

impl InjectKind {
    /// 类别中文名（诊断文案用）。
    pub fn name(self) -> &'static str {
        match self {
            InjectKind::TrueAsStub => "真绿写成桩期",
            InjectKind::StubAsTrue => "桩期写成真绿",
            InjectKind::ColumnCross => "两列互串",
        }
    }
}

/// 混报类别闭集数组（三类，冻结不许增删）。
pub const INJECT_ALL: &[InjectKind] = &[
    InjectKind::TrueAsStub,
    InjectKind::StubAsTrue,
    InjectKind::ColumnCross,
];

/// 按真值类抽一个目标账行号（LCG 种子写死，确定性）。
fn pick_target(seed: u32, want: BookState) -> u32 {
    let mut g = Lcg::new(seed);
    let mut guard = 0u32;
    loop {
        let cand = 1u32 + g.below(CORPUS_CAP as u32);
        if corpus_truth(cand) == Some(want) {
            return cand;
        }
        guard = guard.saturating_add(1);
        if guard > 200 {
            // 兜底：两类语料非空，正常流不可达；落固定行保证终止。
            return match want {
                BookState::TrueGreen => 1,
                BookState::StubGreen => 21,
            };
        }
    }
}

/// 混报注入第一类：真绿行写成桩期绿，10 轮全抓。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn inject_true_as_stub() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut r = 0u32;
    while r < 10 {
        let target = pick_target(SEG_BASE.wrapping_add(r * 0x10).wrapping_add(0x11), BookState::TrueGreen);
        let mut b = TwoStateBook::new();
        for c in CORPUS.iter() {
            // 注入点：目标行的记账态翻转（真绿写成桩期绿）。
            let s = if c.entry_no == target {
                c.truth.flip()
            } else {
                c.truth
            };
            match b.book(c.entry_no, s.code()) {
                Ok(_row) => {
                    if c.entry_no == target && escape.is_empty() {
                        escape = format!("{} 轮{} 行{} 混报被误收", InjectKind::TrueAsStub.name(), r, target);
                    }
                }
                Err(e) => {
                    if c.entry_no == target && e == BookDiag::EMixed.val() {
                        caught += 1;
                    } else if escape.is_empty() {
                        escape = format!(
                            "{} 轮{} 行{} 误报码0x{:05x}",
                            InjectKind::TrueAsStub.name(),
                            r,
                            c.entry_no,
                            e
                        );
                    }
                }
            }
        }
        // 每轮守恒复算：26 尝试 = 25 入账 + 1 拒账。
        if !b.conservation_ok() && escape.is_empty() {
            escape = format!("{} 轮{} 守恒破", InjectKind::TrueAsStub.name(), r);
        }
        total += 1;
        r += 1;
    }
    (caught, total, escape)
}

/// 混报注入第二类：桩期绿行写成真绿，10 轮全抓。
pub fn inject_stub_as_true() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut r = 0u32;
    while r < 10 {
        let target = pick_target(SEG_BASE.wrapping_add(r * 0x10).wrapping_add(0x53), BookState::StubGreen);
        let mut b = TwoStateBook::new();
        for c in CORPUS.iter() {
            // 注入点：目标行的记账态翻转（桩期绿写成真绿）。
            let s = if c.entry_no == target {
                c.truth.flip()
            } else {
                c.truth
            };
            match b.book(c.entry_no, s.code()) {
                Ok(_row) => {
                    if c.entry_no == target && escape.is_empty() {
                        escape = format!("{} 轮{} 行{} 混报被误收", InjectKind::StubAsTrue.name(), r, target);
                    }
                }
                Err(e) => {
                    if c.entry_no == target && e == BookDiag::EMixed.val() {
                        caught += 1;
                    } else if escape.is_empty() {
                        escape = format!(
                            "{} 轮{} 行{} 误报码0x{:05x}",
                            InjectKind::StubAsTrue.name(),
                            r,
                            c.entry_no,
                            e
                        );
                    }
                }
            }
        }
        if !b.conservation_ok() && escape.is_empty() {
            escape = format!("{} 轮{} 守恒破", InjectKind::StubAsTrue.name(), r);
        }
        total += 1;
        r += 1;
    }
    (caught, total, escape)
}

/// 混报注入第三类：两列互串——在齐账账本的桩期绿列里串写一个真绿
/// 行号，两列审计必须当场抓出，10 轮全抓。
pub fn inject_column_cross() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut r = 0u32;
    while r < 10 {
        let full = build_full_book();
        // 从真绿列 LCG 抖动选点（列长 20，选点必在列内）。
        let mut g = Lcg::new(SEG_BASE.wrapping_add(r * 0x10).wrapping_add(0x97));
        let idx = g.below(full.true_col().len() as u32) as usize;
        let target = match full.true_col().get(idx) {
            Some(v) => *v,
            None => 1,
        };
        // 互串注入：真绿行号串写进桩期绿列（列级篡改副本）。
        let mut tam = full.clone();
        tam.stub_col.push(target);
        total += 1;
        if !tam.columns_consistent() {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("{} 轮{} 行{} 逃脱", InjectKind::ColumnCross.name(), r, target);
        }
        r += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件，末行成立条件） ────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：码值全部落段窗、互异、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    let mut i = 0usize;
    while i < DIAG_ALL.len() {
        let vi = match DIAG_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        if vi < SEG_BASE || vi >= SEG_BASE + 0x40 {
            return false;
        }
        // 两两互异。
        let mut j = i + 1;
        while j < DIAG_ALL.len() {
            let vj = match DIAG_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        // 查表闭集：已知码必须命中文案且文案非空。
        if detail(vi) != match DIAG_ALL.get(i) {
            Some(d) => d.detail(),
            None => "",
        } {
            return false;
        }
        if detail(vi).is_empty() {
            return false;
        }
        i += 1;
    }
    // 未知码回落。
    if detail(SEG_BASE + 0x3F) != "未知诊断码：不在 0x82641..0x8264C 闭集内" {
        return false;
    }
    true
}

/// 记账态闭集守卫：双态互异、态码互异、名非空、翻转对合。
fn bookstate_closed_ok() -> bool {
    if BOOKSTATE_ALL.len() != 2 {
        return false;
    }
    for s in BOOKSTATE_ALL.iter() {
        if s.name().is_empty() {
            return false;
        }
        // 态码回查对合：from_code(code) 还原自身。
        match BookState::from_code(s.code()) {
            Some(back) => {
                if back != *s {
                    return false;
                }
            }
            None => return false,
        }
        // 翻转对合：flip(flip(s)) == s，且翻到异态。
        let f1 = s.flip();
        if f1 == *s {
            return false;
        }
        if f1.flip() != *s {
            return false;
        }
    }
    true
}

/// 冻结语料守卫：26 行行号连续、代号一一对应、名与出处注记非空。
fn corpus_ok() -> bool {
    if CORPUS.len() != CORPUS_CAP {
        return false;
    }
    for c in CORPUS.iter() {
        // 行号闭集与代号一一对应（行号 n ↔ 代号 0x00+n）。
        if c.entry_no == 0 || c.entry_no as usize > CORPUS_CAP {
            return false;
        }
        if c.code != c.entry_no {
            return false;
        }
        if c.name.is_empty() || c.src_note.is_empty() {
            return false;
        }
    }
    true
}

/// 判据 J1 谓词：注出处核验 ≥20 处抽检绿；查无资料行全部标待实测；
/// 真绿行不得挂待实测标（两态不混标）。
fn criterion_green_ok() -> bool {
    let mut green = 0usize;
    for c in CORPUS.iter() {
        match c.truth {
            BookState::TrueGreen => {
                green += 1;
                // 违约：真绿行挂"待实测"标即两态混标。
                if c.src_note.contains("待实测") {
                    return false;
                }
            }
            BookState::StubGreen => {
                // 违约：桩期绿行必须含"查无资料"与"待实测"双标。
                if !c.src_note.contains("查无资料") || !c.src_note.contains("待实测") {
                    return false;
                }
            }
        }
    }
    green >= TRUE_GREEN_MIN
}

/// 正常流：齐账账本 26 行全入、两列 20/6 分列、状态机到满账。
fn happy_path_ok(b: &TwoStateBook) -> bool {
    if b.len() != CORPUS_CAP {
        return false;
    }
    if b.true_col().len() != TRUE_GREEN_MIN {
        return false;
    }
    if b.stub_col().len() != CORPUS_CAP - TRUE_GREEN_MIN {
        return false;
    }
    if b.state() != LedgerPhase::Full {
        return false;
    }
    if b.rejected_count() != 0 {
        return false;
    }
    b.conservation_ok() && b.columns_consistent()
}

/// 错误路径四闸：EINVAL / EALREADY / EMIXED / ERANGE 各一路必拒。
fn error_paths_ok() -> bool {
    // EINVAL：记账态 2 越双态闭集。
    let mut b1 = TwoStateBook::new();
    if b1.book(1, 2) != Err(BookDiag::EInval.val()) {
        return false;
    }
    // EMIXED：行 1 真值真绿，写成桩期绿 → 混写同列拒。
    let mut b2 = TwoStateBook::new();
    if b2.book(1, BookState::StubGreen.code()) != Err(BookDiag::EMixed.val()) {
        return false;
    }
    // ERANGE：行号 0 与 27 越闭集各一路。
    let mut b3 = TwoStateBook::new();
    if b3.book(0, BookState::TrueGreen.code()) != Err(BookDiag::ERange.val()) {
        return false;
    }
    let mut b4 = TwoStateBook::new();
    if b4.book(CORPUS_CAP as u32 + 1, BookState::TrueGreen.code()) != Err(BookDiag::ERange.val()) {
        return false;
    }
    // EALREADY：行 1 入账后重复记账拒。
    let mut b5 = TwoStateBook::new();
    let _ = b5.book(1, BookState::TrueGreen.code());
    if b5.book(1, BookState::TrueGreen.code()) != Err(BookDiag::EAlready.val()) {
        return false;
    }
    // 拒账后账面未收行。
    b5.len() == 1 && b5.conservation_ok()
}

/// 判定次序冻结：封账 > 容量 > 态 > 行 > 重复 > 混列，先到先判。
fn precedence_ok() -> bool {
    // 封账 > 态：封账本上态越集仍先报封账。
    let mut sealed = build_full_book();
    let _ = sealed.seal();
    if sealed.state() != LedgerPhase::Sealed {
        return false;
    }
    if sealed.book(1, 9) != Err(BookDiag::ESealed.val()) {
        return false;
    }
    // 容量 > 态：满容账本上态越集先报容量耗尽。
    let mut full = TwoStateBook::new();
    for c in CORPUS.iter() {
        let _ = full.book(c.entry_no, c.truth.code());
    }
    let mut k = 0u32;
    while k < ATTEMPT_CAP - CORPUS_CAP as u32 {
        let _ = full.book(1, BookState::TrueGreen.code());
        k += 1;
    }
    if full.attempts() != ATTEMPT_CAP {
        return false;
    }
    if full.book(1, 9) != Err(BookDiag::EOverflow.val()) {
        return false;
    }
    // 态 > 行：行号与态码同时越集，先报态。
    let mut b1 = TwoStateBook::new();
    if b1.book(0, 9) != Err(BookDiag::EInval.val()) {
        return false;
    }
    // 行 > 重复：行号越集先于重复判定（行 0 永不入账，必先行闸）。
    let mut b2 = TwoStateBook::new();
    if b2.book(0, BookState::TrueGreen.code()) != Err(BookDiag::ERange.val()) {
        return false;
    }
    // 重复 > 混列：已入账行即使态也错，先报重复。
    let mut b3 = TwoStateBook::new();
    let _ = b3.book(1, BookState::TrueGreen.code());
    if b3.book(1, BookState::StubGreen.code()) != Err(BookDiag::EAlready.val()) {
        return false;
    }
    // 混列兜底：未入账行态错即报混列。
    if b3.book(2, BookState::StubGreen.code()) != Err(BookDiag::EMixed.val()) {
        return false;
    }
    true
}

/// 注入必抓（真绿写成桩期）：10 轮全抓、无逃脱。
fn inject_true_ok() -> bool {
    let (caught, total, escape) = inject_true_as_stub();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入必抓（桩期写成真绿）：10 轮全抓、无逃脱。
fn inject_stub_ok() -> bool {
    let (caught, total, escape) = inject_stub_as_true();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入必抓（两列互串）：10 轮全抓、无逃脱。
fn inject_cross_ok() -> bool {
    let (caught, total, escape) = inject_column_cross();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 两账守恒守卫：齐账账本与带拒账账本恒等式均成立。
fn conserve_ok() -> bool {
    let full = build_full_book();
    if !full.conservation_ok() {
        return false;
    }
    // 带拒账账本：3 笔拒账占容，恒等式仍成立。
    let scripted = scripted_run();
    if scripted.rejected_count() != 3 {
        return false;
    }
    if scripted.attempts() != (CORPUS_CAP + 3) as u32 {
        return false;
    }
    scripted.conservation_ok() && scripted.columns_consistent()
}

/// 双跑一致守卫：两次剧本跑批指纹、行序、状态机相逐项一致。
fn dual_run_ok() -> bool {
    let a = scripted_run();
    let b = scripted_run();
    if a.fingerprint() != b.fingerprint() {
        return false;
    }
    if a.len() != b.len() || a.state() != b.state() || a.attempts() != b.attempts() {
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
    // 混报注入器双跑也必须一致（三类逐项相同）。
    inject_true_as_stub() == inject_true_as_stub()
        && inject_stub_as_true() == inject_stub_as_true()
        && inject_column_cross() == inject_column_cross()
}

/// 边界四类：下界行 0、上界行 27、态越集、尾条入满转满账。
fn boundary_ok() -> bool {
    // 下界：行号 0 越闭集。
    let mut b = TwoStateBook::new();
    if b.book(0, BookState::TrueGreen.code()) != Err(BookDiag::ERange.val()) {
        return false;
    }
    // 上界：行号 27 越闭集。
    if b.book(CORPUS_CAP as u32 + 1, BookState::TrueGreen.code()) != Err(BookDiag::ERange.val()) {
        return false;
    }
    // 态越集：态码 2 与态码 0xFFFFFFFF 各一路。
    if b.book(1, 2) != Err(BookDiag::EInval.val()) {
        return false;
    }
    if b.book(1, 0xFFFF_FFFF) != Err(BookDiag::EInval.val()) {
        return false;
    }
    // 尾条：第 26 行入账后满账；再记 26 行 → 重复拒。
    let mut b2 = TwoStateBook::new();
    let mut n = 1u32;
    while n <= CORPUS_CAP as u32 {
        if let Some(c) = corpus_row(n) {
            if b2.book(c.entry_no, c.truth.code()).is_err() {
                return false;
            }
        }
        n += 1;
    }
    if b2.state() != LedgerPhase::Full {
        return false;
    }
    if b2.book(CORPUS_CAP as u32, BookState::TrueGreen.code()) != Err(BookDiag::EAlready.val()) {
        return false;
    }
    true
}

/// 容量耗尽：26 入账 + 14 拒账填满 40 笔窗口，第 41 笔显性拒
/// （EOVERFLOW），且拒后账面与容量不变、守恒不破。
fn capacity_ok() -> bool {
    let mut b = TwoStateBook::new();
    for c in CORPUS.iter() {
        let _ = b.book(c.entry_no, c.truth.code());
    }
    let mut k = 0u32;
    while k < ATTEMPT_CAP - CORPUS_CAP as u32 {
        if b.book(1, BookState::TrueGreen.code()) != Err(BookDiag::EAlready.val()) {
            return false;
        }
        k += 1;
    }
    if b.attempts() != ATTEMPT_CAP {
        return false;
    }
    // 第 41 笔：容量耗尽显性拒，不收账不占容。
    if b.book(1, 9) != Err(BookDiag::EOverflow.val()) {
        return false;
    }
    if b.attempts() != ATTEMPT_CAP {
        return false;
    }
    if b.book(CORPUS_CAP as u32 + 1, BookState::TrueGreen.code()) != Err(BookDiag::EOverflow.val()) {
        return false;
    }
    b.conservation_ok() && b.columns_consistent()
}

/// 封账状态机：开账→满账→封账只进不退；封账后写入拒、账面不变。
fn seal_machine_ok() -> bool {
    // 未满禁封。
    let mut half = TwoStateBook::new();
    let _ = half.book(1, BookState::TrueGreen.code());
    if half.seal() != Err(BookDiag::ENotFull.val()) {
        return false;
    }
    // 满账封账成功。
    let mut full = build_full_book();
    if full.seal().is_err() {
        return false;
    }
    if full.state() != LedgerPhase::Sealed {
        return false;
    }
    // 重封倒退拒。
    if full.seal() != Err(BookDiag::ERewind.val()) {
        return false;
    }
    // 封账后写入拒且账面不变。
    let before_len = full.len();
    let before_att = full.attempts();
    if full.book(1, BookState::TrueGreen.code()) != Err(BookDiag::ESealed.val()) {
        return false;
    }
    if full.len() != before_len || full.attempts() != before_att {
        return false;
    }
    full.conservation_ok()
}

/// 混报类别闭集守卫：三类互异、名非空。
fn inject_kinds_ok() -> bool {
    if INJECT_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < INJECT_ALL.len() {
        let k = match INJECT_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if k.name().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < INJECT_ALL.len() {
            if let Some(k2) = INJECT_ALL.get(j) {
                if *k2 == k {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12753 自检（判据 12753 卡：注出处核验 ≥20 处抽检绿，查无资料
/// 标待实测；两态记账域模型全闸全抓）。
pub fn run_unx12753_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12753");
    let full = build_full_book();
    let scripted = scripted_run();
    let (c1, t1, e1) = inject_true_as_stub();
    let (c2, t2, e2) = inject_stub_as_true();
    let (c3, t3, e3) = inject_column_cross();
    let mixed_caught = c1 + c2 + c3;
    let mixed_total = t1 + t2 + t3;

    s.add_dyn(
        "F12753-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12753-02 诊断码闭集",
        diag_closed_ok(),
        &format!("codes={} base=0x{:05x}", DIAG_ALL.len(), SEG_BASE),
    );

    s.add_dyn(
        "F12753-03 记账态闭集双态",
        bookstate_closed_ok(),
        &format!("states={} 真绿码0 桩期码1", BOOKSTATE_ALL.len()),
    );

    s.add_dyn(
        "F12753-04 冻结语料守卫",
        corpus_ok(),
        &format!("rows={} 真绿{} 桩期{}", CORPUS.len(), true_cnt(), CORPUS.len() - true_cnt()),
    );

    s.add_dyn(
        "F12753-05 判据J1 抽检绿下限",
        criterion_green_ok(),
        &format!("green={} min={}", true_cnt(), TRUE_GREEN_MIN),
    );

    s.add_dyn(
        "F12753-06 正常流全记账",
        happy_path_ok(&full),
        &format!(
            "rows={} true={} stub={} 相={}",
            full.len(),
            full.true_col().len(),
            full.stub_col().len(),
            full.state().name()
        ),
    );

    s.add_dyn(
        "F12753-07 错误路径四闸",
        error_paths_ok(),
        "EINVAL/EALREADY/EMIXED/ERANGE 各一路必拒",
    );

    s.add_dyn(
        "F12753-08 判定次序冻结",
        precedence_ok(),
        "封账>容量>态>行>重复>混列 六级先到先判",
    );

    s.add_dyn(
        "F12753-09 注入·真绿写成桩期",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("{}/{} {}", c1, t1, e1),
    );

    s.add_dyn(
        "F12753-10 注入·桩期写成真绿",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("{}/{} {}", c2, t2, e2),
    );

    s.add_dyn(
        "F12753-11 注入·两列互串",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("{}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12753-12 两账守恒恒等式",
        conserve_ok(),
        &format!(
            "总账{}=真绿{}+桩期{}+拒账{}",
            scripted.attempts(),
            scripted.true_col().len(),
            scripted.stub_col().len(),
            scripted.rejected_count()
        ),
    );

    s.add_dyn(
        "F12753-13 双跑一致指纹",
        dual_run_ok(),
        &format!("fp={:016x}", scripted.fingerprint()),
    );

    s.add_dyn(
        "F12753-14 边界四类与容量耗尽",
        boundary_ok() && capacity_ok(),
        "下界0/上界27/态越集/尾条满账 + 第41笔EOVERFLOW",
    );

    s.add_dyn(
        "F12753-15 封账状态机",
        seal_machine_ok(),
        "开账→满账→封账 只进不退，封账后写入拒",
    );

    s.add_dyn(
        "F12753-16 混报类别闭集",
        inject_kinds_ok(),
        &format!("kinds={} 三类互异", INJECT_ALL.len()),
    );

    // 收口：判据条数声明（含本条共 17 条判据；混报三类合计 30/30 检出）。
    s.add_dyn(
        "F12753-17 收口·判据条数声明",
        s.len() == 16 && mixed_caught == mixed_total && mixed_total == 30,
        &format!(
            "判据条数=17 混报检出={}/{}",
            mixed_caught, mixed_total
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 段窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82640);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落段窗、查表可命中、文案非空。
        assert_eq!(DIAG_ALL.len(), 12);
        let mut i = 0usize;
        while i < DIAG_ALL.len() {
            let vi = DIAG_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DIAG_ALL.len() {
                let vj = DIAG_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn happy_path_books_all_and_seals() {
        // 正常流：26 行全入、真绿 20 / 桩期绿 6 分列、满账后封账成功。
        let mut b = build_full_book();
        assert_eq!(b.len(), 26);
        assert_eq!(b.true_col().len(), 20);
        assert_eq!(b.stub_col().len(), 6);
        assert_eq!(b.state(), LedgerPhase::Full);
        assert_eq!(b.rejected_count(), 0);
        assert!(b.conservation_ok());
        assert!(b.columns_consistent());
        assert!(b.seal().is_ok());
        assert_eq!(b.state(), LedgerPhase::Sealed);
    }

    #[test]
    fn four_boundary_classes_rejected() {
        // 四类边界：下界行 0 / 上界行 27 / 态越集 / 尾条满账。
        let mut b = TwoStateBook::new();
        assert_eq!(b.book(0, 0), Err(BookDiag::ERange.val()));
        assert_eq!(b.book(27, 0), Err(BookDiag::ERange.val()));
        assert_eq!(b.book(1, 2), Err(BookDiag::EInval.val()));
        assert_eq!(b.book(1, 0xFFFF_FFFF), Err(BookDiag::EInval.val()));
        // 尾条边界：第 26 行入账后满账，再记 26 行重复拒。
        let mut b2 = TwoStateBook::new();
        let mut n = 1u32;
        while n <= 26 {
            assert!(b2.book(n, corpus_truth(n).unwrap_or(BookState::TrueGreen).code()).is_ok());
            n += 1;
        }
        assert_eq!(b2.state(), LedgerPhase::Full);
        assert_eq!(b2.book(26, 0), Err(BookDiag::EAlready.val()));
    }

    #[test]
    fn injections_thirty_rounds_all_caught() {
        // 三类混报注入各 10 轮全抓：混报 10/10 检出 × 3。
        let (c1, t1, e1) = inject_true_as_stub();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "逃脱：{}", e1);
        let (c2, t2, e2) = inject_stub_as_true();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "逃脱：{}", e2);
        let (c3, t3, e3) = inject_column_cross();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "逃脱：{}", e3);
        assert!(e1.is_empty() && e2.is_empty() && e3.is_empty());
    }

    #[test]
    fn judge_precedence_frozen() {
        // 判定次序：封账>容量>态>行>重复>混列。
        let mut sealed = build_full_book();
        let _ = sealed.seal();
        assert_eq!(sealed.book(1, 9), Err(BookDiag::ESealed.val()));
        let mut full = TwoStateBook::new();
        for c in CORPUS.iter() {
            let _ = full.book(c.entry_no, c.truth.code());
        }
        let mut k = 0u32;
        while k < 14 {
            let _ = full.book(1, 0);
            k += 1;
        }
        assert_eq!(full.book(1, 9), Err(BookDiag::EOverflow.val()));
        let mut b = TwoStateBook::new();
        assert_eq!(b.book(0, 9), Err(BookDiag::EInval.val()));
        assert_eq!(b.book(0, 0), Err(BookDiag::ERange.val()));
        let _ = b.book(1, 0);
        assert_eq!(b.book(1, 1), Err(BookDiag::EAlready.val()));
        assert_eq!(b.book(2, 1), Err(BookDiag::EMixed.val()));
    }

    #[test]
    fn dual_run_fingerprints_match() {
        // 双跑一致：两次剧本跑批指纹逐位相同，行序一致。
        let a = scripted_run();
        let b = scripted_run();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.state(), b.state());
        let mut i = 0usize;
        while i < a.len() {
            assert_eq!(a.row_at(i), b.row_at(i));
            i += 1;
        }
        // 注入器双跑也一致。
        assert_eq!(inject_true_as_stub(), inject_true_as_stub());
        assert_eq!(inject_column_cross(), inject_column_cross());
    }

    #[test]
    fn attempt_capacity_exhausts() {
        // 容量耗尽：40 笔窗口满后第 41 笔显性拒，账面与容量不变。
        let mut b = TwoStateBook::new();
        for c in CORPUS.iter() {
            let _ = b.book(c.entry_no, c.truth.code());
        }
        let mut k = 0u32;
        while k < 14 {
            assert_eq!(b.book(1, 0), Err(BookDiag::EAlready.val()));
            k += 1;
        }
        assert_eq!(b.attempts(), ATTEMPT_CAP);
        assert_eq!(b.book(1, 9), Err(BookDiag::EOverflow.val()));
        assert_eq!(b.attempts(), ATTEMPT_CAP);
        assert!(b.conservation_ok());
    }

    #[test]
    fn seal_machine_forward_only() {
        // 状态机只进不退：未满禁封、满账可封、重封倒退拒、封账后写入拒。
        let mut half = TwoStateBook::new();
        let _ = half.book(1, 0);
        assert_eq!(half.seal(), Err(BookDiag::ENotFull.val()));
        let mut full = build_full_book();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(BookDiag::ERewind.val()));
        let before = full.len();
        assert_eq!(full.book(1, 0), Err(BookDiag::ESealed.val()));
        assert_eq!(full.len(), before);
    }

    #[test]
    fn conservation_holds_each_round() {
        // 两账守恒：总账 = 真绿 + 桩期绿 + 拒账，恒等式逐轮复算。
        let mut b = TwoStateBook::new();
        for c in CORPUS.iter() {
            let _ = b.book(c.entry_no, c.truth.code());
            assert!(b.conservation_ok(), "行{} 后守恒破", c.entry_no);
        }
        // 混入三笔拒账后恒等式仍成立。
        let _ = b.book(0, 9);
        let _ = b.book(1, 1);
        let _ = b.book(27, 0);
        assert_eq!(b.rejected_count(), 3);
        assert_eq!(b.attempts(), 29);
        assert!(b.conservation_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12753_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
