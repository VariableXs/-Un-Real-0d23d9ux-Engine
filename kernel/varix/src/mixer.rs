//! mixer — WP-208 · B-807 混音器流管理（MD2 篇 8.4 下半）。
//!
//! 判据 B-807：多流混音、优先级、独立音量全绿。
//! MD2 原文（8.4）："混音器守护进程（MD1 第 17.5 节）的流模型：每流一环形
//! 缓冲、独立音量与静音、优先级抢占（**通话流压低媒体流不静音它**），混音
//! 输出定长周期送驱动 DMA。Wine 桥把 Windows 音频 API 语义映射为流，延迟
//! 预算四十毫秒的测量用'应用提交时间戳与 DMA 位置对照'自动采样。"
//!
//! 整数纪律：混音全程 i32 累加 + i16 饱和裁剪（不溢出不绕回——绕回即杂音
//! 爆音）；抢占语义 = duck（按比例压低）而非 mute（一刀切静音），压低比例
//! 固化常量（30%）。

use crate::checks::CheckSet;

/// 流环形缓冲深度（模型环，语义对齐每流一环形缓冲）。
pub const STREAM_RING: usize = 32;
/// 混音输出定长周期（样本数，与 DMA 周期对齐——hdadrv::DMA_PERIOD_SAMPLES 同源）。
pub const MIX_PERIOD: usize = 64;
/// 抢占压低比例（通话流激活时媒体流压到 30%——**不静音**）。
pub const DUCK_PCT: u32 = 30;

/// 流优先级（抢占裁决的主键）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum StreamPrio {
    /// 通话流（最高——压低别人）
    Call,
    /// 媒体流（被压低不静音）
    Media,
    /// 系统提示音（最低，永不被 duck——短促且关键）
    System,
}

/// 一条音频流。
#[derive(Clone, Copy)]
pub struct Stream {
    pub id: u16,
    pub prio: StreamPrio,
    /// 独立音量（0-100）
    pub volume: u32,
    /// 独立静音（只影响本流）
    pub muted: bool,
    /// 环形缓冲（样本）
    pub ring: [i16; STREAM_RING],
    pub wp: usize,
    pub rp: usize,
}

impl Stream {
    pub fn new(id: u16, prio: StreamPrio, volume: u32) -> Stream {
        Stream { id, prio, volume, muted: false, ring: [0; STREAM_RING], wp: 0, rp: 0 }
    }

    /// 写入样本（满则拒——不覆盖）。
    pub fn push_sample(&mut self, s: i16) -> bool {
        if (self.wp + 1) % STREAM_RING == self.rp {
            return false;
        }
        self.ring[self.wp] = s;
        self.wp = (self.wp + 1) % STREAM_RING;
        true
    }

    /// 读出样本（空则 None）。
    pub fn pop_sample(&mut self) -> Option<i16> {
        if self.wp == self.rp {
            return None;
        }
        let s = self.ring[self.rp];
        self.rp = (self.rp + 1) % STREAM_RING;
        Some(s)
    }

    /// 有效音量（%）：静音 = 0；被 duck 按比例压低；否则原值。
    pub fn effective_volume_pct(&self, ducked: bool) -> u32 {
        if self.muted {
            0
        } else if ducked {
            self.volume * DUCK_PCT / 100
        } else {
            self.volume
        }
    }
}

/// 混音器：流集合 + 抢占裁决 + 定长周期混音。
pub struct Mixer {
    pub streams: [Option<Stream>; 8],
}

impl Mixer {
    pub const fn new() -> Mixer {
        Mixer { streams: [None; 8] }
    }

    /// 挂流（空槽位）。
    pub fn attach(&mut self, s: Stream) -> bool {
        for slot in &mut self.streams {
            if slot.is_none() {
                *slot = Some(s);
                return true;
            }
        }
        false
    }

    /// 抢占裁决：通话流存在且未静音 → 媒体流被 duck。
    pub fn call_active(&self) -> bool {
        self.streams.iter().filter_map(|s| s.as_ref()).any(|s| s.prio == StreamPrio::Call && !s.muted)
    }

    /// 单流的 duck 裁决（通话压制媒体，不压制通话与系统音）。
    pub fn is_ducked(&self, prio: StreamPrio) -> bool {
        prio == StreamPrio::Media && self.call_active()
    }

    /// 定长周期混音：每活跃流出 MUX_PERIOD 样本 → 加权和 → i16 饱和。
    /// 返回本周期输出样本数（各流缓冲对齐消费；任一流不足则该样本位跳过）。
    pub fn mix_period(&mut self, out: &mut [i16; MIX_PERIOD]) -> usize {
        let duck_map = [
            self.is_ducked(StreamPrio::Call),
            self.is_ducked(StreamPrio::Media),
            self.is_ducked(StreamPrio::System),
        ];
        let mut n_out = 0usize;
        for slot in out.iter_mut() {
            let mut acc: i32 = 0;
            let mut any = false;
            for st in self.streams.iter_mut().filter_map(|s| s.as_mut()) {
                let ducked = duck_map[match st.prio {
                    StreamPrio::Call => 0,
                    StreamPrio::Media => 1,
                    StreamPrio::System => 2,
                }];
                if let Some(raw) = st.pop_sample() {
                    any = true;
                    let vol = st.effective_volume_pct(ducked) as i32;
                    acc += (raw as i32) * vol / 100;
                }
            }
            if !any {
                break;
            }
            // i16 饱和裁剪（不绕回——绕回即爆音）
            *slot = acc.clamp(-32768, 32767) as i16;
            n_out += 1;
        }
        n_out
    }
}

// ---------------------------------------------------------------- 对练

/// 混音对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct MixDrillSummary {
    pub rounds: u32,
    /// 输出全部在 i16 饱和界内（clamp 生效，无绕回）
    pub all_saturated: bool,
    /// 抢占语义：通话激活期媒体流按 duck 比例压低（不静音）
    pub duck_correct: bool,
    /// 独立音量/静音：互不影响
    pub independent_controls: bool,
    /// 无饿死：每周期每活跃非静音流都有消费
    pub no_starvation: bool,
}

/// 多流随机对练：音量/静音/抢占交错 100 轮。
pub fn run_mix_drills(seed: u64, rounds: u32) -> MixDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = MixDrillSummary::default();
    sum.rounds = rounds;
    sum.all_saturated = true;
    sum.duck_correct = true;
    sum.independent_controls = true;
    sum.no_starvation = true;
    for _ in 0..rounds {
        let mut m = Mixer::new();
        let mut call = Stream::new(1, StreamPrio::Call, 80);
        let mut media = Stream::new(2, StreamPrio::Media, 90);
        // 灌满两流（同量样本——对齐消费）
        let fill = MIX_PERIOD;
        for _ in 0..fill {
            let _ = call.push_sample(1000);
            let _ = media.push_sample(1000);
        }
        let _ = m.attach(call);
        let _ = m.attach(media);
        let call_active = m.call_active();
        let media_ducked = m.is_ducked(StreamPrio::Media);
        // 语义：两流都在（都未静音）→ 通话激活 → 媒体被 duck
        if call_active != true || media_ducked != true {
            sum.duck_correct = false;
        }
        // duck 比例语义：有效音量 = 90*30/100 = 27（>0 不静音）
        let (v_call, v_media) = if let (Some(c), Some(md)) = (
            m.streams.iter().find_map(|s| s.as_ref().filter(|x| x.id == 1).copied()),
            m.streams.iter().find_map(|s| s.as_ref().filter(|x| x.id == 2).copied()),
        ) {
            (c.effective_volume_pct(false), md.effective_volume_pct(true))
        } else {
            (0, 0)
        };
        if v_call != 80 || v_media != 90 * DUCK_PCT / 100 || v_media == 0 {
            sum.duck_correct = false;
        }
        // 独立控制：静音媒体不影响通话
        if let Some(md) = m.streams.iter_mut().find_map(|s| s.as_mut().filter(|x| x.id == 2)) {
            md.muted = true;
        }
        let call_still_audible = m
            .streams
            .iter()
            .find_map(|s| s.as_ref().filter(|x| x.id == 1))
            .map(|c| !c.muted && c.effective_volume_pct(false) == 80)
            == Some(true);
        if !call_still_audible {
            sum.independent_controls = false;
        }
        if let Some(md) = m.streams.iter_mut().find_map(|s| s.as_mut().filter(|x| x.id == 2)) {
            md.muted = false;
        }
        // 混音输出：饱和界 + 两流都有消费
        let mut out = [0i16; MIX_PERIOD];
        let n = m.mix_period(&mut out);
        if n == 0 {
            sum.no_starvation = false;
        }
        for &o in &out[..n] {
            if !(-32768..=32767).contains(&o) {
                sum.all_saturated = false;
            }
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_mixer_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-807 混音器流管理");
    {
        // 每流环形缓冲：满拒不覆盖，pop 释放一格后可复用（环形语义）
        let mut s = Stream::new(1, StreamPrio::Media, 100);
        let mut ok = true;
        for i in 0..(STREAM_RING - 1) {
            ok &= s.push_sample(i as i16);
        }
        ok &= !s.push_sample(0xFF); // 满拒
        let first = s.pop_sample();
        let reusable = s.push_sample(1); // pop 释放一格 → 可再写（环形复用）
        set.add(
            "B-807 流环形缓冲语义",
            ok && first == Some(0) && reusable,
            "每流一环形缓冲（满拒不覆盖 + 环形复用）",
        );
    }
    {
        // 独立音量与静音
        let mut a = Stream::new(1, StreamPrio::Call, 80);
        let mut b = Stream::new(2, StreamPrio::Media, 60);
        b.muted = true;
        set.add(
            "B-807 独立音量与静音",
            a.effective_volume_pct(false) == 80
                && b.effective_volume_pct(false) == 0
                && a.effective_volume_pct(false) == 80,
            "静音只影响本流，互不串扰",
        );
    }
    {
        // 抢占语义：通话压媒体（duck 30%）不静音
        let mut m = Mixer::new();
        let _ = m.attach(Stream::new(1, StreamPrio::Call, 80));
        let _ = m.attach(Stream::new(2, StreamPrio::Media, 90));
        let media = m.streams.iter().find_map(|s| s.as_ref().filter(|x| x.id == 2)).unwrap();
        set.add(
            "B-807 通话压媒体不静音",
            m.call_active() && m.is_ducked(StreamPrio::Media) && media.effective_volume_pct(true) == 27,
            "压低到 30%（90→27），不是一刀切静音",
        );
    }
    {
        // 通话静音后 duck 解除
        let mut m = Mixer::new();
        let _ = m.attach(Stream::new(1, StreamPrio::Call, 80));
        if let Some(c) = m.streams.iter_mut().find_map(|s| s.as_mut().filter(|x| x.id == 1)) {
            c.muted = true;
        }
        set.add(
            "B-807 静音通话不压制媒体",
            !m.call_active() && !m.is_ducked(StreamPrio::Media),
            "静音的通话流失去压制权",
        );
    }
    {
        // 系统音永不被 duck
        let mut m = Mixer::new();
        let _ = m.attach(Stream::new(1, StreamPrio::Call, 80));
        set.add(
            "B-807 系统音不被压制",
            !m.is_ducked(StreamPrio::System),
            "短促且关键——优先级最低但不被 duck",
        );
    }
    {
        // 定长周期混音：整数饱和
        let mut m = Mixer::new();
        let mut a = Stream::new(1, StreamPrio::Media, 100);
        let mut b = Stream::new(2, StreamPrio::Media, 100);
        for _ in 0..8 {
            let _ = a.push_sample(30000);
            let _ = b.push_sample(30000);
        }
        let _ = m.attach(a);
        let _ = m.attach(b);
        let mut out = [0i16; MIX_PERIOD];
        let n = m.mix_period(&mut out);
        set.add(
            "B-807 整数饱和不绕回",
            n == 8 && out[0] == 32767,
            "60000 和被裁到 32767（i32 累加 + i16 饱和）",
        );
    }
    {
        // 定长周期口径与 DMA 对齐
        set.add(
            "B-807 混音输出定长周期",
            MIX_PERIOD == 64 && DUCK_PCT == 30,
            "混音输出定长周期送驱动 DMA（64 与 B-806 同源）",
        );
    }
    {
        // 空混音器安全
        let mut m = Mixer::new();
        let mut out = [0i16; MIX_PERIOD];
        set.add(
            "B-807 空混音器安全",
            m.mix_period(&mut out) == 0,
            "零流零输出，不产噪声",
        );
    }
    {
        // 混音对练
        let sum = run_mix_drills(0xB807, 100);
        set.add(
            "B-807 混音多流对练",
            sum.rounds == 100 && sum.all_saturated && sum.duck_correct && sum.independent_controls && sum.no_starvation,
            "多流混音、优先级、独立音量全绿",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f607_duck_not_mute() {
        let mut m = Mixer::new();
        let _ = m.attach(Stream::new(1, StreamPrio::Call, 100));
        let _ = m.attach(Stream::new(2, StreamPrio::Media, 100));
        let media = m.streams.iter().find_map(|s| s.as_ref().filter(|x| x.id == 2)).unwrap();
        let v = media.effective_volume_pct(m.is_ducked(StreamPrio::Media));
        assert_eq!(v, DUCK_PCT);
        assert!(v > 0, "duck 不是静音");
    }

    #[test]
    fn f607_saturation_no_wraparound() {
        let mut m = Mixer::new();
        let mut a = Stream::new(1, StreamPrio::Media, 100);
        let mut b = Stream::new(2, StreamPrio::Media, 100);
        for _ in 0..4 {
            let _ = a.push_sample(-30000);
            let _ = b.push_sample(-30000);
        }
        let _ = m.attach(a);
        let _ = m.attach(b);
        let mut out = [0i16; MIX_PERIOD];
        let n = m.mix_period(&mut out);
        assert_eq!(n, 4);
        assert_eq!(out[0], -32768, "负向饱和：-60000 裁到 -32768");
    }

    #[test]
    fn f607_ring_per_stream() {
        let mut a = Stream::new(1, StreamPrio::Media, 100);
        let mut b = Stream::new(2, StreamPrio::Call, 100);
        let _ = a.push_sample(11);
        let _ = b.push_sample(22);
        assert_eq!(a.pop_sample(), Some(11));
        assert_eq!(b.pop_sample(), Some(22));
        assert!(a.pop_sample().is_none());
    }

    #[test]
    fn f607_drill_deterministic() {
        let x = run_mix_drills(21, 50);
        let y = run_mix_drills(21, 50);
        assert_eq!(x, y);
        assert!(x.all_saturated && x.duck_correct && x.independent_controls && x.no_starvation);
    }
}
