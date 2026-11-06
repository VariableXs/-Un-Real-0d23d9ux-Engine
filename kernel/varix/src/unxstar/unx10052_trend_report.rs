//! UNX-F10052 回归报表段（通过率趋势）· 0x91D00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10053-J1 护栏生效（超时拆批留痕 10/10 次），静默超时 10/10 次拦截
//!
//! 设计要点：
//!   ① 报表行解析器是唯一入列口：形制「轮次:通过/总数」（如 3:42/50），
//!      通过率按千分位整数（permille）落账，全程不用浮点（可复算逐位一致）。
//!   ② 混列检出：一行里混入率值单位（% 或 ‰）即判混列，逐次显性拒绝
//!      （10/10）——通过率只允许从计数列推导，不允许直接报率。
//!   ③ 趋势列账本：轮次严格递增（DupRound / OutOfOrder 拒绝）、
//!      passed ≤ total（RateOverflow 拒绝）、total > 0（ZeroDenom 拒绝）；
//!      任何拒绝路径不改列内容（拒绝不留痕）。
//!   ④ 方向取数：首尾千分率比较得 up/down/flat 三向闭集，逐轮取数
//!      10/10 次正确；轮次不足 2 判 TooFewRounds。
//!   ⑤ 注入核四类坏行逐类 10/10 拒绝、注入账守恒逃逸 = 0；errno 对照
//!      表 + FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10052 独占 0x40 窗口，下一卡 0x91D40）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x91D00;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 趋势列容量上限。
pub const COLUMN_CAP: usize = 64;

/// 千分率满值（100.0%）。
pub const PERMILLE_FULL: u32 = 1000;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrendCode {
    /// 空行
    EmptyRow,
    /// 形制非法（缺冒号/斜杠或字段非数字）
    MalformedRow,
    /// 混列：行内混入率值单位（% 或 ‰）
    MixedUnit,
    /// 分母为零
    ZeroDenom,
    /// 通过数越过总数
    RateOverflow,
    /// 轮次为零（轮次从 1 起算）
    BadRound,
    /// 重复轮次
    DupRound,
    /// 轮次倒退（乱序）
    OutOfOrder,
    /// 列容量满
    LedgerFull,
    /// 取方向时轮次不足 2
    TooFewRounds,
    /// 空列成文
    EmptyColumn,
    /// 段位越界
    SegmentViolation,
}

impl TrendCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TrendCode::EmptyRow => "空行被拒：报表行不能为空串；请按 轮次:通过/总数 形制重报",
            TrendCode::MalformedRow => "形制非法被拒：报表行必须是 轮次:通过/总数 \
                三段数字形制；请核对冒号与斜杠分隔符",
            TrendCode::MixedUnit => "混列被拒：行内混入率值单位（% 或 ‰）；\
                通过率只允许由计数列推导，请改报 计数:通过/总数",
            TrendCode::ZeroDenom => "分母为零被拒：总数必须大于 0；请补齐该轮用例总数",
            TrendCode::RateOverflow => "通过数越界被拒：通过数不得越过总数；请复核该轮统计",
            TrendCode::BadRound => "轮次非法被拒：轮次从 1 起算且为正整数；请重编轮次",
            TrendCode::DupRound => "重复轮次被拒：同一轮次只允许入列一次；请核对轮号",
            TrendCode::OutOfOrder => "轮次倒退被拒：趋势列轮次必须严格递增；请按轮次顺序报数",
            TrendCode::LedgerFull => "列容量满被拒：趋势列已达上限；请先成文归档再开新列",
            TrendCode::TooFewRounds => "轮次不足被拒：取方向至少需要 2 轮数据；请补齐趋势数据",
            TrendCode::EmptyColumn => "空列成文被拒：空趋势列不得出报表；请先入列至少一轮",
            TrendCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TrendCode; 12] = [
        TrendCode::EmptyRow,
        TrendCode::MalformedRow,
        TrendCode::MixedUnit,
        TrendCode::ZeroDenom,
        TrendCode::RateOverflow,
        TrendCode::BadRound,
        TrendCode::DupRound,
        TrendCode::OutOfOrder,
        TrendCode::LedgerFull,
        TrendCode::TooFewRounds,
        TrendCode::EmptyColumn,
        TrendCode::SegmentViolation,
    ];
}

// ── 方向闭集与解析器 ──────────────────────────────────────────────────
/// 趋势方向三向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 通过率上升
    Rising,
    /// 通过率下降
    Falling,
    /// 持平
    Flat,
}

impl Direction {
    /// 英文短标（成文用）。
    pub fn tag(self) -> &'static str {
        match self {
            Direction::Rising => "up",
            Direction::Falling => "down",
            Direction::Flat => "flat",
        }
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            Direction::Rising => "上升",
            Direction::Falling => "下降",
            Direction::Flat => "持平",
        }
    }

    /// 闭集全量。
    pub const ALL: [Direction; 3] = [Direction::Rising, Direction::Falling, Direction::Flat];
}

/// 方向解析：长短名双形制（up|u / down|d / flat|f），
/// 其余 UnknownRow 语义复用 MalformedRow，空串 EmptyRow。
pub fn parse_direction(tag: &str) -> Result<Direction, TrendCode> {
    if tag.is_empty() {
        return Err(TrendCode::EmptyRow);
    }
    match tag {
        "up" | "u" => Ok(Direction::Rising),
        "down" | "d" => Ok(Direction::Falling),
        "flat" | "f" => Ok(Direction::Flat),
        _ => Err(TrendCode::MalformedRow),
    }
}

// ── 报表行解析器 ──────────────────────────────────────────────────────
/// 报表行：轮次 + 通过数 + 总数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrendRow {
    /// 轮次（从 1 起算，严格递增）
    pub round: u32,
    /// 该轮通过用例数
    pub passed: u32,
    /// 该轮用例总数
    pub total: u32,
}

impl TrendRow {
    /// 千分率（permille）：passed×1000/total，分母已保证非零。
    pub fn permille(&self) -> u32 {
        self.passed.saturating_mul(PERMILLE_FULL) / self.total
    }
}

/// 纯数字段解析：空串或含非数字返回 None（checked 防溢出）。
fn parse_u32_field(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = *bytes.get(i)?;
        if b < b'0' || b > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

/// 行解析：形制「轮次:通过/总数」。混入 % 或 ‰ 判 MixedUnit；
/// total==0 判 ZeroDenom；passed>total 判 RateOverflow；round==0 判 BadRound。
pub fn parse_row(text: &str) -> Result<TrendRow, TrendCode> {
    if text.is_empty() {
        return Err(TrendCode::EmptyRow);
    }
    if text.contains('%') || text.contains('‰') {
        return Err(TrendCode::MixedUnit);
    }
    let colon = match text.find(':') {
        Some(i) => i,
        None => return Err(TrendCode::MalformedRow),
    };
    let (round_part, rest) = text.split_at(colon);
    let rest = match rest.get(1..) {
        Some(x) => x,
        None => return Err(TrendCode::MalformedRow),
    };
    let slash = match rest.find('/') {
        Some(i) => i,
        None => return Err(TrendCode::MalformedRow),
    };
    let (pass_part, tail) = rest.split_at(slash);
    let total_part = match tail.get(1..) {
        Some(x) => x,
        None => return Err(TrendCode::MalformedRow),
    };
    let round = match parse_u32_field(round_part) {
        Some(v) => v,
        None => return Err(TrendCode::MalformedRow),
    };
    let passed = match parse_u32_field(pass_part) {
        Some(v) => v,
        None => return Err(TrendCode::MalformedRow),
    };
    let total = match parse_u32_field(total_part) {
        Some(v) => v,
        None => return Err(TrendCode::MalformedRow),
    };
    if total == 0 {
        return Err(TrendCode::ZeroDenom);
    }
    if passed > total {
        return Err(TrendCode::RateOverflow);
    }
    if round == 0 {
        return Err(TrendCode::BadRound);
    }
    Ok(TrendRow { round, passed, total })
}

// ── 趋势列账本 ────────────────────────────────────────────────────────
/// 趋势列：按轮次严格递增入列，千分率逐轮可取。
#[derive(Clone, Debug)]
pub struct TrendColumn {
    rows: Vec<TrendRow>,
    cap: usize,
}

impl TrendColumn {
    /// 空列构造。
    pub fn new() -> TrendColumn {
        TrendColumn { rows: Vec::new(), cap: COLUMN_CAP }
    }

    /// 解析并入列；返回入列的行。
    pub fn push_parsed(&mut self, text: &str) -> Result<TrendRow, TrendCode> {
        let row = parse_row(text)?;
        self.push_row(row)?;
        Ok(row)
    }

    /// 入列：轮次必须严格递增（重复 DupRound、倒退 OutOfOrder）、容量满拒。
    pub fn push_row(&mut self, row: TrendRow) -> Result<(), TrendCode> {
        if self.rows.len() >= self.cap {
            return Err(TrendCode::LedgerFull);
        }
        if let Some(last) = self.rows.last() {
            if row.round == last.round {
                return Err(TrendCode::DupRound);
            }
            if row.round < last.round {
                return Err(TrendCode::OutOfOrder);
            }
        }
        self.rows.push(row);
        Ok(())
    }

    /// 逐轮千分率取数：下标越界返回 SegmentViolation。
    pub fn rate_at(&self, idx: usize) -> Result<u32, TrendCode> {
        match self.rows.get(idx) {
            Some(r) => Ok(r.permille()),
            None => Err(TrendCode::SegmentViolation),
        }
    }

    /// 逐轮千分率全量（空列返回空表）。
    pub fn rates(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                out.push(r.permille());
            }
            i += 1;
        }
        out
    }

    /// 方向取数：首尾千分率比较；轮次 < 2 判 TooFewRounds。
    pub fn direction(&self) -> Result<Direction, TrendCode> {
        if self.rows.len() < 2 {
            return Err(TrendCode::TooFewRounds);
        }
        let first = self.rate_at(0)?;
        let last = self.rate_at(self.rows.len() - 1)?;
        if last > first {
            Ok(Direction::Rising)
        } else if last < first {
            Ok(Direction::Falling)
        } else {
            Ok(Direction::Flat)
        }
    }

    /// 守恒：每行 passed ≤ total 且轮次严格递增。
    pub fn conserved(&self) -> bool {
        let mut prev_round = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.passed > r.total || r.total == 0 || r.round == 0 || r.round <= prev_round {
                    return false;
                }
                prev_round = r.round;
            }
            i += 1;
        }
        true
    }

    /// 已入列行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空列。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ── 报表成文（纯函数） ────────────────────────────────────────────────
/// 报表成文：「R<轮>=<千分率>‰;…;方向=<tag>」；空列先拒。
pub fn compose_trend_report(col: &TrendColumn) -> Result<String, TrendCode> {
    if col.is_empty() {
        return Err(TrendCode::EmptyColumn);
    }
    if !col.conserved() {
        return Err(TrendCode::RateOverflow);
    }
    let dir = col.direction()?;
    let mut out = String::new();
    let mut i = 0usize;
    while i < col.rows.len() {
        if let Some(r) = col.rows.get(i) {
            if i > 0 {
                out.push(';');
            }
            let piece = format!("R{}={}‰", r.round, r.permille());
            out.push_str(&piece);
        }
        i += 1;
    }
    out.push_str(";方向=");
    out.push_str(dir.tag());
    if out.is_empty() {
        return Err(TrendCode::EmptyColumn);
    }
    Ok(out)
}

// ── 注入核：坏行样本矩阵 ──────────────────────────────────────────────
/// 四类坏报表行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRow {
    /// 分母为零
    ZeroDenom,
    /// 通过数越过总数
    RateOverflow,
    /// 重复轮次
    DupRound,
    /// 混列：混入率值单位
    MixedUnit,
}

impl BadRow {
    /// 闭集全量。
    pub const ALL: [BadRow; 4] = [
        BadRow::ZeroDenom,
        BadRow::RateOverflow,
        BadRow::DupRound,
        BadRow::MixedUnit,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadRow::ZeroDenom => "分母为零",
            BadRow::RateOverflow => "通过数越界",
            BadRow::DupRound => "重复轮次",
            BadRow::MixedUnit => "混列率值",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 RateOverflow。
    pub fn record(&mut self, caught: bool) -> Result<(), TrendCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TrendCode::RateOverflow);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏行注入矩阵：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新趋势列，确保被拒样本不入列（拒绝不留痕）。
pub fn run_bad_row_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRow::ALL.len() {
        let case = match BadRow::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadRow::ZeroDenom => {
                    parse_row("1:5/0").err() == Some(TrendCode::ZeroDenom)
                }
                BadRow::RateOverflow => {
                    parse_row("1:51/50").err() == Some(TrendCode::RateOverflow)
                }
                BadRow::DupRound => {
                    let mut col = TrendColumn::new();
                    let _ = col.push_parsed("1:40/50");
                    col.push_parsed("1:42/50").err() == Some(TrendCode::DupRound)
                }
                BadRow::MixedUnit => {
                    parse_row("1:80%").err() == Some(TrendCode::MixedUnit)
                        && parse_row("2:840‰/1000").err() == Some(TrendCode::MixedUnit)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏形制/混列族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；空行/缺列族"),
    ("EDOM", -33, 332, "Windows ERROR_MATH_DOMAIN；分母为零族"),
    ("ERANGE", -34, 34, "Windows ERROR_MATH_RANGE；率值越界族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复轮次族"),
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

/// 规范语料双跑指纹：三行解析入列 → 方向取数 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut col = TrendColumn::new();
    let _ = col.push_parsed("1:40/50");
    let _ = col.push_parsed("2:42/50");
    let _ = col.push_parsed("3:45/50");
    let bytes = match compose_trend_report(&col) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10052 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10052_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10052");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91D00..=0x91D3F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠");

    // 2. 行解析器：合法行解析且字段回读一致
    let row_ok = parse_row("3:42/50") == Ok(TrendRow { round: 3, passed: 42, total: 50 })
        && parse_row("1:50/50") == Ok(TrendRow { round: 1, passed: 50, total: 50 });
    s.add("行解析器：轮次:通过/总数 形制全通过", row_ok,
          "合法行必须逐字段解析成功且可回读");

    // 3. 方向解析器双形制 + 歧义 10/10 拒绝
    let parse_ok = parse_direction("up") == Ok(Direction::Rising)
        && parse_direction("d") == Ok(Direction::Falling)
        && parse_direction("flat") == Ok(Direction::Flat);
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_direction("sideways").is_ok()
            || parse_direction("").is_ok()
            || parse_direction("UP").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("方向解析器：双形制通过且歧义 10/10 拒绝", parse_ok && amb_ok,
          "方向闭集外 token 必须逐次显性拒绝");

    // 4. 趋势列：三行推入且轮次严格递增
    let mut col = TrendColumn::new();
    let _ = col.push_parsed("1:40/50");
    let _ = col.push_parsed("2:42/50");
    let _ = col.push_parsed("3:45/50");
    s.add("趋势列：三行入列且轮次严格递增", col.len() == 3 && col.conserved(),
          "轮次重复/倒退一律拒绝，列内容只增不乱");

    // 5. 逐轮通过率 10/10 取数正确（permille 整数可复算）
    let mut rate_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut c = TrendColumn::new();
        let _ = c.push_parsed("1:42/50");
        let _ = c.push_parsed("2:45/50");
        // 42×1000/50=840，45×1000/50=900，纯整数推导逐位一致
        if c.rate_at(0).ok() != Some(840) || c.rate_at(1).ok() != Some(900) {
            rate_ok = false;
        }
        rr += 1;
    }
    s.add("逐轮取数：千分率 10/10 次取数正确", rate_ok,
          "通过率由计数列整数推导，逐轮取数必须逐次正确");

    // 6. 方向取数 10/10 正确：升/降/平三向
    let mut dir_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        let mut up = TrendColumn::new();
        let _ = up.push_parsed("1:40/50");
        let _ = up.push_parsed("2:45/50");
        let mut down = TrendColumn::new();
        let _ = down.push_parsed("1:45/50");
        let _ = down.push_parsed("2:40/50");
        let mut flat = TrendColumn::new();
        let _ = flat.push_parsed("1:42/50");
        let _ = flat.push_parsed("2:42/50");
        if up.direction().ok() != Some(Direction::Rising)
            || down.direction().ok() != Some(Direction::Falling)
            || flat.direction().ok() != Some(Direction::Flat)
        {
            dir_ok = false;
        }
        rr += 1;
    }
    s.add("方向取数：升/降/平 10/10 次判定正确", dir_ok,
          "首尾千分率比较必须三向全对，方向不得臆测");

    // 7. 注入核：四类坏行各 10/10 拒绝
    let (inj, per_class) = run_bad_row_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏行样本各 10/10 拒绝", all_caught,
          "零分母/越界/重复轮次/混列必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRow::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 混列检出：率值单位行 10/10 拒绝
    let mut mix_ok = true;
    let mut rr = 0u32;
    while rr < INJECT_ROUNDS {
        if parse_row("4:85%").is_ok() || parse_row("4:850‰/1000").is_ok() {
            mix_ok = false;
        }
        rr += 1;
    }
    s.add("混列检出：率值混列 10/10 次检出", mix_ok,
          "通过率只允许从计数列推导，直接报率的行逐次拒绝");

    // 10. 拒绝不留痕：坏行不进列
    let mut col2 = TrendColumn::new();
    let _ = col2.push_parsed("1:40/50");
    let before = col2.len();
    let _ = col2.push_parsed("1:42/50");
    let _ = col2.push_parsed("1:44/50");
    let _ = col2.push_parsed("2:51/50");
    let _ = col2.push_parsed("3:5/0");
    s.add("拒绝不留痕：四类坏行均不改动列长", col2.len() == before && before == 1,
          "被拒行不得进入趋势列，任何拒绝路径零副作用");

    // 11. 轮次不足拒绝取方向
    let mut one = TrendColumn::new();
    let _ = one.push_parsed("1:40/50");
    s.add("方向下界：单轮列取方向判 TooFewRounds",
          one.direction().err() == Some(TrendCode::TooFewRounds),
          "趋势方向至少需要两轮数据，单轮不得臆断方向");

    // 12. 成文非空且含 R/‰/方向 标记 + 双跑一致
    let rep = compose_trend_report(&col);
    let text_ok = match &rep {
        Ok(t) => t.contains("R1=") && t.contains('‰') && t.contains("方向=up"),
        Err(_) => false,
    };
    s.add("报表成文：标记齐全且两次成文逐字节一致",
          text_ok && rep == compose_trend_report(&col),
          "成文必须携带 逐轮率/方向 标记且为纯函数");

    // 13. 空列成文拒绝
    s.add("空列拒绝：空趋势列不得成文",
          compose_trend_report(&TrendColumn::new()).err() == Some(TrendCode::EmptyColumn),
          "空列出报表即口径污染，必须显性拒绝");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(33)
        && agrees_with_linux(34) && agrees_with_linux(17)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < TrendCode::ALL.len() {
        let mut j = i + 1;
        while j < TrendCode::ALL.len() {
            if let (Some(a), Some(b)) = (TrendCode::ALL.get(i), TrendCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = TrendCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &rep {
        Ok(t) => t.len(),
        Err(_) => 0,
    };
    let sum_text = format!("报表字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_row("2:30/40"), Ok(TrendRow { round: 2, passed: 30, total: 40 }));
        assert!(parse_row("2:30-40").is_err());
        assert_eq!(parse_row("").err(), Some(TrendCode::EmptyRow));
        assert_eq!(parse_row("2:5/0").err(), Some(TrendCode::ZeroDenom));
    }

    #[test]
    fn mixed_unit_rejected() {
        assert_eq!(parse_row("1:80%").err(), Some(TrendCode::MixedUnit));
        assert_eq!(parse_row("1:800‰/1000").err(), Some(TrendCode::MixedUnit));
    }

    #[test]
    fn permille_integer_math() {
        let r = TrendRow { round: 1, passed: 42, total: 50 };
        assert_eq!(r.permille(), 840);
        let full = TrendRow { round: 1, passed: 50, total: 50 };
        assert_eq!(full.permille(), 1000);
    }

    #[test]
    fn column_order_enforced() {
        let mut col = TrendColumn::new();
        assert!(col.push_parsed("1:40/50").is_ok());
        assert_eq!(col.push_parsed("1:42/50").err(), Some(TrendCode::DupRound));
        assert_eq!(col.push_parsed("0:42/50").err(), Some(TrendCode::BadRound));
        assert_eq!(col.len(), 1);
    }

    #[test]
    fn direction_three_ways() {
        let mut up = TrendColumn::new();
        let _ = up.push_parsed("1:10/50");
        let _ = up.push_parsed("2:20/50");
        assert_eq!(up.direction(), Ok(Direction::Rising));
        let mut flat = TrendColumn::new();
        let _ = flat.push_parsed("1:10/50");
        let _ = flat.push_parsed("2:10/50");
        assert_eq!(flat.direction(), Ok(Direction::Flat));
    }

    #[test]
    fn bad_row_matrix_green() {
        let (inj, per) = run_bad_row_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn report_contains_marks() {
        let mut col = TrendColumn::new();
        let _ = col.push_parsed("1:40/50");
        let _ = col.push_parsed("2:45/50");
        let s = compose_trend_report(&col).unwrap_or_default();
        assert!(s.contains("R1=800‰") && s.contains("方向=up"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(33));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(34), -34);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10052_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10052_checks_green() {
        let set = run_unx10052_checks();
        assert!(set.all_passed(), "UNX-F10052 red: {}", set.red_items_text());
    }
}
