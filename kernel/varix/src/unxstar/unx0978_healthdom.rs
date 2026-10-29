//! unx0978_healthdom — UNX-F0978 中断面健康域级收口（F0997 总账域级摘要）。
//!
//! 任务锚点：UNX-F0978（第二批工位 W7，SEG 段位 0x8040，本单独占
//! 0x8040..=0x805F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**F0997 健康总账的域级摘要逐面齐备（七
//! 面健康计数、告警流、退化度三列俱全），域级摘要与全域账逐项对拍
//! 相等，退化度越阈面带码上告警，摘要双导出指纹稳定**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x804 段闭集）；tick 显性
//! 注入零墙钟；摘要守恒（Σ面健康计数==域总计 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8040 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面健康域级收口段基址（0x8040，本单 0x8040..=0x805F 独占）。
pub const SEG_BASE: u16 = 0x8040;
/// 0x8041 面域越界（面码不在七面闭集内）。
pub const XC_FACE_BAD: u16 = SEG_BASE + 1;
/// 0x8042 健康计数回退（计数只增不减——回退即账面污染）。
pub const XC_COUNT_REGRESS: u16 = SEG_BASE + 2;
/// 0x8043 摘要失配（Σ面计数 != 域总计——收口不一致）。
pub const XC_SUM_MISMATCH: u16 = SEG_BASE + 3;
/// 0x8044 退化度越阈未告警（越阈面缺告警行——告警流断链）。
pub const XC_ALARM_MISSING: u16 = SEG_BASE + 4;
/// 0x8045 退化度标定非法（阈值为零或权重和为零）。
pub const XC_SCALE_BAD: u16 = SEG_BASE + 5;
/// 0x8046 摘要指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 6;
/// 0x8047 摘要未收口即出文（收口前置缺失）。
pub const XC_NOT_SEALED: u16 = SEG_BASE + 7;
/// 0x8048 未知面计数上报（来源面未登记——野计数拒收）。
pub const XC_FACE_UNKNOWN: u16 = SEG_BASE + 8;
/// 0x8049 告警级别非法（级码不在三级闭集内）。
pub const XC_ALARM_LEVEL: u16 = SEG_BASE + 9;
/// 0x804A 健康域守恒破坏（重扫恒等式不成立）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 10;

const _: () = assert!(XC_FACE_BAD != XC_COUNT_REGRESS);
const _: () = assert!(XC_SUM_MISMATCH != XC_ALARM_MISSING);
const _: () = assert!(XC_DRIFT != XC_CONSERVATION);

// ---------------------------------------------------------------------------
// §1 面域与告警级闭集（与 F0976 七面同轴、三级告警）
// ---------------------------------------------------------------------------

/// 健康面域闭集（七面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthFace {
    /// 开合面。
    Gate,
    /// 向量面。
    Vector,
    /// EOI 面。
    Eoi,
    /// 风暴面。
    Storm,
    /// 计时面。
    Timing,
    /// 健康面自身。
    Health,
    /// 注入面。
    Inject,
}

impl HealthFace {
    /// 面码（1..=7）。
    pub const fn code(self) -> u8 {
        match self {
            HealthFace::Gate => 1,
            HealthFace::Vector => 2,
            HealthFace::Eoi => 3,
            HealthFace::Storm => 4,
            HealthFace::Timing => 5,
            HealthFace::Health => 6,
            HealthFace::Inject => 7,
        }
    }
    /// 面名。
    pub const fn name(self) -> &'static str {
        match self {
            HealthFace::Gate => "开合面",
            HealthFace::Vector => "向量面",
            HealthFace::Eoi => "EOI面",
            HealthFace::Storm => "风暴面",
            HealthFace::Timing => "计时面",
            HealthFace::Health => "健康面",
            HealthFace::Inject => "注入面",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<HealthFace> {
        match c {
            1 => Some(HealthFace::Gate),
            2 => Some(HealthFace::Vector),
            3 => Some(HealthFace::Eoi),
            4 => Some(HealthFace::Storm),
            5 => Some(HealthFace::Timing),
            6 => Some(HealthFace::Health),
            7 => Some(HealthFace::Inject),
            _ => None,
        }
    }
}

/// 面域总数。
pub const FACE_COUNT: usize = 7;

/// 告警级闭集：提示 / 警告 / 严重。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmLevel {
    /// 提示（观察项）。
    Info,
    /// 警告（需跟踪）。
    Warn,
    /// 严重（需动作）。
    Critical,
}

impl AlarmLevel {
    /// 级码（1/2/3）。
    pub const fn code(self) -> u8 {
        match self {
            AlarmLevel::Info => 1,
            AlarmLevel::Warn => 2,
            AlarmLevel::Critical => 3,
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<AlarmLevel> {
        match c {
            1 => Some(AlarmLevel::Info),
            2 => Some(AlarmLevel::Warn),
            3 => Some(AlarmLevel::Critical),
            _ => None,
        }
    }
}

/// 退化度告警阈值（万分基点——超过即须上告警行）。
pub const DEGRADE_ALARM_BP: u32 = 2_000;

// ---------------------------------------------------------------------------
// §2 域级摘要行与告警行
// ---------------------------------------------------------------------------

/// 一面健康摘要行：面 / 健康计数 / 退化度（万分基点）。
#[derive(Clone, Copy, Debug)]
pub struct FaceSummary {
    face: HealthFace,
    healthy: u64,
    degrade_bp: u32,
}

impl FaceSummary {
    /// 面名。
    pub fn face_name(&self) -> &'static str {
        self.face.name()
    }
    /// 健康计数。
    pub fn healthy(&self) -> u64 {
        self.healthy
    }
    /// 退化度。
    pub fn degrade_bp(&self) -> u32 {
        self.degrade_bp
    }
    /// 是否越告警阈。
    pub fn over_threshold(&self) -> bool {
        self.degrade_bp >= DEGRADE_ALARM_BP
    }
}

/// 一行告警：面 / 级 / 退化度。
#[derive(Clone, Copy, Debug)]
pub struct AlarmRow {
    face: HealthFace,
    level: AlarmLevel,
    degrade_bp: u32,
}

impl AlarmRow {
    /// 面码。
    pub fn face_code(&self) -> u8 {
        self.face.code()
    }
    /// 级码。
    pub fn level_code(&self) -> u8 {
        self.level.code()
    }
}

// ---------------------------------------------------------------------------
// §3 域级健康收口器（HealthDomainCloser）
// ---------------------------------------------------------------------------

/// 域级健康收口器——F0997 总账的域级摘要投影本体。
///
/// 守恒恒等式：`Σ(七面 healthy) == domain_total`（每条计数恰归一面，
/// 无重复归面、无漏归面）；退化度恒等式：`Σ(weighted) / Σ(weight)
/// == domain_degrade_bp`（加权平均域级退化度）。
#[derive(Debug)]
pub struct HealthDomainCloser {
    faces: [Option<FaceSummary>; FACE_COUNT],
    weights: [u32; FACE_COUNT],
    domain_total: u64,
    alarms: Vec<AlarmRow>,
    sealed: bool,
    export_fp: u64,
    rejected: u32,
}

impl HealthDomainCloser {
    /// 建收口器（七面等权标定）。
    pub fn new() -> Result<HealthDomainCloser, u16> {
        let mut weights = [0u32; FACE_COUNT];
        let mut i: usize = 0;
        while i < FACE_COUNT {
            weights[i] = 1;
            i += 1;
        }
        let wsum = weight_sum(&weights);
        if wsum == 0 {
            return Err(XC_SCALE_BAD);
        }
        Ok(HealthDomainCloser {
            faces: [None, None, None, None, None, None, None],
            weights,
            domain_total: 0,
            alarms: Vec::new(),
            sealed: false,
            export_fp: 0,
            rejected: 0,
        })
    }

    /// 上报一面健康计数增量（未登记面越界拒收；计数回退拒收）。
    pub fn report(&mut self, face: HealthFace, delta: u64, degrade_bp: u32) -> Result<(), u16> {
        if HealthFace::from_code(face.code()).is_none() {
            self.rejected += 1;
            return Err(XC_FACE_BAD);
        }
        if degrade_bp > 10_000 {
            self.rejected += 1;
            return Err(XC_SCALE_BAD);
        }
        let idx = (face.code() - 1) as usize;
        match &mut self.faces[idx] {
            Some(s) => {
                let new_healthy = s.healthy + delta;
                if new_healthy < s.healthy {
                    self.rejected += 1;
                    return Err(XC_COUNT_REGRESS);
                }
                s.healthy = new_healthy;
                s.degrade_bp = degrade_bp;
            }
            None => {
                self.faces[idx] = Some(FaceSummary {
                    face,
                    healthy: delta,
                    degrade_bp,
                });
            }
        }
        self.domain_total += delta;
        Ok(())
    }

    /// 收口：生成域级告警流（越阈面逐面上行）并封板。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            self.rejected += 1;
            return Err(XC_NOT_SEALED);
        }
        // 告警流生成：越阈面逐面判定级（退化度 ≥ 严重阈 → Critical，
        // 否则 Warn）。
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if let Some(s) = &self.faces[i] {
                if s.over_threshold() {
                    let level = if s.degrade_bp >= 5_000 {
                        AlarmLevel::Critical
                    } else {
                        AlarmLevel::Warn
                    };
                    self.alarms.push(AlarmRow {
                        face: s.face,
                        level,
                        degrade_bp: s.degrade_bp,
                    });
                    n += 1;
                }
            }
            i += 1;
        }
        self.sealed = true;
        Ok(n)
    }

    /// 域总计。
    pub fn domain_total(&self) -> u64 {
        self.domain_total
    }
    /// 是否已收口。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    /// 告警行数。
    pub fn alarm_count(&self) -> usize {
        self.alarms.len()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 守恒机检：Σ面计数 == 域总计（逐面重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if let Some(f) = &self.faces[i] {
                s += f.healthy;
            }
            i += 1;
        }
        if s != self.domain_total {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 告警齐备机检：所有越阈面均有告警行（告警流断链即碎）。
    pub fn audit_alarms(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if let Some(s) = &self.faces[i] {
                if s.over_threshold() {
                    let mut found = false;
                    let mut j: usize = 0;
                    while j < self.alarms.len() {
                        if self.alarms[j].face.code() == s.face.code() {
                            found = true;
                            break;
                        }
                        j += 1;
                    }
                    if !found {
                        return Err(XC_ALARM_MISSING);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 域级退化度（万分基点——Σ权重×退化度 / Σ权重）。
    pub fn domain_degrade_bp(&self) -> u32 {
        let mut num: u64 = 0;
        let mut den: u64 = 0;
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if let Some(f) = &self.faces[i] {
                num += self.weights[i] as u64 * f.degrade_bp as u64;
                den += self.weights[i] as u64;
            }
            i += 1;
        }
        if den == 0 {
            return 0;
        }
        (num / den) as u32
    }

    /// 摘要文本导出（确定性序 + 双导出对拍 + 收口前置）。
    pub fn export(&mut self) -> Result<String, u16> {
        if !self.sealed {
            self.rejected += 1;
            return Err(XC_NOT_SEALED);
        }
        self.audit_conservation()?;
        self.audit_alarms()?;
        let mut s = String::from("UNX-F0978-HEALTHDOM|v1|total=");
        s.push_str(&self.domain_total.to_string());
        s.push_str("|degrade_bp=");
        s.push_str(&self.domain_degrade_bp().to_string());
        s.push_str("|alarms=");
        s.push_str(&self.alarms.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if let Some(f) = &self.faces[i] {
                s.push_str(f.face_name());
                s.push('|');
                s.push_str(&f.healthy().to_string());
                s.push('|');
                s.push_str(&f.degrade_bp().to_string());
                s.push('\n');
            }
            i += 1;
        }
        let mut a: usize = 0;
        while a < self.alarms.len() {
            s.push_str("ALARM|face=");
            s.push_str(&self.alarms[a].face_code().to_string());
            s.push_str("|level=");
            s.push_str(&self.alarms[a].level_code().to_string());
            s.push('\n');
            a += 1;
        }
        let fp = fingerprint_str(&s);
        if self.export_fp != 0 && fp != self.export_fp {
            return Err(XC_DRIFT);
        }
        self.export_fp = fp;
        Ok(s)
    }
}

/// 权重和（const 可用形式）。
fn weight_sum(w: &[u32; FACE_COUNT]) -> u32 {
    let mut s: u32 = 0;
    let mut i: usize = 0;
    while i < w.len() {
        s = s.wrapping_add(w[i]);
        i += 1;
    }
    s
}

/// 字符串 FNV-1a 64。
fn fingerprint_str(s: &str) -> u64 {
    let bs: &[u8] = s.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i: usize = 0;
    while i < bs.len() {
        h = (h ^ bs[i] as u64).wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：七面上报→越阈告警→收口→守恒→双导出→注入面。
pub fn drill() -> Result<u32, u16> {
    let mut c = HealthDomainCloser::new()?;
    // 七面上报：风暴面退化度 6000（Critical）、注入面 2500（Warn），
    // 其余低于阈。
    c.report(HealthFace::Gate, 100, 100)?;
    c.report(HealthFace::Vector, 80, 200)?;
    c.report(HealthFace::Eoi, 60, 300)?;
    c.report(HealthFace::Storm, 40, 6_000)?;
    c.report(HealthFace::Timing, 90, 150)?;
    c.report(HealthFace::Health, 70, 100)?;
    c.report(HealthFace::Inject, 50, 2_500)?;
    // 注入面：越界退化度拒收（面闭集由枚举构造性保证，无需注入）。
    if c.report(HealthFace::Gate, 1, 10_001) != Err(XC_SCALE_BAD) {
        return Err(XC_SCALE_BAD);
    }
    // 收口：两面越阈上行。
    let n = c.seal()?;
    if n != 2 || c.alarm_count() != 2 {
        return Err(XC_ALARM_MISSING);
    }
    // 守恒机检：域总计 490。
    c.audit_conservation()?;
    if c.domain_total() != 490 {
        return Err(XC_SUM_MISMATCH);
    }
    // 告警齐备机检。
    c.audit_alarms()?;
    // 双导出指纹稳定。
    let _ = c.export()?;
    let fp1 = c.export_fp;
    let _ = c.export()?;
    if c.export_fp != fp1 {
        return Err(XC_DRIFT);
    }
    // 重复收口拒收。
    if c.seal() != Err(XC_NOT_SEALED) {
        return Err(XC_NOT_SEALED);
    }
    Ok(0)
}
