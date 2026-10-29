//! cgb822_power_profile — CGPU-F0822 Intel 功耗画像。
//!
//! 判据原文（验收行摘录）：Intel 功耗画像：RC6 驻留采样（隔帧采样驻留百
//! 分比记账）；频率桶统计（实际频率落入离散桶，桶计数守恒）；温度余量评
//! 估（降频触发阈值判定，整数化比较）；画像归类（按驻留+频率桶分布输出
//! 省电型/均衡型/性能型三类标签）。判据：采样记账、频率桶、余量阈值、
//! 三类画像。
//!
//! 诊断段 0x7140（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std（百分比一律乘
//! 1000 定点整数化）；失败路径 Result<_, u16> 闭集错误码；采样账本定容
//! （环形覆盖），桶计数守恒（桶总和 == 有效样本数）；drill() 自例程返
//! 回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb822-v1";

/// 诊断段基（本单独占 0x7140）。
pub const SEG_BASE: u16 = 0x7140;

/// 采样未开始（未 init 即 push）。
pub const XC_NOT_SAMPLING: u16 = SEG_BASE + 1;
/// 驻留率越界（大于 100.000% 的定点值——硬件计数回绕须申报）。
pub const XC_RESID_RANGE: u16 = SEG_BASE + 2;
/// 频率越界（超出覆盖范围——不可归桶）。
pub const XC_FREQ_RANGE: u16 = SEG_BASE + 3;
/// 样本不足（画像归类要求最少样本数）。
pub const XC_SAMPLES_SHORT: u16 = SEG_BASE + 4;
/// 温度读数非法（超过标定上限——传感器违约）。
pub const XC_TEMP_INVALID: u16 = SEG_BASE + 5;
/// 桶账漂移（桶计数总和 ≠ 有效样本数——守恒破坏）。
pub const XC_BUCKET_DRIFT: u16 = SEG_BASE + 6;

/// 采样账本定容（环形覆盖最旧样本）。
pub const SAMPLE_CAP: usize = 32;
/// 画像归类最少有效样本数。
pub const CLASSIFY_MIN: usize = 4;
/// 百分比定点缩放（100.000% = 100_000）。
pub const PCT_SCALE: u64 = 100_000;
/// 频率覆盖上限（MHz 定点）。
pub const FREQ_MAX_MHZ: u64 = 3000;
/// 温度标定上限（摄氏度）。
pub const TEMP_MAX_C: u64 = 110;

/// 频率桶边界（MHz）：0-399/400-799/800-1199/1200-1599/1600-1999/2000+。
pub const FREQ_BUCKETS: usize = 6;

/// 频率桶名。
pub fn bucket_name(i: usize) -> &'static str {
    match i {
        0 => "<400",
        1 => "400-799",
        2 => "800-1199",
        3 => "1200-1599",
        4 => "1600-1999",
        _ => "2000+",
    }
}

/// 单帧采样记录。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// 帧序号。
    pub frame: u64,
    /// RC6 驻留率（定点 PCT_SCALE，如 87_500 表示 87.5%）。
    pub rc6_pct: u64,
    /// 实际渲染频率（MHz）。
    pub freq_mhz: u64,
    /// 封装温度（摄氏度）。
    pub temp_c: u64,
    /// 本帧是否发生降频（PL1/PL2 触发）。
    pub throttled: bool,
}

/// 画像标签（判据"三类画像"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// 省电型：驻留率高、低频桶占比大。
    Saver,
    /// 均衡型：驻留与频率分布居中。
    Balanced,
    /// 性能型：驻留率低、高频桶占比大、降频少。
    Perf,
}

impl Profile {
    /// 标签名。
    pub fn name(self) -> &'static str {
        match self {
            Profile::Saver => "saver",
            Profile::Balanced => "balanced",
            Profile::Perf => "perf",
        }
    }
}

/// 降频触发判定（判据"余量阈值"——整数化比较）。
/// 温度余量 = TEMP_MAX_C - temp；余量低于阈值且频率高于降频线则触发。
pub fn throttle_check(temp_c: u64, freq_mhz: u64) -> Result<bool, u16> {
    if temp_c > TEMP_MAX_C {
        return Err(XC_TEMP_INVALID);
    }
    let headroom = TEMP_MAX_C - temp_c;
    // 余量 < 15 度 且 频率 >= 1200MHz → 判定降频触发。
    Ok(headroom < 15 && freq_mhz >= 1200)
}

/// 功耗画像采样器。
pub struct PowerProfiler {
    /// 采样器名。
    pub name: String,
    /// 是否开始采样。
    sampling: bool,
    /// 环形样本账（定容，满则覆盖最旧）。
    ring: Vec<Sample>,
    /// 写指针。
    wpos: usize,
    /// 有效样本数（≤ SAMPLE_CAP）。
    valid: usize,
    /// 频率桶计数（守恒项）。
    buckets: [u64; FREQ_BUCKETS],
    /// 降频触发计数。
    throttle_cnt: u64,
    /// 拒绝计数。
    pub reject: u64,
}

/// 守恒恒等式：六个频率桶计数之和 == valid（有效样本数）。
/// 环形覆盖丢弃旧样本时同步递减对应桶，每步 push 后 verify() 复核。
impl PowerProfiler {
    /// 构造。
    pub fn new(name: &str) -> Self {
        PowerProfiler {
            name: name.to_string(),
            sampling: false,
            ring: Vec::new(),
            wpos: 0,
            valid: 0,
            buckets: [0; FREQ_BUCKETS],
            throttle_cnt: 0,
            reject: 0,
        }
    }

    /// 开始采样。
    pub fn start(&mut self) {
        self.sampling = true;
    }

    /// 归桶（判据"频率桶"）。
    pub fn bucket_of(freq: u64) -> Result<usize, u16> {
        if freq > FREQ_MAX_MHZ {
            return Err(XC_FREQ_RANGE);
        }
        Ok(match freq {
            0..=399 => 0,
            400..=799 => 1,
            800..=1199 => 2,
            1200..=1599 => 3,
            1600..=1999 => 4,
            _ => 5,
        })
    }

    /// 推入一帧采样（环形覆盖）。
    pub fn push(&mut self, s: Sample) -> Result<(), u16> {
        if !self.sampling {
            self.reject += 1;
            return Err(XC_NOT_SAMPLING);
        }
        if s.rc6_pct > PCT_SCALE {
            self.reject += 1;
            return Err(XC_RESID_RANGE);
        }
        if s.freq_mhz > FREQ_MAX_MHZ {
            self.reject += 1;
            return Err(XC_FREQ_RANGE);
        }
        if s.temp_c > TEMP_MAX_C {
            self.reject += 1;
            return Err(XC_TEMP_INVALID);
        }
        let b = Self::bucket_of(s.freq_mhz)?;
        if self.valid < SAMPLE_CAP {
            self.ring.push(s);
            self.valid += 1;
        } else {
            // 覆盖最旧：先扣旧样本的桶账。
            let old = self.ring[self.wpos];
            let ob = Self::bucket_of(old.freq_mhz)?;
            self.buckets[ob] -= 1;
            if old.throttled {
                self.throttle_cnt -= 1;
            }
            self.ring[self.wpos] = s;
        }
        self.buckets[b] += 1;
        if s.throttled {
            self.throttle_cnt += 1;
        }
        self.wpos = (self.wpos + 1) % SAMPLE_CAP;
        self.verify()?;
        Ok(())
    }

    /// 有效样本数。
    pub fn len(&self) -> usize {
        self.valid
    }

    /// 按时序读取全部有效样本（旧→新）。
    pub fn ordered(&self) -> Vec<Sample> {
        let mut out = Vec::new();
        if self.valid == 0 {
            return out;
        }
        let start = if self.valid < SAMPLE_CAP { 0 } else { self.wpos };
        for i in 0..self.valid {
            out.push(self.ring[(start + i) % SAMPLE_CAP]);
        }
        out
    }

    /// 平均驻留率（定点，整数平均——先求和后除）。
    pub fn avg_rc6(&self) -> Result<u64, u16> {
        if self.valid < CLASSIFY_MIN {
            return Err(XC_SAMPLES_SHORT);
        }
        let mut sum = 0u64;
        for s in self.ordered() {
            sum += s.rc6_pct;
        }
        Ok(sum / self.valid as u64)
    }

    /// 高频桶占比（定点 PCT_SCALE）：桶 4+5 计数 / 有效样本。
    pub fn high_freq_pct(&self) -> Result<u64, u16> {
        if self.valid < CLASSIFY_MIN {
            return Err(XC_SAMPLES_SHORT);
        }
        let hi = self.buckets[4] + self.buckets[5];
        Ok(hi * PCT_SCALE / self.valid as u64)
    }

    /// 画像归类（判据"三类画像"——阈值整数化）。
    /// 驻留均值 ≥ 60% 且高频占比 < 25% → 省电型；
    /// 高频占比 ≥ 50% 或降频占比 < 10% 且驻留 < 40% → 性能型；其余均衡型。
    pub fn classify(&self) -> Result<Profile, u16> {
        let avg = self.avg_rc6()?;
        let hi = self.high_freq_pct()?;
        let thr_pct = self.throttle_cnt * PCT_SCALE / self.valid as u64;
        if avg >= 60_000 && hi < 25_000 {
            return Ok(Profile::Saver);
        }
        if hi >= 50_000 || (avg < 40_000 && thr_pct < 10_000) {
            return Ok(Profile::Perf);
        }
        Ok(Profile::Balanced)
    }

    /// 守恒校验（判据"桶计数守恒"）。
    pub fn verify(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        for b in self.buckets.iter() {
            sum += *b;
        }
        if sum != self.valid as u64 {
            return Err(XC_BUCKET_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb822[");
        s.push_str(&self.name);
        s.push_str("] valid=");
        s.push_str(&self.valid.to_string());
        s.push_str(" thr=");
        s.push_str(&self.throttle_cnt.to_string());
        s.push_str(" buckets=[");
        for i in 0..FREQ_BUCKETS {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&self.buckets[i].to_string());
        }
        s.push_str("] rej=");
        s.push_str(&self.reject.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut p = PowerProfiler::new("w3-drill-822");

    // 负样本 1：未开始采样——应拒。
    match p.push(Sample { frame: 0, rc6_pct: 50_000, freq_mhz: 900, temp_c: 60, throttled: false }) {
        Err(code) if code == XC_NOT_SAMPLING => {}
        Ok(_) => return Err(XC_NOT_SAMPLING),
        Err(other) => return Err(other),
    }
    p.start();

    // 负样本 2：驻留率越界——应拒。
    match p.push(Sample { frame: 1, rc6_pct: PCT_SCALE + 1, freq_mhz: 900, temp_c: 60, throttled: false }) {
        Err(code) if code == XC_RESID_RANGE => {}
        Ok(_) => return Err(XC_RESID_RANGE),
        Err(other) => return Err(other),
    }
    // 负样本 3：频率越界——应拒。
    match p.push(Sample { frame: 2, rc6_pct: 0, freq_mhz: FREQ_MAX_MHZ + 1, temp_c: 60, throttled: false }) {
        Err(code) if code == XC_FREQ_RANGE => {}
        Ok(_) => return Err(XC_FREQ_RANGE),
        Err(other) => return Err(other),
    }
    // 负样本 4：温度非法——应拒。
    match p.push(Sample { frame: 3, rc6_pct: 0, freq_mhz: 900, temp_c: TEMP_MAX_C + 1, throttled: false }) {
        Err(code) if code == XC_TEMP_INVALID => {}
        Ok(_) => return Err(XC_TEMP_INVALID),
        Err(other) => return Err(other),
    }

    // 正样本：性能型序列（低驻留、高频、不降频）。
    let perf = [
        (10, 2000, 70, false),
        (12, 2100, 72, false),
        (15, 2200, 71, false),
        (18, 2400, 73, false),
        (20, 2500, 74, false),
    ];
    for (i, (rc, fq, tp, th)) in perf.iter().enumerate() {
        p.push(Sample { frame: 10 + i as u64, rc6_pct: *rc * 1000, freq_mhz: *fq, temp_c: *tp, throttled: *th })?;
    }
    let prof = p.classify()?;
    if prof != Profile::Perf {
        return Err(XC_BUCKET_DRIFT);
    }

    // 降频阈值单元：高温高频应触发，低温不触发。
    if !throttle_check(100, 1300)? {
        return Err(XC_TEMP_INVALID);
    }
    if throttle_check(50, 1300)? {
        return Err(XC_TEMP_INVALID);
    }

    // 环形覆盖：再推 30 帧省电型样本，旧样本被覆盖，桶账仍守恒。
    let saver = [
        (90, 300, 45, false),
        (92, 400, 46, false),
        (88, 350, 44, false),
        (95, 380, 47, false),
    ];
    for (i, (rc, fq, tp, th)) in saver.iter().enumerate() {
        p.push(Sample { frame: 100 + i as u64, rc6_pct: *rc * 1000, freq_mhz: *fq, temp_c: *tp, throttled: *th })?;
    }
    for i in 0..30u64 {
        let rc = 80 + (i % 10);
        p.push(Sample { frame: 200 + i, rc6_pct: rc * 1000, freq_mhz: 500, temp_c: 50, throttled: false })?;
    }
    p.verify()?;
    if p.len() != SAMPLE_CAP {
        return Err(XC_BUCKET_DRIFT);
    }
    let prof2 = p.classify()?;
    if prof2 != Profile::Saver {
        return Err(XC_BUCKET_DRIFT);
    }
    // 平均驻留应大于 60%（省电型前提复核）。
    if p.avg_rc6()? < 60_000 {
        return Err(XC_BUCKET_DRIFT);
    }
    if p.reject < 4 {
        return Err(XC_BUCKET_DRIFT);
    }
    Ok(0)
}
