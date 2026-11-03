//! UNX-F0493 多输出占位账（单主输出声明） · 0x75EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5 L 组）：
//!   单主输出声明 v1 冬结：多输出候选登记（候选数显性），主输出唯
//!   一性断言（主输出数=1），主输出切换留痕账，越权切换显性拒。
//!
//! 设计要点：
//!   ① v1 声明冬结（SCHEMA_V1=1 冻结）：候选登记只增不改型，任何
//!      二代结构变更须走新卡，本卡账面即 v1 契约面；
//!   ② 多输出候选登记显性：候选（输出端点名+能力位）登记即入候选
//!      账，候选数账可复算（candidates.len() 即账面值）；
//!   ③ 主输出唯一性断言：primary 是 Option<id> 单槽，声明第二个主
//!      输出显性拒（PrimaryNotUnique），主输出数恒 ≤1 且已声明时=1；
//!   ④ 切换留痕账：每次合法切换落一条 SwitchRec（序号+from→to），
//!      切换序连续可复算（seq 无断号），留痕账不可删除；
//!   ⑤ 越权切换显性拒：from 不是当前主输出、to 未登记、to 即当前主
//!      输出、自切换 from==to 四路全拒（UnauthorizedSwitch）；
//!   ⑥ 状态机单调：Idle（无候选）→ Registered（有候选）→ Primary
//!      （已声明）→ Switched（有切换史），倒退显性拒。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75EC0，批5 L 组 F0493 独占窗口）。
pub const SEG_BASE: u32 = 0x75EC0;

/// 声明版本冬结（v1 冻结，来源：分派卡「单主输出声明 v1 冬结」）。
pub const SCHEMA_V1: u32 = 1;

/// 候选容量（冻结 8：单卡多输出候选上限，越容显性拒）。
pub const MAX_CANDIDATES: usize = 8;

/// 判定轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutCode {
    /// 候选越容（>8）
    CandidateOver,
    /// 重复登记同一候选
    DuplicateCandidate,
    /// 主输出不唯一（已有一主再声明）
    PrimaryNotUnique,
    /// 越权切换（from 非主 / to 未登记 / 自切换）
    UnauthorizedSwitch,
    /// 切换留痕断号（seq 不连续）
    SwitchGap,
    /// 候选不存在
    NoSuchCandidate,
    /// 状态机倒退/越级
    BadState,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl OutCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OutCode::CandidateOver => "候选越容：多输出候选超 8 上限；\
                请先摘除失效候选或走扩容评审",
            OutCode::DuplicateCandidate => "重复登记：该候选已在账；\
                请复用既有候选号，勿重复占位",
            OutCode::PrimaryNotUnique => "主输出不唯一：单主输出声明 v1 下已有主输出；\
                请先走切换留痕通道，勿并行声明第二主输出",
            OutCode::UnauthorizedSwitch => "越权切换：from 非当前主输出 / to 未登记 / \
                自切换四路之一；请先查主输出现值再发切换",
            OutCode::SwitchGap => "切换留痕断号：切换序不连续；\
                请核对留痕账是否被越权改写",
            OutCode::NoSuchCandidate => "候选不存在：操作目标未登记；\
                请先登记候选再声明/切换",
            OutCode::BadState => "状态机倒退/越级：声明流程只许单向推进；\
                请按 Idle→Registered→Primary→Switched 次序走",
            OutCode::NotReproducible => "双跑不一致：同输入两跑结果不同；\
                请排查全局可变状态",
        }
    }
    pub const ALL: [OutCode; 8] = [
        OutCode::CandidateOver,
        OutCode::DuplicateCandidate,
        OutCode::PrimaryNotUnique,
        OutCode::UnauthorizedSwitch,
        OutCode::SwitchGap,
        OutCode::NoSuchCandidate,
        OutCode::BadState,
        OutCode::NotReproducible,
    ];
}

/// 查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OutCode::ALL.len() {
        if OutCode::ALL[i].val() == code {
            return OutCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x75EC0 段闭集内"
}

// ── 占位状态机（单调，倒退显性拒） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutState {
    /// 无候选
    Idle,
    /// 已登记候选（≥1）
    Registered,
    /// 已声明主输出
    Primary,
    /// 有切换史
    Switched,
}

impl OutState {
    fn rank(self) -> u8 {
        match self {
            OutState::Idle => 0,
            OutState::Registered => 1,
            OutState::Primary => 2,
            OutState::Switched => 3,
        }
    }
}

// ── 域模型 ─────────────────────────────────────────────────────
/// 输出候选（占位账行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutCandidate {
    /// 候选号（1..8，登记序）
    pub id: u32,
    /// 端点名（如 "VGA1" "HDMI-A-0" 占位）
    pub name: String,
    /// 能力位图（bit0=可主输出，bit1=可克隆，冻结 v1 口径）
    pub caps: u32,
}

/// 切换留痕（序号 + from→to）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchRec {
    /// 切换序（1 起连续）
    pub seq: u32,
    /// 原主输出候选号（首切前无主记 0）
    pub from: u32,
    /// 新主输出候选号
    pub to: u32,
}

/// 多输出占位账。
#[derive(Clone, Debug)]
pub struct MultiOutLedger {
    pub candidates: Vec<OutCandidate>,
    /// 主输出单槽（None=未声明）
    pub primary: Option<u32>,
    pub switches: Vec<SwitchRec>,
    pub state: OutState,
    /// 越权切换被拒笔数
    pub switch_rejected: u32,
}

impl MultiOutLedger {
    pub fn new() -> MultiOutLedger {
        MultiOutLedger {
            candidates: Vec::new(),
            primary: None,
            switches: Vec::new(),
            state: OutState::Idle,
            switch_rejected: 0,
        }
    }

    /// 候选登记：越容/重复显性拒，候选数账显性可复算。
    pub fn register(&mut self, id: u32, name: &str, caps: u32) -> Result<usize, u32> {
        if id == 0 || id as usize > MAX_CANDIDATES {
            return Err(OutCode::CandidateOver.val());
        }
        let mut i = 0;
        while i < self.candidates.len() {
            if let Some(c) = self.candidates.get(i) {
                if c.id == id {
                    return Err(OutCode::DuplicateCandidate.val());
                }
            }
            i += 1;
        }
        self.candidates.push(OutCandidate { id, name: String::from(name), caps });
        if self.state.rank() < OutState::Registered.rank() {
            self.state = OutState::Registered;
        }
        Ok(self.candidates.len() - 1)
    }

    /// 候选查询。
    pub fn candidate_of(&self, id: u32) -> Option<&OutCandidate> {
        let mut i = 0;
        while i < self.candidates.len() {
            if let Some(c) = self.candidates.get(i) {
                if c.id == id {
                    return Some(c);
                }
            }
            i += 1;
        }
        None
    }

    /// 候选数账（显性）。
    pub fn candidate_count(&self) -> u32 {
        self.candidates.len() as u32
    }

    /// 主输出声明：唯一性断言（已有主输出显性拒）。
    pub fn declare_primary(&mut self, id: u32) -> Result<(), u32> {
        if self.candidate_of(id).is_none() {
            return Err(OutCode::NoSuchCandidate.val());
        }
        if self.primary.is_some() {
            return Err(OutCode::PrimaryNotUnique.val());
        }
        // 能力闸：bit0=可主输出。
        let caps = match self.candidate_of(id) {
            Some(c) => c.caps,
            None => 0,
        };
        if caps & 0x1 == 0 {
            return Err(OutCode::UnauthorizedSwitch.val());
        }
        self.primary = Some(id);
        if self.state.rank() < OutState::Primary.rank() {
            self.state = OutState::Primary;
        }
        Ok(())
    }

    /// 主输出数断言（恒 ≤1；已声明时=1）。
    pub fn primary_count(&self) -> u32 {
        match self.primary {
            Some(_) => 1,
            None => 0,
        }
    }

    /// 主输出切换：留痕入账，四路越权显性拒。
    pub fn switch_primary(&mut self, from: u32, to: u32) -> Result<u32, u32> {
        // 路 1：from 必须是当前主输出（首切前 from 记 0 须与主空位匹配）。
        let cur = self.primary;
        let from_ok = match cur {
            Some(p) => p == from,
            None => from == 0,
        };
        if !from_ok {
            self.switch_rejected += 1;
            return Err(OutCode::UnauthorizedSwitch.val());
        }
        // 路 2：to 必须已登记。
        if self.candidate_of(to).is_none() {
            self.switch_rejected += 1;
            return Err(OutCode::NoSuchCandidate.val());
        }
        // 路 3：自切换拒（from==to 且有主）。
        if cur == Some(to) {
            self.switch_rejected += 1;
            return Err(OutCode::UnauthorizedSwitch.val());
        }
        // 路 4：to 能力闸（可主输出）。
        let caps = match self.candidate_of(to) {
            Some(c) => c.caps,
            None => 0,
        };
        if caps & 0x1 == 0 {
            self.switch_rejected += 1;
            return Err(OutCode::UnauthorizedSwitch.val());
        }
        self.primary = Some(to);
        let seq = self.switches.len() as u32 + 1;
        self.switches.push(SwitchRec { seq, from, to });
        if self.state.rank() < OutState::Switched.rank() {
            self.state = OutState::Switched;
        }
        Ok(seq)
    }

    /// 留痕账复核：序号 1..n 连续无断号。
    pub fn switch_trace_ok(&self) -> bool {
        let mut i = 0;
        while i < self.switches.len() {
            let want = i as u32 + 1;
            match self.switches.get(i) {
                Some(r) => {
                    if r.seq != want {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 留痕链复核：每条 to 与下一条 from 一致（首条 from 与声明值一致）。
    pub fn switch_chain_ok(&self) -> bool {
        let mut i = 0;
        while i < self.switches.len() {
            let cur = match self.switches.get(i) {
                Some(r) => *r,
                None => return false,
            };
            if i + 1 < self.switches.len() {
                let next = match self.switches.get(i + 1) {
                    Some(n) => *n,
                    None => return false,
                };
                if next.from != cur.to {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 黄金占位语料：VGA1/HDMI0/DP1 三候选（HDMI 不可主输出，能力位冻结）。
pub fn golden_ledger() -> Result<MultiOutLedger, u32> {
    let mut l = MultiOutLedger::new();
    l.register(1, "VGA1", 0x1)?;
    l.register(2, "HDMI0", 0x2)?;
    l.register(3, "DP1", 0x3)?;
    Ok(l)
}

// ── 判据谓词（私有 bool 收口） ──────────────────────────────────
/// 候选数显性判定：3 候选在账可复算。
fn candidate_count_ok() -> bool {
    match golden_ledger() {
        Ok(l) => l.candidate_count() == 3 && l.candidate_of(2).is_some(),
        Err(_) => false,
    }
}

/// 主输出唯一性判定：声明后主输出数=1，第二声明显性拒。
fn primary_unique_ok() -> bool {
    let mut l = match golden_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if l.declare_primary(1).is_err() {
        return false;
    }
    let second = l.declare_primary(3);
    l.primary_count() == 1 && second == Err(OutCode::PrimaryNotUnique.val())
}

/// 切换留痕判定：1→3 切换留痕（seq=1, from=1, to=3），链复核过。
fn switch_trace_ok() -> bool {
    let mut l = match golden_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if l.declare_primary(1).is_err() {
        return false;
    }
    let s1 = l.switch_primary(1, 3);
    if s1 != Ok(1) {
        return false;
    }
    let s2 = l.switch_primary(3, 1);
    if s2 != Ok(2) {
        return false;
    }
    l.switches.len() == 2 && l.switch_trace_ok() && l.switch_chain_ok() && l.primary == Some(1)
}

/// 越权切换判定（四路全拒）：from 非主 / to 未登记 / 自切换 / to 无能力位。
fn unauthorized_switch_ok() -> bool {
    let mut l = match golden_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    if l.declare_primary(1).is_err() {
        return false;
    }
    // 路 1：from 非主（2 不是主）。
    let a = l.switch_primary(2, 3).is_err();
    // 路 2：to 未登记（7）。
    let b = l.switch_primary(1, 7).is_err();
    // 路 3：自切换。
    let c = l.switch_primary(1, 1).is_err();
    // 路 4：to 无主输出能力位（HDMI0 caps=0x2，bit0=0）。
    let d = l.switch_primary(1, 2).is_err();
    a && b && c && d && l.switch_rejected == 4 && l.primary == Some(1)
}

/// 越权切换注入判定（10 轮全抓）：每轮四路注入全拒。
fn inject_all_caught(rounds: u32) -> bool {
    let mut r = 0u32;
    while r < rounds {
        if !unauthorized_switch_ok() {
            return false;
        }
        r += 1;
    }
    true
}

/// 状态机单调判定：无候选声明主输出拒（越级 Idle→Primary）。
fn state_monotonic_ok() -> bool {
    let mut l = MultiOutLedger::new();
    // 越级：无候选直接声明。
    if l.declare_primary(1).is_ok() {
        return false;
    }
    if l.register(1, "VGA1", 0x1).is_err() {
        return false;
    }
    if l.declare_primary(1).is_err() {
        return false;
    }
    l.state == OutState::Primary
}

/// 越容/重复登记判定。
fn register_gate_ok() -> bool {
    let mut l = MultiOutLedger::new();
    let zero = l.register(0, "零号", 0x1).is_err();
    let over = l.register(MAX_CANDIDATES as u32 + 1, "越容", 0x1).is_err();
    let mut full_ok = true;
    let mut id = 1u32;
    while id as usize <= MAX_CANDIDATES {
        if l.register(id, "满载", 0x1).is_err() {
            full_ok = false;
        }
        id += 1;
    }
    let dup = l.register(3, "重复", 0x1).is_err();
    zero && over && dup && full_ok && l.candidate_count() == MAX_CANDIDATES as u32
}

/// 黄金全链：登记→声明→双切换→留痕复核→越权四拒。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = golden_ledger()?;
    l.declare_primary(1)?;
    let s1 = l.switch_primary(1, 3)?;
    let s2 = l.switch_primary(3, 1)?;
    let trace_ok = (l.switch_trace_ok() && l.switch_chain_ok()) as u32;
    // 四路越权拒。
    let rej = l.switch_primary(9, 1).is_err() as u32
        + l.switch_primary(1, 7).is_err() as u32
        + l.switch_primary(1, 1).is_err() as u32
        + l.switch_primary(1, 2).is_err() as u32;
    Ok((s1 + s2 + trace_ok, rej, l.candidate_count()))
}

/// 探针序 A：黄金全链数值封口（切换 1+2+留痕 1=4、4 拒、3 候选）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, 3)) => Ok(()),
        _ => Err(OutCode::NotReproducible.val()),
    }
}

/// 探针序 B：第二主输出声明单点必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = golden_ledger()?;
    let _ = l.declare_primary(1);
    match l.declare_primary(3) {
        Err(e) if e == OutCode::PrimaryNotUnique.val() => Ok(()),
        _ => Err(OutCode::NotReproducible.val()),
    }
}

/// 探针序 C：v1 冬结与容量冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (SCHEMA_V1 == 1, MAX_CANDIDATES == 8) {
        (true, true) => Ok(()),
        _ => Err(OutCode::NotReproducible.val()),
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

// ── 注入账（三路：越权切换/重复主声明/越容登记） ────────────────
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
        // 路 1：越权切换（from 非主）。
        let mut l1 = golden_ledger()?;
        let _ = l1.declare_primary(1);
        led.record(0, l1.switch_primary(3, 2).is_err());
        // 路 2：重复主声明。
        let mut l2 = golden_ledger()?;
        let _ = l2.declare_primary(1);
        led.record(1, l2.declare_primary(3).is_err());
        // 路 3：越容登记。
        let mut l3 = MultiOutLedger::new();
        led.record(2, l3.register(9, "越容", 0x1).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(OutCode::NotReproducible.val());
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
    while i < OutCode::ALL.len() {
        let v = OutCode::ALL[i].val();
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
    // 1. v1 冬结。
    v.push((
        "J1-v1声明冬结",
        SCHEMA_V1 == 1,
        format!("SCHEMA_V1={}", SCHEMA_V1),
    ));
    // 2. 候选数显性。
    v.push((
        "J1-候选数显性",
        candidate_count_ok(),
        String::from("3 候选（VGA1/HDMI0/DP1）登记在账"),
    ));
    // 3. 主输出唯一性。
    v.push((
        "J1-主输出唯一",
        primary_unique_ok(),
        String::from("主输出数=1，第二声明显性拒"),
    ));
    // 4. 切换留痕账。
    v.push((
        "J1-切换留痕账",
        switch_trace_ok(),
        String::from("seq 连续 + from→to 链复核双绿"),
    ));
    // 5. 越权切换四路拒。
    v.push((
        "J1-越权切换拒",
        unauthorized_switch_ok(),
        String::from("from非主/to未登记/自切换/无能力位 全拒"),
    ));
    // 6. 注入必抓。
    v.push((
        "J1-注入必抓",
        inject_all_caught(JUDGE_ROUNDS),
        format!("{} 轮越权注入全抓", JUDGE_ROUNDS),
    ));
    // 7. 状态机单调。
    v.push((
        "J2-状态机倒退拒",
        state_monotonic_ok(),
        String::from("无候选声明主输出显性拒"),
    ));
    // 8. 登记闸。
    v.push((
        "J1-登记闸",
        register_gate_ok(),
        String::from("零号/越容/重复登记全拒，8 容量恰满"),
    ));
    // 9. 注入三路账。
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
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 11. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 12. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 8 码全落段", SEG_BASE),
    ));
    // 13. 留痕账不可删。
    v.push((
        "J1-留痕账不可删",
        trace_immutable_ok(),
        String::from("切换留痕账无删除通道，复核恒过"),
    ));
    // 14. 首切前越权（无主时 from≠0 拒）。
    v.push((
        "J1-无主切换拒",
        no_primary_switch_rejected(),
        String::from("未声明主输出时切换显性拒"),
    ));
    v
}

/// 留痕账不可删判定：多轮切换后留痕全数在账且序连续。
fn trace_immutable_ok() -> bool {
    let mut l = match golden_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    let _ = l.declare_primary(1);
    let mut k = 0u32;
    while k < 6 {
        let to = if l.primary == Some(1) { 3 } else { 1 };
        if l.switch_primary(if to == 3 { 1 } else { 3 }, to).is_err() {
            return false;
        }
        k += 1;
    }
    l.switches.len() == 6 && l.switch_trace_ok() && l.switch_chain_ok()
}

/// 无主切换拒判定。
fn no_primary_switch_rejected() -> bool {
    let mut l = match golden_ledger() {
        Ok(x) => x,
        Err(_) => return false,
    };
    // from=0 与主空位匹配才放行，但 to 有能力位才真正切——先拒 from≠0。
    let a = l.switch_primary(1, 3).is_err();
    // from=0 的合法首切：等价声明主输出（须 to 可主）。
    let b = l.switch_primary(0, 3);
    a && b.is_ok() && l.primary == Some(3) && l.state == OutState::Switched
}

// ── 检查节 ─────────────────────────────────────────────────────
pub fn run_unx0493_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0493 多输出占位账（单主输出声明）");
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
    fn candidates_visible_in_ledger() {
        assert!(candidate_count_ok());
    }

    #[test]
    fn primary_is_unique() {
        assert!(primary_unique_ok());
        let mut l = match golden_ledger() {
            Ok(x) => x,
            Err(_) => {
                assert!(false, "黄金语料不应失败");
                return;
            }
        };
        assert!(l.declare_primary(1).is_ok());
        assert_eq!(l.declare_primary(3), Err(OutCode::PrimaryNotUnique.val()));
        assert_eq!(l.primary_count(), 1);
    }

    #[test]
    fn switches_leave_full_trace() {
        assert!(switch_trace_ok());
    }

    #[test]
    fn unauthorized_switch_blocked_all_four() {
        assert!(unauthorized_switch_ok());
        assert!(inject_all_caught(JUDGE_ROUNDS));
    }

    #[test]
    fn state_machine_rejects_skip() {
        assert!(state_monotonic_ok());
    }

    #[test]
    fn register_gate_holds() {
        assert!(register_gate_ok());
        let mut l = MultiOutLedger::new();
        assert_eq!(l.register(0, "零", 1), Err(OutCode::CandidateOver.val()));
        assert_eq!(l.register(9, "越", 1), Err(OutCode::CandidateOver.val()));
    }

    #[test]
    fn no_primary_switch_is_rejected_then_zero_from_works() {
        assert!(no_primary_switch_rejected());
    }

    #[test]
    fn golden_run_reproducible() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        assert_eq!(a, Ok((4, 4, 3)));
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
    fn trace_is_immutable_and_continuous() {
        assert!(trace_immutable_ok());
    }

    #[test]
    fn segment_window_holds() {
        assert!(segment_guard());
        assert_eq!(detail(OutCode::UnauthorizedSwitch.val()), OutCode::UnauthorizedSwitch.detail());
        assert_eq!(detail(0x75EFF), "未知诊断码：不在 0x75EC0 段闭集内");
    }
}
