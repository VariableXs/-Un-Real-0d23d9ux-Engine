
// ---------------------------------------------------------------------------
// F007 · 深化批次三：大位图流式分带计划（>64MP 防内存尖峰——带数/带高/单带
// 峰值上界）
//
// 主册依据（G-A-07【数据与存储】）：「大位图（>64MP）流式处理防内存尖峰」。
// 既有面：STREAMING_THRESHOLD_PIXELS（64MP 线）与 streamed_blocks 计数；本段
// 给出分带计划（怎么流：带数与带高的确定性切分——流式不是口号，是可验算的
// 几何计划）。
// ---------------------------------------------------------------------------

/// 单带像素上界（分带后任意一带的扫描行缓冲 ≤ 此值——内存尖峰的硬顶）。
pub const STREAM_BAND_MAX_PIXELS: u64 = 8_000_000;

/// 流式分带计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamingBandPlan {
    pub width: u64,
    pub height: u64,
    /// 分带数（≤64MP 恒 1——不做无谓切分）。
    pub bands: u32,
    /// 每带行数（末带允许矮——向上取整切分）。
    pub band_height: u64,
}

impl StreamingBandPlan {
    /// 为位图出分带计划：≤64MP 单带直读；>64MP 按单带 ≤8MP 向上取整切分。
    pub fn for_bitmap(width: u64, height: u64) -> StreamingBandPlan {
        if width == 0 || height == 0 {
            return StreamingBandPlan { width, height, bands: 0, band_height: 0 };
        }
        let pixels = width * height;
        if pixels <= STREAMING_THRESHOLD_PIXELS {
            StreamingBandPlan { width, height, bands: 1, band_height: height }
        } else {
            // 行域切分：单带行数上界 bh_max = 单带像素上界 / 行宽（向下取整，
            // 至少 1 行）；带数 = ceil(height / bh_max)。该切法保证带高向上取整
            // 后仍 ≤ bh_max（h ≤ bands×bh_max ⇒ ceil(h/bands) ≤ bh_max），从而
            // 单带峰值 ≤ 上界——几何自洽不靠巧合。（极端长条位图（行宽超上界）
            // 退化为逐行流式：带高 1，峰值 = 行宽×1，如实不越判据面。）
            let bh_max = (STREAM_BAND_MAX_PIXELS / width).max(1);
            let bands = ((height + bh_max - 1) / bh_max) as u32;
            let band_height = (height + bands as u64 - 1) / bands as u64;
            StreamingBandPlan { width, height, bands, band_height }
        }
    }

    /// 单带峰值像素（width × band_height ≤ 单带上界——判据恒等式；
    /// 单带位图按整图计，不适用切分上界）。
    pub fn peak_band_pixels(&self) -> u64 {
        if self.bands <= 1 {
            return self.width * self.height;
        }
        self.width * self.band_height
    }

    /// 覆盖完整性：bands × band_height ≥ height（无行遗漏）。
    pub fn covers_all_rows(&self) -> bool {
        if self.bands == 0 {
            return self.height == 0;
        }
        self.bands as u64 * self.band_height >= self.height
    }
}

/// F007 深化批次三自检。
pub fn run_gdiplus_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep2");
    // 1) 3840×2160（8.3MP）≤64MP → 单带；8000×10000（80MP）→ 10 带、
    //    带高 1000、单带峰值 8MP = 上界（恰好达线不越界）。
    let small = StreamingBandPlan::for_bitmap(3840, 2160);
    let big = StreamingBandPlan::for_bitmap(8000, 10000);
    cs.add(
        "streaming_band_plan_geometry",
        small.bands == 1
            && small.band_height == 2160
            && big.bands == 10
            && big.band_height == 1000
            && big.peak_band_pixels() == STREAM_BAND_MAX_PIXELS,
        "",
    );
    // 2) 覆盖完整性：两计划都无行遗漏；上界恒等式对 >64MP 的竖高成立
    //    （≤64MP 单带不适用切分上界——整图直读语义）。
    let mut bounded = true;
    for h in [17000u64, 20000, 25000] {
        let p = StreamingBandPlan::for_bitmap(3840, h);
        bounded &= p.covers_all_rows() && p.peak_band_pixels() <= STREAM_BAND_MAX_PIXELS;
    }
    cs.add(
        "streaming_band_covers_all_rows",
        small.covers_all_rows() && big.covers_all_rows() && bounded,
        "",
    );
    // 3) 阈值线不变锚（64MP——与既有 STREAMING_THRESHOLD_PIXELS 同源对账）。
    cs.add(
        "stream_threshold_anchor",
        STREAMING_THRESHOLD_PIXELS == 64_000_000,
        "",
    );
    cs
}
