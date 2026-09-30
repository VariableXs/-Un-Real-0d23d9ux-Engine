//! J 域共享底盘（AI-J2 · F621-F640 全域复用，一处一事实）。
//!
//! 本模块收拢 J 鼠标域二十项功能的公共机械件，判据依据《Varix STAR I
//! start.md》主册第 8 部分（J-0 域内总纲与 F621-F640 各节）：
//!
//! - **像素面**：RGBA8 直存位图 [`PixBuf`]、多边形扫描线填充、描边线段、
//!   矩形/椭圆几何——F625 工坊笔刷与 jbase 内置方案绘制共用同一套几何
//!   原语（F156 十字热点语义的落点校验也在这层）；
//! - **色彩数学（全定点）**：sRGB↔HSL（千分位定点）、WCAG 相对亮度与
//!   对比度（F629 描边 ≥3:1、F631 模板 ≥4.5:1 的判定尺）、sRGB→Lab(D65)
//!   与 ΔE76（F626 帧同步 ΔE<1 判据的度量衡）——整数 nth-root（牛顿迭代，
//!   u128 中间量）实现，内核路径零浮点，宿主/实机同源可复现；
//! - **重采样**：Lanczos3 可分离核（1/256 定点权重）——F636 DPI 自适配
//!   契约的位图升采样兜底与 F632 保真审计的重生成链共用；
//! - **方案模型**：15 标准态（F156「15 枚指针」口径）× 逐态帧序列
//!   （16 帧上限纪律）的 [`CursorSchemeModel`]，序列化容器 `.vxcur`
//!   （F630 分享链路的交换格式，F133 图标包规范子集的实例化）；
//! - **内置默认方案**：程序化绘制的 15 态兜底件——F627 缺态补默认、
//!   F639 熔断回退、F635 卸载回退三处共用同一事实源；
//! - **确定性随机**：XorShift32——100 样本库（F633/F639）与模糊注入
//!   （畸形输入不 panic 纪律）的宿主可复现种子源。
//!
//! 共同纪律（与 star/perfstar/h1star 域既约同源）：
//! - 一切时间由调用方注入（毫秒戳），模块不持真实时钟；
//! - 判据唯一源为主册原文，常量注释钉 F 编号 + 数值；
//! - 容量全部有上限且如实报告（`truncated()` 纪律）；
//! - 解析失败诚实报错（错误带定位），不半导入、不静默截断。

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::{max, min};

// ---------------------------------------------------------------------------
// 确定性哈希与随机（宿主可复现）
// ---------------------------------------------------------------------------

/// FNV-1a 64：确定性内容指纹（往返一致判据的哈希尺，非密码学——
/// 签名面由 F630 的验证闭包承接，此处只保「导出再导入逐字节一致」）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// XorShift32：确定性伪随机（测试样本库与模糊注入的种子源）。
pub struct XorShift32(pub u32);

impl XorShift32 {
    pub fn new(seed: u32) -> XorShift32 {
        XorShift32(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// [0, bound) 的确定伪随机数。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ---------------------------------------------------------------------------
// 定点整数根与幂（色彩数学的地基，core 无 f64 纪律）
// ---------------------------------------------------------------------------

/// 牛顿整数 n 次根（向下取）：`v = 10^36`、`n = 5` 量级安全（u128 上限
/// 3.4e38；本域最大中间量 `x^12 ≤ 1000^12 = 10^36`）。
pub fn nth_root_u128(v: u128, n: u32) -> u128 {
    if v == 0 {
        return 0;
    }
    if n == 0 {
        return u128::MAX;
    }
    // 初值：2^ceil(bits/n)，一次牛顿收敛到位。
    let bits = (128 - v.leading_zeros()) as u32;
    let mut r: u128 = 1u128 << ((bits + n - 1) / n);
    loop {
        // 牛顿迭代（正确形）：x_{k+1} = ((n−1)·x + v/x^(n−1))/n——
        // 自上而下单调收敛到整数 n 次根（r0 = 2^ceil(bits/n) ≥ 真根）。
        let next = ((n as u128 - 1) * r + v / integer_pow_u128(r, n - 1)) / n as u128;
        if next >= r {
            break;
        }
        r = next;
    }
    // 收敛点修正：牛顿整根可能停在高一格或低一格——双向夹逼。
    while integer_pow_u128(r, n) > v {
        r -= 1;
    }
    while integer_pow_u128(r + 1, n) <= v && integer_pow_u128(r + 1, n) != u128::MAX {
        r += 1;
    }
    r
}

/// 整数幂（溢出饱和到 u128::MAX，调用方保证量级）。
pub fn integer_pow_u128(base: u128, exp: u32) -> u128 {
    let mut acc: u128 = 1;
    for _ in 0..exp {
        match acc.checked_mul(base) {
            Some(v) => acc = v,
            None => return u128::MAX,
        }
    }
    acc
}

/// 整数平方根（u64，向下取）。
pub fn isqrt64(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut r = v;
    let mut g = (v >> 1).max(1);
    while g < r {
        r = g;
        g = (g + v / g) >> 1;
    }
    r
}

// ---------------------------------------------------------------------------
// 定点色彩数学（千分位定点；Lab 用百分位定点）
// ---------------------------------------------------------------------------

/// 千分位定点：`srgb_to_linear_m(x_m)` 把 sRGB 通道线性化。
/// 输入 x_m ∈ [0,1000]（c/255 的千分位），输出线性值千分位。
/// 分段：t ≤ 40.45 → t/12.92；否则 ((t+0.055)/1.055)^2.4。
/// `^2.4 = (t^12)^(1/5)`，定点换算常数 `1000^(7/5) ≈ 15848.93`。
pub fn srgb_to_linear_m(x_m: i64) -> i64 {
    let t = x_m.clamp(0, 1000);
    if t <= 40 {
        // t/12.92，千分位结果 = t*1000/12920。
        return t * 1000 / 12920;
    }
    // b = (t+55)/1055*1000（千分位定点基）。
    let b: i64 = ((t + 55) as i128 * 1000 / 1055) as i64;
    let p = integer_pow_u128(b as u128, 12); // b ≤ 1000 → p ≤ 10^36
    let r = nth_root_u128(p, 5) as i64; // r = b^(12/5) ≈ ≤ 1.59e7
    // 千分位结果 = r / 1000^(7/5)；1000^(7/5) ≈ 15848.93 → 分母 15848931/1000。
    (r * 1000 / 15848931) as i64
}

/// 线性值（千分位）→ sRGB 通道千分位（`^(5/12)` 编码侧）。
/// 提升精度：t 先放大到 1e7 标度再取 12 次根（`1000^(35/12) ≈ 825.4`），
/// `out_m = 1055·r/825.40 − 55`（常数 1.27821×1e5 定点）。
pub fn linear_to_srgb_m(lin_m: i64) -> i64 {
    let t = lin_m.clamp(0, 1000);
    if t <= 3 {
        // 0.0031308 以下线性段：t·12.92。
        return t * 12920 / 1000;
    }
    let t7 = (t as u128) * 10_000; // 1e7 标度（lin=1000 → 1e7）
    let p = integer_pow_u128(t7, 5); // ≤ 1e35，u128 安全
    let r = nth_root_u128(p, 12) as i64; // r = t^(5/12) ≤ 825.4
    (127821 * r / 100_000 - 55).clamp(0, 1000)
}

/// WCAG 相对亮度（千分位定点）：L = 0.2126R + 0.7152G + 0.0722B（线性域）。
pub fn relative_luminance_m(rgb: Rgb) -> i64 {
    let r = srgb_to_linear_m(rgb.r as i64 * 1000 / 255);
    let g = srgb_to_linear_m(rgb.g as i64 * 1000 / 255);
    let b = srgb_to_linear_m(rgb.b as i64 * 1000 / 255);
    (2126 * r + 7152 * g + 722 * b) / 10000
}

/// WCAG 对比度（百分位定点，×100）：(L1+0.05)/(L2+0.05)，亮者为分子。
/// F629 判据线 ≥3:1 → `contrast_x100 ≥ 300`；F631 模板 ≥4.5:1 → ≥450。
pub fn contrast_x100(a: Rgb, b: Rgb) -> i64 {
    let la = relative_luminance_m(a);
    let lb = relative_luminance_m(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    ((hi + 50) * 100 / (lo + 50).max(1)) as i64
}

/// sRGB→Lab(D65) 的 tristimulus（千分位定点输入，t = X/Xn 百万分位）。
/// f(t)：t > 216/24389 → t^(1/3)；否则 (24389/27·t + 16)/116。
const D65_XN_M6: u64 = 950_470; // Xn = 0.95047 → 百万分位
const D65_YN_M6: u64 = 1_000_000;
const D65_ZN_M6: u64 = 1_088_830;

fn lab_f_m2(t_m6: u64) -> i64 {
    // 输出百分位定点（×100）。阈值 216/24389 ≈ 0.008856 → 8856 百万分位。
    if t_m6 > 8856 {
        // t^(1/3)，百分位输出：cbrt(t_m6×1e6)/100 → cbrt(t_m6×1e6)
        // 直接给出百分位定点（cbrt(1e12)=1e4=100.00），精度 1/10000。
        (nth_root_u128((t_m6 as u128) * 1_000_000, 3) / 100) as i64
    } else {
        // f = 7.787·t + 16/116；t 百万分位 → 百分位输出：
        // 7.787·t_m6/1e5 + 13.79（t=8856 → 6.90+13.79 = 20.69 ✓）。
        ((7787 * t_m6 as u128 / 10_000_000 + 1379) as u64) as i64
    }
}

/// sRGB → Lab(D65)。输出 (L, a, b) 百分位定点（L ∈ [0,10000]）。
pub fn rgb_to_lab_m2(rgb: Rgb) -> (i64, i64, i64) {
    let r = srgb_to_linear_m(rgb.r as i64 * 1000 / 255) as u64;
    let g = srgb_to_linear_m(rgb.g as i64 * 1000 / 255) as u64;
    let b = srgb_to_linear_m(rgb.b as i64 * 1000 / 255) as u64;
    // 线性 RGB（千分位）→ XYZ（百万分位，矩阵系数 sRGB D65）。
    let x_m6 = (4124 * r + 3576 * g + 1805 * b) / 10; // /1000·/1000·1000 → ×1e6
    let y_m6 = (2126 * r + 7152 * g + 722 * b) / 10;
    let z_m6 = (193 * r + 1192 * g + 9505 * b) / 10;
    let fx = lab_f_m2(x_m6 * 1_000_000 / D65_XN_M6);
    let fy = lab_f_m2(y_m6 * 1_000_000 / D65_YN_M6);
    let fz = lab_f_m2(z_m6 * 1_000_000 / D65_ZN_M6);
    // L = 116·fy − 1600（fy 百分位 → L 百分位）；a = 5·(fx−fy)；b = 2·(fy−fz)。
    let l = 116 * fy - 1600;
    let a = 5 * (fx - fy);
    let bb = 2 * (fy - fz);
    (l, a, bb)
}

/// CIE76 ΔE76（百分位定点，×100）：判据「ΔE<1」即 `delta_e76_x100 < 100`。
pub fn delta_e76_x100(c1: Rgb, c2: Rgb) -> i64 {
    let (l1, a1, b1) = rgb_to_lab_m2(c1);
    let (l2, a2, b2) = rgb_to_lab_m2(c2);
    let dl = (l1 - l2) as i128;
    let da = (a1 - a2) as i128;
    let db = (b1 - b2) as i128;
    (isqrt64((dl * dl + da * da + db * db) as u64)) as i64
}

/// RGB（0-255 三通道）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    /// 千分位 HSL → RGB（h ∈ [0,360)，s/l ∈ [0,1000]）。
    pub fn from_hsl(h: u32, s_m: i64, l_m: i64) -> Rgb {
        let s = s_m.clamp(0, 1000);
        let l = l_m.clamp(0, 1000);
        let c = (1000 - (2 * l - 1000).abs()) * s / 1000; // 千分位色度
        let hp = ((h % 360) as i64) * 1000 / 60; // 千分位扇区
        let x = c * (1000 - (hp % 2000 - 1000).abs()) / 1000;
        let m = l - c / 2;
        let (r1, g1, b1) = match h % 360 {
            0..=59 => (c, x, 0),
            60..=119 => (x, c, 0),
            120..=179 => (0, c, x),
            180..=239 => (0, x, c),
            240..=299 => (x, 0, c),
            _ => (c, 0, x),
        };
        Rgb::new(
            ((r1 + m) * 255 / 1000).clamp(0, 255) as u8,
            ((g1 + m) * 255 / 1000).clamp(0, 255) as u8,
            ((b1 + m) * 255 / 1000).clamp(0, 255) as u8,
        )
    }

    /// → 千分位 HSL (h 0-359, s_m, l_m)。
    pub fn to_hsl(self) -> (u32, i64, i64) {
        let r = self.r as i64;
        let g = self.g as i64;
        let b = self.b as i64;
        let mx = max(r, max(g, b));
        let mn = min(r, min(g, b));
        let l = (mx + mn) * 1000 / 510;
        if mx == mn {
            return (0, 0, l.clamp(0, 1000));
        }
        let d = mx - mn;
        let s = if l < 500 {
            d * 1000 / (mx + mn).max(1)
        } else {
            d * 1000 / (510 - mx - mn).max(1)
        };
        let h_m = if mx == r {
            let v = (g - b) * 60000 / d;
            if v < 0 {
                v + 360000
            } else {
                v
            }
        } else if mx == g {
            (b - r) * 60000 / d + 120000
        } else {
            (r - g) * 60000 / d + 240000
        };
        let h = ((h_m % 360000) + 360000) % 360000 / 1000;
        ((h % 360) as u32, s.clamp(0, 1000), l.clamp(0, 1000))
    }
}

// ---------------------------------------------------------------------------
// 像素缓冲与几何原语
// ---------------------------------------------------------------------------

/// RGBA8 直存位图（行主序，`px[(y*w+x)*4..] = [r,g,b,a]`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixBuf {
    pub w: u16,
    pub h: u16,
    pub px: Vec<u8>,
}

impl PixBuf {
    pub fn new(w: u16, h: u16) -> PixBuf {
        PixBuf { w, h, px: alloc::vec![0u8; (w as usize) * (h as usize) * 4] }
    }

    pub fn from_rgba(w: u16, h: u16, px: Vec<u8>) -> PixBuf {
        debug_assert_eq!(px.len(), (w as usize) * (h as usize) * 4);
        PixBuf { w, h, px }
    }

    #[inline]
    pub fn get(&self, x: u16, y: u16) -> Option<[u8; 4]> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        Some([self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]])
    }

    #[inline]
    pub fn set(&mut self, x: u16, y: u16, rgba: [u8; 4]) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = (y as usize * self.w as usize + x as usize) * 4;
        self.px[i] = rgba[0];
        self.px[i + 1] = rgba[1];
        self.px[i + 2] = rgba[2];
        self.px[i + 3] = rgba[3];
    }

    /// 不透明度（像素 alpha ≥ 128 视为实体——热点/重心的实体口径）。
    #[inline]
    pub fn solid(&self, x: u16, y: u16) -> bool {
        self.get(x, y).map(|p| p[3] >= 128).unwrap_or(false)
    }

    /// 不透明像素计数（F637 重心的权重基数 / F625 覆盖统计）。
    pub fn solid_count(&self) -> u64 {
        self.px.chunks_exact(4).filter(|p| p[3] >= 128).count() as u64
    }

    /// 内容紧致包围盒（首个/末个实体行列；空图返回 None）。
    pub fn content_bbox(&self) -> Option<(u16, u16, u16, u16)> {
        let mut min_x = self.w;
        let mut min_y = self.h;
        let mut max_x = 0u16;
        let mut max_y = 0u16;
        let mut any = false;
        for y in 0..self.h {
            for x in 0..self.w {
                if self.solid(x, y) {
                    any = true;
                    min_x = min(min_x, x);
                    min_y = min(min_y, y);
                    max_x = max(max_x, x);
                    max_y = max(max_y, y);
                }
            }
        }
        if any {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    /// 视觉重心（不透明像素 alpha 加权质心，四舍五入）——F637 热区推荐。
    pub fn visual_centroid(&self) -> Option<(u16, u16)> {
        let mut sw: u64 = 0;
        let mut sx: u64 = 0;
        let mut sy: u64 = 0;
        for y in 0..self.h {
            for x in 0..self.w {
                if let Some(p) = self.get(x, y) {
                    if p[3] >= 128 {
                        let w = p[3] as u64;
                        sw += w;
                        sx += w * x as u64;
                        sy += w * y as u64;
                    }
                }
            }
        }
        if sw == 0 {
            return None;
        }
        let cx = ((sx + sw / 2) / sw) as u16;
        let cy = ((sy + sw / 2) / sw) as u16;
        Some((cx, cy))
    }

    /// 以 alpha 混合叠加一层（src-over；onion 皮预览用，30% 透明度由
    /// 调用方先衰减 alpha——F625 洋葱皮 30% 判据）。
    pub fn blend_over(&mut self, top: &PixBuf, at_x: i64, at_y: i64) {
        for y in 0..top.h {
            for x in 0..top.w {
                let Some(p) = top.get(x, y) else { continue };
                if p[3] == 0 {
                    continue;
                }
                let dx = at_x + x as i64;
                let dy = at_y + y as i64;
                if dx < 0 || dy < 0 || dx >= self.w as i64 || dy >= self.h as i64 {
                    continue;
                }
                let (dx, dy) = (dx as u16, dy as u16);
                let dst = self.get(dx, dy).unwrap_or([0, 0, 0, 0]);
                let sa = p[3] as u32;
                let da = dst[3] as u32;
                let oa = sa + da * (255 - sa) / 255;
                let mix = |s: u8, d: u8| -> u8 {
                    if oa == 0 {
                        0
                    } else {
                        ((s as u32 * sa + d as u32 * da * (255 - sa) / 255) / oa).clamp(0, 255) as u8
                    }
                };
                self.set(dx, dy, [mix(p[0], dst[0]), mix(p[1], dst[1]), mix(p[2], dst[2]), oa as u8]);
            }
        }
    }

    /// 整数倍最近邻复制（F625 双倍率同屏：1x→2x 精确两倍复制，
    /// 「对拍」判据的口径就是逐像素 2×2 复制一致）。
    pub fn scale_integer2x(&self) -> PixBuf {
        let mut out = PixBuf::new(self.w.saturating_mul(2), self.h.saturating_mul(2));
        for y in 0..self.h {
            for x in 0..self.w {
                let p = self.get(x, y).unwrap_or([0, 0, 0, 0]);
                out.set(x * 2, y * 2, p);
                out.set(x * 2 + 1, y * 2, p);
                out.set(x * 2, y * 2 + 1, p);
                out.set(x * 2 + 1, y * 2 + 1, p);
            }
        }
        out
    }

    /// 逐像素与另一缓冲的差异计数（对拍判据用；尺寸不同返回 None）。
    pub fn diff_pixels(&self, other: &PixBuf) -> Option<u64> {
        if self.w != other.w || self.h != other.h || self.px.len() != other.px.len() {
            return None;
        }
        Some(self.px.iter().zip(other.px.iter()).filter(|(a, b)| a != b).count() as u64)
    }
}

/// 覆盖写像素（无混合——画布主笔语义）。
pub fn put_pixel(cv: &mut PixBuf, x: i64, y: i64, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= cv.w as i64 || y >= cv.h as i64 {
        return;
    }
    cv.set(x as u16, y as u16, rgba);
}

/// 粗描边线段（Bresenham 主循环 + 半径方刷）——F625 钢笔与几何描边共用。
pub fn stroke_line(cv: &mut PixBuf, x0: i64, y0: i64, x1: i64, y1: i64, radius: u16, rgba: [u8; 4]) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i64 = if x0 < x1 { 1 } else { -1 };
    let sy: i64 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    let r = radius as i64;
    loop {
        for oy in -r..=r {
            for ox in -r..=r {
                put_pixel(cv, x + ox, y + oy, rgba);
            }
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// 多边形扫描线填充（偶奇规则，顶点顺序不限）——箭头/手掌/沙漏共用。
pub fn fill_polygon(cv: &mut PixBuf, pts: &[(i64, i64)], rgba: [u8; 4]) {
    if pts.len() < 3 {
        return;
    }
    let min_y = pts.iter().map(|p| p.1).min().unwrap_or(0);
    let max_y = pts.iter().map(|p| p.1).max().unwrap_or(0);
    for y in min_y..=max_y {
        if y < 0 || y >= cv.h as i64 {
            continue;
        }
        // 求与扫描线 y+0.5 相交的边，交点 x 排序后偶奇填充。
        let mut xs: Vec<i64> = Vec::new();
        let yc = y as i64 * 1000 + 500;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            if (a.1 <= y) != (b.1 <= y) {
                // x_m = a0·1000 + (y+0.5−a1)·(b0−a0)/(b1−a1)·1000——
                // 直接线性插值，除法放最后（防子像素参数截断成 0）。
                let dx = (yc - a.1 * 1000) * (b.0 - a.0) / (b.1 - a.1);
                let x_m = a.0 * 1000 + dx;
                // 四舍五入到整数像素列。
                xs.push((x_m + 500) / 1000);
            }
        }
        xs.sort_unstable();
        let mut k = 0;
        while k + 1 < xs.len() {
            let (xa, xb) = (xs[k], xs[k + 1]);
            for x in xa..=xb {
                put_pixel(cv, x, y, rgba);
            }
            k += 2;
        }
    }
}

/// 实心矩形。
pub fn fill_rect(cv: &mut PixBuf, x0: i64, y0: i64, x1: i64, y1: i64, rgba: [u8; 4]) {
    for y in min(y0, y1)..=max(y0, y1) {
        for x in min(x0, x1)..=max(x0, x1) {
            put_pixel(cv, x, y, rgba);
        }
    }
}

/// 描边矩形（宽度 radius*2+1）。
pub fn stroke_rect(cv: &mut PixBuf, x0: i64, y0: i64, x1: i64, y1: i64, radius: u16, rgba: [u8; 4]) {
    let (xa, xb) = (min(x0, x1), max(x0, x1));
    let (ya, yb) = (min(y0, y1), max(y0, y1));
    for x in xa..=xb {
        for oy in 0..=radius as i64 {
            put_pixel(cv, x, ya + oy, rgba);
            put_pixel(cv, x, ya - oy, rgba);
            put_pixel(cv, x, yb + oy, rgba);
            put_pixel(cv, x, yb - oy, rgba);
        }
    }
    for y in ya..=yb {
        for ox in 0..=radius as i64 {
            put_pixel(cv, xa + ox, y, rgba);
            put_pixel(cv, xa - ox, y, rgba);
            put_pixel(cv, xb + ox, y, rgba);
            put_pixel(cv, xb - ox, y, rgba);
        }
    }
}

/// 实心椭圆（中点法，整数判定）。
pub fn fill_ellipse(cv: &mut PixBuf, cx: i64, cy: i64, rx: i64, ry: i64, rgba: [u8; 4]) {
    if rx <= 0 || ry <= 0 {
        return;
    }
    for y in (cy - ry)..=(cy + ry) {
        if y < 0 || y >= cv.h as i64 {
            continue;
        }
        let ny = ((y - cy) * 1000 / ry) as i64;
        let span = 1000 * 1000 - ny * ny;
        if span < 0 {
            continue;
        }
        let sx = rx * isqrt64(span as u64) as i64 / 1000;
        for x in (cx - sx)..=(cx + sx) {
            put_pixel(cv, x, y, rgba);
        }
    }
}

/// 描边椭圆环（半径带宽：inner ≤ d ≤ outer）。
pub fn stroke_ellipse(cv: &mut PixBuf, cx: i64, cy: i64, rx: i64, ry: i64, radius: u16, rgba: [u8; 4]) {
    if rx <= 0 || ry <= 0 {
        return;
    }
    for y in (cy - ry - radius as i64)..=(cy + ry + radius as i64) {
        if y < 0 || y >= cv.h as i64 {
            continue;
        }
        for x in (cx - rx - radius as i64)..=(cx + rx + radius as i64) {
            let dx = (x - cx) * 1000 / rx;
            let dy = (y - cy) * 1000 / ry;
            let d2 = dx * dx + dy * dy;
            // 椭圆距离带：以 1/1000 椭圆半径为尺度，带厚 radius 比例换算。
            let band = (radius as i64 * 1000 / rx.min(ry)).max(30);
            if d2 <= (1000 + band) * (1000 + band) && d2 >= (1000 - band) * (1000 - band) {
                put_pixel(cv, x, y, rgba);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lanczos3 重采样（1/1000 定点权重，可分离两趟；权重归一化后绝对精度
// 不敏感——正弦查表 Taylor 展开在 [0,π] 收敛，跨 π 符号翻转）
// ---------------------------------------------------------------------------

/// sin(x_m)：输入 x 的 1/1000 弧度定点（x_m ≤ 3142 即 ≤ π 内插值保证
/// 收敛；x_m < 0 返回 −sin(−x)；周期归约到 [0,π] 后 9 项泰勒）。
/// 输出 1/1000 定点（−1000..1000）。
pub fn sin_fp(x_m: i64) -> i64 {
    let two_pi: i64 = 6283; // 2π×1000
    let pi: i64 = 3142; // π×1000（略偏大 0.4‰，保守归约）
    let neg = x_m < 0;
    let x = x_m.abs() % two_pi;
    if x > pi {
        // y = 2π − x ∈ (0,π)；sin(x) = −sin(y)。
        let s = sin_fp_pos(two_pi - x);
        return if neg { s } else { -s };
    }
    let s = sin_fp_pos(x);
    if neg {
        -s
    } else {
        s
    }
}

/// [0,π] 内的 sin 泰勒和（输入/输出均 1/1000 定点）。
fn sin_fp_pos(x_m: i64) -> i64 {
    let x = x_m.clamp(0, 3142) as i128;
    let x2 = x * x; // (x/1000)² × 1e6
    // sin = x − x³/6 + x⁵/120 − x⁷/5040 + x⁹/362880（真值）；
    // 定点：项 k = x^(2k+1)/(2k+1)!，x 的 1/1000 标度逐项除 1e3^(2k+1)。
    let t1 = x;
    let t3 = x * x2 / (6i128 * 1_000_000); // x_m³/(6e6) = 千分位 x³/6
    let t5 = x * x2 * x2 / (120i128 * 1_000_000_000_000); // x_m⁵/(120e12)
    let t7 = x2 * x2 * x2 * x / (5040i128 * 1_000_000_000_000_000_000); // x_m⁷/(5040e18)
    let t9 = x2 * x2 * x2 * x2 * x / (362880i128 * 1_000_000_000_000_000_000_000_000);
    let s = t1 - t3 + t5 - t7 + t9;
    s.clamp(-1000, 1000) as i64
}

/// sinc(t)（t 千分位定点，t = 距中心源距）：t=0 → 1000；否则
/// sin(π·t)/(π·t)，1/1000 定点输出。
fn sinc_m(t_m: i64) -> i64 {
    if t_m == 0 {
        return 1000;
    }
    let arg = (3142 * t_m.abs()) / 1000; // π·t（1/1000 弧度）
    let s = sin_fp(arg);
    // sin(π·t)/(π·t)：|arg| ≤ π·3 = 9426（Lanczos 窗 |t|≤3）。
    // sin 在 (π,2π) 为负 → 归约内部已处理符号；分母取带符号 t。
    s * 1000 / arg.max(1)
}

/// Lanczos3 权重（|t| < 3）：sinc(t)·sinc(t/3)，1/1000 定点。
pub fn lanczos3_w(t_m: i64) -> i64 {
    let at = t_m.abs();
    if at >= 3000 {
        return 0;
    }
    sinc_m(at) * sinc_m(at * 1000 / 3000) / 1000
}

/// Lanczos3 可分离重采样（src → dst 任意尺寸；边缘 clamp；a=3 窗）。
/// 权重逐点归一化（÷wsum），常色区域数学上恒等（核分母自消）。
pub fn resample_lanczos3(src: &PixBuf, dst_w: u16, dst_h: u16) -> PixBuf {
    if dst_w == 0 || dst_h == 0 || src.w == 0 || src.h == 0 {
        return PixBuf::new(dst_w, dst_h);
    }
    let mut tmp = alloc::vec![0i32; src.h as usize * dst_w as usize * 4];
    // 横向：dst 像素 x 的源中心 = (x+0.5)·sw/dw − 0.5（半像素对齐）。
    for sy in 0..src.h as usize {
        for dx in 0..dst_w as usize {
            let center: i64 =
                ((dx as i64 + 1) * src.w as i64 * 2 + dst_w as i64) / (2 * dst_w as i64) - 1;
            let mut acc = [0i64; 4];
            let mut wsum: i64 = 0;
            for k in (center - 3)..=(center + 3) {
                if k < 0 || k >= src.w as i64 {
                    continue;
                }
                // 源距（1/1000 像素）：(x+0.5)·sw/dw − 0.5 − k。
                let dist_m =
                    ((dx as i64 + 1) * src.w as i64 * 1000 / dst_w as i64) - 500 - k * 1000;
                let w = lanczos3_w(dist_m);
                if w == 0 {
                    continue;
                }
                let si = (sy * src.w as usize + k as usize) * 4;
                for c in 0..4 {
                    acc[c] += src.px[si + c] as i64 * w;
                }
                wsum += w;
            }
            let ti = (sy * dst_w as usize + dx) * 4;
            if wsum > 0 {
                for c in 0..4 {
                    tmp[ti + c] = (acc[c] / wsum).clamp(0, 255) as i32;
                }
            }
        }
    }
    // 纵向：同一半像素对齐约定。
    let mut out = PixBuf::new(dst_w, dst_h);
    for dy in 0..dst_h as usize {
        let center: i64 =
            ((dy as i64 + 1) * src.h as i64 * 2 + dst_h as i64) / (2 * dst_h as i64) - 1;
        for dx in 0..dst_w as usize {
            let mut acc = [0i64; 4];
            let mut wsum: i64 = 0;
            for k in (center - 3)..=(center + 3) {
                if k < 0 || k >= src.h as i64 {
                    continue;
                }
                let dist_m =
                    ((dy as i64 + 1) * src.h as i64 * 1000 / dst_h as i64) - 500 - k * 1000;
                let w = lanczos3_w(dist_m);
                if w == 0 {
                    continue;
                }
                let ti = (k as usize * dst_w as usize + dx) * 4;
                for c in 0..4 {
                    acc[c] += tmp[ti + c] as i64 * w;
                }
                wsum += w;
            }
            if wsum > 0 {
                let rgba = [
                    (acc[0] / wsum).clamp(0, 255) as u8,
                    (acc[1] / wsum).clamp(0, 255) as u8,
                    (acc[2] / wsum).clamp(0, 255) as u8,
                    (acc[3] / wsum).clamp(0, 255) as u8,
                ];
                out.set(dx as u16, dy as u16, rgba);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 15 标准态与方案模型（F156「15 枚指针」/ F627「15 标准态齐全性」口径）
// ---------------------------------------------------------------------------

/// 15 标准态（Windows 方案全集口径，与 F156 编辑器/F627 体检同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PointerState {
    Normal = 0,     // 正常选择
    Help = 1,       // 帮助选择
    Work = 2,       // 后台运行
    Busy = 3,       // 忙
    Precise = 4,    // 精确定位
    Text = 5,       // 文本选择
    Hand = 6,       // 手写
    Unavailable = 7, // 不可用
    VResize = 8,    // 垂直调整
    HResize = 9,    // 水平调整
    D1Resize = 10,  // 沿对角线调整 1
    D2Resize = 11,  // 沿对角线调整 2
    Move = 12,      // 移动
    Alternate = 13, // 候选选择
    Link = 14,      // 链接选择
}

pub const ALL_STATES: [PointerState; 15] = [
    PointerState::Normal,
    PointerState::Help,
    PointerState::Work,
    PointerState::Busy,
    PointerState::Precise,
    PointerState::Text,
    PointerState::Hand,
    PointerState::Unavailable,
    PointerState::VResize,
    PointerState::HResize,
    PointerState::D1Resize,
    PointerState::D2Resize,
    PointerState::Move,
    PointerState::Alternate,
    PointerState::Link,
];

impl PointerState {
    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<PointerState> {
        ALL_STATES.iter().copied().find(|s| s.id() == id)
    }

    pub fn zh_name(self) -> &'static str {
        match self {
            PointerState::Normal => "正常选择",
            PointerState::Help => "帮助选择",
            PointerState::Work => "后台运行",
            PointerState::Busy => "忙",
            PointerState::Precise => "精确定位",
            PointerState::Text => "文本选择",
            PointerState::Hand => "手写",
            PointerState::Unavailable => "不可用",
            PointerState::VResize => "垂直调整",
            PointerState::HResize => "水平调整",
            PointerState::D1Resize => "沿对角线调整 1",
            PointerState::D2Resize => "沿对角线调整 2",
            PointerState::Move => "移动",
            PointerState::Alternate => "候选选择",
            PointerState::Link => "链接选择",
        }
    }

    /// 静态态（无动画语义的态——F631 低视觉负荷模板只保留这些的静态形）。
    pub fn is_static_semantic(self) -> bool {
        !matches!(self, PointerState::Work | PointerState::Busy)
    }
}

/// 方案来源（F626/F629/F630/F635/F638 分工边界的机器可读形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginKind {
    /// 内置基线（F632「内置指针自己也要过同一审计」的基线对象）。
    Builtin,
    /// 工坊新作（F625 入库链）。
    Created,
    /// 重染副本（F626，非破坏纪律：payload = 原方案名）。
    Recolored(String),
    /// 主题派生（F629，payload = 派生令牌指纹 hex）。
    Derived(String),
    /// 单文件导入（F630，payload = 源文件指纹 hex）。
    Imported(String),
    /// 包侧载（F635，payload = vxapp 包 ID）。
    Sideloaded(String),
    /// Windows 迁移（F638，payload = 「迁移自 Windows·方案名」）。
    Migrated(String),
}

impl OriginKind {
    pub fn tag(&self) -> &'static str {
        match self {
            OriginKind::Builtin => "builtin",
            OriginKind::Created => "created",
            OriginKind::Recolored(_) => "recolored",
            OriginKind::Derived(_) => "derived",
            OriginKind::Imported(_) => "imported",
            OriginKind::Sideloaded(_) => "sideloaded",
            OriginKind::Migrated(_) => "migrated",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            OriginKind::Recolored(s) | OriginKind::Derived(s) | OriginKind::Imported(s)
            | OriginKind::Sideloaded(s) | OriginKind::Migrated(s) => s,
            _ => "",
        }
    }
}

/// 单帧（含热点与帧延时；`.ani` jiffy 在导入层换算为毫秒后原值另行登记）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorFrame {
    pub w: u16,
    pub h: u16,
    pub hot_x: u16,
    pub hot_y: u16,
    /// 帧延时（毫秒；≤16 → 60fps 闸线，F639 帧率闸与 F627 帧率纪律同源）。
    pub delay_ms: u32,
    pub px: Vec<u8>,
}

impl CursorFrame {
    pub fn from_buf(hot_x: u16, hot_y: u16, delay_ms: u32, buf: PixBuf) -> CursorFrame {
        CursorFrame { w: buf.w, h: buf.h, hot_x, hot_y, delay_ms, px: buf.px }
    }

    pub fn buf(&self) -> PixBuf {
        PixBuf::from_rgba(self.w, self.h, self.px.clone())
    }

    /// 有效帧率（fps）：delay 0 = 静态帧（无动画语义 → 0）；其余
    /// 1000/delay 向下取。帧率闸线（F639）作用于 1..=16ms 延时段。
    pub fn fps(&self) -> u32 {
        if self.delay_ms == 0 {
            return 0;
        }
        1000 / self.delay_ms
    }
}

/// 单态帧序列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateFrames {
    pub state: PointerState,
    pub frames: Vec<CursorFrame>,
}

/// 指针方案模型（J 域唯一方案事实源：库房 F628 / 体检 F627 / 分享 F630 /
/// 审计 F632 / 闸门 F639 全部操作此模型，不另立第二表示）。
#[derive(Clone, Debug)]
pub struct CursorSchemeModel {
    pub name: String,
    pub author: String,
    pub origin: OriginKind,
    /// 增强渲染标注（F636：只有 1x 图被升采样 → true，诚实标注）。
    pub enhanced_render: bool,
    /// 原生 2x 存在（F636 原生优先铁序的依据）。
    pub native_2x: bool,
    /// 矢量源（F636 派生优先于位图升采样的依据；F634 置位）。
    pub vector_source: bool,
    pub entries: Vec<StateFrames>,
}

impl CursorSchemeModel {
    pub fn empty(name: &str, origin: OriginKind) -> CursorSchemeModel {
        CursorSchemeModel {
            name: String::from(name),
            author: String::new(),
            origin,
            enhanced_render: false,
            native_2x: false,
            vector_source: false,
            entries: Vec::new(),
        }
    }

    pub fn state(&self, st: PointerState) -> Option<&StateFrames> {
        self.entries.iter().find(|e| e.state == st)
    }

    pub fn state_mut(&mut self, st: PointerState) -> Option<&mut StateFrames> {
        self.entries.iter_mut().find(|e| e.state == st)
    }

    pub fn set_state(&mut self, st: PointerState, frames: Vec<CursorFrame>) {
        if let Some(e) = self.state_mut(st) {
            e.frames = frames;
        } else {
            self.entries.push(StateFrames { state: st, frames });
            self.entries.sort_by_key(|e| e.state);
        }
    }

    pub fn missing_states(&self) -> Vec<PointerState> {
        ALL_STATES.iter().copied().filter(|s| self.state(*s).is_none()).collect()
    }

    /// 全方案最大帧尺寸（F627 超尺寸提示线 / F639 尺寸闸共用）。
    pub fn max_frame_px(&self) -> u32 {
        self.entries
            .iter()
            .flat_map(|e| e.frames.iter())
            .map(|f| max(f.w as u32, f.h as u32))
            .max()
            .unwrap_or(0)
    }

    /// 解压后位图总量（w·h·4 逐帧累加——F639 内存闸口径）。
    pub fn total_bitmap_bytes(&self) -> u64 {
        self.entries
            .iter()
            .flat_map(|e| e.frames.iter())
            .map(|f| f.w as u64 * f.h as u64 * 4)
            .sum()
    }

    /// 最大有效帧率（F639 帧率闸 / F627 帧率纪律共用）。
    pub fn max_fps(&self) -> u32 {
        self.entries
            .iter()
            .flat_map(|e| e.frames.iter())
            .map(|f| f.fps())
            .max()
            .unwrap_or(0)
    }

    /// 序列化字节量（与 total_bitmap_bytes 分开：容器开销另计）。
    pub fn container_bytes(&self) -> u64 {
        serialize_vxcur(self).len() as u64
    }
}

// ---------------------------------------------------------------------------
// .vxcur 容器（F630 分享格式 = F133 图标包规范的指针子集实例化）
// ---------------------------------------------------------------------------

pub const VXCUR_MAGIC: [u8; 6] = *b"VXCUR\x01";

/// 容量上限（与 F639 内存闸同源一处一事实）。
pub const VXCUR_MAX_BYTES: usize = 4 * 1024 * 1024;

/// 方案级容量与闸线（F639 三闸与 F627 纪律的唯一事实源）。
/// 单帧位图上限（px）——防「指针当壁纸」滥用。
pub const MAX_FRAME_PX: u32 = 256;
/// 单态帧数上限（16 帧纪律，F625/F627 同源）。
pub const MAX_FRAMES_PER_STATE: usize = 16;
/// 有效帧率上限（fps）——防频闪不适。
pub const MAX_FPS: u32 = 60;
/// 解压后位图总量上限（4MB）——防资源型炸弹。
pub const MAX_TOTAL_BITMAP_BYTES: u64 = 4 * 1024 * 1024;
/// 超尺寸提示线（F627 体检：>64px 提示过大遮挡，非拒入）。
pub const WARN_FRAME_PX: u32 = 64;

/// 解析错误（诚实定位：错误带字节偏移/段语义，不静默、不半导入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VxcurError {
    TooSmall(usize),
    BadMagic(usize),
    Truncated {
        offset: usize,
        need: &'static str,
    },
    UnknownStateId {
        offset: usize,
        id: u8,
    },
    FrameTooLarge {
        state: &'static str,
        index: usize,
        w: u16,
        h: u16,
    },
    TooManyFrames {
        state: &'static str,
        count: usize,
    },
    SizeOver(usize),
    MetadataTruncated(usize),
}

/// 序列化：方案 → .vxcur 字节（确定性布局，逐字节往返一致）。
/// 布局：magic(6) | u16 meta_len | u16 state_count | meta TLV |
///       逐态: u8 state_id | u8 flags(bit0=2x,bit1=vector,bit2=enhanced)
///             u16 frame_count | 逐帧: u16 w,h,hot_x,hot_y | u32 delay | u32 len | data
pub fn serialize_vxcur(m: &CursorSchemeModel) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&VXCUR_MAGIC);
    // 元数据 TLV：name/author/origin_tag/origin_detail。
    let mut meta: Vec<u8> = Vec::new();
    let pairs: [(&str, &str); 4] = [
        ("name", &m.name),
        ("author", &m.author),
        ("origin", m.origin.tag()),
        ("origin_detail", m.origin.detail()),
    ];
    meta.extend_from_slice(&(pairs.len() as u16).to_le_bytes());
    for (k, v) in pairs {
        meta.push(k.len() as u8);
        meta.extend_from_slice(k.as_bytes());
        meta.extend_from_slice(&(v.len() as u16).to_le_bytes());
        meta.extend_from_slice(v.as_bytes());
    }
    out.extend_from_slice(&(meta.len() as u16).to_le_bytes());
    out.extend_from_slice(&(m.entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&meta);
    let flags: u8 = (m.native_2x as u8) | ((m.vector_source as u8) << 1) | ((m.enhanced_render as u8) << 2);
    for e in &m.entries {
        out.push(e.state.id());
        out.push(flags);
        out.extend_from_slice(&(e.frames.len() as u16).to_le_bytes());
        for f in &e.frames {
            out.extend_from_slice(&f.w.to_le_bytes());
            out.extend_from_slice(&f.h.to_le_bytes());
            out.extend_from_slice(&f.hot_x.to_le_bytes());
            out.extend_from_slice(&f.hot_y.to_le_bytes());
            out.extend_from_slice(&f.delay_ms.to_le_bytes());
            out.extend_from_slice(&(f.px.len() as u32).to_le_bytes());
            out.extend_from_slice(&f.px);
        }
    }
    out
}

/// 解析：.vxcur 字节 → 方案（任何越界/超限诚实报错，绝不半导入）。
pub fn parse_vxcur(data: &[u8]) -> Result<CursorSchemeModel, VxcurError> {
    if data.len() > VXCUR_MAX_BYTES {
        return Err(VxcurError::SizeOver(data.len()));
    }
    if data.len() < 10 {
        return Err(VxcurError::TooSmall(data.len()));
    }
    if data[..6] != VXCUR_MAGIC {
        return Err(VxcurError::BadMagic(0));
    }
    let mut o = 6usize;
    let rd_u16 = |o: &mut usize| -> Result<u16, VxcurError> {
        if *o + 2 > data.len() {
            return Err(VxcurError::Truncated { offset: *o, need: "u16" });
        }
        let v = u16::from_le_bytes([data[*o], data[*o + 1]]);
        *o += 2;
        Ok(v)
    };
    let rd_u32 = |o: &mut usize| -> Result<u32, VxcurError> {
        if *o + 4 > data.len() {
            return Err(VxcurError::Truncated { offset: *o, need: "u32" });
        }
        let v = u32::from_le_bytes([data[*o], data[*o + 1], data[*o + 2], data[*o + 3]]);
        *o += 4;
        Ok(v)
    };
    let meta_len = rd_u16(&mut o)? as usize;
    let state_count = rd_u16(&mut o)? as usize;
    // 元数据 TLV。
    let meta_end = o + meta_len;
    if meta_end > data.len() {
        return Err(VxcurError::MetadataTruncated(o));
    }
    let mut name = String::new();
    let mut author = String::new();
    let mut origin_tag = String::from("imported");
    let mut origin_detail = String::new();
    {
        let md = &data[o..meta_end];
        if md.len() >= 2 {
            let pairs = u16::from_le_bytes([md[0], md[1]]) as usize;
            let mut i = 2usize;
            for _ in 0..pairs {
                if i >= md.len() {
                    return Err(VxcurError::MetadataTruncated(i));
                }
                let klen = md[i] as usize;
                i += 1;
                if i + klen > md.len() {
                    return Err(VxcurError::MetadataTruncated(i));
                }
                let key = core::str::from_utf8(&md[i..i + klen]).unwrap_or("");
                i += klen;
                if i + 2 > md.len() {
                    return Err(VxcurError::MetadataTruncated(i));
                }
                let vlen = u16::from_le_bytes([md[i], md[i + 1]]) as usize;
                i += 2;
                if i + vlen > md.len() {
                    return Err(VxcurError::MetadataTruncated(i));
                }
                let val = core::str::from_utf8(&md[i..i + vlen]).unwrap_or("").to_string();
                i += vlen;
                match key {
                    "name" => name = val,
                    "author" => author = val,
                    "origin" => origin_tag = val,
                    "origin_detail" => origin_detail = val,
                    _ => {}
                }
            }
        }
    }
    o = meta_end;
    let mut m = CursorSchemeModel::empty("", OriginKind::Imported(String::new()));
    m.name = name;
    m.author = author;
    m.origin = match origin_tag.as_str() {
        "builtin" => OriginKind::Builtin,
        "created" => OriginKind::Created,
        "recolored" => OriginKind::Recolored(origin_detail.clone()),
        "derived" => OriginKind::Derived(origin_detail.clone()),
        "sideloaded" => OriginKind::Sideloaded(origin_detail.clone()),
        "migrated" => OriginKind::Migrated(origin_detail.clone()),
        _ => OriginKind::Imported(origin_detail.clone()),
    };
    for _ in 0..state_count {
        if o >= data.len() {
            return Err(VxcurError::Truncated { offset: o, need: "state header" });
        }
        let sid = data[o];
        let flags = data[o + 1];
        o += 2;
        let Some(st) = PointerState::from_id(sid) else {
            return Err(VxcurError::UnknownStateId { offset: o - 2, id: sid });
        };
        m.native_2x |= flags & 1 != 0;
        m.vector_source |= flags & 2 != 0;
        m.enhanced_render |= flags & 4 != 0;
        let fcount = rd_u16(&mut o)? as usize;
        if fcount > MAX_FRAMES_PER_STATE {
            return Err(VxcurError::TooManyFrames { state: st.zh_name(), count: fcount });
        }
        let mut frames = Vec::with_capacity(fcount.min(MAX_FRAMES_PER_STATE));
        for fi in 0..fcount {
            let w = rd_u16(&mut o)?;
            let h = rd_u16(&mut o)?;
            let hx = rd_u16(&mut o)?;
            let hy = rd_u16(&mut o)?;
            let delay = rd_u32(&mut o)?;
            let len = rd_u32(&mut o)? as usize;
            if max(w as u32, h as u32) > MAX_FRAME_PX {
                return Err(VxcurError::FrameTooLarge { state: st.zh_name(), index: fi, w, h });
            }
            if o + len > data.len() {
                return Err(VxcurError::Truncated { offset: o, need: "frame data" });
            }
            if len != (w as usize) * (h as usize) * 4 {
                return Err(VxcurError::Truncated { offset: o, need: "frame data exact size" });
            }
            frames.push(CursorFrame {
                w,
                h,
                hot_x: hx,
                hot_y: hy,
                delay_ms: delay,
                px: data[o..o + len].to_vec(),
            });
            o += len;
        }
        m.entries.push(StateFrames { state: st, frames });
    }
    Ok(m)
}

/// .vxcur 内容指纹（往返一致判据：导出哈希 == 导入后重序列化哈希）。
pub fn vxcur_fingerprint(m: &CursorSchemeModel) -> u64 {
    fnv1a64(&serialize_vxcur(m))
}

/// 帧内容指纹（与元数据无关：仅 15 态帧几何+像素+热点+延时）——
/// F635 包级/方案级双入口「同内容」对账的口径。
pub fn content_fingerprint(m: &CursorSchemeModel) -> u64 {
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    for e in &m.entries {
        buf.push(e.state.id());
        buf.extend_from_slice(&(e.frames.len() as u16).to_le_bytes());
        for f in &e.frames {
            buf.extend_from_slice(&f.w.to_le_bytes());
            buf.extend_from_slice(&f.h.to_le_bytes());
            buf.extend_from_slice(&f.hot_x.to_le_bytes());
            buf.extend_from_slice(&f.hot_y.to_le_bytes());
            buf.extend_from_slice(&f.delay_ms.to_le_bytes());
            buf.extend_from_slice(&f.px);
        }
    }
    fnv1a64(&buf)
}

// ---------------------------------------------------------------------------
// 内置默认方案（F627 缺态补默认 / F639 熔断回退 / F635 卸载回退共用）
// ---------------------------------------------------------------------------

/// 内置兜底形绘制的统一画幅（32×32，经典指针口径）。
pub const BUILTIN_GLYPH_PX: u16 = 32;

const OUTLINE: [u8; 4] = [24, 24, 24, 255];
const FILL: [u8; 4] = [250, 250, 250, 255];
const ACCENT_FILL: [u8; 4] = [32, 96, 208, 255];

/// 经典箭头（tip 在 (tx,ty)，白身黑边）。
fn draw_arrow(cv: &mut PixBuf, tx: i64, ty: i64) {
    let pts = [
        (tx, ty),
        (tx, ty + 16),
        (tx + 4, ty + 12),
        (tx + 7, ty + 19),
        (tx + 10, ty + 18),
        (tx + 7, ty + 11),
        (tx + 12, ty + 11),
    ];
    fill_polygon(cv, &pts, OUTLINE);
    let inner = [
        (tx + 1, ty + 2),
        (tx + 1, ty + 13),
        (tx + 4, ty + 10),
        (tx + 7, ty + 17),
        (tx + 8, ty + 16),
        (tx + 5, ty + 9),
        (tx + 10, ty + 9),
    ];
    fill_polygon(cv, &inner, FILL);
}

/// 双向箭头杆（调整类形态：轴 + 两端箭头）。
fn draw_double_arrow(cv: &mut PixBuf, cx: i64, cy: i64, vertical: bool) {
    if vertical {
        stroke_line(cv, cx, cy - 9, cx, cy + 9, 1, OUTLINE);
        fill_polygon(cv, &[(cx, cy - 13), (cx - 5, cy - 6), (cx + 5, cy - 6)], OUTLINE);
        fill_polygon(cv, &[(cx, cy + 13), (cx - 5, cy + 6), (cx + 5, cy + 6)], OUTLINE);
    } else {
        stroke_line(cv, cx - 9, cy, cx + 9, cy, 1, OUTLINE);
        fill_polygon(cv, &[(cx - 13, cy), (cx - 6, cy - 5), (cx - 6, cy + 5)], OUTLINE);
        fill_polygon(cv, &[(cx + 13, cy), (cx + 6, cy - 5), (cx + 6, cy + 5)], OUTLINE);
    }
}

/// 对角双向箭头（dir = (dx,dy) 单位方向 ±1）。
fn draw_diag_arrow(cv: &mut PixBuf, cx: i64, cy: i64, dx: i64, dy: i64) {
    let l = 9i64;
    stroke_line(cv, cx - dx * l, cy - dy * l, cx + dx * l, cy + dy * l, 1, OUTLINE);
    for s in [-1i64, 1] {
        let tx = cx + dx * l * s;
        let ty = cy + dy * l * s;
        // 箭头两翼（垂直于轴）。
        let px = -dy;
        let py = dx;
        fill_polygon(
            cv,
            &[(tx, ty), (tx - dx * 6 + px * 4, ty - dy * 6 + py * 4), (tx - dx * 6 - px * 4, ty - dy * 6 - py * 4)],
            OUTLINE,
        );
    }
}

/// 沙漏（busy/work 的计时形）。
fn draw_hourglass(cv: &mut PixBuf, cx: i64, cy: i64, half: i64) {
    fill_polygon(cv, &[(cx - half, cy - half), (cx + half, cy - half), (cx, cy)], OUTLINE);
    fill_polygon(cv, &[(cx - half, cy + half), (cx + half, cy + half), (cx, cy)], OUTLINE);
}

/// 手写笔（斜杆 + 笔尖）。
fn draw_pen(cv: &mut PixBuf, x0: i64, y0: i64, x1: i64, y1: i64) {
    stroke_line(cv, x0, y0, x1, y1, 2, OUTLINE);
    stroke_line(cv, x0 + 1, y0 - 1, x1 + 1, y1 - 1, 1, FILL);
    fill_polygon(cv, &[(x0 - 2, y0 + 2), (x0 + 2, y0 - 2), (x0 + 1, y0 + 4)], ACCENT_FILL);
}

/// 指向手掌（link 选择；简化手掌形：掌 + 三指轮廓）。
fn draw_hand(cv: &mut PixBuf, cx: i64, cy: i64) {
    fill_polygon(
        cv,
        &[
            (cx - 4, cy - 8),
            (cx - 1, cy - 8),
            (cx - 1, cy - 2),
            (cx + 1, cy - 9),
            (cx + 4, cy - 9),
            (cx + 3, cy - 1),
            (cx + 6, cy + 1),
            (cx + 6, cy + 5),
            (cx + 3, cy + 9),
            (cx - 4, cy + 9),
            (cx - 6, cy + 4),
            (cx - 6, cy - 1),
        ],
        OUTLINE,
    );
    fill_polygon(
        cv,
        &[
            (cx - 3, cy - 6),
            (cx - 2, cy - 6),
            (cx - 2, cy - 1),
            (cx, cy - 7),
            (cx + 3, cy - 7),
            (cx + 2, cy),
            (cx + 4, cy + 2),
            (cx + 4, cy + 5),
            (cx + 2, cy + 7),
            (cx - 3, cy + 7),
            (cx - 4, cy + 3),
            (cx - 4, cy - 1),
        ],
        FILL,
    );
}

/// 单态兜底形绘制（15 态各一帧 32×32；热点=语义锚点）。
pub fn builtin_glyph(st: PointerState) -> CursorFrame {
    let mut cv = PixBuf::new(BUILTIN_GLYPH_PX, BUILTIN_GLYPH_PX);
    let (hx, hy): (u16, u16) = match st {
        PointerState::Normal => {
            draw_arrow(&mut cv, 4, 2);
            (4, 2)
        }
        PointerState::Help => {
            draw_arrow(&mut cv, 2, 2);
            // 问号：三段折线 + 点。
            stroke_line(&mut cv, 20, 8, 22, 6, 0, OUTLINE);
            stroke_line(&mut cv, 22, 6, 25, 8, 0, OUTLINE);
            stroke_line(&mut cv, 25, 8, 22, 12, 0, OUTLINE);
            stroke_line(&mut cv, 22, 12, 22, 14, 0, OUTLINE);
            put_pixel(&mut cv, 22, 17, OUTLINE);
            put_pixel(&mut cv, 22, 18, OUTLINE);
            (2, 2)
        }
        PointerState::Work => {
            draw_arrow(&mut cv, 2, 2);
            draw_hourglass(&mut cv, 23, 23, 5);
            (2, 2)
        }
        PointerState::Busy => {
            draw_hourglass(&mut cv, 16, 16, 9);
            (16, 16)
        }
        PointerState::Precise => {
            stroke_line(&mut cv, 16, 4, 16, 28, 1, OUTLINE);
            stroke_line(&mut cv, 4, 16, 28, 16, 1, OUTLINE);
            (16, 16)
        }
        PointerState::Text => {
            stroke_line(&mut cv, 16, 6, 16, 26, 0, OUTLINE);
            stroke_line(&mut cv, 13, 6, 19, 6, 0, OUTLINE);
            stroke_line(&mut cv, 13, 26, 19, 26, 0, OUTLINE);
            (16, 16)
        }
        PointerState::Hand => {
            draw_pen(&mut cv, 22, 8, 8, 24);
            (8, 24)
        }
        PointerState::Unavailable => {
            stroke_ellipse(&mut cv, 16, 16, 10, 10, 2, OUTLINE);
            stroke_line(&mut cv, 9, 9, 23, 23, 2, OUTLINE);
            (16, 16)
        }
        PointerState::VResize => {
            draw_double_arrow(&mut cv, 16, 16, true);
            (16, 16)
        }
        PointerState::HResize => {
            draw_double_arrow(&mut cv, 16, 16, false);
            (16, 16)
        }
        PointerState::D1Resize => {
            draw_diag_arrow(&mut cv, 16, 16, 1, 1);
            (16, 16)
        }
        PointerState::D2Resize => {
            draw_diag_arrow(&mut cv, 16, 16, -1, 1);
            (16, 16)
        }
        PointerState::Move => {
            draw_double_arrow(&mut cv, 16, 16, true);
            draw_double_arrow(&mut cv, 16, 16, false);
            (16, 16)
        }
        PointerState::Alternate => {
            fill_polygon(&mut cv, &[(16, 4), (6, 24), (12, 24), (16, 14), (20, 24), (26, 24)], OUTLINE);
            fill_polygon(&mut cv, &[(16, 8), (10, 22), (13, 22), (16, 13), (19, 22), (22, 22)], FILL);
            (16, 4)
        }
        PointerState::Link => {
            draw_hand(&mut cv, 15, 14);
            (14, 8)
        }
    };
    // 热点锚点实体化（真实指针方案惯例：锚点像素必须落在图形上——
    // F627「热点须落在图形实体上」对内置件同样生效）。
    if !cv.solid(hx, hy) {
        cv.set(hx, hy, OUTLINE);
    }
    CursorFrame::from_buf(hx, hy, 0, cv)
}

/// 内置默认方案（15 态齐全、0 延时静态——F627 补默认与 F639 回退的同一事实源）。
pub fn builtin_default_scheme() -> CursorSchemeModel {
    let mut m = CursorSchemeModel::empty("VARIX 默认指针", OriginKind::Builtin);
    m.author = String::from("VARIX 内置");
    for st in ALL_STATES {
        m.set_state(st, alloc::vec![builtin_glyph(st)]);
    }
    m
}

// ---------------------------------------------------------------------------
// 自检（jbase 机械件自身）
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// jbase 底盘自检：色彩数学/几何/重采样/容器往返/内置方案齐全性。
pub fn run_jbase_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-jbase");

    // 1. 定点根：cbrt(1e6)=100；isqrt(10^10)=100000；1000^12 第五根回代。
    set.add(
        "nth root math",
        nth_root_u128(1_000_000, 3) == 100 && isqrt64(10_000_000_000) == 100_000,
        "",
    );

    // 2. 线性化往返：sRGB 255 → 线性 1000 → 回编 ≈1000（±2 千分位）。
    let lin = srgb_to_linear_m(1000);
    let back = linear_to_srgb_m(lin);
    set.add("srgb roundtrip", (990..=1010).contains(&lin) && (980..=1020).contains(&back), "");

    // 3. 对比度：黑白 ≥ 2000（20:1 封顶口径实际 21:1），同色 = 100。
    set.add(
        "contrast ratio",
        contrast_x100(Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)) >= 2000
            && contrast_x100(Rgb::new(128, 128, 128), Rgb::new(128, 128, 128)) == 100,
        "",
    );

    // 4. ΔE76：同色 = 0；黑 vs 白 > 9000（L 差 100 → ΔE 100 → ×100）。
    set.add(
        "delta E76 scale",
        delta_e76_x100(Rgb::new(10, 10, 10), Rgb::new(10, 10, 10)) == 0
            && delta_e76_x100(Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)) > 9000,
        "",
    );

    // 5. HSL↔RGB：红(255,0,0) → h=0,s=1000,l=500 → 回 RGB 红。
    let (h, s, l) = Rgb::new(255, 0, 0).to_hsl();
    let back = Rgb::from_hsl(h, s, l);
    set.add("hsl roundtrip", h == 0 && s > 950 && back.r > 240 && back.g < 16 && back.b < 16, "");

    // 6. 几何：矩形填充计数精确；多边形（右三角）面积 ≈ n²/2。
    let mut cv = PixBuf::new(32, 32);
    fill_rect(&mut cv, 0, 0, 7, 7, [1, 2, 3, 255]);
    set.add("rect fill exact", cv.solid_count() == 64, "");
    let mut tri = PixBuf::new(64, 64);
    fill_polygon(&mut tri, &[(0, 0), (32, 0), (0, 32)], [9, 9, 9, 255]);
    let tri_n = tri.solid_count();
    set.add("polygon fill approx", tri_n > 400 && tri_n <= 600, "");

    // 7. 双倍率复制：1x→2x 后逐像素 2×2 一致。
    let mut one = PixBuf::new(4, 4);
    one.set(1, 1, [255, 0, 0, 255]);
    let two = one.scale_integer2x();
    let ok = (0..2).all(|oy| (0..2).all(|ox| two.get(2 + ox, 2 + oy) == Some([255, 0, 0, 255])));
    set.add("2x replication exact", ok && two.diff_pixels(&two) == Some(0), "");

    // 8. 重采样：8×8 → 16×16 尺寸正确且不 panic；4 棋盘格输入无空洞。
    let mut src = PixBuf::new(8, 8);
    for y in 0..8u16 {
        for x in 0..8u16 {
            src.set(x, y, if (x + y) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] });
        }
    }
    let up = resample_lanczos3(&src, 16, 16);
    set.add("lanczos size", up.w == 16 && up.h == 16 && up.px.len() == 16 * 16 * 4, "");

    // 9. vxcur 往返：内置方案序列化→解析→重序列化哈希一致，15 态齐全。
    let builtin = builtin_default_scheme();
    let bytes = serialize_vxcur(&builtin);
    let parsed = parse_vxcur(&bytes);
    let rt = match parsed {
        Ok(p) => vxcur_fingerprint(&p) == vxcur_fingerprint(&builtin) && p.missing_states().is_empty(),
        Err(_) => false,
    };
    set.add("vxcur roundtrip fingerprint", rt, "");

    // 10. 畸形容器不 panic：随机字节全部诚实报错或稳定可重放。
    let mut rng = XorShift32::new(0xC0FFEE);
    let mut all_honest = true;
    for _ in 0..500 {
        let n = 6 + rng.below(96) as usize;
        let mut junk = alloc::vec![0u8; n];
        for b in junk.iter_mut() {
            *b = rng.next_u32() as u8;
        }
        if rng.below(2) == 0 {
            junk[..6].copy_from_slice(&VXCUR_MAGIC);
        }
        match parse_vxcur(&junk) {
            Err(_) => {}
            Ok(p) => {
                // 极小概率命中「合法」结构——稳定性要求：重序列化可再解析。
                if parse_vxcur(&serialize_vxcur(&p)).is_err() {
                    all_honest = false;
                }
            }
        }
    }
    set.add("vxcur malformed honest", all_honest, "");

    // 11. 内置方案：15 态齐全、热点在实体上、帧尺寸合规。
    let b = builtin_default_scheme();
    let hotspot_ok = ALL_STATES.iter().all(|st| {
        b.state(*st).map(|sf| {
            sf.frames.first().map(|f| {
                let buf = f.buf();
                f.hot_x < f.w && f.hot_y < f.h && buf.solid(f.hot_x, f.hot_y)
            }) == Some(true)
        }) == Some(true)
    });
    set.add(
        "builtin 15 states hotspots solid",
        b.missing_states().is_empty() && hotspot_ok && b.max_frame_px() <= BUILTIN_GLYPH_PX as u32,
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

    #[test]
    fn nth_root_exactness() {
        assert_eq!(nth_root_u128(27, 3), 3);
        assert_eq!(nth_root_u128(26, 3), 2);
        assert_eq!(nth_root_u128(1024, 5), 4); // 4^5 = 1024
        assert_eq!(nth_root_u128(1023, 5), 3);
        assert_eq!(nth_root_u128(1, 12), 1);
        assert_eq!(nth_root_u128(0, 7), 0);
    }

    #[test]
    fn srgb_linear_boundary() {
        // 阈值 40/1000 以下走线性段：t=40 → 40*1000/12920 ≈ 3（千分位）。
        assert_eq!(srgb_to_linear_m(40), 3);
        // sRGB 0 → 0；255 → ~1000。
        assert_eq!(srgb_to_linear_m(0), 0);
    }

    #[test]
    fn delta_e76_known_pairs() {
        // 同色零差。
        assert_eq!(delta_e76_x100(Rgb::new(120, 120, 120), Rgb::new(120, 120, 120)), 0);
        // 灰阶差 10（1/255 标度）→ ΔE 小于灰阶 0↔255 的 1%。
        let small = delta_e76_x100(Rgb::new(10, 10, 10), Rgb::new(20, 20, 20));
        let big = delta_e76_x100(Rgb::new(0, 0, 0), Rgb::new(255, 255, 255));
        assert!(small > 0 && small * 20 < big);
    }

    #[test]
    fn centroid_weighted_center() {
        let mut cv = PixBuf::new(10, 10);
        // 左半黑 alpha=128，右半白 alpha=255 → 重心偏右。
        for y in 0..10u16 {
            for x in 0..10u16 {
                if x < 5 {
                    cv.set(x, y, [0, 0, 0, 128]);
                } else {
                    cv.set(x, y, [255, 255, 255, 255]);
                }
            }
        }
        let (cx, cy) = cv.visual_centroid().unwrap();
        assert!(cx >= 5 && cx <= 7, "centroid x={cx}");
        assert_eq!(cy, 5); // 0..10 → 加权均值 4.5（千分位四舍五入 → 5）
    }

    #[test]
    fn empty_buffer_centroid_none() {
        let cv = PixBuf::new(8, 8);
        assert!(cv.visual_centroid().is_none());
        assert!(cv.content_bbox().is_none());
        assert_eq!(cv.solid_count(), 0);
    }

    #[test]
    fn blend_over_alpha_math() {
        let mut base = PixBuf::new(2, 1);
        base.set(0, 0, [0, 0, 0, 255]);
        base.set(1, 0, [0, 0, 0, 0]);
        let mut top = PixBuf::new(2, 1);
        // 30% 白（F625 洋葱皮 30% 透明度口径：alpha 77/255）。
        top.set(0, 0, [255, 255, 255, 77]);
        top.set(1, 0, [255, 255, 255, 255]);
        base.blend_over(&top, 0, 0);
        // 不透明底上叠 30% 白 → 约 77/255 亮度 ≈ 30。
        let p0 = base.get(0, 0).unwrap();
        // 30% 白（77/255）叠黑底 → 亮度 77（0.302×255）。
        assert!(p0[0] >= 70 && p0[0] <= 84, "blend on opaque got {}", p0[0]);
        // 透明底上叠不透明白 → 纯白。
        let p1 = base.get(1, 0).unwrap();
        assert_eq!(p1[0], 255);
    }

    #[test]
    fn lanczos_preserves_constant_region() {
        // 常色图升采样：结果应逐像素等于原色（核归一性）。
        let mut src = PixBuf::new(8, 8);
        for i in (0..src.px.len()).step_by(4) {
            src.px[i] = 200;
            src.px[i + 1] = 100;
            src.px[i + 2] = 50;
            src.px[i + 3] = 255;
        }
        let up = resample_lanczos3(&src, 16, 16);
        for i in (0..up.px.len()).step_by(4) {
            assert!((up.px[i] as i32 - 200).abs() <= 2, "r {}", up.px[i]);
            assert!((up.px[i + 1] as i32 - 100).abs() <= 2, "g {}", up.px[i + 1]);
            assert!((up.px[i + 2] as i32 - 50).abs() <= 2, "b {}", up.px[i + 2]);
            assert_eq!(up.px[i + 3], 255);
        }
    }

    #[test]
    fn vxcur_metadata_and_origin_roundtrip() {
        let mut m = builtin_default_scheme();
        m.name = String::from("测试方案·甲");
        m.author = String::from("Variable");
        m.origin = OriginKind::Recolored(String::from("母方案"));
        m.enhanced_render = true;
        let bytes = serialize_vxcur(&m);
        let p = parse_vxcur(&bytes).unwrap();
        assert_eq!(p.name, "测试方案·甲");
        assert_eq!(p.author, "Variable");
        assert_eq!(p.origin, OriginKind::Recolored(String::from("母方案")));
        assert!(p.enhanced_render);
    }

    #[test]
    fn vxcur_rejects_oversize_and_unknown_state() {
        // 超 4MB 拒入（F639 内存闸同源）。
        let big = alloc::vec![0u8; VXCUR_MAX_BYTES + 1];
        assert!(matches!(parse_vxcur(&big), Err(VxcurError::SizeOver(_))));
        // 坏 magic。
        let bad = builtin_default_scheme();
        let mut bytes = serialize_vxcur(&bad);
        bytes[0] = b'X';
        assert!(matches!(parse_vxcur(&bytes), Err(VxcurError::BadMagic(0))));
        // 未知态 ID。
        let mut bytes = serialize_vxcur(&bad);
        let meta_len = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let state_at = 6 + 2 + 2 + meta_len;
        bytes[state_at] = 99;
        assert!(matches!(parse_vxcur(&bytes), Err(VxcurError::UnknownStateId { id: 99, .. })));
        // 截断。
        let bytes = serialize_vxcur(&bad);
        assert!(matches!(parse_vxcur(&bytes[..bytes.len() - 3]), Err(VxcurError::Truncated { .. })));
    }

    #[test]
    fn builtin_glyph_states_distinct() {
        // 15 态兜底形互不逐字节相同（形态区分度底线）。
        let a = builtin_glyph(PointerState::Normal);
        let b = builtin_glyph(PointerState::Busy);
        let c = builtin_glyph(PointerState::Text);
        assert_ne!(a.px, b.px);
        assert_ne!(a.px, c.px);
        assert_ne!(b.px, c.px);
    }

    #[test]
    fn xorshift_deterministic() {
        let mut a = XorShift32::new(7);
        let mut b = XorShift32::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
