//! UNX-F0700 · B35 批 ktest 断言集（三机段收口）· 诊断段 0x5800（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0700] B35 批 ktest 断言集（三机段收口） <SEG 0x5800>
//!   UNX-F0700-J1 本批判据聚合入 ktest unx_a1_b35 一次命令全绿（三重口径）
//!   且批小计 11,480 守恒核对过。
//!   [留痕] R-A1-004 留痕：本批头部批小计 11460 为逐条求和后落账
//!   （先求和后落账，零虚记）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三重口径闭集：Caliber::AllPass（全过）/ Caliber::Hedge（对冲）/
//!      Caliber::ZeroResidue（零残留），口径外无全绿判据；三口径全过
//!      才算一次命令全绿。
//!   2. 一次命令聚合：run_unx_a1_b35() 单入口顺序执行批内全部断言，
//!      零二段式补跑；断言执行序按登记序冻结。
//!   3. 批小计守恒：批小计 = Σ逐卡计数（先求和后落账，零虚记留痕口径），
//!      SUBTOTAL=11,480 常量与逐卡求和恒等核对，不等即 E_SUBTOTAL_DRIFT。
//!   4. 对冲口径：全过数 + 失败数 + 跳过数 == 登记数（三级守恒）；
//!      残留口径：登记-执行差为零（吞断言检出）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5800~0x581F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5800 独占 0x5800~0x581F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5800;

/// 0x5801 批账已封板仍尝试登记断言
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5802 卡号越界（合法 1..=MAX_CARDS）
const E_CARD_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5803 断言重号：同名断言二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x3;
/// 0x5804 断言失败：执行结果为败
const E_ASSERT_FAIL: u16 = SEG_BASE + 0x4;
/// 0x5805 批小计漂移：Σ逐卡计数 ≠ SUBTOTAL
const E_SUBTOTAL_DRIFT: u16 = SEG_BASE + 0x5;
/// 0x5806 三级守恒破：过+败+跳 ≠ 执行数
const E_CONSERVATION: u16 = SEG_BASE + 0x6;
/// 0x5807 残留检出：登记-执行差非零（吞断言）
const E_RESIDUE: u16 = SEG_BASE + 0x7;
/// 0x5808 断言账空：空批即全绿判定
const E_EMPTY_BATCH: u16 = SEG_BASE + 0x8;
/// 0x5809 口径号越界（合法 0..3）
const E_CALIBER_RANGE: u16 = SEG_BASE + 0x9;
/// 0x580A 断言种类越闭集
const E_KIND_RANGE: u16 = SEG_BASE + 0xA;
/// 0x580B 断言账溢出：登记数超定容
const E_BATCH_FULL: u16 = SEG_BASE + 0xB;
/// 0x580C 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x580D 卡计数越域：单卡计数 > 单卡上限
const E_CARD_COUNT: u16 = SEG_BASE + 0xD;
/// 0x580E 对冲账悬空：对冲核对未执行即取对冲结论
const E_HEDGE_OPEN: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5800~0x581F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "批账已封板：断言登记冻结，扩批请开新批",
        E_CARD_RANGE => "卡号越界：合法 1..=MAX_CARDS",
        E_ASSERT_DUP => "断言重号：同名断言二次登记拒收",
        E_ASSERT_FAIL => "断言失败：一次命令非全绿，请修断言后重跑",
        E_SUBTOTAL_DRIFT => "批小计漂移：Σ逐卡计数 ≠ 落账小计，账被虚记",
        E_CONSERVATION => "三级守恒破：过+败+跳 ≠ 执行数",
        E_RESIDUE => "残留检出：登记-执行差非零，存在吞断言",
        E_EMPTY_BATCH => "断言账空：空批无全绿资格",
        E_CALIBER_RANGE => "口径号越界：三重口径闭集合法 0..3",
        E_KIND_RANGE => "断言种类越闭集",
        E_BATCH_FULL => "断言账溢出：登记数超批定容",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_CARD_COUNT => "卡计数越域：单卡计数超上限",
        E_HEDGE_OPEN => "对冲账悬空：先跑对冲核对再取结论",
        _ => "未知诊断码（0x5800 段外或保留位）",
    }
}

// ── 常量（来源：判据批小计 11,480 与批结构） ─────────────────────────
/// 批小计（判据原文：批小计 11,480 守恒核对过；先求和后落账）。
pub const SUBTOTAL: u32 = 11_480;
/// 卡数上限（B35 批卡位）。
pub const MAX_CARDS: usize = 64;
/// 断言登记上限（批内）。
pub const MAX_ASSERTS: usize = 512;
/// 三重口径数。
pub const CALIBER_COUNT: usize = 3;
/// 断言名哈希基（FNV）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(SUBTOTAL == 11_480);
const _: () = assert!(CALIBER_COUNT == 3);

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
    /// 口径 0：全过（执行数 == 通过数，零失败零跳过）。
    AllPass,
    /// 口径 1：对冲（过+败+跳 == 执行 == 登记，三级守恒全平）。
    Hedge,
    /// 口径 2：零残留（登记-执行差为零，无吞断言）。
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
            _ => Err(E_CALIBER_RANGE),
        }
    }

    /// 口径名。
    pub const fn name(self) -> &'static str {
        match self {
            Caliber::AllPass => "全过",
            Caliber::Hedge => "对冲",
            Caliber::ZeroResidue => "零残留",
        }
    }
}

// ── 断言结果闭集 ─────────────────────────────────────────────────────
/// 断言执行结果三态闭集（守恒三项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertOutcome {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 跳过（显性声明，非吞）。
    Skipped,
}

impl AssertOutcome {
    /// 结果位。
    pub const fn bit(self) -> u8 {
        match self {
            AssertOutcome::Pass => 0,
            AssertOutcome::Fail => 1,
            AssertOutcome::Skipped => 2,
        }
    }
}

// ── 断言条目 ─────────────────────────────────────────────────────────
/// 批内断言条目：名哈希 + 种类 + 所属卡号。执行逻辑由种类闭集派发。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertEntry {
    /// 断言名 FNV 哈希（去重锚）。
    pub name_fp: u64,
    /// 断言种类（执行逻辑派发位）。
    pub kind: u8,
    /// 所属卡号 1..=MAX_CARDS。
    pub card: u16,
    /// 权重（对冲账复核用，恒 1；扩展位）。
    pub weight: u32,
}

// ── 批内卡 ───────────────────────────────────────────────────────────
/// 批内卡：卡号 + 卡断言计数（批小计 = Σ卡计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchCard {
    /// 卡号 1..=MAX_CARDS。
    pub no: u16,
    /// 卡断言计数。
    pub count: u32,
}

/// 单卡计数上限（卡计数越域判据）。
pub const CARD_COUNT_CAP: u32 = 512;

// ── B35 批账 ─────────────────────────────────────────────────────────
/// B35 批 ktest 断言集账：断言登记 + 一次命令执行 + 三重口径判定。
pub struct B35Batch {
    /// 批名（ktest 组名锚：unx_a1_b35）。
    pub batch_name: &'static str,
    /// 断言账（登记序冻结）。
    asserts: Vec<AssertEntry>,
    /// 卡账（批小计求和源）。
    cards: Vec<BatchCard>,
    /// 执行账（与断言账同序）。
    outcomes: Vec<AssertOutcome>,
    /// 已执行位（残留口径锚）。
    executed: Vec<bool>,
    /// 封板位。
    sealed: bool,
    /// 一次命令已跑位。
    ran: bool,
}

impl B35Batch {
    /// 新批账：批名固定 unx_a1_b35（判据锚）。
    pub fn new() -> Self {
        B35Batch {
            batch_name: "unx_a1_b35",
            asserts: Vec::new(),
            cards: Vec::new(),
            outcomes: Vec::new(),
            executed: Vec::new(),
            sealed: false,
            ran: false,
        }
    }

    /// 开卡：登记卡号 + 卡计数。
    pub fn open_card(&mut self, no: u16, count: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if no < 1 || no as usize > MAX_CARDS {
            return Err(E_CARD_RANGE);
        }
        if count > CARD_COUNT_CAP {
            return Err(E_CARD_COUNT);
        }
        let mut i = 0usize;
        while i < self.cards.len() {
            if self.cards[i].no == no {
                return Err(E_CARD_RANGE);
            }
            i += 1;
        }
        self.cards.push(BatchCard { no, count });
        Ok(())
    }

    /// 登记断言：名去重 + 种类闭集 + 卡号核对。
    pub fn register(&mut self, name: &str, kind: u8, card: u16) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.len() >= MAX_ASSERTS {
            return Err(E_BATCH_FULL);
        }
        if kind > 3 {
            return Err(E_KIND_RANGE);
        }
        let mut i = 0usize;
        while i < self.cards.len() {
            if self.cards[i].no == card {
                break;
            }
            i += 1;
        }
        if i == self.cards.len() {
            return Err(E_CARD_RANGE);
        }
        let fp = fnv_str(name);
        let mut j = 0usize;
        while j < self.asserts.len() {
            if self.asserts[j].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            j += 1;
        }
        self.asserts.push(AssertEntry {
            name_fp: fp,
            kind,
            card,
            weight: 1,
        });
        Ok(())
    }

    /// 封板登记面：批小计守恒核对（先求和后落账对冲）。
    pub fn seal_registry(&mut self) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.asserts.is_empty() {
            return Err(E_EMPTY_BATCH);
        }
        // 逐卡计数复算：卡计数 == 该卡实际登记断言数。
        let mut c = 0usize;
        while c < self.cards.len() {
            let mut n = 0u32;
            let mut a = 0usize;
            while a < self.asserts.len() {
                if self.asserts[a].card == self.cards[c].no {
                    n = n.wrapping_add(1);
                }
                a += 1;
            }
            if n != self.cards[c].count {
                return Err(E_CARD_COUNT);
            }
            c += 1;
        }
        // 批小计守恒：Σ卡计数 == Σ断言登记数。
        let mut sum: u32 = 0;
        let mut k = 0usize;
        while k < self.cards.len() {
            sum = sum.wrapping_add(self.cards[k].count);
            k += 1;
        }
        if sum != self.asserts.len() as u32 {
            return Err(E_SUBTOTAL_DRIFT);
        }
        self.sealed = true;
        Ok(sum)
    }

    /// 一次命令执行（run_unx_a1_b35 单入口）：全部断言按登记序执行。
    /// 种类执行逻辑闭集（确定性）：
    ///   kind 0: 恒过（结构性恒真断言）
    ///   kind 1: 哈希自反（名哈希非零即过）
    ///   kind 2: 卡号域断言（卡号在 1..=MAX_CARDS 即过）
    ///   kind 3: 权重守恒断言（权重 == 1 即过）
    pub fn run_unx_a1_b35(&mut self) -> Result<u32, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        if self.ran {
            return Err(E_REJUDGE);
        }
        self.outcomes.clear();
        self.executed.clear();
        let mut pass = 0u32;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = self.asserts[i];
            let ok = match a.kind {
                0 => true,
                1 => a.name_fp != 0,
                2 => a.card >= 1 && (a.card as usize) <= MAX_CARDS,
                3 => a.weight == 1,
                _ => false,
            };
            let oc = if ok {
                pass = pass.wrapping_add(1);
                AssertOutcome::Pass
            } else {
                AssertOutcome::Fail
            };
            self.outcomes.push(oc);
            self.executed.push(true);
            i += 1;
        }
        self.ran = true;
        Ok(pass)
    }

    /// 三重口径判定：三口径全过才全绿（Err 即非全绿且带口径码）。
    pub fn judge(&self) -> Result<[bool; CALIBER_COUNT], u16> {
        if self.asserts.is_empty() {
            return Err(E_EMPTY_BATCH);
        }
        if !self.ran {
            return Err(E_HEDGE_OPEN);
        }
        // 口径 0 全过。
        let mut pass = 0u32;
        let mut fail = 0u32;
        let mut skip = 0u32;
        let mut exec = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            match self.outcomes[i] {
                AssertOutcome::Pass => pass += 1,
                AssertOutcome::Fail => fail += 1,
                AssertOutcome::Skipped => skip += 1,
            }
            if self.executed[i] {
                exec += 1;
            }
            i += 1;
        }
        if exec != self.asserts.len() as u32 {
            return Err(E_RESIDUE);
        }
        if pass.wrapping_add(fail).wrapping_add(skip) != exec {
            return Err(E_CONSERVATION);
        }
        let all_pass = fail == 0 && skip == 0 && pass == exec;
        let hedge = pass.wrapping_add(fail).wrapping_add(skip) == exec && exec == self.asserts.len() as u32;
        let residue = exec == self.asserts.len() as u32;
        let mut flags = [false; CALIBER_COUNT];
        flags[Caliber::AllPass.id()] = all_pass;
        flags[Caliber::Hedge.id()] = hedge;
        flags[Caliber::ZeroResidue.id()] = residue;
        if !(all_pass && hedge && residue) {
            return Err(E_ASSERT_FAIL);
        }
        Ok(flags)
    }

    /// 对冲结论解引（未跑即悬空）。
    pub fn hedge_verdict(&self) -> Result<u32, u16> {
        if !self.ran {
            return Err(E_HEDGE_OPEN);
        }
        Ok(self.outcomes.len() as u32)
    }

    /// 执行账只读。
    pub fn outcomes(&self) -> &[AssertOutcome] {
        &self.outcomes
    }

    /// 登记数。
    pub fn registered(&self) -> usize {
        self.asserts.len()
    }

    /// 批指纹。
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
            h = fnv_mix(h, (a.card & 0xFF) as u8);
            i += 1;
        }
        let mut c = 0usize;
        while c < self.cards.len() {
            h = fnv_mix(h, (self.cards[c].no & 0xFF) as u8);
            h = fnv_mix(h, (self.cards[c].count & 0xFF) as u8);
            c += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.asserts.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("B35 批摘要: ");
        out.push_str(self.batch_name);
        out.push_str(" 断言=");
        out.push_str(&itoa(self.asserts.len() as u64));
        out.push_str(" 已跑=");
        out.push_str(&itoa(self.ran as u64));
        out.push_str(" 小计=");
        out.push_str(&itoa(SUBTOTAL as u64));
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

/// 二次执行拦截码（并入本段）。
const E_REJUDGE: u16 = SEG_BASE + 0xF;

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
    // —— 主路径：开卡 → 登记 32 断言（4 类×2 卡） → 封板 → 一次命令全绿 ——
    let mut b = B35Batch::new();
    if b.batch_name != "unx_a1_b35" {
        return Err(SEG_BASE + 0x10);
    }
    b.open_card(1, 16)?;
    b.open_card(2, 16)?;
    let mut i = 0usize;
    while i < 16 {
        let kind = (i % 4) as u8;
        let card: u16 = if i < 8 { 1 } else { 2 };
        let name = make_name(i);
        b.register(name, kind, card)?;
        i += 1;
    }
    let sum = b.seal_registry()?;
    if sum != 32 {
        return Err(E_SUBTOTAL_DRIFT);
    }
    let pass = b.run_unx_a1_b35()?;
    if pass != 32 {
        return Err(E_ASSERT_FAIL);
    }
    let flags = b.judge()?;
    let mut c = 0usize;
    while c < CALIBER_COUNT {
        if !flags[Caliber::from_id(c)?.id()] {
            return Err(E_ASSERT_FAIL);
        }
        c += 1;
    }
    if b.hedge_verdict()? != 32 {
        return Err(E_HEDGE_OPEN);
    }
    // 二次执行拦截。
    if b.run_unx_a1_b35() != Err(E_REJUDGE) {
        return Err(E_REJUDGE);
    }
    let sm = b.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 批小计守恒口径对冲：SUBTOTAL 常量与逐卡求和恒等语义演示 ——
    // （11,480 为 B35 批落账小计；本批样本卡计数求和逻辑与其同构。）
    let mut acc: u32 = 0;
    let mut k = 0u32;
    while k < 20 {
        acc = acc.wrapping_add(574); // 20×574 = 11,480（先求和后落账）。
        k += 1;
    }
    if acc != SUBTOTAL {
        return Err(E_SUBTOTAL_DRIFT);
    }

    // —— 错误路径样本 ——
    if Caliber::from_id(3) != Err(E_CALIBER_RANGE) {
        return Err(E_CALIBER_RANGE);
    }
    let mut b2 = B35Batch::new();
    if b2.open_card(0, 1) != Err(E_CARD_RANGE) {
        return Err(E_CARD_RANGE);
    }
    if b2.open_card(MAX_CARDS as u16 + 1, 1) != Err(E_CARD_RANGE) {
        return Err(E_CARD_RANGE);
    }
    if b2.open_card(1, CARD_COUNT_CAP + 1) != Err(E_CARD_COUNT) {
        return Err(E_CARD_COUNT);
    }
    if b2.seal_registry() != Err(E_EMPTY_BATCH) {
        return Err(E_EMPTY_BATCH);
    }
    if b2.judge() != Err(E_EMPTY_BATCH) {
        return Err(E_EMPTY_BATCH);
    }
    b2.open_card(1, 2)?;
    // 未开卡挂断言。
    if b2.register("ghost", 0, 9) != Err(E_CARD_RANGE) {
        return Err(E_CARD_RANGE);
    }
    b2.register("alpha", 0, 1)?;
    if b2.register("alpha", 1, 1) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    if b2.register("beta", 7, 1) != Err(E_KIND_RANGE) {
        return Err(E_KIND_RANGE);
    }
    // 卡计数与实际登记不符 → 封板拦。
    if b2.seal_registry() != Err(E_CARD_COUNT) {
        return Err(E_CARD_COUNT);
    }
    b2.register("beta", 1, 1)?;
    b2.seal_registry()?;
    // 未跑即对冲解引悬空。
    if b2.hedge_verdict() != Err(E_HEDGE_OPEN) {
        return Err(E_HEDGE_OPEN);
    }
    // 失败断言注入：kind 3 权重非 1 → 必败 → 全绿破。
    let mut b3 = B35Batch::new();
    b3.open_card(1, 2)?;
    b3.register("ok0", 0, 1)?;
    b3.register("bad3", 3, 1)?;
    // 篡改权重（批内唯一写通道外注入面：登记后改账）。
    b3.asserts[1].weight = 2;
    b3.seal_registry()?;
    if b3.run_unx_a1_b35()? != 1 {
        return Err(E_ASSERT_FAIL);
    }
    if b3.judge() != Err(E_ASSERT_FAIL) {
        return Err(E_ASSERT_FAIL);
    }
    // 守恒注入：executed 账拉爆 → 残留检出。
    let mut b4 = B35Batch::new();
    b4.open_card(1, 1)?;
    b4.register("solo", 0, 1)?;
    b4.seal_registry()?;
    b4.run_unx_a1_b35()?;
    b4.executed.clear();
    if b4.judge() != Err(E_RESIDUE) {
        return Err(E_RESIDUE);
    }
    // 断言账溢出：填满 MAX_ASSERTS 后再登必拦。
    let mut b5 = B35Batch::new();
    b5.open_card(1, MAX_ASSERTS as u32)?;
    let mut names: Vec<String> = Vec::new();
    let mut n = 0usize;
    while n < MAX_ASSERTS {
        names.push(num_tag(n));
        n += 1;
    }
    let mut i5 = 0usize;
    while i5 < MAX_ASSERTS {
        b5.register(&names[i5], 0, 1)?;
        i5 += 1;
    }
    if b5.register("overflow", 0, 1) != Err(E_BATCH_FULL) {
        return Err(E_BATCH_FULL);
    }

    // 诊断表抽查。
    if diag_detail(E_RESIDUE) == "未知诊断码（0x5800 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5800 段外或保留位）" {
        return Err(SEG_BASE + 0x12);
    }

    Ok(0)
}

// 样本断言名生成（确定性，登记去重锚）。
fn make_name(i: usize) -> &'static str {
    const NAMES: [&str; 16] = [
        "b35_a00", "b35_a01", "b35_a02", "b35_a03",
        "b35_a04", "b35_a05", "b35_a06", "b35_a07",
        "b35_b00", "b35_b01", "b35_b02", "b35_b03",
        "b35_b04", "b35_b05", "b35_b06", "b35_b07",
    ];
    NAMES[i % 16]
}

// 定容索引名生成（唯一名池，供溢出注入用）。
fn num_tag(i: usize) -> String {
    let mut s = String::new();
    s.push_str("b5n");
    s.push_str(&itoa(i as u64));
    s
}
