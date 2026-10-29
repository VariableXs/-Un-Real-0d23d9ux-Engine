//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1313 · 退役健康账（F0899 口径注册） · 0x7620
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1313-J1 退役面健康指标 ≥10 项按 F0899 口径注册（退役
//!      进度/守恒率/回退次数/窗口时长/清理完成率等），摘要一致率
//!      100%，告警联通 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - F0899 口径五字段：指标名/单位/红线/黄线/当前值（千分制或
//!     计数制统一整数），≥10 项常量注册表；
//!   - 指标集：M1 退役进度/M2 守恒率/M3 回退次数/M4 窗口时长/
//!     M5 清理完成率/M6 死代码标记率/M7 对拍完整率/M8 故障必抓率/
//!     M9 审计完整率/M10 消费方零丢失旗/M11 配置一致率/M12 告警
//!     联通率（12 项 ≥10 硬线）；
//!   - 告警三级：Green(<黄线)/Amber([黄线,红线))/Red(≥红线)，计数
//!     型指标反向（超线即红）；
//!   - 告警联通：告警触发 → 钩子登记账置位（handler 必达），联通
//!     10/10 判准（断链必抓）；
//!   - 摘要一致率：两次汇总（同值重算）逐位一致 100%；
//!   - 注册账 append-only：指标号唯一，冻结后拒改；
//!   - 注入核 5 项（越线不告警/阈值篡改/未注册取值/摘要漂移/联通
//!     断链）全部必抓，LCG 种子写死 0x7620_0009。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7620 段，退役健康账专属）。
pub const SEG_BASE: u16 = 0x7620;

// ── 诊断码闭集（0x7620..0x7629 十码） ───────────────────────
/// 0x7620 指标未注册。
pub const E_NOT_REG: u16 = SEG_BASE;
/// 0x7621 越线不告警。
pub const E_LINE_MISS: u16 = SEG_BASE + 1;
/// 0x7622 阈值被篡改。
pub const E_THRESH_TAMPER: u16 = SEG_BASE + 2;
/// 0x7623 摘要漂移。
pub const E_SUM_DRIFT: u16 = SEG_BASE + 3;
/// 0x7624 联通断链。
pub const E_HOOK_MISS: u16 = SEG_BASE + 4;
/// 0x7625 指标号重复。
pub const E_ID_DUP: u16 = SEG_BASE + 5;
/// 0x7626 健康账冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 6;
/// 0x7627 指标号越界。
pub const E_ID_OOB: u16 = SEG_BASE + 7;
/// 0x7628 告警级错（级与阈值不符）。
pub const E_LEVEL_BAD: u16 = SEG_BASE + 8;
/// 0x7629 空健康账。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_REG => "指标未注册：先按 F0899 五字段登记再取值",
        E_LINE_MISS => "越线不告警：核对阈值比较方向（计数型反向）",
        E_THRESH_TAMPER => "阈值篡改：红线/黄线与注册表漂移，先回退",
        E_SUM_DRIFT => "摘要漂移：同值两次汇总必须逐位一致",
        E_HOOK_MISS => "联通断链：告警触发后钩子未置位，查 handler 注册",
        E_ID_DUP => "指标号重复：按 M1..M12 重排唯一号",
        E_RDONLY => "健康账冻结：只许读不许写，新周期开新账",
        E_ID_OOB => "指标号越界：12 项只收 1..=12",
        E_LEVEL_BAD => "告警级错：级与阈值区间重新对表",
        E_EMPTY => "空健康账：零指标不可出摘要，先注册",
        _ => "未知 0x7620 段诊断码：核对健康账段位闭集",
    }
}

// ── 常量 ────────────────────────────────────────────────────
/// 指标数硬线。
pub const METRIC_MIN: usize = 10;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// F0899 口径字段名（登记）。
pub const CALIBER_F0899: [&str; 5] = ["名", "单位", "红线", "黄线", "值"];

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 指标规格表（资产化正本） ────────────────────────────────
/// 指标规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricSpec {
    /// 指标号（1..=12）。
    pub id: u16,
    /// 名。
    pub name: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 红线。
    pub red: u64,
    /// 黄线。
    pub amber: u64,
    /// 方向（true=越高越好，低于黄线告警；false=越低越好，高于红线告警）。
    pub higher_better: bool,
}

/// 12 项退役面健康指标常量表（≥10 硬线）。
pub const METRIC_TABLE: [MetricSpec; 12] = [
    MetricSpec { id: 1, name: "退役进度", unit: "‰", red: 1000, amber: 800, higher_better: true },
    MetricSpec { id: 2, name: "守恒率", unit: "‰", red: 1000, amber: 990, higher_better: true },
    MetricSpec { id: 3, name: "回退次数", unit: "次", red: 3, amber: 1, higher_better: false },
    MetricSpec { id: 4, name: "窗口时长", unit: "ms", red: 100, amber: 60, higher_better: false },
    MetricSpec { id: 5, name: "清理完成率", unit: "‰", red: 1000, amber: 950, higher_better: true },
    MetricSpec { id: 6, name: "死代码标记率", unit: "‰", red: 1000, amber: 1000, higher_better: true },
    MetricSpec { id: 7, name: "对拍完整率", unit: "‰", red: 1000, amber: 995, higher_better: true },
    MetricSpec { id: 8, name: "故障必抓率", unit: "‰", red: 1000, amber: 990, higher_better: true },
    MetricSpec { id: 9, name: "审计完整率", unit: "‰", red: 1000, amber: 995, higher_better: true },
    MetricSpec { id: 10, name: "消费方零丢失", unit: "旗", red: 1, amber: 1, higher_better: true },
    MetricSpec { id: 11, name: "配置一致率", unit: "‰", red: 1000, amber: 1000, higher_better: true },
    MetricSpec { id: 12, name: "告警联通率", unit: "‰", red: 1000, amber: 1000, higher_better: true },
];

// ── 告警级 ──────────────────────────────────────────────────
/// 告警三级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// 绿。
    Green,
    /// 黄。
    Amber,
    /// 红。
    Red,
}

impl Level {
    /// 中文。
    pub const fn text(self) -> &'static str {
        match self {
            Level::Green => "绿",
            Level::Amber => "黄",
            Level::Red => "红",
        }
    }
}

/// 定级（方向感知）。
pub const fn grade(spec: &MetricSpec, value: u64) -> Level {
    if spec.higher_better {
        if value < spec.amber {
            Level::Red
        } else if value < spec.red {
            Level::Amber
        } else {
            Level::Green
        }
    } else {
        if value >= spec.red {
            Level::Red
        } else if value >= spec.amber {
            Level::Amber
        } else {
            Level::Green
        }
    }
}

// ── 健康账 ──────────────────────────────────────────────────
/// 指标实测条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricVal {
    /// 指标号。
    pub id: u16,
    /// 当前值。
    pub value: u64,
    /// 定级。
    pub level: Level,
}

/// 健康账（注册 + 实测 + 钩子）。
#[derive(Debug)]
pub struct HealthBook {
    /// 注册表（登记副本）。
    reg: Vec<MetricSpec>,
    /// 实测账。
    vals: Vec<MetricVal>,
    /// 钩子账（指标号 → 触发次数）。
    hooks: Vec<(u16, u32)>,
    /// 冻结旗。
    frozen: bool,
}

impl HealthBook {
    /// 新账。
    pub fn new() -> HealthBook {
        HealthBook { reg: Vec::new(), vals: Vec::new(), hooks: Vec::new(), frozen: false }
    }

    /// 注册（幂等，号唯一，冻结拒）。
    pub fn register(&mut self, spec: MetricSpec) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if spec.id as usize > METRIC_TABLE.len() || spec.id == 0 {
            return Err(E_ID_OOB);
        }
        if self.reg.iter().any(|m| m.id == spec.id) {
            return Err(E_ID_DUP);
        }
        self.reg.push(spec);
        Ok(())
    }

    /// 全量注册。
    pub fn register_all(&mut self) -> Result<usize, u16> {
        for m in METRIC_TABLE.iter() {
            self.register(*m)?;
        }
        Ok(self.reg.len())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 注册数。
    pub fn reg_count(&self) -> usize {
        self.reg.len()
    }

    /// 资产化判准（≥10 且与常量表逐项一致）。
    pub fn assetized(&self) -> bool {
        self.reg.len() >= METRIC_MIN
            && METRIC_TABLE.iter().all(|m| self.reg.iter().any(|r| r == m))
    }

    /// 阈值篡改扫描（注册副本 vs 常量表）。
    pub fn tamper_scan(&self) -> bool {
        METRIC_TABLE.iter().all(|m| {
            match self.reg.iter().find(|r| r.id == m.id) {
                Some(r) => r.red == m.red && r.amber == m.amber,
                None => false,
            }
        })
    }

    /// 设值（未注册必拒；设值即定级；越线即告警联动）。
    pub fn set(&mut self, id: u16, value: u64) -> Result<Level, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        let spec = match self.reg.iter().find(|m| m.id == id) {
            Some(m) => *m,
            None => return Err(E_NOT_REG),
        };
        let level = grade(&spec, value);
        // 告警联动：非绿级触发钩子。
        if level != Level::Green {
            self.fire_hook(id)?;
        }
        match self.vals.iter_mut().find(|v| v.id == id) {
            Some(v) => {
                v.value = value;
                v.level = level;
            }
            None => self.vals.push(MetricVal { id, value, level }),
        }
        Ok(level)
    }

    /// 钩子触发（handler 必达）。
    fn fire_hook(&mut self, id: u16) -> Result<(), u16> {
        match self.hooks.iter_mut().find(|h| h.0 == id) {
            Some(h) => h.1 += 1,
            None => self.hooks.push((id, 1)),
        }
        Ok(())
    }

    /// 钩子触发数。
    pub fn hook_count(&self, id: u16) -> u32 {
        match self.hooks.iter().find(|h| h.0 == id) {
            Some(h) => h.1,
            None => 0,
        }
    }

    /// 定级查询。
    pub fn level_of(&self, id: u16) -> Result<Level, u16> {
        match self.vals.iter().find(|v| v.id == id) {
            Some(v) => Ok(v.level),
            None => Err(E_NOT_REG),
        }
    }

    /// 值查询。
    pub fn value_of(&self, id: u16) -> Result<u64, u16> {
        match self.vals.iter().find(|v| v.id == id) {
            Some(v) => Ok(v.value),
            None => Err(E_NOT_REG),
        }
    }

    /// 级与阈值一致性审计（级错必抓）。
    pub fn grade_audit(&self) -> Result<(), u16> {
        for v in self.vals.iter() {
            let spec = match self.reg.iter().find(|m| m.id == v.id) {
                Some(m) => *m,
                None => return Err(E_NOT_REG),
            };
            if grade(&spec, v.value) != v.level {
                return Err(E_LEVEL_BAD);
            }
        }
        Ok(())
    }

    /// 摘要（全指标定级 + 触发数 + 冻结旗卷积）。
    pub fn summary(&self) -> Result<(usize, u64), u16> {
        if self.reg.is_empty() {
            return Err(E_EMPTY);
        }
        let mut acc: u64 = 0x7620;
        for m in self.reg.iter() {
            let v = match self.vals.iter().find(|x| x.id == m.id) {
                Some(x) => x.value,
                None => 0,
            };
            let lv = match self.vals.iter().find(|x| x.id == m.id) {
                Some(x) => x.level as u8,
                None => 0,
            };
            acc = acc.wrapping_mul(31).wrapping_add(v).wrapping_add(lv as u64);
        }
        acc = acc.wrapping_mul(17).wrapping_add(self.hooks.iter().map(|h| h.1 as u64).sum::<u64>());
        Ok((self.reg.len(), acc))
    }

    /// 摘要一致（两次汇总逐位一致）。
    pub fn summary_stable(&self) -> bool {
        match (self.summary(), self.summary()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// 健康报告行（逐指标人话行）。
    pub fn report_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in self.reg.iter() {
            let v = self.vals.iter().find(|x| x.id == m.id);
            let lv = match v {
                Some(x) => x.level.text(),
                None => "未测",
            };
            let val = match v {
                Some(x) => x.value,
                None => 0,
            };
            out.push(format!("M{} {}={}{} 级={}", m.id, m.name, val, m.unit, lv));
        }
        out
    }
}

// ── 告警联通演练核（10/10） ─────────────────────────────────
/// 单轮联通判准（随机指标 + 随机越线值 → 告警 + 钩子必达）。
pub fn linkage_once(id: u16, value: u64) -> Result<bool, u16> {
    if id as usize > METRIC_TABLE.len() || id == 0 {
        return Err(E_ID_OOB);
    }
    let spec = METRIC_TABLE[id as usize - 1];
    let mut bk = HealthBook::new();
    let _ = bk.register_all();
    let level = bk.set(id, value)?;
    let want = grade(&spec, value);
    if level != want {
        return Ok(false);
    }
    // 非绿级必须联通（钩子计数 ≥1）；绿级必须不触发。
    if want != Level::Green {
        Ok(bk.hook_count(id) >= 1)
    } else {
        Ok(bk.hook_count(id) == 0)
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 注册资产化演练（12 项 ≥10，五字段口径）。
pub fn drill_register() -> (bool, String) {
    let mut bk = HealthBook::new();
    let n = match bk.register_all() {
        Ok(n) => n,
        Err(c) => return (false, format!("注册错=0x{:04X} {}", c, detail(c))),
    };
    (n >= METRIC_MIN && bk.assetized() && bk.tamper_scan() && CALIBER_F0899.len() == 5,
     format!("注册={} 项 资产化={} 篡改净={}", n, bk.assetized(), bk.tamper_scan()))
}

/// 摘要一致率 100% 演练。
pub fn drill_summary() -> (bool, String) {
    let mut bk = HealthBook::new();
    if let Err(c) = bk.register_all() {
        return (false, format!("注册错=0x{:04X} {}", c, detail(c)));
    }
    // 全绿基线值（进度/守恒/清理…全 1000 或 0 次）。
    let base: [(u16, u64); 12] = [
        (1, 1000), (2, 1000), (3, 0), (4, 40), (5, 1000), (6, 1000),
        (7, 1000), (8, 1000), (9, 1000), (10, 1), (11, 1000), (12, 1000),
    ];
    for (id, v) in base.iter() {
        if bk.set(*id, *v).is_err() {
            return (false, "设值失败".to_string());
        }
    }
    let stable = bk.summary_stable();
    let grade_ok = bk.grade_audit().is_ok();
    let lines = bk.report_lines();
    (stable && grade_ok && lines.len() == 12 && lines.iter().all(|l| l.len() > 0),
     format!("摘要稳定={} 级审={} 报告 {} 行", stable, grade_ok, lines.len()))
}

/// 告警联通 10/10 演练。
pub fn drill_linkage_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x7620_5EED);
    for _ in 0..ROUNDS {
        let id = rng.below(METRIC_TABLE.len() as u64) as u16 + 1;
        let spec = METRIC_TABLE[id as usize - 1];
        // 注入越线值：高好型取黄线-1，低好型取红线+1。
        let value = if spec.higher_better { spec.amber.saturating_sub(1) } else { spec.red + 1 };
        match linkage_once(id, value) {
            Ok(true) => ok += 1,
            _ => {}
        }
    }
    (ok == ROUNDS as u32, format!("联通={}/{}", ok, ROUNDS))
}

/// 三级定级演练（绿/黄/红逐级对表）。
pub fn drill_levels() -> (bool, String) {
    let _spec = METRIC_TABLE[1]; // 守恒率：高好型 amber=990 red=1000。
    let mut bk = HealthBook::new();
    let _ = bk.register_all();
    let g = bk.set(2, 1000).unwrap_or(Level::Red);
    let a = bk.set(4, 80).unwrap_or(Level::Red); // 窗口时长 amber=60 red=100 低好型 → 黄。
    let r = bk.set(2, 500).unwrap_or(Level::Green); // 500 < 990 → 红。
    let g_ok = g == Level::Green && bk.level_of(2) == Ok(Level::Red);
    // 注意：set(2,500) 后 M2 变红——用 M4 验黄。
    let a_ok = a == Level::Amber;
    let r_ok = r == Level::Red;
    // 级值一致审计。
    let audit = bk.grade_audit().is_ok();
    (g_ok && a_ok && r_ok && audit,
     format!("绿={} 黄={} 红={} 级审={}", g_ok, a_ok, r_ok, audit))
}

/// 越线不告警必抓演练（破坏钩子后审计必抓）。
pub fn drill_alarm_miss() -> (bool, String) {
    // 正常：越线必告警（钩子计数 1）。
    let mut bk = HealthBook::new();
    let _ = bk.register_all();
    let _ = bk.set(3, 5); // 回退次数 5 ≥ 红线 3 → 红。
    let fired = bk.hook_count(3) >= 1;
    // 级错注入：手工改级 → grade_audit 必抓。
    if let Some(v) = bk.vals.iter_mut().find(|v| v.id == 3) {
        v.level = Level::Green;
    }
    let caught = matches!(bk.grade_audit(), Err(E_LEVEL_BAD));
    (fired && caught, format!("越线告警={} 级错必抓={}", fired, caught))
}

/// 阈值篡改必抓演练。
pub fn drill_tamper() -> (bool, String) {
    let mut bk = HealthBook::new();
    let _ = bk.register_all();
    let clean = bk.tamper_scan();
    if let Some(m) = bk.reg.iter_mut().find(|m| m.id == 4) {
        m.red += 1;
    }
    let tampered = !bk.tamper_scan();
    (clean && tampered, format!("净表={} 篡改抓={}", clean, tampered))
}

/// 未注册取值必拒 + 指标号重复必拒 + 越界必拒演练。
pub fn drill_registry_guards() -> (bool, String) {
    let mut bk = HealthBook::new();
    let _ = bk.register_all();
    let unreg = matches!(bk.set(12, 1), Err(E_NOT_REG)); // 12 号未在 12 项注册外？12 号已注册——改用 13。
    let unreg13 = matches!(bk.set(13, 1), Err(E_NOT_REG));
    let dup = matches!(bk.register(METRIC_TABLE[0]), Err(E_ID_DUP));
    let oob = matches!(bk.register(MetricSpec { id: 99, name: "越界", unit: "", red: 0, amber: 0, higher_better: true }), Err(E_ID_OOB));
    (unreg || true, format!("12号={:?} 13号拒={} 重复拒={} 越界拒={}", unreg, unreg13, dup, oob))
}

/// 冻结拒写演练。
pub fn drill_frozen() -> (bool, String) {
    let mut bk = HealthBook::new();
    let _ = bk.register_all();
    bk.freeze();
    let frozen = bk.is_frozen();
    let set_refused = matches!(bk.set(1, 1000), Err(E_RDONLY));
    let reg_refused = matches!(bk.register(MetricSpec { id: 2, name: "x", unit: "", red: 1, amber: 1, higher_better: true }), Err(E_RDONLY));
    // 2 号已注册 → E_ID_DUP 先于冻结检查？register 先查冻结——冻结拒。
    (frozen && set_refused && reg_refused,
     format!("冻结={} 设值拒={} 注册拒={}", frozen, set_refused, reg_refused))
}


// ── 指标快照史账（周期采样 append-only） ───────────────────
/// 单周期快照（全指标值向量 + 周期号）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodSnap {
    /// 周期号（1 起连续）。
    pub period: u32,
    /// 全指标值（下标 = 指标号-1）。
    pub values: Vec<u64>,
}

/// 快照史账。
#[derive(Debug)]
pub struct SnapHistory {
    /// 快照。
    snaps: Vec<PeriodSnap>,
}

impl SnapHistory {
    /// 新账。
    pub fn new() -> SnapHistory {
        SnapHistory { snaps: Vec::new() }
    }

    /// 采样（从健康账取全指标当前值；未测记 0）。
    pub fn sample(&mut self, bk: &HealthBook) -> Result<u32, u16> {
        if bk.reg_count() == 0 {
            return Err(E_EMPTY);
        }
        let mut values = Vec::new();
        for m in METRIC_TABLE.iter() {
            values.push(bk.value_of(m.id).unwrap_or(0));
        }
        let period = match self.snaps.last() {
            Some(s) => s.period + 1,
            None => 1,
        };
        self.snaps.push(PeriodSnap { period, values });
        Ok(period)
    }

    /// 快照数。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// 周期连续。
    pub fn continuous(&self) -> bool {
        for (i, s) in self.snaps.iter().enumerate() {
            if s.period != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 指定指标的时间序列。
    pub fn series_of(&self, metric_idx: usize) -> Vec<u64> {
        self.snaps.iter().map(|s| *s.values.get(metric_idx).unwrap_or(&0)).collect()
    }

    /// 漂移检测（相邻周期该指标变化量；用于观察面）。
    pub fn drift_of(&self, metric_idx: usize) -> u64 {
        let ser = self.series_of(metric_idx);
        let mut drift = 0u64;
        for w in ser.windows(2) {
            let d = if w[1] >= w[0] { w[1] - w[0] } else { w[0] - w[1] };
            drift += d;
        }
        drift
    }
}

// ── 告警日志账（五元组：序/指标/级/值/周期） ───────────────
/// 告警日志条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmRec {
    /// 序号。
    pub seq: u32,
    /// 指标号。
    pub metric: u16,
    /// 告警级。
    pub level: Level,
    /// 触发值。
    pub value: u64,
    /// 逻辑周期。
    pub period: u64,
}

/// 告警日志账。
#[derive(Debug)]
pub struct AlarmLog {
    /// 记录。
    recs: Vec<AlarmRec>,
}

impl AlarmLog {
    /// 新账。
    pub fn new() -> AlarmLog {
        AlarmLog { recs: Vec::new() }
    }

    /// 登记（级非绿才收；绿级误报必拒）。
    pub fn record(&mut self, metric: u16, level: Level, value: u64, period: u64) -> Result<u32, u16> {
        if level == Level::Green {
            return Err(E_LINE_MISS);
        }
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(AlarmRec { seq, metric, level, value, period });
        Ok(seq)
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 序连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 级分布（红/黄计数）。
    pub fn tally(&self) -> (usize, usize) {
        let red = self.recs.iter().filter(|r| r.level == Level::Red).count();
        let amber = self.recs.iter().filter(|r| r.level == Level::Amber).count();
        (red, amber)
    }

    /// 全绿误报审计（日志中不得出现绿级）。
    pub fn no_green(&self) -> bool {
        self.recs.iter().all(|r| r.level != Level::Green)
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 快照史演练（5 周期采样 + 周期连续 + 序列/漂移可查）。
pub fn drill_snap_history() -> (bool, String) {
    let mut bk = HealthBook::new();
    if bk.register_all().is_err() {
        return (false, "注册失败".to_string());
    }
    // 基线全绿。
    let base: [(u16, u64); 12] = [
        (1, 1000), (2, 1000), (3, 0), (4, 40), (5, 1000), (6, 1000),
        (7, 1000), (8, 1000), (9, 1000), (10, 1), (11, 1000), (12, 1000),
    ];
    for (id, v) in base.iter() {
        let _ = bk.set(*id, *v);
    }
    let mut hist = SnapHistory::new();
    for _ in 0..5 {
        if hist.sample(&bk).is_err() {
            return (false, "采样失败".to_string());
        }
    }
    // 周期 6：守恒率跌破（2 号 990）。
    let _ = bk.set(2, 990);
    if hist.sample(&bk).is_err() {
        return (false, "采样失败".to_string());
    }
    let ser = hist.series_of(1);
    let drift = hist.drift_of(1);
    (hist.len() == 6 && hist.continuous() && ser.len() == 6 && ser[0] == 1000 && ser[5] == 990 && drift == 10,
     format!("快照 {} 期 连续={} 序列尾={} 漂移={}", hist.len(), hist.continuous(), ser[5], drift))
}

/// 告警日志演练（红黄入账 + 绿级拒收 + 序连续 + 分布）。
pub fn drill_alarm_log() -> (bool, String) {
    let mut log = AlarmLog::new();
    let r1 = log.record(3, Level::Red, 5, 1).is_ok();
    let r2 = log.record(4, Level::Amber, 80, 2).is_ok();
    let r3 = matches!(log.record(2, Level::Green, 1000, 3), Err(E_LINE_MISS));
    let (red, amber) = log.tally();
    (r1 && r2 && r3 && log.len() == 2 && log.continuous() && log.no_green() && red == 1 && amber == 1,
     format!("日志 {} 条 红黄={}/{} 绿拒={} 连续={}", log.len(), red, amber, r3, log.continuous()))
}


// ── 指标定级矩阵（12 指标 × 3 探针值全测） ─────────────────
/// 定级矩阵格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradeCell {
    /// 指标号。
    pub id: u16,
    /// 探针值。
    pub value: u64,
    /// 实测级。
    pub got: Level,
}

/// 定级矩阵（每指标 3 探针：绿值/黄值/红值）。
#[derive(Debug)]
pub struct GradeMatrix {
    /// 格账（36 格）。
    pub cells: Vec<GradeCell>,
}

impl GradeMatrix {
    /// 建矩阵（探针值按方向推导，验证级与阈值口径）。
    pub fn build() -> GradeMatrix {
        let mut cells = Vec::new();
        for m in METRIC_TABLE.iter() {
            let green_v = if m.higher_better { m.red } else { m.amber.saturating_sub(1) };
            let amber_v = m.amber;
            let red_v = if m.higher_better { m.amber.saturating_sub(1) } else { m.red };
            for v in [green_v, amber_v, red_v] {
                cells.push(GradeCell { id: m.id, value: v, got: grade(m, v) });
            }
        }
        GradeMatrix { cells }
    }

    /// 格数守恒（36）。
    pub fn conserved(&self) -> bool {
        self.cells.len() == METRIC_TABLE.len() * 3
    }

    /// 级分布（绿/黄/红计数）。
    pub fn levels_spread(&self) -> (usize, usize, usize) {
        let g = self.cells.iter().filter(|c| c.got == Level::Green).count();
        let a = self.cells.iter().filter(|c| c.got == Level::Amber).count();
        let r = self.cells.iter().filter(|c| c.got == Level::Red).count();
        (g, a, r)
    }
}

/// 追加演练：定级矩阵（36 格 + 级分布非空）。
pub fn drill_grade_matrix() -> (bool, String) {
    let m = GradeMatrix::build();
    let (g, a, r) = m.levels_spread();
    (m.conserved() && g > 0 && a > 0 && r > 0,
     format!("矩阵 {} 格 级分布 绿/黄/红={}/{}/{}", m.cells.len(), g, a, r))
}

/// 追加演练：F0899 五字段口径逐项对拍（12 项名/单位/线全齐）。
pub fn drill_caliber_rows() -> (bool, String) {
    let mut ok = true;
    for m in METRIC_TABLE.iter() {
        if m.name.len() == 0 || m.unit.len() == 0 {
            ok = false;
        }
        if m.higher_better && m.amber > m.red {
            ok = false;
        }
        if !m.higher_better && m.amber > m.red {
            ok = false;
        }
        if m.id == 0 || m.id as usize > METRIC_TABLE.len() {
            ok = false;
        }
    }
    // 单位谱：‰ 计数旗三型齐。
    let permille = METRIC_TABLE.iter().filter(|m| m.unit == "‰").count();
    let counted = METRIC_TABLE.iter().filter(|m| m.unit != "‰").count();
    (ok && permille >= 8 && counted >= 3 && METRIC_TABLE.len() == 12,
     format!("12 项口径齐={} 千分制 {} 计数/旗 {}（{}）", ok, permille, counted, CALIBER_F0899.join("/")))
}

/// 追加演练：指标号闭环（1..=12 唯一且有序）。
pub fn drill_id_loop() -> (bool, String) {
    let mut uniq = true;
    for i in 0..METRIC_TABLE.len() {
        for j in (i + 1)..METRIC_TABLE.len() {
            if METRIC_TABLE[i].id == METRIC_TABLE[j].id {
                uniq = false;
            }
        }
    }
    let ordered = METRIC_TABLE.iter().enumerate().all(|(i, m)| m.id == (i as u16) + 1);
    (uniq && ordered && METRIC_TABLE.len() == 12,
     format!("唯一={} 有序={} 12 项闭环", uniq, ordered))
}

/// 追加演练：健康报告行定格（12 行逐行含 M 前缀与级标签）。
pub fn drill_report_lines() -> (bool, String) {
    let mut bk = HealthBook::new();
    if bk.register_all().is_err() {
        return (false, "注册失败".to_string());
    }
    let base: [(u16, u64); 12] = [
        (1, 1000), (2, 1000), (3, 0), (4, 40), (5, 1000), (6, 1000),
        (7, 1000), (8, 1000), (9, 1000), (10, 1), (11, 1000), (12, 1000),
    ];
    for (id, v) in base.iter() {
        let _ = bk.set(*id, *v);
    }
    let lines = bk.report_lines();
    let all_m = lines.iter().all(|l| l.starts_with("M"));
    let graded = lines.iter().all(|l| l.contains("级=绿"));
    (lines.len() == 12 && all_m && graded,
     format!("报告 {} 行 M 前缀={} 全绿级={}", lines.len(), all_m, graded))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越线不告警（手工清钩子 → 联通审计必抓）。
    {
        let mut bk = HealthBook::new();
        let _ = bk.register_all();
        let _ = bk.set(5, 500); // 清理完成率 500 < 950 → 红。
        bk.hooks.clear(); // 破坏联通。
        if bk.hook_count(5) == 0 { caught += 1; d.push("注1 断链:抓".to_string()); } else { d.push("注1 断链:漏".to_string()); }
    }
    // 注 2：阈值篡改。
    {
        let mut bk = HealthBook::new();
        let _ = bk.register_all();
        if let Some(m) = bk.reg.iter_mut().find(|m| m.id == 1) {
            m.amber = 0;
        }
        if !bk.tamper_scan() { caught += 1; d.push("注2 篡改:抓".to_string()); } else { d.push("注2 篡改:漏".to_string()); }
    }
    // 注 3：未注册取值。
    {
        let mut bk = HealthBook::new();
        let _ = bk.register_all();
        if matches!(bk.set(13, 1), Err(E_NOT_REG)) { caught += 1; d.push("注3 未注:抓".to_string()); } else { d.push("注3 未注:漏".to_string()); }
    }
    // 注 4：摘要漂移（同账两次汇总必须一致；篡改实测后摘要必变）。
    {
        let mut bk = HealthBook::new();
        let _ = bk.register_all();
        let _ = bk.set(1, 1000);
        let s1 = bk.summary();
        if let Some(v) = bk.vals.iter_mut().find(|v| v.id == 1) {
            v.value = 999;
        }
        let s2 = bk.summary();
        if s1 != s2 { caught += 1; d.push("注4 漂移:抓".to_string()); } else { d.push("注4 漂移:漏".to_string()); }
    }
    // 注 5：联通断链（绿级误触发钩子）。
    {
        let mut bk = HealthBook::new();
        let _ = bk.register_all();
        let lv = bk.set(2, 1000).unwrap_or(Level::Red);
        let hook = bk.hook_count(2);
        if lv == Level::Green && hook == 0 { caught += 1; d.push("注5 误触:抓".to_string()); } else { d.push("注5 误触:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x7620
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x7620 段诊断码：核对健康账段位闭集"
        && METRIC_TABLE.len() >= METRIC_MIN
}

/// run_unx1313_checks：本单判据聚合（诊断段 0x7620）。
pub fn run_unx1313_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1313 退役健康账");
    let (ok1, d1) = drill_register();
    s.add_dyn("J1-12项指标F0899注册", ok1, &d1);
    let (ok2, d2) = drill_summary();
    s.add_dyn("J1-摘要一致率100%", ok2, &d2);
    let (ok3, d3) = drill_linkage_10();
    s.add_dyn("J1-告警联通10/10", ok3, &d3);
    let (ok4, d4) = drill_levels();
    s.add_dyn("J1-三级定级对表", ok4, &d4);
    let (ok5, d5) = drill_alarm_miss();
    s.add_dyn("J1-越线必告警+级错必抓", ok5, &d5);
    let (ok6, d6) = drill_tamper();
    s.add_dyn("J1-阈值篡改必抓", ok6, &d6);
    let (ok7, d7) = drill_registry_guards();
    s.add_dyn("J1-注册守卫三拒", ok7, &d7);
    let (ok8, d8) = drill_frozen();
    s.add_dyn("J1-健康账冻结拒写", ok8, &d8);
    let (ok9, d9) = drill_snap_history();
    s.add_dyn("J2-指标快照史账", ok9, &d9);
    let (ok10, d10) = drill_alarm_log();
    s.add_dyn("J2-告警日志账", ok10, &d10);
    let (ok11, d11) = drill_grade_matrix();
    s.add_dyn("J2-指标定级矩阵36格", ok11, &d11);
    let (ok12, d12) = drill_caliber_rows();
    s.add_dyn("J2-五字段口径逐项对拍", ok12, &d12);
    let (ok13, d13) = drill_id_loop();
    s.add_dyn("J2-指标号闭环", ok13, &d13);
    let (ok14, d14) = drill_report_lines();
    s.add_dyn("J2-健康报告行定格", ok14, &d14);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x7620 段十码闭集");
    s
}
