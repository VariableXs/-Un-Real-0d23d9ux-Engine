//! UNX-F24067 GEM 显存对象与 handle 生命周期·子功能07（0x40A40 段）
//!
//! 锚点：UNX-F24067-J1 对象账齐备；J2 字段齐备；J3~J8 双通道切面操作语义
//! （页数对账/封闭回收）可证伪全绿；J9 引用守恒；J10 守卫拒；J11 闭集守恒；
//! J12 折叠指纹稳定且敏感；J13 注入核 5/5 全抓。
//! 域：CoRun·UNX-F5-B15（GEM 显存对象与 handle 生命周期）。
//!
//! 切面：子功能07 以「页数对账 × 封闭回收」双通道推进，折叠指纹取双车道异或变体，
//! 诊断码闭集 GO_FROZEN/GO_BAD_OBJ/GO_BAD_SIZE/GO_STILL_OPEN/GO_CLOSED。
//!
//! 结构：GEM 对象表 + handle 账（引用计数）+ 生命周期核（alloc/get/put/destroy）+ 引用闸（野对象/活引用销毁/关闭后操作）+ 双通道切面操作 + 折叠指纹 + 注入核（尺寸非法/野对象/活引用销毁/关闭后取用/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40A40 独占 0x20）。
pub const SEG_BASE: u32 = 0x40A40;
pub const GO_FROZEN: u32 = SEG_BASE + 0x01; // 印后操作
pub const GO_BAD_OBJ: u32 = SEG_BASE + 0x02; // 野对象或野 handle
pub const GO_BAD_SIZE: u32 = SEG_BASE + 0x03; // 尺寸非法
pub const GO_STILL_OPEN: u32 = SEG_BASE + 0x04; // 仍被活引用占用
pub const GO_CLOSED: u32 = SEG_BASE + 0x05; // 已关闭 handle 上的操作

/// GEM 显存对象。
#[derive(Clone, PartialEq)]
pub struct GemObj {
    pub obj_id: u32,
    pub size: u64,
    pub seq: u32,
}

impl GemObj {
    /// 4KiB 页数（向上取整）。
    pub fn pages(&self) -> u32 {
        ((self.size + 4095) / 4096) as u32
    }
}

/// 用户态 handle 账目（refcnt 归零视为已关闭）。
#[derive(Clone, PartialEq)]
pub struct GemHandle {
    pub handle: u32,
    pub obj_id: u32,
    pub client: u32,
    pub refcnt: u32,
}

/// GEM 对象表。
pub struct GemTable {
    pub objs: Vec<GemObj>,
    pub handles: Vec<GemHandle>,
    pub obj_next: u32,
    pub hnd_next: u32,
    pub sealed: bool,
}

impl GemTable {
    pub fn new() -> GemTable {
        GemTable { objs: Vec::new(), handles: Vec::new(), obj_next: 1, hnd_next: 1, sealed: false }
    }

    /// 建对象（尺寸非法走 GO_BAD_SIZE）。
    pub fn create_obj(&mut self, size: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        if size == 0 || size > 0x01000000 {
            return Err(GO_BAD_SIZE);
        }
        let obj_id = self.obj_next;
        self.obj_next += 1;
        let seq = self.objs.len() as u32;
        self.objs.push(GemObj { obj_id, size, seq });
        Ok(obj_id)
    }

    /// 分配 handle（野对象走 GO_BAD_OBJ）。
    pub fn alloc_handle(&mut self, obj_id: u32, client: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        if !self.objs.iter().any(|o| o.obj_id == obj_id) {
            return Err(GO_BAD_OBJ);
        }
        let handle = self.hnd_next;
        self.hnd_next += 1;
        self.handles.push(GemHandle { handle, obj_id, client, refcnt: 1 });
        Ok(handle)
    }

    /// 取引用（已关闭走 GO_CLOSED，野 handle 走 GO_BAD_OBJ）。
    pub fn get_ref(&mut self, handle: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        for h in self.handles.iter_mut() {
            if h.handle == handle {
                if h.refcnt == 0 {
                    return Err(GO_CLOSED);
                }
                h.refcnt += 1;
                return Ok(());
            }
        }
        Err(GO_BAD_OBJ)
    }

    /// 还引用（归零即关闭，关闭后复还走 GO_CLOSED）。
    pub fn put_ref(&mut self, handle: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        for h in self.handles.iter_mut() {
            if h.handle == handle {
                if h.refcnt == 0 {
                    return Err(GO_CLOSED);
                }
                h.refcnt -= 1;
                return Ok(());
            }
        }
        Err(GO_BAD_OBJ)
    }

    /// 销毁对象（仍有活引用走 GO_STILL_OPEN，野对象走 GO_BAD_OBJ）。
    pub fn destroy_obj(&mut self, obj_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GO_FROZEN);
        }
        if self.handles.iter().any(|h| h.obj_id == obj_id && h.refcnt > 0) {
            return Err(GO_STILL_OPEN);
        }
        if !self.objs.iter().any(|o| o.obj_id == obj_id) {
            return Err(GO_BAD_OBJ);
        }
        self.objs.retain(|o| o.obj_id != obj_id);
        self.handles.retain(|h| h.obj_id != obj_id);
        Ok(())
    }

    pub fn obj_of(&self, obj_id: u32) -> Option<&GemObj> {
        self.objs.iter().find(|o| o.obj_id == obj_id)
    }

    pub fn handle_of(&self, handle: u32) -> Option<&GemHandle> {
        self.handles.iter().find(|h| h.handle == handle)
    }

    /// 折叠指纹（对账主指纹）。
    pub fn fold(&self) -> u32 {
        let mut ha: u32 = 0x40A40;
        let mut hb: u32 = 0x40A40 ^ 0x9E3779B9;
        for it in &self.handles {
            ha = ha.wrapping_mul(31).wrapping_add(it.handle);
            hb = hb.rotate_left(9) ^ (it.handle).wrapping_add(ha);
            ha = ha.wrapping_mul(31).wrapping_add(it.obj_id);
            hb = hb.rotate_left(9) ^ (it.obj_id).wrapping_add(ha);
            ha = ha.wrapping_mul(31).wrapping_add(it.client);
            hb = hb.rotate_left(9) ^ (it.client).wrapping_add(ha);
            ha = ha.wrapping_mul(31).wrapping_add(it.refcnt);
            hb = hb.rotate_left(9) ^ (it.refcnt).wrapping_add(ha);
        }
        ha ^ hb
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 子功能切面操作（页数对账/封闭回收 通道）。
impl GemTable {
    /// 对象页数查询（野对象回 None）。
    pub fn pages_of(&self, obj_id: u32) -> Option<u32> {
        self.obj_of(obj_id).map(|o| o.pages())
    }

    /// 对象总页数（对账用）。
    pub fn pages_total(&self) -> u32 {
        let mut n = 0u32;
        for o in &self.objs {
            n += o.pages();
        }
        n
    }

    /// 回收：清掉已关闭 handle 与零活引用孤儿对象，回回收条数。
    pub fn gc(&mut self) -> usize {
        let dead: Vec<u32> = self.handles.iter()
            .filter(|h| h.refcnt == 0)
            .map(|h| h.handle)
            .collect();
        self.handles.retain(|h| h.refcnt > 0);
        let mut n = dead.len();
        let orphans: Vec<u32> = self.objs.iter()
            .filter(|o| !self.handles.iter().any(|h| h.obj_id == o.obj_id))
            .map(|o| o.obj_id)
            .collect();
        for oid in orphans {
            self.objs.retain(|o| o.obj_id != oid);
            n += 1;
        }
        n
    }
}

/// 标准样本：两对象（16KiB/4KiB），obj1 开两手柄（客户 7/9）。
pub fn mk_gem() -> GemTable {
    let mut b = GemTable::new();
    let _ = b.create_obj(0x4000);
    let _ = b.create_obj(0x1000);
    let _ = b.alloc_handle(1, 7);
    let _ = b.alloc_handle(1, 9);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_gem();
    // 1) 尺寸非法
    got[0] = b.create_obj(0).err() == Some(GO_BAD_SIZE);
    // 2) 野对象分配
    got[1] = b.alloc_handle(99, 1).err() == Some(GO_BAD_OBJ);
    // 3) 活引用销毁
    got[2] = b.destroy_obj(1).err() == Some(GO_STILL_OPEN);
    // 4) 关闭后取引用
    let _ = b.put_ref(1);
    let _ = b.put_ref(1);
    got[3] = b.get_ref(1).err() == Some(GO_CLOSED);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.create_obj(0x1000).err() == Some(GO_FROZEN);
    got
}

/// 判据主体（13 条 J 判据）。
pub fn run_unx24067_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24067");
    let mut b = mk_gem();

    // J1 账非空
    let j1 = b.objs.len() == 2 && b.handles.len() == 2;
    s.add("J1-对象账齐", j1, "两对象两 handle 全落账");

    // J2 字段齐
    let j2 = b.objs.iter().all(|o| o.size > 0)
        && b.handles.iter().all(|h| h.refcnt > 0);
    s.add("J2-字段齐备", j2, "对象尺寸与引用计数齐");

    // 通道：页数对账
    {
        let mut t = mk_gem();
        let t = mk_gem();
        let m1 = t.pages_of(1) == Some(4) && t.pages_of(2) == Some(1);
        let m2 = t.pages_total() == 5 && t.pages_of(9).is_none();
        let m3 = t.obj_of(1).map(|o| o.size == 0x4000).unwrap_or(false)
            && t.pages_of(1) == t.obj_of(1).map(|o| o.pages());
        s.add("J3-页数对账", m1, "正向语义真实落账");
        s.add("J4-页数对账", m2, "非法入参必拒");
        s.add("J5-页数对账", m3, "复验账目一致");
    }

    // 通道：封闭回收
    {
        let mut t = mk_gem();
        let mut t = mk_gem();
        let m1 = t.gc() == 1 && t.objs.len() == 1 && t.handles.len() == 2;
        let _ = t.put_ref(2);
        let m2 = t.gc() == 1 && t.handle_of(2).is_none() && t.objs.len() == 1;
        let _ = t.put_ref(1);
        let m3 = t.gc() == 2 && t.objs.is_empty() && t.handles.is_empty();
        s.add("J6-封闭回收", m1, "正向语义真实落账");
        s.add("J7-封闭回收", m2, "非法入参必拒");
        s.add("J8-封闭回收", m3, "复验账目一致");
    }

    // J9 引用守恒
    let _ = b.get_ref(1);
    let mut live: u32 = 0;
    for h in &b.handles {
        live += h.refcnt;
    }
    let j9 = b.handle_of(1).map(|h| h.refcnt == 2).unwrap_or(false) && live == 3;
    s.add("J9-引用守恒", j9, "get_ref 计数精确落账");

    // J10 守卫拒
    let j10 = b.get_ref(9).err() == Some(GO_BAD_OBJ)
        && b.put_ref(9).err() == Some(GO_BAD_OBJ)
        && b.destroy_obj(1).err() == Some(GO_STILL_OPEN)
        && b.destroy_obj(9).err() == Some(GO_BAD_OBJ);
    s.add("J10-守卫拒", j10, "野 handle/活引用销毁/野对象全拒");

    // J11 闭集守恒
    let j11 = {
        let mut ok = b.handles.iter().all(|h| h.handle > 0 && h.handle < b.hnd_next);
        for i in 0..b.handles.len() {
            for j in (i + 1)..b.handles.len() {
                if b.handles[i].handle == b.handles[j].handle {
                    ok = false;
                }
            }
        }
        ok
    };
    s.add("J11-handle闭集", j11, "handle 两两相异且落在分配窗内");

    // J12 指纹稳定且敏感
    let f0 = b.fold();
    let _ = b.alloc_handle(2, 33);
    let f1 = b.fold();
    let j12 = b.fold() == f1 && f1 != f0;
    s.add("J12-指纹稳定", j12, "折叠指纹可复现且对 handle 分配敏感");

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
    fn unx24067_flow() {
        let mut b = mk_gem();
        assert!(b.create_obj(0x2000).is_ok());
        assert_eq!(b.create_obj(0).err(), Some(GO_BAD_SIZE));
        assert!(b.alloc_handle(3, 5).is_ok());
        assert_eq!(b.alloc_handle(99, 5).err(), Some(GO_BAD_OBJ));
        assert!(b.get_ref(1).is_ok());
        assert_eq!(b.destroy_obj(1).err(), Some(GO_STILL_OPEN));
        let _ = b.put_ref(1);
        let _ = b.put_ref(1);
        assert_eq!(b.get_ref(1).err(), Some(GO_CLOSED));
        assert!(b.seal() > 0);
        assert_eq!(b.create_obj(0x1000).err(), Some(GO_FROZEN));
    }

    #[test]
    fn unx24067_checks_green() {
        let s = run_unx24067_checks();
        assert!(s.all_passed(), "UNX-F24067 判据未全绿");
    }
}
