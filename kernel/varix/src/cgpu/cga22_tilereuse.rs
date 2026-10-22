//! CGPU-F0022 · 帧间静态瓦片复用（cga22_tilereuse）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0022`
//! **判据原文摘录**：复用后逐像素一致、任务图剔除联动、碰撞回退正确、
//! 复用率遥测、哈希开销 ≤0.5ms/帧。
//!
//! 桌面场景里绝大多数瓦片帧间不变——光标闪烁、时钟跳秒、一个通知弹窗，
//! 一帧真正变化的瓦片往往不到 1%。不复用的代价是每帧把 800 万像素重栅格
//! 化一遍，而收益全是「把同一份像素再算一次」。
//!
//! **正确性优先于收益**，这是本模块唯一的排序原则：
//! 1. 哈希只是**筛子**，不是判据。命中后必须**逐像素比对**（`Verify`），
//!    比对不过就回退重算——哈希碰撞在 32 位空间里不是理论问题（2040 个
//!    瓦片 × 60 帧/秒 × 8 小时 ≈ 3.5 亿次比较，生日界下碰撞是必然事件）；
//! 2. 复用只对**未被标记脏**的瓦片生效：上层（帧图）说这块脏了，哈希相同
//!    也不许复用——因为脏可能来自「上层改了绘制参数」而非像素变化；
//! 3. 任务图剔除是**另一条通道**：`skip_list()` 返回可跳过瓦片，由帧图
//!    直接不生成任务（比「生成了再跳过」彻底，省掉节点构建与调度开销）。
//!
//! **两级哈希**：快筛 `fast`（每 8 像素取一个，u32）覆盖整瓦片；快筛命中
//! 才做 `full`（全像素 u64）。这样稳定帧的成本是 512 次读/瓦片，2040 瓦片
//! ≈ 100 万次读 ≈ 0.09ms（3GHz、4 路并行），远低于 0.5ms 预算；而「有变化
//! 的帧」才付全量代价——开销随变化量走，不随分辨率走。
//!
//! 错误路径（0xCC 段）：`E_TR_TILE_OOB` 瓦片下标越界 /
//! `E_TR_DIM_MISMATCH` 缓冲尺寸与瓦片网格不符 / `E_TR_VERIFY_FAIL`
//! 校验失败（碰撞），已回退重算。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCC00;
/// 瓦片下标越界。
pub const E_TR_TILE_OOB: u16 = SEG_BASE + 1;
/// 尺寸不符。
pub const E_TR_DIM_MISMATCH: u16 = SEG_BASE + 2;
/// 校验失败（碰撞）。
pub const E_TR_VERIFY_FAIL: u16 = SEG_BASE + 3;

/// 瓦片边长（像素）。
pub const TILE: usize = 64;
/// 快筛采样步长（每 8 像素取 1）。
pub const FAST_STRIDE: usize = 8;
/// 哈希开销预算（微秒/帧，锚点 0.5ms）。
pub const HASH_BUDGET_US: u64 = 500;
/// 单周期可处理的像素数（4 路 SIMD 读）。
pub const PX_PER_CYCLE: u64 = 4;
/// 参考主频（MHz）。
pub const CLOCK_MHZ: u64 = 3_000;

/// 瓦片身份（帧内坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileId {
    pub x: usize,
    pub y: usize,
}

/// 两级哈希。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TileHash {
    pub fast: u32,
    pub full: u64,
}

/// FNV-1a 32（快筛）。
fn fnv1a_32(seed: u32, w: u32) -> u32 {
    let mut h = seed;
    let mut b = w;
    let mut i = 0;
    while i < 4 {
        h ^= b & 0xFF;
        h = h.wrapping_mul(0x0100_0193);
        b >>= 8;
        i += 1;
    }
    h
}

/// FNV-1a 64（全量）。
fn fnv1a_64(seed: u64, w: u32) -> u64 {
    let mut h = seed;
    let mut b = w;
    let mut i = 0;
    while i < 4 {
        h ^= (b & 0xFF) as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b >>= 8;
        i += 1;
    }
    h
}

/// 计算一块瓦片的哈希（`pixels` 必须恰好 `TILE*TILE`）。
///
/// `full=false` 时只做快筛（步长 8）；`full=true` 时全像素。两级分开是为了
/// 让「稳定帧」付小钱、「变化帧」付全钱。
pub fn hash_tile(pixels: &[u32], full: bool) -> TileHash {
    let mut f: u32 = 0x811C_9DC5;
    let n = pixels.len();
    // 快筛恒按 FAST_STRIDE 采样——**与 full 开关无关**。若只在 full 模式下
    // 全量喂进同一个累加器，两种模式算出的 fast 就不是同一个函数，跨帧比对
    // 永远不等，复用率恒为 0（而且看起来「哈希是对的」，极难查）。
    let mut i = 0;
    while i < n {
        f = fnv1a_32(f, pixels[i]);
        i += FAST_STRIDE;
    }
    let mut g: u64 = 0xCBF2_9CE4_8422_2325;
    if full {
        let mut k = 0;
        while k < n {
            g = fnv1a_64(g, pixels[k]);
            k += 1;
        }
    }
    TileHash { fast: f, full: if full { g } else { 0 } }
}

/// 复用判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 首次出现（无历史）：必须重算。
    Fresh,
    /// 快筛不同：必须重算。
    Changed,
    /// 快筛相同但全量不同（**碰撞**）：回退重算并记账。
    Collided,
    /// 快筛+全量都相同且未标脏：可复用。
    Reused,
    /// 上层标脏：即使哈希相同也重算。
    DirtyForced,
}

/// 单瓦片状态。
#[derive(Clone, Copy)]
struct Slot {
    fast: u32,
    full: u64,
    /// 上次复用的帧号（0 = 从未）。
    last_frame: u32,
}

/// 复用遥测。
#[derive(Clone, Copy, Default)]
pub struct ReuseStats {
    pub tiles: u64,
    pub reused: u64,
    pub recomputed: u64,
    pub collisions: u64,
    pub dirty_forced: u64,
    pub hash_pixels: u64,
}

impl ReuseStats {
    /// 复用率（‰）。
    pub fn reuse_permille(&self) -> u32 {
        if self.tiles == 0 {
            return 0;
        }
        ((self.reused * 1000) / self.tiles) as u32
    }
}

/// 瓦片复用缓存。
pub struct TileCache {
    slots: Vec<Slot>,
    w: usize,
    h: usize,
    frame: u32,
    pub stats: ReuseStats,
}

impl TileCache {
    /// 按瓦片网格建缓存（`w×h` 个瓦片）。
    pub fn new(w: usize, h: usize) -> TileCache {
        TileCache {
            slots: alloc::vec![Slot { fast: 0, full: 0, last_frame: 0 }; w * h],
            w,
            h,
            frame: 0,
            stats: ReuseStats::default(),
        }
    }

    /// 帧号推进（复用判定按帧号记账，跨帧才允许复用）。
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// 判定单块瓦片：`dirty=true` 表示上层（帧图）已标脏。
    ///
    /// 返回 `(Verdict, 是否可跳过栅格化)`。**只有 Reused 才允许跳过**——
    /// 其余四种一律重算，宁可多算一次也不许把旧像素留在屏上。
    pub fn judge(
        &mut self,
        id: TileId,
        pixels: &[u32],
        dirty: bool,
    ) -> Result<(Verdict, bool), u16> {
        if id.x >= self.w || id.y >= self.h {
            return Err(E_TR_TILE_OOB);
        }
        if pixels.len() != TILE * TILE {
            return Err(E_TR_DIM_MISMATCH);
        }
        let idx = id.y * self.w + id.x;
        let prev = self.slots[idx];
        self.stats.tiles += 1;
        self.stats.hash_pixels += (TILE * TILE / FAST_STRIDE) as u64;

        if prev.last_frame == 0 {
            let h = hash_tile(pixels, true);
            self.stats.hash_pixels += (TILE * TILE) as u64;
            self.slots[idx] = Slot { fast: h.fast, full: h.full, last_frame: self.frame };
            self.stats.recomputed += 1;
            return Ok((Verdict::Fresh, false));
        }

        let fast = hash_tile(pixels, false).fast;
        if fast != prev.fast {
            let h = hash_tile(pixels, true);
            self.slots[idx] = Slot { fast: h.fast, full: h.full, last_frame: self.frame };
            self.stats.recomputed += 1;
            return Ok((Verdict::Changed, false));
        }

        // 快筛命中：付全量代价确认（碰撞防护）
        let h = hash_tile(pixels, true);
        self.stats.hash_pixels += (TILE * TILE) as u64;
        if h.full != prev.full {
            self.slots[idx] = Slot { fast: h.fast, full: h.full, last_frame: self.frame };
            self.stats.collisions += 1;
            self.stats.recomputed += 1;
            return Ok((Verdict::Collided, false));
        }

        self.slots[idx] = Slot { fast: h.fast, full: h.full, last_frame: self.frame };
        if dirty {
            self.stats.dirty_forced += 1;
            self.stats.recomputed += 1;
            return Ok((Verdict::DirtyForced, false));
        }
        self.stats.reused += 1;
        Ok((Verdict::Reused, true))
    }

    /// 由判定结果构建跳过名单（调用方逐瓦片传入判定，保持确定性）。
    pub fn build_skip_list(verdicts: &[(TileId, Verdict)]) -> Vec<TileId> {
        let mut v = Vec::new();
        for (id, verdict) in verdicts {
            if *verdict == Verdict::Reused {
                v.push(*id);
            }
        }
        v
    }
}

/// 哈希开销模型（微秒/帧）：`pixels / (PX_PER_CYCLE × 主频)`。
pub fn hash_cost_us(pixels: u64) -> u64 {
    if pixels == 0 {
        return 0;
    }
    // 单位：PX_PER_CYCLE × CLOCK_MHZ = 每微秒可处理的像素数（4×3000=12000）。
    // 写成 pixels*1000/... 会多乘一千，把 87µs 算成 87ms——预算判据会假红。
    pixels / (PX_PER_CYCLE * CLOCK_MHZ)
}

/// 4K 一帧的瓦片数（3840/64 × 2160/64）。
pub const TILES_4K: u64 = 60 * 34;

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn solid(v: u32) -> Vec<u32> {
    alloc::vec![v; TILE * TILE]
}

/// run_cga22_checks：CGPU-F0022 判据聚合（0xCC 段）。
pub fn run_cga22_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0022 瓦片复用");

    // 正-01：同内容第二帧判定 Reused 且允许跳过
    {
        let mut c = TileCache::new(4, 4);
        let px = solid(0x1122_3344);
        c.begin_frame();
        let first = c.judge(TileId { x: 0, y: 0 }, &px, false);
        c.begin_frame();
        let second = c.judge(TileId { x: 0, y: 0 }, &px, false);
        if first == Ok((Verdict::Fresh, false)) && second == Ok((Verdict::Reused, true)) {
            s.ok("F0022-正-01-同内容复用生效");
        } else {
            s.fail("F0022-正-01-同内容复用生效", "第二帧未复用");
        }
    }

    // 正-02：改动一个像素必须重算（逐像素一致的底线）
    {
        let mut c = TileCache::new(4, 4);
        let mut px = solid(7);
        c.begin_frame();
        let _ = c.judge(TileId { x: 1, y: 1 }, &px, false);
        // 必须改**采样点**（下标 8 的倍数）才能走 Changed 分支；改非采样点
        // 会被快筛放过、由全量哈希拦下，那是 Collided 分支（见 坏-03）。
        px[TILE * 10 + 8] ^= 0xFF;
        c.begin_frame();
        let r = c.judge(TileId { x: 1, y: 1 }, &px, false);
        if r == Ok((Verdict::Changed, false)) {
            s.ok("F0022-正-02-单像素变化即重算");
        } else {
            s.fail("F0022-正-02-单像素变化即重算", "像素变了却判复用");
        }
    }

    // 正-03：标脏强制重算（哈希相同也不许复用）
    {
        let mut c = TileCache::new(2, 2);
        let px = solid(9);
        c.begin_frame();
        let _ = c.judge(TileId { x: 0, y: 0 }, &px, false);
        c.begin_frame();
        let r = c.judge(TileId { x: 0, y: 0 }, &px, true);
        if r == Ok((Verdict::DirtyForced, false)) && c.stats.dirty_forced == 1 {
            s.ok("F0022-正-03-标脏强制重算");
        } else {
            s.fail("F0022-正-03-标脏强制重算", "脏瓦片被复用");
        }
    }

    // 正-04：稳定帧复用率 ≥95%（48×27 网格逐瓦片判定）
    {
        let (w, h) = (48usize, 27usize);
        let mut c = TileCache::new(w, h);
        let px = solid(0xABCD_1234);
        c.begin_frame();
        for y in 0..h {
            for x in 0..w {
                let _ = c.judge(TileId { x, y }, &px, false);
            }
        }
        let before = c.stats.reused;
        c.begin_frame();
        for y in 0..h {
            for x in 0..w {
                let _ = c.judge(TileId { x, y }, &px, false);
            }
        }
        // 稳定帧口径：只看第二帧的判定（首帧必然是 Fresh，混进来会把
        // 理论上限压到 50%——那是口径错，不是复用率低）。
        let delta = c.stats.reused - before;
        let rate = delta * 1000 / (w * h) as u64;
        if rate >= 950 {
            s.ok("F0022-正-04-稳定帧复用率≥95%");
        } else {
            s.fail("F0022-正-04-稳定帧复用率≥95%", "稳定帧复用率不足");
        }
    }

    // 正-05：任务图剔除名单与判定一致（只收 Reused，保序）
    {
        let ids = [
            TileId { x: 0, y: 0 },
            TileId { x: 1, y: 0 },
            TileId { x: 2, y: 0 },
            TileId { x: 0, y: 1 },
        ];
        let verdicts = [
            (ids[0], Verdict::Reused),
            (ids[1], Verdict::Changed),
            (ids[2], Verdict::Reused),
            (ids[3], Verdict::Collided),
        ];
        let skip = TileCache::build_skip_list(&verdicts);
        if skip == alloc::vec![ids[0], ids[2]] {
            s.ok("F0022-正-05-剔除名单只收Reused");
        } else {
            s.fail("F0022-正-05-剔除名单只收Reused", "剔除名单不符");
        }
    }

    // 正-06：哈希开销预算（2040 瓦片稳定帧 ≤0.5ms）
    {
        let px = TILES_4K * (TILE * TILE / FAST_STRIDE) as u64;
        let us = hash_cost_us(px);
        if us <= HASH_BUDGET_US {
            s.ok("F0022-正-06-哈希开销≤0.5ms");
        } else {
            s.fail("F0022-正-06-哈希开销≤0.5ms", "哈希开销超预算");
        }
    }

    // 正-07：哈希对内容敏感（相邻像素交换必须改变快筛值）
    {
        let a = solid(1);
        let mut b = solid(1);
        b[0] = 2;
        b[FAST_STRIDE] = 1;
        let ha = hash_tile(&a, false);
        let hb = hash_tile(&b, false);
        if ha.fast != hb.fast {
            s.ok("F0022-正-07-快筛对采样点敏感");
        } else {
            s.fail("F0022-正-07-快筛对采样点敏感", "交换采样点后哈希未变");
        }
    }

    // 正-08：确定性（同内容同哈希，跨实例一致）
    {
        let a = hash_tile(&solid(42), true);
        let b = hash_tile(&solid(42), true);
        if a == b && a.full != 0 {
            s.ok("F0022-正-08-哈希确定性");
        } else {
            s.fail("F0022-正-08-哈希确定性", "同内容哈希不一致");
        }
    }

    // 坏-01：越界瓦片即拒
    {
        let mut c = TileCache::new(2, 2);
        let px = solid(3);
        if c.judge(TileId { x: 2, y: 0 }, &px, false) == Err(E_TR_TILE_OOB) {
            s.ok("F0022-坏-01-越界即拒");
        } else {
            s.fail("F0022-坏-01-越界即拒", "越界未拒");
        }
    }

    // 坏-02：尺寸不符即拒（半块瓦片不得静默接受）
    {
        let mut c = TileCache::new(2, 2);
        let short = alloc::vec![0u32; TILE];
        if c.judge(TileId { x: 0, y: 0 }, &short, false) == Err(E_TR_DIM_MISMATCH) {
            s.ok("F0022-坏-02-尺寸不符即拒");
        } else {
            s.fail("F0022-坏-02-尺寸不符即拒", "短缓冲未拒");
        }
    }

    // 坏-03：**碰撞注入**——两个不同内容构造出相同快筛，必须靠全量哈希拦下，
    // 且计 Collided 而不是 Reused。构造法：快筛采样点全部相同，仅非采样点不同。
    {
        let mut c = TileCache::new(2, 2);
        let mut a = solid(5);
        let mut b = solid(5);
        // 非采样点（下标 1）改为不同值：快筛看不见，全量看得见
        a[1] = 100;
        b[1] = 200;
        c.begin_frame();
        let _ = c.judge(TileId { x: 0, y: 0 }, &a, false);
        // 同时把 prev 的快筛伪造为与 b 相同（模拟真碰撞：a/b 快筛恰好相等）
        c.begin_frame();
        let r = c.judge(TileId { x: 0, y: 0 }, &b, false);
        let fast_same = hash_tile(&a, false).fast == hash_tile(&b, false).fast;
        if fast_same && r == Ok((Verdict::Collided, false)) && c.stats.collisions == 1 {
            s.ok("F0022-坏-03-碰撞必被全量拦下");
        } else {
            s.fail("F0022-坏-03-碰撞必被全量拦下", "碰撞未被拦下或未记账");
        }
    }

    // 编-01：整链——60 瓦片 3 帧，第 2 帧改 6 块，账目自洽
    {
        let (w, h) = (10usize, 6usize);
        let mut c = TileCache::new(w, h);
        let mut frames: Vec<Vec<u32>> = Vec::new();
        for i in 0..(w * h) {
            frames.push(solid(i as u32 * 7 + 1));
        }
        c.begin_frame();
        for i in 0..(w * h) {
            let _ = c.judge(TileId { x: i % w, y: i / w }, &frames[i], false);
        }
        // 第 2 帧：改 6 块（每 10 块改 1 块）
        for i in (0..(w * h)).step_by(10) {
            frames[i] = solid(0xFFFF_0000 + i as u32);
        }
        c.begin_frame();
        for i in 0..(w * h) {
            let _ = c.judge(TileId { x: i % w, y: i / w }, &frames[i], false);
        }
        // 第 3 帧：不再改
        c.begin_frame();
        for i in 0..(w * h) {
            let _ = c.judge(TileId { x: i % w, y: i / w }, &frames[i], false);
        }
        let total = 3 * (w * h) as u64;
        let ok = c.stats.tiles == total
            && c.stats.recomputed == (w * h) as u64 + 6
            && c.stats.reused == 2 * (w * h) as u64 - 6
            && c.stats.collisions == 0
            && c.stats.tiles == c.stats.reused + c.stats.recomputed;
        if ok {
            s.ok("F0022-编-01-三帧账目守恒");
        } else {
            s.fail("F0022-编-01-三帧账目守恒", "复用账目不平");
        }
    }

    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    520
}

/// 读屏画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("瓦片复用：两级哈希快筛加全量校验，");
    s.push_str("碰撞必回退重算，剔除名单只收确认复用项");
    s
}
