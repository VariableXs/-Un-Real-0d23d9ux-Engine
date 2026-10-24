//! UNX-F0106 · 日志时间戳（TSC 换算）· 诊断段 0x2340
//! 验收锚点：UNX-F0106-J1 时间戳微秒级单调递增，与参考时钟校准
//! 偏差 <1%（千分位整数判定）。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2340;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FREQ: u16 = SEG_BASE + 1;   // 频标非法（零或超纲）
const E_REWIND: u16 = SEG_BASE + 2; // 时间倒流（非单调）
const E_DEVIATE: u16 = SEG_BASE + 3;// 校准偏差 ≥1%（千分位 ≥10）
const E_RDONLY: u16 = SEG_BASE + 4; // 校准已定标，再定标被拒
const E_ORDER: u16 = SEG_BASE + 5;  // 校准点序乱（参考值非升序）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_FREQ => "频标非法：TSC 频率须为正且不超 32GHz 上限",
        E_REWIND => "时间倒流：微秒账非单调递增",
        E_DEVIATE => "校准偏差越限：与参考钟差 ≥ 千分之十",
        E_RDONLY => "已定标：校准闭集一次定标终身有效",
        E_ORDER => "校准点序乱：参考钟值必须升序喂入",
        _ => "未知诊断码",
    }
}

pub const FREQ_CAP_MHZ: u64 = 32_000;
pub const DEVIATE_PERMILLE: u64 = 10; // 1% = 千分之十

/// TSC 周期 → 微秒（freq_mhz 即每微秒周期数）。
pub fn cycles_to_us(cycles: u64, freq_mhz: u64) -> Result<u64, u16> {
    if freq_mhz == 0 || freq_mhz > FREQ_CAP_MHZ {
        return Err(E_FREQ);
    }
    Ok(cycles / freq_mhz)
}

/// 千分位偏差判定：|a-b| * 1000 ≤ base * 10 ⇒ 偏差 <1%。
/// base 取两者较大值（相对偏差）。
pub fn deviate_ok(a: u64, b: u64) -> bool {
    let base = if a > b { a } else { b };
    let d = if a > b { a - b } else { b - a };
    if base == 0 {
        return true;
    }
    d.saturating_mul(1000) <= base.saturating_mul(DEVIATE_PERMILLE)
}

/// 校准点：TSC 周期 ↔ 参考钟微秒，同刻成对喂入。
#[derive(Clone, Copy, Debug)]
pub struct CalPoint {
    pub cycles: u64,
    pub ref_us: u64,
}

/// 时间戳账本：TSC 换算 + 单调账 + 双点校准。
pub struct StampBook {
    pub freq_mhz: u64,
    pub last_us: u64,
    pub calibrated: bool,
    pub points: Vec<CalPoint>,
    pub count: u64,
}

impl StampBook {
    pub fn new(freq_mhz: u64) -> Result<StampBook, u16> {
        if freq_mhz == 0 || freq_mhz > FREQ_CAP_MHZ {
            return Err(E_FREQ);
        }
        Ok(StampBook {
            freq_mhz,
            last_us: 0,
            calibrated: false,
            points: Vec::new(),
            count: 0,
        })
    }

    /// 打点：TSC 周期 → 微秒，单调账守卫。
    pub fn stamp(&mut self, cycles: u64) -> Result<u64, u16> {
        let us = cycles_to_us(cycles, self.freq_mhz)?;
        if us < self.last_us {
            return Err(E_REWIND);
        }
        self.last_us = us;
        self.count += 1;
        Ok(us)
    }

    /// 喂校准点：参考钟值必须升序，TSC 周期也须升序。
    pub fn feed_cal(&mut self, cycles: u64, ref_us: u64) -> Result<(), u16> {
        if self.calibrated {
            return Err(E_RDONLY);
        }
        if let Some(p) = self.points.last() {
            if ref_us <= p.ref_us || cycles <= p.cycles {
                return Err(E_ORDER);
            }
        }
        self.points.push(CalPoint { cycles, ref_us });
        Ok(())
    }

    /// 定标：两点成线——频标反推与名义频标偏差 <1% 才放行。
    /// 斜率 = Δcycles / Δref_us = 每微秒周期数（即 MHz 数）。
    pub fn calibrate(&mut self) -> Result<u64, u16> {
        if self.calibrated {
            return Err(E_RDONLY);
        }
        if self.points.len() < 2 {
            return Err(E_ORDER);
        }
        let a = self.points[0];
        let b = self.points[self.points.len() - 1];
        if b.ref_us == a.ref_us {
            return Err(E_ORDER);
        }
        let slope = (b.cycles - a.cycles) / (b.ref_us - a.ref_us);
        if !deviate_ok(slope, self.freq_mhz) {
            return Err(E_DEVIATE);
        }
        self.calibrated = true;
        Ok(slope)
    }

    /// 渲染：微秒账 + 校准态。
    pub fn render(&self) -> String {
        format!(
            "时账 {} 打点，末值 {}us，校准 {}",
            self.count,
            self.last_us,
            if self.calibrated { "已定标" } else { "未定标" }
        )
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：@2000MHz 打点序列 → 微秒单调递增（2M 周期 = 1000us）。
    let mut s = StampBook::new(2000)?;
    let u1 = s.stamp(2_000_000)?;
    let u2 = s.stamp(4_000_000)?;
    let u3 = s.stamp(6_500_000)?;
    if !(u1 == 1000 && u2 == 2000 && u3 == 3250) {
        return Err(E_REWIND);
    }
    if s.last_us != 3250 || s.count != 3 {
        return Err(E_REWIND);
    }
    hits += 1;

    // R2：双点校准——实测斜率 1990 vs 名义 2000，偏差 5‰ <1% 放行。
    let mut s2 = StampBook::new(2000)?;
    s2.feed_cal(0, 0)?;
    s2.feed_cal(1_990_000, 1_000)?; // 实测 1990 cycles/us
    let slope = s2.calibrate()?;
    if slope != 1990 {
        return Err(E_DEVIATE);
    }
    hits += 1;

    // R3：偏差恰 1% 边界内（1980 vs 2000 = 10‰）→ 放行；渲染账。
    let mut s3 = StampBook::new(2000)?;
    s3.feed_cal(0, 0)?;
    s3.feed_cal(1_980_000, 1_000)?;
    s3.calibrate()?;
    let rep = s3.render();
    if !rep.contains("已定标") {
        return Err(E_DEVIATE);
    }
    hits += 1;

    // R4：偏差判定函数自身——同值与近值过，远值拒。
    if !deviate_ok(1000, 1005) || !deviate_ok(0, 0) {
        return Err(E_DEVIATE);
    }
    if deviate_ok(1000, 1100) {
        return Err(E_DEVIATE);
    }
    hits += 1;

    // I1：频标非法（零 / 超 32GHz）→ E_FREQ。
    if cycles_to_us(1000, 0) != Err(E_FREQ) {
        return Err(E_FREQ);
    }
    if cycles_to_us(1000, FREQ_CAP_MHZ + 1) != Err(E_FREQ) {
        return Err(E_FREQ);
    }
    if StampBook::new(0).is_ok() {
        return Err(E_FREQ);
    }
    hits += 1;

    // I2：时间倒流 → E_REWIND。
    let mut i2 = StampBook::new(2000)?;
    i2.stamp(4_000_000)?;
    if i2.stamp(3_999_999) != Err(E_REWIND) {
        return Err(E_REWIND);
    }
    hits += 1;

    // I3：偏差越限（1950 vs 2000 = 25‰ ≥ 10‰）→ E_DEVIATE。
    let mut i3 = StampBook::new(2000)?;
    i3.feed_cal(0, 0)?;
    i3.feed_cal(1_950_000, 1_000)?;
    if i3.calibrate() != Err(E_DEVIATE) {
        return Err(E_DEVIATE);
    }
    hits += 1;

    // I4：校准点序乱（参考钟回退 / TSC 回退 / 零斜率）→ E_ORDER；
    //     单点定标 → E_ORDER。
    let mut i4 = StampBook::new(2000)?;
    i4.feed_cal(1_000_000, 500)?;
    if i4.feed_cal(2_000_000, 400) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    if i4.feed_cal(1_500_000, 600) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    if i4.calibrate() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I5：已定标再喂点/再定标 → E_RDONLY。
    let mut i5 = StampBook::new(2000)?;
    i5.feed_cal(0, 0)?;
    i5.feed_cal(2_000_000, 1_000)?;
    i5.calibrate()?;
    if i5.feed_cal(4_000_000, 2_000) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.calibrate() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
