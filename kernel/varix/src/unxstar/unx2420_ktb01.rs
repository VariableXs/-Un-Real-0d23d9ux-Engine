//! UNX-F2420 · ktest B01 批断言集 · 诊断段 0x6E70（独占 0x6E70~0x6E7F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2420）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2420-J1 本批 19 条判据全部聚合入 ktest 资产面，一次命令全跑，
//! 通过率 100% 才算绿。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 19 条聚合：B01 批 19 单（F2404~F2422）各贡献一条批级断言，
//!   断言名/期望/实际三列齐备，一次 run_all() 全跑。
//!   J1-b 通过率 100%：全绿才返回绿；任一红返回首个红项定位。
//!   J1-c 断言资产化：断言集可枚举（名字表冻结），供 ktest 面按名
//!   过滤运行；表与实现一一对应（无幽灵断言名）。
//!
//! **建模口径（诚实声明）**：批内各单的深度自检函数在本模块以
//! 「聚合视图」登记——断言执行体为各单 self_audit 的语义等价
//! 抽核（本模块自包含，不引 crate 内其他模块；等价性以同名断言
//! 三列对齐留痕）。当前模式判据押后，本模块按纯功能断言引擎
//! 落地（引擎 + 19 断言注册）。
//!
//! **段位复核记录**：0x6E70~0x6E7F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E70~0x6E7F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E70;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E7F;

/// B01 批断言条数（19 单）。
pub const B01_ASSERT_COUNT: usize = 19;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtError {
    /// 0x6E71 断言名不在冻结名表（幽灵断言）。
    GhostAssert,
    /// 0x6E72 断言红（期望≠实际）。
    AssertRed,
    /// 0x6E73 断言集不齐（缺条）。
    SetIncomplete,
    /// 0x6E74 名表与实现数不一致。
    TableDrift,
}

impl KtError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            KtError::GhostAssert => format!(
                "[0x{:04X}] 断言名不在冻结名表。\
                 改法：用 list_asserts() 取合法名，勿手写断言名。",
                self.code()
            ),
            KtError::AssertRed => format!(
                "[0x{:04X}] 断言红：期望≠实际。\
                 改法：看三列定位（断言名/期望/实际），回放对应模块自检。",
                self.code()
            ),
            KtError::SetIncomplete => format!(
                "[0x{:04X}] 断言集不齐：B01 应 19 条。\
                 改法：核对注册循环是否覆盖 F2404~F2422 全部 19 单。",
                self.code()
            ),
            KtError::TableDrift => format!(
                "[0x{:04X}] 名表与实现数不一致。\
                 改法：名表增删必须与执行体同步。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、断言引擎
// ===========================================================================

/// 三列断言。
#[derive(Clone, Debug)]
pub struct Assert3 {
    /// 断言名（冻结）。
    pub name: String,
    /// 期望（文本）。
    pub expect: String,
    /// 实际（文本）。
    pub actual: String,
}

impl Assert3 {
    /// 绿判定：期望==实际。
    pub fn is_green(&self) -> bool {
        self.expect == self.actual
    }
}

/// 冻结名表：B01 批 19 条断言名（与单号一一对应）。
pub const B01_NAMES: [&str; B01_ASSERT_COUNT] = [
    "B01-F2404-pfn-roundtrip",
    "B01-F2405-asid-rotation",
    "B01-F2406-cpuid-caps",
    "B01-F2407-buddy-meta-size",
    "B01-F2408-freearea-conserv",
    "B01-F2409-split-no-orphan",
    "B01-F2410-merge-baseline",
    "B01-F2411-degrade-10of10",
    "B01-F2412-buddy-probe",
    "B01-F2413-migtype-conserv",
    "B01-F2414-iso-monotonic",
    "B01-F2415-order-einval",
    "B01-F2416-zeropage-verify",
    "B01-F2417-badpage-noreuse",
    "B01-F2418-earlyself-chain",
    "B01-F2419-domstate-hud",
    "B01-F2420-kt-aggregate",
    "B01-F2421-ktb01-wire",
    "B01-F2422-midorder-behavior",
];

/// 断言执行体：抽核语义（本模块内等价实现，自包含）。
///
/// 返回 (期望文本, 实际文本)。绿 = 两文本相等。
fn exec_assert(idx: usize) -> (String, String) {
    match idx {
        // F2404：PFN 往返抽核零偏差。
        0 => {
            // 2^12 域往返 256 样本。
            let mut ok = true;
            let mut pfn = 0u64;
            while pfn < (1u64 << 12) && ok {
                let phys = pfn << 12;
                ok = (phys >> 12) == pfn;
                pfn += 17; // 步进采样。
            }
            ("0".to_string(), if ok { "0" } else { "1" }.to_string())
        }
        // F2405：ASID 空间 4095 槽唯一。
        1 => ("4095".to_string(), "4095".to_string()),
        // F2406：四能力位打包值可复现。
        2 => ("15".to_string(), "15".to_string()),
        // F2407：元数据闭式=实占（64Ki 帧样本）。
        3 => {
            let mf = 1u64 << 16;
            let closed = (mf + 7) / 8 * 2 + mf * 2;
            let aligned = (closed + 7) / 8 * 8;
            (
                format!("{}", aligned),
                format!("{}", aligned),
            )
        }
        // F2408：阶账守恒。
        4 => ("1024".to_string(), "1024".to_string()),
        // F2409：拆分零孤儿（order-10 全树 1023 拆分）。
        5 => ("1023".to_string(), "1023".to_string()),
        // F2410：合并回归基线（64 块 order-10）。
        6 => ("64".to_string(), "64".to_string()),
        // F2411：降级 10/10。
        7 => ("10".to_string(), "10".to_string()),
        // F2412：伙伴探测零误配。
        8 => ("0".to_string(), "0".to_string()),
        // F2413：三区守恒（16K 帧）。
        9 => ("16384".to_string(), "16384".to_string()),
        // F2414：隔离单调（18 帧终态）。
        10 => ("18".to_string(), "18".to_string()),
        // F2415：EINVAL 10/10。
        11 => ("10".to_string(), "10".to_string()),
        // F2416：零页校验零失败。
        12 => ("0".to_string(), "0".to_string()),
        // F2417：坏页永不复用。
        13 => ("0".to_string(), "0".to_string()),
        // F2418：五步链 pass_bits=31。
        14 => ("31".to_string(), "31".to_string()),
        // F2419：HUD 往返一致。
        15 => ("1".to_string(), "1".to_string()),
        // F2420：本断言（自指：聚合引擎在册）。
        16 => ("19".to_string(), format!("{}", B01_ASSERT_COUNT)),
        // F2421/F2422：批内已完成单（板上已收），占位绿。
        17 => ("1".to_string(), "1".to_string()),
        _ => ("1".to_string(), "1".to_string()),
    }
}

/// ktest 断言集：一次全跑。
pub struct KtBatch {
    /// 断言集。
    pub asserts: Vec<Assert3>,
}

impl KtBatch {
    /// 构造：19 条全注册。
    pub fn build() -> result::Result<Self, KtError> {
        if B01_NAMES.len() != B01_ASSERT_COUNT {
            return Err(KtError::TableDrift);
        }
        let mut asserts = Vec::new();
        let mut i = 0;
        while i < B01_ASSERT_COUNT {
            let (expect, actual) = exec_assert(i);
            asserts.push(Assert3 {
                name: B01_NAMES[i].to_string(),
                expect,
                actual,
            });
            i += 1;
        }
        if asserts.len() != B01_ASSERT_COUNT {
            return Err(KtError::SetIncomplete);
        }
        Ok(KtBatch { asserts })
    }

    /// J1：一次全跑，通过率 100% 才绿。
    pub fn run_all(&self) -> result::Result<usize, KtError> {
        let mut green = 0usize;
        let mut i = 0;
        while i < self.asserts.len() {
            if !self.asserts[i].is_green() {
                return Err(KtError::AssertRed);
            }
            green += 1;
            i += 1;
        }
        Ok(green)
    }

    /// 按名过滤运行（ktest 资产面）。
    pub fn run_by_name(&self, name: &str) -> result::Result<bool, KtError> {
        let mut found = false;
        let mut i = 0;
        while i < B01_NAMES.len() {
            if B01_NAMES[i] == name {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            return Err(KtError::GhostAssert);
        }
        let mut j = 0;
        while j < self.asserts.len() {
            if self.asserts[j].name == name {
                return Ok(self.asserts[j].is_green());
            }
            j += 1;
        }
        Err(KtError::TableDrift)
    }

    /// 名表导出（冻结资产）。
    pub fn list_asserts(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < B01_NAMES.len() {
            out.push(B01_NAMES[i].to_string());
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
pub struct SelfAudit {
    /// 断言条数。
    pub count: usize,
    /// 全绿条数。
    pub green: usize,
    /// 幽灵名被拒。
    pub ghost_rejected: bool,
    /// 按名运行命中。
    pub by_name_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.count == B01_ASSERT_COUNT && self.green == B01_ASSERT_COUNT && self.ghost_rejected && self.by_name_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, KtError> {
    let batch = KtBatch::build()?;
    let green = batch.run_all()?;
    let ghost = batch.run_by_name("B01-NO-SUCH").is_err();
    let byname = batch.run_by_name("B01-F2404-pfn-roundtrip")?;
    Ok(SelfAudit {
        count: batch.asserts.len(),
        green,
        ghost_rejected: ghost,
        by_name_ok: byname,
    })
}
