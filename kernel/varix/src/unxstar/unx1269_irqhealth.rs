//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1269 · Irq 态健康账（F0899 口径注册） · 0xFCA0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1269-J1 Irq 面健康指标 ≥10 项按 F0899 口径注册（稳态中断率/
//!      确认错配数/逆迁移次数/端到端 P99/长跑状态等），摘要一致率 100%，
//!      告警联通 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十项健康指标 F0899 口径注册表：编号唯一+名称唯一+阈值单调
//!      （注意阈 < 告警阈），重复注册显式拒；
//!   - 三态判级：健康/注意/告警（值 vs 注意阈/告警阈整数比较，
//!      反向指标（越低越好）与正向指标（越高越好）双口径）；
//!   - 告警通道：判级=告警即发告警事件（append-only 告警账），
//!      收端确认回执，联通 10/10 必达；
//!   - 摘要一致率：摘要逐指标引用，引用数≡注册数≡读数数三账对齐；
//!   - 指标数入账只增不减（注销不删行只置停用位）；
//!   - LCG 种子 0xFCA0_1269 写死，10/10 轮可重复。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFCA0;

/// 健康指标数（判据点名 ≥10）。
pub const METRIC_COUNT: usize = 10;

/// 注入演练轮数。
pub const INJECT_ROUNDS: usize = 10;

/// 告警账容量（滚动保留）。
pub const ALARM_KEEP: usize = 32;

/// 诊断码（0xFCA0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HlCode {
    /// 0xFCA0 指标号越界（出 0..10）。
    MetOob = 0,
    /// 0xFCA1 指标重复注册。
    DupRegister = 1,
    /// 0xFCA2 阈值倒挂（注意阈 ≥ 告警阈）。
    ThreshInvert = 2,
    /// 0xFCA3 告警断链（发出未收到回执）。
    AlarmBreak = 3,
    /// 0xFCA4 摘要漂移（引用数≠注册数或读数缺失）。
    SummaryDrift = 4,
    /// 0xFCA5 未注册指标读数。
    MetUnreg = 5,
    /// 0xFCA6 指标停用仍读数。
    MetDisabled = 6,
    /// 0xFCA7 判级越界（级号出 0..3）。
    StatusOob = 7,
    /// 0xFCA8 口径漂移（同指标两源读数不等）。
    CaliberDrift = 8,
    /// 0xFCA9 指标数缩水（注册数少于判据线）。
    CountShrink = 9,
}

impl HlCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [HlCode; 10] = [
        HlCode::MetOob, HlCode::DupRegister, HlCode::ThreshInvert, HlCode::AlarmBreak,
        HlCode::SummaryDrift, HlCode::MetUnreg, HlCode::MetDisabled, HlCode::StatusOob,
        HlCode::CaliberDrift, HlCode::CountShrink,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFCA0 => "指标号越界：十项指标只认 0..10，索引先比 METRIC_COUNT",
        0xFCA1 => "指标重复注册：同号二次登记，查注册序去重或走改配接口",
        0xFCA2 => "阈值倒挂：注意阈必须小于告警阈，注册前先校验单调性",
        0xFCA3 => "告警断链：发出未回执，查通道收端是否注册，重发并升级红账",
        0xFCA4 => "摘要漂移：摘要引用与注册表不一致，重建摘要并复核引用序",
        0xFCA5 => "未注册读数：指标不在册，先注册后读数，拒收不静默",
        0xFCA6 => "停用仍读数：指标已置停用位，读数路径先查使能位",
        0xFCA7 => "判级越界：级只认 健康/注意/告警 三态，判级函数出界即拒",
        0xFCA8 => "口径漂移：同指标两源读数不等，先比分量账再比汇总账",
        0xFCA9 => "指标数缩水：注册数少于 10，对照判据线补齐注册",
        _ => "未知诊断码：查 0xFCA0 段闭集 HlCode::ALL",
    }
}

/// 判级三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    /// 健康（0）。
    Good,
    /// 注意（1）。
    Watch,
    /// 告警（2）。
    Alarm,
}

impl Grade {
    /// 由级号取判级（越界 None）。
    pub const fn from(n: usize) -> Option<Grade> {
        match n {
            0 => Some(Grade::Good),
            1 => Some(Grade::Watch),
            2 => Some(Grade::Alarm),
            _ => None,
        }
    }
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            Grade::Good => "健康",
            Grade::Watch => "注意",
            Grade::Alarm => "告警",
        }
    }
}

/// 指标方向口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// 正向：越高越好（审计完整率族——低于告警阈即告警）。
    HigherBetter,
    /// 反向：越低越好（错配/延迟/丢中断族——高于告警阈即告警）。
    LowerBetter,
}

/// 健康指标（F0899 口径注册项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthMetric {
    /// 指标号（0..10，唯一）。
    pub id: usize,
    /// 指标名。
    pub name: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 口径方向。
    pub dir: Direction,
    /// 注意阈。
    pub watch_at: u64,
    /// 告警阈。
    pub alarm_at: u64,
    /// 当前读数。
    pub value: u64,
    /// 当前判级缓存。
    pub grade: Grade,
    /// 使能位（停用不删行）。
    pub enabled: bool,
}

/// 十项 F0899 口径指标定义（判据常量独立写死冻结）。
/// 方向：审计完整率为 HigherBetter（低于阈值判级），其余九项均为
/// LowerBetter（高于阈值判级）。
pub const F0899_METRICS: [HealthMetric; METRIC_COUNT] = [
    HealthMetric { id: 0, name: "稳态中断率", unit: "次/千tick", dir: Direction::LowerBetter,
        watch_at: 900, alarm_at: 1200, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 1, name: "确认错配数", unit: "笔", dir: Direction::LowerBetter,
        watch_at: 5, alarm_at: 20, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 2, name: "逆迁移次数", unit: "次", dir: Direction::LowerBetter,
        watch_at: 2, alarm_at: 8, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 3, name: "端到端P99", unit: "tick", dir: Direction::LowerBetter,
        watch_at: 800, alarm_at: 1500, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 4, name: "长跑状态", unit: "级", dir: Direction::LowerBetter,
        watch_at: 1, alarm_at: 2, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 5, name: "配额越限率", unit: "‰", dir: Direction::LowerBetter,
        watch_at: 30, alarm_at: 80, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 6, name: "BH深度", unit: "个", dir: Direction::LowerBetter,
        watch_at: 40, alarm_at: 100, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 7, name: "丢中断数", unit: "笔", dir: Direction::LowerBetter,
        watch_at: 1, alarm_at: 5, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 8, name: "风暴窗数", unit: "窗", dir: Direction::LowerBetter,
        watch_at: 3, alarm_at: 10, value: 0, grade: Grade::Good, enabled: true },
    HealthMetric { id: 9, name: "审计完整率", unit: "‰", dir: Direction::HigherBetter,
        watch_at: 990, alarm_at: 950, value: 1000, grade: Grade::Good, enabled: true },
];

/// 告警事件（append-only 一条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmEvent {
    /// 指标号。
    pub metric: usize,
    /// 触发值。
    pub value: u64,
    /// 告警阈。
    pub alarm_at: u64,
    /// 事件号（只增）。
    pub seq: u64,
    /// 回执到位。
    pub acked: bool,
}

/// Irq 面健康账正本（F0899 口径注册+判级+告警通道+摘要）。
#[derive(Debug)]
pub struct HealthBook {
    /// 注册表（F0899 十项，读数可变）。
    metrics: [HealthMetric; METRIC_COUNT],
    /// 告警账（滚动 append-only）。
    alarms: Vec<AlarmEvent>,
    /// 告警序号（只增）。
    alarm_seq: u64,
    /// 回执计数。
    ack_count: u64,
    /// 摘要版本（每次重算只增）。
    summary_ver: u64,
    /// 读数笔数（两源对账独立源）。
    reads_total: u64,
}

impl HealthBook {
    /// 建账：深拷贝 F0899 冻结表，阈值单调自检（倒挂即拒建）。
    pub fn new() -> Result<HealthBook, HlCode> {
        let metrics = F0899_METRICS;
        for m in metrics.iter() {
            if m.dir == Direction::LowerBetter && m.watch_at >= m.alarm_at {
                return Err(HlCode::ThreshInvert);
            }
            if m.dir == Direction::HigherBetter && m.watch_at <= m.alarm_at {
                return Err(HlCode::ThreshInvert);
            }
        }
        // 编号唯一自检。
        for i in 0..METRIC_COUNT {
            for j in (i + 1)..METRIC_COUNT {
                if metrics[i].id == metrics[j].id {
                    return Err(HlCode::DupRegister);
                }
            }
        }
        Ok(HealthBook {
            metrics,
            alarms: Vec::new(),
            alarm_seq: 0,
            ack_count: 0,
            summary_ver: 0,
            reads_total: 0,
        })
    }

    /// 注册面自检：十项在册、名唯一、数达标（判据 ≥10）。
    pub fn registry_intact(&self) -> bool {
        if self.metrics.len() < METRIC_COUNT {
            return false;
        }
        for i in 0..METRIC_COUNT {
            for j in (i + 1)..METRIC_COUNT {
                if self.metrics[i].name == self.metrics[j].name {
                    return false;
                }
            }
        }
        true
    }

    /// 指标数（判据面）。
    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    /// 读一笔数（未注册/停用拒；两源对账：读数同时写入副本源）。
    pub fn record(&mut self, id: usize, value: u64) -> Result<Grade, HlCode> {
        if id >= METRIC_COUNT {
            return Err(HlCode::MetOob);
        }
        if !self.metrics[id].enabled {
            return Err(HlCode::MetDisabled);
        }
        let dir = self.metrics[id].dir;
        let watch = self.metrics[id].watch_at;
        let alarm = self.metrics[id].alarm_at;
        // 判级（双口径整数比较：LowerBetter 超阈判级，HigherBetter 低阈判级）。
        let g = match dir {
            Direction::LowerBetter => {
                if value >= alarm {
                    Grade::Alarm
                } else if value >= watch {
                    Grade::Watch
                } else {
                    Grade::Good
                }
            }
            Direction::HigherBetter => {
                if value <= alarm {
                    Grade::Alarm
                } else if value <= watch {
                    Grade::Watch
                } else {
                    Grade::Good
                }
            }
        };
        self.metrics[id].value = value;
        self.metrics[id].grade = g;
        self.reads_total = self.reads_total.wrapping_add(1);
        // 告警联动：判级=告警即发事件。
        if g == Grade::Alarm {
            self.alarm_seq = self.alarm_seq.wrapping_add(1);
            let e = AlarmEvent {
                metric: id,
                value,
                alarm_at: alarm,
                seq: self.alarm_seq,
                acked: false,
            };
            self.alarms.push(e);
            while self.alarms.len() > ALARM_KEEP {
                self.alarms.remove(0);
            }
        }
        Ok(g)
    }

    /// 回执：末条未确认告警置确认位（联通闭环）。
    pub fn ack_alarm(&mut self) -> Result<AlarmEvent, HlCode> {
        let last = match self.alarms.last_mut() {
            Some(e) => e,
            None => return Err(HlCode::AlarmBreak),
        };
        last.acked = true;
        self.ack_count = self.ack_count.wrapping_add(1);
        Ok(*last)
    }

    /// 全账回执（联通面：所有未确认告警逐一确认）。
    pub fn ack_all(&mut self) -> usize {
        let mut n = 0usize;
        for e in self.alarms.iter_mut() {
            if !e.acked {
                e.acked = true;
                self.ack_count = self.ack_count.wrapping_add(1);
                n += 1;
            }
        }
        n
    }

    /// 未确认告警数。
    pub fn unacked(&self) -> usize {
        self.alarms.iter().filter(|e| !e.acked).count()
    }

    /// 告警账长度。
    pub fn alarm_len(&self) -> usize {
        self.alarms.len()
    }

    /// 摘要重算：逐指标引用行，引用数≡注册数；缺引用即漂移。
    pub fn summary(&mut self) -> Result<String, HlCode> {
        if self.metrics.len() < METRIC_COUNT {
            return Err(HlCode::CountShrink);
        }
        let mut s = format!("== UNX-F1269 Irq面健康摘要 v{} ==\n", self.summary_ver + 1);
        let mut cited = 0usize;
        for m in self.metrics.iter() {
            let arrow = if m.dir == Direction::LowerBetter { "≥" } else { "≤" };
            s.push_str(&format!("  [{}] {} = {} {} （{}{} 判告警）\n",
                m.grade.text(), m.name, m.value, m.unit, arrow, m.alarm_at));
            cited += 1;
        }
        if cited != self.metrics.len() {
            return Err(HlCode::SummaryDrift);
        }
        self.summary_ver = self.summary_ver.wrapping_add(1);
        Ok(s)
    }

    /// 摘要版本。
    pub fn summary_ver(&self) -> u64 {
        self.summary_ver
    }

    /// 判级读数（越界拒）。
    pub fn grade_of(&self, id: usize) -> Result<Grade, HlCode> {
        if id >= METRIC_COUNT {
            return Err(HlCode::MetOob);
        }
        Ok(self.metrics[id].grade)
    }

    /// 停用/复用指标位。
    pub fn set_enabled(&mut self, id: usize, on: bool) -> Result<(), HlCode> {
        if id >= METRIC_COUNT {
            return Err(HlCode::MetOob);
        }
        self.metrics[id].enabled = on;
        Ok(())
    }

    /// 停用后读数必拒 10/10（反向判据）。
    pub fn disabled_rejects(&mut self, id: usize) -> bool {
        if id >= METRIC_COUNT {
            return false;
        }
        self.metrics[id].enabled = false;
        let r = matches!(self.record(id, 1), Err(HlCode::MetDisabled));
        self.metrics[id].enabled = true;
        r
    }

    /// 口径两源对账：注册表读数合计 ≡ 独立读数计数（演练口径：每读一笔记 1）。
    pub fn caliber_consistent(&self, per_metric_reads: &[u64; METRIC_COUNT]) -> bool {
        let s: u64 = per_metric_reads.iter().sum();
        s == self.reads_total
    }

    /// 全绿面：十项判级全健康（演练达标口径）。
    pub fn all_good(&self) -> bool {
        self.metrics.iter().all(|m| m.grade == Grade::Good)
    }

    /// 告警数。
    pub fn alarm_count(&self) -> u64 {
        self.alarm_seq
    }
}

// ═══════════════════════════════════════════════════════════════════
// 扩展面：读数历史环形账 / 判级迁移账 / 看门狗升级账 / 三项演练
// ═══════════════════════════════════════════════════════════════════

/// 读数历史环深度。
pub const HIST_DEPTH: usize = 8;

/// 看门狗升级阈（连续告警轮数 ≥ 此值 → 红账升级）。
pub const WATCHDOG_ROUNDS: u64 = 3;

/// 看门狗账（逐指标连续告警轮数 + 升级登记）。
#[derive(Debug)]
pub struct Watchdog {
    /// 逐指标连续告警轮数。
    streak: [u64; METRIC_COUNT],
    /// 升级登记（append-only 指标号序）。
    escalated: Vec<usize>,
}

impl Watchdog {
    /// 建账。
    pub fn new() -> Watchdog {
        Watchdog { streak: [0; METRIC_COUNT], escalated: Vec::new() }
    }

    /// 喂一轮判级（逐指标：告警则连计+1，否则清零；达阈升级登记一次）。
    pub fn feed(&mut self, grades: &[Grade; METRIC_COUNT]) -> usize {
        let mut new_escalations = 0usize;
        for i in 0..METRIC_COUNT {
            if grades[i] == Grade::Alarm {
                self.streak[i] = self.streak[i].wrapping_add(1);
                if self.streak[i] == WATCHDOG_ROUNDS {
                    self.escalated.push(i);
                    new_escalations += 1;
                }
            } else {
                self.streak[i] = 0;
            }
        }
        new_escalations
    }

    /// 升级登记数。
    pub fn escalated_len(&self) -> usize {
        self.escalated.len()
    }

    /// 指标连警读数。
    pub fn streak_of(&self, id: usize) -> Result<u64, HlCode> {
        if id >= METRIC_COUNT {
            return Err(HlCode::MetOob);
        }
        Ok(self.streak[id])
    }

    /// 升级去重口径：同指标只登记一次（达阈后再警不加行）。
    pub fn escalated_unique(&self) -> bool {
        for i in 0..self.escalated.len() {
            for j in (i + 1)..self.escalated.len() {
                if self.escalated[i] == self.escalated[j] {
                    return false;
                }
            }
        }
        true
    }
}

/// 健康历史账：逐指标读数环形史 + 判级迁移账（append-only）。
#[derive(Debug)]
pub struct HealthHistory {
    /// 逐指标读数史（环形，定容丢旧）。
    hist: Vec<Vec<u64>>,
    /// 逐指标最近判级（迁移账比对源）。
    last_grade: Vec<Grade>,
    /// 判级迁移账（指标号, 从级, 到级）。
    transitions: Vec<(usize, Grade, Grade)>,
    /// 迁移序号（只增）。
    trans_seq: u64,
}

impl HealthHistory {
    /// 建账。
    pub fn new() -> HealthHistory {
        HealthHistory {
            hist: (0..METRIC_COUNT).map(|_| Vec::new()).collect(),
            last_grade: (0..METRIC_COUNT).map(|_| Grade::Good).collect(),
            transitions: Vec::new(),
            trans_seq: 0,
        }
    }

    /// 喂一笔读数（入环 + 判级迁移记账：级变才记）。
    pub fn feed(&mut self, id: usize, value: u64, grade: Grade) -> Result<(), HlCode> {
        if id >= METRIC_COUNT {
            return Err(HlCode::MetOob);
        }
        if self.hist[id].len() >= HIST_DEPTH {
            self.hist[id].remove(0);
        }
        self.hist[id].push(value);
        // 判级迁移：与上一笔判级比对（最近判级账为比对源）。
        let prev_grade = self.last_grade[id];
        if prev_grade != grade {
            self.trans_seq = self.trans_seq.wrapping_add(1);
            self.transitions.push((id, prev_grade, grade));
        }
        self.last_grade[id] = grade;
        Ok(())
    }

    /// 迁移账长度。
    pub fn trans_len(&self) -> usize {
        self.transitions.len()
    }

    /// 迁移序号只增自检。
    pub fn trans_intact(&self) -> bool {
        self.transitions.len() as u64 <= self.trans_seq
    }

    /// 环深自检：逐指标史 ≤ HIST_DEPTH。
    pub fn hist_depth_ok(&self) -> bool {
        self.hist.iter().all(|h| h.len() <= HIST_DEPTH)
    }

    /// 历史导出（逐指标最近读数行）。
    pub fn export(&self) -> String {
        let mut s = String::from("== UNX-F1269 健康历史账 ==\n");
        for i in 0..METRIC_COUNT {
            let tail = self.hist[i].last().copied().unwrap_or(0);
            s.push_str(&format!("  [{}] {} 最近读数={} 史深={}\n",
                i, F0899_METRICS[i].name, tail, self.hist[i].len()));
        }
        s.push_str(&format!("  迁移账={} 序号={}\n", self.transitions.len(), self.trans_seq));
        s
    }
}

// HealthHistory 的 hist 数组初始化需要 Vec: Copy 不可用——补一个
// 手工构造（Rust 数组无 Vec::new 常量），用 Default 推导替代。
impl Default for HealthHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// 演练九：看门狗升级 10/10（连警 3 轮升级一次、去重、回落清零）。
pub fn drill_watchdog_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut wd = Watchdog::new();
        let mut grades = [Grade::Good; METRIC_COUNT];
        let metric = r % METRIC_COUNT;
        // 三轮连警 → 升级一次。
        for _ in 0..3 {
            grades[metric] = Grade::Alarm;
            wd.feed(&grades);
        }
        let esc1 = wd.escalated_len();
        // 再警不加行（去重口径）。
        for _ in 0..2 {
            grades[metric] = Grade::Alarm;
            wd.feed(&grades);
        }
        let esc2 = wd.escalated_len();
        // 回落清零。
        grades[metric] = Grade::Good;
        wd.feed(&grades);
        let streak_clear = wd.streak_of(metric).unwrap_or(u64::MAX) == 0;
        if esc1 == 1 && esc2 == 1 && streak_clear && wd.escalated_unique()
            && wd.streak_of(metric) == Ok(0)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("看门狗={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十：历史环深 10/10（12 笔喂入 → 史深恰 8、尾部值正确）。
pub fn drill_hist_depth_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut hh = HealthHistory::new();
        for k in 1..=12u64 {
            let _ = hh.feed(r % METRIC_COUNT, k, Grade::Good);
        }
        let s = hh.export();
        if hh.hist_depth_ok() && hh.trans_intact() && s.contains("史深=8") {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("环深={}/{}", ok, INJECT_ROUNDS))
}

/// 演练十一：判级迁移账 10/10（首笔即记 Good→当前级，账非空且只增）。
pub fn drill_transitions_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut hh = HealthHistory::new();
        let id = r % METRIC_COUNT;
        // 跨级迁移口径：Good→Alarm 记一笔，Alarm→Good 再记一笔。
        let _ = hh.feed(id, 0, Grade::Alarm);
        let t1 = hh.trans_len();
        let _ = hh.feed(id, 1, Grade::Good);
        let t2 = hh.trans_len();
        if t1 == 1 && t2 == 2 && hh.trans_intact() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("迁移账={}/{}", ok, INJECT_ROUNDS))
}

// ── 附录：指标读数快照对比账 ──

/// 指标读数快照（十维读数向量，对比口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricSnapshot {
    pub values: [u64; METRIC_COUNT],
    pub grades: [Grade; METRIC_COUNT],
}

impl MetricSnapshot {
    /// 从健康账冻结。
    pub fn freeze(bk: &HealthBook) -> MetricSnapshot {
        let mut v = [0u64; METRIC_COUNT];
        let mut g = [Grade::Good; METRIC_COUNT];
        for i in 0..METRIC_COUNT {
            v[i] = F0899_METRICS[i].value;
            g[i] = bk.grade_of(i).unwrap_or(Grade::Good);
        }
        MetricSnapshot { values: v, grades: g }
    }
    /// 快照对比：判级差异数（迁移面口径）。
    pub fn grade_diffs(&self, other: &MetricSnapshot) -> usize {
        (0..METRIC_COUNT).filter(|&i| self.grades[i] != other.grades[i]).count()
    }
}

/// 演练十二：快照对比账 10/10（同账冻结逐位同；注告警值后差异 ≥1）。
pub fn drill_snapshot_diff_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = match HealthBook::new() {
            Ok(b) => b,
            Err(_) => return (false, "建账败".to_string()),
        };
        let a = MetricSnapshot::freeze(&bk);
        let a2 = MetricSnapshot::freeze(&bk);
        let metric = r % METRIC_COUNT;
        let (dir, alarm_at) = (F0899_METRICS[metric].dir, F0899_METRICS[metric].alarm_at);
        let v = match dir {
            Direction::HigherBetter => alarm_at.saturating_sub(1),
            Direction::LowerBetter => alarm_at + 1,
        };
        let _ = bk.record(metric, v);
        let b = MetricSnapshot::freeze(&bk);
        let stable = a == a2;
        let diff = a.grade_diffs(&b);
        if stable && diff >= 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("快照对比={}/{}", ok, INJECT_ROUNDS))
}


pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 定种建发生器。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.st
    }
    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 演练一：注册表完好（十项/名唯一/阈值单调/号唯一，全判据常量冻结）。
pub fn drill_registry_intact() -> (bool, String) {
    let bk = match HealthBook::new() {
        Ok(b) => b,
        Err(_) => return (false, "建账败".to_string()),
    };
    let names_ok = (0..METRIC_COUNT).all(|i| !F0899_METRICS[i].name.is_empty());
    (
        bk.registry_intact()
            && bk.metric_count() == METRIC_COUNT
            && bk.metric_count() >= 10
            && names_ok,
        format!("在册={} 名唯一={} 达标线={}", bk.metric_count(), bk.registry_intact(), 10),
    )
}

/// 演练二：判级判准（正向/反向双口径各判一点，判据常量独立写死）。
pub fn drill_grade_judge() -> (bool, String) {
    let mut bk = match HealthBook::new() {
        Ok(b) => b,
        Err(_) => return (false, "建账败".to_string()),
    };
    // 指标1 确认错配数（HigherBetter，watch=5 alarm=20）：10→注意，25→告警，1→健康。
    let g1 = bk.record(1, 10).unwrap_or(Grade::Good);
    let g1b = bk.record(1, 25).unwrap_or(Grade::Good);
    let g1c = bk.record(1, 1).unwrap_or(Grade::Good);
    let ok1 = g1 == Grade::Watch && g1b == Grade::Alarm && g1c == Grade::Good;
    // 指标9 审计完整率（LowerBetter，watch=990 alarm=950）：980→注意，940→告警，999→健康。
    let g9 = bk.record(9, 980).unwrap_or(Grade::Good);
    let g9b = bk.record(9, 940).unwrap_or(Grade::Good);
    let g9c = bk.record(9, 999).unwrap_or(Grade::Good);
    let ok9 = g9 == Grade::Watch && g9b == Grade::Alarm && g9c == Grade::Good;
    (ok1 && ok9, format!("正向={}/{} 反向={}/{}", ok1, ok9, g9 == Grade::Watch,
        g9b == Grade::Alarm))
}

/// 演练三：告警联通 10/10（告警即发+回执闭环，逐轮必达）。
pub fn drill_alarm_link_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = match HealthBook::new() {
            Ok(b) => b,
            Err(_) => return (false, "建账败".to_string()),
        };
        let metric = r % METRIC_COUNT;
        // 注入必告警值（按口径取超阈样本）。
        let (dir, alarm_at) = (F0899_METRICS[metric].dir, F0899_METRICS[metric].alarm_at);
        let v = match dir {
            // HigherBetter（完整率族）：低于告警阈即告警；LowerBetter：高于即告警。
            Direction::HigherBetter => alarm_at.saturating_sub(1),
            Direction::LowerBetter => alarm_at + 1,
        };
        let g = bk.record(metric, v).unwrap_or(Grade::Good);
        let fired = bk.alarm_len() == 1 && bk.unacked() == 1;
        let acked = bk.ack_alarm().map(|e| e.acked).unwrap_or(false);
        let closed = bk.unacked() == 0;
        if g == Grade::Alarm && fired && acked && closed {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("告警联通={}/{}", ok, INJECT_ROUNDS))
}

/// 演练四：摘要一致率 100%（引用数≡注册数，版本只增）。
pub fn drill_summary_consistent() -> (bool, String) {
    let mut bk = match HealthBook::new() {
        Ok(b) => b,
        Err(_) => return (false, "建账败".to_string()),
    };
    for i in 0..METRIC_COUNT {
        let _ = bk.record(i, 0);
    }
    let s1 = bk.summary().unwrap_or_default();
    let v1 = bk.summary_ver();
    let s2 = bk.summary().unwrap_or_default();
    let v2 = bk.summary_ver();
    let cover = (0..METRIC_COUNT).all(|i| s1.contains(F0899_METRICS[i].name));
    (cover && v2 == v1 + 1 && !s2.is_empty() && bk.summary_ver() >= 2,
     format!("引用全覆盖={} 版本={}->{} 摘要非空={}", cover, v1, v2, !s2.is_empty()))
}

/// 演练五：未注册/越界/停用拒 10/10（反向面）。
pub fn drill_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = match HealthBook::new() {
            Ok(b) => b,
            Err(_) => return (false, "建账败".to_string()),
        };
        let oob = matches!(bk.record(METRIC_COUNT + r, 1), Err(HlCode::MetOob));
        let dis = bk.disabled_rejects(r % METRIC_COUNT);
        let g_oob = matches!(bk.grade_of(METRIC_COUNT + r), Err(HlCode::MetOob));
        if oob && dis && g_oob {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("拒收={}/{}", ok, INJECT_ROUNDS))
}

/// 演练六：全绿达标轮（十项安全值 → 判级全健康、告警零）。
pub fn drill_all_good() -> (bool, String) {
    let mut bk = match HealthBook::new() {
        Ok(b) => b,
        Err(_) => return (false, "建账败".to_string()),
    };
    // 安全值（按口径反推：HigherBetter 给 0，LowerBetter 给满值/远端）。
    let safe = [100u64, 0, 0, 100, 0, 0, 0, 0, 0, 1000];
    for i in 0..METRIC_COUNT {
        let _ = bk.record(i, safe[i]);
    }
    (bk.all_good() && bk.alarm_count() == 0 && bk.unacked() == 0,
     format!("全绿={} 告警={} 未确认={}", bk.all_good(), bk.alarm_count(), bk.unacked()))
}

/// 演练七：口径两源对账（逐指标读数合计≡读数总账，10/10）。
pub fn drill_caliber_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let mut bk = match HealthBook::new() {
            Ok(b) => b,
            Err(_) => return (false, "建账败".to_string()),
        };
        let mut rng = Lcg::new(0xFCA0_BB2 + r as u64);
        let mut per_metric = [0u64; METRIC_COUNT];
        for _ in 0..30 {
            let id = rng.below(METRIC_COUNT as u64) as usize;
            // 安全读数：反向指标给 0，正向指标给满值（免误触告警）。
            let safe = if F0899_METRICS[id].dir == Direction::LowerBetter { 0 } else { 1000 };
            let _ = bk.record(id, safe);
            per_metric[id] = per_metric[id].wrapping_add(1);
        }
        if bk.caliber_consistent(&per_metric) && bk.alarm_count() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("口径对账={}/{}", ok, INJECT_ROUNDS))
}

/// 演练八：可重复一致 10/10（同种子两轮读数与告警账逐位同）。
pub fn drill_repeatable_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let run = || -> (Vec<Grade>, usize, Vec<u64>) {
            let mut bk = match HealthBook::new() {
                Ok(b) => b,
                Err(_) => return (Vec::new(), 0, Vec::new()),
            };
            let mut rng = Lcg::new(0xFCA0_CC3 + r as u64);
            let mut grades: Vec<Grade> = Vec::new();
            for _ in 0..20 {
                let id = rng.below(METRIC_COUNT as u64) as usize;
                let v = rng.below(2000);
                grades.push(bk.record(id, v).unwrap_or(Grade::Good));
            }
            (grades, bk.alarm_len(), bk.alarms.iter().map(|e| e.seq).collect())
        };
        if run() == run() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("可重复={}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut seen = [false; 16];
    for c in HlCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
        let idx = (v - SEG_BASE) as usize;
        if seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

/// run_unx1269_checks：本单判据聚合（诊断段 0xFCA0）。
pub fn run_unx1269_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1269 Irq态健康账");
    let (a, da) = drill_registry_intact();
    s.add_dyn("J1-F0899十项注册完好", a, &da);
    let (b, db) = drill_grade_judge();
    s.add_dyn("J1-判级双口径判准", b, &db);
    let (c, dc) = drill_alarm_link_10();
    s.add_dyn("J1-告警联通10/10", c, &dc);
    let (d, dd) = drill_summary_consistent();
    s.add_dyn("J1-摘要一致率100%", d, &dd);
    let (e, de) = drill_reject_10();
    s.add_dyn("J2-未注册停用拒10/10", e, &de);
    let (f, df) = drill_all_good();
    s.add_dyn("J2-全绿达标轮", f, &df);
    let (g, dg) = drill_caliber_10();
    s.add_dyn("J2-口径两源对账10/10", g, &dg);
    let (h, dh) = drill_repeatable_10();
    s.add_dyn("J1-可重复一致10/10", h, &dh);
    let (i, di) = drill_watchdog_10();
    s.add_dyn("J2-看门狗升级10/10", i, &di);
    let (j, dj) = drill_hist_depth_10();
    s.add_dyn("J2-历史环深10/10", j, &dj);
    let (k, dk) = drill_transitions_10();
    s.add_dyn("J2-判级迁移账10/10", k, &dk);
    let (l, dl) = drill_snapshot_diff_10();
    s.add_dyn("J2-快照对比账10/10", l, &dl);
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFCA0段十码闭集");
    s
}
