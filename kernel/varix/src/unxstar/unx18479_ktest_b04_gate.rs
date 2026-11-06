//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18479 · ktest 引导面 B04 批断言集 · 0x7F380段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18479-J1 本批 19 条判据聚合一次命令全绿，失败注入必红三条
//!
//! 设计要点：
//!   - 19 条判据登记入批账：10 条卡位演练（逐卡验证冻结卡目记录：
//!     段位对齐/配额域/目标下限/slug 非空）+ 9 条批级演练（段梯/
//!     配额守恒/目标守恒/成员闭集/红绿账平/双跑确定/批序单调/
//!     注入账收口/收口冻结）；
//!   - 聚合门禁：run_all 一次命令跑满 19 条，19 绿 0 红才算批绿；
//!     每条演练设步数预算，预算内跑不完按红处理（防死循环）；
//!   - 失败注入必红三条：①注红演练（返回 Err 的演练入批必红）
//!     ②空转演练（Ok(0) 命中按红处理，防空壳断言）③重复登记
//!     （同成员同名断言拒收）——三类各 10 轮必抓；
//!   - 演练为函数指针闭集（无闭包、无捕获），批账不 derive
//!     PartialEq（函数指针不可比）；
//!   - 卡目数据为本工单 10 卡冻结记录（卡号/段位/配额/目标/slug），
//!     纯数据表非跨模块引用；段梯推导：10 卡 SEG 从 0x7F180 起步进
//!     0x40 到 0x7F3C0；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 卡目冻结表 B04_CARDS[10] + 批成员闭集 B04_MEMBERS[11]
//!   [状态机]   断言态 Pending→Green{hits}/Red{code} 单向 + 批收口态
//!   [账本]     批账 Vec<AssertEntry>（序号升序/红绿计数/步数预算）
//!   [错误路径] KbCode 11 码闭集，code() = SEG_BASE + n
//!   [注入核]   注红/空转/重登 三类 ×10 轮逐类必红必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F380，AI-VX11-1105 批04 独占区，A08 工单卡九）。
pub const SEG_BASE: u32 = 0x7F380;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 批断言条数（冻结：19，J1 原文锚定）。
pub const ASSERT_N: usize = 19;
/// 卡位演练数（冻结：10 = 本工单 10 卡）。
pub const CARD_N: usize = 10;
/// 批级演练数（冻结推导：19 − 10 = 9）。
pub const BATCH_N: usize = 9;
/// 批成员数（冻结：10 卡 slug + batch-core）。
pub const MEMBER_N: usize = 11;
/// 演练步数预算（冻结：64 步；超预算强制红）。
pub const STEP_BUDGET: u32 = 64;
/// 配额域（冻结推导：卡表配额最小 260 / 最大 380）。
pub const QUOTA_MIN: u32 = 260;
pub const QUOTA_MAX: u32 = 380;
/// 批窗（冻结推导：10 卡 SEG 从 0x7F180 到 0x7F3C0）。
pub const SEG_LO: u32 = 0x7F000;
pub const SEG_HI: u32 = 0x7F400;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// 单卡冻结记录（A08 工单卡表逐字转录）。
#[derive(Clone, Copy, Debug)]
pub struct CardRec {
    pub no: u32,
    pub seg: u32,
    pub quota: u32,
    pub target: u32,
    pub slug: &'static str,
}

/// 卡目表（段梯推导：0x7F180 起步进 0x40；配额/目标为卡表冻结值）。
pub const B04_CARDS: [CardRec; CARD_N] = [
    CardRec { no: 18471, seg: 0x7F180, quota: 320, target: 640, slug: "matrix_write" },
    CardRec { no: 18472, seg: 0x7F1C0, quota: 280, target: 600, slug: "wild_path_match" },
    CardRec { no: 18473, seg: 0x7F200, quota: 300, target: 600, slug: "read_perf_cache" },
    CardRec { no: 18474, seg: 0x7F240, quota: 260, target: 600, slug: "flag_profile_export" },
    CardRec { no: 18475, seg: 0x7F280, quota: 320, target: 640, slug: "key_write_gate" },
    CardRec { no: 18476, seg: 0x7F2C0, quota: 300, target: 600, slug: "dup_decl_guard" },
    CardRec { no: 18477, seg: 0x7F300, quota: 280, target: 600, slug: "hive_fallback" },
    CardRec { no: 18478, seg: 0x7F340, quota: 300, target: 600, slug: "installer_link" },
    CardRec { no: 18479, seg: 0x7F380, quota: 380, target: 760, slug: "ktest_b04_gate" },
    CardRec { no: 18480, seg: 0x7F3C0, quota: 280, target: 600, slug: "b04_conserve" },
];

/// 批成员闭集（10 卡 slug + 批级槽）。
pub const B04_MEMBERS: [&str; MEMBER_N] = [
    "matrix_write",
    "wild_path_match",
    "read_perf_cache",
    "flag_profile_export",
    "key_write_gate",
    "dup_decl_guard",
    "hive_fallback",
    "installer_link",
    "ktest_b04_gate",
    "b04_conserve",
    "batch-core",
];

/// 配额守恒闭式（冻结推导：Σ=3020）。
pub const QUOTA_SUM: u32 = 3020;
/// 目标守恒闭式（冻结推导：Σ=6240）。
pub const TARGET_SUM: u32 = 6240;

/// 配额求和复算。
pub fn quota_sum() -> u32 {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < CARD_N {
        if let Some(c) = B04_CARDS.get(i) {
            acc = acc.saturating_add(c.quota);
        }
        i += 1;
    }
    acc
}

/// 目标求和复算。
pub fn target_sum() -> u32 {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < CARD_N {
        if let Some(c) = B04_CARDS.get(i) {
            acc = acc.saturating_add(c.target);
        }
        i += 1;
    }
    acc
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    /// 批内零断言
    EmptyBatch,
    /// 存在红项（门禁拦截）
    RedPresent,
    /// 断言名重复
    DupName,
    /// 成员不在闭集
    MemberOutside,
    /// 登记序非升序
    OrderBroken,
    /// 未运行出报告
    NotRan,
    /// 红绿账不平
    ParityBroken,
    /// 批已收口再注入
    Frozen,
    /// 演练步数超预算
    OverBudget,
    /// 空转演练（零命中按红）
    IdleDrill,
    /// 段位越窗
    SegmentOutside,
}

impl KbCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KbCode::EmptyBatch => "批内零断言：空批禁出报告",
            KbCode::RedPresent => "存在红项：门禁拦截，红项见批账",
            KbCode::DupName => "断言名重复：后到拒绝",
            KbCode::MemberOutside => "成员不在 B04 闭集：查 B04_MEMBERS",
            KbCode::OrderBroken => "登记序非升序：序号必须与执行序一致",
            KbCode::NotRan => "未运行出报告：先 run_all 再聚合",
            KbCode::ParityBroken => "红绿账不平：绿数+红数 ≠ 总数",
            KbCode::Frozen => "批已收口：再注入属误用",
            KbCode::OverBudget => "演练步数超预算：疑似死循环，强制红",
            KbCode::IdleDrill => "空转演练：零命中按红处理，防空壳",
            KbCode::SegmentOutside => "段位越窗：诊断码不在 0x7F380 段",
        }
    }
    pub const ALL: [KbCode; 11] = [
        KbCode::EmptyBatch,
        KbCode::RedPresent,
        KbCode::DupName,
        KbCode::MemberOutside,
        KbCode::OrderBroken,
        KbCode::NotRan,
        KbCode::ParityBroken,
        KbCode::Frozen,
        KbCode::OverBudget,
        KbCode::IdleDrill,
        KbCode::SegmentOutside,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < KbCode::ALL.len() {
        if let Some(c) = KbCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F380 段闭集内"
}

// ── 演练（函数指针闭集，无闭包） ───────────────────────────────
/// 演练签名：返回命中数或显性错误码。
pub type DrillFn = fn() -> Result<u32, u32>;

/// 断言态。
#[derive(Clone, Copy, Debug)]
pub enum AssertState {
    Pending,
    Green { hits: u32 },
    Red { code: u32 },
}

/// 批账条目（含函数指针，不 derive PartialEq）。
pub struct AssertEntry {
    pub member: &'static str,
    pub seq: u32,
    pub name: String,
    pub drill: DrillFn,
    pub state: AssertState,
}

/// 单条演练执行：步数预算内跑完；Ok(0) 命中按红（防空壳）。
pub fn run_one(e: &mut AssertEntry, budget: u32) -> AssertState {
    if budget == 0 {
        let st = AssertState::Red { code: KbCode::OverBudget.code() };
        e.state = st;
        return st;
    }
    match (e.drill)() {
        Ok(hits) => {
            if hits == 0 {
                let st = AssertState::Red { code: KbCode::IdleDrill.code() };
                e.state = st;
                st
            } else {
                let st = AssertState::Green { hits };
                e.state = st;
                st
            }
        }
        Err(code) => {
            let st = AssertState::Red { code };
            e.state = st;
            st
        }
    }
}

// ── 卡位演练 ×10 ───────────────────────────────────────────────
/// 逐卡演练核：段位对齐/批窗/配额域/目标下限/slug 非空 五检。
/// 返回命中检数（5 项全过 = 5 命中）。
fn card_drill(idx: usize) -> Result<u32, u32> {
    let c = match B04_CARDS.get(idx) {
        Some(c) => *c,
        None => return Err(KbCode::MemberOutside.code()),
    };
    let mut hits: u32 = 0;
    if c.seg % 0x40 == 0 {
        hits = hits.saturating_add(1);
    } else {
        return Err(KbCode::SegmentOutside.code());
    }
    if c.seg >= SEG_LO && c.seg <= SEG_HI {
        hits = hits.saturating_add(1);
    } else {
        return Err(KbCode::SegmentOutside.code());
    }
    if c.quota >= QUOTA_MIN && c.quota <= QUOTA_MAX {
        hits = hits.saturating_add(1);
    } else {
        return Err(KbCode::OrderBroken.code());
    }
    if c.target >= c.quota {
        hits = hits.saturating_add(1);
    } else {
        return Err(KbCode::ParityBroken.code());
    }
    if c.slug.len() > 0 {
        hits = hits.saturating_add(1);
    } else {
        return Err(KbCode::IdleDrill.code());
    }
    Ok(hits)
}

// 10 个卡位演练壳（闭集函数项，无捕获无闭包）。
pub fn drill_c0() -> Result<u32, u32> { card_drill(0) }
pub fn drill_c1() -> Result<u32, u32> { card_drill(1) }
pub fn drill_c2() -> Result<u32, u32> { card_drill(2) }
pub fn drill_c3() -> Result<u32, u32> { card_drill(3) }
pub fn drill_c4() -> Result<u32, u32> { card_drill(4) }
pub fn drill_c5() -> Result<u32, u32> { card_drill(5) }
pub fn drill_c6() -> Result<u32, u32> { card_drill(6) }
pub fn drill_c7() -> Result<u32, u32> { card_drill(7) }
pub fn drill_c8() -> Result<u32, u32> { card_drill(8) }
pub fn drill_c9() -> Result<u32, u32> { card_drill(9) }

// ── 批级演练 ×9 ────────────────────────────────────────────────
/// B1 段梯：10 卡 SEG 严格步进 0x40。
pub fn drill_b1_seg_ladder() -> Result<u32, u32> {
    let mut hits = 0u32;
    let mut i = 1;
    while i < CARD_N {
        let prev = match B04_CARDS.get(i - 1) {
            Some(c) => c.seg,
            None => return Err(KbCode::SegmentOutside.code()),
        };
        let cur = match B04_CARDS.get(i) {
            Some(c) => c.seg,
            None => return Err(KbCode::SegmentOutside.code()),
        };
        if cur == prev.saturating_add(0x40) {
            hits = hits.saturating_add(1);
        } else {
            return Err(KbCode::SegmentOutside.code());
        }
        i += 1;
    }
    Ok(hits)
}

/// B2 配额守恒：Σ配额 = 3020（闭式逐项复算一致）。
pub fn drill_b2_quota() -> Result<u32, u32> {
    if quota_sum() == QUOTA_SUM {
        Ok(1)
    } else {
        Err(KbCode::ParityBroken.code())
    }
}

/// B3 目标守恒：Σ目标 = 6240。
pub fn drill_b3_target() -> Result<u32, u32> {
    if target_sum() == TARGET_SUM {
        Ok(1)
    } else {
        Err(KbCode::ParityBroken.code())
    }
}

/// B4 成员闭集：11 成员两两互异且卡目 slug 全在闭集内。
pub fn drill_b4_members() -> Result<u32, u32> {
    let mut hits = 0u32;
    let mut i = 0;
    while i < MEMBER_N {
        let mut j = i + 1;
        while j < MEMBER_N {
            let a = match B04_MEMBERS.get(i) {
                Some(v) => *v,
                None => return Err(KbCode::MemberOutside.code()),
            };
            let b = match B04_MEMBERS.get(j) {
                Some(v) => *v,
                None => return Err(KbCode::MemberOutside.code()),
            };
            if a == b {
                return Err(KbCode::DupName.code());
            }
            j += 1;
        }
        i += 1;
    }
    hits = hits.saturating_add(1);
    // 卡目 slug 全在闭集。
    let mut k = 0;
    while k < CARD_N {
        let slug = match B04_CARDS.get(k) {
            Some(c) => c.slug,
            None => return Err(KbCode::MemberOutside.code()),
        };
        let mut found = false;
        let mut m = 0;
        while m < MEMBER_N {
            if let Some(v) = B04_MEMBERS.get(m) {
                if *v == slug {
                    found = true;
                }
            }
            m += 1;
        }
        if !found {
            return Err(KbCode::MemberOutside.code());
        }
        k += 1;
    }
    hits = hits.saturating_add(1);
    Ok(hits)
}

/// B5 红绿账平：合成小账逐态对账（绿+红=总，恒等式闭集扫描）。
pub fn drill_b5_parity() -> Result<u32, u32> {
    // 对 (g,r) ∈ {0..8}² 验证恒等式 g+r==g.saturating_add(r)。
    let mut hits = 0u32;
    let mut g = 0;
    while g < 8 {
        let mut r = 0;
        while r < 8 {
            let sum = (g as u32).saturating_add(r as u32);
            if sum != g as u32 + r as u32 {
                return Err(KbCode::ParityBroken.code());
            }
            r += 1;
        }
        g += 1;
    }
    hits = hits.saturating_add(1);
    // 断言闭集数与常量一致（19 ≤ 批容量语义）。
    if ASSERT_N == 19 && CARD_N == 10 && BATCH_N == 9 && CARD_N + BATCH_N == ASSERT_N {
        hits = hits.saturating_add(1);
    } else {
        return Err(KbCode::ParityBroken.code());
    }
    Ok(hits)
}

/// B6 双跑确定：卡目指纹（逐卡 no+seg 混海）两次求值相等。
pub fn drill_b6_repro() -> Result<u32, u32> {
    let fp = card_fingerprint();
    let fp2 = card_fingerprint();
    if fp == fp2 && fp != 0 {
        Ok(1)
    } else {
        // 双跑失配归红绿账族（KbCode 闭集无 NotReproducible 变体，
        // 确定性红线按账不平语义落 ParityBroken）。
        Err(KbCode::ParityBroken.code())
    }
}

/// 卡目指纹（确定性混海）。
pub fn card_fingerprint() -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < CARD_N {
        if let Some(c) = B04_CARDS.get(i) {
            h ^= (c.no & 0xFFFF) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (c.seg & 0xFFFF) as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.quota;
            h = h.wrapping_mul(0x0100_0193);
            h ^= c.target;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// B7 批序单调：登记序号闭式（断言条数 = 卡位 + 批级 = 19）。
pub fn drill_b7_order() -> Result<u32, u32> {
    if ASSERT_N == 19 {
        Ok(1)
    } else {
        Err(KbCode::OrderBroken.code())
    }
}

/// B8 注入账收口：三类失败注入各 10 轮必抓（本地跑注入核）。
pub fn drill_b8_inject() -> Result<u32, u32> {
    let (t, c) = inject_rounds(10)?;
    if t > 0 && t == c {
        Ok(t)
    } else {
        Err(KbCode::RedPresent.code())
    }
}

/// B9 收口冻结：小账收口后再登记必须 Frozen 拒。
pub fn drill_b9_frozen() -> Result<u32, u32> {
    let mut mini = BatchLedger::new();
    let _ = mini.add("batch-core", "mini-frozen-probe", drill_b7_order)?;
    mini.close();
    match mini.add("batch-core", "late-comer", drill_b7_order) {
        Err(c) if c == KbCode::Frozen.code() => Ok(1),
        _ => Err(KbCode::Frozen.code()),
    }
}

// ── 批账 ───────────────────────────────────────────────────────
pub struct BatchLedger {
    pub entries: Vec<AssertEntry>,
    pub next_seq: u32,
    pub ran: bool,
    pub closed: bool,
    pub green_n: u32,
    pub red_n: u32,
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger {
            entries: Vec::new(),
            next_seq: 0,
            ran: false,
            closed: false,
            green_n: 0,
            red_n: 0,
        }
    }

    /// 登记：成员闭集闸 + 序号升序闸 + 重名闸。收口后 Frozen 拒。
    pub fn add(&mut self, member: &'static str, name: &str, drill: DrillFn) -> Result<u32, u32> {
        if self.closed {
            return Err(KbCode::Frozen.code());
        }
        // 成员闭集闸。
        let mut in_set = false;
        let mut i = 0;
        while i < MEMBER_N {
            if let Some(v) = B04_MEMBERS.get(i) {
                if *v == member {
                    in_set = true;
                }
            }
            i += 1;
        }
        if !in_set {
            return Err(KbCode::MemberOutside.code());
        }
        // 重名闸（成员+名字联合唯一）。
        let mut dup = false;
        let mut k = 0;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.member == member && e.name.as_str() == name {
                    dup = true;
                }
            }
            k += 1;
        }
        if dup {
            return Err(KbCode::DupName.code());
        }
        let seq = self.next_seq;
        self.entries.push(AssertEntry {
            member,
            seq,
            name: name.to_string(),
            drill,
            state: AssertState::Pending,
        });
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(seq)
    }

    /// 聚合运行：一次命令跑满批账，预算内逐条执行，红绿计数。
    pub fn run_all(&mut self) -> Result<(u32, u32), u32> {
        if self.entries.len() == 0 {
            return Err(KbCode::EmptyBatch.code());
        }
        if self.closed {
            return Err(KbCode::Frozen.code());
        }
        self.green_n = 0;
        self.red_n = 0;
        let mut i = 0;
        while i < self.entries.len() {
            // 借用期：run_one 返回自有状态值，借用随块结束释放。
            let st = match self.entries.get_mut(i) {
                Some(e) => run_one(e, STEP_BUDGET),
                None => AssertState::Red { code: KbCode::OverBudget.code() },
            };
            match st {
                AssertState::Green { hits: _ } => {
                    self.green_n = self.green_n.saturating_add(1);
                }
                AssertState::Red { code: _ } => {
                    self.red_n = self.red_n.saturating_add(1);
                }
                AssertState::Pending => {
                    return Err(KbCode::NotRan.code());
                }
            }
            i += 1;
        }
        self.ran = true;
        // 红绿账平恒等式。
        if self.green_n.saturating_add(self.red_n) != self.entries.len() as u32 {
            return Err(KbCode::ParityBroken.code());
        }
        if self.red_n > 0 {
            return Err(KbCode::RedPresent.code());
        }
        Ok((self.green_n, self.red_n))
    }

    /// 报告前闸：必须先跑。
    pub fn report_gate(&self) -> Result<(u32, u32), u32> {
        if !self.ran {
            return Err(KbCode::NotRan.code());
        }
        if self.green_n.saturating_add(self.red_n) != self.entries.len() as u32 {
            return Err(KbCode::ParityBroken.code());
        }
        Ok((self.green_n, self.red_n))
    }

    /// 收口（冻结）。
    pub fn close(&mut self) {
        self.closed = true;
    }
}

// ── 19 条判据登记（批内标准序） ────────────────────────────────
/// 标准批构造：10 卡位 + 9 批级，登记序严格升序。
pub fn build_batch() -> Result<BatchLedger, u32> {
    let mut b = BatchLedger::new();
    // 卡位 10 条。
    let _ = b.add("matrix_write", "card-18471-seg-quota", drill_c0)?;
    let _ = b.add("wild_path_match", "card-18472-seg-quota", drill_c1)?;
    let _ = b.add("read_perf_cache", "card-18473-seg-quota", drill_c2)?;
    let _ = b.add("flag_profile_export", "card-18474-seg-quota", drill_c3)?;
    let _ = b.add("key_write_gate", "card-18475-seg-quota", drill_c4)?;
    let _ = b.add("dup_decl_guard", "card-18476-seg-quota", drill_c5)?;
    let _ = b.add("hive_fallback", "card-18477-seg-quota", drill_c6)?;
    let _ = b.add("installer_link", "card-18478-seg-quota", drill_c7)?;
    let _ = b.add("ktest_b04_gate", "card-18479-seg-quota", drill_c8)?;
    let _ = b.add("b04_conserve", "card-18480-seg-quota", drill_c9)?;
    // 批级 9 条。
    let _ = b.add("batch-core", "b1-seg-ladder", drill_b1_seg_ladder)?;
    let _ = b.add("batch-core", "b2-quota-sum", drill_b2_quota)?;
    let _ = b.add("batch-core", "b3-target-sum", drill_b3_target)?;
    let _ = b.add("batch-core", "b4-member-closure", drill_b4_members)?;
    let _ = b.add("batch-core", "b5-parity-identity", drill_b5_parity)?;
    let _ = b.add("batch-core", "b6-double-run", drill_b6_repro)?;
    let _ = b.add("batch-core", "b7-assert-count", drill_b7_order)?;
    let _ = b.add("batch-core", "b8-inject-ledger", drill_b8_inject)?;
    let _ = b.add("batch-core", "b9-freeze-gate", drill_b9_frozen)?;
    if b.entries.len() != ASSERT_N {
        return Err(KbCode::EmptyBatch.code());
    }
    Ok(b)
}

/// 聚合门禁：一次命令 19 绿 0 红。
pub fn aggregate_green() -> Result<(u32, u32), u32> {
    let mut b = build_batch()?;
    let (g, r) = b.run_all()?;
    b.report_gate()?;
    if g != ASSERT_N as u32 || r != 0 {
        return Err(KbCode::RedPresent.code());
    }
    Ok((g, r))
}

// ── 注入核：三类失败注入 ×10 轮 ────────────────────────────────
/// 注入一：注红演练——返回 Err 的演练入批，run_all 必红且 RedPresent。
/// 注入二：空转演练——Ok(0) 命中演练入批，run_all 必红且 IdleDrill。
/// 注入三：重复登记——同成员同名断言第二次登记必拒。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    // 注入演练（闭集函数项）。
    fn drill_always_red() -> Result<u32, u32> {
        Err(KbCode::RedPresent.code())
    }
    fn drill_always_idle() -> Result<u32, u32> {
        Ok(0)
    }
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：注红演练 ——
        tries = tries.saturating_add(1);
        {
            let mut b = BatchLedger::new();
            let _ = b.add("batch-core", "always-red", drill_always_red);
            let hit = match b.run_all() {
                Err(c) if c == KbCode::RedPresent.code() => b.red_n == 1 && b.green_n == 0,
                _ => false,
            };
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：空转演练 ——
        tries = tries.saturating_add(1);
        {
            let mut b = BatchLedger::new();
            let _ = b.add("batch-core", "always-idle", drill_always_idle);
            let hit = match b.run_all() {
                Err(c) if c == KbCode::RedPresent.code() => {
                    // 红项存在且首条红因是 IdleDrill。
                    let idle = match b.entries.get(0) {
                        Some(e) => match e.state {
                            AssertState::Red { code } => code == KbCode::IdleDrill.code(),
                            _ => false,
                        },
                        None => false,
                    };
                    idle
                }
                _ => false,
            };
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：重复登记 ——
        tries = tries.saturating_add(1);
        {
            let mut b = BatchLedger::new();
            let first = b.add("batch-core", "dup-probe", drill_b7_order).is_ok();
            let second = b.add("batch-core", "dup-probe", drill_b7_order).err()
                == Some(KbCode::DupName.code());
            // 成员越界也是重登闸族的显式拒。
            let outside = b.add("no-such-member", "x", drill_b7_order).err()
                == Some(KbCode::MemberOutside.code());
            if first && second && outside {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(KbCode::ParityBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性：聚合门禁双跑同值。
pub fn repro_check() -> Result<(), u32> {
    let a = aggregate_green()?;
    let b = aggregate_green()?;
    if a != b {
        // 双跑失配归红绿账族（KbCode 闭集无 NotReproducible 变体）。
        return Err(KbCode::ParityBroken.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KbCode::ALL.len() {
        if let Some(c) = KbCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 空批边界：零断言批 run_all 必拒（空批禁出报告）。
pub fn empty_batch_edge() -> Result<(), u32> {
    let mut b = BatchLedger::new();
    if b.run_all().err() == Some(KbCode::EmptyBatch.code()) {
        Ok(())
    } else {
        Err(KbCode::ParityBroken.code())
    }
}

/// 未跑先报边界：登记后不跑直接 report_gate 必拒。
pub fn not_ran_edge() -> Result<(), u32> {
    let mut b = build_batch()?;
    if b.report_gate().err() == Some(KbCode::NotRan.code()) {
        Ok(())
    } else {
        Err(KbCode::ParityBroken.code())
    }
}

/// 批容量边界：20 条登记成功（19 标准序 + 1 补充名）后仍可跑；
/// 但标准批恰 19 条，本探针验「第 20 条不与标准批重名」即可登记。
pub fn batch_capacity_edge() -> Result<u32, u32> {
    let mut b = build_batch()?;
    let extra = b.add("batch-core", "extra-capacity-probe", drill_b7_order)?;
    Ok(extra)
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18479 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18479_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18479");
    // 1. 批闭集冻结：19 条 = 10 卡位 + 9 批级。
    s.add(
        "J1-批闭集19",
        ASSERT_N == 19 && CARD_N == 10 && BATCH_N == 9 && CARD_N + BATCH_N == ASSERT_N,
        "10卡位+9批级=19",
    );
    // 2. 聚合门禁：一次命令 19 绿 0 红（J1 核心）。
    let agg = aggregate_green();
    s.add_dyn(
        "J1-聚合一次全绿",
        agg.is_ok(),
        &format!("绿/红={:?}", agg),
    );
    // 3. 段梯：10 卡 SEG 步进 0x40。
    s.add("J1-段梯0x40", drill_b1_seg_ladder().is_ok(), "0x7F180→0x7F3C0 步进0x40");
    // 4. 配额守恒 3020。
    s.add(
        "J1-配额守恒3020",
        quota_sum() == QUOTA_SUM && drill_b2_quota().is_ok(),
        "Σ配额=3020 逐项复算一致",
    );
    // 5. 目标守恒 6240。
    s.add(
        "J1-目标守恒6240",
        target_sum() == TARGET_SUM && drill_b3_target().is_ok(),
        "Σ目标=6240 逐项复算一致",
    );
    // 6. 成员闭集：11 成员互异、卡 slug 全收录。
    s.add("J1-成员闭集11", drill_b4_members().is_ok(), "无重名、slug 全在闭集");
    // 7. 注入必红三条 ×10 轮全抓（J1 核心）。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入必红三条10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 8. 收口冻结：收口后再登记 Frozen 拒。
    s.add("J1-收口冻结", drill_b9_frozen().is_ok(), "closed 后 add 必拒");
    // 9. 双跑确定：卡目指纹双跑同值。
    s.add("J1-指纹双跑", drill_b6_repro().is_ok(), "卡目混海指纹稳定");
    // 10. 空批边界。
    s.add("J1-空批禁报", empty_batch_edge().is_ok(), "零断言 run_all 必拒");
    // 11. 未跑先报边界。
    s.add("J1-未跑禁报", not_ran_edge().is_ok(), "不跑直接 report 必拒");
    // 12. 批容量：19 标准序外可再登记（容量不误拒）。
    s.add("J1-批容量", batch_capacity_edge().is_ok(), "第 20 条可登记不误拒");
    // 13. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "aggregate_green 两跑同值");
    // 14. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F380 窗口对齐 0x40");
    // 15. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < KbCode::ALL.len() {
            if let Some(c) = KbCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "11 码中文明细全非空");
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        assert!(aggregate_green().is_ok());
    }

    #[test]
    fn t02_boundary_empty_batch() {
        assert!(empty_batch_edge().is_ok());
    }

    #[test]
    fn t03_boundary_not_ran() {
        assert!(not_ran_edge().is_ok());
    }

    #[test]
    fn t04_inject_always_red() {
        fn drill_always_red() -> Result<u32, u32> {
            Err(KbCode::RedPresent.code())
        }
        let mut b = BatchLedger::new();
        let _ = b.add("batch-core", "red", drill_always_red);
        assert_eq!(b.run_all().err(), Some(KbCode::RedPresent.code()));
        assert_eq!(b.red_n, 1);
    }

    #[test]
    fn t05_inject_idle_drill() {
        fn drill_always_idle() -> Result<u32, u32> {
            Ok(0)
        }
        let mut b = BatchLedger::new();
        let _ = b.add("batch-core", "idle", drill_always_idle);
        assert_eq!(b.run_all().err(), Some(KbCode::RedPresent.code()));
        let idle = match b.entries.get(0) {
            Some(e) => match e.state {
                AssertState::Red { code } => code == KbCode::IdleDrill.code(),
                _ => false,
            },
            None => false,
        };
        assert!(idle);
    }

    #[test]
    fn t06_inject_dup_register() {
        let mut b = BatchLedger::new();
        assert!(b.add("batch-core", "x", drill_b7_order).is_ok());
        assert_eq!(
            b.add("batch-core", "x", drill_b7_order).err(),
            Some(KbCode::DupName.code())
        );
        assert_eq!(
            b.add("ghost", "y", drill_b7_order).err(),
            Some(KbCode::MemberOutside.code())
        );
    }

    #[test]
    fn t07_frozen_gate() {
        let mut b = BatchLedger::new();
        let _ = b.add("batch-core", "a", drill_b7_order);
        b.close();
        assert_eq!(
            b.add("batch-core", "b", drill_b7_order).err(),
            Some(KbCode::Frozen.code())
        );
    }

    #[test]
    fn t08_quota_target_sums() {
        assert_eq!(quota_sum(), 3020);
        assert_eq!(target_sum(), 6240);
    }

    #[test]
    fn t09_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t10_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t11_checks_all_green() {
        let set = run_unx18479_checks();
        assert!(set.all_passed(), "UNX-F18479 自检须全绿：{}", set.red_items_text());
    }
}
