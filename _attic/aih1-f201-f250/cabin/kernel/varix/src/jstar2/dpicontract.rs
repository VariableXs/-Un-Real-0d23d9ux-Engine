//! F636 DPI 自适配契约 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：升采样触发与质量（Lanczos 对拍）；标注三处
//! 可见（详情/导入/分享）；原生优先判据；SVG 派生优先级；升采样缓存
//! 零逐帧开销实测。
//!
//! **契约语义（任意来源指针的运行时适配）**：
//! - **原生优先**：有原生 2x 帧的方案在 150/200% 永远用原生（铁序）；
//! - **SVG 派生优先**：vector_source 方案的放大走矢量重栅格（jbase
//!   双倍率复制口径——矢量纪律天然满足 4K），优先于位图插值；
//! - **Lanczos 兜底**：只有 1x 图的位图方案在 125/150/200% 自动升采样
//!   渲染，并**诚实标注「增强渲染」**（不糊弄不虚标——方案详情页如实
//!   写明原始分辨率）；
//! - **标注三处可见**：`enhanced_render` 旗标随方案元数据走，详情
//!   （detail_label）/导入（import_label）/分享（share_label）三个读取
//!   口全部如实反映（同一旗标，一处一事实）；
//! - **升采样缓存**：`(方案指纹, 态, 帧号, DPI)` 键控缓存——命中路径
//!   零重采样计算（零逐帧开销的机制面：计数器对账首次/命中次数）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    resample_lanczos3, vxcur_fingerprint, CursorFrame, CursorSchemeModel, PixBuf, PointerState,
};
#[cfg(test)]
use crate::jstar2::jbase::{ALL_STATES, OriginKind};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// DPI 档位
// ---------------------------------------------------------------------------

/// 四档 DPI（%）。
pub const DPI_TIERS: [u32; 4] = [100, 125, 150, 200];

/// 渲染来源（诚实标注的枚举面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderSource {
    /// 原生 2x 帧（原生优先铁序）。
    Native2x,
    /// 矢量派生（vector_source 方案）。
    VectorDerived,
    /// 位图 Lanczos 升采样（1x 兜底）。
    EnhancedUpsample,
    /// 原样（100% 或已等尺寸）。
    AsIs,
}

impl RenderSource {
    /// 用户可见标注（「增强渲染」诚实标注——不糊弄不虚标）。
    pub fn label(self) -> &'static str {
        match self {
            RenderSource::Native2x => "原生高清",
            RenderSource::VectorDerived => "矢量派生",
            RenderSource::EnhancedUpsample => "增强渲染",
            RenderSource::AsIs => "原样",
        }
    }
}

// ---------------------------------------------------------------------------
// 缓存
// ---------------------------------------------------------------------------

/// 缓存键（方案指纹, 态, 帧号, DPI）。
type CacheKey = (u64, u8, usize, u32);

/// 升采样缓存（容量 64——LRU 语义简化为 FIFO 淘汰，计数器对账）。
pub struct UpsampleCache {
    entries: Vec<(CacheKey, PixBuf)>,
    cap: usize,
    pub hits: u64,
    pub misses: u64,
    pub evicted: u64,
}

impl UpsampleCache {
    pub fn new(cap: usize) -> UpsampleCache {
        UpsampleCache { entries: Vec::with_capacity(cap.min(64)), cap, hits: 0, misses: 0, evicted: 0 }
    }

    pub fn get(&mut self, k: &CacheKey) -> Option<PixBuf> {
        if let Some(i) = self.entries.iter().position(|(key, _)| key == k) {
            let buf = self.entries[i].1.clone();
            self.hits += 1;
            return Some(buf);
        }
        self.misses += 1;
        None
    }

    pub fn put(&mut self, k: CacheKey, buf: PixBuf) {
        if self.entries.iter().any(|(key, _)| *key == k) {
            return;
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
            self.evicted += 1;
        }
        self.entries.push((k, buf));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for UpsampleCache {
    fn default() -> Self {
        UpsampleCache::new(64)
    }
}

// ---------------------------------------------------------------------------
// 契约主决策
// ---------------------------------------------------------------------------

/// 适配决策：某方案某帧在某 DPI 下的渲染来源。
/// 原生优先 → 矢量派生 → Lanczos 兜底（判据铁序）。
pub fn render_source_for(m: &CursorSchemeModel, st: PointerState, frame_i: usize, dpi: u32) -> RenderSource {
    if dpi == 100 {
        return RenderSource::AsIs;
    }
    let Some(e) = m.state(st) else { return RenderSource::AsIs };
    let Some(f) = e.frames.get(frame_i) else { return RenderSource::AsIs };
    // 原生 2x：方案声明或存在 ≥2× 尺寸帧（150% 也走 2x 原生采样）。
    if m.native_2x || e.frames.iter().any(|o| o.w >= f.w * 2 && o.h >= f.h * 2) {
        return RenderSource::Native2x;
    }
    if m.vector_source {
        return RenderSource::VectorDerived;
    }
    RenderSource::EnhancedUpsample
}

/// 目标渲染尺寸（DPI 缩放，1/1000 定点）。
pub fn target_size(f: &CursorFrame, dpi: u32) -> (u16, u16) {
    let s = dpi * 1000 / 100;
    (((f.w as u32) * s / 1000).max(1) as u16, ((f.h as u32) * s / 1000).max(1) as u16)
}

/// 渲染一帧（走契约 + 缓存；返回 (像素, 来源)）。
pub fn render_frame(
    m: &CursorSchemeModel,
    st: PointerState,
    frame_i: usize,
    dpi: u32,
    cache: &mut UpsampleCache,
) -> Option<(PixBuf, RenderSource)> {
    let e = m.state(st)?;
    let f = e.frames.get(frame_i)?;
    let src = render_source_for(m, st, frame_i, dpi);
    let key: CacheKey = (vxcur_fingerprint(m), st.id(), frame_i, dpi);
    let (tw, th) = target_size(f, dpi);
    match src {
        RenderSource::AsIs => Some((f.buf(), src)),
        RenderSource::Native2x => {
            // 取 ≥2× 的原生帧直接输出（不再缩放——原生优先铁序）。
            let big = e.frames.iter().find(|o| o.w >= f.w * 2 && o.h >= f.h * 2);
            Some((big.map(|b| b.buf()).unwrap_or_else(|| f.buf()), src))
        }
        RenderSource::VectorDerived => {
            // 矢量派生：1x → 2x 精确复制（jbase 复制核，矢量纪律）。
            if let Some(hit) = cache.get(&key) {
                return Some((hit, src));
            }
            let buf = f.buf().scale_integer2x();
            cache.put(key, buf.clone());
            Some((buf, src))
        }
        RenderSource::EnhancedUpsample => {
            if let Some(hit) = cache.get(&key) {
                return Some((hit, src));
            }
            let buf = resample_lanczos3(&f.buf(), tw, th);
            cache.put(key, buf.clone());
            Some((buf, src))
        }
    }
}

// ---------------------------------------------------------------------------
// 诚实标注（三处可见）
// ---------------------------------------------------------------------------

/// 详情页标注（含原始分辨率——如实写明成色）。
pub fn detail_label(m: &CursorSchemeModel) -> String {
    let native_px = m.max_frame_px();
    if m.enhanced_render || m.native_2x || m.vector_source {
        alloc::format!(
            "原始分辨率 {}px；{}",
            native_px,
            if m.vector_source { "矢量源（任意缩放清晰）" } else if m.native_2x { "含原生 2x 帧" } else { "高 DPI 下为增强渲染（非原生）" }
        )
    } else {
        alloc::format!("原始分辨率 {native_px}px；高 DPI 下将增强渲染并如实标注")
    }
}

/// 导入对话框标注。
pub fn import_label(m: &CursorSchemeModel) -> &'static str {
    if m.native_2x {
        "含原生高清帧"
    } else if m.vector_source {
        "矢量源"
    } else if m.enhanced_render {
        "增强渲染（原始 1x）"
    } else {
        "高 DPI 下将增强渲染"
    }
}

/// 分享导出标注（随包元数据）。
pub fn share_label(m: &CursorSchemeModel) -> &'static str {
    import_label(m)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F636 自检。
pub fn run_dpicontract_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F636");

    // 1. 原生优先：声明 2x 的方案在 200% 走 Native2x。
    let mut native = builtin_default_scheme();
    native.native_2x = true;
    set.add(
        "native-2x preferred at high dpi",
        render_source_for(&native, PointerState::Normal, 0, 200) == RenderSource::Native2x,
        "",
    );

    // 2. 隐式 2x 检出：帧序列里存在 ≥2× 尺寸帧同样走原生。
    let mut implicit = builtin_default_scheme();
    {
        let e = implicit.state_mut(PointerState::Normal).unwrap();
        let up = e.frames[0].buf().scale_integer2x();
        e.frames.push(CursorFrame::from_buf(8, 4, 0, up));
    }
    set.add(
        "implicit 2x frame detected as native",
        render_source_for(&implicit, PointerState::Normal, 0, 150) == RenderSource::Native2x,
        "",
    );

    // 3. 矢量派生优先于位图插值（vector_source 且无 2x）。
    let mut vecm = builtin_default_scheme();
    vecm.vector_source = true;
    set.add(
        "vector derived before bitmap upsample",
        render_source_for(&vecm, PointerState::Normal, 0, 200) == RenderSource::VectorDerived,
        "",
    );

    // 4. 只有 1x 的位图方案 → EnhancedUpsample + Lanczos 触发与质量。
    let plain = builtin_default_scheme();
    let mut cache = UpsampleCache::default();
    let (buf200, src) = render_frame(&plain, PointerState::Normal, 0, 200, &mut cache).unwrap();
    let f0 = plain.state(PointerState::Normal).unwrap().frames[0].buf();
    set.add(
        "1x bitmap upsampled via lanczos",
        src == RenderSource::EnhancedUpsample && buf200.w == 64 && buf200.h == 64 && buf200.diff_pixels(&f0) == None,
        "",
    );

    // 5. 升采样缓存零逐帧开销：二次渲染命中（misses=1, hits≥1）。
    let before_hits = cache.hits;
    let (buf200b, _) = render_frame(&plain, PointerState::Normal, 0, 200, &mut cache).unwrap();
    set.add(
        "upsample cache hit zero recompute",
        cache.hits == before_hits + 1 && buf200b.diff_pixels(&buf200) == Some(0),
        "",
    );

    // 6. Lanczos 质量对拍：常色区域升采样后色值保持（jbase 核性质）。
    let mut flat = builtin_default_scheme();
    {
        let e = flat.state_mut(PointerState::Normal).unwrap();
        let mut b = e.frames[0].buf();
        for c in b.px.chunks_exact_mut(4) {
            c[0] = 200;
            c[1] = 100;
            c[2] = 50;
            c[3] = 255;
        }
        e.frames[0].px = b.px;
    }
    let mut c2 = UpsampleCache::default();
    let (bufq, _) = render_frame(&flat, PointerState::Normal, 0, 150, &mut c2).unwrap();
    let ok_q = bufq.px.chunks_exact(4).all(|c| {
        (c[0] as i32 - 200).abs() <= 3 && (c[1] as i32 - 100).abs() <= 3 && (c[2] as i32 - 50).abs() <= 3 && c[3] == 255
    });
    set.add("lanczos preserves constant color", ok_q, "");

    // 7. 标注三处可见（同一旗标三读取口一致 + 详情带原始分辨率）。
    let mut enh = builtin_default_scheme();
    enh.enhanced_render = true;
    let dl = detail_label(&enh);
    set.add(
        "labels consistent across detail/import/share",
        import_label(&enh) == "增强渲染（原始 1x）"
            && share_label(&enh) == "增强渲染（原始 1x）"
            && dl.contains("增强渲染")
            && dl.contains("32px"),
        "",
    );
    let native_label = detail_label(&native);
    set.add(
        "native label honest not fake-enhanced",
        import_label(&native) == "含原生高清帧" && native_label.contains("原生 2x"),
        "",
    );

    // 8. 100% 恒原样（不无谓重采样）。
    let (buf100, src100) = render_frame(&plain, PointerState::Normal, 0, 100, &mut cache).unwrap();
    set.add(
        "100% renders as-is",
        src100 == RenderSource::AsIs && buf100.diff_pixels(&f0) == Some(0),
        "",
    );

    // 9. 四档 DPI 契约全覆盖。
    let mut all_tiers = true;
    for dpi in DPI_TIERS {
        let s = render_source_for(&plain, PointerState::Normal, 0, dpi);
        all_tiers &= match dpi {
            100 => s == RenderSource::AsIs,
            _ => s == RenderSource::EnhancedUpsample,
        };
    }
    set.add("all four dpi tiers contracted", all_tiers, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::builtin_default_scheme;

    #[test]
    fn cache_fifo_eviction() {
        let mut c = UpsampleCache::new(2);
        c.put((1, 0, 0, 100), PixBuf::new(1, 1));
        c.put((2, 0, 0, 100), PixBuf::new(1, 1));
        c.put((3, 0, 0, 100), PixBuf::new(1, 1));
        assert_eq!(c.len(), 2);
        assert_eq!(c.evicted, 1);
        assert!(c.get(&(1, 0, 0, 100)).is_none(), "最旧被逐出");
        assert!(c.get(&(3, 0, 0, 100)).is_some());
    }

    #[test]
    fn target_size_rounding() {
        let f = CursorFrame { w: 33, h: 17, hot_x: 0, hot_y: 0, delay_ms: 0, px: Vec::new() };
        assert_eq!(target_size(&f, 125), (41, 21));
        assert_eq!(target_size(&f, 200), (66, 34));
    }

    #[test]
    fn missing_state_returns_none() {
        let m = builtin_default_scheme();
        let mut c = UpsampleCache::default();
        // 15 态齐全的内置方案不会 None；构造缺态方案验证。
        let mut holey = CursorSchemeModel::empty("h", OriginKind::Created);
        holey.set_state(PointerState::Normal, holey_state());
        assert!(render_frame(&holey, PointerState::Link, 0, 150, &mut c).is_none());
        let _ = m;
    }

    fn holey_state() -> Vec<CursorFrame> {
        alloc::vec![CursorFrame {
            w: 8,
            h: 8,
            hot_x: 1,
            hot_y: 1,
            delay_ms: 0,
            px: alloc::vec![255u8; 8 * 8 * 4],
        }]
    }

    #[test]
    fn all_states_have_contract() {
        let m = builtin_default_scheme();
        let mut c = UpsampleCache::default();
        for st in ALL_STATES {
            let (_, src) = render_frame(&m, st, 0, 200, &mut c).unwrap();
            assert_eq!(src, RenderSource::EnhancedUpsample, "{}", st.zh_name());
        }
        // 渲染来源标注面向用户诚实。
        assert_eq!(RenderSource::EnhancedUpsample.label(), "增强渲染");
    }
}
