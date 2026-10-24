//! unx0374_e820cross —— UNX-F0374 内存盘点交叉验证（E820 vs 自检段）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0374 · 盘点交叉验证 · 目标 240 行
//! 判据原文：UNX-F0374-J1 E820 表与自检段（内存探测写参）交叉：
//!           两源对同一区间的可用/保留分类一致率出账；分歧区
//!           间逐条立案（以保守侧为准——分歧时按保留处理）；
//!           E820 语义闭合（类型闭集 + 区间升序无叠）。
//! 组件：E820 行（区间 + 类型闭集）+ 自检行（区间 + 可用位）+
//!       交叉引擎（分类一致率 + 分歧立案 + 保守裁决）。
//! 职责定位：双源交叉是「内存地图的防伪验证」——E820 是固件
//!   说的话，自检段是探测出来的实情；两者分歧时（固件漏报
//!   保留区最常见）按保守侧裁决：宁可浪费一段内存，不可把
//!   保留区当可用（把 ACPI 表分配出去 = 三天后的幽灵 bug）。
//! 性能逐项分解：交叉 O(N×M)；裁决 O(分歧数)；一致率 O(1)。
//! 诊断段基址：0x20A00（0x20A01~0x20A09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const EC_VERSION: &str = "unx0374_e820cross-v1";

/// 诊断段基址（0x20A00 段 · 本模块独占）。
pub const EC_SEG_BASE: u32 = 0x20A00;
pub const XC_TYPE_OOR: u32 = 0x20A01; // E820 类型越域
pub const XC_RANGE_BAD: u32 = 0x20A02; // 区间倒挂
pub const XC_OVERLAP: u32 = 0x20A03; // E820 区间重叠
pub const XC_ORDER_OOO: u32 = 0x20A04; // 区间乱序
pub const XC_EMPTY_SRC: u32 = 0x20A05; // 空源拒
pub const XC_CONSERVATIVE: u32 = 0x20A06; // 保守裁决立案
pub const XC_RATE_OOR: u32 = 0x20A07; // 一致率越域
pub const XC_DISAGREE: u32 = 0x20A08; // 分歧存在
pub const XC_DUP_RANGE: u32 = 0x20A09; // 区间重复

/// E820 类型闭集。
pub const T_USABLE: u8 = 1; // 可用 RAM
pub const T_RESERVED: u8 = 2; // 保留
pub const T_ACPI: u8 = 3; // ACPI 可回收
pub const T_NVS: u8 = 4; // ACPI NVS
pub const T_BAD: u8 = 5; // 坏内存

/// E820 行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct E820Row {
    pub base: u32,
    pub len: u32,
    pub kind: u8,
}

impl E820Row {
    pub fn usable(&self) -> bool {
        self.kind == T_USABLE
    }

    fn valid(&self) -> Result<(), u32> {
        if self.len == 0 {
            return Err(XC_RANGE_BAD);
        }
        if !matches!(self.kind, T_USABLE | T_RESERVED | T_ACPI | T_NVS | T_BAD) {
            return Err(XC_TYPE_OOR);
        }
        Ok(())
    }
}

/// 自检行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeRow {
    pub base: u32,
    pub len: u32,
    /// 探测可用位。
    pub usable: bool,
}

/// E820 表闭合核：类型闭集 + 升序无叠。
pub fn e820_close(rows: &[E820Row]) -> Result<(), u32> {
    if rows.is_empty() {
        return Err(XC_EMPTY_SRC);
    }
    for (i, r) in rows.iter().enumerate() {
        r.valid()?;
        if i > 0 {
            if r.base <= rows[i - 1].base {
                return Err(XC_ORDER_OOO);
            }
            if r.base < rows[i - 1].base + rows[i - 1].len {
                return Err(XC_OVERLAP);
            }
        }
    }
    Ok(())
}

/// 交叉判定：给定地址的 E820 可用性 vs 自检可用性。
fn probe_usable_at(probes: &[ProbeRow], addr: u32) -> bool {
    probes.iter().any(|p| addr >= p.base && addr < p.base + p.len && p.usable)
}

/// 交叉引擎：以 1MB 步进采样比较，输出分歧区间清单。
/// 保守裁决：分歧地址按保留处理（E820 可用但探测非可用 → 砍）。
pub fn cross_check(e820: &[E820Row], probes: &[ProbeRow]) -> Result<(u32, Vec<(u32, u32)>), u32> {
    if e820.is_empty() || probes.is_empty() {
        return Err(XC_EMPTY_SRC);
    }
    e820_close(e820)?;
    let mut total = 0u32;
    let mut agree = 0u32;
    let mut disputes: Vec<(u32, u32)> = Vec::new();
    for r in e820.iter() {
        // 步进采样每 MB。
        let mut off = 0u32;
        while off < r.len {
            let addr = r.base + off;
            let e_usable = r.usable();
            let p_usable = probe_usable_at(probes, addr);
            total += 1;
            if e_usable == p_usable {
                agree += 1;
            } else {
                disputes.push((addr, if e_usable { XC_CONSERVATIVE } else { XC_DISAGREE }));
            }
            off += 0x100_000;
        }
    }
    if total == 0 {
        return Err(XC_EMPTY_SRC);
    }
    let rate = ((agree as u64) * 1000 / total as u64) as u32;
    if rate > 1000 {
        return Err(XC_RATE_OOR);
    }
    Ok((rate, disputes))
}

/// 保守可用量：E820 可用 ∩ 探测可用（分歧砍掉）。
pub fn conservative_usable_bytes(e820: &[E820Row], probes: &[ProbeRow]) -> Result<u64, u32> {
    if e820.is_empty() || probes.is_empty() {
        return Err(XC_EMPTY_SRC);
    }
    let mut total: u64 = 0;
    for r in e820.iter().filter(|r| r.usable()) {
        let mut off = 0u32;
        while off < r.len {
            let addr = r.base + off;
            if probe_usable_at(probes, addr) {
                total += 0x100_000;
            }
            off += 0x100_000;
        }
    }
    Ok(total)
}

pub fn run_unx0374_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0374_e820cross");
    // 族一：E820 闭合。
    let e820 = vec![
        E820Row { base: 0, len: 0x9_FC00, kind: T_USABLE },
        E820Row { base: 0x9_FC00, len: 0x400, kind: T_RESERVED },
        E820Row { base: 0x100_000, len: 0x100_000, kind: T_USABLE },
        E820Row { base: 0xF_0000, len: 0x1_0000, kind: T_RESERVED },
    ];
    cs.add("闭合过", e820_close(&e820).is_ok(), "升序无叠");
    cs.add("空表拒", e820_close(&[]).is_err(), "EMPTY");
    cs.add("乱序拒", e820_close(&[
        E820Row { base: 0x200_000, len: 0x1000, kind: T_USABLE },
        E820Row { base: 0x100_000, len: 0x1000, kind: T_USABLE },
    ]).is_err(), "ORDER");
    cs.add("重叠拒", e820_close(&[
        E820Row { base: 0x100_000, len: 0x2_0000, kind: T_USABLE },
        E820Row { base: 0x10_F000, len: 0x1000, kind: T_USABLE },
    ]).is_err(), "OVERLAP");
    cs.add("类型越域拒", E820Row { base: 0, len: 1, kind: 9 }.valid().is_err(), "TYPE");
    cs.add("零长拒", E820Row { base: 0, len: 0, kind: T_USABLE }.valid().is_err(), "RANGE");
    // 族二：全一致交叉。
    let probes_ok = vec![
        ProbeRow { base: 0, len: 0x10_0000, usable: true },
        ProbeRow { base: 0x100_000, len: 0x1_0000, usable: true },
    ];
    // 注意 0x9FC00..0x100000 的 E820 保留段采样点 0x9FC00（0x9FC00 < 0x100000
    // 但 probes[0] 声称 0..1MB 全可用 → 分歧）→ 用精确 probe 分段。
    let probes_fit = vec![
        ProbeRow { base: 0, len: 0x9_FC00, usable: true },
        ProbeRow { base: 0x100_000, len: 0x1_0000, usable: true },
    ];
    let (rate, disp) = match cross_check(&e820, &probes_fit) {
        Ok(x) => x,
        Err(_) => {
            cs.add("交叉", false, "被拒");
            return cs;
        }
    };
    cs.add("全一致", rate == 1000 && disp.is_empty(), "零分歧");
    let _ = probes_ok;
    // 族三：分歧立案与保守裁决。
    // E820 说 0x100000..0x110000 可用，探测说那里是保留（固件漏报）。
    let probes_bad = vec![
        ProbeRow { base: 0, len: 0x9_FC00, usable: true },
        ProbeRow { base: 0x100_000, len: 0x1_0000, usable: false },
    ];
    let (rate2, disp2) = cross_check(&e820, &probes_bad).unwrap_or((0, Vec::new()));
    cs.add("分歧立案", !disp2.is_empty(), "1MB 采样点分歧");
    cs.add("一致率降", rate2 < 1000, "存在分歧");
    cs.add("保守标记", disp2.iter().any(|&(_, k)| k == XC_CONSERVATIVE), "分歧按保留");
    let usable = conservative_usable_bytes(&e820, &probes_bad).unwrap_or(0);
    cs.add("保守砍量", usable == 0x10_0000, "1MB 高区被砍，低区 1 步保留");
    // 族四：反向分歧（探测可用但 E820 保留）。
    let probes_rev = vec![
        ProbeRow { base: 0, len: 0x10_0000, usable: true },
        ProbeRow { base: 0x100_000, len: 0x1_0000, usable: true },
    ];
    let (_, disp3) = cross_check(&e820, &probes_rev).unwrap_or((0, Vec::new()));
    cs.add("反向分歧", disp3.iter().any(|&(_, k)| k == XC_DISAGREE), "保留区被探可用");
    // 族五：构造闸。
    cs.add("空源拒", cross_check(&[], &probes_fit).is_err(), "EMPTY");
    cs.add("空探拒", cross_check(&e820, &[]).is_err(), "EMPTY");
    cs
}
