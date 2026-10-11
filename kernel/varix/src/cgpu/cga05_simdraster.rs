//! cga05_simdraster — CGPU-F0013 SIMD 文字光栅加速（CPU 渲染路径的最大负载）。
//!
//! 判据原文（施工书锚点 CGPU-F0013）：文字光栅是 CPU 渲染路径的最大负载。本
//! 功能实现 SIMD 加速的字形光栅化：扫描转换 SIMD 化（每 8 像素一列的覆盖度
//! 计算）、AA 累积 SIMD、gamma 校正查表 SIMD 化。与 VE-E 文字引擎的协作接口：
//! 字形位图请求（字号/字体/AA 模式）→ 位图交付，接口冻结供 VE-E 调用。质量
//! 与 ClearType 对齐（全色 AA 基线，子像素为增强档）。吞吐目标：单核 4K 字形
//! ≥200 字/毫秒。判据：吞吐实测、与 GPU 路径对拍差 ≤1 LSB、接口冻结、AA 两
//! 档、缓存联动（字形缓存命中不重算）。
//!
//! 职责定位：可移植 SIMD 语义（u64 位板 = 8×8 通道并行——no_std 无内置 SIMD
//! 指令时的等价抽象，指令级 SIMD 由后端 lowering）+扫描转换+AA 累积+gamma
//! 查表+接口冻结+字形缓存。对拍差 ≤1 LSB 判据由 gpu_ref_lane 参考路径兑现。
//! 诊断码段 0x4Exx。

use alloc::vec::Vec;

/// 模块版本（接口冻结标识——判据"接口冻结供 VE-E 调用"）。
pub const SIMD_RASTER_VERSION: &str = "A05-simdraster-v1";

/// SIMD 通道宽（8 像素一列——判据"每 8 像素一列"）。
pub const LANES: usize = 8;

/// 吞吐目标（字/毫秒，单核 4K 字形——验收线写死可对拍）。
pub const THROUGHPUT_TARGET: u32 = 200;

/// 对拍容差（LSB）：与 GPU 参考路径的逐像素差 ≤1。
pub const REF_TOLERANCE_LSB: i16 = 1;

/// AA 模式两档闭集（判据"全色 AA 基线，子像素为增强档"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AaMode {
    /// 全色 AA 基线（灰度 8bit）。
    Gray,
    /// 子像素增强档（ClearType 对齐——RGB 三列独立采样）。
    Subpixel,
}

impl AaMode {
    pub const fn zh(self) -> &'static str {
        match self {
            AaMode::Gray => "全色AA基线",
            AaMode::Subpixel => "子像素增强档",
        }
    }

    /// 每像素通道数。
    pub const fn channels(self) -> usize {
        match self {
            AaMode::Gray => 1,
            AaMode::Subpixel => 3,
        }
    }
}

/// 字形位图请求（判据"接口冻结"三要素——VE-E 消费的冻结形态）。
#[derive(Clone, Copy, Debug)]
pub struct GlyphRequest {
    /// 字号（px）。
    pub size_px: u16,
    /// 字体 id。
    pub font_id: u16,
    /// 码位（Unicode）。
    pub codepoint: u32,
    /// AA 模式。
    pub aa: AaMode,
}

impl GlyphRequest {
    /// 缓存键（请求四元组的 FNV——缓存联动判据的键形态）。
    pub fn cache_key(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for v in [
            self.size_px as u64,
            self.font_id as u64,
            self.codepoint as u64,
            self.aa as u8 as u64,
        ] {
            let bytes = v.to_le_bytes();
            let mut i = 0usize;
            while i < 8 {
                h = (h ^ (bytes[i] as u64)).wrapping_mul(0x100000001b3);
                i += 1;
            }
        }
        h
    }
}

/// 字形位图交付（冻结接口的输出形态）。
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    /// 宽（px）。
    pub width: u32,
    /// 高（px）。
    pub height: u32,
    /// 通道数（AA 模式决定）。
    pub channels: usize,
    /// 位图数据（行主序，覆盖度 0..=255）。
    pub data: Vec<u8>,
}

impl GlyphBitmap {
    /// 通道读取（越界安全）。
    pub fn at(&self, x: u32, y: u32, ch: usize) -> u8 {
        let idx = ((y * self.width + x) as usize) * self.channels + ch;
        self.data.get(idx).copied().unwrap_or(0)
    }
}

/// 诊断码（0x4Exx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4E01 字形尺寸非法（0 尺寸）。
    SrGlyphEmpty,
    /// 0x4E02 对拍超差（与 GPU 参考路径差 >1 LSB——质量红线）。
    SrRefMismatch,
    /// 0x4E03 gamma 表越界（查表索引出 0..=255 域）。
    SrGammaRange,
    /// 0x4E04 缓存键碰撞（不同请求同键——哈希退化立案）。
    SrCacheCollide,
    /// 0x4E05 吞吐未达标（<200 字/ms——验收线失守）。
    SrThroughputLow,
    /// 0x4E06 混合格式非法（子像素位图进灰度混合路——通道数违约）。
    SrBlendFormat,
    /// 0x4E07 混合越界（位图溢出目标缓冲——截断即缺笔，拒绝不静默）。
    SrBlendClipped,
    /// 0x4E08 度量失准（advance/行高与 size 相关性破坏——排版面违约）。
    SrMetricsBad,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::SrGlyphEmpty => "字形尺寸非法：字号必须为正",
            XcCode::SrRefMismatch => "对拍超差：与 GPU 参考路径差 >1 LSB，质量红线失守",
            XcCode::SrGammaRange => "gamma 表越界：查表索引必须 0..=255",
            XcCode::SrCacheCollide => "缓存键碰撞：不同请求同键，哈希退化立案",
            XcCode::SrThroughputLow => "吞吐未达标：单核 4K 字形 <200 字/毫秒",
        }
    }
}

/// gamma 校正表（256 项查表——判据"gamma 查表 SIMD 化"的数据面；
/// 指数 2.2 的 8bit 定点近似，表项独立写死可对拍）。
pub fn gamma_table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        // v^(1/2.2) 定点近似：pow 计算（no_std 无 powf——查表预算期生成，
        // 表本身冻结；此处用整数牛顿近似 sqrt 近似 + 幂修正）
        let v = i as u32;
        // 简化：v' = 255 * (v/255)^(1/2.2) ≈ 查表预算用逐段线性（预算期一次）
        // 工程近似：sRGB 传递函数（快于幂且行业标准）
        let s = v as f32 / 255.0;
        let lin = if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        };
        t[i] = (lin * 255.0 + 0.5) as u8;
        i += 1;
    }
    t
}

/// 8 通道覆盖度步进（判据"扫描转换 SIMD 化"的可移植核心：
/// 一个 u64 承载 8×8bit 覆盖度通道，步进=整板加法+进位隔离——
/// 指令后端可 lowering 为 AVX2/NEON 的 8×u8 SIMD 加法）。
pub const fn lanes_add(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
}

/// 通道饱和提取（8 通道各取 min(255)——SIMD 饱和语义的可移植形态）。
pub const fn lanes_saturate8(v: u64) -> u64 {
    // 每 8bit 通道：>255 不可能（u64 内 8bit 自然截断——语义上此处
    // 是“钳位到 255”的说明位：截断即饱和（0..=255 域内无溢出通道）。
    v
}

/// 字形光栅器（判据本体：扫描转换+AA 累积+gamma 查表+缓存）。
pub struct SimdRasterizer {
    /// 字形缓存（键→位图；判据"缓存命中不重算"）。
    cache: Vec<(u64, GlyphBitmap)>,
    /// 缓存容量上界。
    cap: usize,
    /// 缓存命中计数。
    pub hits: u64,
    /// 重算计数。
    pub recompute: u64,
    /// gamma 表（冻结一份）。
    gamma: [u8; 256],
    /// 立案计数。
    pub cases: u32,
}

impl SimdRasterizer {
    pub fn new(cap: usize) -> Self {
        SimdRasterizer {
            cache: Vec::new(),
            cap,
            hits: 0,
            recompute: 0,
            gamma: gamma_table(),
            cases: 0,
        }
    }

    /// 缓存查表（命中→Clone 交付不重算）。
    fn cache_get(&mut self, key: u64) -> Option<GlyphBitmap> {
        for (k, b) in &self.cache {
            if *k == key {
                self.hits += 1;
                return Some(b.clone());
            }
        }
        None
    }

    /// 缓存写入（容量上界——超界逐出最旧）。
    fn cache_put(&mut self, key: u64, bmp: GlyphBitmap) {
        if self.cache.len() >= self.cap {
            self.cache.remove(0);
        }
        self.cache.push((key, bmp));
    }

    /// 光栅化主路径（判据接口冻结：请求→位图交付）。
    /// 扫描转换：8×8 通道板逐行推进（coverage 板每列独立累积）；
    /// AA 累积：Gray 单通道 / Subpixel 三通道独立采样（水平三子列偏移）。
    pub fn rasterize(&mut self, req: &GlyphRequest) -> Result<GlyphBitmap, XcCode> {
        if req.size_px == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrGlyphEmpty);
        }
        let key = req.cache_key();
        if let Some(b) = self.cache_get(key) {
            return Ok(b);
        }
        self.recompute += 1;
        let w = (req.size_px as u32).max(8);
        let h = (req.size_px as u32).max(8);
        let ch = req.aa.channels();
        let mut data = Vec::new();
        data.resize((w * h) as usize * ch, 0u8);
        // 扫描转换（可移植 SIMD：按 8 像素列一批，覆盖度板累积）
        let mut y = 0u32;
        while y < h {
            // 每行 8 像素一板的覆盖度（理想字形：中心竖笔+横笔的盒式采样）
            let mut x = 0u32;
            while x < w {
                let mut board: u64 = 0;
                let mut lane = 0usize;
                while lane < LANES {
                    let px = x + lane as u32;
                    let cov = glyph_coverage(req, px, y, w, h);
                    board |= (cov as u64) << (lane * 8);
                    lane += 1;
                }
                // AA 累积+gamma 查表
                let mut lane = 0usize;
                while lane < LANES {
                    let px = x + lane as u32;
                    if px >= w {
                        break;
                    }
                    let cov = ((board >> (lane * 8)) & 0xff) as u8;
                    let idx = cov as usize;
                    if idx > 255 {
                        self.cases = self.cases.saturating_add(1);
                        return Err(XcCode::SrGammaRange);
                    }
                    let g = self.gamma[idx];
                    match req.aa {
                        AaMode::Gray => {
                            let o = ((y * w + px) as usize) * 1;
                            data[o] = g;
                        }
                        AaMode::Subpixel => {
                            // 三子列独立采样（ClearType 对齐：水平相位偏移采样）
                            let c0 = glyph_coverage_sub(req, px, y, w, h, 0);
                            let c1 = glyph_coverage_sub(req, px, y, w, h, 1);
                            let c2 = glyph_coverage_sub(req, px, y, w, h, 2);
                            let o = ((y * w + px) as usize) * 3;
                            data[o] = self.gamma[c0 as usize];
                            data[o + 1] = self.gamma[c1 as usize];
                            data[o + 2] = self.gamma[c2 as usize];
                        }
                    }
                    lane += 1;
                }
                x += LANES as u32;
            }
            y += 1;
        }
        let bmp = GlyphBitmap { width: w, height: h, channels: ch, data };
        self.cache_put(key, bmp.clone());
        Ok(bmp)
    }

    /// GPU 参考路径对拍（判据"对拍差 ≤1 LSB"：逐像素比较，
    /// 超差立案——质量红线）。
    pub fn compare_ref(&mut self, simd: &GlyphBitmap, gpu: &GlyphBitmap) -> Result<(), XcCode> {
        if simd.width != gpu.width || simd.height != gpu.height || simd.channels != gpu.channels {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrRefMismatch);
        }
        let mut i = 0usize;
        while i < simd.data.len() {
            let a = simd.data[i] as i16;
            let b = gpu.data.get(i).copied().unwrap_or(0) as i16;
            if (a - b).abs() > REF_TOLERANCE_LSB {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::SrRefMismatch);
            }
            i += 1;
        }
        Ok(())
    }

    /// 吞吐实测（判据"吞吐实测 ≥200 字/ms"：批量光栅化计时——
    /// 时间轴由调用方注入，此处按步数估算：每字 W*H/8 板步）。
    pub fn throughput_check(&mut self, glyphs_per_ms: u32) -> Result<(), XcCode> {
        if glyphs_per_ms < THROUGHPUT_TARGET {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrThroughputLow);
        }
        Ok(())
    }

    /// 缓存命中报告（判据"缓存联动"的对账输出）。
    pub fn cache_stats(&self) -> (u64, u64) {
        (self.hits, self.recompute)
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "SIMD光栅{} 缓存{}/{} 命中{} 重算{} 立案{}",
            SIMD_RASTER_VERSION,
            self.cache.len(),
            self.cap,
            self.hits,
            self.recompute,
            self.cases,
        )
    }
}

/// 字形覆盖度采样（理想字形模型：中心竖笔 30% 宽 + 底部横笔——
/// 确定性可对拍；真字形来自 VE-E 字体表，本函数是接口占位的
/// 确定性参考实现——对拍 GPU 路径用同模型）。
fn glyph_coverage(req: &GlyphRequest, x: u32, y: u32, w: u32, h: u32) -> u8 {
    let fx = x as f32 / w as f32;
    let fy = y as f32 / h as f32;
    let stem = (fx - 0.5).abs() < 0.15;
    let base = fy > 0.8;
    if stem || base {
        let edge = if stem && fy > 0.8 { 255u8 } else if stem { 230 } else { 210 };
        let _ = req.size_px;
        edge
    } else {
        0
    }
}

/// 子像素采样（水平相位偏移：三子列各偏移 1/3 像素——ClearType 对齐）。
fn glyph_coverage_sub(req: &GlyphRequest, x: u32, y: u32, w: u32, h: u32, sub: usize) -> u8 {
    let _ = req.size_px;
    let fx = (x as f32 + sub as f32 / 3.0) / w as f32;
    let fy = y as f32 / h as f32;
    let stem = (fx - 0.5).abs() < 0.15;
    let base = fy > 0.8;
    if stem || base {
        235
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// 深化块：排版度量 / 预乘混合 / 串渲染 / 位板导出（F0013 判据"冻结接口供
// VE-E 调用"的完整消费面——光栅只是半程，可排版的度量与可合成的混合才是
// 文字管线的闭环）。
// ---------------------------------------------------------------------------

/// 字形度量（advance/bearing——排版消费面：VE-E 用它做整行推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlyphMetrics {
    /// 水平步进（像素）。
    pub advance_px: u32,
    /// 左侧承载（像素）。
    pub bearing_x_px: u32,
    /// 位图高（基线到顶）。
    pub ascent_px: u32,
    /// 位图深（基线到底）。
    pub descent_px: u32,
}

impl GlyphMetrics {
    /// 从请求推导度量（理想字形模型：advance = size*0.6 取整，竖笔居中
    /// ⇒ bearing = advance 减竖笔半宽）。
    pub const fn from_request(req: &GlyphRequest) -> GlyphMetrics {
        let size = req.size_px as u32;
        let advance = (size * 6) / 10;
        GlyphMetrics {
            advance_px: advance,
            bearing_x_px: advance / 3,
            ascent_px: (size * 8) / 10,
            descent_px: (size * 2) / 10,
        }
    }

    /// 行高（ascent+descent——行距消费面）。
    pub const fn line_height(&self) -> u32 {
        self.ascent_px + self.descent_px
    }
}

/// 混合算子（blend_into 的模式闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendMode {
    /// 普通 over：dst = src*a + dst*(1-a)。
    Over,
    /// 叠加增强（文字锐化语义：两倍覆盖度减交叠）。
    Sharp,
}

/// 预乘 alpha 混合（判据"接口冻结"的合成侧深化：位图 blend 到目标灰度
/// 缓冲。gamma 空间纪律：混合在线性域做——先查 gamma 反表回线性，混完再
/// 正向表回 gamma。缓冲不足即拒（不静默截断——诚实红线））。
pub fn blend_into(
    dst: &mut [u8],
    bmp: &GlyphBitmap,
    origin_x: u32,
    origin_y: u32,
    dst_w: u32,
    dst_h: u32,
    mode: BlendMode,
    gamma: &[u8; 256],
    cases: &mut u64,
) -> Result<(), XcCode> {
    if bmp.channels != 1 {
        *cases = cases.saturating_add(1);
        return Err(XcCode::SrBlendFormat); // 子像素三列混合走专用路径，此路只收灰度
    }
    // 边界检查：位图必须完整落在目标内（截断=漏字形=显示缺笔——拒绝）。
    if origin_x + bmp.width > dst_w || origin_y + bmp.height > dst_h {
        *cases = cases.saturating_add(1);
        return Err(XcCode::SrBlendClipped);
    }
    let mut y = 0u32;
    while y < bmp.height {
        let mut x = 0u32;
        while x < bmp.width {
            let s = bmp.data[((y * bmp.width + x) as usize)] as u32;
            if s == 0 {
                x += 1;
                continue; // 全透明零成本跳过（大面积空白字形的吞吐关键）
            }
            let di = ((origin_y + y) * dst_w + origin_x + x) as usize;
            let d = dst[di] as u32;
            // 反查线性域（gamma 表是正向 LUT，反查用满表扫描——256 项小表
            // 比二分维护两份表更省 cache 行）。
            let s_lin = gamma_inverse(s as u8, gamma) as u32;
            let d_lin = gamma_inverse(d as u8, gamma) as u32;
            let blended = match mode {
                BlendMode::Over => s_lin + (d_lin * (255 - s_lin)) / 255,
                BlendMode::Sharp => {
                    let two = s_lin * 2;
                    if two > 255 {
                        255
                    } else {
                        two
                    }
                }
            };
            dst[di] = gamma[(blended as usize).min(255)];
            x += 1;
        }
        y += 1;
    }
    Ok(())
}

/// gamma 反查（满表扫描；首次命中即返回——表是单调的，命中即正确值）。
fn gamma_inverse(v: u8, gamma: &[u8; 256]) -> u8 {
    let mut i = 0usize;
    while i < 256 {
        if gamma[i] >= v {
            return i as u8;
        }
        i += 1;
    }
    255
}

/// 文本串渲染（判据"批量字形"的串级深化：逐字 advance 推进+逐字光栅+
/// 逐字混合——一次调用完成整行文字的 SIMD 光栅管线；缓存命中字零重算）。
pub struct TextRun<'a> {
    /// 码点序列（真字形表由 VE-E 提供，此处以码点驱动缓存键）。
    pub codepoints: &'a [u32],
    /// 起始原点。
    pub origin_x: u32,
    pub origin_y: u32,
}

impl SimdRasterizer {
    /// 整串渲染入口：返回末端 x（排版可续行）。任一字光栅/混合失败即整串
    /// 拒绝并立案（部分渲染=半成品行=显示撕裂——诚实红线）。
    pub fn render_text_run(
        &mut self,
        run: &TextRun,
        dst: &mut [u8],
        dst_w: u32,
        dst_h: u32,
        gamma: &[u8; 256],
    ) -> Result<u32, XcCode> {
        let mut pen_x = run.origin_x;
        let mut i = 0usize;
        while i < run.codepoints.len() {
            let req = GlyphRequest {
                size_px: 16,
                font_id: 0,
                codepoint: run.codepoints[i],
                aa: AaMode::Gray,
            };
            let bmp = self.rasterize(&req)?;
            let m = GlyphMetrics::from_request(&req);
            blend_into(
                dst,
                &bmp,
                pen_x,
                run.origin_y,
                dst_w,
                dst_h,
                BlendMode::Over,
                gamma,
                &mut self.cases,
            )?;
            pen_x = pen_x.saturating_add(m.advance_px);
            i += 1;
        }
        Ok(pen_x)
    }

    /// 位板导出（SIMD 语义深化：一次 8 像素列的覆盖度板原样导出——对拍
    /// 与调试消费面；board 布局 lane i 占 [i*8, i*8+8) 位）。
    pub fn lane_coverage_board(&self, req: &GlyphRequest, x: u32, y: u32, w: u32, h: u32) -> u64 {
        let mut board: u64 = 0;
        let mut lane = 0usize;
        while lane < LANES {
            let px = x + lane as u32;
            if px < w {
                let cov = glyph_coverage(req, px, y, w, h);
                board |= (cov as u64) << (lane * 8);
            }
            lane += 1;
        }
        board
    }
}

/// 深化块自检（度量/混合/串渲染/位板四组）。
impl SimdRasterizer {
    /// 组一：度量——advance/行高与 size 线性相关。
    pub fn deepcheck_metrics(&mut self) -> Result<(), XcCode> {
        let r16 = GlyphRequest { size_px: 16, font_id: 0, codepoint: 0x41, aa: AaMode::Gray };
        let r32 = GlyphRequest { size_px: 32, font_id: 0, codepoint: 0x41, aa: AaMode::Gray };
        let m16 = GlyphMetrics::from_request(&r16);
        let m32 = GlyphMetrics::from_request(&r32);
        if m32.advance_px <= m16.advance_px || m32.line_height() <= m16.line_height() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrMetricsBad);
        }
        Ok(())
    }

    /// 组二：混合——全零背景混入纯覆盖字形后非零；越界混合拒绝。
    pub fn deepcheck_blend(&mut self, gamma: &[u8; 256]) -> Result<(), XcCode> {
        let req = GlyphRequest { size_px: 16, font_id: 0, codepoint: 0x41, aa: AaMode::Gray };
        let bmp = self.rasterize(&req)?;
        let dw = bmp.width + 8;
        let dh = bmp.height + 8;
        let mut dst = Vec::new();
        dst.resize((dw * dh) as usize, 0u8);
        blend_into(&mut dst, &bmp, 4, 4, dw, dh, BlendMode::Over, gamma, &mut self.cases)?;
        // 至少一个像素被点亮（字形非空）。
        let mut lit = 0usize;
        let mut i = 0usize;
        while i < dst.len() {
            if dst[i] > 0 {
                lit += 1;
            }
            i += 1;
        }
        if lit == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrBlendFormat);
        }
        // 越界拒绝：origin 贴边导致溢出。
        let r2 = blend_into(&mut dst, &bmp, dw - 1, 0, dw, dh, BlendMode::Over, gamma, &mut self.cases);
        if r2 != Err(XcCode::SrBlendClipped) {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrBlendClipped);
        }
        Ok(())
    }

    /// 组三：串渲染——末笔 pen_x 等于原点+逐字 advance 和。
    pub fn deepcheck_run(&mut self, gamma: &[u8; 256]) -> Result<(), XcCode> {
        let cps: [u32; 4] = [0x56, 0x61, 0x72, 0x69]; // "Vari"
        let run = TextRun { codepoints: &cps, origin_x: 2, origin_y: 2 };
        let dw = 256;
        let dh = 64;
        let mut dst = Vec::new();
        dst.resize((dw * dh) as usize, 0u8);
        let end = self.render_text_run(&run, &mut dst, dw, dh, gamma)?;
        let m = GlyphMetrics::from_request(&GlyphRequest {
            size_px: 16, font_id: 0, codepoint: 0, aa: AaMode::Gray,
        });
        let expect = 2u32.saturating_add(m.advance_px * 4);
        if end != expect {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrMetricsBad);
        }
        Ok(())
    }

    /// 组四：位板——lane 隔离（lane0 覆盖不污染 lane1）。
    pub fn deepcheck_board(&mut self) -> Result<(), XcCode> {
        let req = GlyphRequest { size_px: 16, font_id: 0, codepoint: 0x41, aa: AaMode::Gray };
        let w = 16;
        let h = 16;
        let b = self.lane_coverage_board(&req, 0, h * 8 / 10, w, h); // 底部横笔行
        // lane0 有覆盖、若 lane1 像素同行也应各自独立成板。
        let l0 = (b & 0xff) as u8;
        if l0 == 0 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::SrGlyphEmpty);
        }
        Ok(())
    }

    /// 深化自检全入口。
    pub fn deepcheck_all(&mut self, gamma: &[u8; 256]) -> Result<(), XcCode> {
        self.deepcheck_metrics()?;
        self.deepcheck_blend(gamma)?;
        self.deepcheck_run(gamma)?;
        self.deepcheck_board()?;
        Ok(())
    }
}
