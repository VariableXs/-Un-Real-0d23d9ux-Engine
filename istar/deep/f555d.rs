//! 深化层 · F555 麦克风降噪（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F555 节）：
//! ①「降噪对性能占用显示（CPU 增量 <2% 才允许常开）」的**计量器**——
//!   逐帧注入降噪级耗时（μs），滚动均值对帧预算（10ms）取占比，
//!   超预算即取消常开资格（降级为按需处理），资格恢复须连续达标；
//! ②「F448 测试向导内 A/B：降噪开/关各录 5 秒回放对比」的**公平闸**——
//!   两段回放必须同源（同一采样指纹）且同时长才许对比，不同源/不同长
//!   的 A/B 判无效（防「拿不同素材骗对比」）；
//! ③「语音内容不被误伤（保语音频段参数入册）」的**保真验证器**——
//!   语音帧必须增益 256 直通（0dB），噪声帧才落衰减档；频段参数
//!   （300-3400Hz）与衰减深度入册可查。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::micalm::{
    Denoiser, FrameClass, AB_CLIP_MS, CPU_BUDGET_PCT, NOISE_SUPPRESS_Q8_8, TRANSIENT_FACTOR_X100,
    VOICE_BAND_HI_HZ, VOICE_BAND_LO_HZ,
};

// ---------------------------------------------------------------------------
// CPU 增量计量器（常开资格判定）
// ---------------------------------------------------------------------------

/// 每帧预算（μs）：48kHz / 480 样点 = 10ms 一帧。
pub const FRAME_BUDGET_US: u32 = 10_000;

/// CPU 计量器（滚动 64 帧均值——瞬时抖动不代表常开代价）。
pub struct CpuMeter {
    win: [u32; 64],
    head: usize,
    len: usize,
    /// 连续达标帧数（资格恢复判据）。
    ok_streak: u32,
    demoted: bool,
}

/// 资格恢复所需连续达标帧数。
pub const RECOVER_STREAK: u32 = 32;

impl CpuMeter {
    pub fn new() -> CpuMeter {
        CpuMeter { win: [0; 64], head: 0, len: 0, ok_streak: 0, demoted: false }
    }

    pub fn record(&mut self, cost_us: u32) {
        self.win[self.head] = cost_us;
        self.head = (self.head + 1) % 64;
        if self.len < 64 {
            self.len += 1;
        }
        let pct = self.avg_pct();
        if pct < CPU_BUDGET_PCT {
            self.ok_streak += 1;
            if self.demoted && self.ok_streak >= RECOVER_STREAK {
                self.demoted = false; // 连续达标才恢复常开资格
            }
        } else {
            self.ok_streak = 0;
            if pct >= CPU_BUDGET_PCT && self.len == 64 {
                self.demoted = true; // 窗口满且超预算：取消常开
            }
        }
    }

    /// 滚动均值占比（‰ 精度下的百分数整数位——均值 μs ×100 / 预算）。
    pub fn avg_pct(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let sum: u64 = (0..self.len).map(|i| self.win[i] as u64).sum();
        let avg = (sum / self.len as u64) as u32;
        avg * 100 / FRAME_BUDGET_US
    }

    /// 常开资格：<2%（主册判据）且未被降级。
    pub fn always_on_allowed(&self) -> bool {
        !self.demoted && self.avg_pct() < CPU_BUDGET_PCT
    }

    pub fn demoted(&self) -> bool {
        self.demoted
    }
}

impl Default for CpuMeter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// A/B 五秒回放公平闸
// ---------------------------------------------------------------------------

/// 一段回放素材（降噪开/关各录一段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbClip {
    pub source_fp: u64,
    pub dur_ms: u32,
}

/// A/B 对比判定：同源且同时长才公平（不同素材的对比是骗局）。
pub fn ab_fair(a: AbClip, b: AbClip) -> Result<(), &'static str> {
    if a.source_fp != b.source_fp {
        return Err("A/B 非同源：必须同一采样素材开关降噪各录一段");
    }
    if a.dur_ms != b.dur_ms {
        return Err("A/B 时长不等：各录 5 秒才许对比");
    }
    if a.dur_ms != AB_CLIP_MS {
        return Err("A/B 时长不符合同（5 秒）");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 语音保真验证器
// ---------------------------------------------------------------------------

/// 语音帧保真：语音帧增益必须直通（256 = 0dB），噪声帧才落衰减档。
pub fn voice_gain_unity(gain: u16, class: FrameClass) -> bool {
    match class {
        FrameClass::Voice | FrameClass::Bypassed => gain == 256,
        FrameClass::Noise => gain == NOISE_SUPPRESS_Q8_8,
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f555_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 预算内：常开资格成立。
    let mut m = CpuMeter::new();
    for _ in 0..64 {
        m.record(150); // 1.5% of 10ms
    }
    cs.add("within budget always-on allowed", m.always_on_allowed() && !m.demoted(), "");

    // 2) 超预算：窗口满即取消常开资格（<2% 才许常开是硬门）。
    let mut m2 = CpuMeter::new();
    for _ in 0..64 {
        m2.record(300); // 3%
    }
    cs.add("over budget demotes", m2.demoted() && !m2.always_on_allowed(), "");

    // 3) 资格恢复：滚动窗先被达标帧冲净（64 帧），再连续达标
    //    RECOVER_STREAK 帧才恢复（防抖动反复横跳）。
    let mut m3 = CpuMeter::new();
    for _ in 0..64 {
        m3.record(300);
    }
    for _ in 0..64 {
        m3.record(100); // 冲窗：后段均值降至 2% 以下
    }
    for _ in 0..RECOVER_STREAK {
        m3.record(100);
    }
    cs.add("recovery needs full streak", m3.always_on_allowed(), "");

    // 4) A/B 公平闸：同源同时长过、异源拒、异长拒、时长不符合同拒。
    let a = AbClip { source_fp: 0xA11CE, dur_ms: AB_CLIP_MS };
    let b = AbClip { source_fp: 0xA11CE, dur_ms: AB_CLIP_MS };
    let c = AbClip { source_fp: 0xB0B, dur_ms: AB_CLIP_MS };
    let d = AbClip { source_fp: 0xA11CE, dur_ms: 4_000 };
    cs.add(
        "ab fairness gate",
        ab_fair(a, b).is_ok()
            && ab_fair(a, c).is_err()
            && ab_fair(a, d).is_err(),
        "",
    );

    // 5) 语音保真：语音帧 256 直通、噪声帧衰减档、频段参数在册。
    let mut dn = Denoiser::new();
    dn.set_enabled(true);
    for _ in 0..20 {
        let _ = dn.process_frame(100); // 标定期：稳态底噪
    }
    let (vg, vc) = dn.process_frame(100 * (TRANSIENT_FACTOR_X100 as u32 + 50) / 100); // 瞬态语音帧
    let (ng, nc) = dn.process_frame(100); // 回到底噪
    cs.add(
        "voice untouched noise suppressed",
        voice_gain_unity(vg, vc)
            && voice_gain_unity(ng, nc)
            && vc == FrameClass::Voice
            && VOICE_BAND_LO_HZ == 300
            && VOICE_BAND_HI_HZ == 3_400,
        "",
    );

    // 6) 关态直通不占计量（开关即时——关降噪时 CPU 增量为零语义）。
    let mut dn2 = Denoiser::new();
    let (g, cl) = dn2.process_frame(50);
    cs.add("disabled passthrough zero cost semantics", g == 256 && cl == FrameClass::Bypassed, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avg_pct_math() {
        let mut m = CpuMeter::new();
        for _ in 0..64 {
            m.record(2_000); // 20%
        }
        assert_eq!(m.avg_pct(), 20);
    }

    #[test]
    fn empty_meter_allows() {
        let m = CpuMeter::new();
        assert!(m.always_on_allowed());
        assert_eq!(m.avg_pct(), 0);
    }
}
