//! CGPU-F1926 · 显示缩放与 DPI（CGPU-M 域 · 显示输出域 · M03 组 · 目标 320 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1926`
//!
//! **判据（锚点原文）**：缩放路径、DPI 复用、清晰度、联动、四组、判据。
//!
//! **职责定位（锚点原文）**：缩放（显示缩放（GPU 缩放 vs 显示器缩放——
//! 缩放路径——路径选择；DPI（DPI 传递复用——DPI 复用；清晰度（缩放
//! 清晰度（整数倍优先——清晰度规范；与 O 域（应用 DPI 适配联动（O 域
//! 联动——联动声明。
//!
//! ## 一、缩放路径（判据一）
//!
//! 路径二选一（[`ScalePath`]）：GPU 缩放（源像素在 GPU 上放大到目标
//! 模式——延迟低一帧内完成、全屏独占可用、采样质量由 GPU 滤波档决定）；
//! 显示器缩放（源以原生分辨率送出、显示器自行放大——延迟多一跳、
//! 质量取决于显示器scaler、GPU 零开销）。路径选择（[`choose_path`]）
//! 是显性决策表：全屏游戏+低延迟诉求→GPU；桌面混排+文本清晰诉求→
//! 显示器（文本走显示器的次像素 scaler 通常优于 GPU 双线性）。窗口化
//! 无缩放路径（合成器直接 1:1——缩放决策只属于独占全屏与模式切换）。
//!
//! ## 二、DPI 复用（判据二）
//!
//! DPI 传递（[`DpiChain`]）复用 M 域既有链路：EDID 物理尺寸（F1923
//! 能力面）→ 系统 DPI（本模块换算）→ 应用 DPI（O 域消费）。换算是
//! 纯整数算术（ppm 口径：physical_mm × dpi_scale——1440mm 高 × 25.4
//! 毫英寸），显示器物理对角线英寸 = sqrt(mm²)²用 isqrt（D 域整数根
//! 复用家族——零浮点）。DPI 档位四档（100/125/150/200——Windows 惯例
//! 档位对齐，应用适配面成本最低）。
//!
//! ## 三、缩放清晰度（判据三）
//!
//! 整数倍优先规范（[`clarity_verdict`]）：缩放比恰为整数（2×/3×）时
//! 逐像素映射零模糊——优先选能达到整数倍的显示模式；非整数倍时
//! 按最近邻/双线性/锐化滤波给出清晰度裁决与建议（比如 1440p→4K
//! 恰 1.333×：建议改用整数模式或开启锐化）。裁决是建议不是强制
//! （模式切换的执行权在 F1925 模式管理面）。
//!
//! ## 四、O 域联动（判据四）
//!
//! 联动声明（[`OdomainLink`]）：本模块产出的系统 DPI 是 O 域应用适配
//! 的**唯一输入**（单点传递——应用各自探测 DPI 是漂移之源）；DPI 变更
//! 事件带版本号（应用按版本对账防旧事件回放）；O 域适配失败不回写
//! 本模块（单向通报——回流走 O 域自己的账）。
//!
//! **对接**：F1925（显示模式管理——模式切换执行权）；F1923（EDID——
//! 物理尺寸上游）；O 域（应用 DPI 适配——DPI 消费面）；F1922（热插拔
//! ——DPI 链随显示器到达重建）。零 panic 面、零 IO、零墙钟、no_std、
//! 零浮点。

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SCALING_DPI_VERSION: &str = "M03-scaling-v1";

/// DPI 档位闭集（Windows 惯例四档；应用适配面成本最低）。
pub const DPI_STEPS_PPM: [u32; 4] = [100, 125, 150, 200];

/// 毫英寸（1 英寸=25.4mm——物理换算常量）。
pub const MM_PER_INCH_X10: u32 = 254;

// ---------------------------------------------------------------------------
// 二、诊断码（0x36 段独占：M03 缩放 DPI 面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x36 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 物理尺寸非法（0mm——EDID 未闭合）。
    BadPhysicalSize,
    /// DPI 档位表外（闭集四档之外）。
    UnknownDpiStep,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::BadPhysicalSize => "物理尺寸 0mm：EDID 未闭合不是数据，回查 F1923 能力面",
            XcCode::UnknownDpiStep => "DPI 档位表外：四档闭集 100/125/150/200，建议按档位取用",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、缩放路径（判据一）
// ---------------------------------------------------------------------------

/// 缩放路径二态（窗口化无缩放路径——1:1 直通不进本闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalePath {
    /// GPU 缩放（延迟低一帧内、采样质量由 GPU 滤波档决定）。
    GpuScale,
    /// 显示器缩放（GPU 零开销、质量看显示器 scaler、多一跳延迟）。
    MonitorScale,
}

impl ScalePath {
    /// 路径名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            ScalePath::GpuScale => "GPU 缩放",
            ScalePath::MonitorScale => "显示器缩放",
        }
    }
}

/// 路径选择输入（显性决策表的事实面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathFacts {
    /// 独占全屏（窗口化直接 1:1 无需决策）。
    pub exclusive_fullscreen: bool,
    /// 低延迟诉求（游戏/交互）。
    pub latency_sensitive: bool,
    /// 文本密集（桌面混排/文档）。
    pub text_heavy: bool,
}

/// 路径选择（决策表显性：独占全屏缺一不可入决策——窗口化直接 1:1；
/// 低延迟优先 GPU；文本密集优先显示器；其余默认 GPU（源侧可控性））。
pub const fn choose_path(f: PathFacts) -> Option<ScalePath> {
    if !f.exclusive_fullscreen {
        return None; // 窗口化：合成器 1:1，无缩放决策
    }
    if f.latency_sensitive {
        return Some(ScalePath::GpuScale);
    }
    if f.text_heavy {
        return Some(ScalePath::MonitorScale);
    }
    Some(ScalePath::GpuScale)
}

// ---------------------------------------------------------------------------
// 四、DPI 传递（判据二）
// ---------------------------------------------------------------------------

/// DPI 链（EDID 物理尺寸 → 系统 DPI → 应用档位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DpiChain {
    /// 显示器物理对角线（mm；EDID 注入）。
    pub diag_mm: u32,
    /// 水平像素数（当前模式）。
    pub px_w: u32,
    /// 垂直像素数。
    pub px_h: u32,
}

impl DpiChain {
    /// 整数 PPI（像素每英寸；isqrt 整数根——√(w²+h²)/diag_inch，
    /// 零浮点：先算 (w²+h²)×100 / diag_mm² 再开根放大 10 倍精度）。
    pub const fn ppi(&self) -> Result<u32, XcCode> {
        if self.diag_mm == 0 {
            return Err(XcCode::BadPhysicalSize);
        }
        let w2 = (self.px_w as u64) * (self.px_w as u64);
        let h2 = (self.px_h as u64) * (self.px_h as u64);
        // diag_px = √(w²+h²)；diag_px/diag_inch = diag_px×25.4/diag_mm。
        // 避免中途开根损失：PPI ≈ √(w²+h²)×254/(diag_mm×10)。
        let sum2 = w2.saturating_add(h2);
        let diag_px_x10 = crate::cgpu::cgd04_frameinterval::isqrt(sum2) * 10;
        let ppi = diag_px_x10.saturating_mul(MM_PER_INCH_X10 as u64)
            / (self.diag_mm as u64).saturating_mul(10);
        Ok(ppi as u32)
    }

    /// 最近 DPI 档位（四档闭集取最近——|ppi−step| 最小；平局取低档
    /// 防过放大）。
    pub fn nearest_step(&self) -> Result<u32, XcCode> {
        let ppi = self.ppi()? as i64;
        let mut best = DPI_STEPS_PPM[0] as i64;
        let mut best_d = i64::MAX;
        let mut i = 0usize;
        while i < DPI_STEPS_PPM.len() {
            let step = DPI_STEPS_PPM[i] as i64;
            let d = (ppi - step).abs();
            if d < best_d {
                best_d = d;
                best = step;
            }
            i += 1;
        }
        Ok(best as u32)
    }
}

/// 系统 DPI 结论（O 域联动的唯一输入载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemDpi {
    /// 实测 PPI（整数根）。
    pub measured_ppi: u32,
    /// 归档档位（四档之一）。
    pub step: u32,
}

/// DPI 换算入口（M 域单点：链入 → 系统档位；O 域只认这个）。
pub fn resolve_dpi(chain: &DpiChain) -> Result<SystemDpi, XcCode> {
    Ok(SystemDpi {
        measured_ppi: chain.ppi()?,
        step: chain.nearest_step()?,
    })
}

// ---------------------------------------------------------------------------
// 五、缩放清晰度（判据三）
// ---------------------------------------------------------------------------

/// 清晰度裁决（整数倍优先规范）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClarityVerdict {
    /// 整数倍逐像素映射（零模糊——最优）。
    IntegerExact {
        /// 整数倍率。
        factor: u32,
    },
    /// 非整数倍（建议换整数模式或开锐化——给出建议倍率）。
    NonInteger {
        /// 建议的最近整数倍率（向下取整；<1 表示降采样）。
        suggest_factor: u32,
    },
}

impl ClarityVerdict {
    /// 裁决名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            ClarityVerdict::IntegerExact { .. } => "整数倍零模糊",
            ClarityVerdict::NonInteger { .. } => "非整数倍需决策",
        }
    }
}

/// 清晰度裁决（源分辨率 → 目标分辨率；整数倍优先，非整数给出建议）。
pub const fn clarity_verdict(src_px: u32, dst_px: u32) -> ClarityVerdict {
    if src_px == 0 || dst_px == 0 {
        return ClarityVerdict::NonInteger { suggest_factor: 0 };
    }
    if dst_px >= src_px {
        let factor = dst_px / src_px; // 整数除法向下取整
        if factor > 0 && dst_px == factor * src_px {
            return ClarityVerdict::IntegerExact { factor };
        }
        ClarityVerdict::NonInteger {
            suggest_factor: if factor == 0 { 1 } else { factor },
        }
    } else {
        // 降采样：整数倍语义不适用（<1），建议按 1 处理（1:1 或显示器自缩）。
        ClarityVerdict::NonInteger { suggest_factor: 1 }
    }
}

// ---------------------------------------------------------------------------
// 六、O 域联动（判据四）
// ---------------------------------------------------------------------------

/// O 域联动通告（单点传递 + 版本对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OdomainLink {
    /// 系统档位（resolve_dpi 的结论）。
    pub step: u32,
    /// 通告版本号（DPI 变更递增；应用按版本对账防旧事件回放）。
    pub revision: u64,
    /// 单向通报声明（本结构只出不负——O 域适配失败不回写本模块）。
    pub one_way: bool,
}

/// 通告构造（revision 由调用方计数器维护——零墙钟：单调由调用方保证）。
pub const fn announce(step: u32, revision: u64) -> OdomainLink {
    OdomainLink {
        step,
        revision,
        one_way: true,
    }
}

/// DPI 链摘要（人读）。
pub fn dpi_summary(chain: &DpiChain) -> String {
    let mut s = String::from("DPI：PPI ");
    match chain.ppi() {
        Ok(p) => s.push_str(&p.to_string()),
        Err(e) => s.push_str(e.detail()),
    }
    s.push_str("，档位 ");
    match chain.nearest_step() {
        Ok(v) => s.push_str(&v.to_string()),
        Err(_) => s.push('？'),
    }
    s.push('%');
    s
}

// ---------------------------------------------------------------------------
// 七、缩放比与整数模式候选（判据三的深化面）
// ---------------------------------------------------------------------------

/// 缩放比（ppm；dst/src——判据三的机器可检表述；src 为零显性拒）。
pub const fn scale_ratio_ppm(src_px: u32, dst_px: u32) -> Result<u64, XcCode> {
    if src_px == 0 {
        return Err(XcCode::BadPhysicalSize); // 语义：零源分辨率按规格非法拒
    }
    Ok((dst_px as u64).saturating_mul(1_000_000) / src_px as u64)
}

/// 整数模式候选（判据三的深化：给定源宽度，列出可达整数倍的候选
/// 目标宽度（×2/×3/×4）与对应 16:9 高度——模式切换面（F1925）的
/// 建议输入；返回 (factor, dst_w, dst_h) 三元组清单）。
pub fn integer_candidates(src_w: u32, src_h: u32) -> alloc::vec::Vec<(u32, u32, u32)> {
    let mut out = alloc::vec::Vec::new();
    let mut f = 2u32;
    while f <= 4 {
        let w = src_w.saturating_mul(f);
        let h = src_h.saturating_mul(f);
        // 4K 红线：候选不超 3840×2160（超出的整数倍不列——4K 基线纪律）。
        if w <= 3840 && h <= 2160 {
            out.push((f, w, h));
        }
        f += 1;
    }
    out
}

/// 路径+清晰度合并建议（两判据的合并呈现：路径选型 × 清晰度裁决
/// 一次给全——消费方不拼两次调用）。
pub fn combined_advice(f: PathFacts, src_w: u32, dst_w: u32) -> String {
    let mut s = String::from("建议：路径=");
    match choose_path(f) {
        Some(p) => s.push_str(p.zh()),
        None => {
            s.push_str("1:1（窗口化无缩放）");
            return s;
        }
    }
    s.push_str("；清晰度=");
    match clarity_verdict(src_w, dst_w) {
        ClarityVerdict::IntegerExact { factor } => {
            s.push_str("整数倍×");
            s.push_str(&factor.to_string());
            s.push_str("零模糊");
        }
        ClarityVerdict::NonInteger { suggest_factor } => {
            s.push_str("非整数（建议倍率 ");
            s.push_str(&suggest_factor.to_string());
            s.push_str(" 或开锐化）");
        }
    }
    s
}

// ---------------------------------------------------------------------------
// 八、DPI 变更账（判据四的深化面）
// ---------------------------------------------------------------------------

/// DPI 变更账（revision 单调 + 变更历史封顶滚动——应用对账面）。
#[derive(Clone, Debug, Default)]
pub struct DpiChangeLog {
    events: alloc::vec::Vec<OdomainLink>,
}

impl DpiChangeLog {
    /// 通告入账（revision 必须单调递增——回退显性拒：旧事件回放就是
    /// 这里的反向断言拦的）。
    pub fn push(&mut self, link: OdomainLink) -> Result<(), XcCode> {
        if let Some(last) = self.events.last() {
            if link.revision <= last.revision {
                return Err(XcCode::UnknownDpiStep); // 语义：序违规按表外拒
            }
        }
        self.events.push(link);
        if self.events.len() > 64 {
            self.events.remove(0); // 封顶滚动（对账只看近窗）
        }
        Ok(())
    }

    /// 按版本对账（应用拿自己见过的 revision 来查——比它新的变更数）。
    pub fn pending_since(&self, seen_revision: u64) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                if e.revision > seen_revision {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 最新通告。
    pub fn latest(&self) -> Option<OdomainLink> {
        self.events.last().copied()
    }

    /// 账长。
    pub const fn len(&self) -> usize {
        self.events.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
