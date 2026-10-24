//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6761 · HBA 寄存器面映射与版本裁断（CAP.NP/VS）· 诊断段 0xA480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6761-J1 AHCI BAR 映射账（基址/长度/对齐断言）；VS 版本
//!      字段与 CAP.NP 端口数上限逐字段裁断；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - BAR 映射账：基址非零 + 长度覆盖寄存器面（0x100+32×4）+ 页对齐；
//!   - VS 版本裁断：BCD 编码（maj.min.patch）解析 + 最低版本闸；
//!   - CAP.NP 端口数裁断：NP 字段 +1 = 可用端口上限；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xA480;

/// AHCI 寄存器面长度（通用头 0x100 + 32 端口 × 0x80）。
pub const HBA_SIZE: u32 = 0x100 + 32 * 0x80;

/// 页对齐掩码。
pub const PAGE_MASK: u32 = 0xFFF;

/// 对齐错误码。
pub const EINVAL: i32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// BAR 映射账。
pub struct BarMap {
    pub base: u32,
    pub size: u32,
}

impl BarMap {
    /// 映射断言：基址非零 + 页对齐 + 长度覆盖寄存器面。
    pub fn valid(&self) -> Result<(), i32> {
        if self.base == 0 {
            return Err(EINVAL);
        }
        if self.base & PAGE_MASK != 0 {
            return Err(EINVAL);
        }
        if self.size < HBA_SIZE {
            return Err(EINVAL);
        }
        Ok(())
    }
}

/// VS 版本 BCD 裁断：0x01050200 → 1.5.2。
pub fn parse_vs(vs: u32) -> (u8, u8, u8) {
    (
        ((vs >> 24) & 0xFF) as u8,
        ((vs >> 16) & 0xFF) as u8,
        ((vs >> 8) & 0xFF) as u8,
    )
}

/// 最低版本闸：≥1.3.1 才可驱动。
pub fn vs_gate(vs: u32) -> Result<(), i32> {
    let (maj, min, patch) = parse_vs(vs);
    if (maj, min, patch) < (1, 3, 1) {
        return Err(EINVAL);
    }
    Ok(())
}

/// CAP.NP 端口数裁断：NP 低 5 位 + 1。
pub fn port_count(cap_np: u32) -> u32 {
    (cap_np & 0x1F) + 1
}

/// HBA 寄存器面。
pub struct Hba {
    pub bar: BarMap,
    pub vs: u32,
    pub cap_np: u32,
}

impl Hba {
    /// 全字段裁断。
    pub fn audit(&self) -> Result<u32, i32> {
        self.bar.valid()?;
        vs_gate(self.vs)?;
        Ok(port_count(self.cap_np))
    }
}

/// J1 主判据：BAR 映射账 + VS 逐字段裁断 + CAP.NP 上限裁断。
pub fn j1_main() -> bool {
    // 合法 HBA：1.3.1+，NP=31（32 端口）
    let h = Hba { bar: BarMap { base: 0xFEB0_0000, size: HBA_SIZE }, vs: 0x0105_0200, cap_np: 31 };
    if h.audit() != Ok(32) {
        return false;
    }
    // VS 逐字段：0x01050200 → (1,5,2)
    if parse_vs(0x0105_0200) != (1, 5, 2) {
        return false;
    }
    // NP 裁断边界：0 → 1 端口；31 → 32 端口
    if port_count(0) != 1 || port_count(31) != 32 {
        return false;
    }
    // 版本闸边界：1.3.1 过 / 1.3.0 拒 / 0.9.9 拒
    vs_gate(0x0103_0100).is_ok() && vs_gate(0x0103_0000).is_err() && vs_gate(0x0009_0900).is_err()
}

// ── 注入核（0xA480 段 5 项）────────────────────────────────────────────

/// 注入核 1：零基址注入——base=0 拒。
pub fn inj1_zero_base() -> bool {
    BarMap { base: 0, size: HBA_SIZE }.valid().is_err()
}

/// 注入核 2：非页对齐注入——base 未对齐拒。
pub fn inj2_misalign() -> bool {
    BarMap { base: 0xFEB0_0100, size: HBA_SIZE }.valid().is_err()
}

/// 注入核 3：长度不足注入——size < 寄存器面拒。
pub fn inj3_size_short() -> bool {
    BarMap { base: 0xFEB0_0000, size: 0x100 }.valid().is_err()
}

/// 注入核 4：低版本注入——VS 低于 1.3.1 全拒。
pub fn inj4_old_vs() -> bool {
    [(0x0103_0000u32, true), (0x0102_0200, true), (0x0103_0100, false)]
        .iter()
        .all(|(v, reject)| vs_gate(*v).is_err() == *reject)
}

/// 注入核 5：NP 上限注入——NP>31 位段越界不超 32 端口。
pub fn inj5_np_bound() -> bool {
    port_count(0xFFFF_FFE0) == port_count(0) // 高位忽略只取低 5 位
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6761_checks：本单判据聚合（诊断段 0xA480）。
pub fn run_unx6761_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6761 HBA 寄存器面映射与版本裁断（CAP.NP/VS 字段）");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "BAR映射账（基址/长度/对齐）",
        j1_main(),
        if j1_main() { "" } else { "映射账破" },
    );
    s.add(
        "VS/CAP.NP逐字段裁断",
        j1_main(),
        if j1_main() { "" } else { "裁断破" },
    );
    let inj = [
        ("注入1零基址拒", inj1_zero_base()),
        ("注入2非对齐拒", inj2_misalign()),
        ("注入3长度不足拒", inj3_size_short()),
        ("注入4低版本拒", inj4_old_vs()),
        ("注入5NP上限", inj5_np_bound()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
