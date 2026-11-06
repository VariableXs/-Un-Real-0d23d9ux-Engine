//! UNX-F50054 水印 fuzz 面（坏图/坏参数注入不崩）（0x432e0 段）
//!
//! 锚点：UNX-F50054-J1 fuzz ×10^3 零崩；判据锚定 §AI-63 §5。
//!
//! 结构清单：
//! * 几何校验器 `validate`：把 `ImageSpec`（w/h/stride/channels）折叠成一份
//!   可信的 `Geometry`，所有后续下标都从它派生，杜绝「先算再校验」的越界窗口；
//! * 嵌入器 `embed` / 提取器 `extract`：LSB 冗余嵌入，每个载荷位复制
//!   r 份（r = 1..4，由强度决定），提取按多数表决——这是一对真往返，
//!   不是只返回常量的桩；
//! * 确定性语料发生器 `Rng` + `gen_case`：xorshift64*，同种子同序列；
//! * 结果账 `FuzzLedger`：接受/拒绝/往返一致/最大畸变/诊断直方图/顺序摘要；
//! * 注入核：零维 / 超面积 / 非法通道 / 步长不足 / 缓冲截断 / 空载荷 /
//!   参数越界 / 容量不足 八类坏样本，逐类断言具体诊断码。
//!
//! 错误纪律：诊断码闭集 `0x432e0 + 1..=8`；内核路径零 panic——Vec 一律
//! `get`/`get_mut`，所有容量在分配前用 u64 校验，防伪造 stride 撑爆堆。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x432e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x432e0;

pub const E_DIM: u16 = 1;
pub const E_AREA: u16 = 2;
pub const E_CHAN: u16 = 3;
pub const E_STRIDE: u16 = 4;
pub const E_TRUNC: u16 = 5;
pub const E_PAYLOAD: u16 = 6;
pub const E_PARAM: u16 = 7;
pub const E_CAP: u16 = 8;

/// 直方图槽位：诊断码 n 落在 `hist[n]`，槽 0 永远为 0（码从 1 起）。
pub const CODE_SLOTS: usize = 9;

/// 单边上限 4096：水印嵌入只跑在小尺寸采样图上，超大输入属于伪造参数。
pub const MAX_SIDE: u32 = 4096;
/// 16 Mi 像素上限（4096²）。超过就不是「图」而是「盘」，直接判面积越界。
pub const MAX_PIXELS: u64 = 16_777_216;
/// 帧缓冲上限 64 MiB：stride 可由调用方伪造，若不在分配前夹住，
/// 一个 `stride = u32::MAX` 的坏参数就能把内核堆打穿。
pub const MAX_FRAME: u64 = 64 * 1024 * 1024;

/// fuzz 面错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmError {
    Dim,
    Area,
    Chan,
    Stride,
    Trunc,
    Payload,
    Param,
    Cap,
}

impl WmError {
    pub fn code(self) -> u32 {
        let n = match self {
            WmError::Dim => E_DIM,
            WmError::Area => E_AREA,
            WmError::Chan => E_CHAN,
            WmError::Stride => E_STRIDE,
            WmError::Trunc => E_TRUNC,
            WmError::Payload => E_PAYLOAD,
            WmError::Param => E_PARAM,
            WmError::Cap => E_CAP,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            WmError::Dim => "image has a zero dimension",
            WmError::Area => "image area or frame exceeds budget",
            WmError::Chan => "channel count not in {1,3,4}",
            WmError::Stride => "stride narrower than width*channels",
            WmError::Trunc => "buffer shorter than stride*height",
            WmError::Payload => "watermark payload is empty",
            WmError::Param => "strength outside [0,1] or NaN",
            WmError::Cap => "payload bits exceed embedding capacity",
        }
    }
}

/// 8 位采样图的几何描述。`stride` 允许大于 `w*channels`（行对齐填充）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageSpec {
    pub w: u32,
    pub h: u32,
    pub stride: u32,
    pub channels: u8,
}

impl ImageSpec {
    pub const fn new(w: u32, h: u32, stride: u32, channels: u8) -> ImageSpec {
        ImageSpec {
            w,
            h,
            stride,
            channels,
        }
    }
}

/// 水印参数：强度决定冗余度，载荷按位写入。
#[derive(Clone, Debug, PartialEq)]
pub struct WmParams {
    pub strength: f32,
    pub payload: Vec<u8>,
}

/// 校验后的几何量（usize 域）。所有写入/读取下标都由它派生。
#[derive(Clone, Copy, Debug)]
struct Geometry {
    w: usize,
    stride: usize,
    channels: usize,
    frame: usize,
    pixels: usize,
}

/// 几何校验：任何一条不满足都返回具体诊断码。
///
/// 顺序固定为 维度 → 面积 → 通道 → 步长 → 帧长：先挡住「0 维」这种会让
/// 后续除法爆炸的输入，再谈容量；通道数决定步长下界，必须先定通道。
fn validate(spec: &ImageSpec) -> Result<Geometry, WmError> {
    if spec.w == 0 || spec.h == 0 {
        return Err(WmError::Dim);
    }
    if spec.w > MAX_SIDE || spec.h > MAX_SIDE {
        return Err(WmError::Area);
    }
    let px = spec.w as u64 * spec.h as u64;
    if px > MAX_PIXELS {
        return Err(WmError::Area);
    }
    let ch = spec.channels as usize;
    if ch != 1 && ch != 3 && ch != 4 {
        return Err(WmError::Chan);
    }
    let min_stride = spec.w as u64 * ch as u64;
    if (spec.stride as u64) < min_stride {
        return Err(WmError::Stride);
    }
    let frame = spec.stride as u64 * spec.h as u64;
    if frame > MAX_FRAME {
        return Err(WmError::Area);
    }
    Ok(Geometry {
        w: spec.w as usize,
        stride: spec.stride as usize,
        channels: ch,
        frame: frame as usize,
        pixels: px as usize,
    })
}

/// 强度 → 冗余度 1..4。
///
/// `!(s >= 0 && s <= 1)` 写法把 NaN 一并拒绝（NaN 的任何比较都为假），
/// 这是 fuzz 面最容易被忽略的入口——`0.0 <= NaN` 是假的，但只写
/// `s < 0 || s > 1` 会漏掉 NaN。
pub fn redundancy_of(strength: f32) -> Result<u8, WmError> {
    if !(strength >= 0.0 && strength <= 1.0) {
        return Err(WmError::Param);
    }
    let r = 1 + (strength * 3.0) as u8;
    Ok(if r > 4 { 4 } else { r })
}

/// 给定几何与冗余度，可写入的载荷位数 = 可用像素 / 冗余度。
pub fn capacity_bits(spec: &ImageSpec, redundancy: u8) -> Result<usize, WmError> {
    let g = validate(spec)?;
    let r = if redundancy == 0 { 1 } else { redundancy as usize };
    Ok(g.pixels / r)
}

/// 嵌入结果：位数、翻转采样数、容量、冗余度、畸变率（翻转/像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmbedReport {
    pub bits: usize,
    pub flips: usize,
    pub capacity_bits: usize,
    pub redundancy: u8,
    pub distortion: f32,
}

fn bit_at(payload: &[u8], bitpos: usize) -> bool {
    match payload.get(bitpos >> 3) {
        Some(b) => ((*b >> (bitpos & 7)) & 1) != 0,
        None => false,
    }
}

/// 把载荷嵌进采样图的最低有效位，返回 (**新缓冲**, 报告)。
///
/// 写入位置：按行优先遍历通道 0 的像素，每个载荷位连续占用 r 个像素
/// （第 i 位写第 i*r .. i*r+r-1 个像素）。这种「连续复制」布局让提取端
/// 不需要额外索引表，只凭 (payload_len, strength, spec) 就能对齐。
pub fn embed(
    spec: &ImageSpec,
    p: &WmParams,
    src: &[u8],
) -> Result<(Vec<u8>, EmbedReport), WmError> {
    let g = validate(spec)?;
    let r = redundancy_of(p.strength)?;
    if p.payload.is_empty() {
        return Err(WmError::Payload);
    }
    if src.len() < g.frame {
        return Err(WmError::Trunc);
    }
    let bits = p.payload.len() * 8;
    let cap = g.pixels / r as usize;
    if bits > cap {
        return Err(WmError::Cap);
    }
    let mut out = Vec::with_capacity(g.frame);
    let mut i = 0usize;
    while i < g.frame {
        if let Some(b) = src.get(i) {
            out.push(*b);
        }
        i += 1;
    }
    let mut flips = 0usize;
    let mut idx = 0usize;
    let mut bitpos = 0usize;
    while bitpos < bits {
        let bit = bit_at(&p.payload, bitpos);
        let mut rep = 0u8;
        while rep < r {
            let y = idx / g.w;
            let x = idx % g.w;
            let off = y * g.stride + x * g.channels;
            if let Some(slot) = out.get_mut(off) {
                let cur = *slot & 1;
                let want: u8 = if bit { 1 } else { 0 };
                if cur != want {
                    *slot = (*slot & 0xfe) | want;
                    flips += 1;
                }
            }
            idx += 1;
            rep += 1;
        }
        bitpos += 1;
    }
    let distortion = if g.pixels == 0 {
        0.0
    } else {
        flips as f32 / g.pixels as f32
    };
    Ok((
        out,
        EmbedReport {
            bits,
            flips,
            capacity_bits: cap,
            redundancy: r,
            distortion,
        },
    ))
}

/// 从采样图提取载荷（多数表决）。与 `embed` 共用同一套下标推导，
/// 冗余度必须一致——强度是提取契约的一部分，不是可选调参。
pub fn extract(
    spec: &ImageSpec,
    payload_len: usize,
    strength: f32,
    img: &[u8],
) -> Result<Vec<u8>, WmError> {
    let g = validate(spec)?;
    let r = redundancy_of(strength)?;
    if payload_len == 0 {
        return Err(WmError::Payload);
    }
    if img.len() < g.frame {
        return Err(WmError::Trunc);
    }
    let bits = payload_len * 8;
    let cap = g.pixels / r as usize;
    if bits > cap {
        return Err(WmError::Cap);
    }
    let mut out = Vec::with_capacity(payload_len);
    let mut z = 0usize;
    while z < payload_len {
        out.push(0u8);
        z += 1;
    }
    let mut idx = 0usize;
    let mut bitpos = 0usize;
    while bitpos < bits {
        let mut ones = 0usize;
        let mut rep = 0u8;
        while rep < r {
            let y = idx / g.w;
            let x = idx % g.w;
            let off = y * g.stride + x * g.channels;
            if let Some(b) = img.get(off) {
                if *b & 1 == 1 {
                    ones += 1;
                }
            }
            idx += 1;
            rep += 1;
        }
        if ones * 2 > r as usize {
            if let Some(byte) = out.get_mut(bitpos >> 3) {
                *byte |= 1u8 << (bitpos & 7);
            }
        }
        bitpos += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 确定性语料发生器
// ---------------------------------------------------------------------------

/// xorshift64*：no_std 下没有 rand crate，且 fuzz 语料必须同种子可复现，
/// 否则「跑过 10^3 例」这句话在别的机器上无法被检验。
#[derive(Clone, Copy, Debug)]
pub struct Rng {
    s: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng {
            s: (seed ^ 0x9e37_79b9_7f4a_7c15) | 1,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.s = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as u32
        }
    }
}

/// 一例 fuzz 输入。
#[derive(Clone, Debug)]
pub struct FuzzCase {
    pub spec: ImageSpec,
    pub params: WmParams,
    pub buf: Vec<u8>,
}

fn build_buf(n: usize, rng: &mut Rng) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push((rng.next_u64() & 0xff) as u8);
        i += 1;
    }
    v
}

/// 生成一例语料：10 档里 1 档合法、9 档各打一个坏点。
///
/// 坏点的选取不是随机的，而是按校验器的分支顺序逐个覆盖——随机坏点
/// 很容易连续撞在同一分支上，跑一万次也只测到一条路径。
pub fn gen_case(rng: &mut Rng) -> FuzzCase {
    let w = 6 + rng.below(7);
    let h = 6 + rng.below(7);
    let channels = match rng.below(3) {
        0 => 1u8,
        1 => 3u8,
        _ => 4u8,
    };
    let stride = w * channels as u32;
    let frame = stride as usize * h as usize;
    let mut buf = build_buf(frame, rng);
    let strength = match rng.below(4) {
        0 => 0.25f32,
        1 => 0.5f32,
        2 => 0.75f32,
        _ => 1.0f32,
    };
    let r = match redundancy_of(strength) {
        Ok(v) => v as usize,
        Err(_) => 1,
    };
    let pixels = w as usize * h as usize;
    let max_bytes = pixels / (r * 8);
    let plen = if max_bytes < 2 {
        1
    } else {
        1 + rng.below(2) as usize
    };
    let mut payload = Vec::with_capacity(plen);
    let mut k = 0usize;
    while k < plen {
        payload.push((rng.next_u64() & 0xff) as u8);
        k += 1;
    }
    let mut spec = ImageSpec::new(w, h, stride, channels);
    let mut params = WmParams { strength, payload };
    match rng.below(10) {
        0 => spec.w = 0,
        1 => spec.h = 0,
        2 => spec.w = MAX_SIDE + 1,
        3 => spec.channels = 2,
        4 => {
            if spec.stride > 0 {
                spec.stride -= 1;
            }
        }
        5 => {
            let keep = frame / 2;
            while buf.len() > keep {
                buf.pop();
            }
        }
        6 => params.payload = Vec::new(),
        7 => {
            params.strength = if rng.below(2) == 0 { 1.5f32 } else { f32::NAN };
        }
        8 => {
            let mut big = Vec::new();
            let mut t = 0usize;
            while t < 48 {
                big.push(0xAA);
                t += 1;
            }
            params.payload = big;
        }
        _ => {}
    }
    FuzzCase { spec, params, buf }
}

/// fuzz 结果账：所有计数都必须满足 接受 + 拒绝 = 总数。
#[derive(Clone, Debug, PartialEq)]
pub struct FuzzLedger {
    pub total: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub roundtrip_ok: usize,
    pub max_distortion: f32,
    pub hist: [u32; CODE_SLOTS],
    pub digest: u64,
}

impl FuzzLedger {
    pub fn new() -> FuzzLedger {
        FuzzLedger {
            total: 0,
            accepted: 0,
            rejected: 0,
            roundtrip_ok: 0,
            max_distortion: 0.0,
            hist: [0u32; CODE_SLOTS],
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 实际命中的诊断码种类数（不含未命中的槽）。
    pub fn distinct_codes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 1usize;
        while i < CODE_SLOTS {
            match self.hist.get(i) {
                Some(v) => {
                    if *v > 0 {
                        n += 1;
                    }
                }
                None => {}
            }
            i += 1;
        }
        n
    }

    /// 闭集自检：任何落在 1..=8 之外的码都说明有未被登记的诊断路径。
    pub fn codes_closed(&self) -> bool {
        let mut i = 0usize;
        while i < CODE_SLOTS {
            match self.hist.get(i) {
                Some(v) => {
                    if i == 0 && *v != 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

fn fold(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 跑 `n` 例 fuzz。每例：生成 → 嵌入 → （成功时）提取往返 → 记入直方图。
///
/// 「零崩」在这里的落地口径：函数必须正常返回，且 1000 例逐例被分类；
/// 任何一次 panic 都会让调用方拿不到账本，也就谈不上断言。
pub fn run_fuzz(n: usize, seed: u64) -> FuzzLedger {
    let mut led = FuzzLedger::new();
    let mut rng = Rng::new(seed);
    let mut i = 0usize;
    while i < n {
        let c = gen_case(&mut rng);
        match embed(&c.spec, &c.params, &c.buf) {
            Ok((out, rep)) => {
                led.accepted += 1;
                let back = extract(&c.spec, c.params.payload.len(), c.params.strength, &out);
                let mut same = false;
                if let Ok(v) = back {
                    same = v == c.params.payload;
                }
                if same {
                    led.roundtrip_ok += 1;
                }
                if rep.distortion > led.max_distortion {
                    led.max_distortion = rep.distortion;
                }
                led.digest = fold(led.digest, 0);
            }
            Err(e) => {
                led.rejected += 1;
                let code = (e.code() - SEG_BASE) as usize;
                if let Some(slot) = led.hist.get_mut(code) {
                    *slot = slot.saturating_add(1);
                }
                led.digest = fold(led.digest, code as u64);
            }
        }
        i += 1;
    }
    led.total = n;
    led
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn params(payload: &[u8], strength: f32) -> WmParams {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            v.push(*b);
        }
        i += 1;
    }
    WmParams {
        strength,
        payload: v,
    }
}

fn zeros(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(0u8);
        i += 1;
    }
    v
}

fn gray(w: u32, h: u32) -> ImageSpec {
    ImageSpec::new(w, h, w, 1)
}

/// 八类坏样本。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    // 1 零维
    let bad = ImageSpec::new(0, 4, 4, 1);
    if embed(&bad, &params(&[1], 1.0), &zeros(16)).err() == Some(WmError::Dim) {
        caught += 1;
    }

    // 2 超面积
    let bad = ImageSpec::new(MAX_SIDE + 1, 8, MAX_SIDE + 1, 1);
    if embed(&bad, &params(&[1], 1.0), &[]).err() == Some(WmError::Area) {
        caught += 1;
    }

    // 3 非法通道
    let bad = ImageSpec::new(4, 4, 8, 2);
    if embed(&bad, &params(&[1], 1.0), &zeros(32)).err() == Some(WmError::Chan) {
        caught += 1;
    }

    // 4 步长不足
    let bad = ImageSpec::new(4, 4, 3, 1);
    if embed(&bad, &params(&[1], 1.0), &zeros(12)).err() == Some(WmError::Stride) {
        caught += 1;
    }

    // 5 缓冲截断
    let ok_spec = gray(4, 4);
    if embed(&ok_spec, &params(&[1], 1.0), &zeros(8)).err() == Some(WmError::Trunc) {
        caught += 1;
    }

    // 6 空载荷
    if embed(&ok_spec, &params(&[], 1.0), &zeros(16)).err() == Some(WmError::Payload) {
        caught += 1;
    }

    // 7 参数越界（NaN）
    if embed(&ok_spec, &params(&[1], f32::NAN), &zeros(16)).err() == Some(WmError::Param) {
        caught += 1;
    }

    // 8 容量不足（64 bit 载荷塞 16 像素图）
    let mut big = Vec::new();
    let mut i = 0usize;
    while i < 8 {
        big.push(0xFFu8);
        i += 1;
    }
    if embed(&ok_spec, &params(&big, 1.0), &zeros(16)).err() == Some(WmError::Cap) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50054_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50054");

    // J1-1 fuzz ×10^3 零崩：1000 例全部被分类，且至少有接受样本
    let led = run_fuzz(1000, 0x0054_5EED);
    let ok = led.total == 1000 && led.accepted + led.rejected == 1000 && led.accepted > 0;
    let det = format!(
        "ok={} rej={} total={}",
        led.accepted, led.rejected, led.total
    );
    s.add_dyn("50054-J1-万例零崩", ok, &det);

    // J1-2 接受样本往返一致（嵌入→提取 == 原载荷）
    let ok = led.accepted > 0 && led.roundtrip_ok == led.accepted;
    let det = format!("roundtrip={}/{}", led.roundtrip_ok, led.accepted);
    s.add_dyn("50054-J1-往返一致", ok, &det);

    // J1-3 语料确定性：同种子两跑，摘要/接受数/直方图逐位一致
    let led2 = run_fuzz(1000, 0x0054_5EED);
    let ok = led2.digest == led.digest
        && led2.accepted == led.accepted
        && led2.hist == led.hist
        && led2.roundtrip_ok == led.roundtrip_ok;
    let det = format!("d1={:016x} d2={:016x}", led.digest, led2.digest);
    s.add_dyn("50054-J1-语料确定", ok, &det);

    // J1-4 诊断码闭集 + 覆盖：槽 0 恒空，命中种类 ≥6
    let ok = led.codes_closed() && led.distinct_codes() >= 6;
    let det = format!("distinct={}", led.distinct_codes());
    s.add_dyn("50054-J1-码闭集", ok, &det);

    // J1-5 写入位数守恒：零图 + 全 1 载荷，翻转数 == 位数 × 冗余度
    let spec = gray(8, 8);
    let p1 = params(&[0xFF], 1.0);
    let zeros64 = zeros(64);
    let emb = embed(&spec, &p1, &zeros64);
    let ok = match &emb {
        Ok((_, rep)) => {
            rep.redundancy == 4
                && rep.bits == 8
                && rep.flips == rep.bits * rep.redundancy as usize
                && rep.distortion <= 0.5 + 1e-6
        }
        Err(_) => false,
    };
    let det = match &emb {
        Ok((_, rep)) => format!("flips={} bits={}", rep.flips, rep.bits),
        Err(e) => format!("err={}", e.message()),
    };
    s.add_dyn("50054-J1-写入守恒", ok, &det);

    // J1-6 全 0 载荷零翻转（嵌入是内容敏感的最小改动，不是无脑刷位）
    let p0 = params(&[0x00], 1.0);
    let ok = match embed(&spec, &p0, &zeros64) {
        Ok((_, rep)) => rep.flips == 0 && rep.distortion == 0.0,
        Err(_) => false,
    };
    s.add("50054-J1-零载荷零翻转", ok, if ok { "" } else { "全零载荷仍在翻转" });

    // J1-7 零维必拒
    let ok = embed(&ImageSpec::new(0, 4, 4, 1), &params(&[1], 1.0), &zeros(16)).err()
        == Some(WmError::Dim);
    s.add("50054-J1-零维必拒", ok, if ok { "" } else { "零维被放行" });

    // J1-8 超面积必拒
    let big = ImageSpec::new(5000, 8, 5000, 1);
    let ok = embed(&big, &params(&[1], 1.0), &[]).err() == Some(WmError::Area);
    s.add("50054-J1-超面积必拒", ok, if ok { "" } else { "超面积被放行" });

    // J1-9 非法通道必拒
    let bad = ImageSpec::new(4, 4, 8, 2);
    let ok = embed(&bad, &params(&[1], 1.0), &zeros(32)).err() == Some(WmError::Chan);
    s.add("50054-J1-通道必拒", ok, if ok { "" } else { "非法通道被放行" });

    // J1-10 步长不足必拒
    let bad = ImageSpec::new(4, 4, 3, 1);
    let ok = embed(&bad, &params(&[1], 1.0), &zeros(12)).err() == Some(WmError::Stride);
    s.add("50054-J1-步长必拒", ok, if ok { "" } else { "步长不足被放行" });

    // J1-11 缓冲截断必拒
    let ok = embed(&gray(4, 4), &params(&[1], 1.0), &zeros(8)).err() == Some(WmError::Trunc);
    s.add("50054-J1-截断必拒", ok, if ok { "" } else { "截断缓冲被放行" });

    // J1-12 空载荷必拒
    let ok = embed(&gray(4, 4), &params(&[], 1.0), &zeros(16)).err() == Some(WmError::Payload);
    s.add("50054-J1-空载荷必拒", ok, if ok { "" } else { "空载荷被放行" });

    // J1-13 参数越界必拒：NaN、-0.5、1.5 全部 Param；[0,1] 内必接受
    let nan = redundancy_of(f32::NAN).err() == Some(WmError::Param);
    let low = redundancy_of(-0.5).err() == Some(WmError::Param);
    let high = redundancy_of(1.5).err() == Some(WmError::Param);
    let ok = nan && low && high && redundancy_of(0.0).is_ok() && redundancy_of(1.0).is_ok();
    s.add("50054-J1-参数必拒", ok, if ok { "" } else { "越界强度未挡" });

    // J1-14 容量不足必拒
    let mut eight = Vec::new();
    let mut i = 0usize;
    while i < 8 {
        eight.push(0xFFu8);
        i += 1;
    }
    let ok = embed(&gray(4, 4), &params(&eight, 1.0), &zeros(16)).err() == Some(WmError::Cap);
    s.add("50054-J1-容量必拒", ok, if ok { "" } else { "超容量被放行" });

    // J1-15 冗余度单调 + 容量反向：强度升 → 冗余不降 → 容量不升
    let (r0, r5, r1) = (redundancy_of(0.0), redundancy_of(0.5), redundancy_of(1.0));
    let ok = match (r0, r5, r1) {
        (Ok(a), Ok(b), Ok(c)) => {
            let cap_spec = gray(16, 16);
            let ca = capacity_bits(&cap_spec, a);
            let cb = capacity_bits(&cap_spec, b);
            let cc = capacity_bits(&cap_spec, c);
            match (ca, cb, cc) {
                (Ok(x), Ok(y), Ok(z)) => a <= b && b <= c && x >= y && y >= z && a == 1 && c == 4,
                _ => false,
            }
        }
        _ => false,
    };
    s.add("50054-J1-冗余单调", ok, if ok { "" } else { "冗余/容量关系不单调" });

    // J1-16 提取与嵌入同契约：错误码同构（同一坏 spec 两边都拒）
    let bad = ImageSpec::new(4, 4, 3, 1);
    let ok = extract(&bad, 1, 1.0, &zeros(12)).err() == Some(WmError::Stride)
        && extract(&gray(4, 4), 0, 1.0, &zeros(16)).err() == Some(WmError::Payload);
    s.add("50054-J1-提取同构", ok, if ok { "" } else { "提取端校验缺失" });

    // J1-17 注入核八类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50054-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50054_checks_green() {
        let set = run_unx50054_checks();
        assert!(set.all_passed(), "UNX-F50054 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50054_fuzz_is_reproducible() {
        let a = run_fuzz(200, 7);
        let b = run_fuzz(200, 7);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.accepted, b.accepted);
        assert_eq!(a.hist, b.hist);
    }
}
