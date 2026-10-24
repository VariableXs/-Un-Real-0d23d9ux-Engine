//! VE-F4281 · 工具集成总纲（U05 一致性与工具集成组）
//! 职责：一致性能力嵌进九大工具面（CI/审查/IDE/设计/调试/看板/市场/文档/SDK）
//! + 两原则（工具面不改变判定权——判定恒单源 U03；结果单源分发）
//! + 统一事件总线+适配器注册制 + 死信队列 + 适配器崩溃隔离。
//! 段位：0x2980（续表 idx515）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2980;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InCode {
    /// 工具私改判定（拒绝接入并公示）。
    JudgeHijack = SEG_BASE,
    /// 事件丢失（重投+死信）。
    EventLost = SEG_BASE + 0x01,
    /// 适配器崩溃（隔离）。
    AdapterCrash = SEG_BASE + 0x02,
    /// 面越界（九面外）。
    FaceOver = SEG_BASE + 0x03,
}

/// 九大工具面。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Ci = 0,
    Review = 1,
    Ide = 2,
    Design = 3,
    Debugger = 4,
    PerfBoard = 5,
    MarketGate = 6,
    DocSys = 7,
    Sdk = 8,
}
pub const FACE_N: usize = 9;

/// 事件。
#[derive(Clone, Copy, Debug)]
pub struct BusEvent {
    pub id: u32,
    pub face: Face,
    pub payload: u64,
    /// 投递状态：0 待投 1 已达 2 死信。
    pub state: u8,
    pub retries: u8,
}

/// 事件总线。
pub struct EventBus {
    pub events: Vec<BusEvent>,
    /// 适配器注册表：面→在线。
    pub adapters: [bool; FACE_N],
    /// 崩溃隔离的适配器。
    pub quarantined: [bool; FACE_N],
    /// 判定单源声明（U03）。
    pub judge_single_src: bool,
    /// 死信上限。
    pub dead_cap: u32,
}

impl EventBus {
    pub fn new(dead_cap: u32) -> Self {
        EventBus { events: Vec::new(), adapters: [false; FACE_N], quarantined: [false; FACE_N], judge_single_src: true, dead_cap }
    }

    /// 适配器注册（面必须九面内——由 Face 类型保证；判定权不可私改）。
    pub fn register(&mut self, f: Face, claims_judge: bool) -> Result<(), InCode> {
        if claims_judge {
            return Err(InCode::JudgeHijack); // 工具面不改变判定权
        }
        self.adapters[f as usize] = true;
        Ok(())
    }

    /// 事件投递（扇出 O(1)）。
    pub fn publish(&mut self, face: Face, payload: u64) -> u32 {
        let id = self.events.len() as u32 + 1;
        self.events.push(BusEvent { id, face, payload, state: 0, retries: 0 });
        id
    }

    /// 投递重试：三次失败入死信；隔离适配器不投。
    pub fn pump(&mut self, id: u32, deliver_ok: bool) -> Result<u8, InCode> {
        let e = self.events.iter_mut().find(|e| e.id == id).ok_or(InCode::EventLost)?;
        if self.quarantined[e.face as usize] {
            return Err(InCode::AdapterCrash);
        }
        if deliver_ok {
            e.state = 1;
            return Ok(1);
        }
        e.retries += 1;
        if e.retries >= 3 {
            e.state = 2; // 死信
            return Ok(2);
        }
        Ok(0) // 待重投
    }

    /// 死信队列。
    pub fn dead_letters(&self) -> Vec<u32> {
        self.events.iter().filter(|e| e.state == 2).map(|e| e.id).collect()
    }

    /// 适配器崩溃隔离（不拖垮总线）。
    pub fn quarantine(&mut self, f: Face) {
        self.quarantined[f as usize] = true;
        self.adapters[f as usize] = false;
    }

    /// 在线面数。
    pub fn online(&self) -> usize {
        self.adapters.iter().filter(|a| **a).count()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C515-01..12。
pub fn run_ves515_checks() -> CheckSet {
    let mut set = CheckSet::new("ves515");

    let mut eb = EventBus::new(10);
    chk(&mut set, "C515-01", eb.judge_single_src, "判定单源声明在册（U03）");
    chk(&mut set, "C515-02", eb.register(Face::Ci, true) == Err(InCode::JudgeHijack), "工具私改判定拒绝接入");
    chk(&mut set, "C515-03", eb.register(Face::Ci, false).is_ok() && eb.online() == 1, "合规注册成功");

    for f in [Face::Review, Face::Ide, Face::Design, Face::Debugger, Face::PerfBoard, Face::MarketGate, Face::DocSys, Face::Sdk] {
        let _ = eb.register(f, false);
    }
    chk(&mut set, "C515-04", eb.online() == 9, "九面全注册");

    // 事件总线
    let e1 = eb.publish(Face::Ci, 0xAA);
    let e2 = eb.publish(Face::Ide, 0xBB);
    chk(&mut set, "C515-05", e1 == 1 && e2 == 2, "事件扇出编号递增");
    chk(&mut set, "C515-06", eb.pump(e1, true) == Ok(1), "投递已达");
    chk(&mut set, "C515-07", eb.pump(e2, false) == Ok(0) && eb.pump(e2, false) == Ok(0), "重投两次");
    chk(&mut set, "C515-08", eb.pump(e2, false) == Ok(2), "三次失败入死信");
    chk(&mut set, "C515-09", eb.dead_letters() == alloc::vec![2], "死信队列在册");

    // 崩溃隔离
    eb.quarantine(Face::Design);
    let e3 = eb.publish(Face::Design, 1);
    chk(&mut set, "C515-10", eb.pump(e3, true) == Err(InCode::AdapterCrash), "隔离适配器不投递");
    chk(&mut set, "C515-11", eb.online() == 8, "隔离不拖垮总线（8 面在线）");

    chk(&mut set, "C515-12", FACE_N == 9 && Face::Sdk as usize == 8, "九面枚举完备");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bus() {
        let mut eb = EventBus::new(5);
        assert_eq!(eb.register(Face::Ide, true), Err(InCode::JudgeHijack));
        let _ = eb.register(Face::Ide, false);
        let id = eb.publish(Face::Ide, 7);
        assert_eq!(eb.pump(id, true), Ok(1));
        eb.quarantine(Face::Ide);
        let id2 = eb.publish(Face::Ide, 8);
        assert_eq!(eb.pump(id2, true), Err(InCode::AdapterCrash));
    }
}
