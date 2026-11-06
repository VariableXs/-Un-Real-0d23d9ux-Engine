//! UNX-F10034 清偿报表段（清偿率列） · 0x91840段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10035-J1 清偿闸生效（计划内未偿闸保持红 10/10 次），越闸
//! 10/10 次拦截。
//!
//! SEG_BASE = 0x91840：F10034 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0D 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 清偿列按「三格行模型」建模：每行三格——存量格（stock）、已偿格
//!      （repaid）、占比格（ratio‰ = repaid×1000/stock，冻结推导式）；三格
//!      齐（存量/已偿/占比）且逐格取数正确 10/10 次，混列 10/10 次检出。
//!   ② 解析器 + 账本 + 状态机三件齐：行句式 `INN:SSS/RRR` 解析、行账本
//!      （逐行可查 + 总和守恒）、报表生命周期 Draft→Computed→Frozen 只进
//!      不退。
//!   ③ 注入核四类坏样本：混列（已偿>存量）/ 零存量 / 重复条目 / 句式坏，
//!      逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 占比只做整数千分比（无浮点），口径可复算：同输入两次逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10034 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91840;

/// 报表行容量（行数上限）。
pub const ROW_CAPACITY: usize = 16;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportCode {
    /// 行句式坏：不符 `INN:SSS/RRR`
    BadRowSyntax,
    /// 混列：已偿 > 存量（两格互换或越格）
    MixedCells,
    /// 零存量：分母为零
    ZeroStock,
    /// 重复条目
    DuplicateItem,
    /// 容量满
    RowsFull,
    /// 未计算冻结
    FreezeBeforeCompute,
    /// 冻结后再入行
    RowAfterFreeze,
    /// 状态回退
    StateRegress,
    /// 占比复核不符：占比格与推导式不一致
    RatioMismatch,
    /// 空表冻结
    EmptyReport,
    /// 双跑不一致
    NotReproducible,
    /// 数值越界：格子超 9999
    ValueOutOfRange,
    /// 段位越界
    SegmentViolation,
}

impl ReportCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ReportCode::BadRowSyntax => "行句式坏被拒：必须为 `INN:SSS/RRR`；\
                请按条目/存量/已偿重排后再入表",
            ReportCode::MixedCells => "混列被拒：已偿格不得大于存量格；\
                请核对两格数值，混列视为取数错误",
            ReportCode::ZeroStock => "零存量被拒：存量格为零则占比无分母；\
                请先补存量或删除该行",
            ReportCode::DuplicateItem => "重复条目被拒：同条目行已在表；\
                请先查表再补录，重复回 EEXIST",
            ReportCode::RowsFull => "容量满被拒：报表行数已用尽；请先归档\
                旧表再开新表",
            ReportCode::FreezeBeforeCompute => "未计算冻结被拒：先走 Computed \
                才能冻结；请按 Draft→Computed→Frozen 次序",
            ReportCode::RowAfterFreeze => "冻结后入行被拒：报表冻结即只读；\
                补行请开新一期报表",
            ReportCode::StateRegress => "状态回退被拒：报表生命周期只进不退；\
                回退视为私改报表",
            ReportCode::RatioMismatch => "占比复核不符：占比格必须等于 \
                repaid×1000/stock 整数推导值；请按冻结推导式复算",
            ReportCode::EmptyReport => "空表冻结被拒：零行报表不得冻结；\
                请先入行",
            ReportCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            ReportCode::ValueOutOfRange => "数值越界被拒：格子取值 1..=9999；\
                越界数值请拆行或缩口径",
            ReportCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91840..=0x918C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [ReportCode; 13] = [
        ReportCode::BadRowSyntax,
        ReportCode::MixedCells,
        ReportCode::ZeroStock,
        ReportCode::DuplicateItem,
        ReportCode::RowsFull,
        ReportCode::FreezeBeforeCompute,
        ReportCode::RowAfterFreeze,
        ReportCode::StateRegress,
        ReportCode::RatioMismatch,
        ReportCode::EmptyReport,
        ReportCode::NotReproducible,
        ReportCode::ValueOutOfRange,
        ReportCode::SegmentViolation,
    ];
}

// ── 行句式解析器 ──────────────────────────────────────────────────────
/// 报表行：条目号 + 三格（存量/已偿/占比）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepayRow {
    /// 条目号（1..=99）
    pub item: u8,
    /// 存量格（1..=9999）
    pub stock: u16,
    /// 已偿格（0..=stock）
    pub repaid: u16,
    /// 占比格（repaid×1000/stock，冻结推导式）
    pub ratio_permille: u16,
}

/// 冻结推导式：占比 = repaid×1000/stock（整数除法，可复算）。
pub fn derive_ratio(stock: u16, repaid: u16) -> u16 {
    if stock == 0 {
        return 0;
    }
    ((repaid as u32 * 1000) / stock as u32) as u16
}

/// 行句式解析：`INN:SSS/RRR`；SSS/RRR 各 1..4 位数字，值 1..=9999（RRR 可为 0）。
pub fn parse_row(text: &str) -> Result<RepayRow, ReportCode> {
    let bytes = text.as_bytes();
    // 头：'I' + 两位数字 + ':'，共 4 字节。
    if bytes.len() < 7 {
        return Err(ReportCode::BadRowSyntax);
    }
    let head_i = match bytes.get(0) {
        Some(b) => *b,
        None => return Err(ReportCode::BadRowSyntax),
    };
    if head_i != b'I' {
        return Err(ReportCode::BadRowSyntax);
    }
    let d1 = match bytes.get(1) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(ReportCode::BadRowSyntax),
    };
    let d2 = match bytes.get(2) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(ReportCode::BadRowSyntax),
    };
    let colon = match bytes.get(3) {
        Some(b) => *b,
        None => return Err(ReportCode::BadRowSyntax),
    };
    if colon != b':' {
        return Err(ReportCode::BadRowSyntax);
    }
    let item = d1 * 10 + d2;
    if item < 1 {
        return Err(ReportCode::BadRowSyntax);
    }
    // 存量段：数字直到 '/'。
    let mut i = 4usize;
    let mut stock: u32 = 0;
    let mut stock_digits = 0usize;
    loop {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => return Err(ReportCode::BadRowSyntax),
        };
        if b == b'/' {
            break;
        }
        if !b.is_ascii_digit() {
            return Err(ReportCode::BadRowSyntax);
        }
        stock = stock * 10 + (b - b'0') as u32;
        stock_digits += 1;
        if stock_digits > 4 {
            return Err(ReportCode::ValueOutOfRange);
        }
        i += 1;
    }
    // 已偿段：数字到行尾。
    i += 1;
    let mut repaid: u32 = 0;
    let mut repaid_digits = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(b) => *b,
            None => return Err(ReportCode::BadRowSyntax),
        };
        if !b.is_ascii_digit() {
            return Err(ReportCode::BadRowSyntax);
        }
        repaid = repaid * 10 + (b - b'0') as u32;
        repaid_digits += 1;
        if repaid_digits > 4 {
            return Err(ReportCode::ValueOutOfRange);
        }
        i += 1;
    }
    if stock_digits == 0 || repaid_digits == 0 {
        return Err(ReportCode::BadRowSyntax);
    }
    if stock > 9999 || repaid > 9999 {
        return Err(ReportCode::ValueOutOfRange);
    }
    if stock == 0 {
        return Err(ReportCode::ZeroStock);
    }
    if repaid > stock {
        return Err(ReportCode::MixedCells);
    }
    let stock16 = stock as u16;
    let repaid16 = repaid as u16;
    Ok(RepayRow {
        item,
        stock: stock16,
        repaid: repaid16,
        ratio_permille: derive_ratio(stock16, repaid16),
    })
}

// ── 报表生命周期状态机 ────────────────────────────────────────────────
/// 报表生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportState {
    /// 起草：可入行
    Draft,
    /// 已计算：可冻结
    Computed,
    /// 已冻结：只读
    Frozen,
}

/// 清偿报表：行账本 + 三格口径 + 状态机。
#[derive(Clone, Debug)]
pub struct RepayReport {
    state: ReportState,
    rows: Vec<RepayRow>,
}

impl RepayReport {
    /// 构造 Draft 空表。
    pub fn new() -> RepayReport {
        RepayReport { state: ReportState::Draft, rows: Vec::new() }
    }

    /// 当前状态。
    pub fn state(&self) -> ReportState {
        self.state
    }

    /// 入行：Draft 才可入；句式 + 重复 + 容量守卫。
    pub fn add_row(&mut self, text: &str) -> Result<(), ReportCode> {
        if self.state != ReportState::Draft {
            return Err(ReportCode::RowAfterFreeze);
        }
        let r = parse_row(text)?;
        if self.rows.len() >= ROW_CAPACITY {
            return Err(ReportCode::RowsFull);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(x) = self.rows.get(i) {
                if x.item == r.item {
                    return Err(ReportCode::DuplicateItem);
                }
            }
            i += 1;
        }
        self.rows.push(r);
        Ok(())
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 按条目号查行。
    pub fn row_of(&self, item: u8) -> Option<RepayRow> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.item == item {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 存量总和（分母口径）。
    pub fn stock_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                s = s.saturating_add(r.stock as u32);
            }
            i += 1;
        }
        s
    }

    /// 已偿总和（分子口径）。
    pub fn repaid_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                s = s.saturating_add(r.repaid as u32);
            }
            i += 1;
        }
        s
    }

    /// 总占比（∑repaid×1000/∑stock，与行级同推导式）。
    pub fn total_ratio_permille(&self) -> u16 {
        let st = self.stock_sum();
        if st == 0 {
            return 0;
        }
        ((self.repaid_sum() * 1000) / st) as u16
    }

    /// 三格复核：逐行占比格与推导式一致。
    pub fn cells_consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.ratio_permille != derive_ratio(r.stock, r.repaid) {
                    return false;
                }
                if r.repaid > r.stock || r.stock == 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 计算：Draft → Computed；要求三格齐（非空且一致）。
    pub fn compute(&mut self) -> Result<(), ReportCode> {
        if self.state != ReportState::Draft {
            return Err(ReportCode::StateRegress);
        }
        if self.rows.is_empty() {
            return Err(ReportCode::EmptyReport);
        }
        if !self.cells_consistent() {
            return Err(ReportCode::RatioMismatch);
        }
        self.state = ReportState::Computed;
        Ok(())
    }

    /// 冻结：Computed → Frozen；Draft 直接冻结拒绝。
    pub fn freeze(&mut self) -> Result<(), ReportCode> {
        if self.state == ReportState::Draft {
            return Err(ReportCode::FreezeBeforeCompute);
        }
        if self.state == ReportState::Frozen {
            return Err(ReportCode::StateRegress);
        }
        self.state = ReportState::Frozen;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(ReportCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本：0 混列、1 零存量、2 重复条目、3 句式坏。
pub const BAD_SAMPLES: [(&str, usize); 4] = [
    ("I01:40/100", 0), // 已偿>存量 → 混列
    ("I02:0/0", 1),    // 零存量 → 零分母
    ("I01:100/40", 2), // 与首行同条目 → 重复
    ("I03:100-", 3),   // 句式坏
];

/// 执行注入矩阵：4 类坏样本 × 10 轮（首行 I01:100/40 垫底）。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < BAD_SAMPLES.len() {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut rp = RepayReport::new();
            let _ = rp.add_row("I01:100/40");
            let (text, want) = match BAD_SAMPLES.get(c) {
                Some((t, w)) => (*t, *w),
                None => ("", 3),
            };
            let res = rp.add_row(text);
            let caught = match want {
                0 => res.err() == Some(ReportCode::MixedCells),
                1 => res.err() == Some(ReportCode::ZeroStock),
                2 => res.err() == Some(ReportCode::DuplicateItem),
                _ => res.err() == Some(ReportCode::BadRowSyntax),
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 构造完整报表：四行三格齐 → 计算 → 冻结。
pub fn build_frozen_report() -> RepayReport {
    let mut rp = RepayReport::new();
    let _ = rp.add_row("I01:100/40");
    let _ = rp.add_row("I02:200/100");
    let _ = rp.add_row("I03:50/50");
    let _ = rp.add_row("I04:800/200");
    let _ = rp.compute();
    let _ = rp.freeze();
    rp
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；冻结后写入/状态回退族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺行/空表族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复条目族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；句式/混列族"),
    ("ERANGE", -34, 206, "Windows ERROR_ARITHMETIC_OVERFLOW；数值越界族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 报表指纹：状态 + 逐行四字段入哈希。
pub fn report_fingerprint(r: &RepayReport) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match r.state() {
        ReportState::Draft => 0,
        ReportState::Computed => 1,
        ReportState::Frozen => 2,
    });
    bytes.push(r.row_count() as u8);
    let mut i = 0usize;
    while i < r.row_count() {
        if let Some(x) = r.rows.get(i) {
            bytes.push(x.item);
            bytes.push((x.stock & 0xFF) as u8);
            bytes.push((x.stock >> 8) as u8);
            bytes.push((x.repaid & 0xFF) as u8);
            bytes.push((x.repaid >> 8) as u8);
            bytes.push((x.ratio_permille & 0xFF) as u8);
            bytes.push((x.ratio_permille >> 8) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10034 判据集（17 条 + 2 条动态明细）。
pub fn run_unx10034_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10034");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91840..=0x918C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91840..=0x918C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 行句式解析：合法样本逐段命中
    let p = parse_row("I02:200/100");
    let parse_ok = p == Ok(RepayRow { item: 2, stock: 200, repaid: 100,
        ratio_permille: 500 });
    s.add("行解析：`INN:SSS/RRR` 逐段命中且占比推导正确", parse_ok,
          "三格行必须能解析回条目/存量/已偿，占比按冻结推导式");

    // 3. 坏句式拒绝
    let bad = parse_row("X01:100/40").err() == Some(ReportCode::BadRowSyntax)
        && parse_row("I01:100").err() == Some(ReportCode::BadRowSyntax)
        && parse_row("I01:/40").err() == Some(ReportCode::BadRowSyntax)
        && parse_row("").err() == Some(ReportCode::BadRowSyntax);
    s.add("句式拒绝：错头/缺段/空段逐类拒绝", bad,
          "句式破格必须显性报错，不许静默容忍");

    // 4. 占比推导式冻结：1000/1000=1000‰、50/50=1000‰、200/800=250‰
    let ratio_ok = derive_ratio(1000, 1000) == 1000
        && derive_ratio(50, 50) == 1000
        && derive_ratio(800, 200) == 250
        && derive_ratio(0, 0) == 0;
    s.add("推导冻结：占比=repaid×1000/stock 逐例可复算", ratio_ok,
          "清偿率列口径必须成文冻结，整数推导无浮点");

    // 5. 完整报表：四行三格齐 → 计算 → 冻结
    let full = build_frozen_report();
    s.add("三格齐：完整报表四行三格齐且冻结成功",
          full.state() == ReportState::Frozen && full.row_count() == 4
              && full.cells_consistent(),
          "每行三格（存量/已偿/占比）齐备且一致后才能冻结");

    // 6. 取数正确（核心判据）：总和与总占比 10/10 次复算一致
    let mut take_ok = true;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let rp = build_frozen_report();
        if rp.stock_sum() != 1150 || rp.repaid_sum() != 390
            || rp.total_ratio_permille() != derive_ratio(1150, 390) {
            take_ok = false;
        }
        round += 1;
    }
    s.add("取数正确：存量/已偿/总占比 10/10 次复算一致", take_ok,
          "∑stock=1150、∑repaid=390、总占比=∑repaid×1000/∑stock");

    // 7. 混列检出（核心判据）：存量/已偿互换 10/10 次拦截
    let mut mix_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        // 三格互换样本：存量 40 已偿 100。
        if parse_row("I01:40/100").err() == Some(ReportCode::MixedCells) {
            mix_caught += 1;
        }
        round += 1;
    }
    s.add("混列检出：格值互换 10/10 次拦截", mix_caught == INJECT_ROUNDS,
          "已偿格不得大于存量格，混列即取数错误");

    // 8. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "混列/零存量/重复/句式坏必须全部显性拒绝");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BAD_SAMPLES.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 零存量拒绝（复测口径）
    let mut zero_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if parse_row("I05:0/0").err() == Some(ReportCode::ZeroStock) {
            zero_caught += 1;
        }
        round += 1;
    }
    s.add("零存量拦截：10/10 次拒绝", zero_caught == INJECT_ROUNDS,
          "存量格为零则占比无分母，必须显性拒绝");

    // 11. 未计算冻结拒绝
    let mut draft = RepayReport::new();
    let _ = draft.add_row("I01:100/40");
    s.add("越闸拦截：Draft 直接冻结被拒",
          draft.freeze().err() == Some(ReportCode::FreezeBeforeCompute),
          "先走 Computed 才能冻结，越位即拦");

    // 12. 冻结后入行拒绝、状态只进不退
    let mut frozen = build_frozen_report();
    let late = frozen.add_row("I05:100/10");
    s.add("状态机：冻结后入行被拒、只进不退",
          late.err() == Some(ReportCode::RowAfterFreeze)
              && frozen.state() == ReportState::Frozen,
          "Draft→Computed→Frozen 单向流，回退/越位都拒绝");

    // 13. 重复条目拒绝
    let mut dup = RepayReport::new();
    let a = dup.add_row("I01:100/40");
    let b = dup.add_row("I01:100/50");
    s.add("重复拒绝：同条目二次入行被拒",
          a.is_ok() && b.err() == Some(ReportCode::DuplicateItem),
          "重复条目必须显性拒绝，不许静默覆盖");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑指纹一致
    let fp_a = report_fingerprint(&build_frozen_report());
    let fp_b = report_fingerprint(&build_frozen_report());
    s.add("双跑一致：报表指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 16. 文案闭集
    let text_ok = ReportCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 17. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ReportCode::ALL.len() {
        let mut j = i + 1;
        while j < ReportCode::ALL.len() {
            if let (Some(a), Some(b)) = (ReportCode::ALL.get(i), ReportCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!(
        "报表指纹 0x{:08X}；∑stock={} ∑repaid={} 总占比 {}‰",
        fp_a, full.stock_sum(), full.repaid_sum(), full.total_ratio_permille()
    );
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_parse() {
        assert_eq!(parse_row("I07:50/50"),
                   Ok(RepayRow { item: 7, stock: 50, repaid: 50, ratio_permille: 1000 }));
        assert_eq!(parse_row("I01:800/200").map(|r| r.ratio_permille), Ok(250));
    }

    #[test]
    fn bad_rows_rejected() {
        assert_eq!(parse_row("X01:100/40"), Err(ReportCode::BadRowSyntax));
        assert_eq!(parse_row("I01:100/40/50"), Err(ReportCode::BadRowSyntax));
    }

    #[test]
    fn mixed_cells_detected() {
        assert_eq!(parse_row("I01:40/100"), Err(ReportCode::MixedCells));
    }

    #[test]
    fn zero_stock_detected() {
        assert_eq!(parse_row("I02:0/0"), Err(ReportCode::ZeroStock));
    }

    #[test]
    fn value_overflow_detected() {
        assert_eq!(parse_row("I03:123456/1"), Err(ReportCode::ValueOutOfRange));
    }

    #[test]
    fn full_report_freezes() {
        let rp = build_frozen_report();
        assert_eq!(rp.state(), ReportState::Frozen);
        assert_eq!(rp.stock_sum(), 1150);
        assert_eq!(rp.repaid_sum(), 390);
    }

    #[test]
    fn freeze_before_compute_rejected() {
        let mut rp = RepayReport::new();
        let _ = rp.add_row("I01:100/40");
        assert_eq!(rp.freeze(), Err(ReportCode::FreezeBeforeCompute));
    }

    #[test]
    fn row_after_freeze_rejected() {
        let mut rp = build_frozen_report();
        assert_eq!(rp.add_row("I05:100/10"), Err(ReportCode::RowAfterFreeze));
    }

    #[test]
    fn duplicate_item_rejected() {
        let mut rp = RepayReport::new();
        assert!(rp.add_row("I01:100/40").is_ok());
        assert_eq!(rp.add_row("I01:100/50"), Err(ReportCode::DuplicateItem));
    }

    #[test]
    fn empty_report_rejected() {
        let mut rp = RepayReport::new();
        assert_eq!(rp.compute(), Err(ReportCode::EmptyReport));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BAD_SAMPLES.len() as u32 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(report_fingerprint(&build_frozen_report()),
                   report_fingerprint(&build_frozen_report()));
    }

    #[test]
    fn unx10034_checks_green() {
        let set = run_unx10034_checks();
        assert!(set.all_passed(), "UNX-F10034 red: {}", set.red_items_text());
    }
}
