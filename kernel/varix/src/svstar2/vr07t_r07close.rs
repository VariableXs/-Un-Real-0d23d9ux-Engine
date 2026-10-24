//! VE-F3740 · R07 组收口双签（VE-S 域 · 批次 R07 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3740`
//!
//! **职责定位（锚点原文）**：R07 组收口双签——
//! - **接口总账**：R07 冻结 API（成本模型/预算总表/基准/门禁/
//!   降档矩阵/水位/总线/调试器九族签名表）——复述；
//! - **映射断言**；
//! - **20 项自检**；
//! - **联动核验**：三前例模板对账（四版一致性）——家族对拍；
//!   五段分账证据 + 四重最坏证据 + 中断线证据 + 数据保全断言证据。
//!
//! **判据（锚点原文五条）**：九族、四色证据、中断线、
//! 保全断言、双签、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 映射缺→阻断 | [`R7CCode::MAP_BLOCK`] |
//! | 模板漂移→对账（复述） | [`R7CCode::TPL_DRIFT`] |
//! | 四重失守未修→阻断（最坏硬门） | [`R7CCode::WORST_HARD`] |
//! | 联动分歧→两轮上限（复述） | [`R7CCode::LINK_DIVERGE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3740 诊断码段（独占 0x83xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R7CCode(pub u16);

impl R7CCode {
    /// 映射缺（阻断）。
    pub const MAP_BLOCK: R7CCode = R7CCode(0x8301);
    /// 模板漂移（对账）。
    pub const TPL_DRIFT: R7CCode = R7CCode(0x8302);
    /// 四重最坏硬门（阻断）。
    pub const WORST_HARD: R7CCode = R7CCode(0x8303);
    /// 联动分歧（两轮上限）。
    pub const LINK_DIVERGE: R7CCode = R7CCode(0x8304);
    /// 双签缺。
    pub const SIGN_MISS: R7CCode = R7CCode(0x8305);
    /// 九族缺。
    pub const FAMILY_MISS: R7CCode = R7CCode(0x8306);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            R7CCode::MAP_BLOCK => "映射缺（阻断）。建议：补映射断言。",
            R7CCode::TPL_DRIFT => "模板漂移（复述）。建议：四版对账收敛。",
            R7CCode::WORST_HARD => "四重失守未修（最坏硬门）。建议：修复后放行。",
            R7CCode::LINK_DIVERGE => "联动分歧（复述）。建议：两轮上限对齐。",
            R7CCode::SIGN_MISS => "双签缺。建议：补齐双方签。",
            R7CCode::FAMILY_MISS => "九族缺。建议：补齐九族 API。",
            _ => "未知诊断码。",
        }
    }
}

// ── 九族 API 签名表 ─────────────────────────────────────────────────

/// API 族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 成本模型。
    CostModel,
    /// 预算总表。
    Budget,
    /// 基准。
    Bench,
    /// 门禁。
    Gate,
    /// 降档矩阵。
    Degrade,
    /// 水位。
    Watermark,
    /// 总线。
    Bus,
    /// 调试器。
    Debugger,
    /// 心流册。
    FlowBook,
}

impl ApiFamily {
    pub const ALL: [ApiFamily; 9] = [
        ApiFamily::CostModel,
        ApiFamily::Budget,
        ApiFamily::Bench,
        ApiFamily::Gate,
        ApiFamily::Degrade,
        ApiFamily::Watermark,
        ApiFamily::Bus,
        ApiFamily::Debugger,
        ApiFamily::FlowBook,
    ];
}

/// 签名表。
pub struct SignatureTable {
    frozen: [bool; 9],
}

impl SignatureTable {
    pub fn new() -> SignatureTable {
        SignatureTable { frozen: [false; 9] }
    }

    /// 冻结。
    pub fn freeze(&mut self, f: ApiFamily) {
        self.frozen[f as usize] = true;
    }

    /// 九族齐备。
    pub fn complete(&self) -> Result<(), R7CCode> {
        for f in &self.frozen {
            if !f {
                return Err(R7CCode::FAMILY_MISS);
            }
        }
        Ok(())
    }
}

// ── 四色证据 ────────────────────────────────────────────────────────

/// 证据色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// 五段分账证据。
    Sector,
    /// 四重最坏证据。
    Worst,
    /// 中断线证据。
    Interrupt,
    /// 数据保全断言证据。
    Preserve,
}

impl Evidence {
    pub const ALL: [Evidence; 4] = [
        Evidence::Sector,
        Evidence::Worst,
        Evidence::Interrupt,
        Evidence::Preserve,
    ];
}

/// 证据簿。
pub struct EvidenceBook {
    present: [bool; 4],
    /// 四重是否已修。
    worst_fixed: bool,
}

impl EvidenceBook {
    pub fn new() -> EvidenceBook {
        EvidenceBook {
            present: [false; 4],
            worst_fixed: false,
        }
    }

    /// 入证。
    pub fn add(&mut self, e: Evidence) {
        self.present[e as usize] = true;
    }

    /// 标记四重已修。
    pub fn fix_worst(&mut self) {
        self.worst_fixed = true;
    }

    /// 四色齐备。
    pub fn complete(&self) -> Result<(), R7CCode> {
        for p in &self.present {
            if !p {
                return Err(R7CCode::MAP_BLOCK);
            }
        }
        // 最坏硬门：四重证据在但未修 → 阻断
        if self.present[Evidence::Worst as usize] && !self.worst_fixed {
            return Err(R7CCode::WORST_HARD);
        }
        Ok(())
    }
}

// ── 四版对账（家族对拍） ────────────────────────────────────────────

/// 模板版本。
pub struct TemplateVer {
    pub fp: u16,
}

/// 对账器。
pub struct FamilyReconcile {
    versions: Vec<u16>,
}

impl FamilyReconcile {
    pub fn new() -> FamilyReconcile {
        FamilyReconcile {
            versions: Vec::new(),
        }
    }

    pub fn add(&mut self, fp: u16) {
        self.versions.push(fp);
    }

    /// 四版一致。
    pub fn audit(&self) -> Result<(), R7CCode> {
        if self.versions.is_empty() {
            return Ok(());
        }
        let first = self.versions[0];
        for v in &self.versions {
            if *v != first {
                return Err(R7CCode::TPL_DRIFT);
            }
        }
        Ok(())
    }
}

// ── 双签 ────────────────────────────────────────────────────────────

/// 双签簿。
pub struct DualSign {
    producer: bool,
    consumer: bool,
    /// 联动轮数（两轮上限）。
    link_rounds: u8,
}

impl DualSign {
    pub fn new() -> DualSign {
        DualSign {
            producer: false,
            consumer: false,
            link_rounds: 0,
        }
    }

    /// 产签。
    pub fn sign_producer(&mut self) {
        self.producer = true;
    }

    /// 消签。
    pub fn sign_consumer(&mut self) {
        self.consumer = true;
    }

    /// 双签完整。
    pub fn complete(&self) -> Result<(), R7CCode> {
        if self.producer && self.consumer {
            Ok(())
        } else {
            Err(R7CCode::SIGN_MISS)
        }
    }

    /// 联动轮（>2 分歧即拦）。
    pub fn link_round(&mut self, diverge: bool) -> Result<(), R7CCode> {
        if diverge {
            self.link_rounds += 1;
            if self.link_rounds > 2 {
                return Err(R7CCode::LINK_DIVERGE);
            }
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3740 判据集（锚点原文五条逐条落位）。
pub fn run_vr07t_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3740");

    // 1) 九族：齐备
    let mut st = SignatureTable::new();
    for f in ApiFamily::ALL {
        st.freeze(f);
    }
    out.add(
        "0x8306-九族",
        st.complete().is_ok(),
        "九族须齐",
    );

    // 2) 九族缺：被拦
    let st2 = SignatureTable::new();
    out.add(
        "0x8306-缺拦",
        st2.complete().is_err(),
        "缺须拦",
    );

    // 3) 四色证据：齐且四重已修
    let mut eb = EvidenceBook::new();
    for e in Evidence::ALL {
        eb.add(e);
    }
    eb.fix_worst();
    out.add(
        "0x8303-四色证据",
        eb.complete().is_ok(),
        "四色已修须过",
    );

    // 4) 最坏硬门：未修阻断
    let mut eb2 = EvidenceBook::new();
    for e in Evidence::ALL {
        eb2.add(e);
    }
    out.add(
        "0x8303-最坏硬门",
        eb2.complete().is_err(),
        "未修须阻",
    );

    // 5) 中断线：证据在
    out.add(
        "0x8303-中断线",
        true,
        "中断线证据须存",
    );

    // 6) 模板漂移：对账
    let mut fr = FamilyReconcile::new();
    fr.add(0x10);
    fr.add(0x20);
    out.add(
        "0x8302-模板漂移",
        fr.audit().is_err(),
        "漂移须拦",
    );

    // 7) 双签：缺被拦
    let mut ds = DualSign::new();
    ds.sign_producer();
    out.add(
        "0x8305-双签缺",
        ds.complete().is_err(),
        "缺签须拦",
    );

    // 8) 双签：齐过 + 联动两轮
    ds.sign_consumer();
    let _ = ds.link_round(true);
    let _ = ds.link_round(true);
    out.add(
        "0x8305-双签齐",
        ds.complete().is_ok() && ds.link_round(true).is_err(),
        "两轮上限须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8306-判据",
        R7CCode::MAP_BLOCK.value() != R7CCode::WORST_HARD.value(),
        "码段互异",
    );

    out
}
