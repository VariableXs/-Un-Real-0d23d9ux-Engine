//! VE-F3796 · R 域变更纪律与承诺条款（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3796`
//!
//! **判据（锚点原文）**：四档七用、双承诺、通告含双维、红线判据、判据。
//!
//! **本条是什么**：变更四档制度（局部/模块/跨域/全局）合议执行 + 对外承诺
//! 五要素回执 + 变更通告含双维承诺措辞。预演不过线不得变更。
//!
//! 诊断码段：独占 0xBBxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xBBxx）
// ─────────────────────────────────────────────────────────────

/// F3796 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BBCode(pub u16);

impl BBCode {
    /// 预演不过——不得变更。
    pub const PREVIEW_BLOCK: BBCode = BBCode(0xBB01);
    /// 承诺无回执——催办。
    pub const NO_RECEIPT: BBCode = BBCode(0xBB02);
    /// 通告缺双维——P1。
    pub const NOTICE_MISS: BBCode = BBCode(0xBB03);
    /// 未合议——不得变更。
    pub const NO_BOARD: BBCode = BBCode(0xBB04);
    /// 五要素缺。
    pub const FACTOR_MISS: BBCode = BBCode(0xBB05);
    /// 档位越权。
    pub const LEVEL_OVER: BBCode = BBCode(0xBB06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xBB01 => "变更预演不过线，不得变更。",
            0xBB02 => "对外承诺无回执，需催办。",
            0xBB03 => "变更通告缺双维承诺，P1。",
            0xBB04 => "变更未经合议，不得执行。",
            0xBB05 => "承诺五要素缺失。",
            0xBB06 => "变更档位越权。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 四档变更 + 合议
// ─────────────────────────────────────────────────────────────

/// 变更档（四档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// 局部。
    Local,
    /// 模块。
    Module,
    /// 跨域。
    Cross,
    /// 全局。
    Global,
}

impl Level {
    /// 全集。
    pub const ALL: [Level; 4] = [Level::Local, Level::Module, Level::Cross, Level::Global];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Level::Local => 0,
            Level::Module => 1,
            Level::Cross => 2,
            Level::Global => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Level::Local => "局部",
            Level::Module => "模块",
            Level::Cross => "跨域",
            Level::Global => "全局",
        }
    }

    /// 所需合议票。
    pub fn votes_needed(self) -> u8 {
        match self {
            Level::Local => 1,
            Level::Module => 2,
            Level::Cross => 3,
            Level::Global => 4,
        }
    }
}

/// 变更记录。
#[derive(Clone, Copy, Debug)]
pub struct ChangeRec {
    /// 档。
    pub level: Level,
    /// 预演通过。
    pub preview_ok: bool,
    /// 合议票数。
    pub votes: u8,
    /// 通告已发（含双维）。
    pub notice: bool,
    /// 通告含双维承诺。
    pub notice_dual: bool,
}

impl ChangeRec {
    /// 新建。
    pub const fn new(level: Level) -> Self {
        ChangeRec {
            level,
            preview_ok: false,
            votes: 0,
            notice: false,
            notice_dual: false,
        }
    }

    /// 可执行（预演过 + 票够）。
    pub fn executable(self) -> bool {
        self.preview_ok && self.votes >= self.level.votes_needed()
    }

    /// 通告合规（已发且含双维）。
    pub fn notice_ok(self) -> bool {
        self.notice && self.notice_dual
    }
}

// ─────────────────────────────────────────────────────────────
// 对外承诺五要素 + 回执
// ─────────────────────────────────────────────────────────────

/// 承诺要素（五要素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseFactor {
    /// 范围。
    Scope,
    /// 质量线。
    Quality,
    /// 时限。
    Deadline,
    /// 例外。
    Exception,
    /// 回执机制。
    Receipt,
}

impl PromiseFactor {
    /// 全集。
    pub const ALL: [PromiseFactor; 5] = [
        PromiseFactor::Scope,
        PromiseFactor::Quality,
        PromiseFactor::Deadline,
        PromiseFactor::Exception,
        PromiseFactor::Receipt,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            PromiseFactor::Scope => 0,
            PromiseFactor::Quality => 1,
            PromiseFactor::Deadline => 2,
            PromiseFactor::Exception => 3,
            PromiseFactor::Receipt => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            PromiseFactor::Scope => "范围",
            PromiseFactor::Quality => "质量线",
            PromiseFactor::Deadline => "时限",
            PromiseFactor::Exception => "例外",
            PromiseFactor::Receipt => "回执机制",
        }
    }
}

/// 承诺书。
#[derive(Clone, Debug)]
pub struct PromiseDoc {
    filled: [bool; 5],
    /// 工具维回执。
    pub receipt_tool: bool,
    /// 产出维回执。
    pub receipt_output: bool,
}

impl PromiseDoc {
    /// 新建。
    pub const fn new() -> Self {
        PromiseDoc {
            filled: [false; 5],
            receipt_tool: false,
            receipt_output: false,
        }
    }

    /// 填要素。
    pub fn fill(&mut self, f: PromiseFactor) -> bool {
        let i = f.ordinal();
        if self.filled[i] {
            false
        } else {
            self.filled[i] = true;
            true
        }
    }

    /// 缺要素数。
    pub fn missing(&self) -> usize {
        self.filled.iter().filter(|f| !**f).count()
    }

    /// 五要素齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 双回执齐。
    pub fn receipts_ok(&self) -> bool {
        self.receipt_tool && self.receipt_output
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3796 判据集。
pub fn run_vr10p_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3796");

    // 档位票数
    {
        set.add("0xBB06-局部1票", Level::Local.votes_needed() == 1, "");
        set.add("0xBB06-全局4票", Level::Global.votes_needed() == 4, "");
    }

    // 0xBB01/0xBB04 执行门
    {
        let mut c = ChangeRec::new(Level::Module);
        set.add("0xBB01-未预演不执行", !c.executable(), "");
        c.preview_ok = true;
        set.add("0xBB04-票不足不执行", !c.executable(), "");
        c.votes = 2;
        set.add("0xBB04-票够可执行", c.executable(), "");
    }

    // 0xBB03 通告双维
    {
        let mut c = ChangeRec::new(Level::Local);
        set.add("0xBB03-未发不合规", !c.notice_ok(), "");
        c.notice = true;
        set.add("0xBB03-缺双维不合规", !c.notice_ok(), "");
        c.notice_dual = true;
        set.add("0xBB03-双维合规", c.notice_ok(), "");
    }

    // 0xBB05 五要素
    {
        let mut p = PromiseDoc::new();
        set.add("0xBB05-初始缺五", p.missing() == 5, "");
        for f in PromiseFactor::ALL.iter() {
            p.fill(*f);
        }
        set.add("0xBB05-五要素齐", p.complete(), "");
        set.add("0xBB05-重填拒", !p.fill(PromiseFactor::Scope), "");
    }

    // 0xBB02 双回执
    {
        let mut p = PromiseDoc::new();
        for f in PromiseFactor::ALL.iter() {
            p.fill(*f);
        }
        set.add("0xBB02-无回执不齐", !p.receipts_ok(), "");
        p.receipt_tool = true;
        set.add("0xBB02-单回执不齐", !p.receipts_ok(), "");
        p.receipt_output = true;
        set.add("0xBB02-双回执齐", p.receipts_ok(), "");
    }

    // 档序自洽
    {
        let mut ok = true;
        for (i, l) in Level::ALL.iter().enumerate() {
            if l.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBB06-档序自洽", ok, "");
    }

    // 票数单调
    {
        let mono = Level::Local.votes_needed() <= Level::Module.votes_needed()
            && Level::Module.votes_needed() <= Level::Cross.votes_needed()
            && Level::Cross.votes_needed() <= Level::Global.votes_needed();
        set.add("0xBB06-票数单调", mono, "");
    }

    set
}
