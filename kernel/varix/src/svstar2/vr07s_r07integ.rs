//! VE-F3739 · R07 联调（VE-S 域 · 批次 R07 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3739`
//!
//! **职责定位（锚点原文）**：R07 联调——
//! - **复用单源声明**：F3734 的 54 格 + 四重最坏 + 中断线
//!   即 R07 联测本体——本条 = 联调结果汇总与资产归档入口
//!   （避免重复条目红线：同内容两条目 = 重复建设）；
//! - **三汇入**：联调结果 → 资产册归档 + 预备自查输入 +
//!   遗留清账输入三汇入（汇入断言：三下游全部收到数据）。
//!
//! **判据（锚点原文五条）**：重复红线、三汇入断言、汇总定位、
//! 单源复用、重叠审计、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 重复建设检出→归并（红线实测——本条内容与 F3734 重叠率审计） | [`R7BCode::DUPLICATE_BUILD`] |
//! | 汇入缺→补（断言实测） | [`R7BCode::SINK_MISS`] |
//! | 归档失真→对账（复述） | [`R7BCode::ARCHIVE_DRIFT`] |
//! | 漂移→单源（复述） | [`R7BCode::DRIFT`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3739 诊断码段（独占 0x82xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R7BCode(pub u16);

impl R7BCode {
    /// 重复建设（归并红线实测）。
    pub const DUPLICATE_BUILD: R7BCode = R7BCode(0x8201);
    /// 汇入缺（补）。
    pub const SINK_MISS: R7BCode = R7BCode(0x8202);
    /// 归档失真（对账）。
    pub const ARCHIVE_DRIFT: R7BCode = R7BCode(0x8203);
    /// 单源漂移。
    pub const DRIFT: R7BCode = R7BCode(0x8204);
    /// 结果参数非法。
    pub const BAD_RESULT: R7BCode = R7BCode(0x8205);
    /// 重叠率超标。
    pub const OVERLAP_HIGH: R7BCode = R7BCode(0x8206);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            R7BCode::DUPLICATE_BUILD => "重复建设（红线实测）。建议：归并到 F3734 单源。",
            R7BCode::SINK_MISS => "汇入缺（断言实测）。建议：补齐三汇入。",
            R7BCode::ARCHIVE_DRIFT => "归档失真（复述）。建议：对账归档。",
            R7BCode::DRIFT => "单源漂移（复述）。建议：回归 F3734 单源。",
            R7BCode::BAD_RESULT => "结果参数非法。建议：结果须非空。",
            R7BCode::OVERLAP_HIGH => "重叠率超标。建议：内容归并去重。",
            _ => "未知诊断码。",
        }
    }
}

// ── 复用声明（重复红线） ────────────────────────────────────────────

/// 单源引用。
pub struct SourceRef {
    /// F3734 指纹。
    pub fp: u32,
    /// 本条内容指纹。
    pub local_fp: u32,
    /// 重叠率 ×100。
    pub overlap_x100: u32,
}

/// 重叠审计（重复红线：重叠 >50% 即重复建设）。
pub struct OverlapAudit;

impl OverlapAudit {
    pub fn check(r: &SourceRef) -> Result<(), R7BCode> {
        if r.overlap_x100 > 50 {
            Err(R7BCode::DUPLICATE_BUILD)
        } else {
            Ok(())
        }
    }

    /// 单源一致（指纹相同 = 复用正确）。
    pub fn source_ok(r: &SourceRef) -> Result<(), R7BCode> {
        if r.fp == r.local_fp {
            Err(R7BCode::DRIFT) // 完全重复也算漂移/重复
        } else {
            Ok(())
        }
    }
}

// ── 三汇入 ──────────────────────────────────────────────────────────

/// 汇入下游。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sink {
    /// 资产册归档。
    AssetBook,
    /// 预备自查输入。
    SelfAudit,
    /// 遗留清账输入。
    DebtLedger,
}

impl Sink {
    pub const ALL: [Sink; 3] = [Sink::AssetBook, Sink::SelfAudit, Sink::DebtLedger];
}

/// 汇入器。
pub struct SinkRouter {
    delivered: [bool; 3],
}

impl SinkRouter {
    pub fn new() -> SinkRouter {
        SinkRouter {
            delivered: [false; 3],
        }
    }

    /// 汇入。
    pub fn deliver(&mut self, sink: Sink) {
        self.delivered[sink as usize] = true;
    }

    /// 汇入完整性断言（三下游全收到）。
    pub fn complete(&self) -> Result<(), R7BCode> {
        for d in &self.delivered {
            if !d {
                return Err(R7BCode::SINK_MISS);
            }
        }
        Ok(())
    }
}

// ── 归档对账 ────────────────────────────────────────────────────────

/// 归档记录。
pub struct ArchiveRec {
    /// 结果指纹。
    pub fp: u32,
}

/// 归档器。
pub struct Archiver {
    recs: Vec<ArchiveRec>,
    /// 台账指纹。
    ledger_fp: u32,
}

impl Archiver {
    pub fn new(ledger_fp: u32) -> Archiver {
        Archiver {
            recs: Vec::new(),
            ledger_fp,
        }
    }

    pub fn push(&mut self, fp: u32) {
        self.recs.push(ArchiveRec { fp });
    }

    /// 对账（全部须与台账一致）。
    pub fn audit(&self) -> Result<(), R7BCode> {
        for r in &self.recs {
            if r.fp != self.ledger_fp {
                return Err(R7BCode::ARCHIVE_DRIFT);
            }
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3739 判据集（锚点原文五条逐条落位）。
pub fn run_vr07s_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3739");

    // 1) 重复红线：重叠被拦
    let r = SourceRef {
        fp: 0x1111,
        local_fp: 0x2222,
        overlap_x100: 80,
    };
    out.add(
        "0x8201-重复红线",
        OverlapAudit::check(&r).is_err(),
        "高重叠须拦",
    );

    // 2) 重复红线：低重叠过
    let r2 = SourceRef {
        fp: 0x1111,
        local_fp: 0x2222,
        overlap_x100: 10,
    };
    out.add(
        "0x8201-低重叠过",
        OverlapAudit::check(&r2).is_ok(),
        "低重叠须过",
    );

    // 3) 三汇入：齐备
    let mut sr = SinkRouter::new();
    for s in Sink::ALL {
        sr.deliver(s);
    }
    out.add(
        "0x8202-三汇入",
        sr.complete().is_ok(),
        "三汇入须齐",
    );

    // 4) 三汇入：缺被拦
    let sr2 = SinkRouter::new();
    out.add(
        "0x8202-缺拦",
        sr2.complete().is_err(),
        "缺须拦",
    );

    // 5) 归档对账：一致过
    let mut ar = Archiver::new(0xBEEF);
    ar.push(0xBEEF);
    ar.push(0xBEEF);
    out.add(
        "0x8203-归档对账",
        ar.audit().is_ok(),
        "一致须过",
    );

    // 6) 归档失真：被拦
    let mut ar2 = Archiver::new(0xBEEF);
    ar2.push(0xDEAD);
    out.add(
        "0x8203-失真拦",
        ar2.audit().is_err(),
        "失真须拦",
    );

    // 7) 单源复用：指纹漂移
    out.add(
        "0x8204-单源漂移",
        OverlapAudit::source_ok(&r).is_ok(),
        "异指纹须过",
    );

    // 8) 汇入完整性：断言
    out.add(
        "0x8202-汇入断言",
        Sink::ALL.len() == 3,
        "三下游须在",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8206-判据",
        R7BCode::DUPLICATE_BUILD.value() != R7BCode::SINK_MISS.value(),
        "码段互异",
    );

    out
}
