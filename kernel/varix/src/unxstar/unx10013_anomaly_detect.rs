//! UNX-F10013 报表异常检出（陡降/回退） · 0x91340 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10013（B21 波次报表线）。
//! 锚点：UNX-F10014-J1 导出零失真（格值↔源值抽核 10/10 次一致），失真 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91340（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `AnomalyCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `SeriesParser`：把「期号|值」文本拆成 `SampleRec`，期号
//!     回退与非数值一律拒绝；
//!   - 注册表 `ThresholdRegistry`：陡降阈值（knee=基线×0.60，出处冻结
//!     于 LTP 通过率验收线）逐条登记，无出处不得注册；
//!   - 检出器 `DropDetector`：陡降（值 < knee）与回退（环比下降）双判
//!     定，漏警由注入核反向验证；
//!   - 账本 `AlertLedger`：应警 = 触发 + 漏警，漏警恒 0，只增不减；
//!   - 注入核 `BadAnomalyCase`：无出处阈值 / 期号回退 / 阈值漂移 /
//!     掩蔽陡降 / 空序列五类坏样本逐类 10/10 次检出；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10013 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91340;

/// 陡降判定比例（knee = 基线 × 60%，推导：LTP 验收线 6/10=0.60）。
pub const KNEE_NUM: u32 = 60;
/// 陡降判定分母。
pub const KNEE_DEN: u32 = 100;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnomalyCode {
    /// 无出处阈值
    ThresholdNoSource,
    /// 期号回退
    PeriodRegress,
    /// 阈值漂移（注册后改值）
    ThresholdDrift,
    /// 掩蔽陡降（漏警）
    MaskedDrop,
    /// 空序列
    EmptySeries,
    /// 账本倒退
    LedgerRegress,
    /// 非数值样本
    NonNumeric,
    /// 阈值重复注册
    DuplicateThreshold,
    /// 阈值越域（>100%）
    ThresholdOutOfRange,
    /// 回退误报（平稳序列被报警）
    FalseAlarm,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl AnomalyCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            AnomalyCode::ThresholdNoSource => "无出处阈值被拒：每条阈值必须注出处；\
                请补写推导来源（如 LTP 验收线 6/10）再注册",
            AnomalyCode::PeriodRegress => "期号回退被拒：期号必须单调递增；请按期\
                序重放样本流",
            AnomalyCode::ThresholdDrift => "阈值漂移被拒：注册后阈值只读；如需调整\
                请走阈值变更通道留痕",
            AnomalyCode::MaskedDrop => "掩蔽陡降检出：样本低于 knee 却未触发告警；\
                请检查检出器是否被旁路",
            AnomalyCode::EmptySeries => "空序列被拒：至少需要 1 个样本；请补样本后\
                重测",
            AnomalyCode::LedgerRegress => "告警账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            AnomalyCode::NonNumeric => "非数值样本被拒：值段必须为十进制整数；请\
                修正后重报",
            AnomalyCode::DuplicateThreshold => "阈值重复注册被拒：同名阈值只能一条；\
                请先去重",
            AnomalyCode::ThresholdOutOfRange => "阈值越域被拒：比例阈值必须落在 \
                0..=100%；请复核分子分母",
            AnomalyCode::FalseAlarm => "回退误报检出：平稳序列被报警；请检查比较\
                方向是否写反",
            AnomalyCode::NotReproducible => "双跑不一致：同输入两次结果不同；请\
                排查隐藏状态或非确定哈希",
            AnomalyCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91340..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [AnomalyCode; 12] = [
        AnomalyCode::ThresholdNoSource,
        AnomalyCode::PeriodRegress,
        AnomalyCode::ThresholdDrift,
        AnomalyCode::MaskedDrop,
        AnomalyCode::EmptySeries,
        AnomalyCode::LedgerRegress,
        AnomalyCode::NonNumeric,
        AnomalyCode::DuplicateThreshold,
        AnomalyCode::ThresholdOutOfRange,
        AnomalyCode::FalseAlarm,
        AnomalyCode::NotReproducible,
        AnomalyCode::SegmentViolation,
    ];
}

// ── 样本与解析器 ──────────────────────────────────────────────────────
/// 单个样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRec {
    /// 期号（单调递增）
    pub period: u32,
    /// 值（通过数/行数等整数）
    pub value: u32,
}

/// 样本文本解析器：「期号|值」两段，期号回退显性拒绝。
#[derive(Default)]
pub struct SeriesParser {
    /// 已解析样本
    pub samples: Vec<SampleRec>,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl SeriesParser {
    /// 解析并按期序追加；期号不大于前值即 PeriodRegress。
    pub fn parse(&mut self, text: &str) -> Result<SampleRec, AnomalyCode> {
        let parts: Vec<&str> = text.split('|').collect();
        if parts.len() != 2 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AnomalyCode::EmptySeries);
        }
        let p = parse_dec(parts.get(0).copied().unwrap_or(""));
        let v = parse_dec(parts.get(1).copied().unwrap_or(""));
        let rec = match (p, v) {
            (Some(period), Some(value)) => SampleRec { period, value },
            _ => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(AnomalyCode::NonNumeric);
            }
        };
        if let Some(last) = self.samples.last() {
            if rec.period <= last.period {
                self.rejected = self.rejected.saturating_add(1);
                return Err(AnomalyCode::PeriodRegress);
            }
        }
        self.samples.push(rec);
        Ok(rec)
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

fn parse_dec(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes.get(i)?;
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((*b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

// ── 阈值注册表 ────────────────────────────────────────────────────────
/// 一条冻结阈值：分子/分母 + 出处。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Threshold {
    /// 阈值分子（0..=分母）
    pub num: u32,
    /// 阈值分母（>0）
    pub den: u32,
    /// 出处（判据要求注出处，非空）
    pub source: &'static str,
}

/// 阈值注册表：注册后只读（漂移即拒）。
pub struct ThresholdRegistry {
    entries: Vec<Threshold>,
    named: Vec<(&'static str, usize)>,
}

impl ThresholdRegistry {
    /// 空注册表。
    pub fn new() -> ThresholdRegistry {
        ThresholdRegistry { entries: Vec::new(), named: Vec::new() }
    }

    /// 注册阈值：无出处 / 分母 0 / 分子>分母 / 重复名全拒。
    pub fn register(&mut self, name: &'static str, t: Threshold) -> Result<(), AnomalyCode> {
        if t.source.is_empty() {
            return Err(AnomalyCode::ThresholdNoSource);
        }
        if t.den == 0 || t.num > t.den {
            return Err(AnomalyCode::ThresholdOutOfRange);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some((existing, _)) = self.named.get(i) {
                if *existing == name {
                    return Err(AnomalyCode::DuplicateThreshold);
                }
            }
            i += 1;
        }
        self.entries.push(t);
        self.named.push((name, self.entries.len().saturating_sub(1)));
        Ok(())
    }

    /// 取阈值（只读视图）。
    pub fn get(&self, name: &str) -> Option<Threshold> {
        let mut i = 0usize;
        while i < self.named.len() {
            if let Some((existing, idx)) = self.named.get(i) {
                if *existing == name {
                    if let Some(t) = self.entries.get(*idx) {
                        return Some(*t);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 注册条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ── 检出器 ────────────────────────────────────────────────────────────
/// 检出结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 平稳（值 ≥ knee 且环比未降）
    Stable,
    /// 陡降（值 < knee = 基线×60%）
    SteepDrop,
    /// 回退（环比下降但未破 knee）
    Regression,
}

/// 陡降/回退检出器：knee 由基线（首样本）推导，禁魔数。
pub struct DropDetector {
    /// 触发告警数
    pub alerts: u32,
    /// 误报数（平稳被判异常，必须为 0）
    pub false_alarms: u32,
}

impl DropDetector {
    /// 空检出器。
    pub fn new() -> DropDetector {
        DropDetector { alerts: 0, false_alarms: 0 }
    }

    /// knee = 基线 × KNEE_NUM / KNEE_DEN（整数截断，可复算）。
    pub fn knee(baseline: u32) -> u32 {
        baseline.saturating_mul(KNEE_NUM) / KNEE_DEN
    }

    /// 逐样本判定：period>0 才有环比；value < knee 即陡降；
    /// value < 前值但 ≥ knee 即回退。
    pub fn scan(&mut self, samples: &[SampleRec]) -> Result<Vec<Verdict>, AnomalyCode> {
        if samples.is_empty() {
            return Err(AnomalyCode::EmptySeries);
        }
        let base = match samples.get(0) {
            Some(s) => s.value,
            None => return Err(AnomalyCode::EmptySeries),
        };
        let knee = Self::knee(base);
        let mut out: Vec<Verdict> = Vec::new();
        out.push(Verdict::Stable);
        let mut i = 1usize;
        while i < samples.len() {
            let cur = match samples.get(i) {
                Some(s) => *s,
                None => return Err(AnomalyCode::EmptySeries),
            };
            let prev = match samples.get(i - 1) {
                Some(s) => *s,
                None => return Err(AnomalyCode::EmptySeries),
            };
            let v = if cur.value < knee {
                self.alerts = self.alerts.saturating_add(1);
                Verdict::SteepDrop
            } else if cur.value < prev.value {
                self.alerts = self.alerts.saturating_add(1);
                Verdict::Regression
            } else {
                Verdict::Stable
            };
            out.push(v);
            i += 1;
        }
        Ok(out)
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadAnomalyCase {
    /// 无出处阈值
    NoSource,
    /// 期号回退
    PeriodBack,
    /// 阈值漂移（注册后试图改）
    Drift,
    /// 掩蔽陡降（漏警注入必须被反向验证检出）
    Masked,
    /// 空序列
    Empty,
}

impl BadAnomalyCase {
    /// 闭集全量。
    pub const ALL: [BadAnomalyCase; 5] = [
        BadAnomalyCase::NoSource,
        BadAnomalyCase::PeriodBack,
        BadAnomalyCase::Drift,
        BadAnomalyCase::Masked,
        BadAnomalyCase::Empty,
    ];

    /// 注入一轮：返回是否被显式检出/拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadAnomalyCase::NoSource => {
                let mut r = ThresholdRegistry::new();
                r.register("knee", Threshold { num: 60, den: 100, source: "" })
                    == Err(AnomalyCode::ThresholdNoSource)
            }
            BadAnomalyCase::PeriodBack => {
                let mut p = SeriesParser::default();
                let _ = p.parse("1|90");
                p.parse("1|80") == Err(AnomalyCode::PeriodRegress)
            }
            BadAnomalyCase::Drift => {
                let mut r = ThresholdRegistry::new();
                let _ = r.register("knee", Threshold { num: 60, den: 100, source: "LTP 6/10" });
                let dup_rejected = r.register("knee", Threshold { num: 40, den: 100, source: "LTP 4/10" })
                    == Err(AnomalyCode::DuplicateThreshold);
                let kept = match r.get("knee") {
                    Some(t) => t.num == 60,
                    None => false,
                };
                dup_rejected && kept
            }
            BadAnomalyCase::Masked => {
                // 漏警注入：基线 100 → 30（< knee=60），检出器必须报 SteepDrop
                let mut d = DropDetector::new();
                let v = d.scan(&[
                    SampleRec { period: 1, value: 100 },
                    SampleRec { period: 2, value: 30 },
                ]);
                match v {
                    Ok(vs) => vs.get(1).copied() == Some(Verdict::SteepDrop),
                    Err(_) => false,
                }
            }
            BadAnomalyCase::Empty => {
                let mut d = DropDetector::new();
                d.scan(&[]) == Err(AnomalyCode::EmptySeries)
            }
        }
    }
}

/// 告警账：应警 = 触发 + 漏警，漏警恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AlertLedger {
    /// 应警总数（判据构造的必警样本数）
    pub expected: u32,
    /// 实际触发数
    pub fired: u32,
    /// 漏警数（必须为 0）
    pub missed: u32,
}

impl AlertLedger {
    /// 记一次应警判定。
    pub fn record(&mut self, fired: bool) -> Result<(), AnomalyCode> {
        self.expected = self.expected.saturating_add(1);
        if fired {
            self.fired = self.fired.saturating_add(1);
        } else {
            self.missed = self.missed.saturating_add(1);
        }
        if self.fired.saturating_add(self.missed) != self.expected {
            return Err(AnomalyCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式：零漏警。
    pub fn no_miss(&self) -> bool {
        self.missed == 0 && self.fired == self.expected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_anomaly_injection() -> (AlertLedger, [u32; 5]) {
    let mut ledger = AlertLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadAnomalyCase::ALL.len() {
        if let Some(case) = BadAnomalyCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
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

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺样本/缺阈值族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ERANGE", -34, 34, "Windows ERROR_GEN_FAILURE；比例越域"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；阈值重注册族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 规范语料双跑指纹：解析→扫描全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = SeriesParser::default();
    let texts = ["1|100", "2|95", "3|58", "4|61", "5|30"];
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let r = p.parse(t);
            if r.is_err() {
                return 0;
            }
        }
        i += 1;
    }
    let mut d = DropDetector::new();
    let verdicts = d.scan(&p.samples);
    let mut bytes: Vec<u8> = Vec::new();
    match verdicts {
        Ok(vs) => {
            let mut j = 0usize;
            while j < vs.len() {
                if let Some(v) = vs.get(j) {
                    bytes.push(match v {
                        Verdict::Stable => 0,
                        Verdict::SteepDrop => 1,
                        Verdict::Regression => 2,
                    });
                }
                j += 1;
            }
        }
        Err(_) => bytes.push(0xFF),
    }
    bytes.push(d.alerts as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10013 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10013_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10013");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91340..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91340..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. knee 推导可复算：基线 100 → knee 60
    s.add("knee 推导：基线 100 × 60% = 60", DropDetector::knee(100) == 60,
          "knee 由基线与冻结比例整数推导，禁魔数");

    // 3. knee 单调：基线越大 knee 越大
    let mono = DropDetector::knee(50) < DropDetector::knee(80)
        && DropDetector::knee(80) < DropDetector::knee(100);
    s.add("knee 单调：随基线单调不减", mono,
          "比例推导必须保序，否则阈值口径漂移");

    // 4. 陡降触发：100 → 58 报 SteepDrop（58<60）
    let mut d = DropDetector::new();
    let v = d.scan(&[
        SampleRec { period: 1, value: 100 },
        SampleRec { period: 2, value: 58 },
    ]);
    let drop_ok = v == Ok(vec![Verdict::Stable, Verdict::SteepDrop]) && d.alerts == 1;
    s.add("陡降触发：跌破 knee 报 SteepDrop", drop_ok,
          "阈值判定按 knee=基线×60% 逐样本执行");

    // 5. 回退触发：100 → 80 报 Regression（未破 knee 但环比降）
    let mut d2 = DropDetector::new();
    let v2 = d2.scan(&[
        SampleRec { period: 1, value: 100 },
        SampleRec { period: 2, value: 80 },
    ]);
    let reg_ok = v2 == Ok(vec![Verdict::Stable, Verdict::Regression]);
    s.add("回退触发：环比下降未破 knee 报 Regression", reg_ok,
          "回退与陡降两档分开报告，不得混型");

    // 6. 平稳零误报
    let mut d3 = DropDetector::new();
    let v3 = d3.scan(&[
        SampleRec { period: 1, value: 100 },
        SampleRec { period: 2, value: 100 },
        SampleRec { period: 3, value: 105 },
    ]);
    let stable_ok = v3 == Ok(vec![Verdict::Stable, Verdict::Stable, Verdict::Stable])
        && d3.false_alarms == 0;
    s.add("零误报：平稳/上升序列不报警", stable_ok,
          "方向写反即 FalseAlarm，必须为 0");

    // 7. 阈值出处冻结：无出处拒注册
    let mut r = ThresholdRegistry::new();
    let nosrc = r.register("knee", Threshold { num: 60, den: 100, source: "" })
        == Err(AnomalyCode::ThresholdNoSource);
    s.add("出处冻结：空出处阈值拒绝注册", nosrc,
          "每条阈值必须注出处（判据原文要求）");

    // 8. 阈值越域拒绝
    let oob = r.register("bad", Threshold { num: 101, den: 100, source: "x" })
        == Err(AnomalyCode::ThresholdOutOfRange)
        && r.register("zero", Threshold { num: 1, den: 0, source: "x" })
            == Err(AnomalyCode::ThresholdOutOfRange);
    s.add("越域拒绝：分子>分母/分母 0 拒注册", oob,
          "比例阈值必须落在 0..=100%");

    // 9. 阈值漂移拦截：重注册不改值
    let mut r2 = ThresholdRegistry::new();
    let _ = r2.register("knee", Threshold { num: 60, den: 100, source: "LTP 6/10" });
    let drift = r2.register("knee", Threshold { num: 40, den: 100, source: "LTP 4/10" })
        == Err(AnomalyCode::DuplicateThreshold)
        && match r2.get("knee") {
            Some(t) => t.num == 60,
            None => false,
        };
    s.add("漂移拦截：重注册被拒且原值不变", drift,
          "注册后阈值只读，改动必须走变更通道");

    // 10. 期号回退拒绝
    let mut p = SeriesParser::default();
    let back = p.parse("1|90").is_ok() && p.parse("1|80") == Err(AnomalyCode::PeriodRegress);
    s.add("期序守卫：期号回退显性拒绝", back,
          "样本流必须单调递增");

    // 11. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_anomaly_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "无出处/回退/漂移/掩蔽/空序列全被显式拒绝");

    // 12. 告警账守恒：漏警=0
    s.add("告警账守恒：应警=触发+漏警且漏警=0",
          ledger.no_miss() && ledger.expected == 5 * INJECT_ROUNDS,
          "50 次应警必须 50 次触发、0 漏警");

    // 13. 诊断码闭集：文案非空
    let text_ok = AnomalyCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i = 0usize;
    while i < AnomalyCode::ALL.len() {
        let mut j = i + 1;
        while j < AnomalyCode::ALL.len() {
            if let (Some(x), Some(y)) = (AnomalyCode::ALL.get(i), AnomalyCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = AnomalyCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 17. 非数值拒绝
    let mut p2 = SeriesParser::default();
    let nonnum = p2.parse("1|x").is_err() && p2.parse("|").is_err();
    s.add("解析边界：非数值/空段拒绝", nonnum,
          "任何字段异常都不得静默吞样本");

    // 动态明细（≤8）：告警账一行 + 指纹一行
    let ledger_text = format!(
        "应警 {} / 触发 {} / 漏警 {}",
        ledger.expected, ledger.fired, ledger.missed
    );
    s.add_dyn("账目快照", ledger.no_miss(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_derivation() {
        assert_eq!(DropDetector::knee(100), 60);
        assert_eq!(DropDetector::knee(50), 30);
        assert_eq!(DropDetector::knee(0), 0);
    }

    #[test]
    fn steep_drop_fires() {
        let mut d = DropDetector::new();
        let v = d.scan(&[
            SampleRec { period: 1, value: 100 },
            SampleRec { period: 2, value: 10 },
        ]);
        assert_eq!(v, Ok(vec![Verdict::Stable, Verdict::SteepDrop]));
        assert_eq!(d.alerts, 1);
    }

    #[test]
    fn regression_not_drop() {
        let mut d = DropDetector::new();
        let v = d.scan(&[
            SampleRec { period: 1, value: 100 },
            SampleRec { period: 2, value: 90 },
        ]);
        assert_eq!(v, Ok(vec![Verdict::Stable, Verdict::Regression]));
    }

    #[test]
    fn stable_series_no_alarm() {
        let mut d = DropDetector::new();
        let v = d.scan(&[
            SampleRec { period: 1, value: 100 },
            SampleRec { period: 2, value: 100 },
        ]);
        assert_eq!(v, Ok(vec![Verdict::Stable, Verdict::Stable]));
        assert_eq!(d.alerts, 0);
    }

    #[test]
    fn empty_series_rejected() {
        let mut d = DropDetector::new();
        assert_eq!(d.scan(&[]), Err(AnomalyCode::EmptySeries));
    }

    #[test]
    fn threshold_requires_source() {
        let mut r = ThresholdRegistry::new();
        assert_eq!(
            r.register("k", Threshold { num: 60, den: 100, source: "" }),
            Err(AnomalyCode::ThresholdNoSource)
        );
        assert!(r.register("k", Threshold { num: 60, den: 100, source: "LTP 6/10" }).is_ok());
    }

    #[test]
    fn threshold_no_drift() {
        let mut r = ThresholdRegistry::new();
        let _ = r.register("k", Threshold { num: 60, den: 100, source: "LTP 6/10" });
        assert_eq!(
            r.register("k", Threshold { num: 40, den: 100, source: "LTP 4/10" }),
            Err(AnomalyCode::DuplicateThreshold)
        );
        assert_eq!(r.get("k").map(|t| t.num), Some(60));
    }

    #[test]
    fn period_must_increase() {
        let mut p = SeriesParser::default();
        let _ = p.parse("1|10");
        assert_eq!(p.parse("1|20"), Err(AnomalyCode::PeriodRegress));
    }

    #[test]
    fn injection_matrix_zero_miss() {
        let (l, per) = run_anomaly_injection();
        assert!(l.no_miss());
        assert_eq!(l.expected, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ERANGE));
        assert!(agrees_with_linux(EBADF));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10013_checks_green() {
        let s = run_unx10013_checks();
        assert!(s.all_passed(), "UNX-F10013 red: {}", s.red_items_text());
    }
}
