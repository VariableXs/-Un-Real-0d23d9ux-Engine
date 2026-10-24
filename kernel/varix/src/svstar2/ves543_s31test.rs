//! VE-F4309 · 场景应用测试（U06 场景应用组）
//! 职责：七景应用测试——场景剧本测试（每景端到端剧本 + 乱序操作变体；
//! 变体爆炸配额采样）+ 场景间交叉测试（同一违例跨场景流转不丢账——对账
//! 表核验）+ 剧本失败阻断场景发版 + 账目丢失对账回溯 + 剧本含键盘盲操作
//! 路径（域本色）。
//! 段位：0x3120（续表 idx543）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3120;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 剧本失败（阻断场景发版）。
    ScriptFailed = SEG_BASE,
    /// 交叉账目丢失（对账回溯）。
    LedgerLost = SEG_BASE + 0x01,
    /// 变体爆炸（配额采样兜底）。
    VariantBurst = SEG_BASE + 0x02,
    /// 盲操作路径缺失（无障碍断言——域色）。
    BlindPathMissing = SEG_BASE + 0x03,
}

/// 场景剧本（七景之一）。
#[derive(Clone, Debug)]
pub struct Script {
    pub scene: u8,      // 1..=7
    pub steps: u32,
    /// 键盘盲操作路径步数（无障碍硬要求 >0——域本色）。
    pub blind_steps: u32,
    pub passed: bool,
    /// 乱序变体（采样后）。
    pub variants: u32,
}

/// 交叉对账表：违例账目跨场景流转登记。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XferEntry {
    pub ticket: u32,
    pub from_scene: u8,
    pub to_scene: u8,
    /// 流转是否被对账确认。
    pub confirmed: bool,
}

/// 场景测试台。
#[derive(Default)]
pub struct SceneTest {
    pub scripts: Vec<Script>,
    pub xfers: Vec<XferEntry>,
    pub quota: u32,
    pub blocked_releases: u32,
}

const SCENE_MAX: u8 = 7;
const VARIANT_QUOTA: u32 = 64;

impl SceneTest {
    pub fn new(quota: u32) -> Self {
        SceneTest { quota: quota.max(1), ..Default::default() }
    }

    /// 登记剧本：盲操作路径缺失直接拒（域色硬线）。
    pub fn add_script(&mut self, scene: u8, steps: u32, blind_steps: u32) -> Result<(), ScCode> {
        if scene == 0 || scene > SCENE_MAX {
            return Err(ScCode::ScriptFailed);
        }
        if blind_steps == 0 {
            return Err(ScCode::BlindPathMissing);
        }
        self.scripts.push(Script { scene, steps, blind_steps, passed: false, variants: 0 });
        Ok(())
    }

    /// 乱序变体生成：超出配额即配额采样（爆炸兜底）并显性计数。
    pub fn make_variants(&mut self, scene: u8, raw: u32) -> Result<u32, ScCode> {
        let s = self.scripts.iter_mut().find(|s| s.scene == scene).ok_or(ScCode::ScriptFailed)?;
        if raw > self.quota * 4 {
            s.variants = self.quota;
            return Err(ScCode::VariantBurst);
        }
        s.variants = raw.min(self.quota);
        Ok(s.variants)
    }

    /// 剧本执行：失败即置发版阻断。
    pub fn run_script(&mut self, scene: u8, ok: bool) -> Result<(), ScCode> {
        let s = self.scripts.iter_mut().find(|s| s.scene == scene).ok_or(ScCode::ScriptFailed)?;
        s.passed = ok;
        if !ok {
            self.blocked_releases += 1;
            return Err(ScCode::ScriptFailed);
        }
        Ok(())
    }

    /// 发版闸：七景剧本全过才放行（缺景 = 拒）。
    pub fn release_gate(&self) -> bool {
        if self.scripts.len() < SCENE_MAX as usize {
            return false;
        }
        self.scripts.iter().all(|s| s.passed)
    }

    /// 交叉流转登记（违例账目跨场景）。
    pub fn xfer(&mut self, ticket: u32, from_scene: u8, to_scene: u8) {
        self.xfers.push(XferEntry { ticket, from_scene, to_scene, confirmed: false });
    }

    /// 对账核验：登记过的票必须流转确认；丢失 = 回溯（返回丢失票号表）。
    pub fn reconcile(&mut self, arrived: &[u32]) -> Vec<u32> {
        let mut lost = Vec::new();
        for e in self.xfers.iter_mut() {
            if arrived.contains(&e.ticket) {
                e.confirmed = true;
            } else {
                lost.push(e.ticket);
            }
        }
        lost
    }

    /// 丢失回溯后补账重对（票号补登即确认）。
    pub fn backfill(&mut self, ticket: u32) {
        if let Some(e) = self.xfers.iter_mut().find(|e| e.ticket == ticket) {
            e.confirmed = true;
        }
    }

    /// 全账一致：无未确认流转。
    pub fn ledger_clean(&self) -> bool {
        self.xfers.iter().all(|e| e.confirmed)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves543_checks() -> CheckSet {
    let mut set = CheckSet::new("ves543");
    let mut st = SceneTest::new(VARIANT_QUOTA);

    // C543-01 七景剧本登记（盲操作路径齐备）。
    let mut all_ok = true;
    for s in 1..=7u8 {
        if st.add_script(s, 10 + s as u32, 3).is_err() {
            all_ok = false;
        }
    }
    chk(&mut set, "C543-01", all_ok && st.scripts.len() == 7, "七景剧本齐备");

    // C543-02 盲操作路径缺失拒绝（域色硬线）。
    chk(&mut set, "C543-02", st.add_script(8, 10, 0) == Err(ScCode::BlindPathMissing), "盲路径缺失拒");

    // C543-03 景号越界拒绝（0 与 >7）。
    chk(&mut set, "C543-03", st.add_script(0, 10, 2).is_err() && st.add_script(9, 10, 2).is_err(), "景号越界拒");

    // C543-04 变体配额采样（配额内直取）。
    chk(&mut set, "C543-04", st.make_variants(1, 30) == Ok(30), "变体配额内直取");

    // C543-05 变体爆炸配额采样兜底。
    chk(&mut set, "C543-05", st.make_variants(2, 300) == Err(ScCode::VariantBurst) && st.scripts[1].variants == 64, "爆炸采样兜底");

    // C543-06 剧本失败阻断发版（计数）。
    chk(&mut set, "C543-06", st.run_script(3, false) == Err(ScCode::ScriptFailed) && st.blocked_releases == 1, "失败阻断发版");

    // C543-07 发版闸：未全过不放行。
    chk(&mut set, "C543-07", !st.release_gate(), "未全过不放行");

    // C543-08 复跑转绿后发版闸放行（七景全过）。
    let _ = st.run_script(3, true);
    for s in 1..=7u8 {
        let _ = st.run_script(s, true);
    }
    chk(&mut set, "C543-08", st.release_gate(), "七景全过放行");

    // C543-09 交叉流转登记 + 对账确认。
    st.xfer(9001, 1, 4);
    st.xfer(9002, 4, 7);
    let lost = st.reconcile(&[9001]);
    chk(&mut set, "C543-09", lost == [9002] && st.xfers[0].confirmed, "流转对账确认");

    // C543-10 丢失票回溯补账后全账一致。
    st.backfill(9002);
    chk(&mut set, "C543-10", st.ledger_clean(), "回溯补账全一致");

    // C543-11 对账不误标：未到场票保持未确认。
    st.xfer(9003, 2, 5);
    let lost2 = st.reconcile(&[]);
    chk(&mut set, "C543-11", lost2 == [9003] && !st.ledger_clean(), "未到票不误标");

    // C543-12 空流转表恒洁 + 配额下限防护（0 钳 1）。
    let st2 = SceneTest::new(0);
    let st3 = SceneTest::new(0);
    chk(&mut set, "C543-12", st2.ledger_clean() && st3.quota == 1, "空表恒洁+配额钳制");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blind_path_hardline() {
        let mut st = SceneTest::new(8);
        assert!(st.add_script(1, 10, 0).is_err());
        assert!(st.add_script(1, 10, 2).is_ok());
    }

    #[test]
    fn release_requires_all() {
        let mut st = SceneTest::new(8);
        for s in 1..=7u8 {
            let _ = st.add_script(s, 5, 1);
        }
        assert!(!st.release_gate());
        for s in 1..=7u8 {
            let _ = st.run_script(s, true);
        }
        assert!(st.release_gate());
    }

    #[test]
    fn reconcile_lost_tickets() {
        let mut st = SceneTest::new(8);
        st.xfer(1, 1, 2);
        st.xfer(2, 2, 3);
        assert_eq!(st.reconcile(&[2]), [1]);
        st.backfill(1);
        assert!(st.ledger_clean());
    }
}
