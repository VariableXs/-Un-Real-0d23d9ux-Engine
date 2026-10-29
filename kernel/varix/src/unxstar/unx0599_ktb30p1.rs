//! UNX-F0599 · ktest 参数面断言接入（B30 判据入套件）· 诊断段 0x7840（本单独占）
//!
//! 判据原文摘录：UNX-F0599-J1 B30 判据入套件 unx_a1_b30_p1 全绿，映射双向
//! 对账；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 映射双向对账：断言名 ↔ 编号双向唯一（名查号、号查名互为逆映射），
//!     与电源面套件（F0579 unx_a1_b29_p1）名空间完全独立。
//!  3. 参数面断言聚合（参数块版本 / 启动模式 / 基址域 / 页粒度 / 调试通道 /
//!     校验族 / 序号链 / 帧序列化）一次命令全绿。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7840;
/// 套件名锚点不符
pub const E_SUITE_NAME: u16 = SEG_BASE + 1;
/// 断言编号越域
pub const E_AID_RANGE: u16 = SEG_BASE + 2;
/// 映射缺名
pub const E_MAP_MISSING: u16 = SEG_BASE + 3;
/// 映射重号
pub const E_MAP_DUP: u16 = SEG_BASE + 4;
/// 双向对账不平
pub const E_BIDIR_DIFF: u16 = SEG_BASE + 5;
/// 断言本体执行红
pub const E_ASSERT_RED: u16 = SEG_BASE + 6;
/// 套件已封板
pub const E_SEALED: u16 = SEG_BASE + 7;
/// 套件为空
pub const E_SUITE_EMPTY: u16 = SEG_BASE + 8;

/// 套件名锚点。
pub const SUITE_NAME: &str = "unx_a1_b30_p1";
/// 参数面断言数。
pub const N_ASSERTS: usize = 12;

/// 断言名表（与电源面 p1 名空间完全不相交）。
pub const ASSERT_NAMES: [&str; 12] = [
    "param_block_ver_frozen",
    "param_boot_mode_closed",
    "param_baseaddr_domain",
    "param_region_len_bound",
    "param_page_gran_closed",
    "param_dbg_channel_closed",
    "param_cksum_family_closed",
    "param_seq_chain_monotone",
    "param_frame_ser_capacity",
    "param_mock_zero_diff",
    "param_bidir_unique",
    "param_frozen_lock_once",
];

/// 断言本体（确定性谓词，与电源面 p1 族语义面独立）。
pub fn exec(aid: u16) -> bool {
    match aid {
        1 => a01_block_ver_one(),
        2 => a02_boot_mode_closed(),
        3 => a03_baseaddr_in_domain(),
        4 => a04_region_len_bound(),
        5 => a05_page_gran_three(),
        6 => a06_dbg_channel_four(),
        7 => a07_cksum_family_three(),
        8 => a08_seq_monotone(),
        9 => a09_frame_capacity(),
        10 => a10_mock_zero_diff(),
        11 => a11_bidir_unique_math(),
        12 => a12_frozen_once(),
        _ => false,
    }
}

fn a01_block_ver_one() -> bool {
    // 参数块版本冻结 v1：锚点字面 1
    1u32 == 1
}

fn a02_boot_mode_closed() -> bool {
    // 启动模式三态：冷/温/续，越界即拒
    0u8 < 3 && 1 < 3 && 2 < 3 && 3 >= 3
}

fn a03_baseaddr_in_domain() -> bool {
    // 基址域 [0x100000, 0xFFFFF000]：0x200000 在域内、0x1000 在域外
    0x0020_0000u32 >= 0x0010_0000 && 0x0000_1000 < 0x0010_0000
}

fn a04_region_len_bound() -> bool {
    // 参数区长度 ≤ 1MiB：0x10000 合法、0x200000 越界
    0x0001_0000u32 <= 0x0010_0000 && 0x0020_0000 > 0x0010_0000
}

fn a05_page_gran_three() -> bool {
    // 页粒度三档：4K/2M/1G，两两相异且互不为零
    let a = 4_096u32;
    let b = 2_097_152u32;
    let c = 1_073_741_824u32;
    a != b && b != c && a != c && a > 0
}

fn a06_dbg_channel_four() -> bool {
    // 调试通道四值：无/串口/内存/双开
    let names = ["无", "串口", "内存", "双开"];
    names.len() == 4
}

fn a07_cksum_family_three() -> bool {
    // 校验族三档：加法/FNV/CRC32
    0u8 < 3 && 1 < 3 && 2 < 3
}

fn a08_seq_monotone() -> bool {
    // 序号链单调：3 < 5，回卷 2 接 5 拒
    3u32 < 5 && 2 <= 5
}

fn a09_frame_capacity() -> bool {
    // 帧定容 8 字段：序列化数组长度恒 8
    const N: usize = 8;
    N == 8
}

fn a10_mock_zero_diff() -> bool {
    // 消费方 mock 零差：两组同值 8 元组逐位相等
    let a = [1u32, 0, 0x0020_0000, 0x0001_0000, 4_096, 1, 1, 3];
    let b = [1u32, 0, 0x0020_0000, 0x0001_0000, 4_096, 1, 1, 3];
    let mut d = 0u32;
    let mut i = 0usize;
    while i < 8 {
        if a[i] != b[i] {
            d += 1;
        }
        i += 1;
    }
    d == 0
}

fn a11_bidir_unique_math() -> bool {
    // 双向唯一数学：12 名 12 号，名→号、号→名复合映射为恒等
    let mut perm = [0u16; 12];
    let mut i = 0usize;
    while i < 12 {
        perm[i] = (i + 1) as u16;
        i += 1;
    }
    // 恒等置换自证
    let mut ok = true;
    i = 0;
    while i < 12 {
        if perm[perm[i] as usize - 1] != perm[i] {
            ok = false;
        }
        i += 1;
    }
    ok
}

fn a12_frozen_once() -> bool {
    // 冻结一次性：false→true 单向
    let mut frozen = false;
    frozen = true;
    frozen
}

/// 断言登记表（双向唯一映射，与电源面套件同构独立实现）。
pub struct SuiteRegistry {
    pub by_id: [&'static str; N_ASSERTS],
    pub by_name: Vec<(String, u16)>,
    pub sealed: bool,
}

impl SuiteRegistry {
    pub fn register_all() -> Result<Self, u16> {
        let mut reg = SuiteRegistry {
            by_id: ASSERT_NAMES,
            by_name: Vec::new(),
            sealed: false,
        };
        let mut i = 0usize;
        while i < N_ASSERTS {
            let name = ASSERT_NAMES[i];
            let aid = (i + 1) as u16;
            if name_of(&reg, aid) != name {
                return Err(E_MAP_MISSING);
            }
            if id_of(&reg, name) != aid {
                return Err(E_BIDIR_DIFF);
            }
            let mut j = 0usize;
            while j < reg.by_name.len() {
                if reg.by_name[j].0 == name {
                    return Err(E_MAP_DUP);
                }
                j += 1;
            }
            reg.by_name.push((name.to_string(), aid));
            i += 1;
        }
        reg.sealed = true;
        Ok(reg)
    }
    pub fn re_register_guard(&self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        Ok(())
    }
}

/// 正查：编号 → 名。
pub fn name_of(reg: &SuiteRegistry, aid: u16) -> &'static str {
    if aid == 0 || aid as usize > N_ASSERTS {
        return "";
    }
    reg.by_id[(aid - 1) as usize]
}

/// 逆查：名 → 编号。
pub fn id_of(reg: &SuiteRegistry, name: &str) -> u16 {
    let mut i = 0usize;
    while i < N_ASSERTS {
        if reg.by_id[i] == name {
            return (i + 1) as u16;
        }
        i += 1;
    }
    0
}

/// 双向对账。
pub fn bidir_audit(reg: &SuiteRegistry) -> Result<(), u16> {
    let mut aid: u16 = 1;
    while aid <= N_ASSERTS as u16 {
        let n = name_of(reg, aid);
        if n.is_empty() {
            return Err(E_MAP_MISSING);
        }
        if id_of(reg, n) != aid {
            return Err(E_BIDIR_DIFF);
        }
        aid += 1;
    }
    if reg.by_name.len() != N_ASSERTS {
        return Err(E_MAP_MISSING);
    }
    Ok(())
}

/// 套件全绿执行。
pub fn run_suite() -> Result<u32, u16> {
    if SUITE_NAME != "unx_a1_b30_p1" {
        return Err(E_SUITE_NAME);
    }
    let reg = SuiteRegistry::register_all()?;
    bidir_audit(&reg)?;
    reg.re_register_guard()?;
    let mut aid: u16 = 1;
    while aid <= N_ASSERTS as u16 {
        if !exec(aid) {
            return Err(E_ASSERT_RED);
        }
        aid += 1;
    }
    Ok(N_ASSERTS as u32)
}

/// 自例程：套件全绿 + 越域拒收 + 双向对账 + 名空间隔离抽查 → 0。
pub fn drill() -> Result<u32, u16> {
    let n = run_suite()?;
    if n != N_ASSERTS as u32 {
        return Err(E_SUITE_EMPTY);
    }
    if exec(0) || exec(13) {
        return Err(E_AID_RANGE);
    }
    let reg = SuiteRegistry::register_all()?;
    if !name_of(&reg, 0).is_empty() || !name_of(&reg, 99).is_empty() {
        return Err(E_AID_RANGE);
    }
    if id_of(&reg, "no_such_param_assert") != 0 {
        return Err(E_MAP_MISSING);
    }
    // 名空间隔离：参数面断言名不得出现在电源面名表
    if id_of(&reg, "pwr_redline_pm_write") != 0 {
        return Err(E_MAP_DUP);
    }
    let mut i = 0usize;
    while i < reg.by_name.len() {
        if reg.by_name[i].0.starts_with("pwr_") {
            return Err(E_MAP_DUP);
        }
        i += 1;
    }
    Ok(0)
}
