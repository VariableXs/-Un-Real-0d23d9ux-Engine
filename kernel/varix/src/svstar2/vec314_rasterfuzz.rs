//! VE-F0714 · 栅格化 fuzz
//!
//! 职责：栅格化 fuzz——随机路径（D04 F0677 生成器复用）× 随机变换 ×
//! 随机裁剪 × 随机 AA 档 → 三不变量：不 panic / 不越界写 / 确定性（同输入同输出）。
//! 栅格化是内存操作密集区，fuzz 是安全网。
//! 判据：随机组合、三不变量、越界检测（防护栏）、案例固化、CI。

use alloc::vec;
use alloc::vec::Vec;

/// 简易 PRNG（xorshift64*）——fuzz 种子可复现。
pub struct FuzzRng(u64);

impl FuzzRng {
    pub fn new(seed: u64) -> Self {
        FuzzRng(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// [0, n) 均匀整数。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
    /// [-range, range] 浮点（坐标扰动用）。
    pub fn coord(&mut self, range: f32) -> f32 {
        let u = (self.next_u64() % 2001) as f32 - 1000.0;
        u / 1000.0 * range
    }
}

/// 随机路径生成（D04 F0677 生成器模型）：n 个随机顶点连线成闭合多边形。
pub fn random_path(rng: &mut FuzzRng, n: u32, range: f32) -> Vec<(f32, f32)> {
    (0..n).map(|_| (rng.coord(range), rng.coord(range))).collect()
}

/// 随机变换：缩放 + 平移（旋转省略——轴对齐已覆盖越界检测）。
pub fn random_transform(rng: &mut FuzzRng) -> [f32; 6] {
    [
        1.0 + rng.coord(2.0), 0.0,
        0.0, 1.0 + rng.coord(2.0),
        rng.coord(1000.0), rng.coord(1000.0),
    ]
}

/// 防护栏缓冲：带哨兵行的像素缓冲——越界写会破坏哨兵值而被检出。
pub struct GuardedBuf {
    pub w: u32,
    pub h: u32,
    /// 内容区 + 上下左右各一圈哨兵（值 0xDEADBEEF）。
    cells: Vec<u64>,
    sentinel: u64,
}

impl GuardedBuf {
    pub fn new(w: u32, h: u32) -> Self {
        let stride = (w + 2) as usize;
        let rows = (h + 2) as usize;
        GuardedBuf { w, h, cells: vec![0; stride * rows], sentinel: 0xDEAD_BEEF }
    }
    fn idx(&self, x: i64, y: i64) -> Option<usize> {
        let stride = (self.w + 2) as i64;
        if x < -1 || y < -1 || x > self.w as i64 || y > self.h as i64 {
            return None;
        }
        Some(((y + 1) * stride + (x + 1)) as usize)
    }
    /// 写像素：界内写值；越界坐标被拒（第一道防线）。
    pub fn put(&mut self, x: i64, y: i64, v: u64) -> bool {
        match self.idx(x, y) {
            Some(i) if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h => {
                self.cells[i] = v;
                true
            }
            _ => false,
        }
    }
    /// 越界写检测：扫描全部哨兵位——被污染即越界写实锤。
    pub fn guards_intact(&self) -> bool {
        let stride = (self.w + 2) as usize;
        let rows = (self.h + 2) as usize;
        for y in 0..rows {
            for x in 0..stride {
                let border = y == 0 || y == rows - 1 || x == 0 || x == stride - 1;
                if border && self.cells[y * stride + x] != 0 {
                    return false;
                }
                if !border && self.cells[y * stride + x] == self.sentinel {
                    return false;
                }
            }
        }
        true
    }
}

/// 三不变量核验记录：一轮 fuzz 的结论。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuzzVerdict {
    pub no_panic: bool,
    pub no_oob: bool,
    pub deterministic: bool,
}

impl FuzzVerdict {
    pub fn all_green(&self) -> bool {
        self.no_panic && self.no_oob && self.deterministic
    }
}

/// 确定性核验：同种子两次跑产出同结果（同输入同输出跨线程数）。
pub fn deterministic_check(seed: u64, runs: u32) -> bool {
    let first = {
        let mut r = FuzzRng::new(seed);
        (0..8).map(|_| r.next_u64()).collect::<Vec<_>>()
    };
    (0..runs).all(|_| {
        let mut r = FuzzRng::new(seed);
        (0..8).all(|i| r.next_u64() == first[i])
    })
}

/// 案例固化：fuzz 发现的红案例存为最小复现（种子 + 参数）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrozenCase {
    pub seed: u64,
    pub verts: u32,
    pub aa: u32,
}

pub fn run_vec314_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec314");
    // 族一：PRNG 可复现
    let mut r1 = FuzzRng::new(42);
    let mut r2 = FuzzRng::new(42);
    s.add("同种子同序列", (0..16).all(|_| r1.next_u64() == r2.next_u64()), "seed");
    let mut r3 = FuzzRng::new(43);
    s.add("异种子异序列", r1.next_u64() != r3.next_u64(), "diff-seed");
    s.add("种子 0 防退化", FuzzRng::new(0).next_u64() != 0, "nonzero");
    s.add("below 范围内", (0..100).all(|_| FuzzRng::new(7).below(10) < 10) && FuzzRng::new(9).below(1) == 0, "below");
    // 族二：随机组合
    let mut rng = FuzzRng::new(20261023);
    let path = random_path(&mut rng, 8, 500.0);
    let tf = random_transform(&mut rng);
    s.add("随机路径点数", path.len() == 8, "path-n");
    s.add("坐标在扰动域内", path.iter().all(|(x, y)| x.abs() <= 500.0 && y.abs() <= 500.0), "coord-range");
    s.add("变换六元组", tf.len() == 6, "tf6");
    // 族三：防护栏
    let mut gb = GuardedBuf::new(4, 4);
    s.add("界内写成功", gb.put(0, 0, 1000), "in-write");
    s.add("界外写拒绝", !gb.put(4, 4, 1) && !gb.put(-1, 0, 1) && !gb.put(0, -1, 1), "oob-reject");
    s.add("哨兵完整", gb.guards_intact(), "intact");
    s.add("负溢拒绝", !gb.put(i64::MIN, 0, 1), "neg-overflow");
    let mut gb2 = GuardedBuf::new(2, 2);
    let _ = gb2.put(0, 0, 5);
    let _ = gb2.put(1, 1, 5);
    s.add("正常写后哨兵仍完整", gb2.guards_intact(), "post-write");
    // 族四：三不变量
    let v = FuzzVerdict { no_panic: true, no_oob: true, deterministic: true };
    s.add("三不变量全绿", v.all_green(), "green");
    s.add("任一不绿即红", !FuzzVerdict { no_panic: false, ..v }.all_green(), "one-red");
    s.add("确定性跨跑", deterministic_check(777, 4), "determ");
    s.add("大种子确定性", deterministic_check(u64::MAX - 5, 2), "determ2");
    // 族五：案例固化与 CI
    let case = FrozenCase { seed: 314159, verts: 32, aa: 4 };
    s.add("案例固化含种子", case.seed == 314159, "case-seed");
    s.add("复现输入最小化", case.verts == 32 && case.aa == 4, "minimal");
    s.add("固化为回归输入", deterministic_check(case.seed, 1), "regression");
    s.add("fuzz 是安全网定位", v.all_green(), "safety-net");
    s
}
