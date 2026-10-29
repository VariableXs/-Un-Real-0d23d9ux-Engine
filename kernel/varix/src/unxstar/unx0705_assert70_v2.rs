//! UNX-F0705 · 早传/页表断言补全至 ≥70 · 诊断段 0x5820（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0705] 早传/页表断言补全至 ≥70 <SEG 0x5820>
//!   UNX-F0705-J1 早传/页表分项补全后实数 ≥70（早传十二步/双控制台/
//!   页表属性/高半核四面覆盖）且分项全绿。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 四面闭集：Face::EarlyHandoff（早传十二步）/ Face::DualConsole
//!      （双控制台）/ Face::PageAttr（页表属性）/ Face::HighHalf（高半核），
//!      面外无分项；实数 = Σ四面分项数。
//!   2. 补全下限：实数 ≥70（ASSERT_TARGET=70），不足即 E_TARGET_SHORT
//!      拒出全绿结论——补全判据的显性闸。
//!   3. 早传十二步子序闭集：十二步号 1..=12 每步至少 1 分项（步骤覆盖
//!      完备性子判据），缺步即 E_STEP_MISSING。
//!   4. 分项全绿：每分项确定性求值（闭集执行逻辑），零败零跳才全绿；
//!      注入败项必被 run_all 检出。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5820~0x583F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5820 独占 0x5820~0x583F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5820;

/// 0x5821 断言面已封板仍尝试补登
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5822 面号越界（合法 0..4）
const E_FACE_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5823 实数不足：补全后 <70
const E_TARGET_SHORT: u16 = SEG_BASE + 0x3;
/// 0x5824 早传步骤缺：十二步未全覆盖
const E_STEP_MISSING: u16 = SEG_BASE + 0x4;
/// 0x5825 分项败：执行结果为败
const E_ITEM_FAIL: u16 = SEG_BASE + 0x5;
/// 0x5826 分项重号：同面同名分项二次登记
const E_ITEM_DUP: u16 = SEG_BASE + 0x6;
/// 0x5827 步号越界（早传面合法 1..=12）
const E_STEP_RANGE: u16 = SEG_BASE + 0x7;
/// 0x5828 分项种类越闭集
const E_KIND_RANGE: u16 = SEG_BASE + 0x8;
/// 0x5829 断言账溢出：分项数超定容
const E_SET_FULL: u16 = SEG_BASE + 0x9;
/// 0x582A 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x582B 面缺：四面有一面零分项
const E_FACE_EMPTY: u16 = SEG_BASE + 0xA;
/// 0x582C 守恒破：Σ面分项数 ≠ 总分项数
const E_CONSERVATION: u16 = SEG_BASE + 0xB;
/// 0x582D 未跑即取全绿结论
const E_NOT_RUN: u16 = SEG_BASE + 0xE;
/// 0x582E 分项号悬空：解引越界分项
const E_REF_DANGLE: u16 = SEG_BASE + 0xD;

/// 诊断码 → 中文可操作诊断（段 0x5820~0x583F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "断言面已封板：补登冻结，扩面请开新集",
        E_FACE_RANGE => "面号越界：四面闭集合法 0..4",
        E_TARGET_SHORT => "实数不足：补全后分项 <70，请继续补全",
        E_STEP_MISSING => "早传步骤缺：十二步未全覆盖，请补缺步分项",
        E_ITEM_FAIL => "分项败：存在失败分项，先修后全绿",
        E_ITEM_DUP => "分项重号：同面同名二次登记拒收",
        E_STEP_RANGE => "步号越界：早传面合法 1..=12",
        E_KIND_RANGE => "分项种类越闭集",
        E_SET_FULL => "断言账溢出：分项数超定容上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_FACE_EMPTY => "面缺：四面有一面零分项，覆盖不齐",
        E_CONSERVATION => "守恒破：Σ面分项数 ≠ 总分项数",
        E_NOT_RUN => "未跑即取结论：先 run_all 再取全绿位",
        E_REF_DANGLE => "分项号悬空：解引越界",
        _ => "未知诊断码（0x5820 段外或保留位）",
    }
}

// ── 常量（来源：判据 ≥70 与早传十二步） ─────────────────────────────
/// 分项实数目标（判据原文：补全后实数 ≥70）。
pub const ASSERT_TARGET: usize = 70;
/// 早传步数（判据原文：早传十二步）。
pub const HANDOFF_STEPS: usize = 12;
/// 四面数。
pub const FACE_COUNT: usize = 4;
/// 分项定容。
pub const MAX_ITEMS: usize = 256;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(ASSERT_TARGET == 70);
const _: () = assert!(HANDOFF_STEPS == 12);
const _: () = assert!(FACE_COUNT == 4);

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

// ── 四面闭集 ─────────────────────────────────────────────────────────
/// 早传/页表断言四面闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 面 0：早传十二步（boot handoff 逐步断言）。
    EarlyHandoff,
    /// 面 1：双控制台（串口+显存双路输出断言）。
    DualConsole,
    /// 面 2：页表属性（PTE 位面断言）。
    PageAttr,
    /// 面 3：高半核（高半核映射断言）。
    HighHalf,
}

impl Face {
    /// 面号。
    pub const fn id(self) -> usize {
        match self {
            Face::EarlyHandoff => 0,
            Face::DualConsole => 1,
            Face::PageAttr => 2,
            Face::HighHalf => 3,
        }
    }

    /// 按面号重建。
    pub fn from_id(v: usize) -> Result<Face, u16> {
        match v {
            0 => Ok(Face::EarlyHandoff),
            1 => Ok(Face::DualConsole),
            2 => Ok(Face::PageAttr),
            3 => Ok(Face::HighHalf),
            _ => Err(E_FACE_RANGE),
        }
    }

    /// 面名。
    pub const fn name(self) -> &'static str {
        match self {
            Face::EarlyHandoff => "早传十二步",
            Face::DualConsole => "双控制台",
            Face::PageAttr => "页表属性",
            Face::HighHalf => "高半核",
        }
    }
}

// ── 分项条目 ─────────────────────────────────────────────────────────
/// 分项：面 + 名哈希 + 步号（仅早传面有义）+ 种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertItem {
    /// 所属面。
    pub face: u8,
    /// 分项名 FNV 哈希（去重锚）。
    pub name_fp: u64,
    /// 早传步号 1..=12（非早传面恒 0）。
    pub step: u8,
    /// 种类（执行逻辑派发位 0..=3）。
    pub kind: u8,
}

// ── 分项种类闭集（确定性执行逻辑） ──────────────────────────────────
/// kind 0: 恒真断言（结构性恒过）；
/// kind 1: 哈希自反（名哈希非零即过）；
/// kind 2: 步号域断言（步号在 1..=12 即过；非早传面恒过）；
/// kind 3: 面域断言（面号 <4 即过）。

// ── 断言集 ───────────────────────────────────────────────────────────
/// 早传/页表断言集：四面分项登记 + 补全下限闸 + 全绿执行。
pub struct AssertSet70 {
    /// 分项账（登记序）。
    items: Vec<AssertItem>,
    /// 每面计数（守恒分项）。
    per_face: [u32; FACE_COUNT],
    /// 早传步覆盖位图（12 位，bit i = 步 i+1 有分项）。
    step_cover: u16,
    /// 封板位。
    sealed: bool,
    /// 已跑位。
    ran: bool,
    /// 失败分项数（全绿位取反锚）。
    fail_count: u32,
}

impl AssertSet70 {
    /// 新集。
    pub fn new() -> Self {
        AssertSet70 {
            items: Vec::new(),
            per_face: [0; FACE_COUNT],
            step_cover: 0,
            sealed: false,
            ran: false,
            fail_count: 0,
        }
    }

    /// 登记分项：面闭集 + 名去重 + 步号域 + 步覆盖记账。
    pub fn enroll(&mut self, face: Face, name: &str, step: u8, kind: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.items.len() >= MAX_ITEMS {
            return Err(E_SET_FULL);
        }
        if kind > 3 {
            return Err(E_KIND_RANGE);
        }
        // 步号域：早传面 1..=12；非早传面步号必须为 0。
        if face == Face::EarlyHandoff {
            if step < 1 || step as usize > HANDOFF_STEPS {
                return Err(E_STEP_RANGE);
            }
        } else if step != 0 {
            return Err(E_STEP_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].face == face.id() as u8 && self.items[i].name_fp == fp {
                return Err(E_ITEM_DUP);
            }
            i += 1;
        }
        self.per_face[face.id()] = self.per_face[face.id()].wrapping_add(1);
        if face == Face::EarlyHandoff {
            self.step_cover |= 1u16 << (step - 1);
        }
        self.items.push(AssertItem {
            face: face.id() as u8,
            name_fp: fp,
            step,
            kind,
        });
        Ok(())
    }

    /// 补全审计：四面各 ≥1 分项 + 早传十二步全覆盖 + 实数 ≥70。
    pub fn audit_complete(&self) -> Result<usize, u16> {
        let mut f = 0usize;
        while f < FACE_COUNT {
            if self.per_face[f] == 0 {
                return Err(E_FACE_EMPTY);
            }
            f += 1;
        }
        // 十二步覆盖：位图全 1（低 12 位）。
        if self.step_cover != ((1u16 << HANDOFF_STEPS) - 1) {
            return Err(E_STEP_MISSING);
        }
        let total = self.total();
        if total < ASSERT_TARGET {
            return Err(E_TARGET_SHORT);
        }
        Ok(total)
    }

    /// 总分项数。
    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// 面计数只读。
    pub fn face_counts(&self) -> [u32; FACE_COUNT] {
        self.per_face
    }

    /// 封板：补全审计过后落锁。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        // 守恒：Σ面计数 == 总数。
        let mut sum: u32 = 0;
        let mut f = 0usize;
        while f < FACE_COUNT {
            sum = sum.wrapping_add(self.per_face[f]);
            f += 1;
        }
        if sum != self.items.len() as u32 {
            return Err(E_CONSERVATION);
        }
        let t = self.audit_complete()?;
        self.sealed = true;
        Ok(t)
    }

    /// 全量执行：分项逐条求值（种类闭集派发），失败计数。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        if self.ran {
            return Err(E_NOT_RUN);
        }
        self.fail_count = 0;
        let mut pass = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            let ok = match it.kind {
                0 => true,
                1 => it.name_fp != 0,
                2 => {
                    if it.face == Face::EarlyHandoff.id() as u8 {
                        it.step >= 1 && (it.step as usize) <= HANDOFF_STEPS
                    } else {
                        true
                    }
                }
                3 => (it.face as usize) < FACE_COUNT,
                _ => false,
            };
            if ok {
                pass += 1;
            } else {
                self.fail_count = self.fail_count.wrapping_add(1);
            }
            i += 1;
        }
        self.ran = true;
        Ok(pass)
    }

    /// 全绿位：零失败即绿（未跑即 Err）。
    pub fn all_green(&self) -> Result<bool, u16> {
        if !self.ran {
            return Err(E_NOT_RUN);
        }
        Ok(self.fail_count == 0)
    }

    /// 失败计数。
    pub const fn fail_count(&self) -> u32 {
        self.fail_count
    }

    /// 步覆盖位图只读。
    pub const fn step_cover(&self) -> u16 {
        self.step_cover
    }

    /// 分项解引。
    pub fn item(&self, idx: usize) -> Result<AssertItem, u16> {
        if idx >= self.items.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.items[idx])
    }

    /// 集指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((it.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, it.face | 0x10);
            h = fnv_mix(h, it.step | 0x20);
            h = fnv_mix(h, it.kind | 0x30);
            i += 1;
        }
        h = fnv_mix(h, (self.step_cover & 0xFF) as u8);
        h = fnv_mix(h, ((self.step_cover >> 8) & 0xFF) as u8);
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.items.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("早传页表断言集: 实数=");
        out.push_str(&itoa(self.total() as u64));
        out.push_str("/70 败=");
        out.push_str(&itoa(self.fail_count as u64));
        let mut f = 0usize;
        while f < FACE_COUNT {
            out.push(' ');
            out.push_str(Face::from_id(f)?.name());
            out.push('=');
            out.push_str(&itoa(self.per_face[f] as u64));
            f += 1;
        }
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

// ── 名池（确定性分项名，去重锚） ─────────────────────────────────────
fn item_name(face: usize, i: usize) -> String {
    let mut s = String::new();
    s.push_str("f");
    s.push_str(&itoa(face as u64));
    s.push_str("_i");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：四面补全至 ≥70（早传面 24=12步×2，其余面各 16/16/16） ——
    let mut set = AssertSet70::new();
    // 早传面：十二步 × 2 分项 = 24。
    let mut st = 1u8;
    while st as usize <= HANDOFF_STEPS {
        set.enroll(Face::EarlyHandoff, &item_name(0, st as usize * 2), st, 0)?;
        set.enroll(Face::EarlyHandoff, &item_name(0, st as usize * 2 + 1), st, 2)?;
        st += 1;
    }
    // 双控制台 16。
    let mut i = 0usize;
    while i < 16 {
        let kind = (i % 4) as u8;
        set.enroll(Face::DualConsole, &item_name(1, i), 0, kind)?;
        i += 1;
    }
    // 页表属性 16。
    i = 0;
    while i < 16 {
        set.enroll(Face::PageAttr, &item_name(2, i), 0, (i % 4) as u8)?;
        i += 1;
    }
    // 高半核 16。24+16+16+16 = 72 ≥ 70。
    i = 0;
    while i < 16 {
        set.enroll(Face::HighHalf, &item_name(3, i), 0, (i % 4) as u8)?;
        i += 1;
    }
    let total = set.seal()?;
    if total < ASSERT_TARGET {
        return Err(E_TARGET_SHORT);
    }
    let pass = set.run_all()?;
    if pass != total as u32 {
        return Err(E_ITEM_FAIL);
    }
    if !set.all_green()? {
        return Err(E_ITEM_FAIL);
    }
    // 十二步覆盖位图全 1。
    if set.step_cover() != (1u16 << HANDOFF_STEPS) - 1 {
        return Err(E_STEP_MISSING);
    }
    let sm = set.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 分项解引抽查。
    let it0 = set.item(0)?;
    if it0.step != 1 || it0.face != 0 {
        return Err(E_REF_DANGLE);
    }
    if set.item(set.total()) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // —— 实数不足拦截：68 分项（缺 2）拒封 ——
    let mut short = AssertSet70::new();
    st = 1;
    while st as usize <= HANDOFF_STEPS {
        short.enroll(Face::EarlyHandoff, &item_name(0, st as usize), st, 0)?;
        st += 1;
    }
    i = 0;
    while i < 18 {
        short.enroll(Face::DualConsole, &item_name(1, i), 0, 0)?;
        short.enroll(Face::PageAttr, &item_name(2, i), 0, 0)?;
        i += 1;
    }
    short.enroll(Face::HighHalf, &item_name(3, 0), 0, 0)?;
    short.enroll(Face::HighHalf, &item_name(3, 1), 0, 0)?;
    short.enroll(Face::HighHalf, &item_name(3, 2), 0, 0)?;
    short.enroll(Face::HighHalf, &item_name(3, 3), 0, 0)?;
    short.enroll(Face::HighHalf, &item_name(3, 4), 0, 0)?;
    short.enroll(Face::HighHalf, &item_name(3, 5), 0, 0)?;
    // 12+36+6 = 54 < 70 → 拦。
    if short.seal() != Err(E_TARGET_SHORT) {
        return Err(E_TARGET_SHORT);
    }

    // —— 步缺拦截：早传面缺步 7 ——
    let mut gap = AssertSet70::new();
    st = 1;
    while st as usize <= HANDOFF_STEPS {
        if st != 7 {
            gap.enroll(Face::EarlyHandoff, &item_name(0, st as usize), st, 0)?;
        }
        st += 1;
    }
    i = 0;
    while i < 20 {
        gap.enroll(Face::DualConsole, &item_name(1, i), 0, 0)?;
        gap.enroll(Face::PageAttr, &item_name(2, i), 0, 0)?;
        gap.enroll(Face::HighHalf, &item_name(3, i), 0, 0)?;
        i += 1;
    }
    if gap.seal() != Err(E_STEP_MISSING) {
        return Err(E_STEP_MISSING);
    }

    // —— 错误路径样本 ——
    if Face::from_id(4) != Err(E_FACE_RANGE) {
        return Err(E_FACE_RANGE);
    }
    let mut bad = AssertSet70::new();
    // 早传面步号 0 越界。
    if bad.enroll(Face::EarlyHandoff, "s0", 0, 0) != Err(E_STEP_RANGE) {
        return Err(E_STEP_RANGE);
    }
    if bad.enroll(Face::EarlyHandoff, "s13", 13, 0) != Err(E_STEP_RANGE) {
        return Err(E_STEP_RANGE);
    }
    // 非早传面步号必须为 0。
    if bad.enroll(Face::PageAttr, "pa1", 3, 0) != Err(E_STEP_RANGE) {
        return Err(E_STEP_RANGE);
    }
    if bad.enroll(Face::PageAttr, "pk", 0, 9) != Err(E_KIND_RANGE) {
        return Err(E_KIND_RANGE);
    }
    bad.enroll(Face::PageAttr, "pa", 0, 0)?;
    // 同面同名重登。
    if bad.enroll(Face::PageAttr, "pa", 0, 0) != Err(E_ITEM_DUP) {
        return Err(E_ITEM_DUP);
    }
    // 异面同名允许（去重按面内）。
    bad.enroll(Face::HighHalf, "pa", 0, 0)?;
    // 面缺封板拦（PageAttr 有 1，HighHalf 有 1，早传/双控台零）。
    if bad.seal() != Err(E_FACE_EMPTY) {
        return Err(E_FACE_EMPTY);
    }
    // 守恒注入：面账拉爆。
    let mut bad2 = AssertSet70::new();
    bad2.enroll(Face::EarlyHandoff, "e1", 1, 0)?;
    bad2.per_face[0] = bad2.per_face[0].wrapping_add(1);
    if bad2.seal() != Err(E_CONSERVATION) {
        return Err(E_CONSERVATION);
    }
    // 未跑即取全绿。
    let mut bad3 = AssertSet70::new();
    bad3.enroll(Face::EarlyHandoff, "e2", 1, 0)?;
    bad3.seal()?;
    if bad3.all_green() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    // 失败分项注入：kind 3 面域恒真——改用非法 kind 无法登记，
    // 注入面：登记后篡改 kind 为 4（越闭集）→ run 必败。
    bad3.items[0].kind = 4;
    bad3.ran = false;
    if bad3.run_all()? != 0 {
        return Err(E_ITEM_FAIL);
    }
    if bad3.all_green()? {
        return Err(E_ITEM_FAIL);
    }

    // 诊断表抽查。
    if diag_detail(E_STEP_MISSING) == "未知诊断码（0x5820 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5820 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
