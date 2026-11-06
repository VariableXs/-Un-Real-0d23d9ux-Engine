//! UNX-F10009 行数守恒列（预算↔实计） · 0x91240段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10010-J1 快照并入零漂移（版本锚核对 10/10 次），私改快照
//! 10/10 次拦截。
//!
//! 设计要点：
//!   ① 行数守恒按「40 批双列账」建模：每批带预算行数（冻结口径 40 行）
//!      与实计行数两列；偏离（实计 ≠ 预算）必须进偏离清单显性标出，
//!      「偏离不标」由注入核逐轮拦截（10/10）。
//!   ② 守恒总式：Σ实计 = Σ预算（零偏离基准）；总式不符时必须能定位
//!      到批（偏离清单非空），不许笼统报错。
//!   ③ 快照版本锚：预算面哈希（FNV-1a）作为快照锚，并入（merge）时
//!      核对锚与版本号；锚不符（私改快照）与版本回退一律拒绝，
//!      版本号只增不减。
//!   ④ 坏样本闸：偏离不标/越界批号/重复批号/私改快照四类，逐类
//!      10/10 检出；注入账守恒逃逸恒为 0。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；指纹双跑逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10009 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91240;

/// 批数（40 批）。
pub const BATCH_COUNT: usize = 40;

/// 单批预算行数（冻结口径：每批 40 行）。
pub const BUDGET_PER_BATCH: u32 = 40;

/// 初始快照版本号（版本链起点）。
pub const SNAPSHOT_VERSION_INIT: u32 = 1;

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
pub enum ConsCode {
    /// 批号越界：不在 1..=40
    BatchOutOfRange,
    /// 重复批号
    DuplicateBatch,
    /// 零行数：实计为 0 属非法行账
    ZeroLines,
    /// 偏离不标：守恒破约但偏离清单为空
    UnflaggedDeviation,
    /// 守恒破约：Σ实计 ≠ Σ预算
    ConservationBreach,
    /// 账不齐：出表时批数不足 40
    BatchShort,
    /// 快照锚不符：并入哈希与账面预算面不符
    AnchorMismatch,
    /// 版本回退：并入版本号 ≤ 已有版本
    VersionRegress,
    /// 未并入出表：出表前未过快照并入
    EmitBeforeMerge,
    /// 预算漂移：批预算 ≠ 冻结口径 40
    BudgetDrift,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ConsCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ConsCode::BatchOutOfRange => "批号越界被拒：批号只允许 1..=40；请按\
                B01..B40 编号入账",
            ConsCode::DuplicateBatch => "重复批号被拒：该批已入账；请先查账再补录，\
                重复回 EEXIST",
            ConsCode::ZeroLines => "零行数被拒：实计行数必须大于 0；空批不属于\
                守恒列，请回查该批账目",
            ConsCode::UnflaggedDeviation => "偏离不标被拒：守恒破约时偏离清单\
                必须非空；静默并入按拦截处理",
            ConsCode::ConservationBreach => "守恒破约：实计合计与预算合计不等；\
                请按偏离清单逐批定位",
            ConsCode::BatchShort => "账不齐被拒：出表要求 40 批逐批在账；请补齐\
                缺失批次",
            ConsCode::AnchorMismatch => "快照锚不符被拒：并入哈希与账面预算面\
                不一致；快照疑似被私改，拒绝并入",
            ConsCode::VersionRegress => "版本回退被拒：并入版本号不大于已有版本；\
                版本链只增不减",
            ConsCode::EmitBeforeMerge => "未并入出表被拒：出表前必须过快照并入；\
                请先 merge 冻结预算面",
            ConsCode::BudgetDrift => "预算漂移被拒：批预算不等于冻结口径 40 行；\
                预算是守恒基准，不许漂移",
            ConsCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            ConsCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [ConsCode; 12] = [
        ConsCode::BatchOutOfRange,
        ConsCode::DuplicateBatch,
        ConsCode::ZeroLines,
        ConsCode::UnflaggedDeviation,
        ConsCode::ConservationBreach,
        ConsCode::BatchShort,
        ConsCode::AnchorMismatch,
        ConsCode::VersionRegress,
        ConsCode::EmitBeforeMerge,
        ConsCode::BudgetDrift,
        ConsCode::NotReproducible,
        ConsCode::SegmentViolation,
    ];
}

// ── 双列账 ────────────────────────────────────────────────────────────
/// 单批双列：预算与实计分离，偏离显性标出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchLines {
    /// 批号（1..=40）
    pub batch: usize,
    /// 预算行数（冻结口径）
    pub budget: u32,
    /// 实计行数
    pub actual: u32,
}

impl BatchLines {
    /// 是否偏离预算。
    pub fn deviates(&self) -> bool {
        self.actual != self.budget
    }
}

/// 行数守恒账：40 批双列 + 快照版本锚 + 出表闸。
#[derive(Clone, Debug)]
pub struct ConservationLedger {
    entries: Vec<BatchLines>,
    /// 快照版本链头（0 = 尚未并入）
    version: u32,
    merged: bool,
}

impl ConservationLedger {
    /// 空账。
    pub fn new() -> ConservationLedger {
        ConservationLedger { entries: Vec::new(), version: 0, merged: false }
    }

    /// 入账一批：越界/重复/零行数/预算漂移逐项拒绝。
    pub fn add(&mut self, batch: usize, actual: u32) -> Result<(), ConsCode> {
        if batch < 1 || batch > BATCH_COUNT {
            return Err(ConsCode::BatchOutOfRange);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.batch == batch {
                    return Err(ConsCode::DuplicateBatch);
                }
            }
            i += 1;
        }
        if actual == 0 {
            return Err(ConsCode::ZeroLines);
        }
        // 预算恒取冻结口径；这里防止未来有人把它改成可变参数。
        if BUDGET_PER_BATCH != 40 {
            return Err(ConsCode::BudgetDrift);
        }
        self.merged = false;
        self.entries.push(BatchLines { batch, budget: BUDGET_PER_BATCH, actual });
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

    /// Σ预算。
    pub fn sum_budget(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                s = s.saturating_add(e.budget);
            }
            i += 1;
        }
        s
    }

    /// Σ实计。
    pub fn sum_actual(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                s = s.saturating_add(e.actual);
            }
            i += 1;
        }
        s
    }

    /// 偏离清单：实计 ≠ 预算的批号（升序即入账序）。
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

    /// 预算面快照哈希（FNV-1a over (批号, 预算) 字节）。
    pub fn budget_anchor(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                bytes.push((e.batch & 0xFF) as u8);
                bytes.push(((e.budget >> 8) & 0xFF) as u8);
                bytes.push((e.budget & 0xFF) as u8);
            }
            i += 1;
        }
        fnv1a(&bytes)
    }

    /// 当前版本号（0 = 未并入）。
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 是否已并入快照。
    pub fn is_merged(&self) -> bool {
        self.merged
    }

    /// 快照并入：版本只增 + 锚核对（10/10 私改拦截的唯一闸口）。
    pub fn merge_snapshot(&mut self, version: u32, anchor: u32) -> Result<(), ConsCode> {
        if self.len() != BATCH_COUNT {
            return Err(ConsCode::BatchShort);
        }
        if version <= self.version {
            return Err(ConsCode::VersionRegress);
        }
        if anchor != self.budget_anchor() {
            return Err(ConsCode::AnchorMismatch);
        }
        self.version = version;
        self.merged = true;
        Ok(())
    }

    /// 守恒复核：零偏离时 Σ实计 = Σ预算；破约时偏离清单必须非空
    /// （偏离不标在此拦截）。
    pub fn conservation(&self) -> Result<(u32, u32), ConsCode> {
        let sb = self.sum_budget();
        let sa = self.sum_actual();
        if sa != sb {
            if self.deviations().is_empty() {
                return Err(ConsCode::UnflaggedDeviation);
            }
            return Err(ConsCode::ConservationBreach);
        }
        Ok((sb, sa))
    }

    /// 出表：40 批齐 + 已并入 + 守恒过；返回 (Σ预算, Σ实计)。
    pub fn emit(&self) -> Result<(u32, u32), ConsCode> {
        if self.len() != BATCH_COUNT {
            return Err(ConsCode::BatchShort);
        }
        if !self.merged {
            return Err(ConsCode::EmitBeforeMerge);
        }
        self.conservation()
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
            return Err(-(ConsCode::ConservationBreach as i32));
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
    /// 偏离不标：破约但偏离清单必须非空
    Unflagged,
    /// 越界批号
    OutOfRange,
    /// 重复批号
    Duplicate,
    /// 私改快照：锚不符与版本回退轮换
    TamperedSnapshot,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::Unflagged,
        BadCase::OutOfRange,
        BadCase::Duplicate,
        BadCase::TamperedSnapshot,
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
                    // 偏离不标：抽走一批实计+1 造成破约，偏离清单必须点名该批。
                    BadCase::Unflagged => {
                        let dev_batch = round as usize % BATCH_COUNT + 1;
                        let mut lg = ConservationLedger::new();
                        let mut b = 1usize;
                        while b <= BATCH_COUNT {
                            let lines = if b == dev_batch {
                                BUDGET_PER_BATCH + 1
                            } else {
                                BUDGET_PER_BATCH
                            };
                            let _ = lg.add(b, lines);
                            b += 1;
                        }
                        let dev = lg.deviations();
                        lg.conservation().is_err() && dev.len() == 1
                            && dev.first() == Some(&dev_batch)
                    }
                    BadCase::OutOfRange => {
                        let mut lg = ConservationLedger::new();
                        lg.add(0, BUDGET_PER_BATCH) == Err(ConsCode::BatchOutOfRange)
                            && lg.add(BATCH_COUNT + 1, BUDGET_PER_BATCH)
                                == Err(ConsCode::BatchOutOfRange)
                    }
                    BadCase::Duplicate => {
                        let mut lg = ConservationLedger::new();
                        let first = lg.add(9, BUDGET_PER_BATCH).is_ok();
                        first && lg.add(9, BUDGET_PER_BATCH)
                            == Err(ConsCode::DuplicateBatch)
                    }
                    BadCase::TamperedSnapshot => {
                        let mut lg = ConservationLedger::new();
                        let mut b = 1usize;
                        while b <= BATCH_COUNT {
                            let _ = lg.add(b, BUDGET_PER_BATCH);
                            b += 1;
                        }
                        let true_anchor = lg.budget_anchor();
                        // 私改锚与版本回退两种手法轮换。
                        if round % 2 == 0 {
                            lg.merge_snapshot(SNAPSHOT_VERSION_INIT + 1, true_anchor ^ 0xDEAD)
                                == Err(ConsCode::AnchorMismatch)
                        } else {
                            // 先正常并入 v2，再注入 v1 回退。
                            let _ = lg.merge_snapshot(SNAPSHOT_VERSION_INIT + 1, true_anchor);
                            lg.merge_snapshot(SNAPSHOT_VERSION_INIT, true_anchor)
                                == Err(ConsCode::VersionRegress)
                        }
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

/// 零偏离基准账：40 批逐批入账（实计 = 预算），并入 v2 快照。
pub fn build_zero_deviation_ledger() -> ConservationLedger {
    let mut lg = ConservationLedger::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let _ = lg.add(b, BUDGET_PER_BATCH);
        b += 1;
    }
    let anchor = lg.budget_anchor();
    let _ = lg.merge_snapshot(SNAPSHOT_VERSION_INIT + 1, anchor);
    lg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；批缺员/未并入族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复批号族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界批/私改锚族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；版本回退/守恒破约族"),
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

/// 守恒账指纹：逐批 (批号, 预算, 实计) + 版本 + 并入位入哈希。
pub fn ledger_fingerprint(lg: &ConservationLedger) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < lg.len() {
        if let Some(e) = lg.entries.get(i) {
            bytes.push((e.batch & 0xFF) as u8);
            bytes.push((e.budget & 0xFF) as u8);
            bytes.push(((e.actual >> 8) & 0xFF) as u8);
            bytes.push((e.actual & 0xFF) as u8);
        }
        i += 1;
    }
    bytes.push(((lg.version >> 8) & 0xFF) as u8);
    bytes.push((lg.version & 0xFF) as u8);
    bytes.push(if lg.merged { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10009 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10009_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10009");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 零偏离基准：40 批 Σ实计 = Σ预算 = 1600
    let lg = build_zero_deviation_ledger();
    let cons = lg.conservation();
    s.add("守恒零偏离：Σ实计=Σ预算=40×40",
          cons == Ok((BATCH_COUNT as u32 * BUDGET_PER_BATCH,
                      BATCH_COUNT as u32 * BUDGET_PER_BATCH)),
          "零偏离基准账必须双列逐位相等且可复算");

    // 3. 快照并入零漂移：v2 并入成功、版本链头可查
    s.add("快照并入：v2 锚核通过且版本落账",
          lg.is_merged() && lg.version() == SNAPSHOT_VERSION_INIT + 1,
          "锚核通过才许并入，版本链头必须可查");

    // 4. 出表：并入后双列成组返回
    let emit = lg.emit();
    s.add("出表：双列 (1600,1600) 成组返回",
          emit == Ok((BATCH_COUNT as u32 * BUDGET_PER_BATCH,
                      BATCH_COUNT as u32 * BUDGET_PER_BATCH)),
          "出表必须一次给出双列，不许只报单列");

    // 5. 注入矩阵：四类各 10/10 检出
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
    s.add("注入矩阵：偏离不标/越界/重复/私改快照各 10/10", all_caught,
          "四类坏样本必须全部显性拒绝或定位");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 偏离定位：单批实计+1 时偏离清单点名该批
    let mut dev_lg = ConservationLedger::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let lines = if b == 17 { BUDGET_PER_BATCH + 1 } else { BUDGET_PER_BATCH };
        let _ = dev_lg.add(b, lines);
        b += 1;
    }
    let dev = dev_lg.deviations();
    s.add("偏离定位：B17 实计+1 被点名且守恒破约",
          dev.len() == 1 && dev.first() == Some(&17)
              && dev_lg.conservation().err() == Some(ConsCode::ConservationBreach),
          "破约必须能定位到批，不许笼统报错");

    // 8. 私改快照拦截：错锚拒绝且账面未被污染
    let mut tamper = build_zero_deviation_ledger_v1();
    let anchor = tamper.budget_anchor();
    let merge_bad = tamper.merge_snapshot(SNAPSHOT_VERSION_INIT + 1, anchor ^ 0x1);
    s.add("私改拦截：错锚拒绝且版本未推进",
          merge_bad.err() == Some(ConsCode::AnchorMismatch)
              && tamper.version() == 0
              && !tamper.is_merged(),
          "并入被拒时账面必须保持未并入态，不许半并入");

    // 9. 版本回退拦截
    let mut vl = build_zero_deviation_ledger();
    s.add("版本回退：v1 并入 v2 后再并 v1 被拒",
          vl.merge_snapshot(SNAPSHOT_VERSION_INIT, vl.budget_anchor())
              == Err(ConsCode::VersionRegress),
          "版本链只增不减，回退一律拒绝");

    // 10. 未并入出表拦截
    let mut unmerged = ConservationLedger::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let _ = unmerged.add(b, BUDGET_PER_BATCH);
        b += 1;
    }
    s.add("未并入出表：40 批但未 merge 被拒",
          unmerged.emit().err() == Some(ConsCode::EmitBeforeMerge),
          "快照并入是出表前置闸，跳闸即拦");

    // 11. 越界与重复批号拒绝
    let mut rng = ConservationLedger::new();
    let _ = rng.add(12, BUDGET_PER_BATCH);
    s.add("批号闸：B00/B41 越界、B12 重复均拒",
          rng.add(0, BUDGET_PER_BATCH) == Err(ConsCode::BatchOutOfRange)
              && rng.add(BATCH_COUNT + 1, BUDGET_PER_BATCH)
                  == Err(ConsCode::BatchOutOfRange)
              && rng.add(12, BUDGET_PER_BATCH) == Err(ConsCode::DuplicateBatch),
          "批号域与唯一性是守恒列的入账前提");

    // 12. 零行数拒绝
    let mut z = ConservationLedger::new();
    s.add("零行数：实计 0 拒绝",
          z.add(3, 0).err() == Some(ConsCode::ZeroLines),
          "空批不属于守恒列，必须显性拒绝");

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
    let fp_a = ledger_fingerprint(&build_zero_deviation_ledger());
    let fp_b = ledger_fingerprint(&build_zero_deviation_ledger());
    s.add("双跑一致：守恒账指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = ConsCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ConsCode::ALL.len() {
        let mut j = i + 1;
        while j < ConsCode::ALL.len() {
            if let (Some(a), Some(b)) = (ConsCode::ALL.get(i), ConsCode::ALL.get(j)) {
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
    let fp_text = format!("守恒账指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

/// v1 态账（40 批齐但未并入）：供私改拦截判据使用。
pub fn build_zero_deviation_ledger_v1() -> ConservationLedger {
    let mut lg = ConservationLedger::new();
    let mut b = 1usize;
    while b <= BATCH_COUNT {
        let _ = lg.add(b, BUDGET_PER_BATCH);
        b += 1;
    }
    lg
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_deviation_conserves() {
        let lg = build_zero_deviation_ledger();
        assert_eq!(lg.sum_budget(), 1600);
        assert_eq!(lg.sum_actual(), 1600);
        assert!(lg.deviations().is_empty());
    }

    #[test]
    fn deviation_localized() {
        let mut lg = ConservationLedger::new();
        let mut b = 1usize;
        while b <= 40 {
            let lines = if b == 5 { BUDGET_PER_BATCH - 2 } else { BUDGET_PER_BATCH };
            let _ = lg.add(b, lines);
            b += 1;
        }
        let dev = lg.deviations();
        assert_eq!(dev.len(), 1);
        assert_eq!(dev.first(), Some(&5));
        assert_eq!(lg.conservation(), Err(ConsCode::ConservationBreach));
    }

    #[test]
    fn merge_gates() {
        let mut lg = build_zero_deviation_ledger_v1();
        assert_eq!(lg.version(), 0);
        assert!(lg.merge_snapshot(SNAPSHOT_VERSION_INIT + 1, lg.budget_anchor()).is_ok());
        assert_eq!(lg.version(), SNAPSHOT_VERSION_INIT + 1);
    }

    #[test]
    fn anchor_mismatch_rejected() {
        let mut lg = build_zero_deviation_ledger_v1();
        assert_eq!(
            lg.merge_snapshot(2, lg.budget_anchor() ^ 0xFFFF),
            Err(ConsCode::AnchorMismatch)
        );
        assert_eq!(lg.version(), 0);
    }

    #[test]
    fn version_regress_rejected() {
        let mut lg = build_zero_deviation_ledger();
        assert_eq!(
            lg.merge_snapshot(1, lg.budget_anchor()),
            Err(ConsCode::VersionRegress)
        );
    }

    #[test]
    fn emit_gates() {
        let mut lg = build_zero_deviation_ledger_v1();
        assert_eq!(lg.emit(), Err(ConsCode::EmitBeforeMerge));
    }

    #[test]
    fn out_of_range_and_duplicate_rejected() {
        let mut lg = ConservationLedger::new();
        assert_eq!(lg.add(41, BUDGET_PER_BATCH), Err(ConsCode::BatchOutOfRange));
        assert!(lg.add(7, BUDGET_PER_BATCH).is_ok());
        assert_eq!(lg.add(7, BUDGET_PER_BATCH), Err(ConsCode::DuplicateBatch));
    }

    #[test]
    fn zero_lines_rejected() {
        let mut lg = ConservationLedger::new();
        assert_eq!(lg.add(1, 0), Err(ConsCode::ZeroLines));
    }

    #[test]
    fn bad_case_matrix_green() {
        let (l, per) = run_bad_case_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(
            ledger_fingerprint(&build_zero_deviation_ledger()),
            ledger_fingerprint(&build_zero_deviation_ledger())
        );
    }

    #[test]
    fn unx10009_checks_green() {
        let set = run_unx10009_checks();
        assert!(set.all_passed(), "UNX-F10009 red: {}", set.red_items_text());
    }
}
