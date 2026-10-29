//! cgb864_f04f05handover —— CGPU-F0864 F04 组收口与 F05 移交。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0864 F04 组收口与 F05 移交  <SEG 0x7680>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0864；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（F04 收口 → F05 移交，整数账面）：
//!   1. F04 收口检查项闭集七项（写死）：账本守恒 / 错误码闭集齐
//!      备 / 禁词零命中 / 基准全跑 / 一致性出章 / 安全放行 / 回归
//!      矩阵出报告；七项全绿才出收口章；
//!   2. 移交清单状态机：Draft → Frozen → Handed；Handed 只读；
//!   3. 移交条目账（定容 28 槽）守恒：移交数 = 已接数 + 退回数；
//!      漂移红码（XC_HANDOVER_DRIFT）；
//!   4. 条目类别闭集六类：代码 / 账本 / 文档 / 基准 / 工具 / 报告；
//!      类别越界红码（XC_BAD_CATEGORY）；
//!   5. 冻结前置：七项全绿且无退回未接条目；违例红码
//!      （XC_FREEZE_BLOCKED）；
//!   6. F05 接收面：接收方逐条签收（签名去重），未签收条目不许
//!      终移交（XC_NOT_SIGNED）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7680 段为本模块独占（SEG_BASE=0x7680），判别式
//!   SEG_BASE+1..+N；与 0x7660/0x76A0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const H2_VERSION: &str = "cgb864_f04f05handover-v1";

/// 诊断段基址（0x7680 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7680;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7681 检查项号越界（0..=6 之外）。
pub const XC_BAD_CHECK: u16 = SEG_BASE + 1;
/// 0x7682 条目类别越界（0..=5 之外）。
pub const XC_BAD_CATEGORY: u16 = SEG_BASE + 2;
/// 0x7683 移交账漂移（移交 ≠ 已接 + 退回）。
pub const XC_HANDOVER_DRIFT: u16 = SEG_BASE + 3;
/// 0x7684 清单封存仍写。
pub const XC_HANDOVER_SEALED: u16 = SEG_BASE + 4;
/// 0x7685 冻结前置不满足。
pub const XC_FREEZE_BLOCKED: u16 = SEG_BASE + 5;
/// 0x7686 条目号越界。
pub const XC_BAD_ENTRY: u16 = SEG_BASE + 6;
/// 0x7687 重复条目名。
pub const XC_DUP_ENTRY: u16 = SEG_BASE + 7;
/// 0x7688 未冻结即移交。
pub const XC_NOT_FROZEN: u16 = SEG_BASE + 8;
/// 0x7689 条目账满（28）。
pub const XC_ENTRY_FULL: u16 = SEG_BASE + 9;
/// 0x768A 退回未带原因。
pub const XC_NO_REASON: u16 = SEG_BASE + 10;
/// 0x768B 未签收条目残存即终移交。
pub const XC_NOT_SIGNED: u16 = SEG_BASE + 11;
/// 0x768C 重复签收。
pub const XC_DUP_SIGN: u16 = SEG_BASE + 12;

/// 收口检查项数（写死：7）。
pub const CHECK_CAP: usize = 7;
/// 移交账容量（写死：28）。
pub const ENTRY_CAP: usize = 28;
/// 类别闭集上限。
pub const CAT_CAP: u8 = 5;

/// 检查项闭集：0=账本守恒 1=错误码齐备 2=禁词零命中 3=基准全跑
/// 4=一致性出章 5=安全放行 6=回归出报告。
pub const C_BAL: usize = 0;
pub const C_XC: usize = 1;
pub const C_FORBID: usize = 2;
pub const C_BENCH: usize = 3;
pub const C_CONSIST: usize = 4;
pub const C_SEC: usize = 5;
pub const C_REGRESS: usize = 6;

/// 类别闭集：0=代码 1=账本 2=文档 3=基准 4=工具 5=报告。
pub const K_CODE: u8 = 0;
pub const K_LEDGER: u8 = 1;
pub const K_DOC: u8 = 2;
pub const K_BENCH: u8 = 3;
pub const K_TOOL: u8 = 4;
pub const K_REPORT: u8 = 5;

/// 移交清单状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HPhase {
    /// 草拟。
    Draft,
    /// 冻结。
    Frozen,
    /// 已移交。
    Handed,
}

/// 单条移交条目。
#[derive(Clone, Debug)]
pub struct Entry {
    /// 条目名（去重键）。
    pub name: String,
    /// 类别。
    pub kind: u8,
    /// F05 签收名（空 = 未签收）。
    pub signed_by: u8,
    /// 已派标记。
    pub dispatched: bool,
    /// 退回标记。
    pub bounced: bool,
    /// 退回原因长度。
    pub reason_len: u32,
}

/// F04 收口与 F05 移交册。
pub struct Handover2 {
    /// 清单状态。
    pub phase: HPhase,
    /// 七项检查板。
    pub checks: [bool; CHECK_CAP],
    /// 条目账。
    pub entries: Vec<Entry>,
    /// 已接数。
    pub accepted: u64,
    /// 退回数。
    pub bounced: u64,
    /// 派发数。
    pub dispatched: u64,
    /// 签收数。
    pub signed: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl Handover2 {
    /// 开册。
    pub fn new() -> Self {
        Handover2 {
            phase: HPhase::Draft,
            checks: [false; CHECK_CAP],
            entries: Vec::new(),
            accepted: 0,
            bounced: 0,
            dispatched: 0,
            signed: 0,
            red_count: 0,
        }
    }

    /// 设检查项。
    pub fn set_check(&mut self, item: usize, ok: bool) -> Result<(), u16> {
        if item >= CHECK_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHECK);
        }
        self.checks[item] = ok;
        Ok(())
    }

    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        let mut i = 0;
        while i < CHECK_CAP {
            if !self.checks[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 立条目（Draft 期）。
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
            signed_by: 0,
            dispatched: false,
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

    /// 冻结（七项全绿前置）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.phase != HPhase::Draft {
            self.red_count += 1;
            return Err(XC_HANDOVER_SEALED);
        }
        if !self.all_green() {
            self.red_count += 1;
            return Err(XC_FREEZE_BLOCKED);
        }
        self.phase = HPhase::Frozen;
        Ok(())
    }

    /// 派发（Frozen 期）。
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
        if self.entries[idx].dispatched && !self.entries[idx].bounced {
            return Err(XC_DUP_ENTRY);
        }
        self.entries[idx].dispatched = true;
        self.entries[idx].bounced = false;
        self.dispatched += 1;
        self.accepted += 1;
        Ok(())
    }

    /// 退回（带原因）。
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
        if !self.entries[idx].dispatched || self.entries[idx].bounced {
            return Err(XC_BAD_ENTRY);
        }
        self.entries[idx].bounced = true;
        self.entries[idx].reason_len = reason_len;
        self.accepted -= 1;
        self.bounced += 1;
        Ok(())
    }

    /// F05 签收（接收方代号 1..=255；去重）。
    pub fn sign(&mut self, idx: usize, who: u8) -> Result<(), u16> {
        if self.phase != HPhase::Frozen {
            self.red_count += 1;
            return Err(XC_HANDOVER_SEALED);
        }
        if idx >= self.entries.len() {
            self.red_count += 1;
            return Err(XC_BAD_ENTRY);
        }
        if who == 0 {
            self.red_count += 1;
            return Err(XC_BAD_ENTRY);
        }
        if self.entries[idx].signed_by != 0 {
            self.red_count += 1;
            return Err(XC_DUP_SIGN);
        }
        if !self.entries[idx].dispatched || self.entries[idx].bounced {
            return Err(XC_NOT_SIGNED);
        }
        self.entries[idx].signed_by = who;
        self.signed += 1;
        Ok(())
    }

    /// 移交账守恒。
    pub fn balanced(&self) -> bool {
        self.dispatched == self.accepted + self.bounced
    }

    /// 终移交（全条目已派且已签）。
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
            if !self.entries[i].dispatched || self.entries[i].bounced {
                self.red_count += 1;
                return Err(XC_NOT_SIGNED);
            }
            if self.entries[i].signed_by == 0 {
                self.red_count += 1;
                return Err(XC_NOT_SIGNED);
            }
            i += 1;
        }
        self.phase = HPhase::Handed;
        Ok(self.entries.len() as u32)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("f04->f05 phase=");
        s += match self.phase {
            HPhase::Draft => "draft",
            HPhase::Frozen => "frozen",
            HPhase::Handed => "handed",
        };
        s += " entries=";
        s += &self.entries.len().to_string();
        s += " signed=";
        s += &self.signed.to_string();
        s += " acc=";
        s += &self.accepted.to_string();
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_HANDOVER_DRIFT);
        }
        let mut acc = 0u64;
        let mut bnc = 0u64;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].dispatched && !self.entries[i].bounced {
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

/// 自例程：七项全绿 → 冻结 → 派发/退回 → 签收 → 终移交 + 负例。
pub fn drill() -> Result<u32, u16> {
    let mut h = Handover2::new();
    // 负例：未全绿冻结。
    if h.freeze() != Err(XC_FREEZE_BLOCKED) {
        return Err(XC_FREEZE_BLOCKED);
    }
    let mut c = 0;
    while c < CHECK_CAP {
        h.set_check(c, true)?;
        c += 1;
    }
    if h.set_check(CHECK_CAP, true) != Err(XC_BAD_CHECK) {
        return Err(XC_BAD_CHECK);
    }
    h.push("cgb860_amdbench", K_CODE)?;
    h.push("cgb861_amdconsist", K_CODE)?;
    h.push("cgb862_amdsec", K_CODE)?;
    h.push("cgb863_amdregress", K_CODE)?;
    h.push("regress-report", K_REPORT)?;
    if h.push("regress-report", K_DOC) != Err(XC_DUP_ENTRY) {
        return Err(XC_DUP_ENTRY);
    }
    if h.push("bad", 9) != Err(XC_BAD_CATEGORY) {
        return Err(XC_BAD_CATEGORY);
    }
    h.freeze()?;
    if h.push("late", K_TOOL) != Err(XC_HANDOVER_SEALED) {
        return Err(XC_HANDOVER_SEALED);
    }
    // 派发 4、退回 1 再派。
    let mut i = 0;
    while i < 5 {
        h.dispatch(i)?;
        i += 1;
    }
    h.bounce(4, 21)?;
    // 负例：空原因退回（先再派）。
    h.dispatch(4)?;
    if h.bounce(4, 0) != Err(XC_NO_REASON) {
        return Err(XC_NO_REASON);
    }
    // 负例：未签收终移交。
    if h.complete() != Err(XC_NOT_SIGNED) {
        return Err(XC_NOT_SIGNED);
    }
    // 签收 0..4。
    let mut i2 = 0;
    while i2 < 5 {
        h.sign(i2, 7)?;
        i2 += 1;
    }
    // 负例：重复签收。
    if h.sign(0, 7) != Err(XC_DUP_SIGN) {
        return Err(XC_DUP_SIGN);
    }
    h.audit()?;
    let n = h.complete()?;
    // 负例：Handed 后派发。
    if h.dispatch(0) != Err(XC_HANDOVER_SEALED) {
        return Err(XC_HANDOVER_SEALED);
    }
    h.audit()?;
    // 签收数守恒：signed = 5。
    if n != 5 || h.signed != 5 {
        return Err(XC_HANDOVER_DRIFT);
    }
    if h.summary().len() == 0 {
        return Err(XC_HANDOVER_DRIFT);
    }
    Ok(0)
}
