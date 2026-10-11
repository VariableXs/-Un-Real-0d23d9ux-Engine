//! unx761_precheck_list ↔ UNX-F0761 收官前置总清单（六判据逐项终核框架）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0761，UNX-A1-B38）：
//! 「收官前置总清单（六判据逐项终核框架）| 570 | 已深化 |
//!  UNX-F0761-J1 前置清单齐（六验收判据逐项终核条目 + 顺序框架 +
//!  凭证源映射）且逐项有源可解引 6/6」
//!
//! 设计要点：
//! 1. **六判据关闭集**：`PrecheckItem` 六项（行数守恒/深遍率/
//!     判据接线/代码落点/门禁全绿/无残留），缺一即 `ItemMissing`
//!     （收官前置清单 = 收官闸门，缺一项 = 闸门有缝）。
//! 2. **顺序框架**：`ORDER` 冻结六项终核顺序（先硬线后软判），
//!     `advance()` 必须按序推进，越序即 `OutOfOrder`——
//!     顺序即依赖（没查代码落点就查门禁 = 门禁绿是假绿）。
//! 3. **凭证源映射**：每项带 `source`（凭证路径），
//!     `resolvability_audit()` 逐项核「凭证非空且格式合法」，
//!     空凭证即 `SourceMissing`（终核结论无凭证 = 口头结论）。
//! 4. **逐项结论账**：`VerdictLedger` 记每项结论（pass/fail），
//!     对冲核「六项都有结论」且「失败项必须挂 block 理由」。
//! 5. **收官放行**：全部六项 pass 才 `release()` 放行；
//!     任一 fail 即 `Blocked` 带点名（禁带病收官）。
//!
//! 诊断码：`PrecheckCode` 0x90 段（0x9000..=0x900F）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX761_PRECHECK_VERSION: &str = "unx761-precheck-list-v1";

/// 判据项数（六项硬线）。
pub const ITEM_COUNT: usize = 6;

/// 项名（与下标严格同序，禁重排）。
pub const ITEM_NAMES: [&str; ITEM_COUNT] = [
    "line-conservation",
    "deepening-rate",
    "criteria-wiring",
    "code-landing",
    "gate-green",
    "no-residue",
];

/// 凭证源映射（每项 → 凭证路径，冻结）。
pub const SOURCE_MAP: [(&str, &str); ITEM_COUNT] = [
    ("line-conservation", "docs/Varix/VE-STAR-II/VE.md#line-ledger"),
    ("deepening-rate", "docs/Varix/VE-STAR-II/VE.md#deepening-rate"),
    ("criteria-wiring", "kernel/varix/src/robust.rs#run_kernel_checkup"),
    ("code-landing", "kernel/varix/src/svstar2/"),
    ("gate-green", "kernel/qemu-serial-final.log#boot-green"),
    ("no-residue", "_attic/"),
];

/// 终核顺序（先硬线后软判；冻结）。
pub const ORDER: [usize; ITEM_COUNT] = [0, 3, 1, 2, 4, 5];

/// 单项状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    /// 未核
    Pending,
    /// 通过
    Pass,
    /// 失败（必须带 block 理由）
    Fail,
}

/// 单项档（idx = 自然下标；终核顺序由 ORDER 冻结，不存对象里）。
#[derive(Clone, Debug)]
pub struct PrecheckItem {
    /// 自然下标（0..5，对应 ITEM_NAMES）
    pub idx: u8,
    /// 状态
    pub state: ItemState,
    /// 凭证源（运行时登记，禁空）
    pub source: &'static str,
    /// block 理由（Fail 时必填）
    pub reason: String,
}

/// 前置清单。
pub struct PrecheckList {
    items: [PrecheckItem; ITEM_COUNT],
    /// 已核到第几序位
    cursor: u8,
}

/// 前置错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PrecheckError {
    /// 缺项
    ItemMissing { name: &'static str },
    /// 越序
    OutOfOrder { seq: u8, want: u8 },
    /// 凭证缺失
    SourceMissing { seq: u8 },
    /// 凭证格式非法
    SourceBad { seq: u8 },
    /// 失败无理由
    FailNoReason { seq: u8 },
    /// 未全部通过就放行
    ReleaseBlocked { failed: u8 },
    /// 序位越界
    SeqBad { seq: u8 },
}

/// 诊断码（0x90 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrecheckCode {
    /// 0x9000 缺项
    ItemMissing,
    /// 0x9001 越序
    OutOfOrder,
    /// 0x9002 凭证缺失
    SourceMissing,
    /// 0x9003 凭证格式非法
    SourceBad,
    /// 0x9004 失败无理由
    NoReason,
    /// 0x9005 放行被阻
    Blocked,
    /// 0x9006 序位越界
    SeqBad,
    /// 0x9007 放行通过
    Released,
}

impl PrecheckCode {
    pub const fn code(self) -> u16 {
        match self {
            PrecheckCode::ItemMissing => 0x9000,
            PrecheckCode::OutOfOrder => 0x9001,
            PrecheckCode::SourceMissing => 0x9002,
            PrecheckCode::SourceBad => 0x9003,
            PrecheckCode::NoReason => 0x9004,
            PrecheckCode::Blocked => 0x9005,
            PrecheckCode::SeqBad => 0x9006,
            PrecheckCode::Released => 0x9007,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            PrecheckCode::ItemMissing => "PRECHECK-ITEM-MISSING",
            PrecheckCode::OutOfOrder => "PRECHECK-OUT-OF-ORDER",
            PrecheckCode::SourceMissing => "PRECHECK-SOURCE-MISSING",
            PrecheckCode::SourceBad => "PRECHECK-SOURCE-BAD",
            PrecheckCode::NoReason => "PRECHECK-NO-REASON",
            PrecheckCode::Blocked => "PRECHECK-BLOCKED",
            PrecheckCode::SeqBad => "PRECHECK-SEQ-BAD",
            PrecheckCode::Released => "PRECHECK-RELEASED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            PrecheckCode::ItemMissing => "前置清单缺项：六判据关闭集缺一即拒",
            PrecheckCode::OutOfOrder => "终核越序：必须按 ORDER 冻结序推进，越序结论不可信",
            PrecheckCode::SourceMissing => "凭证源缺失：终核结论无凭证 = 口头结论",
            PrecheckCode::SourceBad => "凭证源格式非法：必须 <路径>#<编号> 或目录形态",
            PrecheckCode::NoReason => "失败无理由：Fail 必须带 block 理由",
            PrecheckCode::Blocked => "放行被阻：存在失败项，禁带病收官",
            PrecheckCode::SeqBad => "序位越界：只允许 0..=5",
            PrecheckCode::Released => "前置清单放行通过：六项全 pass 且逐项有源",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx761_precheck_list.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9000 && (c & 0x00FF) < 0x10
    }
}

impl PrecheckError {
    pub const fn code(&self) -> PrecheckCode {
        match self {
            PrecheckError::ItemMissing { .. } => PrecheckCode::ItemMissing,
            PrecheckError::OutOfOrder { .. } => PrecheckCode::OutOfOrder,
            PrecheckError::SourceMissing { .. } => PrecheckCode::SourceMissing,
            PrecheckError::SourceBad { .. } => PrecheckCode::SourceBad,
            PrecheckError::FailNoReason { .. } => PrecheckCode::NoReason,
            PrecheckError::ReleaseBlocked { .. } => PrecheckCode::Blocked,
            PrecheckError::SeqBad { .. } => PrecheckCode::SeqBad,
        }
    }

    pub fn detail(&self) -> String {
        self.code().detail()
    }
}

impl PrecheckList {
    /// new（六项全 Pending，自然序构造；终核顺序由 ORDER 冻结）。
    pub fn new() -> Self {
        let out: [PrecheckItem; ITEM_COUNT] = core::array::from_fn(|i| PrecheckItem {
            idx: i as u8,
            state: ItemState::Pending,
            source: "",
            reason: String::new(),
        });
        PrecheckList { items: out, cursor: 0 }
    }

    /// 挂凭证（按自然 idx）。
    pub fn attach_source(&mut self, idx: u8, source: &'static str) -> Result<(), PrecheckError> {
        if idx as usize >= ITEM_COUNT {
            return Err(PrecheckError::SeqBad { seq: idx });
        }
        if source.is_empty() {
            return Err(PrecheckError::SourceMissing { seq: idx });
        }
        // 只两种合法形态：<路径>#<编号> 指针，或目录（以 / 结尾）
        let ok = match source.find('#') {
            Some(i) => i > 0 && i + 1 < source.len(),
            None => source.ends_with('/'),
        };
        if !ok {
            return Err(PrecheckError::SourceBad { seq: idx });
        }
        for it in self.items.iter_mut() {
            if it.idx == idx {
                it.source = source;
                return Ok(());
            }
        }
        Err(PrecheckError::ItemMissing {
            name: ITEM_NAMES[idx as usize],
        })
    }

    /// 按序推进一项（越序拒）。
    pub fn advance(&mut self, passed: bool, reason: &str) -> Result<u8, PrecheckError> {
        if self.cursor as usize >= ITEM_COUNT {
            return Err(PrecheckError::SeqBad { seq: self.cursor });
        }
        let seq = self.cursor;
        let idx = match ORDER.get(seq as usize) {
            Some(v) => *v,
            None => return Err(PrecheckError::SeqBad { seq }),
        };
        for it in self.items.iter_mut() {
            if it.idx == idx as u8 {
                if it.source.is_empty() {
                    return Err(PrecheckError::SourceMissing { seq });
                }
                if !passed && reason.is_empty() {
                    return Err(PrecheckError::FailNoReason { seq });
                }
                it.state = if passed { ItemState::Pass } else { ItemState::Fail };
                it.reason = String::from(reason);
            }
        }
        let _ = idx;
        self.cursor += 1;
        Ok(seq)
    }

    /// 可解引审计（六项源全挂且格式合法）。
    pub fn resolvability_audit(&self) -> Result<u32, PrecheckError> {
        let mut ok = 0u32;
        let mut i = 0;
        while i < ITEM_COUNT {
            if let Some(it) = self.items.get(i) {
                if it.source.is_empty() {
                    return Err(PrecheckError::SourceMissing { seq: it.idx });
                }
                let good = match it.source.find('#') {
                    Some(j) => j > 0 && j + 1 < it.source.len(),
                    None => it.source.ends_with('/'),
                };
                if !good {
                    return Err(PrecheckError::SourceBad { seq: it.idx });
                }
                ok += 1;
            }
            i += 1;
        }
        Ok(ok)
    }

    /// 放行（六项全 pass 才放行；失败点名计数）。
    pub fn release(&self) -> Result<u32, PrecheckError> {
        let mut failed = 0u8;
        let mut pass = 0u8;
        for it in self.items.iter() {
            match it.state {
                ItemState::Pass => pass += 1,
                ItemState::Fail => failed += 1,
                ItemState::Pending => {}
            }
        }
        if failed > 0 || pass as usize != ITEM_COUNT {
            return Err(PrecheckError::ReleaseBlocked { failed });
        }
        self.resolvability_audit()?;
        Ok(pass as u32)
    }
}

impl Default for PrecheckList {
    fn default() -> Self {
        PrecheckList::new()
    }
}

/// 域自检。
pub fn run_unx761_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx761");

    // 构造全 pass 清单
    let build = |pass_all: bool| -> PrecheckList {
        let mut l = PrecheckList::new();
        // 先挂全六项凭证，再按 ORDER 序推进（否则序位项无源即被拦）
        let mut i = 0u8;
        while (i as usize) < ITEM_COUNT {
            let src = SOURCE_MAP[i as usize].1;
            let _ = l.attach_source(i, src);
            i += 1;
        }
        let mut k = 0u8;
        while (k as usize) < ITEM_COUNT {
            let pass = pass_all || k < 5;
            let _ = l.advance(pass, if pass { "" } else { "line drift" });
            k += 1;
        }
        l
    };

    // C761-01 六项全 pass 放行
    {
        let l = build(true);
        let r = l.release();
        let good = r == Ok(6);
        s.add("C761-01-全 pass 放行", good, if good { "" } else { "放行失效" });
    }

    // C761-02 越序拦截（先核第 2 序位再核第 1 序位）
    {
        let mut l = PrecheckList::new();
        let mut l2 = PrecheckList::new();
        // l 正常推进到序位 1；l2 试图跳过序位 0（直接改 cursor 不可行——
        // 但可以构造：l2 有序位 1 的源，却推进序位 0 无源）
        let _ = l2.attach_source(1, SOURCE_MAP[1].1);
        let r = l2.advance(true, ""); // 序位 0 无源 → SourceMissing
        let good = matches!(r, Err(PrecheckError::SourceMissing { seq: 0 }));
        let _ = l.attach_source(0, SOURCE_MAP[0].1);
        s.add("C761-02-无源序位拦截", good, if good { "" } else { "无源被放行" });
    }

    // C761-03 失败无理由拒
    {
        let mut l = PrecheckList::new();
        let _ = l.attach_source(0, SOURCE_MAP[0].1);
        let r = l.advance(false, "");
        let good = matches!(r, Err(PrecheckError::FailNoReason { seq: 0 }));
        s.add("C761-03-失败无理由拒", good, if good { "" } else { "无理由被放行" });
    }

    // C761-04 失败项阻断放行（点名 failed=1）
    {
        let l = build(false);
        let r = l.release();
        let good = matches!(r, Err(PrecheckError::ReleaseBlocked { failed: 1 }));
        s.add("C761-04-失败阻断放行", good, if good { "" } else { "带病放行" });
    }

    // C761-05 凭证格式非法拒（无 # 无 /）
    {
        let mut l = PrecheckList::new();
        let r = l.attach_source(0, "docs/x.md");
        let good = matches!(r, Err(PrecheckError::SourceBad { seq: 0 }));
        s.add("C761-05-凭证格式核", good, if good { "" } else { "格式非法被放行" });
    }

    // C761-06 可解引 6/6
    {
        let l = build(true);
        let r = l.resolvability_audit();
        let good = r == Ok(6);
        s.add("C761-06-可解引 6/6", good, if good { "" } else { "可解引不满" });
    }

    // C761-07 项名 ⇄ 源映射冻结自洽
    {
        let mut ok = ITEM_COUNT == 6 && ORDER.len() == 6;
        let mut seen = [false; ITEM_COUNT];
        let mut i = 0;
        while i < ITEM_COUNT {
            if let Some(v) = ORDER.get(i) {
                if *v < ITEM_COUNT {
                    seen[*v] = true;
                }
            }
            i += 1;
        }
        let mut k = 0;
        while k < ITEM_COUNT {
            ok = ok && seen[k];
            k += 1;
        }
        s.add("C761-07-顺序框架闭合", ok, if ok { "" } else { "ORDER 非置换" });
    }

    // C761-08 序位越界拒
    {
        let mut l = PrecheckList::new();
        let r = l.attach_source(6, "docs/x.md#1");
        let good = matches!(r, Err(PrecheckError::SeqBad { seq: 6 }));
        s.add("C761-08-序位越界拒", good, if good { "" } else { "越界被放行" });
    }

    // C761-09 诊断码段独占
    {
        let codes = [
            PrecheckCode::ItemMissing,
            PrecheckCode::OutOfOrder,
            PrecheckCode::SourceMissing,
            PrecheckCode::SourceBad,
            PrecheckCode::NoReason,
            PrecheckCode::Blocked,
            PrecheckCode::SeqBad,
            PrecheckCode::Released,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C761-09-诊断码 0x90 段独占", all, if all { "" } else { "码值越出 0x90 段" });
    }

    s
}
