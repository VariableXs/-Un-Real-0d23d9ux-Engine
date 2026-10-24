//! unx0443_rsdtwalk —— UNX-F0443 RSDT/XSDT 表指针遍历。
//! 锚点（CoRun Unxreal 主册）：UNX-F0443 · RSDT/XSDT 表指针遍历 · 目标 240 行
//! 判据原文：UNX-F0443-J1 根表双代遍历：RSDT（32 位指针族，
//!           签名 "RSDT"，条目 = 表基 + 36 起每 4 字节一个
//!           u32 指针）与 XSDT（64 位指针族，签名 "XSDT"，
//!           条目 = 表基 + 36 起每 8 字节一个 u64 指针）；
//!           代际判别 = 根表签名逐字节比（RSDT 走 4 字节族，
//!           XSDT 走 8 字节族，混走立案）；遍历产出 = 按序
//!           指针清单（含签名解码，四字符大端）；越界核：
//!           指针清单数不得超过 (表长-36)/步长（越表读立案）；
//!           空表核：条目数 0 合法（产出空清单）；重放核：
//!           两次遍历清单逐项同。
//! 组件：根表签名解码器 + 双代指针遍历器 + 越界闸 + 签名
//!       清单导出 + 重放核。
//! 职责定位：RSDT/XSDT 是 ACPI 的「目录页」——后续一切表
//!   （MADT/HPET/FADT/MCFG/DSDT）都要从目录页按指针找到；
//!   走错代（RSDT 表按 8 字节读）会把两个指针拼成一个垃圾
//!   地址；越界读是目录页残缺时的无声错账，必须立案。
//! 性能逐项分解：代判 O(1)；遍历 O(N)；越界核 O(1)；重放
//!   O(N)。
//! 诊断码段：0x34000（0x34001~0x34009 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RW_VERSION: &str = "unx0443_rsdtwalk-v1";

/// 诊断段基址（0x34000 段 · 本模块独占）。
pub const RW_SEG_BASE: u32 = 0x34000;
pub const XC_SIG_UNKNOWN: u32 = 0x34001; // 根表签名不识
pub const XC_PTR_OOB: u32 = 0x34002; // 指针读越表
pub const XC_MIXED_GEN: u32 = 0x34003; // 混代读
pub const XC_HDR_SHORT: u32 = 0x34004; // 表头不足 36 字节
pub const XC_REPLAY: u32 = 0x34005; // 重放不一致
pub const XC_NULL_PTR: u32 = 0x34006; // 目录内零指针
pub const XC_MAGIC: u32 = 0x34009; // 账魔数破

/// ACPI 表头公共区长度（至 Length 字段末）。
pub const HDR_SZ: usize = 36;
/// 签名四字符。
pub const SIG_RSDT: [u8; 4] = *b"RSDT";
pub const SIG_XSDT: [u8; 4] = *b"XSDT";

/// 四字符签名解码（大端拼 uint32，供清单导出可读化）。
pub fn sig_u32(sig: &[u8; 4]) -> u32 {
    ((sig[0] as u32) << 24) | ((sig[1] as u32) << 16) | ((sig[2] as u32) << 8) | sig[3] as u32
}

/// 模拟 ACPI 表：字节阵列 + 表头字段。
pub struct AcpiTable {
    /// 原始字节（头 36 字节 + 条目区）。
    pub raw: Vec<u8>,
}

impl AcpiTable {
    /// 造根表：sig = RSDT/XSDT，ptrs = 条目指针清单（自动按
    /// 代际宽度编码进条目区）。
    pub fn root(sig: &[u8; 4], ptrs: &[u64]) -> Self {
        let width = if *sig == SIG_XSDT { 8 } else { 4 };
        let mut raw = vec![0u8; HDR_SZ + ptrs.len() * width];
        raw[0..4].copy_from_slice(sig);
        let len = raw.len() as u32;
        raw[4..8].copy_from_slice(&len.to_le_bytes());
        for (i, p) in ptrs.iter().enumerate() {
            let off = HDR_SZ + i * width;
            if width == 8 {
                raw[off..off + 8].copy_from_slice(&p.to_le_bytes());
            } else {
                raw[off..off + 4].copy_from_slice(&(*p as u32).to_le_bytes());
            }
        }
        AcpiTable { raw }
    }

    /// 表长字段回读。
    pub fn declared_len(&self) -> Result<u32, u32> {
        if self.raw.len() < 8 {
            return Err(XC_HDR_SHORT);
        }
        Ok(u32::from_le_bytes([self.raw[4], self.raw[5], self.raw[6], self.raw[7]]))
    }

    /// 签名回读。
    pub fn sig(&self) -> Result<[u8; 4], u32> {
        if self.raw.len() < 4 {
            return Err(XC_HDR_SHORT);
        }
        Ok([self.raw[0], self.raw[1], self.raw[2], self.raw[3]])
    }
}

/// 遍历产出项：子表物理指针 + 预期签名位（此处只管指针）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// 子表指针（RSDT 族高 32 位恒零）。
    pub ptr: u64,
}

/// 遍历器：签名判代 → 指针清单提取 → 越界/零指针核。
pub struct RsdtWalker;

impl RsdtWalker {
    /// 代际判别：RSDT → 4，XSDT → 8，他签名 → 立案。
    pub fn gen_width(sig: &[u8; 4]) -> Result<usize, u32> {
        if *sig == SIG_RSDT {
            Ok(4)
        } else if *sig == SIG_XSDT {
            Ok(8)
        } else {
            Err(XC_SIG_UNKNOWN)
        }
    }

    /// 遍历：产出指针清单。
    pub fn walk(t: &AcpiTable) -> Result<Vec<Entry>, u32> {
        let sig = t.sig()?;
        let width = Self::gen_width(&sig)?;
        if t.raw.len() < HDR_SZ {
            return Err(XC_HDR_SHORT);
        }
        let declared = t.declared_len()? as usize;
        // 越界闸：declared 不得小于头、不得超过实际缓冲。
        if declared < HDR_SZ || declared > t.raw.len() {
            return Err(XC_PTR_OOB);
        }
        let cap = (declared - HDR_SZ) / width;
        let mut out = Vec::new();
        for i in 0..cap {
            let off = HDR_SZ + i * width;
            let ptr = if width == 8 {
                u64::from_le_bytes([
                    t.raw[off], t.raw[off + 1], t.raw[off + 2], t.raw[off + 3],
                    t.raw[off + 4], t.raw[off + 5], t.raw[off + 6], t.raw[off + 7],
                ])
            } else {
                u32::from_le_bytes([t.raw[off], t.raw[off + 1], t.raw[off + 2], t.raw[off + 3]]) as u64
            };
            if ptr == 0 {
                return Err(XC_NULL_PTR);
            }
            // RSDT 族指针必须在 32 位域内（混代读防护）。
            if width == 4 && ptr > 0xFFFF_FFFF {
                return Err(XC_MIXED_GEN);
            }
            out.push(Entry { ptr });
        }
        Ok(out)
    }

    /// 重放核：两次遍历逐项同。
    pub fn replay(t: &AcpiTable) -> Result<(), u32> {
        let a = Self::walk(t)?;
        let b = Self::walk(t)?;
        if a == b {
            Ok(())
        } else {
            Err(XC_REPLAY)
        }
    }
}

/// 判据入口：UNX-F0443。
pub fn run_unx0443_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0443_rsdtwalk");
    // 族一：代际判别。
    cs.add("RSDT 走 4", RsdtWalker::gen_width(&SIG_RSDT) == Ok(4), "32 位族");
    cs.add("XSDT 走 8", RsdtWalker::gen_width(&SIG_XSDT) == Ok(8), "64 位族");
    cs.add("野签名拒", RsdtWalker::gen_width(b"FACP") == Err(XC_SIG_UNKNOWN), "非根表");
    // 族二：RSDT 遍历。
    let rsdt = AcpiTable::root(&SIG_RSDT, &[0x10_0000, 0x10_1000, 0x10_2000]);
    let l1 = RsdtWalker::walk(&rsdt);
    cs.add("RSDT 三项", l1.as_ref().map(|v| v.len()).unwrap_or(0) == 3, "逐项");
    cs.add("首项 0x100000", l1.as_ref().map(|v| v[0].ptr == 0x10_0000).unwrap_or(false), "序保");
    cs.add("高 32 位零", l1.as_ref().map(|v| v.iter().all(|e| e.ptr <= 0xFFFF_FFFF)).unwrap_or(false), "32 位域");
    // 族三：XSDT 遍历。
    let xsdt = AcpiTable::root(&SIG_XSDT, &[0x1_0000_0000, 0x1_0000_1000]);
    let l2 = RsdtWalker::walk(&xsdt);
    cs.add("XSDT 两项", l2.as_ref().map(|v| v.len()).unwrap_or(0) == 2, "逐项");
    cs.add("64 位指针通", l2.as_ref().map(|v| v[0].ptr == 0x1_0000_0000).unwrap_or(false), "高位有效");
    // 族四：空表。
    let empty = AcpiTable::root(&SIG_RSDT, &[]);
    cs.add("空表空单", RsdtWalker::walk(&empty).map(|v| v.is_empty()).unwrap_or(false), "合法");
    cs.add("空表重放", RsdtWalker::replay(&empty).is_ok(), "恒等");
    // 族五：越界闸。
    let mut bad = AcpiTable::root(&SIG_RSDT, &[0x10_0000]);
    bad.raw[4] = 0xFF; // declared_len 抬到超出实际缓冲
    bad.raw[5] = 0xFF;
    bad.raw[6] = 0xFF;
    bad.raw[7] = 0xFF;
    cs.add("declared 超缓冲拒", RsdtWalker::walk(&bad) == Err(XC_PTR_OOB), "越表读立案");
    let mut short = AcpiTable::root(&SIG_RSDT, &[]);
    short.raw.truncate(20); // 不足 36 头
    cs.add("短头拒", RsdtWalker::walk(&short) == Err(XC_HDR_SHORT), "头残");
    // 族六：零指针。
    let zeroed = AcpiTable::root(&SIG_RSDT, &[0]);
    cs.add("零指针立案", RsdtWalker::walk(&zeroed) == Err(XC_NULL_PTR), "目录空洞");
    // 族七：混代防护。
    // 造 RSDT 壳塞 8 字节读出的高值：用 XSDT 编码 + RSDT 签名不可直构
    // （构造器按签名编码），改用条目区直改。
    let mut mixed = AcpiTable::root(&SIG_RSDT, &[0x10_0000]);
    let dlen = mixed.raw.len() as u32;
    mixed.raw.resize(HDR_SZ + 8, 0); // 硬塞 8 字节条目
    mixed.raw[4..8].copy_from_slice(&((HDR_SZ as u32) + 8).to_le_bytes());
    let _ = dlen;
    mixed.raw[HDR_SZ..HDR_SZ + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // 低 32 位顶满
    mixed.raw[HDR_SZ + 4..HDR_SZ + 8].copy_from_slice(&0x1u32.to_le_bytes()); // 高位非零
    cs.add("混代高值立案", RsdtWalker::walk(&mixed) == Err(XC_MIXED_GEN), "RSDT 见 64 位值");
    // 族八：重放。
    cs.add("RSDT 重放", RsdtWalker::replay(&rsdt).is_ok(), "两次同");
    cs.add("XSDT 重放", RsdtWalker::replay(&xsdt).is_ok(), "两次同");
    // 族九：签名解码。
    cs.add("RSDT 签名值", sig_u32(&SIG_RSDT) == 0x5444_5352, "大端拼");
    cs.add("XSDT 签名值", sig_u32(&SIG_XSDT) == 0x5444_5358, "大端拼");
    // 族十：declared 自洽。
    cs.add("declared 等实际", rsdt.declared_len() == Ok(rsdt.raw.len() as u32), "构造自洽");
    cs
}
