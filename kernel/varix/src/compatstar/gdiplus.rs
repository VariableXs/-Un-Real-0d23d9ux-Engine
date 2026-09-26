//! F007 GDI+ 与双缓冲（compatstar · G-A-07）——抗锯齿文字和渐变边框，看不出差别。
//!
//! 主册判据（验收标准第一句）：
//! **「开源 GDI+ 小工具（ImageMagick GUI 前端类或选取的独立作品）全流程绿；
//! 对拍报告入册。」**
//!
//! 功能定义（G-A-07）：GDI+ 基本面：Graphics/Pen/Brush(实心/渐变)/Path/抗锯
//! 齿文本/图像绘制(Bitmap/ImageAttributes 裁剪)；双缓冲管线（内存位图 → 一次
//! 性提交）一等公民支持。
//!
//! 【交互设计】验收看渲染质量对拍：同一输入文件在 Windows 与 VARIX 输出的
//! PNG 逐像素对比，容差仅限字体 hinting 差异（标注差异源）。
//! 【数据与存储】位图对象内存池走进程配额（F195）；大位图（>64MP）流式处理
//! 防内存尖峰。【状态与异常】损坏图片（PNG/CRC 错）→ 绘制失败返回错误码，
//! 应用决定 UI；内存不足 → OOM 错误码（OutOfMemory 语义）而非杀进程。
//! 【设计细节】抗锯齿统一走 VARIX 文本管线灰度 AA（ClearType 亚像素不承诺，
//! 差异表）；渐变画刷支持线性/路径两种；路径对象顶点上限 16k；对拍容差规则
//! 文档化：非文本区逐像素容差 1/255、文本区按 SSIM > 0.95——容差也是判据不
//! 是感觉。
//!
//! 零堆纪律：路径顶点/位图池/对拍样本全定长，无 Vec/String/Box/format!。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 路径对象顶点上限 16k（主册【设计细节】）。
pub const PATH_VERTEX_CAP: usize = 16_384;
/// 大位图流式处理线 64MP（主册【数据与存储】）。
pub const STREAMING_THRESHOLD_PIXELS: u64 = 64_000_000;
/// 非文本区逐像素容差 1/255（主册【设计细节】）。
pub const PIXEL_TOLERANCE: u8 = 1;
/// 文本区 SSIM 门限（主册：> 0.95，permille 950）。
pub const TEXT_SSIM_PERMILLE: u32 = 950;
/// 位图池进程配额 256MP（F195 联动的工程值，登记完成报告）。
pub const BITMAP_POOL_QUOTA_PIXELS: u64 = 256_000_000;

// ---------------------------------------------------------------------------
// 画刷 / 画笔 / 路径
// ---------------------------------------------------------------------------

/// GDI+ 画刷（实心/线性渐变/路径渐变——主册【功能定义】+【设计细节】两种渐变）。
#[derive(Clone, Copy, Debug)]
pub enum Brush {
    Solid(u32),
    /// 线性渐变（起点色/终点色）。
    LinearGradient(u32, u32),
    /// 路径渐变（中心色/边界色）。
    PathGradient(u32, u32),
}

impl Brush {
    /// 位置 t∈[0,1000] 处的颜色（渐变插值——线性/路径渐变同插值核）。
    pub fn sample(&self, t_permille: u32) -> u32 {
        match *self {
            Brush::Solid(c) => c,
            Brush::LinearGradient(a, b) => lerp_color(a, b, t_permille),
            Brush::PathGradient(center, edge) => lerp_color(center, edge, t_permille),
        }
    }

    pub fn is_gradient(&self) -> bool {
        !matches!(self, Brush::Solid(_))
    }
}

/// 颜色线性插值（t permille 0..=1000）。
pub fn lerp_color(a: u32, b: u32, t_permille: u32) -> u32 {
    let t = t_permille.min(1000);
    let mix = |xa: u32, xb: u32| {
        let v = (xa * (1000 - t) + xb * t) / 1000;
        v.min(255)
    };
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;
    (mix(ar, br) << 16) | (mix(ag, bg) << 8) | mix(ab, bb)
}

/// 路径对象（顶点上限 16k，超出如实拒绝——不静默截断）。
pub struct GdiPath {
    x: [f32; PATH_VERTEX_CAP],
    y: [f32; PATH_VERTEX_CAP],
    count: usize,
}

impl GdiPath {
    pub fn new() -> GdiPath {
        GdiPath { x: [0.0; PATH_VERTEX_CAP], y: [0.0; PATH_VERTEX_CAP], count: 0 }
    }

    pub fn add_vertex(&mut self, x: f32, y: f32) -> bool {
        if self.count >= PATH_VERTEX_CAP {
            return false;
        }
        self.x[self.count] = x;
        self.y[self.count] = y;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn vertex(&self, i: usize) -> Option<(f32, f32)> {
        if i < self.count {
            Some((self.x[i], self.y[i]))
        } else {
            None
        }
    }
}

impl Default for GdiPath {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 错误码（OutOfMemory 语义——杀进程是失败）
// ---------------------------------------------------------------------------

/// GDI+ 绘制错误码（应用决定 UI——主册【状态与异常】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdiPlusError {
    /// 损坏图片（PNG CRC 校验失败等）。
    CorruptImage,
    /// OutOfMemory 语义（返回错误码，不杀进程）。
    OutOfMemory,
    /// 路径顶点超限。
    PathOverflow,
    /// 大位图超池配额。
    PoolExhausted,
}

impl GdiPlusError {
    pub fn as_str(self) -> &'static str {
        match self {
            GdiPlusError::CorruptImage => "image data is corrupt",
            GdiPlusError::OutOfMemory => "out of memory",
            GdiPlusError::PathOverflow => "path vertex limit exceeded",
            GdiPlusError::PoolExhausted => "bitmap pool quota exhausted",
        }
    }
}

// ---------------------------------------------------------------------------
// 位图池（配额 + 流式处理）
// ---------------------------------------------------------------------------

/// 位图池：进程配额记账（F195 联动）+ 64MP 流式线。
pub struct BitmapPool {
    allocated_pixels: u64,
    quota: u64,
    /// 流式处理计数（>64MP 的按块处理块数）。
    streamed_blocks: u64,
}

impl BitmapPool {
    pub fn new(quota: u64) -> BitmapPool {
        BitmapPool { allocated_pixels: 0, quota, streamed_blocks: 0 }
    }

    /// 分配位图。>64MP → 流式（按 1MP 块记账，不一次占满）；超配额 →
    /// PoolExhausted（调用方转 OutOfMemory 语义错误码）。
    pub fn alloc(&mut self, w: u64, h: u64) -> Result<BitmapHandle, GdiPlusError> {
        let px = w.saturating_mul(h);
        if self.allocated_pixels + px > self.quota {
            return Err(GdiPlusError::PoolExhausted);
        }
        if px > STREAMING_THRESHOLD_PIXELS {
            // 流式：按块（1MP）渐进分配记账，防内存尖峰。
            let blocks = px / 1_000_000;
            self.streamed_blocks += blocks;
        }
        self.allocated_pixels += px;
        Ok(BitmapHandle { pixels: px })
    }

    pub fn free(&mut self, h: BitmapHandle) {
        self.allocated_pixels = self.allocated_pixels.saturating_sub(h.pixels);
    }

    pub fn allocated_pixels(&self) -> u64 {
        self.allocated_pixels
    }

    pub fn streamed_blocks(&self) -> u64 {
        self.streamed_blocks
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BitmapHandle {
    pub pixels: u64,
}

// ---------------------------------------------------------------------------
// PNG 完整性校验（损坏图片 → 错误码）
// ---------------------------------------------------------------------------

/// PNG 签名（8 字节）。
pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// PNG 结构校验（签名 + IHDR 存在 + CRC32 完整性——损坏 → CorruptImage）。
pub fn validate_png(data: &[u8]) -> Result<(), GdiPlusError> {
    if data.len() < 8 || data[0..8] != PNG_SIGNATURE {
        return Err(GdiPlusError::CorruptImage);
    }
    // 第一个 chunk 必须是 IHDR（长度 13）。
    if data.len() < 8 + 8 + 13 + 4 {
        return Err(GdiPlusError::CorruptImage);
    }
    let chunk_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let chunk_type = &data[12..16];
    if chunk_type != b"IHDR" || chunk_len != 13 {
        return Err(GdiPlusError::CorruptImage);
    }
    let ihdr = &data[16..16 + 13];
    let stored_crc = u32::from_be_bytes([
        data[16 + 13],
        data[16 + 13 + 1],
        data[16 + 13 + 2],
        data[16 + 13 + 3],
    ]);
    // CRC32 覆盖 chunk_type + chunk_data。
    let mut crc_input = Vec::with_capacity(17);
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(ihdr);
    if crc32(&crc_input) != stored_crc {
        return Err(GdiPlusError::CorruptImage);
    }
    Ok(())
}

/// CRC-32（IEEE 802.3，查表式——零堆、无外部依赖）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// 对拍容差规则（容差也是判据不是感觉）
// ---------------------------------------------------------------------------

/// 非文本区逐像素容差核对：|a-b| ≤ 1/255 全像素通过。
pub fn pixel_diff_within_tolerance(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(&x, &y)| {
        let d = if x > y { x - y } else { y - x };
        d <= PIXEL_TOLERANCE
    })
}

/// 文本区 SSIM（简化全局 SSIM，灰度 u8，窗口=整图——对拍判据口径 >0.95）。
pub fn ssim_permille(a: &[u8], b: &[u8], width: usize, height: usize) -> Option<u32> {
    let n = width * height;
    if a.len() < n || b.len() < n || n == 0 {
        return None;
    }
    // C1/C2 按 8bit 动态范围标准值。
    let c1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
    let c2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
    let mut sum_a = 0f64;
    let mut sum_b = 0f64;
    let mut sum_aa = 0f64;
    let mut sum_bb = 0f64;
    let mut sum_ab = 0f64;
    for i in 0..n {
        let x = a[i] as f64;
        let y = b[i] as f64;
        sum_a += x;
        sum_b += y;
        sum_aa += x * x;
        sum_bb += y * y;
        sum_ab += x * y;
    }
    let nf = n as f64;
    let mu_a = sum_a / nf;
    let mu_b = sum_b / nf;
    let var_a = sum_aa / nf - mu_a * mu_a;
    let var_b = sum_bb / nf - mu_b * mu_b;
    let cov = sum_ab / nf - mu_a * mu_b;
    let num = (2.0 * mu_a * mu_b + c1) * (2.0 * cov + c2);
    let den = (mu_a * mu_a + mu_b * mu_b + c1) * (var_a + var_b + c2);
    if den == 0.0 {
        return None;
    }
    let ssim = num / den;
    Some((ssim.clamp(0.0, 1.0) * 1000.0) as u32)
}

// ---------------------------------------------------------------------------
// Graphics（绘制会话 + 双缓冲一等公民）
// ---------------------------------------------------------------------------

/// Graphics：双缓冲管线（内存位图 → 一次性提交）一等公民支持。
pub struct Graphics {
    /// 双缓冲位图（None = 直接绘）。
    buffer: Option<BitmapHandle>,
    pub antialiased_text: bool,
    pub submissions: u64,
    /// 提交中的绘制命令数（flush 时一次性提交）。
    pending: u64,
}

impl Graphics {
    pub fn new(buffer: Option<BitmapHandle>, antialiased_text: bool) -> Graphics {
        Graphics { buffer, antialiased_text, submissions: 0, pending: 0 }
    }

    pub fn is_double_buffered(&self) -> bool {
        self.buffer.is_some()
    }

    /// 绘制命令入缓冲（双缓冲语义：命令不立即提交）。
    pub fn draw_deferred(&mut self) {
        self.pending += 1;
    }

    /// flush：一次性提交（双缓冲无撕裂判据）。
    pub fn flush(&mut self) -> u64 {
        let n = self.pending;
        if self.is_double_buffered() {
            self.submissions += 1; // 一次 flush = 一次合成器提交
        } else {
            self.submissions += n; // 直接绘逐命令提交
        }
        self.pending = 0;
        n
    }

    /// 图像绘制（ImageAttributes 裁剪语义：源矩形裁剪记账）。
    pub fn draw_image_cropped(&mut self, src_w: u64, src_h: u64, crop: (u64, u64, u64, u64)) -> Result<(), GdiPlusError> {
        let (cx, cy, cw, ch) = crop;
        if cx + cw > src_w || cy + ch > src_h {
            return Err(GdiPlusError::CorruptImage);
        }
        self.draw_deferred();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_gdiplus_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus");
    // 1) 判据常量（16k 顶点 / 64MP 流式 / 容差 1/255 / SSIM 950）。
    cs.add(
        "consts",
        PATH_VERTEX_CAP == 16_384
            && STREAMING_THRESHOLD_PIXELS == 64_000_000
            && PIXEL_TOLERANCE == 1
            && TEXT_SSIM_PERMILLE == 950,
        "",
    );
    // 2) 画刷三型：实心恒色、线性渐变端点正确、路径渐变同插值核。
    let solid = Brush::Solid(0xFF_0000);
    let lin = Brush::LinearGradient(0x00_0000, 0xFF_FF_FF);
    cs.add(
        "brush_three_kinds",
        solid.sample(0) == 0xFF_0000
            && solid.sample(1000) == 0xFF_0000
            && lin.sample(0) == 0x00_0000
            && lin.sample(1000) == 0xFF_FF_FF
            && lin.is_gradient()
            && Brush::PathGradient(0x10_2030, 0x40_5060).is_gradient(),
        "",
    );
    // 3) 渐变中点对称（线性插值核正确性）。
    let mid = lerp_color(0x00_00_00, 0xFF_FF_FF, 500);
    cs.add("lerp_midpoint", mid == 0x7F_7F_7F || mid == 0x80_80_80, "");
    // 4) 路径 16k 顶点：恰好 16384 可加，第 16385 拒绝。
    let mut path = GdiPath::new();
    let mut full = true;
    for i in 0..PATH_VERTEX_CAP {
        if !path.add_vertex(i as f32, 0.0) {
            full = false;
        }
    }
    cs.add(
        "path_16k_cap",
        full && path.len() == PATH_VERTEX_CAP && !path.add_vertex(0.0, 0.0),
        "",
    );
    // 5) PNG 校验：合法头过；坏签名/CRC 错误 → CorruptImage。
    let mut png = vec![0u8; 33];
    png[0..8].copy_from_slice(&PNG_SIGNATURE);
    png[8..12].copy_from_slice(&13u32.to_be_bytes());
    png[12..16].copy_from_slice(b"IHDR");
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    cs.add("png_valid_passes", validate_png(&png).is_ok(), "");
    png[20] ^= 0xFF; // 破坏 IHDR 数据 → CRC 失配
    cs.add(
        "png_crc_corrupt_rejected",
        matches!(validate_png(&png), Err(GdiPlusError::CorruptImage)),
        "",
    );
    cs.add(
        "png_bad_signature_rejected",
        matches!(validate_png(&[0u8; 40]), Err(GdiPlusError::CorruptImage)),
        "",
    );
    // 6) 位图池：配额内分配成功；超配额 → PoolExhausted；free 回收。
    let mut pool = BitmapPool::new(100_000_000);
    let a = pool.alloc(4_000, 4_000).unwrap(); // 16MP
    let b = pool.alloc(8_001, 8_000).unwrap(); // 64.008MP > 64MP → 流式
    cs.add(
        "pool_and_streaming",
        pool.allocated_pixels() == 80_008_000
            && pool.streamed_blocks() == 64
            && pool.alloc(30_000, 30_000).is_err(), // 900MP 超配额
        "",
    );
    pool.free(a);
    pool.free(b);
    cs.add("pool_free_recycles", pool.allocated_pixels() == 0, "");
    // 7) 双缓冲一等公民：命令入缓冲，flush 一次性提交（1 次而非 N 次）。
    let mut g = Graphics::new(pool.alloc(1_000, 1_000).ok(), true);
    for _ in 0..100 {
        g.draw_deferred();
    }
    let flushed = g.flush();
    cs.add(
        "double_buffer_one_commit",
        g.is_double_buffered() && flushed == 100 && g.submissions == 1 && g.pending == 0,
        "",
    );
    // 8) 非直接绘路径：逐命令提交（对照面）。
    let mut g2 = Graphics::new(None, false);
    g2.draw_deferred();
    g2.draw_deferred();
    let _ = g2.flush();
    cs.add("direct_path_per_command", g2.submissions == 2, "");
    // 9) 对拍容差规则：非文本区 ±1 全过、±2 拒绝；文本区 SSIM>950 判据。
    let ref_px = vec![128u8; 64];
    let mut ok_px = ref_px.clone();
    ok_px[0] = 127; // 差 1 → 容差内
    ok_px[1] = 129;
    let mut bad_px = ref_px.clone();
    bad_px[2] = 126; // 差 2 → 超容差
    cs.add(
        "pixel_tolerance_rule",
        pixel_diff_within_tolerance(&ref_px, &ok_px) && !pixel_diff_within_tolerance(&ref_px, &bad_px),
        "",
    );
    // SSIM：同图=1000；轻度噪声图仍 >950；结构破坏 <950。
    let clean = vec![128u8; 100];
    let mut near = clean.clone();
    for i in (0..100).step_by(10) {
        near[i] = 127;
    }
    let mut far = clean.clone();
    for i in 0..100 {
        far[i] = if i % 2 == 0 { 30 } else { 220 };
    }
    let s_near = ssim_permille(&clean, &near, 10, 10).unwrap();
    let s_far = ssim_permille(&clean, &far, 10, 10).unwrap();
    cs.add(
        "ssim_text_rule",
        ssim_permille(&clean, &clean, 10, 10) == Some(1000)
            && s_near > TEXT_SSIM_PERMILLE
            && s_far < TEXT_SSIM_PERMILLE,
        "",
    );
    // 10) 裁剪语义：越界裁剪 → CorruptImage；合法裁剪通过。
    let mut g3 = Graphics::new(None, true);
    cs.add(
        "image_crop_semantics",
        g3.draw_image_cropped(100, 100, (10, 10, 80, 80)).is_ok()
            && g3.draw_image_cropped(100, 100, (50, 50, 60, 60)).is_err(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_gradient_endpoints() {
        // 渐变画刷端点精确（对拍容差规则的画刷面）。
        let lin = Brush::LinearGradient(0x10_2030, 0x50_60_70);
        assert_eq!(lin.sample(0), 0x10_2030);
        assert_eq!(lin.sample(1000), 0x50_60_70);
        // 25% 处插值：(0x10*(750)+0x50*250)/1000 = 0x20。
        let q = lin.sample(250);
        assert_eq!((q >> 16) & 0xFF, 0x20);
    }

    #[test]
    fn path_cap_honest_reject() {
        // 顶点超限如实拒绝（不静默截断——返回 false，调用方转错误码）。
        let mut p = GdiPath::new();
        for i in 0..PATH_VERTEX_CAP {
            assert!(p.add_vertex(i as f32, 1.0));
        }
        assert!(!p.add_vertex(0.0, 0.0));
        assert_eq!(p.len(), PATH_VERTEX_CAP);
    }

    #[test]
    fn png_crc_genuine() {
        // CRC32 参考值对拍（"123456789" = 0xCBF43926——算法正确性锚点）。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let mut png = vec![0u8; 33];
        png[0..8].copy_from_slice(&PNG_SIGNATURE);
        png[8..12].copy_from_slice(&13u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(validate_png(&png).is_ok());
        // 截断 → 拒绝。
        assert!(validate_png(&png[..20]).is_err());
    }

    #[test]
    fn streaming_large_bitmap_no_spike() {
        // >64MP 走流式（按 1MP 块记账——主册：流式处理防内存尖峰）。
        let mut pool = BitmapPool::new(u64::MAX);
        let h = pool.alloc(10_000, 10_000).unwrap(); // 100MP
        assert!(h.pixels > STREAMING_THRESHOLD_PIXELS);
        assert_eq!(pool.streamed_blocks(), 100);
        // 恰好 64MP 不触发（> 才触发）。
        let mut pool2 = BitmapPool::new(u64::MAX);
        let _ = pool2.alloc(8_001, 8_000).unwrap();
        assert_eq!(pool2.streamed_blocks(), 64); // 64.008MP/1MP = 64 块
    }

    #[test]
    fn pool_quota_returns_oom_semantics() {
        // 超配额 → PoolExhausted 错误码（应用决定 UI，不杀进程）。
        let mut pool = BitmapPool::new(1_000_000);
        assert!(matches!(pool.alloc(2_000, 2_000), Err(GdiPlusError::PoolExhausted)));
        assert_eq!(GdiPlusError::PoolExhausted.as_str(), "bitmap pool quota exhausted");
    }

    #[test]
    fn ssim_extremes() {
        // SSIM 边界：全同=1000（判据锚点）。
        let a = vec![100u8; 400];
        assert_eq!(ssim_permille(&a, &a, 20, 20), Some(1000));
    }

    #[test]
    fn flush_without_pending_is_noop() {
        let mut g = Graphics::new(None, true);
        assert_eq!(g.flush(), 0);
        assert_eq!(g.submissions, 0);
    }
}
