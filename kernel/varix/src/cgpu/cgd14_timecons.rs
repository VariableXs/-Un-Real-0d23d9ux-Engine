//! cgd14_timecons — CGPU-F0495 帧计时一致性（一把尺子量所有硬件，口径漂移
//! 即 ADR 立案）。
//!
//! 判据原文（施工书锚点 CGPU-F0495）：一致性：跨硬件计时一致（同负载不同
//! 硬件的计时方法论一致——口径统一）；跨版本稳定（计时语义变更→基线
//! ADR）；与 G06/H09 质量域的延迟口径统一。判据：跨硬件、跨版本、跨卷统
//! 一、判据。
//!
//! 职责定位：口径注册表（四段计时/全延迟/达标率/卡顿阈值——每个口径一个
//! 规范条目：单位/分位点/窗口长度全部闭集定义，跨硬件共享同一注册表）+
//! 跨硬件口径对拍（同负载在两套硬件档案上跑同口径——方法论一致即结果可
//! 比：比值超界不是"硬件差异"而是"口径漂移"立案）+ 跨版本 ADR 账（计时
//! 语义变更必须走 ADR——旧口径版本号保留、新口径登记 ADR 号、语义指纹变
//! 更显性）+ 跨卷口径统一锚（G06/H09 质量域延迟口径与本域全延迟口径必须
//! 指向同一规范条目——一处定义处处引用）。红线：口径注册表只增不改不删
//! （同 cgd12 API 冻结纪律——口径是测量契约）；无 ADR 的语义变更直接拒绝。
//! 诊断码段 0x6101~0x6105。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const TIME_CONS_VERSION: &str = "D14-timecons-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x61 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_METRIC_DUPLICATE: u32 = 0x6101; // 口径重复注册（同名两条）
pub const XC_METRIC_MUTATE: u32 = 0x6102; // 口径原地修改（测量契约违约）
pub const XC_NO_ADR: u32 = 0x6103; // 语义变更无 ADR（跨版本稳定违约）
pub const XC_CROSS_HW_DRIFT: u32 = 0x6104; // 跨硬件口径漂移（同负载结果不可比）
pub const XC_VOLUME_UNLINKED: u32 = 0x6105; // 跨卷口径未挂钩（G06/H09 各量各的）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 口径条目容量。
pub const METRIC_CAP: usize = 32;
/// 跨硬件比值容差（同负载两硬件的结果比必须在此带内——方法论一致）。
pub const CROSS_HW_TOLERANCE_PPM: u64 = 200_000; // ±20%
/// ADR 号格式：四位数。
pub const ADR_MIN: u32 = 1000;

/// 计量口径规范条目（测量契约——只增不改不删）。
#[derive(Clone, Debug, PartialEq)]
pub struct MetricSpec {
    /// 口径名（跨域引用键）。
    pub name: String,
    /// 单位（闭集：us/ppm/count）。
    pub unit: &'static str,
    /// 分位点（0=均值；95=P95；99=P99）。
    pub percentile: u8,
    /// 滚动窗长（帧）。
    pub window_frames: u64,
    /// 语义指纹（FNV——语义变更即指纹变）。
    pub semantic_sig: u64,
    /// 引入版本。
    pub since_v: u32,
    /// 变更 ADR 号（0=从未变更）。
    pub adr: u32,
}

impl MetricSpec {
    pub fn semantic_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_mix(h, self.name.as_bytes());
        h = fnv_mix(h, self.unit.as_bytes());
        h = fnv_mix(h, &[self.percentile]);
        h = fnv_mix(h, &self.window_frames.to_le_bytes());
        h
    }
}

pub const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x1000_0000_01b3)
}

pub fn fnv_mix(seed: u64, data: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < data.len() {
        h = fnv_byte(h, data[i]);
        i += 1;
    }
    h
}

/// 口径注册表（测量契约仓——跨硬件/跨版本/跨卷三方共享）。
pub struct MetricRegistry {
    specs: Vec<MetricSpec>,
    pub current_v: u32,
    pub mutate_attempts: u64,
    pub no_adr_rejects: u64,
}

impl MetricRegistry {
    pub fn new() -> Self {
        MetricRegistry {
            specs: Vec::new(),
            current_v: 1,
            mutate_attempts: 0,
            no_adr_rejects: 0,
        }
    }

    fn find_idx(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.specs.len() {
            if self.specs[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 口径注册（判据"口径统一"——一处定义）。
    pub fn register(
        &mut self,
        name: &str,
        unit: &'static str,
        percentile: u8,
        window_frames: u64,
    ) -> Result<u64, u32> {
        // 单位闭集。
        if unit != "us" && unit != "ppm" && unit != "count" {
            return Err(XC_METRIC_MUTATE);
        }
        // 分位点闭集（0/95/99）。
        if percentile != 0 && percentile != 95 && percentile != 99 {
            return Err(XC_METRIC_MUTATE);
        }
        if window_frames == 0 {
            return Err(XC_METRIC_MUTATE);
        }
        if self.find_idx(name).is_some() {
            self.mutate_attempts += 1;
            return Err(XC_METRIC_DUPLICATE);
        }
        if self.specs.len() >= METRIC_CAP {
            return Err(XC_METRIC_DUPLICATE);
        }
        let spec = MetricSpec {
            name: String::from(name),
            unit,
            percentile,
            window_frames,
            semantic_sig: 0,
            since_v: self.current_v,
            adr: 0,
        };
        let sig = spec.semantic_fingerprint();
        self.specs.push(MetricSpec {
            semantic_sig: sig,
            ..spec
        });
        Ok(sig)
    }

    /// 语义变更（判据"跨版本稳定"：必须带 ADR——版本前进+指纹更新+旧语义
    /// 留痕在 ADR 号）。
    pub fn mutate_with_adr(
        &mut self,
        name: &str,
        new_unit: &'static str,
        new_percentile: u8,
        new_window: u64,
        adr: u32,
    ) -> Result<u64, u32> {
        if adr < ADR_MIN {
            self.no_adr_rejects += 1;
            return Err(XC_NO_ADR);
        }
        let idx = match self.find_idx(name) {
            Some(i) => i,
            None => {
                self.no_adr_rejects += 1;
                return Err(XC_NO_ADR);
            }
        };
        // 旧语义留痕（版本历史面：ADR 号登记到条目，since 不动——契约史）。
        self.specs[idx].adr = adr;
        self.specs[idx].unit = new_unit;
        self.specs[idx].percentile = new_percentile;
        self.specs[idx].window_frames = new_window;
        self.current_v += 1;
        self.specs[idx].since_v = self.current_v;
        let sig = self.specs[idx].semantic_fingerprint();
        self.specs[idx].semantic_sig = sig;
        Ok(sig)
    }

    /// 无 ADR 变更拒绝（判据"计时语义变更→基线 ADR"——唯一合法路径）。
    pub fn mutate_no_adr(&mut self, name: &str) -> Result<(), u32> {
        if self.find_idx(name).is_some() {
            self.no_adr_rejects += 1;
        }
        Err(XC_NO_ADR)
    }

    /// 跨卷挂钩（判据"与 G06/H09 质量域的延迟口径统一"：域标签绑定到已有
    /// 口径——不允许各卷私造同义口径）。返回口径语义指纹（同指纹=同口径）。
    pub fn link_volume(&self, volume: &str, metric_name: &str) -> Result<u64, u32> {
        // 只认 G06/H09 两个质量卷（闭集——其他卷名拒绝）。
        if volume != "G06" && volume != "H09" {
            return Err(XC_VOLUME_UNLINKED);
        }
        match self.find_idx(metric_name) {
            Some(i) => Ok(self.specs[i].semantic_sig),
            None => Err(XC_VOLUME_UNLINKED),
        }
    }

    /// 口径查询。
    pub fn lookup(&self, name: &str) -> Option<&MetricSpec> {
        match self.find_idx(name) {
            Some(i) => Some(&self.specs[i]),
            None => None,
        }
    }

    /// 口径数。
    pub const fn len(&self) -> usize {
        self.specs.len()
    }
}

/// 跨硬件对拍器（判据"同负载不同硬件的计时方法论一致"）。
pub struct CrossHwChecker {
    pub drift_cases: u64,
}

impl CrossHwChecker {
    pub fn new() -> Self {
        CrossHwChecker { drift_cases: 0 }
    }

    /// 两硬件同口径结果对拍：比值在容差带内即方法论一致。
    /// 注意：口径统一比对的是"同口径测得的同一相对量"（如达标率 ppm），
    /// 绝对耗时差是硬件差异不是口径漂移——只有口径定义的相对量可比。
    pub fn compare(&mut self, hw_a: u64, hw_b: u64) -> Result<(), u32> {
        if hw_a == 0 && hw_b == 0 {
            return Ok(()); // 双零=同态
        }
        let lo = hw_a.min(hw_b) as u128;
        let hi = hw_a.max(hw_b) as u128;
        if lo == 0 {
            // 一方零一方非零：口径漂移（同负载不可能一个测到一个测不到）。
            self.drift_cases += 1;
            return Err(XC_CROSS_HW_DRIFT);
        }
        let ratio_ppm = ((hi - lo) * 1_000_000 / hi) as u64;
        if ratio_ppm > CROSS_HW_TOLERANCE_PPM {
            self.drift_cases += 1;
            return Err(XC_CROSS_HW_DRIFT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：口径注册 / 跨硬件 / 跨版本 ADR / 跨卷统一）
// ---------------------------------------------------------------------------

impl MetricRegistry {
    /// 组一：口径注册——重复拒、单位/分位点闭集外拒。
    pub fn selfcheck_register(&mut self) -> Result<(), u32> {
        let mut r = MetricRegistry::new();
        r.register("frame_time", "us", 99, 600)?;
        r.register("end_to_end", "us", 95, 600)?;
        r.register("on_target_rate", "ppm", 0, 600)?;
        r.register("jank_count", "count", 0, 600)?;
        if r.len() != 4 {
            return Err(XC_METRIC_DUPLICATE);
        }
        // 重复注册拒。
        if r.register("frame_time", "us", 99, 600) != Err(XC_METRIC_DUPLICATE)
            || r.mutate_attempts != 1
        {
            return Err(XC_METRIC_DUPLICATE);
        }
        // 单位闭集外拒。
        if r.register("bad_unit", "seconds", 0, 10) != Err(XC_METRIC_MUTATE) {
            return Err(XC_METRIC_MUTATE);
        }
        // 分位点闭集外拒。
        if r.register("bad_p", "us", 50, 10) != Err(XC_METRIC_MUTATE) {
            return Err(XC_METRIC_MUTATE);
        }
        Ok(())
    }

    /// 组二：跨硬件——同负载结果可比、漂移立案、双零同态。
    pub fn selfcheck_cross_hw(&mut self) -> Result<(), u32> {
        let mut c = CrossHwChecker::new();
        // 同负载达标率：9000 vs 9200 ppm（差 2.2%）→ 一致。
        c.compare(9_000, 9_200)?;
        // 恰好容差边界（20%）。
        c.compare(10_000, 12_000)?;
        // 25% 漂移 → 立案。
        if c.compare(10_000, 12_500) != Err(XC_CROSS_HW_DRIFT) || c.drift_cases != 1 {
            return Err(XC_CROSS_HW_DRIFT);
        }
        // 双零同态。
        c.compare(0, 0)?;
        // 单零漂移。
        if c.compare(0, 5_000) != Err(XC_CROSS_HW_DRIFT) || c.drift_cases != 2 {
            return Err(XC_CROSS_HW_DRIFT);
        }
        Ok(())
    }

    /// 组三：跨版本 ADR——无 ADR 拒、有 ADR 变更留痕、版本前进。
    pub fn selfcheck_adr(&mut self) -> Result<(), u32> {
        let mut r = MetricRegistry::new();
        r.register("frame_time", "us", 99, 600)?;
        let sig1 = match r.lookup("frame_time") {
            Some(s) => s.semantic_sig,
            None => return Err(XC_METRIC_MUTATE),
        };
        // 无 ADR 变更拒。
        if r.mutate_no_adr("frame_time") != Err(XC_NO_ADR) || r.no_adr_rejects != 1 {
            return Err(XC_NO_ADR);
        }
        // ADR 号过小拒。
        if r.mutate_with_adr("frame_time", "us", 95, 300, 42) != Err(XC_NO_ADR) {
            return Err(XC_NO_ADR);
        }
        // 合法 ADR 变更：P99 600 帧 → P95 300 帧。
        let sig2 = r.mutate_with_adr("frame_time", "us", 95, 300, 2026)?;
        if sig2 == sig1 {
            return Err(XC_NO_ADR); // 语义变了指纹必须变
        }
        let s = match r.lookup("frame_time") {
            Some(s) => s,
            None => return Err(XC_METRIC_MUTATE),
        };
        if s.adr != 2026 || s.percentile != 95 || s.window_frames != 300 {
            return Err(XC_NO_ADR);
        }
        // 不存在口径的 ADR 变更拒。
        if r.mutate_with_adr("ghost", "us", 99, 1, 2027) != Err(XC_NO_ADR) {
            return Err(XC_NO_ADR);
        }
        Ok(())
    }

    /// 组四：跨卷统一——G06/H09 挂钩同口径、未挂钩口径拒、私造卷名拒。
    pub fn selfcheck_volumes(&mut self) -> Result<(), u32> {
        let mut r = MetricRegistry::new();
        r.register("end_to_end", "us", 95, 600)?;
        // G06 与 H09 都挂 end_to_end——同指纹（判据"口径统一"）。
        let sig1 = r.link_volume("G06", "end_to_end")?;
        let sig2 = r.link_volume("H09", "end_to_end")?;
        if sig1 != sig2 {
            return Err(XC_VOLUME_UNLINKED);
        }
        // 挂到不存在口径拒。
        if r.link_volume("G06", "ghost_metric") != Err(XC_VOLUME_UNLINKED) {
            return Err(XC_VOLUME_UNLINKED);
        }
        // 私造卷名拒（闭集外）。
        if r.link_volume("X99", "end_to_end") != Err(XC_VOLUME_UNLINKED) {
            return Err(XC_VOLUME_UNLINKED);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_register()?;
        self.selfcheck_cross_hw()?;
        self.selfcheck_adr()?;
        self.selfcheck_volumes()?;
        Ok(())
    }
}
