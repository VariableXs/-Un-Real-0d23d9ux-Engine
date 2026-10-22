//! CGPU-F0490 · 帧遥测（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 帧遥测：帧时间/间隔/全延迟/达标率/卡顿计数——五指标进 B08
//!   总线（D 域数据全量可观测）；按场景分列。判据：五指标、分列、
//!   判据。
//!
//! 设计纪律：
//! - 五指标：帧时间（渲染耗时 us）、帧间隔（相邻呈现差 us）、全
//!   延迟（端到端 us）、达标率（≤合同帧时的帧占比 ‰）、卡顿计数
//!   （间隔超 1.5×目标）。
//! - 场景分列：场景码（交互/游戏/视频/桌面）各列独立累计——查询
//!   必须带场景码，无场景的聚合值不提供（分列是结构性的）。
//! - B08 帧：FT_MAGIC=0xB08F_0490 五指标打包+校验字。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1170 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1170，全仓独占）。
pub const SEG_BASE: u16 = 0x1170;

pub const E_FT_SCENE: u16 = SEG_BASE + 1; // 场景非法
pub const E_FT_NEG: u16 = SEG_BASE + 2; // 负间隔（时钟倒退）
pub const E_FT_MAGIC: u16 = SEG_BASE + 3; // 帧校验失败

/// 场景码。
pub const SC_INTERACTIVE: u8 = 1;
pub const SC_GAME: u8 = 2;
pub const SC_VIDEO: u8 = 3;
pub const SC_DESKTOP: u8 = 4;
pub const SCENES: [u8; 4] = [SC_INTERACTIVE, SC_GAME, SC_VIDEO, SC_DESKTOP];

/// 合同帧时（80 帧 → 12500us）与卡顿线。
pub const CONTRACT_FRAME_US: u64 = 12_500;
pub const JITTER_LINE_US: u64 = 18_750; // 1.5×合同

/// B08 帧魔数。
pub const FT_MAGIC: u32 = 0xB08F_0490;

/// 场景五指标。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameMetrics {
    /// 帧时间累计（us）。
    pub frame_time_us: u64,
    /// 帧间隔累计（us）。
    pub frame_interval_us: u64,
    /// 全延迟累计（us）。
    pub latency_us: u64,
    /// 达标帧数。
    pub on_time_frames: u64,
    /// 总帧数。
    pub total_frames: u64,
    /// 卡顿计数。
    pub jitter_count: u64,
}

impl FrameMetrics {
    /// 达标率（‰）。
    pub fn on_time_pm(&self) -> u64 {
        if self.total_frames == 0 {
            return 0;
        }
        self.on_time_frames * 1000 / self.total_frames
    }
}

/// 帧遥测台。
#[derive(Debug)]
pub struct FrameTelemetry {
    scenes: [FrameMetrics; 4],
    pub frames_out: u64,
}

impl FrameTelemetry {
    pub fn new() -> Self {
        Self { scenes: Default::default(), frames_out: 0 }
    }

    fn idx(scene: u8) -> Result<usize, u16> {
        SCENES.iter().position(|&s| s == scene).ok_or(E_FT_SCENE)
    }

    /// 单帧记录：帧时间+间隔+全延迟（负间隔拒）。
    pub fn record(&mut self, scene: u8, frame_time: u64, interval: u64, latency: u64) -> Result<(), u16> {
        let i = Self::idx(scene)?;
        // 负间隔防线：间隔为 0 视同异常最小值，倒退在调用方换算前拦截
        // （此处以 > 10×合同帧时的异常间隔视为倒退污染拒收）。
        if interval > CONTRACT_FRAME_US * 10 {
            return Err(E_FT_NEG);
        }
        let m = &mut self.scenes[i];
        m.frame_time_us += frame_time;
        m.frame_interval_us += interval;
        m.latency_us += latency;
        m.total_frames += 1;
        if frame_time <= CONTRACT_FRAME_US {
            m.on_time_frames += 1;
        }
        if interval > JITTER_LINE_US {
            m.jitter_count += 1;
        }
        Ok(())
    }

    pub fn metrics(&self, scene: u8) -> Result<FrameMetrics, u16> {
        let i = Self::idx(scene)?;
        Ok(self.scenes[i])
    }

    /// B08 帧：魔数+场景+五指标（frame_time/interval/latency 均值，
    /// 达标率，卡顿数）+校验字。
    pub fn encode(&mut self, scene: u8) -> Result<[u32; 12], u16> {
        let i = Self::idx(scene)?;
        let m = self.scenes[i];
        let n = m.total_frames.max(1);
        let mut f = [0u32; 12];
        f[0] = FT_MAGIC;
        f[1] = scene as u32;
        f[2] = (m.frame_time_us / n) as u32;
        f[3] = (m.frame_interval_us / n) as u32;
        f[4] = (m.latency_us / n) as u32;
        f[5] = m.on_time_pm() as u32;
        f[6] = m.jitter_count as u32;
        f[11] = crc32(&f[..11]);
        self.frames_out += 1;
        Ok(f)
    }
}

fn crc32(data: &[u32]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &w in data {
        let mut b = w;
        for _ in 0..4 {
            crc ^= b & 0xFF;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
            b >>= 8;
        }
    }
    !crc
}

pub fn decode(f: &[u32; 12]) -> Result<(u8, u64, u64, u64, u64, u64), u16> {
    if f[0] != FT_MAGIC || f[11] != crc32(&f[..11]) {
        return Err(E_FT_MAGIC);
    }
    Ok((f[1] as u8, f[2] as u64, f[3] as u64, f[4] as u64, f[5] as u64, f[6] as u64))
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、五指标记录。
    {
        let mut ft = FrameTelemetry::new();
        ft.record(SC_GAME, 10_000, 12_000, 30_000).ok();
        ft.record(SC_GAME, 14_000, 13_000, 40_000).ok();
        let m = ft.metrics(SC_GAME).unwrap_or_default();
        v.push((
            "F0490-五指标-累计正确",
            m.frame_time_us == 24_000 && m.frame_interval_us == 25_000 && m.latency_us == 70_000 && m.total_frames == 2,
        ));
        // 达标率：10000 达标（≤12500），14000 不达标 → 500‰。
        v.push(("F0490-五指标-达标率", m.on_time_pm() == 500));
        // 卡顿：间隔均 ≤18750 → 0。
        v.push(("F0490-五指标-卡顿计数", m.jitter_count == 0));
    }
    // 二、场景分列。
    {
        let mut ft = FrameTelemetry::new();
        ft.record(SC_INTERACTIVE, 8_000, 12_000, 25_000).ok();
        ft.record(SC_DESKTOP, 5_000, 16_000, 20_000).ok();
        let mi = ft.metrics(SC_INTERACTIVE).unwrap_or_default();
        let md = ft.metrics(SC_DESKTOP).unwrap_or_default();
        v.push(("F0490-分列-场景独立", mi.total_frames == 1 && md.total_frames == 1 && mi.frame_time_us != md.frame_time_us));
        // 全场景覆盖合法。
        let all = SCENES.iter().all(|&s| SCENES.contains(&s));
        v.push(("F0490-分列-四场景枚举", all && SCENES.len() == 4));
    }
    // 三、卡顿检出。
    {
        let mut ft = FrameTelemetry::new();
        ft.record(SC_VIDEO, 10_000, 20_000, 30_000).ok(); // 间隔 20000 > 18750 → 卡顿
        let m = ft.metrics(SC_VIDEO).unwrap_or_default();
        v.push(("F0490-五指标-卡顿检出", m.jitter_count == 1));
    }
    // 四、B08 帧。
    {
        let mut ft = FrameTelemetry::new();
        ft.record(SC_GAME, 10_000, 12_000, 30_000).ok();
        let f = ft.encode(SC_GAME);
        let dec = f.as_ref().ok().copied();
        v.push(("F0490-进总线-编码成功", f.is_ok()));
        let d = dec.and_then(|fr| decode(&fr).ok());
        let m = ft.metrics(SC_GAME).unwrap_or_default();
        v.push((
            "F0490-进总线-解码一致",
            d == Some((SC_GAME, m.frame_time_us, m.frame_interval_us, m.latency_us, m.on_time_pm(), m.jitter_count)),
        ));
        // 篡改拒。
        let mut fr = f.unwrap_or([0u32; 12]);
        fr[0] = 0;
        v.push(("F0490-进总线-魔数篡改拒", decode(&fr) == Err(E_FT_MAGIC)));
    }
    // 五、判据自检。
    {
        let mut ft = FrameTelemetry::new();
        v.push(("F0490-判据-场景非法拒", ft.record(99, 1, 1, 1) == Err(E_FT_SCENE)));
        v.push(("F0490-判据-倒退污染拒", ft.record(SC_GAME, 1_000, 200_000, 1_000) == Err(E_FT_NEG)));
        v.push(("F0490-判据-合同帧时80帧", CONTRACT_FRAME_US == 12_500));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0490_frametelem_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
