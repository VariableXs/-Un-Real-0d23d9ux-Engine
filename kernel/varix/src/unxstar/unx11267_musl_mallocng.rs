//! UNX-F11267 musl mallocng 内核消费面（0x1f430 段）
//! 纯功能模型：mallocng 的两条内核消费路径——小组走 brk 顺延、大组走 mmap
//! 独立池；逐笔登记块（号/地址/长度/来源/存活位），释放按号回收；
//! 压力账要求零重叠、零泄漏、提交量与两池用量逐字节对平。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段（0x1f430 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f430;
/// 请求长度 0 或超上限。
pub const MG_SIZE: u32 = SEG_BASE + 0x01;
/// 对齐非 2 幂或越出 8..=4096。
pub const MG_ALIGN: u32 = SEG_BASE + 0x02;
/// 块号重登。
pub const MG_DUP: u32 = SEG_BASE + 0x03;
/// 释放未登块或重复释放（泄漏账）。
pub const MG_LEAK: u32 = SEG_BASE + 0x04;
/// 来源与尺寸类别不符（小组必须 brk、大组必须 mmap）。
pub const MG_SRC: u32 = SEG_BASE + 0x05;
/// 封册后写账。
pub const MG_FROZEN: u32 = SEG_BASE + 0x06;
/// 块表已满。
pub const MG_CAP: u32 = SEG_BASE + 0x07;

/// 来源闭集。
pub const SRC_BRK: u8 = 1;
pub const SRC_MMAP: u8 = 2;
/// mmap 阈值（128K，mallocng 大组界线）。
pub const MMAP_THRESHOLD: u32 = 0x2_0000;
/// 单次请求上限（8M）。
pub const REQ_MAX: u32 = 0x80_0000;
/// 对齐下限/上限。
pub const ALIGN_LO: u32 = 8;
pub const ALIGN_HI: u32 = 4096;
/// 块表定容。
pub const BLK_CAP: usize = 128;
/// brk 池起点与 mmap 池起点。
pub const BRK_BASE: u64 = 0x0060_0000;
pub const MMAP_BASE: u64 = 0x7f00_0000_0000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_67;

/// 单块：号/起始/长度/来源/存活。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blk {
    pub id: u32,
    pub start: u64,
    pub size: u32,
    pub src: u8,
    pub live: bool,
}

impl Blk {
    pub fn end(&self) -> u64 {
        self.start.wrapping_add(self.size as u64)
    }

    pub fn render(&self) -> String {
        let s = if self.src == SRC_BRK { "B" } else { "M" };
        let l = if self.live { "*" } else { "." };
        format!("#{} {}{}@{:x}+{:x}", self.id, s, l, self.start, self.size)
    }
}

/// mallocng 消费账：两池 + 块表。
#[derive(Clone, Debug)]
pub struct Heap {
    pub brk_top: u64,
    pub mmap_top: u64,
    pub brk_used: u32,
    pub mmap_used: u32,
    pub committed: u32,
    pub blocks: Vec<Blk>,
    pub accepts: u32,
    pub rejects: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            brk_top: BRK_BASE,
            mmap_top: MMAP_BASE,
            brk_used: 0,
            mmap_used: 0,
            committed: 0,
            blocks: Vec::new(),
            accepts: 0,
            rejects: 0,
            frozen: false,
            root: None,
        }
    }

    /// 申请一块。判定序：封册 → 号域 → 长度 → 对齐 → 重号 → 定容。
    pub fn alloc(&mut self, id: u32, size: u32, align: u32) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_FROZEN);
        }
        if id == 0 || id > 9999 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_ID);
        }
        if size == 0 || size > REQ_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_SIZE);
        }
        if align < ALIGN_LO || align > ALIGN_HI || align & align.wrapping_sub(1) != 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_ALIGN);
        }
        if self.blocks.iter().any(|b| b.id == id) {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_DUP);
        }
        if self.blocks.len() >= BLK_CAP {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_CAP);
        }
        let rounded = round_up(size, align);
        let (src, start) = if rounded >= MMAP_THRESHOLD {
            let s = self.mmap_top;
            self.mmap_top = s.wrapping_add(round_up(rounded, 4096) as u64);
            self.mmap_used = self.mmap_used.wrapping_add(rounded);
            (SRC_MMAP, s)
        } else {
            let s = align_up(self.brk_top, align as u64);
            self.brk_top = s.wrapping_add(rounded as u64);
            self.brk_used = self.brk_used.wrapping_add(rounded);
            (SRC_BRK, s)
        };
        self.committed = self.committed.wrapping_add(rounded);
        self.blocks.push(Blk { id, start, size: rounded, src, live: true });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(start)
    }

    /// 释放一块（未登号或重复释放按泄漏账入账）。
    pub fn free(&mut self, id: u32) -> Result<u32, u32> {
        if self.frozen {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_FROZEN);
        }
        let mut idx: Option<usize> = None;
        let mut sz = 0u32;
        let mut k = 0usize;
        while k < self.blocks.len() {
            if let Some(b) = self.blocks.get(k) {
                if b.id == id {
                    idx = Some(k);
                    sz = b.size;
                }
            }
            k = k.wrapping_add(1);
        }
        let at = match idx {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(MG_LEAK);
            }
        };
        let live = match self.blocks.get(at) {
            Some(b) => b.live,
            None => false,
        };
        if !live {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_LEAK);
        }
        if let Some(slot) = self.blocks.get_mut(at) {
            slot.live = false;
        }
        self.accepts = self.accepts.wrapping_add(1);
        Ok(sz)
    }

    /// 存活块数。
    pub fn live_count(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.blocks.len() {
            if let Some(b) = self.blocks.get(k) {
                if b.live {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 账实对平：提交量 = 两池之和；存活块两两不重叠且按 8 字节对齐。
    pub fn audit(&self) -> bool {
        if self.committed != self.brk_used.wrapping_add(self.mmap_used) {
            return false;
        }
        let mut a = 0usize;
        while a < self.blocks.len() {
            let ba = match self.blocks.get(a) {
                Some(v) => v,
                None => return false,
            };
            if !ba.live {
                a = a.wrapping_add(1);
                continue;
            }
            if ba.start & 7 != 0 {
                return false;
            }
            let mut b = a.wrapping_add(1);
            while b < self.blocks.len() {
                if let Some(bb) = self.blocks.get(b) {
                    if bb.live && ba.start < bb.end() && bb.start < ba.end() {
                        return false;
                    }
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        true
    }

    /// 钉定来源的申请：来源与尺寸类别不符即拒（mallocng 只按尺寸选路）。
    pub fn alloc_pinned(&mut self, id: u32, size: u32, align: u32, src: u8) -> Result<u64, u32> {
        let rounded = round_up(size.max(1), if align == 0 { 8 } else { align });
        let want = if rounded >= MMAP_THRESHOLD { SRC_MMAP } else { SRC_BRK };
        if src != want {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(MG_SRC);
        }
        self.alloc(id, size, align)
    }

    /// FNV-1a 链指纹：两池游标/用量/块序/存活位敏感。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.brk_top).wrapping_mul(FNV_PRIME);
        h = (h ^ self.mmap_top).wrapping_mul(FNV_PRIME);
        h = (h ^ self.brk_used as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.mmap_used as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.committed as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.blocks.len() {
            if let Some(b) = self.blocks.get(k) {
                h = (h ^ b.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.start).wrapping_mul(FNV_PRIME);
                h = (h ^ b.size as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.src as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.live as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 封册：根幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.root;
        }
        let r = match self.root {
            Some(v) => v,
            None => self.fold(),
        };
        self.root = Some(r);
        self.frozen = true;
        Some(r)
    }
}

/// 向上取整到 2 幂对齐（align 已校验为 2 幂）。
fn round_up(n: u32, align: u32) -> u32 {
    let a = align.wrapping_sub(1);
    n.wrapping_add(a) & !a
}

/// 地址向上对齐。
fn align_up(addr: u64, align: u64) -> u64 {
    let a = align.wrapping_sub(1);
    addr.wrapping_add(a) & !a
}

/// 压力样本：100 笔申请、隔笔释放、再补 20 笔。
pub fn stress() -> Heap {
    let mut h = Heap::new();
    let mut i = 1u32;
    while i <= 100 {
        let size = 16u32.wrapping_add(i.wrapping_mul(37) % 2000);
        let _ = h.alloc(i, size, if i % 2 == 0 { 16 } else { 8 });
        i = i.wrapping_add(1);
    }
    let mut j = 2u32;
    while j <= 100 {
        let _ = h.free(j);
        j = j.wrapping_add(2);
    }
    let mut k = 101u32;
    while k <= 120 {
        let _ = h.alloc(k, 200_000, 4096);
        k = k.wrapping_add(1);
    }
    h
}

/// 注入核：四路坏样本。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // (a) 块号越界
    let mut a = Heap::new();
    got[0] = a.alloc(0, 64, 8).err() == Some(MG_ID);
    // (b) 块号重登
    let mut b = Heap::new();
    let _ = b.alloc(1, 64, 8);
    got[1] = b.alloc(1, 32, 8).err() == Some(MG_DUP);
    // (c) 对齐非 2 幂
    let mut c = Heap::new();
    got[2] = c.alloc(1, 64, 24).err() == Some(MG_ALIGN);
    // (d) 封册后写账
    let mut d = stress();
    let _ = d.freeze();
    got[3] = d.alloc(200, 64, 8).err() == Some(MG_FROZEN);
    got
}

/// 块号越界码（本卡第 8 码，追加于 0x08）。
pub const MG_ID: u32 = SEG_BASE + 0x08;

/// mallocng 内核消费面判据。
pub fn run_unx11267_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11267");

    // X1 两路选径：小组 brk、大组 mmap
    let x1 = {
        let mut h = Heap::new();
        let small = h.alloc(1, 64, 8);
        let big = h.alloc(2, 300_000, 8);
        let s_ok = small == Ok(BRK_BASE);
        let m_ok = big == Ok(MMAP_BASE);
        let src_ok = match (h.blocks.first(), h.blocks.get(1)) {
            (Some(a), Some(b)) => a.src == SRC_BRK && b.src == SRC_MMAP,
            _ => false,
        };
        s_ok && m_ok && src_ok
    };
    s.add("X1-两路选径", x1, "小请求落 brk、大请求落 mmap");

    // X2 账实对平
    let x2 = {
        let mut h = Heap::new();
        let _ = h.alloc(1, 100, 16);
        let _ = h.alloc(2, 200_000, 4096);
        h.audit() && h.committed == h.brk_used.wrapping_add(h.mmap_used)
    };
    s.add("X2-账实对平", x2, "提交量等于两池用量之和");

    // X3 压力账
    let x3 = {
        let h = stress();
        h.audit() && h.live_count() == 70 && h.blocks.len() == 120
    };
    s.add("X3-压力账", x3, "100 申请/50 释放/20 补充后零错位");

    // X4 泄漏账：重复释放与未登号
    let x4 = {
        let mut h = Heap::new();
        let _ = h.alloc(1, 64, 8);
        let _ = h.free(1);
        let dbl = h.free(1).err() == Some(MG_LEAK);
        let ghost = h.free(9).err() == Some(MG_LEAK);
        dbl && ghost
    };
    s.add("X4-泄漏账", x4, "重复释放与未知号均入泄漏账");

    // X5 块号域
    let x5 = {
        let mut a = Heap::new();
        let zero = a.alloc(0, 64, 8).err() == Some(MG_ID);
        let over = a.alloc(10_000, 64, 8).err() == Some(MG_ID);
        zero && over
    };
    s.add("X5-块号域", x5, "0 号与 10000 号均越界");

    // X6 长度门禁
    let x6 = {
        let mut a = Heap::new();
        let zero = a.alloc(1, 0, 8).err() == Some(MG_SIZE);
        let over = a.alloc(1, REQ_MAX.wrapping_add(1), 8).err() == Some(MG_SIZE);
        zero && over
    };
    s.add("X6-长度门禁", x6, "零长与超上限均拒");

    // X7 对齐门禁
    let x7 = {
        let mut a = Heap::new();
        let low = a.alloc(1, 64, 4).err() == Some(MG_ALIGN);
        let high = a.alloc(1, 64, 8192).err() == Some(MG_ALIGN);
        let odd = a.alloc(1, 64, 24).err() == Some(MG_ALIGN);
        low && high && odd
    };
    s.add("X7-对齐门禁", x7, "过小/过大/非 2 幂三路均拒");

    // X8 重号
    let x8 = {
        let mut h = Heap::new();
        let _ = h.alloc(3, 64, 8);
        h.alloc(3, 128, 16).err() == Some(MG_DUP)
    };
    s.add("X8-重号", x8, "同号二次申请报重号");

    // X9 来源错位
    let x9 = {
        let mut h = Heap::new();
        let bogus = h.alloc_pinned(1, 64, 8, SRC_MMAP).err() == Some(MG_SRC);
        let bogus2 = h.alloc_pinned(2, 200_000, 4096, SRC_BRK).err() == Some(MG_SRC);
        let good = h.alloc_pinned(3, 64, 8, SRC_BRK).is_ok();
        bogus && bogus2 && good
    };
    s.add("X9-来源错位", x9, "小组钉 mmap、大组钉 brk 均拒");

    // X10 块表定容
    let x10 = {
        let mut h = Heap::new();
        let mut i = 1u32;
        let mut ok = true;
        while i <= BLK_CAP as u32 {
            if h.alloc(i, 32, 8).is_err() {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok && h.alloc(200, 32, 8).err() == Some(MG_CAP)
    };
    s.add("X10-块表定容", x10, "128 块后第 129 块报满");

    // X11 指纹确定
    let x11 = {
        let h = stress();
        let a = h.fold();
        a == h.fold() && a != 0
    };
    s.add("X11-指纹确定", x11, "同账两次指纹一致且非零");

    // X12 封册幂等
    let x12 = {
        let mut h = stress();
        let r1 = h.freeze();
        let r2 = h.freeze();
        let live = h.fold();
        h.blocks.clear();
        let r3 = h.freeze();
        r1 == r2 && r2 == r3 && r1.is_some() && live != h.fold()
    };
    s.add("X12-封册幂等", x12, "二次封册回原根，账变不改根");

    // X13 封册拒写与块行明细
    let (x13, line13) = {
        let mut h = stress();
        let _ = h.freeze();
        let a = h.alloc(200, 64, 8).err() == Some(MG_FROZEN);
        let f = h.free(1).err() == Some(MG_FROZEN);
        let line = format!("alloc={} free={} first={}", a, f, match h.blocks.first() {
            Some(v) => v.render(),
            None => String::from("-"),
        });
        (a && f && line.contains("B*@600000"), line)
    };
    s.add_dyn("X13-封册明细", x13, &line13);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11267_flow() {
        let mut h = stress();
        assert!(h.audit());
        assert_eq!(h.live_count(), 70);
        assert_eq!(h.blocks.len(), 120);
        assert_eq!(h.rejects, 0);
        let r1 = h.freeze();
        let r2 = h.freeze();
        assert_eq!(r1, r2);
        assert!(r1.is_some());
        assert!(h.root == r1);
        assert!(h.alloc(300, 64, 8).err() == Some(MG_FROZEN));
        assert!(h.free(1).err() == Some(MG_FROZEN));
    }

    #[test]
    fn unx11267_gates() {
        let mut h = Heap::new();
        assert!(h.alloc(1, 64, 8).is_ok());
        assert!(h.alloc(1, 64, 8).err() == Some(MG_DUP));
        assert!(h.alloc(0, 64, 8).err() == Some(MG_ID));
        assert!(h.alloc(2, 0, 8).err() == Some(MG_SIZE));
        assert!(h.alloc(2, 64, 24).err() == Some(MG_ALIGN));
        assert!(h.alloc_pinned(2, 64, 8, SRC_MMAP).err() == Some(MG_SRC));
        assert!(h.free(1).is_ok());
        assert!(h.free(1).err() == Some(MG_LEAK));
        assert!(h.free(7).err() == Some(MG_LEAK));
        assert!(h.alloc_pinned(2, 200_000, 4096, SRC_MMAP).is_ok());
        assert!(h.freeze().is_some());
        assert!(h.alloc(3, 64, 8).err() == Some(MG_FROZEN));
        let accepts = h.accepts;
        let rejects = h.rejects;
        assert_eq!(accepts, 3);
        assert_eq!(rejects, 8);
    }

    #[test]
    fn unx11267_checks_green() {
        assert!(run_unx11267_checks().all_passed(), "UNX-F11267 判据未全绿");
    }
}
