//! VE-F0160 · 场景图浏览器（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：节点遍历，层级可见，属性可查，变更可追。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E6;
pub const XC_NODE_NOT_FOUND: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneNode {
    pub id: u32,
    pub parent: u32,
    pub visible: bool,
}

pub struct SceneBrowser {
    nodes: Vec<SceneNode>,
    last_code: u16,
}

impl SceneBrowser {
    pub fn new() -> Self { Self { nodes: Vec::new(), last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn add_node(&mut self, id: u32, parent: u32, visible: bool) -> Result<(), u16> {
        if self.nodes.iter().any(|n| n.id == id) { self.last_code = XC_NODE_NOT_FOUND; return Err(XC_NODE_NOT_FOUND); }
        self.nodes.push(SceneNode { id, parent, visible });
        self.last_code = 0;
        Ok(())
    }
    pub fn node_count(&self) -> usize { self.nodes.len() }
    pub fn visible_count(&self) -> usize { self.nodes.iter().filter(|n| n.visible).count() }
    pub fn find(&self, id: u32) -> Option<&SceneNode> { self.nodes.iter().find(|n| n.id == id) }
}

pub fn run_vea160_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea160_scenegraph");
    { let s = SceneBrowser::new(); chk(&mut set, "C160-01", s.node_count() == 0, "err"); }
    { let mut s = SceneBrowser::new(); let r = s.add_node(1, 0, true);
        chk(&mut set, "C160-02", r.is_ok() && s.node_count() == 1, "err"); }
    { let mut s = SceneBrowser::new(); s.add_node(1, 0, true); let r = s.add_node(1, 0, true);
        chk(&mut set, "C160-03", r.err() == Some(XC_NODE_NOT_FOUND), "err"); }
    { let mut s = SceneBrowser::new(); s.add_node(1, 0, true); s.add_node(2, 1, false);
        chk(&mut set, "C160-04", s.visible_count() == 1, "err"); }
    { let mut s = SceneBrowser::new(); s.add_node(1, 0, true);
        chk(&mut set, "C160-05", s.find(1).is_some(), "err"); }
    { let mut s = SceneBrowser::new(); s.add_node(1, 0, true);
        chk(&mut set, "C160-06", s.find(99).is_none(), "err"); }
    { let mut s = SceneBrowser::new(); for i in 0..5 { s.add_node(i, 0, i % 2 == 0); }
        chk(&mut set, "C160-07", s.visible_count() == 3, "err"); }
    { let s = SceneBrowser::new(); chk(&mut set, "C160-08", s.visible_count() == 0, "err"); }
    { let mut s = SceneBrowser::new(); s.add_node(1, 0, true);
        chk(&mut set, "C160-09", s.last_code() == 0, "err"); }
    { let mut s = SceneBrowser::new(); s.add_node(1, 0, true);
        chk(&mut set, "C160-10", s.find(1).unwrap().visible, "err"); }
    set
}
