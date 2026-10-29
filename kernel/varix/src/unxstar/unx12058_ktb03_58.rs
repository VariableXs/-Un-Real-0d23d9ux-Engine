//! UNX-F12058 PEB 布局静态断言
//! 分段: 0x19960 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：环境块布局契约（64 位）的关键不变量：
//!   - 字段顺序固定，偏移单调递增；
//!   - 指针字段必须 8 字节对齐；
//!   - 总尺寸按 0x1000 窗口约束；
//!   - 布局描述符表在注册后封存；
//!   - 违反对齐 → A58_ALIGN；偏移回退 → A58_ORDER；
//!   - 总尺寸越窗 → A58_WINDOW；重复字段 → A58_DUP；
//!   - 封存后注册 → A58_SEALED。

use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19960;
pub const SEG_A58_ALIGN: u32 = SEG_BASE + 0x01;
pub const SEG_A58_ORDER: u32 = SEG_BASE + 0x02;
pub const SEG_A58_WINDOW: u32 = SEG_BASE + 0x03;
pub const SEG_A58_DUP: u32 = SEG_BASE + 0x04;
pub const SEG_A58_SEALED: u32 = SEG_BASE + 0x05;

pub const LAYOUT_WIN: u32 = 0x1000;
pub const PTR_ALIGN: u32 = 8;

/// 字段布局描述符
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutEntry {
    pub slot: u32,    // 槽号（顺序）
    pub offset: u32,
    pub size: u32,
    pub is_ptr: bool,
    pub sealed: bool,
}

impl LayoutEntry {
    /// 单条校验：对齐 + 越窗
    pub fn validate(&self) -> Result<(), u32> {
        if self.is_ptr && self.offset % PTR_ALIGN != 0 {
            return Err(SEG_A58_ALIGN);
        }
        if self.offset + self.size > LAYOUT_WIN {
            return Err(SEG_A58_WINDOW);
        }
        Ok(())
    }
}

/// 布局描述符表
#[derive(Debug, Clone)]
pub struct LayoutTable {
    pub entries: Vec<LayoutEntry>,
    pub sealed: bool,
}

impl LayoutTable {
    pub fn new() -> LayoutTable {
        LayoutTable { entries: Vec::new(), sealed: false }
    }

    /// 注册：顺序校验（偏移单调）+ 对齐 + 越窗 + 重复槽号
    pub fn add(&mut self, e: LayoutEntry) -> Result<usize, u32> {
        if self.sealed {
            return Err(SEG_A58_SEALED);
        }
        e.validate()?;
        for x in &self.entries {
            if x.slot == e.slot {
                return Err(SEG_A58_DUP);
            }
            if e.offset < x.offset + x.size {
                return Err(SEG_A58_ORDER);
            }
        }
        self.entries.push(e);
        Ok(self.entries.len() - 1)
    }

    /// 端到端布局校验：全部条目 + 首偏移非负 + 顺序单调
    pub fn audit(&self) -> Result<(), u32> {
        let mut prev_end: u32 = 0;
        for e in &self.entries {
            if e.offset < prev_end {
                return Err(SEG_A58_ORDER);
            }
            e.validate()?;
            prev_end = e.offset + e.size;
        }
        Ok(())
    }

    /// 布局总尾
    pub fn tail(&self) -> u32 {
        let mut t = 0;
        for e in &self.entries {
            let end = e.offset + e.size;
            if end > t {
                t = end;
            }
        }
        t
    }

    /// 按槽号查
    pub fn by_slot(&self, slot: u32) -> Option<&LayoutEntry> {
        for e in &self.entries {
            if e.slot == slot {
                return Some(e);
            }
        }
        None
    }

    pub fn seal(&mut self) {
        self.sealed = true;
        for e in &mut self.entries {
            e.sealed = true;
        }
    }
}

/// 事实布局快照（用于对照断言）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutSnap {
    pub slot: u32,
    pub offset: u32,
    pub size: u32,
}

impl LayoutSnap {
    /// 与描述符对照：槽号/偏移/尺寸全符
    pub fn matches(&self, e: &LayoutEntry) -> bool {
        self.slot == e.slot && self.offset == e.offset && self.size == e.size
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(t: &LayoutTable) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    for e in &t.entries {
        h = (h.wrapping_mul(211).wrapping_add(e.slot as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(e.offset as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(e.size as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(e.is_ptr as u64)) & 0xFFFF_FFFF;
    }
    h = (h.wrapping_mul(211).wrapping_add(t.sealed as u64)) & 0xFFFF_FFFF;
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 指针未对齐 → ALIGN
    let e1 = LayoutEntry { slot: 0, offset: 0x4, size: 8, is_ptr: true, sealed: false };
    if e1.validate() == Err(SEG_A58_ALIGN) {
        ok[0] = true;
    }
    // 样2: 偏移回退 → ORDER
    let mut t = LayoutTable::new();
    let _ = t.add(LayoutEntry { slot: 0, offset: 0x10, size: 8, is_ptr: true, sealed: false });
    let e2 = LayoutEntry { slot: 1, offset: 0x14, size: 4, is_ptr: false, sealed: false };
    if t.add(e2) == Err(SEG_A58_ORDER) {
        ok[1] = true;
    }
    // 样3: 越窗 → WINDOW
    let e3 = LayoutEntry { slot: 2, offset: 0xFF8, size: 0x10, is_ptr: false, sealed: false };
    if e3.validate() == Err(SEG_A58_WINDOW) {
        ok[2] = true;
    }
    // 样4: 重复槽号 → DUP
    let mut t2 = LayoutTable::new();
    let _ = t2.add(LayoutEntry { slot: 0, offset: 0x10, size: 8, is_ptr: true, sealed: false });
    let e4 = LayoutEntry { slot: 0, offset: 0x20, size: 8, is_ptr: true, sealed: false };
    if t2.add(e4) == Err(SEG_A58_DUP) {
        ok[3] = true;
    }
    // 样5: 封存后注册 → SEALED
    let mut t3 = LayoutTable::new();
    t3.seal();
    let e5 = LayoutEntry { slot: 3, offset: 0x30, size: 8, is_ptr: true, sealed: false };
    if t3.add(e5) == Err(SEG_A58_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12058_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12058");
    // J1 标准 PEB 头部布局可注册
    let mut t = LayoutTable::new();
    let r0 = t.add(LayoutEntry { slot: 0, offset: 0x0, size: 1, is_ptr: false, sealed: false });
    let r1 = t.add(LayoutEntry { slot: 1, offset: 0x2, size: 1, is_ptr: false, sealed: false });
    let r2 = t.add(LayoutEntry { slot: 2, offset: 0x4, size: 4, is_ptr: false, sealed: false });
    let r3 = t.add(LayoutEntry { slot: 3, offset: 0x8, size: 8, is_ptr: true, sealed: false });
    cs.add("J1", r0.is_ok() && r1.is_ok() && r2.is_ok() && r3.is_ok(), "standard head");
    // J2 audit 全绿
    cs.add("J2", t.audit().is_ok(), "audit ok");
    // J3 尾部 0x10
    cs.add("J3", t.tail() == 0x10, "tail 0x10");
    // J4 指针对齐拒绝
    cs.add("J4", LayoutEntry { slot: 4, offset: 0x12, size: 8, is_ptr: true, sealed: false }
        .validate().err() == Some(SEG_A58_ALIGN), "ptr unaligned");
    // J5 非指针不受对齐约束
    cs.add("J5", LayoutEntry { slot: 4, offset: 0x11, size: 1, is_ptr: false, sealed: false }
        .validate().is_ok(), "byte ok");
    // J6 越窗
    cs.add("J6", LayoutEntry { slot: 4, offset: 0xFF0, size: 0x20, is_ptr: false, sealed: false }
        .validate().err() == Some(SEG_A58_WINDOW), "oob window");
    // J7 恰好贴窗合法
    cs.add("J7", LayoutEntry { slot: 4, offset: 0xFF0, size: 0x10, is_ptr: false, sealed: false }
        .validate().is_ok(), "flush window ok");
    // J8 重复槽号
    cs.add("J8", t.add(LayoutEntry { slot: 2, offset: 0x20, size: 4, is_ptr: false, sealed: false })
        .err() == Some(SEG_A58_DUP), "dup slot");
    // J9 偏移回退
    cs.add("J9", t.add(LayoutEntry { slot: 5, offset: 0x6, size: 2, is_ptr: false, sealed: false })
        .err() == Some(SEG_A58_ORDER), "order back");
    // J10 与前一区重叠
    cs.add("J10", t.add(LayoutEntry { slot: 6, offset: 0xC, size: 4, is_ptr: false, sealed: false })
        .err() == Some(SEG_A58_ORDER), "overlap prev");
    // J11 按槽查询
    let e = t.by_slot(3).unwrap_or(&LayoutEntry { slot: 0, offset: 0, size: 0, is_ptr: false, sealed: false });
    cs.add("J11", e.offset == 0x8 && e.is_ptr, "by slot 3");
    // J12 快照对照命中
    let snap = LayoutSnap { slot: 3, offset: 0x8, size: 8 };
    let e3 = t.by_slot(3).unwrap_or(&LayoutEntry { slot: 0, offset: 0, size: 0, is_ptr: false, sealed: false });
    cs.add("J12", snap.matches(e3), "snap match");
    // J13 快照对照失配
    let snap2 = LayoutSnap { slot: 3, offset: 0x10, size: 8 };
    let e3b = t.by_slot(3).unwrap_or(&LayoutEntry { slot: 0, offset: 0, size: 0, is_ptr: false, sealed: false });
    cs.add("J13", !snap2.matches(e3b), "snap mismatch");
    // J14 封存传播
    let mut t2 = LayoutTable::new();
    let _ = t2.add(LayoutEntry { slot: 0, offset: 0x0, size: 8, is_ptr: true, sealed: false });
    t2.seal();
    cs.add("J14", t2.entries.iter().all(|e| e.sealed), "seal propagate");
    // J15 封存后注册
    cs.add("J15", t2.add(LayoutEntry { slot: 9, offset: 0x40, size: 8, is_ptr: true, sealed: false })
        .err() == Some(SEG_A58_SEALED), "sealed add");
    // J16 注入五样
    let inj = inject_five();
    cs.add("J16", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_matrix() {
        let mut off = 0;
        while off <= 0x20 {
            let e = LayoutEntry { slot: 0, offset: off, size: 8, is_ptr: true, sealed: false };
            if off % 8 == 0 {
                assert!(e.validate().is_ok());
            } else {
                assert_eq!(e.validate().err(), Some(SEG_A58_ALIGN));
            }
            off += 1;
        }
    }

    #[test]
    fn test_order_enforced() {
        let mut t = LayoutTable::new();
        let _ = t.add(LayoutEntry { slot: 0, offset: 0x10, size: 8, is_ptr: true, sealed: false });
        let _ = t.add(LayoutEntry { slot: 1, offset: 0x18, size: 8, is_ptr: true, sealed: false });
        assert_eq!(t.add(LayoutEntry { slot: 2, offset: 0x1C, size: 8, is_ptr: false, sealed: false }).err(), Some(SEG_A58_ORDER));
        assert!(t.add(LayoutEntry { slot: 2, offset: 0x20, size: 8, is_ptr: true, sealed: false }).is_ok());
    }

    #[test]
    fn test_window_edges() {
        // 贴窗合法 / 半字节越窗拒的双样面。
        let flush = LayoutEntry { slot: 7, offset: 0xFF0, size: 0x10, is_ptr: false, sealed: false };
        let over = LayoutEntry { slot: 8, offset: 0xFF1, size: 0x10, is_ptr: false, sealed: false };
        assert!(flush.validate().is_ok());
        assert_eq!(over.validate().err(), Some(SEG_A58_WINDOW));
    }

    #[test]
    fn test_fold_stable() {
        let mut t = LayoutTable::new();
        let _ = t.add(LayoutEntry { slot: 0, offset: 0, size: 8, is_ptr: true, sealed: false });
        let a = fold(&t);
        let b = fold(&t);
        assert_eq!(a, b);
        assert_ne!(a, SEG_BASE);
    }

    #[test]
    fn test_snap_match() {
        let mut t = LayoutTable::new();
        let _ = t.add(LayoutEntry { slot: 0, offset: 0x8, size: 8, is_ptr: true, sealed: false });
        let e = t.by_slot(0).unwrap_or(&LayoutEntry { slot: 0, offset: 0, size: 0, is_ptr: false, sealed: false });
        let hit = LayoutSnap { slot: 0, offset: 0x8, size: 8 };
        let miss = LayoutSnap { slot: 0, offset: 0x10, size: 8 };
        assert!(hit.matches(e));
        assert!(!miss.matches(e));
        assert_eq!(t.tail(), 0x10);
    }
}
