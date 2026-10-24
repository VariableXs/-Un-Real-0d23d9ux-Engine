//! VE-F4344 · 一致性仿真器（U08 测试与资产组）
//! 职责：一致性仿真器——仿真三景（多设备分档仿真 / 降级梯度仿真 / CGPU
//! 卸载仿真）+ 仿真保真（与实机对拍校准——偏差超阈停用仿真结论）+ 使用
//! 场景（预算验证 / 降级演练 / 无实机环境开发）+ 仿真通过实机失败以实机为
//! 准立案 + 资源超限降档仿真 + 读屏环境仿真（域本色）。
//! 段位：0x3CE0（续表 idx578）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3CE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmCode {
    /// 保真失偏（停用仿真结论）。
    Unfaithful = SEG_BASE,
    /// 仿真通过实机失败（以实机为准立案）。
    EmuPassRealFail = SEG_BASE + 0x01,
    /// 资源超限（降档仿真）。
    ResourceOver = SEG_BASE + 0x02,
    /// 景号越界。
    SceneRange = SEG_BASE + 0x03,
}

/// 仿真三景。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EmuScene {
    /// 多设备分档仿真。
    DeviceTier,
    /// 降级梯度仿真。
    DegradeLadder,
    /// CGPU 卸载仿真。
    CgpuOffload,
}

/// 对拍样本：(仿真结果, 实机结果)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibPair {
    pub scene: EmuScene,
    pub emu: u32,
    pub real: u32,
}

/// 仿真器台。
#[derive(Default)]
pub struct Simulator {
    pub calib: Vec<CalibPair>,
    /// 偏差容忍（bp）。
    pub tol_bp: u32,
    /// 停用开关（保真失偏即停）。
    pub disabled: bool,
    pub real_wins: u32,
    pub downgrades: u32,
}

impl Simulator {
    pub fn new(tol_bp: u32) -> Self {
        Simulator { tol_bp: tol_bp.max(1), ..Default::default() }
    }

    /// 对拍采样入册。
    pub fn feed(&mut self, scene: EmuScene, emu: u32, real: u32) {
        self.calib.push(CalibPair { scene, emu, real });
    }

    /// 保真核验：同景样本偏差率（|emu−real|/real）均值 ≤ 容忍；超阈停用。
    pub fn fidelity(&mut self, scene: EmuScene) -> Result<u32, SmCode> {
        if self.disabled {
            return Err(SmCode::Unfaithful);
        }
        let pairs: Vec<&CalibPair> = self.calib.iter().filter(|p| p.scene == scene).collect();
        if pairs.is_empty() {
            return Err(SmCode::SceneRange);
        }
        let mut sum_bp: u64 = 0;
        for p in pairs.iter() {
            let bp = if p.real == 0 {
                if p.emu == 0 { 0 } else { u32::MAX as u64 / 2 }
            } else {
                (p.emu.abs_diff(p.real) as u64) * 10000 / p.real as u64
            };
            sum_bp += bp;
        }
        let avg = (sum_bp / pairs.len() as u64) as u32;
        if avg > self.tol_bp {
            self.disabled = true;
            return Err(SmCode::Unfaithful);
        }
        Ok(avg)
    }

    /// 分歧立案：仿真通过实机失败 → 以实机为准立案（计数）。
    pub fn divergence(&mut self, emu_pass: bool, real_pass: bool) -> Result<(), SmCode> {
        if self.disabled {
            return Err(SmCode::Unfaithful);
        }
        if emu_pass && !real_pass {
            self.real_wins += 1;
            return Err(SmCode::EmuPassRealFail);
        }
        Ok(())
    }

    /// 资源超限降档仿真：请求资源超限 → 自动降档跑（计数留痕）。
    pub fn resource(&mut self, need_mb: u32, have_mb: u32) -> Result<u32, SmCode> {
        if have_mb == 0 {
            return Err(SmCode::ResourceOver);
        }
        if need_mb > have_mb {
            self.downgrades += 1;
            let scaled = need_mb * (have_mb / 2) / have_mb;
            return Ok(scaled.max(1));
        }
        Ok(need_mb)
    }

    /// 读屏环境仿真在册断言（域本色）。
    pub fn sr_scene_ready(&self, sr_present: bool) -> bool {
        !self.disabled && sr_present
    }

    /// 恢复启用（保真重校后）。
    pub fn reenable(&mut self) {
        self.disabled = false;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves578_checks() -> CheckSet {
    let mut set = CheckSet::new("ves578");
    let mut sm = Simulator::new(500);

    // C578-01 三景对拍采样入册。
    sm.feed(EmuScene::DeviceTier, 100, 102);
    sm.feed(EmuScene::DeviceTier, 200, 198);
    sm.feed(EmuScene::DegradeLadder, 50, 52);
    sm.feed(EmuScene::CgpuOffload, 80, 90);
    chk(&mut set, "C578-01", sm.calib.len() == 4, "三景对拍入册");

    // C578-02 保真核验通过（偏差率均值 148bp = (196+101)/2 ≤ 500）。
    let f1 = sm.fidelity(EmuScene::DeviceTier);
    chk(&mut set, "C578-02", f1 == Ok(148), "保真核验过");

    // C578-03 无样本景拒绝（不臆测）。
    let mut sm0 = Simulator::new(500);
    chk(&mut set, "C578-03", sm0.fidelity(EmuScene::CgpuOffload) == Err(SmCode::SceneRange), "无样本不臆测");

    // C578-04 偏差超阈停用仿真结论。
    let mut sm2 = Simulator::new(100);
    sm2.feed(EmuScene::DegradeLadder, 200, 100);
    chk(&mut set, "C578-04", sm2.fidelity(EmuScene::DegradeLadder) == Err(SmCode::Unfaithful) && sm2.disabled, "超阈停用");

    // C578-05 停用后一切仿真拒绝。
    chk(&mut set, "C578-05", sm2.divergence(true, false) == Err(SmCode::Unfaithful), "停用全拒");

    // C578-06 保真重校恢复启用。
    sm2.reenable();
    chk(&mut set, "C578-06", !sm2.disabled, "重校恢复");

    // C578-07 分歧立案：仿真过实机挂（以实机为准）。
    chk(&mut set, "C578-07", sm.divergence(true, false) == Err(SmCode::EmuPassRealFail) && sm.real_wins == 1, "实机为准立案");

    // C578-08 一致通过/一致失败不立案。
    chk(&mut set, "C578-08", sm.divergence(true, true).is_ok() && sm.divergence(false, false).is_ok()
        && sm.real_wins == 1, "一致不立案");

    // C578-09 资源超限降档仿真。
    let scaled = sm.resource(1000, 400);
    chk(&mut set, "C578-09", scaled == Ok(500) && sm.downgrades == 1, "超限降档 500");

    // C578-10 预算内不降档。
    chk(&mut set, "C578-10", sm.resource(300, 400) == Ok(300) && sm.downgrades == 1, "预算内不降");

    // C578-11 零资源拒绝。
    chk(&mut set, "C578-11", sm.resource(1, 0).is_err(), "零资源拒");

    // C578-12 读屏环境仿真断言（域本色）。
    chk(&mut set, "C578-12", sm.sr_scene_ready(true) && !sm.sr_scene_ready(false) && sm2.sr_scene_ready(true), "读屏仿真断言");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fidelity_gate() {
        let mut sm = Simulator::new(500);
        sm.feed(EmuScene::DeviceTier, 105, 100);
        assert!(sm.fidelity(EmuScene::DeviceTier).is_ok());
        sm.feed(EmuScene::DeviceTier, 300, 100); // 均值被拉高
        assert_eq!(sm.fidelity(EmuScene::DeviceTier), Err(SmCode::Unfaithful));
        assert!(sm.disabled);
    }

    #[test]
    fn real_wins_on_divergence() {
        let mut sm = Simulator::new(5000);
        sm.feed(EmuScene::CgpuOffload, 100, 101);
        assert_eq!(sm.divergence(true, false), Err(SmCode::EmuPassRealFail));
        assert_eq!(sm.real_wins, 1);
        assert!(sm.divergence(false, true).is_ok()); // 仿真挂实机过不算此案（保守不立案）
    }
}
