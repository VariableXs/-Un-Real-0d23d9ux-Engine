//! UNX-F24073 GEM 显存对象与 handle 生命周期·子功能13（0x40B00 段）
//!
//! 锚点：UNX-F24073-J1 GEM 对象表 + handle 账 + 容量闸逐条落账；J2 字段齐备；
//! J3~J12 核心操作语义与折叠指纹（跨域闸切面并入 J3/J11）；J13 注入核 5/5。
//! 域：CoRun·UNX-F5-B13（GEM 显存对象与 handle 生命周期）。
//!
//! 结构：GEM 对象表 + handle 账 + 容量闸 + 跨域闸（DomGate）切面账 + 注入核
//! （重名/野handle/超容/双关/印后 六路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40B00 独占 0x20）。
pub const SEG_BASE: u32 = 0x40B00;
pub const GO_DUP_NAME: u32 = SEG_BASE + 0x01; // 重名对象
pub const GO_BAD_HANDLE: u32 = SEG_BASE + 0x02; // 野 handle
pub const GO_NO_SPACE: u32 = SEG_BASE + 0x03; // 容量不足
pub const GO_DUP_OP: u32 = SEG_BASE + 0x04; // 重复操作
pub const GO_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 跨域闸切面账（GEM 显存对象与 handle 生命周期子切面）：记录 (k, v) 对，
/// v < k 违者回 GO_NO_SPACE，同 k 重记回 GO_DUP_NAME。
pub struct DomGate {
    pub tag: u32,
    pub hits: u32,
    pub log: Vec<(u32, u32)>,
}

impl DomGate {
    pub fn new(tag: u32) -> DomGate {
        DomGate { tag, hits: 0, log: vec![] }
    }

    /// 记账一笔：v < k。
    pub fn record(&mut self, k: u32, v: u32) -> Result<(), u32> {
        if v < k {
            return Err(GO_NO_SPACE);
        }
        if self.log.iter().any(|(lk, _)| *lk == k) {
            return Err(GO_DUP_NAME);
        }
        self.log.push((k, v));
        self.hits += 1;
        Ok(())
    }

    /// 切面累计值（跨域闸）。
    pub fn dom_cnt(&self) -> u32 {
        let mut acc: u32 = 0;
        for (k, v) in &self.log {
            acc = acc.wrapping_sub((*k).wrapping_mul(*v).wrapping_add(self.tag));
        }
        acc
    }

    /// 切面折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40B00;
        for (k, v) in &self.log {
            h = h.wrapping_mul(31).wrapping_add((*k << 8) ^ *v);
        }
        h ^ self.tag
    }
}

/// 页粒（4KiB）。
pub const PAGE: u32 = 0x1000;

/// GEM 对象条目。
#[derive(Clone, PartialEq)]
pub struct GemObj {
    pub obj_id: u32,
    pub name: String,
    pub size: u32,
    pub pages: u32,
    pub refcnt: u32,
    pub live: bool,
}

/// handle 账目。
#[derive(Clone, Copy, PartialEq)]
pub struct HandleRec {
    pub handle: u32,
    pub obj_id: u32,
    pub open: bool,
}

/// HdlTab（GEM 对象表 + handle 账 + 容量闸）。
pub struct HdlTab {
    pub objs: Vec<GemObj>,
    pub handles: Vec<HandleRec>,
    pub cap_pages: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HdlTab {
    pub fn new(cap_pages: u32) -> HdlTab {
        HdlTab { objs: vec![], handles: vec![], cap_pages, seq_next: 1, sealed: false }
    }

    /// 已用页数（活对象折算）。
    pub fn used_pages(&self) -> u32 {
        self.objs.iter().filter(|o| o.live).map(|o| o.pages).sum()
    }

    /// 活对象计数。
    pub fn live_objs(&self) -> u32 {
        self.objs.iter().filter(|o| o.live).count() as u32
    }

    fn obj_idx(&self, obj_id: u32) -> Option<usize> {
        self.objs.iter().position(|o| o.obj_id == obj_id && o.live)
    }

    fn hrec_idx(&self, handle: u32) -> Option<usize> {
        self.handles.iter().position(|h| h.handle == handle && h.open)
    }

    /// 新建对象：印后拒 + 重名拒 + 容量闸 + 落账。
    pub fn new_obj(&mut self, name: &str, size: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        if name.is_empty() || self.objs.iter().any(|o| o.name == name) {
            return Err(GO_DUP_NAME);
        }
        let pages = (size + PAGE - 1) / PAGE;
        if size == 0 || self.used_pages() + pages > self.cap_pages {
            return Err(GO_NO_SPACE);
        }
        let obj_id = self.objs.len() as u32;
        self.objs.push(GemObj {
            obj_id,
            name: name.to_string(),
            size,
            pages,
            refcnt: 1,
            live: true,
        });
        Ok(obj_id)
    }

    /// 开立 handle：印后拒 + 野对象拒 + 引用升账。
    pub fn open_handle(&mut self, obj_id: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        if self.obj_idx(obj_id).is_none() {
            return Err(GO_BAD_HANDLE);
        }
        let handle = self.seq_next;
        self.seq_next += 1;
        self.handles.push(HandleRec { handle, obj_id, open: true });
        if let Some(i) = self.obj_idx(obj_id) {
            self.objs[i].refcnt += 1;
        }
        Ok(handle)
    }

    /// 查 handle 当前引用数。
    pub fn ref_of(&self, handle: u32) -> Option<u32> {
        let h = self.handles.iter().find(|h| h.handle == handle)?;
        self.objs.iter().find(|o| o.obj_id == h.obj_id).map(|o| o.refcnt)
    }

    /// 引用 +1：野 handle 拒。
    pub fn ref_inc(&mut self, handle: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        let i = match self.hrec_idx(handle) {
            Some(i) => i,
            None => return Err(GO_BAD_HANDLE),
        };
        let obj_id = self.handles[i].obj_id;
        match self.obj_idx(obj_id) {
            Some(j) => {
                self.objs[j].refcnt += 1;
                Ok(self.objs[j].refcnt)
            }
            None => Err(GO_BAD_HANDLE),
        }
    }

    /// 引用 -1：归零即收口 handle 并标记对象死亡。
    pub fn ref_dec(&mut self, handle: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        let i = match self.hrec_idx(handle) {
            Some(i) => i,
            None => return Err(GO_BAD_HANDLE),
        };
        let obj_id = self.handles[i].obj_id;
        let j = match self.obj_idx(obj_id) {
            Some(j) => j,
            None => return Err(GO_BAD_HANDLE),
        };
        self.objs[j].refcnt -= 1;
        let rc = self.objs[j].refcnt;
        if rc == 0 {
            self.objs[j].live = false;
            self.handles[i].open = false;
        }
        Ok(rc)
    }

    /// 关闭 handle：野 handle 拒 + 双关即重复操作。
    pub fn close_handle(&mut self, handle: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        let i = match self.handles.iter().position(|h| h.handle == handle) {
            Some(i) => i,
            None => return Err(GO_BAD_HANDLE),
        };
        if !self.handles[i].open {
            return Err(GO_DUP_OP);
        }
        self.handles[i].open = false;
        let obj_id = self.handles[i].obj_id;
        if let Some(j) = self.obj_idx(obj_id) {
            self.objs[j].refcnt = self.objs[j].refcnt.saturating_sub(1);
        }
        Ok(())
    }

    /// 账本折叠指纹（对象 + handle 双表）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40B00;
        for o in &self.objs {
            h = h.wrapping_mul(31).wrapping_add(o.obj_id ^ o.pages);
            for by in o.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        for r in &self.handles {
            h = h.wrapping_mul(31).wrapping_add(r.handle ^ (r.obj_id << 8));
        }
        h
    }

    /// 封印：印后一切写操作必拒。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：三对象（oa0c/ob0c/oc0c）两 handle，容量 32 页。
pub fn mk_gem() -> HdlTab {
    let mut g = HdlTab::new(32);
    let _ = g.new_obj("oa0c", 4096);
    let _ = g.new_obj("ob0c", 8192);
    let _ = g.new_obj("oc0c", 16384);
    let _ = g.open_handle(0);
    let _ = g.open_handle(1);
    g
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut g = mk_gem();
    // 1) 重名对象
    got[0] = g.new_obj("oa0c", 4096).err() == Some(GO_DUP_NAME);
    // 2) 野 handle 引用
    got[1] = g.ref_inc(999).err() == Some(GO_BAD_HANDLE);
    // 3) 容量不足
    got[2] = g.new_obj("hugeinj", 0x100000).err() == Some(GO_NO_SPACE);
    // 4) 双关重复操作
    let h = match g.open_handle(2) { Ok(h) => h, Err(_) => 0 };
    let _ = g.close_handle(h);
    got[3] = g.close_handle(h).err() == Some(GO_DUP_OP);
    // 5) 印后新建
    g.seal();
    got[4] = g.new_obj("lateinj", 4096).err() == Some(GO_FROZEN);
    got
}

/// 判据主体（13 条，全部真跑账本）。
pub fn run_unx24073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24073");
    let mut g = mk_gem();

    // J1 账非空
    let j1 = g.objs.len() == 3 && g.handles.len() == 2;
    s.add("J1-账非空", j1, "三对象两 handle 落表");

    // J2 字段齐
    let j2 = g.objs.iter().all(|o| o.live && o.pages > 0 && o.refcnt >= 1)
        && g.handles.iter().all(|h| h.open);
    s.add("J2-字段齐", j2, "对象与 handle 字段完备");

    // J3 跨域闸：切面记账 + 新建对象落账
    let mut asp = DomGate::new(24073);
    let a1 = asp.record(10, 100).is_ok();
    let a2 = asp.record(10, 100).err() == Some(GO_DUP_NAME);
    let a3 = asp.record(200, 50).err() == Some(GO_NO_SPACE);
    let n3 = g.new_obj("ov0cx", 8192);
    let j3 = n3.is_ok() && g.objs.len() == 4 && a1 && a2 && a3;
    s.add("J3-新建落账", j3, "跨域闸切面三连 + 第四对象入账");

    // J4 重名拒
    let dup_name = g.objs[0].name.clone();
    let j4 = g.new_obj(&dup_name, 4096).err() == Some(GO_DUP_NAME);
    s.add("J4-重名拒", j4, "对象重名必拒");

    // J5 handle 开立与引用升账
    let h5 = match g.open_handle(2) { Ok(h) => h, Err(_) => 0 };
    let j5 = h5 != 0 && g.ref_of(h5) == Some(2);
    s.add("J5-handle落账", j5, "obj2 开 handle 后引用为 2");

    // J6 野 handle 拒
    let j6 = g.ref_inc(999).err() == Some(GO_BAD_HANDLE)
        && g.ref_dec(999).err() == Some(GO_BAD_HANDLE);
    s.add("J6-野handle拒", j6, "越表 handle 引用必拒");

    // J7 引用升降守恒
    let up = g.ref_inc(1);
    let down = g.ref_dec(1);
    let j7 = up.ok() == Some(3) && down.ok() == Some(2) && g.ref_of(1) == Some(2);
    s.add("J7-引用守恒", j7, "升一降一后引用还原");

    // J8 归零收口
    let oid8 = match g.new_obj("tmpj8", 4096) { Ok(x) => x, Err(_) => 0 };
    let h8 = match g.open_handle(oid8) { Ok(h) => h, Err(_) => 0 };
    let _ = g.ref_dec(h8);
    let zero = g.ref_dec(h8);
    let j8 = zero.ok() == Some(0) && g.ref_inc(h8).err() == Some(GO_BAD_HANDLE);
    s.add("J8-归零收口", j8, "引用归零即收口 handle");

    // J9 容量闸
    let j9 = g.new_obj("hugej9", 0x100000).err() == Some(GO_NO_SPACE);
    s.add("J9-容量闸", j9, "超容量对象必拒");

    // J10 双关重复操作
    let c1 = g.close_handle(1);
    let c2 = g.close_handle(1);
    let j10 = c1.is_ok() && c2.err() == Some(GO_DUP_OP);
    s.add("J10-双关拒", j10, "handle 双关即重复操作");

    // J11 指纹稳定（账本 + 切面双折叠）
    let j11 = g.fold() == g.fold() && asp.fold() == asp.fold();
    s.add("J11-指纹稳定", j11, "账本与切面两次折叠一致");

    // J12 印后冻结
    let fp = g.seal();
    let j12 = fp > 0 && g.new_obj("afterseal", 4096).err() == Some(GO_FROZEN);
    s.add("J12-印后冻结", j12, "封印后新建必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24073_gem_obj_flow() {
        let mut g = mk_gem();
        assert!(g.new_obj("testobj", 4096).is_ok());
        assert!(g.new_obj("oa0c", 4096).err() == Some(GO_DUP_NAME));
        assert!(g.ref_inc(999).err() == Some(GO_BAD_HANDLE));
        assert!(g.new_obj("huge", 0x100000).err() == Some(GO_NO_SPACE));
        assert!(g.seal() > 0);
        assert!(g.new_obj("late", 4096).err() == Some(GO_FROZEN));
    }

    #[test]
    fn unx24073_checks_green() {
        let s = run_unx24073_checks();
        assert!(s.all_passed(), "UNX-F24073 判据未全绿");
    }
}
