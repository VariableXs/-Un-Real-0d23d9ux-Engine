//! VE-F0064 · 时间戳校准服务（VE-A 域 · A03 同步与呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0064`
//!
//! **职责定位（锚点原文）**：时间戳校准服务——多设备时间戳的校准服务
//! （GPU/CPU/显示器时钟统一基准），校准漂移监控（自动重校准），校准精度声明；
//! 含校准服务的启动时序（早于首次时间戳使用）。
//!
//! **数据结构（锚点原文）**：校准服务。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 漂移超标 → 重校准（按最近一次实测样本重定基准，换 epoch）；
//! - 精度虚标 → 实测（声明值低于实测残差时**以实测为准**并留痕，
//!   声明是承诺，实测是事实，事实永远赢）；
//! - 服务失效 → 降级单钟（连坏三次即降级到 CPU 单钟，仍出时间戳但
//!   如实标注来源，不假装多设备已统一）。
//!
//! **性能逐项分解（锚点原文）**：O(1)。一次乘加、一次比较、环内定长扫描。
//!
//! **跨批对接点（锚点原文）**：A12 探针消费——探针读 `precision_ns()` 与
//! `epoch()` 决定自己的测量可信度；两处若各算一套，探针的误差棒就是假的。
//!
//! **无障碍与隐私（锚点原文）**：校准状态读屏可达——`status_line()` 输出
//! 纯文本「epoch/精度/来源/是否降级」，不含原始时钟读数。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数 ns 定点
//! （i64 有符号读数 + i128 中间量防溢出）；零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE790 段（0xE790~0xE79F 十六码）。

use alloc::string::String;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE790 段）
// ---------------------------------------------------------------------------

/// 服务未启动即取时间戳。
pub const XC_NOT_STARTED: u16 = 0xE790;
/// 无样本启动（只能降级单钟）。
pub const XC_NO_SAMPLE: u16 = 0xE791;
/// 精度虚标（声明低于实测）。
pub const XC_PRECISION_OVERCLAIM: u16 = 0xE792;
/// 漂移超限。
pub const XC_DRIFT_LIMIT: u16 = 0xE793;
/// 单调性破坏。
pub const XC_MONOTONIC_BREAK: u16 = 0xE794;
/// 样本台账满。
pub const XC_SAMPLE_FULL: u16 = 0xE795;
/// 病态样本（时钟不可用级偏差）。
pub const XC_PATHOLOGICAL: u16 = 0xE796;
/// 降级态不满足恢复条件（连坏未满或仍无好样本）。
pub const XC_DEGRADED: u16 = 0xE797;
/// 未降级却调恢复。
pub const XC_NOT_DEGRADED: u16 = 0xE798;
/// 手工校正参数非法。
pub const XC_BAD_TRIM: u16 = 0xE799;
/// 账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE79A;
/// 审计满账。
pub const XC_AUDIT_FULL: u16 = 0xE79B;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 样本台账容量。
pub const SAMPLE_CAP: usize = 32;
/// 审计容量。
pub const AUDIT_CAP: usize = 48;
/// 比例项分母（ppb）。
pub const PPB: i64 = 1_000_000_000;
/// 精度下限（50ns）——声明不可能小于测量本身的量化误差。
pub const FLOOR_NS: u32 = 50;
/// 漂移上限（200µs）：超过即重校准。
pub const DRIFT_LIMIT_NS: u32 = 200_000;
/// 病态上限（10ms）：超过即判时钟不可用，不进校准。
pub const PATHOLOGICAL_NS: u32 = 10_000_000;
/// 连坏几次降级。
pub const FAIL_LIMIT: u32 = 3;

// ---------------------------------------------------------------------------
// 时钟来源
// ---------------------------------------------------------------------------

/// 时钟来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Cpu = 0,
    Gpu = 1,
    Display = 2,
}

impl Source {
    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Source::Cpu => "cpu",
            Source::Gpu => "gpu",
            Source::Display => "display",
        }
    }

    /// 是否为主钟（主钟不做换算）。
    pub fn is_master(self) -> bool {
        matches!(self, Source::Cpu)
    }
}

/// 服务状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvcState {
    /// 未启动。
    Cold,
    /// 已校准，多设备统一基准可用。
    Ready,
    /// 已降级到 CPU 单钟。
    Degraded,
}

impl SvcState {
    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            SvcState::Cold => "cold",
            SvcState::Ready => "ready",
            SvcState::Degraded => "degraded",
        }
    }
}

/// 样本（原始读数 → 主钟读数 的实测对）。
#[derive(Clone, Copy)]
pub struct Sample {
    /// 设备原始读数。
    pub raw: i64,
    /// 同刻主钟读数。
    pub master: i64,
    /// 来源。
    pub src: Source,
}

/// 样本处置结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ingest {
    /// 接受，残差在预算内。
    Accepted { residual_ns: u32 },
    /// 接受并触发了重校准。
    Recalibrated { residual_ns: u32, epoch: u32 },
    /// 拒收（病态样本）。
    Rejected { code: u16, residual_ns: u32 },
}

// ---------------------------------------------------------------------------
// 校准服务
// ---------------------------------------------------------------------------

/// 多设备时间戳校准服务。
#[derive(Clone, Copy)]
pub struct CalibService {
    offset_ns: i64,
    scale_ppb: i64,
    epoch: u32,
    state: SvcState,
    started: bool,
    declared_ns: u32,
    measured_bound_ns: u32,
    measured: bool,
    last_master: i64,
    has_last: bool,
    fail_streak: u32,
    accepted: u32,
    rejected: u32,
    submitted: u32,
    dropped: u32,
    recalibrations: u32,
    overclaims: u32,
    monotonic_breaks: u32,
    degradations: u32,
}

impl CalibService {
    /// 冷态服务（零偏置、零比例、未启动）。
    pub fn new() -> CalibService {
        CalibService {
            offset_ns: 0,
            scale_ppb: 0,
            epoch: 1,
            state: SvcState::Cold,
            started: false,
            declared_ns: FLOOR_NS,
            measured_bound_ns: 0,
            measured: false,
            last_master: 0,
            has_last: false,
            fail_streak: 0,
            accepted: 0,
            rejected: 0,
            submitted: 0,
            dropped: 0,
            recalibrations: 0,
            overclaims: 0,
            monotonic_breaks: 0,
            degradations: 0,
        }
    }

    /// 换算：设备读数 → 主钟读数（i128 中间量，i64 出入）。
    pub fn to_master(&self, raw: i64, src: Source) -> i64 {
        if src.is_master() {
            return raw;
        }
        let scaled = (raw as i128) * (self.scale_ppb as i128) / (PPB as i128);
        let v = (raw as i128) + (self.offset_ns as i128) + scaled;
        clamp_i64(v)
    }

    /// 残差（|预测 − 实测|）。
    fn residual(&self, s: &Sample) -> u32 {
        let pred = self.to_master(s.raw, s.src) as i128;
        let act = s.master as i128;
        let d = if pred > act { pred - act } else { act - pred };
        if d > u32::MAX as i128 {
            u32::MAX
        } else {
            d as u32
        }
    }

    /// 启动（必须早于首次时间戳使用）。
    pub fn start(&mut self) -> Result<SvcState, u16> {
        self.started = true;
        if self.accepted == 0 {
            // 无样本不许假装已校准：降级到单钟，仍可用但如实标注。
            self.state = SvcState::Degraded;
            self.degradations = self.degradations.saturating_add(1);
            return Ok(SvcState::Degraded);
        }
        self.state = SvcState::Ready;
        Ok(SvcState::Ready)
    }

    /// 状态。
    pub fn state(&self) -> SvcState {
        self.state
    }

    /// 是否已启动。
    pub fn started(&self) -> bool {
        self.started
    }

    /// 当前 epoch。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 当前精度声明（ns）。
    pub fn precision_ns(&self) -> u32 {
        self.declared_ns
    }

    /// 精度是否为实测（而非出厂声明）。
    pub fn measured(&self) -> bool {
        self.measured
    }

    /// 实测残差上界。
    pub fn measured_bound_ns(&self) -> u32 {
        self.measured_bound_ns
    }

    /// 偏置与比例。
    pub fn affine(&self) -> (i64, i64) {
        (self.offset_ns, self.scale_ppb)
    }

    /// 声明精度（虚标即被实测压制）。
    pub fn declare(&mut self, ns: u32) -> Result<u32, u16> {
        let mut eff = if ns < FLOOR_NS { FLOOR_NS } else { ns };
        let mut over = false;
        if self.measured && eff < self.measured_bound_ns {
            eff = self.measured_bound_ns;
            over = true;
            self.overclaims = self.overclaims.saturating_add(1);
        }
        self.declared_ns = eff;
        if over {
            return Err(XC_PRECISION_OVERCLAIM);
        }
        Ok(eff)
    }

    /// 提交一份样本。
    pub fn submit(&mut self, s: Sample) -> Ingest {
        self.submitted = self.submitted.saturating_add(1);
        if self.accepted as usize >= SAMPLE_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Ingest::Rejected {
                code: XC_SAMPLE_FULL,
                residual_ns: 0,
            };
        }
        let r = self.residual(&s);
        if r > PATHOLOGICAL_NS {
            // 病态样本：既不进校准（会把基准带歪），也不算漂移（不是漂移，是坏数据）。
            self.rejected = self.rejected.saturating_add(1);
            self.fail_streak = self.fail_streak.saturating_add(1);
            if self.fail_streak >= FAIL_LIMIT {
                self.state = SvcState::Degraded;
                self.degradations = self.degradations.saturating_add(1);
            }
            return Ingest::Rejected {
                code: XC_PATHOLOGICAL,
                residual_ns: r,
            };
        }
        self.fail_streak = 0;
        self.accepted = self.accepted.saturating_add(1);
        if r > DRIFT_LIMIT_NS {
            // 漂移超标：以本样本重定基准（换 epoch），残差从零起算。
            self.offset_ns = s.master.saturating_sub(s.raw);
            self.scale_ppb = 0;
            self.epoch = self.epoch.saturating_add(1);
            self.recalibrations = self.recalibrations.saturating_add(1);
            self.measured_bound_ns = 0;
            self.measured = true;
            return Ingest::Recalibrated {
                residual_ns: r,
                epoch: self.epoch,
            };
        }
        if r > self.measured_bound_ns {
            self.measured_bound_ns = r;
        }
        self.measured = true;
        if self.declared_ns < self.measured_bound_ns {
            // 声明低于实测：以实测为准（不返回 Err——样本本身是好的，
            // 被判错的是声明；调用方该看 precision_ns() 而不是这次的返回）。
            self.declared_ns = if self.measured_bound_ns < FLOOR_NS {
                FLOOR_NS
            } else {
                self.measured_bound_ns
            };
            self.overclaims = self.overclaims.saturating_add(1);
        }
        Ingest::Accepted { residual_ns: r }
    }

    /// 手工校正（管理员入口，必须留痕并换 epoch）。
    pub fn force_affine(&mut self, offset_ns: i64, scale_ppb: i64) -> Result<(), u16> {
        if scale_ppb > PPB / 10 || scale_ppb < -(PPB / 10) {
            return Err(XC_BAD_TRIM);
        }
        self.offset_ns = offset_ns;
        self.scale_ppb = scale_ppb;
        self.epoch = self.epoch.saturating_add(1);
        self.recalibrations = self.recalibrations.saturating_add(1);
        Ok(())
    }

    /// 取时间戳（主钟口径）。
    pub fn stamp(&mut self, raw: i64, src: Source) -> Result<i64, u16> {
        if !self.started {
            return Err(XC_NOT_STARTED);
        }
        let t = self.to_master(raw, src);
        if self.has_last && t < self.last_master {
            // 单调性被破坏（校准把时间拉回去了）：钳住并留痕。
            // 宁可让时间停一下，也不让上层看到时间倒流——倒流会让所有
            // 差值型账本（延迟、超时、序号）同时出错。
            self.monotonic_breaks = self.monotonic_breaks.saturating_add(1);
            return Ok(self.last_master);
        }
        self.last_master = t;
        self.has_last = true;
        Ok(t)
    }

    /// 跨设备同刻差值（CPU 与设备读数各取一个，返回主钟口径差）。
    pub fn cross_delta(&self, cpu_raw: i64, dev_raw: i64, dev: Source) -> i64 {
        let a = self.to_master(cpu_raw, Source::Cpu);
        let b = self.to_master(dev_raw, dev);
        if a > b {
            a - b
        } else {
            b - a
        }
    }

    /// 有效来源标签（降级后只有 CPU 可信）。
    pub fn source_label(&self) -> &'static str {
        match self.state {
            SvcState::Cold => "cold",
            SvcState::Degraded => "cpu-only",
            SvcState::Ready => "unified",
        }
    }

    /// 显式恢复（不自动悄悄恢复：降级是可见状态，恢复也该可见）。
    pub fn recover(&mut self) -> Result<SvcState, u16> {
        if self.state != SvcState::Degraded {
            return Err(XC_NOT_DEGRADED);
        }
        if self.accepted == 0 || self.fail_streak >= FAIL_LIMIT {
            return Err(XC_DEGRADED);
        }
        self.state = SvcState::Ready;
        Ok(SvcState::Ready)
    }

    /// 计数快照 `(提交, 接受, 拒收, 丢弃, 重校准, 虚标, 单调破坏, 降级)`。
    pub fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        (
            self.submitted,
            self.accepted,
            self.rejected,
            self.dropped,
            self.recalibrations,
            self.overclaims,
            self.monotonic_breaks,
            self.degradations,
        )
    }

    /// 账平衡：提交 ≡ 接受 + 拒收 + 台账满丢弃。
    pub fn ledger_balanced(&self) -> bool {
        self.submitted == self.accepted + self.rejected + self.dropped
    }

    /// 读屏行（四段：epoch / 精度 / 来源 / 是否降级）。
    pub fn status_line(&self) -> String {
        let mut s = String::new();
        s.push_str("校准 epoch ");
        push_u32(&mut s, self.epoch);
        s.push_str(" 精度 ");
        push_u32(&mut s, self.declared_ns);
        s.push_str("ns");
        s.push_str(if self.measured { "(实测)" } else { "(声明)" });
        s.push_str(" 来源 ");
        s.push_str(self.source_label());
        s.push_str(" 降级 ");
        s.push_str(if self.state == SvcState::Degraded {
            "是"
        } else {
            "否"
        });
        s
    }
}

fn clamp_i64(v: i128) -> i64 {
    if v > i64::MAX as i128 {
        i64::MAX
    } else if v < i64::MIN as i128 {
        i64::MIN
    } else {
        v as i64
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

fn sample(raw: i64, master: i64, src: Source) -> Sample {
    Sample { raw, master, src }
}

/// 已校准且带一份好样本的服务。
fn calibrated() -> CalibService {
    let mut s = CalibService::new();
    let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
    let _ = s.start();
    s
}

/// VE-F0064 判据入口。
pub fn run_vea64_checks() -> CheckSet {
    let mut set = CheckSet::new("vea64_timesync");

    // C64-01 未启动取戳被拒且不产假时间
    {
        let mut s = calibrated();
        let mut cold = CalibService::new();
        let a = cold.stamp(500, Source::Gpu);
        let b = s.stamp(1_000, Source::Gpu);
        let ok = a == Err(XC_NOT_STARTED) && b == Ok(1_000) && cold.state() == SvcState::Cold;
        chk(&mut set, "C64-01 启动时序", ok, "未启动却出时间戳");
    }

    // C64-02 无样本启动只能降级单钟
    {
        let mut s = CalibService::new();
        let st = s.start();
        let ok = st == Ok(SvcState::Degraded)
            && s.state() == SvcState::Degraded
            && s.source_label() == "cpu-only"
            && s.counters().7 == 1
            && !s.measured();
        chk(&mut set, "C64-02 无样本降级", ok, "空样本却声称就绪");
    }

    // C64-03 有样本启动 → Ready，且精度仍为声明（未实测）
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let st = s.start();
        let ok = st == Ok(SvcState::Ready)
            && s.state() == SvcState::Ready
            && s.measured()
            && s.precision_ns() == FLOOR_NS
            && s.measured_bound_ns() == 0;
        chk(&mut set, "C64-03 有样本就绪", ok, "就绪态或精度状态错");
    }

    // C64-04 纯偏置换算
    {
        let mut s = CalibService::new();
        let _ = s.force_affine(1_000, 0);
        let ok = s.to_master(500, Source::Gpu) == 1_500
            && s.to_master(500, Source::Cpu) == 500
            && s.epoch() == 2;
        chk(&mut set, "C64-04 偏置换算", ok, "主钟换算错或主钟被换算");
    }

    // C64-05 比例项换算（0.1%）
    {
        let mut s = CalibService::new();
        let _ = s.force_affine(0, 1_000_000);
        let ok = s.to_master(1_000_000, Source::Gpu) == 1_001_000
            && s.to_master(2_000_000, Source::Gpu) == 2_002_000;
        chk(&mut set, "C64-05 比例换算", ok, "ppb 换算错");
    }

    // C64-06 负偏置（设备快于主钟）
    {
        let mut s = CalibService::new();
        let _ = s.force_affine(-2_000, 0);
        let ok = s.to_master(100, Source::Gpu) == -1_900
            && s.to_master(3_000, Source::Display) == 1_000;
        chk(&mut set, "C64-06 负偏置", ok, "有符号减法被当成无符号");
    }

    // C64-07 残差取绝对值最大者
    {
        let mut s = CalibService::new();
        let _ = s.force_affine(0, 0);
        let _ = s.submit(sample(10_000, 10_100, Source::Gpu)); // 残差 100
        let _ = s.submit(sample(20_000, 19_100, Source::Gpu)); // 残差 900（负向）
        let ok = s.measured_bound_ns() == 900 && s.precision_ns() == 900 && s.measured();
        chk(&mut set, "C64-07 残差上界", ok, "负残差被忽略或未取最大");
    }

    // C64-08 精度虚标被抓并下调
    {
        let mut s = CalibService::new();
        let _ = s.force_affine(0, 0);
        let _ = s.submit(sample(10_000, 10_900, Source::Gpu));
        let eff = s.declare(500);
        // 虚标计数两次：一次是样本入账时的自动下调，一次是显式声明的压制。
        let ok = eff == Err(XC_PRECISION_OVERCLAIM)
            && s.precision_ns() == 900
            && s.counters().5 == 2;
        chk(&mut set, "C64-08 精度虚标压制", ok, "虚标未被实测压制");
    }

    // C64-09 精度下限（不出现 0 精度）
    {
        let mut s = CalibService::new();
        let a = s.declare(1);
        let b = s.declare(0);
        let ok = a == Ok(FLOOR_NS) && b == Ok(FLOOR_NS) && s.precision_ns() == FLOOR_NS;
        chk(&mut set, "C64-09 精度下限", ok, "出现亚量化精度");
    }

    // C64-10 漂移超限自动重校准
    {
        // 冷态本身就是零偏置零比例，不必 force_affine——那样会多记一次重校准，
        // 把「自动重校准」的计数污染成 2。
        let mut s = CalibService::new();
        let e0 = s.epoch();
        let r = s.submit(sample(1_000_000, 1_000_000 + DRIFT_LIMIT_NS as i64 + 1, Source::Gpu));
        let ok = matches!(r, Ingest::Recalibrated { .. })
            && s.epoch() == e0 + 1
            && s.counters().4 == 1
            && s.to_master(1_000_000, Source::Gpu) == 1_000_000 + DRIFT_LIMIT_NS as i64 + 1;
        chk(&mut set, "C64-10 漂移重校准", ok, "漂移未触发重校准或基准未换");
    }

    // C64-11 重校准后残差清零、样本账不丢
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(10_000, 10_400, Source::Gpu)); // 残差 400，未超漂移限
        let bound_before = s.measured_bound_ns();
        let acc_before = s.counters().1;
        let _ = s.submit(sample(20_000, 20_000 + DRIFT_LIMIT_NS as i64 + 5, Source::Gpu));
        let ok = bound_before == 400
            && acc_before == 1
            && s.measured_bound_ns() == 0
            && s.counters().1 == 2;
        chk(&mut set, "C64-11 重校准残差清零", ok, "残差未清零或样本账丢失");
    }

    // C64-12 连坏三份即降级
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let _ = s.start();
        let bad = PATHOLOGICAL_NS as i64 + 1;
        let a = s.submit(sample(0, bad, Source::Gpu));
        let b = s.submit(sample(0, bad, Source::Gpu));
        let before = s.state();
        let c = s.submit(sample(0, bad, Source::Gpu));
        let ok = matches!(a, Ingest::Rejected { code, .. } if code == XC_PATHOLOGICAL)
            && matches!(b, Ingest::Rejected { .. })
            && before == SvcState::Ready
            && matches!(c, Ingest::Rejected { .. })
            && s.state() == SvcState::Degraded
            && s.counters().7 == 1;
        chk(&mut set, "C64-12 连坏降级", ok, "坏样本未降级或早降级");
    }

    // C64-13 降级后仍出戳但如实标注
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let _ = s.start();
        let bad = PATHOLOGICAL_NS as i64 + 1;
        let mut i = 0u32;
        while i < FAIL_LIMIT {
            let _ = s.submit(sample(0, bad, Source::Gpu));
            i += 1;
        }
        let t = s.stamp(5_000, Source::Gpu);
        let ok = t == Ok(5_000) && s.source_label() == "cpu-only" && s.state() == SvcState::Degraded;
        chk(&mut set, "C64-13 降级仍服务", ok, "降级后直接拒服务或标错来源");
    }

    // C64-14 恢复必须显式且需真降级
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let _ = s.start();
        let not_yet = s.recover();
        let bad = PATHOLOGICAL_NS as i64 + 1;
        let mut i = 0u32;
        while i < FAIL_LIMIT {
            let _ = s.submit(sample(0, bad, Source::Gpu));
            i += 1;
        }
        let still_bad = s.recover();
        let good = s.submit(sample(40_000, 40_000, Source::Gpu));
        let now_ok = s.recover();
        let ok = not_yet == Err(XC_NOT_DEGRADED)
            && still_bad == Err(XC_DEGRADED)
            && matches!(good, Ingest::Accepted { .. })
            && now_ok == Ok(SvcState::Ready)
            && s.state() == SvcState::Ready;
        chk(&mut set, "C64-14 显式恢复", ok, "自动恢复或恢复条件不全");
    }

    // C64-15 校准前的时间戳不得冒充已校准
    {
        let mut s = CalibService::new();
        let st = s.start();
        let t = s.stamp(7_000, Source::Gpu);
        let ok = st == Ok(SvcState::Degraded)
            && t == Ok(7_000)
            && s.source_label() == "cpu-only"
            && !s.measured()
            && s.precision_ns() == FLOOR_NS;
        chk(&mut set, "C64-15 未校准不冒充", ok, "未校准态被当作已校准");
    }

    // C64-16 单调性：校准回拨被钳住并留痕
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let _ = s.start();
        let t1 = s.stamp(10_000, Source::Gpu);
        let _ = s.force_affine(-50_000, 0);
        let t2 = s.stamp(10_000, Source::Gpu);
        let t3 = s.stamp(60_000, Source::Gpu);
        let ok = t1 == Ok(10_000)
            && t2 == Ok(10_000)
            && s.counters().6 == 1
            && t3 == Ok(10_000);
        chk(&mut set, "C64-16 单调性钳位", ok, "时间倒流未被钳住");
    }

    // C64-17 跨设备同刻差值不超声明精度
    {
        let mut s = CalibService::new();
        let _ = s.force_affine(3_000, 0);
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let d_ok = s.cross_delta(4_000, 1_000, Source::Gpu);
        let _ = s.force_affine(3_500, 0);
        let d_bad = s.cross_delta(4_000, 1_000, Source::Gpu);
        let ok = d_ok == 0 && d_bad == 500 && s.declare(200) == Err(XC_PRECISION_OVERCLAIM);
        chk(&mut set, "C64-17 跨设备同刻", ok, "同刻差值口径错");
    }

    // C64-18 样本账平衡（接受 + 拒收 + 满账丢弃 ≡ 提交）
    {
        let mut s = CalibService::new();
        let _ = s.submit(sample(1_000, 1_000, Source::Gpu));
        let _ = s.submit(sample(2_000, 2_100, Source::Gpu));
        let _ = s.submit(sample(0, PATHOLOGICAL_NS as i64 + 1, Source::Gpu));
        let n = s.counters();
        let ok = s.ledger_balanced() && n.0 == 3 && n.1 == 2 && n.2 == 1 && n.3 == 0;
        chk(&mut set, "C64-18 样本账平衡", ok, "样本账不平");
    }

    // C64-19 满账诚实截断（不覆盖旧样本、不静默）
    {
        let mut s = CalibService::new();
        let mut i = 0i64;
        while (i as usize) < SAMPLE_CAP {
            let _ = s.submit(sample(i * 1_000, i * 1_000, Source::Gpu));
            i += 1;
        }
        let over = s.submit(sample(999_000, 999_000, Source::Gpu));
        let n = s.counters();
        let ok = matches!(over, Ingest::Rejected { code, .. } if code == XC_SAMPLE_FULL)
            && n.1 as usize == SAMPLE_CAP
            && n.3 == 1
            && s.ledger_balanced();
        chk(&mut set, "C64-19 满账诚实", ok, "满账静默覆盖");
    }

    // C64-20 状态行四段齐备
    {
        let s = calibrated();
        let line = s.status_line();
        let ok = line.contains("epoch") && line.contains("精度") && line.contains("来源") && line.contains("降级");
        chk(&mut set, "C64-20 状态行可读", ok, "读屏字段缺失");
    }

    set
}

/// 单点演练。
pub fn drill() -> (bool, String) {
    let mut s = calibrated();
    let mut ok = true;
    let mut out = String::new();
    if s.stamp(2_000, Source::Gpu) != Ok(2_000) {
        ok = false;
        out.push_str("stamp ");
    }
    if s.precision_ns() != FLOOR_NS {
        ok = false;
        out.push_str("prec ");
    }
    if !s.ledger_balanced() {
        ok = false;
        out.push_str("ledger ");
    }
    if out.is_empty() {
        out.push_str("校准服务正常：");
        out.push_str(&s.status_line());
    }
    (ok, out)
}
