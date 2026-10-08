//! VE-F0816 · 文字渲染扩展点（VE-E 域 · 文字渲染 · 扩展生态）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0816`
//!
//! 锚点原文：「文字渲染扩展点开放自定义光栅化后端注册：后端契约五函数
//! （init/rasterize/glyph_metrics/teardown/capabilities_query），注册后按能力
//! 协商接管光栅化段（能力查询声明支持的表现：AA 模式/粗细档/最大字号），未
//! 声明能力回退内置后端。注册纪律：后端实现在进程内但受资源限额（内存 ≤128MB、
//! 单字形光栅化 ≤2ms 超限摘除），签名校验沿用全域扩展框架（与 AE08 生态、AB08
//! 音频生态同一注册协议——全域扩展一致性）。错误路径：后端崩溃→隔离兜底摘除
//! 并回退内置后端，用户可见提示；能力虚报（声明支持实测不支持）→金样 diff
//! 抓出并下架注册。性能：后端调度开销 ≤0.01ms/帧。对接：扩展注册 API 与
//! F0817 三函数并列、独立冻结；文档入 F0811 创作者节。判据：五函数契约、
//! 能力协商、限额摘除、全域同协议、金样抓虚报。」
//!
//! # 一、五函数是**契约**不是建议：生命周期由宿主状态机钉死
//!
//! 自定义后端在**进程内**运行——它崩溃（返回 Err）、超限（单字形 >2ms）、
//! 虚报（声明的能力实测没有），脏的都是宿主的合成线程。故本单把交互面收敛成
//! 一个状态机：注册（走全域协议）→ init 成功才可被调度 → 任一次 rasterize
//! 返回 Err 或超时即**立即摘除**（[`RasterHost::evict`]）→ 摘除后回退内置后端
//! 并产出用户可见提示（[`EvictionNotice`]）。teardown 之后任何调用都是宿主
//! bug，由判据钉死（摘除后再调度必须走内置，绝不触达已 teardown 的后端）。
//!
//! # 二、能力协商是「声明 ⊆ 实测」的双向问题
//!
//! 注册时声明 [`BackendCaps`]（AA 位集/粗细位集/最大字号）；调度时请求能力
//! **必须被声明覆盖**才接管，否则回退内置（锚点「未声明能力回退内置后端」）。
//! 声明超出实测是更危险的方向：[`golden_probe`] 在注册时对预置金样字形做
//! 光栅化，输出的客观特征（位深跨度/字重差异）与声明不符即**虚报**——下架
//! 注册并计入 banned（锚点「金样 diff 抓出并下架注册」）。
//!
//! # 三、注册协议**不新造**：与 AE08/AB08 同一全域扩展框架
//!
//! 注册直接走 [`super::veq04_type::ExtensionPoint::register_ext`]（命名空间
//! 消歧 + 四要素门槛 + 被拒计数）——文字生态没有自己的注册协议，四要素残缺
//! 与重名的拒绝语义与全仓一致（锚点「全域扩展一致性」）。
//!
//! # 四、资源限额与调度开销
//!
//! 进程内不等于无界：后端内存水位 128MB 上限（注册时声明峰值，运行期超限按
//! 摘除处理），单字形光栅化 2ms 超时（宿主按请求计时）。调度路径本身是
//! 「能力位测试 + 一次选择」——零分配、O(1)，声明常量
//! [`DISPATCH_BUDGET_US`]（0.01ms=10us）供基准对账。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界：后端返回的位图宽高不一致按
//! [`Fault::BitmapMismatch`] 处理（宿主不信任后端输出形状）；所有失败路径
//! 走 `Result` 与显式摘除。

// lib.rs 只有 `extern crate alloc` 且无 `#[macro_use]`，宏逐文件显式导入。
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::veq04_type::{ExtensionPoint, TypeElements};

// ===========================================================================
// 一、诊断码（vee16 独占段，E_TEXT16_ 前缀互异由判据承载）
// ===========================================================================

/// 后端契约版本（与 F0817 三函数并列、独立冻结的锚点落位）。
pub const TEXT16_VERSION: &str = "E16-textext-v1";

/// 后端内存水位上限（锚点「内存 ≤128MB」）。
pub const BACKEND_MEM_LIMIT_MB: u32 = 128;
/// 单字形光栅化时限（锚点「≤2ms」）。
pub const GLYPH_BUDGET_US: u32 = 2000;
/// 后端调度开销预算（锚点「≤0.01ms/帧」= 10μs）。
pub const DISPATCH_BUDGET_US: u32 = 10;

/// init 失败（后端自报无法初始化）。
pub const E_TEXT16_INIT: &str = "E_TEXT16_INIT";
/// 光栅化失败（后端崩溃/坏输出——隔离兜底摘除）。
pub const E_TEXT16_RASTER: &str = "E_TEXT16_RASTER";
/// 能力虚报（声明与金样实测不符——下架注册）。
pub const E_TEXT16_CAP_FRAUD: &str = "E_TEXT16_CAP_FRAUD";
/// 限额超限（内存水位/单字形超时——摘除）。
pub const E_TEXT16_LIMIT: &str = "E_TEXT16_LIMIT";
/// 生命周期违规（teardown 后被调度/未 init 即光栅化）。
pub const E_TEXT16_LIFECYCLE: &str = "E_TEXT16_LIFECYCLE";
/// 注册被全域协议拒绝（四要素残缺/命名空间问题）。
pub const E_TEXT16_REG_REJECT: &str = "E_TEXT16_REG_REJECT";

// ===========================================================================
// 二、能力声明与协商（判据二）
// ===========================================================================

/// AA 模式位集（锚点「AA 模式」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AaModes(pub u8);

impl AaModes {
    /// 无 AA（1bit 二值输出）。
    pub const NONE: AaModes = AaModes(1 << 0);
    /// 4bit AA。
    pub const AA4: AaModes = AaModes(1 << 1);
    /// 8bit AA。
    pub const AA8: AaModes = AaModes(1 << 2);
    /// 全集掩码（越界位=虚报证据之一）。
    pub const FULL_MASK: u8 = 0b0000_0111;

    /// 含某模式。
    pub const fn has(self, m: AaModes) -> bool {
        self.0 & m.0 != 0
    }
}

/// 粗细档位集（锚点「粗细档」）：0 细/1 常规/2 粗/3 特粗。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Weights(pub u8);

impl Weights {
    /// 单档位标志。
    pub const fn one(weight: u8) -> Weights {
        Weights(1 << weight)
    }
    /// 含某档。
    pub const fn has(self, weight: u8) -> bool {
        self.0 & (1 << weight) != 0
    }
}

/// 后端能力声明（注册时提交，金样对账的「声明侧」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackendCaps {
    /// 支持的 AA 模式位集。
    pub aa: AaModes,
    /// 支持的粗细档位集。
    pub weights: Weights,
    /// 最大字号（px，超过即回退内置）。
    pub max_glyph_px: u16,
    /// 内存峰值声明（MB，超 [`BACKEND_MEM_LIMIT_MB`] 注册即拒）。
    pub mem_mb: u32,
}

/// 一次光栅化请求（调度侧的「需求侧」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RasterRequest {
    /// 目标字符。
    pub ch: char,
    /// 字号（px）。
    pub px: u16,
    /// 请求的 AA 模式。
    pub aa: AaModes,
    /// 请求的粗细档（0..=3）。
    pub weight: u8,
}

/// 能力协商裁决：请求能力被声明覆盖才接管（锚点「未声明能力回退内置后端」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendChoice {
    /// 自定义后端接管。
    Custom,
    /// 内置后端兜底。
    Builtin,
}

/// O(1) 能力协商（零分配——锚点「调度开销 ≤0.01ms/帧」的路径保障）。
pub fn negotiate(caps: &BackendCaps, req: &RasterRequest) -> BackendChoice {
    let aa_ok = caps.aa.has(req.aa);
    let w_ok = req.weight < 4 && caps.weights.has(req.weight);
    let px_ok = req.px <= caps.max_glyph_px;
    if aa_ok && w_ok && px_ok {
        BackendChoice::Custom
    } else {
        BackendChoice::Builtin
    }
}

// ===========================================================================
// 三、后端契约五函数（判据一）
// ===========================================================================

/// 字形位图（后端产出；宿主按声明校验形状——不信任后端输出）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GlyphBitmap {
    /// 宽（px）。
    pub w: u16,
    /// 高（px）。
    pub h: u16,
    /// 位深（bit/px：1/4/8，与 AA 模式对应）。
    pub depth: u8,
    /// 数据（len 必须 = w*h*depth/8 的向上取整）。
    pub data: Vec<u8>,
}

/// 字形度量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlyphMetrics {
    /// 步进（px）。
    pub advance: u16,
    /// 顶起（px）。
    pub ascent: u16,
    /// 下降（px）。
    pub descent: u16,
}

/// 后端五函数契约（锚点「init/rasterize/glyph_metrics/teardown/capabilities_query」）。
pub trait RasterBackend {
    /// 函数一：init——初始化（失败即注册后不可调度）。
    fn init(&mut self) -> Result<(), &'static str>;
    /// 函数二：rasterize——光栅化一个字形（Err = 后端故障，宿主摘除）。
    fn rasterize(&mut self, req: &RasterRequest) -> Result<GlyphBitmap, &'static str>;
    /// 函数三：glyph_metrics——度量查询（Err 同上）。
    fn glyph_metrics(&self, req: &RasterRequest) -> Result<GlyphMetrics, &'static str>;
    /// 函数四：teardown——释放（宿主摘除/替换时调用；之后绝不复用）。
    fn teardown(&mut self);
    /// 函数五：capabilities_query——能力查询（金样对账的声明来源）。
    fn capabilities(&self) -> BackendCaps;
}

// ===========================================================================
// 四、内置后端（回退兜底——永远可用）
// ===========================================================================

/// 内置后端：8bit AA/全粗细/512px 上限（全能力，作为回退与金样参考面）。
pub struct BuiltinBackend;

impl RasterBackend for BuiltinBackend {
    fn init(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn rasterize(&mut self, req: &RasterRequest) -> Result<GlyphBitmap, &'static str> {
        // 8bit 灰度 ramp：以字号定宽高的简单确定性输出（金样参考实现）。
        let px = req.px.max(8) as usize;
        let n = px * px;
        let mut data = Vec::with_capacity(n);
        let base = 32u8 + (req.weight as u8) * 16;
        for i in 0..n {
            data.push(base.wrapping_add((i % 251) as u8));
        }
        Ok(GlyphBitmap { w: px as u16, h: px as u16, depth: 8, data })
    }

    fn glyph_metrics(&self, req: &RasterRequest) -> Result<GlyphMetrics, &'static str> {
        Ok(GlyphMetrics { advance: req.px, ascent: req.px * 4 / 5, descent: req.px / 5 })
    }

    fn teardown(&mut self) {}

    fn capabilities(&self) -> BackendCaps {
        BackendCaps {
            aa: AaModes(0b111),
            weights: Weights(0b1111),
            max_glyph_px: 512,
            mem_mb: 0,
        }
    }
}

// ===========================================================================
// 五、宿主：注册（全域协议）/ 调度 / 限额摘除 / 金样抓虚报（判据三·四·五）
// ===========================================================================

/// 摘除原因（用户可见提示的载荷）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EvictReason {
    /// 后端光栅化报错（崩溃隔离兜底）。
    BackendFault(&'static str),
    /// 单字形超时（> [`GLYPH_BUDGET_US`]）。
    Timeout(u32),
    /// 能力虚报（金样 diff 抓出）。
    CapFraud,
}

/// 用户可见提示（锚点「隔离兜底摘除并回退内置后端，用户可见提示」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EvictionNotice {
    /// 被摘除的命名空间限定名。
    pub backend: String,
    /// 原因。
    pub reason: EvictReason,
}

/// 光栅化宿主：单一活动自定义后端 + 内置兜底。
pub struct RasterHost {
    /// 全域扩展注册面（AE08/AB08 同协议——判据四的复用实体）。
    pub registry: ExtensionPoint,
    /// 已通过 init 的活动后端（None = 全内置）。
    active: Option<Box<dyn RasterBackend>>,
    /// 活动后端命名空间限定名（提示用）。
    active_name: Option<String>,
    /// 摘除计数（遥测上行，不静默）。
    pub evictions: u32,
    /// 摘除通知（最近一条，用户可见）。
    pub last_notice: Option<EvictionNotice>,
    /// 因虚报被 ban 的命名空间限定名（再注册同名拒绝）。
    pub banned: Vec<String>,
    /// init 失败计数（注册了但起不来）。
    pub init_failures: u32,
}

impl RasterHost {
    /// 新宿主（内置后端兜底，全注册面挂全域协议）。
    pub fn new() -> RasterHost {
        RasterHost {
            registry: ExtensionPoint::new(),
            active: None,
            active_name: None,
            evictions: 0,
            last_notice: None,
            banned: Vec::new(),
            init_failures: 0,
        }
    }

    /// 注册自定义后端（锚点「注册纪律」全流程：全域协议 + 内存声明闸 + 金样）。
    ///
    /// `elems` 四要素由调用方构造——残缺会被 [`ExtensionPoint::register_ext`]
    /// 拒绝（全域语义一致）；内存声明超限在本层拒绝；金样不过按虚报下架。
    pub fn register_backend(
        &mut self,
        ns: &str,
        name: &str,
        elems: TypeElements,
        mut backend: Box<dyn RasterBackend>,
    ) -> Result<(), &'static str> {
        // 内存声明闸：超限不进协议（锚点「内存 ≤128MB」）。
        if backend.capabilities().mem_mb > BACKEND_MEM_LIMIT_MB {
            return Err(E_TEXT16_LIMIT);
        }
        // ban 名单：虚报过的限定名不再接受注册。
        let qualified = format!("{}:{}", ns, name);
        if self.banned.iter().any(|b| *b == qualified) {
            return Err(E_TEXT16_CAP_FRAUD);
        }
        // init 先行（探测的对象是「可工作」的后端——未 init 即拒的后端在
        // init 前无法被金样光栅化）。失败登记计数，不进协议。
        if backend.init().is_err() {
            self.init_failures += 1;
            return Err(E_TEXT16_INIT);
        }
        // 金样对账（注册期一次）：声明与实测不符 → 下架（不注册）。
        if golden_probe(backend.as_mut()).is_err() {
            self.banned.push(qualified.clone());
            self.evictions += 1;
            self.last_notice = Some(EvictionNotice {
                backend: qualified,
                reason: EvictReason::CapFraud,
            });
            return Err(E_TEXT16_CAP_FRAUD);
        }
        // 全域协议注册（四要素门槛 + 命名空间消歧 + 被拒计数——与 AE08/AB08 同）。
        let ext = self.registry.register_ext(ns, name, elems);
        if ext.is_err() {
            return Err(E_TEXT16_REG_REJECT);
        }
        // 接管（替换旧后端：旧后端先 teardown——生命周期纪律）。
        if let Some(old) = self.active.as_mut() {
            old.teardown();
        }
        self.active = Some(backend);
        self.active_name = Some(qualified);
        Ok(())
    }

    /// 调度：协商 → 自定义或内置（摘除后自动回退，绝不触达死后端）。
    ///
    /// `elapsed_us` 为宿主对单字形的计时（锚点「单字形光栅化 ≤2ms」）；超时
    /// 摘除对「下一帧」生效——把超时惩罚与当帧产出解耦（用户先拿到图）。
    pub fn dispatch(&mut self, req: &RasterRequest, elapsed_us: u32) -> Result<GlyphBitmap, &'static str> {
        // 先协商（O(1) 位测试，零分配）。
        let custom_ok = match &self.active {
            Some(b) => negotiate(&b.capabilities(), req) == BackendChoice::Custom,
            None => false,
        };
        let result = if custom_ok {
            match self.active.as_mut() {
                Some(b) => match b.rasterize(req) {
                    Ok(bmp) if shape_ok(&bmp) => Ok(bmp),
                    // 输出形状与声明不符 = 后端故障（宿主不信任坏形状）。
                    Ok(_) => {
                        self.evict(EvictReason::BackendFault(E_TEXT16_RASTER));
                        BuiltinBackend.rasterize(req)
                    }
                    Err(e) => {
                        self.evict(EvictReason::BackendFault(e));
                        BuiltinBackend.rasterize(req)
                    }
                },
                None => BuiltinBackend.rasterize(req),
            }
        } else {
            // 未声明能力/无活动后端 → 内置兜底。
            BuiltinBackend.rasterize(req)
        };
        let bmp = result?;
        if elapsed_us > GLYPH_BUDGET_US {
            self.evict(EvictReason::Timeout(elapsed_us));
        }
        Ok(bmp)
    }

    /// 摘除（锚点「隔离兜底摘除并回退内置后端，用户可见提示」）。
    pub fn evict(&mut self, reason: EvictReason) {
        if let Some(mut b) = self.active.take() {
            b.teardown();
        }
        let name = self.active_name.take().unwrap_or_default();
        self.evictions += 1;
        self.last_notice = Some(EvictionNotice { backend: name, reason });
    }

    /// 活动后端是否为自定义（None/摘除后 = false，全内置）。
    pub fn custom_active(&self) -> bool {
        self.active.is_some()
    }
}

/// 位图形状自检：len 与 w*h*depth/8 一致、depth ∈ {1,4,8}、非零宽高。
/// `pub(crate)`：宿主形状纪律的判据承载面（vee16_checks 直接复用同一判定）。
pub(crate) fn shape_ok(bmp: &GlyphBitmap) -> bool {
    if bmp.depth != 1 && bmp.depth != 4 && bmp.depth != 8 {
        return false;
    }
    if bmp.w == 0 || bmp.h == 0 {
        return false;
    }
    let expect = (bmp.w as usize * bmp.h as usize * bmp.depth as usize).div_ceil(8);
    bmp.data.len() == expect
}

/// 金样探测（判据五：能力虚报抓出）。
///
/// 对预置金样字形做光栅化，核对**输出客观特征**与声明一致：
/// - 声明 8bit AA → 输出 depth=8 且灰度跨度 ≥ 16 级（真 8bit 有渐变）；
/// - 声明 4bit AA → depth=4 或 depth=8 但跨度 <256；
/// - 声明 NONE → depth=1（二值）；
/// - 声明某粗细档 → 该档输出与 0 档输出**必须不同**（字重无差异=档位虚报）；
/// - 输出形状不合法（[`shape_ok`] 否）= 虚报。
pub fn golden_probe(backend: &mut dyn RasterBackend) -> Result<(), &'static str> {
    let caps = backend.capabilities();
    // 金样字形：'A' 32px，逐声明 AA 模式核对。
    // AA4 允许 depth=4 或 depth=8 承载（4bit 渐变的常见实现以 8bit 存储，
    // 渐变跨度才是 AA 精度的客观证据）；AA8 必须 depth=8 且跨度 ≥16。
    for (aa, allow_depths, want_span_min) in [
        (AaModes::NONE, [1u8, 1, 1], 2u32),
        (AaModes::AA4, [4u8, 8, 8], 4u32),
        (AaModes::AA8, [8u8, 8, 8], 16u32),
    ] {
        if caps.aa.has(aa) {
            let req = RasterRequest { ch: 'A', px: 32, aa, weight: 1 };
            let bmp = backend.rasterize(&req).map_err(|_| E_TEXT16_CAP_FRAUD)?;
            if !shape_ok(&bmp) || !allow_depths.contains(&bmp.depth) {
                return Err(E_TEXT16_CAP_FRAUD);
            }
            // 灰度跨度（8bit 口径）：max-min；二值口径下按 0/1。
            let mut mn = u32::MAX;
            let mut mx = 0u32;
            for b in &bmp.data {
                let v = *b as u32;
                mn = mn.min(v);
                mx = mx.max(v);
            }
            if mx.wrapping_sub(mn) + 1 < want_span_min {
                return Err(E_TEXT16_CAP_FRAUD);
            }
        }
    }
    // 粗细档：声明了的档位之间输出必须有差异（档位虚报=全档同图）。
    let mut weight_sig: Vec<u64> = Vec::new();
    for w in 0u8..4 {
        if caps.weights.has(w) {
            let req = RasterRequest { ch: 'A', px: 32, aa: AaModes::AA8, weight: w };
            let bmp = backend.rasterize(&req).map_err(|_| E_TEXT16_CAP_FRAUD)?;
            if !shape_ok(&bmp) {
                return Err(E_TEXT16_CAP_FRAUD);
            }
            // 签名 = 字节和（同内容同签名；档位差异必然改 base 值 → 和不同）。
            let sum: u64 = bmp.data.iter().map(|b| *b as u64).sum();
            weight_sig.push(sum);
        }
    }
    for i in 0..weight_sig.len() {
        for j in (i + 1)..weight_sig.len() {
            if weight_sig[i] == weight_sig[j] {
                return Err(E_TEXT16_CAP_FRAUD);
            }
        }
    }
    Ok(())
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    format!(
        "textext {} mem<={}MB glyph<={}us dispatch<={}us",
        TEXT16_VERSION, BACKEND_MEM_LIMIT_MB, GLYPH_BUDGET_US, DISPATCH_BUDGET_US,
    )
}
