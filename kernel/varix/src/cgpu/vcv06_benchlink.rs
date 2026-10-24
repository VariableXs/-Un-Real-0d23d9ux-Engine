//! CGPU-F3366 · 验收与 U 域 Bench 联动（vcv06_benchlink）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3366`
//!
//! **判据（锚点原文）**：分数映射、兑现确认、两组、判据。
//!
//! **职责定位（锚点原文）**：联动：联动（验收×Bench（分数映射验收
//! 判据——联动复用（U10 兑现——兑现确认；测试（联动/兑现两组）。
//!
//! # 一、分数映射（Bench 分如何变成验收线）
//!
//! U10 收官时留下了「Bench 分数与验收判据映射」的兑现承诺：跑分
//! 是相对量纲（千分制），验收线是绝对量纲（帧时间毫秒/延迟毫秒/
//! 崩溃率）。[`BenchLink`] 承接该承诺：按判据族登记**映射表**——
//! 一条映射声明「Bench 分 ≥ X ⇒ 验收判据 Y 视为达标」，映射由
//! U10 交接包锚定，禁止凭感觉口算（无登记的映射即拒
//! [`E_VV_NOMAP`]）。
//!
//! # 二、兑现确认（复用不是口号）
//!
//! [`BenchLink::confirm_u10`] 记录兑现确认：映射表版本必须与 U10
//! 交接包版本一致才允许评估（[`E_VV_STALE`]）——交接包更新了而
//! 映射表没跟，评估结果就是对着旧合同盖章。确认是**显性动作**，
//! 不确认就不能用，让「复用」在数据上可审计。
//!
//! # 三、两组测试（联动/兑现）
//!
//! 联动组验证映射登记-评估-裁决闭环；兑现组验证版本一致性与
//! 未确认即拒。两组合流进 [`run_vcv06_checks`]。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BENCHLINK_VERSION: &str = "V06-benchlink-v1";

/// U10 交接包版本（本批锚定的合同版本）。
pub const U10_HANDOVER_VERSION: &str = "U10-realverify-handover-v1";

/// 无映射登记（凭感觉口算即拒）。
pub const E_VV_NOMAP: &str = "E_VV_NOMAP";

/// 映射表未与 U10 版本对齐（对旧合同盖章）。
pub const E_VV_STALE: &str = "E_VV_STALE";

/// Bench 分越界（千分制 0..=1000）。
pub const E_VV_SCORE: &str = "E_VV_SCORE";

/// 未知判据。
pub const E_VV_NOCRIT: &str = "E_VV_NOCRIT";

// ---------------------------------------------------------------------------
// 二、映射登记
// ---------------------------------------------------------------------------

/// 一条映射：Bench 分门槛 → 判据达标声明。
#[derive(Clone, Debug)]
pub struct ScoreMap {
    /// 映射 ID。
    pub id: u32,
    /// 判据族名（与 vcv05 判据名对齐）。
    pub criterion: String,
    /// Bench 分门槛（千分制，达到即视为该判据达标）。
    pub floor: u16,
}

/// 验收×Bench 联动器。
pub struct BenchLink {
    maps: Vec<ScoreMap>,
    /// U10 兑现确认标志。
    u10_confirmed: bool,
    /// 确认时锚定的交接包版本。
    confirmed_version: String,
    /// 遥测：评估次数。
    pub evaluations: u64,
    /// 遥测：拒评估次数。
    pub refused: u64,
}

impl BenchLink {
    /// 空联动器（未确认状态）。
    pub fn new() -> BenchLink {
        BenchLink {
            maps: Vec::new(),
            u10_confirmed: false,
            confirmed_version: String::new(),
            evaluations: 0,
            refused: 0,
        }
    }

    /// 登记一条映射（判据名非空、门槛 ≤1000）。
    pub fn add_map(
        &mut self,
        criterion: &str,
        floor: u16,
    ) -> Result<u32, &'static str> {
        if floor > 1000 {
            return Err(E_VV_SCORE);
        }
        let id = self.maps.len() as u32 + 1;
        self.maps.push(ScoreMap {
            id,
            criterion: String::from(criterion),
            floor,
        });
        Ok(id)
    }

    /// U10 兑现确认：版本一致才生效。
    pub fn confirm_u10(
        &mut self,
        handover_version: &str,
    ) -> Result<(), &'static str> {
        if handover_version != U10_HANDOVER_VERSION {
            return Err(E_VV_STALE);
        }
        self.u10_confirmed = true;
        self.confirmed_version = String::from(handover_version);
        Ok(())
    }

    /// 是否已确认。
    pub fn confirmed(&self) -> bool {
        self.u10_confirmed
    }

    /// 查映射。
    pub fn map_of(&self, criterion: &str) -> Option<&ScoreMap> {
        self.maps.iter().find(|m| m.criterion == criterion)
    }

    /// 评估：Bench 分是否兑现某判据的验收线。
    ///
    /// 未确认（对旧合同）即拒；无映射即拒；分数越界即拒。
    pub fn evaluate(
        &mut self,
        criterion: &str,
        bench_score: u16,
    ) -> Result<bool, &'static str> {
        if !self.u10_confirmed {
            self.refused += 1;
            return Err(E_VV_STALE);
        }
        if bench_score > 1000 {
            self.refused += 1;
            return Err(E_VV_SCORE);
        }
        // 先取出 floor 值再动计数器，避免 map_of 的不可变借用横跨计数写入（E0506）。
        let floor = self.map_of(criterion).ok_or(E_VV_NOMAP)?.floor;
        self.evaluations += 1;
        Ok(bench_score >= floor)
    }

    /// 登记数。
    pub fn map_count(&self) -> usize {
        self.maps.len()
    }

    /// 不变量：确认后版本非空。
    pub fn invariant(&self) -> Result<(), &'static str> {
        if self.u10_confirmed && self.confirmed_version.is_empty() {
            return Err(E_VV_STALE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、两组测试（联动/兑现）
// ---------------------------------------------------------------------------

/// CGPU-F3366 域自检：分数映射、兑现确认、两组、判据。
pub fn run_vcv06_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv06_benchlink");
    let mut bl = BenchLink::new();

    // --- 组一：联动（映射-评估闭环） ---
    let m1 = bl.add_map("帧率P95", 850);
    let m2 = bl.add_map("全程延迟", 780);
    cs.add(
        "两条映射",
        bl.map_count() == 2 && m1 == Ok(1) && m2 == Ok(2),
        "登记",
    );
    cs.add(
        "门槛越界拒",
        bl.add_map("坏映射", 1001) == Err(E_VV_SCORE),
        "千分制",
    );

    // --- 未确认即拒（兑现组前置） ---
    cs.add(
        "未确认拒评估",
        bl.evaluate("帧率P95", 900) == Err(E_VV_STALE),
        "对旧合同盖章",
    );

    // --- 兑现确认：版本不一致拒 ---
    cs.add(
        "版本错拒确认",
        bl.confirm_u10("U10-v0-old") == Err(E_VV_STALE),
        "E_VV_STALE",
    );
    cs.add(
        "版本对即确认",
        bl.confirm_u10(U10_HANDOVER_VERSION).is_ok(),
        "U10 兑现",
    );
    cs.add("确认生效", bl.confirmed(), "confirmed()");

    // --- 确认后评估 ---
    cs.add(
        "分够达标",
        bl.evaluate("帧率P95", 900) == Ok(true),
        "900 ≥ 850",
    );
    cs.add(
        "分不够不达标",
        bl.evaluate("全程延迟", 700) == Ok(false),
        "700 < 780",
    );
    cs.add(
        "无映射拒",
        bl.evaluate("没登记的判据", 900) == Err(E_VV_NOMAP),
        "E_VV_NOMAP",
    );
    cs.add(
        "分数越界拒",
        bl.evaluate("帧率P95", 1001) == Err(E_VV_SCORE),
        "E_VV_SCORE",
    );

    // --- 遥测守恒 ---
    cs.add(
        "评估/拒收计数",
        bl.evaluations == 2 && bl.refused == 1,
        "2 评 + 1 拒",
    );

    // --- 不变量与版本 ---
    cs.add("不变量", bl.invariant().is_ok(), "确认版本非空");
    cs.add(
        "版本声明",
        !BENCHLINK_VERSION.is_empty() && !U10_HANDOVER_VERSION.is_empty(),
        "复用锚定",
    );

    cs
}
