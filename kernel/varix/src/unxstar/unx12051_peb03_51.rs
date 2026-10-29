//! UNX-F12051 PEB GdiHandleBuffer 图形句柄缓存槽
//! 分段: 0x19880 批次: B03 领域: PEB
//! 验收: 折叠指纹 + 封存 + 注入五样 + 12~20 条判据全绿
//!
//! 语义模型：GdiHandleBuffer 是环境块中承载 GDI 句柄表的
//! 进程内缓存指针。本卡以"槽位表"建模：
//!   - 表容量固定（进程类型决定 0x30 / 0x80 槽）；
//!   - 每槽记录 { 类型(笔/刷/位图/区域), 计数 }；
//!   - 句柄值 = 槽位索引 << 2 | 类型标记；
//!   - 越界索引、类型不匹配、槽满均为语义错误；
//!   - 封存后禁止再分配。

use crate::alloc::format;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x19880;
pub const SEG_GDI_RANGE: u32 = SEG_BASE + 0x01;
pub const SEG_GDI_TYPE: u32 = SEG_BASE + 0x02;
pub const SEG_GDI_FULL: u32 = SEG_BASE + 0x03;
pub const SEG_GDI_SEALED: u32 = SEG_BASE + 0x04;
pub const SEG_GDI_STALE: u32 = SEG_BASE + 0x05;

/// GDI 对象类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GdiKind {
    Pen = 1,
    Brush = 2,
    Bitmap = 3,
    Region = 4,
}

impl GdiKind {
    pub fn tag(self) -> u32 {
        self as u32
    }

    pub fn from_tag(t: u32) -> Option<GdiKind> {
        match t {
            1 => Some(GdiKind::Pen),
            2 => Some(GdiKind::Brush),
            3 => Some(GdiKind::Bitmap),
            4 => Some(GdiKind::Region),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GdiKind::Pen => "PEN",
            GdiKind::Brush => "BRUSH",
            GdiKind::Bitmap => "BMP",
            GdiKind::Region => "RGN",
        }
    }
}

/// 单槽记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdiSlot {
    pub index: u32,
    pub kind: GdiKind,
    pub used: bool,
    pub sealed: bool,
}

/// 句柄编解码：高 29 位为槽索引，低 3 位为类型标记（1..4）
pub fn encode_handle(index: u32, kind: GdiKind) -> u32 {
    (index << 3) | kind.tag()
}

pub fn decode_handle(h: u32) -> Result<(u32, GdiKind), u32> {
    let idx = h >> 3;
    let t = h & 0x7;
    let k = GdiKind::from_tag(t).ok_or(SEG_GDI_TYPE)?;
    Ok((idx, k))
}

/// 槽位表
#[derive(Debug, Clone)]
pub struct GdiBuffer {
    pub capacity: u32,
    pub slots: Vec<GdiSlot>,
    pub sealed: bool,
}

impl GdiBuffer {
    /// 以容量建表（0x30 或 0x80）
    pub fn new(capacity: u32) -> GdiBuffer {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < capacity {
            slots.push(GdiSlot {
                index: i,
                kind: GdiKind::Pen, // 占位类型，分配时覆盖
                used: false,
                sealed: false,
            });
            i += 1;
        }
        GdiBuffer { capacity, slots, sealed: false }
    }

    /// 分配槽：越界 → RANGE；封存 → SEALED；无空槽 → FULL
    pub fn alloc(&mut self, kind: GdiKind) -> Result<u32, u32> {
        if self.sealed {
            return Err(SEG_GDI_SEALED);
        }
        let mut found: Option<u32> = None;
        for s in &self.slots {
            if !s.used {
                found = Some(s.index);
                break;
            }
        }
        let idx = match found {
            Some(i) => i,
            None => return Err(SEG_GDI_FULL),
        };
        if idx >= self.capacity {
            return Err(SEG_GDI_RANGE);
        }
        self.slots[idx as usize].used = true;
        self.slots[idx as usize].kind = kind;
        Ok(encode_handle(idx, kind))
    }

    /// 释放槽：句柄类型与槽类型不符 → TYPE；未用槽 → STALE；越界 → RANGE
    pub fn free(&mut self, h: u32) -> Result<(), u32> {
        let (idx, k) = decode_handle(h)?;
        if idx >= self.capacity {
            return Err(SEG_GDI_RANGE);
        }
        let slot = &self.slots[idx as usize];
        if !slot.used {
            return Err(SEG_GDI_STALE);
        }
        if slot.kind != k {
            return Err(SEG_GDI_TYPE);
        }
        self.slots[idx as usize].used = false;
        Ok(())
    }

    /// 在用槽计数
    pub fn used_count(&self) -> u32 {
        self.slots.iter().filter(|s| s.used).count() as u32
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 折叠指纹：211 进制卷积，种子 SEG_BASE
pub fn fold(buf: &GdiBuffer) -> u32 {
    let mut h: u64 = SEG_BASE as u64;
    h = (h.wrapping_mul(211).wrapping_add(buf.capacity as u64)) & 0xFFFF_FFFF;
    h = (h.wrapping_mul(211).wrapping_add(buf.sealed as u64)) & 0xFFFF_FFFF;
    for s in &buf.slots {
        h = (h.wrapping_mul(211).wrapping_add(s.used as u64)) & 0xFFFF_FFFF;
        h = (h.wrapping_mul(211).wrapping_add(s.kind as u64)) & 0xFFFF_FFFF;
    }
    h as u32
}

/// 注入五样：每个坏样命中一个独立错误分支
pub fn inject_five() -> [bool; 5] {
    let mut ok = [false; 5];
    // 样1: 类型标记 0 非法 → TYPE
    if decode_handle(0x8).err() == Some(SEG_GDI_TYPE) {
        ok[0] = true;
    }
    // 样2: 越界释放 → RANGE
    let mut b = GdiBuffer::new(0x30);
    if b.free(encode_handle(0xFF, GdiKind::Pen)).err() == Some(SEG_GDI_RANGE) {
        ok[1] = true;
    }
    // 样3: 槽满 → FULL
    let mut b2 = GdiBuffer::new(2);
    let _ = b2.alloc(GdiKind::Pen);
    let _ = b2.alloc(GdiKind::Brush);
    if b2.alloc(GdiKind::Bitmap) == Err(SEG_GDI_FULL) {
        ok[2] = true;
    }
    // 样4: 双重释放 → STALE
    let mut b3 = GdiBuffer::new(0x30);
    let h = b3.alloc(GdiKind::Bitmap).unwrap_or(0);
    let _ = b3.free(h);
    if b3.free(h) == Err(SEG_GDI_STALE) {
        ok[3] = true;
    }
    // 样5: 封存后分配 → SEALED
    let mut b4 = GdiBuffer::new(0x30);
    b4.seal();
    if b4.alloc(GdiKind::Pen) == Err(SEG_GDI_SEALED) {
        ok[4] = true;
    }
    ok
}

/// 自检：12 条以上判据
pub fn run_unx12051_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F12051");
    // J1 编解码往返
    let h = encode_handle(5, GdiKind::Brush);
    cs.add("J1", decode_handle(h) == Ok((5, GdiKind::Brush)), "roundtrip brush");
    // J2 四种类型编解码
    let mut all = true;
    let mut t = 1;
    while t <= 4 {
        let k = GdiKind::from_tag(t).unwrap_or(GdiKind::Pen);
        all = all && decode_handle(encode_handle(9, k)) == Ok((9, k));
        t += 1;
    }
    cs.add("J2", all, "all kinds roundtrip");
    // J3 非法类型标记（tag 5 越闭集）
    cs.add("J3", decode_handle(0x5).err() == Some(SEG_GDI_TYPE), "bad tag");
    // J4 分配递增占槽
    let mut b = GdiBuffer::new(0x30);
    let h1 = b.alloc(GdiKind::Pen).unwrap_or(0xFFFF);
    let h2 = b.alloc(GdiKind::Brush).unwrap_or(0xFFFF);
    cs.add("J4", b.used_count() == 2 && h1 != h2, "two allocs");
    // J5 释放后复用同槽
    let _ = b.free(h1);
    let h3 = b.alloc(GdiKind::Bitmap).unwrap_or(0xFFFF);
    cs.add("J5", decode_handle(h3).unwrap_or((0, GdiKind::Pen)).0
        == decode_handle(h1).unwrap_or((9, GdiKind::Pen)).0, "slot reuse");
    // J6 释放类型不符 → TYPE
    let mut b2 = GdiBuffer::new(0x30);
    let ha = b2.alloc(GdiKind::Region).unwrap_or(0);
    let wrong = encode_handle(decode_handle(ha).unwrap_or((0, GdiKind::Pen)).0, GdiKind::Pen);
    cs.add("J6", b2.free(wrong).err() == Some(SEG_GDI_TYPE), "type mismatch");
    // J7 双重释放 → STALE
    let mut b3 = GdiBuffer::new(0x30);
    let hb = b3.alloc(GdiKind::Pen).unwrap_or(0);
    let _ = b3.free(hb);
    cs.add("J7", b3.free(hb).err() == Some(SEG_GDI_STALE), "stale free");
    // J8 越界索引 → RANGE
    cs.add("J8", GdiBuffer::new(0x30).free(encode_handle(0x30, GdiKind::Pen))
        .err() == Some(SEG_GDI_RANGE), "oob range");
    // J9 容量 0x30
    cs.add("J9", GdiBuffer::new(0x30).slots.len() == 0x30, "cap 0x30");
    // J10 槽满检测（容量 2 小表）
    let mut small = GdiBuffer::new(2);
    let _ = small.alloc(GdiKind::Pen);
    let _ = small.alloc(GdiKind::Brush);
    cs.add("J10", small.alloc(GdiKind::Bitmap).err() == Some(SEG_GDI_FULL), "table full");
    // J11 封存后分配 → SEALED
    let mut b4 = GdiBuffer::new(0x30);
    b4.seal();
    cs.add("J11", b4.alloc(GdiKind::Pen).err() == Some(SEG_GDI_SEALED), "sealed alloc");
    // J12 标签文本
    cs.add("J12", GdiKind::Bitmap.label() == "BMP", "kind label");
    // J13 注入五样
    let inj = inject_five();
    cs.add("J13", inj.iter().all(|&b| b), "inject five");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        for t in 1..=4u32 {
            let k = GdiKind::from_tag(t).unwrap_or(GdiKind::Pen);
            let h = encode_handle(0x11, k);
            assert_eq!(decode_handle(h), Ok((0x11, k)));
        }
    }

    #[test]
    fn test_alloc_free_cycle() {
        let mut b = GdiBuffer::new(0x80);
        let h = b.alloc(GdiKind::Region).ok().unwrap_or(0);
        assert_eq!(b.used_count(), 1);
        assert!(b.free(h).is_ok());
        assert_eq!(b.used_count(), 0);
    }

    #[test]
    fn test_fold_stable() {
        let mut b = GdiBuffer::new(0x30);
        let _ = b.alloc(GdiKind::Brush);
        let a = fold(&b);
        let c = fold(&b);
        assert_eq!(a, c);
        assert_ne!(a, SEG_BASE);
    }
}
