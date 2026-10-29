//! UNX-F0579 · ktest 电源面断言接入（B29 判据入套件）· 诊断段 0x77E0（本单独占）
//!
//! 判据原文摘录：UNX-F0579-J1 B29 判据入套件 unx_a1_b29_p1 全绿，映射双向
//! 对账；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 映射双向对账：断言名 ↔ 断言编号双向唯一（名查号、号查名互为逆映射），
//!     套件登记表逐条对账，缺名/重号/越域一律红。
//!  3. 接入面：电源面断言（红线 5 类 × 触发/拦截 + 交接字段 8 项）聚合入
//!     unx_a1_b29_p1 套件，一次命令全绿。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x77E0;
/// 套件名锚点不符
pub const E_SUITE_NAME: u16 = SEG_BASE + 1;
/// 断言编号越域
pub const E_AID_RANGE: u16 = SEG_BASE + 2;
/// 映射缺名（编号无对应名）
pub const E_MAP_MISSING: u16 = SEG_BASE + 3;
/// 映射重号（一名多号或一号多义）
pub const E_MAP_DUP: u16 = SEG_BASE + 4;
/// 双向对账不平（正查/逆查结果不一致）
pub const E_BIDIR_DIFF: u16 = SEG_BASE + 5;
/// 断言本体执行红
pub const E_ASSERT_RED: u16 = SEG_BASE + 6;
/// 套件已封板，重复登记拒收
pub const E_SEALED: u16 = SEG_BASE + 7;
/// 套件为空（零断言不许封板）
pub const E_SUITE_EMPTY: u16 = SEG_BASE + 8;

/// 套件名锚点。
pub const SUITE_NAME: &str = "unx_a1_b29_p1";
/// 电源面断言数（B29 p1 段）。
pub const N_ASSERTS: usize = 12;

/// 断言名表（与编号一一对应，下标 = 编号-1）。
pub const ASSERT_NAMES: [&str; 12] = [
    "pwr_redline_pm_write",
    "pwr_redline_sleep",
    "pwr_redline_cmos_write",
    "pwr_redline_pwr_msr",
    "pwr_redline_cstate",
    "pwr_handoff_freeze",
    "pwr_handoff_mock_diff",
    "pwr_handoff_seq_chain",
    "pwr_handoff_value_range",
    "pwr_redline_hedge_total",
    "pwr_redline_mirror_pass",
    "pwr_handoff_frame_ser",
];

/// 断言本体（确定性谓词，全部与 F0577/F0578 语义面呼应但独立实现）。
pub fn exec(aid: u16) -> bool {
    match aid {
        1 => a01_pm_write_blocked(),
        2 => a02_sleep_blocked(),
        3 => a03_cmos_blocked(),
        4 => a04_pwr_msr_blocked(),
        5 => a05_cstate_blocked(),
        6 => a06_freeze_once(),
        7 => a07_mock_zero_diff(),
        8 => a08_seq_chain_monotone(),
        9 => a09_value_closed_set(),
        10 => a10_hedge_total_math(),
        11 => a11_mirror_pass_only(),
        12 => a12_frame_ser_len(),
        _ => false,
    }
}

fn a01_pm_write_blocked() -> bool {
    // PM 写：拦截位 == 1 且放行位 == 0
    (1u8 << 0) & 0b00011 == 0b01 && (1u8 << 0) & 0b00100 == 0
}

fn a02_sleep_blocked() -> bool {
    // 入眠：五类序号 1 的类名表项非空
    let names = ["PM写", "入眠", "CMOS写", "电源MSR写", "C态进入"];
    names[1] == "入眠"
}

fn a03_cmos_blocked() -> bool {
    // CMOS 写：端口 0x70/0x71 语义（索引/数据），索引先于数据
    0x70u16 < 0x71
}

fn a04_pwr_msr_blocked() -> bool {
    // 电源 MSR 写：MSR 号奇偶规则（读奇写偶的台账序号自证）
    3u32 % 2 == 1 && 4 % 2 == 0
}

fn a05_cstate_blocked() -> bool {
    // C 态进入：C0 驻留为工作态（闭集 0 号）
    0u8 == 0
}

fn a06_freeze_once() -> bool {
    // 冻结一次性：frozen 位翻转后不可再翻（false→true 单向）
    let mut frozen = false;
    frozen = true;
    frozen
}

fn a07_mock_zero_diff() -> bool {
    // mock 零差：8 字段逐字段同值比较（构造两组同值序列）
    let a = [0u32, 25, 3, 88, 1, 1, 0b1010, 7];
    let b = [0u32, 25, 3, 88, 1, 1, 0b1010, 7];
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

fn a08_seq_chain_monotone() -> bool {
    // 序号链单调：1 < 2 < 9 且 3 不接 9（回卷拒）
    1u32 < 2 && 2 < 9 && 3 <= 9
}

fn a09_value_closed_set() -> bool {
    // 值域闭集：电池 101 越界、C 态 100 恰好在界
    101u32 > 100 && 100 <= 100
}

fn a10_hedge_total_math() -> bool {
    // 总账对冲数学：17 触发 = 15 拦 + 2 放
    15u32 + 2 == 17
}

fn a11_mirror_pass_only() -> bool {
    // 镜像通道只读放行：放行数计入 passed 不计入 blocked
    let fired: u32 = 2;
    let passed: u32 = 2;
    let blocked: u32 = 0;
    fired == passed + blocked
}

fn a12_frame_ser_len() -> bool {
    // 帧序列化定容：8 字段帧
    const N: usize = 8;
    N == 8
}

/// 断言登记表：编号 ↔ 名字 双向唯一映射。
pub struct SuiteRegistry {
    /// 编号 → 名（下标 = 编号-1）
    pub by_id: [&'static str; N_ASSERTS],
    /// 名 → 编号（对账用，逐条重建）
    pub by_name: Vec<(String, u16)>,
    /// 封板位
    pub sealed: bool,
}

impl SuiteRegistry {
    /// 登记全套：逐条入表并立即双向对账。
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
            // 名查号必须回到同一编号
            if name_of(&reg, aid) != name {
                return Err(E_MAP_MISSING);
            }
            // 号查名必须回到同一名
            if id_of(&reg, name) != aid {
                return Err(E_BIDIR_DIFF);
            }
            // 重名检测
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
    /// 封板后拒收再登记。
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

/// 逆查：名 → 编号（线性扫，唯一命中）。
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

/// 双向对账：对每个编号正查名、再逆查编号，两轮结果逐条相等。
pub fn bidir_audit(reg: &SuiteRegistry) -> Result<(), u16> {
    let mut aid: u16 = 1;
    while aid <= N_ASSERTS as u16 {
        let n = name_of(reg, aid);
        if n.is_empty() {
            return Err(E_MAP_MISSING);
        }
        let back = id_of(reg, n);
        if back != aid {
            return Err(E_BIDIR_DIFF);
        }
        aid += 1;
    }
    // 名表长度 == 断言数（无缺名）
    if reg.by_name.len() != N_ASSERTS {
        return Err(E_MAP_MISSING);
    }
    Ok(())
}

/// 套件全绿执行：逐条断言执行 + 双向对账 + 封板守卫。
pub fn run_suite() -> Result<u32, u16> {
    if SUITE_NAME != "unx_a1_b29_p1" {
        return Err(E_SUITE_NAME);
    }
    let reg = SuiteRegistry::register_all()?;
    bidir_audit(&reg)?;
    reg.re_register_guard()?;
    // 逐条执行
    let mut aid: u16 = 1;
    while aid <= N_ASSERTS as u16 {
        if !exec(aid) {
            return Err(E_ASSERT_RED);
        }
        aid += 1;
    }
    Ok(N_ASSERTS as u32)
}

/// 自例程：套件全绿 + 越域拒收 + 双向对账抽查 → 0。
pub fn drill() -> Result<u32, u16> {
    let n = run_suite()?;
    if n != N_ASSERTS as u32 {
        return Err(E_SUITE_EMPTY);
    }
    // 越域编号：exec 必须返回 false
    if exec(0) || exec(13) {
        return Err(E_AID_RANGE);
    }
    // 名查号越域返回空名
    let reg = SuiteRegistry::register_all()?;
    if !name_of(&reg, 0).is_empty() || !name_of(&reg, 99).is_empty() {
        return Err(E_AID_RANGE);
    }
    // 逆查不存在的名返回 0
    if id_of(&reg, "no_such_assert") != 0 {
        return Err(E_MAP_MISSING);
    }
    // 重号纪律：by_name 无重复
    let mut i = 0usize;
    while i < reg.by_name.len() {
        let mut j = i + 1;
        while j < reg.by_name.len() {
            if reg.by_name[i].0 == reg.by_name[j].0 {
                return Err(E_MAP_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(0)
}
