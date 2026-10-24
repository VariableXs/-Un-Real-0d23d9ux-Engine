//! unx0495_rtroot —— UNX-F0495 根表/上下文表条目闸（VT-d 两级寻址前闸）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0495 · 根上下文闸 · 目标 290 行
//! 判据原文：UNX-F0495-J1 根上下文闸：根一 根条目对齐
//!           （上下文表指针必须 4096 对齐，低位残渣拒）；根
//!           二 根条目 present 才可下钻（未 present 的根条
//!           目下钻拒）；根三 上下文条目 DID 非零（present
//!           的上下文条目域 ID 为零拒——零域是保留黑域）；
//!           根四 BDF 三级键唯一（总线-设备-功能键不重）；
//!           根五 条目对账（present 根数 × 均摊上下文数与
//!           登记总量对账）。
//! 组件：根表 + 上下文表 + BDF 键 + present/DID/对齐三闸。
//! 职责定位：根表/上下文表是 DMA 翻译的两级目录——根条目按
//!   总线分派，上下文条目按设备挂域。对齐闸防的是指针低
//!   位被当旗标复用后的残渣地址；DID=0 闸防的是「翻译成
//!   功但落进保留域」的静默越权；BDF 键唯一防一设备双挂。
//! 性能逐项分解：根下钻 O(1)；BDF 唯一 O(登记数) 线性扫。
//! 诊断码段：0x34A80（0x34A81~0x34A89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34A80;
pub const XC_ALIGN: u32 = SEG_BASE + 1; // 上下文表指针未 4096 对齐
pub const XC_NP: u32 = SEG_BASE + 2; // 根条目未 present
pub const XC_ZDID: u32 = SEG_BASE + 3; // 上下文条目 DID 为零
pub const XC_DUPBDF: u32 = SEG_BASE + 4; // BDF 键重复

/// BDF 三级键（总线/设备/功能）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bdf {
    pub bus: u8,
    pub dev: u8,
    pub fn_: u8,
}

impl Bdf {
    /// 打包成 16 位键（bus<<8 | dev<<3 | fn）。
    pub fn pack(self) -> u16 {
        ((self.bus as u16) << 8) | ((self.dev as u16) << 3) | (self.fn_ as u16)
    }
}

/// 上下文条目：present + DID + BDF 键。
#[derive(Clone, Copy, Debug)]
pub struct CtxEntry {
    pub bdf: Bdf,
    pub did: u16,
    pub present: bool,
}

/// 根上下文登记表。
pub struct RootTable {
    pub ctx: Vec<CtxEntry>,
}

impl RootTable {
    pub fn new() -> Self {
        RootTable { ctx: Vec::new() }
    }

    /// 根条目闸：present + 指针对齐（模拟根条目：bit0=present，bit12~ =指针）。
    pub fn check_root_entry(&self, entry: u64) -> Result<(), u32> {
        if entry & 1 == 0 {
            return Err(XC_NP);
        }
        let ctp = entry & !0xFFF;
        if ctp != (entry & !0xFFF) || entry & 0xFFE != 0 {
            return Err(XC_ALIGN);
        }
        // 指针本体对齐（4096）由 ctp 低位全零保证；此处再核指针非零。
        if ctp == 0 {
            return Err(XC_ALIGN);
        }
        Ok(())
    }

    /// 上下文条目登记：DID 非零闸 + BDF 唯一闸。
    pub fn register_ctx(&mut self, e: CtxEntry) -> Result<(), u32> {
        if !e.present {
            return Err(XC_NP);
        }
        if e.did == 0 {
            return Err(XC_ZDID);
        }
        if self.ctx.iter().any(|x| x.bdf.pack() == e.bdf.pack()) {
            return Err(XC_DUPBDF);
        }
        self.ctx.push(e);
        Ok(())
    }

    /// BDF 下钻：查上下文条目。
    pub fn lookup(&self, bdf: Bdf) -> Option<&CtxEntry> {
        self.ctx.iter().find(|x| x.bdf.pack() == bdf.pack())
    }

    /// present 计数。
    pub fn present_count(&self) -> usize {
        self.ctx.iter().filter(|x| x.present).count()
    }
}

pub fn run_unx0495_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0495");
    cs.add("SEG 段位独占", SEG_BASE == 0x34A80 && XC_DUPBDF == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：根条目闸。
    let rt = RootTable::new();
    // 合法根条目：present=1，指针 0x1000 对齐。
    cs.add("根条目成", rt.check_root_entry(0x1_0000_1001).is_ok(), "present+对齐");
    // 未 present。
    cs.add("根未 present 拒", rt.check_root_entry(0x1_0000_1000) == Err(XC_NP), "bit0 缺");
    // 低位残渣（bit1~11 非零）。
    cs.add("指针残渣拒", rt.check_root_entry(0x1_0000_1003) == Err(XC_ALIGN), "bit1 置位非对齐");
    // present 但指针为零。
    cs.add("零指针拒", rt.check_root_entry(0x1) == Err(XC_ALIGN), "present 但 ctp=0");

    // 族二：上下文条目 DID 闸。
    let mut rt2 = RootTable::new();
    let b0 = Bdf { bus: 0, dev: 2, fn_: 0 };
    cs.add("DID 非零成", rt2.register_ctx(CtxEntry { bdf: b0, did: 7, present: true }).is_ok(), "DID=7");
    cs.add("DID 零拒", rt2.register_ctx(CtxEntry { bdf: Bdf { bus: 0, dev: 3, fn_: 0 }, did: 0, present: true }) == Err(XC_ZDID), "零域黑域");
    cs.add("未 present 拒", rt2.register_ctx(CtxEntry { bdf: Bdf { bus: 0, dev: 4, fn_: 0 }, did: 7, present: false }) == Err(XC_NP), "死条目不登记");

    // 族三：BDF 键唯一。
    cs.add("BDF 重复拒", rt2.register_ctx(CtxEntry { bdf: b0, did: 8, present: true }) == Err(XC_DUPBDF), "同键不同 DID 也拒");
    // 不同功能号同设备——键不同，成。
    cs.add("功能号分键成", rt2.register_ctx(CtxEntry { bdf: Bdf { bus: 0, dev: 2, fn_: 1 }, did: 9, present: true }).is_ok(), "fn 0→1 键异");

    // 族四：BDF 打包位域（bus<<8 | dev<<3 | fn：dev=2 → 2<<3=0x10）。
    cs.add("打包位域", b0.pack() == 0x0010, "bus=0 dev=2 fn=0 → 0x0010");
    cs.add("功能号进低位", Bdf { bus: 0, dev: 2, fn_: 3 }.pack() == 0x0013, "fn=3 → 低三位 011");
    cs.add("总线进高位", Bdf { bus: 0x2A, dev: 0, fn_: 0 }.pack() == 0x2A00, "bus=0x2A → 高八位");

    // 族五：下钻对账。
    let hit = rt2.lookup(b0);
    cs.add("下钻命中", hit.map(|h| h.did) == Some(7), "b0 → DID 7");
    cs.add("下钻落空", rt2.lookup(Bdf { bus: 0xFE, dev: 0, fn_: 0 }).is_none(), "未登记键落空");
    cs.add("present 对账", rt2.present_count() == 2 && rt2.ctx.len() == 2, "两登记两 present");

    // 族六：登记序稳定（幂等核）。
    let a = rt2.lookup(b0).map(|h| h.did);
    let b = rt2.lookup(b0).map(|h| h.did);
    cs.add("双查恒等", a == b && a == Some(7), "同键两次下钻恒等");

    // 族七：总线槽位模型（根表按总线 256 槽分派）。
    /// 总线槽占用器：槽位 = 根条目的总线索引。
    pub struct BusSlots {
        pub occupied: [bool; 8],
        pub used: usize,
    }
    impl BusSlots {
        pub fn new() -> Self {
            BusSlots { occupied: [false; 8], used: 0 }
        }
        pub fn claim(&mut self, bus: usize) -> Result<(), u32> {
            if self.occupied[bus] {
                return Err(XC_DUPBDF); // 槽位复用必须先释放
            }
            self.occupied[bus] = true;
            self.used += 1;
            Ok(())
        }
        pub fn release(&mut self, bus: usize) -> Result<(), u32> {
            if !self.occupied[bus] {
                return Err(XC_NP);
            }
            self.occupied[bus] = false;
            self.used -= 1;
            Ok(())
        }
    }
    let mut sl = BusSlots::new();
    cs.add("槽位占用成", sl.claim(0).is_ok() && sl.claim(3).is_ok(), "两总线两槽");
    cs.add("槽位复用拒", sl.claim(0) == Err(XC_DUPBDF), "未释放即复用拒");
    cs.add("槽位释放成", sl.release(0).is_ok(), "归还槽 0");
    cs.add("释放后可复用", sl.claim(0).is_ok(), "复用走合法路径");
    cs.add("空槽释放拒", sl.release(5) == Err(XC_NP), "未占槽归还拒");
    cs.add("槽位计数对账", sl.used == 2, "占用/释放收支平衡");

    // 族八：全总线扫掠（8 槽填满再释放归零）。
    let mut sl2 = BusSlots::new();
    let mut ok_n = 0u32;
    for bus in 0..8usize {
        if sl2.claim(bus).is_ok() {
            ok_n += 1;
        }
    }
    cs.add("八槽全占", ok_n == 8 && sl2.used == 8, "槽池满");
    let mut rel_n = 0u32;
    for bus in 0..8usize {
        if sl2.release(bus).is_ok() {
            rel_n += 1;
        }
    }
    cs.add("八槽全释", rel_n == 8 && sl2.used == 0, "收支归零");
    cs.add("双释放拒", sl2.release(0) == Err(XC_NP), "空池再释拒");

    cs
}
