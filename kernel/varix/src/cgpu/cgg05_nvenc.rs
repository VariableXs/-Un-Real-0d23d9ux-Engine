//! cgg05_nvenc — CGPU-F0965 NVIDIA NVENC 媒体联动（编解码能力的算力标签化）。
//!
//! 判据原文（施工书锚点 CGPU-F0965）：NVENC 联动：NVENC 编解码能力（NVENC
//! 代际探测（编码器数量/AV1 支持）——算力标签登记对接 VE 视频管线；媒体选
//! 路 NVENC vs shader vs CPU）；代际差异诚实标注；联动验证。判据：代际探测、
//! 标签、选路、验证、判据。
//!
//! 职责定位：NVENC 代际闭集表（编码器数/AV1/特性差）+算力标签登记（对接 VE
//! 视频管线的消费口径）+媒体选路三路仲裁（NVENC/shader/CPU）+联动验证。与
//! cgg02_nvprobe（F0962）分工——彼管 3D 特性，此管媒体编解码专项。代际差异
//! 诚实标注：不虚报不存在的能力。诊断码段 0x4Dxx。

use alloc::vec::Vec;

/// 模块版本。
pub const NVENC_VERSION: &str = "G05-nvenc-v1";

/// NVENC 代际闭集（Kepler 起；AV1 自 Ada 起——诚实标注的数字基础）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum NvencGen {
    /// 旧代（Kepler~Maxwell）：H.264 单路。
    Legacy,
    /// Pascal 代：H.264/H.265 双路。
    Pascal,
    /// Turing 代：H.265 B 帧+HEVC 8K。
    Turing,
    /// Ampere 代：双编码器并行。
    Ampere,
    /// Ada 代：AV1 编解码+双 NVENC。
    Ada,
}

impl NvencGen {
    pub const ALL: [NvencGen; 5] = [
        NvencGen::Legacy,
        NvencGen::Pascal,
        NvencGen::Turing,
        NvencGen::Ampere,
        NvencGen::Ada,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            NvencGen::Legacy => 0,
            NvencGen::Pascal => 1,
            NvencGen::Turing => 2,
            NvencGen::Ampere => 3,
            NvencGen::Ada => 4,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            NvencGen::Legacy => "旧代(Kepler~Maxwell)",
            NvencGen::Pascal => "Pascal 代",
            NvencGen::Turing => "Turing 代",
            NvencGen::Ampere => "Ampere 代",
            NvencGen::Ada => "Ada 代",
        }
    }

    /// 编码器数量（诚实标注——代际表写死）。
    pub const fn encoders(self) -> u8 {
        match self {
            NvencGen::Legacy => 1,
            NvencGen::Pascal => 1,
            NvencGen::Turing => 1,
            NvencGen::Ampere => 2,
            NvencGen::Ada => 2,
        }
    }

    /// AV1 支持（诚实标注：仅 Ada 起）。
    pub const fn av1(self) -> bool {
        matches!(self, NvencGen::Ada)
    }

    /// H.265 支持（Pascal 起）。
    pub const fn hevc(self) -> bool {
        !matches!(self, NvencGen::Legacy)
    }

    /// B 帧 H.265（Turing 起）。
    pub const fn hevc_bframes(self) -> bool {
        matches!(self, NvencGen::Turing | NvencGen::Ampere | NvencGen::Ada)
    }

    /// 算力标签（对接 VE 视频管线的登记口径——ppm 相对吞吐：
    /// 以 Legacy 为基线 100_000，代际倍增近似，跨版本改动走台账）。
    pub const fn throughput_ppm(self) -> u64 {
        match self {
            NvencGen::Legacy => 100_000,
            NvencGen::Pascal => 150_000,
            NvencGen::Turing => 220_000,
            NvencGen::Ampere => 320_000,
            NvencGen::Ada => 450_000,
        }
    }
}

/// 媒体选路三路闭集（判据"NVENC vs shader vs CPU"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaPath {
    /// NVENC 硬件编码（首选：能效最优）。
    Nvenc,
    /// Shader 编码（GPU 通用算力兜底——NVENC 忙/不支持时）。
    Shader,
    /// CPU 编码（最后兜底：兼容性最强能效最差）。
    Cpu,
}

impl MediaPath {
    pub const fn zh(self) -> &'static str {
        match self {
            MediaPath::Nvenc => "NVENC 硬件编码",
            MediaPath::Shader => "Shader 编码",
            MediaPath::Cpu => "CPU 编码",
        }
    }
}

/// 编解码需求行（选路输入）。
#[derive(Clone, Copy, Debug)]
pub struct CodecDemand {
    /// 是否 AV1 负载。
    pub need_av1: bool,
    /// 是否 H.265 负载。
    pub need_hevc: bool,
    /// 是否需要 B 帧。
    pub need_bframes: bool,
    /// 并行编码路数需求。
    pub parallel_sessions: u8,
}

/// 诊断码（0x4Dxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4D01 代际未探测（先探测再选路）。
    NeGenUnknown,
    /// 0x4D02 需求超代际能力（AV1/B帧/路数——诚实拒绝不虚报）。
    NeDemandOver,
    /// 0x4D03 需求非法（零会话）。
    NeDemandInvalid,
    /// 0x4D04 联动验证失败（选路后标签与实测不符）。
    NeVerifyFail,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::NeGenUnknown => "NVENC 代际未探测：先探测再选路（不虚报不猜）",
            XcCode::NeDemandOver => "需求超代际能力：诚实拒绝并给替代路（代际差异标注）",
            XcCode::NeDemandInvalid => "需求非法：并行会话数必须 ≥1",
            XcCode::NeVerifyFail => "联动验证失败：选路结论与标签登记不一致",
        }
    }
}

/// NVENC 联动器（判据本体）。
pub struct NvencLink {
    /// 已探测代际（None=未探测）。
    gen: Option<NvencGen>,
    /// 登记的算力标签（对接 VE 视频管线）。
    pub registered_ppm: Option<u64>,
    /// 选路验证账（(会话, 路径) 留痕）。
    pub verify_log: Vec<(u32, MediaPath)>,
    /// 立案计数。
    pub cases: u32,
}

impl NvencLink {
    pub fn new() -> Self {
        NvencLink { gen: None, registered_ppm: None, verify_log: Vec::new(), cases: 0 }
    }

    /// 代际探测回填（判据"代际探测"）。
    pub fn probe(&mut self, g: NvencGen) {
        self.gen = Some(g);
    }

    /// 已探测代际。
    pub const fn gen(&self) -> Option<NvencGen> {
        self.gen
    }

    /// 算力标签登记（判据"算力标签登记对接 VE 视频管线"）。
    pub fn register_label(&mut self) -> Result<u64, XcCode> {
        let g = self.gen.ok_or(XcCode::NeGenUnknown)?;
        let ppm = g.throughput_ppm();
        self.registered_ppm = Some(ppm);
        Ok(ppm)
    }

    /// 媒体选路（判据主路径：需求 vs 代际能力闭集匹配 →
    /// ①能力齐且会话未满 → NVENC；②代际不支持某特性或会话满 → Shader；
    /// ③shader 也不堪（调用方声明 gpu_busy_ppm 高）→ CPU。诚实拒绝：
    /// 需求超能力时返回 Err 带替代说明——不静默降质）。
    pub fn route(&mut self, demand: &CodecDemand, gpu_busy_ppm: u64) -> Result<MediaPath, XcCode> {
        if demand.parallel_sessions == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeDemandInvalid);
        }
        let g = self.gen.ok_or(XcCode::NeGenUnknown)?;
        // 诚实标注校验：需求逐项 vs 代际能力
        if demand.need_av1 && !g.av1() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeDemandOver); // AV1 需 Ada——诚实拒
        }
        if demand.need_hevc && !g.hevc() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeDemandOver);
        }
        if demand.need_bframes && !g.hevc_bframes() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeDemandOver);
        }
        // 会话容量：编码器数 × 每器 3 路（工程基线，台账对拍口径）
        let cap = (g.encoders() as u32) * 3;
        let p = if (demand.parallel_sessions as u32) <= cap {
            MediaPath::Nvenc
        } else if gpu_busy_ppm < 800_000 {
            MediaPath::Shader // NVENC 满但 GPU 算力有余
        } else {
            MediaPath::Cpu // GPU 也不堪——CPU 兜底
        };
        Ok(p)
    }

    /// 联动验证（判据"联动验证"：选路后登记标签与实测吞吐对拍——
    /// 实测 ppm 相对标签偏差 ≤30% 为过；失守立案）。
    pub fn verify(&mut self, session: u32, path: MediaPath, measured_ppm: u64) -> Result<(), XcCode> {
        self.verify_log.push((session, path));
        if path != MediaPath::Nvenc {
            return Ok(()); // 非 NVENC 路径不与 NVENC 标签对拍
        }
        let reg = self.registered_ppm.ok_or(XcCode::NeGenUnknown)?;
        let diff = if measured_ppm > reg {
            measured_ppm - reg
        } else {
            reg - measured_ppm
        };
        if diff * 1_000_000 / reg > 300_000 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeVerifyFail);
        }
        Ok(())
    }

    /// 代际差异诚实标注表（判据"代际差异诚实标注"——消费方展示用）。
    pub fn honesty_table() -> [(NvencGen, u8, bool, bool, bool); 5] {
        let mut t = [(NvencGen::Legacy, 0, false, false, false); 5];
        for (i, g) in NvencGen::ALL.iter().enumerate() {
            t[i] = (*g, g.encoders(), g.av1(), g.hevc(), g.hevc_bframes());
        }
        t
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        let g = match self.gen {
            Some(g) => g.zh(),
            None => "未探测",
        };
        alloc::format!(
            "NVENC{} 代际{} 标签{} 验证{} 立案{}",
            NVENC_VERSION,
            g,
            self.registered_ppm.map(|p| p / 1000).unwrap_or(0),
            self.verify_log.len(),
            self.cases,
        )
    }
}


/// 编码会话账（深化：并行会话的分配与回收——选路通过即占坑，回收释放；
/// 容量判定从"瞬时请求"升级为"存量会话+新请求"的联合判定）。
#[derive(Clone, Copy, Debug)]
pub struct NvencSession {
    /// 会话 id。
    pub sid: u32,
    /// 占用路径（NVENC 会话才占编码器坑位）。
    pub path: MediaPath,
}

#[derive(Clone, Debug)]
pub struct SessionBook {
    /// 在册会话。
    pub sessions: Vec<NvencSession>,
}

impl SessionBook {
    pub fn new() -> Self {
        SessionBook { sessions: Vec::new() }
    }

    /// NVENC 坑位占用数。
    pub fn nvenc_occupied(&self) -> u32 {
        let mut n = 0u32;
        for s in &self.sessions {
            if s.path == MediaPath::Nvenc {
                n += 1;
            }
        }
        n
    }

    /// 容量判定（gen：编码器数 × 3 路基线）。
    pub fn has_room(&self, g: NvencGen, extra: u32) -> bool {
        let cap = (g.encoders() as u32) * 3;
        self.nvenc_occupied() + extra <= cap
    }

    /// 登记。
    pub fn admit(&mut self, sid: u32, path: MediaPath) {
        self.sessions.push(NvencSession { sid, path });
    }

    /// 回收。
    pub fn release(&mut self, sid: u32) {
        self.sessions.retain(|s| s.sid != sid);
    }
}

impl NvencLink {
    /// 存量感知选路（深化：route 的会话账版——存量占用参与容量判定）。
    pub fn route_with_book(
        &mut self,
        demand: &CodecDemand,
        gpu_busy_ppm: u64,
        book: &SessionBook,
    ) -> Result<MediaPath, XcCode> {
        let g = self.gen.ok_or(XcCode::NeGenUnknown)?;
        if demand.parallel_sessions == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeDemandInvalid);
        }
        if demand.need_av1 && !g.av1() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::NeDemandOver);
        }
        let cap = (g.encoders() as u32) * 3;
        let free = cap.saturating_sub(book.nvenc_occupied());
        if (demand.parallel_sessions as u32) <= free {
            Ok(MediaPath::Nvenc)
        } else if gpu_busy_ppm < 800_000 {
            Ok(MediaPath::Shader)
        } else {
            Ok(MediaPath::Cpu)
        }
    }
}
