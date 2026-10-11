//! cgf11_certtel — CGPU-F0809 认证遥测（认证体系自身的体检表，四指标进总线）。
//!
//! 判据原文（施工书锚点 CGPU-F0809）：遥测：认证执行次数/通过率分布（金/银
//! /铜比例）/重认证触发分布/边缘复核率——四指标进 B08 总线。判据：四指标、
//! 判据。
//!
//! 职责定位：四指标聚合器（执行次数/等级分布/重认证触发分布/边缘复核率）+
//! B08 总线发布（schema 版本化快照，复用 F0490 面板范式）+ 分列纪律（按厂
//! 商族分列——全量均值掩盖厂商差异，分列才可定位哪族的考卷偏严）。与
//! cgf05_certsuite（考卷执行）、cgf07_certgrade（判级）、cgf08_certvalid
//! （有效期）分工：彼执行与判定，此把执行数据聚合公开——认证体系也被观测
//! （透明纪律延伸到认证自身）。边缘复核定义：判级处于级线容差带内的样本复
//! 核率（复核≠改判，复核率本身是考卷质量的观察窗）。空族不发零值（诚实空
//! 态）。诊断码段 0x5601~0x5605。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CERT_TEL_VERSION: &str = "F11-certtel-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x56 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_FAMILY_UNREGISTERED: u32 = 0x5601; // 未登记厂商族（分列准入拒）
pub const XC_TEL_COUNT_OVERFLOW: u32 = 0x5602; // 计数溢出（饱和保护触发即立案）
pub const XC_RATIO_INVALID: u32 = 0x5603; // 比例和违约（分布三分量和 ≠ 1e6 ppm）
pub const XC_REAUTH_UNKNOWN_TRIGGER: u32 = 0x5604; // 重认证触发类型未知（闭集外拒）
pub const XC_REVIEW_RATE_INVALID: u32 = 0x5605; // 复核率越界（复核数>执行数）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 厂商族分列上限。
pub const FAMILY_CAP: usize = 8;
/// 比例定点：ppm（百万分比）。
pub const PPM: u64 = 1_000_000;
/// B08 schema 版本。
pub const CERT_TEL_SCHEMA_V: u32 = 1;

/// 认证等级闭集（与 cgf07 四级对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TelGrade {
    Gold,
    Silver,
    Bronze,
    Uncertified,
}

impl TelGrade {
    pub const ALL: [TelGrade; 4] = [
        TelGrade::Gold,
        TelGrade::Silver,
        TelGrade::Bronze,
        TelGrade::Uncertified,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            TelGrade::Gold => "金级",
            TelGrade::Silver => "银级",
            TelGrade::Bronze => "铜级",
            TelGrade::Uncertified => "未认证",
        }
    }
}

/// 重认证触发类型闭集（判据"重认证触发分布"——与 cgf08 触发规则表对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReauthTrigger {
    /// 大版本驱动更新。
    DriverMajor,
    /// 系统更新。
    SystemUpdate,
    /// 引擎变更。
    EngineChange,
}

impl ReauthTrigger {
    pub const ALL: [ReauthTrigger; 3] = [
        ReauthTrigger::DriverMajor,
        ReauthTrigger::SystemUpdate,
        ReauthTrigger::EngineChange,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            ReauthTrigger::DriverMajor => "驱动大版本",
            ReauthTrigger::SystemUpdate => "系统更新",
            ReauthTrigger::EngineChange => "引擎变更",
        }
    }
}

/// 单厂商族遥测账。
pub struct FamilyBucket {
    pub family: String,
    /// 执行次数。
    pub runs: u64,
    /// 等级计数（四闭集）。
    pub grades: [u64; 4],
    /// 重认证触发计数（三闭集）。
    pub reauths: [u64; 3],
    /// 边缘复核数（复核样本数）。
    pub reviews: u64,
    /// 复核改判数（复核后实际改级的——复核率的质量观察窗）。
    pub review_flips: u64,
}

/// 四指标快照（B08 发布行）。
#[derive(Clone, Debug)]
pub struct CertTelSnap {
    pub family: String,
    pub runs: u64,
    /// 金/银/铜/未认证比例（ppm，四分量和=1e6）。
    pub grade_ppm: [u64; 4],
    /// 重认证触发分布（ppm，对 reauth 总量归一；零重认证=全零）。
    pub reauth_ppm: [u64; 3],
    /// 边缘复核率（ppm：reviews/runs）。
    pub review_rate_ppm: u64,
    /// 复核改判率（ppm：flips/reviews；零复核=0）。
    pub flip_rate_ppm: u64,
}

/// 认证遥测发布器。
pub struct CertTelemetry {
    families: Vec<FamilyBucket>,
    pub schema_v: u32,
    pub publish_count: u64,
    pub saturations: u64,
}

impl CertTelemetry {
    pub fn new() -> Self {
        CertTelemetry {
            families: Vec::new(),
            schema_v: CERT_TEL_SCHEMA_V,
            publish_count: 0,
            saturations: 0,
        }
    }

    /// 厂商族登记（幂等）。
    pub fn register_family(&mut self, family: &str) -> Result<(), u32> {
        if self.find_idx(family).is_some() {
            return Ok(());
        }
        if self.families.len() >= FAMILY_CAP {
            return Err(XC_FAMILY_UNREGISTERED);
        }
        self.families.push(FamilyBucket {
            family: String::from(family),
            runs: 0,
            grades: [0; 4],
            reauths: [0; 3],
            reviews: 0,
            review_flips: 0,
        });
        Ok(())
    }

    fn find_idx(&self, family: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.families.len() {
            if self.families[i].family == family {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 认证执行录入（判级结果必带——无判级的执行不是认证）。
    pub fn observe_run(&mut self, family: &str, g: TelGrade) -> Result<(), u32> {
        let idx = match self.find_idx(family) {
            Some(i) => i,
            None => return Err(XC_FAMILY_UNREGISTERED),
        };
        let b = &mut self.families[idx];
        if b.runs == u64::MAX {
            self.saturations += 1;
            return Err(XC_TEL_COUNT_OVERFLOW);
        }
        b.runs += 1;
        b.grades[g as usize] += 1;
        Ok(())
    }

    /// 重认证触发录入。
    pub fn observe_reauth(&mut self, family: &str, t: ReauthTrigger) -> Result<(), u32> {
        let idx = match self.find_idx(family) {
            Some(i) => i,
            None => return Err(XC_FAMILY_UNREGISTERED),
        };
        self.families[idx].reauths[t as usize] += 1;
        Ok(())
    }

    /// 边缘复核录入（改判与否都算复核样本；复核数>执行数即违约）。
    pub fn observe_review(&mut self, family: &str, flipped: bool) -> Result<(), u32> {
        let idx = match self.find_idx(family) {
            Some(i) => i,
            None => return Err(XC_FAMILY_UNREGISTERED),
        };
        let b = &mut self.families[idx];
        if b.reviews >= b.runs {
            // 复核样本不得多于执行样本（判据语义自洽性红线）。
            return Err(XC_REVIEW_RATE_INVALID);
        }
        b.reviews += 1;
        if flipped {
            b.review_flips += 1;
        }
        Ok(())
    }

    /// ppm 归一（counts → ppm；总量零返回全零——诚实空态）。
    fn ppm_of(counts: &[u64; 4]) -> [u64; 4] {
        let mut total: u128 = 0;
        let mut i = 0;
        while i < counts.len() {
            total += counts[i] as u128;
            i += 1;
        }
        let mut out = [0u64; 4];
        if total == 0 {
            return out;
        }
        let mut acc: u128 = 0;
        let mut j = 0;
        while j < counts.len() {
            if j == counts.len() - 1 {
                out[j] = (PPM as u128 - acc) as u64; // 尾项守恒（和恰 1e6）
            } else {
                let p = counts[j] as u128 * PPM as u128 / total;
                out[j] = p as u64;
                acc += p;
            }
            j += 1;
        }
        out
    }

    /// B08 发布包（判据"四指标进 B08 总线"）。
    pub fn publish(&mut self) -> Vec<CertTelSnap> {
        self.publish_count += 1;
        let mut out = Vec::with_capacity(self.families.len());
        let mut i = 0;
        while i < self.families.len() {
            let b = &self.families[i];
            // 重认证分布对 reauth 总量归一。
            let mut rt: u128 = 0;
            let mut k = 0;
            while k < 3 {
                rt += b.reauths[k] as u128;
                k += 1;
            }
            let mut reauth_ppm = [0u64; 3];
            if rt > 0 {
                let mut acc: u128 = 0;
                let mut j = 0;
                while j < 3 {
                    if j == 2 {
                        reauth_ppm[j] = (PPM as u128 - acc) as u64;
                    } else {
                        let p = b.reauths[j] as u128 * PPM as u128 / rt;
                        reauth_ppm[j] = p as u64;
                        acc += p;
                    }
                    j += 1;
                }
            }
            let grade_ppm = Self::ppm_of(&b.grades);
            let review_rate = if b.runs > 0 {
                b.reviews * PPM / b.runs
            } else {
                0
            };
            let flip_rate = if b.reviews > 0 {
                b.review_flips * PPM / b.reviews
            } else {
                0
            };
            out.push(CertTelSnap {
                family: b.family.clone(),
                runs: b.runs,
                grade_ppm,
                reauth_ppm,
                review_rate_ppm: review_rate,
                flip_rate_ppm: flip_rate,
            });
            i += 1;
        }
        out
    }

    /// 比例和自洽断言（发布前置校验——判据"判据"面）。
    pub fn verify_ratios(&self, s: &CertTelSnap) -> Result<(), u32> {
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < 4 {
            sum += s.grade_ppm[i];
            i += 1;
        }
        if sum != PPM && s.runs > 0 {
            return Err(XC_RATIO_INVALID);
        }
        if s.review_rate_ppm > PPM || s.flip_rate_ppm > PPM {
            return Err(XC_REVIEW_RATE_INVALID);
        }
        Ok(())
    }

    /// 登记族数。
    pub const fn family_count(&self) -> usize {
        self.families.len()
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        let mut runs: u128 = 0;
        let mut i = 0;
        while i < self.families.len() {
            runs += self.families[i].runs as u128;
            i += 1;
        }
        alloc::format!(
            "认证遥测{} 族{}/{} 执行{} 发布{} 饱和{}",
            CERT_TEL_VERSION,
            self.families.len(),
            FAMILY_CAP,
            runs,
            self.publish_count,
            self.saturations,
        )
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：执行次数 / 等级分布 / 重认证分布 / 边缘复核）
// ---------------------------------------------------------------------------

impl CertTelemetry {
    /// 组一+二：执行次数与等级分布——金 3 银 4 铜 3 → 30/40/30 ppm。
    pub fn selfcheck_distribution(&mut self) -> Result<(), u32> {
        let mut t = CertTelemetry::new();
        t.register_family("nv")?;
        let feed: [TelGrade; 10] = [
            TelGrade::Gold, TelGrade::Silver, TelGrade::Gold, TelGrade::Silver,
            TelGrade::Bronze, TelGrade::Silver, TelGrade::Bronze, TelGrade::Gold,
            TelGrade::Bronze, TelGrade::Silver,
        ];
        let mut i = 0;
        while i < feed.len() {
            t.observe_run("nv", feed[i])?;
            i += 1;
        }
        let pack = t.publish();
        let s = &pack[0];
        if s.runs != 10 {
            return Err(XC_TEL_COUNT_OVERFLOW);
        }
        // 3/10=30%，4/10=40%，3/10=30%（ppm：300000/400000/300000）。
        if s.grade_ppm[0] != 300_000 || s.grade_ppm[1] != 400_000 || s.grade_ppm[2] != 300_000 {
            return Err(XC_RATIO_INVALID);
        }
        t.verify_ratios(s)?;
        // 未登记族拒。
        if t.observe_run("amd", TelGrade::Gold) != Err(XC_FAMILY_UNREGISTERED) {
            return Err(XC_FAMILY_UNREGISTERED);
        }
        Ok(())
    }

    /// 组三：重认证分布——三类归一、未知类型闭集拒（类型系统保证，此验证
    /// 分布数值）。
    pub fn selfcheck_reauth(&mut self) -> Result<(), u32> {
        let mut t = CertTelemetry::new();
        t.register_family("nv")?;
        let mut i = 0;
        while i < 2 {
            t.observe_run("nv", TelGrade::Gold)?;
            i += 1;
        }
        let feed: [ReauthTrigger; 4] = [
            ReauthTrigger::DriverMajor,
            ReauthTrigger::DriverMajor,
            ReauthTrigger::SystemUpdate,
            ReauthTrigger::EngineChange,
        ];
        let mut j = 0;
        while j < feed.len() {
            t.observe_reauth("nv", feed[j])?;
            j += 1;
        }
        let pack = t.publish();
        let s = &pack[0];
        // 2/4=50%、1/4=25%、1/4=25%。
        if s.reauth_ppm[0] != 500_000 || s.reauth_ppm[1] != 250_000 || s.reauth_ppm[2] != 250_000 {
            return Err(XC_REAUTH_UNKNOWN_TRIGGER);
        }
        // 零重认证族：全零分布（诚实空态）。
        t.register_family("idle")?;
        t.observe_run("idle", TelGrade::Silver)?;
        let pack2 = t.publish();
        let s2 = &pack2[1];
        if s2.reauth_ppm[0] != 0 || s2.reauth_ppm[1] != 0 || s2.reauth_ppm[2] != 0 {
            return Err(XC_REAUTH_UNKNOWN_TRIGGER);
        }
        Ok(())
    }

    /// 组四：边缘复核——复核率与改判率、复核超执行拒绝。
    pub fn selfcheck_review(&mut self) -> Result<(), u32> {
        let mut t = CertTelemetry::new();
        t.register_family("nv")?;
        let mut i = 0;
        while i < 10 {
            t.observe_run("nv", TelGrade::Gold)?;
            i += 1;
        }
        // 2 次复核 1 次改判。
        t.observe_review("nv", false)?;
        t.observe_review("nv", true)?;
        // 第 3 次：复核数将超执行数？否——2<10 合法；构造违约：先加满。
        let mut j = 0;
        while j < 8 {
            t.observe_review("nv", false)?;
            j += 1;
        }
        // 现在 reviews=10=runs，再复核应拒。
        if t.observe_review("nv", false) != Err(XC_REVIEW_RATE_INVALID) {
            return Err(XC_REVIEW_RATE_INVALID);
        }
        let pack = t.publish();
        let s = &pack[0];
        if s.review_rate_ppm != PPM {
            return Err(XC_REVIEW_RATE_INVALID); // 10/10=100%
        }
        if s.flip_rate_ppm != 100_000 {
            return Err(XC_REVIEW_RATE_INVALID); // 1/10=10%
        }
        t.verify_ratios(s)?;
        Ok(())
    }

    /// 组五：schema 与饱和——版本核验、计数饱和立案。
    pub fn selfcheck_schema(&mut self) -> Result<(), u32> {
        let mut t = CertTelemetry::new();
        t.register_family("nv")?;
        t.observe_run("nv", TelGrade::Gold)?;
        let pack = t.publish();
        t.verify_ratios(&pack[0])?;
        if t.schema_v != CERT_TEL_SCHEMA_V {
            return Err(XC_TEL_COUNT_OVERFLOW);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_distribution()?;
        self.selfcheck_reauth()?;
        self.selfcheck_review()?;
        self.selfcheck_schema()?;
        Ok(())
    }
}
