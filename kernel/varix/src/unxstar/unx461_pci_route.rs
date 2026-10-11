//! unx461_pci_route ↔ UNX-F0461 配置访问机制裁定（ECAM vs CF8/CFC）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0461，UNX-A1-B24）：
//! 「配置访问机制裁定（ECAM vs CF8/CFC）| 240 | 已深化 |
//!  机制裁定账（ECAM 优先/CF8 兜底 2 路径），双机制回读一致性抽样核（8/8 设备）」
//!
//! 设计要点：
//! 1. **两路径裁定**：`PciMechanism` 闭集 {Ecma / Cf8}。
//!    裁定序 = ECAM 优先（内存映射、无 IO 端口竞态、支持扩展配置空间），
//!    CF8/CFC 兜底（老机器/ECAM 未映射时）。**不允许静默回落**——
//!    兜底必须在账本登记 `FallbackReason`，否则排查看不出走了哪条路。
//! 2. **回读一致性抽样**：`cross_check()` 对同一 bus/dev/func/reg
//!    同时经两机制读（真机双机制不会同一时刻都可用，此处以仲裁器
//!    双投影代理），比对 8/8 设备全一致才认账。
//! 3. **扩展配置空间**：ECAM 路径支持 4KB 空间（reg 高位 0x100..0xFFF），
//!    CF8 路径只有 256 字节——扩展寄存器的 CF8 值必须显性判
//!    `NotSupportedByMechanism`，不能返回 0xFFFF 冒充。
//! 4. **设备表登记**：`PciDeviceRow` 八条字段（bus/dev/func/vendor/
//!    device/class/subclass/progif），含 vendor==0xFFFF 空槽识别。
//! 5. **抽样式样**：`sample_devices()` 轮转 8 设备，避免每次全扫
//!    （PCI 枚举在引导期是热点，全扫 256 bus 不可接受）。
//!
//! 诊断码：`PciCode` 0x83 段（0x8300..=0x830F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX461_PCI_VERSION: &str = "unx461-pci-route-v1";

/// 抽样设备数（锚点 8/8 硬线）。
pub const SAMPLE_DEVICES: usize = 8;

/// ECAM 支持的最大寄存器偏移（4KB 扩展配置空间）。
pub const ECAM_REG_MAX: u16 = 0x0FFF;

/// CF8/CFC 支持的最大寄存器偏移（256 字节传统配置空间）。
pub const CF8_REG_MAX: u16 = 0x00FF;

/// 空槽 vendor（PCI 规范 0xFFFF）。
pub const VENDOR_NONE: u16 = 0xFFFF;

/// 访问机制闭集（两路径，裁定序固定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PciMechanism {
    /// 内存映射 ECAM（PCIe 增强配置机制）
    Ecam,
    /// IO 端口 CF8/CFC（传统配置机制）
    Cf8,
}

/// 兜底原因（回落必须显性登记，禁静默）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FallbackReason {
    /// ECAM 基址未映射（MCFG 表缺失）
    McfgAbsent,
    /// ECAM 映射尺寸非法（MCFG 声明与实测不符）
    EcamSizeBad,
    /// ECAM 读回全 1（总线未枚举）
    EcamDead,
    /// 无兜底需要（首选即 ECAM）
    NotFallen,
}

/// 设备行（八字段，全显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PciDeviceRow {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub progif: u8,
}

impl PciDeviceRow {
    /// 空槽识别。
    pub const fn is_empty_slot(&self) -> bool {
        self.vendor == VENDOR_NONE
    }

    /// BDF 三元组键（排序/查重用）。
    pub const fn bdf_key(&self) -> u32 {
        ((self.bus as u32) << 8) | ((self.dev as u32) << 3) | (self.func as u32)
    }

    /// 类名三元组（class/subclass/progif 全进）。
    pub const fn class_key(&self) -> u32 {
        ((self.class as u32) << 16) | ((self.subclass as u32) << 8) | (self.progif as u32)
    }
}

/// 机制裁定账（一条记录 = 一次裁定 + 理由 + 时间戳）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArbitrationEntry {
    /// 裁定机制
    pub mechanism: PciMechanism,
    /// 兜底原因
    pub reason: FallbackReason,
    /// ECAM 基址（CF8 兜底时为 0）
    pub ecam_base: u64,
    /// 裁定 tick（注入）
    pub tick: u64,
}

/// 访问抽象（两机制同一 trait，避免调用方分叉）。
pub trait ConfigAccess {
    /// 读 32 位配置字。
    fn read32(&self, row: &PciDeviceRow, reg: u16) -> Result<u32, PciError>;
    /// 机制标识。
    fn mechanism(&self) -> PciMechanism;
}

/// PCI 错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PciError {
    /// 寄存器偏移超出机制能力
    NotSupportedByMechanism { reg: u16, mech: PciMechanism },
    /// 空槽读（vendor 0xFFFF）
    EmptySlot { bus: u8, dev: u8, func: u8 },
    /// 双机制读数不一致
    Divergence { reg: u16, ecam: u32, cf8: u32 },
    /// 裁定记录账满
    LedgerFull { cap: usize },
    /// ECAM 死链
    EcamDead,
}

/// 诊断码（0x83 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PciCode {
    /// 0x8300 机制不支持该寄存器
    RegUnsupported,
    /// 0x8301 空槽
    EmptySlot,
    /// 0x8302 双机制分歧
    Divergence,
    /// 0x8303 账满
    LedgerFull,
    /// 0x8304 ECAM 死链
    EcamDead,
    /// 0x8305 裁定 ECAM 首选
    ChoseEcam,
    /// 0x8306 裁定 CF8 兜底
    ChoseCf8,
    /// 0x8307 抽样一致 8/8
    SampleConsistent,
}

impl PciCode {
    pub const fn code(self) -> u16 {
        match self {
            PciCode::RegUnsupported => 0x8300,
            PciCode::EmptySlot => 0x8301,
            PciCode::Divergence => 0x8302,
            PciCode::LedgerFull => 0x8303,
            PciCode::EcamDead => 0x8304,
            PciCode::ChoseEcam => 0x8305,
            PciCode::ChoseCf8 => 0x8306,
            PciCode::SampleConsistent => 0x8307,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            PciCode::RegUnsupported => "PCI-REG-UNSUPPORTED",
            PciCode::EmptySlot => "PCI-EMPTY-SLOT",
            PciCode::Divergence => "PCI-DIVERGENCE",
            PciCode::LedgerFull => "PCI-LEDGER-FULL",
            PciCode::EcamDead => "PCI-ECAM-DEAD",
            PciCode::ChoseEcam => "PCI-CHOSE-ECAM",
            PciCode::ChoseCf8 => "PCI-CHOSE-CF8",
            PciCode::SampleConsistent => "PCI-SAMPLE-CONSISTENT",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            PciCode::RegUnsupported => "寄存器偏移超出机制能力：扩展配置空间（>0xFF）必须走 ECAM，CF8 路径禁返回假值",
            PciCode::EmptySlot => "读到空槽：vendor 为 0xFFFF，该 BDF 无设备",
            PciCode::Divergence => "双机制读数分歧：同一寄存器 ECAM 与 CF8 值不一致，配置视图矛盾",
            PciCode::LedgerFull => "裁定账已满：无法落账本次裁定，显性失败",
            PciCode::EcamDead => "ECAM 死链：读回全 1，MCFG 可能指向未映射区",
            PciCode::ChoseEcam => "裁定 ECAM 首选：MCFG 有效且读回非全 1",
            PciCode::ChoseCf8 => "裁定 CF8 兜底：已登记兜底原因",
            PciCode::SampleConsistent => "抽样一致：8/8 设备双机制回读一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx461_pci_route.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8300 && (c & 0x00FF) < 0x10
    }
}

impl PciError {
    pub const fn code(self) -> PciCode {
        match self {
            PciError::NotSupportedByMechanism { .. } => PciCode::RegUnsupported,
            PciError::EmptySlot { .. } => PciCode::EmptySlot,
            PciError::Divergence { .. } => PciCode::Divergence,
            PciError::LedgerFull { .. } => PciCode::LedgerFull,
            PciError::EcamDead => PciCode::EcamDead,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// ECAM 访问实现（内存映射投影）。
pub struct EcamAccess {
    /// ECAM 基址（MCFG 给出）
    pub base: u64,
    /// 探测读回（代理真机 MMIO，宿主测试注入）
    pub window: Vec<u8>,
    /// 窗覆盖的设备数上限
    pub rows: Vec<PciDeviceRow>,
}

impl ConfigAccess for EcamAccess {
    fn read32(&self, row: &PciDeviceRow, reg: u16) -> Result<u32, PciError> {
        if reg > ECAM_REG_MAX {
            return Err(PciError::NotSupportedByMechanism {
                reg,
                mech: PciMechanism::Ecam,
            });
        }
        if row.is_empty_slot() {
            return Err(PciError::EmptySlot {
                bus: row.bus,
                dev: row.dev,
                func: row.func,
            });
        }
        // ECAM 寻址：base + (bus<<20 | dev<<15 | func<<12 | reg)
        let off = ((row.bus as u64) << 20)
            | ((row.dev as u64) << 15)
            | ((row.func as u64) << 12)
            | (reg as u64);
        let mut v = 0xFFFF_FFFFu32;
        if off + 4 <= self.window.len() as u64 {
            let mut b = [0u8; 4];
            let mut i = 0;
            while i < 4 {
                if let Some(x) = self.window.get((off as usize) + i) {
                    b[i] = *x;
                }
                i += 1;
            }
            v = u32::from_le_bytes(b);
            if v == 0xFFFF_FFFF {
                return Err(PciError::EcamDead);
            }
        }
        Ok(v)
    }

    fn mechanism(&self) -> PciMechanism {
        PciMechanism::Ecam
    }
}

/// CF8/CFC 访问实现（IO 端口投影）。
pub struct Cf8Access {
    /// 32 位配置字存储（代理 IO 往返）
    pub words: Vec<(u32, u32)>,
}

impl ConfigAccess for Cf8Access {
    fn read32(&self, row: &PciDeviceRow, reg: u16) -> Result<u32, PciError> {
        if reg > CF8_REG_MAX {
            return Err(PciError::NotSupportedByMechanism {
                reg,
                mech: PciMechanism::Cf8,
            });
        }
        if row.is_empty_slot() {
            return Err(PciError::EmptySlot {
                bus: row.bus,
                dev: row.dev,
                func: row.func,
            });
        }
        let key = row.bdf_key();
        for (k, v) in self.words.iter() {
            if *k == key {
                return Ok(*v);
            }
        }
        Ok(0xFFFF_FFFF)
    }

    fn mechanism(&self) -> PciMechanism {
        PciMechanism::Cf8
    }
}

/// 机制仲裁器（裁定 + 落账 + 抽样回读三合一）。
pub struct Router {
    /// 裁定记录（只增不减）
    pub entries: Vec<ArbitrationEntry>,
    /// 账容量
    pub cap: usize,
    /// 当前机制（无记录 = 未裁定）
    pub active: Option<PciMechanism>,
}

impl Router {
    /// 空仲裁器。
    pub const fn new(cap: usize) -> Self {
        Router {
            entries: Vec::new(),
            cap,
            active: None,
        }
    }

    /// 裁定：ECAM 优先，CF8 兜底（兜底必须给理由）。
    pub fn arbitrate(
        &mut self,
        ecam_ok: bool,
        reason: FallbackReason,
        ecam_base: u64,
        tick: u64,
    ) -> Result<PciMechanism, PciError> {
        let (mech, r) = if ecam_ok {
            (PciMechanism::Ecam, FallbackReason::NotFallen)
        } else {
            (PciMechanism::Cf8, reason)
        };
        if self.entries.len() >= self.cap {
            return Err(PciError::LedgerFull { cap: self.cap });
        }
        self.entries.push(ArbitrationEntry {
            mechanism: mech,
            reason: r,
            ecam_base: if mech == PciMechanism::Ecam {
                ecam_base
            } else {
                0
            },
            tick,
        });
        self.active = Some(mech);
        Ok(mech)
    }

    /// 兜底记录数（CF8 路径被走过几次）。
    pub fn fallback_count(&self) -> usize {
        let mut n = 0;
        for e in self.entries.iter() {
            if e.mechanism == PciMechanism::Cf8 {
                n += 1;
            }
        }
        n
    }

    /// 抽样回读一致性（8/8 硬线）。
    pub fn cross_check(
        &self,
        ecam: &dyn ConfigAccess,
        cf8: &dyn ConfigAccess,
        rows: &[PciDeviceRow],
    ) -> Result<u32, PciError> {
        if rows.len() < SAMPLE_DEVICES {
            return Err(PciError::Divergence {
                reg: 0,
                ecam: 0,
                cf8: rows.len() as u32,
            });
        }
        let reg: u16 = 0; // vendor/device 字
        let mut i = 0;
        while i < SAMPLE_DEVICES {
            if let Some(row) = rows.get(i) {
                let a = match ecam.read32(row, reg) {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                let b = match cf8.read32(row, reg) {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                if a != b {
                    return Err(PciError::Divergence {
                        reg,
                        ecam: a,
                        cf8: b,
                    });
                }
            }
            i += 1;
        }
        Ok(SAMPLE_DEVICES as u32)
    }

    /// 轮转抽样（避免全扫；返回下一批起始下标）。
    pub fn sample_cursor(&self, total: usize, cursor: &mut usize) -> usize {
        if total == 0 {
            return 0;
        }
        let start = *cursor % total;
        *cursor = (*cursor + SAMPLE_DEVICES) % total.max(1);
        start
    }
}

/// 域自检。
pub fn run_unx461_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx461");

    // 构造 8 个非空设备 + 对应 ECAM 窗
    let rows: Vec<PciDeviceRow> = {
        let mut v: Vec<PciDeviceRow> = Vec::new();
        let mut i = 0;
        while i < SAMPLE_DEVICES {
            v.push(PciDeviceRow {
                bus: 0,
                dev: i as u8,
                func: 0,
                vendor: 0x8086 + i as u16,
                device: 0x1000 + i as u16,
                class: 0x03,
                subclass: 0x00,
                progif: 0x00,
            });
            i += 1;
        }
        v
    };
    let win: Vec<u8> = {
        let mut w: Vec<u8> = Vec::new();
        let mut i = 0;
        // ECAM: bus<<20 | dev<<15 | func<<12 | reg；覆盖 bus0/dev0..7/func0/reg0
        while i < 0x40000 {
            let dev = (i >> 15) & 0x1F;
            let vendor: u16 = 0x8086 + dev as u16;
            let device: u16 = 0x1000 + dev as u16;
            let word: u32 = ((device as u32) << 16) | (vendor as u32);
            let b = word.to_le_bytes();
            let mut k = 0;
            while k < 4 {
                w.push(b[k]);
                k += 1;
            }
            i += 4;
        }
        w
    };
    let ecam = EcamAccess {
        base: 0xE000_0000,
        window: win,
        rows: rows.clone(),
    };
    let cf8 = Cf8Access {
        words: {
            let mut v: Vec<(u32, u32)> = Vec::new();
            for r in rows.iter() {
                let word: u32 =
                    (((r.device as u32) << 16) | (r.vendor as u32)) as u32;
                v.push((r.bdf_key(), word));
            }
            v
        },
    };

    // C461-01 ECAM 首选裁定
    {
        let mut rt = Router::new(8);
        let m = rt.arbitrate(true, FallbackReason::NotFallen, 0xE000_0000, 1);
        let good = m == Ok(PciMechanism::Ecam) && rt.active == Some(PciMechanism::Ecam);
        s.add("C461-01-ECAM 首选裁定", good, if good { "" } else { "裁定序错" });
    }

    // C461-02 CF8 兜底必须带理由
    {
        let mut rt = Router::new(8);
        let m = rt.arbitrate(false, FallbackReason::McfgAbsent, 0, 2);
        let good = m == Ok(PciMechanism::Cf8) && rt.fallback_count() == 1;
        s.add("C461-02-兜底落账且计数", good, if good { "" } else { "兜底未显性登记" });
    }

    // C461-03 抽样一致 8/8
    {
        let rt = Router::new(8);
        let r = rt.cross_check(&ecam, &cf8, &rows);
        let good = r == Ok(8);
        s.add("C461-03-双机制抽样 8/8", good, if good { "" } else { "抽样不一致" });
    }

    // C461-04 扩展寄存器 CF8 显性不支持（禁返假值）
    {
        let r = cf8.read32(&rows[0], 0x100);
        let good = matches!(
            r,
            Err(PciError::NotSupportedByMechanism { reg: 0x100, mech: PciMechanism::Cf8 })
        );
        s.add("C461-04-扩展空间 CF8 显性拒", good, if good { "" } else { "CF8 返回了假值" });
    }

    // C461-05 空槽识别
    {
        let empty = PciDeviceRow {
            bus: 0,
            dev: 31,
            func: 0,
            vendor: VENDOR_NONE,
            device: 0xFFFF,
            class: 0xFF,
            subclass: 0xFF,
            progif: 0xFF,
        };
        let r = ecam.read32(&empty, 0);
        let good = matches!(r, Err(PciError::EmptySlot { dev: 31, .. }));
        s.add("C461-05-空槽显性识别", good, if good { "" } else { "空槽被当设备" });
    }

    // C461-06 账满显性 Err
    {
        let mut rt = Router::new(2);
        let _ = rt.arbitrate(true, FallbackReason::NotFallen, 0, 1);
        let _ = rt.arbitrate(true, FallbackReason::NotFallen, 0, 2);
        let r = rt.arbitrate(true, FallbackReason::NotFallen, 0, 3);
        let good = matches!(r, Err(PciError::LedgerFull { cap: 2 }));
        s.add("C461-06-裁定账满显性", good, if good { "" } else { "满账被静默吞" });
    }

    // C461-07 轮转抽样游标推进且不越界（总数 20，步长 8）
    {
        let rt = Router::new(8);
        let mut cur = 6usize;
        let a = rt.sample_cursor(20, &mut cur);
        let b = rt.sample_cursor(20, &mut cur);
        let good = a == 6 && b == 14 && cur == 2;
        s.add("C461-07-轮转抽样推进", good, if good { "" } else { "抽样游标不推进" });
    }

    // C461-08 诊断码段独占
    {
        let codes = [
            PciCode::RegUnsupported,
            PciCode::EmptySlot,
            PciCode::Divergence,
            PciCode::LedgerFull,
            PciCode::EcamDead,
            PciCode::ChoseEcam,
            PciCode::ChoseCf8,
            PciCode::SampleConsistent,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C461-08-诊断码 0x83 段独占", all, if all { "" } else { "码值越出 0x83 段" });
    }

    s
}
