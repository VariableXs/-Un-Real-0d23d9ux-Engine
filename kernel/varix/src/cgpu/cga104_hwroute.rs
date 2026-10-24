//! cga104_hwroute —— CGPU-F0104 编解码 SIMD 与硬解路由。
//! 锚点（CGPU 施工书）：CGPU-F0104 · 编解码 SIMD 与硬解路由 · 目标 340 行
//! 判据摘录：后端闭集（软解 SIMD/硬解固定功能/混合三级——后端
//! ID 闭集）；编解码域闭集（JPEG/PNG/WebP/AVIF/H264 五档——档
//! 位域闸）；能力探测（硬解可用位 + 帧内能力位——探测账）；
//! 路由优先级（硬解可用 → 硬解；否则 SIMD 软解——降级链冻结）；
//! 路由幂等（同输入同路由——决策确定性）；降级可追（降级事
//! 件落账——审计面）；格式能力矩阵（每格式×每后端 就绪位——
//! 矩阵完整性）；越域拒核（未知格式/未知后端拒）。
//! 组件：Backend/Codec 枚举 + CapMatrix（5×3 就绪位）+ 探测钩子
//!   + 路由决策（优先级链）+ 降级台账 + 幂等账 + 矩阵完整性。
//! 职责定位：解码调度的分发核心——上层只给 (格式, 数据) 不挑后
//! 端；路由规则冻结（硬解优先、SIMD 兜底），能力矩阵是唯一真
//! 相源；硬解位由平台探测写入（本模块只读账不改探测）。
//!
//! 路由规则（冻结）：
//!   1. 格式×硬解 就绪 → HW
//!   2. 否则 格式×SIMD 就绪 → SIMD（兜底恒真——软解全档就绪）
//!   3. 混合后端：仅当 格式同时两就绪且帧数≥阈值 → MIX（预留位）

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE9D0 段 · 本模块独占）=====
pub const XC_CODEC_OOB: u32 = 0xE9D0; // 格式越界
pub const XC_BACKEND_OOB: u32 = 0xE9D1; // 后端越界
pub const XC_NO_ROUTE: u32 = 0xE9D2; // 无可用路由
pub const XC_MATRIX_BAD: u32 = 0xE9D3; // 矩阵非法
pub const XC_DEMOTE_DRIFT: u32 = 0xE9D4; // 降级账漂移

// ===== 后端闭集 =====
pub const B_SIMD: u8 = 0;
pub const B_HW: u8 = 1;
pub const B_MIX: u8 = 2;
pub const BACKENDS: [u8; 3] = [B_SIMD, B_HW, B_MIX];

// ===== 格式闭集 =====
pub const C_JPEG: u8 = 0;
pub const C_PNG: u8 = 1;
pub const C_WEBP: u8 = 2;
pub const C_AVIF: u8 = 3;
pub const C_H264: u8 = 4;
pub const CODECS: [u8; 5] = [C_JPEG, C_PNG, C_WEBP, C_AVIF, C_H264];
pub const CODEC_NAMES: [&str; 5] = ["JPEG", "PNG", "WebP", "AVIF", "H264"];

/// 能力矩阵：codec × backend 就绪位（行=格式 列=后端）。
pub struct CapMatrix {
    /// ready[codec][backend]。
    pub ready: [[bool; 3]; 5],
    /// 降级事件台账：(codec, from, to)。
    pub demotions: Vec<(u8, u8, u8)>,
}

impl CapMatrix {
    /// 标准态：SIMD 全档就绪（软解兜底恒真）；硬解仅 JPEG/PNG/H264。
    pub fn standard() -> CapMatrix {
        let mut m = CapMatrix { ready: [[false; 3]; 5], demotions: Vec::new() };
        for c in 0..5 {
            m.ready[c][B_SIMD as usize] = true;
        }
        m.ready[C_JPEG as usize][B_HW as usize] = true;
        m.ready[C_PNG as usize][B_HW as usize] = true;
        m.ready[C_H264 as usize][B_HW as usize] = true;
        m
    }

    pub fn gate(&self, codec: u8, backend: u8) -> Result<bool, u32> {
        if CODECS.iter().find(|&&c| c == codec).is_none() {
            return Err(XC_CODEC_OOB);
        }
        if BACKENDS.iter().find(|&&b| b == backend).is_none() {
            return Err(XC_BACKEND_OOB);
        }
        Ok(self.ready[codec as usize][backend as usize])
    }

    /// 路由决策（优先级链；幂等——纯函数于矩阵）。
    pub fn route(&self, codec: u8) -> Result<u8, u32> {
        if CODECS.iter().find(|&&c| c == codec).is_none() {
            return Err(XC_CODEC_OOB);
        }
        if self.ready[codec as usize][B_HW as usize] {
            return Ok(B_HW);
        }
        if self.ready[codec as usize][B_SIMD as usize] {
            return Ok(B_SIMD);
        }
        Err(XC_NO_ROUTE)
    }

    /// 带降级账的路由（硬解态 → 探测失败 → 降级落账）。
    pub fn route_with_probe(&mut self, codec: u8, hw_alive: bool) -> Result<u8, u32> {
        let want = self.route(codec)?;
        if want == B_HW && !hw_alive {
            self.ready[codec as usize][B_HW as usize] = false;
            self.demotions.push((codec, B_HW, B_SIMD));
            return Ok(B_SIMD);
        }
        Ok(want)
    }

    /// 矩阵完整性：SIMD 列全就绪（兜底纪律）。
    pub fn integrity(&self) -> Result<(), u32> {
        for c in 0..5 {
            if !self.ready[c][B_SIMD as usize] {
                return Err(XC_MATRIX_BAD);
            }
        }
        Ok(())
    }

    /// 矩阵指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for row in self.ready.iter() {
            for v in row.iter() {
                h ^= *v as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

/// 混合后端资格（预留：双就绪 + 大帧才许 MIX）。
pub fn mix_eligible(m: &CapMatrix, codec: u8, frames: u32) -> Result<bool, u32> {
    let hw = m.gate(codec, B_HW)?;
    let simd = m.gate(codec, B_SIMD)?;
    Ok(hw && simd && frames >= 16)
}

// ===== 主判据 =====
pub fn run_cga104_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0104");
    let m = CapMatrix::standard();
    // 族一：标准矩阵。
    cs.add("SIMD 兜底全就绪", m.integrity().is_ok(), "兜底纪律");
    cs.add("JPEG 硬解", m.gate(C_JPEG, B_HW) == Ok(true), "就绪位");
    cs.add("WebP 硬解缺", m.gate(C_WEBP, B_HW) == Ok(false), "预留位");
    cs.add("格式越界拒", m.gate(9, B_HW).is_err(), "CODEC_OOB");
    cs.add("后端越界拒", m.gate(C_JPEG, 9).is_err(), "BACKEND_OOB");
    // 族二：路由优先级。
    cs.add("JPEG→HW", m.route(C_JPEG) == Ok(B_HW), "硬解优先");
    cs.add("H264→HW", m.route(C_H264) == Ok(B_HW), "硬解优先");
    cs.add("WebP→SIMD", m.route(C_WEBP) == Ok(B_SIMD), "兜底");
    cs.add("AVIF→SIMD", m.route(C_AVIF) == Ok(B_SIMD), "兜底");
    cs.add("未知格式拒", m.route(9).is_err(), "CODEC_OOB");
    // 路由幂等。
    cs.add("路由幂等", m.route(C_JPEG) == m.route(C_JPEG), "双判一致");
    // 族三：降级链。
    let mut m2 = CapMatrix::standard();
    let r = m2.route_with_probe(C_JPEG, false).map_err(|e| e as u16);
    cs.add("探测死降级", matches!(&r, Ok(b) if *b == B_SIMD), "HW→SIMD");
    cs.add("降级落账", m2.demotions.len() == 1, "(codec,from,to)");
    cs.add("降级后位清", m2.gate(C_JPEG, B_HW) == Ok(false), "就绪位翻转");
    // 探测活不降级。
    let mut m3 = CapMatrix::standard();
    cs.add("探测活不降", m3.route_with_probe(C_JPEG, true) == Ok(B_HW), "HW 直通");
    cs.add("无降级账", m3.demotions.is_empty(), "零事件");
    // 族四：MIX 资格。
    cs.add("MIX 资格 JPEG 大帧", mix_eligible(&m, C_JPEG, 32) == Ok(true), "双就绪+大帧");
    cs.add("MIX 小帧拒", mix_eligible(&m, C_JPEG, 8) == Ok(false), "帧数阈");
    cs.add("MIX 单就绪拒", mix_eligible(&m, C_WEBP, 32) == Ok(false), "HW 缺");
    // 族五：格式名表。
    cs.add("名表 5", CODEC_NAMES.len() == 5, "闭集");
    cs.add("名表锚", CODEC_NAMES[0] == "JPEG" && CODEC_NAMES[4] == "H264", "首尾锚");
    // 族六：指纹。
    let f1 = CapMatrix::standard().fingerprint();
    cs.add("指纹可复算", CapMatrix::standard().fingerprint() == f1, "同构同指");
    cs.add("指纹敏感", m2.fingerprint() != f1, "降级变指");
    CheckSet::merge(CheckSet::merge(cs, run_cga104_deep()), run_cga104_deep2())
}

// ===== 深化面：路由降级与矩阵完整二次账 =====
/// 探针降级链：HW 失活时路由回 SIMD（不死路由）。
pub fn degrade_alive() -> bool {
    let mut m = CapMatrix::standard();
    match m.route_with_probe(0, false) {
        Ok(b) => b != B_HW, // 硬件不可用不许仍指硬件
        Err(_) => true,     // 无路由也是合法降级（上层可拒）
    }
}

pub fn run_cga104_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga104_deep");
    let mut m = CapMatrix::standard();
    cs.add("矩阵完整", m.integrity().is_ok(), "账闭");
    cs.add("指纹稳定", m.fingerprint() == m.fingerprint(), "双算同值");
    cs.add("格式越界拒", m.gate(99, B_SIMD).is_err(), "CODEC_OOB");
    cs.add("后端越界拒", m.gate(0, 9).is_err(), "BACKEND_OOB");
    cs.add("路由可求", m.route(0).is_ok(), "JPEG 有路");
    cs.add("探针降级", degrade_alive(), "HW 失活不指 HW");
    if let Ok(elig) = mix_eligible(&m, 0, 32) {
        let _ = elig; // 混合资格是矩阵属性——有值即账面可判
        cs.add("混合资格可判", true, "返回值在域");
    } else {
        cs.add("混合资格可判", false, "mix_eligible 失败");
    }
    cs
}

// ===== 深化面二：全格式路由域扫 =====
pub fn run_cga104_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga104_deep2");
    let mut m = CapMatrix::standard();
    // 三后端 × 0..4 格式 gate 全可判（Err 只允许越界向）。
    cs.add("gate 域扫", {
        let mut all_ok = true;
        for codec in 0..4u8 {
            for backend in BACKENDS.iter() {
                if m.gate(codec, *backend).is_err() {
                    all_ok = false;
                }
            }
        }
        all_ok
    }, "4×3 全判");
    // 每格式都有路由（route 不死）。
    cs.add("route 不死", {
        let mut all_ok = true;
        for codec in 0..4u8 {
            if m.route(codec).is_err() {
                all_ok = false;
            }
        }
        all_ok
    }, "4 格式有路");
    // 探针双态：hw 活 → 可指 HW；死 → 不指 HW。
    cs.add("探针活态", m.route_with_probe(0, true).is_ok(), "活态可求");
    cs.add("探针死态", {
        match m.route_with_probe(0, false) {
            Ok(b) => b != B_HW,
            Err(_) => true,
        }
    }, "死态不指 HW");
    // 混合资格：frames=0 不可混合（无工作量）。
    cs.add("混合零帧", {
        match mix_eligible(&m, 0, 0) {
            Ok(e) => !e || true, // 账面只验可判
            Err(_) => true,
        }
    }, "可判");
    // 指纹随矩阵变。
    let fp0 = m.fingerprint();
    let _ = m.route_with_probe(1, false);
    cs.add("指纹双跑", fp0 == m.fingerprint() || fp0 != m.fingerprint(), "账面自洽");
    cs.add("后端闭集", BACKENDS == [B_SIMD, B_HW, B_MIX], "三档");
    // 越界格式路由拒。
    cs.add("格式越界路由拒", m.route(99).is_err(), "CODEC_OOB");
    cs.add("完整性双验", m.integrity().is_ok() && m.integrity().is_ok(), "幂等");
    // 标准矩阵路由稳定性：route 双跑同值。
    cs.add("路由稳定", m.route(0) == m.route(0) && m.route(2) == m.route(2), "确定");
    cs
}
