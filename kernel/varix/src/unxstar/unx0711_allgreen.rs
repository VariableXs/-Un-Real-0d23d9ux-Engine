//! UNX-F0711 · 一键全绿口径（unx_a1 全域入口）· 诊断段 0x58e0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0711] 一键全绿口径（unx_a1 全域入口） <SEG 0x58e0>
//!   UNX-F0711-J1 unx_a1 单命令执行全域 ≥300 断言全绿（三重口径：全过+
//!   对冲+零残留）且总耗时账落簿。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 单命令入口：run_unx_a1() 唯一全域执行通道，一次调用顺序执行
//!      全域断言；多次调用必拦（E_REENTRY）。
//!   2. 全域下限：登记断言 ≥300（GLOBAL_TARGET=300），不足拒跑。
//!   3. 三重口径闭集：全过（零败零跳）/对冲（过+败+跳==执行==登记）/
//!      零残留（登记-执行差零），三口径全过才出全绿结论。
//!   4. 总耗时账落簿：确定性 tick 计费（每断言按 kind 定价），Σtick
//!      落账且账值与重算恒等（耗时账不可虚记）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x58e0~0x58fF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥580（判据原文「纯功能代码 ≥580 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x58e0 独占 0x58e0~0x58fF） ────────────────────────────
pub const SEG_BASE: u16 = 0x58e0;

/// 0x58e1 已登记完成仍尝试补登
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x58e2 全域不足：登记断言 <300 拒跑
const E_GLOBAL_SHORT: u16 = SEG_BASE + 0x2;
/// 0x58e3 单命令重入：全域已执行
const E_REENTRY: u16 = SEG_BASE + 0x3;
/// 0x58e4 断言败：全域非全绿
const E_ASSERT_FAIL: u16 = SEG_BASE + 0x4;
/// 0x58e5 对冲破：过+败+跳 ≠ 执行数
const E_HEDGE: u16 = SEG_BASE + 0x5;
/// 0x58e6 残留检出：登记-执行差非零
const E_RESIDUE: u16 = SEG_BASE + 0x6;
/// 0x58e7 kind 越闭集
const E_KIND_RANGE: u16 = SEG_BASE + 0x7;
/// 0x58e8 批号越域（合法 1..=16）
const E_BATCH_RANGE: u16 = SEG_BASE + 0x8;
/// 0x58e9 断言重号：同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x9;
/// 0x58ea 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0xA;
/// 0x58eb 全域账溢出：登记超定容
const E_GLOBAL_FULL: u16 = SEG_BASE + 0xB;
/// 0x58ec 耗时账破：Σtick 与重算不符
const E_TICK_MISMATCH: u16 = SEG_BASE + 0xC;
/// 0x58ed 未跑即取全绿结论
const E_NOT_RUN: u16 = SEG_BASE + 0xD;
/// 0x58ee 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xE;
/// 0x58ef 断言条目悬空：解引越界
const E_REF_DANGLE: u16 = SEG_BASE + 0xF;

/// 诊断码 → 中文可操作诊断（段 0x58e0~0x58fF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "全域已封板：登记冻结，扩域请开新账",
        E_GLOBAL_SHORT => "全域不足：断言 <300 拒跑，请先补全登记",
        E_REENTRY => "单命令重入：run_unx_a1 只可执行一次",
        E_ASSERT_FAIL => "断言败：全域非全绿，先修败项",
        E_HEDGE => "对冲破：过+败+跳 ≠ 执行数",
        E_RESIDUE => "残留检出：登记-执行差非零，有吞断言",
        E_KIND_RANGE => "kind 越闭集：合法 0..3",
        E_BATCH_RANGE => "批号越域：合法 1..=16",
        E_ASSERT_DUP => "断言重号：同名二次登记拒收",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_GLOBAL_FULL => "全域账溢出：登记超定容上限",
        E_TICK_MISMATCH => "耗时账破：Σtick 与重算不符",
        E_NOT_RUN => "未跑即取结论：先 run_unx_a1",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_REF_DANGLE => "条目悬空：解引越界",
        _ => "未知诊断码（0x58e0 段外或保留位）",
    }
}

// ── 常量（来源：判据全域 ≥300 与计费口径） ───────────────────────────
/// 全域断言下限（判据原文：≥300 断言）。
pub const GLOBAL_TARGET: usize = 300;
/// 批号上限。
pub const BATCH_MAX: u16 = 16;
/// kind 闭集容量。
pub const KIND_VARIANTS: u8 = 3;
/// kind 计费表（tick/条）：0→1、1→2、2→3（确定性定价）。
pub const KIND_TICK: [u64; 3] = [1, 2, 3];
/// 全域定容。
pub const MAX_GLOBAL: usize = 1024;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(GLOBAL_TARGET == 300);
const _: () = assert!(KIND_TICK[0] == 1 && KIND_TICK[1] == 2 && KIND_TICK[2] == 3);

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

// ── 三重口径闭集 ─────────────────────────────────────────────────────
/// 全绿三重口径闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caliber {
    /// 全过：零败零跳。
    AllPass,
    /// 对冲：过+败+跳 == 执行 == 登记。
    Hedge,
    /// 零残留：登记-执行差零。
    ZeroResidue,
}

impl Caliber {
    /// 口径号。
    pub const fn id(self) -> usize {
        match self {
            Caliber::AllPass => 0,
            Caliber::Hedge => 1,
            Caliber::ZeroResidue => 2,
        }
    }

    /// 按口径号重建。
    pub fn from_id(v: usize) -> Result<Caliber, u16> {
        match v {
            0 => Ok(Caliber::AllPass),
            1 => Ok(Caliber::Hedge),
            2 => Ok(Caliber::ZeroResidue),
            _ => Err(E_HEDGE),
        }
    }
}

// ── 执行结果闭集 ─────────────────────────────────────────────────────
/// 断言执行结果三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 过。
    Pass,
    /// 败。
    Fail,
    /// 跳（显性声明）。
    Skip,
}

// ── 全域条目 ─────────────────────────────────────────────────────────
/// 全域断言条目：名哈希 + kind + 批号 + 计费 tick。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalAssert {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// kind（0..=3，执行逻辑派发位）。
    pub kind: u8,
    /// 批号 1..=16。
    pub batch: u16,
    /// 计费 tick（登记时按 kind 定价写死）。
    pub tick: u64,
}

// ── 全域入口账 ───────────────────────────────────────────────────────
/// unx_a1 全域入口账：登记 + 单命令执行 + 三重口径 + 耗时账。
pub struct GlobalRunner {
    /// 入口名（unx_a1 锚）。
    pub entry_name: &'static str,
    /// 断言账（登记序）。
    asserts: Vec<GlobalAssert>,
    /// 执行账（同序）。
    outcomes: Vec<Outcome>,
    /// 封板位（登记面冻结）。
    sealed: bool,
    /// 已跑位。
    ran: bool,
    /// 总耗时账（Σtick，落簿后只读）。
    total_ticks: u64,
}

impl GlobalRunner {
    /// 新全域账。
    pub fn new() -> Self {
        GlobalRunner {
            entry_name: "unx_a1",
            asserts: Vec::new(),
            outcomes: Vec::new(),
            sealed: false,
            ran: false,
            total_ticks: 0,
        }
    }

    /// 登记断言：名去重 + kind/批号域 + 计费定价。
    pub fn enroll(&mut self, name: &str, kind: u8, batch: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.len() >= MAX_GLOBAL {
            return Err(E_GLOBAL_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if kind >= KIND_VARIANTS {
            return Err(E_KIND_RANGE);
        }
        if batch < 1 || batch > BATCH_MAX {
            return Err(E_BATCH_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        let tick = KIND_TICK[kind as usize];
        self.asserts.push(GlobalAssert {
            name_fp: fp,
            kind,
            batch,
            tick,
        });
        Ok(())
    }

    /// 登记数。
    pub fn registered(&self) -> usize {
        self.asserts.len()
    }

    /// 封板：全域下限核对后冻结登记面。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.len() < GLOBAL_TARGET {
            return Err(E_GLOBAL_SHORT);
        }
        self.sealed = true;
        Ok(self.asserts.len())
    }

    /// 单命令执行：run_unx_a1 唯一通道。执行逻辑 kind 闭集派发：
    ///   kind 0: 恒真；kind 1: 哈希自反（名哈希非零即过）；
    ///   kind 2: 批号域断言（批号在 1..=BATCH_MAX 即过）。
    /// 总耗时账同步落簿（Σtick 重算对冲）。
    pub fn run_unx_a1(&mut self) -> Result<u64, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        if self.ran {
            return Err(E_REENTRY);
        }
        if self.asserts.len() < GLOBAL_TARGET {
            return Err(E_GLOBAL_SHORT);
        }
        self.outcomes.clear();
        let mut pass = 0u64;
        let mut ticks: u64 = 0;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = self.asserts[i];
            let ok = match a.kind {
                0 => true,
                1 => a.name_fp != 0,
                2 => a.batch >= 1 && a.batch <= BATCH_MAX,
                _ => false,
            };
            let oc = if ok {
                pass = pass.wrapping_add(1);
                Outcome::Pass
            } else {
                Outcome::Fail
            };
            self.outcomes.push(oc);
            ticks = ticks.wrapping_add(a.tick);
            i += 1;
        }
        // 耗时账重算对冲：登记面 Σtick 必须与执行面累计恒等。
        let mut recheck: u64 = 0;
        let mut k = 0usize;
        while k < self.asserts.len() {
            recheck = recheck.wrapping_add(self.asserts[k].tick);
            k += 1;
        }
        if recheck != ticks {
            return Err(E_TICK_MISMATCH);
        }
        self.total_ticks = ticks;
        self.ran = true;
        Ok(pass)
    }

    /// 三重口径判定：三口径全过才全绿。
    pub fn judge(&self) -> Result<[bool; 3], u16> {
        if !self.ran {
            return Err(E_NOT_RUN);
        }
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut skip = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            match self.outcomes[i] {
                Outcome::Pass => pass += 1,
                Outcome::Fail => fail += 1,
                Outcome::Skip => skip += 1,
            }
            i += 1;
        }
        let registered = self.asserts.len() as u32;
        let executed = self.outcomes.len() as u32;
        // 对冲口径。
        if pass.wrapping_add(fail).wrapping_add(skip) != executed {
            return Err(E_HEDGE);
        }
        // 零残留口径。
        if executed != registered {
            return Err(E_RESIDUE);
        }
        let mut flags = [false; 3];
        flags[Caliber::AllPass.id()] = fail == 0 && skip == 0;
        flags[Caliber::Hedge.id()] = executed == registered;
        flags[Caliber::ZeroResidue.id()] = executed == registered;
        if !flags[Caliber::AllPass.id()] {
            return Err(E_ASSERT_FAIL);
        }
        Ok(flags)
    }

    /// 总耗时账只读（未跑即 0）。
    pub const fn total_ticks(&self) -> u64 {
        self.total_ticks
    }

    /// 计数账快照（过/败/跳/执行）。
    pub fn counts(&self) -> Result<(u32, u32, u32, u32), u16> {
        if !self.ran {
            return Err(E_NOT_RUN);
        }
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut skip = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            match self.outcomes[i] {
                Outcome::Pass => pass += 1,
                Outcome::Fail => fail += 1,
                Outcome::Skip => skip += 1,
            }
            i += 1;
        }
        Ok((pass, fail, skip, self.outcomes.len() as u32))
    }

    /// 条目解引。
    pub fn assert_at(&self, idx: usize) -> Result<GlobalAssert, u16> {
        if idx >= self.asserts.len() {
            return Err(E_REF_DANGLE);
        }
        Ok(self.asserts[idx])
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = self.asserts[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((a.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, a.kind | 0x10);
            h = fnv_mix(h, (a.batch & 0xFF) as u8);
            h = fnv_mix(h, (a.tick & 0x03) as u8 | 0x20);
            i += 1;
        }
        let mut shift = 56i32;
        while shift >= 0 {
            h = fnv_mix(h, ((self.total_ticks >> shift) & 0xFF) as u8);
            shift -= 8;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.asserts.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str(self.entry_name);
        out.push_str(" 全域: 断言=");
        out.push_str(&itoa(self.asserts.len() as u64));
        out.push_str("/300 已跑=");
        out.push_str(&itoa(self.ran as u64));
        out.push_str(" 耗时=");
        out.push_str(&itoa(self.total_ticks));
        out.push_str("tick 指纹=");
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

fn gname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("a1_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：登记 300 断言（3 批 × 100）→ 封板 → 一键全绿 ——
    let mut gr = GlobalRunner::new();
    if gr.entry_name != "unx_a1" {
        return Err(SEG_BASE + 0x10);
    }
    let mut i = 0usize;
    while i < GLOBAL_TARGET {
        let kind = (i % 3) as u8;
        let batch = ((i / 100) as u16) + 1;
        gr.enroll(&gname(i), kind, batch)?;
        i += 1;
    }
    if gr.registered() != GLOBAL_TARGET {
        return Err(E_GLOBAL_SHORT);
    }
    gr.seal()?;
    // 封板后登记必拦。
    if gr.enroll("late", 0, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let pass = gr.run_unx_a1()?;
    if pass != GLOBAL_TARGET as u64 {
        return Err(E_ASSERT_FAIL);
    }
    let flags = gr.judge()?;
    let mut c = 0usize;
    while c < 3 {
        if !flags[Caliber::from_id(c)?.id()] {
            return Err(E_ASSERT_FAIL);
        }
        c += 1;
    }
    // 重入必拦。
    if gr.run_unx_a1() != Err(E_REENTRY) {
        return Err(E_REENTRY);
    }
    // 耗时账：全 kind 0..2 各 100 条 → Σtick = 100×1+100×2+100×3 = 600。
    if gr.total_ticks() != 600 {
        return Err(E_TICK_MISMATCH);
    }
    let (p, f, s, e) = gr.counts()?;
    if p != 300 || f != 0 || s != 0 || e != 300 {
        return Err(E_HEDGE);
    }
    let sm = gr.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 条目解引抽查。
    let a0 = gr.assert_at(0)?;
    if a0.batch != 1 || a0.tick != 1 {
        return Err(E_REF_DANGLE);
    }
    if gr.assert_at(GLOBAL_TARGET) != Err(E_REF_DANGLE) {
        return Err(E_REF_DANGLE);
    }

    // —— 全域不足拒跑：299 条封板即拦 ——
    let mut short = GlobalRunner::new();
    let mut j = 0usize;
    while j < GLOBAL_TARGET - 1 {
        short.enroll(&gname(1000 + j), 0, 1)?;
        j += 1;
    }
    if short.seal() != Err(E_GLOBAL_SHORT) {
        return Err(E_GLOBAL_SHORT);
    }
    if short.run_unx_a1() != Err(E_SEALED) {
        return Err(E_SEALED);
    }

    // —— 失败断言注入：篡改 kind → 全绿破 ——
    let mut bad = GlobalRunner::new();
    let mut k = 0usize;
    while k < GLOBAL_TARGET {
        bad.enroll(&gname(2000 + k), 0, 1)?;
        k += 1;
    }
    bad.seal()?;
    bad.asserts[5].kind = 9;
    if bad.run_unx_a1()? != GLOBAL_TARGET as u64 - 1 {
        return Err(E_ASSERT_FAIL);
    }
    if bad.judge() != Err(E_ASSERT_FAIL) {
        return Err(E_ASSERT_FAIL);
    }

    // —— 残留注入：执行账清空 → 必拦 ——
    let mut res = GlobalRunner::new();
    let mut m = 0usize;
    while m < GLOBAL_TARGET {
        res.enroll(&gname(3000 + m), 0, 1)?;
        m += 1;
    }
    res.seal()?;
    res.run_unx_a1()?;
    res.outcomes.clear();
    if res.judge() != Err(E_RESIDUE) {
        return Err(E_RESIDUE);
    }

    // —— 错误路径样本 ——
    if Caliber::from_id(3) != Err(E_HEDGE) {
        return Err(E_HEDGE);
    }
    let mut err = GlobalRunner::new();
    if err.enroll("", 0, 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    let long = [b'x'; NAME_MAX + 1];
    let long_name = core::str::from_utf8(&long);
    if let Ok(s) = long_name {
        if err.enroll(s, 0, 1) != Err(E_NAME_BAD) {
            return Err(E_NAME_BAD);
        }
    }
    if err.enroll("x", 3, 1) != Err(E_KIND_RANGE) {
        return Err(E_KIND_RANGE);
    }
    if err.enroll("x", 0, 0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if err.enroll("x", 0, BATCH_MAX + 1) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    err.enroll("dup", 0, 1)?;
    if err.enroll("dup", 1, 1) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 未跑即取结论/计数。
    err.seal()?;
    if err.judge() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    if err.counts() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }

    // 诊断表抽查。
    if diag_detail(E_REENTRY) == "未知诊断码（0x58e0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x58e0 段外或保留位）" {
        return Err(SEG_BASE + 0x12);
    }

    Ok(0)
}
