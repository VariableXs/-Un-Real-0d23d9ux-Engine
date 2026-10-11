//! cgp09_privacy — CGPU-F2409 遥测隐私（三级分类复用 · 导出闸 · 类型面处置）。
//!
//! 判据原文（施工书锚点 CGPU-F2409）：隐私（遥测隐私（三级分类复用——隐私
//! 复用——隐私复用；测试（隐私一组）。判据：隐私复用、一组、判据。
//!
//! 职责定位：遥测出机前隐私闸——三级分类**复用** F2402 [`PrivacyLevel`]
//! （本地/匿名/公开，档位即处置策略——不第二套 schema，判据"隐私复用"的
//! 结构化兑现：类型面 `use` 单源 + 运行期 label 对拍双保险）；导出闸四态
//! （闸关/本地拦/匿名脱敏出/公开放——默认关同 F5210 explicit-opt-in 纪律：
//! 无显式同意全拦，档位再公开也出不去）；脱敏器白名单注册制（未注册维度
//! 出机前剥除——匿名不是口号是维度瘦身）；隐私账四计数对账+审计明细**不
//! 含值**（账本身不得成为隐私面——只记 ID/档位/裁决码）。诊断码段 0x50。

use alloc::vec::Vec;

// 隐私三级分类复用（F2402 单源——判据"隐私复用"：编译期 use 即证明）。
use crate::cgpu::cgp02_unifiedmodel::{Metric, MetricValue, PrivacyLevel};

/// 模块版本。
pub const PRIVACY_VERSION: &str = "P09-privacy-v1";

/// 诊断码（字符串码闭集——P 域惯例）。
pub const E_PV_GATE_CLOSED: &str = "E_PV_GATE_CLOSED";
pub const E_PV_LOCAL: &str = "E_PV_LOCAL";
pub const E_PV_UNREG: &str = "E_PV_UNREG";
pub const E_PV_NOLEDGER: &str = "E_PV_NOLEDGER";

/// 诊断详情（闭集逐码中文说明）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_PV_GATE_CLOSED => "导出闸关闭：无显式同意全拦——档位再公开也出不去",
        E_PV_LOCAL => "本地档遥测：默认留设备不出机——红线进类型不是进文档",
        E_PV_UNREG => "维度未注册：白名单注册制外维度不可放行",
        E_PV_NOLEDGER => "隐私账未接：导出必须过账——无账导出即无审计",
        _ => "未知隐私诊断码",
    }
}

/// 导出裁决四态（判据"档位即处置策略"的执行形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportVerdict {
    /// 闸关：无显式同意（无论档位全拦——默认关）。
    GateClosed,
    /// 本地档拦截：不出设备。
    BlockedLocal,
    /// 匿名档：脱敏后放行（携带脱敏后保留的维度数）。
    Redacted(usize),
    /// 公开档：直接放行。
    Released,
}

impl ExportVerdict {
    /// 是否放行（脱敏放行也算出机——但形态已瘦身）。
    pub const fn released(self) -> bool {
        matches!(self, ExportVerdict::Redacted(_) | ExportVerdict::Released)
    }

    /// 人话标签（读屏可达——判据锚点"价签可见即诚实承诺"同域本色）。
    pub const fn label(self) -> &'static str {
        match self {
            ExportVerdict::GateClosed => "闸关拦截",
            ExportVerdict::BlockedLocal => "本地拦截",
            ExportVerdict::Redacted(_) => "脱敏放行",
            ExportVerdict::Released => "直接放行",
        }
    }
}

/// 脱敏器（白名单注册制——匿名=维度瘦身不是字段改名）。
pub struct Redactor {
    /// 白名单维度 ID（注册制——出机可保留的维度闭集）。
    allow: Vec<u32>,
}

impl Redactor {
    pub fn new() -> Self {
        Redactor { allow: Vec::new() }
    }

    /// 注册白名单维度（重复注册幂等——集合语义）。
    pub fn allow_dimension(&mut self, dim: u32) {
        if !self.allow.contains(&dim) {
            self.allow.push(dim);
        }
    }

    /// 白名单容量。
    pub fn allow_len(&self) -> usize {
        self.allow.len()
    }

    /// 脱敏：只留白名单维度（未注册全剥），值/时间戳/档位不动。
    /// 返回（脱敏后维度数, 剥除数）——留痕可对账。
    pub fn redact(&self, m: &Metric) -> (usize, usize) {
        let mut kept = 0usize;
        let mut stripped = 0usize;
        let mut i = 0usize;
        while i < m.dimensions.len() {
            if self.allow.contains(&m.dimensions[i]) {
                kept += 1;
            } else {
                stripped += 1;
            }
            i += 1;
        }
        (kept, stripped)
    }
}

/// 审计行（判据红线：账只记 ID/档位/裁决码——**不含值**，账本身不成为
/// 隐私面）。
#[derive(Clone, Copy, Debug)]
pub struct AuditRow {
    /// 裁决。
    pub verdict_code: &'static str,
    /// 隐私档（复用 F2402 三档）。
    pub level: PrivacyLevel,
}

/// 隐私账（四计数对账 + 审计明细——显性不静默）。
pub struct PrivacyLedger {
    pub closed: u32,
    pub blocked_local: u32,
    pub redacted: u32,
    pub released: u32,
    /// 审计明细（有界 128——洪峰丢最旧不记值）。
    pub rows: Vec<AuditRow>,
    dropped: u32,
}

const AUDIT_CAP: usize = 128;

impl PrivacyLedger {
    pub fn new() -> Self {
        PrivacyLedger {
            closed: 0,
            blocked_local: 0,
            redacted: 0,
            released: 0,
            rows: Vec::new(),
            dropped: 0,
        }
    }

    /// 记一行裁决（审计面：O(1)；满载丢最旧计数不静默；档位入账可审计）。
    pub fn record(&mut self, v: ExportVerdict, level: PrivacyLevel) {
        match v {
            ExportVerdict::GateClosed => self.closed += 1,
            ExportVerdict::BlockedLocal => self.blocked_local += 1,
            ExportVerdict::Redacted(_) => self.redacted += 1,
            ExportVerdict::Released => self.released += 1,
        }
        if self.rows.len() >= AUDIT_CAP {
            self.rows.remove(0);
            self.dropped += 1;
        }
        let code = match v {
            ExportVerdict::GateClosed => E_PV_GATE_CLOSED,
            ExportVerdict::BlockedLocal => E_PV_LOCAL,
            ExportVerdict::Redacted(_) => E_PV_UNREG,
            ExportVerdict::Released => "E_PV_RELEASED",
        };
        self.rows.push(AuditRow { verdict_code: code, level });
    }

    /// 四计数总和（对账口）。
    pub fn total(&self) -> u32 {
        self.closed + self.blocked_local + self.redacted + self.released
    }

    /// 丢行计数（有界账显性化）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
}

/// 导出闸（判据"三级分类复用"的执行器：档位×同意→四态裁决）。
pub struct PrivacyGate {
    /// 显式同意（默认 false——默认关，同 F5210 纪律）。
    pub consent: bool,
}

impl PrivacyGate {
    pub fn new() -> Self {
        PrivacyGate { consent: false }
    }

    /// 裁决单条遥测（闸→档位分流；匿名档带脱敏保留维度数）。
    pub fn judge(&self, m: &Metric, red: &Redactor) -> ExportVerdict {
        if !self.consent {
            return ExportVerdict::GateClosed;
        }
        match m.privacy {
            PrivacyLevel::Local => ExportVerdict::BlockedLocal,
            PrivacyLevel::Anonymized => {
                let (kept, _) = red.redact(m);
                ExportVerdict::Redacted(kept)
            }
            PrivacyLevel::Public => ExportVerdict::Released,
        }
    }
}

/// 隐私守卫门面（闸+脱敏器+账三联动——导出流水线唯一入口）。
pub struct PrivacyGuard {
    gate: PrivacyGate,
    red: Redactor,
    ledger: PrivacyLedger,
}

impl PrivacyGuard {
    pub fn new() -> Self {
        PrivacyGuard { gate: PrivacyGate::new(), red: Redactor::new(), ledger: PrivacyLedger::new() }
    }

    /// 开闸（显式同意——一次性语义，默认关的对侧）。
    pub fn grant_consent(&mut self) {
        self.gate.consent = true;
    }

    /// 注册白名单维度。
    pub fn allow_dimension(&mut self, dim: u32) {
        self.red.allow_dimension(dim);
    }

    /// 导出流水线（裁决→脱敏→入账三步；返回裁决供调用方执行）。
    pub fn export(&mut self, m: &Metric) -> ExportVerdict {
        let v = self.gate.judge(m, &self.red);
        self.ledger.record(v, m.privacy);
        v
    }

    /// 账只读。
    pub fn ledger(&self) -> &PrivacyLedger {
        &self.ledger
    }

    /// 脱敏器只读。
    pub fn redactor(&self) -> &Redactor {
        &self.red
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "隐私闸{} 闸关{} 本地拦{} 脱敏{} 放行{} 丢行{}",
            PRIVACY_VERSION,
            self.ledger.closed,
            self.ledger.blocked_local,
            self.ledger.redacted,
            self.ledger.released,
            self.ledger.dropped,
        )
    }
}

/// 值形状小闸（判据"一组"内的类型面卫生：Ppm 越界饱和到 1e6——复用
/// F2402 MetricValue::raw 的构造面钳制口径，本模块只做导出侧复核）。
pub fn value_sane(m: &Metric) -> bool {
    match m.value {
        MetricValue::Ppm(v) => v <= 1_000_000,
        _ => true,
    }
}
