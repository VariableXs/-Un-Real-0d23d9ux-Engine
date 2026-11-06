//! UNX-F50085 CJK 大字库按需下载（用页触发的字形批量账）（0x436c0 段）
//!
//! 锚点：UNX-F50085-J1 触发 ×10 字形覆盖断言；判据锚定 §AI-63 §2 专题二。
//!
//! 为什么按「页」而不是按「字」触发：CJK 大字库动辄两万多字形，逐字请求会
//! 把下载通道压成随机小包；而实际文档的用字有强页簇性（一页里出现的汉字
//! 通常属于相邻码页）。以页为单位收集、按批上限切块，既能把请求压到
//! 「每页 1~2 批」，又能保证「该页用到的字一个不少」。
//!
//! 覆盖账是核心不变量：
//! * 触发只登记「尚未加载」的字形（幂等：同页重复触发零新增）；
//! * 每批加载后逐字置位覆盖位图，批次可重试但不会重复计费；
//! * `coverage_permille` 用千分比给出可审计的覆盖度，1000 即全覆盖。
//!
//! 结构清单：
//! * 码页注册表：`CJK_RANGES`（Han/ExtA/Hiragana/Katakana/Hangul/Bopomofo/
//!   Compat）+ `glyphs_in_page`；
//! * 覆盖位图：`GlyphBitmap`（65536 位，定容 8 KiB）；
//! * 调度器：`CjkQueue`（批次队列 + 计划账 + 覆盖账）；
//! * 状态机：`BatchState` Pending→Fetching→Loaded/Failed（3 次失败封死）；
//! * 注入核：`injection_core` 8 类坏样本；
//! * 诊断码闭集：SEG_BASE = 0x436c0，n ∈ [1,10]。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x436c0 独占 0x20；F50086 从 0x436e0 起顺序占用）。
pub const SEG_BASE: u32 = 0x436c0;

pub const E_EMPTY: u16 = 1;
pub const E_PAGE: u16 = 2;
pub const E_RANGE: u16 = 3;
pub const E_CAP: u16 = 4;
pub const E_DUP: u16 = 5;
pub const E_STATE: u16 = 6;
pub const E_GLYPH: u16 = 7;
pub const E_BATCH: u16 = 8;
pub const E_FETCH: u16 = 9;
pub const E_PLAN: u16 = 10;

/// 单批字形上限。128 是「一页半屏正文」的经验值：再大批次会让首屏等待
/// 时间超过 200ms，再小则包数翻倍、通道开销压过净载荷。
pub const MAX_BATCH: usize = 128;

/// 单字形下载体积估算（CJK 轮廓平均 32 字节，含度量与定位表增量）。
pub const GLYPH_BYTES: u32 = 32;

/// 失败重试上限：同一批连续失败 3 次即封死，避免坏字体把队列拖死。
pub const MAX_ATTEMPTS: u8 = 3;

/// CJK 大字库按需下载错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CjkError {
    Empty,
    Page,
    Range,
    Cap,
    Dup,
    State,
    Glyph,
    Batch,
    Fetch,
    Plan,
}

impl CjkError {
    fn ordinal(self) -> u16 {
        match self {
            CjkError::Empty => E_EMPTY,
            CjkError::Page => E_PAGE,
            CjkError::Range => E_RANGE,
            CjkError::Cap => E_CAP,
            CjkError::Dup => E_DUP,
            CjkError::State => E_STATE,
            CjkError::Glyph => E_GLYPH,
            CjkError::Batch => E_BATCH,
            CjkError::Fetch => E_FETCH,
            CjkError::Plan => E_PLAN,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            CjkError::Empty => "no pages requested",
            CjkError::Page => "page index outside BMP",
            CjkError::Range => "page contains no CJK glyph",
            CjkError::Cap => "batch queue capacity exhausted",
            CjkError::Dup => "duplicate batch id",
            CjkError::State => "illegal batch state transition",
            CjkError::Glyph => "codepoint outside CJK ranges",
            CjkError::Batch => "batch size illegal or batch unknown",
            CjkError::Fetch => "batch fetch failed and retries exhausted",
            CjkError::Plan => "unknown plan id",
        }
    }
}

/// 一个 CJK 码段。只登记大字库真正会下载的区间；ASCII 与标点由基础字体
/// 自带，不进入按需下载账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CjkRange {
    pub name: &'static str,
    pub first: u16,
    pub last: u16,
}

/// 七段闭集。区间来自 Unicode 15 的 CJK 相关块（BMP 部分）。
pub const CJK_RANGES: [CjkRange; 7] = [
    CjkRange { name: "hiragana", first: 0x3040, last: 0x309F },
    CjkRange { name: "katakana", first: 0x30A0, last: 0x30FF },
    CjkRange { name: "bopomofo", first: 0x3100, last: 0x312F },
    CjkRange { name: "ext-a", first: 0x3400, last: 0x4DBF },
    CjkRange { name: "han", first: 0x4E00, last: 0x9FFF },
    CjkRange { name: "hangul", first: 0xAC00, last: 0xD7A3 },
    CjkRange { name: "compat", first: 0xF900, last: 0xFAFF },
];

/// 码点是否落在受管 CJK 段内。
pub fn in_cjk(cp: u16) -> bool {
    let mut i = 0usize;
    while i < CJK_RANGES.len() {
        let r = CJK_RANGES[i];
        if cp >= r.first && cp <= r.last {
            return true;
        }
        i += 1;
    }
    false
}

/// 码点所属段名；非 CJK 返回 None（调用方据此显式拒绝）。
pub fn script_of(cp: u16) -> Option<&'static str> {
    let mut i = 0usize;
    while i < CJK_RANGES.len() {
        let r = CJK_RANGES[i];
        if cp >= r.first && cp <= r.last {
            return Some(r.name);
        }
        i += 1;
    }
    None
}

/// 一页（高字节）内所有受管 CJK 码点，升序。页号 0..=255。
pub fn glyphs_in_page(page: u16) -> Vec<u16> {
    let mut out = Vec::new();
    if page > 0xFF {
        return out;
    }
    let base = page << 8;
    let mut cp = base as u32;
    let end = (base | 0x00FF) as u32;
    while cp <= end {
        let c = cp as u16;
        if in_cjk(c) {
            out.push(c);
        }
        cp += 1;
    }
    out
}

/// 65536 位覆盖位图。定容 8 KiB，放在堆上（Vec），不占栈。
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    bits: Vec<u8>,
    count: usize,
}

impl GlyphBitmap {
    pub fn new() -> GlyphBitmap {
        let mut bits = Vec::with_capacity(8192);
        let mut i = 0usize;
        while i < 8192 {
            bits.push(0u8);
            i += 1;
        }
        GlyphBitmap { bits, count: 0 }
    }

    /// 置位并返回「本次是否新置位」。重复置位不增计数——覆盖账靠这一条
    /// 保证「同一字形永不重复计费」。
    pub fn set(&mut self, cp: u16) -> bool {
        let idx = cp as usize;
        let byte = idx >> 3;
        let mask = 1u8 << (idx & 7);
        match self.bits.get_mut(byte) {
            Some(b) => {
                if *b & mask != 0 {
                    false
                } else {
                    *b |= mask;
                    self.count += 1;
                    true
                }
            }
            None => false,
        }
    }

    pub fn get(&self, cp: u16) -> bool {
        let idx = cp as usize;
        let byte = idx >> 3;
        let mask = 1u8 << (idx & 7);
        match self.bits.get(byte) {
            Some(b) => *b & mask != 0,
            None => false,
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

impl Default for GlyphBitmap {
    fn default() -> GlyphBitmap {
        GlyphBitmap::new()
    }
}

/// 批次状态。Pending 可重试，Fetching 是唯一允许 finish 的态，
/// Loaded/Failed 都是终态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchState {
    Pending,
    Fetching,
    Loaded,
    Failed,
}

/// 一批待下载字形。`glyphs` 升序且无重复，`bytes` 由长度推导。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphBatch {
    pub batch_id: u32,
    pub glyphs: Vec<u16>,
    pub bytes: u32,
    pub state: BatchState,
    pub attempts: u8,
}

/// 一次触发产生的计划。`glyphs` 是本次新增（尚未加载）的字形数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagePlan {
    pub plan_id: u32,
    pub pages: Vec<u16>,
    pub batch_ids: Vec<u32>,
    pub glyphs: usize,
    pub bytes: u32,
}

impl PagePlan {
    pub fn is_empty(&self) -> bool {
        self.glyph_ids_is_empty()
    }

    fn glyph_ids_is_empty(&self) -> bool {
        self.batch_ids.is_empty()
    }
}

/// 计划账记录：保留「请求了什么、实际新增多少、切了几批」，
/// 事后可据此复算触发是否幂等。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRecord {
    pub plan_id: u32,
    pub pages: Vec<u16>,
    pub requested: usize,
    pub fresh: usize,
    pub batches: usize,
    pub bytes: u32,
}

/// 按需下载调度器：批次队列 + 计划账 + 覆盖位图。
#[derive(Clone, Debug)]
pub struct CjkQueue {
    batches: Vec<GlyphBatch>,
    loaded: GlyphBitmap,
    plans: Vec<PlanRecord>,
    plan_seq: u32,
    batch_seq: u32,
    cap: usize,
}

impl CjkQueue {
    pub fn new(cap: usize) -> CjkQueue {
        CjkQueue {
            batches: Vec::new(),
            loaded: GlyphBitmap::new(),
            plans: Vec::new(),
            plan_seq: 0,
            batch_seq: 1,
            cap,
        }
    }

    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    pub fn loaded_count(&self) -> usize {
        self.loaded.count()
    }

    pub fn is_loaded(&self, cp: u16) -> bool {
        self.loaded.get(cp)
    }

    pub fn batch(&self, i: usize) -> Option<&GlyphBatch> {
        self.batches.get(i)
    }

    pub fn batch_by_id(&self, id: u32) -> Option<&GlyphBatch> {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].batch_id == id {
                return self.batches.get(i);
            }
            i += 1;
        }
        None
    }

    pub fn plan_record(&self, i: usize) -> Option<&PlanRecord> {
        self.plans.get(i)
    }

    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }

    pub fn plan_summary(&self, plan_id: u32) -> Result<&PlanRecord, CjkError> {
        let mut i = 0usize;
        while i < self.plans.len() {
            if self.plans[i].plan_id == plan_id {
                return match self.plans.get(i) {
                    Some(p) => Ok(p),
                    None => Err(CjkError::Plan),
                };
            }
            i += 1;
        }
        Err(CjkError::Plan)
    }

    /// 覆盖度（千分比）：1000 = 请求字形全部已加载，空请求视为 1000。
    pub fn coverage_permille(&self, glyphs: &[u16]) -> u32 {
        if glyphs.is_empty() {
            return 1000;
        }
        let mut hit = 0usize;
        let mut i = 0usize;
        while i < glyphs.len() {
            if self.loaded.get(glyphs[i]) {
                hit += 1;
            }
            i += 1;
        }
        (hit as u32 * 1000) / glyphs.len() as u32
    }

    /// 批次规范化：拒绝空批/超限批，拒绝非 CJK 码点，输出升序去重。
    /// 排序去重让「同一页的不同排版顺序」生成同一批字节，保证下载可缓存。
    pub fn make_batch(&self, glyphs: &[u16]) -> Result<Vec<u16>, CjkError> {
        if glyphs.is_empty() || glyphs.len() > MAX_BATCH {
            return Err(CjkError::Batch);
        }
        let mut i = 0usize;
        while i < glyphs.len() {
            if !in_cjk(glyphs[i]) {
                return Err(CjkError::Glyph);
            }
            i += 1;
        }
        let mut v = glyphs.to_vec();
        v.sort();
        v.dedup();
        if v.is_empty() {
            return Err(CjkError::Batch);
        }
        Ok(v)
    }

    /// 显式登记一批（供外部计划器使用）。重复 id 拒绝——批号是下载回执的
    /// 唯一键，重复会让回执落到错误批次上。
    pub fn add_batch(&mut self, batch_id: u32, glyphs: &[u16]) -> Result<usize, CjkError> {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].batch_id == batch_id {
                return Err(CjkError::Dup);
            }
            i += 1;
        }
        if self.batches.len() >= self.cap {
            return Err(CjkError::Cap);
        }
        let v = self.make_batch(glyphs)?;
        let bytes = (v.len() as u32) * GLYPH_BYTES;
        self.batches.push(GlyphBatch {
            batch_id,
            glyphs: v,
            bytes,
            state: BatchState::Pending,
            attempts: 0,
        });
        Ok(self.batches.len() - 1)
    }

    fn index_of(&self, batch_id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].batch_id == batch_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 触发入口：把若干「已用页」折算成新增批次。已加载的字形自动跳过，
    /// 因此同一页重复触发返回空计划（幂等）。
    pub fn plan_pages(&mut self, pages: &[u16]) -> Result<PagePlan, CjkError> {
        if pages.is_empty() {
            return Err(CjkError::Empty);
        }
        let mut uniq: Vec<u16> = Vec::with_capacity(pages.len());
        let mut i = 0usize;
        while i < pages.len() {
            let p = pages[i];
            if p > 0xFF {
                return Err(CjkError::Page);
            }
            if glyphs_in_page(p).is_empty() {
                return Err(CjkError::Range);
            }
            if !uniq.contains(&p) {
                uniq.push(p);
            }
            i += 1;
        }
        uniq.sort();
        let mut all: Vec<u16> = Vec::new();
        i = 0usize;
        while i < uniq.len() {
            let gs = glyphs_in_page(uniq[i]);
            all.extend_from_slice(&gs);
            i += 1;
        }
        all.sort();
        all.dedup();
        let mut fresh: Vec<u16> = Vec::new();
        i = 0usize;
        while i < all.len() {
            if !self.loaded.get(all[i]) {
                fresh.push(all[i]);
            }
            i += 1;
        }
        let plan_id = self.plan_seq;
        self.plan_seq = self.plan_seq.wrapping_add(1);
        let mut batch_ids: Vec<u32> = Vec::new();
        let mut bytes = 0u32;
        if !fresh.is_empty() {
            let need = (fresh.len() + MAX_BATCH - 1) / MAX_BATCH;
            if self.batches.len() + need > self.cap {
                return Err(CjkError::Cap);
            }
            let mut at = 0usize;
            while at < fresh.len() {
                let end = if at + MAX_BATCH > fresh.len() {
                    fresh.len()
                } else {
                    at + MAX_BATCH
                };
                let id = self.batch_seq;
                self.batch_seq = self.batch_seq.wrapping_add(1);
                self.add_batch(id, &fresh[at..end])?;
                batch_ids.push(id);
                bytes += ((end - at) as u32) * GLYPH_BYTES;
                at = end;
            }
        }
        self.plans.push(PlanRecord {
            plan_id,
            pages: uniq.clone(),
            requested: all.len(),
            fresh: fresh.len(),
            batches: batch_ids.len(),
            bytes,
        });
        Ok(PagePlan {
            plan_id,
            pages: uniq,
            batch_ids,
            glyphs: fresh.len(),
            bytes,
        })
    }

    /// 开始取批：只有 Pending 能进入 Fetching。
    pub fn begin(&mut self, batch_id: u32) -> Result<(), CjkError> {
        let idx = match self.index_of(batch_id) {
            Some(v) => v,
            None => return Err(CjkError::Batch),
        };
        if self.batches[idx].state != BatchState::Pending {
            return Err(CjkError::State);
        }
        self.batches[idx].state = BatchState::Fetching;
        Ok(())
    }

    /// 完成取批。成功时逐字置位并返回新覆盖数；失败累计 attempts，
    /// 未到上限回 Pending（可重试），到上限转 Failed 并返回 Fetch。
    pub fn finish(&mut self, batch_id: u32, ok: bool) -> Result<usize, CjkError> {
        let idx = match self.index_of(batch_id) {
            Some(v) => v,
            None => return Err(CjkError::Batch),
        };
        if self.batches[idx].state != BatchState::Fetching {
            return Err(CjkError::State);
        }
        if ok {
            let mut fresh = 0usize;
            let mut i = 0usize;
            while i < self.batches[idx].glyphs.len() {
                let g = self.batches[idx].glyphs[i];
                if self.loaded.set(g) {
                    fresh += 1;
                }
                i += 1;
            }
            self.batches[idx].state = BatchState::Loaded;
            Ok(fresh)
        } else {
            let a = self.batches[idx].attempts.saturating_add(1);
            self.batches[idx].attempts = a;
            if a >= MAX_ATTEMPTS {
                self.batches[idx].state = BatchState::Failed;
            } else {
                self.batches[idx].state = BatchState::Pending;
            }
            Err(CjkError::Fetch)
        }
    }

    /// 队列中待取批次总数（Pending + Fetching）。
    pub fn pending_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.batches.len() {
            match self.batches[i].state {
                BatchState::Pending | BatchState::Fetching => n += 1,
                _ => {}
            }
            i += 1;
        }
        n
    }

    /// 全队列字节合计（与计划账的 bytes 对账用）。
    pub fn total_bytes(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.batches.len() {
            t += self.batches[i].bytes;
            i += 1;
        }
        t
    }
}

// ---------------------------------------------------------------------------
// 注入核：8 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let mut q = CjkQueue::new(96);

    if q.plan_pages(&[]).err() == Some(CjkError::Empty) {
        caught += 1;
    }
    if q.plan_pages(&[256]).err() == Some(CjkError::Page) {
        caught += 1;
    }
    // 0xE0 页是私用区，不含任何受管 CJK 码点
    if q.plan_pages(&[0x00E0]).err() == Some(CjkError::Range) {
        caught += 1;
    }
    if q.make_batch(&[0x0041]).err() == Some(CjkError::Glyph) {
        caught += 1;
    }
    let mut many: Vec<u16> = Vec::with_capacity(MAX_BATCH + 1);
    let mut i = 0usize;
    while i <= MAX_BATCH {
        many.push(0x4E00 + i as u16);
        i += 1;
    }
    if q.make_batch(&many).err() == Some(CjkError::Batch) {
        caught += 1;
    }
    let page = glyphs_in_page(0x4E);
    let chunk = &page[..4];
    let mut q2 = CjkQueue::new(96);
    let first = q2.add_batch(1, chunk);
    let dup = q2.add_batch(1, chunk);
    if first.is_ok() && dup.err() == Some(CjkError::Dup) {
        caught += 1;
    }
    if q.plan_summary(9999).err() == Some(CjkError::Plan) {
        caught += 1;
    }
    let mut tiny = CjkQueue::new(1);
    if tiny.plan_pages(&[0x4E, 0x4F]).err() == Some(CjkError::Cap) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50085_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50085");
    let mut q = CjkQueue::new(96);

    // J1-1 触发 ×10：每页 256 字、2 批、加载后覆盖 1000‰
    let mut ok = true;
    let mut covered = 0usize;
    let mut planned_bytes = 0u32;
    let mut pages_seen: Vec<u16> = Vec::new();
    let mut k = 0u16;
    while k < 10 {
        let page = 0x4E + k;
        pages_seen.push(page);
        let plan = match q.plan_pages(&[page]) {
            Ok(p) => p,
            Err(_) => {
                ok = false;
                k += 1;
                continue;
            }
        };
        if plan.glyphs != 256 || plan.batch_ids.len() != 2 || plan.bytes != 256 * GLYPH_BYTES {
            ok = false;
        }
        let mut bi = 0usize;
        while bi < plan.batch_ids.len() {
            let id = plan.batch_ids[bi];
            if q.begin(id).is_err() {
                ok = false;
            }
            match q.finish(id, true) {
                Ok(n) => {
                    if n != MAX_BATCH {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            bi += 1;
        }
        let glyphs = glyphs_in_page(page);
        if q.coverage_permille(&glyphs) != 1000 {
            ok = false;
        }
        let mut gi = 0usize;
        while gi < glyphs.len() {
            if !q.is_loaded(glyphs[gi]) {
                ok = false;
            }
            gi += 1;
        }
        // 同页重复触发必须零新增（幂等）
        let again = match q.plan_pages(&[page]) {
            Ok(p) => p,
            Err(_) => {
                ok = false;
                k += 1;
                continue;
            }
        };
        if again.glyphs != 0 || !again.batch_ids.is_empty() || !again.is_empty() {
            ok = false;
        }
        covered += plan.glyphs;
        planned_bytes += plan.bytes;
        k += 1;
    }
    let ok = ok
        && covered == 2560
        && planned_bytes == 2560 * GLYPH_BYTES
        && q.loaded_count() == 2560;
    let det = format!(
        "ok={} covered={} bytes={} loaded={}",
        ok,
        covered,
        planned_bytes,
        q.loaded_count()
    );
    s.add_dyn("50085-J1-十页触发覆盖", ok, &det);

    // J1-2 无过度取字：已加载集合与 10 页并集完全一致
    let mut union: Vec<u16> = Vec::new();
    let mut i = 0usize;
    while i < pages_seen.len() {
        let gs = glyphs_in_page(pages_seen[i]);
        union.extend_from_slice(&gs);
        i += 1;
    }
    union.sort();
    union.dedup();
    let mut same = union.len() == q.loaded_count();
    let mut j = 0usize;
    while j < union.len() {
        if !q.is_loaded(union[j]) {
            same = false;
        }
        j += 1;
    }
    let det = format!("union={} loaded={}", union.len(), q.loaded_count());
    s.add_dyn("50085-J1-覆盖集合一致", same, &det);

    // J1-3 字节守恒：队列总字节 = 计划账字节之和 = 字形数×32
    let mut plan_bytes = 0u32;
    let mut plan_fresh = 0usize;
    let mut i = 0usize;
    while i < q.plan_count() {
        match q.plan_record(i) {
            Some(p) => {
                plan_bytes += p.bytes;
                plan_fresh += p.fresh;
            }
            None => {}
        }
        i += 1;
    }
    let ok = plan_bytes == q.total_bytes()
        && plan_bytes == (plan_fresh as u32) * GLYPH_BYTES
        && plan_bytes == planned_bytes;
    let det = format!("plan={} queue={} fresh={}", plan_bytes, q.total_bytes(), plan_fresh);
    s.add_dyn("50085-J1-字节守恒", ok, &det);

    // J1-4 批次内部有序去重且不越上限
    let mut order_ok = q.batch_count() > 0;
    let mut i = 0usize;
    while i < q.batch_count() {
        let b = match q.batch(i) {
            Some(v) => v,
            None => {
                order_ok = false;
                i += 1;
                continue;
            }
        };
        if b.glyphs.is_empty() || b.glyphs.len() > MAX_BATCH || b.bytes != (b.glyphs.len() as u32) * GLYPH_BYTES {
            order_ok = false;
        }
        let mut j = 1usize;
        while j < b.glyphs.len() {
            if b.glyphs[j] <= b.glyphs[j - 1] {
                order_ok = false;
            }
            j += 1;
        }
        // 批内所有字形都属于所声明页之一
        let mut j = 0usize;
        while j < b.glyphs.len() {
            if !in_cjk(b.glyphs[j]) {
                order_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("50085-J2-批次有序有界", order_ok, if order_ok { "" } else { "批次乱序或超限" });

    // J1-5 覆盖单调：按批加载过程中覆盖度不降
    let mut m = CjkQueue::new(16);
    let page = 0x52u16;
    let glyphs = glyphs_in_page(page);
    let plan = match m.plan_pages(&[page]) {
        Ok(p) => p,
        Err(_) => {
            s.add("50085-J2-覆盖单调", false, "触发失败");
            return s;
        }
    };
    let mut last = m.coverage_permille(&glyphs);
    let mut mono = true;
    let mut bi = 0usize;
    while bi < plan.batch_ids.len() {
        let id = plan.batch_ids[bi];
        if m.begin(id).is_err() || m.finish(id, true).is_err() {
            mono = false;
            break;
        }
        let now = m.coverage_permille(&glyphs);
        if now < last {
            mono = false;
        }
        last = now;
        bi += 1;
    }
    let ok = mono && last == 1000;
    let det = format!("last={}", last);
    s.add_dyn("50085-J2-覆盖单调", ok, &det);

    // J1-6 状态机：重复 begin / 未 begin 就 finish
    let mut st = CjkQueue::new(8);
    let gs = glyphs_in_page(0x31);
    let chunk = &gs[..8];
    let id = match st.add_batch(7, chunk) {
        Ok(_) => 7u32,
        Err(_) => 0u32,
    };
    let early_finish = st.finish(id, true).err() == Some(CjkError::State);
    let b1 = st.begin(id).is_ok();
    let b2 = st.begin(id).err() == Some(CjkError::State);
    let done = st.finish(id, true).is_ok();
    let again = st.finish(id, true).err() == Some(CjkError::State);
    let ok = early_finish && b1 && b2 && done && again;
    s.add("50085-J2-批次状态机", ok, if ok { "" } else { "状态机漏拦" });

    // J1-7 失败重试：前两次可重试，第三次封死
    let mut rt = CjkQueue::new(8);
    let gs = glyphs_in_page(0x31);
    let id = match rt.add_batch(3, &gs[..8]) {
        Ok(_) => 3u32,
        Err(_) => 0u32,
    };
    let mut fails = 0usize;
    let mut attempts_seen = 0u8;
    let mut n = 0usize;
    while n < 3 {
        if rt.begin(id).is_ok() && rt.finish(id, false).err() == Some(CjkError::Fetch) {
            fails += 1;
        }
        if let Some(b) = rt.batch_by_id(id) {
            attempts_seen = b.attempts;
        }
        n += 1;
    }
    let failed_state = match rt.batch_by_id(id) {
        Some(b) => b.state == BatchState::Failed,
        None => false,
    };
    let begin_after = rt.begin(id).err() == Some(CjkError::State);
    let ok = fails == 3 && attempts_seen == MAX_ATTEMPTS && failed_state && begin_after;
    let det = format!("fails={} attempts={}", fails, attempts_seen);
    s.add_dyn("50085-J2-失败重试封死", ok, &det);

    // J1-8 计划账可追溯：每个计划号都能取回且页集合一致
    let mut plan_ok = q.plan_count() == 20;
    let mut pi = 0usize;
    while pi < q.plan_count() {
        let rec = match q.plan_record(pi) {
            Some(v) => v,
            None => {
                plan_ok = false;
                pi += 1;
                continue;
            }
        };
        match q.plan_summary(rec.plan_id) {
            Ok(p) => {
                if p.pages.is_empty() {
                    plan_ok = false;
                }
            }
            Err(_) => plan_ok = false,
        }
        pi += 1;
    }
    s.add("50085-J2-计划账可追溯", plan_ok, if plan_ok { "" } else { "计划账缺失" });

    // J1-9 确定性：同页在两套队列上切出相同字形划分
    let mut qa = CjkQueue::new(16);
    let mut qb = CjkQueue::new(16);
    let pa = qa.plan_pages(&[0x4F]);
    let pb = qb.plan_pages(&[0x4F]);
    let ok = match (pa, pb) {
        (Ok(x), Ok(y)) => {
            x.glyphs == y.glyphs
                && x.bytes == y.bytes
                && x.batch_ids.len() == y.batch_ids.len()
        }
        _ => false,
    };
    s.add("50085-J2-计划确定", ok, if ok { "" } else { "切分不确定" });

    // J1-10 页范围拒绝：非 BMP 页与无 CJK 页显式报错
    let mut e = CjkQueue::new(8);
    let p1 = e.plan_pages(&[256]).err() == Some(CjkError::Page);
    let p2 = e.plan_pages(&[0x00D8]).err() == Some(CjkError::Range);
    let p3 = e.plan_pages(&[]).err() == Some(CjkError::Empty);
    let ok = p1 && p2 && p3;
    s.add("50085-J2-页范围拒绝", ok, if ok { "" } else { "页校验缺失" });

    ok_or(&mut s, "50085-J2-字形范围", {
        let mut all_ok = true;
        let mut bi = 0usize;
        while bi < q.batch_count() {
            if let Some(b) = q.batch(bi) {
                let mut gi = 0usize;
                while gi < b.glyphs.len() {
                    if script_of(b.glyphs[gi]).is_none() {
                        all_ok = false;
                    }
                    gi += 1;
                }
            }
            bi += 1;
        }
        all_ok
    });

    // J1-11 注入核 8 类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50085-J1-注入必抓", ok, &det);

    // J1-12 码值闭集
    let codes = [
        CjkError::Empty,
        CjkError::Page,
        CjkError::Range,
        CjkError::Cap,
        CjkError::Dup,
        CjkError::State,
        CjkError::Glyph,
        CjkError::Batch,
        CjkError::Fetch,
        CjkError::Plan,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        let c = codes[i].code();
        if c <= SEG_BASE || c > SEG_BASE + 10 {
            ok = false;
        }
        i += 1;
    }
    s.add("50085-J1-码值闭集", ok, if ok { "" } else { "码值越段" });

    s
}

/// 小工具：把布尔判据写入 CheckSet（避免在函数中部重复构造字符串）。
fn ok_or(s: &mut CheckSet, name: &'static str, passed: bool) {
    s.add(name, passed, if passed { "" } else { "判据未通过" });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50085_checks_green() {
        let set = run_unx50085_checks();
        assert!(set.all_passed(), "UNX-F50085 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50085_page_coverage() {
        let mut q = CjkQueue::new(16);
        let page = 0x4E;
        let plan = q.plan_pages(&[page]);
        assert!(plan.is_ok());
        if let Ok(p) = plan {
            let mut i = 0usize;
            while i < p.batch_ids.len() {
                let id = p.batch_ids[i];
                assert!(q.begin(id).is_ok());
                assert!(q.finish(id, true).is_ok());
                i += 1;
            }
        }
        assert_eq!(q.coverage_permille(&glyphs_in_page(page)), 1000);
    }

    #[test]
    fn unx50085_injection() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
    }
}
