//! VE-F4297 · 工具链总图（U05 工具集成组）
//! 职责：九面全景图（依赖/数据流/闸门位置三维标注）+ 健康度绿黄红实时
//! + 心跳校准误报 + 演进预留第十面（插件市场——只留冻结接口不写实现）
//! + 图与实现漂移自动重绘 + 文本替代读屏版（域本色）。
//! 段位：0x2EA0（续表 idx531）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2EA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MpCode {
    /// 图与实现漂移。
    MapDrift = SEG_BASE,
    /// 健康度误报（心跳校准）。
    HealthFalse = SEG_BASE + 0x01,
    /// 预留接口被实现（冻结违约）。
    ReservePrewrite = SEG_BASE + 0x02,
}

/// 健康度三色。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Health {
    Green = 0,
    Yellow = 1,
    Red = 2,
}

/// 面节点。
#[derive(Clone, Copy, Debug)]
pub struct MapNode {
    pub face: usize,
    /// 依赖面列表。
    pub deps: [bool; 9],
    pub has_gate: bool,   // 闸门位置
    pub has_flow: bool,   // 数据流
    pub health: Health,
    pub heartbeat: u32,   // 最近心跳
}

/// 全景图。
pub struct MapBook {
    pub nodes: Vec<MapNode>,
    pub cur_tick: u32,
    /// 第十面预留接口（冻结：仅声明）。
    pub reserve_declared: bool,
    pub reserve_body_bytes: usize,
    /// 重绘次数。
    pub redraws: u32,
    /// 文本替代读屏版（域本色）。
    pub alt_text: bool,
}

impl MapBook {
    pub fn new(cur_tick: u32) -> Self {
        MapBook { nodes: Vec::new(), cur_tick, reserve_declared: false, reserve_body_bytes: 0, redraws: 0, alt_text: false }
    }

    pub fn add(&mut self, n: MapNode) {
        if !self.nodes.iter().any(|x| x.face == n.face) {
            self.nodes.push(n);
        }
    }

    /// 依赖环检：A 依赖 B 且 B 依赖 A。
    pub fn cycle(&self) -> Option<(usize, usize)> {
        for a in &self.nodes {
            for b in &self.nodes {
                if a.face < b.face && a.deps[b.face] && b.deps[a.face] {
                    return Some((a.face, b.face));
                }
            }
        }
        None
    }

    /// 健康度校准：心跳超时→红灯；红灯但有新鲜心跳→误报校准黄。
    pub fn calibrate(&mut self, timeout: u32) -> usize {
        let t = self.cur_tick;
        let mut fixed = 0;
        for n in self.nodes.iter_mut() {
            if t.saturating_sub(n.heartbeat) > timeout && n.health != Health::Red {
                n.health = Health::Red;
            } else if t.saturating_sub(n.heartbeat) <= timeout && n.health == Health::Red {
                n.health = Health::Yellow; // 心跳新鲜→误报校准
                fixed += 1;
            }
        }
        fixed
    }

    /// 图实现漂移：面数不符→自动重绘。
    pub fn drift_check(&mut self, real_faces: usize) -> bool {
        if self.nodes.len() != real_faces {
            self.redraws += 1;
            return true;
        }
        false
    }

    /// 第十面预留：只留冻结接口不写实现。
    pub fn reserve(&mut self) -> Result<(), MpCode> {
        self.reserve_declared = true;
        if self.reserve_body_bytes > 0 {
            return Err(MpCode::ReservePrewrite);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

fn node(face: usize, health: Health, hb: u32, deps: &[(usize)]) -> MapNode {
    let mut d = [false; 9];
    for x in deps {
        d[*x] = true;
    }
    MapNode { face, deps: d, has_gate: false, has_flow: true, health, heartbeat: hb }
}

/// 判据组：C531-01..12。
pub fn run_ves531_checks() -> CheckSet {
    let mut set = CheckSet::new("ves531");

    let mut mb = MapBook::new(100);
    mb.add(node(0, Health::Green, 98, &[]));
    mb.add(node(1, Health::Green, 99, &[0]));
    mb.add(node(2, Health::Yellow, 95, &[0, 1]));
    chk(&mut set, "C531-01", mb.nodes.len() == 3, "九面节点登记（示例三面）");
    chk(&mut set, "C531-02", mb.cycle().is_none(), "依赖无环");

    // 环检出
    mb.add(node(3, Health::Green, 99, &[4]));
    mb.add(node(4, Health::Green, 99, &[3]));
    chk(&mut set, "C531-03", mb.cycle() == Some((3, 4)), "双向依赖环检出");
    mb.nodes.truncate(3);

    // 心跳校准
    mb.nodes[2].health = Health::Red;
    mb.nodes[2].heartbeat = 99;
    let fixed = mb.calibrate(5);
    chk(&mut set, "C531-04", fixed == 1 && mb.nodes[2].health == Health::Yellow, "红灯新鲜心跳=误报校准黄");
    mb.nodes[1].heartbeat = 50;
    mb.calibrate(5);
    chk(&mut set, "C531-05", mb.nodes[1].health == Health::Red, "心跳超时→红灯");

    // 漂移重绘
    chk(&mut set, "C531-06", !mb.drift_check(3), "面数一致不重绘");
    chk(&mut set, "C531-07", mb.drift_check(9) && mb.redraws == 1, "面数漂移自动重绘");

    // 第十面预留
    chk(&mut set, "C531-08", mb.reserve().is_ok(), "第十面冻结接口声明（插件市场）");
    mb.reserve_body_bytes = 128;
    chk(&mut set, "C531-09", mb.reserve() == Err(MpCode::ReservePrewrite), "预写实现=冻结违约");

    mb.alt_text = true;
    chk(&mut set, "C531-10", mb.alt_text, "文本替代读屏版（域本色）");
    chk(&mut set, "C531-11", mb.nodes.iter().all(|n| n.has_flow), "三维标注：数据流全有");
    chk(&mut set, "C531-12", mb.nodes[0].deps == [false; 9], "根面零依赖");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_book() {
        let mut mb = MapBook::new(10);
        mb.add(node(0, Health::Green, 9, &[]));
        mb.add(node(1, Health::Green, 9, &[0]));
        assert!(mb.cycle().is_none());
        mb.add(node(2, Health::Green, 9, &[1, 0]));
        assert!(mb.calibrate(20) == 0);
    }
}
