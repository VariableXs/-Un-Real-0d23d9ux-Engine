# -*- coding: utf-8 -*-
"""S4.1 缺口二修复 v3：.bss 后备 + HHDM 访问路径（v0 实证路径）+ translate 总线地址。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\drivers\xhci.rs")
s = p.read_text(encoding="utf-8")

def rep(old, new, n=1):
    global s
    c = s.count(old)
    assert c == n, "count=%d (want %d) for: %r" % (c, n, old[:70])
    s = s.replace(old, new)

# R1: BssDma v2 → v3（访问路径 = phys+HHDM，v0 实证路径；查找先掩码防 miss）
old_block = """    pub struct BssDma {
        /// (帧虚拟 key, 真物理) —— 分配时经页表翻译登记。
        vmap: [(u64, u64); DMA_POOL_FRAMES],
        next: usize,
    }

    impl BssDma {
        fn pool_virt() -> u64 {
            (unsafe { (&raw const DMA_POOL as *const u8).addr() }) as u64
        }

        pub fn new() -> BssDma {
            BssDma { vmap: [(0, 0); DMA_POOL_FRAMES], next: 0 }
        }

        /// 帧物理：页表翻译为权威（内核映像映射真值，2MiB 大叶含全偏移）；
        /// 翻译不可用时兜底 executable_address 滑移。
        fn phys_of_frame(virt: u64) -> u64 {
            use crate::mem::pfh::PageTableOps as _;
            if let Some(p) = crate::mem::pfh::target_ops().translate(virt) {
                return p;
            }
            let (phys, virtb) = crate::limine::executable_address().unwrap_or((0, 0));
            virt - (virtb - phys)
        }
    }

    impl DmaMem for BssDma {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next >= DMA_POOL_FRAMES {
                return None;
            }
            let i = self.next;
            let virt = Self::pool_virt() + (i * 4096) as u64;
            let phys = Self::phys_of_frame(virt);
            self.vmap[i] = (virt, phys);
            self.next += 1;
            Some(virt) // key = 虚拟地址
        }
        fn free_frame(&mut self, _key: u64) {}
        fn write_bytes(&mut self, key: u64, off: u64, data: &[u8]) {
            let virt = key + off;
            // SAFETY: key 出自本池（.bss 驻留内核映像，恒映射），off+len ≤4KiB。
            for (i, &b) in data.iter().enumerate() {
                unsafe { core::ptr::write_volatile((virt + i as u64) as *mut u8, b) };
            }
        }
        fn read_bytes(&self, key: u64, off: u64, out: &mut [u8]) {
            let virt = key + off;
            for (i, slot) in out.iter_mut().enumerate() {
                // SAFETY: 同 write_bytes。
                *slot = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
            }
        }
        fn zero_frame(&mut self, key: u64) {
            const Z: [u8; 256] = [0u8; 256];
            let mut off = 0u64;
            while off < 4096 {
                self.write_bytes(key, off, &Z);
                off += 256;
            }
        }
        fn bus_addr(&self, key: u64) -> u64 {
            // 帧基查表 + 低 12 位帧内偏移（key 全部出自本池帧基 ± <4KiB 偏移）。
            let frame = key & !0xFFF;
            for (v, p) in self.vmap.iter() {
                if *v == frame {
                    return p | (key & 0xFFF);
                }
            }
            frame // 不可达：key 全部出自本池分配的帧
        }
    }"""
new_block = """    pub struct BssDma {
        /// (帧虚拟 token, 真物理) —— 分配时经页表翻译登记。
        frames: [Option<(u64, u64)>; DMA_POOL_FRAMES],
        hhdm: u64,
        next: usize,
    }

    impl BssDma {
        fn pool_virt() -> u64 {
            (unsafe { (&raw const DMA_POOL as *const u8).addr() }) as u64
        }

        pub fn new() -> BssDma {
            BssDma {
                frames: [None; DMA_POOL_FRAMES],
                hhdm: crate::limine::hhdm_offset().unwrap_or(0),
                next: 0,
            }
        }

        /// 帧物理：页表翻译为权威（内核映像映射真值，2MiB 大叶含全偏移）；
        /// 翻译不可用时兜底 executable_address 滑移。
        fn phys_of_frame(virt: u64) -> u64 {
            use crate::mem::pfh::PageTableOps as _;
            if let Some(p) = crate::mem::pfh::target_ops().translate(virt) {
                return p;
            }
            let (phys, virtb) = crate::limine::executable_address().unwrap_or((0, 0));
            virt - (virtb - phys)
        }

        /// key（帧 token + <4KiB 偏移）→ HHDM 访问地址。
        /// **先掩码取帧基再查表**（v1 的 #PF 根因 = 带 偏移 的键去精确匹配
        /// 帧基条目），命中后拼回 帧物理+HHDM+偏移 —— v0 pmm 版在同一
        /// QEMU 上实证过的访问路径（trace2 boot1 全链枚举+报告流动）。
        fn hhdm_of(&self, key: u64) -> u64 {
            let frame = key & !0xFFF;
            for (tok, phys) in self.frames.iter().flatten() {
                if *tok == frame {
                    return phys + self.hhdm + (key & 0xFFF);
                }
            }
            key + self.hhdm // 不可达：key 全部出自本池分配的帧
        }
    }

    impl DmaMem for BssDma {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next >= DMA_POOL_FRAMES {
                return None;
            }
            let i = self.next;
            let virt = Self::pool_virt() + (i * 4096) as u64;
            let phys = Self::phys_of_frame(virt);
            self.frames[i] = Some((virt, phys));
            self.next += 1;
            Some(virt) // key = 帧虚拟 token
        }
        fn free_frame(&mut self, _key: u64) {}
        fn write_bytes(&mut self, key: u64, off: u64, data: &[u8]) {
            let virt = self.hhdm_of(key + off);
            // SAFETY: 帧物理来自页表翻译的内核映像 .bss，HHDM 全覆盖，off+len ≤4KiB。
            for (i, &b) in data.iter().enumerate() {
                unsafe { core::ptr::write_volatile((virt + i as u64) as *mut u8, b) };
            }
        }
        fn read_bytes(&self, key: u64, off: u64, out: &mut [u8]) {
            let virt = self.hhdm_of(key + off);
            for (i, slot) in out.iter_mut().enumerate() {
                // SAFETY: 同 write_bytes。
                *slot = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
            }
        }
        fn zero_frame(&mut self, key: u64) {
            const Z: [u8; 256] = [0u8; 256];
            let mut off = 0u64;
            while off < 4096 {
                self.write_bytes(key, off, &Z);
                off += 256;
            }
        }
        fn bus_addr(&self, key: u64) -> u64 {
            // 帧基查表 + 低 12 位帧内偏移。
            let frame = key & !0xFFF;
            for (tok, phys) in self.frames.iter().flatten() {
                if *tok == frame {
                    return phys | (key & 0xFFF);
                }
            }
            frame // 不可达：key 全部出自本池分配的帧
        }
    }"""
rep(old_block, new_block)

p.write_text(s, encoding="utf-8")
print("ALL WRITTEN")
