//! VE-F4284 · 一致性×IDE 插件（U05 工具集成组）
//! 职责：实时反馈三件（编辑即查波浪线+悬浮解释+一键快修）+ 离线降级
//! （本地规则子集兜底）+ 快修可撤销（undo 链）+ 能力协商降级
//! + 快修冲突差异预览人工确认 + 波浪线不只靠颜色（纹理+读屏标签——域本色）。
//! 段位：0x2A00（续表 idx518）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2A00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdCode {
    /// 版本失配（能力协商降级）。
    VerMismatch = SEG_BASE,
    /// 快修冲突（差异预览人工确认）。
    FixClash = SEG_BASE + 0x01,
    /// 撤销链断裂（整批回滚提示）。
    UndoBroken = SEG_BASE + 0x02,
    /// 离线无子集包。
    OfflineEmpty = SEG_BASE + 0x03,
}

/// 编辑响应（P95<50ms 目标）。
#[derive(Clone, Copy, Debug)]
pub struct EdResp {
    pub ms: u32,
}

/// 快修建议。
#[derive(Clone, Copy, Debug)]
pub struct QuickFix {
    pub line: u16,
    pub from_val: u32,
    pub to_val: u32,
    pub applied: bool,
    pub undone: bool,
}

/// IDE 插件器。
pub struct IdePlugin {
    /// 本地规则子集（离线兜底）。
    pub local_rules: Vec<u16>,
    /// IDE 能力版本。
    pub ide_ver: u32,
    /// 插件要求版本。
    pub plugin_need: u32,
    pub fixes: Vec<QuickFix>,
    /// undo 链。
    pub undo_chain: Vec<u16>,
    /// 响应样本。
    pub resp_samples: Vec<u32>,
    /// 纹理+读屏标签（不只靠颜色——域本色）。
    pub texture_on: bool,
    pub sr_label_on: bool,
}

impl IdePlugin {
    pub fn new(ide_ver: u32, plugin_need: u32) -> Self {
        IdePlugin { local_rules: Vec::new(), ide_ver, plugin_need, fixes: Vec::new(), undo_chain: Vec::new(), resp_samples: Vec::new(), texture_on: false, sr_label_on: false }
    }

    /// 能力协商：IDE 版本 ≥ 插件要求全功能；不足降级（关快修）。
    pub fn negotiate(&self) -> u8 {
        if self.ide_ver >= self.plugin_need {
            2 // 全功能
        } else if self.ide_ver + 10 >= self.plugin_need {
            1 // 降级（无快修）
        } else {
            0 // 失配不可用
        }
    }

    /// 离线兜底：本地子集非空即可查。
    pub fn offline_ok(&self) -> bool {
        !self.local_rules.is_empty()
    }

    /// 编辑响应采样。
    pub fn sample(&mut self, ms: u32) {
        self.resp_samples.push(ms);
    }

    /// 响应 P95。
    pub fn p95(&self) -> u32 {
        if self.resp_samples.is_empty() {
            return 0;
        }
        let mut s = self.resp_samples.clone();
        s.sort_unstable();
        let idx = (s.len() * 95 / 100).saturating_sub(1).min(s.len() - 1);
        s[idx]
    }

    /// 快修应用：冲突（同行已有未撤销修改）→差异预览人工确认。
    pub fn apply_fix(&mut self, line: u16, from: u32, to: u32) -> Result<bool, IdCode> {
        if self.negotiate() < 2 {
            return Err(IdCode::VerMismatch);
        }
        let clash = self.fixes.iter().any(|f| f.line == line && f.applied && !f.undone);
        self.fixes.push(QuickFix { line, from_val: from, to_val: to, applied: !clash, undone: false });
        if clash {
            return Ok(false); // 需人工确认
        }
        self.undo_chain.push(line);
        Ok(true)
    }

    /// undo：链断裂（行不在链尾）→整批回滚提示。
    pub fn undo_fix(&mut self, line: u16) -> Result<(), IdCode> {
        match self.undo_chain.last() {
            Some(&l) if l == line => {
                self.undo_chain.pop();
                if let Some(f) = self.fixes.iter_mut().rev().find(|f| f.line == line && f.applied && !f.undone) {
                    f.undone = true;
                }
                Ok(())
            }
            Some(_) => Err(IdCode::UndoBroken), // 断链→整批回滚提示
            None => Err(IdCode::UndoBroken),
        }
    }

    /// 全批回滚（断链处置）。
    pub fn rollback_all(&mut self) -> usize {
        let n = self.undo_chain.len();
        for f in self.fixes.iter_mut() {
            if f.applied && !f.undone {
                f.undone = true;
            }
        }
        self.undo_chain.clear();
        n
    }

    /// 域本色标记齐。
    pub fn a11y_marks(&self) -> bool {
        self.texture_on && self.sr_label_on
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C518-01..12。
pub fn run_ves518_checks() -> CheckSet {
    let mut set = CheckSet::new("ves518");

    let mut ip = IdePlugin::new(30, 30);
    ip.texture_on = true;
    ip.sr_label_on = true;
    chk(&mut set, "C518-01", ip.negotiate() == 2, "版本匹配全功能");
    chk(&mut set, "C518-02", ip.a11y_marks(), "波浪线纹理+读屏标签（不只靠颜色——域本色）");

    // 离线兜底
    chk(&mut set, "C518-03", !ip.offline_ok(), "无子集包离线不可用");
    ip.local_rules = alloc::vec![1, 2];
    chk(&mut set, "C518-04", ip.offline_ok(), "本地子集兜底可查");

    // 快修
    chk(&mut set, "C518-05", ip.apply_fix(10, 1, 2) == Ok(true), "快修应用");
    chk(&mut set, "C518-06", ip.apply_fix(10, 2, 3) == Ok(false), "同行冲突→差异预览人工确认");
    chk(&mut set, "C518-07", ip.undo_chain == alloc::vec![10], "undo 链在册");

    // undo
    chk(&mut set, "C518-08", ip.undo_fix(10).is_ok(), "链尾撤销成功");
    chk(&mut set, "C518-09", ip.undo_fix(10) == Err(IdCode::UndoBroken), "空链撤销报断链");

    let _ = ip.apply_fix(20, 5, 6);
    let _ = ip.apply_fix(30, 7, 8);
    chk(&mut set, "C518-10", ip.undo_fix(30) == Err(IdCode::UndoBroken), "非链尾撤销=断链");
    chk(&mut set, "C518-11", ip.rollback_all() == 2, "断链整批回滚 2 处");

    // P95<50ms
    for i in 0..20u32 {
        ip.sample(30 + i);
    }
    chk(&mut set, "C518-12", ip.p95() < 50, "编辑响应 P95<50ms");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ide_plugin() {
        let mut ip = IdePlugin::new(1, 5);
        assert_eq!(ip.negotiate(), 0);
        let mut ip2 = IdePlugin::new(15, 5);
        assert_eq!(ip2.negotiate(), 1);
        let _ = ip2.apply_fix(1, 1, 2);
        assert_eq!(ip2.apply_fix(1, 2, 3), Ok(false));
    }
}
