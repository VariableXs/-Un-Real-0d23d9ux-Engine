//! CGPU-F1698 · 异构指标注册表终版
//!
//! 判据锚点（施工书原文摘录）：
//! 注册表：指标族（引擎运行族（利用率/队列/延迟）/协同族（加速比/开销/迁移）/
//! 能效族（能效比/功耗）/质量族（miss/跳帧/回退）——四族指标——注册表 v2 扩展
//! （F1556 模式复用——扩展记录）；口径（每指标公式唯一——口径纪律复用；生命周期
//! （新增/废弃复用；一致性（派生守恒抽查复用；测试（四族/口径/生命周期/一致性四组）。
//! 判据：四族、v2 扩展、口径、守恒、四组、判据。
//!
//! 诊断码段 0x8501~0x8505（独占）：
//!   0x8501 族越界（指标挂到不存在的族）
//!   0x8502 口径冲突（同名指标双公式 / 公式指纹漂移）
//!   0x8503 生命周期违规（废弃后写入 / 重名注册 / 未注册即派生）
//!   0x8504 派生守恒破（抽查重算与登记值失配）
//!   0x8505 段越界（判码落 0x8500..0x8600 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const OBS_METRIC_VERSION: &str = "K17-obsmetric-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8501~0x8505）
// ---------------------------------------------------------------------------

pub const XC_FAMILY_OOB: u32 = 0x8501;
pub const XC_CALIBER_CONFLICT: u32 = 0x8502;
pub const XC_LIFECYCLE_VIOLATION: u32 = 0x8503;
pub const XC_DERIVED_BROKEN: u32 = 0x8504;
pub const XC_SEG_OOR: u32 = 0x8505;

// ---------------------------------------------------------------------------
// 一、四族指标闭集
// ---------------------------------------------------------------------------

/// 指标族四闭集（判据字面：引擎运行/协同/能效/质量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricFamily {
    /// 引擎运行族：利用率/队列/延迟。
    EngineRun,
    /// 协同族：加速比/开销/迁移。
    Synergy,
    /// 能效族：能效比/功耗。
    Efficacy,
    /// 质量族：miss/跳帧/回退。
    Quality,
}

impl MetricFamily {
    pub const ALL: [MetricFamily; 4] =
        [MetricFamily::EngineRun, MetricFamily::Synergy, MetricFamily::Efficacy, MetricFamily::Quality];

    pub const fn ordinal(self) -> u8 {
        match self {
            MetricFamily::EngineRun => 0,
            MetricFamily::Synergy => 1,
            MetricFamily::Efficacy => 2,
            MetricFamily::Quality => 3,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            MetricFamily::EngineRun => "engine_run",
            MetricFamily::Synergy => "synergy",
            MetricFamily::Efficacy => "efficacy",
            MetricFamily::Quality => "quality",
        }
    }

    /// 族内标准指标名（闭集，呈现面用）。
    pub const fn members(self) -> &'static [&'static str] {
        match self {
            MetricFamily::EngineRun => &["engine_utili_permille", "queue_depth", "exec_latency_us"],
            MetricFamily::Synergy => &["speedup_bp", "handoff_overhead_us", "migration_count"],
            MetricFamily::Efficacy => &["fps_per_watt_bp", "power_milliwatt"],
            MetricFamily::Quality => &["cache_miss_permille", "frame_skip_count", "fallback_count"],
        }
    }
}

// ---------------------------------------------------------------------------
// 二、指标定义与生命周期
// ---------------------------------------------------------------------------

/// 指标生命周期三态（判据"生命周期（新增/废弃）复用"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LifeCycle {
    /// 新增生效：可写入可派生。
    Active,
    /// 废弃：停写，查询仍在册（不删除——历史账可查）。
    Deprecated,
}

/// 单指标定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetricDef {
    /// 指标名（英文标识符，注册键）。
    pub name: &'static str,
    /// 所属族。
    pub family: MetricFamily,
    /// 公式 FNV-1a 指纹——口径唯一性的机器锚（同名必须同指纹）。
    pub formula_fp: u64,
    /// 口径版本（口径变更须 bump 版本+新指纹）。
    pub caliber_ver: u32,
    /// 生命周期态。
    pub life: LifeCycle,
}

/// FNV-1a 64 位。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_4842_22325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三、注册表 v2（F1556 模式复用——扩展记录）
// ---------------------------------------------------------------------------

/// 派生关系：派生指标 = 基指标经派生函数折算（守恒抽查=重算比对）。
#[derive(Clone, Copy)]
pub struct DerivedEdge {
    /// 派生指标名。
    pub derived: &'static str,
    /// 基指标名。
    pub base: &'static str,
    /// 折算分子（万分比口径）。
    pub numer_bp: u32,
    /// 折算分母。
    pub denom: u32,
}

/// 指标注册表 v2：四族登记 + 口径锁 + 生命周期 + 派生守恒抽查。
pub struct RegistryV2 {
    defs: Vec<MetricDef>,
    edges: Vec<DerivedEdge>,
    /// 事件账：口径冲突次数 / 生命周期违规次数。
    pub caliber_events: u32,
    pub life_events: u32,
}

impl RegistryV2 {
    pub fn new() -> Self {
        RegistryV2 { defs: Vec::new(), edges: Vec::new(), caliber_events: 0, life_events: 0 }
    }

    /// 注册新指标：重名即拒（0x8503）；重名同指纹=幂等放行（v2 扩展记录）。
    pub fn enroll(&mut self, name: &'static str, family: MetricFamily, formula: &'static str, caliber_ver: u32) -> Result<(), u32> {
        if family.ordinal() > 3 {
            return Err(XC_FAMILY_OOB);
        }
        let fp = fnv64(formula.as_bytes());
        for d in &self.defs {
            if d.name == name {
                if d.formula_fp == fp && d.family == family {
                    return Ok(()); // 幂等重注册。
                }
                self.caliber_events += 1;
                return Err(XC_CALIBER_CONFLICT);
            }
        }
        self.defs.push(MetricDef {
            name,
            family,
            formula_fp: fp,
            caliber_ver,
            life: LifeCycle::Active,
        });
        Ok(())
    }

    /// 废弃：停写（查询仍在册）。
    pub fn deprecate(&mut self, name: &'static str) -> Result<(), u32> {
        for d in &mut self.defs {
            if d.name == name {
                d.life = LifeCycle::Deprecated;
                return Ok(());
            }
        }
        self.life_events += 1;
        Err(XC_LIFECYCLE_VIOLATION)
    }

    /// 写入闸：只许写 Active 指标（废弃后写入=0x8503）。
    pub fn write(&mut self, name: &'static str) -> Result<(), u32> {
        for d in &self.defs {
            if d.name == name {
                if d.life != LifeCycle::Active {
                    self.life_events += 1;
                    return Err(XC_LIFECYCLE_VIOLATION);
                }
                return Ok(());
            }
        }
        self.life_events += 1;
        Err(XC_LIFECYCLE_VIOLATION)
    }

    /// 挂派生关系：派生与基都必须在册 Active。
    pub fn derive(&mut self, derived: &'static str, base: &'static str, numer_bp: u32, denom: u32) -> Result<(), u32> {
        let d_ok = self.defs.iter().any(|d| d.name == derived && d.life == LifeCycle::Active);
        let b_ok = self.defs.iter().any(|d| d.name == base && d.life == LifeCycle::Active);
        if !d_ok || !b_ok {
            self.life_events += 1;
            return Err(XC_LIFECYCLE_VIOLATION);
        }
        if denom == 0 {
            self.caliber_events += 1;
            return Err(XC_CALIBER_CONFLICT);
        }
        self.edges.push(DerivedEdge { derived, base, numer_bp, denom });
        Ok(())
    }

    /// 派生守恒抽查：登记值 vs 重算值（折算=base×numer_bp/denom）。
    pub fn conserve_check(&self, derived: &'static str, base_val: u64, registered: u64) -> Result<(), u32> {
        let mut edge = None;
        for e in &self.edges {
            if e.derived == derived {
                edge = Some(*e);
            }
        }
        let e = match edge {
            Some(e) => e,
            None => return Err(XC_DERIVED_BROKEN),
        };
        let expect = base_val * e.numer_bp as u64 / e.denom as u64;
        if expect != registered {
            return Err(XC_DERIVED_BROKEN);
        }
        Ok(())
    }

    /// 四族覆盖审计：每族至少一个 Active 指标（终版要求四族全活着）。
    pub fn four_family_covered(&self) -> Result<(), u32> {
        for f in MetricFamily::ALL {
            let alive = self.defs.iter().any(|d| d.family == f && d.life == LifeCycle::Active);
            if !alive {
                return Err(XC_FAMILY_OOB);
            }
        }
        Ok(())
    }

    /// 口径唯一审计：同名指标在册只此一份（重名注册在 enroll 已拒，这里对
    /// 全表复核——终版不变式）。
    pub fn caliber_unique(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.defs.len() {
            let mut j = i + 1;
            while j < self.defs.len() {
                if self.defs[i].name == self.defs[j].name && self.defs[i].formula_fp != self.defs[j].formula_fp {
                    return Err(XC_CALIBER_CONFLICT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// 查定义。
    pub fn def(&self, idx: usize) -> Option<&MetricDef> {
        self.defs.get(idx)
    }
}

// ---------------------------------------------------------------------------
// 终版种子：四族 × 标准指标全量入册（每族成员闭集全注册）
// ---------------------------------------------------------------------------

/// 终版种子注册（判据"四族指标"字面成员全量）。
pub fn seed_final() -> RegistryV2 {
    let mut r = RegistryV2::new();
    // 引擎运行族三指标。
    let _ = r.enroll("engine_utili_permille", MetricFamily::EngineRun, "busy_ticks*1000/total_ticks", 1);
    let _ = r.enroll("queue_depth", MetricFamily::EngineRun, "pending_tasks", 1);
    let _ = r.enroll("exec_latency_us", MetricFamily::EngineRun, "done_us-count_us", 1);
    // 协同族三指标。
    let _ = r.enroll("speedup_bp", MetricFamily::Synergy, "cpu_us*10000/het_us", 1);
    let _ = r.enroll("handoff_overhead_us", MetricFamily::Synergy, "handoff_end-handoff_start", 1);
    let _ = r.enroll("migration_count", MetricFamily::Synergy, "migrations", 1);
    // 能效族两指标。
    let _ = r.enroll("fps_per_watt_bp", MetricFamily::Efficacy, "fps*10000/milliwatt", 1);
    let _ = r.enroll("power_milliwatt", MetricFamily::Efficacy, "rail_read", 1);
    // 质量族三指标。
    let _ = r.enroll("cache_miss_permille", MetricFamily::Quality, "miss*1000/lookups", 1);
    let _ = r.enroll("frame_skip_count", MetricFamily::Quality, "skipped", 1);
    let _ = r.enroll("fallback_count", MetricFamily::Quality, "fallbacks", 1);
    // 派生：能效比由 fps（速度）与功耗派生（演示守恒抽查口径）。
    let _ = r.derive("fps_per_watt_bp", "power_milliwatt", 10_000, 60);
    r
}

// ---------------------------------------------------------------------------
// 自检（四组：四族/口径/生命周期/一致性）
// ---------------------------------------------------------------------------

/// 终版引擎。
pub struct MetricEngine {
    pub reg: RegistryV2,
}

impl MetricEngine {
    pub fn new() -> Self {
        MetricEngine { reg: seed_final() }
    }

    /// 组一：四族——种子 11 指标全在 + 四族全活 + 族成员名闭集入册。
    pub fn selfcheck_families(&mut self) -> Result<(), u32> {
        if self.reg.len() != 11 {
            return Err(XC_FAMILY_OOB);
        }
        self.reg.four_family_covered()?;
        // 族成员名与种子名逐族对齐。
        for f in MetricFamily::ALL {
            for m in f.members() {
                let mut found = false;
                let mut i = 0;
                while i < self.reg.len() {
                    if let Some(d) = self.reg.def(i) {
                        if d.name == *m && d.family == f {
                            found = true;
                        }
                    }
                    i += 1;
                }
                if !found {
                    return Err(XC_FAMILY_OOB);
                }
            }
        }
        Ok(())
    }

    /// 组二：口径——同名双公式拒 + 幂等重注册放行 + 口径唯一审计。
    pub fn selfcheck_caliber(&mut self) -> Result<(), u32> {
        // 双公式冲突：同名异指纹。
        if self.reg.enroll("speedup_bp", MetricFamily::Synergy, "het_us*10000/cpu_us", 2).is_ok() {
            return Err(XC_CALIBER_CONFLICT);
        }
        // 幂等：同名声同指纹重注册放行。
        self.reg.enroll("speedup_bp", MetricFamily::Synergy, "cpu_us*10000/het_us", 1)?;
        self.reg.caliber_unique()?;
        Ok(())
    }

    /// 组三：生命周期——废弃后写入拒 / 未注册派生拒 / 废弃指标仍在册可查。
    pub fn selfcheck_lifecycle(&mut self) -> Result<(), u32> {
        self.reg.deprecate("migration_count")?;
        // 废弃后写入拒。
        if self.reg.write("migration_count").is_ok() {
            return Err(XC_LIFECYCLE_VIOLATION);
        }
        // 未注册派生拒。
        if self.reg.derive("ghost_metric", "speedup_bp", 1, 1).is_ok() {
            return Err(XC_LIFECYCLE_VIOLATION);
        }
        // 废弃仍在册可查。
        let mut found = false;
        let mut i = 0;
        while i < self.reg.len() {
            if let Some(d) = self.reg.def(i) {
                if d.name == "migration_count" && d.life == LifeCycle::Deprecated {
                    found = true;
                }
            }
            i += 1;
        }
        if !found {
            return Err(XC_LIFECYCLE_VIOLATION);
        }
        // 活指标写入放行。
        self.reg.write("speedup_bp")?;
        Ok(())
    }

    /// 组四：一致性——派生守恒抽查（正例）+ 失配拦截（反例）。
    pub fn selfcheck_consistency(&mut self) -> Result<(), u32> {
        // 正例：power_milliwatt=30000（30W），fps_per_watt_bp 派生=30000*10000/60/... 
        // 折算式：base*numer_bp/denom = 30000*10000/60 = 5_000_000（bp 口径演示值）。
        self.reg.conserve_check("fps_per_watt_bp", 30_000, 5_000_000)?;
        // 反例：登记值失配。
        if self.reg.conserve_check("fps_per_watt_bp", 30_000, 4_999_999).is_ok() {
            return Err(XC_DERIVED_BROKEN);
        }
        // 未挂边指标抽查拒。
        if self.reg.conserve_check("power_milliwatt", 1, 1).is_ok() {
            return Err(XC_DERIVED_BROKEN);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_families()?;
        self.selfcheck_caliber()?;
        self.selfcheck_lifecycle()?;
        self.selfcheck_consistency()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8500..0x8600 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_FAMILY_OOB,
            XC_CALIBER_CONFLICT,
            XC_LIFECYCLE_VIOLATION,
            XC_DERIVED_BROKEN,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8500 || codes[i] >= 0x8600 {
                return Err(XC_FAMILY_OOB);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_FAMILY_OOB);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
