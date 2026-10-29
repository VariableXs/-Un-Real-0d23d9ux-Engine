//! cgb848_f03f04handover —— CGPU-F0848 F03 组收口与 F04 移交。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0848 F03 组收口与 F04 移交  <SEG 0x7480>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0848；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（F03 收口 → F04 移交，整数账面）：
//!   1. 收口检查项闭集六项（写死）：账本守恒 / 错误码闭集齐备 /
//!      禁词零命中 / 基准全跑 / 缺陷清账 / 文档落册；每项布尔判定，
//!      六项全绿才可出收口章；
//!   2. 移交清单状态机：Draft → Frozen → Handed；Handed 后清单
//!      只读（XC_HANDOVER_SEALED）；
//!   3. 移交条目账（定容 24 槽）守恒：移交数 = 已接数 + 退回数；
//!      漂移红码（XC_HANDOVER_DRIFT）；
//!   4. 条目类别闭集：代码 / 账本 / 文档 / 基准 / 工具；类别越界
//!      红码（XC_BAD_CATEGORY）；
//!   5. 冻结前置：Draft 期禁冻结缺项（任一检查红即拒），
//!      冻结前置不满足红码（XC_FREEZE_BLOCKED）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7480 段为本模块独占（SEG_BASE=0x7480），判别式
//!   SEG_BASE+1..+N；与 0x7460/0x74A0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const HO_VERSION: &str = "cgb848_f03f04handover-v1";

/// 诊断段基址（0x7480 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7480;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7481 检查项号越界（0..=5 之外）。
pub const XC_BAD_CHECK: u16 = SEG_BASE + 1;
/// 0x7482 移交条目类别越界（0..=4 之外）。
pub const XC_BAD_CATEGORY: u16 = SEG_BASE + 2;
/// 0x7483 移交账漂移（移交 ≠ 已接 + 退回）。
pub const XC_HANDOVER_DRIFT: u16 = SEG_BASE + 3;
/// 0x7484 清单已封存仍写入。
pub const XC_HANDOVER_SEALED: u16 = SEG_BASE + 4;
/// 0x7485 冻结前置不满足（收口检查有红）。
pub const XC_FREEZE_BLOCKED: u16 = SEG_BASE + 5;
/// 0x7486 条目号越界（账外）。
pub const XC_BAD_ENTRY: u16 = SEG_BASE + 6;
/// 0x7487 重复条目名。
pub const XC_DUP_ENTRY: u16 = SEG_BASE + 7;
/// 0x7488 未冻结即移交。
pub const XC_NOT_FROZEN: u16 = SEG_BASE + 8;
/// 0x7489 条目账满（24 槽）。
pub const XC_ENTRY_FULL: u16 = SEG_BASE + 9;
/// 0x7490 退回条目未带原因。
pub const XC_NO_REASON: u16 = SEG_BASE + 10;

/// 收口检查项数（写死：6）。
pub const CHECK_CAP: usize = 6;
/// 移交账容量（写死：24）。
pub const ENTRY_CAP: usize = 24;
/// 条目类别闭集上限。
pub const CAT_CAP: u8 = 4;

/// 收口检查项闭集：0=账本守恒 1=错误码齐备 2=禁词零命中
/// 3=基准全跑 4=缺陷清账 5=文档落册。
pub const C_LEDGER_BAL: usize = 0;
pub const C_XCCODE_SET: usize = 1;
pub const C_FORBID_CLEAN: usize = 2;
pub const C_BENCH_ALL: usize = 3;
pub const C_DEFECT_ZERO: usize = 4;
pub const C_DOC_SEALED: usize = 5;

/// 条目类别闭集：0=代码 1=账本 2=文档 3=基准 4=工具。
pub const K_CODE: u8 = 0;
pub const K_LEDGER: u8 = 1;
pub const K_DOC: u8 = 2;
pub const K_BENCH: u8 = 3;
pub const K_TOOL: u8 = 4;

/// 移交清单状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HPhase {
    /// 草拟期（可增改条目）。
    Draft,
    /// 已冻结（只读）。
    Frozen,
    /// 已移交（封存）。
    Handed,
}

/// 单条移交条目。
#[derive(Clone, Debug)]
pub struct Entry {
    /// 条目名（去重键）。
    pub name: String,
    /// 类别（0..=4）。
    pub kind: u8,
    /// 已接标记。
    pub accepted: bool,
    /// 退回标记。
    pub bounced: bool,
    /// 退回原因长度（0 = 未退回）。
    pub reason_len: u32,
}

/// 收口判定板（六项布尔）。
#[derive(Clone, Copy, Debug)]
pub struct Checklist {
    /// 六项绿/红。
    pub pass: [bool; CHECK_CAP],
}

impl Checklist {
    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        let mut i = 0;
        while i < CHECK_CAP {
            if !self.pass[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 绿项计数。
    pub fn green_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < CHECK_CAP {
            if self.pass[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// F03 收口与 F04 移交册。
pub struct Handover {
    /// 清单状态。
    pub phase: HPhase,
    /// 收口检查板。
    pub checks: Checklist,
    /// 移交条目账。
    pub entries: Vec<Entry>,
    /// 已接数。
    pub accepted: u64,
    /// 退回数。
    pub bounced: u64,
    /// 累计移交数。
    pub dispatched: u64,
    /// 累计红码次数。
    pub red_count: u64,
}

impl Handover {
    /// 开新移交册（检查板初始全红）。
    pub fn new() -> Self {
        Handover {
            phase: HPhase::Draft,
            checks: Checklist {
                pass: [false; CHECK_CAP],
            },
            entries: Vec::new(),
            accepted: 0,
            bounced: 0,
            dispatched: 0,
            red_count: 0,
        }
    }

    /// 设置收口检查项（越界红）。
    pub fn set_check(&mut self, item: usize, ok: bool) -> Result<(), u16> {
        if item >= CHECK_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHECK);
        }
        self.checks.pass[item] = ok;
        Ok(())
    }

    /// 追加移交条目（Draft 期、类别闭集内、名不重复、账未满）。
    pub fn push(&mut self, name: &str, kind: u8) -> Result<u32, u16> {
        if self.phase != HPhase::Draft {
            self.red_count += 1;
            return Err(XC_HANDOVER_SEALED);
        }
        if kind > CAT_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CATEGORY);
        }
        if self.has_name(name) {
            self.red_count += 1;
            return Err(XC_DUP_ENTRY);
        }
        if self.entries.len() == ENTRY_CAP {
            self.red_count += 1;
            return Err(XC_ENTRY_FULL);
        }
        self.entries.push(Entry {
            name: String::from(name),
            kind,
            accepted: false,
            bounced: false,
            reason_len: 0,
        });
        Ok(self.entries.len() as u32)
    }

    /// 名查重。
    pub fn has_name(&self, name: &str) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 冻结清单（前置：六项全绿）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.phase != HPhase::Draft {
            self.red_count += 1;
            return Err(XC_HANDOVER_SEALED);
        }
        if !self.checks.all_green() {
            self.red_count += 1;
            return Err(XC_FREEZE_BLOCKED);
        }
        self.phase = HPhase::Frozen;
        Ok(())
    }

    /// 移交单条（Frozen 期才可派发；Dispatched 计数入账）。
    pub fn dispatch(&mut self, idx: usize) -> Result<(), u16> {
        if self.phase != HPhase::Frozen {
            self.red_count += 1;
            if self.phase == HPhase::Handed {
                return Err(XC_HANDOVER_SEALED);
            }
            return Err(XC_NOT_FROZEN);
        }
        if idx >= self.entries.len() {
            self.red_count += 1;
            return Err(XC_BAD_ENTRY);
        }
        if self.entries[idx].accepted || self.entries[idx].bounced {
            return Err(XC_DUP_ENTRY);
        }
        self.entries[idx].accepted = true;
        self.accepted += 1;
        self.dispatched += 1;
        Ok(())
    }

    /// 接收方退回（带原因长度；未派发条目不可退）。
    pub fn bounce(&mut self, idx: usize, reason_len: u32) -> Result<(), u16> {
        if self.phase != HPhase::Frozen {
            self.red_count += 1;
            return Err(XC_HANDOVER_SEALED);
        }
        if idx >= self.entries.len() {
            self.red_count += 1;
            return Err(XC_BAD_ENTRY);
        }
        if reason_len == 0 {
            self.red_count += 1;
            return Err(XC_NO_REASON);
        }
        if !self.entries[idx].accepted {
            return Err(XC_BAD_ENTRY);
        }
        self.entries[idx].accepted = false;
        self.entries[idx].bounced = true;
        self.entries[idx].reason_len = reason_len;
        self.accepted -= 1;
        self.bounced += 1;
        Ok(())
    }

    /// 移交账守恒：dispatched = accepted + bounced。
    pub fn balanced(&self) -> bool {
        self.dispatched == self.accepted + self.bounced
    }

    /// 终移交：全条目已接才可 Handed。
    pub fn complete(&mut self) -> Result<u32, u16> {
        if self.phase != HPhase::Frozen {
            self.red_count += 1;
            if self.phase == HPhase::Handed {
                return Err(XC_HANDOVER_SEALED);
            }
            return Err(XC_NOT_FROZEN);
        }
        if !self.balanced() {
            self.red_count += 1;
            return Err(XC_HANDOVER_DRIFT);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if !self.entries[i].accepted {
                self.red_count += 1;
                return Err(XC_NOT_FROZEN);
            }
            i += 1;
        }
        self.phase = HPhase::Handed;
        Ok(self.entries.len() as u32)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("f03->f04 phase=");
        s += match self.phase {
            HPhase::Draft => "draft",
            HPhase::Frozen => "frozen",
            HPhase::Handed => "handed",
        };
        s += " green=";
        s += &self.checks.green_count().to_string();
        s += " entries=";
        s += &self.entries.len().to_string();
        s += " acc=";
        s += &self.accepted.to_string();
        s += " bnc=";
        s += &self.bounced.to_string();
        s
    }

    /// 全账自检。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_HANDOVER_DRIFT);
        }
        let mut acc = 0u64;
        let mut bnc = 0u64;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].accepted {
                acc += 1;
            }
            if self.entries[i].bounced {
                bnc += 1;
            }
            i += 1;
        }
        if acc != self.accepted || bnc != self.bounced {
            return Err(XC_HANDOVER_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：检查板全绿 → 冻结 → 派发 → 退回一次再派 → 终移交。
pub fn drill() -> Result<u32, u16> {
    let mut ho = Handover::new();
    // 先试冻结：应被前置挡下。
    if ho.freeze() != Err(XC_FREEZE_BLOCKED) {
        return Err(XC_FREEZE_BLOCKED);
    }
    let mut c = 0;
    while c < CHECK_CAP {
        ho.set_check(c, true)?;
        c += 1;
    }
    // 负例：检查项越界。
    if ho.set_check(CHECK_CAP, true) != Err(XC_BAD_CHECK) {
        return Err(XC_BAD_CHECK);
    }
    ho.push("cgb846_intelbench", K_CODE)?;
    ho.push("cgb847_inteldefect", K_CODE)?;
    ho.push("bench-ledger", K_LEDGER)?;
    // 负例：重复名。
    if ho.push("bench-ledger", K_DOC) != Err(XC_DUP_ENTRY) {
        return Err(XC_DUP_ENTRY);
    }
    // 负例：类别越界。
    if ho.push("bad-kind", 9) != Err(XC_BAD_CATEGORY) {
        return Err(XC_BAD_CATEGORY);
    }
    ho.freeze()?;
    // 负例：冻结后追加。
    if ho.push("late-item", K_TOOL) != Err(XC_HANDOVER_SEALED) {
        return Err(XC_HANDOVER_SEALED);
    }
    ho.dispatch(0)?;
    ho.dispatch(1)?;
    ho.dispatch(2)?;
    ho.bounce(2, 12)?;
    // 负例：空原因退回。
    ho.dispatch(2)?;
    if ho.bounce(2, 0) != Err(XC_NO_REASON) {
        return Err(XC_NO_REASON);
    }
    ho.audit()?;
    // 负例：退回未接条目。
    if ho.bounce(2, 5) != Err(XC_BAD_ENTRY) {
        return Err(XC_BAD_ENTRY);
    }
    ho.dispatch(2)?;
    let n = ho.complete()?;
    // 负例：Handed 后再派发。
    if ho.dispatch(0) != Err(XC_HANDOVER_SEALED) {
        return Err(XC_HANDOVER_SEALED);
    }
    ho.audit()?;
    if ho.summary().len() == 0 || n != 3 {
        return Err(XC_HANDOVER_DRIFT);
    }
    Ok(0)
}
