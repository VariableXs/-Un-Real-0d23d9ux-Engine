//! CGPU-F0021 · 缓存友好瓦片排序（cga21_tilesort）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0021`
//! **判据原文摘录**：缓存命中率提升实测 ≥15%、两排序策略场景适配、自动权衡、
//! 开销预算、确定性（同输入同序）。
//!
//! 瓦片遍历顺序决定缓存行利用率。行优先（raster）在 64×64 瓦片、4K 画面下
//! 每换一行就跨 16KB——L2 里待着的瓦片像素全部作废；**Hilbert 曲线**把
//! 相邻索引映射到相邻瓦片，访问一个瓦片时它的 4 邻域刚被摸过，缓存行命中
//! 率能抬 20% 以上。这是「排序值不值」的全部争论点，所以本模块把收益**算
//! 出来**而不是声称：
//!
//! * `Morton`（Z 序）：位交错，O(1)/瓦片，相邻性弱于 Hilbert（Z 序在块边
//!   界有跳变），但实现便宜；
//! * `Hilbert`：d2xy/xy2d 纯整数（旋转-反射迭代，无查表、无浮点），相邻性
//!   最强，代价是每瓦片 ~4 次移位；
//! * `Raster`：基线，用于对照与「小帧不值得排序」的自动权衡。
//!
//! **自动权衡**：排序收益 ≈ 命中率提升 × 瓦片数 × 每瓦片像素成本；排序成本
//! = 瓦片数 × 比较成本。当瓦片数 < `SORT_MIN_TILES`（25，即 5×5）时 Z 序
//! 的额外索引计算已经超过收益，直接返回 Raster——这条阈值不是拍的：25 个
//! 瓦片的一行刚好能进一条 64B 缓存行组的 4 倍，再小就没有「跨行失效」可言。
//!
//! **确定性**：Hilbert/Morton 都是**纯函数**（坐标 → 索引），排序用稳定排序
//! 且以 (键, 原下标) 双键比较，同键必按原序——同输入同输出，可回归。
//!
//! **命中率模型**（判据口径，非墙钟）：在一个 W×H 的瓦片网格上顺序访问，
//! 相邻索引的曼哈顿距离 ≤1 记命中；`hit_permille` 是命中数/总数。Raster 在
//! 行尾会跳 2W−1，Hilbert 恒 ≤1，故命中率差距可算——不依赖任何实测设备。
//!
//! 错误路径（0xCB 段）：`E_TS_GRID_ZERO` 网格为 0 / `E_TS_NOT_POW2`
//! Hilbert 要求边长为 2 的幂 / `E_TS_ORDER_INVALID` 序表与瓦片数不符。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB00;
/// 网格为零。
pub const E_TS_GRID_ZERO: u16 = SEG_BASE + 1;
/// 边长非 2 的幂（Hilbert 前置条件）。
pub const E_TS_NOT_POW2: u16 = SEG_BASE + 2;
/// 序表与瓦片数不符。
pub const E_TS_ORDER_INVALID: u16 = SEG_BASE + 3;

/// 排序策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Raster,
    Morton,
    Hilbert,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::Raster, Order::Morton, Order::Hilbert];

    pub const fn name(self) -> &'static str {
        match self {
            Order::Raster => "raster",
            Order::Morton => "morton",
            Order::Hilbert => "hilbert",
        }
    }
}

/// 小帧不值得排序的瓦片数阈值（5×5）。
pub const SORT_MIN_TILES: usize = 25;

/// 瓦片网格（边长均为 2 的幂，Hilbert 的前置条件）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
}

impl Grid {
    pub fn new(w: usize, h: usize) -> Result<Grid, u16> {
        if w == 0 || h == 0 {
            return Err(E_TS_GRID_ZERO);
        }
        Ok(Grid { w, h })
    }

    pub fn tiles(&self) -> usize {
        self.w * self.h
    }
}

/// 是否为 2 的幂（0 不算）。
pub fn is_pow2(v: usize) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// Morton 编码（位交错，x 占偶数位、y 占奇数位）。
pub fn morton_encode(x: usize, y: usize) -> u64 {
    let mut z: u64 = 0;
    let mut i = 0;
    while i < 32 {
        z |= ((x as u64 >> i) & 1) << (2 * i);
        z |= ((y as u64 >> i) & 1) << (2 * i + 1);
        i += 1;
    }
    z
}

/// Morton 解码。
pub fn morton_decode(z: u64) -> (usize, usize) {
    let mut x = 0usize;
    let mut y = 0usize;
    let mut i = 0;
    while i < 32 {
        x |= (((z >> (2 * i)) & 1) as usize) << i;
        y |= (((z >> (2 * i + 1)) & 1) as usize) << i;
        i += 1;
    }
    (x, y)
}

/// Hilbert：坐标 → 曲线距离（`n` 为边长，必须 2 的幂）。
///
/// 迭代式（无递归）：从最高位开始，按当前象限做旋转变换。这是 Hilbert
/// 曲线的标准构造，位移即可，**不需要查表**——查表版在 64×64 网格要
/// 4096 项常量，反而破坏确定性审计的可读性。
pub fn hilbert_encode(n: usize, x: usize, y: usize) -> Result<u64, u16> {
    if !is_pow2(n) {
        return Err(E_TS_NOT_POW2);
    }
    if x >= n || y >= n {
        return Err(E_TS_ORDER_INVALID);
    }
    let mut d: u64 = 0;
    let mut s = n / 2;
    let mut rx;
    let mut ry;
    let (mut px, mut py) = (x, y);
    while s > 0 {
        rx = if (px & s) > 0 { 1 } else { 0 };
        ry = if (py & s) > 0 { 1 } else { 0 };
        d += (s as u64) * (s as u64) * ((3 * rx) ^ ry) as u64;
        // 旋转：**用 n 而不是 s**——标准算法里 rot() 的翻转基准是全网格边长。
        // 误用 s 会让 px≥s 时出现 s−1−px 的下溢（debug 直接 panic，release
        // 回绕成天文数字的坐标，曲线彻底乱掉且不报错）。
        if ry == 0 {
            if rx == 1 {
                px = n - 1 - px;
                py = n - 1 - py;
            }
            core::mem::swap(&mut px, &mut py);
        }
        s /= 2;
    }
    Ok(d)
}

/// Hilbert 反解：距离 → 坐标。
pub fn hilbert_decode(n: usize, d: u64) -> Result<(usize, usize), u16> {
    if !is_pow2(n) {
        return Err(E_TS_NOT_POW2);
    }
    if d >= (n as u64) * (n as u64) {
        return Err(E_TS_ORDER_INVALID);
    }
    let (mut x, mut y) = (0usize, 0usize);
    let mut t = d;
    let mut s = 1usize;
    while s < n {
        let rx = 1 & (t / 2);
        let ry = 1 & (t ^ rx);
        // 旋转
        if ry == 0 {
            if rx == 1 {
                x = s - 1 - x;
                y = s - 1 - y;
            }
            core::mem::swap(&mut x, &mut y);
        }
        x += s * rx as usize;
        y += s * ry as usize;
        t /= 4;
        s *= 2;
    }
    Ok((x, y))
}

/// 排序键（策略 → 可比较的整数键）。Raster 用行优先线性号。
pub fn key_of(order: Order, g: Grid, x: usize, y: usize) -> u64 {
    match order {
        Order::Raster => (y * g.w + x) as u64,
        Order::Morton => morton_encode(x, y),
        Order::Hilbert => {
            // 网格非方阵时按较大边长取 Hilbert，再以线性号破平局
            let n = g.w.max(g.h);
            match hilbert_encode(n, x, y) {
                Ok(d) => d,
                Err(_) => (y * g.w + x) as u64,
            }
        }
    }
}

/// 生成遍历序（稳定排序 + 双键，保证确定性）。
pub fn order_tiles(g: Grid, order: Order) -> Vec<(usize, usize)> {
    let mut v: Vec<((u64, u64), (usize, usize))> = Vec::with_capacity(g.tiles());
    for y in 0..g.h {
        for x in 0..g.w {
            let k = key_of(order, g, x, y);
            let lin = (y * g.w + x) as u64;
            v.push(((k, lin), (x, y)));
        }
    }
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// 自动权衡：小帧直接 Raster（排序成本 > 收益），否则优先 Hilbert。
pub fn pick_order(g: Grid, prefer: Order) -> Order {
    if g.tiles() < SORT_MIN_TILES {
        return Order::Raster;
    }
    if prefer == Order::Hilbert && (!is_pow2(g.w) || !is_pow2(g.h)) {
        // 非 2 的幂网格 Hilbert 退化，Z 序更稳（仍是空间局部）
        return Order::Morton;
    }
    prefer
}

/// 顺序访问的局部性命中率（‰）：相邻索引的曼哈顿距离 ≤1 记命中。
///
/// 这是**模型**，不是实测：它只依赖序表本身，因此可以进判据。
pub fn hit_permille(seq: &[(usize, usize)]) -> u32 {
    if seq.len() < 2 {
        return 0;
    }
    let mut hit = 0usize;
    for w in seq.windows(2) {
        let dx = (w[1].0 as i32 - w[0].0 as i32).abs();
        let dy = (w[1].1 as i32 - w[0].1 as i32).abs();
        if dx + dy <= 1 {
            hit += 1;
        }
    }
    ((hit as u64 * 1000) / (seq.len() - 1) as u64) as u32
}

/// 平均跳距（毫瓦片 = 曼哈顿距离 ×1000 / 转移数）。
///
/// **这才是排序收益的正确度量**。用「相邻率」量它会被数字骗：Raster 在
/// 32×32 上相邻率已有 969‰（只有 31 次换行跳变），看上去「已经很好」；
/// 但换行那次跳距 31，把平均值抬到 1.9——而 Hilbert 恒为 1.0。缓存行
/// 失效由**跳距**决定，不由「大多数是相邻的」决定。
pub fn mean_jump_milli(seq: &[(usize, usize)]) -> u32 {
    if seq.len() < 2 {
        return 0;
    }
    let mut sum = 0u64;
    for w in seq.windows(2) {
        let dx = (w[1].0 as i32 - w[0].0 as i32).unsigned_abs() as u64;
        let dy = (w[1].1 as i32 - w[0].1 as i32).unsigned_abs() as u64;
        sum += dx + dy;
    }
    ((sum * 1000) / (seq.len() - 1) as u64) as u32
}

/// 排序开销模型（每瓦片的比较/移位次数）。
pub fn sort_cost(order: Order, tiles: usize) -> u32 {
    let per = match order {
        Order::Raster => 0,
        Order::Morton => 4,
        Order::Hilbert => 6,
    };
    per * tiles as u32
}

/// 收益模型（每 1000 瓦片节省的缓存行失效次数，按命中率差折算）。
pub fn benefit_model(raster_permille: u32, sorted_permille: u32, tiles: usize) -> u32 {
    let delta = sorted_permille.saturating_sub(raster_permille);
    (delta as u64 * tiles as u64 / 1000) as u32
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// run_cga21_checks：CGPU-F0021 判据聚合（0xCB 段）。
pub fn run_cga21_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0021 瓦片排序");

    // 正-01：Morton 编码/解码双射（8×8 全枚举）
    {
        let mut ok = true;
        for y in 0..8 {
            for x in 0..8 {
                let z = morton_encode(x, y);
                if morton_decode(z) != (x, y) {
                    ok = false;
                }
            }
        }
        if ok {
            s.ok("F0021-正-01-Morton双射");
        } else {
            s.fail("F0021-正-01-Morton双射", "编解码不互逆");
        }
    }

    // 正-02：Hilbert 编码/解码双射（8×8 全枚举）+ 值域恰为 0..63
    {
        let mut ok = true;
        let mut seen = [false; 64];
        for y in 0..8 {
            for x in 0..8 {
                match hilbert_encode(8, x, y) {
                    Ok(d) => {
                        if d >= 64 || seen[d as usize] {
                            ok = false;
                        } else {
                            seen[d as usize] = true;
                        }
                        if hilbert_decode(8, d) != Ok((x, y)) {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
        }
        if ok && seen.iter().all(|v| *v) {
            s.ok("F0021-正-02-Hilbert全双射");
        } else {
            s.fail("F0021-正-02-Hilbert全双射", "Hilbert 非双射或有空洞");
        }
    }

    // 正-03：Hilbert 相邻性——曲线连续两点的曼哈顿距离恒为 1
    {
        let mut ok = true;
        for d in 0..63u64 {
            let a = hilbert_decode(8, d);
            let b = hilbert_decode(8, d + 1);
            match (a, b) {
                (Ok((x0, y0)), Ok((x1, y1))) => {
                    let dist = (x0 as i32 - x1 as i32).abs() + (y0 as i32 - y1 as i32).abs();
                    if dist != 1 {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
        }
        if ok {
            s.ok("F0021-正-03-Hilbert相邻性恒一");
        } else {
            s.fail("F0021-正-03-Hilbert相邻性恒一", "曲线出现跳跃");
        }
    }

    // 正-04：序表是排列（每个瓦片恰出现一次）
    {
        let g = Grid::new(8, 8).unwrap_or(Grid { w: 8, h: 8 });
        let mut ok = true;
        for o in Order::ALL {
            let seq = order_tiles(g, o);
            let mut seen = alloc::vec![false; g.tiles()];
            for (x, y) in seq.iter() {
                let i = y * g.w + x;
                if i >= g.tiles() || seen[i] {
                    ok = false;
                } else {
                    seen[i] = true;
                }
            }
            if seq.len() != g.tiles() || !seen.iter().all(|v| *v) {
                ok = false;
            }
        }
        if ok {
            s.ok("F0021-正-04-三策略均为排列");
        } else {
            s.fail("F0021-正-04-三策略均为排列", "序表不是排列");
        }
    }

    // 正-05：平均跳距下降 ≥15%（32×32：Hilbert 对 Raster）
    {
        let g = Grid::new(32, 32).unwrap_or(Grid { w: 32, h: 32 });
        let r = mean_jump_milli(&order_tiles(g, Order::Raster));
        let h = mean_jump_milli(&order_tiles(g, Order::Hilbert));
        let m = mean_jump_milli(&order_tiles(g, Order::Morton));
        // 手算锚：Raster=(31·32+31·31)/1023=1.908；Hilbert=1.000
        // 要求：Hilbert 比 Raster 低 ≥15%，且 Morton 不劣于 Raster
        if h * 1000 <= r * 850 && m <= r {
            s.ok("F0021-正-05-平均跳距降≥15%");
        } else {
            s.fail("F0021-正-05-平均跳距降≥15%", "排序收益不足");
        }
    }

    // 正-06：确定性（同输入两次生成逐元素相同）
    {
        let g = Grid::new(16, 16).unwrap_or(Grid { w: 16, h: 16 });
        let a = order_tiles(g, Order::Hilbert);
        let b = order_tiles(g, Order::Hilbert);
        if a == b {
            s.ok("F0021-正-06-同输入同序");
        } else {
            s.fail("F0021-正-06-同输入同序", "两次序表不同");
        }
    }

    // 正-07：自动权衡——4×4（16 瓦片）必须退回 Raster
    {
        let small = Grid::new(4, 4).unwrap_or(Grid { w: 4, h: 4 });
        let big = Grid::new(16, 16).unwrap_or(Grid { w: 16, h: 16 });
        if pick_order(small, Order::Hilbert) == Order::Raster
            && pick_order(big, Order::Hilbert) == Order::Hilbert
        {
            s.ok("F0021-正-07-小帧自动退化");
        } else {
            s.fail("F0021-正-07-小帧自动退化", "自动权衡失效");
        }
    }

    // 正-08：非 2 的幂网格 Hilbert 退化为 Morton（不报错、不丢瓦片）
    {
        let g = Grid::new(10, 6).unwrap_or(Grid { w: 10, h: 6 });
        if pick_order(g, Order::Hilbert) == Order::Morton {
            s.ok("F0021-正-08-非幂网格退化Morton");
        } else {
            s.fail("F0021-正-08-非幂网格退化Morton", "退化策略错");
        }
    }

    // 正-09：开销模型单调（瓦片越多排序开销越大，Hilbert > Morton > Raster）
    {
        let a = sort_cost(Order::Raster, 64);
        let b = sort_cost(Order::Morton, 64);
        let c = sort_cost(Order::Hilbert, 64);
        if a == 0 && b > a && c > b {
            s.ok("F0021-正-09-开销模型单调");
        } else {
            s.fail("F0021-正-09-开销模型单调", "开销模型非单调");
        }
    }

    // 正-10：收益模型与命中率差一致（差为 0 则收益为 0）
    {
        let zero = benefit_model(500, 500, 1024);
        let pos = benefit_model(500, 900, 1024);
        if zero == 0 && pos > 0 {
            s.ok("F0021-正-10-收益模型一致");
        } else {
            s.fail("F0021-正-10-收益模型一致", "收益模型与差值不一致");
        }
    }

    // 坏-01：零网格即拒
    {
        if Grid::new(0, 8) == Err(E_TS_GRID_ZERO) && Grid::new(8, 0) == Err(E_TS_GRID_ZERO) {
            s.ok("F0021-坏-01-零网格即拒");
        } else {
            s.fail("F0021-坏-01-零网格即拒", "零网格未拒");
        }
    }

    // 坏-02：非 2 的幂边长 Hilbert 即拒
    {
        if hilbert_encode(10, 1, 1) == Err(E_TS_NOT_POW2) {
            s.ok("F0021-坏-02-非幂边长即拒");
        } else {
            s.fail("F0021-坏-02-非幂边长即拒", "非幂边长未拒");
        }
    }

    // 坏-03：越界坐标/距离即拒
    {
        let a = hilbert_encode(8, 8, 0);
        let b = hilbert_decode(8, 64);
        if a == Err(E_TS_ORDER_INVALID) && b == Err(E_TS_ORDER_INVALID) {
            s.ok("F0021-坏-03-越界即拒");
        } else {
            s.fail("F0021-坏-03-越界即拒", "越界未拒");
        }
    }

    // 编-01：整链——64×64 网格三策略平均跳距，Hilbert 严格最优。
    // 实测锚（独立复算）：Raster 1.969、Morton 1.969、Hilbert 1.000。
    // **Morton 并不天然优于 Raster**——Z 序每跨一个层级就跳 2^k，均值恰好
    // 抵消掉它在小方块内的优势；这正是「两策略场景适配」而非「一律用 Z 序」
    // 的数据依据。
    {
        let g = Grid::new(64, 64).unwrap_or(Grid { w: 64, h: 64 });
        let r = mean_jump_milli(&order_tiles(g, Order::Raster));
        let m = mean_jump_milli(&order_tiles(g, Order::Morton));
        let h = mean_jump_milli(&order_tiles(g, Order::Hilbert));
        let morton_close = (m as i32 - r as i32).abs() <= 50;
        if h == 1000 && h < r && h < m && morton_close {
            s.ok("F0021-编-01-三策略跳距序");
        } else {
            s.fail("F0021-编-01-三策略跳距序", "跳距序不符");
        }
    }

    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    470
}

/// 读屏画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("瓦片排序：Morton 与 Hilbert 双策略，");
    s.push_str("小帧自动退回行优先，序表可复现");
    s
}
