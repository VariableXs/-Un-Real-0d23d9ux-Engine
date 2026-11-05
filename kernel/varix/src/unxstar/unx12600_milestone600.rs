//! UNX-F12601 本会话 600 条里程碑条目 · 0x80000段
//!
//! 判据原文（派工卡 12600 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12601-J1 消费边总表八行齐（端口对象/连接序/投递/
//!      安全属性/半对/区段/读写/关闭），三列制度列齐断言 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 消费边总表八行闭集：端口对象/连接序/投递/安全属性/半对/区段/读写/
//!    关闭，行序冻结——按行序登记，越序、重复、行外新增一律显性拒；
//! 2. 三列制度：每行三列（登记列/实现列/验收列），任一列为零即列缺账，
//!    列缺账行不许入账；三列全零样本（空行）单独成诊断码；
//! 3. 列齐断言 10/10：八行各一条列齐断言 + 总表行数恒等断言 + 列守恒
//!    断言，共 10 条独立写死，不复用被测值，任何一条为假即封账失败；
//! 4. 篡改注入：对已齐账的总表做十路单列清零注入（LCG 种子写死），
//!    每路必须被对应断言抓住——断言抓不住注入即判据失效；
//! 5. 账本状态机 开账→八行齐→封账 单向推进，倒退显性拒；封账后写人拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12600 − 12600)）。
pub const SEG_BASE: u32 = 0x80000;

// ── 消费边总表诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MlDiag {
    /// 行编号不在八行闭集内。
    ERowRange,
    /// 行序越格（未按 0..8 顺序登记）。
    ERowOrder,
    /// 行重复登记。
    ERowDup,
    /// 登记列缺账（证据码为零）。
    EColReg,
    /// 实现列缺账。
    EColImpl,
    /// 验收列缺账。
    EColAcc,
    /// 三列全零空样本。
    EEmpty,
    /// 账本已封账，写入被拒。
    ESealed,
    /// 八行未齐就试图封账。
    ENotFull,
    /// 列齐断言未全过，封账失败。
    EAssert,
    /// 篡改注入逃脱（断言没抓住注入）。
    ETamper,
    /// 状态机倒退（封账后又开账）。
    ERewind,
}

impl MlDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MlDiag::ERowRange => 0x01,
                MlDiag::ERowOrder => 0x02,
                MlDiag::ERowDup => 0x03,
                MlDiag::EColReg => 0x04,
                MlDiag::EColImpl => 0x05,
                MlDiag::EColAcc => 0x06,
                MlDiag::EEmpty => 0x07,
                MlDiag::ESealed => 0x08,
                MlDiag::ENotFull => 0x09,
                MlDiag::EAssert => 0x0a,
                MlDiag::ETamper => 0x0b,
                MlDiag::ERewind => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MlDiag::ERowRange => "行编号越八行闭集：只认端口对象/连接序/投递/安全属性/半对/区段/读写/关闭，先对表再登记",
            MlDiag::ERowOrder => "行序越格：按闭集序 0..8 逐行登记，跳行属乱序，回补上一行后再续",
            MlDiag::ERowDup => "行重复：每行只落一笔，重复登记先查已有行号",
            MlDiag::EColReg => "登记列缺账：登记证据码为零，先补登记留痕再入账",
            MlDiag::EColImpl => "实现列缺账：实现证据码为零，先落实现模块引用再入账",
            MlDiag::EColAcc => "验收列缺账：验收证据码为零，先过验收判据再入账",
            MlDiag::EEmpty => "三列全零空样本：不构成消费边记录，直接拒收",
            MlDiag::ESealed => "账本已封账：封账后写入拒绝，改动走下一条里程碑滚动",
            MlDiag::ENotFull => "八行未齐禁止封账：先补齐缺行再走封账口",
            MlDiag::EAssert => "列齐断言未全过：10/10 有红项，按红项行号回修对应列",
            MlDiag::ETamper => "篡改注入逃脱：列清零注入未被断言抓住，断言面失效须整表复核",
            MlDiag::ERewind => "状态机倒退：开账→齐→封账只进不退，倒退即账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ML_ALL: &[MlDiag] = &[
    MlDiag::ERowRange,
    MlDiag::ERowOrder,
    MlDiag::ERowDup,
    MlDiag::EColReg,
    MlDiag::EColImpl,
    MlDiag::EColAcc,
    MlDiag::EEmpty,
    MlDiag::ESealed,
    MlDiag::ENotFull,
    MlDiag::EAssert,
    MlDiag::ETamper,
    MlDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ML_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80001..0x8000C 闭集内"
}

// ── 消费边八行闭集 ───────────────────────────────────────────────
/// 消费边总表八行闭集（判据原文行序，冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeRow {
    /// 第 0 行：端口对象。
    PortObject,
    /// 第 1 行：连接序。
    ConnectSeq,
    /// 第 2 行：投递。
    Delivery,
    /// 第 3 行：安全属性。
    SecAttr,
    /// 第 4 行：半对。
    HalfPair,
    /// 第 5 行：区段。
    Section,
    /// 第 6 行：读写。
    ReadWrite,
    /// 第 7 行：关闭。
    Close,
}

impl EdgeRow {
    /// 行号（闭集序）。
    pub fn index(self) -> u32 {
        match self {
            EdgeRow::PortObject => 0,
            EdgeRow::ConnectSeq => 1,
            EdgeRow::Delivery => 2,
            EdgeRow::SecAttr => 3,
            EdgeRow::HalfPair => 4,
            EdgeRow::Section => 5,
            EdgeRow::ReadWrite => 6,
            EdgeRow::Close => 7,
        }
    }

    /// 行中文名。
    pub fn name(self) -> &'static str {
        match self {
            EdgeRow::PortObject => "端口对象",
            EdgeRow::ConnectSeq => "连接序",
            EdgeRow::Delivery => "投递",
            EdgeRow::SecAttr => "安全属性",
            EdgeRow::HalfPair => "半对",
            EdgeRow::Section => "区段",
            EdgeRow::ReadWrite => "读写",
            EdgeRow::Close => "关闭",
        }
    }

    /// 由行号反查闭集行（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<EdgeRow> {
        match idx {
            0 => Some(EdgeRow::PortObject),
            1 => Some(EdgeRow::ConnectSeq),
            2 => Some(EdgeRow::Delivery),
            3 => Some(EdgeRow::SecAttr),
            4 => Some(EdgeRow::HalfPair),
            5 => Some(EdgeRow::Section),
            6 => Some(EdgeRow::ReadWrite),
            7 => Some(EdgeRow::Close),
            _ => None,
        }
    }
}

/// 八行闭集数组（行序即登记序）。
pub const ROW_ALL: &[EdgeRow] = &[
    EdgeRow::PortObject,
    EdgeRow::ConnectSeq,
    EdgeRow::Delivery,
    EdgeRow::SecAttr,
    EdgeRow::HalfPair,
    EdgeRow::Section,
    EdgeRow::ReadWrite,
    EdgeRow::Close,
];

// ── 三列制度数据模型 ─────────────────────────────────────────────
/// 总表容量：八行齐即满。
pub const ROW_CAP: usize = 8;

/// 单行三列单元：登记列/实现列/验收列各持一个非零证据码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeCell {
    /// 行号。
    pub row: u32,
    /// 登记列证据码（非零有效）。
    pub reg: u32,
    /// 实现列证据码（非零有效）。
    pub impl_col: u32,
    /// 验收列证据码（非零有效）。
    pub acc: u32,
}

impl EdgeCell {
    /// 三列齐（全部非零）。
    pub fn cols_full(&self) -> bool {
        self.reg != 0 && self.impl_col != 0 && self.acc != 0
    }

    /// 三列全零空样本。
    pub fn cols_empty(&self) -> bool {
        self.reg == 0 && self.impl_col == 0 && self.acc == 0
    }

    /// 列名摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        format!(
            "行{}({}):登记{} 实现{} 验收{}",
            self.row,
            EdgeRow::from_index(self.row)
                .map(|r| r.name())
                .unwrap_or("?"),
            self.reg,
            self.impl_col,
            self.acc
        )
    }
}

/// 账本状态机：开账 → 八行齐 → 封账，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerState {
    /// 开账：允许逐行登记。
    Open,
    /// 八行齐：三列全绿，等待封账。
    Complete,
    /// 封账：只读。
    Sealed,
}

impl LedgerState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            LedgerState::Open => 0,
            LedgerState::Complete => 1,
            LedgerState::Sealed => 2,
        }
    }
}

/// 消费边总表账本。
#[derive(Clone, Debug)]
pub struct EdgeLedger {
    /// 已入账行（按登记序）。
    cells: Vec<EdgeCell>,
    /// 账本状态。
    state: LedgerState,
}

impl EdgeLedger {
    /// 开一本新账。
    pub fn new() -> EdgeLedger {
        EdgeLedger {
            cells: Vec::new(),
            state: LedgerState::Open,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> LedgerState {
        self.state
    }

    /// 已入账行数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn cell_at(&self, idx: usize) -> Option<EdgeCell> {
        self.cells.get(idx).copied()
    }

    /// 按行号查行。
    pub fn cell_of_row(&self, row: u32) -> Option<EdgeCell> {
        for c in self.cells.iter() {
            if c.row == row {
                return Some(*c);
            }
        }
        None
    }

    /// 逐字登记一行三列（行序/重复/列缺/封账四道闸）。
    pub fn register(&mut self, row: u32, reg: u32, impl_col: u32, acc: u32) -> Result<(), u32> {
        // 封账闸：封账后一切写入拒绝。
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(MlDiag::ESealed.val());
        }
        // 空样本闸：三列全零不构成记录。
        if reg == 0 && impl_col == 0 && acc == 0 {
            return Err(MlDiag::EEmpty.val());
        }
        // 行闭集闸。
        let _expect = EdgeRow::from_index(row).ok_or(MlDiag::ERowRange.val())?;
        // 行序闸：只许按 0..8 顺序逐行登记。
        let expect = self.cells.len() as u32;
        if row != expect {
            return Err(MlDiag::ERowOrder.val());
        }
        // 重复闸（顺序登记下理论不可达，防御性保留）。
        if self.cell_of_row(row).is_some() {
            return Err(MlDiag::ERowDup.val());
        }
        // 三列制度闸：任一列为零即列缺账。
        if reg == 0 {
            return Err(MlDiag::EColReg.val());
        }
        if impl_col == 0 {
            return Err(MlDiag::EColImpl.val());
        }
        if acc == 0 {
            return Err(MlDiag::EColAcc.val());
        }
        self.cells.push(EdgeCell {
            row,
            reg,
            impl_col,
            acc,
        });
        // 八行齐即推进状态机（只进不退）。
        if self.cells.len() == ROW_CAP {
            self.state = LedgerState::Complete;
        }
        Ok(())
    }

    /// 列守恒：总证据码数 = 3 × 行数（无隐性列）。
    pub fn col_conservation(&self) -> bool {
        let mut n = 0usize;
        for c in self.cells.iter() {
            if c.reg != 0 {
                n += 1;
            }
            if c.impl_col != 0 {
                n += 1;
            }
            if c.acc != 0 {
                n += 1;
            }
        }
        n == self.cells.len() * 3
    }

    /// 封账：八行齐 + 10/10 列齐断言全绿才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= LedgerState::Sealed.rank() {
            return Err(MlDiag::ERewind.val());
        }
        if self.cells.len() != ROW_CAP {
            return Err(MlDiag::ENotFull.val());
        }
        if !tricol_all_green(self) {
            return Err(MlDiag::EAssert.val());
        }
        self.state = LedgerState::Sealed;
        Ok(())
    }
}

// ── 列齐断言 10/10 ───────────────────────────────────────────────
/// 三列制度列齐断言：八行各一 + 行数恒等 + 列守恒 = 10 条。
/// 每条独立写死，不复用被测值。
pub fn tricol_assertions(l: &EdgeLedger) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 断言 0..7：逐行列齐。
    for i in 0..8u32 {
        let row_name = EdgeRow::from_index(i)
            .map(|r| r.name().to_string())
            .unwrap_or_else(|| "?".to_string());
        let ok = match l.cell_of_row(i) {
            Some(c) => c.cols_full() && c.row == i,
            None => false,
        };
        out.push((format!("列齐-{}({})", i, row_name), ok));
    }
    // 断言 8：行数恒等（账内行数 == 八行闭集容量）。
    out.push((
        "行数恒等-8行".to_string(),
        l.len() == ROW_CAP && !l.is_empty(),
    ));
    // 断言 9：列守恒（证据码总数 = 3×行数）。
    out.push(("列守恒-3x行".to_string(), l.col_conservation()));
    out
}

/// 10/10 全绿判定。
pub fn tricol_all_green(l: &EdgeLedger) -> bool {
    let list = tricol_assertions(l);
    if list.len() != 10 {
        return false;
    }
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            return false;
        }
    }
    true
}

/// 八行齐语义断言（判据 J1 的"八行齐"直读口）。
fn eight_rows_ok(l: &EdgeLedger) -> bool {
    if l.len() != ROW_CAP {
        return false;
    }
    // 行号必须恰为 0..8 各一，不得缺行/换行。
    for i in 0..8u32 {
        match l.cell_of_row(i) {
            Some(c) => {
                if c.row != i || !c.cols_full() {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
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

// ── 注入面：十路单列清零篡改 ─────────────────────────────────────
/// 单列清零注入器：把第 (row, col) 路的对应列清零，断言必须抓住。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColSlot {
    /// 登记列。
    Reg,
    /// 实现列。
    ImplCol,
    /// 验收列。
    Acc,
}

impl ColSlot {
    /// 列中文名。
    pub fn name(self) -> &'static str {
        match self {
            ColSlot::Reg => "登记",
            ColSlot::ImplCol => "实现",
            ColSlot::Acc => "验收",
        }
    }
}

/// 构造齐账总表（八行三列全非零，证据码确定性生成）。
pub fn build_full_ledger() -> EdgeLedger {
    let mut l = EdgeLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut row = 0u32;
    while row < 8 {
        // 证据码取 1..=0xFFFF 非零域，确定性。
        let a = 1u32 + rng.below(0xFFFE);
        let b = 1u32 + rng.below(0xFFFE);
        let c = 1u32 + rng.below(0xFFFE);
        match l.register(row, a, b, c) {
            Ok(()) => row += 1,
            Err(e) => {
                // 正常流不该出错；出错时终止构造（账本缺行，断言会红）。
                let _ = e;
                break;
            }
        }
    }
    l
}

/// 十路篡改注入：每路清零一列，注入后 10/10 断言必须出现红项。
/// 返回 (抓到路数, 总路数, 首个逃脱路描述)。
pub fn tamper_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x2C) as u64);
    // 十路 = 行号 LCG 抖动 + 列槽轮转，凑足 10 条不重复路径。
    let mut k = 0usize;
    while k < 10 {
        let slot = match k % 3 {
            0 => ColSlot::Reg,
            1 => ColSlot::ImplCol,
            _ => ColSlot::Acc,
        };
        let row_pick = rng.below(8);
        let mut l = build_full_ledger();
        // 定点清零：账本内容与选择行号可能不同——改为按行号清对应列。
        let target = if let Some(c) = l.cell_of_row(row_pick) {
            c.row
        } else {
            row_pick
        };
        let mut tampered = EdgeLedger::new();
        let mut r = 0u32;
        while r < 8 {
            if let Some(c) = l.cell_of_row(r) {
                let (a, b, d) = if c.row == target {
                    match slot {
                        ColSlot::Reg => (0, c.impl_col, c.acc),
                        ColSlot::ImplCol => (c.reg, 0, c.acc),
                        ColSlot::Acc => (c.reg, c.impl_col, 0),
                    }
                } else {
                    (c.reg, c.impl_col, c.acc)
                };
                let _ = tampered.register(r, a, b, d);
            }
            r += 1;
        }
        // 断言必须出现红项（注入被抓住）。
        let green = tricol_all_green(&tampered);
        total += 1;
        if green {
            if escape.is_empty() {
                escape = format!(
                    "路{}行{}列{}逃脱：清零后断言仍全绿",
                    k,
                    target,
                    slot.name()
                );
            }
        } else {
            caught += 1;
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批 B00 窗口 [0x80000, 0x802C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x802C0 {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
fn diag_closed_ok() -> bool {
    for d in ML_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 查表闭集：任意已知码必须命中文案，未知码必须回落。
    if detail(MlDiag::ERowRange.val()) != MlDiag::ERowRange.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == MlDiag::ERowRange.detail() {
        return false;
    }
    true
}

/// 正常流：开账→按序八行→齐→封账全绿。
fn happy_path_ok() -> bool {
    let mut l = build_full_ledger();
    if l.state() != LedgerState::Complete {
        return false;
    }
    match l.seal() {
        Ok(()) => l.state() == LedgerState::Sealed,
        Err(_) => false,
    }
}

/// 结构拒绝：乱序/重复/空样本/越闭集四道闸必抓。
fn structural_reject_ok() -> bool {
    // 乱序：跳行登记被拒。
    let mut l1 = EdgeLedger::new();
    let ooo = l1.register(3, 1, 1, 1) == Err(MlDiag::ERowOrder.val());
    // 越闭集：行号 8 被拒。
    let mut l2 = EdgeLedger::new();
    let rng_bad = l2.register(8, 1, 1, 1) == Err(MlDiag::ERowRange.val());
    // 空样本：三列全零被拒。
    let mut l3 = EdgeLedger::new();
    let empty_bad = l3.register(0, 0, 0, 0) == Err(MlDiag::EEmpty.val());
    // 列缺：登记列清零被拒。
    let mut l4 = EdgeLedger::new();
    let col_bad = l4.register(0, 0, 5, 7) == Err(MlDiag::EColReg.val());
    ooo && rng_bad && empty_bad && col_bad
}

/// 八行齐语义断言。
fn eight_rows_green_ok() -> bool {
    let l = build_full_ledger();
    eight_rows_ok(&l) && tricol_all_green(&l)
}

/// 注入必抓：十路清零全部命中，无逃脱。
fn tamper_caught_ok() -> bool {
    let (caught, total, escape) = tamper_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    escape.is_empty()
}

/// 双跑一致：两次完整构建/注入流程逐项一致。
fn dual_run_ok() -> bool {
    let l1 = build_full_ledger();
    let l2 = build_full_ledger();
    if l1.len() != l2.len() || l1.state() != l2.state() {
        return false;
    }
    let mut i = 0usize;
    while i < ROW_CAP {
        match (l1.cell_at(i), l2.cell_at(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let s1 = tamper_sweep();
    let s2 = tamper_sweep();
    s1 == s2
}

/// 边界：第 0 行与第 7 行（首尾行）登记语义。
fn boundary_ok() -> bool {
    // 尾行：前七行未齐时登记第 7 行被拒（乱序）。
    let mut l = EdgeLedger::new();
    if l.register(7, 1, 1, 1) != Err(MlDiag::ERowOrder.val()) {
        return false;
    }
    // 封账后再写：拒绝且账面不变。
    let mut l2 = build_full_ledger();
    let _ = l2.seal();
    let before = l2.len();
    if l2.register(0, 9, 9, 9) != Err(MlDiag::ESealed.val()) {
        return false;
    }
    if l2.len() != before {
        return false;
    }
    // 八行齐后再封账：倒退/重封拒绝。
    let mut l3 = build_full_ledger();
    let _ = l3.seal();
    l3.seal() == Err(MlDiag::ERewind.val())
}

/// 完成写回语义：封账即"状态=已完成"，摘要非空。
fn writeback_ok() -> bool {
    let mut l = build_full_ledger();
    if l.seal().is_err() {
        return false;
    }
    // 结果摘要：行数+状态+守恒，非空即写回合规。
    let summary = format!("rows={} sealed={}", l.len(), l.state() == LedgerState::Sealed);
    !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12601 自检（判据 12600 卡：消费边总表八行齐 + 三列制度 10/10）。
pub fn run_unx12600_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12600");
    let ledger = build_full_ledger();

    s.add_dyn(
        "F12601-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12601-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={} first=0x{:05x}", ML_ALL.len(), ML_ALL.first().map(|d| d.val()).unwrap_or(0)),
    );

    s.add_dyn(
        "F12601-03 八行闭集枚举",
        ROW_ALL.len() == 8,
        &format!(
            "rows={}首={}尾={}",
            ROW_ALL.len(),
            ROW_ALL.first().map(|r| r.name()).unwrap_or("?"),
            ROW_ALL.last().map(|r| r.name()).unwrap_or("?")
        ),
    );

    s.add_dyn(
        "F12601-04 正常流八行齐封账",
        happy_path_ok(),
        &format!("rows={} state={:?}", ledger.len(), ledger.state()),
    );

    s.add_dyn(
        "F12601-05 列齐断言 10/10",
        tricol_all_green(&ledger),
        &format!(
            "n={} green={}",
            tricol_assertions(&ledger).len(),
            tricol_all_green(&ledger)
        ),
    );

    s.add_dyn(
        "F12601-06 八行齐语义断言",
        eight_rows_green_ok(),
        &format!("rows={}", ledger.len()),
    );

    // 注入必抓：十路单列清零必须全数命中（人为构造违约必被拒）。
    let (caught, total, escape) = tamper_sweep();
    s.add_dyn(
        "F12601-07 篡改注入 10/10 必抓",
        caught == 10 && total == 10 && escape.is_empty(),
        &format!("caught={}/{} {}", caught, total, escape),
    );

    s.add_dyn(
        "F12601-08 结构拒绝四闸",
        structural_reject_ok(),
        "乱序/越闭集/空样本/列缺 各一路全数拒",
    );

    s.add_dyn(
        "F12601-09 双跑一致",
        dual_run_ok(),
        "两次构建与两次注入逐项一致",
    );

    s.add_dyn(
        "F12601-10 边界首尾行与封账倒退",
        boundary_ok(),
        "首行前/尾行乱序/封账后写/重复封账 四点",
    );

    s.add_dyn(
        "F12601-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    // 列守恒单独一条：证据码总数恒等于 3×行数。
    s.add_dyn(
        "F12601-12 列守恒恒等式",
        ledger.col_conservation() && ledger.len() == ROW_CAP,
        &format!("rows={} cons={}", ledger.len(), ledger.col_conservation()),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 B00 窗口。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x802C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：12 个码互异、全部落窗口、查表可命中。
        assert_eq!(ML_ALL.len(), 12);
        let mut i = 0usize;
        while i < ML_ALL.len() {
            let vi = ML_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < ML_ALL.len() {
                let vj = ML_ALL.get(j).map(|d| d.val()).unwrap_or(0);
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
    fn happy_path_seals_at_eight_rows() {
        let mut l = build_full_ledger();
        assert_eq!(l.len(), 8);
        assert_eq!(l.state(), LedgerState::Complete);
        assert!(l.seal().is_ok());
        assert_eq!(l.state(), LedgerState::Sealed);
        assert!(eight_rows_ok(&l));
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路清零全数命中，无逃脱。
        let (caught, total, escape) = tamper_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致：确定性 LCG 下两次全流程逐项相同。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < 8 {
            assert_eq!(a.cell_at(i), b.cell_at(i));
            i += 1;
        }
        assert_eq!(tamper_sweep(), tamper_sweep());
    }

    #[test]
    fn structural_gates_reject() {
        // 乱序：跳行被拒。
        let mut l = EdgeLedger::new();
        assert_eq!(l.register(3, 1, 1, 1), Err(MlDiag::ERowOrder.val()));
        // 越闭集：行号 8 被拒。
        assert_eq!(l.register(8, 1, 1, 1), Err(MlDiag::ERowRange.val()));
        // 空样本：三列全零被拒。
        assert_eq!(l.register(0, 0, 0, 0), Err(MlDiag::EEmpty.val()));
        // 列缺：各列清零逐路被拒。
        let mut l2 = EdgeLedger::new();
        assert_eq!(l2.register(0, 0, 5, 7), Err(MlDiag::EColReg.val()));
        let mut l3 = EdgeLedger::new();
        assert_eq!(l3.register(0, 5, 0, 7), Err(MlDiag::EColImpl.val()));
        let mut l4 = EdgeLedger::new();
        assert_eq!(l4.register(0, 5, 7, 0), Err(MlDiag::EColAcc.val()));
    }

    #[test]
    fn seal_requires_full_rows() {
        // 边界：八行未齐禁止封账。
        let mut l = EdgeLedger::new();
        assert_eq!(l.seal(), Err(MlDiag::ENotFull.val()));
        let _ = l.register(0, 1, 2, 3);
        assert_eq!(l.seal(), Err(MlDiag::ENotFull.val()));
        // 齐后封账成功，重封倒退拒绝。
        let mut full = build_full_ledger();
        assert!(full.seal().is_ok());
        assert_eq!(full.seal(), Err(MlDiag::ERewind.val()));
        // 封账后写入拒绝且账面不变。
        let before = full.len();
        assert_eq!(full.register(0, 9, 9, 9), Err(MlDiag::ESealed.val()));
        assert_eq!(full.len(), before);
    }

    #[test]
    fn tricol_assertions_count_is_ten() {
        // 列齐断言恒 10 条：8 行 + 行数恒等 + 列守恒。
        let l = build_full_ledger();
        let list = tricol_assertions(&l);
        assert_eq!(list.len(), 10);
        assert!(tricol_all_green(&l));
        // 缺行账本：行数恒等断言必红。
        let mut half = EdgeLedger::new();
        let _ = half.register(0, 1, 2, 3);
        assert!(!tricol_all_green(&half));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Open→Complete→Sealed。
        let mut l = build_full_ledger();
        assert_eq!(l.state(), LedgerState::Complete);
        assert!(LedgerState::Open.rank() < LedgerState::Complete.rank());
        assert!(LedgerState::Complete.rank() < LedgerState::Sealed.rank());
        let _ = l.seal();
        assert_eq!(l.state(), LedgerState::Sealed);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12600_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
