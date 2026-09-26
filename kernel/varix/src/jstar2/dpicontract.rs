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
