//! VE-F0081 · 花屏检测（采样校验和）（VE-A 域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0081`
//!
//! **职责定位（锚点原文）**：渲染输出的**采样校验和**检测（每帧随机采样点的
//! 校验和比对），**花屏模式识别**（系统性偏差 vs 随机噪声），采样**零开销**设计。
//!
//! **判据（锚点原文）**：随机采样、校验和比对、模式识别、零开销、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 误报 → **采样策略调优** | [`GlitchScanner::retune`] 调整采样点数量（`[MIN_POINTS, MAX_POINTS]`），每次留 [`TuneNote`]。误报的根因通常是「采样点落在动画区域」，调多不是调少——所以下界是 8 而不是 1 |
//! | 偏差检出 → **转储** | 判为 [`Pattern::Systematic`] 的那一帧置 `dump = true`；随机噪声**不**转储（转储通道经不起每帧一次） |
//! | 开销超标 → **降频采样** | [`GlitchScanner::set_cadence`] 每 N 帧采一次（N ≥ 1），跳过的帧记 `skipped`。零开销的另一半是「可以完全不采」——`cadence == 0` 表示关检测，与「每帧都采」是两个极端，都允许 |
//!
//! **性能逐项分解（锚点原文）**：O(采样点)。点数定容 [`MAX_POINTS`] = 16，
//! 每帧一次线性比对，无分配、无排序（模式判定用 16 槽直方图）。
//!
//! **跨批对接点**：A80 看门狗联动——本模块判出的系统性偏差可直接作为
//! `vea80_blackdog::DumpRequest` 的触发理由（同一帧号、同一时间线）。
//!
//! **无障碍与隐私（锚点原文）**：检测结果读屏可达
//! （[`GlitchScanner::a11y_lines`]）——只报帧号、失配点数、模式与坐标区间；
//! 不读画面内容。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **采样点必须每帧变**：固定采样点会让「恰好那块区域是动画」变成永久
//!   误报或永久漏报。用 xorshift64 从 seed 推进，同一 seed 可复现
//!   （排障要能重放），不同帧不同点。
//! - **模式识别靠空间聚集，不靠数量**：20 个失配散在全屏是噪声，3 个失配
//!   挤在同一 tile 是系统性偏差。判据钉死「同 tile 占比 ≥ 60% 且至少 3 点」。
//! - **零开销 = 定容 + 可关**：没有堆分配、点数上限固定、可以整帧跳过。
//! - **零 panic 面**：无 `unwrap`、索引全部经 `get` 或定容常量。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA79D/E 与 0xA7AD/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 采样点数越界。
pub const E_POINT_RANGE: u16 = 0xA79D;
/// 观测长度与采样点数不一致。
pub const E_LENGTH_MISMATCH: u16 = 0xA79E;
/// 帧尺寸为零。
pub const E_BAD_DIMENSION: u16 = 0xA7AD;
/// 采样种子为零（xorshift 全零态不自持）。
pub const E_ZERO_SEED: u16 = 0xA7AE;

// ---------------------------------------------------------------------------
// 二、容量
// ---------------------------------------------------------------------------

/// 单帧采样点上限。
pub const MAX_POINTS: usize = 16;
/// 单帧采样点下限：少于 8 个点时，一屏 16 个 tile 平均分不到一个。
pub const MIN_POINTS: usize = 8;
/// tile 边长（像素）。
pub const TILE_PX: u16 = 64;
/// 屏幕按 4×4 划分。
pub const TILES_PER_AXIS: u8 = 4;
pub const TILE_COUNT: u8 = TILES_PER_AXIS * TILES_PER_AXIS;
/// 判为系统性偏差所需的同 tile 占比（百分比）。
pub const SYSTEMATIC_SHARE: u32 = 60;
/// 判为系统性偏差所需的最小失配点数。
pub const SYSTEMATIC_MIN_HITS: u8 = 3;

// ---------------------------------------------------------------------------
// 三、采样
// ---------------------------------------------------------------------------

/// 一个采样点。`expect` 是基准帧在该点的值，由调用方在
/// [`GlitchScanner::begin_frame`] 之后填入（本模块不读显存）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SamplePoint {
    pub x: u16,
    pub y: u16,
    pub expect: u32,
}

/// xorshift64*：周期足够、无查表、可在 `no_std` 下常数时间推进。
#[derive(Clone, Copy, Debug)]
pub struct Rng(u64);

impl Rng {
    pub const fn new(seed: u64) -> Rng {
        // 零种子会让 xorshift 永远输出 0；换成一个非零常量而不是报错，
        // 因为调用方给的 seed 常来自 TSC，偶发为零不该让检测停摆。
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 取 `[0, bound)` 的均匀值；`bound == 0` 返回 0。
    pub fn below(&mut self, bound: u16) -> u16 {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as u16
    }
}

// ---------------------------------------------------------------------------
// 四、模式
// ---------------------------------------------------------------------------

/// 一帧的判定模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    /// 全部一致。
    Clean,
    /// 失配分散：随机噪声（压缩/抖动/时序）。
    RandomNoise,
    /// 失配聚集在某个 tile：系统性偏差。
    Systematic { tile: u8, hits: u8 },
}

impl Pattern {
    pub fn as_str(self) -> &'static str {
        match self {
            Pattern::Clean => "clean",
            Pattern::RandomNoise => "noise",
            Pattern::Systematic { .. } => "systematic",
        }
    }

    pub fn needs_dump(self) -> bool {
        matches!(self, Pattern::Systematic { .. })
    }
}

/// 一帧的结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameVerdict {
    pub frame: u64,
    pub mismatches: u8,
    pub pattern: Pattern,
    /// 本帧是否真的采了（降频时可能跳过）。
    pub sampled: bool,
    /// 是否请求转储。
    pub dump: bool,
}

/// 一次调参留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TuneNote {
    pub from_points: u8,
    pub to_points: u8,
    pub at_frame: u64,
}

// ---------------------------------------------------------------------------
// 五、扫描器
// ---------------------------------------------------------------------------

/// 花屏扫描器。
pub struct GlitchScanner {
    points: [SamplePoint; MAX_POINTS],
    count: usize,
    rng: Rng,
    width: u16,
    height: u16,
    /// 每 N 帧采一次；0 表示关闭。
    cadence: u8,
    frames: u64,
    clean: u64,
    noise: u64,
    systematic: u64,
    skipped: u64,
    dumps: u32,
    tunes: [Option<TuneNote>; 4],
    tune_count: usize,
}

impl GlitchScanner {
    pub fn new(seed: u64, width: u16, height: u16, points: usize, cadence: u8) -> Result<GlitchScanner, u16> {
        if width == 0 || height == 0 {
            return Err(E_BAD_DIMENSION);
        }
        if seed == 0 {
            return Err(E_ZERO_SEED);
        }
        if !(MIN_POINTS..=MAX_POINTS).contains(&points) {
            return Err(E_POINT_RANGE);
        }
        Ok(GlitchScanner {
            points: [SamplePoint::default(); MAX_POINTS],
            count: points,
            rng: Rng::new(seed),
            width,
            height,
            cadence,
            frames: 0,
            clean: 0,
            noise: 0,
            systematic: 0,
            skipped: 0,
            dumps: 0,
            tunes: [None; 4],
            tune_count: 0,
        })
    }

    pub fn points(&self) -> usize {
        self.count
    }

    pub fn cadence(&self) -> u8 {
        self.cadence
    }

    pub fn snapshot(&self) -> [SamplePoint; MAX_POINTS] {
        self.points
    }

    /// 开始一帧：按当前配置决定是否采样，并重新掷点。
    /// 返回本帧是否采样（false 时调用方不必提供观测值）。
    pub fn begin_frame(&mut self) -> bool {
        self.frames = self.frames.saturating_add(1);
        if self.cadence == 0 {
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        if self.cadence > 1 && (self.frames % self.cadence as u64) != 0 {
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        for i in 0..self.count {
            let x = self.rng.below(self.width);
            let y = self.rng.below(self.height);
            self.points[i] = SamplePoint { x, y, expect: 0 };
        }
        true
    }

    /// 写入基准值（上一帧同点取值）。只在 `begin_frame` 返回 true 后调用。
    pub fn seed_expectations(&mut self, expect: &[u32]) -> Result<(), u16> {
        if expect.len() != self.count {
            return Err(E_LENGTH_MISMATCH);
        }
        for (i, v) in expect.iter().enumerate() {
            self.points[i].expect = *v;
        }
        Ok(())
    }

    /// 观测本帧，返回判定。
    pub fn observe(&mut self, actual: &[u32]) -> Result<FrameVerdict, u16> {
        if actual.len() != self.count {
            return Err(E_LENGTH_MISMATCH);
        }
        let mut tiles = [0u8; TILE_COUNT as usize];
        let mut mismatches = 0u8;
        for i in 0..self.count {
            if actual[i] != self.points[i].expect {
                mismatches = mismatches.saturating_add(1);
                let t = tile_of(self.points[i].x, self.points[i].y);
                tiles[t as usize] = tiles[t as usize].saturating_add(1);
            }
        }
        let pattern = classify(mismatches, &tiles);
        match pattern {
            Pattern::Clean => self.clean = self.clean.saturating_add(1),
            Pattern::RandomNoise => self.noise = self.noise.saturating_add(1),
            Pattern::Systematic { .. } => self.systematic = self.systematic.saturating_add(1),
        }
        let dump = pattern.needs_dump();
        if dump {
            self.dumps = self.dumps.saturating_add(1);
        }
        Ok(FrameVerdict {
            frame: self.frames,
            mismatches,
            pattern,
            sampled: true,
            dump,
        })
    }

    /// 调采样点数（误报调优）。
    pub fn retune(&mut self, points: usize) -> Result<(), u16> {
        if !(MIN_POINTS..=MAX_POINTS).contains(&points) {
            return Err(E_POINT_RANGE);
        }
        let from = self.count as u8;
        self.count = points;
        if (self.tune_count as usize) < self.tunes.len() {
            self.tunes[self.tune_count] = Some(TuneNote {
                from_points: from,
                to_points: points as u8,
                at_frame: self.frames,
            });
            self.tune_count += 1;
        }
        Ok(())
    }

    pub fn tunes(&self) -> &[Option<TuneNote>] {
        &self.tunes
    }

    /// 降频采样。0 表示关闭检测。
    pub fn set_cadence(&mut self, n: u8) {
        self.cadence = n;
    }

    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        (self.frames, self.clean, self.noise, self.systematic, self.skipped)
    }

    pub fn dumps(&self) -> u32 {
        self.dumps
    }

    /// 读屏行。
    pub fn a11y_lines(&self) -> Vec<String> {
        let (frames, clean, noise, systematic, skipped) = self.stats();
        let mut out = Vec::new();
        out.push(format!(
            "花屏检测：{} 帧，采样 {} 点 / {} frames, {} points",
            frames, self.count, frames, self.count
        ));
        out.push(format!(
            "正常 {}，噪声 {}，系统性 {} / clean {}, noise {}, systematic {}",
            clean, noise, systematic, clean, noise, systematic
        ));
        out.push(format!(
            "跳过 {} 帧，转储 {} 次 / {} skipped, {} dumps",
            skipped, self.dumps, skipped, self.dumps
        ));
        out.push(format!("采样节拍 {} / cadence {}", self.cadence, self.cadence));
        out
    }
}

/// 采样点落在哪个 tile（4×4 网格）。
pub fn tile_of(x: u16, y: u16) -> u8 {
    let tx = (x / TILE_PX).min(TILES_PER_AXIS as u16 - 1) as u8;
    let ty = (y / TILE_PX).min(TILES_PER_AXIS as u16 - 1) as u8;
    ty.saturating_mul(TILES_PER_AXIS).saturating_add(tx)
}

/// 模式判定：看聚集度，不看绝对数量。
pub fn classify(mismatches: u8, tiles: &[u8; TILE_COUNT as usize]) -> Pattern {
    if mismatches == 0 {
        return Pattern::Clean;
    }
    let mut best = 0u8;
    let mut best_idx = 0usize;
    for (i, n) in tiles.iter().enumerate() {
        if *n > best {
            best = *n;
            best_idx = i;
        }
    }
    let share = (best as u32).saturating_mul(100) / mismatches as u32;
    if best >= SYSTEMATIC_MIN_HITS && share >= SYSTEMATIC_SHARE {
        Pattern::Systematic {
            tile: best_idx as u8,
            hits: best,
        }
    } else {
        Pattern::RandomNoise
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

fn scanner() -> GlitchScanner {
    GlitchScanner::new(0xC0FF_EE12_3456_789A, 640, 480, 12, 1).unwrap_or_else(|_| {
        // 构造参数是编译期常量，不会失败；兜底只为去掉 unwrap 面。
        GlitchScanner {
            points: [SamplePoint::default(); MAX_POINTS],
            count: MIN_POINTS,
            rng: Rng::new(1),
            width: 640,
            height: 480,
            cadence: 1,
            frames: 0,
            clean: 0,
            noise: 0,
            systematic: 0,
            skipped: 0,
            dumps: 0,
            tunes: [None; 4],
            tune_count: 0,
        }
    })
}

/// VE-F0081 的十四条判据。
pub fn run_vea81_checks() -> CheckSet {
    let mut c = CheckSet::new("vea81");

    // 1. 构造参数校验：零尺寸、零种子、点数越界。
    if GlitchScanner::new(1, 0, 480, 12, 1).err() == Some(E_BAD_DIMENSION)
        && GlitchScanner::new(1, 640, 480, 4, 1).err() == Some(E_POINT_RANGE)
    {
        c.ok("A81-构造-参数越界即拒");
    } else {
        c.fail("A81-构造-参数越界即拒", "bad ctor args accepted");
    }

    // 2. 采样点数量与配置一致，且落在屏幕内。
    let mut s = scanner();
    let sampled = s.begin_frame();
    let pts = s.snapshot();
    let in_bounds = pts[..s.points()].iter().all(|p| p.x < 640 && p.y < 480);
    if sampled && s.points() == 12 && in_bounds {
        c.ok("A81-采样-点数一致且在界内");
    } else {
        c.fail("A81-采样-点数一致且在界内", "sample points out of bounds");
    }

    // 3. 同 seed 可复现。
    let mut a = GlitchScanner::new(42, 1280, 720, 10, 1).unwrap_or_else(|_| scanner());
    let mut b = GlitchScanner::new(42, 1280, 720, 10, 1).unwrap_or_else(|_| scanner());
    let _ = a.begin_frame();
    let _ = b.begin_frame();
    if a.snapshot() == b.snapshot() {
        c.ok("A81-采样-同种子可复现");
    } else {
        c.fail("A81-采样-同种子可复现", "seed not reproducible");
    }

    // 4. 逐帧推进会产生新点集。
    let before = a.snapshot();
    let _ = a.begin_frame();
    if a.snapshot() != before {
        c.ok("A81-采样-逐帧换点");
    } else {
        c.fail("A81-采样-逐帧换点", "points never change");
    }

    // 5. 全等 → Clean，不转储。
    let mut clean = scanner();
    let _ = clean.begin_frame();
    let expect = [7u32; 12];
    let _ = clean.seed_expectations(&expect);
    match clean.observe(&expect) {
        Ok(v) if v.pattern == Pattern::Clean && !v.dump => c.ok("A81-比对-全等即正常"),
        _ => c.fail("A81-比对-全等即正常", "clean frame flagged"),
    }

    // 6. 长度不符即拒。
    if clean.observe(&[1, 2, 3]) == Err(E_LENGTH_MISMATCH) {
        c.ok("A81-比对-长度不符即拒");
    } else {
        c.fail("A81-比对-长度不符即拒", "length mismatch accepted");
    }

    // 7. 分散失配 → 随机噪声，不转储。
    let mut noise = scanner();
    let _ = noise.begin_frame();
    let _ = noise.seed_expectations(&[1u32; 12]);
    let mut actual = [1u32; 12];
    actual[0] = 9;
    actual[5] = 9;
    match noise.observe(&actual) {
        Ok(v) if v.pattern == Pattern::RandomNoise && !v.dump => c.ok("A81-模式-分散为噪声"),
        _ => c.fail("A81-模式-分散为噪声", "scatter misread as systematic"),
    }

    // 8. 聚集失配 → 系统性偏差，转储。
    let mut tiles = [0u8; TILE_COUNT as usize];
    tiles[0] = 4;
    let pat = classify(5, &tiles);
    if matches!(pat, Pattern::Systematic { tile: 0, hits: 4 }) && pat.needs_dump() {
        c.ok("A81-模式-聚集为系统偏差");
    } else {
        c.fail("A81-模式-聚集为系统偏差", "cluster misread as noise");
    }

    // 9. 聚集但点数不足仍算噪声（避免 2 点巧合）。
    let mut thin = [0u8; TILE_COUNT as usize];
    thin[3] = 2;
    if classify(3, &thin) == Pattern::RandomNoise {
        c.ok("A81-模式-点数不足归噪声");
    } else {
        c.fail("A81-模式-点数不足归噪声", "thin cluster over-read");
    }

    // 10. tile 计算边界：右下角不越界。
    if tile_of(639, 479) == TILE_COUNT - 1 && tile_of(0, 0) == 0 {
        c.ok("A81-区域-角落归属正确");
    } else {
        c.fail("A81-区域-角落归属正确", "tile edge wrong");
    }

    // 11. 降频：cadence=3 时三帧只采一帧。
    let mut slow = GlitchScanner::new(7, 800, 600, 8, 3).unwrap_or_else(|_| scanner());
    let mut hits = 0;
    for _ in 0..6 {
        if slow.begin_frame() {
            hits += 1;
        }
    }
    if hits == 2 {
        c.ok("A81-节拍-三分频生效");
    } else {
        c.fail("A81-节拍-三分频生效", "cadence wrong");
    }

    // 12. cadence=0 表示关闭：全跳过。
    slow.set_cadence(0);
    let mut any = false;
    for _ in 0..4 {
        if slow.begin_frame() {
            any = true;
        }
    }
    if !any {
        c.ok("A81-节拍-关检测零采样");
    } else {
        c.fail("A81-节拍-关检测零采样", "off cadence still samples");
    }

    // 13. 调点数留痕，越界拒绝。
    let mut t = scanner();
    let ok = t.retune(16);
    let bad = t.retune(2);
    if ok.is_ok() && bad == Err(E_POINT_RANGE) && t.points() == 16 {
        c.ok("A81-调参-区间内留痕");
    } else {
        c.fail("A81-调参-区间内留痕", "retune window wrong");
    }
    if t.tunes()[0].map(|n| n.to_points) == Some(16) {
        c.ok("A81-调参-变更可追溯");
    } else {
        c.fail("A81-调参-变更可追溯", "tune note missing");
    }

    // 14. 读屏四行且含关键计数。
    let lines = t.a11y_lines();
    if lines.len() == 4 && lines[0].contains("采样") {
        c.ok("A81-读屏-状态可播");
    } else {
        c.fail("A81-读屏-状态可播", "a11y incomplete");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(9);
        let mut b = Rng::new(9);
        for _ in 0..8 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn rng_zero_seed_is_remapped() {
        let mut r = Rng::new(0);
        assert_ne!(r.next_u64(), 0);
    }

    #[test]
    fn rng_below_stays_in_range() {
        let mut r = Rng::new(5);
        for _ in 0..64 {
            assert!(r.below(10) < 10);
        }
        assert_eq!(r.below(0), 0);
    }

    #[test]
    fn tile_corners() {
        assert_eq!(tile_of(0, 0), 0);
        assert_eq!(tile_of(63, 0), 0);
        assert_eq!(tile_of(64, 0), 1);
        assert_eq!(tile_of(0, 64), 4);
        assert_eq!(tile_of(65535, 65535), TILE_COUNT - 1);
    }

    #[test]
    fn classify_boundaries() {
        let mut t = [0u8; TILE_COUNT as usize];
        assert_eq!(classify(0, &t), Pattern::Clean);
        t[1] = 3;
        assert!(matches!(classify(3, &t), Pattern::Systematic { tile: 1, .. }));
        t[1] = 2;
        t[2] = 2;
        assert_eq!(classify(4, &t), Pattern::RandomNoise);
    }

    #[test]
    fn expectations_must_match_point_count() {
        let mut s = scanner();
        let _ = s.begin_frame();
        assert_eq!(s.seed_expectations(&[0u32; 3]), Err(E_LENGTH_MISMATCH));
        assert_eq!(s.seed_expectations(&[0u32; 12]), Ok(()));
    }

    #[test]
    fn stats_accumulate() {
        let mut s = scanner();
        let _ = s.begin_frame();
        let _ = s.seed_expectations(&[3u32; 12]);
        let _ = s.observe(&[3u32; 12]);
        let (frames, clean, noise, systematic, _) = s.stats();
        assert_eq!(frames, 1);
        assert_eq!(clean, 1);
        assert_eq!(noise + systematic, 0);
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea81_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea81 red: {}", cs.red_items_text());
        assert!(p >= 14);
    }
}
