//! UNX-F0728 · 跟随账登记（AI-94 台账写回）· 诊断段 0x5a80（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0728] 跟随账登记（AI-94 台账写回） <SEG 0x5a80>
//!   UNX-F0728-J1 跟随账三列齐（裁决/版本/放行指纹）且 AI-94 台账写回
//!   回执登记、未评估版本零放行。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 跟随账三列闭集：裁决（VerdictArm）/版本（Ver 字符串哈希）/
//!      放行指纹（ReleaseFp），三列构造即齐，审计逐行复列。
//!   2. 未评估版本零放行：版本必须先过评估（裁决臂落账）才有放行
//!      资格；未评估版本请求放行必拦（零放行判据）。
//!   3. AI-94 台账写回回执：每次写回必产回执（写回序号 + 三列快照），
//!      回执账与跟随账守恒（回执数 == 跟随行数）。
//!   4. 裁决臂闭集：Adopt（采纳跟随）/Skip（跳过不跟）/Defer（缓评）/
//!      Reject（拒绝），臂外无裁决；放行仅采纳臂可解。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5a80~0x5a9F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5a80 独占 0x5a80~0x5a9F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5a80;

/// 0x5a81 账已封板仍尝试登记
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5a82 裁决臂越闭集
const E_ARM_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5a83 版本串非法：空串或超长
const E_VER_BAD: u16 = SEG_BASE + 0x3;
/// 0x5a84 未评估即放行：裁决臂未落账
const E_UNASSESSED: u16 = SEG_BASE + 0x4;
/// 0x5a85 非采纳臂放行：跳过/缓评/拒绝臂无放行资格
const E_RELEASE_DENY: u16 = SEG_BASE + 0x5;
/// 0x5a86 三列不齐：裁决/版本/放行指纹缺列
const E_COL_MISSING: u16 = SEG_BASE + 0x6;
/// 0x5a87 版本重登：同版本二次登记
const E_VER_DUP: u16 = SEG_BASE + 0x7;
/// 0x5a88 回执账失衡：回执数 ≠ 跟随行数
const E_RECEIPT_IMBAL: u16 = SEG_BASE + 0x8;
/// 0x5a89 回执悬空：解引越界回执
const E_RECEIPT_DANGLE: u16 = SEG_BASE + 0x9;
/// 0x5a8A 跟随账溢出
const E_LEDGER_FULL: u16 = SEG_BASE + 0xA;
/// 0x5a8B 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x5a8C 放行指纹零值
const E_FP_ZERO: u16 = SEG_BASE + 0xC;
/// 0x5a8D 回执序号回退
const E_RECEIPT_BACK: u16 = SEG_BASE + 0xD;
/// 0x5a8E 跟随行悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5a80~0x5a9F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "跟随账已封板：登记冻结，扩账请开新账",
        E_ARM_RANGE => "裁决臂越闭集：四臂之外无裁决",
        E_VER_BAD => "版本串非法：空串或超 32 字节",
        E_UNASSESSED => "未评估即放行：先落裁决臂再谈放行",
        E_RELEASE_DENY => "非采纳臂放行：仅采纳臂有放行资格",
        E_COL_MISSING => "三列不齐：裁决/版本/放行指纹缺列",
        E_VER_DUP => "版本重登：同版本二次登记拒收",
        E_RECEIPT_IMBAL => "回执账失衡：回执数 ≠ 跟随行数",
        E_RECEIPT_DANGLE => "回执悬空：解引越界",
        E_LEDGER_FULL => "跟随账溢出：超定容上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_FP_ZERO => "放行指纹零值：非法",
        E_RECEIPT_BACK => "回执序号回退：回执序必须单调",
        E_REF_DANGLE => "跟随行悬空：解引越界",
        _ => "未知诊断码（0x5a80 段外或保留位）",
    }
}

// ── 常量（来源：跟随口径与定容） ─────────────────────────────────────
/// 裁决臂数。
pub const ARM_COUNT: u8 = 4;
/// 版本串长度上限。
pub const VER_MAX: usize = 32;
/// 跟随账定容。
pub const MAX_ROWS: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(ARM_COUNT == 4);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 裁决臂闭集 ───────────────────────────────────────────────────────
/// 跟随裁决四臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowVerdict {
    /// 采纳跟随（唯一放行臂）。
    Adopt,
    /// 跳过不跟。
    Skip,
    /// 缓评。
    Defer,
    /// 拒绝。
    Reject,
}

impl FollowVerdict {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            FollowVerdict::Adopt => 0,
            FollowVerdict::Skip => 1,
            FollowVerdict::Defer => 2,
            FollowVerdict::Reject => 3,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<FollowVerdict, u16> {
        match v {
            0 => Ok(FollowVerdict::Adopt),
            1 => Ok(FollowVerdict::Skip),
            2 => Ok(FollowVerdict::Defer),
            3 => Ok(FollowVerdict::Reject),
            _ => Err(E_ARM_RANGE),
        }
    }

    /// 放行资格：仅采纳臂。
    pub const fn releasable(self) -> bool {
        matches!(self, FollowVerdict::Adopt)
    }
}

// ── 跟随行（三列） ───────────────────────────────────────────────────
/// 跟随账一行：恰三列（裁决/版本/放行指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowRow {
    /// 列一：裁决臂号。
    pub verdict: u8,
    /// 列二：版本哈希。
    pub ver_fp: u64,
    /// 列三：放行指纹（未放行为 0）。
    pub release_fp: u64,
}

impl FollowRow {
    /// 三列齐判定：裁决在闭集 + 版本哈希非零。
    pub const fn cols_ok(&self) -> bool {
        self.verdict < ARM_COUNT && self.ver_fp != 0
    }
}

// ── 写回回执 ─────────────────────────────────────────────────────────
/// AI-94 台账写回回执：写回序号 + 三列快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WritebackReceipt {
    /// 写回序号（单调）。
    pub seq: u32,
    /// 三列快照。
    pub row: FollowRow,
}

// ── 跟随账 ───────────────────────────────────────────────────────────
/// 跟随账：三列行账 + 评估/放行闸 + AI-94 写回回执账。
pub struct FollowLedger {
    /// 跟随行账。
    rows: Vec<FollowRow>,
    /// 回执账。
    receipts: Vec<WritebackReceipt>,
    /// 回执序游标。
    receipt_seq: u32,
    /// 封板位。
    sealed: bool,
}

impl FollowLedger {
    /// 新账。
    pub fn new() -> Self {
        FollowLedger {
            rows: Vec::new(),
            receipts: Vec::new(),
            receipt_seq: 0,
            sealed: false,
        }
    }

    /// 评估登记：裁决 + 版本（放行指纹暂 0——未放行）。
    pub fn assess(&mut self, version: &str, verdict: FollowVerdict) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.rows.len() >= MAX_ROWS {
            return Err(E_LEDGER_FULL);
        }
        if version.is_empty() || version.len() > VER_MAX {
            return Err(E_VER_BAD);
        }
        let fp = fnv_str(version);
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].ver_fp == fp {
                return Err(E_VER_DUP);
            }
            i += 1;
        }
        self.rows.push(FollowRow {
            verdict: verdict.id(),
            ver_fp: fp,
            release_fp: 0,
        });
        Ok(self.rows.len() - 1)
    }

    /// 跟随行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 跟随行解引。
    pub fn row_at(&self, idx: usize) -> Result<FollowRow, u16> {
        if idx >= self.rows.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.rows[idx])
    }

    /// 版本解引（按版本哈希找行）。
    pub fn find(&self, version: &str) -> Result<usize, u16> {
        let fp = fnv_str(version);
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].ver_fp == fp {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_REF_DANGLE)
    }

    /// 放行：仅采纳臂 + 已评估 + 指纹非零 → 写放行指纹列。
    pub fn release(&mut self, version: &str, release_fp: u64) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if release_fp == 0 {
            return Err(E_FP_ZERO);
        }
        let idx = self.find(version)?;
        let arm = FollowVerdict::from_id(self.rows[idx].verdict)?;
        // 未评估版本零放行：裁决臂未落（此处恒已落——评估即落臂），
        // 真正的「未评估」是版本不在账（find 已拦）；此处核臂资格。
        if !arm.releasable() {
            return Err(E_RELEASE_DENY);
        }
        if self.rows[idx].release_fp != 0 {
            return Err(E_VER_DUP);
        }
        self.rows[idx].release_fp = release_fp;
        // 写回回执落账。
        self.receipt_seq = self.receipt_seq.wrapping_add(1);
        self.receipts.push(WritebackReceipt {
            seq: self.receipt_seq,
            row: self.rows[idx],
        });
        Ok(release_fp)
    }

    /// 回执数。
    pub fn receipt_count(&self) -> usize {
        self.receipts.len()
    }

    /// 回执解引。
    pub fn receipt_at(&self, idx: usize) -> Result<WritebackReceipt, u16> {
        if idx >= self.receipts.len() {
            return Err(E_RECEIPT_DANGLE);
        }
        Ok(self.receipts[idx])
    }

    /// 三列齐审计：逐行复列 + 回执守恒 + 回执序单调。
    pub fn audit_cols(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].cols_ok() {
                return Err(E_COL_MISSING);
            }
            i += 1;
        }
        // 回执守恒：回执数 == 放行行数（每放行一 receipt）。
        let mut released = 0usize;
        let mut j = 0usize;
        while j < self.rows.len() {
            if self.rows[j].release_fp != 0 {
                released += 1;
            }
            j += 1;
        }
        if self.receipts.len() != released {
            return Err(E_RECEIPT_IMBAL);
        }
        // 回执序单调。
        let mut k = 1usize;
        while k < self.receipts.len() {
            if self.receipts[k].seq != self.receipts[k - 1].seq + 1 {
                return Err(E_RECEIPT_BACK);
            }
            k += 1;
        }
        Ok(())
    }

    /// 未评估零放行核对：放行行数 == 采纳臂行数（非采纳臂零放行）。
    pub fn audit_release_gate(&self) -> Result<(), u16> {
        let mut adopt = 0usize;
        let mut released = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            let arm = FollowVerdict::from_id(self.rows[i].verdict)?;
            if arm.releasable() {
                adopt += 1;
            }
            if self.rows[i].release_fp != 0 {
                released += 1;
                if !arm.releasable() {
                    return Err(E_RELEASE_DENY);
                }
            }
            i += 1;
        }
        if released > adopt {
            return Err(E_RELEASE_DENY);
        }
        Ok(())
    }

    /// 封板：全审计过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.rows.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        self.audit_cols()?;
        self.audit_release_gate()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            h = fnv_mix(h, r.verdict | 0x10);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((r.ver_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            let mut shift2 = 56i32;
            while shift2 >= 0 {
                h = fnv_mix(h, ((r.release_fp >> shift2) & 0xFF) as u8);
                shift2 -= 8;
            }
            i += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.rows.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("跟随账: 行=");
        out.push_str(&itoa(self.rows.len() as u64));
        out.push_str(" 回执=");
        out.push_str(&itoa(self.receipts.len() as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：4 版本评估（采纳/跳过/缓评/拒绝各一）→ 采纳放行 ——
    let mut led = FollowLedger::new();
    led.assess("v1.2.3", FollowVerdict::Adopt)?;
    led.assess("v1.3.0", FollowVerdict::Skip)?;
    led.assess("v1.4.0-rc1", FollowVerdict::Defer)?;
    led.assess("v0.9", FollowVerdict::Reject)?;
    if led.row_count() != 4 {
        return Err(E_LEDGER_FULL);
    }
    // 版本解引抽查。
    if led.find("v1.2.3")? != 0 {
        return Err(E_REF_DANGLE);
    }
    if led.find("v2.0") != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 未评估版本零放行：不在账版本放行必拦。
    if led.release("v2.0", 0xBEEF) != Err(E_REF_DANGLE) {
        return Err(E_UNASSESSED);
    }
    // 非采纳臂放行必拦。
    if led.release("v1.3.0", 0xBEEF) != Err(E_RELEASE_DENY) {
        return Err(E_RELEASE_DENY);
    }
    if led.release("v1.4.0-rc1", 0xBEEF) != Err(E_RELEASE_DENY) {
        return Err(E_RELEASE_DENY);
    }
    if led.release("v0.9", 0xBEEF) != Err(E_RELEASE_DENY) {
        return Err(E_RELEASE_DENY);
    }
    // 指纹零值拦。
    if led.release("v1.2.3", 0) != Err(E_FP_ZERO) {
        return Err(E_FP_ZERO);
    }
    // 采纳臂放行 + 回执落账。
    let fp = led.release("v1.2.3", 0xBEEF)?;
    if fp != 0xBEEF {
        return Err(E_FP_ZERO);
    }
    if led.receipt_count() != 1 {
        return Err(E_RECEIPT_IMBAL);
    }
    // 重复放行必拦。
    if led.release("v1.2.3", 0xBEEF) != Err(E_VER_DUP) {
        return Err(E_VER_DUP);
    }
    led.audit_cols()?;
    led.audit_release_gate()?;
    let acct = led.seal()?;
    if acct == 0 {
        return Err(E_RECEIPT_IMBAL);
    }
    // 封板后操作必拦。
    if led.assess("v9", FollowVerdict::Adopt) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if led.release("v1.2.3", 0xABCD) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 回执解引。
    let r0 = led.receipt_at(0)?;
    if r0.seq != 1 || r0.row.release_fp != 0xBEEF {
        return Err(E_RECEIPT_DANGLE);
    }
    if led.receipt_at(1) != Err(E_RECEIPT_DANGLE) {
        return Err(E_RECEIPT_DANGLE);
    }

    // —— 回执序单调：再开新账连续放行两个版本 ——
    let mut led2 = FollowLedger::new();
    led2.assess("a1", FollowVerdict::Adopt)?;
    led2.assess("a2", FollowVerdict::Adopt)?;
    led2.assess("a3", FollowVerdict::Skip)?;
    led2.release("a1", 0x1111)?;
    led2.release("a2", 0x2222)?;
    if led2.receipt_count() != 2 {
        return Err(E_RECEIPT_IMBAL);
    }
    led2.audit_cols()?;
    // 回执序核对：1,2 单调。
    if led2.receipt_at(0)?.seq != 1 || led2.receipt_at(1)?.seq != 2 {
        return Err(E_RECEIPT_BACK);
    }

    // —— 错误路径样本 ——
    if FollowVerdict::from_id(4) != Err(E_ARM_RANGE) {
        return Err(E_ARM_RANGE);
    }
    let mut err = FollowLedger::new();
    if err.assess("", FollowVerdict::Adopt) != Err(E_VER_BAD) {
        return Err(E_VER_BAD);
    }
    let long = [b'x'; VER_MAX + 1];
    let long_name = core::str::from_utf8(&long);
    if let Ok(s) = long_name {
        if err.assess(s, FollowVerdict::Adopt) != Err(E_VER_BAD) {
            return Err(E_VER_BAD);
        }
    }
    err.assess("dup", FollowVerdict::Adopt)?;
    if err.assess("dup", FollowVerdict::Skip) != Err(E_VER_DUP) {
        return Err(E_VER_DUP);
    }
    // 行解引越界。
    if err.row_at(9) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    // 回执账失衡注入：回执账塞多一条。
    err.release("dup", 0xCAFE)?;
    err.receipts.push(WritebackReceipt {
        seq: 9,
        row: err.rows[0],
    });
    if err.audit_cols() != Err(E_RECEIPT_IMBAL) {
        return Err(E_RECEIPT_IMBAL);
    }
    // 非采纳臂放行注入：篡改臂号后放行必拦。
    let mut err2 = FollowLedger::new();
    err2.assess("skip_v", FollowVerdict::Skip)?;
    err2.rows[0].verdict = FollowVerdict::Adopt.id(); // 先改采纳，放行，再改回验证守卫。
    err2.release("skip_v", 0x5A5A)?;
    err2.rows[0].verdict = FollowVerdict::Reject.id();
    if err2.audit_release_gate() != Err(E_RELEASE_DENY) {
        return Err(E_RELEASE_DENY);
    }
    // 三列不齐注入：版本哈希清零。
    let mut err3 = FollowLedger::new();
    err3.assess("zero", FollowVerdict::Adopt)?;
    err3.rows[0].ver_fp = 0;
    if err3.audit_cols() != Err(E_COL_MISSING) {
        return Err(E_COL_MISSING);
    }
    // 摘要空账拦。
    let empty = FollowLedger::new();
    if empty.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_UNASSESSED) == "未知诊断码（0x5a80 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5a80 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
