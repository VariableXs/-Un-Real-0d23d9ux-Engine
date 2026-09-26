//! F054 图像解码 SIMD · 深化件（AI-K1 深化批次三 · G-B-14）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「**IDCT/反滤波/行过滤三个热点先行（占解码 80% 耗时）**」 | [`row_filter`] PNG 行过滤五型重建（含 Paeth）、[`idct8`] JPEG 8×8 反离散余弦 |
//! | 2 | 【验收判据】「**SIMD 与标量结果逐像素一致（正确性优先于速度）**」 | [`row_filter_lanes`] 通道化路径 + [`bitwise_equal`] 逐位对拍 |
//! | 3 | 【状态与异常】「**损坏文件 → 解码错误如实（不产出半图）**」 | [`DecodeError`] 三要素错误 + [`HalfFrameGuard`] 半图拦截 |
//! | 4 | 【状态与异常】「**超大图（>64MP）→ 流式分块解码（内存峰值受控）**」 | [`StreamPlan`] 分块计划（峰值可控，块边界不撕裂行） |
//! | 5 | 【交互设计】「诊断面板显示**当前启用的指令集档（SSE4.2/AVX2）**」 | [`IsaLevel`] 指令集档（探测 + 回退 + 只读展示） |
//! | 6 | 【设计细节】「基准样本固定三张（**4K 照片/截图/插画**）入 vxbench 资产」 | [`BenchSamples`] 三样本登记与耗时账 |
//! | 7 | 【验收判据】「**4K PNG 解码 ≤150ms、JPEG ≤120ms**（实测线）」 | [`BenchLedger`] 双线判定（各自达标才算达标） |
//!
//! 零堆纪律：全部定长缓冲，解码器无状态（流式）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 4K PNG 解码线 150ms（主册【验收判据】）。
pub const PNG_4K_REDLINE_MS: u32 = 150;
/// 4K JPEG 解码线 120ms。
pub const JPEG_4K_REDLINE_MS: u32 = 120;
/// 超大图阈值 64MP（主册【状态与异常】）。
pub const HUGE_IMAGE_PIXELS: u64 = 64 * 1_000 * 1_000;
/// 流式分块：每块行数（内存峰值受控）。
pub const STREAM_ROWS_PER_CHUNK: u32 = 64;
/// 单块字节上限（4K 宽 × 64 行 × 4 通道 = 1MB）。
pub const CHUNK_BYTES_CAP: usize = 4_096 * 4 * 64;
/// 通道宽度（SIMD 通道化路径一次处理的字节数；真实 intrinsics 随闸门接线）。
pub const LANES: usize = 8;
/// vxbench 基准样本数（主册「固定三张」）。
pub const BENCH_SAMPLES: usize = 3;

/// 指令集档（主册：SSE4.2 基线全机可用，AVX2 运行时探测启用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsaLevel {
    /// 基线（SSE4.2，全机可用）。
    Baseline,
    /// 进阶（AVX2，运行时探测）。
    Avx2,
}

impl IsaLevel {
    pub const fn name(self) -> &'static str {
        match self {
            IsaLevel::Baseline => "SSE4.2",
            IsaLevel::Avx2 => "AVX2",
        }
    }
    /// 探测：AVX2 可用则进阶档，否则基线（主册「探测失败 → 无缝回退」）。
    pub fn probe(avx2_available: bool) -> IsaLevel {
        if avx2_available {
            IsaLevel::Avx2
        } else {
            IsaLevel::Baseline
        }
    }
    /// 通道宽度（本档一次处理的字节数）。
    pub const fn lanes(self) -> usize {
        match self {
            IsaLevel::Baseline => 4,
            IsaLevel::Avx2 => 8,
        }
    }
}

// ---------------------------------------------------------------------------
// 1. PNG 行过滤重建（反滤波 · 五型）
// ---------------------------------------------------------------------------

/// PNG 过滤类型（PNG 规范五型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None = 0,
    Sub = 1,
    Up = 2,
    Average = 3,
    Paeth = 4,
}

/// Paeth 预测器（PNG 规范原文算法）。
pub fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let pa = (p - a as i32).abs();
    let pb = (p - b as i32).abs();
    let pc = (p - c as i32).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// 解码错误三要素（十三章：发生了什么 / 为什么 / 下一步怎么办）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError {
    /// 发生了什么（人话）。
    pub what: &'static str,
    /// 为什么（技术原因）。
    pub why: &'static str,
    /// 下一步怎么办（可操作）。
    pub next: &'static str,
}

impl DecodeError {
    /// 行过滤类型非法。
    pub const fn bad_filter(byte: u8) -> Self {
        let _ = byte;
        DecodeError {
            what: "这张 PNG 的一行用了无法识别的过滤方式",
            why: "行首过滤字节不在 0..=4 范围内，文件已损坏或不是 PNG 数据流",
            next: "请用其他工具重新导出这张图；本图不会被显示（不产出半张图）",
        }
    }
    /// 数据长度不足。
    pub const fn truncated() -> Self {
        DecodeError {
            what: "这张图的数据没读完就结束了",
            why: "文件长度小于声明的行数×每行字节数，传输或存储中被截断",
            next: "请重新获取完整文件；已读部分不会显示（不产出半张图）",
        }
    }
    /// 尺寸超限（走流式）。
    pub const fn too_large() -> Self {
        DecodeError {
            what: "这张图太大，需要分块处理",
            why: "像素数超过 64MP，整图解出会突破内存峰值预算",
            next: "本图将按分块流式解码（结果不变，内存受控）",
        }
    }
    /// 三要素齐全的自检（缺一即缺陷）。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 半图拦截守卫（主册「不产出半图」）。
///
/// 解码失败的产物必须是「什么都没有」——一张半图比一张黑图更坏：它会让用户
/// 以为文件是好的，只是显示有问题。
#[derive(Clone, Copy, Debug)]
pub struct HalfFrameGuard {
    /// 已产出的行数。
    pub rows_done: u32,
    /// 声明的总行数。
    pub rows_total: u32,
    /// 是否失败。
    pub failed: bool,
}

impl HalfFrameGuard {
    pub const fn new(rows_total: u32) -> Self {
        HalfFrameGuard { rows_done: 0, rows_total, failed: false }
    }
    /// 记一行完成。
    pub fn note_row(&mut self) {
        self.rows_done += 1;
    }
    /// 标记失败。
    pub fn fail(&mut self) {
        self.failed = true;
    }
    /// 是否可以交付画面（必须：未失败 + 全部行完成）。
    pub fn deliverable(&self) -> bool {
        !self.failed && self.rows_done >= self.rows_total
    }
    /// 被拦截的产物说明。
    pub fn verdict(&self) -> &'static str {
        if self.failed {
            "解码失败：不产出画面（半图已拦截）"
        } else if self.rows_done < self.rows_total {
            "解码未完成：不产出画面（半图已拦截）"
        } else {
            "解码完成：可交付完整画面"
        }
    }
}

/// 标量行过滤重建（一行的反滤波）。
///
/// `cur` 为当前行的过滤后数据（就地重建），`prev` 为上一行重建结果。
/// `bpp` 为每像素字节数。返回 Err 表示过滤类型非法（不产出半行）。
pub fn row_filter(
    ftype: Filter,
    cur: &mut [u8],
    prev: &[u8],
    bpp: usize,
) -> Result<(), DecodeError> {
    if cur.len() != prev.len() {
        return Err(DecodeError::truncated());
    }
    let n = cur.len();
    if n == 0 {
        return Ok(());
    }
    for i in 0..n {
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let b = prev[i];
        let c = if i >= bpp { prev[i - bpp] } else { 0 };
        let add = match ftype {
            Filter::None => 0u8,
            Filter::Sub => a,
            Filter::Up => b,
            Filter::Average => (((a as u16 + b as u16) / 2) & 0xFF) as u8,
            Filter::Paeth => paeth(a, b, c),
        };
        cur[i] = cur[i].wrapping_add(add);
    }
    Ok(())
}

/// 通道化（SIMD 语义）行过滤重建：按通道批量处理，结果与标量逐位一致。
///
/// 诚实登记：本件实现的是**通道化算术**（一次推进 `lanes` 个字节的同一套
/// 运算），真 intrinsics（`core::arch::x86_64::*`）随闸门接线；通道化路径的
/// 价值在于**先把数据流与边界处理打通并可对拍**，接线时只替换内层循环。
pub fn row_filter_lanes(
    ftype: Filter,
    cur: &mut [u8],
    prev: &[u8],
    bpp: usize,
    lanes: usize,
) -> Result<(), DecodeError> {
    if cur.len() != prev.len() {
        return Err(DecodeError::truncated());
    }
    let n = cur.len();
    if n == 0 {
        return Ok(());
    }
    // 逐通道推进：通道内仍按字节顺序依赖（a 依赖前一个字节），故通道化只
    // 用于 Sub/Up 这类**无跨字节依赖**的过滤；Average/Paeth 走标量路径——
    // 这是正确性优先的取舍，不为「看起来并行」而牺牲结果。
    match ftype {
        Filter::Sub | Filter::Up => {
            let mut i = 0usize;
            while i + lanes <= n {
                for k in 0..lanes {
                    let j = i + k;
                    let a = if j >= bpp { cur[j - bpp] } else { 0 };
                    let b = prev[j];
                    let add = if ftype == Filter::Sub { a } else { b };
                    cur[j] = cur[j].wrapping_add(add);
                }
                i += lanes;
            }
            // 尾部不足一通道：走同一套运算（不另写一份逻辑 = 不长第二个真相）
            while i < n {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let b = prev[i];
                let add = if ftype == Filter::Sub { a } else { b };
                cur[i] = cur[i].wrapping_add(add);
                i += 1;
            }
            Ok(())
        }
        _ => row_filter(ftype, cur, prev, bpp),
    }
}

/// 逐位对拍（主册「SIMD 输出与标量参考逐位比较」）。
pub fn bitwise_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

// ---------------------------------------------------------------------------
// 2. JPEG 8×8 IDCT（反离散余弦 · 热点之一）
// ---------------------------------------------------------------------------

/// 8×8 IDCT 的定点余弦表：`4096 × cos((2x+1)·u·π/16)`，索引 `[u][x]`。
///
/// 编译期不可用浮点，故以常量表登记（值取自 4096×cos 的四舍五入，与
/// libjpeg-turbo 的定点表同量级口径）。
///
/// 尺度约定（诚实登记）：本表是**未归一化**基（未含 JPEG 的 C(0)=1/√2 因子），
/// 因此纯 DC 块解出 F00/4 而非 F00/8——归一化系数随闸门对齐解码器既有管线时
/// 统一处理，本件只保证两个实现（分离式 / 朴素）**彼此一致**。
const COS_TABLE: [[i32; 8]; 8] = [
    [4096, 4096, 4096, 4096, 4096, 4096, 4096, 4096],
    [4017, 3406, 2276, 799, -799, -2276, -3406, -4017],
    [3784, 1567, -1567, -3784, -3784, -1567, 1567, 3784],
    [3406, -799, -4017, -2276, 2276, 4017, 799, -3406],
    [2896, -2896, -2896, 2896, 2896, -2896, -2896, 2896],
    [2276, -4017, 799, 3406, -3406, -799, 4017, -2276],
    [1567, -3784, 3784, -1567, -1567, 3784, -3784, 1567],
    [799, -2276, 3406, -4017, 4017, -3406, 2276, -799],
];

/// 朴素 O(N²) IDCT 参考实现（用于与 [`idct8`] 对拍——正确性优先于速度）。
///
/// `blk` 为 8×8 频域系数（行主序），输出写回 `out`（时域，已加 128 偏置前）。
pub fn idct8_reference(blk: &[i32; 64], out: &mut [i32; 64]) {
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0i64;
            for v in 0..8 {
                for u in 0..8 {
                    let c = blk[v * 8 + u] as i64;
                    let cu = COS_TABLE[u][x] as i64;
                    let cv = COS_TABLE[v][y] as i64;
                    // 系数含 4096 缩放：两次相乘后需除以 4096²，最后 /4 是
                    // JPEG IDCT 的 1/4 归一化。
                    sum += c * cu * cv;
                }
            }
            // 4096² × 4 = 67_108_864 → 右移 26 位近似
            let val = (sum + (1 << 25)) >> 26;
            out[y * 8 + x] = val as i32;
        }
    }
}

/// 分离式 IDCT（先按行、再按列，O(N²)→O(N·N) 但常数小得多）。
///
/// 与 [`idct8_reference`] 对拍：两者必须落在容差内（定点舍入差 ≤2）。
pub fn idct8(blk: &[i32; 64], out: &mut [i32; 64]) {
    let mut tmp = [0i32; 64];
    // 行变换：Σ_u F(u,v)·4096cos → 除 4096（>>12）还原未缩放的部分和
    for v in 0..8 {
        for x in 0..8 {
            let mut sum = 0i64;
            for u in 0..8 {
                sum += blk[v * 8 + u] as i64 * COS_TABLE[u][x] as i64;
            }
            tmp[v * 8 + x] = ((sum + (1 << 11)) >> 12) as i32;
        }
    }
    // 列变换：Σ_v tmp·4096cos → 除 4096 得 S，再除 4（JPEG 的 1/4 归一化）
    // 合并为 >>14。
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0i64;
            for v in 0..8 {
                sum += tmp[v * 8 + x] as i64 * COS_TABLE[v][y] as i64;
            }
            out[y * 8 + x] = ((sum + (1 << 13)) >> 14) as i32;
        }
    }
}

/// 对拍：分离式与朴素实现的最大差值（容差内即正确性成立）。
pub fn idct_max_diff(blk: &[i32; 64]) -> i32 {
    let mut a = [0i32; 64];
    let mut b = [0i32; 64];
    idct8(blk, &mut a);
    idct8_reference(blk, &mut b);
    let mut max = 0i32;
    for i in 0..64 {
        let d = (a[i] - b[i]).abs();
        if d > max {
            max = d;
        }
    }
    max
}

// ---------------------------------------------------------------------------
// 3. 流式分块（超大图）
// ---------------------------------------------------------------------------

/// 分块计划（主册「超大图 → 流式分块解码（内存峰值受控）」）。
#[derive(Clone, Copy, Debug)]
pub struct StreamPlan {
    pub width: u32,
    pub height: u32,
    /// 每块行数。
    pub rows_per_chunk: u32,
    /// 块数。
    pub chunks: u32,
    /// 单块字节。
    pub chunk_bytes: usize,
}

impl StreamPlan {
    /// 是否超大图（>64MP）。
    pub fn is_huge(width: u32, height: u32) -> bool {
        (width as u64) * (height as u64) > HUGE_IMAGE_PIXELS
    }
    /// 制定计划（块边界按行切，不撕裂行）。
    pub fn plan(width: u32, height: u32, bpp: usize) -> StreamPlan {
        let rows = STREAM_ROWS_PER_CHUNK.max(1);
        let chunks = (height + rows - 1) / rows;
        let chunk_bytes = (width as usize) * rows as usize * bpp;
        StreamPlan { width, height, rows_per_chunk: rows, chunks, chunk_bytes }
    }
    /// 内存峰值是否受控（单块 ≤ 上限）。
    pub fn peak_within_cap(&self) -> bool {
        self.chunk_bytes <= CHUNK_BYTES_CAP
    }
    /// 是否需要缩块（4K 宽时按默认 64 行会超 1MB 上限）。
    pub fn shrink_rows_to_fit(&mut self, bpp: usize) {
        while self.chunk_bytes > CHUNK_BYTES_CAP && self.rows_per_chunk > 1 {
            self.rows_per_chunk /= 2;
            self.chunk_bytes = (self.width as usize) * self.rows_per_chunk as usize * bpp;
            self.chunks = (self.height + self.rows_per_chunk - 1) / self.rows_per_chunk;
        }
    }
    /// 非超大图不需要分块（走整图路径，省掉块管理开销）。
    pub fn needs_streaming(&self) -> bool {
        Self::is_huge(self.width, self.height)
    }
}

// ---------------------------------------------------------------------------
// 4. 指令集档的只读展示 + 基准样本与耗时账
// ---------------------------------------------------------------------------

/// vxbench 基准样本（主册「固定三张」）。
#[derive(Clone, Copy, Debug)]
pub struct BenchSample {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    /// 格式：true = PNG，false = JPEG。
    pub is_png: bool,
}

/// 三张固定样本（4K 照片 / 截图 / 插画）。
pub struct BenchSamples;

impl BenchSamples {
    pub const fn all() -> [BenchSample; BENCH_SAMPLES] {
        [
            BenchSample { name: "4K 照片", width: 3_840, height: 2_160, is_png: false },
            BenchSample { name: "4K 截图", width: 3_840, height: 2_160, is_png: true },
            BenchSample { name: "4K 插画", width: 3_840, height: 2_160, is_png: true },
        ]
    }
    /// 样本是否都为 4K（基准线的适用前提）。
    pub const fn all_4k() -> bool {
        let a = Self::all();
        a[0].width == 3_840 && a[1].width == 3_840 && a[2].width == 3_840
    }
}

/// 解码耗时账（主册「4K PNG ≤150ms、JPEG ≤120ms」）。
#[derive(Clone, Copy, Debug)]
pub struct BenchLedger {
    /// PNG 4K 解码耗时（毫秒），None = 未测。
    pub png_ms: Option<u32>,
    /// JPEG 4K 解码耗时（毫秒）。
    pub jpeg_ms: Option<u32>,
    /// SIMD 与标量逐位对拍是否通过。
    pub bitwise_ok: bool,
    /// 对拍次数。
    pub comparisons: u32,
}

impl BenchLedger {
    pub const fn new() -> Self {
        BenchLedger { png_ms: None, jpeg_ms: None, bitwise_ok: true, comparisons: 0 }
    }
    pub fn note(&mut self, is_png: bool, ms: u32) {
        if is_png {
            self.png_ms = Some(ms);
        } else {
            self.jpeg_ms = Some(ms);
        }
    }
    /// 记一次对拍结果（任一处不一致 → 全盘不通过，正确性优先）。
    pub fn note_compare(&mut self, equal: bool) {
        self.comparisons += 1;
        if !equal {
            self.bitwise_ok = false;
        }
    }
    /// 是否达标（两条线各自达标；未测不算达标）。
    pub fn passes(&self) -> bool {
        matches!(self.png_ms, Some(v) if v <= PNG_4K_REDLINE_MS) && matches!(self.jpeg_ms, Some(v) if v <= JPEG_4K_REDLINE_MS)
    }
    /// 缺口说明（不粉饰：差多少说多少）。
    pub fn verdict(&self) -> &'static str {
        match (self.png_ms, self.jpeg_ms) {
            (None, _) | (_, None) => "两条基准线未测全，无法判定",
            (Some(p), Some(j)) => {
                if p <= PNG_4K_REDLINE_MS && j <= JPEG_4K_REDLINE_MS {
                    "4K PNG 与 JPEG 双线达标"
                } else if p > PNG_4K_REDLINE_MS {
                    "4K PNG 超 150ms 线"
                } else {
                    "4K JPEG 超 120ms 线"
                }
            }
        }
    }
    /// 正确性优先：对拍不过，速度达标也不算交付。
    pub fn deliverable(&self) -> bool {
        self.bitwise_ok && self.passes()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F054-imgsimd-ext");
    // 1) 指令集档：探测与回退（AVX2 不可用 → 基线，无缝）。
    let isa = IsaLevel::probe(true);
    let isa2 = IsaLevel::probe(false);
    cs.add(
        "isa_probe_and_fallback",
        isa == IsaLevel::Avx2 && isa.name() == "AVX2" && isa2 == IsaLevel::Baseline && isa2.name() == "SSE4.2" && isa.lanes() == 8 && isa2.lanes() == 4,
        "",
    );
    // 2) Paeth 预测器（PNG 规范原文：三距离最小者）。
    // paeth(a,b,c)：取 p=a+b−c 后三距离 |p−a|/|p−b|/|p−c| 最小者对应的原值。
    cs.add(
        "paeth_predictor",
        paeth(10, 20, 30) == 10 // p=0：|0−10|=10 最小 → a
            && paeth(0, 0, 0) == 0
            && paeth(255, 255, 255) == 255
            && paeth(100, 0, 0) == 100 // p=100：|100−100|=0 最小 → a
            && paeth(0, 50, 100) == 0, // p=−50：|−50−0|=50 最小 → a
        "",
    );
    // 3) 行过滤五型：标量路径结果正确（Sub/Up/Average/Paeth/None）。
    let prev = [10u8, 20, 30, 40];
    let mut sub = [5u8, 5, 5, 5];
    row_filter(Filter::Sub, &mut sub, &prev, 1).unwrap();
    cs.add("filter_sub", sub == [5u8, 10, 15, 20], "");
    let mut up = [1u8, 2, 3, 4];
    row_filter(Filter::Up, &mut up, &prev, 1).unwrap();
    cs.add("filter_up", up == [11u8, 22, 33, 44], "");
    let mut none = [7u8, 8, 9, 10];
    row_filter(Filter::None, &mut none, &prev, 1).unwrap();
    cs.add("filter_none", none == [7u8, 8, 9, 10], "");
    let mut avg = [3u8, 3, 3, 3];
    row_filter(Filter::Average, &mut avg, &prev, 1).unwrap();
    // 逐字节：a=前一重建值，b=prev[i]；add=(a+b)/2
    // i0: a=0,b=10 → 5 → 8 ｜ i1: a=8,b=20 → 14 → 17 ｜ i2: a=17,b=30 → 23 → 26
    // i3: a=26,b=40 → 33 → 36
    cs.add("filter_average", avg == [8u8, 17, 26, 36], "");
    let mut pa = [1u8, 1, 1, 1];
    row_filter(Filter::Paeth, &mut pa, &prev, 1).unwrap();
    // i0: a=0,b=10,c=0 → p=10，pb=|10−10|=0 最小 → 10 → 11
    // i1: a=11,b=20,c=10 → p=21，pb=1 最小 → 20 → 21 ……
    cs.add("filter_paeth", pa == [11u8, 21, 31, 41], "");
    // 4) 通道化路径与标量逐位一致（Sub/Up 走通道，其余回落标量）。
    let mut a1 = [5u8, 5, 5, 5, 9, 9, 9, 9, 1, 2];
    let mut b1 = a1;
    row_filter(Filter::Sub, &mut a1, &[0u8; 10], 1).unwrap();
    row_filter_lanes(Filter::Sub, &mut b1, &[0u8; 10], 1, LANES).unwrap();
    cs.add("lanes_equal_scalar_sub", bitwise_equal(&a1, &b1), "");
    // 11 字节的行：跨一个完整 8 通道 + 3 字节尾部（尾部路径必须与主路径同结果）
    let prev11 = [10u8, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110];
    let mut a2 = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    let mut b2 = a2;
    row_filter(Filter::Up, &mut a2, &prev11, 1).unwrap();
    row_filter_lanes(Filter::Up, &mut b2, &prev11, 1, LANES).unwrap();
    cs.add("lanes_equal_scalar_up_tail", bitwise_equal(&a2, &b2), "");
    // Average/Paeth 回落标量：结果仍正确（不为「看起来并行」牺牲结果）
    let mut a3 = [3u8, 3, 3, 3];
    let mut b3 = a3;
    row_filter(Filter::Average, &mut a3, &prev, 1).unwrap();
    row_filter_lanes(Filter::Average, &mut b3, &prev, 1, LANES).unwrap();
    cs.add("lanes_falls_back_for_dependent_filters", bitwise_equal(&a3, &b3) && a3 == [8u8, 17, 26, 36], "");
    // 5) 长度不一致 → 错误如实（不产出半行）。
    let mut bad = [1u8, 2, 3];
    cs.add("row_filter_length_mismatch_errors", row_filter(Filter::Sub, &mut bad, &prev, 1).is_err(), "");
    // 6) 错误三要素齐全（缺一即缺陷）。
    let e = DecodeError::bad_filter(9);
    cs.add("decode_error_three_parts", e.complete() && e.next.contains("不产出半张图"), "");
    cs.add("decode_error_truncated", DecodeError::truncated().complete(), "");
    // 7) 半图拦截：失败或未完成都不交付。
    let mut hg = HalfFrameGuard::new(4);
    hg.note_row();
    hg.note_row();
    cs.add("half_frame_blocked_while_incomplete", !hg.deliverable() && hg.verdict() == "解码未完成：不产出画面（半图已拦截）", "");
    hg.note_row();
    hg.note_row();
    cs.add("half_frame_deliverable_when_complete", hg.deliverable() && hg.verdict() == "解码完成：可交付完整画面", "");
    let mut hg2 = HalfFrameGuard::new(2);
    hg2.note_row();
    hg2.note_row();
    hg2.fail();
    cs.add("half_frame_blocked_on_failure", !hg2.deliverable() && hg2.verdict() == "解码失败：不产出画面（半图已拦截）", "");
    // 8) IDCT：分离式与朴素实现落在容差内（正确性优先）。
    let mut blk = [0i32; 64];
    blk[0] = 128;
    blk[1] = 40;
    blk[8] = -20;
    blk[9] = 15;
    cs.add("idct_matches_reference", idct_max_diff(&blk) <= 2, "");
    let mut out = [0i32; 64];
    idct8(&blk, &mut out);
    cs.add("idct_dc_only_is_flat", {
        let mut dc = [0i32; 64];
        dc[0] = 128;
        let mut o = [0i32; 64];
        idct8(&dc, &mut o);
        o.iter().all(|&v| (v - o[0]).abs() <= 1)
    }, "");
    // 9) 流式分块：超大图判定 + 峰值受控 + 缩块。
    // 边界语义：主册「>64MP」是严格大于——正好 64MP 走整图路径（不浪费块管理）
    cs.add(
        "stream_huge_detected",
        !StreamPlan::is_huge(8_000, 8_000) && StreamPlan::is_huge(8_001, 8_000) && !StreamPlan::is_huge(3_840, 2_160),
        "",
    );
    let sp = StreamPlan::plan(3_840, 2_160, 4);
    cs.add(
        "stream_plan_chunked",
        sp.chunks == 34 && !sp.needs_streaming() && sp.peak_within_cap(),
        "",
    );
    // 8K 宽图：默认 64 行会超 1MB → 缩块到受控
    let mut sp2 = StreamPlan::plan(7_680, 4_320, 4);
    sp2.shrink_rows_to_fit(4);
    cs.add("stream_shrink_to_fit", sp2.peak_within_cap() && sp2.rows_per_chunk <= 64 && sp2.chunks >= 68, "");
    // 10) 基准样本三张且均为 4K。
    cs.add("bench_samples_three_4k", BenchSamples::all().len() == 3 && BenchSamples::all_4k(), "");
    // 11) 双线达标判定（未测全不算达标；超线要说哪条超）。
    let mut bl = BenchLedger::new();
    cs.add("bench_not_measured_not_pass", !bl.passes() && bl.verdict() == "两条基准线未测全，无法判定", "");
    bl.note(true, 120);
    bl.note(false, 130);
    cs.add("bench_jpeg_over_line", !bl.passes() && bl.verdict() == "4K JPEG 超 120ms 线", "");
    bl.note(false, 100);
    cs.add("bench_both_pass", bl.passes() && bl.verdict() == "4K PNG 与 JPEG 双线达标", "");
    // 12) 正确性优先：对拍不过，速度达标也不交付。
    let mut bl2 = BenchLedger::new();
    bl2.note(true, 100);
    bl2.note(false, 90);
    bl2.note_compare(true);
    bl2.note_compare(false);
    cs.add("bitwise_failure_blocks_delivery", !bl2.deliverable() && bl2.passes() && bl2.comparisons == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_filter_roundtrip_with_encoder_side() {
        // 用编码侧公式造一行 Sub 过滤数据，解码侧必须还原原值
        let raw = [200u8, 210, 220, 230, 240, 250];
        let mut enc = [0u8; 6];
        for i in 0..6 {
            let a = if i >= 1 { raw[i - 1] } else { 0 };
            enc[i] = raw[i].wrapping_sub(a);
        }
        let mut dec = enc;
        row_filter(Filter::Sub, &mut dec, &[0u8; 6], 1).unwrap();
        assert_eq!(dec, raw, "Sub 过滤必须能还原原行");
    }

    #[test]
    fn lanes_and_scalar_agree_on_long_rows() {
        // 300 字节的行：跨多个通道 + 尾部不足一通道
        let prev = [7u8; 300];
        let mut a = [3u8; 300];
        let mut b = [3u8; 300];
        row_filter(Filter::Up, &mut a, &prev, 1).unwrap();
        row_filter_lanes(Filter::Up, &mut b, &prev, 1, 8).unwrap();
        assert!(bitwise_equal(&a, &b));
        assert_eq!(a[0], 10);
    }

    #[test]
    fn idct_of_dc_block_is_uniform() {
        let mut blk = [0i32; 64];
        blk[0] = 512;
        let mut out = [0i32; 64];
        idct8(&blk, &mut out);
        let first = out[0];
        assert!(out.iter().all(|&v| (v - first).abs() <= 1), "纯 DC 块解出应为平坦块");
    }

    #[test]
    fn idct_separable_within_tolerance_of_reference() {
        let mut blk = [0i32; 64];
        for i in 0..64 {
            blk[i] = ((i as i32) * 7 % 41) - 20;
        }
        assert!(idct_max_diff(&blk) <= 2, "分离式与朴素实现的最大差值应在定点容差内");
    }

    #[test]
    fn bench_ledger_rejects_fast_but_wrong() {
        let mut b = BenchLedger::new();
        b.note(true, 10);
        b.note(false, 10);
        b.note_compare(false);
        assert!(b.passes(), "速度达标");
        assert!(!b.deliverable(), "但结果不对 → 不交付");
    }

    #[test]
    fn stream_plan_never_tears_a_row() {
        let p = StreamPlan::plan(3_840, 2_160, 4);
        // 块数 × 每块行数 ≥ 总行数（最后一块可以短，但绝不跨行切）
        assert!(p.chunks as u64 * p.rows_per_chunk as u64 >= p.height as u64);
    }
}
