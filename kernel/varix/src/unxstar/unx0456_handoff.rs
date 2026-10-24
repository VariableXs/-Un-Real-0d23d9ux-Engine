//! unx0456_handoff —— UNX-F0456 ACPI 交接字段（A2 预备）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0456 · ACPI 交接字段 · 目标 240 行
//! 判据原文：UNX-F0456-J1 A2 交接信包 ACPI 面：信包 64 字
//!           节定长，字段布局——magic 4B（"ACPI"）+ 版本
//!           1B + 旗 1B + 表数 2B + RSDP 物理 8B + 首表指
//!           针 8B + 校验和 4B（信包 0..60 四字节和的 32 位
//!           取负）+ 保留 36B（必须全零）；账二 布局核：保
//!           留区非零 → 拒（版本升级兼容性闸）；magic 非
//!           "ACPI" → 拒；账三 校验和核：信包和自洽；账四
//!           表数核：表数与快照行数一致（0451 消费）；账五
//!           RSDP 物理指针非零且页对齐（低 12 位零）。
//! 组件：信包编解码器 + 布局闸 + 校验和器 + 表数对冲核。
//! 职责定位：交接信包是启动链的「接力棒」——boot 阶段找到
//!   的 ACPI 世界必须原封不动传给内核主体；字段布局一错，
//!   后手拿到的 RSDP 指针就是垃圾地址，等于把地图撕了再交
//!   给下一棒。
//! 性能逐项分解：编解码 O(1)；校验和 O(64/4)。
//! 诊断码段：0x34340（0x34341~0x34349 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34340 起）。
pub const SEG_BASE: u32 = 0x34340;
pub const XC_MAGIC: u32 = SEG_BASE + 1; // magic 非 "ACPI"
pub const XC_RSVD: u32 = SEG_BASE + 2; // 保留区非零
pub const XC_CSUM: u32 = SEG_BASE + 3; // 信包校验和坏
pub const XC_RSDP: u32 = SEG_BASE + 4; // RSDP 指针零/非对齐
pub const XC_TBCNT: u32 = SEG_BASE + 5; // 表数对冲不一致

/// 信包定长 64 字节。
pub const PKT_LEN: usize = 64;
/// magic 四字节。
pub const MAGIC: [u8; 4] = *b"ACPI";
/// 保留区起点（60）与长度（4…64）。布局：0..4 magic、4 版本、
/// 5 旗、6..8 表数、8..16 rsdp、16..24 首表、24..28 校验和、
/// 28..60 扩展保留、60..64 尾保留。
pub const RSVD_LO: usize = 28;
pub const RSVD_HI: usize = 64;

/// A2 交接信包（结构投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandPkt {
    pub version: u8,
    pub flags: u8,
    pub table_count: u16,
    pub rsdp_phys: u64,
    pub first_table: u64,
}

impl HandPkt {
    /// 编码为 64 字节信包（含校验和）。
    pub fn encode(&self) -> [u8; PKT_LEN] {
        let mut raw = [0u8; PKT_LEN];
        raw[0..4].copy_from_slice(&MAGIC);
        raw[4] = self.version;
        raw[5] = self.flags;
        raw[6..8].copy_from_slice(&self.table_count.to_le_bytes());
        raw[8..16].copy_from_slice(&self.rsdp_phys.to_le_bytes());
        raw[16..24].copy_from_slice(&self.first_table.to_le_bytes());
        // 校验和：0..60 四字节 32 位和取负 @24。
        let sum = pkt_sum32(&raw);
        raw[24..28].copy_from_slice(&sum.wrapping_neg().to_le_bytes());
        raw
    }

    /// 布局 + 语义全核。
    pub fn validate(raw: &[u8; PKT_LEN]) -> Result<(), u32> {
        if raw[0..4] != MAGIC {
            return Err(XC_MAGIC);
        }
        if raw[RSVD_LO..RSVD_HI].iter().any(|b| *b != 0) {
            return Err(XC_RSVD);
        }
        let sum = u32::from_le_bytes([raw[24], raw[25], raw[26], raw[27]]);
        if pkt_sum32(raw).wrapping_add(sum) != 0 {
            return Err(XC_CSUM);
        }
        let rsdp = u64::from_le_bytes([
            raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15],
        ]);
        if rsdp == 0 || rsdp & 0xFFF != 0 {
            return Err(XC_RSDP);
        }
        Ok(())
    }

    /// 从信包解码（布局通过后调用）。
    pub fn decode(raw: &[u8; PKT_LEN]) -> HandPkt {
        let rsdp = u64::from_le_bytes([
            raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15],
        ]);
        let first = u64::from_le_bytes([
            raw[16], raw[17], raw[18], raw[19], raw[20], raw[21], raw[22], raw[23],
        ]);
        HandPkt {
            version: raw[4],
            flags: raw[5],
            table_count: u16::from_le_bytes([raw[6], raw[7]]),
            rsdp_phys: rsdp,
            first_table: first,
        }
    }
}

/// 信包 32 位和：0..60 区按四字节小端累加。
pub fn pkt_sum32(raw: &[u8; PKT_LEN]) -> u32 {
    let mut s = 0u32;
    let mut off = 0usize;
    while off < 60 {
        let w = u32::from_le_bytes([raw[off], raw[off + 1], raw[off + 2], raw[off + 3]]);
        s = s.wrapping_add(w);
        off += 4;
    }
    s
}

/// 表数对冲核：信包表数 vs 快照行数。
pub fn tbl_reconcile(raw: &[u8; PKT_LEN], snap_rows: usize) -> Result<(), u32> {
    let p = HandPkt::decode(raw);
    if p.table_count as usize != snap_rows {
        return Err(XC_TBCNT);
    }
    Ok(())
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0456 全族核。
pub fn run_unx0456_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0456");

    // 族一：正常编码/解码往返。
    let p0 = HandPkt { version: 2, flags: 0x01, table_count: 5, rsdp_phys: 0xE000_0000, first_table: 0xE000_1000 };
    let raw0 = p0.encode();
    let back = HandPkt::decode(&raw0);
    cs.add("往返全等", back == p0, "编解码闭环");
    cs.add("magic 就位", raw0[0..4] == MAGIC, "ACPI");
    cs.add("保留区全零", raw0[RSVD_LO..RSVD_HI].iter().all(|b| *b == 0), "布局纪律");
    cs.add("布局核过", HandPkt::validate(&raw0).is_ok(), "全闸");

    // 族二：校验和自洽。
    let sum = pkt_sum32(&raw0);
    let chk = u32::from_le_bytes([raw0[24], raw0[25], raw0[26], raw0[27]]);
    cs.add("和 + 补 = 0", sum.wrapping_add(chk) == 0, "32 位取负");

    // 族三：magic 破坏。
    let mut r1 = raw0;
    r1[0] = b'X';
    cs.add("magic 破拒", HandPkt::validate(&r1) == Err(XC_MAGIC), "身份闸");

    // 族四：保留区污染。
    let mut r2 = raw0;
    r2[30] = 1;
    cs.add("保留污染拒", HandPkt::validate(&r2) == Err(XC_RSVD), "兼容性闸");
    let mut r3 = raw0;
    r3[63] = 0xFF;
    cs.add("尾保留拒", HandPkt::validate(&r3) == Err(XC_RSVD), "尾字节");

    // 族五：校验和破坏。
    let mut r4 = raw0;
    r4[10] ^= 0x40; // rsdp 字段中位翻转 → 校验和不再自洽
    cs.add("校验和破拒", HandPkt::validate(&r4) == Err(XC_CSUM), "内容闸");

    // 族六：RSDP 指针闸。
    let p1 = HandPkt { rsdp_phys: 0, ..p0 };
    let r5 = p1.encode();
    cs.add("RSDP 零拒", HandPkt::validate(&r5) == Err(XC_RSDP), "空指针");
    let p2 = HandPkt { rsdp_phys: 0xE000_0ABC, ..p0 };
    let r6 = p2.encode();
    cs.add("非对齐拒", HandPkt::validate(&r6) == Err(XC_RSDP), "低 12 位");

    // 族七：表数对冲。
    cs.add("表数一致", tbl_reconcile(&raw0, 5).is_ok(), "5 行快照");
    cs.add("表数不齐拒", tbl_reconcile(&raw0, 4) == Err(XC_TBCNT), "4 行快照");
    let p3 = HandPkt { table_count: 0, ..p0 };
    let r7 = p3.encode();
    cs.add("零表数对冲", tbl_reconcile(&r7, 0).is_ok(), "空快照合法");

    // 族八：和器单元（全零包和为 0）。
    let z = [0u8; PKT_LEN];
    cs.add("全零包和零", pkt_sum32(&z) == 0, "基线");

    // 族九：多字段翻转检出（字节级解码路径核——不依赖布局字段直读）。
    let mut r8 = raw0;
    r8[20] ^= 0x20; // first_table 字段位翻转
    cs.add("翻转检出", HandPkt::validate(&r8) == Err(XC_CSUM), "校验和兜住");

    cs
}
