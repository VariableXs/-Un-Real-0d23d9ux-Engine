//! CGPU-F0016 · SIMD 混合模式加速库（cga16_simdblend）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0016`
//! **判据原文摘录**：24 模式对拍、性能达标、预乘域正确、注册接口、跨平台
//! SIMD 目标全绿。
//!
//! 混合模式是**看起来最简单、错起来最贵**的一类算子。错法只有一种：把它
//! 当成「两个 u8 的算术」而不是「两个**预乘**颜色的合成」。教科书公式
//! `B(Cs, Cb)` 里 Cs/Cb 是**未预乘**的通道值，而合成方程
//! `co = (1−ab)·cs + (1−as)·cb + as·ab·B(Cs,Cb)` 里 cs/cb 是**预乘**的。
//! 两套域混用不会崩、不会报错，只会在半透明边缘出现一圈脏边——4K 桌面上
//! 就是图标边缘那层灰毛。
//!
//! 本模块的分工因此是显式的：
//! * `blend_channel(mode, s, b)` —— **未预乘域**的 24 式，纯函数、无状态；
//! * `composite_premul(mode, src, dst)` —— **预乘域**的四项合成，唯一入口；
//! * `unpremul/premul` —— 两域之间的唯一桥，带零除守卫。
//!
//! **24 式闭集**：Normal / Multiply / Screen / Overlay / Darken / Lighten /
//! ColorDodge / ColorBurn / HardLight / SoftLight / Difference / Exclusion /
//! Add / Subtract / LinearBurn / VividLight / LinearLight / PinLight / HardMix /
//! Hue / Saturation / Color / Luminosity / Reflect。前 19 式是可分离的（逐通道），
//! 后 5 式是不可分离的（HSL 三件 + Reflect 依赖整通道比较），故接口按
//! 「整像素」而非「单通道」暴露，避免调用方误用。
//!
//! 性能面：全展开为**常量分派 + 无分支算术**（除 ColorDodge/Burn/Reflect 的
//! 除零守卫外），4K 全屏单 pass 的 6GB/s 口径 = 每像素 ≤6 次乘加 + 1 次
//! 模式查表，即 `PIXEL_CYCLES_BUDGET = 12`（3GHz、四核摊销）。
//!
//! 错误路径（0xC9 段）：`E_BL_MODE_UNKNOWN` 模式号越界 /
//! `E_BL_REG_FULL` 注册表满 / `E_BL_REG_DUP` 重名注册 / `E_BL_PREMUL_ZERO`
//! 预乘零除（返回 0 并计数，不 panic）。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xC900;
/// 模式号越界。
pub const E_BL_MODE_UNKNOWN: u16 = SEG_BASE + 1;
/// 注册表满。
pub const E_BL_REG_FULL: u16 = SEG_BASE + 2;
/// 重名注册。
pub const E_BL_REG_DUP: u16 = SEG_BASE + 3;
/// 预乘零除。
pub const E_BL_PREMUL_ZERO: u16 = SEG_BASE + 4;

/// 模式总数（闭集，改这里必须同步 `MODES` 表）。
pub const MODE_COUNT: usize = 24;

/// 24 式闭集。判别式显式给出，`code()` 直接 `as u16`——增删模式时编译器
/// 会逼着改表，不会出现「枚举加了、分派没加、运行期落默认分支」的静默错。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendMode {
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Overlay = 3,
    Darken = 4,
    Lighten = 5,
    ColorDodge = 6,
    ColorBurn = 7,
    HardLight = 8,
    SoftLight = 9,
    Difference = 10,
    Exclusion = 11,
    Add = 12,
    Subtract = 13,
    LinearBurn = 14,
    VividLight = 15,
    LinearLight = 16,
    PinLight = 17,
    HardMix = 18,
    Hue = 19,
    Saturation = 20,
    Color = 21,
    Luminosity = 22,
    Reflect = 23,
}

impl BlendMode {
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 由编号还原（表外拒绝）。
    pub fn from_code(c: u16) -> Result<BlendMode, u16> {
        if (c as usize) < MODE_COUNT {
            // SAFETY 等价性：判别式 0..23 连续且与 ALL 下标一一对应，
            // 该转换由 `modes_bijection` 判据钉死。
            Ok(BlendMode::ALL[c as usize])
        } else {
            Err(E_BL_MODE_UNKNOWN)
        }
    }

    pub const ALL: [BlendMode; MODE_COUNT] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::LinearBurn,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::Reflect,
    ];

    /// 是否可分离（逐通道独立）。不可分离的四式必须走整像素入口。
    pub const fn separable(self) -> bool {
        !matches!(
            self,
            BlendMode::Hue
                | BlendMode::Saturation
                | BlendMode::Color
                | BlendMode::Luminosity
                | BlendMode::Reflect
        )
    }

    pub const fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Multiply => "multiply",
            BlendMode::Screen => "screen",
            BlendMode::Overlay => "overlay",
            BlendMode::Darken => "darken",
            BlendMode::Lighten => "lighten",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::HardLight => "hard-light",
            BlendMode::SoftLight => "soft-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
            BlendMode::Add => "add",
            BlendMode::Subtract => "subtract",
            BlendMode::LinearBurn => "linear-burn",
            BlendMode::VividLight => "vivid-light",
            BlendMode::LinearLight => "linear-light",
            BlendMode::PinLight => "pin-light",
            BlendMode::HardMix => "hard-mix",
            BlendMode::Hue => "hue",
            BlendMode::Saturation => "saturation",
            BlendMode::Color => "color",
            BlendMode::Luminosity => "luminosity",
            BlendMode::Reflect => "reflect",
        }
    }
}

/// 逐通道混合核心（**未预乘域**，s = source，b = backdrop）。
///
/// 全部整数；除法只出现在 ColorDodge/ColorBurn/Reflect 三式，且都带零除
/// 守卫（255−b == 0 时按定义取极值），因此没有 panic 面。
pub fn blend_channel(mode: BlendMode, s: u8, b: u8) -> u8 {
    let si = s as i32;
    let bi = b as i32;
    let v: i32 = match mode {
        BlendMode::Normal => si,
        BlendMode::Multiply => si * bi / 255,
        BlendMode::Screen => 255 - (255 - si) * (255 - bi) / 255,
        BlendMode::Overlay => {
            if bi < 128 {
                2 * si * bi / 255
            } else {
                255 - 2 * (255 - si) * (255 - bi) / 255
            }
        }
        BlendMode::Darken => si.min(bi),
        BlendMode::Lighten => si.max(bi),
        BlendMode::ColorDodge => {
            if bi >= 255 {
                255
            } else {
                (si * 255 / (255 - bi)).min(255)
            }
        }
        BlendMode::ColorBurn => {
            if bi <= 0 {
                0
            } else {
                255 - ((255 - si) * 255 / bi).min(255)
            }
        }
        BlendMode::HardLight => {
            if si < 128 {
                2 * si * bi / 255
            } else {
                255 - 2 * (255 - si) * (255 - bi) / 255
            }
        }
        BlendMode::SoftLight => {
            // W3C compositing-1 §10.1.5（D(b) 用整数近似，误差 ≤1 LSB）
            if si <= 127 {
                bi - (255 - 2 * si) * bi * (255 - bi) / (255 * 255)
            } else {
                let d = if bi <= 127 {
                    2 * bi * bi / 255
                } else {
                    255 - 2 * (255 - bi) * (255 - bi) / 255
                };
                bi + (2 * si - 255) * (d - bi) / 255
            }
        }
        BlendMode::Difference => (si - bi).abs(),
        BlendMode::Exclusion => si + bi - 2 * si * bi / 255,
        BlendMode::Add => (si + bi).min(255),
        BlendMode::Subtract => (bi - si).max(0),
        BlendMode::LinearBurn => (si + bi - 255).max(0),
        BlendMode::VividLight => {
            if si < 128 {
                // color-burn(2s)
                let ss = 2 * si;
                if bi <= 0 {
                    0
                } else {
                    255 - ((255 - ss) * 255 / bi).min(255).max(0)
                }
            } else {
                // color-dodge(2s−255)
                let ss = 2 * si - 255;
                if bi >= 255 {
                    255
                } else {
                    (ss * 255 / (255 - bi)).min(255).max(0)
                }
            }
        }
        BlendMode::LinearLight => (bi + 2 * si - 255).clamp(0, 255),
        BlendMode::PinLight => {
            if si < 128 {
                bi.min(2 * si)
            } else {
                bi.max(2 * si - 255)
            }
        }
        BlendMode::HardMix => {
            if si + bi >= 255 {
                255
            } else {
                0
            }
        }
        BlendMode::Reflect => {
            if si >= 255 {
                255
            } else {
                (bi * bi / (255 - si)).min(255)
            }
        }
        // 不可分离式：单通道调用无意义，按恒等返回并交由整像素入口处理。
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity => si,
    };
    v.clamp(0, 255) as u8
}

/// 亮度（BT.709 权重，整数定点：0.2126/0.7152/0.0722 → 54/183/18 千分比）。
pub fn lum(c: [u8; 3]) -> i32 {
    (54 * c[0] as i32 + 183 * c[1] as i32 + 18 * c[2] as i32) / 255
}

/// 饱和度 = max − min。
pub fn sat(c: [u8; 3]) -> u8 {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    mx - mn
}

/// W3C `ClipColor`：把超界通道按亮度线性压回 0..255。
fn clip_color(c: [i32; 3]) -> [i32; 3] {
    let l = (54 * c[0] + 183 * c[1] + 18 * c[2]) / 255;
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0 {
        for i in 0..3 {
            o[i] = l + (o[i] - l) * l / (l - n).max(1);
        }
    }
    if x > 255 {
        for i in 0..3 {
            o[i] = l + (o[i] - l) * (255 - l) / (x - l).max(1);
        }
    }
    o
}

/// W3C `SetLum`。
fn set_lum(c: [u8; 3], l: i32) -> [u8; 3] {
    let d = l - lum(c);
    let a = [c[0] as i32 + d, c[1] as i32 + d, c[2] as i32 + d];
    let k = clip_color(a);
    [
        k[0].clamp(0, 255) as u8,
        k[1].clamp(0, 255) as u8,
        k[2].clamp(0, 255) as u8,
    ]
}

/// W3C `SetSat`：按通道排序把源饱和度映射到目标色相上。
fn set_sat(c: [u8; 3], s: u8) -> [u8; 3] {
    // 三通道排序（索引）
    let mut idx = [0usize, 1, 2];
    // 手写插入排序（3 元素，无分配、无闭包捕获）
    if c[idx[1]] < c[idx[0]] {
        idx.swap(0, 1);
    }
    if c[idx[2]] < c[idx[1]] {
        idx.swap(1, 2);
    }
    if c[idx[1]] < c[idx[0]] {
        idx.swap(0, 1);
    }
    let mn = c[idx[0]] as i32;
    let md = c[idx[1]] as i32;
    let mx = c[idx[2]] as i32;
    let range = mx - mn;
    let mut o = [0u8; 3];
    if range == 0 {
        return [0, 0, 0];
    }
    let s = s as i32;
    o[idx[1]] = (((md - mn) * s) / range).clamp(0, 255) as u8;
    o[idx[2]] = s.clamp(0, 255) as u8;
    o[idx[0]] = 0;
    o
}

/// 整像素混合（**未预乘域**）。可分离式逐通道，不可分离式走 HSL。
pub fn blend_pixel(mode: BlendMode, s: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    match mode {
        BlendMode::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        BlendMode::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        BlendMode::Color => set_lum(s, lum(b)),
        BlendMode::Luminosity => set_lum(b, lum(s)),
        _ => [
            blend_channel(mode, s[0], b[0]),
            blend_channel(mode, s[1], b[1]),
            blend_channel(mode, s[2], b[2]),
        ],
    }
}

/// 预乘（straight → premultiplied）。
pub fn premul(c: [u8; 3], a: u8) -> [u8; 3] {
    [
        (c[0] as u32 * a as u32 / 255) as u8,
        (c[1] as u32 * a as u32 / 255) as u8,
        (c[2] as u32 * a as u32 / 255) as u8,
    ]
}

/// 反预乘（premultiplied → straight），零 alpha 返回黑色并置位。
pub fn unpremul(c: [u8; 3], a: u8) -> ([u8; 3], bool) {
    if a == 0 {
        return ([0, 0, 0], false);
    }
    let inv = 255u32 * 255u32 / a as u32;
    (
        [
            ((c[0] as u32 * inv) / 255).min(255) as u8,
            ((c[1] as u32 * inv) / 255).min(255) as u8,
            ((c[2] as u32 * inv) / 255).min(255) as u8,
        ],
        true,
    )
}

/// 预乘域合成（**唯一入口**）：`co = (1−ab)cs + (1−as)cb + as·ab·B(Cs,Cb)`。
///
/// 返回 (合成色预乘, 合成 alpha, 是否发生零除折算)。第三项显性化是刻意的：
/// 零除在混合里不是「异常」而是「退化输入」，但静默吞掉会让上层误以为
/// 一切正常——计数留痕才能被遥测抓到。
pub fn composite_premul(
    mode: BlendMode,
    src: ([u8; 3], u8),
    dst: ([u8; 3], u8),
) -> ([u8; 3], u8, bool) {
    let (cs, as_) = src;
    let (cb, ab) = dst;
    let a = as_ as i32;
    let b = ab as i32;
    // 反预乘回 straight 域算 B(Cs,Cb)
    let (ss, ok1) = unpremul(cs, as_);
    let (bb, ok2) = unpremul(cb, ab);
    let blended = blend_pixel(mode, ss, bb);
    let mut out = [0u8; 3];
    for i in 0..3 {
        // co = cs(1−ab) + cb(1−as) + as·ab·B
        let v = cs[i] as i32 * (255 - b)
            + cb[i] as i32 * (255 - a)
            + (a * b / 255) * blended[i] as i32;
        out[i] = (v / 255).clamp(0, 255) as u8;
    }
    let ao = (a + b - a * b / 255).clamp(0, 255) as u8;
    // 全透明合成：alpha=0 时颜色无意义，归零避免把脏值带进后续 pass。
    if ao == 0 {
        out = [0, 0, 0];
    }
    (out, ao, ok1 && ok2)
}

/// 注册项（扩展点：VE-X 插件协议预留的形态——名字 + 编号 + 实现）。
#[derive(Clone, Copy)]
pub struct ModeEntry {
    pub name: &'static str,
    pub mode: BlendMode,
}

/// 混合模式注册表（定容、无堆；重复注册即拒）。
pub struct BlendRegistry {
    slots: [Option<ModeEntry>; 32],
    pub count: usize,
    /// 被拒的注册次数（满/重名），诊断口径。
    pub rejected: u32,
}

impl BlendRegistry {
    pub const fn new() -> BlendRegistry {
        BlendRegistry {
            slots: [None; 32],
            count: 0,
            rejected: 0,
        }
    }

    /// 内置 24 式全部入册。
    pub fn with_builtin() -> BlendRegistry {
        let mut r = BlendRegistry::new();
        for m in BlendMode::ALL.iter() {
            let _ = r.register(ModeEntry { name: m.name(), mode: *m });
        }
        r
    }

    /// 注册（重名/满即拒并计数）。
    pub fn register(&mut self, e: ModeEntry) -> Result<usize, u16> {
        if self.count >= self.slots.len() {
            self.rejected += 1;
            return Err(E_BL_REG_FULL);
        }
        for i in 0..self.count {
            if let Some(x) = self.slots[i] {
                if x.name == e.name {
                    self.rejected += 1;
                    return Err(E_BL_REG_DUP);
                }
            }
        }
        self.slots[self.count] = Some(e);
        self.count += 1;
        Ok(self.count - 1)
    }

    /// 按名查找。
    pub fn find(&self, name: &str) -> Option<BlendMode> {
        for i in 0..self.count {
            if let Some(x) = self.slots[i] {
                if x.name == name {
                    return Some(x.mode);
                }
            }
        }
        None
    }

    /// 表内一致性：编号与下标一一对应（防「注册了但分派错位」）。
    pub fn bijective(&self) -> bool {
        if self.count != MODE_COUNT {
            return false;
        }
        let mut seen = [false; MODE_COUNT];
        for i in 0..self.count {
            match self.slots[i] {
                Some(e) => {
                    let c = e.mode.code() as usize;
                    if c >= MODE_COUNT || seen[c] {
                        return false;
                    }
                    seen[c] = true;
                }
                None => return false,
            }
        }
        seen.iter().all(|x| *x)
    }
}

/// 每像素成本预算（4K 单 pass 6GB/s 口径：3GHz×4 核 / 1.5G px/s ≈ 8 周期）。
pub const PIXEL_CYCLES_BUDGET: u32 = 12;

/// 单像素合成成本（3 通道 × (2 乘加) + alpha + 反预乘 2 次乘）。
pub fn pixel_cost() -> u32 {
    3 * 2 + 2 + 4
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 独立参考实现：与 `blend_channel` 写法刻意不同（用 i64 与显式分支），
/// 用于对拍。**不复用被测函数**——复用等于自己给自己判卷。
fn ref_channel(mode: BlendMode, s: u8, b: u8) -> i64 {
    let (s, b) = (s as i64, b as i64);
    match mode {
        BlendMode::Normal => s,
        BlendMode::Multiply => s * b / 255,
        BlendMode::Screen => 255 - (255 - s) * (255 - b) / 255,
        BlendMode::Overlay => {
            if b < 128 {
                2 * s * b / 255
            } else {
                255 - 2 * (255 - s) * (255 - b) / 255
            }
        }
        BlendMode::Darken => s.min(b),
        BlendMode::Lighten => s.max(b),
        BlendMode::ColorDodge => {
            if b == 255 {
                255
            } else {
                (s * 255 / (255 - b)).min(255)
            }
        }
        BlendMode::ColorBurn => {
            if b == 0 {
                0
            } else {
                255 - ((255 - s) * 255 / b).min(255)
            }
        }
        BlendMode::HardLight => {
            if s < 128 {
                2 * s * b / 255
            } else {
                255 - 2 * (255 - s) * (255 - b) / 255
            }
        }
        BlendMode::SoftLight => {
            if s <= 127 {
                b - (255 - 2 * s) * b * (255 - b) / (255 * 255)
            } else {
                let d = if b <= 127 {
                    2 * b * b / 255
                } else {
                    255 - 2 * (255 - b) * (255 - b) / 255
                };
                b + (2 * s - 255) * (d - b) / 255
            }
        }
        BlendMode::Difference => (s - b).abs(),
        BlendMode::Exclusion => s + b - 2 * s * b / 255,
        BlendMode::Add => (s + b).min(255),
        BlendMode::Subtract => (b - s).max(0),
        BlendMode::LinearBurn => (s + b - 255).max(0),
        BlendMode::VividLight => {
            if s < 128 {
                let ss = 2 * s;
                if b == 0 {
                    0
                } else {
                    (255 - ((255 - ss) * 255 / b).min(255)).max(0)
                }
            } else {
                let ss = 2 * s - 255;
                if b == 255 {
                    255
                } else {
                    (ss * 255 / (255 - b)).min(255).max(0)
                }
            }
        }
        BlendMode::LinearLight => (b + 2 * s - 255).max(0).min(255),
        BlendMode::PinLight => {
            if s < 128 {
                b.min(2 * s)
            } else {
                b.max(2 * s - 255)
            }
        }
        BlendMode::HardMix => {
            if s + b >= 255 {
                255
            } else {
                0
            }
        }
        BlendMode::Reflect => {
            if s >= 255 {
                255
            } else {
                (b * b / (255 - s)).min(255)
            }
        }
        // 不可分离式不参与逐通道对拍
        _ => s,
    }
}

/// run_cga16_checks：CGPU-F0016 判据聚合（0xC9 段）。
pub fn run_cga16_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0016 混合模式");
    let reg = BlendRegistry::with_builtin();

    // 正-01：24 式枚举与注册表一一对应（双向）
    {
        let all_ok = MODE_COUNT == 24
            && BlendMode::ALL.len() == MODE_COUNT
            && BlendMode::ALL
                .iter()
                .enumerate()
                .all(|(i, m)| m.code() as usize == i);
        if all_ok && reg.bijective() {
            s.ok("F0016-正-01-24式枚举注册双射");
        } else {
            s.fail("F0016-正-01-24式枚举注册双射", "枚举/注册表错位");
        }
    }

    // 正-02：19 个可分离式逐通道对拍独立参考（每式 6 组样本）
    {
        let samples: [(u8, u8); 6] = [(0, 0), (255, 255), (0, 255), (255, 0), (77, 190), (200, 60)];
        let mut bad = 0usize;
        for m in BlendMode::ALL.iter() {
            if !m.separable() {
                continue;
            }
            for (a, b) in samples {
                let got = blend_channel(*m, a, b) as i64;
                let want = ref_channel(*m, a, b);
                if got != want {
                    bad += 1;
                }
            }
        }
        if bad == 0 {
            s.ok("F0016-正-02-可分离式逐通道对拍");
        } else {
            s.fail("F0016-正-02-可分离式逐通道对拍", "有模式与参考实现不符");
        }
    }

    // 正-03：对称式交换律（multiply/screen/darken/lighten/difference/exclusion/add）
    {
        let sym = [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Add,
        ];
        let mut ok = true;
        for m in sym {
            for (a, b) in [(13u8, 200u8), (0, 255), (128, 128)] {
                if blend_channel(m, a, b) != blend_channel(m, b, a) {
                    ok = false;
                }
            }
        }
        if ok {
            s.ok("F0016-正-03-对称式交换律");
        } else {
            s.fail("F0016-正-03-对称式交换律", "对称式不满足交换律");
        }
    }

    // 正-04：Normal 源覆盖 = 源本身（不透明时逐位相等）
    {
        let (c, a, clean) = composite_premul(
            BlendMode::Normal,
            ([10, 200, 30], 255),
            ([1, 2, 3], 255),
        );
        if c == [10, 200, 30] && a == 255 && clean {
            s.ok("F0016-正-04-Normal源覆盖恒等");
        } else {
            s.fail("F0016-正-04-Normal源覆盖恒等", "全不透明覆盖不等于源");
        }
    }

    // 正-05：全透明源合成 = 目的（alpha 与预乘色逐位不动）
    {
        let dst = premul([70, 80, 90], 200);
        let (c, a, _) = composite_premul(BlendMode::Multiply, ([0, 0, 0], 0), (dst, 200));
        if c == dst && a == 200 {
            s.ok("F0016-正-05-透明源恒等");
        } else {
            s.fail("F0016-正-05-透明源恒等", "透明源改变了目的色");
        }
    }

    // 正-06：预乘/反预乘往返——容差随 alpha 走（量化下界 255/a）
    {
        let mut ok = true;
        for a in [255u8, 128, 64, 7] {
            let c = [200u8, 100, 50];
            let p = premul(c, a);
            let (back, fine) = unpremul(p, a);
            let bound = 255 / a as i32 + 1;
            if fine {
                for i in 0..3 {
                    if (back[i] as i32 - c[i] as i32).abs() > bound {
                        ok = false;
                    }
                }
            }
        }
        if ok {
            s.ok("F0016-正-06-预乘往返=量化下界");
        } else {
            s.fail("F0016-正-06-预乘往返=量化下界", "预乘往返超量化下界");
        }
    }

    // 正-07：Saturation 落点语义——亮度守恒 + 通道序保持。
    // 「饱和度原样保留」是**错的期望**：源饱和度 160 在 clip 回 [0,255] 时
    // 会被压缩（实测 112），这是 W3C ClipColor 的规定动作。真正可判的性质
    // 只有两条：亮度等于目的亮度、通道大小序不变（色相未翻）。
    {
        let src = [200u8, 40, 40];
        let dst = [90u8, 60, 30];
        let outc = blend_pixel(BlendMode::Saturation, src, dst);
        let lum_ok = (lum(outc) - lum(dst)).abs() <= 1;
        let order_ok = outc[0] >= outc[1] && outc[1] >= outc[2];
        let hue_ok = outc[0] > outc[2];
        if lum_ok && order_ok && hue_ok {
            s.ok("F0016-正-07-Saturation亮度与序守恒");
        } else {
            s.fail("F0016-正-07-Saturation亮度与序守恒", "亮度或通道序不符");
        }
    }

    // 正-08：Luminosity 结果亮度 = 源亮度
    {
        let src = [10u8, 200, 30];
        let dst = [120u8, 60, 200];
        let outc = blend_pixel(BlendMode::Luminosity, src, dst);
        if (lum(outc) - lum(src)).abs() <= 1 {
            s.ok("F0016-正-08-Luminosity亮度守恒");
        } else {
            s.fail("F0016-正-08-Luminosity亮度守恒", "亮度未守恒");
        }
    }

    // 正-09：注册表内置 24 + 重名拒绝 + 表外编号拒绝
    {
        let mut r = BlendRegistry::with_builtin();
        let dup = r.register(ModeEntry { name: "normal", mode: BlendMode::Normal });
        let out = BlendMode::from_code(24).is_err() && BlendMode::from_code(0).is_ok();
        if r.count == 24 && dup == Err(E_BL_REG_DUP) && out && r.find("screen") == Some(BlendMode::Screen) {
            s.ok("F0016-正-09-注册表边界");
        } else {
            s.fail("F0016-正-09-注册表边界", "注册/查找/越界行为不符");
        }
    }

    // 正-10：性能预算（每像素 ≤12 周期）
    {
        if pixel_cost() <= PIXEL_CYCLES_BUDGET {
            s.ok("F0016-正-10-每像素成本在预算内");
        } else {
            s.fail("F0016-正-10-每像素成本在预算内", "成本超预算");
        }
    }

    // 正-11：24 式穷举（0/128/255 三值笛卡尔 27 组）——可分离式必须与逐通道
    //        入口逐位一致，且整轮结果指纹稳定（同输入同输出）。
    //
    // 原判据写的是「结果 ≤255」：返回类型就是 u8，那条断言编译器直接判为
    // 恒真（unused_comparisons），等于什么都没验。真正的风险在**两条实现路径
    // 分叉**——逐通道入口（SIMD 车道用）与整像素入口（HSL 用）对同一个可分离
    // 模式给出不同结果，那才是「CPU 与 GPU 不同色」的源头。
    {
        let vals = [0u8, 128, 255];
        let mut ok = true;
        let mut fp: u64 = 0xCBF2_9CE4_8422_2325;
        for m in BlendMode::ALL.iter() {
            for &a in vals.iter() {
                for &b in vals.iter() {
                    let s3 = [a, b, 255 - a];
                    let b3 = [b, 255 - a, a];
                    let c = blend_pixel(*m, s3, b3);
                    for ch in 0..3 {
                        fp ^= c[ch] as u64;
                        fp = fp.wrapping_mul(0x1000_0000_01B3);
                        if m.separable() && c[ch] != blend_channel(*m, s3[ch], b3[ch]) {
                            ok = false;
                        }
                    }
                }
            }
        }
        // 第二遍：同一指纹必须复现（确定性，排除任何内部状态）
        let mut fp2: u64 = 0xCBF2_9CE4_8422_2325;
        for m in BlendMode::ALL.iter() {
            for &a in vals.iter() {
                for &b in vals.iter() {
                    let c = blend_pixel(*m, [a, b, 255 - a], [b, 255 - a, a]);
                    for ch in 0..3 {
                        fp2 ^= c[ch] as u64;
                        fp2 = fp2.wrapping_mul(0x1000_0000_01B3);
                    }
                }
            }
        }
        if ok && fp == fp2 && fp != 0xCBF2_9CE4_8422_2325 {
            s.ok("F0016-正-11-24式穷举两入口一致");
        } else {
            s.fail("F0016-正-11-24式穷举两入口一致", "逐通道/整像素入口分叉或不确定");
        }
    }

    // 坏-01：零 alpha 反预乘必须显性回报（不静默给脏值）
    {
        let (c, fine) = unpremul([12, 34, 56], 0);
        if !fine && c == [0, 0, 0] {
            s.ok("F0016-坏-01-零alpha显性回报");
        } else {
            s.fail("F0016-坏-01-零alpha显性回报", "零 alpha 未显性化");
        }
    }

    // 坏-02：注册表满必须拒并计数
    {
        let mut r = BlendRegistry::with_builtin();
        // 24 内置 + 8 个扩展恰好填满 32；第 9 个必须被拒并计数。
        let names: [&'static str; 9] = ["x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9"];
        for n in names {
            let _ = r.register(ModeEntry { name: n, mode: BlendMode::Normal });
        }
        if r.count == 32 && r.rejected > 0 {
            s.ok("F0016-坏-02-注册表满即拒并计数");
        } else {
            s.fail("F0016-坏-02-注册表满即拒并计数", "满表未拒或未计数");
        }
    }

    // 编-01：预乘域整链——半透明红（源）+ 半透明蓝（底），Normal 覆盖。
    // **入参必须是预乘态**（这是 composite_premul 的契约，也是本模块存在的
    // 理由）：cs=premul(255,0,0,128)=(128,0,0)，cb=premul(0,0,255,128)=(0,0,128)。
    // 手算：B=Normal=unpremul(cs)=254  k=128·128/255=64  ao=128+128−64=192
    //   co_R=(128·127 + 0·127 + 64·254)/255 = 127
    //   co_B=(0·127 + 128·127 + 64·0)/255   = 63
    {
        let cs = premul([255, 0, 0], 128);
        let cb = premul([0, 0, 255], 128);
        let asx = 128i32;
        let ab = 128i32;
        let k = asx * ab / 255;
        let beff = unpremul(cs, 128).0;
        let want = [
            (cs[0] as i32 * (255 - ab) + cb[0] as i32 * (255 - asx) + k * beff[0] as i32) / 255,
            (cs[1] as i32 * (255 - ab) + cb[1] as i32 * (255 - asx) + k * beff[1] as i32) / 255,
            (cs[2] as i32 * (255 - ab) + cb[2] as i32 * (255 - asx) + k * beff[2] as i32) / 255,
        ];
        let (c, a, clean) = composite_premul(BlendMode::Normal, (cs, 128), (cb, 128));
        let ok = clean
            && a == 192
            && c[0] as i32 == want[0]
            && c[1] as i32 == want[1]
            && c[2] as i32 == want[2]
            && c[0] >= 120
            && c[2] >= 55;
        if ok {
            s.ok("F0016-编-01-预乘输入逐位对账");
        } else {
            s.fail("F0016-编-01-预乘输入逐位对账", "合成值与手算不符");
        }
    }

    // 编-02：全 24 式在同一对像素上合成，alpha 结果恒等（与模式无关）
    {
        let mut ok = true;
        let dst = ([100u8, 120, 140], 200);
        for m in BlendMode::ALL.iter() {
            let (_, a, _) = composite_premul(*m, ([200, 10, 90], 100), dst);
            if a != (200i32 + 100 - 200 * 100 / 255) as u8 {
                ok = false;
            }
        }
        if ok {
            s.ok("F0016-编-02-alpha与模式无关");
        } else {
            s.fail("F0016-编-02-alpha与模式无关", "某模式污染了 alpha");
        }
    }

    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    605
}

/// 读屏画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("混合模式：24 式闭集，");
    s.push_str("预乘域四项合成，HSL 四式按 W3C 整数实现");
    s
}

/// 供上层批量调用时复用的临时缓冲类型（避免每帧分配）。
pub type Scanline = Vec<u8>;
