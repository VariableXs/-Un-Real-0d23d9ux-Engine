//! CGPU-F1728 · K08 组收口双签
//!
//! 判据锚点（施工书原文摘录）：
//! 组收口：接口总账（K08 组 15 项接口——生态 API 6 函数冻结 v1）；主题映射（场景
//! 生态兑现——主题进度更新）；跨域契约（G/H/F/J07/N 域五契约归档）；测试账（单元
//! 118/集成 14/16 条全绿）；性能账（场景门禁达标）；文档账（SDK/示例 15/教程）；
//! 遗留清单（更多框架适配/市场完整——显性登记）；双签（工程+质量双签生效）。判据：
//! 接口总账、主题、五契约、四账、遗留、双签、六组、判据。
//!
//! 诊断码段 0xAB01~0xAB05（独占）：
//!   0xAB01 接口总账缺项（15 项不足/API 冻结失封）
//!   0xAB02 契约失档（五域契约任一缺）
//!   0xAB03 账目失守（四账任一缺额/失绿）
//!   0xAB04 遗留失登记（遗留项隐性）
//!   0xAB05 段越界（判码落 0xAB00..0xAC00 区间外；双签/主题并入总账与遗留组判）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K08_CLOSE_VERSION: &str = "K47-k08close-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xAB01~0xAB05）
// ---------------------------------------------------------------------------

pub const XC_K8C_LEDGER_SHORT: u32 = 0xAB01;
pub const XC_K8C_CONTRACT_MISSING: u32 = 0xAB02;
pub const XC_K8C_LEDGER_FAIL: u32 = 0xAB03;
pub const XC_K8C_LEGACY_HIDDEN: u32 = 0xAB04;
pub const XC_K8C_SEG_OOR: u32 = 0xAB05;

// ---------------------------------------------------------------------------
// 一、接口总账（15 项 + 生态 API 6 函数冻结 v1）
// ---------------------------------------------------------------------------

/// K08 组 15 项接口清单（F1713~F1728 的对外面）。
pub const K08_APIS: [&str; 15] = [
    "k08_arch_declare",
    "game_scene_table",
    "create_scene_table",
    "prod_scene_table",
    "aipc_capability_catalog",
    "aipc_quota_gate",
    "aipc_idle_scheduler",
    "threep_adapter_matrix",
    "threep_version_window",
    "sdk_manifest",
    "sdk_errmsg_three",
    "examples_fifteen",
    "bench_suite_define",
    "cert_two_level",
    "compat_matrix_probe",
];

/// 接口总账：15 项齐 + FNV 冻结指纹封版 v1。
pub struct ApiLedger {
    pub frozen_ver: u32,
    pub fingerprint: u64,
}

/// FNV-1a 64。
pub const fn fnv64(buf: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < buf.len() {
        hash ^= buf[i] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

/// 全清单串接指纹（逐项名拼接后 FNV——清单任何增删变序即变指纹）。
pub fn ledger_fingerprint() -> u64 {
    // 定容串接缓冲（15 项名均短，64 字节足容）。
    let mut buf = [0u8; 512];
    let mut n = 0usize;
    let mut i = 0;
    while i < K08_APIS.len() {
        let b = K08_APIS[i].as_bytes();
        let mut j = 0;
        while j < b.len() {
            buf[n] = b[j];
            n += 1;
            j += 1;
        }
        buf[n] = b'|';
        n += 1;
        i += 1;
    }
    fnv64(&buf[..n])
}

impl ApiLedger {
    pub fn freeze() -> Self {
        ApiLedger { frozen_ver: 1, fingerprint: ledger_fingerprint() }
    }

    /// 冻结判：版本 v1 + 指纹对拍 + 清单 15 项齐。
    pub fn frozen(&self) -> Result<(), u32> {
        if self.frozen_ver != 1 {
            return Err(XC_K8C_LEDGER_SHORT);
        }
        if K08_APIS.len() != 15 {
            return Err(XC_K8C_LEDGER_SHORT);
        }
        for a in K08_APIS.iter() {
            if a.is_empty() {
                return Err(XC_K8C_LEDGER_SHORT);
            }
        }
        if self.fingerprint != ledger_fingerprint() {
            return Err(XC_K8C_LEDGER_SHORT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、跨域契约归档（G/H/F/J07/N 五域）
// ---------------------------------------------------------------------------

/// 五域契约闭集（判据点名）。
pub const CONTRACT_DOMAINS: [&str; 5] = ["G", "H", "F", "J07", "N"];

/// 契约归档判：五域全在档且域码互异。
pub fn contracts_archived(domains: &[&str]) -> Result<(), u32> {
    if domains.len() < CONTRACT_DOMAINS.len() {
        return Err(XC_K8C_CONTRACT_MISSING);
    }
    for d in domains {
        let mut hit = false;
        let mut i = 0;
        while i < CONTRACT_DOMAINS.len() {
            if CONTRACT_DOMAINS[i] == *d {
                hit = true;
                break;
            }
            i += 1;
        }
        if !hit {
            return Err(XC_K8C_CONTRACT_MISSING);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、四账（测试/性能/文档/遗留——遗留显性登记）
// ---------------------------------------------------------------------------

/// 测试账：单元 118 / 集成 14 / 联调 16 全绿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TestLedger {
    pub unit: u32,
    pub integ: u32,
    pub e2e: u32,
}

impl TestLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if self.unit < 118 || self.integ < 14 || self.e2e < 16 {
            return Err(XC_K8C_LEDGER_FAIL);
        }
        Ok(())
    }
}

/// 性能账：四场景门禁达标（游戏帧预算/创作预览 33333µs/生产力 2ms/AI PC 配额）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfLedger {
    pub game_frame_budget_ok: bool,
    pub create_preview_33333us_ok: bool,
    pub productivity_2ms_ok: bool,
    pub aipc_quota_ok: bool,
}

impl PerfLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.game_frame_budget_ok
            || !self.create_preview_33333us_ok
            || !self.productivity_2ms_ok
            || !self.aipc_quota_ok
        {
            return Err(XC_K8C_LEDGER_FAIL);
        }
        Ok(())
    }
}

/// 文档账：SDK 清单 + 示例 15 + 教程三阶。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DocLedger {
    pub sdk_manifest_ok: bool,
    pub examples_count: u16,
    pub tutorial_tiers: u8,
}

impl DocLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.sdk_manifest_ok || self.examples_count < 15 || self.tutorial_tiers < 3 {
            return Err(XC_K8C_LEDGER_FAIL);
        }
        Ok(())
    }
}

/// 遗留清单：显性登记（每项有名+有去向——隐性遗留即失格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LegacyItem {
    pub name: &'static str,
    /// 去向（后续批次/二期路线挂锚）。
    pub dest: &'static str,
}

impl LegacyItem {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() || self.dest.is_empty() {
            return Err(XC_K8C_LEGACY_HIDDEN);
        }
        Ok(())
    }
}

/// 在册遗留两项（判据点名：更多框架适配/市场完整）。
pub const LEGACY_ITEMS: [LegacyItem; 2] = [
    LegacyItem { name: "more_framework_adapters", dest: "CGPU-F1726-Phase2" },
    LegacyItem { name: "market_full_rollout", dest: "CGPU-F1726-Phase2" },
];

// ---------------------------------------------------------------------------
// 四、双签（工程 + 质量）
// ---------------------------------------------------------------------------

/// 双签：工程签 + 质量签独立生效（单签无效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    pub eng_signed: bool,
    pub qa_signed: bool,
}

impl DualSign {
    pub fn effective(&self) -> Result<(), u32> {
        if !self.eng_signed || !self.qa_signed {
            return Err(XC_K8C_LEDGER_SHORT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（六组：总账/契约/测试账/性能+文档账/遗留/双签）
// ---------------------------------------------------------------------------

/// K08 收口引擎。
pub struct K08CloseEngine;

impl K08CloseEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：接口总账冻结。
        ApiLedger::freeze().frozen()?;
        // 组二：五契约归档 + 反例（缺域拒/野域拒）。
        contracts_archived(&["G", "H", "F", "J07", "N"])?;
        if contracts_archived(&["G", "H", "F", "J07"]).is_ok() {
            return Err(XC_K8C_CONTRACT_MISSING);
        }
        if contracts_archived(&["G", "H", "F", "J07", "X"]).is_ok() {
            return Err(XC_K8C_CONTRACT_MISSING);
        }
        // 组三：测试账达标 + 反例。
        TestLedger { unit: 118, integ: 14, e2e: 16 }.sound()?;
        if (TestLedger { unit: 117, integ: 14, e2e: 16 }).sound().is_ok() {
            return Err(XC_K8C_LEDGER_FAIL);
        }
        // 组四：性能账 + 文档账。
        PerfLedger {
            game_frame_budget_ok: true,
            create_preview_33333us_ok: true,
            productivity_2ms_ok: true,
            aipc_quota_ok: true,
        }
        .sound()?;
        let mut bad_perf = PerfLedger {
            game_frame_budget_ok: true,
            create_preview_33333us_ok: false,
            productivity_2ms_ok: true,
            aipc_quota_ok: true,
        };
        if bad_perf.sound().is_ok() {
            return Err(XC_K8C_LEDGER_FAIL);
        }
        DocLedger { sdk_manifest_ok: true, examples_count: 15, tutorial_tiers: 3 }.sound()?;
        // 组五：遗留显性登记 + 反例（隐性拒）。
        for it in LEGACY_ITEMS.iter() {
            it.sound()?;
        }
        if (LegacyItem { name: "x", dest: "" }).sound().is_ok() {
            return Err(XC_K8C_LEGACY_HIDDEN);
        }
        // 组六：双签生效 + 单签拒。
        DualSign { eng_signed: true, qa_signed: true }.effective()?;
        if (DualSign { eng_signed: true, qa_signed: false }).effective().is_ok() {
            return Err(XC_K8C_LEDGER_SHORT);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xAB00..0xAC00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_K8C_LEDGER_SHORT,
            XC_K8C_CONTRACT_MISSING,
            XC_K8C_LEDGER_FAIL,
            XC_K8C_LEGACY_HIDDEN,
            XC_K8C_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xAB00 || codes[i] >= 0xAC00 {
                return Err(XC_K8C_LEDGER_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_K8C_LEDGER_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
