//! UNX-F0492 渲染操作计数账（操作面守恒） · 0x75E40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5 L 组）：
//!   操作面守恒：Σ(清除+填充+位块+滚动+回读)=操作总数守恒，分类
//!   计数账齐，越账操作显性拒，注入（漏记一类）必抓。
//!
//! 设计要点：
//!   ① 五类操作闭集：清除/填充/位块/滚动/回读（OpKind::ALL），闭集
//!      之外的操作码一律显性拒（UnknownOp）——越账第一道闸；
//!   ② 计数账守恒恒等式：Σ五类计数 = 操作总数（checked_add 逐类
//!      复算 + 总复核），漏记/多记即翻红——注入必抓的锚点；
//!   ③ 操作预算冻结（OP_BUDGET=4096）：总操作数超预算显性拒
//!      （OverAccount）——越账第二道闸，防计数器无限膨胀；
//!   ④ 影子账对拍：每类操作同步记主账与影子账，主账漏记一类时
//!      影子对拍发现缺类（MissingKind）——漏记注入必抓；
//!   ⑤ 状态机单调：Open（记账中）→ Balanced（对账平）→ Frozen
//!      （冻结），冻结后拒绝一切再记账，倒退显性拒；
//!   ⑥ 操作面模型真实：每类操作带像素工作量（宽×高等整数积），
//!      工作量随操作累计可复算。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75E40，批5 L 组 F0492 独占窗口）。
pub const SEG_BASE: u32 = 0x75E40;

/// 操作预算（冻结 4096：单账期总操作上限，越账即拒）。
pub const OP_BUDGET: u32 = 4096;

/// 五类闭集容量（冻结 5）。
pub const KIND_COUNT: usize = 5;

/// 判定轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpCode {
    /// 未知操作码（五类闭集之外）
    UnknownOp,
    /// 越账：总操作数超预算
    OverAccount,
    /// 守恒破坏：Σ五类 ≠ 总数
    ConservationBroken,
    /// 缺类：影子账有记录而主账缺类（漏记注入特征）
    MissingKind,
    /// 重复对账：Balanced 后再 reconcile 且账已变动
    DoubleReconcile,
    /// 状态机倒退/越级
    BadState,
    /// 工作量复算失配
    WorkMismatch,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl OpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OpCode::UnknownOp => "未知操作码：五类闭集（清除/填充/位块/滚动/回读）之外；\
                请给新操作类型走扩闭集评审，勿用野码入账",
            OpCode::OverAccount => "越账：操作总数超出 4096 预算；\
                请开新账期或提高预算评审后再记账",
            OpCode::ConservationBroken => "守恒破坏：Σ五类计数 ≠ 操作总数；\
                请全量重对五类计数账并核对总数计数器",
            OpCode::MissingKind => "缺类：影子账存在而主账缺类；\
                怀疑漏记注入，请恢复五类分类记账通道",
            OpCode::DoubleReconcile => "重复对账：账已变动后再对账须先重开账期；\
                请按 Open→Balanced→Frozen 单向流程走",
            OpCode::BadState => "状态机倒退/越级：账期只许单向推进；\
                请按 Open→Balanced→Frozen 次序操作",
            OpCode::WorkMismatch => "工作量复算失配：Σ类工作量 ≠ 总工作量；\
                请核对每类操作的像素工作量口径",
            OpCode::NotReproducible => "双跑不一致：同输入两跑结果不同；\
                请排查全局可变状态",
        }
    }
    pub const ALL: [OpCode; 8] = [
        OpCode::UnknownOp,
        OpCode::OverAccount,
        OpCode::ConservationBroken,
        OpCode::MissingKind,
        OpCode::DoubleReconcile,
        OpCode::BadState,
        OpCode::WorkMismatch,
        OpCode::NotReproducible,
    ];
}

/// 查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OpCode::ALL.len() {
        if OpCode::ALL[i].val() == code {
            return OpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x75E40 段闭集内"
}

// ── 五类操作闭集 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    /// 清除（清屏/清区）
    Clear,
    /// 填充（矩形填色）
    Fill,
    /// 位块（blit 搬运）
    Blit,
    /// 滚动（卷动换位）
    Scroll,
    /// 回读（像素回读）
    ReadBack,
}

impl OpKind {
    /// 闭集下标（0..5）。
    pub fn idx(self) -> usize {
        match self {
            OpKind::Clear => 0,
            OpKind::Fill => 1,
            OpKind::Blit => 2,
            OpKind::Scroll => 3,
            OpKind::ReadBack => 4,
        }
    }
    /// 操作码（闭集编号 1..5，0 留给「无操作」哨兵）。
    pub fn code(self) -> u32 {
        self.idx() as u32 + 1
    }
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            OpKind::Clear => "清除",
            OpKind::Fill => "填充",
            OpKind::Blit => "位块",
            OpKind::Scroll => "滚动",
            OpKind::ReadBack => "回读",
        }
    }
    /// 闭集遍历（数组闭集返回）。
    pub const ALL: [OpKind; 5] = [
        OpKind::Clear,
        OpKind::Fill,
        OpKind::Blit,
        OpKind::Scroll,
        OpKind::ReadBack,
    ];
}

/// 操作码 → 闭集（野码返回 None，由调用方显性拒）。
pub fn kind_of_code(code: u32) -> Option<OpKind> {
    let mut i = 0;
    while i < OpKind::ALL.len() {
        if let Some(k) = OpKind::ALL.get(i) {
            if k.code() == code {
                return Some(*k);
            }
        }
        i += 1;
    }
    None
}

// ── 账期状态机（单调，倒退显性拒） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpState {
    /// 记账中
    Open,
    /// 对账平
    Balanced,
    /// 冻结（只读）
    Frozen,
}

impl OpState {
    fn rank(self) -> u8 {
        match self {
            OpState::Open => 0,
            OpState::Balanced => 1,
            OpState::Frozen => 2,
        }
    }
}

// ── 操作计数账 ─────────────────────────────────────────────────
/// 单条操作流水（影子账行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpRecord {
    pub kind: OpKind,
    /// 像素工作量（整数积，如 宽×高）
    pub work: u64,
}

/// 渲染操作计数账。
#[derive(Clone, Debug)]
pub struct OpLedger {
    /// 五类计数（下标对齐 OpKind::idx）
    counts: [u32; KIND_COUNT],
    /// 五类工作量（同下标）
    works: [u64; KIND_COUNT],
    /// 操作总数
    total: u32,
    /// 总工作量
    total_work: u64,
    pub state: OpState,
    /// 影子账流水（逐笔）
    pub shadow: Vec<OpRecord>,
    /// 越账拒笔数
    pub over_rejected: u32,
    /// 野码拒笔数
    pub wild_rejected: u32,
}

impl OpLedger {
    pub fn new() -> OpLedger {
        OpLedger {
            counts: [0u32; KIND_COUNT],
            works: [0u64; KIND_COUNT],
            total: 0,
            total_work: 0,
            state: OpState::Open,
            shadow: Vec::new(),
            over_rejected: 0,
            wild_rejected: 0,
        }
    }

    /// 记账：野码显性拒、超预算显性拒、冻结后拒；主账+影子账双记。
    pub fn record(&mut self, kind: OpKind, work: u64) -> Result<u32, u32> {
        if self.state.rank() >= OpState::Frozen.rank() {
            return Err(OpCode::BadState.val());
        }
        // 闭集闸：kind 恒在闭集（类型保证），但预算闸在此后。
        let new_total = match self.total.checked_add(1) {
            Some(t) => t,
            None => return Err(OpCode::OverAccount.val()),
        };
        if new_total > OP_BUDGET {
            self.over_rejected += 1;
            return Err(OpCode::OverAccount.val());
        }
        let i = kind.idx();
        let c = match self.counts.get_mut(i) {
            Some(c) => c,
            None => return Err(OpCode::UnknownOp.val()),
        };
        *c += 1;
        self.total = new_total;
        if let Some(w) = self.works.get_mut(i) {
            *w += work;
        }
        self.total_work += work;
        self.shadow.push(OpRecord { kind, work });
        Ok(self.total)
    }

    /// 野码记账入口（u32 码面）：闭集外显性拒。
    pub fn record_code(&mut self, code: u32, work: u64) -> Result<u32, u32> {
        match kind_of_code(code) {
            Some(k) => self.record(k, work),
            None => {
                self.wild_rejected += 1;
                Err(OpCode::UnknownOp.val())
            }
        }
    }

    /// 守恒复算：Σ五类 = 总数（checked_add 逐类）。
    pub fn conservation(&self) -> Result<(u32, u32), u32> {
        let mut sum = 0u32;
        let mut i = 0;
        while i < KIND_COUNT {
            let c = match self.counts.get(i) {
                Some(c) => *c,
                None => return Err(OpCode::ConservationBroken.val()),
            };
            sum = match sum.checked_add(c) {
                Some(s) => s,
                None => return Err(OpCode::ConservationBroken.val()),
            };
            i += 1;
        }
        if sum != self.total {
            return Err(OpCode::ConservationBroken.val());
        }
        Ok((sum, self.total))
    }

    /// 工作量复算：Σ类工作量 = 总工作量。
    pub fn work_conservation(&self) -> Result<(u64, u64), u32> {
        let mut sum = 0u64;
        let mut i = 0;
        while i < KIND_COUNT {
            let w = match self.works.get(i) {
                Some(w) => *w,
                None => return Err(OpCode::WorkMismatch.val()),
            };
            sum = match sum.checked_add(w) {
                Some(s) => s,
                None => return Err(OpCode::WorkMismatch.val()),
            };
            i += 1;
        }
        if sum != self.total_work {
            return Err(OpCode::WorkMismatch.val());
        }
        Ok((sum, self.total_work))
    }

    /// 缺类对拍：影子账出现过某类而主账计数为 0 ⇒ 漏记（MissingKind）。
    pub fn missing_kind_scan(&self) -> Result<(), u32> {
        // 影子账逐类统计。
        let mut shadow_counts = [0u32; KIND_COUNT];
        let mut i = 0;
        while i < self.shadow.len() {
            if let Some(r) = self.shadow.get(i) {
                let k = r.kind.idx();
                if let Some(c) = shadow_counts.get_mut(k) {
                    *c += 1;
                }
            }
            i += 1;
        }
        let mut k = 0;
        while k < KIND_COUNT {
            let sc = match shadow_counts.get(k) {
                Some(c) => *c,
                None => 0,
            };
            let mc = match self.counts.get(k) {
                Some(c) => *c,
                None => 0,
            };
            if sc > 0 && mc == 0 {
                return Err(OpCode::MissingKind.val());
            }
            k += 1;
        }
        Ok(())
    }

    /// 对账：守恒 + 工作量 + 缺类三复核，平则推进 Balanced。
    pub fn reconcile(&mut self) -> Result<(u32, u64), u32> {
        if self.state != OpState::Open {
            return Err(OpCode::BadState.val());
        }
        self.conservation()?;
        self.work_conservation()?;
        self.missing_kind_scan()?;
        self.state = OpState::Balanced;
        Ok((self.total, self.total_work))
    }

    /// 冻结：Balanced → Frozen，冻结后只读。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state != OpState::Balanced {
            return Err(OpCode::BadState.val());
        }
        self.state = OpState::Frozen;
        Ok(())
    }

    /// 主账缺类注入（漏记模拟）：把某类主账计数清零——守恒破，缺类
    /// 扫描与守恒复核都必抓。
    pub fn inject_drop_kind(&mut self, kind: OpKind) -> bool {
        let i = kind.idx();
        match self.counts.get_mut(i) {
            Some(c) => {
                *c = 0;
                true
            }
            None => false,
        }
    }

    /// 类计数查询。
    pub fn count_of(&self, kind: OpKind) -> u32 {
        match self.counts.get(kind.idx()) {
            Some(c) => *c,
            None => 0,
        }
    }

    /// 总数查询。
    pub fn total(&self) -> u32 {
        self.total
    }
}

/// 黄金账语料：五类各 3 笔，工作量口径固定（清除=宽，填充=宽×高，
/// 位块=源+目标，滚动=行宽，回读=读点数）。
pub fn golden_ledger() -> Result<OpLedger, u32> {
    let mut l = OpLedger::new();
    // 清除 ×3
    l.record(OpKind::Clear, 80)?;
    l.record(OpKind::Clear, 80)?;
    l.record(OpKind::Clear, 80)?;
    // 填充 ×3（80×25）
    l.record(OpKind::Fill, 2000)?;
    l.record(OpKind::Fill, 2000)?;
    l.record(OpKind::Fill, 2000)?;
    // 位块 ×3（源+目标 8×8 ×2）
    l.record(OpKind::Blit, 128)?;
    l.record(OpKind::Blit, 128)?;
    l.record(OpKind::Blit, 128)?;
    // 滚动 ×3（24 行 × 80）
    l.record(OpKind::Scroll, 1920)?;
    l.record(OpKind::Scroll, 1920)?;
    l.record(OpKind::Scroll, 1920)?;
    // 回读 ×3（4 点）
    l.record(OpKind::ReadBack, 4)?;
    l.record(OpKind::ReadBack, 4)?;
    l.record(OpKind::ReadBack, 4)?;
    Ok(l)
}

// ── 判据谓词（私有 bool 收口） ──────────────────────────────────
/// 五类齐判定：黄金账五类各 3 笔、总数 15。
fn all_kinds_present() -> bool {
    match golden_ledger() {
        Ok(l) => {
            let mut k = 0;
            while k < KIND_COUNT {
                match OpKind::ALL.get(k) {
                    Some(kind) => {
                        if l.count_of(*kind) != 3 {
                            return false;
                        }
                    }
                    None => return false,
                }
                k += 1;
            }
            l.total() == 15
        }
        Err(_) => false,
    }
}

/// 守恒判定。
fn conservation_ok() -> bool {
    match golden_ledger() {
        Ok(l) => matches!(l.conservation(), Ok((15, 15))),
        Err(_) => false,
    }
}

/// 野码越账判定：码 0/6/999 显性拒。
fn wild_code_rejected() -> bool {
    let mut l = OpLedger::new();
    let a = l.record_code(0, 1).is_err();
    let b = l.record_code(6, 1).is_err();
    let c = l.record_code(999, 1).is_err();
    let d = l.record_code(1, 1).is_ok();
    a && b && c && d && l.wild_rejected == 3
}

/// 预算越账判定：贴线注入第 4097 笔必拒。
fn budget_rejected() -> bool {
    let mut l = OpLedger::new();
    let mut n = 0u32;
    while n < OP_BUDGET {
        if l.record(OpKind::ReadBack, 1).is_err() {
            return false;
        }
        n += 1;
    }
    let over = l.record(OpKind::ReadBack, 1);
    over.is_err() && l.over_rejected == 1 && l.total() == OP_BUDGET
}

/// 漏记注入判定（10 轮全抓）：逐类清零主账，守恒与缺类扫描必抓。
fn missing_inject_all_caught(rounds: u32) -> bool {
    let mut r = 0u32;
    while r < rounds {
        let mut l = match golden_ledger() {
            Ok(x) => x,
            Err(_) => return false,
        };
        let victim = match OpKind::ALL.get((r as usize) % KIND_COUNT) {
            Some(k) => *k,
            None => return false,
        };
        if !l.inject_drop_kind(victim) {
            return false;
        }
        // 双闸必抓：守恒破 + 缺类扫描命中。
        if l.conservation().is_ok() {
            return false;
        }
        if l.missing_kind_scan().is_ok() {
            return false;
        }
        r += 1;
    }
    true
}

/// 状态机单调判定：冻结后记账拒、Open 直接冻结拒。
fn state_monotonic_ok() -> bool {
    let mut l = match golden_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    // 越级：Open 直接 freeze 拒。
    if l.freeze().is_ok() {
        return false;
    }
    if l.reconcile().is_err() || l.freeze().is_err() {
        return false;
    }
    // 倒退：Frozen 后再记账/再对账均拒。
    l.record(OpKind::Fill, 1).is_err() && l.reconcile().is_err()
}

/// 黄金全链：记账→对账→冻结→守恒+工作量+五类齐三项全绿。
pub fn golden_run() -> Result<(u32, u64, u32), u32> {
    let mut l = golden_ledger()?;
    let (total, work) = l.reconcile()?;
    l.freeze()?;
    let kinds = count_kinds_present(&l);
    Ok((total, work, kinds))
}

/// 五类齐复核。
fn five_kinds_ok(l: &OpLedger) -> bool {
    count_kinds_present(l) == KIND_COUNT as u32
}

/// 在账类数（五类中计数>0 的类数）。
fn count_kinds_present(l: &OpLedger) -> u32 {
    let mut n = 0u32;
    let mut k = 0;
    while k < KIND_COUNT {
        match OpKind::ALL.get(k) {
            Some(kind) => {
                if l.count_of(*kind) > 0 {
                    n += 1;
                }
            }
            None => return n,
        }
        k += 1;
    }
    n
}

/// 探针序 A：黄金全链数值封口（15 笔 / 12396 工作量 / 五类齐）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((15, 12396, 5)) => Ok(()),
        _ => Err(OpCode::NotReproducible.val()),
    }
}

/// 探针序 B：野码单点必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = OpLedger::new();
    match l.record_code(7, 1) {
        Err(e) if e == OpCode::UnknownOp.val() => Ok(()),
        _ => Err(OpCode::NotReproducible.val()),
    }
}

/// 探针序 C：预算与闭集容量冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (OP_BUDGET == 4096, KIND_COUNT == 5) {
        (true, true) => Ok(()),
        _ => Err(OpCode::NotReproducible.val()),
    }
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入账（三路：漏记/野码/越账） ─────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        // 路 1：漏记一类。
        let mut l1 = golden_ledger()?;
        let victim = match OpKind::ALL.get(rng.below(KIND_COUNT as u64) as usize) {
            Some(k) => *k,
            None => OpKind::Clear,
        };
        let _ = l1.inject_drop_kind(victim);
        led.record(0, l1.conservation().is_err() && l1.missing_kind_scan().is_err());
        // 路 2：野码。
        let mut l2 = OpLedger::new();
        led.record(1, l2.record_code(77, 1).is_err());
        // 路 3：越账（Frozen 后记账）。
        let mut l3 = golden_ledger()?;
        let _ = l3.reconcile();
        let _ = l3.freeze();
        led.record(2, l3.record(OpKind::Clear, 1).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(OpCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < OpCode::ALL.len() {
        let v = OpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks ≥12 条） ───────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 操作面守恒。
    v.push((
        "J1-操作面守恒",
        conservation_ok(),
        String::from("Σ五类=总数=15（黄金账）"),
    ));
    // 2. 分类计数账齐。
    v.push((
        "J1-分类账齐",
        all_kinds_present(),
        String::from("清除/填充/位块/滚动/回读 各 3 笔"),
    ));
    // 3. 工作量守恒。
    v.push((
        "J1-工作量守恒",
        match golden_ledger() {
            Ok(l) => matches!(l.work_conservation(), Ok((12396, 12396))),
            Err(_) => false,
        },
        String::from("Σ类工作量=总工作量=12396"),
    ));
    // 4. 野码越账显性拒。
    v.push((
        "J1-野码显性拒",
        wild_code_rejected(),
        String::from("码 0/6/999 全拒，拒笔记账 3"),
    ));
    // 5. 预算越账显性拒。
    v.push((
        "J1-预算越账拒",
        budget_rejected(),
        format!("第 {} 笔必拒", OP_BUDGET + 1),
    ));
    // 6. 漏记注入必抓。
    v.push((
        "J1-漏记注入必抓",
        missing_inject_all_caught(JUDGE_ROUNDS),
        format!("{} 轮逐类漏记全抓", JUDGE_ROUNDS),
    ));
    // 7. 状态机单调。
    v.push((
        "J2-状态机倒退拒",
        state_monotonic_ok(),
        String::from("越级冻结/冻结后记账均拒"),
    ));
    // 8. 注入三路账。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = inj.as_ref().map(|l| l.all_caught()).unwrap_or(false);
    v.push((
        "J1-注入三路账",
        inj_ok,
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 9. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 10. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 11. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 8 码全落段", SEG_BASE),
    ));
    // 12. 闭集映射完备：码 1..5 双向可逆。
    v.push((
        "J1-闭集映射可逆",
        kind_map_bijective(),
        String::from("码↔类 双向查得且互斥"),
    ));
    // 13. 影子账逐笔齐。
    v.push((
        "J1-影子账逐笔齐",
        match golden_ledger() {
            Ok(l) => l.shadow.len() == 15,
            Err(_) => false,
        },
        String::from("影子账 15 笔与主账同步"),
    ));
    // 14. 空账对账平。
    v.push((
        "J1-空账对账平",
        empty_ledger_reconciles(),
        String::from("0 笔空账守恒平（Σ0=0）"),
    ));
    v
}

/// 码↔类双向可逆判定。
fn kind_map_bijective() -> bool {
    let mut k = 0;
    while k < KIND_COUNT {
        let kind = match OpKind::ALL.get(k) {
            Some(x) => *x,
            None => return false,
        };
        match kind_of_code(kind.code()) {
            Some(back) => {
                if back != kind {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    // 码 0 与码 6 必不在闭集。
    kind_of_code(0).is_none() && kind_of_code(6).is_none()
}

/// 空账对账判定。
fn empty_ledger_reconciles() -> bool {
    let mut l = OpLedger::new();
    match l.reconcile() {
        Ok((0, 0)) => true,
        _ => false,
    }
}

// ── 检查节 ─────────────────────────────────────────────────────
pub fn run_unx0492_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0492 渲染操作计数账（操作面守恒）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_ledger_conserves() {
        assert!(conservation_ok());
        assert!(all_kinds_present());
    }

    #[test]
    fn work_conserves() {
        match golden_ledger() {
            Ok(l) => assert_eq!(l.work_conservation(), Ok((12396, 12396))),
            Err(_) => assert!(false, "黄金账不应失败"),
        }
    }

    #[test]
    fn wild_codes_rejected() {
        assert!(wild_code_rejected());
        let mut l = OpLedger::new();
        assert_eq!(l.record_code(7, 1), Err(OpCode::UnknownOp.val()));
    }

    #[test]
    fn budget_enforced_at_boundary() {
        assert!(budget_rejected());
    }

    #[test]
    fn missing_kind_injection_always_caught() {
        assert!(missing_inject_all_caught(JUDGE_ROUNDS));
    }

    #[test]
    fn state_machine_rejects_regression() {
        assert!(state_monotonic_ok());
    }

    #[test]
    fn kind_map_is_bijective() {
        assert!(kind_map_bijective());
        assert_eq!(OpKind::ALL.len(), 5);
        assert_eq!(OpKind::Clear.name(), "清除");
        assert_eq!(OpKind::ReadBack.name(), "回读");
    }

    #[test]
    fn golden_run_reproducible() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert_eq!(a, Ok((15, 12396, 5)));
    }

    #[test]
    fn probes_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn inject_rounds_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => assert!(l.all_caught(), "三路注入全抓：{:?}", l.caught),
            Err(_) => assert!(false, "注入轮不应失败"),
        }
    }

    #[test]
    fn segment_window_holds() {
        assert!(segment_guard());
        assert_eq!(detail(OpCode::MissingKind.val()), OpCode::MissingKind.detail());
        assert_eq!(detail(0x75E7F), "未知诊断码：不在 0x75E40 段闭集内");
    }

    #[test]
    fn empty_ledger_reconciles_and_freeze_gate() {
        assert!(empty_ledger_reconciles());
        let mut l = OpLedger::new();
        assert!(l.reconcile().is_ok());
        assert!(l.freeze().is_ok());
        assert_eq!(l.record(OpKind::Fill, 1), Err(OpCode::BadState.val()));
    }
}
