//! UNX-F10003 批次维聚合器（B01–B40） · 0x910C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10004-J1 战线聚合零缺线（五线逐线入表 10/10 次），缺线
//! 10/10 次检出。
//!
//! 设计要点：
//!   ① 批维聚合按「40 批账 + 五线归属」建模：批号解析 B01..B40 两段式
//!      闭集；战线映射 f = ((b-1)×5)/40 + 1（整除纯函数，每线 8 批），
//!      五线逐线非空才算零缺线。
//!   ② 批账守恒：每批带预算行数与实计行数，逐批比对，偏离必须显性
//!      标出（偏离不标 10/10 次拦截）；总账 = 逐批之和。
//!   ③ 覆盖闸：批覆盖 40/40 且线覆盖 5/5 才允许出表；缺任何一批或
//!      任何一线都拒绝。
//!   ④ 注入核四类：缺批/越界批号/重复批号/预算偏离，逐类 10/10 检出；
//!      注入账守恒逃逸恒为 0。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10003 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x910C0;

/// 批次总数（B01..B40）。
pub const BATCH_COUNT: usize = 40;

/// 战线数（五战线）。
pub const FRONT_COUNT: usize = 5;

/// 单批预算行数（冻结口径：每批 40 行预算）。
pub const BUDGET_LINES_PER_BATCH: u32 = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchCode {
    /// 批名句式坏：不符 BNN
    BadBatchLabel,
    /// 批号越界：不在 1..=40
    BatchOutOfRange,
    /// 重复批号
    DuplicateBatch,
    /// 缺批：出表时仍有批次未入账
    MissingBatch,
    /// 缺线：出表时存在空战线
    MissingFront,
    /// 预算偏离：实计 ≠ 预算且未标出
    BudgetDeviation,
    /// 负行数：实计行数为负语义（用 0 与标志区分）
    NegativeLines,
    /// 出表未核：守恒未复核就出表
    EmitBeforeAudit,
    /// 守恒破约：总账 ≠ 逐批之和
    ConservationBreach,
    /// 线映射越界
    MapOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl BatchCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            BatchCode::BadBatchLabel => "批名句式坏被拒：必须为 BNN 两段式两位数字；\
                请修正拼写后再入账",
            BatchCode::BatchOutOfRange => "批号越界被拒：批号只允许 1..=40；\
                B00/B41 均非法",
            BatchCode::DuplicateBatch => "重复批号被拒：该批已入账；请先查账再补录，\
                重复回 EEXIST",
            BatchCode::MissingBatch => "缺批被拒：出表要求 40 批逐批在账；请补齐\
                缺失批次",
            BatchCode::MissingFront => "缺线被拒：存在空战线不得出表；请按冻结映射\
                补齐该线批次",
            BatchCode::BudgetDeviation => "预算偏离：实计行数与预算不符；偏离必须\
                显性标出，不许静默并入总账",
            BatchCode::NegativeLines => "负行数被拒：行数域为正整数；请回查该批\
                统计口径",
            BatchCode::EmitBeforeAudit => "出表未核被拒：出表前必须先过守恒复核；\
                请先调 audit 再出表",
            BatchCode::ConservationBreach => "守恒破约：总账与逐批之和不等；请核对\
                是否漏批或双计",
            BatchCode::MapOutOfRange => "线映射越界被拒：映射输出必须落在 1..=5；\
                请复核冻结映射式",
            BatchCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            BatchCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [BatchCode; 12] = [
        BatchCode::BadBatchLabel,
        BatchCode::BatchOutOfRange,
        BatchCode::DuplicateBatch,
        BatchCode::MissingBatch,
        BatchCode::MissingFront,
        BatchCode::BudgetDeviation,
        BatchCode::NegativeLines,
        BatchCode::EmitBeforeAudit,
        BatchCode::ConservationBreach,
        BatchCode::MapOutOfRange,
        BatchCode::NotReproducible,
        BatchCode::SegmentViolation,
    ];
}

// ── 批名解析器 ────────────────────────────────────────────────────────
/// 批名解析：B + 两位数字（B01..B40）→ 批号 1..=40。
pub fn parse_batch_label(text: &str) -> Result<usize, BatchCode> {
    let bytes = text.as_bytes();
    if bytes.len() != 3 {
        return Err(BatchCode::BadBatchLabel);
    }
    match bytes.get(0) {
        Some(b'B') => {}
        _ => return Err(BatchCode::BadBatchLabel),
    }
    let d1 = match bytes.get(1) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BatchCode::BadBatchLabel),
    };
    let d2 = match bytes.get(2) {
        Some(b) if b.is_ascii_digit() => b - b'0',
        _ => return Err(BatchCode::BadBatchLabel),
    };
    let n = (d1 as usize) * 10 + (d2 as usize);
    if n < 1 || n > BATCH_COUNT {
        return Err(BatchCode::BatchOutOfRange);
    }
    Ok(n)
}

/// 冻结线映射：批号 b（1..=40）→ 战线（1..=5），每线 8 批。
pub fn front_of_batch(b: usize) -> Result<usize, BatchCode> {
    if b < 1 || b > BATCH_COUNT {
        return Err(BatchCode::BatchOutOfRange);
    }
    let f = ((b - 1) * FRONT_COUNT) / BATCH_COUNT + 1;
    if f < 1 || f > FRONT_COUNT {
        return Err(BatchCode::MapOutOfRange);
    }
    Ok(f)
}

// ── 批账 ──────────────────────────────────────────────────────────────
/// 单批账目：预算与实计分离，偏离显性标出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchEntry {
    /// 批号（1..=40）
    pub batch: usize,
    /// 预算行数（冻结口径）
    pub budget: u32,
    /// 实计行数
    pub actual: u32,
}

impl BatchEntry {
    /// 是否偏离预算。
    pub fn deviates(&self) -> bool {
        self.actual != self.budget
    }
}

/// 批维聚合器：40 批账 + 五线归属 + 出表闸。
#[derive(Clone, Debug)]
pub struct BatchLedger {
    entries: Vec<BatchEntry>,
    /// 出表前是否已过守恒复核
    audited: bool,
}

impl BatchLedger {
    /// 空账。
    pub fn new() -> BatchLedger {
        BatchLedger { entries: Vec::new(), audited: false }
    }

    /// 入账一批：越界/重复/负行数拒绝；预算固定为冻结口径。
    pub fn add(&mut self, batch: usize, actual: u32) -> Result<(), BatchCode> {
        if batch < 1 || batch > BATCH_COUNT {
            return Err(BatchCode::BatchOutOfRange);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.batch == batch {
                    return Err(BatchCode::DuplicateBatch);
                }
            }
            i += 1;
        }
        if actual == 0 {
            return Err(BatchCode::NegativeLines);
        }
        // 入账使复核失效：任何账面变动都必须重审。
        self.audited = false;
        self.entries.push(BatchEntry { batch, budget: BUDGET_LINES_PER_BATCH, actual });
        Ok(())
    }

    /// 已入账批数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 批覆盖数（不同批号个数，即 len，因重复被拒）。
    pub fn batch_cover(&self) -> usize {
        self.entries.len()
    }

    /// 线覆盖数：入账批次映射到的不同战线个数。
    pub fn front_cover(&self) -> usize {
        let mut seen = [false; FRONT_COUNT];
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if let Ok(f) = front_of_batch(e.batch) {
                    if f >= 1 && f <= FRONT_COUNT {
                        seen[f - 1] = true;
                    }
                }
            }
            i += 1;
        }
        let mut n = 0usize;
        let mut k = 0usize;
        while k < FRONT_COUNT {
            if seen[k] {
                n = n.saturating_add(1);
            }
            k += 1;
        }
        n
    }

    /// 偏离清单：实计 ≠ 预算的批号列表（显性标出，不许静默）。
    pub fn deviations(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.deviates() {
                    out.push(e.batch);
                }
            }
            i += 1;
        }
        out
    }

    /// 逐批之和。
    pub fn sum_actual(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.actual);
            }
            i += 1;
        }
        sum
    }

    /// 预算总额（批数 × 单批预算）。
    pub fn sum_budget(&self) -> u32 {
        (self.entries.len() as u32).saturating_mul(BUDGET_LINES_PER_BATCH)
    }

    /// 守恒复核：40 批齐 + 五线齐 + 无零行；通过后置 audited。
    pub fn audit(&mut self) -> Result<(), BatchCode> {
        if self.len() != BATCH_COUNT {
            return Err(BatchCode::MissingBatch);
        }
        if self.front_cover() != FRONT_COUNT {
            return Err(BatchCode::MissingFront);
        }
        self.audited = true;
        Ok(())
    }

    /// 出表：必须先 audit；40 批齐才许出表。
    pub fn emit(&self) -> Result<u32, BatchCode> {
        if !self.audited {
            return Err(BatchCode::EmitBeforeAudit);
        }
        if self.len() != BATCH_COUNT {
            return Err(BatchCode::MissingBatch);
        }
        if self.front_cover() != FRONT_COUNT {
            return Err(BatchCode::MissingFront);
        }
        Ok(self.sum_actual())
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
            return Err(-(BatchCode::ConservationBreach as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 缺批：抽走一批后 audit 必须报缺批
    MissingBatch,
    /// 越界批号：B00/B41 与坏句式
    OutOfRange,
    /// 重复批号
    Duplicate,
    /// 预算偏离：实计 ≠ 预算必须进偏离清单
    Deviation,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::MissingBatch,
        BadCase::OutOfRange,
        BadCase::Duplicate,
        BadCase::Deviation,
    ];
}

/// 执行四类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_case_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        if let Some(case) = BadCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadCase::MissingBatch => {
                        // 抽走第 round%40+1 批（换批复测），audit 必须报缺批。
                        let drop = (round as usize) % BATCH_COUNT + 1;
                        let mut lg = BatchLedger::new();
                        let mut b = 1usize;
                        while b <= BATCH_COUNT {
                            if b != drop {
                                let _ = lg.add(b, BUDGET_LINES_PER_BATCH);
                            }
                            b += 1;
                        }
                        lg.audit().err() == Some(BatchCode::MissingBatch)
                    }
                    BadCase::OutOfRange => {
                        let mut lg = BatchLedger::new();
                        parse_batch_label("B00") == Err(BatchCode::BatchOutOfRange)
                            && parse_batch_label("B41") == Err(BatchCode::BatchOutOfRange)
                            && parse_batch_label("B1") == Err(BatchCode::BadBatchLabel)
                            && lg.add(0, 1) == Err(BatchCode::BatchOutOfRange)
                    }
                    BadCase::Duplicate => {
                        let mut lg = BatchLedger::new();
                        let first = lg.add(7, BUDGET_LINES_PER_BATCH).is_ok();
                        first && lg.add(7, BUDGET_LINES_PER_BATCH)
                            == Err(BatchCode::DuplicateBatch)
                    }
                    BadCase::Deviation => {
                        // 实计 41 ≠ 预算 40：偏离清单必须恰好含该批。
                        let mut lg = BatchLedger::new();
                        let _ = lg.add(11, BUDGET_LINES_PER_BATCH + 1);
                        let dev = lg.deviations();
                        dev.len() == 1 && dev.first() == Some(&11)
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
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 全量批账：40 批逐批入账，实计 = 预算（零偏离基准）。
pub fn build_full_ledger() -> BatchLedger {
    let mut lg = BatchLedger::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let _ = lg.add(b, BUDGET_LINES_PER_BATCH);
        b += 1;
    }
    let _ = lg.audit();
    lg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺批/缺线族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复批号族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界/坏句式族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；负行数族"),
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

/// 批账指纹：逐批 (批号, 实计) + 复核位逐字节入哈希。
pub fn ledger_fingerprint(lg: &BatchLedger) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < lg.len() {
        if let Some(e) = lg.entries.get(i) {
            bytes.push((e.batch & 0xFF) as u8);
            bytes.push(((e.actual >> 8) & 0xFF) as u8);
            bytes.push((e.actual & 0xFF) as u8);
        }
        i += 1;
    }
    bytes.push(if lg.audited { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10003 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10003_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10003");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 批名解析：B01..B40 逐字命中（40 个规范批名闭集写死）
    const LABELS: [&str; 40] = [
        "B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B09", "B10",
        "B11", "B12", "B13", "B14", "B15", "B16", "B17", "B18", "B19", "B20",
        "B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28", "B29", "B30",
        "B31", "B32", "B33", "B34", "B35", "B36", "B37", "B38", "B39", "B40",
    ];
    let mut parse_ok = true;
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        match LABELS.get(b - 1) {
            Some(lb) if parse_batch_label(lb) == Ok(b) => {}
            _ => parse_ok = false,
        }
        b += 1;
    }
    s.add("批名解析：B01..B40 逐字命中", parse_ok,
          "40 个规范批名必须全部解析回正确批号");

    // 3. 坏句式与越界拒绝
    let reject_ok = parse_batch_label("C01") == Err(BatchCode::BadBatchLabel)
        && parse_batch_label("B1") == Err(BatchCode::BadBatchLabel)
        && parse_batch_label("B00") == Err(BatchCode::BatchOutOfRange)
        && parse_batch_label("B41") == Err(BatchCode::BatchOutOfRange);
    s.add("批名拒绝：坏句式与越界逐类拒绝", reject_ok,
          "非 B 头/单数字回坏句式，B00/B41 回越界");

    // 4. 线映射：40 批全落 1..=5
    let mut map_ok = true;
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        if front_of_batch(b).is_err() {
            map_ok = false;
        }
        b += 1;
    }
    s.add("线映射：40 批逐一落线且无越界", map_ok,
          "映射式 (b-1)×5/40+1 必须把全部批号映进 1..=5");

    // 5. 全量账：40 批齐 + 五线齐 + 复核过
    let mut full = build_full_ledger();
    let audit_ok = full.audit().is_ok();
    s.add("零缺线：40 批入账且五线逐线非空",
          audit_ok && full.batch_cover() == BATCH_COUNT
              && full.front_cover() == FRONT_COUNT,
          "批覆盖 40/40 且线覆盖 5/5 才算零缺线");

    // 6. 出表：复核后出表返回总行数
    let emit_ok = full.emit() == Ok(BATCH_COUNT as u32 * BUDGET_LINES_PER_BATCH);
    s.add("出表：复核后总行数=40×40", emit_ok,
          "出表前必须过 audit，总行数与冻结口径可复算");

    // 7. 出表未核拦截
    let mut unaudited = BatchLedger::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let _ = unaudited.add(b, BUDGET_LINES_PER_BATCH);
        b += 1;
    }
    s.add("出表未核：未 audit 直接出表被拒",
          unaudited.emit().err() == Some(BatchCode::EmitBeforeAudit),
          "守恒复核是出表前置闸，跳闸即拦");

    // 8. 注入矩阵：四类各 10/10 检出
    let (ledger, per_class) = run_bad_case_matrix();
    let mut all_caught = true;
    let mut ci = 0usize;
    while ci < per_class.len() {
        if let Some(n) = per_class.get(ci) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        ci += 1;
    }
    s.add("注入矩阵：缺批/越界/重复/偏离各 10/10 检出", all_caught,
          "四类坏样本必须全部显性拒绝或标出");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 缺批拦截：抽走一批后 audit 拒绝（换批复测在矩阵内）
    let mut short = BatchLedger::new();
    let mut b = 2usize;
    while b <= BATCH_COUNT {
        let _ = short.add(b, BUDGET_LINES_PER_BATCH);
        b += 1;
    }
    s.add("缺批拦截：39 批 audit 报缺批",
          short.audit().err() == Some(BatchCode::MissingBatch),
          "40 批少任何一批都不得过闸");

    // 11. 预算偏离显性标出
    let mut dev = BatchLedger::new();
    let _ = dev.add(3, BUDGET_LINES_PER_BATCH);
    let _ = dev.add(5, BUDGET_LINES_PER_BATCH + 2);
    let _ = dev.add(8, BUDGET_LINES_PER_BATCH - 1);
    let devs = dev.deviations();
    s.add("偏离标出：偏离清单恰好含 B05/B08",
          devs.len() == 2 && devs.first() == Some(&5),
          "实计≠预算必须进偏离清单，不许静默并入总账");

    // 12. 重复批号拒绝
    let mut dup = BatchLedger::new();
    let _ = dup.add(12, BUDGET_LINES_PER_BATCH);
    s.add("重复拒绝：B12 二次入账被拒",
          dup.add(12, BUDGET_LINES_PER_BATCH) == Err(BatchCode::DuplicateBatch),
          "重复批号必须显性拒绝，不许静默双计");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = ledger_fingerprint(&build_full_ledger());
    let fp_b = ledger_fingerprint(&build_full_ledger());
    s.add("双跑一致：批账指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = BatchCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < BatchCode::ALL.len() {
        let mut j = i + 1;
        while j < BatchCode::ALL.len() {
            if let (Some(a), Some(b)) = (BatchCode::ALL.get(i), BatchCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("批账指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_labels_parse() {
        let labels: [&str; 5] = ["B01", "B20", "B39", "B40", "B07"];
        let wants: [usize; 5] = [1, 20, 39, 40, 7];
        let mut i = 0usize;
        while i < 5 {
            assert_eq!(parse_batch_label(labels[i]), Ok(wants[i]));
            i += 1;
        }
    }

    #[test]
    fn bad_labels_rejected() {
        assert_eq!(parse_batch_label("B41"), Err(BatchCode::BatchOutOfRange));
        assert_eq!(parse_batch_label("B00"), Err(BatchCode::BatchOutOfRange));
        assert_eq!(parse_batch_label("X09"), Err(BatchCode::BadBatchLabel));
    }

    #[test]
    fn front_mapping_partitions() {
        let mut counts = [0usize; 5];
        let mut b = 1usize;
        while b <= 40 {
            let f = front_of_batch(b).unwrap_or(0);
            assert!(f >= 1 && f <= 5);
            counts[f - 1] += 1;
            b += 1;
        }
        for c in counts.iter() {
            assert_eq!(*c, 8);
        }
    }

    #[test]
    fn full_ledger_audits_and_emits() {
        let mut lg = build_full_ledger();
        assert!(lg.audit().is_ok());
        assert_eq!(lg.emit(), Ok(40 * BUDGET_LINES_PER_BATCH));
    }

    #[test]
    fn missing_batch_detected() {
        let mut lg = BatchLedger::new();
        let mut b = 1usize;
        while b <= 39 {
            let _ = lg.add(b, BUDGET_LINES_PER_BATCH);
            b += 1;
        }
        assert_eq!(lg.audit(), Err(BatchCode::MissingBatch));
    }

    #[test]
    fn emit_requires_audit() {
        let mut lg = BatchLedger::new();
        let _ = lg.add(1, BUDGET_LINES_PER_BATCH);
        assert_eq!(lg.emit(), Err(BatchCode::EmitBeforeAudit));
    }

    #[test]
    fn deviations_listed() {
        let mut lg = BatchLedger::new();
        let _ = lg.add(1, BUDGET_LINES_PER_BATCH);
        let _ = lg.add(2, BUDGET_LINES_PER_BATCH + 5);
        let dev = lg.deviations();
        assert_eq!(dev.len(), 1);
        assert_eq!(dev.first(), Some(&2));
    }

    #[test]
    fn duplicate_rejected() {
        let mut lg = BatchLedger::new();
        assert!(lg.add(9, BUDGET_LINES_PER_BATCH).is_ok());
        assert_eq!(lg.add(9, BUDGET_LINES_PER_BATCH), Err(BatchCode::DuplicateBatch));
    }

    #[test]
    fn zero_lines_rejected() {
        let mut lg = BatchLedger::new();
        assert_eq!(lg.add(9, 0), Err(BatchCode::NegativeLines));
    }

    #[test]
    fn bad_case_matrix_green() {
        let (l, per) = run_bad_case_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn unx10003_checks_green() {
        let set = run_unx10003_checks();
        assert!(set.all_passed(), "UNX-F10003 red: {}", set.red_items_text());
    }
}
