# -*- coding: utf-8 -*-
"""S4.1 缺口二修复补丁：.bss 池 v2（key=虚拟地址）+ bus_addr 边界翻译。"""
from pathlib import Path

p = Path(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\drivers\xhci.rs")
s = p.read_text(encoding="utf-8")

def rep(old, new, n=1):
    global s
    c = s.count(old)
    assert c == n, "count=%d (want %d) for: %r" % (c, n, old[:70])
    s = s.replace(old, new)

# R1: UsbDma → BssDma v2
old_block = """    /// DMA 桶池：PMM 单帧 ×16（共享 6 帧 + 每设备 3 帧，键盘+鼠标=12）。
    ///
    /// **已知缺口（2026-09-21 登记，见验收记录）**：PMM 帧与 Limine 装载
    /// 的 initfs/内核页存在重叠可能（memmap 预登记缺口），表现为部分
    /// boot 的命令 TRB 对控制器不可见（graceful kwarn 跳过，绝不致命）。
    /// 修复方向已定：DMA 帧改 .bss 驻留 + 寄存器编程边界处页表翻译
    /// （当前 .bss 尝试在 virt_of 缺失路径引入 #PF 崩溃，已回退本版）。
    pub struct UsbDma {
        frames: [Option<u64>; 16],
        hhdm: u64,
    }

    impl UsbDma {
        pub fn new() -> UsbDma {
            UsbDma { frames: [None; 16], hhdm: crate::limine::hhdm_offset().unwrap_or(0) }
        }
    }

    impl DmaMem for UsbDma {
        fn alloc_frame(&mut self) -> Option<u64> {
            let f = crate::mem::pmm::alloc_page()?;
            self.frames.iter_mut().find(|s| s.is_none()).map(|s| *s = Some(f))?;
            Some(f)
        }
        fn free_frame(&mut self, phys: u64) {
            if let Some(slot) = self.frames.iter_mut().find(|s| **s == Some(phys)) {
                *slot = None;
                crate::mem::pmm::free_order(phys, 0);
            }
        }
        fn write_bytes(&mut self, phys: u64, off: u64, data: &[u8]) {
            let virt = phys + off + self.hhdm;
            // SAFETY: phys 为 PMM 持有帧，off+len ≤4KiB。
            for (i, &b) in data.iter().enumerate() {
                unsafe { core::ptr::write_volatile((virt + i as u64) as *mut u8, b) };
            }
        }
        fn read_bytes(&self, phys: u64, off: u64, out: &mut [u8]) {
            let virt = phys + off + self.hhdm;
            for (i, slot) in out.iter_mut().enumerate() {
                // SAFETY: 同 write_bytes。
                *slot = unsafe { core::ptr::read_volatile((virt + i as u64) as *const u8) };
            }
        }
    }"""
new_block = """    /// DMA 池页数（共享 6 帧 + 每设备 3 帧 ×4 = 18，取整 24）。
    const DMA_POOL_FRAMES: usize = 24;
    const DMA_POOL_BYTES: usize = DMA_POOL_FRAMES * 4096;

    /// .bss 驻留 DMA 池 v2（2026-09-21 晚间改型，修缺口二）：
    /// - **key = 内核虚拟地址**：驱动内部内存访问全部 key+off 直接算术
    ///   （v1 的 #PF 根因 = 「帧基+偏移」键在帧基精确匹配表里反查失败，
    ///   兜底返回物理形态地址被当虚拟写——本版从结构上消灭该类反查）；
    /// - 池页是内核映像 .bss（Limine 装载区，引导保留，免疫 PMM 与
    ///   initfs/内核页重叠——v0 pmm 版的失败根因假设）；
    /// - `bus_addr()` 只在寄存器编程边界（CRCR/DCBAA/ERSTBA/ERST 表内容/
    ///   ERDP/EP 上下文 dequeue/TRB 参数/Link 目标/事件匹配）把 key
    ///   翻译为页表权威真物理（`RealPt::translate`，2MiB 大叶语义正确）。
    #[repr(align(4096))]
    struct DmaPoolBytes([u8; DMA_POOL_BYTES]);
    static mut DMA_POOL: DmaPoolBytes = DmaPoolBytes([0; DMA_POOL_BYTES]);

    pub struct BssDma {
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
rep(old_block, new_block)

rep("static mut GLOBAL: Option<XhciCtrl<BarMmio, UsbDma>> = None;",
    "static mut GLOBAL: Option<XhciCtrl<BarMmio, BssDma>> = None;")
rep("fn global() -> Option<&'static mut XhciCtrl<BarMmio, UsbDma>> {",
    "fn global() -> Option<&'static mut XhciCtrl<BarMmio, BssDma>> {")
rep("fn install_global(c: XhciCtrl<BarMmio, UsbDma>) -> bool {",
    "fn install_global(c: XhciCtrl<BarMmio, BssDma>) -> bool {")
rep("let dma = UsbDma::new();", "let dma = BssDma::new();")

rep("self.mem.write_bytes(self.cmd_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.cmd_phys, self.cmd_cycle));\n        self.mem.write_bytes(self.ep0_ring_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.ep0_ring_phys, self.ep0_cycle));",
    "self.mem.write_bytes(self.cmd_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(self.cmd_phys), self.cmd_cycle));\n        self.mem.write_bytes(self.ep0_ring_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(self.ep0_ring_phys), self.ep0_cycle));")

rep("""        // ERST 表（单段 16B）：表 @ 帧头，环 @ +64（都 64B 对齐）。
        let seg = self.evt_phys + 64;
        let mut erst = [0u8; 16];
        erst[0..8].copy_from_slice(&seg.to_le_bytes());
        erst[8..12].copy_from_slice(&(EVENT_ENTRIES as u32).to_le_bytes());
        self.mem.write_bytes(self.evt_phys, 0, &erst);
        // CRCR：低 64B 对齐基址 | RCS=1，先低后高（QEMU 在高写时建环）。
        self.opw32(OP_CRCR, self.cmd_phys as u32 | 0x1);
        self.opw32(OP_CRCR + 4, (self.cmd_phys >> 32) as u32);
        self.opw32(OP_DCBAAP, self.dcbaa_phys as u32);
        self.opw32(OP_DCBAAP + 4, (self.dcbaa_phys >> 32) as u32);
        self.opw32(OP_CONFIG, self.max_slots as u32);
        // 中断器 0：ERSTSZ=1 → ERSTBA（高写触发段装载）→ ERDP 指向环头。
        self.rtw32(RT_ERSTSZ, 1);
        self.rtw32(RT_ERSTBA, self.evt_phys as u32);
        self.rtw32(RT_ERSTBA + 4, (self.evt_phys >> 32) as u32);
        self.rtw32(RT_ERDP, seg as u32);
        self.rtw32(RT_ERDP + 4, (seg >> 32) as u32);""",
    """        // ERST 表（单段 16B）：表 @ 帧头，环 @ +64（都 64B 对齐）。
        // 段基址写**总线地址**（控制器经 ERST DMA 到事件环）。
        let seg = self.evt_phys + 64;
        let seg_bus = self.mem.bus_addr(seg);
        let mut erst = [0u8; 16];
        erst[0..8].copy_from_slice(&seg_bus.to_le_bytes());
        erst[8..12].copy_from_slice(&(EVENT_ENTRIES as u32).to_le_bytes());
        self.mem.write_bytes(self.evt_phys, 0, &erst);
        // CRCR：低 64B 对齐基址 | RCS=1，先低后高（QEMU 在高写时建环）。
        // 以下寄存器值全部是**总线地址**（bus_addr 边界翻译）。
        let cmd_bus = self.mem.bus_addr(self.cmd_phys);
        self.opw32(OP_CRCR, cmd_bus as u32 | 0x1);
        self.opw32(OP_CRCR + 4, (cmd_bus >> 32) as u32);
        let dcbaa_bus = self.mem.bus_addr(self.dcbaa_phys);
        self.opw32(OP_DCBAAP, dcbaa_bus as u32);
        self.opw32(OP_DCBAAP + 4, (dcbaa_bus >> 32) as u32);
        self.opw32(OP_CONFIG, self.max_slots as u32);
        // 中断器 0：ERSTSZ=1 → ERSTBA（高写触发段装载）→ ERDP 指向环头。
        let evt_bus = self.mem.bus_addr(self.evt_phys);
        self.rtw32(RT_ERSTSZ, 1);
        self.rtw32(RT_ERSTBA, evt_bus as u32);
        self.rtw32(RT_ERSTBA + 4, (evt_bus >> 32) as u32);
        self.rtw32(RT_ERDP, seg_bus as u32);
        self.rtw32(RT_ERDP + 4, (seg_bus >> 32) as u32);""")

rep("""        let erdp = self.evt_phys + 64 + self.evt_idx as u64 * 32;
        self.rtw32(RT_ERDP, erdp as u32 | ERDP_EHB);
        self.rtw32(RT_ERDP + 4, (erdp >> 32) as u32);""",
    """        let erdp_tok = self.evt_phys + 64 + self.evt_idx as u64 * 32;
        let erdp = self.mem.bus_addr(erdp_tok);
        self.rtw32(RT_ERDP, erdp as u32 | ERDP_EHB);
        self.rtw32(RT_ERDP + 4, (erdp >> 32) as u32);""")

rep("""        self.mem.write_bytes(trb_addr, 0, &t.le_bytes());
        self.cmd_outstanding = Some(trb_addr);
        let (ntail, ncyc) = ring_advance(self.cmd_tail, self.cmd_cycle);
        if ntail == 0 {
            self.mem.write_bytes(self.cmd_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.cmd_phys, self.cmd_cycle));
        }""",
    """        self.mem.write_bytes(trb_addr, 0, &t.le_bytes());
        // 事件匹配用总线地址（QEMU 事件 ptr = TRB 的 guest 物理）。
        let trb_bus = self.mem.bus_addr(trb_addr);
        self.cmd_outstanding = Some(trb_bus);
        let (ntail, ncyc) = ring_advance(self.cmd_tail, self.cmd_cycle);
        if ntail == 0 {
            self.mem.write_bytes(self.cmd_phys + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(self.cmd_phys), self.cmd_cycle));
        }""")
rep("if ev.ptr == trb_addr {", "if ev.ptr == trb_bus {")

rep("""            (ep0_deq & !0xF) as u32 | (self.ep0_cycle as u32),
            (ep0_deq >> 32) as u32,""",
    """            (self.mem.bus_addr(ep0_deq) & !0xF) as u32 | (self.ep0_cycle as u32),
            (self.mem.bus_addr(ep0_deq) >> 32) as u32,""")

rep("""            (ep1_ring & !0xF) as u32 | 0x1, // DCS=1
            (ep1_ring >> 32) as u32,""",
    """            (self.mem.bus_addr(ep1_ring) & !0xF) as u32 | 0x1, // DCS=1
            (self.mem.bus_addr(ep1_ring) >> 32) as u32,""")

rep("let dt = Trb::data(buf, len, dir_in, cycle);",
    "let dt = Trb::data(self.mem.bus_addr(buf), len, dir_in, cycle);")
rep("""        self.ep0_tail = nidx;
        self.ep0_cycle = ncyc;
        self.ep0_outstanding = Some(status_addr);""",
    """        self.ep0_tail = nidx;
        self.ep0_cycle = ncyc;
        // 事件匹配用总线地址。
        let status_bus = self.mem.bus_addr(status_addr);
        self.ep0_outstanding = Some(status_bus);""")
rep("if ev.ptr == status_addr {", "if ev.ptr == status_bus {")

rep("""            let trb_addr = ring + tail as u64 * 32;
            let (nt, nc) = self.ring_put(ring, tail, cyc, Trb::normal(buf, HID_REPORT_LEN, cyc));
            if let Some(d) = self.devs[i].as_mut() {
                d.ep1_tail = nt;
                d.ep1_cycle = nc;
                d.ep1_outstanding = Some(trb_addr);
            }""",
    """            let trb_addr = ring + tail as u64 * 32;
            let trb_bus = self.mem.bus_addr(trb_addr);
            let (nt, nc) = self.ring_put(ring, tail, cyc, Trb::normal(self.mem.bus_addr(buf), HID_REPORT_LEN, cyc));
            if let Some(d) = self.devs[i].as_mut() {
                d.ep1_tail = nt;
                d.ep1_cycle = nc;
                d.ep1_outstanding = Some(trb_bus);
            }""")

rep("self.mem\n            .write_bytes(ep1_ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(ep1_ring, true));",
    "self.mem\n            .write_bytes(ep1_ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(ep1_ring), true));")

rep("""        self.mem
            .write_bytes(self.dcbaa_phys, (slotid as u64) * 8, &octx.to_le_bytes());""",
    """        self.mem
            .write_bytes(self.dcbaa_phys, (slotid as u64) * 8, &self.mem.bus_addr(octx).to_le_bytes());""")

rep("""            // 本轮结束：Link TRB 的合法周期仍是**本轮**周期——控制器在
            // 下一轮首轮门铃时以本轮周期校验 Link，随后才自行翻转周期
            // （QEMU ring_fetch 的 TC 语义）。写成翻转后的值会提前失配。
            self.mem.write_bytes(ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(ring, cycle));""",
    """            // 本轮结束：Link TRB 的合法周期仍是**本轮**周期——控制器在
            // 下一轮首轮门铃时以本轮周期校验 Link，随后才自行翻转周期
            // （QEMU ring_fetch 的 TC 语义）。写成翻转后的值会提前失配。
            // Link 目标写**总线地址**（控制器循链 DMA）。
            self.mem.write_bytes(ring + (RING_ENTRIES - 1) as u64 * 32, 0, &link_trb_bytes(self.mem.bus_addr(ring), cycle));""")

p.write_text(s, encoding="utf-8")
print("ALL WRITTEN")
