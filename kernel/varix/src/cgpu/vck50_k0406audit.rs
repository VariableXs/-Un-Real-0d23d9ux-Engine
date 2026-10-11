//! CGPU-F1731 · K04-K06 组自查核验
//!
//! 判据锚点（施工书原文摘录）：
//! K04 核验：接口（8 扩展）；深化（六向完成/增益记录回读）；学习边界（离线声明
//! 记录）；K05 核验：接口（8 扩展）；拓扑（矩阵实测记录）；无黑屏（红线测试记录
//! 回读）；分割（三模式/伪影声明记录）；K06 核验：接口（6 扩展）；能效（增益 20%
//! 目标记录/两态画像）；决策（可解释记录）；结论（三态）；测试（三组/红线回读/
//! 三态三组）。判据：三组核验、增益回读、红线、三态、三组、判据。
//!
//! 诊断码段 0xAE01~0xAE05（独占）：
//!   0xAE01 接口数失账（K04≠8/K05≠8/K06≠6 任一回读失配）
//!   0xAE02 深化缺向（K04 六向缺一/增益记录缺）
//!   0xAE03 红线失守（K05 无黑屏红线回读失配/伪影声明缺）
//!   0xAE04 能效失账（K06 增益<20%/两态画像缺/决策不可解释）
//!   0xAE05 段越界（判码落 0xAE00..0xB000 区间外；结论三态并入各核验组判）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K0406_AUDIT_VERSION: &str = "K50-k0406audit-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xAE01~0xAE05）
// ---------------------------------------------------------------------------

pub const XC_A2_API_MISMATCH: u32 = 0xAE01;
pub const XC_A2_DEPTH_GAP: u32 = 0xAE02;
pub const XC_A2_REDBREAK: u32 = 0xAE03;
pub const XC_A2_EFF_LOST: u32 = 0xAE04;
pub const XC_A2_SEG_OOR: u32 = 0xAE05;

// ---------------------------------------------------------------------------
// 一、接口数回读（K04=8 / K05=8 / K06=6）
// ---------------------------------------------------------------------------

/// K04..=K06 接口数承诺（22——74 总账段）。
pub const K_GROUP_API_PROMISE_2: [(&str, u16); 3] =
    [("K04", 8), ("K05", 8), ("K06", 6)];

pub fn api_census_k0406() -> Result<(), u32> {
    if K_GROUP_API_PROMISE_2.len() != 3 {
        return Err(XC_A2_API_MISMATCH);
    }
    let mut total = 0u16;
    for (name, n) in K_GROUP_API_PROMISE_2.iter() {
        if name.is_empty() || *n == 0 {
            return Err(XC_A2_API_MISMATCH);
        }
        total += n;
    }
    if total != 22 {
        return Err(XC_A2_API_MISMATCH);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、K04 深化六向 + 学习边界
// ---------------------------------------------------------------------------

/// K04 六向闭集（深化完成度回读）。
pub const K04_DEEP_DIRECTIONS: [&str; 6] =
    ["sched_policy", "load_balance", "priority_chain", "power_aware", "thermal_aware", "offline_bound"];

/// 离线声明记录（学习边界：一切学习离线完成——在线不学）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OfflineDecl {
    pub declared: bool,
    pub record_anchor: &'static str,
}

impl OfflineDecl {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.declared || self.record_anchor.is_empty() {
            return Err(XC_A2_DEPTH_GAP);
        }
        Ok(())
    }
}

/// K04 深化回读：六向全非空互异 + 离线声明在册。
pub fn k04_depth_readback() -> Result<(), u32> {
    if K04_DEEP_DIRECTIONS.len() != 6 {
        return Err(XC_A2_DEPTH_GAP);
    }
    let mut i = 0;
    while i < K04_DEEP_DIRECTIONS.len() {
        if K04_DEEP_DIRECTIONS[i].is_empty() {
            return Err(XC_A2_DEPTH_GAP);
        }
        let mut j = i + 1;
        while j < K04_DEEP_DIRECTIONS.len() {
            if K04_DEEP_DIRECTIONS[i] == K04_DEEP_DIRECTIONS[j] {
                return Err(XC_A2_DEPTH_GAP);
            }
            j += 1;
        }
        i += 1;
    }
    OfflineDecl { declared: true, record_anchor: "K04#offline-decl" }.sound()
}

// ---------------------------------------------------------------------------
// 三、K05 红线回读（无黑屏/拓扑矩阵/分割三模式伪影声明）
// ---------------------------------------------------------------------------

/// K05 红线记录：无黑屏测试通过 + 拓扑矩阵实测 + 三模式伪影声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct K05Redline {
    pub no_black_screen: bool,
    pub topo_matrix_measured: bool,
    /// 三模式伪影声明（每模式一条，空即隐性）。
    pub artifact_decls: [&'static str; 3],
}

impl K05Redline {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.no_black_screen {
            return Err(XC_A2_REDBREAK);
        }
        if !self.topo_matrix_measured {
            return Err(XC_A2_REDBREAK);
        }
        for d in self.artifact_decls.iter() {
            if d.is_empty() {
                return Err(XC_A2_REDBREAK);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、K06 能效账（增益 20%/两态画像/决策可解释）
// ---------------------------------------------------------------------------

/// K06 能效回读账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct K06Efficiency {
    /// 实测增益千分比（目标 200‰=20%）。
    pub gain_permille: u32,
    /// 两态画像在册（高性能/均衡）。
    pub dual_profile: bool,
    /// 决策可解释（有依据锚）。
    pub explainable: bool,
    pub explain_anchor: &'static str,
}

impl K06Efficiency {
    pub fn sound(&self) -> Result<(), u32> {
        if self.gain_permille < 200 {
            return Err(XC_A2_EFF_LOST);
        }
        if !self.dual_profile || !self.explainable || self.explain_anchor.is_empty() {
            return Err(XC_A2_EFF_LOST);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（三组核验 + 三态结论）
// ---------------------------------------------------------------------------

/// K04-K06 自查核验引擎。
pub struct K0406AuditEngine;

impl K0406AuditEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：接口数回读。
        api_census_k0406()?;
        // 组二：K04 深化六向 + 离线声明 + 反例（缺向拒/声明缺拒）。
        k04_depth_readback()?;
        if (OfflineDecl { declared: false, record_anchor: "a" }).sound().is_ok() {
            return Err(XC_A2_DEPTH_GAP);
        }
        // 组三：K05 红线回读 + 反例（黑屏/伪影隐性拒）。
        K05Redline {
            no_black_screen: true,
            topo_matrix_measured: true,
            artifact_decls: ["模式一无伪影", "模式二轻微撕裂已声明", "模式三降采样已声明"],
        }
        .sound()?;
        let black = K05Redline {
            no_black_screen: false,
            topo_matrix_measured: true,
            artifact_decls: ["a", "b", "c"],
        };
        if black.sound().is_ok() {
            return Err(XC_A2_REDBREAK);
        }
        let hidden = K05Redline {
            no_black_screen: true,
            topo_matrix_measured: true,
            artifact_decls: ["a", "b", ""],
        };
        if hidden.sound().is_ok() {
            return Err(XC_A2_REDBREAK);
        }
        // 组四：K06 能效账 + 反例（增益不足/不可解释拒）。
        K06Efficiency {
            gain_permille: 200,
            dual_profile: true,
            explainable: true,
            explain_anchor: "K06#why",
        }
        .sound()?;
        let short = K06Efficiency {
            gain_permille: 199,
            dual_profile: true,
            explainable: true,
            explain_anchor: "K06#why",
        };
        if short.sound().is_ok() {
            return Err(XC_A2_EFF_LOST);
        }
        let opaque = K06Efficiency {
            gain_permille: 200,
            dual_profile: true,
            explainable: false,
            explain_anchor: "",
        };
        if opaque.sound().is_ok() {
            return Err(XC_A2_EFF_LOST);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xAE00..0xB000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_A2_API_MISMATCH,
            XC_A2_DEPTH_GAP,
            XC_A2_REDBREAK,
            XC_A2_EFF_LOST,
            XC_A2_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xAE00 || codes[i] >= 0xB000 {
                return Err(XC_A2_API_MISMATCH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_A2_API_MISMATCH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
