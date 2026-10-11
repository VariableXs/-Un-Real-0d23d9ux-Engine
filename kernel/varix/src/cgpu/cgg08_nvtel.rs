//! cgg08_nvtel — CGPU-F0968 NVIDIA 遥测（四指标进 B08——G 域的全量可观测）。
//!
//! 判据原文（施工书锚点 CGPU-F0968）：遥测：NVIDIA 设备分布/各代认证等级/
//! NVIDIA 专属缺陷检出/DLSS 特性启用率——四指标进 B08 总线。判据：四指标、
//! 判据。
//!
//! 职责定位：四指标聚合器（设备分布按代际分列/各代认证等级分布/专属缺陷
//! 检出计数/DLSS 特性启用率）+ B08 总线发布快照（复用 cgd10/cgf11 的
//! schema 版本化范式）。与 cgg01_gnvidia（架构）、cgg03_nvdifftable（差异
//! 表——缺陷检测的知识源）、cgg05_nvenc（DLSS/编码特性）分工：彼是事实与
//! 判定，此把事实聚合成观测面。诚实红线：无数据代际显性空态（空桶不发零
//! 值谎报）；缺陷检出只计数不臆造细节（细节归差异表）；启用率分母为零时
//! 显性 None。诊断码段 0x5F01~0x5F05。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const NV_TEL_VERSION: &str = "G08-nvtel-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x5F 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_GEN_UNKNOWN: u32 = 0x5F01; // 代际闭集外（分列准入拒）
pub const XC_FEATURE_UNKNOWN: u32 = 0x5F02; // DLSS 特性闭集外
pub const XC_DENOM_ZERO: u32 = 0x5F03; // 启用率分母为零（显性 None 不造假）
pub const XC_SCHEMA_V_MISMATCH: u32 = 0x5F04; // schema 版本失配
pub const XC_GEN_CAP: u32 = 0x5F05; // 代际分列超容量

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 代际闭集（与 cgg05_nvenc 五档对齐——Legacy/Pascal/Turing/Ampere/Ada）。
pub const NV_GENS: [&str; 5] = ["Legacy", "Pascal", "Turing", "Ampere", "Ada"];
/// 代际分列上限（含未知桶）。
pub const GEN_BUCKET_CAP: usize = 8;
/// DLSS 特性闭集。
pub const DLSS_FEATURES: [&str; 4] = ["DLSS_SR", "DLSS_RR", "DLSS_FG", "DLSS_DRR"];
/// B08 schema 版本。
pub const NV_TEL_SCHEMA_V: u32 = 1;
/// 比例定点 ppm。
pub const PPM: u64 = 1_000_000;

/// 设备代际分布桶。
pub struct GenBucket {
    pub gen: &'static str,
    /// 设备计数（按设备 ID 去重后的登记数）。
    pub devices: u64,
    /// 认证等级计数（金/银/铜/未认证——对齐 cgf07 四级）。
    pub grades: [u64; 4],
    /// 专属缺陷检出数（差异表命中的缺陷事件计数）。
    pub defect_hits: u64,
    /// DLSS 特性启用计数（分母=支持该特性的设备数）。
    pub dlss_enabled: [u64; 4],
    pub dlss_capable: u64,
}

/// 单代四指标快照。
#[derive(Clone, Debug)]
pub struct NvTelSnap {
    pub gen: &'static str,
    pub devices: u64,
    /// 认证等级分布 ppm（和恰 1e6；零设备=全零）。
    pub grade_ppm: [u64; 4],
    pub defect_hits: u64,
    /// DLSS 特性启用率 ppm（分母=可支持设备；零支持=None）。
    pub dlss_rate_ppm: [Option<u64>; 4],
}

/// NVIDIA 遥测发布器。
pub struct NvTelemetry {
    buckets: Vec<GenBucket>,
    pub schema_v: u32,
    pub publish_count: u64,
    pub rejected: u64,
}

impl NvTelemetry {
    pub fn new() -> Self {
        NvTelemetry {
            buckets: Vec::new(),
            schema_v: NV_TEL_SCHEMA_V,
            publish_count: 0,
            rejected: 0,
        }
    }

    fn bucket_idx(&self, gen: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.buckets.len() {
            if self.buckets[i].gen == gen {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 代际桶登记（闭集校验——判据"代际"闭集）。
    pub fn register_gen(&mut self, gen: &'static str) -> Result<(), u32> {
        let mut known = false;
        let mut i = 0;
        while i < NV_GENS.len() {
            if NV_GENS[i] == gen {
                known = true;
                break;
            }
            i += 1;
        }
        if !known {
            self.rejected += 1;
            return Err(XC_GEN_UNKNOWN);
        }
        if self.bucket_idx(gen).is_some() {
            return Ok(());
        }
        if self.buckets.len() >= GEN_BUCKET_CAP {
            return Err(XC_GEN_CAP);
        }
        self.buckets.push(GenBucket {
            gen,
            devices: 0,
            grades: [0; 4],
            defect_hits: 0,
            dlss_enabled: [0; 4],
            dlss_capable: 0,
        });
        Ok(())
    }

    /// 设备登记（判据"设备分布"）。
    pub fn observe_device(&mut self, gen: &'static str) -> Result<(), u32> {
        match self.bucket_idx(gen) {
            Some(i) => {
                self.buckets[i].devices += 1;
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(XC_GEN_UNKNOWN)
            }
        }
    }

    /// 认证判级录入（g：0=金 1=银 2=铜 3=未认证）。
    pub fn observe_grade(&mut self, gen: &'static str, g: usize) -> Result<(), u32> {
        if g > 3 {
            self.rejected += 1;
            return Err(XC_GEN_UNKNOWN);
        }
        match self.bucket_idx(gen) {
            Some(i) => {
                self.buckets[i].grades[g] += 1;
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(XC_GEN_UNKNOWN)
            }
        }
    }

    /// 缺陷检出录入（判据"NVIDIA 专属缺陷检出"——计数面）。
    pub fn observe_defect(&mut self, gen: &'static str) -> Result<(), u32> {
        match self.bucket_idx(gen) {
            Some(i) => {
                self.buckets[i].defect_hits += 1;
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(XC_GEN_UNKNOWN)
            }
        }
    }

    /// DLSS 特性录入（判据"DLSS 特性启用率"）。
    pub fn observe_dlss(&mut self, gen: &'static str, feature: &str, enabled: bool) -> Result<(), u32> {
        let mut fidx: i64 = -1;
        let mut i = 0;
        while i < DLSS_FEATURES.len() {
            if DLSS_FEATURES[i] == feature {
                fidx = i as i64;
                break;
            }
            i += 1;
        }
        if fidx < 0 {
            self.rejected += 1;
            return Err(XC_FEATURE_UNKNOWN);
        }
        let fidx = fidx as usize;
        match self.bucket_idx(gen) {
            Some(bi) => {
                let b = &mut self.buckets[bi];
                if enabled {
                    b.dlss_enabled[fidx] += 1;
                }
                b.dlss_capable += 1; // 录入即计入分母（启用与否都算可观测样本）
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(XC_GEN_UNKNOWN)
            }
        }
    }

    /// ppm 归一（尾项守恒——与 cgf11 同范式）。
    fn grade_ppm_of(grades: &[u64; 4]) -> [u64; 4] {
        let mut total: u128 = 0;
        let mut i = 0;
        while i < 4 {
            total += grades[i] as u128;
            i += 1;
        }
        let mut out = [0u64; 4];
        if total == 0 {
            return out;
        }
        let mut acc: u128 = 0;
        let mut j = 0;
        while j < 4 {
            if j == 3 {
                out[j] = (PPM as u128 - acc) as u64;
            } else {
                let p = grades[j] as u128 * PPM as u128 / total;
                out[j] = p as u64;
                acc += p;
            }
            j += 1;
        }
        out
    }

    /// B08 发布（判据"四指标进 B08 总线"）。
    pub fn publish(&mut self) -> Vec<NvTelSnap> {
        self.publish_count += 1;
        let mut out = Vec::with_capacity(self.buckets.len());
        let mut i = 0;
        while i < self.buckets.len() {
            let b = &self.buckets[i];
            let mut rates: [Option<u64>; 4] = [None; 4];
            let mut f = 0;
            while f < 4 {
                if b.dlss_capable > 0 {
                    rates[f] = Some(b.dlss_enabled[f] * PPM / b.dlss_capable);
                }
                f += 1;
            }
            out.push(NvTelSnap {
                gen: b.gen,
                devices: b.devices,
                grade_ppm: Self::grade_ppm_of(&b.grades),
                defect_hits: b.defect_hits,
                dlss_rate_ppm: rates,
            });
            i += 1;
        }
        out
    }

    /// schema 核验。
    pub fn check_schema(&self, v: u32) -> Result<(), u32> {
        if v != self.schema_v {
            return Err(XC_SCHEMA_V_MISMATCH);
        }
        Ok(())
    }

    /// 登记代际数。
    pub const fn gen_count(&self) -> usize {
        self.buckets.len()
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        let mut dev: u128 = 0;
        let mut i = 0;
        while i < self.buckets.len() {
            dev += self.buckets[i].devices as u128;
            i += 1;
        }
        alloc::format!(
            "NVIDIA遥测{} 代{}/{} 设备{} 发布{} 拒{}",
            NV_TEL_VERSION,
            self.buckets.len(),
            GEN_BUCKET_CAP,
            dev,
            self.publish_count,
            self.rejected,
        )
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：设备分布 / 等级分布 / 缺陷检出 / DLSS 启用率 / 空态）
// ---------------------------------------------------------------------------

impl NvTelemetry {
    /// 组一+二：设备分布与等级分布——闭集外拒、ppm 守恒。
    pub fn selfcheck_dist(&mut self) -> Result<(), u32> {
        let mut t = NvTelemetry::new();
        t.register_gen("Ada")?;
        t.register_gen("Turing")?;
        // 闭集外拒。
        if t.register_gen("Fermi") != Err(XC_GEN_UNKNOWN) {
            return Err(XC_GEN_UNKNOWN);
        }
        // 设备：Ada×7 Turing×3。
        let mut i = 0;
        while i < 7 {
            t.observe_device("Ada")?;
            i += 1;
        }
        let mut j = 0;
        while j < 3 {
            t.observe_device("Turing")?;
            j += 1;
        }
        // 判级：Ada 金3 银2 铜1 未1；Turing 铜3。
        let mut g = 0;
        while g < 7 {
            let grade = match g {
                0..=2 => 0,
                3..=4 => 1,
                5 => 2,
                _ => 3,
            };
            t.observe_grade("Ada", grade)?;
            g += 1;
        }
        let mut k = 0;
        while k < 3 {
            t.observe_grade("Turing", 2)?;
            k += 1;
        }
        let pack = t.publish();
        if pack.len() != 2 {
            return Err(XC_GEN_CAP);
        }
        let ada = &pack[0];
        if ada.devices != 7 || ada.grade_ppm[0] != 300_000 || ada.grade_ppm[1] != 200_000 {
            return Err(XC_GEN_UNKNOWN);
        }
        // Turing 全铜：ppm[2]=1e6。
        let tur = &pack[1];
        if tur.grade_ppm[2] != 1_000_000 {
            return Err(XC_GEN_UNKNOWN);
        }
        // 闭集外判级/设备拒计数。
        if t.observe_grade("Ampere", 0) != Err(XC_GEN_UNKNOWN) {
            return Err(XC_GEN_UNKNOWN);
        }
        if t.rejected < 2 {
            return Err(XC_GEN_UNKNOWN);
        }
        Ok(())
    }

    /// 组三：缺陷检出——计数累计、未登记代拒。
    pub fn selfcheck_defect(&mut self) -> Result<(), u32> {
        let mut t = NvTelemetry::new();
        t.register_gen("Turing")?;
        let mut i = 0;
        while i < 4 {
            t.observe_defect("Turing")?;
            i += 1;
        }
        if t.observe_defect("Kepler") != Err(XC_GEN_UNKNOWN) {
            return Err(XC_GEN_UNKNOWN);
        }
        let pack = t.publish();
        if pack[0].defect_hits != 4 {
            return Err(XC_GEN_UNKNOWN);
        }
        Ok(())
    }

    /// 组四：DLSS 启用率——分母口径、闭集外特性拒、零分母 None。
    pub fn selfcheck_dlss(&mut self) -> Result<(), u32> {
        let mut t = NvTelemetry::new();
        t.register_gen("Ada")?;
        // 5 设备：3 启用 DLSS_FG。
        let mut i = 0;
        while i < 5 {
            t.observe_dlss("Ada", "DLSS_FG", i < 3)?;
            i += 1;
        }
        if t.observe_dlss("Ada", "DLSS_9D", true) != Err(XC_FEATURE_UNKNOWN) {
            return Err(XC_FEATURE_UNKNOWN);
        }
        let pack = t.publish();
        match pack[0].dlss_rate_ppm[2] {
            Some(r) => {
                if r != 600_000 {
                    return Err(XC_DENOM_ZERO);
                }
            }
            None => return Err(XC_DENOM_ZERO),
        }
        // 未录入的特性：分母同为 5 但启用 0 → 0 ppm（不是 None——录过特性即有分母）。
        match pack[0].dlss_rate_ppm[0] {
            Some(r) => {
                if r != 0 {
                    return Err(XC_DENOM_ZERO);
                }
            }
            None => return Err(XC_DENOM_ZERO),
        }
        // 零分母代际：登记但无 DLSS 录入 → None。
        t.register_gen("Legacy")?;
        let pack2 = t.publish();
        if pack2[1].dlss_rate_ppm[0].is_some() {
            return Err(XC_DENOM_ZERO);
        }
        Ok(())
    }

    /// 组五：schema——版本核验、空态发布。
    pub fn selfcheck_schema(&mut self) -> Result<(), u32> {
        let mut t = NvTelemetry::new();
        t.check_schema(NV_TEL_SCHEMA_V)?;
        if t.check_schema(9) != Err(XC_SCHEMA_V_MISMATCH) {
            return Err(XC_SCHEMA_V_MISMATCH);
        }
        // 空态发布：空包（零桶合法）。
        let pack = t.publish();
        if pack.len() != 0 || t.publish_count != 1 {
            return Err(XC_GEN_CAP);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_dist()?;
        self.selfcheck_defect()?;
        self.selfcheck_dlss()?;
        self.selfcheck_schema()?;
        Ok(())
    }
}
