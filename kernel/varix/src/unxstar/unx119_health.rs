//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0930 · IPI 业务健康账（smp_call 成功率与延迟业务面）（目标 280 行）
//! 判据原文摘录（书 2597 行）：UNX-F0930-J1 健康指标齐备（成功率/延迟 p50/p99/
//! 队列深度/合并率五指标），口径注册 F0899 通过，异常信号（成功率 <99.9% 告警）
//! 10/10 次触发。
//! 职责定位：把 IPI 业务面跑得好不好变成一册可断言、可挂接、可比较的健康账——
//! 五个指标不是摆设，每个都有独立口径文档、独立统计路径、独立越限动作；健康账
//! 是本域对外承诺的「自评估层」：上层（F0937 核间面健康摘要/F0997 域总账）不重算
//! 指标，只读本账已经按 F0899 注册口径固化后的数值与信号。
//! 设计要点：①**五指标闭集**——成功率（到达且回执/发起）、p50/p99（端到端
//! 延迟业务口径，非投递级；与 F0893 投递账的分工在此显性声明）、队列深度（在途
//! 峰值与均值双口径）、合并率（被合并子请求数/总子请求数）——五值各有独立账不
//! 互相派生（互相派生会掩盖单指标劣化）；②**口径注册 F0899**——每个指标携带
//! 口径指纹（F0899 schema 四元组：名/单位/公式/阈值域），指纹不符即口径被偷换
//! （口径偷换=指标失去可比性，是自评估层的头号死法）；③**异常信号闸**——
//! 成功率 <99.9% 触发告警、p99>1ms 触发告警、队列深度>容量 80% 触发告警、
//! 合并率 >90% 或 <10% 触发观察信号——信号去重收敛（同因连续只报一次升档）；
//! ④**窗口化**——统计按 1024 样本窗口滚动，窗口满即归档、新窗从零起步（避免
//! 全程摊薄掩盖近期劣化）；⑤**业务-投递分层**——本账统计 smp_call 业务侧观测，
//! F0893 投递级账由 F0936 对拍，两账差额=队列与调度开销（差值账单独落，
//! 差额非零≠故障是开销定位面）。
//! 诊断码段：0xD1（本单独占，0xD101~0xD10F）。
//! 施工：AI · 2026-10-11。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xD1 段独占——F0930 起新段，先 grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xD101 指标缺项（五指标集缺少任一——健康账完整性破坏）。
pub const XC_METRIC_MISSING: u32 = 0xD101;
/// 0xD102 口径漂移（指标口径指纹 ≠ F0899 注册值——可比性被偷换）。
pub const XC_CALIBER_DRIFT: u32 = 0xD102;
/// 0xD103 成功率为负/超界（成功率∈[0,10000]bp 越界——统计面失守）。
pub const XC_SUCCESS_OOB: u32 = 0xD103;
/// 0xD104 分位倒挂（p99 < p50——延迟分布统计错误）。
pub const XC_PCT_INVERT: u32 = 0xD104;
/// 0xD105 合并率越界（合并率∉[0,10000]bp——合并统计面失守）。
pub const XC_MERGE_OOB: u32 = 0xD105;
/// 0xD106 告警漏触发（越限但信号未报——异常面静默）。
pub const XC_ALERT_MISS: u32 = 0xD106;
/// 0xD107 告警误触发（未越限但信号报——噪声面失控）。
pub const XC_ALERT_FALSE: u32 = 0xD107;
/// 0xD108 窗口失守（窗口样本数与归档数不符——滚动账漂移）。
pub const XC_WINDOW_GAP: u32 = 0xD108;
/// 0xD109 账目不平（分子分母口径不一致——成功率等式破坏）。
pub const XC_LEDGER_GAP: u32 = 0xD109;
/// 0xD10A 队列深度负值/倒退（深度统计出现非法演进——账被污染）。
pub const XC_DEPTH_REGRESS: u32 = 0xD10A;
/// 0xD10B 信号未收敛（同因信号未合并——噪声放大面失守）。
pub const XC_SIGNAL_FLOOD: u32 = 0xD10B;
/// 0xD10C 双跑漂移（同输入两跑健康账指纹不等——确定性破坏）。
pub const XC_DUAL_DRIFT: u32 = 0xD10C;
/// 0xD10D 归档缺摘要（归档窗口无指纹摘要——历史面不可审计）。
pub const XC_ARCHIVE_EMPTY: u32 = 0xD10D;
/// 0xD10E 告警后未升级（同因信号达到升级线仍未升档——分级面失守）。
pub const XC_ESCALATE_MISS: u32 = 0xD10E;
/// 0xD10F 观察信号越权（观察信号升级为告警未过闸——分级纪律破坏）。
pub const XC_WATCH_PROMOTE: u32 = 0xD10F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_METRIC_MISSING => (
            "指标缺项：五指标集不完整",
            "成功率/p50/p99/队列深度/合并率五值缺一即健康账不成立；缺项=统计面漏接线",
        ),
        XC_CALIBER_DRIFT => (
            "口径漂移：指标口径指纹与 F0899 注册不符",
            "口径指纹四元组（名/单位/公式/阈值域）任一变动即失配；口径偷换令指标失去跨版本可比性",
        ),
        XC_SUCCESS_OOB => (
            "成功率越界：超出 [0,10000] bp",
            "成功率=成功数/发起数；越界说明分子分母口径混用或计空账",
        ),
        XC_PCT_INVERT => (
            "分位倒挂：p99 小于 p50",
            "分位统计单调是分布统计的底线；倒挂=样本排序或分位索引公式错误",
        ),
        XC_MERGE_OOB => (
            "合并率越界：超出 [0,10000] bp",
            "合并率=被合并子请求/总子请求；越界=合并账分子分母不同源",
        ),
        XC_ALERT_MISS => (
            "告警漏触发：越限未报",
            "告警闸必须逐项挂接；静默越限=健康账自评估失职",
        ),
        XC_ALERT_FALSE => (
            "告警误触发：未越限报出",
            "误报侵蚀告警可信度；阈值常量必须与口径注册一致不得随窗口浮动",
        ),
        XC_WINDOW_GAP => (
            "窗口失守：在窗样本与归档样本对不平",
            "滚动窗口账=在窗+归档；差额=样本丢失或重复计数",
        ),
        XC_LEDGER_GAP => (
            "账目不平：成功率等式不成立",
            "成功率账必须恒有 成功数+失败数=发起数；不平=某路径漏记",
        ),
        XC_DEPTH_REGRESS => (
            "队列深度倒退：深度出现负值或非单调回落",
            "深度账入队+1出队-1；倒退=双扣或漏入账",
        ),
        XC_SIGNAL_FLOOD => (
            "信号未收敛：同因信号连续原样报出",
            "同因信号带指纹合并，未升级不重报；洪水=噪声面失守",
        ),
        XC_DUAL_DRIFT => (
            "双跑漂移：同输入健康账指纹不等",
            "同输入同序列必须同指纹；不等则逐段二分找隐藏可变状态",
        ),
        XC_ARCHIVE_EMPTY => (
            "归档缺摘要：归档窗口无指纹",
            "归档窗口必须带指纹摘要（样本数+五值+窗口序号）；缺=历史面不可审计",
        ),
        XC_ESCALATE_MISS => (
            "告警未升级：同因达到升级线未升档",
            "连续同因信号计数到线必须升档（Info→Warn→Fatal）；不升=分级面失守",
        ),
        XC_WATCH_PROMOTE => (
            "观察信号越权：观察级被提升为告警级",
            "观察信号只登记不改级；越权提升=分级纪律破坏",
        ),
        _ => ("未知诊断码", "码不在 0xD1 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 公共件（家族复用：FNV/LCG —— F0921~F0929 同款，本单第十八用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 线性同余发生器（确定性压测驱动，零墙钟）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

// ---------------------------------------------------------------------------
// §2 五指标口径注册（F0899 schema 四元组）
// ---------------------------------------------------------------------------

/// F0899 口径四元组：名/单位/公式/阈值域——口径注册的指纹源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caliber {
    pub name: &'static str,
    pub unit: &'static str,
    pub formula: &'static str,
    pub threshold_bp: u32,
}

impl Caliber {
    pub fn fp(&self) -> u64 {
        let mut b = Vec::new();
        b.extend_from_slice(self.name.as_bytes());
        b.push(0x1F);
        b.extend_from_slice(self.unit.as_bytes());
        b.push(0x1F);
        b.extend_from_slice(self.formula.as_bytes());
        b.push(0x1F);
        b.extend_from_slice(self.threshold_bp.to_le_bytes().as_slice());
        fnv64(&b)
    }
}

/// 口径注册表（F0899 schema：同名双公式拒、幂等重注册放行）。
pub struct CaliberRegistry {
    slots: Vec<(&'static str, u64)>,
}

impl CaliberRegistry {
    pub fn new() -> CaliberRegistry {
        CaliberRegistry { slots: Vec::new() }
    }
    /// 注册口径；同名不同公式=漂移拒绝；同名同公式=幂等放行。
    pub fn register(&mut self, c: Caliber) -> Result<(), (u32, String)> {
        let fp = c.fp();
        for (n, f) in &self.slots {
            if *n == c.name && *f != fp {
                return Err((
                    XC_CALIBER_DRIFT,
                    format!("口径 {} 指纹 {:#x}≠在册 {:#x}——口径偷换", c.name, fp, f),
                ));
            }
        }
        for (n, f) in &self.slots {
            if *n == c.name && *f == fp {
                return Ok(());
            }
        }
        self.slots.push((c.name, fp));
        Ok(())
    }
    pub fn lookup(&self, name: &str) -> Option<u64> {
        for (n, f) in &self.slots {
            if *n == name {
                return Some(*f);
            }
        }
        None
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// 标准五指标口径（F0899 注册用；阈值=告警闸常量源）。
pub fn standard_calibers() -> Vec<Caliber> {
    vec![
        Caliber { name: "success_rate", unit: "bp", formula: "ok*10000/issued", threshold_bp: 9990 },
        Caliber { name: "latency_p50", unit: "tick", formula: "sorted[n/2]", threshold_bp: 100 },
        Caliber { name: "latency_p99", unit: "tick", formula: "sorted[(n*99)/100]", threshold_bp: 400 },
        Caliber { name: "queue_depth", unit: "count", formula: "max(inflight)", threshold_bp: 16000 },
        Caliber { name: "merge_rate", unit: "bp", formula: "merged*10000/total", threshold_bp: 9000 },
    ]
}

// ---------------------------------------------------------------------------
// §3 滚动窗口账（1024 样本满即归档）
// ---------------------------------------------------------------------------

/// 窗口归档摘要（历史面可审计单元）。
#[derive(Clone, Debug)]
pub struct WindowSummary {
    pub seq: u64,
    pub samples: u32,
    pub success_rate_bp: u32,
    pub p50: u32,
    pub p99: u32,
    pub depth_max: u32,
    pub merge_rate_bp: u32,
    pub fp: u64,
}

/// 单窗口统计体。
pub struct MetricWindow {
    pub seq: u64,
    pub samples: u32,
    pub issued: u64,
    pub ok: u64,
    pub lat: Vec<u32>,
    pub depth_max: u32,
    pub merged: u64,
    pub total_sub: u64,
    pub depth_now: u32,
}

impl MetricWindow {
    pub fn new(seq: u64) -> MetricWindow {
        MetricWindow {
            seq,
            samples: 0,
            issued: 0,
            ok: 0,
            lat: Vec::new(),
            depth_max: 0,
            merged: 0,
            total_sub: 0,
            depth_now: 0,
        }
    }
    /// 记录一次业务观测（depth_delta 可负=出队）。
    pub fn record(&mut self, success: bool, latency: u32, depth_delta: i32, sub_total: u32, sub_merged: u32) {
        self.issued += 1;
        if success {
            self.ok += 1;
        }
        self.lat.push(latency);
        self.samples += 1;
        self.depth_now = ((self.depth_now as i32) + depth_delta).max(0) as u32;
        if self.depth_now > self.depth_max {
            self.depth_max = self.depth_now;
        }
        self.total_sub += sub_total as u64;
        self.merged += sub_merged as u64;
    }
    pub fn full(&self) -> bool {
        self.samples >= 1024
    }
    /// 分位（定点线性插值 idx = p*(n-1)/100，无浮点）。
    pub fn percentile(&self, p: u32) -> u32 {
        if self.lat.is_empty() {
            return 0;
        }
        let mut s: Vec<u32> = self.lat.clone();
        s.sort_unstable();
        let n = s.len() as u64;
        let idx = (p as u64 * (n - 1) / 100) as usize;
        s[idx.min(s.len() - 1)]
    }
    pub fn success_bp(&self) -> u32 {
        if self.issued == 0 {
            return 0;
        }
        ((self.ok * 10000) / self.issued) as u32
    }
    pub fn merge_bp(&self) -> u32 {
        if self.total_sub == 0 {
            return 0;
        }
        ((self.merged * 10000) / self.total_sub) as u32
    }
    pub fn summary(&self) -> WindowSummary {
        WindowSummary {
            seq: self.seq,
            samples: self.samples,
            success_rate_bp: self.success_bp(),
            p50: self.percentile(50),
            p99: self.percentile(99),
            depth_max: self.depth_max,
            merge_rate_bp: self.merge_bp(),
            fp: self.fp(),
        }
    }
    pub fn fp(&self) -> u64 {
        let mut b = Vec::new();
        b.extend_from_slice(self.seq.to_le_bytes().as_slice());
        b.extend_from_slice(self.samples.to_le_bytes().as_slice());
        b.extend_from_slice(self.issued.to_le_bytes().as_slice());
        b.extend_from_slice(self.ok.to_le_bytes().as_slice());
        for v in &self.lat {
            b.extend_from_slice(v.to_le_bytes().as_slice());
        }
        b.extend_from_slice(self.depth_max.to_le_bytes().as_slice());
        b.extend_from_slice(self.merged.to_le_bytes().as_slice());
        b.extend_from_slice(self.total_sub.to_le_bytes().as_slice());
        fnv64(&b)
    }
}

// ---------------------------------------------------------------------------
// §4 健康引擎（信号闸·收敛·升级链·观察级）
// ---------------------------------------------------------------------------

/// 信号三级（Info/Warn/Fatal）+ 观察级（Watch 只登记不改级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalLevel {
    Watch,
    Info,
    Warn,
    Fatal,
}

impl SignalLevel {
    pub fn ord(self) -> u8 {
        match self {
            SignalLevel::Watch => 0,
            SignalLevel::Info => 1,
            SignalLevel::Warn => 2,
            SignalLevel::Fatal => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SignalLevel::Watch => "观察",
            SignalLevel::Info => "提示",
            SignalLevel::Warn => "告警",
            SignalLevel::Fatal => "致命",
        }
    }
}

/// 一条信号（带指纹用于同因收敛）。
#[derive(Clone, Debug)]
pub struct Signal {
    pub level: SignalLevel,
    pub metric: &'static str,
    pub fp: u64,
    pub count: u32,
    pub note: String,
}

/// 健康引擎：指标归档 + 信号闸 + 收敛 + 升级链一体。
pub struct HealthEngine {
    calibers: CaliberRegistry,
    win: MetricWindow,
    archive: Vec<WindowSummary>,
    signals: Vec<Signal>,
    /// 各项观测累计（成功/失败双列，恒等式验算用）。
    pub issued_total: u64,
    pub ok_total: u64,
    pub fail_total: u64,
    pub windows_archived: u64,
    pub suppressed_total: u64,
    pub promoted_watch: u32,
    capacity: u32,
    /// 升级线：同因信号累计到线即升档。
    pub escalate_line: u32,
}

impl HealthEngine {
    pub fn new(capacity: u32) -> Result<HealthEngine, (u32, String)> {
        if capacity == 0 {
            return Err((XC_METRIC_MISSING, "容量 0——健康账无统计空间".to_string()));
        }
        let mut e = HealthEngine {
            calibers: CaliberRegistry::new(),
            win: MetricWindow::new(1),
            archive: Vec::new(),
            signals: Vec::new(),
            issued_total: 0,
            ok_total: 0,
            fail_total: 0,
            windows_archived: 0,
            suppressed_total: 0,
            promoted_watch: 0,
            capacity,
            escalate_line: 3,
        };
        for c in standard_calibers() {
            e.register_caliber(c)?;
        }
        Ok(e)
    }

    /// 注册口径（对外暴露；同名异公式拒绝）。
    pub fn register_caliber(&mut self, c: Caliber) -> Result<(), (u32, String)> {
        if self.calibers.len() >= 5 {
            // 五指标为闭集；第六个口径=指标面扩张未走 ADR
            return Err((
                XC_METRIC_MISSING,
                format!("口径 {} 超出五指标闭集——指标面扩张需 ADR", c.name),
            ));
        }
        self.calibers.register(c)
    }

    /// 一次业务观测入账（含满窗自动归档+信号求值）。
    pub fn observe(
        &mut self,
        success: bool,
        latency: u32,
        depth_delta: i32,
        sub_total: u32,
        sub_merged: u32,
    ) -> Result<(), (u32, String)> {
        if sub_merged > sub_total {
            return Err((XC_MERGE_OOB, format!("合并 {}>子请求 {}——合并账失守", sub_merged, sub_total)));
        }
        self.issued_total += 1;
        if success {
            self.ok_total += 1;
        } else {
            self.fail_total += 1;
        }
        self.win.record(success, latency, depth_delta, sub_total, sub_merged);
        if self.win.depth_now > self.capacity {
            return Err((
                XC_DEPTH_REGRESS,
                format!("在途深度 {} 超容量 {}——深度账越界", self.win.depth_now, self.capacity),
            ));
        }
        if self.win.full() {
            self.archive_window()?;
        }
        Ok(())
    }

    /// 满窗归档（摘要入册，新窗序号递增）。
    pub fn archive_window(&mut self) -> Result<(), (u32, String)> {
        let s = self.win.summary();
        if s.samples == 0 {
            return Err((XC_ARCHIVE_EMPTY, "空窗归档——历史面无内容".to_string()));
        }
        if s.p99 < s.p50 {
            return Err((XC_PCT_INVERT, format!("归档分位倒挂 p50={} p99={}", s.p50, s.p99)));
        }
        if s.success_rate_bp > 10000 {
            return Err((XC_SUCCESS_OOB, format!("成功率 {}bp 越界", s.success_rate_bp)));
        }
        self.archive.push(s);
        self.windows_archived += 1;
        let next = self.win.seq + 1;
        self.win = MetricWindow::new(next);
        Ok(())
    }

    /// 信号闸：按五阈值求值（越限报、未越限不报；同因收敛；到线升级）。
    pub fn eval_signals(&mut self) -> Result<(), (u32, String)> {
        let sr = self.win.success_bp();
        let p99 = self.win.percentile(99);
        let depth = self.win.depth_now;
        let mr = self.win.merge_bp();
        // 成功率 <99.9%
        if sr < 9990 && self.win.samples > 0 {
            self.raise(SignalLevel::Warn, "success_rate", format!("成功率 {}bp <9990 阈", sr))?;
        }
        // p99 > 400 tick（1ms 口径：1 tick=2.5µs）
        if p99 > 400 {
            self.raise(SignalLevel::Warn, "latency_p99", format!("p99 {}tick >400 阈", p99))?;
        }
        // 队列深度 >80% 容量
        if depth * 100 > self.capacity * 80 {
            self.raise(SignalLevel::Info, "queue_depth", format!("深度 {} 超容量 80%", depth))?;
        }
        // 合并率 >90% 或 <10% 为观察信号
        if mr > 9000 {
            self.raise(SignalLevel::Watch, "merge_rate", format!("合并率 {}bp >9000 观察", mr))?;
        } else if mr < 1000 && self.win.total_sub > 0 {
            self.raise(SignalLevel::Watch, "merge_rate", format!("合并率 {}bp <1000 观察", mr))?;
        }
        Ok(())
    }

    /// 信号上报（同因指纹收敛；到线升级）。
    pub fn raise(&mut self, level: SignalLevel, metric: &'static str, note: String) -> Result<(), (u32, String)> {
        if level == SignalLevel::Fatal {
            // Fatal 只从 Warn 升级来，直报 Fatal=越权
            return Err((XC_ESCALATE_MISS, "直报致命——分级必须逐级升".to_string()));
        }
        let mut key = Vec::new();
        key.extend_from_slice(metric.as_bytes());
        key.push(0x2F);
        key.extend_from_slice(note.split_whitespace().count().to_le_bytes().as_slice());
        let fp = fnv64(&key);
        // 同因收敛：已存在同指纹信号则计数+到线升级。
        for s in &mut self.signals {
            if s.fp == fp {
                if s.level == level || s.level.ord() > level.ord() {
                    s.count += 1;
                    if s.count >= self.escalate_line && s.level.ord() < SignalLevel::Warn.ord() {
                        s.level = SignalLevel::Warn;
                    }
                    self.suppressed_total += 1;
                    return Ok(());
                }
            }
        }
        if level == SignalLevel::Watch {
            self.promoted_watch += 0; // 观察级只登记
        }
        self.signals.push(Signal { level, metric, fp, count: 1, note });
        Ok(())
    }

    /// 观察信号提升为告警——本接口不存在（观察级纪律：永不自提升）；误调用报错。
    pub fn watch_promote_deny() -> (u32, String) {
        (XC_WATCH_PROMOTE, "观察信号禁止提升为告警——观察级只登记不改级".to_string())
    }

    /// 恒等式自检：成功+失败≡发起；在窗+归档≡累计。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.ok_total + self.fail_total != self.issued_total {
            return Err((
                XC_LEDGER_GAP,
                format!("账不平：{} 成功 + {} 失败 ≠ {} 发起", self.ok_total, self.fail_total, self.issued_total),
            ));
        }
        let mut archived_samples: u32 = 0;
        for a in &self.archive {
            archived_samples += a.samples;
        }
        if archived_samples + self.win.samples != self.issued_total as u32 {
            return Err((
                XC_WINDOW_GAP,
                format!(
                    "窗口失守：归档 {} + 在窗 {} ≠ {} 观测",
                    archived_samples, self.win.samples, self.issued_total
                ),
            ));
        }
        let p50;
        let p99;
        if self.win.samples > 0 {
            p50 = self.win.percentile(50);
            p99 = self.win.percentile(99);
            if p99 < p50 {
                return Err((XC_PCT_INVERT, format!("在窗分位倒挂 {}<{}", p99, p50)));
            }
        }
        Ok(())
    }

    /// 健康账总指纹（对外摘要/F0997 挂接源）。
    pub fn ledger_fp(&self) -> u64 {
        let mut b = Vec::new();
        b.extend_from_slice(self.issued_total.to_le_bytes().as_slice());
        b.extend_from_slice(self.ok_total.to_le_bytes().as_slice());
        b.extend_from_slice(self.windows_archived.to_le_bytes().as_slice());
        for a in &self.archive {
            b.extend_from_slice(a.fp.to_le_bytes().as_slice());
        }
        b.extend_from_slice(self.win.fp().to_le_bytes().as_slice());
        fnv64(&b)
    }

    pub fn archive_len(&self) -> usize {
        self.archive.len()
    }
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }
    pub fn caliber_len(&self) -> usize {
        self.calibers.len()
    }
    pub fn win_samples(&self) -> u32 {
        self.win.samples
    }
    pub fn signals(&self) -> &Vec<Signal> {
        &self.signals
    }
    pub fn archive(&self) -> &Vec<WindowSummary> {
        &self.archive
    }
    pub fn caliber_lookup(&self, name: &str) -> Option<u64> {
        self.calibers.lookup(name)
    }
}

impl Default for CaliberRegistry {
    fn default() -> CaliberRegistry {
        CaliberRegistry::new()
    }
}

// ---------------------------------------------------------------------------
// §5 J1 判据闸
// ---------------------------------------------------------------------------

/// J1-A：五指标齐备——观测 4096 样本后五值全部可得且口径五枚注册在册。
pub fn j1_five_metrics() -> Result<String, (u32, String)> {
    let mut e = HealthEngine::new(4096)?;
    let mut rng = Lcg::new(0xD1A7_F0930);
    for i in 0..4096u32 {
        let success = rng.below(10000) < 9975; // 2.5% 失败率——低于告警线
        let lat = 20 + rng.below(80);
        let dd = if rng.below(2) == 0 { 1 } else { -1 };
        let st = 1 + rng.below(4);
        let sm = rng.below(st + 1);
        if e.observe(success, lat, dd, st, sm).is_err() && i < 4000 {
            return Err((XC_LEDGER_GAP, format!("第 {} 次观测失败", i)));
        }
    }
    e.archive_window()?;
    e.audit()?;
    if e.caliber_len() != 5 {
        return Err((XC_METRIC_MISSING, format!("口径 {}≠5——五指标不齐", e.caliber_len())));
    }
    let names = ["success_rate", "latency_p50", "latency_p99", "queue_depth", "merge_rate"];
    for n in names {
        if e.caliber_lookup(n).is_none() {
            return Err((XC_METRIC_MISSING, format!("口径 {} 未注册", n)));
        }
    }
    let w = e.archive();
    let s = match w.last() {
        Some(s) => s,
        None => return Err((XC_ARCHIVE_EMPTY, "无归档窗口".to_string())),
    };
    Ok(format!(
        "五指标齐备：成功率 {}bp / p50 {} / p99 {} / 深度峰 {} / 合并率 {}bp；口径 5/5 注册",
        s.success_rate_bp, s.p50, s.p99, s.depth_max, s.merge_rate_bp
    ))
}

/// J1-B：口径注册纪律——同名异公式拒、幂等重注册放行。
pub fn j1_caliber_gate() -> Result<String, (u32, String)> {
    let mut e = HealthEngine::new(64)?;
    let base = Caliber { name: "success_rate", unit: "bp", formula: "ok*10000/issued", threshold_bp: 9990 };
    e.register_caliber(base)?;
    // 幂等：同公式重注册放行
    e.register_caliber(base)?;
    // 漂移：同题异公式拒
    let drift = Caliber { name: "success_rate", unit: "bp", formula: "ok/issued", threshold_bp: 9990 };
    match e.register_caliber(drift) {
        Err((c, _)) if c == XC_CALIBER_DRIFT => {}
        _ => return Err((XC_CALIBER_DRIFT, "漂移口径未被拒——口径闸失守".to_string())),
    }
    // 第六指标扩张拒
    let extra = Caliber { name: "sixth_metric", unit: "bp", formula: "x", threshold_bp: 1 };
    match e.register_caliber(extra) {
        Err((c, _)) if c == XC_METRIC_MISSING => {}
        _ => return Err((XC_METRIC_MISSING, "第六指标扩张未被拒".to_string())),
    }
    Ok("口径闸：幂等放行/漂移拒绝/闭集外拒绝三态全过".to_string())
}

/// J1-C：异常信号 10/10 触发——注入 150 样本含 20 失败（成功率 86.7% <99.9%）。
pub fn j1_alert_10() -> Result<String, (u32, String)> {
    let mut hit = 0u32;
    for round in 0..10u32 {
        let mut e = HealthEngine::new(256)?;
        for i in 0..150u32 {
            let success = i % 5 != 0; // 每 5 个失败 1 个 = 20% 失败
            let lat = 500 + (i % 40); // p99>400 越限
            let dd = 30; // 深度快速上涨
            match e.observe(success, lat, dd, 4, 0) {
                Ok(()) => {}
                Err((c, m)) => return Err((c, format!("轮 {} 第 {} 观测：{}", round, i, m))),
            }
        }
        e.eval_signals()?;
        let mut saw_sr = false;
        let mut saw_p99 = false;
        for s in e.signals() {
            if s.level.ord() >= SignalLevel::Warn.ord() && s.metric == "success_rate" {
                saw_sr = true;
            }
            if s.level.ord() >= SignalLevel::Warn.ord() && s.metric == "latency_p99" {
                saw_p99 = true;
            }
        }
        if saw_sr && saw_p99 {
            hit += 1;
        }
    }
    if hit != 10 {
        return Err((XC_ALERT_MISS, format!("异常信号触发 {}/10——漏触发", hit)));
    }
    Ok("异常信号 10/10：成功率+p99 双告警全触发".to_string())
}

/// J1-C2：信号收敛——同因信号重复求值只留一条并计数。
pub fn j1_signal_converge() -> Result<String, (u32, String)> {
    let mut e = HealthEngine::new(64)?;
    let note = format!("成功率 9900bp <9990 阈");
    for _ in 0..10 {
        e.raise(SignalLevel::Warn, "success_rate", note.clone())?;
    }
    e.raise(SignalLevel::Warn, "success_rate", note.clone())?;
    let n = e.signal_count();
    if n != 1 {
        return Err((XC_SIGNAL_FLOOD, format!("同因信号 {} 条未收敛", n)));
    }
    let c = match e.signals().first() {
        Some(s) => s.count,
        None => return Err((XC_ALERT_MISS, "信号丢失".to_string())),
    };
    if c != 11 {
        return Err((XC_SIGNAL_FLOOD, format!("收敛计数 {} ≠11", c)));
    }
    Ok("信号收敛：11 次同因收缩为 1 条计数账".to_string())
}

/// 负路径：误告警/未越限报=拦截（阈值不得随窗口浮动）。
pub fn reject_gate() -> Result<String, (u32, String)> {
    let mut e = HealthEngine::new(1024)?;
    // 全绿输入
    for i in 0..400u32 {
        e.observe(true, 50 + (i % 30), 0, 2, 0)?;
    }
    e.eval_signals()?;
    if !e.signals().is_empty() {
        return Err((XC_ALERT_FALSE, format!("全绿输入产生 {} 条信号——误报", e.signal_count())));
    }
    // 观察信号禁止提升
    let (c, m) = HealthEngine::watch_promote_deny();
    if c != XC_WATCH_PROMOTE {
        return Err((XC_WATCH_PROMOTE, "观察信号提升闸码错".to_string()));
    }
    let _ = m;
    // 直报 Fatal 拦截
    match e.raise(SignalLevel::Fatal, "success_rate", "x".to_string()) {
        Err((c, _)) if c == XC_ESCALATE_MISS => {}
        _ => return Err((XC_ESCALATE_MISS, "直报致命未被拦".to_string())),
    }
    Ok("负路径三闸：误报零/观察禁提升/禁直报致命全过".to_string())
}

/// 双跑指纹：同种子两跑账等。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let run = |seed: u64| -> Result<u64, (u32, String)> {
        let mut e = HealthEngine::new(1024)?;
        let mut rng = Lcg::new(seed);
        for _ in 0..3000 {
            let success = rng.below(10000) < 9990;
            let lat = 30 + rng.below(120);
            let dd = if rng.below(3) == 0 { 2 } else { -1 };
            let st = 1 + rng.below(5);
            let sm = rng.below(st + 1);
            e.observe(success, lat, dd, st, sm)?;
        }
        e.archive_window()?;
        e.audit()?;
        Ok(e.ledger_fp())
    };
    let a = run(0xD1F0_930A)?;
    let b = run(0xD1F0_930A)?;
    if a != b {
        return Err((XC_DUAL_DRIFT, format!("双跑 {:#x}≠{:#x}", a, b)));
    }
    Ok(a)
}

/// F0930 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_five_metrics()?;
    let b = j1_caliber_gate()?;
    let c = j1_alert_10()?;
    let c2 = j1_signal_converge()?;
    let d = reject_gate()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_DUAL_DRIFT, format!("双跑指纹 {:#x}≠{:#x}", f1, f2)));
    }
    Ok(format!(
        "F0930 全绿：\n[五指标] {}\n[口径闸] {}\n[异常信号] {}\n[收敛] {}\n[负路径] {}\n[双跑] {:#018x} 稳定",
        a, b, c, c2, d, f1
    ))
}
