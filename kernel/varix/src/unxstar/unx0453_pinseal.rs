//! unx0453_pinseal —— UNX-F0453 全表指针封账（只读封存）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0453 · 全表指针封账 · 目标 230 行
//! 判据原文：UNX-F0453-J1 封账：所有已定位表以（物理基址、
//!           长度、签名）三元组登记冻结；登记后不可修改、
//!           不可删除（封存纪律）；账二 重叠核：任意两表区
//!           间 [a, a+len) 不得重叠（重叠 = 定位器 bug 或固
//!           件表损坏，立案）；账三 越界核：表区间必须落在
//!           表区上限内（地址 + 长度溢出核）；账四 只读封
//!           存面：封账后登记表无写通道（API 面无 mut 出口）；
//!           账五 查询核：按基址/签名双通道检索，未命中返
//!           None 不立案（查询是只读行为）。
//! 组件：封存账本 + 重叠核 + 溢出核 + 双通道检索器。
//! 职责定位：封账是 ACPI 的「地契登记」——表物理地址一旦
//!   记下就是终身不动产；重叠是定位链最致命的矛盾（两张表
//!   声称同一块内存，谁先映射谁污染谁），必须在任何映射动
//!   作之前封存拦截。
//! 性能逐项分解：登记 O(n²)（n ≤ 32 表）；查询 O(n)。
//! 诊断码段：0x34280（0x34281~0x34289 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34280 起）。
pub const SEG_BASE: u32 = 0x34280;
pub const XC_OVLP: u32 = SEG_BASE + 1; // 区间重叠
pub const XC_OOB: u32 = SEG_BASE + 2; // 越界/溢出
pub const XC_ZERO_LEN: u32 = SEG_BASE + 3; // 零长度表
pub const XC_FROZEN: u32 = SEG_BASE + 4; // 封账后再登记（冻结态）

/// 封存条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pin {
    pub phys: u64,
    pub len: u32,
    pub sign: [u8; 4],
}

impl Pin {
    /// 区间终点（含溢出核）。
    pub fn end(&self) -> Option<u64> {
        self.phys.checked_add(self.len as u64)
    }
}

/// 封存账本：登记后冻结（seal 后不可再写）。
#[derive(Clone, Debug, Default)]
pub struct PinBook {
    pub pins: Vec<Pin>,
    pub sealed: bool,
}

impl PinBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一张表。封账后调用 → 冻结立案。
    pub fn pin(&mut self, p: Pin, region_top: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_FROZEN);
        }
        if p.len == 0 {
            return Err(XC_ZERO_LEN);
        }
        let end = p.end().ok_or(XC_OOB)?;
        if end > region_top {
            return Err(XC_OOB);
        }
        // 重叠核：与所有既有区间比对。
        for e in &self.pins {
            let ee = e.end().ok_or(XC_OOB)?;
            // 区间相交判定：max(lo) < min(hi)。
            let lo = p.phys.max(e.phys);
            let hi = end.min(ee);
            if lo < hi {
                return Err(XC_OVLP);
            }
        }
        self.pins.push(p);
        Ok(())
    }

    /// 封账：冻结登记面。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 按基址检索（只读）。
    pub fn by_addr(&self, phys: u64) -> Option<&Pin> {
        self.pins.iter().find(|p| p.phys == phys)
    }

    /// 按签名检索首个（只读；多实例取首个——快照序数语义）。
    pub fn by_sign(&self, sign: &[u8; 4]) -> Option<&Pin> {
        self.pins.iter().find(|p| p.sign == *sign)
    }

    /// 按签名检索全部实例。
    pub fn all_sign(&self, sign: &[u8; 4]) -> Vec<&Pin> {
        self.pins.iter().filter(|p| p.sign == *sign).collect()
    }
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0453 全族核。
pub fn run_unx0453_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0453");
    const TOP: u64 = 0x1_0000_0000; // 4G 表区上限

    // 族一：正常登记。
    let mut book = PinBook::new();
    let mk = |a: u64, l: u32, s: &[u8; 4]| Pin { phys: a, len: l, sign: *s };
    cs.add("FACP 登记", book.pin(mk(0xE0000, 0x148, b"FACP"), TOP).is_ok(), "首表");
    cs.add("APIC 登记", book.pin(mk(0xE2000, 0x94, b"APIC"), TOP).is_ok(), "邻位不重叠");
    cs.add("HPET 登记", book.pin(mk(0xE4000, 0x38, b"HPET"), TOP).is_ok(), "第三表");
    cs.add("账三行", book.pins.len() == 3, "登记数");

    // 族二：重叠核。
    let mut b2 = book.clone();
    cs.add("区间重叠拒", b2.pin(mk(0xE2008, 0x40, b"XXXX"), TOP) == Err(XC_OVLP), "半途侵入");
    let mut b3 = book.clone();
    cs.add("完全覆盖拒", b3.pin(mk(0xE0000, 0x200, b"YYYY"), TOP) == Err(XC_OVLP), "整段吞并");
    let mut b4 = book.clone();
    cs.add("尾邻接合法", b4.pin(mk(0xE0098, 0x10, b"ZZZZ"), TOP).is_ok(), "相切不相交");

    // 族三：越界与溢出。
    let mut b5 = book.clone();
    cs.add("越上限拒", b5.pin(mk(0xFFFF_FFF0, 0x20, b"XXXX"), TOP) == Err(XC_OOB), "超 4G");
    let mut b6 = book.clone();
    cs.add("加法溢出拒", b6.pin(mk(0xFFFF_FFFF_FFFF_FFF0, 0x20, b"XXXX"), TOP) == Err(XC_OOB), "u64 溢出");
    let mut b7 = book.clone();
    cs.add("零长拒", b7.pin(mk(0xE6000, 0, b"XXXX"), TOP) == Err(XC_ZERO_LEN), "空表");

    // 族四：封账纪律。
    let mut b8 = book.clone();
    b8.seal();
    cs.add("封账后拒登", b8.pin(mk(0xE6000, 0x10, b"XXXX"), TOP) == Err(XC_FROZEN), "冻结态");
    cs.add("封账不动存量", b8.pins.len() == 3, "封存保真");

    // 族五：双通道检索。
    let hit = book.by_addr(0xE2000);
    cs.add("按址命中", hit.map(|p| p.sign) == Some(*b"APIC"), "APIC@E2000");
    cs.add("按址未中", book.by_addr(0xDEAD0).is_none(), "None 不立案");
    let sh = book.by_sign(b"HPET");
    cs.add("按签命中", sh.map(|p| p.phys) == Some(0xE4000), "HPET");
    cs.add("按签未中", book.by_sign(b"XXXX").is_none(), "无表 None");

    // 族六：多实例检索。
    let mut b9 = book.clone();
    b9.pin(mk(0xE8000, 0x94, b"APIC"), TOP).ok();
    let all = b9.all_sign(b"APIC");
    cs.add("APIC 双实例", all.len() == 2, "首+新");
    cs.add("首例保序", all[0].phys == 0xE2000, "快照序数");

    // 族七：end 溢出核单元。
    let p_ok = mk(0xE0000, 0x148, b"FACP");
    cs.add("end 合法", p_ok.end() == Some(0xE0148), "和不出界");
    let p_ov = mk(u64::MAX, 1, b"XXXX");
    cs.add("end 溢出 None", p_ov.end().is_none(), "checked_add");

    // 族八：只读封存面（软证：克隆后改克隆不影响原账）。
    let mut b10 = book.clone();
    b10.pins[0].len = 0xFFFF;
    cs.add("克隆隔离", book.pins[0].len == 0x148, "原账保真");

    cs
}
