//! CGPU-F0460 · 边界场景遥测（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 遥测：各边界场景触发频率/持续时间/恢复耗时/应急弃车清单实际
//!   执行统计——四指标进 B08 总线。判据：四指标、进总线、判据。
//!
//! 设计纪律：
//! - 四指标结构化（触发次数/持续 tick/恢复 tick/弃车执行数），每
//!   场景独立累计，只增不灭（运行期防线）。
//! - B08 同源总线帧：魔数+序号+场景+四指标+校验字——与既有 B08
//!   帧族同构（cgb445 的 AT 族扩展，新魔数独占段）。
//! - 只增防线：四指标计数器不得回退（除显性 reset 通道）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0FF0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0FF0，全仓独占）。
pub const SEG_BASE: u16 = 0x0FF0;

pub const E_ET_SCENE: u16 = SEG_BASE + 1; // 场景非法
pub const E_ET_ROLLBACK: u16 = SEG_BASE + 2; // 指标回退（只增破缺）
pub const E_ET_MAGIC: u16 = SEG_BASE + 3; // 帧魔数错

/// 五场景（与 cgb459 演练场景同表）。
pub const S_LOWRES: u8 = 1;
pub const S_EMERGENCY: u8 = 2;
pub const S_COLDBOOT: u8 = 3;
pub const S_OFFLINE: u8 = 4;
pub const S_THERMAL: u8 = 5;
pub const SCENES: [u8; 5] = [S_LOWRES, S_EMERGENCY, S_COLDBOOT, S_OFFLINE, S_THERMAL];

/// B08 族帧魔数（边界遥测扩展，独占）。
pub const ET_MAGIC: u32 = 0xB08F_0FF0;

/// 四指标条目（每场景）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneMetrics {
    /// 触发次数。
    pub triggers: u64,
    /// 累计持续（tick）。
    pub duration_ticks: u64,
    /// 累计恢复耗时（tick）。
    pub recover_ticks: u64,
    /// 应急弃车清单实际执行数。
    pub sacrificed_ops: u64,
}

impl SceneMetrics {
    pub fn record_trigger(&mut self, duration: u64, recover: u64, sacrificed: u64) {
        self.triggers += 1;
        self.duration_ticks += duration;
        self.recover_ticks += recover;
        self.sacrificed_ops += sacrificed;
    }
}

/// 边界遥测台。
#[derive(Debug)]
pub struct EdgeTelemetry {
    scenes: [SceneMetrics; 5],
    /// 帧序号。
    seq: u32,
    pub frames_out: u64,
}

impl EdgeTelemetry {
    pub fn new() -> Self {
        Self { scenes: Default::default(), seq: 0, frames_out: 0 }
    }

    fn idx(scene: u8) -> Result<usize, u16> {
        SCENES.iter().position(|&s| s == scene).ok_or(E_ET_SCENE)
    }

    /// 触发记录：四指标只增。
    pub fn record(&mut self, scene: u8, duration: u64, recover: u64, sacrificed: u64) -> Result<(), u16> {
        let i = Self::idx(scene)?;
        self.scenes[i].record_trigger(duration, recover, sacrificed);
        Ok(())
    }

    /// 查询场景四指标。
    pub fn metrics(&self, scene: u8) -> Result<SceneMetrics, u16> {
        let i = Self::idx(scene)?;
        Ok(self.scenes[i])
    }

    /// B08 帧编码：魔数+序号+场景+四指标（8×u32 打包）+CRC 校验字。
    pub fn encode_frame(&mut self, scene: u8) -> Result<[u32; 12], u16> {
        let i = Self::idx(scene)?;
        let m = self.scenes[i];
        self.seq = self.seq.wrapping_add(1);
        let mut f = [0u32; 12];
        f[0] = ET_MAGIC;
        f[1] = self.seq;
        f[2] = scene as u32;
        f[3] = m.triggers as u32;
        f[4] = (m.triggers >> 32) as u32;
        f[5] = m.duration_ticks as u32;
        f[6] = (m.duration_ticks >> 32) as u32;
        f[7] = m.recover_ticks as u32;
        f[8] = (m.recover_ticks >> 32) as u32;
        f[9] = m.sacrificed_ops as u32;
        f[10] = (m.sacrificed_ops >> 32) as u32;
        f[11] = crc32(&f[..11]);
        self.frames_out += 1;
        Ok(f)
    }
}

/// 帧解码校验：魔数+CRC。
pub fn decode_frame(f: &[u32; 12]) -> Result<(u8, u64, u64, u64, u64), u16> {
    if f[0] != ET_MAGIC {
        return Err(E_ET_MAGIC);
    }
    if f[11] != crc32(&f[..11]) {
        return Err(E_ET_MAGIC);
    }
    let scene = f[2] as u8;
    let triggers = (f[4] as u64) << 32 | f[3] as u64;
    let duration = (f[6] as u64) << 32 | f[5] as u64;
    let recover = (f[8] as u64) << 32 | f[7] as u64;
    let sacrificed = (f[10] as u64) << 32 | f[9] as u64;
    Ok((scene, triggers, duration, recover, sacrificed))
}

fn crc32(data: &[u32]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &w in data {
        let mut b = w;
        for _ in 0..4 {
            let byte = (b & 0xFF) as u32;
            crc ^= byte;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
            b >>= 8;
        }
    }
    !crc
}

/// 只增防线：新指标任一字段 < 旧指标即判回退。
pub fn no_rollback(old: &SceneMetrics, new: &SceneMetrics) -> Result<(), u16> {
    if new.triggers < old.triggers
        || new.duration_ticks < old.duration_ticks
        || new.recover_ticks < old.recover_ticks
        || new.sacrificed_ops < old.sacrificed_ops
    {
        return Err(E_ET_ROLLBACK);
    }
    Ok(())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、四指标记录与累计。
    {
        let mut et = EdgeTelemetry::new();
        et.record(S_LOWRES, 10, 5, 2).ok();
        et.record(S_LOWRES, 20, 8, 3).ok();
        let m = et.metrics(S_LOWRES).unwrap_or_default();
        v.push((
            "F0460-四指标-累计正确",
            m.triggers == 2 && m.duration_ticks == 30 && m.recover_ticks == 13 && m.sacrificed_ops == 5,
        ));
        // 各场景独立。
        et.record(S_EMERGENCY, 5, 3, 1).ok();
        let m2 = et.metrics(S_EMERGENCY).unwrap_or_default();
        v.push(("F0460-四指标-场景独立", m2.triggers == 1 && et.metrics(S_LOWRES).unwrap_or_default().triggers == 2));
    }
    // 二、进总线：B08 帧编码+解码一致。
    {
        let mut et = EdgeTelemetry::new();
        et.record(S_THERMAL, 40, 12, 6).ok();
        let f = et.encode_frame(S_THERMAL);
        let dec = f.ok().and_then(|fr| decode_frame(&fr).ok());
        v.push((
            "F0460-进总线-编解码一致",
            dec == Some((S_THERMAL, 1, 40, 12, 6)),
        ));
        // 帧序号递增。
        let f1 = et.encode_frame(S_THERMAL);
        let f2 = et.encode_frame(S_EMERGENCY);
        v.push(("F0460-进总线-帧序递增", f1.map(|x| x[1]) == Ok(2) && f2.map(|x| x[1]) == Ok(3)));
    }
    // 三、只增防线。
    {
        let old = SceneMetrics { triggers: 3, duration_ticks: 30, recover_ticks: 15, sacrificed_ops: 4 };
        let new = SceneMetrics { triggers: 4, duration_ticks: 50, recover_ticks: 20, sacrificed_ops: 7 };
        v.push(("F0460-只增防线-正常递增通过", no_rollback(&old, &new).is_ok()));
        let back = SceneMetrics { triggers: 2, duration_ticks: 50, recover_ticks: 20, sacrificed_ops: 7 };
        v.push(("F0460-只增防线-触发数回退拒绝", no_rollback(&old, &back) == Err(E_ET_ROLLBACK)));
        let back2 = SceneMetrics { triggers: 4, duration_ticks: 20, recover_ticks: 20, sacrificed_ops: 7 };
        v.push(("F0460-只增防线-时长回退拒绝", no_rollback(&old, &back2) == Err(E_ET_ROLLBACK)));
    }
    // 四、帧防线：魔数/CRC 篡改必检。
    {
        let mut et = EdgeTelemetry::new();
        et.record(S_OFFLINE, 8, 4, 1).ok();
        let mut f = et.encode_frame(S_OFFLINE).unwrap_or([0u32; 12]);
        v.push(("F0460-帧防线-正常帧解码", decode_frame(&f).is_ok()));
        let magic = f[0];
        f[0] = 0xDEAD_0000;
        v.push(("F0460-帧防线-魔数篡改拒", decode_frame(&f) == Err(E_ET_MAGIC)));
        f[0] = magic;
        let crc = f[11];
        f[11] = crc ^ 1;
        v.push(("F0460-帧防线-CRC篡改拒", decode_frame(&f) == Err(E_ET_MAGIC)));
    }
    // 五、判据自检。
    {
        let mut et = EdgeTelemetry::new();
        v.push(("F0460-判据-场景非法拒绝", et.record(99, 1, 1, 1) == Err(E_ET_SCENE)));
        v.push(("F0460-判据-帧计数", et.frames_out == 0 && { let _ = et.encode_frame(S_COLDBOOT); et.frames_out == 1 }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0460_edgetelem_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
