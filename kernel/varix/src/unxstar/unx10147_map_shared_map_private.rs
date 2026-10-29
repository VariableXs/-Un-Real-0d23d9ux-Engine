//! UNX 卡 10147 — MAP_SHARED/MAP_PRIVATE 正反双判据（共享隔离达成与组合非法误用受控）
//! 段位 seg=0x10580
//! 判据原文：UNX-F10147-J1 共享隔离达成（SHARED 双进程可见/PRIVATE COW 隔离 10/10 次）
//! 锚点：SHARED 跨进程写可见 + PRIVATE 写时复制隔离 + 双标志组合非法受控
//! 域：ShareMap 双视图册（共享页池 + 私有 COW 副本层）
//! 结构：SHARED 共享页池 → PRIVATE COW 副本 → 组合/误用受控 → 隔离审计与注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10580;
pub const SM_BAD_COMBO: u32 = SEG_BASE + 0x01;
pub const SM_UNBACKED: u32 = SEG_BASE + 0x02;
pub const SM_BAD_TYPE: u32 = SEG_BASE + 0x03;
pub const SM_SEALED: u32 = SEG_BASE + 0x04;
pub const SM_POOL_FULL: u32 = SEG_BASE + 0x05;

/// 页粒度与共享页池容量
pub const PAGE: usize = 0x1000;
pub const POOL_CAP: usize = 0x20;

/// 映射类型
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MapType {
    Shared,
    Private,
}

/// 一个映射视图：进程 + 页池槽 + 类型
#[derive(Clone, Debug)]
pub struct View {
    pub pid: u32,
    pub slot: usize,
    pub typ: MapType,
    /// PRIVATE 视图的 COW 副本（未写时为 None）
    pub cow: Option<[u8; 8]>,
    /// 该视图已发生私有写
    pub dirty: bool,
}

/// 共享/私有双视图册
pub struct ShareMap {
    /// 共享页池：槽 → 8 字节页内容指纹
    pool: Vec<Option<[u8; 8]>>,
    views: Vec<View>,
    sealed: bool,
}

impl ShareMap {
    pub fn new() -> Self {
        let mut pool = Vec::new();
        for _ in 0..POOL_CAP {
            pool.push(None);
        }
        ShareMap {
            pool,
            views: Vec::new(),
            sealed: false,
        }
    }

    pub fn views(&self) -> &[View] {
        &self.views
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 解析类型：SHARED=1 / PRIVATE=2，其他值非法；两值同时给出为组合非法
    pub fn parse_type(shared: bool, private: bool) -> Result<MapType, u32> {
        if shared && private {
            return Err(SM_BAD_COMBO);
        }
        if !shared && !private {
            return Err(SM_BAD_TYPE);
        }
        if shared {
            Ok(MapType::Shared)
        } else {
            Ok(MapType::Private)
        }
    }

    /// 建映射：SHARED 建池槽（空页），PRIVATE 不建池槽（首写时才 COW）
    pub fn map(&mut self, pid: u32, typ: MapType, seed: u8) -> Result<usize, u32> {
        if self.sealed {
            return Err(SM_SEALED);
        }
        let slot = match typ {
            MapType::Shared => {
                // 同种子即同一共享对象：挂既有槽实现跨进程互见；无则取空新槽
                if let Some(idx) = self.pool.iter().position(|p| *p == Some([seed; 8])) {
                    idx
                } else {
                    if self.pool.iter().all(|p| p.is_some()) {
                        return Err(SM_POOL_FULL);
                    }
                    let idx = self.pool.iter().position(|p| p.is_none()).unwrap();
                    self.pool[idx] = Some([seed; 8]);
                    idx
                }
            }
            MapType::Private => {
                if self.views.len() >= POOL_CAP {
                    return Err(SM_POOL_FULL);
                }
                usize::MAX
            }
        };
        self.views.push(View {
            pid,
            slot,
            typ,
            cow: None,
            dirty: false,
        });
        Ok(self.views.len() - 1)
    }

    /// 读：SHARED 读池；PRIVATE 未写读池？——否，PRIVATE 建时不入池，读自身副本或零页
    pub fn read(&self, vid: usize) -> Result<[u8; 8], u32> {
        let v = self.views.get(vid).ok_or(SM_UNBACKED)?;
        match v.typ {
            MapType::Shared => self.pool[v.slot].ok_or(SM_UNBACKED),
            MapType::Private => Ok(v.cow.unwrap_or([0; 8])),
        }
    }

    /// 写：SHARED 直写池（双进程可见）；PRIVATE 首写触发 COW 复制
    pub fn write(&mut self, vid: usize, data: [u8; 8]) -> Result<(), u32> {
        if self.sealed {
            return Err(SM_SEALED);
        }
        let v = self.views.get_mut(vid).ok_or(SM_UNBACKED)?;
        match v.typ {
            MapType::Shared => {
                self.pool[v.slot] = Some(data);
                v.dirty = true;
                Ok(())
            }
            MapType::Private => {
                v.cow = Some(data);
                v.dirty = true;
                Ok(())
            }
        }
    }

    /// 共享可见性：两 SHARED 视图同池同内容
    pub fn shared_visible(&self, a: usize, b: usize) -> bool {
        match (self.views.get(a), self.views.get(b)) {
            (Some(x), Some(y)) => {
                x.typ == MapType::Shared
                    && y.typ == MapType::Shared
                    && x.slot == y.slot
                    && self.read(a) == self.read(b)
            }
            _ => false,
        }
    }

    /// 私有隔离：PRIVATE 写后不影响任何 SHARED 视图
    pub fn private_isolated(&self, pv: usize, sh: usize) -> bool {
        match (self.views.get(pv), self.views.get(sh)) {
            (Some(p), Some(s)) => {
                p.typ == MapType::Private
                    && p.dirty
                    && s.typ == MapType::Shared
                    && self.read(pv) != self.read(sh)
            }
            _ => false,
        }
    }

    /// COW 审计：PRIVATE 视图未写时读零页
    pub fn cow_audit(&self, vid: usize) -> bool {
        match self.views.get(vid) {
            Some(v) => v.typ == MapType::Private && !v.dirty && self.read(vid) == Ok([0; 8]),
            None => false,
        }
    }

    /// 脏视图计数
    pub fn dirty_count(&self) -> usize {
        self.views.iter().filter(|v| v.dirty).count()
    }

    /// 池占用
    pub fn pool_used(&self) -> usize {
        self.pool.iter().filter(|p| p.is_some()).count()
    }

    /// 类型指纹
    pub fn render_types(&self) -> String {
        let mut s = String::new();
        for v in &self.views {
            let t = match v.typ {
                MapType::Shared => "S",
                MapType::Private => "P",
            };
            s.push_str(&format!("{}{};", v.pid, t));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = ShareMap::new();
    // 1) SHARED|PRIVATE 组合必拒
    got[0] = ShareMap::parse_type(true, true).is_err();
    // 2) 无类型必拒
    got[1] = ShareMap::parse_type(false, false).is_err();
    // 3) 读未映射视图必拒
    got[2] = m.read(9).is_err();
    // 4) 写未映射视图必拒
    got[3] = m.write(9, [1; 8]).is_err();
    // 5) PRIVATE 写不得进共享池（池占用不变）
    let _ = m.map(1, MapType::Shared, 7);
    let used = m.pool_used();
    let _ = m.map(2, MapType::Private, 7);
    let _ = m.write(1, [9; 8]);
    got[4] = m.pool_used() == used;
    got
}

/// 判据入口：UNX-F10147
pub fn run_unx10147_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10147");
    let mut m = ShareMap::new();

    // J1 共享可见性 10/10：10 轮双 SHARED 互见
    let mut j1 = true;
    for i in 0..10u32 {
        let a = m.map(1, MapType::Shared, (i as u8) + 1);
        let b = m.map(2, MapType::Shared, (i as u8) + 1);
        if a.is_err() || b.is_err() {
            j1 = false;
            break;
        }
        let va = a.unwrap();
        let vb = b.unwrap();
        if m.write(va, [i as u8 + 1; 8]).is_err() {
            j1 = false;
        }
        if !m.shared_visible(va, vb) {
            j1 = false;
        }
    }
    s.add("J1_shared_10x", j1, "SHARED 双进程互见 10/10");

    // J2 PRIVATE COW 隔离 10/10
    let mut m2 = ShareMap::new();
    let base = m2.map(1, MapType::Shared, 0xAA);
    let mut j2 = true;
    for i in 0..10u32 {
        let pv = m2.map(i + 3, MapType::Private, 0);
        if pv.is_err() {
            j2 = false;
            break;
        }
        let pv = pv.unwrap();
        if !m2.cow_audit(pv) {
            j2 = false;
        }
        if m2.write(pv, [i as u8 + 1; 8]).is_err() {
            j2 = false;
        }
        if !m2.private_isolated(pv, base.unwrap()) {
            j2 = false;
        }
    }
    s.add("J2_cow_10x", j2, "PRIVATE COW 隔离 10/10");

    // J3 组合非法受控
    let j3 = ShareMap::parse_type(true, true) == Err(SM_BAD_COMBO)
        && ShareMap::parse_type(false, false) == Err(SM_BAD_TYPE);
    s.add("J3_bad_combo", j3, "SHARED|PRIVATE 组合与无类型双受控");

    // J4 未背书读写受控
    let mut m3 = ShareMap::new();
    let j4 = m3.read(0) == Err(SM_UNBACKED) && m3.write(0, [0; 8]) == Err(SM_UNBACKED);
    s.add("J4_unbacked", j4, "空册读写受控 UNBACKED");

    // J5 首写前 COW 零页
    let mut m4 = ShareMap::new();
    let pv = m4.map(5, MapType::Private, 0).unwrap();
    s.add("J5_cow_zero", m4.read(pv) == Ok([0; 8]), "PRIVATE 首写前读零页");

    // J6 写后副本生效
    let _ = m4.write(pv, [0x5A; 8]);
    s.add("J6_cow_copy", m4.read(pv) == Ok([0x5A; 8]), "首写后副本生效");

    // J7 脏视图计数对平
    let dc = m4.dirty_count();
    s.add("J7_dirty", dc == 1, "脏视图计数 1/1 对平");

    // J8 池占用守恒
    let used = m.pool_used();
    s.add("J8_pool", used == 10, "共享池占用 10 槽与 J1 对平");

    // J9 SHARED 写不建新槽（改写既有槽）
    let slots: Vec<usize> = m.views().iter().map(|v| v.slot).collect();
    s.add("J9_slots", slots.len() == 20 && m.pool_used() == 10, "20 视图仅占 10 槽（两两同槽）");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封账受控
    let mut m5 = ShareMap::new();
    m5.seal();
    let j11 = m5.map(1, MapType::Shared, 1) == Err(SM_SEALED)
        && m5.map(1, MapType::Private, 1) == Err(SM_SEALED)
        && m5.write(0, [0; 8]) == Err(SM_SEALED);
    s.add("J11_sealed", j11, "封账后映射与写皆拒 SEALED");

    // J12 池满受控
    let mut m6 = ShareMap::new();
    let mut filled = true;
    for i in 0..POOL_CAP {
        if m6.map(1, MapType::Shared, i as u8).is_err() {
            filled = false;
        }
    }
    let j12 = filled && m6.map(1, MapType::Shared, 0xFF) == Err(SM_POOL_FULL);
    s.add("J12_pool_full", j12, "池满 32 槽后异种子拒 POOL_FULL（同种子仍共槽）");

    // J13 类型指纹稳定
    let fp = m.render_types();
    let fp2 = m.render_types();
    s.add("J13_fingerprint", fp == fp2 && fp.contains("1S;") && fp.contains("2S;"), "类型指纹稳定含 1S/2S");

    // J14 同槽对数守恒：10 槽 × 每 2 视图
    let mut pair_ok = true;
    for i in 0..10usize {
        if !m.shared_visible(i * 2, i * 2 + 1) {
            pair_ok = false;
        }
    }
    s.add("J14_pairs", pair_ok, "10 对共享视图逐对互见");

    // J15 混合隔离：PRIVATE 写不影响最末 SHARED 对
    let last = m.map(3, MapType::Private, 0).unwrap();
    let _ = m.write(last, [0xEE; 8]);
    let j15 = m.private_isolated(last, 0) && m.shared_visible(0, 1);
    s.add("J15_mixed", j15, "PRIVATE 写不扩散至 SHARED 视图");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_share47_map() {
        let mut m = ShareMap::new();
        let a = m.map(1, MapType::Shared, 7).unwrap();
        let b = m.map(2, MapType::Shared, 0).unwrap();
        m.write(a, [3; 8]).unwrap();
        assert_eq!(m.read(b), Ok([3; 8]));
        assert!(m.shared_visible(a, b));
    }

    #[test]
    fn test_share47_cow() {
        let mut m = ShareMap::new();
        let p = m.map(1, MapType::Private, 9).unwrap();
        assert_eq!(m.read(p), Ok([0; 8]));
        m.write(p, [1; 8]).unwrap();
        assert_eq!(m.read(p), Ok([1; 8]));
    }

    #[test]
    fn test_share47_checks() {
        let cs = run_unx10147_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_share47_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
