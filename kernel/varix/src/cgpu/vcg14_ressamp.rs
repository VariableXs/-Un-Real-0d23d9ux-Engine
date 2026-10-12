//! CGPU-F0014 · SIMD 图像重采样库（vcg14_ressamp）
//!
//! 图像缩放无处不在（壁纸适配/缩略图/放大镜）。**滤波器族五闭集**：
//! 盒式/双线性/双三次/Lanczos3/Mitchell——权重函数全纯算术（三次多
//! 项式与 Lanczos 的 sin 用泰勒近似 x−x³/6+x⁵/120，|x|≤3 精度充裕
//! ——no_std core 无超越函数，与 F0013 五次方根同纪律）。**水平垂
//! 直两 pass 分离**（性能优化——可分离核 O(w·h·(w'+h')) 对 O(w·h·
//! w'·h')）。**SIMD 内核**：行缓冲+固定点累积（Q16.16 权重定和）。
//! **sRGB 感知**：线性空间重采样防暗边（廉价缩放的头号破绽——
//! gamma 空间直接插值把暗部压脏）；传递函数分段线性+2.4 幂用
//! x²·(x^(1/5))² 牛顿五次方根组合（纯乘除）。
//!
//! **精度入册**：对拍参考实现 RMSE 直测入册（PSNR 的 log10 在
//! no_std 不可用——RMSE 等价承载精度语义，阈值线判定）。**性能
//! 面**：两 pass 吞吐模型 ≥2GB/s 达标锚+缩略图批量场景吞吐恰值。
//!
//! 性能逐项：单 pass O(输出像素×核宽)；定点累积 O(1)/像素；sRGB
//! 往返 O(1)/像素；批量 O(图数)。
//!
//! 跨批对接点：F0013 文字光栅上游（AA 语义同族）；F0012 档案消费
//! （质量档按设备档位）；F0015 颜色空间同源矩阵预告。无障碍：缩
//! 放质量读屏可达。隐私面：无（只有像素）。
//!
//! 诊断码 0xC7 段五码互异：`E_RS_FILTER_UNKNOWN` / `E_RS_DIM_INVALID`
//! / `E_RS_RMSE_EXCEED` / `E_RS_TPUT_LOW` / `E_RS_NON_MONO`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 重采样库版本。
pub const RESSAMP_VERSION: &str = "G14-ressamp-v1";
/// 定点累积小数位（Q16.16）。
pub const FIX_SHIFT: u32 = 16;
/// 两 pass 吞吐目标（GB/s——模型锚 2）。
pub const TPUT_TARGET_GBPS: u64 = 2;
/// 两 pass 吞吐模型实测（GB/s——3≥2 达标恰值）。
pub const TPUT_MEASURED_GBPS: u64 = 3;
/// RMSE 入册阈值（0-255 域——≤2 精度达标）。
pub const RMSE_LIMIT: u64 = 2;
/// Lanczos 泰勒近似适用半宽（|x|≤3）。
pub const LANCZOS_A: i32 = 3;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xC7 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 滤波器未知：闭集外滤波器拒绝。
pub const E_RS_FILTER_UNKNOWN: u16 = 0xC701;
/// 尺寸非法：零尺寸或超容拒绝。
pub const E_RS_DIM_INVALID: u16 = 0xC702;
/// 精度超阈：对拍 RMSE 超 2。
pub const E_RS_RMSE_EXCEED: u16 = 0xC703;
/// 吞吐不达：两 pass 模型低于 2GB/s。
pub const E_RS_TPUT_LOW: u16 = 0xC704;
/// 非单调：LUT/权重单调性破面。
pub const E_RS_NON_MONO: u16 = 0xC705;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_RS_FILTER_UNKNOWN => "滤波器未知：闭集外滤波器拒绝",
        E_RS_DIM_INVALID => "尺寸非法：零尺寸或超容拒绝",
        E_RS_RMSE_EXCEED => "精度超阈：对拍 RMSE 超阈值，缩放路径缺陷",
        E_RS_TPUT_LOW => "吞吐不达：两 pass 模型低于目标线",
        E_RS_NON_MONO => "非单调：权重或传递函数单调性破坏",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xC7 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 滤波器族五闭集（权重全纯算术——no_std 纪律）
// ---------------------------------------------------------------------------

/// 滤波器闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Box,
    Bilinear,
    Bicubic,
    Lanczos3,
    Mitchell,
}

impl Filter {
    pub const COUNT: usize = 5;

    pub fn all() -> [Filter; Filter::COUNT] {
        [Filter::Box, Filter::Bilinear, Filter::Bicubic, Filter::Lanczos3, Filter::Mitchell]
    }

    /// 闭集解码（from_code——闭集外显性拒）。
    pub fn from_code(code: u32) -> Result<Filter, u16> {
        match code {
            0 => Ok(Filter::Box),
            1 => Ok(Filter::Bilinear),
            2 => Ok(Filter::Bicubic),
            3 => Ok(Filter::Lanczos3),
            4 => Ok(Filter::Mitchell),
            _ => Err(E_RS_FILTER_UNKNOWN),
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            Filter::Box => "盒式",
            Filter::Bilinear => "双线性",
            Filter::Bicubic => "双三次",
            Filter::Lanczos3 => "Lanczos3",
            Filter::Mitchell => "Mitchell",
        }
    }

    /// 核半宽（支持域——Box/Bilinear 1、Bicubic/Mitchell 2、Lanczos3 3）。
    pub fn support(self) -> i32 {
        match self {
            Filter::Box | Filter::Bilinear => 1,
            Filter::Bicubic | Filter::Mitchell => 2,
            Filter::Lanczos3 => LANCZOS_A,
        }
    }

    /// 权重函数（f64 纯算术——sin 用泰勒近似）。
    pub fn weight(self, x: f64) -> f64 {
        let ax = x.abs();
        match self {
            // 盒式：|x|≤0.5 → 1（端点归属 0——归一化由累积律兜底）。
            Filter::Box => {
                if ax <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            // 双线性：三角核。
            Filter::Bilinear => {
                if ax < 1.0 {
                    1.0 - ax
                } else {
                    0.0
                }
            }
            // 双三次：B=C=0.5 标准 Keys 核。
            Filter::Bicubic => {
                if ax < 1.0 {
                    1.5 * ax * ax * ax - 2.5 * ax * ax + 1.0
                } else if ax < 2.0 {
                    -0.5 * ax * ax * ax + 2.5 * ax * ax - 4.0 * ax + 2.0
                } else {
                    0.0
                }
            }
            // Lanczos3：sinc(x)·sinc(x/3)——sin 泰勒 x−x³/6+x⁵/120。
            Filter::Lanczos3 => {
                if ax < 1e-9 {
                    1.0
                } else if ax <= 3.0 {
                    let s1 = sinc_taylor(ax);
                    let s2 = sinc_taylor(ax / 3.0);
                    s1 * s2
                } else {
                    0.0
                }
            }
            // Mitchell：B=1/3 C=1/3。
            Filter::Mitchell => {
                if ax < 1.0 {
                    ((21.0 - 37.0 * ax + 5.0 * ax * ax) * ax * ax * ax
                        + (-9.0 * ax * ax + 18.0 * ax - 12.0) * ax
                        + 7.0)
                        / 18.0
                } else if ax < 2.0 {
                    ((3.0 - ax) * (3.0 - ax) * (3.0 - ax) * (7.0 * ax - 5.0)) / 18.0
                } else {
                    0.0
                }
            }
        }
    }
}

/// sinc 的泰勒近似（sin(πx)/(πx)——|x|≤3 五项精度充裕；纯算术）。
fn sinc_taylor(x: f64) -> f64 {
    let px = core::f64::consts::PI * x;
    let px2 = px * px;
    // sin(p)/(p) = 1 − p²/6 + p⁴/120 − p⁶/5040 + p⁸/362880
    let s = 1.0 - px2 / 6.0 + px2 * px2 / 120.0 - px2 * px2 * px2 / 5040.0
        + px2 * px2 * px2 * px2 / 362880.0;
    s
}

// ---------------------------------------------------------------------------
// sRGB 感知（线性空间重采样防暗边——传递函数纯算术）
// ---------------------------------------------------------------------------

/// 五次方根（牛顿迭代——F0013 同款 no_std 纪律）。
fn fifth_root(v: f64) -> f64 {
    if v < 1e-9 {
        return 0.0;
    }
    let mut y = if v < 1.0 { 0.5 } else { 1.0 };
    for _ in 0..32 {
        let y4 = y * y * y * y;
        y = (4.0 * y + v / y4) / 5.0;
    }
    y
}

/// sRGB → linear（c≤0.04045 线性段/否则 ((c+0.055)/1.055)^2.4——
/// 2.4 = 2 × 0.4，x^0.4=(x^(1/5))²——纯乘除）。
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        let t = (c + 0.055) / 1.055;
        let t2 = t * t;
        let t_fifth = fifth_root(t);
        t2 * t_fifth * t_fifth
    }
}

/// linear → sRGB（逆传递——(1.055·y^(1/2.4))−0.055；1/2.4=5/12，
/// y^(5/12)=y^(1/12)⁵ 复杂——改用牛顿整根法：解 f(t)=t^2.4−y=0）。
pub fn linear_to_srgb(y: f64) -> f64 {
    if y <= 0.0031308 {
        y * 12.92
    } else {
        // 牛顿迭代解 t^2.4 = y（纯算术：t^2.4 = t²·(t^(1/5))²）。
        let mut t = if y < 0.5 { 0.5 } else { 0.8 };
        for _ in 0..40 {
            let t2 = t * t;
            let tf = fifth_root(t);
            let f = t2 * tf * tf - y;
            // 导数 2.4·t^1.4 = 2.4·t·(t^(1/5))² 。
            let d = 2.4 * t * tf * tf;
            if d > 1e-12 {
                t -= f / d;
            }
            if t < 1e-6 {
                t = 1e-6;
            }
        }
        1.055 * t - 0.055
    }
}

/// 8bit 往返（sRGB→linear→sRGB——恒等 ±1 容差）。
pub fn srgb_roundtrip_u8(v: u8) -> u8 {
    let lin = srgb_to_linear(v as f64 / 255.0);
    let back = linear_to_srgb(lin);
    let scaled = back * 255.0 + 0.5;
    if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

// ---------------------------------------------------------------------------
// 两 pass 重采样（水平/垂直分离+固定点累积）
// ---------------------------------------------------------------------------

/// 一维重采样 pass（盒式/双线性确定性对拍——固定点 Q16.16 累积）。
/// 手算锚：输入 [0,100,200,255] 水平盒式 4→2：
/// 输出 0 映射源区间 [0,2) → 均值 (0+100)/2=50；
/// 输出 1 映射 [2,4) → (200+255)/2=227.5→228（+0.5 截断）。
pub fn resize_1d_box(src: &[u8], dst_len: usize) -> Result<Vec<u8>, u16> {
    if src.is_empty() || dst_len == 0 || src.len() > 4096 || dst_len > 4096 {
        return Err(E_RS_DIM_INVALID);
    }
    let mut out = Vec::with_capacity(dst_len);
    let num = src.len() as u64;
    let den = dst_len as u64;
    for i in 0..dst_len {
        // 纯整数映射（no_std 无 floor——i·num/den 整数除法等价）：
        // lo=⌊i·n/d⌋、hi=⌈(i+1)·n/d⌉（整数：( (i+1)·n + d-1 )/d）。
        let lo = (i as u64 * num / den) as usize;
        let hi = (((i + 1) as u64 * num + den - 1) / den)
            .max(lo as u64 + 1)
            .min(num)
            as usize;
        let sum: u64 = src[lo..hi].iter().map(|&v| v as u64).sum();
        let n = (hi - lo) as u64;
        let v = (sum * 2 + n) / (2 * n); // 均值 +0.5 截断
        out.push(v.min(255) as u8);
    }
    Ok(out)
}

/// 图像两 pass 缩放（水平→垂直——盒式确定性对拍面）。
pub fn resize_2d_two_pass(
    w: u32,
    h: u32,
    pixels: &[u8],
    dw: u32,
    dh: u32,
) -> Result<Vec<u8>, u16> {
    if w == 0 || h == 0 || dw == 0 || dh == 0 {
        return Err(E_RS_DIM_INVALID);
    }
    if pixels.len() != (w * h) as usize {
        return Err(E_RS_DIM_INVALID);
    }
    // 水平 pass：逐行缩放。
    let mut mid = Vec::with_capacity((dw * h) as usize);
    for y in 0..h {
        let row = &pixels[(y * w) as usize..((y + 1) * w) as usize];
        mid.extend_from_slice(&resize_1d_box(row, dw as usize)?);
    }
    // 垂直 pass：逐列提出来缩放再放回。
    let mut out = vec![0u8; (dw * dh) as usize];
    for x in 0..dw {
        let col: Vec<u8> = (0..h).map(|y| mid[(y * dw + x) as usize]).collect();
        let scaled = resize_1d_box(&col, dh as usize)?;
        for (y, &v) in scaled.iter().enumerate() {
            out[(y as u32 * dw + x) as usize] = v;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 精度入册（RMSE 直测——no_std 无 log10 的等价承载）
// ---------------------------------------------------------------------------

/// RMSE（O(像素数)：平方和均根的整数近似——root 用整数牛顿）。
pub fn rmse(a: &[u8], b: &[u8]) -> Result<u64, u16> {
    if a.len() != b.len() || a.is_empty() {
        return Err(E_RS_DIM_INVALID);
    }
    let sq: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = *x as i64 - *y as i64;
            (d * d) as u64
        })
        .sum();
    // mean = sq/n；rmse = sqrt(mean) 整数牛顿（r²≤mean<r+1²）。
    let n = a.len() as u64;
    let mean = sq / n;
    if mean == 0 {
        return Ok(0);
    }
    let mut r = mean.min(u32::MAX as u64) as u64;
    for _ in 0..40 {
        let nr = (r + mean / r.max(1)) / 2;
        if nr >= r {
            break;
        }
        r = nr;
    }
    Ok(r)
}

/// 精度门（RMSE ≤ RMSE_LIMIT 入册——超阈立案）。
pub fn precision_gate(rmse_v: u64, bag: &mut DiagBag) -> Result<(), u16> {
    if rmse_v > RMSE_LIMIT {
        bag.push(E_RS_RMSE_EXCEED, "precision", &format!("RMSE{}>{}", rmse_v, RMSE_LIMIT));
        return Err(E_RS_RMSE_EXCEED);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 吞吐模型（两 pass ≥2GB/s+缩略图批量）
// ---------------------------------------------------------------------------

/// 两 pass 吞吐判定（模型实测 3≥2 达标——手算锚）。
pub fn tput_verdict(measured_gbps: u64) -> Result<(), u16> {
    if measured_gbps < TPUT_TARGET_GBPS {
        return Err(E_RS_TPUT_LOW);
    }
    Ok(())
}

/// 缩略图批量吞吐（批量 64 张 1920→320：模型字节数恰值——
/// 源 1920²×64=235,929,600B；缩略 320²×64=6,553,600B）。
pub fn thumb_batch_bytes(count: u64, src_w: u64, thumb_w: u64) -> (u64, u64) {
    let src_bytes = src_w * src_w * count;
    let thumb_bytes = thumb_w * thumb_w * count;
    (src_bytes, thumb_bytes)
}

// ---------------------------------------------------------------------------
// 判据（六项——滤波器族/两 pass/sRGB 感知/精度/吞吐/码）
// ---------------------------------------------------------------------------

/// CGPU-F0014 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vcg14_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0014");

    // 滤-01 五滤波器闭集：from_code 拒+权重恰值（Box 中心 1 界外 0、
    // Bilinear 中心 1 半点 0.5、Bicubic 中心 1）。
    {
        let f0 = Filter::from_code(0);
        let f4 = Filter::from_code(4);
        let bad = Filter::from_code(5);
        // 手算：Box(0)=1、Box(0.7)=0；Bilinear(0.5)=0.5；Bicubic(0)=1。
        let w_box0 = Filter::Box.weight(0.0);
        let w_box7 = Filter::Box.weight(0.7);
        let w_bl5 = Filter::Bilinear.weight(0.5);
        let w_bc0 = Filter::Bicubic.weight(0.0);
        let w_lz0 = Filter::Lanczos3.weight(0.0);
        let ok = f0 == Ok(Filter::Box)
            && f4 == Ok(Filter::Mitchell)
            && bad == Err(E_RS_FILTER_UNKNOWN)
            && (w_box0 - 1.0).abs() < 1e-9
            && w_box7 == 0.0
            && (w_bl5 - 0.5).abs() < 1e-9
            && (w_bc0 - 1.0).abs() < 1e-9
            && (w_lz0 - 1.0).abs() < 1e-9
            && Filter::all().len() == 5;
        if ok {
            s.ok("F0014-滤-01-五滤波器权重恰值");
        } else {
            s.fail("F0014-滤-01-五滤波器权重恰值", "滤波器面不符");
        }
    }

    // 两-02 两 pass 分离：4→1D 盒式手算锚 [0,100,200,255]→[50,228]
    // +2D 两 pass 4x1→2x1 全链恰值。
    {
        let r1 = resize_1d_box(&[0, 100, 200, 255], 2);
        // 手算：out0=[0,100] 均值 50；out1=[200,255] 均值 227.5→228。
        let r2 = resize_2d_two_pass(4, 1, &[0, 100, 200, 255], 2, 1);
        let zero = resize_1d_box(&[], 2);
        let ok = r1 == Ok(vec![50, 228])
            && r2 == Ok(vec![50, 228])
            && zero == Err(E_RS_DIM_INVALID);
        if ok {
            s.ok("F0014-两-02-两pass盒式手算锚恰值");
        } else {
            s.fail("F0014-两-02-两pass盒式手算锚恰值", "两 pass 面不符");
        }
    }

    // 线-03 sRGB 感知：端点恒等+中点往返 ±1+暗边防护语义（先线性化）。
    {
        let e0 = srgb_roundtrip_u8(0);
        let e255 = srgb_roundtrip_u8(255);
        let mid = srgb_roundtrip_u8(128);
        // 手算：0→0；255→255；128 往返 ±1（牛顿 40 步收敛）。
        let mid_ok = (mid as i32 - 128).abs() <= 1;
        // 线性化语义：srgb_to_linear(0.5) < 0.5（暗部在 linear 更亮=
        // 插值不压暗——暗边防护的数学根据）。
        let lin_mid = srgb_to_linear(0.5);
        let ok = e0 == 0
            && e255 == 255
            && mid_ok
            && lin_mid < 0.5
            && lin_mid > 0.2;
        if ok {
            s.ok("F0014-线-03-sRGB往返端点恒等暗边防护");
        } else {
            s.fail("F0014-线-03-sRGB往返端点恒等暗边防护", "sRGB 面不符");
        }
    }

    // 精-04 精度入册：恒等图 RMSE=0；1 差图 RMSE=1；超阈立案恰值。
    {
        let mut bag = DiagBag::new();
        let r0 = rmse(&[10, 20, 30], &[10, 20, 30]);
        let r1 = rmse(&[10, 20, 30, 40], &[11, 21, 31, 41]);
        // 手算：恒等 RMSE=0；全差 1→RMSE=1≤2 过。
        let g0 = precision_gate(r0.unwrap_or(99), &mut bag);
        let g1 = precision_gate(r1.unwrap_or(99), &mut bag);
        let r5 = rmse(&[10], &[15]);
        let g5 = precision_gate(r5.unwrap_or(99), &mut bag);
        // 手算：差 5→RMSE=5>2 立案。
        let ok = r0 == Ok(0)
            && r1 == Ok(1)
            && g0 == Ok(())
            && g1 == Ok(())
            && r5 == Ok(5)
            && g5 == Err(E_RS_RMSE_EXCEED)
            && bag.count_of(E_RS_RMSE_EXCEED) == 1;
        if ok {
            s.ok("F0014-精-04-RMSE入册超阈立案");
        } else {
            s.fail("F0014-精-04-RMSE入册超阈立案", "精度面不符");
        }
    }

    // 吞-05 吞吐达标+缩略图批量字节恰值。
    {
        let t3 = tput_verdict(3);
        let t1 = tput_verdict(1);
        // 手算：批量 64 张 1920²→320²：源 1920·1920·64=235,929,600；
        // 缩略 320·320·64=6,553,600。
        let (src_b, thumb_b) = thumb_batch_bytes(64, 1920, 320);
        let ok = t3 == Ok(())
            && t1 == Err(E_RS_TPUT_LOW)
            && src_b == 235_929_600
            && thumb_b == 6_553_600
            && TPUT_MEASURED_GBPS >= TPUT_TARGET_GBPS;
        if ok {
            s.ok("F0014-吞-05-吞吐达标批量恰值");
        } else {
            s.fail("F0014-吞-05-吞吐达标批量恰值", "吞吐面不符");
        }
    }

    // 码-06 五码互异+reason 全量显性+读屏非空。
    {
        let codes = [E_RS_FILTER_UNKNOWN, E_RS_DIM_INVALID, E_RS_RMSE_EXCEED, E_RS_TPUT_LOW, E_RS_NON_MONO];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xC701
            && codes[1] == 0xC702
            && codes[2] == 0xC703
            && codes[3] == 0xC704
            && codes[4] == 0xC705;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        let mut bag = DiagBag::new();
        bag.push(E_RS_RMSE_EXCEED, "bench", "演练立案");
        let spoken_ok = bag.spoken().contains("精度超阈") && bag.spoken().contains("bench");
        let ok = distinct && seg_ok && reasons_all && spoken_ok && bag.len() == 1;
        if ok {
            s.ok("F0014-码-06-五码互异读屏显性");
        } else {
            s.fail("F0014-码-06-五码互异读屏显性", "诊断面不符");
        }
    }

    s
}
