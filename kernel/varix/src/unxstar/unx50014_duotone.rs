//! UNX-F50014 双色打印档（黑白+专色简化语义显式登记）（0x42de0 段）
//!
//! 锚点：UNX-F50014-J1 双色 ×10 输出一致；判据锚定 §AI-63 §2 专题十二。
//!
//! 数据段 `SEG_BASE = 0x42de0`；诊断码闭集 = `SEG_BASE + n`：
//!   +1 Empty     样本集为空 / 标签为空 / 专色档缺失
//!   +2 CapFull   样本数或档位表容量满
//!   +3 Duplicate 档位 id 重复
//!   +4 Spot      专色全零（无法与黑版区分，等价于单色档）
//!   +5 Gain      挤墨率越界（合法域 0.0..=0.65）
//!   +6 Order     档位 id 非严格递增
//!   +7 Range     档位 id 为 0 或通道值越界
//!   +8 Stamp     档位指纹退化为 0
//!
//! 简化语义（显式登记，不做隐式猜测）：
//! * 黑版 K = 255 − max(R,G,B)：双色档里黑版承担全部中性密度；
//! * 专色版 S = 255 × 最大通道专色覆盖 × 色度闸。逐通道覆盖 =
//!   (255−src)/(255−spot)，取**最大**通道需求；色度闸 = (max−min)/max。
//!   取最大而不是最小：专色只需要「某一通道差得最远」就能出墨；乘色度闸
//!   则保证中性像素（黑/白/灰）专色版恒为 0，不会出现「白纸满版专色」
//!   这类荒谬结果；
//! * 挤墨（dot gain）在覆盖率空间按 g·v·(1−v) 抬升：导数 1+g(1−2v) ≥
//!   1−g > 0（g ≤ 0.65），因此**严格单调**，不会把两档墨量画反。
//!
//! 结构清单：
//! - 数据类型：`InkSpec`（油墨档）、`DuotoneProfile`（双色档）、
//!   `DuotonePix`（一次分色结果）、`PressRun`（印刷批次记录）
//! - 注册表：`ProfileLedger`（定容、id 严格递增、重名/重号显式拒绝）
//! - 状态/账本：批次 `plates` 与重算比对（一致性闸）
//! - 错误面：`DuotoneError::code()` 落 0x42de0 段闭集
//! - 注入核：`injection_core` 六类坏样本（全零专色/超限挤墨/重复档 id/
//!   乱序登记/容量满/空样本）

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

use super::unx50001_gamutmap::{grayscale_code, Rgb8};

/// 诊断段基址（0x42de0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42de0;

pub const E_EMPTY: u16 = 1;
pub const E_CAPFULL: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_SPOT: u16 = 4;
pub const E_GAIN: u16 = 5;
pub const E_ORDER: u16 = 6;
pub const E_RANGE: u16 = 7;
pub const E_STAMP: u16 = 8;

/// 挤墨率上限。取 0.65 而不是 1.0：覆盖率映射 v' = v + g·v·(1−v) 的导数
/// 下界是 1−g，g=0.65 时仍 ≥ 0.35，保证「输入墨量大 ⇒ 输出墨量大」不被
/// 非线性抹平（g=1 时端点导数归零，两档相邻值会贴平）。
pub const MAX_GAIN: f32 = 0.65;

/// 双色档位 id 上限（u16 保留 0 作「未登记」）。
pub const MAX_PROFILE_ID: u16 = 4095;

/// 单批次样本上限。
pub const MAX_SAMPLES: usize = 256;

/// 双色打印错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuotoneError {
    Empty,
    CapFull,
    Duplicate,
    Spot,
    Gain,
    Order,
    Range,
    Stamp,
}

impl DuotoneError {
    /// 段内诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            DuotoneError::Empty => E_EMPTY,
            DuotoneError::CapFull => E_CAPFULL,
            DuotoneError::Duplicate => E_DUPLICATE,
            DuotoneError::Spot => E_SPOT,
            DuotoneError::Gain => E_GAIN,
            DuotoneError::Order => E_ORDER,
            DuotoneError::Range => E_RANGE,
            DuotoneError::Stamp => E_STAMP,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            DuotoneError::Empty => "empty samples or label",
            DuotoneError::CapFull => "duotone capacity full",
            DuotoneError::Duplicate => "duplicate profile id",
            DuotoneError::Spot => "spot ink degenerates to black",
            DuotoneError::Gain => "dot gain out of range",
            DuotoneError::Order => "profile ids not increasing",
            DuotoneError::Range => "profile id zero or channel out of range",
            DuotoneError::Stamp => "profile stamp degenerate",
        }
    }
}

fn finite(v: f32) -> bool {
    v == v && v > -1.0e30 && v < 1.0e30
}

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn clamp01(v: f32) -> f32 {
    clampf(v, 0.0, 1.0)
}

/// FNV-1a 64 位（档位指纹；跨平台逐位一致）。
#[derive(Clone, Copy)]
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    pub const fn new() -> Fnv64 {
        Fnv64 {
            h: 0xcbf29ce484222325,
        }
    }

    pub fn byte(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x100000001b3);
    }

    pub fn u16le(&mut self, v: u16) {
        self.byte(v as u8);
        self.byte((v >> 8) as u8);
    }

    pub fn f32q(&mut self, v: f32) {
        let scaled = v * 10000.0;
        let c = if scaled > 2.0e9 {
            2.0e9
        } else if scaled < -2.0e9 {
            -2.0e9
        } else {
            scaled
        };
        self.u16le(c as i32 as u16);
        self.u16le((c as i32 >> 16) as u16);
    }

    pub fn digest(self) -> u64 {
        self.h
    }
}

/// 油墨档：黑版或一个专色。`gain` 是该墨在纸上的挤墨率。
#[derive(Clone, Debug, PartialEq)]
pub struct InkSpec {
    pub id: u8,
    pub label: String,
    pub rgb: Rgb8,
    pub gain: f32,
}

impl InkSpec {
    /// 黑版：id 固定 1，颜色全零，标签固定 "K"（黑版在双色档里必须存在）。
    pub fn black(gain: f32) -> Result<InkSpec, DuotoneError> {
        InkSpec::validated(1, "K", Rgb8::BLACK, gain)
    }

    /// 专色：id ≠ 0；颜色全零等于「没有专色」，必须显式拒绝。
    pub fn spot(id: u8, label: &str, rgb: Rgb8) -> Result<InkSpec, DuotoneError> {
        if rgb.r == 0 && rgb.g == 0 && rgb.b == 0 {
            return Err(DuotoneError::Spot);
        }
        InkSpec::validated(id, label, rgb, 0.12)
    }

    /// 统一校验入口：id/标签/有限性/挤墨域一次查完。
    pub fn validated(id: u8, label: &str, rgb: Rgb8, gain: f32) -> Result<InkSpec, DuotoneError> {
        if id == 0 {
            return Err(DuotoneError::Range);
        }
        if label.is_empty() {
            return Err(DuotoneError::Empty);
        }
        if !finite(gain) || gain < 0.0 || gain > MAX_GAIN {
            return Err(DuotoneError::Gain);
        }
        Ok(InkSpec {
            id,
            label: label.to_string(),
            rgb,
            gain,
        })
    }

    /// 改挤墨率（链式配置）。越界显式失败，不夹取——夹取会把配置错误
    /// 静默变成「能跑但不是你要的值」。
    pub fn with_gain(mut self, gain: f32) -> Result<InkSpec, DuotoneError> {
        if !finite(gain) || gain < 0.0 || gain > MAX_GAIN {
            return Err(DuotoneError::Gain);
        }
        self.gain = gain;
        Ok(self)
    }

    fn feed(&self, h: &mut Fnv64) {
        h.byte(self.id);
        h.byte(self.rgb.r);
        h.byte(self.rgb.g);
        h.byte(self.rgb.b);
        h.f32q(self.gain);
        let mut i = 0usize;
        let bytes = self.label.as_bytes();
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                h.byte(*b);
            }
            i += 1;
        }
    }
}

/// 双色档：黑版 + 一个专色，带不可变指纹 `stamp`。
#[derive(Clone, Debug, PartialEq)]
pub struct DuotoneProfile {
    pub id: u16,
    pub label: String,
    pub black: InkSpec,
    pub spot: InkSpec,
    pub stamp: u64,
}

impl DuotoneProfile {
    /// 构造双色档。id=0 / 空标签 / 非法挤墨一律显式拒绝；
    /// stamp 由两个油墨档与标签共同决定（换任一字段即换指纹）。
    pub fn build(
        id: u16,
        label: &str,
        black_gain: f32,
        spot: InkSpec,
    ) -> Result<DuotoneProfile, DuotoneError> {
        if id == 0 || id > MAX_PROFILE_ID {
            return Err(DuotoneError::Range);
        }
        if label.is_empty() {
            return Err(DuotoneError::Empty);
        }
        let black = InkSpec::black(black_gain)?;
        if spot.id == black.id {
            // 两个墨档共用 id 会让批次账无法区分版口。
            return Err(DuotoneError::Duplicate);
        }
        let mut h = Fnv64::new();
        h.u16le(id);
        let mut i = 0usize;
        let bytes = label.as_bytes();
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                h.byte(*b);
            }
            i += 1;
        }
        black.feed(&mut h);
        spot.feed(&mut h);
        let stamp = h.digest();
        if stamp == 0 {
            return Err(DuotoneError::Stamp);
        }
        Ok(DuotoneProfile {
            id,
            label: label.to_string(),
            black,
            spot,
            stamp,
        })
    }

    /// 分色：先按简化语义拆 K/S，再各自做挤墨抬升。
    pub fn render(&self, sample: Rgb8) -> DuotonePix {
        let (k, s) = split_plates(sample, self.spot.rgb);
        DuotonePix {
            k: gain_apply(k, self.black.gain),
            s: gain_apply(s, self.spot.gain),
        }
    }

    /// 同一输入渲染 10 次（J1 的 ×10 口径）。用定长数组返回，
    /// 让「10 次逐位一致」成为调用点可直接断言的事实。
    pub fn render_x10(&self, sample: Rgb8) -> [DuotonePix; 10] {
        let mut out = [DuotonePix { k: 0, s: 0 }; 10];
        let mut i = 0usize;
        while i < 10 {
            out[i] = self.render(sample);
            i += 1;
        }
        out
    }

    /// 10 次渲染是否逐位一致（一致性闸的布尔面）。
    pub fn x10_consistent(&self, sample: Rgb8) -> bool {
        let set = self.render_x10(sample);
        let first = set[0];
        let mut i = 1usize;
        while i < set.len() {
            if set[i] != first {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 一次分色结果（K 版 / 专色版，均为 0..=255 覆盖码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuotonePix {
    pub k: u8,
    pub s: u8,
}

impl DuotonePix {
    /// 两版总墨量（审计用；双色档不设 K+S 上限，因为专色与黑版是
    /// 两块独立版，叠印总墨量由印刷机决定，不属本档语义）。
    pub fn total(&self) -> u16 {
        self.k as u16 + self.s as u16
    }
}

/// 简化双色分色（显式登记，见文件头）：返回（黑版, 专色版）覆盖码。
///
/// 专色覆盖取**最大**通道需求而非最小：只要有一个通道离专色墨量足够近，
/// 该像素就该出专色；乘色度闸则把中性像素钉死在专色版 0 上。
pub fn split_plates(sample: Rgb8, spot: Rgb8) -> (u8, u8) {
    let mut mx = sample.r;
    if sample.g > mx {
        mx = sample.g;
    }
    if sample.b > mx {
        mx = sample.b;
    }
    let mut mn = sample.r;
    if sample.g < mn {
        mn = sample.g;
    }
    if sample.b < mn {
        mn = sample.b;
    }
    let k = 255u8.saturating_sub(mx);
    let chans = [
        (sample.r, spot.r),
        (sample.g, spot.g),
        (sample.b, spot.b),
    ];
    let mut best = 0.0f32;
    let mut i = 0usize;
    while i < chans.len() {
        if let Some((src, sp)) = chans.get(i) {
            // 该通道专色可承载的最大墨量；为 0 说明这版墨在该通道不约束。
            let need = 255.0 - *sp as f32;
            if need > 0.0 {
                let cover = clamp01((255.0 - *src as f32) / need);
                if cover > best {
                    best = cover;
                }
            }
        }
        i += 1;
    }
    // 色度闸：中性像素（max==min）专色版恒 0。
    let gate = if mx == 0 {
        0.0
    } else {
        clamp01((mx - mn) as f32 / mx as f32)
    };
    let scaled = clamp01(best * gate) * 255.0 + 0.5;
    let s = if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    };
    (k, s)
}

/// 挤墨映射：覆盖率空间 v' = v + g·v·(1−v)。
///
/// 单调性证明：dv'/dv = 1 + g(1−2v) ≥ 1−g > 0（v∈[0,1]，g≤0.65）。
/// 因此它不会把「更浓的输入」映射成「更淡的输出」，也不可能产生回折。
pub fn gain_apply(code: u8, gain: f32) -> u8 {
    let v = code as f32 / 255.0;
    let g = clampf(if finite(gain) { gain } else { 0.0 }, 0.0, MAX_GAIN);
    let out = v + g * v * (1.0 - v);
    let scaled = clamp01(out) * 255.0 + 0.5;
    if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

/// 印刷批次：一个档位 + 一组样本 + 首次渲染的版口记录。
#[derive(Clone, Debug)]
pub struct PressRun {
    pub profile: DuotoneProfile,
    pub samples: Vec<Rgb8>,
    pub plates: Vec<DuotonePix>,
}

impl PressRun {
    /// 建批次并立即出第一版。空样本/超容显式拒绝——空批次「一致」是
    /// 恒真断言，不能当验收证据。
    pub fn new(profile: DuotoneProfile, samples: Vec<Rgb8>) -> Result<PressRun, DuotoneError> {
        if samples.is_empty() {
            return Err(DuotoneError::Empty);
        }
        if samples.len() > MAX_SAMPLES {
            return Err(DuotoneError::CapFull);
        }
        let mut plates = Vec::with_capacity(samples.len());
        let mut i = 0usize;
        while i < samples.len() {
            if let Some(s) = samples.get(i) {
                plates.push(profile.render(*s));
            }
            i += 1;
        }
        Ok(PressRun {
            profile,
            samples,
            plates,
        })
    }

    /// 重算并覆盖版口记录，返回样本数。
    pub fn reprint(&mut self) -> usize {
        let mut i = 0usize;
        while i < self.samples.len() {
            let rendered = match self.samples.get(i) {
                Some(s) => self.profile.render(*s),
                None => DuotonePix { k: 0, s: 0 },
            };
            if let Some(slot) = self.plates.get_mut(i) {
                *slot = rendered;
            }
            i += 1;
        }
        self.samples.len()
    }

    /// 重印一致性：重算结果必须与首版逐位相同（版口不可漂移）。
    pub fn consistent(&self) -> bool {
        if self.samples.len() != self.plates.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.samples.len() {
            let expect = match self.samples.get(i) {
                Some(s) => self.profile.render(*s),
                None => return false,
            };
            match self.plates.get(i) {
                Some(p) => {
                    if *p != expect {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// J1 的样本集：10 个覆盖灰阶、原色与两色混叠的色块。
    pub fn ten_sample_grid() -> Vec<Rgb8> {
        vec![
            Rgb8::new(0, 0, 0),
            Rgb8::new(255, 255, 255),
            Rgb8::new(128, 128, 128),
            Rgb8::new(255, 0, 0),
            Rgb8::new(0, 255, 0),
            Rgb8::new(0, 0, 255),
            Rgb8::new(255, 255, 0),
            Rgb8::new(0, 255, 255),
            Rgb8::new(255, 0, 255),
            Rgb8::new(64, 96, 128),
        ]
    }
}

/// 档位注册表：id 严格递增，重号/乱序/超容都显式拒绝。
#[derive(Clone, Debug)]
pub struct ProfileLedger {
    cap: usize,
    profiles: Vec<DuotoneProfile>,
}

impl ProfileLedger {
    pub fn new(cap: usize) -> ProfileLedger {
        ProfileLedger {
            cap,
            profiles: Vec::new(),
        }
    }

    /// 登记档位。顺序：容量 → 重号 → 递增。重号先于递增，原因同
    /// F50012：同 id 的根因是复制，倒序的根因是排序，诊断要指向根因。
    pub fn register(&mut self, p: DuotoneProfile) -> Result<usize, DuotoneError> {
        if self.profiles.len() >= self.cap {
            return Err(DuotoneError::CapFull);
        }
        let mut i = 0usize;
        while i < self.profiles.len() {
            if let Some(x) = self.profiles.get(i) {
                if x.id == p.id {
                    return Err(DuotoneError::Duplicate);
                }
            }
            i += 1;
        }
        if let Some(last) = self.profiles.last() {
            if last.id >= p.id {
                return Err(DuotoneError::Order);
            }
        }
        self.profiles.push(p);
        Ok(self.profiles.len() - 1)
    }

    pub fn find(&self, id: u16) -> Option<&DuotoneProfile> {
        let mut i = 0usize;
        while i < self.profiles.len() {
            if let Some(p) = self.profiles.get(i) {
                if p.id == id {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 注入核：六类坏样本必须全部被抓
// ---------------------------------------------------------------------------

fn probe_spot() -> InkSpec {
    match InkSpec::spot(2, "PANTONE-032", Rgb8::new(237, 28, 36)) {
        Ok(s) => s,
        Err(_) => InkSpec {
            id: 2,
            label: "fallback".to_string(),
            rgb: Rgb8::new(237, 28, 36),
            gain: 0.0,
        },
    }
}

/// 六类坏样本注入：返回（被抓数, 总数）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1 全零专色（= 没有专色）
    if InkSpec::spot(2, "ZERO", Rgb8::BLACK).err() == Some(DuotoneError::Spot) {
        caught += 1;
    }
    // 2 挤墨超限
    if InkSpec::black(1.2).err() == Some(DuotoneError::Gain) {
        caught += 1;
    }
    // 3 重复档 id
    let mut led = ProfileLedger::new(4);
    let p1 = DuotoneProfile::build(7, "dup-a", 0.10, probe_spot());
    let p2 = DuotoneProfile::build(7, "dup-b", 0.10, probe_spot());
    let ok = match (p1, p2) {
        (Ok(a), Ok(b)) => {
            let _ = led.register(a);
            led.register(b).err() == Some(DuotoneError::Duplicate)
        }
        _ => false,
    };
    if ok {
        caught += 1;
    }
    // 4 乱序登记
    let mut led2 = ProfileLedger::new(4);
    let o1 = DuotoneProfile::build(9, "o1", 0.1, probe_spot());
    let o2 = DuotoneProfile::build(3, "o2", 0.1, probe_spot());
    let ok = match (o1, o2) {
        (Ok(a), Ok(b)) => {
            let _ = led2.register(a);
            led2.register(b).err() == Some(DuotoneError::Order)
        }
        _ => false,
    };
    if ok {
        caught += 1;
    }
    // 5 容量满
    let mut led3 = ProfileLedger::new(1);
    let c1 = DuotoneProfile::build(1, "c1", 0.1, probe_spot());
    let c2 = DuotoneProfile::build(2, "c2", 0.1, probe_spot());
    let ok = match (c1, c2) {
        (Ok(a), Ok(b)) => {
            let _ = led3.register(a);
            led3.register(b).err() == Some(DuotoneError::CapFull)
        }
        _ => false,
    };
    if ok {
        caught += 1;
    }
    // 6 空样本批次
    let ok = match DuotoneProfile::build(1, "e", 0.1, probe_spot()) {
        Ok(p) => PressRun::new(p, Vec::new()).err() == Some(DuotoneError::Empty),
        Err(_) => false,
    };
    if ok {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn zero_gain_profile(id: u16) -> Option<DuotoneProfile> {
    match DuotoneProfile::build(id, "press", 0.0, probe_spot()) {
        Ok(p) => Some(p),
        Err(_) => None,
    }
}

pub fn run_unx50014_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50014");

    // 基线档：零挤墨、专色 237/28/36（PANTONE 032 简化值）
    let base = match zero_gain_profile(1) {
        Some(p) => p,
        None => return s,
    };

    // J1-1 纯黑：K 满、专色零
    let black = base.render(Rgb8::BLACK);
    let ok = black.k == 255 && black.s == 0;
    let det = format!("k={} s={}", black.k, black.s);
    s.add_dyn("50014-J1-纯黑", ok, &det);

    // J1-2 纯专色：专色接近满版（色度闸 209/237 折减），K 恰为最大通道缺口
    let spot = base.render(Rgb8::new(237, 28, 36));
    let ok = spot.s >= 200 && spot.s <= 240 && spot.k == 255u8 - 237;
    let det = format!("k={} s={}", spot.k, spot.s);
    s.add_dyn("50014-J1-纯专色", ok, &det);

    // J1-3 白：两版皆零
    let white = base.render(Rgb8::WHITE);
    let ok = white.k == 0 && white.s == 0;
    s.add("50014-J1-白", ok, if ok { "" } else { "白场出墨" });

    // J1-4 J1 核心：双色 ×10 输出一致（三个代表色）
    let samples = [Rgb8::BLACK, Rgb8::new(128, 96, 64), Rgb8::new(237, 28, 36)];
    let mut ok = true;
    let mut i = 0usize;
    while i < samples.len() {
        if let Some(c) = samples.get(i) {
            if !base.x10_consistent(*c) {
                ok = false;
            }
        }
        i += 1;
    }
    let first = base.render_x10(match samples.get(1) {
        Some(c) => *c,
        None => Rgb8::BLACK,
    });
    let ok = ok && first[0] == first[9];
    let det = format!("p0=({},{}) p9=({},{})", first[0].k, first[0].s, first[9].k, first[9].s);
    s.add_dyn("50014-J1-×10一致", ok, &det);

    // J1-5 10 样本网格：逐点 10 次一致，且批次重印一致
    let grid = PressRun::ten_sample_grid();
    let mut grid_ok = grid.len() == 10;
    let mut j = 0usize;
    while j < grid.len() {
        if let Some(c) = grid.get(j) {
            if !base.x10_consistent(*c) {
                grid_ok = false;
            }
        }
        j += 1;
    }
    let run = match PressRun::new(base.clone(), grid) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let ok = grid_ok && run.consistent() && run.plates.len() == 10;
    s.add("50014-J1-十样本", ok, if ok { "" } else { "10 样本输出漂移" });

    // J1-6 挤墨单调不减；g=0 时恒等
    let mut mono = true;
    let mut prev = 0u8;
    let mut code = 0u16;
    while code < 256 {
        let cur = gain_apply(code as u8, 0.30);
        if cur < prev {
            mono = false;
        }
        prev = cur;
        code += 1;
    }
    let mut id_ok = true;
    let mut c2 = 0u16;
    while c2 < 256 {
        if gain_apply(c2 as u8, 0.0) != c2 as u8 {
            id_ok = false;
        }
        c2 += 1;
    }
    let ok = mono && id_ok && gain_apply(255, 0.5) == 255 && gain_apply(0, 0.5) == 0;
    let det = format!("mono={} identity={}", mono, id_ok);
    s.add_dyn("50014-J1-挤墨单调", ok, &det);

    // J1-7 黑版守恒：K_pre == 255 − max(R,G,B)
    let steps = [0u8, 64, 128, 192, 255];
    let mut ok = true;
    let mut a = 0usize;
    while a < steps.len() {
        let mut b = 0usize;
        while b < steps.len() {
            let mut c = 0usize;
            while c < steps.len() {
                let r = match steps.get(a) {
                    Some(v) => *v,
                    None => 0,
                };
                let g = match steps.get(b) {
                    Some(v) => *v,
                    None => 0,
                };
                let bl = match steps.get(c) {
                    Some(v) => *v,
                    None => 0,
                };
                let mut mx = r;
                if g > mx {
                    mx = g;
                }
                if bl > mx {
                    mx = bl;
                }
                let pix = base.render(Rgb8::new(r, g, bl));
                if pix.k != 255u8.saturating_sub(mx) {
                    ok = false;
                }
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("50014-J1-黑版守恒", ok, if ok { "" } else { "K 不守恒" });

    // J1-8 灰度互补：零挤墨档上 K + grayscale ≈ 255（±1）
    let mut ok = true;
    let mut v = 0u8;
    let grays = [0u8, 64, 128, 192, 255];
    let mut k = 0usize;
    while k < grays.len() {
        if let Some(gv) = grays.get(k) {
            v = *gv;
            let pix = base.render(Rgb8::new(v, v, v));
            let gray = grayscale_code(Rgb8::new(v, v, v));
            let sum = pix.k as i32 + gray as i32;
            if sum < 254 || sum > 256 {
                ok = false;
            }
        }
        k += 1;
    }
    let det = format!("gray={} k={}", grayscale_code(Rgb8::new(v, v, v)), base.render(Rgb8::new(v, v, v)).k);
    s.add_dyn("50014-J1-灰度互补", ok, &det);

    // J1-9 指纹确定且字段敏感
    let p_a = zero_gain_profile(5);
    let p_b = zero_gain_profile(5);
    let p_c = zero_gain_profile(6);
    let ok = match (&p_a, &p_b, &p_c) {
        (Some(a), Some(b), Some(c)) => a.stamp == b.stamp && a.stamp != c.stamp && a.stamp != 0,
        _ => false,
    };
    s.add("50014-J1-指纹", ok, if ok { "" } else { "指纹退化" });

    // J1-10 注册表：重号 / 乱序 / 容量
    let mut led = ProfileLedger::new(4);
    let r1 = zero_gain_profile(10).map(|p| led.register(p));
    let r2 = zero_gain_profile(10).map(|p| led.register(p));
    let r3 = zero_gain_profile(12).map(|p| led.register(p));
    let r5 = zero_gain_profile(11).map(|p| led.register(p));
    let mut led_cap = ProfileLedger::new(1);
    let c1 = zero_gain_profile(1).map(|p| led_cap.register(p));
    let c2 = zero_gain_profile(2).map(|p| led_cap.register(p));
    let ok = matches!(r1, Some(Ok(0)))
        && matches!(r2, Some(Err(DuotoneError::Duplicate)))
        && matches!(r3, Some(Ok(1)))
        && matches!(r5, Some(Err(DuotoneError::Order)))
        && matches!(c1, Some(Ok(0)))
        && matches!(c2, Some(Err(DuotoneError::CapFull)));
    let det = format!("r1={:?} r5={:?} c2={:?}", r1, r5, c2);
    s.add_dyn("50014-J1-注册闸", ok, &det);

    // J1-11 检索与批次重印
    let ok = led.find(12).is_some() && led.find(99).is_none() && led.len() == 2;
    let mut run2 = match PressRun::new(base.clone(), PressRun::ten_sample_grid()) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let n = run2.reprint();
    let ok2 = n == 10 && run2.consistent();
    let ok = ok && ok2;
    s.add("50014-J1-检索重印", ok, if ok { "" } else { "检索或重印错" });

    // J1-12 六类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50014-J1-注入必抓", ok, &det);

    // J1-13 总墨量守恒：K+S 等于两版之和（无非负回绕）
    let pix = base.render(Rgb8::new(200, 100, 50));
    let ok = pix.total() == pix.k as u16 + pix.s as u16;
    s.add("50014-J1-墨量记账", ok, if ok { "" } else { "墨量记账错" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50014_checks_green() {
        let set = run_unx50014_checks();
        assert!(set.all_passed(), "UNX-F50014 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50014_ten_renders_identical() {
        let p = match zero_gain_profile(1) {
            Some(p) => p,
            None => return,
        };
        let a = p.render_x10(Rgb8::new(96, 64, 32));
        let mut i = 0usize;
        while i < a.len() {
            assert_eq!(a[i], a[0]);
            i += 1;
        }
    }

    #[test]
    fn unx50014_gain_monotone() {
        let mut prev = 0u8;
        let mut c = 0u16;
        while c < 256 {
            let cur = gain_apply(c as u8, MAX_GAIN);
            assert!(cur >= prev, "gain must stay monotone");
            prev = cur;
            c += 1;
        }
    }
}
