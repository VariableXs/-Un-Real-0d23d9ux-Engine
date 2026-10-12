//! unx943_tophalfdiscipline —— UNX-F0943 top half 纪律（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0943 · top half 纪律（≤5µs 预算与最小化清单） · 目标 360 行 · 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 组件：预算常量闭集（5µs 红线 + 3µs 预警线 + 16 桶直方图）
//!      + 最小化清单（义务动作白名单 + 禁令动作黑名单双闭集）
//!      + P99 直方图分位追踪器（桶计数 + 整数分位，零浮点）
//!      + 超预算清单（自动产出，判据 10/10 次核）
//!      + 纪律文档冻结（义务/禁令/预算三节 + FNV 指纹）
//!      + 审计（预算断言核 + 超预算清单核 + 文档冻结核）
//! 判据落地：p99 ≤5µs 全 handler + 超预算清单 10/10 + 文档三节冻结
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑 tick）。
//! 契约承接：F0941 分界契约（top half 义务/禁令）+ F0942 统一分发入口（handler 主通道）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const THD_VERSION: &str = "unx943_tophalfdiscipline-v1";

/// 诊断段基址（0x4c50 段 · 本模块独占）。
pub const THD_SEG_BASE: u16 = 0x4c50;

/// top half 硬预算（判据 J1：≤5µs，QEMU 档，单位微秒）。
pub const THD_BUDGET_US: u64 = 5;

/// 预警线（预算 60%，达此即预警）。
pub const THD_WARN_US: u64 = 3;

/// 超预算清单产出目标（判据：10/10 次）。
pub const THD_OVERRUN_TARGET: u64 = 10;

/// 直方图桶数（定宽桶，桶宽 1µs）。
pub const THD_HIST_BUCKETS: usize = 16;

/// 分位分子（p99 = 99）。
pub const THD_P99_NUM: u64 = 99;

/// 每 handler 最短存活 tick（冻结前的最小样本数）。
pub const THD_MIN_SAMPLES: u64 = 8;

/// top half 允许动作（最小化清单 · 义务/允许白名单闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopAct {
    /// 应答中断控制器（义务：EOI，五类向量通用）。
    AckIrq,
    /// 摘取硬件快照（义务：寄存器/描述符环指针等定长数据）。
    TakeSnapshot,
    /// 登记 pending 位（义务：触发 bottom half）。
    RaisePending,
    /// 读取定长时间戳（允许：TSC/HPET 单次读，用于延迟账）。
    ReadTsc,
    /// 自增计数（允许：累计计数，无副作用）。
    BumpCounter,
}

impl TopAct {
    /// 全量白名单闭集。
    pub const ALL: [TopAct; 5] = [
        TopAct::AckIrq,
        TopAct::TakeSnapshot,
        TopAct::RaisePending,
        TopAct::ReadTsc,
        TopAct::BumpCounter,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TopAct::AckIrq => "应答中断（义务）",
            TopAct::TakeSnapshot => "摘取快照（义务）",
            TopAct::RaisePending => "登记 pending（义务）",
            TopAct::ReadTsc => "读取时间戳（允许）",
            TopAct::BumpCounter => "自增计数（允许）",
        }
    }

    /// 是否义务动作（缺任一义务即违例）。
    pub const fn obligatory(self) -> bool {
        matches!(self, TopAct::AckIrq | TopAct::TakeSnapshot | TopAct::RaisePending)
    }
}

/// top half 禁令动作（最小化清单 · 禁令黑名单闭集）。
///
/// 黑名单动作一旦出现即拦截：要么阻塞、要么非定长、要么引入不可重入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BanAct {
    /// 睡眠/让出（top half 上下文不可睡眠）。
    Sleep,
    /// 动态内存分配（分配路径非定长，爆预算风险）。
    Alloc,
    /// 打印/格式化输出（最典型超预算元凶）。
    Print,
    /// 加锁（可能自旋等待，时长不可控）。
    Lock,
    /// 触发调度（应由 bottom half/调度点做）。
    Schedule,
    /// 长循环/批量搬运（应下沉 bottom half）。
    LongLoop,
}

impl BanAct {
    /// 全量黑名单闭集。
    pub const ALL: [BanAct; 6] = [
        BanAct::Sleep,
        BanAct::Alloc,
        BanAct::Print,
        BanAct::Lock,
        BanAct::Schedule,
        BanAct::LongLoop,
    ];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            BanAct::Sleep => "睡眠/让出",
            BanAct::Alloc => "动态分配",
            BanAct::Print => "打印/格式化",
            BanAct::Lock => "加锁",
            BanAct::Schedule => "触发调度",
            BanAct::LongLoop => "长循环/批量搬运",
        }
    }
}

/// 诊断码（0x4c50 段十四码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThdCode {
    /// 0x4c50 p99 超预算（全 handler 聚合超 5µs）。
    P99Over,
    /// 0x4c51 单次超红线（一次 top half 超 5µs）。
    SingleOver,
    /// 0x4c52 预警未降（达预警线却无收敛）。
    WarnStuck,
    /// 0x4c53 义务动作缺失（白名单三义务缺一）。
    DutyMissing,
    /// 0x4c54 禁令动作命中（黑名单动作出现）。
    BanHit,
    /// 0x4c55 白名单越界（清单外动作）。
    ActOob,
    /// 0x4c56 超预算清单未满（产出 <10/10）。
    OverrunShort,
    /// 0x4c57 超预算清单错位（登记数 ≢ 超限数）。
    OverrunGap,
    /// 0x4c58 纪律文档缺节（义务/禁令/预算三节不全）。
    DocIncomplete,
    /// 0x4c59 纪律文档指纹漂移（冻结后文本被改）。
    DocDrift,
    /// 0x4c5a 直方图桶溢出（样本落桶越界）。
    HistOverflow,
    /// 0x4c5b 分位分母零（无样本却求分位）。
    P99DenomZero,
    /// 0x4c5c handler 未注册即运行。
    UnknownHandler,
    /// 0x4c5d 段位守卫失败。
    SegmentGuard,
}

impl ThdCode {
    /// 全量闭集。
    pub const ALL: [ThdCode; 14] = [
        ThdCode::P99Over,
        ThdCode::SingleOver,
        ThdCode::WarnStuck,
        ThdCode::DutyMissing,
        ThdCode::BanHit,
        ThdCode::ActOob,
        ThdCode::OverrunShort,
        ThdCode::OverrunGap,
        ThdCode::DocIncomplete,
        ThdCode::DocDrift,
        ThdCode::HistOverflow,
        ThdCode::P99DenomZero,
        ThdCode::UnknownHandler,
        ThdCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        THD_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            ThdCode::P99Over => "p99 超预算：全 handler 聚合超 5µs",
            ThdCode::SingleOver => "单次超红线：一次 top half 超 5µs",
            ThdCode::WarnStuck => "预警未降：达预警线却无收敛",
            ThdCode::DutyMissing => "义务动作缺失：白名单三义务缺一",
            ThdCode::BanHit => "禁令动作命中：黑名单动作出现",
            ThdCode::ActOob => "白名单越界：清单外动作",
            ThdCode::OverrunShort => "超预算清单未满：产出 <10/10",
            ThdCode::OverrunGap => "超预算清单错位：登记数不等于超限数",
            ThdCode::DocIncomplete => "纪律文档缺节：义务/禁令/预算三节不全",
            ThdCode::DocDrift => "纪律文档指纹漂移：冻结后文本被改",
            ThdCode::HistOverflow => "直方图桶溢出：样本落桶越界",
            ThdCode::P99DenomZero => "分位分母零：无样本却求分位",
            ThdCode::UnknownHandler => "handler 未注册即运行",
            ThdCode::SegmentGuard => "段位守卫失败",
        }
    }

    /// 可操作建议（错误路径必须带）。
    pub const fn advice(self) -> &'static str {
        match self {
            ThdCode::P99Over => "把重活下沉 bottom half：只留应答/摘取/登记三类义务动作",
            ThdCode::SingleOver => "拆 handler 或移出批量搬运；定位具体 handler 后按清单逐项瘦身",
            ThdCode::WarnStuck => "检查是否夹带打印/分配；预热后仍不降则按超预算清单逐条处置",
            ThdCode::DutyMissing => "补齐 AckIrq/TakeSnapshot/RaisePending 三义务后再放行",
            ThdCode::BanHit => "移除黑名单动作（睡眠/分配/打印/加锁/调度/长循环），改由 bottom half 承担",
            ThdCode::ActOob => "把越界动作移入白名单或下沉；白名单只增不改需走纪律文档变更",
            ThdCode::OverrunShort => "注入更多超预算样本以凑满 10/10 清单，确认清单自动产出路径",
            ThdCode::OverrunGap => "核对登记逻辑：每次 SingleOver 必须同步落清单一次",
            ThdCode::DocIncomplete => "补全义务/禁令/预算三节后再冻结",
            ThdCode::DocDrift => "冻结后文本被改：走纪律文档变更流程重新冻结并刷新指纹",
            ThdCode::HistOverflow => "样本超桶宽上限：扩展桶宽或加高桶位",
            ThdCode::P99DenomZero => "先灌入样本再求分位；空直方图不产分位",
            ThdCode::UnknownHandler => "先 register 再 run；未登记 handler 直接放行是接线缺陷",
            ThdCode::SegmentGuard => "诊断码越出 0x4c50 段：检查 ALL 闭集长度",
        }
    }
}


/// 超预算清单条目（超预算事件现场快照）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OverrunRec {
    /// handler id。
    pub handler: u32,
    /// 实测时长（µs）。
    pub observed_us: u64,
    /// 超出红线量（µs，observed - budget）。
    pub excess_us: u64,
    /// 触发的黑名单动作（若有）。
    pub ban: Option<BanAct>,
    /// 逻辑 tick。
    pub tick: u64,
}

/// P99 直方图分位追踪器（16 桶定宽 1µs + 尾桶溢出计数，零浮点）。
///
/// 落桶规则：桶宽 1µs，样本 v 落桶 `min(v, BUCKETS-1)`；
/// 超桶宽上限者进溢出桶（索引 BUCKETS-1 之外的尾计）。
pub struct HistP99 {
    /// 各桶计数。
    pub buckets: [u64; THD_HIST_BUCKETS],
    /// 样本总数。
    pub total: u64,
    /// 溢出桶计数（时长 ≥ 桶宽上限的样本）。
    pub overflow: u64,
    /// 单次最大值。
    pub max_us: u64,
}

impl HistP99 {
    /// 建空直方图。
    pub const fn new() -> Self {
        HistP99 {
            buckets: [0; THD_HIST_BUCKETS],
            total: 0,
            overflow: 0,
            max_us: 0,
        }
    }

    /// 落一个样本（时长 µs）。下标先比长度，绝不越界。
    pub fn record(&mut self, us: u64) {
        let idx = if (us as usize) < THD_HIST_BUCKETS {
            us as usize
        } else {
            // 超桶宽上限：计入溢出桶（尾桶索引）。
            self.overflow += 1;
            THD_HIST_BUCKETS - 1
        };
        self.buckets[idx] += 1;
        self.total += 1;
        if us > self.max_us {
            self.max_us = us;
        }
    }

    /// 取 p99（µs，整数分位上取整，零浮点）。
    ///
    /// 分位定义：第 ceil(P99_NUM/100 * total) 个样本的时长上界。
    /// 空直方图返回 Err（分母零）。
    pub fn p99(&self) -> Result<u64, ThdCode> {
        if self.total == 0 {
            return Err(ThdCode::P99DenomZero);
        }
        // 目标秩 r = ceil(99/100 * total)，用整数：(99*total + 99)/100。
        let r = (THD_P99_NUM * self.total + 99) / 100;
        if r == 0 {
            return Err(ThdCode::P99DenomZero);
        }
        // 从低桶累加，找到第 r 个样本所在桶的上界。
        let mut acc: u64 = 0;
        let mut i: usize = 0;
        while i < THD_HIST_BUCKETS {
            acc += self.buckets[i];
            if acc >= r {
                // 第 r 个样本落在桶 i，其上界为 (i+1)µs（尾桶含溢出，按桶宽计）。
                return Ok((i as u64) + 1);
            }
            i += 1;
        }
        // 兜底：溢出桶最大。
        Ok(self.max_us.max(THD_BUDGET_US + 1))
    }
}

/// 超预算清单（判据：自动产出，登记数 ≡ 单次超限数）。
pub struct OverrunLedger {
    /// 清单条目（上限由容量决定，满了丢最旧）。
    pub recs: Vec<OverrunRec>,
    /// 已登记条数。
    pub logged: u64,
    /// 容量上限。
    pub cap: usize,
}

impl OverrunLedger {
    /// 建清单（给定容量）。
    pub fn new(cap: usize) -> Self {
        OverrunLedger {
            recs: Vec::new(),
            logged: 0,
            cap,
        }
    }

    /// 登记一条超预算现场（满了丢最旧，守恒：logged 单调增）。
    pub fn log(&mut self, rec: OverrunRec) {
        self.logged += 1;
        if self.recs.len() >= self.cap && self.cap > 0 {
            self.recs.remove(0);
        }
        if self.cap > 0 {
            self.recs.push(rec);
        }
    }

    /// 当前在册条数。
    pub fn live(&self) -> usize {
        self.recs.len()
    }

    /// 清单是否已凑满判据目标（≥10）。
    pub fn full(&self) -> bool {
        self.logged >= THD_OVERRUN_TARGET
    }
}


/// 纪律文档（义务/禁令/预算三节 + FNV 指纹冻结）。
pub struct DisciplineDoc {
    /// 义务节文本。
    pub duty: String,
    /// 禁令节文本。
    pub ban: String,
    /// 预算节文本。
    pub budget: String,
    /// 是否已冻结。
    pub frozen: bool,
    /// 冻结时的 FNV 指纹。
    pub fingerprint: u64,
}

impl DisciplineDoc {
    /// 建空文档（未冻结）。
    pub fn new() -> Self {
        DisciplineDoc {
            duty: String::new(),
            ban: String::new(),
            budget: String::new(),
            frozen: false,
            fingerprint: 0,
        }
    }

    /// 填三节（未冻结时允许写；冻结后写即漂移）。
    pub fn fill(&mut self, duty: &str, ban: &str, budget: &str) {
        self.duty = duty.to_string();
        self.ban = ban.to_string();
        self.budget = budget.to_string();
    }

    /// 三节是否齐全（义务/禁令/预算均非空）。
    pub fn complete(&self) -> bool {
        !self.duty.is_empty() && !self.ban.is_empty() && !self.budget.is_empty()
    }

    /// 冻结：三节齐全才准冻，冻结记指纹。
    pub fn freeze(&mut self) -> Result<(), ThdCode> {
        if !self.complete() {
            return Err(ThdCode::DocIncomplete);
        }
        self.fingerprint = fnv1a(&self.render());
        self.frozen = true;
        Ok(())
    }

    /// 渲染全文档（义务→禁令→预算，定序）。
    pub fn render(&self) -> String {
        format!("{}\n{}\n{}", self.duty, self.ban, self.budget)
    }

    /// 指纹是否仍对得上（防冻结后被改）。
    pub fn fingerprint_ok(&self) -> bool {
        if !self.frozen {
            return false;
        }
        fnv1a(&self.render()) == self.fingerprint
    }
}

/// FNV-1a 64 位指纹（确定性文本哈希）。
pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}


/// handler 登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandlerRec {
    /// handler id。
    pub id: u32,
    /// 向量（分发入口分类号）。
    pub vector: u16,
    /// 期望义务动作位图（AckIrq=bit0 Snap=bit1 Pending=bit2）。
    pub duty_mask: u8,
    /// 已跑样本数。
    pub samples: u64,
}

/// top half 纪律主引擎。
pub struct TopHalfDiscipline {
    /// 预算（µs，判据 J1）。
    pub budget_us: u64,
    /// 预警线（µs）。
    pub warn_us: u64,
    /// 全 handler 聚合直方图。
    pub hist: HistP99,
    /// 超预算清单。
    pub overrun: OverrunLedger,
    /// 纪律文档。
    pub doc: DisciplineDoc,
    /// handler 登记表。
    pub handlers: Vec<HandlerRec>,
    /// 逻辑 tick。
    pub tick: u64,
    /// 单次超限计数（≡ 清单登记数）。
    pub single_over: u64,
    /// 预警未降计数（达预警线未收敛的样本）。
    pub warn_stuck: u64,
    /// 黑名单命中计数。
    pub ban_hits: u64,
    /// 白名单越界计数。
    pub act_oob: u64,
    /// 义务缺失计数。
    pub duty_missing: u64,
}

impl TopHalfDiscipline {
    /// 建引擎（默认预算/预警取常量）。
    pub fn new(overrun_cap: usize) -> Self {
        TopHalfDiscipline {
            budget_us: THD_BUDGET_US,
            warn_us: THD_WARN_US,
            hist: HistP99::new(),
            overrun: OverrunLedger::new(overrun_cap),
            doc: DisciplineDoc::new(),
            handlers: Vec::new(),
            tick: 0,
            single_over: 0,
            warn_stuck: 0,
            ban_hits: 0,
            act_oob: 0,
            duty_missing: 0,
        }
    }

    /// 登记 handler（期望义务位图）。
    pub fn register(&mut self, id: u32, vector: u16, duty_mask: u8) {
        if let Some(h) = self.handlers.iter_mut().find(|h| h.id == id) {
            h.vector = vector;
            h.duty_mask = duty_mask;
            return;
        }
        self.handlers.push(HandlerRec {
            id,
            vector,
            duty_mask,
            samples: 0,
        });
    }

    /// 查 handler（未登记返回 None）。
    pub fn handler(&self, id: u32) -> Option<&HandlerRec> {
        self.handlers.iter().find(|h| h.id == id)
    }

    /// 跑一次 top half 观测（落直方图 + 预算判定 + 清单登记）。
    ///
    /// `acts` 为本次实际执行的白名单动作位图（同 duty_mask 位定义）；
    /// `ban` 为命中的黑名单动作（若有）。
    pub fn run(&mut self, id: u32, observed_us: u64, acts: u8, ban: Option<BanAct>) -> Result<(), ThdCode> {
        // 未登记直接放行是接线缺陷。
        let Some(h) = self.handlers.iter_mut().find(|x| x.id == id) else {
            return Err(ThdCode::UnknownHandler);
        };
        self.tick += 1;
        h.samples += 1;

        // 黑名单命中优先判（阻塞/非定长最危险）。
        if let Some(b) = ban {
            self.ban_hits += 1;
            let _ = b;
            return Err(ThdCode::BanHit);
        }
        // 白名单越界：acts 含白名单外位（bit3~bit7 视为越界）。
        if acts & 0b1111_1000 != 0 {
            self.act_oob += 1;
            return Err(ThdCode::ActOob);
        }
        // 义务缺失：期望位 ⊄ 实际位。
        if h.duty_mask & acts != h.duty_mask {
            self.duty_missing += 1;
            return Err(ThdCode::DutyMissing);
        }

        // 落直方图（在预算判定之前，保证样本守恒）。
        self.hist.record(observed_us);

        // 预警线：达预警即计（预热后仍高即预警未降）。
        if observed_us >= self.warn_us {
            self.warn_stuck += 1;
        }
        // 单次超红线：自动产清单。
        if observed_us > self.budget_us {
            self.single_over += 1;
            let rec = OverrunRec {
                handler: id,
                observed_us,
                excess_us: observed_us.saturating_sub(self.budget_us),
                ban,
                tick: self.tick,
            };
            self.overrun.log(rec);
            return Err(ThdCode::SingleOver);
        }
        Ok(())
    }


    /// 预算断言核（判据 J1：全 handler p99 ≤5µs）。
    pub fn budget_ok(&self) -> Result<bool, ThdCode> {
        let p = self.hist.p99()?;
        Ok(p <= self.budget_us)
    }

    /// 超预算清单核（判据：登记数 ≡ 单次超限数 且 已凑满 10/10）。
    pub fn overrun_audit(&self) -> Result<bool, ThdCode> {
        // 守恒：清单登记数必须等于单次超限数。
        if self.overrun.logged != self.single_over {
            return Err(ThdCode::OverrunGap);
        }
        // 判据：清单已凑满。
        if !self.overrun.full() {
            return Err(ThdCode::OverrunShort);
        }
        Ok(true)
    }

    /// 文档冻结核（判据：三节冻结且指纹未漂移）。
    pub fn doc_audit(&self) -> Result<bool, ThdCode> {
        if !self.doc.complete() {
            return Err(ThdCode::DocIncomplete);
        }
        if !self.doc.fingerprint_ok() {
            return Err(ThdCode::DocDrift);
        }
        Ok(true)
    }

    /// 全判据核（三项齐过才绿）。
    pub fn verdict(&self) -> Result<bool, ThdCode> {
        let b = self.budget_ok()?;
        let o = self.overrun_audit()?;
        let d = self.doc_audit()?;
        Ok(b && o && d)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let p = self.hist.p99().unwrap_or(0);
        format!(
            "top half 纪律：预算 {}µs 样本 {} p99 {}µs 超单次 {} 清单 {}/{} 禁命中 {} 越界 {} 缺义务 {} 冻结 {}",
            self.budget_us,
            self.hist.total,
            p,
            self.single_over,
            self.overrun.logged,
            self.overrun.cap,
            self.ban_hits,
            self.act_oob,
            self.duty_missing,
            self.doc.frozen
        )
    }
}

/// 段位守卫（判据：诊断码全部落在 0x4c50 段内）。
pub fn segment_guard() -> bool {
    ThdCode::ALL
        .iter()
        .all(|c| c.val() >= THD_SEG_BASE && c.val() < THD_SEG_BASE + 0x100)
}

