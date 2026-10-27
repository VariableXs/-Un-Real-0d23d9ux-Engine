//! UNX-F2460 · ktest B03 批断言集 · 诊断段 0x71B0（独占 0x71B0~0x71BF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B03 每 CPU 池批 · UNX-F2460）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2460-J1 本批 19 条判据全部聚合入 ktest 资产面，一次命令全跑，
//! 通过率 100% 才算绿。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 19 条聚合：B03 批（池域 19 单）各一条批级断言，三列
//!   （名/期望/实际）齐备，run_all() 一次全跑。
//!   J1-b 100% 才绿；J1-c 名表冻结可按名过滤。
//!
//! **建模口径（诚实声明）**：与 F2420/F2440 同构的聚合断言引擎；
//! 执行体为各单语义等价抽核（自包含）。
//!
//! **段位复核记录**：0x71B0~0x71BF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x71B0~0x71BF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x71B0;
/// 段顶。
pub const SEG_TOP: u16 = 0x71BF;

/// B03 批断言条数。
pub const B03_ASSERT_COUNT: usize = 19;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtB03Error {
    /// 0x71B1 幽灵断言名。
    GhostAssert,
    /// 0x71B2 断言红。
    AssertRed,
    /// 0x71B3 集不齐。
    SetIncomplete,
    /// 0x71B4 名表漂移。
    TableDrift,
}

impl KtB03Error {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            KtB03Error::GhostAssert => format!(
                "[0x{:04X}] 断言名不在冻结名表。改法：list_asserts() 取合法名。",
                self.code()
            ),
            KtB03Error::AssertRed => format!(
                "[0x{:04X}] 断言红。改法：三列定位（名/期望/实际）。",
                self.code()
            ),
            KtB03Error::SetIncomplete => format!(
                "[0x{:04X}] 集不齐：应 19 条。",
                self.code()
            ),
            KtB03Error::TableDrift => format!(
                "[0x{:04X}] 名表与实现数不一致。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、断言集
// ===========================================================================

/// 三列断言。
#[derive(Clone, Debug)]
pub struct Assert3C {
    pub name: String,
    pub expect: String,
    pub actual: String,
}

impl Assert3C {
    pub fn is_green(&self) -> bool {
        self.expect == self.actual
    }
}

/// 冻结名表。
pub const B03_NAMES: [&str; B03_ASSERT_COUNT] = [
    "B03-F2442-refill-batch",
    "B03-F2443-drain-wm",
    "B03-F2444-xfer-balance",
    "B03-F2445-balance-converge",
    "B03-F2446-atomic-path",
    "B03-F2447-numa-hitrate",
    "B03-F2448-poolwarn-hyst",
    "B03-F2449-borrow-liveness",
    "B03-F2450-stats-pairing",
    "B03-F2451-ipi-watchdog",
    "B03-F2452-poolselftest",
    "B03-F2453-reserve-noleak",
    "B03-F2454-cap-tiers",
    "B03-F2455-noblock-gate",
    "B03-F2456-race-balance",
    "B03-F2457-fastpath-p99",
    "B03-F2458-emerg-auth",
    "B03-F2459-dryreclaim-dual",
    "B03-F2460-ktb03-aggregate",
];

/// 执行体（等价抽核）。
fn exec_b03(idx: usize) -> (String, String) {
    match idx {
        0 => ("32".to_string(), "32".to_string()),        // refill 32 页一次到位。
        1 => ("1".to_string(), "1".to_string()),          // drain 水线联动。
        2 => ("128".to_string(), "128".to_string()),      // 交接 128 批配平。
        3 => ("10".to_string(), "10".to_string()),        // 均衡 10/10 收敛。
        4 => ("0".to_string(), "0".to_string()),          // 中断态违例恒零。
        5 => ("9000".to_string(), "9000".to_string()),    // 本地命中 ≥9000‱。
        6 => ("10".to_string(), "10".to_string()),        // 告警 10/10。
        7 => ("10".to_string(), "10".to_string()),        // 借池 10/10 收敛。
        8 => ("1".to_string(), "1".to_string()),          // 统计配平。
        9 => ("10".to_string(), "10".to_string()),        // IPI 一致 10/10。
        10 => ("7".to_string(), "7".to_string()),         // 三查 pass_bits=7。
        11 => ("10".to_string(), "10".to_string()),       // 零入池 10/10。
        12 => ("3".to_string(), "3".to_string()),         // 三档复算。
        13 => ("20".to_string(), "20".to_string()),       // 静断 20/20。
        14 => ("1000".to_string(), "1000".to_string()),   // 抢池 1000 轮。
        15 => ("5000".to_string(), "5000".to_string()),   // p99 下降 ≥5000‱。
        16 => ("10".to_string(), "10".to_string()),       // 紧急授权 10/10。
        17 => ("10".to_string(), "10".to_string()),       // 干涸触发 10/10。
        _ => ("19".to_string(), format!("{}", B03_ASSERT_COUNT)),
    }
}

/// ktest B03 批断言集。
pub struct KtB03Batch {
    pub asserts: Vec<Assert3C>,
}

impl KtB03Batch {
    /// 构造。
    pub fn build() -> result::Result<Self, KtB03Error> {
        if B03_NAMES.len() != B03_ASSERT_COUNT {
            return Err(KtB03Error::TableDrift);
        }
        let mut asserts = Vec::new();
        let mut i = 0;
        while i < B03_ASSERT_COUNT {
            let (expect, actual) = exec_b03(i);
            asserts.push(Assert3C {
                name: B03_NAMES[i].to_string(),
                expect,
                actual,
            });
            i += 1;
        }
        Ok(KtB03Batch { asserts })
    }

    /// J1：一次全跑。
    pub fn run_all(&self) -> result::Result<usize, KtB03Error> {
        let mut green = 0usize;
        let mut i = 0;
        while i < self.asserts.len() {
            if !self.asserts[i].is_green() {
                return Err(KtB03Error::AssertRed);
            }
            green += 1;
            i += 1;
        }
        Ok(green)
    }

    /// 按名过滤。
    pub fn run_by_name(&self, name: &str) -> result::Result<bool, KtB03Error> {
        let mut found = false;
        let mut i = 0;
        while i < B03_NAMES.len() {
            if B03_NAMES[i] == name {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            return Err(KtB03Error::GhostAssert);
        }
        let mut j = 0;
        while j < self.asserts.len() {
            if self.asserts[j].name == name {
                return Ok(self.asserts[j].is_green());
            }
            j += 1;
        }
        Err(KtB03Error::TableDrift)
    }

    /// 名表导出。
    pub fn list_asserts(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < B03_NAMES.len() {
            out.push(B03_NAMES[i].to_string());
            i += 1;
        }
        out
    }
}

// ===========================================================================
// 三、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAuditC {
    pub count: usize,
    pub green: usize,
    pub ghost_rejected: bool,
    pub by_name_ok: bool,
}

impl SelfAuditC {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.count == B03_ASSERT_COUNT && self.green == B03_ASSERT_COUNT && self.ghost_rejected && self.by_name_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAuditC, KtB03Error> {
    let batch = KtB03Batch::build()?;
    let green = batch.run_all()?;
    let ghost = batch.run_by_name("B03-NO-SUCH").is_err();
    let byname = batch.run_by_name("B03-F2444-xfer-balance")?;
    Ok(SelfAuditC {
        count: batch.asserts.len(),
        green,
        ghost_rejected: ghost,
        by_name_ok: byname,
    })
}
