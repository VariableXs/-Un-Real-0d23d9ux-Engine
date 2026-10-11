//! CGPU-F1701 · 引擎级调试面板
//!
//! 判据锚点（施工书原文摘录）：
//! 调试面板：面板族（NPU 面板（F1629 复用统一化）/DSP 面板/多 GPU 面板/能效
//! 面板——四面板统一框架（面板框架统一（一致性复用 14 章第十章——框架统一；
//! 面板数据（与注册表对齐（口径一致——数据一致；实时性（面板刷新（1Hz 基础
//! +可调——刷新规范；协议（CIPC 复用；测试（四面板/框架/口径/刷新/协议五组）。
//! 判据：四面板、框架统一、口径、刷新、协议、五组、判据。
//!
//! 诊断码段 0x8801~0x8805（独占）：
//!   0x8801 面板族缺员（四面板任一缺席）
//!   0x8802 框架失统一（面板元数据异构——同一框架两套字段语义）
//!   0x8803 口径失对齐（面板数据源不在注册表/口径 id 漂移）
//!   0x8804 刷新越范（Hz 落在可调区间外/基础档失锚 1Hz）
//!   0x8805 协议失据（CIPC 通道未注册/源单号空）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const DBG_PANEL_VERSION: &str = "K20-dbgpanel-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8801~0x8805）
// ---------------------------------------------------------------------------

pub const XC_PANEL_MISSING: u32 = 0x8801;
pub const XC_FRAME_HETERO: u32 = 0x8802;
pub const XC_CALIBER_MISALIGNED: u32 = 0x8803;
pub const XC_REFRESH_OOR: u32 = 0x8804;
pub const XC_PROTO_NO_EVIDENCE: u32 = 0x8805;

// ---------------------------------------------------------------------------
// 一、面板族四闭集
// ---------------------------------------------------------------------------

/// 面板族四闭集（判据字面：NPU/DSP/多GPU/能效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelKind {
    /// NPU 面板（F1629 复用统一化）。
    Npu,
    /// DSP 面板。
    Dsp,
    /// 多 GPU 面板。
    MultiGpu,
    /// 能效面板。
    Efficacy,
}

impl PanelKind {
    pub const ALL: [PanelKind; 4] =
        [PanelKind::Npu, PanelKind::Dsp, PanelKind::MultiGpu, PanelKind::Efficacy];

    pub const fn name(self) -> &'static str {
        match self {
            PanelKind::Npu => "npu_panel",
            PanelKind::Dsp => "dsp_panel",
            PanelKind::MultiGpu => "multi_gpu_panel",
            PanelKind::Efficacy => "efficacy_panel",
        }
    }

    /// 复用源单号（每面板必有出处——NPU 面板判据点名 F1629）。
    pub const fn origin(self) -> &'static str {
        match self {
            PanelKind::Npu => "CGPU-F1629",
            PanelKind::Dsp => "CGPU-K07-DSP",
            PanelKind::MultiGpu => "CGPU-F1653",
            PanelKind::Efficacy => "CGPU-F1682",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、统一面板框架（框架统一——一套元数据结构四面板同构）
// ---------------------------------------------------------------------------

/// 统一框架元数据：四面板共占一结构（异构字段=0x8802）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelFrame {
    /// 面板族。
    pub kind: PanelKind,
    /// 数据源：注册表指标名（口径对齐锚）。
    pub metric: &'static str,
    /// 口径 id（与注册表登记口径一致——漂移即 0x8803）。
    pub caliber_id: &'static str,
    /// 刷新 mHz（1000=1Hz 基础档；可调区间 500..=10000）。
    pub refresh_mhz: u32,
    /// CIPC 通道 id。
    pub cipc_ch: u16,
    /// 框架在册。
    pub enrolled: bool,
}

/// 面板框架簿：四面板统一登记。
pub struct PanelBook {
    frames: [PanelFrame; 4],
}

/// 口径对齐回调面：宿主（注册表）按指标名报口径 id——面板侧对拍用。
pub trait CaliberSource {
    /// 查指标口径 id；未注册返回 None。
    fn caliber_of(&self, metric: &str) -> Option<&'static str>;
}

/// 宿主模拟：vck17 注册表的口径视图（本模块内自持，测试口径对齐）。
pub struct RegCaliberView {
    rows: [(&'static str, &'static str); 4],
}

impl RegCaliberView {
    pub fn build() -> Self {
        RegCaliberView {
            rows: [
                ("npu_batch_utili", "utili_permille"),
                ("dsp_sched_lat_us", "lat_us_delta"),
                ("mgpu_split_ratio", "ratio_bp"),
                ("eff_fps_per_watt", "fps_per_watt_bp"),
            ],
        }
    }
}

impl CaliberSource for RegCaliberView {
    fn caliber_of(&self, metric: &str) -> Option<&'static str> {
        let mut found = None;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].0 == metric {
                found = Some(self.rows[i].1);
            }
            i += 1;
        }
        found
    }
}

impl PanelBook {
    /// 构造：四面板统一元数据（同构框架 + 各自数据源 + 1Hz 基础档 + CIPC 通道顺位）。
    pub const fn build() -> Self {
        PanelBook {
            frames: [
                PanelFrame {
                    kind: PanelKind::Npu,
                    metric: "npu_batch_utili",
                    caliber_id: "utili_permille",
                    refresh_mhz: 1000,
                    cipc_ch: 0x71,
                    enrolled: true,
                },
                PanelFrame {
                    kind: PanelKind::Dsp,
                    metric: "dsp_sched_lat_us",
                    caliber_id: "lat_us_delta",
                    refresh_mhz: 1000,
                    cipc_ch: 0x72,
                    enrolled: true,
                },
                PanelFrame {
                    kind: PanelKind::MultiGpu,
                    metric: "mgpu_split_ratio",
                    caliber_id: "ratio_bp",
                    refresh_mhz: 1000,
                    cipc_ch: 0x73,
                    enrolled: true,
                },
                PanelFrame {
                    kind: PanelKind::Efficacy,
                    metric: "eff_fps_per_watt",
                    caliber_id: "fps_per_watt_bp",
                    refresh_mhz: 1000,
                    cipc_ch: 0x74,
                    enrolled: true,
                },
            ],
        }
    }

    /// 四面板齐：全部在册。
    pub fn all_present(&self) -> Result<(), u32> {
        for f in &self.frames {
            if !f.enrolled {
                return Err(XC_PANEL_MISSING);
            }
        }
        Ok(())
    }

    /// 框架统一审计：四面板元数据同构（全字段语义一致：metric 非空/caliber 非空/
    /// refresh 在带/通道唯一）——同一框架两套语义即 0x8802。
    pub fn frame_uniform(&self) -> Result<(), u32> {
        let mut used_ch: Vec<u16> = Vec::new();
        for f in &self.frames {
            if f.metric.is_empty() || f.caliber_id.is_empty() {
                return Err(XC_FRAME_HETERO);
            }
            if f.refresh_mhz < REFRESH_MIN_MHZ || f.refresh_mhz > REFRESH_MAX_MHZ {
                return Err(XC_FRAME_HETERO);
            }
            for c in &used_ch {
                if *c == f.cipc_ch {
                    return Err(XC_FRAME_HETERO);
                }
            }
            used_ch.push(f.cipc_ch);
        }
        Ok(())
    }

    /// 口径对齐：每面板数据源在宿主注册表在册且口径 id 与面板侧登记一致。
    pub fn caliber_aligned(&self, src: &dyn CaliberSource) -> Result<(), u32> {
        for f in &self.frames {
            match src.caliber_of(f.metric) {
                Some(reg_caliber) => {
                    if reg_caliber != f.caliber_id {
                        return Err(XC_CALIBER_MISALIGNED);
                    }
                }
                None => return Err(XC_CALIBER_MISALIGNED),
            }
        }
        Ok(())
    }

    /// 查面板。
    pub fn frame(&self, idx: usize) -> Option<&PanelFrame> {
        self.frames.get(idx)
    }
}

// ---------------------------------------------------------------------------
// 三、刷新规范：1Hz 基础 + 可调
// ---------------------------------------------------------------------------

/// 刷新可调区间 mHz（0.5Hz..10Hz）。
pub const REFRESH_MIN_MHZ: u32 = 500;
pub const REFRESH_MAX_MHZ: u32 = 10_000;
/// 基础档 mHz（判据"1Hz 基础"）。
pub const REFRESH_BASE_MHZ: u32 = 1000;

/// 刷新策略：基础档锚 + 可调校验。
pub struct RefreshPolicy;

impl RefreshPolicy {
    /// 基础档恰 1Hz。
    pub const fn base_is_1hz() -> bool {
        REFRESH_BASE_MHZ == 1000
    }

    /// 调节校验：区间内放行，越界拒（0x8804）。
    pub fn tune(mhz: u32) -> Result<u32, u32> {
        if mhz < REFRESH_MIN_MHZ || mhz > REFRESH_MAX_MHZ {
            return Err(XC_REFRESH_OOR);
        }
        Ok(mhz)
    }

    /// 面板刷新调节入口。
    pub fn apply(f: &mut PanelFrame, mhz: u32) -> Result<(), u32> {
        f.refresh_mhz = Self::tune(mhz)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、CIPC 协议复用
// ---------------------------------------------------------------------------

/// CIPC 通道登记（协议复用声明——源单号非空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CipcChannel {
    pub ch_id: u16,
    /// 复用源单号。
    pub origin: &'static str,
    /// 承载面板。
    pub panel: PanelKind,
}

/// 协议簿。
pub struct ProtoBook {
    chans: Vec<CipcChannel>,
}

impl ProtoBook {
    pub fn new() -> Self {
        ProtoBook { chans: Vec::new() }
    }

    /// 注册通道：源单号空拒 / 重通道拒。
    pub fn register(&mut self, ch_id: u16, origin: &'static str, panel: PanelKind) -> Result<(), u32> {
        if origin.is_empty() {
            return Err(XC_PROTO_NO_EVIDENCE);
        }
        for c in &self.chans {
            if c.ch_id == ch_id {
                return Err(XC_PROTO_NO_EVIDENCE);
            }
        }
        self.chans.push(CipcChannel { ch_id, origin, panel });
        Ok(())
    }

    /// 四面板通道全注册。
    pub fn cover_all_panels(&self) -> Result<(), u32> {
        for p in PanelKind::ALL {
            let mut found = false;
            for c in &self.chans {
                if c.panel == p {
                    found = true;
                }
            }
            if !found {
                return Err(XC_PROTO_NO_EVIDENCE);
            }
        }
        Ok(())
    }

    /// 通道数。
    pub fn len(&self) -> usize {
        self.chans.len()
    }
}

// ---------------------------------------------------------------------------
// 自检（五组：四面板/框架/口径/刷新/协议）
// ---------------------------------------------------------------------------

/// 调试面板引擎。
pub struct PanelEngine {
    pub book: PanelBook,
    pub proto: ProtoBook,
}

impl PanelEngine {
    pub fn new() -> Self {
        let mut proto = ProtoBook::new();
        // CIPC 复用四通道（与框架簿通道顺位一致）。
        let _ = proto.register(0x71, "CGPU-F1704", PanelKind::Npu);
        let _ = proto.register(0x72, "CGPU-F1704", PanelKind::Dsp);
        let _ = proto.register(0x73, "CGPU-F1704", PanelKind::MultiGpu);
        let _ = proto.register(0x74, "CGPU-F1704", PanelKind::Efficacy);
        PanelEngine { book: PanelBook::build(), proto }
    }

    /// 组一：四面板全在 + 每面板有复用源单号。
    pub fn selfcheck_panels(&self) -> Result<(), u32> {
        self.book.all_present()?;
        for p in PanelKind::ALL {
            if p.origin().is_empty() {
                return Err(XC_PANEL_MISSING);
            }
        }
        Ok(())
    }

    /// 组二：框架统一——同构审计 + 通道唯一 + 异构反例拦截。
    pub fn selfcheck_frame(&self) -> Result<(), u32> {
        self.book.frame_uniform()?;
        // 异构反例：口径字段清空即拒（同框架出现空语义字段）。
        let mut bad = PanelBook::build();
        bad.frames[2].caliber_id = "";
        if bad.frame_uniform().is_ok() {
            return Err(XC_FRAME_HETERO);
        }
        // 通道撞车反例。
        let mut bad2 = PanelBook::build();
        bad2.frames[3].cipc_ch = 0x71;
        if bad2.frame_uniform().is_ok() {
            return Err(XC_FRAME_HETERO);
        }
        Ok(())
    }

    /// 组三：口径对齐——全面板对齐 + 漂移反例 + 未注册反例。
    pub fn selfcheck_caliber(&self) -> Result<(), u32> {
        let src = RegCaliberView::build();
        self.book.caliber_aligned(&src)?;
        // 漂移反例：面板侧口径 id 改写即拒。
        let mut bad = PanelBook::build();
        bad.frames[0].caliber_id = "utili_bp";
        if bad.caliber_aligned(&src).is_ok() {
            return Err(XC_CALIBER_MISALIGNED);
        }
        // 未注册反例：数据源不在宿主注册表。
        let mut bad2 = PanelBook::build();
        bad2.frames[1].metric = "ghost_metric";
        if bad2.caliber_aligned(&src).is_ok() {
            return Err(XC_CALIBER_MISALIGNED);
        }
        Ok(())
    }

    /// 组四：刷新——基础档 1Hz + 可调边界恰值 + 越界拒 + 面板落笔。
    pub fn selfcheck_refresh(&self) -> Result<(), u32> {
        if !RefreshPolicy::base_is_1hz() {
            return Err(XC_REFRESH_OOR);
        }
        // 边界恰值：500 与 10000 放行。
        if RefreshPolicy::tune(REFRESH_MIN_MHZ)? != 500 {
            return Err(XC_REFRESH_OOR);
        }
        if RefreshPolicy::tune(REFRESH_MAX_MHZ)? != 10_000 {
            return Err(XC_REFRESH_OOR);
        }
        // 越界反例。
        if RefreshPolicy::tune(499).is_ok() || RefreshPolicy::tune(10_001).is_ok() {
            return Err(XC_REFRESH_OOR);
        }
        // 面板落笔：NPU 面板调至 5Hz。
        let mut book = PanelBook::build();
        match &mut book.frames[0] {
            f => RefreshPolicy::apply(f, 5000)?,
        }
        if book.frames[0].refresh_mhz != 5000 {
            return Err(XC_REFRESH_OOR);
        }
        Ok(())
    }

    /// 组五：协议——四面板通道覆盖 + 断据反例 + 重通道反例。
    pub fn selfcheck_proto(&mut self) -> Result<(), u32> {
        self.proto.cover_all_panels()?;
        // 断据反例：源单号空。
        if self.proto.register(0x80, "", PanelKind::Npu).is_ok() {
            return Err(XC_PROTO_NO_EVIDENCE);
        }
        // 重通道反例。
        if self.proto.register(0x71, "CGPU-F1704", PanelKind::Dsp).is_ok() {
            return Err(XC_PROTO_NO_EVIDENCE);
        }
        // 通道数=4。
        if self.proto.len() != 4 {
            return Err(XC_PROTO_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_panels()?;
        self.selfcheck_frame()?;
        self.selfcheck_caliber()?;
        self.selfcheck_refresh()?;
        self.selfcheck_proto()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8800..0x8900 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_PANEL_MISSING,
            XC_FRAME_HETERO,
            XC_CALIBER_MISALIGNED,
            XC_REFRESH_OOR,
            XC_PROTO_NO_EVIDENCE,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8800 || codes[i] >= 0x8900 {
                return Err(XC_PANEL_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_PANEL_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
