//! UNX-F0719 · 断言集移交声明（下游复用登记）· 诊断段 0x59e0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0719] 断言集移交声明（下游复用登记） <SEG 0x59e0>
//!   UNX-F0719-J1 移交声明三段齐（集清单/复用口径/补测态）且引用批号
//!   可解引 100%，移交包挂载断言过。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三段闭集：HandoverSec 恰 3 变体（集清单/复用口径/补测态），
//!      段外无声明对象；三段全齐才允许签发移交声明。
//!   2. 引用批号可解引 100%：声明内每个批号引用必须能在批账解引
//!      （悬空引用即拒签）；可解引核对逐引用全查。
//!   3. 移交包挂载：断言集挂载入移交包（包指纹折叠）；挂载后包只读
//!      （E_PKG_RDONLY），断言挂载核对逐条全过才可签发。
//!   4. 补测态闭集：RetestState 4 臂（未需/已过/待补/豁免），臂外无态。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x59e0~0x59fF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x59e0 独占 0x59e0~0x59fF） ────────────────────────────
pub const SEG_BASE: u16 = 0x59e0;

/// 0x59e1 三段缺：声明段 3/3 未齐即签发
const E_SEC_MISSING: u16 = SEG_BASE + 0x1;
/// 0x59e2 包已签发仍尝试挂载
const E_PKG_RDONLY: u16 = SEG_BASE + 0x2;
/// 0x59e3 批号引用悬空：批账解引失败
const E_REF_DANGLE: u16 = SEG_BASE + 0x4;
/// 0x59e4 批号越域（合法 1..=MAX_BATCHES）
const E_BATCH_RANGE: u16 = SEG_BASE + 0x5;
/// 0x59e5 补测态越臂
const E_RETEST_RANGE: u16 = SEG_BASE + 0x6;
/// 0x59e6 断言挂载败：挂载核对未全过
const E_MOUNT_FAIL: u16 = SEG_BASE + 0x7;
/// 0x59e7 断言重号：包内同名二次挂载
const E_MOUNT_DUP: u16 = SEG_BASE + 0x8;
/// 0x59e8 段号越界（合法 0..3）
const E_SEC_RANGE: u16 = SEG_BASE + 0x9;
/// 0x59e9 包溢出：挂载断言超定容
const E_PKG_FULL: u16 = SEG_BASE + 0xA;
/// 0x59ea 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x59eb 未签发即解引用途
const E_NOT_SIGNED: u16 = SEG_BASE + 0xC;
/// 0x59ec 签发哈希漂移：复算与落账不符
const E_SIGN_DRIFT: u16 = SEG_BASE + 0xD;
/// 0x59ed 集清单空：零断言即签发
const E_LIST_EMPTY: u16 = SEG_BASE + 0xE;
/// 0x59e3 复用口径未定（口径臂缺）
const E_CALIBER_OPEN: u16 = SEG_BASE + 0x3;

/// 诊断码 → 中文可操作诊断（段 0x59e0~0x59fF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEC_MISSING => "三段缺：集清单/复用口径/补测态缺段",
        E_PKG_RDONLY => "包已签发：挂载面只读，变更走新包",
        E_REF_DANGLE => "批号引用悬空：批账解引失败",
        E_BATCH_RANGE => "批号越域：合法 1..=MAX_BATCHES",
        E_RETEST_RANGE => "补测态越臂：合法 0..4",
        E_MOUNT_FAIL => "断言挂载败：挂载核对未全过",
        E_MOUNT_DUP => "断言重号：包内同名二次挂载拒收",
        E_SEC_RANGE => "段号越界：三段闭集合法 0..3",
        E_PKG_FULL => "包溢出：挂载断言超定容",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_NOT_SIGNED => "未签发即解引：先 sign 再取包指纹",
        E_SIGN_DRIFT => "签发哈希漂移：复算与落账不符",
        E_LIST_EMPTY => "集清单空：零断言无签发资格",
        E_CALIBER_OPEN => "复用口径未定：先定口径臂再签发",
        _ => "未知诊断码（0x59e0 段外或保留位）",
    }
}

// ── 常量（来源：移交口径与定容） ─────────────────────────────────────
/// 声明段数（判据原文：三段齐）。
pub const SEC_COUNT: usize = 3;
/// 批号上限。
pub const MAX_BATCHES: u16 = 64;
/// 包定容。
pub const MAX_PACKAGE: usize = 256;
/// 复用口径臂数。
pub const CALIBER_ARMS: u8 = 4;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(SEC_COUNT == 3);

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

// ── 三段闭集 ─────────────────────────────────────────────────────────
/// 移交声明三段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoverSec {
    /// 段 0：集清单。
    Inventory,
    /// 段 1：复用口径。
    Caliber,
    /// 段 2：补测态。
    Retest,
}

impl HandoverSec {
    /// 段号。
    pub const fn id(self) -> usize {
        match self {
            HandoverSec::Inventory => 0,
            HandoverSec::Caliber => 1,
            HandoverSec::Retest => 2,
        }
    }

    /// 按段号重建。
    pub fn from_id(v: usize) -> Result<HandoverSec, u16> {
        match v {
            0 => Ok(HandoverSec::Inventory),
            1 => Ok(HandoverSec::Caliber),
            2 => Ok(HandoverSec::Retest),
            _ => Err(E_SEC_RANGE),
        }
    }
}

// ── 复用口径臂闭集 ───────────────────────────────────────────────────
/// 复用口径四臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReuseCaliber {
    /// 全量复用。
    Full,
    /// 只复用批级。
    BatchOnly,
    /// 只复用域补全。
    DomainOnly,
    /// 不复用（仅存档）。
    ArchiveOnly,
}

impl ReuseCaliber {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            ReuseCaliber::Full => 0,
            ReuseCaliber::BatchOnly => 1,
            ReuseCaliber::DomainOnly => 2,
            ReuseCaliber::ArchiveOnly => 3,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<ReuseCaliber, u16> {
        match v {
            0 => Ok(ReuseCaliber::Full),
            1 => Ok(ReuseCaliber::BatchOnly),
            2 => Ok(ReuseCaliber::DomainOnly),
            3 => Ok(ReuseCaliber::ArchiveOnly),
            _ => Err(E_RETEST_RANGE),
        }
    }
}

// ── 补测态闭集 ───────────────────────────────────────────────────────
/// 补测态四臂闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetestState {
    /// 未需补测。
    NotNeeded,
    /// 补测已过。
    Passed,
    /// 待补测。
    Pending,
    /// 豁免。
    Waived,
}

impl RetestState {
    /// 臂号。
    pub const fn id(self) -> u8 {
        match self {
            RetestState::NotNeeded => 0,
            RetestState::Passed => 1,
            RetestState::Pending => 2,
            RetestState::Waived => 3,
        }
    }

    /// 按臂号重建。
    pub fn from_id(v: u8) -> Result<RetestState, u16> {
        match v {
            0 => Ok(RetestState::NotNeeded),
            1 => Ok(RetestState::Passed),
            2 => Ok(RetestState::Pending),
            3 => Ok(RetestState::Waived),
            _ => Err(E_RETEST_RANGE),
        }
    }
}

// ── 批账（引用解引目标） ─────────────────────────────────────────────
/// 批账：批号 → 批指纹（引用可解引性核对目标）。
pub struct BatchRegistry {
    /// 批号 → 指纹槽。
    slots: [Option<u64>; 64],
}

impl BatchRegistry {
    /// 空批账。
    pub fn new() -> Self {
        BatchRegistry {
            slots: [None; 64],
        }
    }

    /// 开批。
    pub fn open(&mut self, batch: u16, fp: u64) -> Result<(), u16> {
        if batch < 1 || batch > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        if self.slots[(batch - 1) as usize].is_some() {
            return Err(E_MOUNT_DUP);
        }
        self.slots[(batch - 1) as usize] = Some(fp);
        Ok(())
    }

    /// 批号解引。
    pub fn deref(&self, batch: u16) -> Result<u64, u16> {
        if batch < 1 || batch > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        match self.slots[(batch - 1) as usize] {
            Some(fp) => Ok(fp),
            None => Err(E_REF_DANGLE),
        }
    }

    /// 已开批数。
    pub fn open_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 64 {
            if self.slots[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ── 移交包 ───────────────────────────────────────────────────────────
/// 移交包：断言挂载 + 三段声明 + 批号引用 + 签发。
pub struct HandoverPackage {
    /// 包名。
    pub pkg_name: &'static str,
    /// 挂载断言（名哈希 + 源批号）。
    asserts: Vec<(u64, u16)>,
    /// 三段落账位。
    sec_done: [bool; SEC_COUNT],
    /// 复用口径臂。
    caliber: Option<ReuseCaliber>,
    /// 补测态臂。
    retest: Option<RetestState>,
    /// 批号引用账。
    refs: Vec<u16>,
    /// 签发位。
    signed: bool,
    /// 签发哈希。
    sign_fp: u64,
}

impl HandoverPackage {
    /// 新包。
    pub fn new(name: &'static str) -> Self {
        HandoverPackage {
            pkg_name: name,
            asserts: Vec::new(),
            sec_done: [false; SEC_COUNT],
            caliber: None,
            retest: None,
            refs: Vec::new(),
            signed: false,
            sign_fp: 0,
        }
    }

    /// 挂载断言（签发前）。
    pub fn mount(&mut self, name: &str, batch: u16) -> Result<(), u16> {
        if self.signed {
            return Err(E_PKG_RDONLY);
        }
        if self.asserts.len() >= MAX_PACKAGE {
            return Err(E_PKG_FULL);
        }
        if name.is_empty() || name.len() > 64 {
            return Err(E_MOUNT_FAIL);
        }
        if batch < 1 || batch > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].0 == fp {
                return Err(E_MOUNT_DUP);
            }
            i += 1;
        }
        self.asserts.push((fp, batch));
        Ok(())
    }

    /// 挂载数。
    pub fn mount_count(&self) -> usize {
        self.asserts.len()
    }

    /// 落段：三段声明逐段落位。
    pub fn put_sec(&mut self, sec: HandoverSec) -> Result<(), u16> {
        if self.signed {
            return Err(E_PKG_RDONLY);
        }
        if self.sec_done[sec.id()] {
            return Err(E_MOUNT_DUP);
        }
        self.sec_done[sec.id()] = true;
        Ok(())
    }

    /// 定复用口径臂。
    pub fn set_caliber(&mut self, arm: ReuseCaliber) -> Result<(), u16> {
        if self.signed {
            return Err(E_PKG_RDONLY);
        }
        if self.caliber.is_some() {
            return Err(E_MOUNT_DUP);
        }
        self.caliber = Some(arm);
        Ok(())
    }

    /// 定补测态臂。
    pub fn set_retest(&mut self, arm: RetestState) -> Result<(), u16> {
        if self.signed {
            return Err(E_PKG_RDONLY);
        }
        if self.retest.is_some() {
            return Err(E_MOUNT_DUP);
        }
        self.retest = Some(arm);
        Ok(())
    }

    /// 挂批号引用（签发前登记，签发时统一解引核对）。
    pub fn add_ref(&mut self, batch: u16) -> Result<(), u16> {
        if self.signed {
            return Err(E_PKG_RDONLY);
        }
        if batch < 1 || batch > MAX_BATCHES {
            return Err(E_BATCH_RANGE);
        }
        let mut i = 0usize;
        while i < self.refs.len() {
            if self.refs[i] == batch {
                return Err(E_MOUNT_DUP);
            }
            i += 1;
        }
        self.refs.push(batch);
        Ok(())
    }

    /// 引用数。
    pub fn ref_count(&self) -> usize {
        self.refs.len()
    }

    /// 三段齐检。
    pub fn audit_secs(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < SEC_COUNT {
            if !self.sec_done[i] {
                return Err(E_SEC_MISSING);
            }
            i += 1;
        }
        if self.caliber.is_none() {
            return Err(E_CALIBER_OPEN);
        }
        if self.retest.is_none() {
            return Err(E_RETEST_RANGE);
        }
        Ok(())
    }

    /// 签发：三段齐 + 清单非空 + 批号引用全可解引（100%）+ 挂载核对
    /// （挂载断言源批号全可解引）→ 签发哈希落账。
    pub fn sign(&mut self, batches: &BatchRegistry) -> Result<u64, u16> {
        if self.signed {
            return Err(E_PKG_RDONLY);
        }
        self.audit_secs()?;
        if self.asserts.is_empty() {
            return Err(E_LIST_EMPTY);
        }
        // 批号引用可解引 100%。
        let mut i = 0usize;
        while i < self.refs.len() {
            batches.deref(self.refs[i])?;
            i += 1;
        }
        // 挂载断言源批号全可解引（挂载核对）。
        let mut j = 0usize;
        while j < self.asserts.len() {
            batches.deref(self.asserts[j].1)?;
            j += 1;
        }
        self.sign_fp = self.compute_sign_fp();
        self.signed = true;
        Ok(self.sign_fp)
    }

    /// 签发哈希（挂载 + 口径 + 补测 + 引用折叠）。
    fn compute_sign_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = self.asserts[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((a.0 >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, (a.1 & 0xFF) as u8 | 0x10);
            i += 1;
        }
        if let Some(c) = self.caliber {
            h = fnv_mix(h, c.id() | 0x20);
        }
        if let Some(r) = self.retest {
            h = fnv_mix(h, r.id() | 0x30);
        }
        let mut k = 0usize;
        while k < self.refs.len() {
            h = fnv_mix(h, (self.refs[k] & 0xFF) as u8 | 0x40);
            k += 1;
        }
        let mut s = 0usize;
        while s < SEC_COUNT {
            h = fnv_mix(h, self.sec_done[s] as u8 | 0x50);
            s += 1;
        }
        h
    }

    /// 签发核验（漂移检出）。
    pub fn verify_sign(&self) -> Result<(), u16> {
        if !self.signed {
            return Err(E_NOT_SIGNED);
        }
        if self.compute_sign_fp() != self.sign_fp {
            return Err(E_SIGN_DRIFT);
        }
        Ok(())
    }

    /// 签发哈希只读。
    pub fn sign_fp(&self) -> Result<u64, u16> {
        if !self.signed {
            return Err(E_NOT_SIGNED);
        }
        Ok(self.sign_fp)
    }

    /// 三段落账位只读。
    pub const fn sec_flags(&self) -> [bool; SEC_COUNT] {
        self.sec_done
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.asserts.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("移交声明: ");
        out.push_str(self.pkg_name);
        out.push_str(" 挂载=");
        out.push_str(&itoa(self.asserts.len() as u64));
        out.push_str(" 引用=");
        out.push_str(&itoa(self.refs.len() as u64));
        out.push_str(" 签发=");
        out.push_str(&itoa(self.signed as u64));
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

fn hname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("hv_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 批账：开 3 批 ——
    let mut batches = BatchRegistry::new();
    batches.open(1, 0xAA01)?;
    batches.open(2, 0xAA02)?;
    batches.open(3, 0xAA03)?;
    if batches.open_count() != 3 {
        return Err(E_MOUNT_FAIL);
    }
    // 批解引抽查。
    if batches.deref(1)? != 0xAA01 {
        return Err(E_REF_DANGLE);
    }
    if batches.deref(4) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }
    if batches.deref(0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }

    // —— 主路径：挂载 12 断言（3 批）→ 三段齐 → 引用 2 条 → 签发 ——
    let mut pkg = HandoverPackage::new("pkg_a1_b35");
    let mut i = 0usize;
    while i < 12 {
        let batch = ((i / 4) as u16) + 1;
        pkg.mount(&hname(i), batch)?;
        i += 1;
    }
    pkg.put_sec(HandoverSec::Inventory)?;
    pkg.put_sec(HandoverSec::Caliber)?;
    pkg.put_sec(HandoverSec::Retest)?;
    pkg.set_caliber(ReuseCaliber::Full)?;
    pkg.set_retest(RetestState::Passed)?;
    pkg.add_ref(1)?;
    pkg.add_ref(2)?;
    // 三段齐前签发拦（未签发态核对：段齐才可签）。
    pkg.audit_secs()?;
    let fp = pkg.sign(&batches)?;
    if fp == 0 {
        return Err(E_SIGN_DRIFT);
    }
    // 签发核验过。
    pkg.verify_sign()?;
    // 签发后挂载必拦。
    if pkg.mount("late", 1) != Err(E_PKG_RDONLY) {
        return Err(E_PKG_RDONLY);
    }
    if pkg.put_sec(HandoverSec::Inventory) != Err(E_PKG_RDONLY) {
        return Err(E_PKG_RDONLY);
    }
    if pkg.sign_fp()? != fp {
        return Err(E_SIGN_DRIFT);
    }
    let sm = pkg.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 引用悬空拒签：引用未开批号 9 ——
    let mut pkg2 = HandoverPackage::new("pkg_bad_ref");
    pkg2.mount(&hname(100), 1)?;
    pkg2.put_sec(HandoverSec::Inventory)?;
    pkg2.put_sec(HandoverSec::Caliber)?;
    pkg2.put_sec(HandoverSec::Retest)?;
    pkg2.set_caliber(ReuseCaliber::BatchOnly)?;
    pkg2.set_retest(RetestState::Pending)?;
    pkg2.add_ref(9)?;
    if pkg2.sign(&batches) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // —— 挂载核对拒签：断言源批号未开批 ——
    let mut pkg3 = HandoverPackage::new("pkg_bad_mount");
    pkg3.mount(&hname(200), 7)?;
    pkg3.put_sec(HandoverSec::Inventory)?;
    pkg3.put_sec(HandoverSec::Caliber)?;
    pkg3.put_sec(HandoverSec::Retest)?;
    pkg3.set_caliber(ReuseCaliber::DomainOnly)?;
    pkg3.set_retest(RetestState::Waived)?;
    if pkg3.sign(&batches) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // —— 三段缺拒签 ——
    let mut pkg4 = HandoverPackage::new("pkg_missing_sec");
    pkg4.mount(&hname(300), 1)?;
    pkg4.put_sec(HandoverSec::Inventory)?;
    pkg4.put_sec(HandoverSec::Caliber)?;
    pkg4.set_caliber(ReuseCaliber::Full)?;
    pkg4.set_retest(RetestState::NotNeeded)?;
    if pkg4.sign(&batches) != Err(E_SEC_MISSING) {
        return Err(E_SEC_MISSING);
    }
    // 口径缺拒签。
    let mut pkg5 = HandoverPackage::new("pkg_no_caliber");
    pkg5.mount(&hname(400), 1)?;
    pkg5.put_sec(HandoverSec::Inventory)?;
    pkg5.put_sec(HandoverSec::Caliber)?;
    pkg5.put_sec(HandoverSec::Retest)?;
    pkg5.set_retest(RetestState::Passed)?;
    if pkg5.sign(&batches) != Err(E_CALIBER_OPEN) {
        return Err(E_CALIBER_OPEN);
    }

    // —— 签发哈希漂移注入 ——
    let mut pkg6 = HandoverPackage::new("pkg_drift");
    pkg6.mount(&hname(500), 1)?;
    pkg6.put_sec(HandoverSec::Inventory)?;
    pkg6.put_sec(HandoverSec::Caliber)?;
    pkg6.put_sec(HandoverSec::Retest)?;
    pkg6.set_caliber(ReuseCaliber::ArchiveOnly)?;
    pkg6.set_retest(RetestState::Waived)?;
    pkg6.sign(&batches)?;
    pkg6.refs.push(3);
    if pkg6.verify_sign() != Err(E_SIGN_DRIFT) {
        return Err(E_SIGN_DRIFT);
    }

    // —— 错误路径样本 ——
    if HandoverSec::from_id(3) != Err(E_SEC_RANGE) {
        return Err(E_SEC_RANGE);
    }
    if ReuseCaliber::from_id(4) != Err(E_RETEST_RANGE) {
        return Err(E_RETEST_RANGE);
    }
    if RetestState::from_id(4) != Err(E_RETEST_RANGE) {
        return Err(E_RETEST_RANGE);
    }
    let mut err = HandoverPackage::new("pkg_err");
    if err.mount("", 1) != Err(E_MOUNT_FAIL) {
        return Err(E_MOUNT_FAIL);
    }
    if err.mount("x", 0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.mount("x", MAX_BATCHES + 1) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.add_ref(0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    err.mount("dup", 1)?;
    if err.mount("dup", 1) != Err(E_MOUNT_DUP) {
        return Err(E_MOUNT_DUP);
    }
    err.put_sec(HandoverSec::Inventory)?;
    if err.put_sec(HandoverSec::Inventory) != Err(E_MOUNT_DUP) {
        return Err(E_MOUNT_DUP);
    }
    err.set_caliber(ReuseCaliber::Full)?;
    if err.set_caliber(ReuseCaliber::Full) != Err(E_MOUNT_DUP) {
        return Err(E_MOUNT_DUP);
    }
    // 未签发即解引。
    if err.sign_fp() != Err(E_NOT_SIGNED) {
        return Err(E_NOT_SIGNED);
    }
    if err.verify_sign() != Err(E_NOT_SIGNED) {
        return Err(E_NOT_SIGNED);
    }
    // 清单空签发拦。
    err.put_sec(HandoverSec::Caliber)?;
    err.put_sec(HandoverSec::Retest)?;
    err.set_retest(RetestState::Passed)?;
    if err.sign(&batches) != Err(E_LIST_EMPTY) {
        return Err(E_LIST_EMPTY);
    }
    // 摘要空包拦。
    let empty = HandoverPackage::new("pkg_empty");
    if empty.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_REF_DANGLE) == "未知诊断码（0x59e0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x59e0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
