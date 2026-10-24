//! unx0470_roline —— UNX-F0470 配置空间零写封账（只读红线）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0470 · 零写封账 · 目标 220 行
//! 判据原文：UNX-F0470-J1 枚举全程只读红线账：软模型配置空
//!           间（64KB 定容镜像）带写监护器（任何字节写 → 计
//!           数 + 立案）；账二 解析面零写核：头解析/BAR/能
//!           力链/链路四解析器跑完，写计数必须为 0；账三 白
//!           名单写通道：仅显式声明的配置写接口可写（如后
//!           续 BAR 分配阶段），且每次写留档（who/offset/
//!           old/new）；账四 写留档完整性：留档条目 old/new
//!           不同才有效（幂等写留档 = 无效写立案）；账五 红
//!           线报警器：非白名单写 → 立即立案（fail-fast）。
//! 组件：写监护器 + 白名单账 + 留档核 + fail-fast 闸。
//! 职责定位：配置空间零写是 PCI 枚举的「第零戒律」——枚举
//!   器的手只许看不许动：乱写一个 BAR 就把固件布好的地址
//!   图撕了，乱写命令寄存器就把设备炸了；写监护器是把这戒
//!   律从「自觉」变成「机制」的唯一途径。
//! 性能逐项分解：监护 O(1)；留档 O(n)。
//! 诊断码段：0x345C0（0x345C1~0x345C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x345C0 起）。
pub const SEG_BASE: u32 = 0x345C0;
pub const XC_RO_LINE: u32 = SEG_BASE + 1; // 只读红线破
pub const XC_IDLE_WRITE: u32 = SEG_BASE + 2; // 幂等写留档无效
pub const XC_NO_LOG: u32 = SEG_BASE + 3; // 写无留档

/// 配置空间镜像 64KB。
pub const CFG_SIZE: usize = 0x1_0000;

/// 写留档条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WLog {
    pub who: u8,
    pub offset: u32,
    pub old: u8,
    pub new: u8,
}

impl WLog {
    /// 留档有效性：old != new 才有效。
    pub fn valid(&self) -> bool {
        self.old != self.new
    }
}

/// 写监护镜像。
#[derive(Clone, Debug)]
pub struct GuardedCfg {
    pub mem: Vec<u8>,
    /// 非白名单写计数（红线报警）。
    pub violations: u32,
    /// 白名单写留档。
    pub logs: Vec<WLog>,
}

impl GuardedCfg {
    pub fn new(init: &[u8]) -> GuardedCfg {
        let mut mem = alloc::vec::Vec::new();
        for b in init.iter().take(CFG_SIZE) {
            mem.push(*b);
        }
        while mem.len() < CFG_SIZE {
            mem.push(0);
        }
        GuardedCfg { mem, violations: 0, logs: Vec::new() }
    }

    /// 白名单写：留档 + 值写入。幂等写留档无效（立案）。
    pub fn whitelisted_write(&mut self, who: u8, offset: u32, new: u8) -> Result<(), u32> {
        let off = offset as usize;
        if off >= CFG_SIZE {
            return Err(XC_NO_LOG);
        }
        let old = self.mem[off];
        let log = WLog { who, offset, old, new };
        if !log.valid() {
            return Err(XC_IDLE_WRITE);
        }
        self.mem[off] = new;
        self.logs.push(log);
        Ok(())
    }

    /// 非白名单写：fail-fast 立案（写不生效）。
    pub fn rogue_write(&mut self, offset: u32) {
        self.violations += 1;
        let _ = offset; // 值不落地——红线拦截
    }

    /// 零写核：解析面跑完后写计数与留档数须为 0（纯只读阶段）。
    pub fn ro_clean(&self) -> Result<(), u32> {
        if self.violations != 0 || !self.logs.is_empty() {
            return Err(XC_RO_LINE);
        }
        Ok(())
    }
}

/// 解析面模拟：只读消费配置空间字节（模拟 0465~0469 四解析器）。
pub fn parse_face(cfg: &GuardedCfg) -> (u16, u16, u8) {
    let v = u16::from_le_bytes([cfg.mem[0], cfg.mem[1]]);
    let d = u16::from_le_bytes([cfg.mem[2], cfg.mem[3]]);
    let c = cfg.mem[0x0B];
    (v, d, c)
}

/// 判据入口：UNX-F0470 全族核。
pub fn run_unx0470_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0470");

    // 夹具：一份有内容的配置空间。
    let mut init = [0u8; 256];
    init[0..2].copy_from_slice(&0x8086u16.to_le_bytes());
    init[2..4].copy_from_slice(&0x1234u16.to_le_bytes());
    init[0x0B] = 0x06;

    // 族一：解析面零写。
    let mut g1 = GuardedCfg::new(&init);
    let (v, d, c) = parse_face(&g1);
    cs.add("解析读出", v == 0x8086 && d == 0x1234 && c == 0x06, "字段全读");
    cs.add("解析面零写", g1.ro_clean().is_ok(), "红线净");

    // 族二：劫掠写拦截。
    g1.rogue_write(0x10);
    cs.add("劫掠写立案", g1.violations == 1, "fail-fast");
    cs.add("劫掠不落地", g1.ro_clean() == Err(XC_RO_LINE), "红线破");

    // 族三：白名单写留档。
    let mut g2 = GuardedCfg::new(&init);
    cs.add("白名单写成", g2.whitelisted_write(1, 0x04, 0x07).is_ok(), "命令寄存器");
    cs.add("留档一行", g2.logs.len() == 1, "who=1");
    cs.add("留档有效", g2.logs[0].valid(), "old!=new");
    cs.add("值落地", g2.mem[0x04] == 0x07, "写入生效");
    cs.add("多笔留档", { g2.whitelisted_write(1, 0x04, 0x03); g2.logs.len() == 2 }, "累加");

    // 族四：幂等写立案。
    let mut g3 = GuardedCfg::new(&init);
    g3.whitelisted_write(1, 0x04, 0x00).ok(); // old=0x00 new=0x00
    cs.add("幂等写拒", g3.whitelisted_write(1, 0x04, 0x00) == Err(XC_IDLE_WRITE), "无效留档");
    cs.add("幂等不落地", g3.mem[0x04] == 0x00, "值不变");

    // 族五：越界写拒。
    let mut g4 = GuardedCfg::new(&init);
    cs.add("越界写拒", g4.whitelisted_write(1, CFG_SIZE as u32, 0xFF) == Err(XC_NO_LOG), "64KB 界外");

    // 族六：零写核负样本（有留档即不净）。
    let mut g5 = GuardedCfg::new(&init);
    g5.whitelisted_write(2, 0x40, 0x01).ok();
    cs.add("留档破零写", g5.ro_clean() == Err(XC_RO_LINE), "白名单写也破净面");

    // 族七：镜像容量。
    let g6 = GuardedCfg::new(&init);
    cs.add("镜像 64KB", g6.mem.len() == CFG_SIZE, "定容");
    cs.add("镜像保真", g6.mem[0] == 0x86 && g6.mem[1] == 0x80, "小端投影");

    cs
}
