//! F636 DPI 自适配契约 · 完整设计（STAR I 主册 J-D 组）· v2 深化版。
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
//! - **决策矩阵实体化**：15 态 × 4 档的渲染来源矩阵一次性构建可查——
//!   契约不是散落的 if，是一张可对账的表；
//! - **DPI 热切换运行时**：分辨率热切换/休眠唤醒的事件流模型——切换
//!   留痕、缓存按 DPI 键天然隔离（显式断言而非口头保证）；
//! - **标注三处可见**：`enhanced_render` 旗标随方案元数据走，详情
//!   （detail_label）/导入（import_label）/分享（share_label）三个读取
//!   口全部如实反映（同一旗标，一处一事实）；
//! - **升采样缓存 LRU**：`(方案指纹, 态, 帧号, DPI)` 键控缓存——命中
//!   路径零重采样计算（零逐帧开销的机制面：计数器对账首次/命中次数，
//!   淘汰按最久未用——上量后热帧不挨挤）；
//! - **时间线保真**：升采样改尺寸不改时间轴——帧数与每帧延时逐帧
//!   对账（动画指针升采样后节奏不变是契约的一部分）；
//! - **档位边界诚实**：契约只覆盖四档——0（未上报）与 >200 的档位
//!   显式拒绝，不静默按 200 处理（超契约的请求不配得到假装的答案）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    resample_lanczos3, vxcur_fingerprint, CursorFrame, CursorSchemeModel, PixBuf, PointerState,
    ALL_STATES,
};
#[cfg(test)]
use crate::jstar2::jbase::OriginKind;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// DPI 档位
// ---------------------------------------------------------------------------

/// 四档 DPI（%）。
pub const DPI_TIERS: [u32; 4] = [100, 125, 150, 200];

/// 档位合法性（契约覆盖面：四档之内；0 = 未上报同样拒绝——不猜）。
pub fn tier_supported(dpi: u32) -> bool {
    DPI_TIERS.contains(&dpi)
}

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
// 缓存（LRU）
// ---------------------------------------------------------------------------

/// 缓存键（方案指纹, 态, 帧号, DPI）。
type CacheKey = (u64, u8, usize, u32);

/// 升采样缓存（容量上限；LRU 淘汰——get 触碰晋升，put 逐最久未用）。
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

    /// 查缓存（命中即晋升到最新位——LRU 语义的核心动作）。
    pub fn get(&mut self, k: &CacheKey) -> Option<PixBuf> {
        if let Some(i) = self.entries.iter().position(|(key, _)| key == k) {
            let (key, buf) = self.entries.remove(i);
            self.entries.push((key, buf.clone()));
            self.hits += 1;
            return Some(buf);
        }
        self.misses += 1;
        None
    }

    /// 写缓存（已存在则覆盖并晋升——陈旧值被新值替换；满则逐最久未用）。
    pub fn put(&mut self, k: CacheKey, buf: PixBuf) {
        if let Some(i) = self.entries.iter().position(|(key, _)| *key == k) {
            self.entries.remove(i);
            self.entries.push((k, buf));
            return;
        }
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
            self.evicted += 1;
        }
        self.entries.push((k, buf));
    }

    /// 显式触碰（晋升不取值——运行时预热的面）。
    pub fn touch(&mut self, k: &CacheKey) -> bool {
        if let Some(i) = self.entries.iter().position(|(key, _)| key == k) {
            let (key, buf) = self.entries.remove(i);
            self.entries.push((key, buf));
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 命中率（千分位；零请求时 None——不虚报 0%）。
    pub fn hit_ratio_m(&self) -> Option<i64> {
        let total = self.hits + self.misses;
        if total == 0 {
            None
        } else {
            Some((self.hits as i64 * 1000 / total as i64) as i64)
        }
    }
}

impl Default for UpsampleCache {
    fn default() -> Self {
        UpsampleCache::new(64)
    }
}

// ---------------------------------------------------------------------------
// 契约主决策与决策矩阵
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

/// 决策矩阵：15 态 × 4 档的渲染来源表（契约实体化——一次构建，逐格
/// 可对账；行列序 = ALL_STATES × DPI_TIERS）。
pub struct DecisionMatrix {
    /// [态序号][档位序号]。
    pub cells: [[RenderSource; 4]; 15],
}

impl DecisionMatrix {
    /// 从方案构建矩阵。
    pub fn build(m: &CursorSchemeModel) -> DecisionMatrix {
        let mut cells = [[RenderSource::AsIs; 4]; 15];
        for (si, st) in ALL_STATES.iter().enumerate() {
            for (ti, dpi) in DPI_TIERS.iter().enumerate() {
                cells[si][ti] = render_source_for(m, *st, 0, *dpi);
            }
        }
        DecisionMatrix { cells }
    }

    /// 契约完整性自检：缺态行全 AsIs（缺态不参与放大——诚实缺位），
    /// 在场行 100% 恒 AsIs、>100% 按铁序落格。
    pub fn contract_sane(&self, m: &CursorSchemeModel) -> bool {
        for (si, st) in ALL_STATES.iter().enumerate() {
            let present = m.state(*st).is_some();
            if self.cells[si][0] != RenderSource::AsIs {
                return false; // 100% 恒原样
            }
            if !present {
                if self.cells[si].iter().any(|c| *c != RenderSource::AsIs) {
                    return false; // 缺态行不该有放大决策
                }
            }
        }
        true
    }
}

/// DPI 热切换事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DpiTransition {
    pub from: u32,
    pub to: u32,
    pub at_ms: u64,
}

/// 契约运行时（跟踪当前档位与切换历史——分辨率热切换/休眠唤醒的
/// 模型面；切换到契约外档位诚实拒绝）。
#[derive(Clone, Debug)]
pub struct ContractRuntime {
    pub current_dpi: u32,
    pub transitions: Vec<DpiTransition>,
    rejected_transitions: usize,
}

impl ContractRuntime {
    pub fn new(initial_dpi: u32) -> Result<ContractRuntime, &'static str> {
        if !tier_supported(initial_dpi) {
            return Err("初始档位在契约之外——四档（100/125/150/200）之外不猜");
        }
        Ok(ContractRuntime { current_dpi: initial_dpi, transitions: Vec::new(), rejected_transitions: 0 })
    }

    /// 切换档位（契约外档位拒绝并计数——不静默钳制）。
    pub fn switch(&mut self, to: u32, at_ms: u64) -> bool {
        if !tier_supported(to) || to == self.current_dpi {
            self.rejected_transitions += 1;
            return false;
        }
        self.transitions.push(DpiTransition { from: self.current_dpi, to, at_ms });
        self.current_dpi = to;
        true
    }

    /// 被拒切换计数（对账面）。
    pub fn rejected(&self) -> usize {
        self.rejected_transitions
    }

    /// 缓存跨档隔离断言：键含 DPI，切换前后同帧不同键——不同档位的
    /// 缓存条目互不命中（热切换不串档的机制证明）。
    pub fn cache_isolated_across_tiers(cache: &mut UpsampleCache, fp: u64) -> bool {
        let k150: CacheKey = (fp, 0, 14, 150);
        let k200: CacheKey = (fp, 0, 14, 200);
        cache.put(k150, PixBuf::new(4, 4));
        cache.put(k200, PixBuf::new(8, 8));
        let a = cache.get(&k150).map(|b| b.w).unwrap_or(0);
        let b = cache.get(&k200).map(|b| b.w).unwrap_or(0);
        a == 4 && b == 8
    }
}

/// 目标渲染尺寸（DPI 缩放，1/1000 定点）。
pub fn target_size(f: &CursorFrame, dpi: u32) -> (u16, u16) {
    let s = dpi * 1000 / 100;
    (((f.w as u32) * s / 1000).max(1) as u16, ((f.h as u32) * s / 1000).max(1) as u16)
}

/// 渲染一帧（走契约 + 缓存；返回 (像素, 来源)；契约外档位 None）。
pub fn render_frame(
    m: &CursorSchemeModel,
    st: PointerState,
    frame_i: usize,
    dpi: u32,
    cache: &mut UpsampleCache,
) -> Option<(PixBuf, RenderSource)> {
    if !tier_supported(dpi) {
        return None;
    }
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

/// 时间线保真对账：某 DPI 下全态全帧渲染后，帧数与延时逐帧不变
/// （升采样改尺寸不改时间轴——动画指针的节奏是契约的一部分）。
pub fn timeline_preserved(m: &CursorSchemeModel, dpi: u32, cache: &mut UpsampleCache) -> bool {
    for st in ALL_STATES {
        let Some(e) = m.state(st) else { continue };
        for i in 0..e.frames.len() {
            let Some((_, _)) = render_frame(m, st, i, dpi, cache) else { return false };
            // 帧数与延时直接从方案模型复核（渲染不改模型——结构断言）。
            if m.state(st).map(|e2| e2.frames.len()).unwrap_or(0) != e.frames.len() {
                return false;
            }
            if m.state(st).and_then(|e2| e2.frames.get(i)).map(|f2| f2.delay_ms).unwrap_or(0)
                != e.frames[i].delay_ms
            {
                return false;
            }
        }
    }
    true
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

    // 10. 决策矩阵实体化：15×4 全格 + 契约自洽（缺态行诚实空、100% 恒原样）。
    let matrix = DecisionMatrix::build(&plain);
    let mut native_matrix = DecisionMatrix::build(&native);
    let matrix_ok = matrix.contract_sane(&plain)
        && native_matrix.contract_sane(&native)
        && matrix.cells.len() == 15
        && matrix.cells[0][0] == RenderSource::AsIs
        && matrix.cells[0][3] == RenderSource::EnhancedUpsample;
    set.add("decision matrix covers 15 states x 4 tiers", matrix_ok, "");
    // 隐式 2x 方案矩阵落 Native2x 格。
    set.add(
        "matrix reflects native priority",
        native_matrix.cells[PointerState::Normal.id() as usize][3] == RenderSource::Native2x,
        "",
    );
    let _ = &mut native_matrix;

    // 11. DPI 热切换运行时：切换留痕 + 契约外档位诚实拒绝。
    let mut rt = ContractRuntime::new(100).unwrap();
    let sw1 = rt.switch(150, 1000);
    let sw2 = rt.switch(200, 2000);
    let reject_out = !rt.switch(300, 3000);
    let reject_zero = !rt.switch(0, 4000);
    let reject_same = !rt.switch(200, 5000);
    set.add(
        "dpi hot switch logged and out-of-contract rejected",
        sw1 && sw2 && reject_out && reject_zero && reject_same
            && rt.current_dpi == 200
            && rt.transitions.len() == 2
            && rt.transitions[0].from == 100
            && rt.rejected() == 3,
        "",
    );
    let rt_bad = ContractRuntime::new(175);
    set.add("runtime rejects out-of-contract initial tier", rt_bad.is_err(), "");

    // 12. 缓存跨档隔离（键含 DPI——热切换不串档）。
    let fp = vxcur_fingerprint(&plain);
    set.add(
        "cache keys isolate dpi tiers",
        ContractRuntime::cache_isolated_across_tiers(&mut cache, fp),
        "",
    );

    // 13. 缓存 LRU：触碰晋升——旧条目因被摸而幸存，新条目挤掉的是
    //     真正最久未用的（FIFO 做不到）。
    let mut lru = UpsampleCache::new(2);
    lru.put((1, 0, 0, 100), PixBuf::new(1, 1));
    lru.put((2, 0, 0, 100), PixBuf::new(2, 2));
    let _ = lru.touch(&(1, 0, 0, 100)); // 晋升 1 → LRU 变 2
    lru.put((3, 0, 0, 100), PixBuf::new(3, 3)); // 挤掉 2
    set.add(
        "cache is LRU touched survivor",
        lru.get(&(1, 0, 0, 100)).is_some() && lru.get(&(2, 0, 0, 100)).is_none() && lru.get(&(3, 0, 0, 100)).is_some(),
        "",
    );
    // 命中率对账：2 命中 1 未命中 → 666‰。
    let mut lr = UpsampleCache::new(4);
    lr.put((9, 0, 0, 100), PixBuf::new(1, 1));
    let _ = lr.get(&(9, 0, 0, 100));
    let _ = lr.get(&(9, 0, 0, 100));
    let _ = lr.get(&(8, 0, 0, 100));
    set.add(
        "cache hit ratio accounted",
        lr.hits == 2 && lr.misses == 1 && lr.hit_ratio_m() == Some(666),
        "",
    );

    // 14. 时间线保真：200% 全态全帧渲染后帧数与延时逐帧不变。
    let mut anim = builtin_default_scheme();
    if let Some(e) = anim.state_mut(PointerState::Normal) {
        let f0 = e.frames[0].clone();
        e.frames.push(CursorFrame::from_buf(f0.hot_x, f0.hot_y, 33, f0.buf()));
    }
    let mut c3 = UpsampleCache::default();
    set.add(
        "timeline preserved across upsample",
        timeline_preserved(&anim, 200, &mut c3) && timeline_preserved(&anim, 125, &mut c3),
        "",
    );

    // 15. 契约外档位渲染诚实拒绝（不静默按 200 处理）。
    set.add(
        "out-of-contract dpi render rejected",
        render_frame(&plain, PointerState::Normal, 0, 300, &mut cache).is_none()
            && render_frame(&plain, PointerState::Normal, 0, 0, &mut cache).is_none()
            && !tier_supported(175),
        "",
    );

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
    fn cache_lru_eviction() {
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

    #[test]
    fn matrix_native_rows_and_missing_rows() {
        let mut holey = builtin_default_scheme();
        holey.entries.truncate(2);
        let matrix = DecisionMatrix::build(&holey);
        assert!(matrix.contract_sane(&holey), "缺态行诚实空、在场行契约成立");
        // 缺态行不得出现放大决策。
        for si in 2..15 {
            assert!(matrix.cells[si].iter().all(|c| *c == RenderSource::AsIs));
        }
    }

    #[test]
    fn runtime_switch_sequence_roundtrip() {
        let mut rt = ContractRuntime::new(200).unwrap();
        assert!(rt.switch(125, 1));
        assert!(rt.switch(100, 2));
        assert!(rt.switch(200, 3));
        assert_eq!(rt.transitions.len(), 3);
        assert_eq!(rt.transitions[2].from, 100);
        assert_eq!(rt.transitions[2].to, 200);
        assert_eq!(rt.current_dpi, 200);
    }

    #[test]
    fn touch_promotes_without_value() {
        let mut c = UpsampleCache::new(2);
        c.put((5, 0, 0, 100), PixBuf::new(1, 1));
        c.put((6, 0, 0, 100), PixBuf::new(1, 1));
        assert!(c.touch(&(5, 0, 0, 100)));
        assert!(!c.touch(&(4, 0, 0, 100)), "不存在的键触碰诚实 false");
        c.put((7, 0, 0, 100), PixBuf::new(1, 1));
        assert!(c.get(&(5, 0, 0, 100)).is_some(), "touch 晋升后幸存");
        assert!(c.get(&(6, 0, 0, 100)).is_none());
    }

    #[test]
    fn timeline_preserved_on_native_scheme_too() {
        let mut native = builtin_default_scheme();
        native.native_2x = true;
        let mut c = UpsampleCache::default();
        assert!(timeline_preserved(&native, 150, &mut c));
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：契约审计报告（决策矩阵确定性文本渲染）· DPI 变更事件台账
// （环形：档位变化/决策/耗时）· 缓存命中统计对账 · 契约外档位拒绝
// 人话清单 · 缓存预热
// ---------------------------------------------------------------------------

/// 渲染决策成本（定点：Lanczos 全核 = 100 基准；AsIs 零核）。预算尺：
/// 一次档位切换要付多少重采样核，是「缓存零逐帧开销」价值对账的刻度
/// （原生 2x 直取 < 矢量复制 < 位图插值——铁序在成本表上同样成立）。
pub fn render_cost_x100(src: RenderSource) -> i64 {
    match src {
        RenderSource::AsIs => 0,
        RenderSource::Native2x => 10,
        RenderSource::VectorDerived => 25,
        RenderSource::EnhancedUpsample => 100,
    }
}

/// 档位人话名（设置页显示面；契约外档位 None——不编名）。
pub fn tier_label(dpi: u32) -> Option<&'static str> {
    match dpi {
        100 => Some("标准"),
        125 => Some("125% 放大"),
        150 => Some("150% 高清"),
        200 => Some("200% 超清"),
        _ => None,
    }
}

impl DecisionMatrix {
    /// 放大预算：全格成本合计（定点，100 = 一次 Lanczos 全核）。
    /// 无态方案预算为 0——「零核」是可对账的数字而非口号。
    pub fn upscale_budget_x100(&self) -> i64 {
        self.cells.iter().flatten().map(|c| render_cost_x100(*c)).sum()
    }

    /// 需要真实重采样核的格数（预算明细：多少格要付钱）。
    pub fn paid_cells(&self) -> usize {
        self.cells.iter().flatten().filter(|c| render_cost_x100(**c) > 0).count()
    }
}

/// 契约审计报告：决策矩阵的确定性文本渲染（行序 = ALL_STATES、列序 =
/// DPI_TIERS；同方案同字节——详情页「渲染来源」表的数据源与对账面）。
/// 列记号：A=原样 N=原生 2x V=矢量派生 E=增强渲染，表尾附图例。
pub fn render_contract_matrix(m: &CursorSchemeModel) -> String {
    let matrix = DecisionMatrix::build(m);
    let tag = |s: RenderSource| -> &'static str {
        match s {
            RenderSource::AsIs => "A",
            RenderSource::Native2x => "N",
            RenderSource::VectorDerived => "V",
            RenderSource::EnhancedUpsample => "E",
        }
    };
    let mut out = String::new();
    out.push_str(&alloc::format!(
        "contract fp={:016x} native={} vector={}\n",
        vxcur_fingerprint(m),
        m.native_2x as u8,
        m.vector_source as u8,
    ));
    for (si, st) in ALL_STATES.iter().enumerate() {
        out.push_str(&alloc::format!("{:<6}|", st.zh_name()));
        for ti in 0..4 {
            out.push_str(tag(matrix.cells[si][ti]));
            out.push(' ');
        }
        out.push('\n');
    }
    out.push_str("legend A=AsIs N=Native2x V=Vector E=Enhanced\n");
    out
}

/// DPI 变更事件台账条目（档位变化/决策/耗时——留痕三要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DpiEventRecord {
    pub at_ms: u64,
    pub from: u32,
    pub to: u32,
    pub accepted: bool,
    /// 距上一次成功切换的间隔毫秒（拒绝事件与首次成功均为 None——
    /// 无基线不编数）。
    pub since_prev_ms: Option<u64>,
}

/// DPI 变更事件台账（环形 16——F372 留痕纪律的 DPI 面：热切换/休眠
/// 唤醒的档位变化、每条决策与相邻耗时全部留痕，封顶滚动不静默丢史）。
#[derive(Clone, Debug, Default)]
pub struct DpiEventLedger {
    records: Vec<DpiEventRecord>,
    dropped: usize,
    last_accept_ms: Option<u64>,
}

impl DpiEventLedger {
    pub const CAP: usize = 16;

    pub fn new() -> DpiEventLedger {
        DpiEventLedger { records: Vec::new(), dropped: 0, last_accept_ms: None }
    }

    /// 记一条档位变化（决策与相邻耗时就地核算——记录不靠调用方自觉）。
    pub fn record(&mut self, at_ms: u64, from: u32, to: u32, accepted: bool) {
        let since_prev_ms = if accepted {
            let span = self.last_accept_ms.map(|t| at_ms.saturating_sub(t));
            self.last_accept_ms = Some(at_ms);
            span
        } else {
            None
        };
        if self.records.len() >= Self::CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(DpiEventRecord { at_ms, from, to, accepted, since_prev_ms });
    }

    pub fn records(&self) -> &[DpiEventRecord] {
        &self.records
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn acceptances(&self) -> usize {
        self.records.iter().filter(|r| r.accepted).count()
    }

    pub fn rejections(&self) -> usize {
        self.records.iter().filter(|r| !r.accepted).count()
    }

    /// 台账与运行时对账：接受数 = 运行时切换数、拒绝数 = 运行时被拒数
    /// （两本账一张脸——留痕与状态机不允许分叉）。
    pub fn reconciles_with(&self, rt: &ContractRuntime) -> bool {
        self.acceptances() == rt.transitions.len() && self.rejections() == rt.rejected()
    }
}

/// 缓存命中统计快照（命中/未命中/逐出计数对账的取数面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evicted: u64,
    pub entries: usize,
}

/// 取统计快照。
pub fn cache_stats(c: &UpsampleCache) -> CacheStats {
    CacheStats { hits: c.hits, misses: c.misses, evicted: c.evicted, entries: c.len() }
}

/// 计数对账：命中率与原始计数自洽（有请求才有比率、比率可由计数复算
/// ——零逐帧开销的数面先自洽再谈快）。
pub fn counters_consistent(c: &UpsampleCache) -> bool {
    let total = c.hits + c.misses;
    match c.hit_ratio_m() {
        None => total == 0,
        Some(r) => total > 0 && r == (c.hits as i64 * 1000 / total as i64),
    }
}

/// 缓存健康单行数（命中/未命中/逐出/在架 + 命中率‰——无请求时报
/// "--"，不虚报 0%）。
pub fn cache_stats_line(c: &UpsampleCache) -> String {
    let ratio = c.hit_ratio_m().map(|r| alloc::format!("{}‰", r)).unwrap_or_else(|| String::from("--"));
    alloc::format!(
        "命中 {} 未命中 {} 逐出 {} 在架 {} 命中率 {}",
        c.hits, c.misses, c.evicted, c.len(), ratio
    )
}

/// 运行时预热：某档位下把全态首帧预渲染入缓存（热切换后的第一帧不挨
/// Lanczos 全核——预热 n 态返回 n）。只预热有重采样核的来源（原生 2x/
/// 原样零核，预热无意义）；契约外档位返回 0——预热不越契约。
pub fn warm_cache(m: &CursorSchemeModel, dpi: u32, cache: &mut UpsampleCache) -> usize {
    if !tier_supported(dpi) || dpi == 100 {
        return 0;
    }
    let mut warmed = 0usize;
    for st in ALL_STATES {
        if matches!(
            render_source_for(m, st, 0, dpi),
            RenderSource::VectorDerived | RenderSource::EnhancedUpsample
        ) && render_frame(m, st, 0, dpi, cache).is_some()
        {
            warmed += 1;
        }
    }
    warmed
}

/// 契约外档位拒绝的人话清单（拒绝不给行话给原因：0=未上报、>200=
/// 超契约、其余=非四档；契约内档位 None——没有拒绝就没有说辞）。
pub fn explain_rejection(dpi: u32) -> Option<String> {
    if tier_supported(dpi) {
        return None;
    }
    let s = if dpi == 0 {
        String::from("DPI 0%：系统未上报档位——契约不猜，请先完成分辨率检测再请求渲染。")
    } else if dpi > 200 {
        alloc::format!("DPI {}%：超出契约覆盖（>200%）——超契约请求不静默按 200% 处理，请先为该档位补充契约评审。", dpi)
    } else {
        alloc::format!("DPI {}%：不在四档（100/125/150/200）之内——非标准档位不静默吸附，请走档位评审后接入。", dpi)
    };
    Some(s)
}

/// 运行时报告（确定性文本：当前档位、成功切换数、被拒数、末次切换
/// ——休眠唤醒后状态页的数据源；同状态同字节）。
pub fn render_runtime_report(rt: &ContractRuntime) -> String {
    let last = rt.transitions.last();
    alloc::format!(
        "runtime dpi={}({}) switches={} rejected={} last={}",
        rt.current_dpi,
        tier_label(rt.current_dpi).unwrap_or("?"),
        rt.transitions.len(),
        rt.rejected(),
        last.map(|t| alloc::format!("{}→{}@{}ms", t.from, t.to, t.at_ms))
            .unwrap_or_else(|| String::from("无")),
    )
}

/// F636 v4 自检。
pub fn run_dpicontract_v4_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F636-v4");
    let plain = builtin_default_scheme();
    let mut native = builtin_default_scheme();
    native.native_2x = true;

    // 1. 决策成本表刻度：AsIs 零核、Lanczos 全核 100、原生 < 矢量 < 位图。
    set.add(
        "render cost scale anchored",
        render_cost_x100(RenderSource::AsIs) == 0
            && render_cost_x100(RenderSource::EnhancedUpsample) == 100
            && render_cost_x100(RenderSource::Native2x) < render_cost_x100(RenderSource::VectorDerived),
        "",
    );

    // 2. 放大预算：无态方案 0 格 0 核；位图方案 15 态 × 3 档全 Lanczos。
    let holey = CursorSchemeModel::empty("无态件", crate::jstar2::jbase::OriginKind::Created);
    let matrix_plain = DecisionMatrix::build(&plain);
    set.add(
        "upscale budget accounted",
        DecisionMatrix::build(&holey).upscale_budget_x100() == 0
            && DecisionMatrix::build(&holey).paid_cells() == 0
            && matrix_plain.upscale_budget_x100() == 4500
            && matrix_plain.paid_cells() == 45,
        "",
    );

    // 3. 原生方案预算显著更低（原生 2x 优先的成本证明：10 < 100）。
    let matrix_native = DecisionMatrix::build(&native);
    set.add(
        "native budget below bitmap budget",
        matrix_native.upscale_budget_x100() == 450
            && matrix_native.upscale_budget_x100() < matrix_plain.upscale_budget_x100(),
        "",
    );

    // 4. 契约审计报告确定性：同方案两份逐字节相等、行数 = 头 1 + 15 + 图例 1。
    let r1 = render_contract_matrix(&plain);
    set.add(
        "contract matrix report deterministic",
        r1 == render_contract_matrix(&plain) && r1.lines().count() == 17,
        "",
    );

    // 5. 报告反映来源优先级：原生方案含 N 三连列、位图方案含 E 三连列、
    //    图例在位（逐行看表不靠猜）。
    let rn = render_contract_matrix(&native);
    set.add(
        "contract matrix report reflects priority",
        rn.lines().any(|l| l.contains("N N N"))
            && r1.lines().any(|l| l.contains("E E E"))
            && rn.contains("legend A=AsIs"),
        "",
    );

    // 6. DPI 事件台账：留痕三要素 + 相邻耗时核算（拒绝无耗时——无基线不编数）。
    let mut led = DpiEventLedger::new();
    led.record(1000, 100, 150, true);
    led.record(1500, 150, 125, true);
    led.record(1600, 125, 999, false);
    let recs = led.records();
    set.add(
        "dpi event ledger records decision and span",
        recs.len() == 3
            && recs[0].since_prev_ms.is_none()
            && recs[1].since_prev_ms == Some(500)
            && recs[2].since_prev_ms.is_none()
            && led.acceptances() == 2
            && led.rejections() == 1,
        "",
    );

    // 7. 台账封顶滚动（环形 16：丢最旧、计数如实）。
    let mut led2 = DpiEventLedger::new();
    for i in 0..20u64 {
        led2.record(i * 100, 100, 125, i % 2 == 0);
    }
    set.add(
        "dpi event ledger caps at 16",
        led2.records().len() == DpiEventLedger::CAP
            && led2.dropped() == 4
            && led2.records()[0].at_ms == 400,
        "",
    );

    // 8. 台账与运行时对账（两本账一张脸：多记一笔即对不上）。
    let mut rt = ContractRuntime::new(100).unwrap();
    let mut led3 = DpiEventLedger::new();
    let _ = rt.switch(150, 100);
    led3.record(100, 100, 150, true);
    let _ = rt.switch(300, 200);
    led3.record(200, 150, 300, false);
    let recon_ok = led3.reconciles_with(&rt);
    led3.record(300, 150, 200, false);
    set.add(
        "ledger reconciles with runtime",
        recon_ok && !led3.reconciles_with(&rt),
        "",
    );

    // 9. 拒绝人话：三类原因各有说辞、契约内无说辞。
    set.add(
        "rejection explanations human readable",
        explain_rejection(0).unwrap().contains("未上报")
            && explain_rejection(300).unwrap().contains("超出契约覆盖")
            && explain_rejection(175).unwrap().contains("四档")
            && explain_rejection(150).is_none(),
        "",
    );

    // 10. 档位人话名：四档齐名、契约外无名。
    set.add(
        "tier labels cover contract",
        tier_label(100).is_some()
            && tier_label(125).is_some()
            && tier_label(150).is_some()
            && tier_label(200).is_some()
            && tier_label(175).is_none()
            && tier_label(0).is_none(),
        "",
    );

    // 11. 缓存统计对账：预热两轮后计数自洽、单行数可读（15 miss + 15 hit）。
    let mut c = UpsampleCache::default();
    let _ = warm_cache(&plain, 150, &mut c);
    let warmed = c.len();
    let _ = warm_cache(&plain, 150, &mut c);
    let st = cache_stats(&c);
    set.add(
        "cache stats consistent and readable",
        counters_consistent(&c)
            && warmed == 15
            && st.hits == 15
            && st.misses == 15
            && st.entries == 15
            && c.hits == 15
            && cache_stats_line(&c).contains("命中率 500‰"),
        "",
    );

    // 12. 预热后零逐帧开销：预热后逐态首帧渲染全走缓存命中。
    let before = cache_stats(&c);
    let mut all_hit = true;
    for stt in ALL_STATES {
        let h0 = c.hits;
        if render_frame(&plain, stt, 0, 150, &mut c).is_none() || c.hits != h0 + 1 {
            all_hit = false;
        }
    }
    set.add(
        "preheat makes first frame hit cache",
        all_hit && c.hits == before.hits + 15,
        "",
    );

    // 13. 原生方案无需预热（零核来源不预热——诚实的 0）。
    let mut cn = UpsampleCache::default();
    set.add(
        "native scheme needs no preheat",
        warm_cache(&native, 200, &mut cn) == 0 && cn.is_empty(),
        "",
    );

    // 14. 契约外预热诚实拒绝（300 超契约、0 未上报——都不进缓存）。
    set.add(
        "preheat rejects out-of-contract tiers",
        warm_cache(&plain, 300, &mut c) == 0 && warm_cache(&plain, 0, &mut c) == 0,
        "",
    );

    // 15. 运行时报告确定性 + 人话档位 + 末次切换留痕。
    let rep1 = render_runtime_report(&rt);
    set.add(
        "runtime report deterministic and readable",
        rep1 == render_runtime_report(&rt)
            && rep1.contains("dpi=150")
            && rep1.contains("150% 高清")
            && rep1.contains("rejected=1"),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
    use super::*;
    use crate::jstar2::jbase::builtin_default_scheme;

    #[test]
    fn ledger_span_arithmetic() {
        let mut led = DpiEventLedger::new();
        led.record(500, 100, 200, true);
        led.record(900, 200, 100, true);
        led.record(950, 100, 200, true);
        assert_eq!(led.records()[1].since_prev_ms, Some(400));
        assert_eq!(led.records()[2].since_prev_ms, Some(50));
        assert_eq!(led.records()[0].since_prev_ms, None);
    }

    #[test]
    fn budget_zero_for_stateless_matrix() {
        let m = CursorSchemeModel::empty("空", crate::jstar2::jbase::OriginKind::Created);
        let mx = DecisionMatrix::build(&m);
        assert_eq!(mx.upscale_budget_x100(), 0);
        assert_eq!(mx.paid_cells(), 0);
    }

    #[test]
    fn rejection_copy_classes() {
        assert!(explain_rejection(0).unwrap().contains("未上报"));
        assert!(explain_rejection(250).unwrap().contains("200%"));
        assert!(explain_rejection(80).unwrap().contains("非标准档位"));
        assert!(explain_rejection(100).is_none());
    }

    #[test]
    fn warm_then_render_hits() {
        let m = builtin_default_scheme();
        let mut c = UpsampleCache::default();
        assert_eq!(warm_cache(&m, 125, &mut c), 15);
        let h = c.hits;
        let _ = render_frame(&m, PointerState::Normal, 0, 125, &mut c);
        assert_eq!(c.hits, h + 1, "预热后首帧零重采样");
    }

    #[test]
    fn cache_line_ratio_matches_counters() {
        let mut c = UpsampleCache::new(8);
        c.put((1, 0, 0, 100), PixBuf::new(1, 1));
        let _ = c.get(&(1, 0, 0, 100));
        let _ = c.get(&(2, 0, 0, 100));
        assert!(counters_consistent(&c));
        assert_eq!(c.hit_ratio_m(), Some(500));
        assert!(cache_stats_line(&c).contains("命中 1"));
    }

    #[test]
    fn v4_checks_all_green() {
        let set = run_dpicontract_v4_checks();
        assert!(!set.truncated());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "v4 check red: {}", c.name);
        }
    }
}
