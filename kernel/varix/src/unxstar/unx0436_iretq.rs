//! unx0436_iretq —— UNX-F0436 iretq 帧守恒（异常返回五元组）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0436 · iretq 帧守恒 · 目标 240 行
//! 判据原文：UNX-F0436-J1 异常返回帧五元组 (RIP, CS, RFLAGS,
//!           RSP, SS) 在「进异常 → 处理 → iretq 返回」全程守
//!           恒：入帧存档五元组，出帧回读五元组，逐元相等
//!           （除白名单：TF 陷阱位允许处理路径消费——单步调
//!           试语义）；CS 特权级核：返回目标 CPL 由 CS 低 2
//!           位定，内核态返回 (CS=0x08) 不得落用户码段
//!           (CS=0x1B/0x23)；SS 一致性核：CPL 变更时 SS 必须
//!           同步变更（CPL 0→3 时 SS 须 0x20 系）；哨兵帧核：
//!           未初始化帧（全零）禁 iretq。
//! 组件：帧五元组存档器 + 守恒核对器（白名单位掩）+ CPL 提
//!       取器 + CS/SS 一致性核 + 哨兵帧闸 + 往返模拟。
//! 职责定位：iretq 是「把 CPU 交还给被打断的世界」的独木桥
//!   ——五元组有一个元被处理路径弄脏，返回后就是天崩（RIP
//!   错 = 跳野码，SS 错 = 栈面混乱）；守恒核把「进来什么样
//!   出去什么样」变成逐元硬判据，白名单只开 TF 一位，其余
//!   一律守恒。
//! 性能逐项分解：存档 O(1)；守恒核 O(5)；CPL 提取 O(1)；
//!   一致性 O(1)。
//! 诊断码段：0x30480（0x30481~0x30489 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const IR_VERSION: &str = "unx0436_iretq-v1";

/// 诊断段基址（0x30480 段 · 本模块独占）。
pub const IR_SEG_BASE: u32 = 0x30480;
pub const XC_RIP_DRIFT: u32 = 0x30481; // RIP 漂移
pub const XC_CS_DRIFT: u32 = 0x30482; // CS 漂移
pub const XC_RFLAGS_DRIFT: u32 = 0x30483; // RFLAGS 漂移（白名单外）
pub const XC_RSP_DRIFT: u32 = 0x30484; // RSP 漂移
pub const XC_SS_DRIFT: u32 = 0x30485; // SS 漂移
pub const XC_CPL_BREAK: u32 = 0x30486; // 特权级违规
pub const XC_SS_SYNC: u32 = 0x30487; // CPL 变更 SS 不同步
pub const XC_ZOMBIE_FRAME: u32 = 0x30488; // 未初始化帧
pub const XC_MAGIC: u32 = 0x30489; // 账魔数破

/// 允许消费的 RFLAGS 位（TF 位 8）。
pub const RFLAGS_WHITELIST: u64 = 1 << 8;
/// 内核码段 / 用户码段 / 内核栈段 / 用户栈段。
pub const CS_KERNEL: u16 = 0x08;
pub const CS_USER: u16 = 0x1B;
pub const SS_KERNEL: u16 = 0x10;
pub const SS_USER: u16 = 0x23;

/// 帧五元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetFrame {
    pub rip: u64,
    pub cs: u16,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u16,
}

impl RetFrame {
    /// 哨兵帧核：全零禁返。
    pub fn zombie(&self) -> Result<(), u32> {
        if self.rip == 0 && self.cs == 0 && self.rsp == 0 && self.ss == 0 {
            return Err(XC_ZOMBIE_FRAME);
        }
        Ok(())
    }

    /// CPL 提取：CS 低 2 位。
    pub fn cpl(&self) -> u8 {
        (self.cs & 0x3) as u8
    }

    /// CPL 违规核：出帧 CPL 不得高于（数值上小于）入帧 CPL
    /// ——即内核路径不许替用户「升权」；也不许从内核帧落
    /// 用户码段（标准 GDT 布局：选择子索引 3 = 用户码段族）。
    pub fn cpl_ok(&self, entry: &RetFrame) -> Result<(), u32> {
        if self.cpl() < entry.cpl() {
            return Err(XC_CPL_BREAK);
        }
        // 用户码段族判别：索引 3（0x18/0x1B 系）。
        let out_user = (self.cs >> 3) == 3;
        let in_user = (entry.cs >> 3) == 3;
        if !in_user && out_user {
            return Err(XC_CPL_BREAK);
        }
        Ok(())
    }

    /// CPL 变更时 SS 同步核：入帧 CPL == 出帧 CPL 则 SS 必须
    /// 相等；CPL 变更则 SS 必须换族。
    pub fn ss_sync(&self, entry: &RetFrame) -> Result<(), u32> {
        if self.cpl() == entry.cpl() {
            if self.ss != entry.ss {
                return Err(XC_SS_SYNC);
            }
        } else {
            let entry_user = entry.ss & 0x3 != 0;
            let out_user = self.ss & 0x3 != 0;
            if entry_user == out_user {
                return Err(XC_SS_SYNC);
            }
        }
        Ok(())
    }

    /// 守恒核：出帧对入帧逐元比（RFLAGS 白名单位掩掉）。
    pub fn conserve(&self, entry: &RetFrame) -> Result<(), u32> {
        self.zombie()?;
        if self.rip != entry.rip {
            return Err(XC_RIP_DRIFT);
        }
        if self.cs != entry.cs {
            return Err(XC_CS_DRIFT);
        }
        let mask = !RFLAGS_WHITELIST;
        if (self.rflags & mask) != (entry.rflags & mask) {
            return Err(XC_RFLAGS_DRIFT);
        }
        if self.rsp != entry.rsp {
            return Err(XC_RSP_DRIFT);
        }
        if self.ss != entry.ss {
            return Err(XC_SS_DRIFT);
        }
        Ok(())
    }
}

/// 异常往返模拟：入帧存档 → 处理（可改白名单位）→ 出帧核。
pub fn roundtrip(entry: RetFrame, handler_tf_consume: bool) -> Result<(), u32> {
    let mut exit = entry;
    if handler_tf_consume {
        exit.rflags &= !RFLAGS_WHITELIST; // 处理路径消费 TF
    }
    exit.conserve(&entry)?;
    exit.cpl_ok(&entry)?;
    exit.ss_sync(&entry)
}

/// 判据入口：UNX-F0436。
pub fn run_unx0436_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0436_iretq");
    let k = RetFrame { rip: 0x401000, cs: CS_KERNEL, rflags: 0x202, rsp: 0x9000, ss: SS_KERNEL };
    // 族一：守恒全绿。
    cs.add("内核帧守恒", roundtrip(k, false).is_ok(), "五元全等");
    cs.add("TF 消费白名单", roundtrip(k, true).is_ok(), "位 8 可吃");
    // 族二：逐元漂移。
    let d_rip = RetFrame { rip: 0x401004, ..k };
    cs.add("RIP 漂移立案", d_rip.conserve(&k) == Err(XC_RIP_DRIFT), "跳野码");
    let d_cs = RetFrame { cs: 0x10, ..k };
    cs.add("CS 漂移立案", d_cs.conserve(&k) == Err(XC_CS_DRIFT), "换段");
    let d_rf = RetFrame { rflags: 0x2102, ..k }; // 动了位 12（IOPL 外）实际 0x100 差
    cs.add("RFLAGS 漂移立案", d_rf.conserve(&k) == Err(XC_RFLAGS_DRIFT), "白名单外");
    let d_ok = RetFrame { rflags: 0x302, ..k }; // 只加 TF 位
    cs.add("TF 位豁免", d_ok.conserve(&k).is_ok(), "位 8 掩掉");
    let d_rsp = RetFrame { rsp: 0x9008, ..k };
    cs.add("RSP 漂移立案", d_rsp.conserve(&k) == Err(XC_RSP_DRIFT), "栈面乱");
    let d_ss = RetFrame { ss: 0x18, ..k };
    cs.add("SS 漂移立案", d_ss.conserve(&k) == Err(XC_SS_DRIFT), "换栈段");
    // 族三：CPL 核。
    let u = RetFrame { rip: 0x1000, cs: CS_USER, rflags: 0x202, rsp: 0x7000, ss: SS_USER };
    cs.add("用户帧守恒", roundtrip(u, false).is_ok(), "用户全等");
    // 内核帧落用户码段：构造出帧 CS=0x1B。
    let to_user = RetFrame { cs: CS_USER, ss: SS_USER, ..k };
    cs.add("内核落用户拒", to_user.cpl_ok(&k) == Err(XC_CPL_BREAK), "升权违规");
    // CPL 升（数值降）核：入帧用户 → 出帧内核。
    let to_kernel = RetFrame { cs: CS_KERNEL, ss: SS_KERNEL, ..u };
    cs.add("用户升内核拒", to_kernel.cpl_ok(&u) == Err(XC_CPL_BREAK), "CPL 3→0 违规");
    // 族四：SS 同步。
    let dss_same = RetFrame { ss: SS_KERNEL + 8, ..k }; // 同 CPL 换 SS
    cs.add("同 CPL 换 SS 拒", dss_same.ss_sync(&k) == Err(XC_SS_SYNC), "无谓换栈段");
    let dss_ok = RetFrame { ss: SS_USER, cs: CS_USER, ..k };
    cs.add("CPL 变 SS 换族过", dss_ok.ss_sync(&k).is_ok(), "0→3 换族");
    let dss_stuck = RetFrame { ss: SS_KERNEL, cs: CS_USER, ..k };
    cs.add("CPL 变 SS 不动拒", dss_stuck.ss_sync(&k) == Err(XC_SS_SYNC), "栈段没跟");
    // 族五：哨兵帧。
    let z = RetFrame { rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0 };
    cs.add("僵尸帧禁返", z.zombie() == Err(XC_ZOMBIE_FRAME), "全零");
    cs.add("僵尸守恒拒", z.conserve(&k) == Err(XC_ZOMBIE_FRAME), "守恒先查哨兵");
    // 族六：TF 语义账。
    let tf_set = RetFrame { rflags: 0x302, ..k };
    cs.add("TF 入帧可消费", roundtrip(tf_set, true).is_ok(), "单步退出");
    let tf_keep = RetFrame { rflags: 0x302, ..k };
    cs.add("TF 保留也行", roundtrip(tf_keep, false).is_ok(), "不消费也守恒");
    // 族七：半白名单漂移仍立案。
    let half = RetFrame { rflags: (k.rflags & !RFLAGS_WHITELIST) | 0x40 | 0x100, ..k };
    cs.add("白名单外两位拒", half.conserve(&k) == Err(XC_RFLAGS_DRIFT), "位 6/8 外动");
    // 族八：往返返回值。
    cs.add("往返 Ok", roundtrip(k, false) == Ok(()), "模拟闭环");
    cs
}
