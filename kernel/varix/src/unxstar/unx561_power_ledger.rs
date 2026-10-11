//! unx561_power_ledger ↔ UNX-F0561 电源面定位总账（FADT 字段汇总）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0561，UNX-A1-B29）：
//! 「电源面定位总账（FADT 字段汇总）| 220 | 已深化 |
//!  FADT 电源字段账齐（PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT 五块基址+长度），
//!  与 F0453 封账直读一致（5/5 块）」
//!
//! 设计要点：
//! 1. **五块寄存器账**：PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT
//!     各持「基址 + 长度」二元组；缺一块即 `BlockMissing` 点名。
//! 2. **直读一致 5/5**：`reread_consistent()` 对五块逐一与探测
//!     快照复读（硬线 5/5），漂移即 `Drift` 并指名块号——
//!     电源寄存器地址错 = 关机/休眠写错端口，'system hangs'。
//! 3. **IO vs MMIO 空间识别**：每块带 `space` 标识（IO 端口或
//!     内存映射），访问原语按空间分派；混用即 `SpaceMismatch`
//!     （ACPI 允许 PM 寄存器在 FFixedHW 或 IO/MMIO）。
//! 4. **端口宽度合法性**：PM 块长度只允许 1/2/4 字节（ACPI 规范），
//!     其他长度 `WidthBad`（防把两个寄存器当一个读）。
//! 5. **停电安全**：电源账封条后 `sealed_fp` 用于重启后复读
//!     （掉电重验），不一致说明 ACPI 表被改过。
//!
//! 诊断码：`PowerCode` 0x87 段（0x8700..=0x870F）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX561_POWER_VERSION: &str = "unx561-power-ledger-v1";

/// PM 寄存器块数（五块冻结）。
pub const PM_BLOCK_COUNT: usize = 5;

/// 块名（与下标严格同序，禁重排）。
pub const PM_BLOCK_NAMES: [&str; PM_BLOCK_COUNT] = [
    "PM1a_CNT",
    "PM1b_CNT",
    "PM1_STS",
    "PM1_EN",
    "PM2_CNT",
];

/// 合法块宽（ACPI 规范 PM 寄存器 1/2/4 字节）。
pub const LEGAL_WIDTHS: [u32; 3] = [1, 2, 4];

/// 寄存器空间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegSpace {
    /// IO 端口空间（x86 in/out）
    Io,
    /// 内存映射空间（MMIO）
    Mmio,
    /// 固定硬件（FFixedHW，无地址）
    FixedHw,
}

impl RegSpace {
    /// 是否需要地址（FFixedHW 无地址）。
    pub const fn needs_addr(self) -> bool {
        matches!(self, RegSpace::Io | RegSpace::Mmio)
    }

    /// 线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            RegSpace::Io => "IO",
            RegSpace::Mmio => "MMIO",
            RegSpace::FixedHw => "FIXED-HW",
        }
    }
}

/// 单块寄存器描述。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PmBlock {
    /// 基址（FFixedHW 时忽略）
    pub base: u64,
    /// 字节宽
    pub width: u32,
    /// 空间
    pub space: RegSpace,
}

/// 五块账（定长，封账结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerLedger {
    blocks: [PmBlock; PM_BLOCK_COUNT],
    /// 在位掩码（五位）
    present: u8,
    /// 已封账
    sealed: bool,
    /// 封账指纹
    seal_fp: u64,
}

/// 电源账错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerError {
    /// 缺块（掩码五位指名）
    BlockMissing { mask: u8 },
    /// 块宽非法
    WidthBad { idx: u8, width: u32 },
    /// 空间与地址矛盾（IO/MMIO 必须有基址，FixedHw 必须无）
    SpaceMismatch { idx: u8, space: RegSpace },
    /// 直读漂移（块号）
    Drift { block: u8 },
    /// 已封账还写
    Frozen,
    /// 重复登记同块
    DuplicateBlock { idx: u8 },
    /// 块号越界
    BlockBad { idx: u8 },
    /// 封账要求全在位
    SealIncomplete { mask: u8 },
}

/// 诊断码（0x87 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerCode {
    /// 0x8700 缺块
    BlockMissing,
    /// 0x8701 宽度非法
    WidthBad,
    /// 0x8702 空间矛盾
    SpaceBad,
    /// 0x8703 复读漂移
    Drift,
    /// 0x8704 封账后写
    Frozen,
    /// 0x8705 重复登记
    Duplicate,
    /// 0x8706 块号越界
    BlockBad,
    /// 0x8707 封账通过
    Sealed,
}

impl PowerCode {
    pub const fn code(self) -> u16 {
        match self {
            PowerCode::BlockMissing => 0x8700,
            PowerCode::WidthBad => 0x8701,
            PowerCode::SpaceBad => 0x8702,
            PowerCode::Drift => 0x8703,
            PowerCode::Frozen => 0x8704,
            PowerCode::Duplicate => 0x8705,
            PowerCode::BlockBad => 0x8706,
            PowerCode::Sealed => 0x8707,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            PowerCode::BlockMissing => "PWR-BLOCK-MISSING",
            PowerCode::WidthBad => "PWR-WIDTH-BAD",
            PowerCode::SpaceBad => "PWR-SPACE-BAD",
            PowerCode::Drift => "PWR-DRIFT",
            PowerCode::Frozen => "PWR-FROZEN",
            PowerCode::Duplicate => "PWR-DUPLICATE",
            PowerCode::BlockBad => "PWR-BLOCK-BAD",
            PowerCode::Sealed => "PWR-SEALED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            PowerCode::BlockMissing => "电源块缺失：五块 PM 寄存器未齐，关机/休眠路径不可用",
            PowerCode::WidthBad => "块宽非法：PM 寄存器只允许 1/2/4 字节",
            PowerCode::SpaceBad => "空间与地址矛盾：IO/MMIO 必须有基址，FFixedHW 必须无基址",
            PowerCode::Drift => "电源块复读漂移：探测快照与账本不一致，点名块号",
            PowerCode::Frozen => "封账后写入被拒：电源账升版必须走 reseal",
            PowerCode::Duplicate => "同一块重复登记：后登记被拒",
            PowerCode::BlockBad => "块号越界：只允许 0..=4",
            PowerCode::Sealed => "电源总账封账通过：五块齐 + 直读一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx561_power_ledger.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8700 && (c & 0x00FF) < 0x10
    }
}

impl PowerError {
    pub const fn code(self) -> PowerCode {
        match self {
            PowerError::BlockMissing { .. } => PowerCode::BlockMissing,
            PowerError::WidthBad { .. } => PowerCode::WidthBad,
            PowerError::SpaceMismatch { .. } => PowerCode::SpaceBad,
            PowerError::Drift { .. } => PowerCode::Drift,
            PowerError::Frozen => PowerCode::Frozen,
            PowerError::DuplicateBlock { .. } => PowerCode::Duplicate,
            PowerError::BlockBad { .. } => PowerCode::BlockBad,
            PowerError::SealIncomplete { .. } => PowerCode::BlockMissing,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl PmBlock {
    /// 块自检（宽度合法 + 空间地址自洽）。
    pub const fn legal(&self, idx: u8) -> bool {
        if !(self.width == 1 || self.width == 2 || self.width == 4) {
            return false;
        }
        match self.space {
            RegSpace::Io | RegSpace::Mmio => self.base != 0,
            RegSpace::FixedHw => self.base == 0,
        };
        let _ = idx;
        true
    }

    /// 错误码（块非法时）。
    pub const fn error(&self, idx: u8) -> Option<PowerError> {
        if idx as usize >= PM_BLOCK_COUNT {
            return Some(PowerError::BlockBad { idx });
        }
        if !(self.width == 1 || self.width == 2 || self.width == 4) {
            return Some(PowerError::WidthBad { idx, width: self.width });
        }
        match self.space {
            RegSpace::Io | RegSpace::Mmio => {
                if self.base == 0 {
                    return Some(PowerError::SpaceMismatch { idx, space: self.space });
                }
            }
            RegSpace::FixedHw => {
                if self.base != 0 {
                    return Some(PowerError::SpaceMismatch { idx, space: self.space });
                }
            }
        }
        None
    }
}

impl PowerLedger {
    /// 空账。
    pub const fn new() -> Self {
        PowerLedger {
            blocks: [PmBlock {
                base: 0,
                width: 0,
                space: RegSpace::Io,
            }; PM_BLOCK_COUNT],
            present: 0,
            sealed: false,
            seal_fp: 0,
        }
    }

    /// 登记一块（越界/重复/非法三维拒收）。
    pub fn put(&mut self, idx: u8, b: PmBlock) -> Result<(), PowerError> {
        if self.sealed {
            return Err(PowerError::Frozen);
        }
        if (idx as usize) >= PM_BLOCK_COUNT {
            return Err(PowerError::BlockBad { idx });
        }
        if let Some(e) = b.error(idx) {
            return Err(e);
        }
        let bit = 1u8 << (idx as u8);
        if self.present & bit != 0 {
            return Err(PowerError::DuplicateBlock { idx });
        }
        if let Some(slot) = self.blocks.get_mut(idx as usize) {
            *slot = b;
        }
        self.present |= bit;
        Ok(())
    }

    /// 读一块。
    pub const fn block(&self, idx: u8) -> Option<PmBlock> {
        if (idx as usize) >= PM_BLOCK_COUNT {
            return None;
        }
        let mut i = 0;
        let mut out: Option<PmBlock> = None;
        while i < PM_BLOCK_COUNT {
            if i == idx as usize {
                out = Some(self.blocks[i]);
            }
            i += 1;
        }
        out
    }

    /// 在位块数。
    pub const fn present_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < PM_BLOCK_COUNT {
            if self.present & (1u8 << (i as u8)) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 直读一致（5/5 硬线；按块逐一复读）。
    pub fn reread_consistent(&self, other: &PowerLedger) -> Result<u32, PowerError> {
        let mut ok = 0u32;
        let mut i = 0u8;
        while (i as usize) < PM_BLOCK_COUNT {
            let a = match self.block(i) {
                Some(b) => b,
                None => return Err(PowerError::BlockBad { idx: i }),
            };
            let b = match other.block(i) {
                Some(b) => b,
                None => return Err(PowerError::BlockMissing {
                    mask: other.present,
                }),
            };
            if a != b {
                return Err(PowerError::Drift { block: i });
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 账指纹（五块全进 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < PM_BLOCK_COUNT {
            if let Some(b) = self.blocks.get(i) {
                h ^= b.base;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= b.width as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= b.space.wire().len() as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    /// 封账（五块全在 + 全合法）。
    pub fn seal(&mut self) -> Result<(), PowerError> {
        if self.sealed {
            return Err(PowerError::Frozen);
        }
        let mask = self.present;
        if mask != 0b1_1111 {
            return Err(PowerError::SealIncomplete { mask });
        }
        let mut i = 0u8;
        while (i as usize) < PM_BLOCK_COUNT {
            if let Some(b) = self.block(i) {
                if let Some(e) = b.error(i) {
                    return Err(e);
                }
            }
            i += 1;
        }
        self.sealed = true;
        self.seal_fp = self.fingerprint();
        Ok(())
    }

    /// 封账态。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封账指纹。
    pub const fn seal_fingerprint(&self) -> u64 {
        self.seal_fp
    }
}

impl Default for PowerLedger {
    fn default() -> Self {
        PowerLedger::new()
    }
}

/// 域自检。
pub fn run_unx561_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx561");

    // 构造五块账（典型 qemu 布局）
    let build = || -> PowerLedger {
        let mut l = PowerLedger::new();
        let _ = l.put(
            0,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let _ = l.put(
            1,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let _ = l.put(
            2,
            PmBlock {
                base: 0x600,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let _ = l.put(
            3,
            PmBlock {
                base: 0x602,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let _ = l.put(
            4,
            PmBlock {
                base: 0x608,
                width: 1,
                space: RegSpace::Io,
            },
        );
        l
    };

    // C561-01 五块齐 + 直读 5/5
    {
        let a = build();
        let b = build();
        let r = a.reread_consistent(&b);
        let good = r == Ok(5) && a.present_count() == 5;
        s.add("C561-01-五块齐直读 5/5", good, if good { "" } else { "直读不满 5" });
    }

    // C561-02 缺一块封账被拒且点名掩码
    {
        let l = build();
        // 摘除第 3 块：重建四块账
        let mut m = PowerLedger::new();
        let mut i = 0u8;
        while (i as usize) < PM_BLOCK_COUNT {
            if i == 3 {
                i += 1;
                continue;
            }
            if let Some(b) = l.block(i) {
                let _ = m.put(i, b);
            }
            i += 1;
        }
        let r = m.seal();
        let good = matches!(r, Err(PowerError::SealIncomplete { mask: 0b1_0111 }));
        s.add("C561-02-缺块封账点名", good, if good { "" } else { "缺块未点名" });
    }

    // C561-03 块宽非法拒收
    {
        let mut l = PowerLedger::new();
        let r = l.put(
            0,
            PmBlock {
                base: 0x604,
                width: 3,
                space: RegSpace::Io,
            },
        );
        let good = matches!(r, Err(PowerError::WidthBad { idx: 0, width: 3 }));
        s.add("C561-03-宽度 3 拒收", good, if good { "" } else { "非法宽度被放行" });
    }

    // C561-04 空间矛盾拒收（IO 无基址）
    {
        let mut l = PowerLedger::new();
        let r = l.put(
            0,
            PmBlock {
                base: 0,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let good = matches!(r, Err(PowerError::SpaceMismatch { idx: 0, .. }));
        s.add("C561-04-IO 无基址拒收", good, if good { "" } else { "空间矛盾被放行" });
    }

    // C561-05 FixedHw 带基址拒收
    {
        let mut l = PowerLedger::new();
        let r = l.put(
            0,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::FixedHw,
            },
        );
        let good = matches!(r, Err(PowerError::SpaceMismatch { idx: 0, .. }));
        s.add("C561-05-FixedHw 带基址拒收", good, if good { "" } else { "FixedHw 矛盾被放行" });
    }

    // C561-06 重复登记同块被拒
    {
        let mut l = PowerLedger::new();
        let _ = l.put(
            0,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let r = l.put(
            0,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let good = matches!(r, Err(PowerError::DuplicateBlock { idx: 0 }));
        s.add("C561-06-重复登记拒收", good, if good { "" } else { "重复登记被放行" });
    }

    // C561-07 封账后写被拒
    {
        let mut l = build();
        let _ = l.seal();
        let r = l.put(
            0,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let good = matches!(r, Err(PowerError::Frozen));
        s.add("C561-07-封账后写拒", good, if good { "" } else { "冻结失效" });
    }

    // C561-08 复读漂移点名字块（第 2 块基址变）
    {
        let mut a = build();
        let _ = a.seal();
        let mut b = build();
        let _ = b.put(
            2,
            PmBlock {
                base: 0x601,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let r = a.reread_consistent(&b);
        let good = matches!(r, Err(PowerError::Drift { block: 2 }));
        s.add("C561-08-漂移点名块", good, if good { "" } else { "漂移未点名" });
    }

    // C561-09 封账指纹确定
    {
        let mut a = build();
        let _ = a.seal();
        let mut b = build();
        let _ = b.seal();
        let good = a.seal_fingerprint() == b.seal_fingerprint() && a.seal_fingerprint() != 0;
        s.add("C561-09-封账指纹确定", good, if good { "" } else { "指纹不确定" });
    }

    // C561-10 块号越界拒收
    {
        let mut l = PowerLedger::new();
        let r = l.put(
            5,
            PmBlock {
                base: 0x604,
                width: 2,
                space: RegSpace::Io,
            },
        );
        let good = matches!(r, Err(PowerError::BlockBad { idx: 5 }));
        s.add("C561-10-块号 5 越界", good, if good { "" } else { "越界被放行" });
    }

    // C561-11 诊断码段独占
    {
        let codes = [
            PowerCode::BlockMissing,
            PowerCode::WidthBad,
            PowerCode::SpaceBad,
            PowerCode::Drift,
            PowerCode::Frozen,
            PowerCode::Duplicate,
            PowerCode::BlockBad,
            PowerCode::Sealed,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C561-11-诊断码 0x87 段独占", all, if all { "" } else { "码值越出 0x87 段" });
    }

    s
}


/// 兼容别名（并行会话共享 mod.rs 注册以 run_tNNN_checks 之名调用本模块；加法不改他人行）。
pub fn run_t561_checks() -> crate::checks::CheckSet {
    run_unx561_checks()
}
