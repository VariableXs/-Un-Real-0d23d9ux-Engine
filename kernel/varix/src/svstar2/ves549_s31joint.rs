//! VE-F4315 · 场景与 U 域各组联动（U06 场景应用组）
//! 职责：场景面与 U01-U05 各组联动矩阵（每景×每组标注消费什么/回供什么
//! ——双向关系显式化）+ 联动健康度心跳探针（失联降级标注）+ 矩阵漂移
//! 生成器重绘 + 联动变更冻结流程（越权变更拦截）+ 矩阵读屏替代文本
//! （域本色）。
//! 段位：0x3220（续表 idx549）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3220;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JkCode {
    /// 心跳失联（联动降级标注）。
    HeartbeatLost = SEG_BASE,
    /// 越权变更（冻结流程拦截）。
    FrozenChange = SEG_BASE + 0x01,
    /// 联动格越界（组号 1..=5 景号 1..=7 之外）。
    CellRange = SEG_BASE + 0x02,
    /// 替代文本缺失（读屏不可达——域色）。
    AltMissing = SEG_BASE + 0x03,
}

/// 联动格：景 × 组，双向关系显式。
#[derive(Clone, Debug, PartialEq)]
pub struct LinkCell {
    pub scene: u8,
    pub group: u8, // U01..U05
    /// 消费什么（下游取）。
    pub consumes: &'static str,
    /// 回供什么（上游给）。
    pub feeds: &'static str,
    /// 读屏替代文本（域本色硬要求非空）。
    pub alt: &'static str,
    /// 心跳失联降级标注。
    pub degraded: bool,
}

/// 联动矩阵台。
#[derive(Default)]
pub struct JointMatrix {
    pub cells: Vec<LinkCell>,
    /// 心跳时刻表：(组, 最后心跳)。
    pub beats: Vec<(u8, u64)>,
    now: u64,
    /// 冻结中的组（变更走冻结流程）。
    pub frozen: Vec<u8>,
    pub redraws: u32,
}

const GROUP_MAX: u8 = 5;
const SCENE_MAX: u8 = 7;
const BEAT_TTL: u64 = 30;

impl JointMatrix {
    pub fn new(now: u64) -> Self {
        JointMatrix { now, ..Default::default() }
    }

    /// 联动格登记：替代文本缺失即拒（域色）；格越界拒。
    pub fn link(&mut self, cell: LinkCell) -> Result<(), JkCode> {
        if cell.scene == 0 || cell.scene > SCENE_MAX || cell.group == 0 || cell.group > GROUP_MAX {
            return Err(JkCode::CellRange);
        }
        if cell.alt.is_empty() {
            return Err(JkCode::AltMissing);
        }
        if let Some(c) = self.cells.iter_mut().find(|c| c.scene == cell.scene && c.group == cell.group) {
            *c = cell;
        } else {
            self.cells.push(cell);
        }
        Ok(())
    }

    /// 心跳上报。
    pub fn beat(&mut self, group: u8, at: u64) {
        self.now = self.now.max(at);
        if let Some(b) = self.beats.iter_mut().find(|b| b.0 == group) {
            b.1 = at;
        } else {
            self.beats.push((group, at));
        }
    }

    /// 心跳校准：超 TTL 未跳组 → 其联动格降级标注（返回降级格数）。
    pub fn calibrate(&mut self) -> usize {
        let now = self.now;
        let mut n = 0;
        for b in self.beats.iter() {
            let lost = now >= b.1 + BEAT_TTL;
            for c in self.cells.iter_mut().filter(|c| c.group == b.0) {
                if lost && !c.degraded {
                    c.degraded = true;
                    n += 1;
                } else if !lost && c.degraded {
                    c.degraded = false; // 心跳恢复自动解除
                }
            }
        }
        // 完全无心跳的组的格子也降级。
        for c in self.cells.iter_mut() {
            if !self.beats.iter().any(|b| b.0 == c.group) && !c.degraded {
                c.degraded = true;
                n += 1;
            }
        }
        n
    }

    /// 冻结流程：组进冻结名单。
    pub fn freeze(&mut self, group: u8) {
        if !self.frozen.contains(&group) {
            self.frozen.push(group);
        }
    }

    /// 变更执行：冻结组拦截（越权变更）；未冻结组放行。
    pub fn mutate(&mut self, cell: LinkCell) -> Result<(), JkCode> {
        if self.frozen.contains(&cell.group) {
            return Err(JkCode::FrozenChange);
        }
        self.link(cell)
    }

    /// 解冻（冻结流程走完）。
    pub fn unfreeze(&mut self, group: u8) {
        self.frozen.retain(|g| *g != group);
    }

    /// 矩阵漂移重绘：期望格数与在册格数不符 → 生成器重绘（计数留痕）。
    pub fn drift_redraw(&mut self, expected: usize) -> bool {
        if self.cells.len() != expected {
            self.redraws += 1;
            true
        } else {
            false
        }
    }

    /// 时钟推进（心跳校准时间源）。
    pub fn advance(&mut self, dt: u64) {
        self.now += dt;
    }

    /// 读屏替代文本取用（域本色）。
    pub fn alt_of(&self, scene: u8, group: u8) -> Option<&'static str> {
        self.cells.iter().find(|c| c.scene == scene && c.group == group).map(|c| c.alt)
    }

    /// 健康度：全格非降级 = 健康。
    pub fn healthy(&self) -> bool {
        self.cells.iter().all(|c| !c.degraded)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves549_checks() -> CheckSet {
    let mut set = CheckSet::new("ves549");
    let mut jm = JointMatrix::new(0);

    // C549-01 联动格登记（双向关系 + 替代文本）。
    let mut ok = true;
    for s in 1..=7u8 {
        let c = LinkCell {
            scene: s, group: (s % 5) + 1,
            consumes: "判定结论", feeds: "触达事件",
            alt: "场景对组的联动说明", degraded: false,
        };
        if jm.link(c).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C549-01", ok && jm.cells.len() == 7, "联动矩阵登记");

    // C549-02 替代文本缺失拒收（域色硬线）。
    chk(&mut set, "C549-02", jm.link(LinkCell { scene: 1, group: 1, consumes: "x", feeds: "y", alt: "", degraded: false })
        == Err(JkCode::AltMissing), "替代文本缺失拒");

    // C549-03 格越界拒（组 6 / 景 8）。
    chk(&mut set, "C549-03", jm.link(LinkCell { scene: 1, group: 6, consumes: "x", feeds: "y", alt: "a", degraded: false }).is_err()
        && jm.link(LinkCell { scene: 8, group: 1, consumes: "x", feeds: "y", alt: "a", degraded: false }).is_err(), "格越界拒");

    // C549-04 心跳上报后全格健康。
    for g in 1..=5u8 {
        jm.beat(g, 10);
    }
    let deg = jm.calibrate();
    chk(&mut set, "C549-04", deg == 0 && jm.healthy(), "心跳齐全健康");

    // C549-05 心跳失联降级标注。
    jm.advance(60); // 全组超 TTL
    let deg2 = jm.calibrate();
    chk(&mut set, "C549-05", deg2 == 7 && !jm.healthy(), "失联降级标注");

    // C549-06 心跳恢复自动解除降级。
    for g in 1..=5u8 {
        jm.beat(g, 61);
    }
    let deg3 = jm.calibrate();
    chk(&mut set, "C549-06", deg3 == 0 && jm.healthy(), "恢复自动解除");

    // C549-07 冻结流程：冻结组变更拦截。
    jm.freeze(2);
    chk(&mut set, "C549-07", jm.mutate(LinkCell { scene: 2, group: 2, consumes: "x", feeds: "y", alt: "a", degraded: false })
        == Err(JkCode::FrozenChange), "冻结组变更拦");

    // C549-08 解冻后放行。
    jm.unfreeze(2);
    chk(&mut set, "C549-08", jm.mutate(LinkCell { scene: 2, group: 2, consumes: "x2", feeds: "y2", alt: "a2", degraded: false }).is_ok(), "解冻放行");

    // C549-09 矩阵漂移重绘（格数不符触发 + 计数）。
    let d1 = jm.drift_redraw(99);
    let d2 = jm.drift_redraw(jm.cells.len());
    chk(&mut set, "C549-09", d1 && !d2 && jm.redraws == 1, "漂移重绘留痕");

    // C549-10 读屏替代文本取用（域本色）。
    chk(&mut set, "C549-10", jm.alt_of(2, 2) == Some("a2") && jm.alt_of(9, 9).is_none(), "替代文本可达");

    // C549-11 同格覆盖更新（幂等不膨胀）。
    let before = jm.cells.len();
    let _ = jm.link(LinkCell { scene: 2, group: 2, consumes: "x3", feeds: "y3", alt: "a3", degraded: false });
    chk(&mut set, "C549-11", jm.cells.len() == before && jm.alt_of(2, 2) == Some("a3"), "同格覆盖幂等");

    // C549-12 完全无心跳组的格子降级兜底。
    let mut jm2 = JointMatrix::new(0);
    let _ = jm2.link(LinkCell { scene: 1, group: 5, consumes: "x", feeds: "y", alt: "a", degraded: false });
    let deg4 = jm2.calibrate();
    chk(&mut set, "C549-12", deg4 == 1 && !jm2.healthy(), "无心跳兜底降级");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(scene: u8, group: u8) -> LinkCell {
        LinkCell { scene, group, consumes: "c", feeds: "f", alt: "alt", degraded: false }
    }

    #[test]
    fn freeze_gate() {
        let mut jm = JointMatrix::new(0);
        jm.freeze(3);
        assert_eq!(jm.mutate(cell(1, 3)), Err(JkCode::FrozenChange));
        assert!(jm.mutate(cell(1, 4)).is_ok());
        jm.unfreeze(3);
        assert!(jm.mutate(cell(1, 3)).is_ok());
    }

    #[test]
    fn heartbeat_degrade_cycle() {
        let mut jm = JointMatrix::new(0);
        let _ = jm.link(cell(1, 1));
        jm.beat(1, 5);
        assert_eq!(jm.calibrate(), 0);
        jm.advance(BEAT_TTL);
        assert_eq!(jm.calibrate(), 1);
        jm.beat(1, BEAT_TTL + 5);
        assert_eq!(jm.calibrate(), 0);
    }

    #[test]
    fn alt_required() {
        let mut jm = JointMatrix::new(0);
        assert_eq!(jm.link(LinkCell { scene: 1, group: 1, consumes: "c", feeds: "f", alt: "", degraded: false }), Err(JkCode::AltMissing));
    }
}
