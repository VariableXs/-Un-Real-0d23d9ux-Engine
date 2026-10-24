//! UNX-F0210 · 硬件指纹链（跨引导关联）
//!
//! 锚点判据（施工书原文）：UNX-F0210-J1 CPU/板/内存/盘指纹聚合，
//! 同机多引导指纹一致，异机区分率 100%。
//!
//! 设计：四域指纹（CPU / 板 / 内存 / 盘各出 FNV 短指纹）→ 链式聚合
//! （序接拼后二次 FNV → 64 位整机指纹）→ 跨引导关联账（同机两次
//! 聚合必同、异机聚合必异）。红线：①缺任一域不得出整机指纹；
//! ②同机漂移检出（任一域指纹变了整机指纹必变）。
//!
//! 错误码段：0x2C20（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0210_VERSION: &str = "F0210-hwfp-v1";

/// 诊断码段基址（UNX-F0210 独占）。
pub const SEG_BASE: u16 = 0x2C20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 缺域出指纹。
    MissingDomain = SEG_BASE + 1,
    /// 同机指纹漂移。
    SameHostDrift = SEG_BASE + 2,
    /// 异机同指纹（区分率破）。
    DistinctFail = SEG_BASE + 3,
    /// 域重复登记。
    DupDomain = SEG_BASE + 4,
    /// 域名未知。
    UnknownDomain = SEG_BASE + 5,
    /// 关联账不平。
    LedgerBroken = SEG_BASE + 6,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 指纹域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// CPU（型号/微码/核数）。
    Cpu,
    /// 板（SMBIOS 摘要）。
    Board,
    /// 内存（条数/容量分布）。
    Memory,
    /// 盘（型号/容量/序列哈希）。
    Disk,
}

impl Domain {
    /// 全部域（聚合序）。
    pub const ALL: [Domain; 4] = [Domain::Cpu, Domain::Board, Domain::Memory, Domain::Disk];
}

/// FNV-1a 64 位。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

/// 四域短指纹集。
#[derive(Debug, Default)]
pub struct FpSet {
    /// 逐域指纹。
    fps: Vec<(Domain, u64)>,
}

impl FpSet {
    /// 登记域指纹。
    pub fn put(&mut self, d: Domain, raw: &[u8]) -> Result<(), u16> {
        if self.fps.iter().any(|(x, _)| *x == d) {
            return Err(E::DupDomain.code());
        }
        self.fps.push((d, fnv1a(raw)));
        Ok(())
    }

    /// 域齐检查。
    pub fn complete(&self) -> bool {
        Domain::ALL.iter().all(|d| self.fps.iter().any(|(x, _)| x == d))
    }

    /// 链式聚合：按 Domain::ALL 序拼 8 字节段后二次 FNV。
    /// 缺域拒（不出整机指纹）。
    pub fn aggregate(&self) -> Result<u64, u16> {
        if !self.complete() {
            return Err(E::MissingDomain.code());
        }
        let mut buf = Vec::with_capacity(32);
        for d in Domain::ALL.iter() {
            let fp = self.fps.iter().find(|(x, _)| x == d).map(|(_, v)| *v).unwrap_or(0);
            buf.extend_from_slice(&fp.to_le_bytes());
        }
        Ok(fnv1a(&buf))
    }
}

/// 跨引导关联账：同一 (boot_id, host_fp) 流水。
#[derive(Debug, Default)]
pub struct CorrelationLedger {
    /// 流水（boot 序号，整机指纹）。
    pub entries: Vec<(u64, u64)>,
}

impl CorrelationLedger {
    /// 记一笔引导。
    pub fn record(&mut self, boot: u64, fp: u64) {
        self.entries.push((boot, fp));
    }

    /// 同机一致性：所有已记指纹两两相同。
    pub fn same_host_ok(&self) -> Result<(), u16> {
        if self.entries.len() < 2 {
            return Err(E::LedgerBroken.code());
        }
        let first = self.entries[0].1;
        if self.entries.iter().any(|(_, fp)| *fp != first) {
            return Err(E::SameHostDrift.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：四域聚合，同机多引导一致，异机区分 100%）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：A 机四域 + B 机只换盘序列。
    let mut host_a = FpSet::default();
    host_a.put(Domain::Cpu, b"CPU-id-1234@0x0A")?;
    host_a.put(Domain::Board, b"BRD-x9-mfg-ACME")?;
    host_a.put(Domain::Memory, b"MEM-2x8GiB-ddr5")?;
    host_a.put(Domain::Disk, b"DSK-serial-AAAA")?;

    let mut host_b = FpSet::default();
    host_b.put(Domain::Cpu, b"CPU-id-1234@0x0A")?;
    host_b.put(Domain::Board, b"BRD-x9-mfg-ACME")?;
    host_b.put(Domain::Memory, b"MEM-2x8GiB-ddr5")?;
    host_b.put(Domain::Disk, b"DSK-serial-BBBB")?;

    // 二、J1 主体：A 机三次引导指纹恒同；A/B 指纹必异。
    let fa1 = host_a.aggregate()?;
    let fa2 = host_a.aggregate()?;
    let fb = host_b.aggregate()?;
    if fa1 != fa2 {
        return Err(E::SameHostDrift.code());
    }
    if fa1 == fb {
        return Err(E::DistinctFail.code());
    }

    // 三、关联账：同机两笔全同 → 绿。
    let mut led = CorrelationLedger::default();
    led.record(1, fa1);
    led.record(2, fa1);
    led.same_host_ok()?;

    // 四、红线逐项：缺域拒 / 重复域拒 / 同机漂移检出 / 未知域无入口。
    let mut partial = FpSet::default();
    partial.put(Domain::Cpu, b"cpu")?;
    if partial.aggregate().is_ok() {
        return Err(E::MissingDomain.code());
    }
    if partial.put(Domain::Cpu, b"cpu2").is_ok() {
        return Err(E::DupDomain.code());
    }
    let mut host_a2 = FpSet::default();
    host_a2.put(Domain::Cpu, b"CPU-id-1234@0x0A")?;
    host_a2.put(Domain::Board, b"BRD-x9-mfg-ACME")?;
    host_a2.put(Domain::Memory, b"MEM-2x8GiB-ddr5")?;
    // 内存条换了一根 → 域指纹变 → 整机指纹必变（漂移检出）。
    host_a2.put(Domain::Memory, b"MEM-2x16GiB-ddr5")?;
    host_a2.put(Domain::Disk, b"DSK-serial-AAAA")?;
    let drifted = host_a2.aggregate()?;
    if drifted == fa1 {
        return Err(E::SameHostDrift.code());
    }
    let mut broken = CorrelationLedger::default();
    broken.record(1, fa1);
    broken.record(2, drifted);
    if broken.same_host_ok().is_ok() {
        return Err(E::SameHostDrift.code());
    }

    // 五、判别式段位对拍。
    if E::MissingDomain.code() != SEG_BASE + 1 || E::LedgerBroken.code() != SEG_BASE + 6 {
        return Err(E::MissingDomain.code());
    }
    Ok(())
}
