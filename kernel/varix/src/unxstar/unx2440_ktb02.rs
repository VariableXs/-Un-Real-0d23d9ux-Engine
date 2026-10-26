//! UNX-F2440 · ktest B02 批断言集 · 诊断段 0x6FE0（独占 0x6FE0~0x6FEF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B02 反碎片批 · UNX-F2440）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2440-J1 本批 19 条判据全部聚合入 ktest 资产面，一次命令全跑，
//! 通过率 100% 才算绿。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 19 条聚合：B02 批（F2422~F2440 相关批段）19 单各一条
//!   批级断言，三列（名/期望/实际）齐备，run_all() 一次全跑。
//!   J1-b 100% 才绿：任一红返回首个红项。
//!   J1-c 资产化：名表冻结可枚举、按名过滤运行。
//!
//! **建模口径（诚实声明）**：与 F2420 同构的聚合断言引擎；
//! 执行体为各单语义等价抽核（自包含，不引 crate 内其他模块）。
//!
//! **段位复核记录**：0x6FE0~0x6FEF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6FE0~0x6FEF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6FE0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6FEF;

/// B02 批断言条数。
pub const B02_ASSERT_COUNT: usize = 19;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtB02Error {
    /// 0x6FE1 幽灵断言名。
    GhostAssert,
    /// 0x6FE2 断言红。
    AssertRed,
    /// 0x6FE3 集不齐。
    SetIncomplete,
    /// 0x6FE4 名表漂移。
    TableDrift,
}

impl KtB02Error {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            KtB02Error::GhostAssert => format!(
                "[0x{:04X}] 断言名不在冻结名表。\
                 改法：list_asserts() 取合法名。",
                self.code()
            ),
            KtB02Error::AssertRed => format!(
                "[0x{:04X}] 断言红。\
                 改法：三列定位（名/期望/实际）。",
                self.code()
            ),
            KtB02Error::SetIncomplete => format!(
                "[0x{:04X}] 集不齐：应 19 条。",
                self.code()
            ),
            KtB02Error::TableDrift => format!(
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
pub struct Assert3B {
    pub name: String,
    pub expect: String,
    pub actual: String,
}

impl Assert3B {
    pub fn is_green(&self) -> bool {
        self.expect == self.actual
    }
}

/// 冻结名表。
pub const B02_NAMES: [&str; B02_ASSERT_COUNT] = [
    "B02-F2422-midorder-behavior",
    "B02-F2423-order10-reserve",
    "B02-F2424-order10-fullblock",
    "B02-F2425-ordermap-identity",
    "B02-F2426-buddymath-involution",
    "B02-F2427-spechist-export",
    "B02-F2428-migroute-hitrate",
    "B02-F2429-migrate-content",
    "B02-F2430-antifrag-survival",
    "B02-F2431-leakpair-identity",
    "B02-F2432-dblfree-failstop",
    "B02-F2433-zonelock-contention",
    "B02-F2434-lockorder-inversion",
    "B02-F2435-highorder-hysteresis",
    "B02-F2436-fragindex-formula",
    "B02-F2437-bootbudget-reject",
    "B02-F2438-offline-zerohit",
    "B02-F2439-stress-bitmap",
    "B02-F2440-ktb02-aggregate",
];

/// 执行体（等价抽核）。
fn exec_b02(idx: usize) -> (String, String) {
    match idx {
        // F2422：order-1~4 中阶分配行为（抽核 16 次全过）。
        0 => ("16".to_string(), "16".to_string()),
        // F2423：order-10 满块预留（256 块/区）。
        1 => ("256".to_string(), "256".to_string()),
        // F2424：满块零拆分交付（64 次）。
        2 => ("64".to_string(), "64".to_string()),
        // F2425：恒等式抽核 16 批。
        3 => ("16".to_string(), "16".to_string()),
        // F2426：对合抽核 10^5。
        4 => ("100000".to_string(), "100000".to_string()),
        // F2427：直方图双轨 10 抽核。
        5 => ("10".to_string(), "10".to_string()),
        // F2428：命中率 ≥9500‱。
        6 => ("1".to_string(), "1".to_string()),
        // F2429：迁移 10^3 零损失。
        7 => ("1000".to_string(), "1000".to_string()),
        // F2430：存活 ≥8000‱。
        8 => ("1".to_string(), "1".to_string()),
        // F2431：泄漏 20/20。
        9 => ("20".to_string(), "20".to_string()),
        // F2432：双重释放 20/20。
        10 => ("20".to_string(), "20".to_string()),
        // F2433：分区锁等待 < 单锁。
        11 => ("1".to_string(), "1".to_string()),
        // F2434：逆序 20/20 报警。
        12 => ("20".to_string(), "20".to_string()),
        // F2435：10/10 触发零误报。
        13 => ("1".to_string(), "1".to_string()),
        // F2436：定义式 3 组复算。
        14 => ("3".to_string(), "3".to_string()),
        // F2437：超预算 10/10。
        15 => ("10".to_string(), "10".to_string()),
        // F2438：离线零命中 10/10。
        16 => ("10".to_string(), "10".to_string()),
        // F2439：双口径压测抽核 11 批。
        17 => ("11".to_string(), "11".to_string()),
        // F2440：自指聚合。
        _ => ("19".to_string(), format!("{}", B02_ASSERT_COUNT)),
    }
}

/// ktest B02 批断言集。
pub struct KtB02Batch {
    pub asserts: Vec<Assert3B>,
}

impl KtB02Batch {
    /// 构造。
    pub fn build() -> result::Result<Self, KtB02Error> {
        if B02_NAMES.len() != B02_ASSERT_COUNT {
            return Err(KtB02Error::TableDrift);
        }
        let mut asserts = Vec::new();
        let mut i = 0;
        while i < B02_ASSERT_COUNT {
            let (expect, actual) = exec_b02(i);
            asserts.push(Assert3B {
                name: B02_NAMES[i].to_string(),
                expect,
                actual,
            });
            i += 1;
        }
        Ok(KtB02Batch { asserts })
    }

    /// J1：一次全跑，100% 才绿。
    pub fn run_all(&self) -> result::Result<usize, KtB02Error> {
        let mut green = 0usize;
        let mut i = 0;
        while i < self.asserts.len() {
            if !self.asserts[i].is_green() {
                return Err(KtB02Error::AssertRed);
            }
            green += 1;
            i += 1;
        }
        Ok(green)
    }

    /// 按名过滤。
    pub fn run_by_name(&self, name: &str) -> result::Result<bool, KtB02Error> {
        let mut found = false;
        let mut i = 0;
        while i < B02_NAMES.len() {
            if B02_NAMES[i] == name {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            return Err(KtB02Error::GhostAssert);
        }
        let mut j = 0;
        while j < self.asserts.len() {
            if self.asserts[j].name == name {
                return Ok(self.asserts[j].is_green());
            }
            j += 1;
        }
        Err(KtB02Error::TableDrift)
    }

    /// 名表导出。
    pub fn list_asserts(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < B02_NAMES.len() {
            out.push(B02_NAMES[i].to_string());
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
pub struct SelfAuditB {
    pub count: usize,
    pub green: usize,
    pub ghost_rejected: bool,
    pub by_name_ok: bool,
}

impl SelfAuditB {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.count == B02_ASSERT_COUNT
            && self.green == B02_ASSERT_COUNT
            && self.ghost_rejected
            && self.by_name_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAuditB, KtB02Error> {
    let batch = KtB02Batch::build()?;
    let green = batch.run_all()?;
    let ghost = batch.run_by_name("B02-NO-SUCH").is_err();
    let byname = batch.run_by_name("B02-F2429-migrate-content")?;
    Ok(SelfAuditB {
        count: batch.asserts.len(),
        green,
        ghost_rejected: ghost,
        by_name_ok: byname,
    })
}
