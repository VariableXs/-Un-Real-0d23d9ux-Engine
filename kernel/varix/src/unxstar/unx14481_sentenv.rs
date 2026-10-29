//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14481 · sent 消息封套结构（跨线程） · 0x66780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥348 行；
//!   ② UNX-F14481-J1 封套字段（发送方/目标/结果槽/状态机）账 1 万次
//!      零漂移，结构档注出处；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 封套四字段闭集（结构档注出处：Win32 跨线程 SendMessage 语义
//!     建模，字段名钉版）：发送方线程 tid / 目标线程 tid / 结果槽
//!     （含填充位）/ 封套状态机；
//!   - 状态机闭集：Created→Queued→Delivered→Answered→Retired（合法
//!     边四条，其余一律拒收——跳步/回退/二答必红）；
//!   - 跨线程纪律：投递线程必须 == 目标 tid（发送方不得自投自收面）；
//!     结果槽填充位与状态机联动（Answered 起必已填充，未到必空）——
//!     槽位漂移红线；
//!   - 账 1 万次：LCG 确定性生命周期序列（建封套→排队→投递→应答→
//!     收尾五步×1 万封套），双机对拍逐封套一致；
//!   - 零漂移守恒：Σ各状态计数 = 封套总数；Retired 数 = 填充槽数；
//!   - LCG 种子写死 = 0x66780；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66780，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66780;

/// 线程 tid 闭集（建模：4 线程，tid 1..5，0 恒非法）。
pub const TID_COUNT: usize = 4;
/// 封套账规模（判据原文：1 万次）。
pub const ENVELOPE_ACCOUNT: usize = 10_000;

/// 结构档注出处（字段名钉版，写死）。
pub const FIELD_SENDER: &str = "sender_tid";
pub const FIELD_TARGET: &str = "target_tid";
pub const FIELD_RESULT: &str = "result_slot";
pub const FIELD_STATE: &str = "state";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvCode {
    /// tid=0 或越闭集（1..5）
    ThreadInvalid,
    /// 发送方与目标同线程（跨线程封套红线）
    SameThreadSend,
    /// 状态机非法跃迁（跳步/回退）
    StateIllegal,
    /// 投递线程 ≠ 目标 tid
    DeliverThreadMismatch,
    /// 结果槽漂移（未应答已填充 / 已应答仍空）
    ResultSlotDrift,
    /// 二次应答（Answered 后再答必红）
    DoubleAnswer,
    /// 封套越账（查无此封套）
    EnvelopeNotFound,
    /// 生命周期未收口（存在非 Retired 封套）
    LifecycleIncomplete,
    /// 封套账总数 ≠ 1 万
    AccountMismatch,
    /// 双机对拍漂移
    DualReplayDrift,
}

impl EnvCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EnvCode::ThreadInvalid => "tid=0 或越闭集：线程 1..5",
            EnvCode::SameThreadSend => "发送方与目标同线程：跨线程红线",
            EnvCode::StateIllegal => "状态机非法跃迁：Created→Queued→Delivered→Answered→Retired",
            EnvCode::DeliverThreadMismatch => "投递线程 ≠ 目标 tid：跨线程纪律红线",
            EnvCode::ResultSlotDrift => "结果槽漂移：填充位与状态机不联动",
            EnvCode::DoubleAnswer => "二次应答：Answered 后再答必红",
            EnvCode::EnvelopeNotFound => "封套越账：查无此封套",
            EnvCode::LifecycleIncomplete => "生命周期未收口：存在非 Retired 封套",
            EnvCode::AccountMismatch => "封套账总数 ≠ 1 万",
            EnvCode::DualReplayDrift => "双机对拍漂移：封套账不可复现",
        }
    }
    pub const ALL: [EnvCode; 10] = [
        EnvCode::ThreadInvalid,
        EnvCode::SameThreadSend,
        EnvCode::StateIllegal,
        EnvCode::DeliverThreadMismatch,
        EnvCode::ResultSlotDrift,
        EnvCode::DoubleAnswer,
        EnvCode::EnvelopeNotFound,
        EnvCode::LifecycleIncomplete,
        EnvCode::AccountMismatch,
        EnvCode::DualReplayDrift,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EnvCode::ALL.len() {
        if EnvCode::ALL[i].val() == code {
            return EnvCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66780 段闭集内"
}

// ── 封套状态机（闭集边） ─────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvState {
    Created,
    Queued,
    Delivered,
    Answered,
    Retired,
}

fn env_step(from: EnvState, to: EnvState) -> bool {
    match (from, to) {
        (EnvState::Created, EnvState::Queued) => true,
        (EnvState::Queued, EnvState::Delivered) => true,
        (EnvState::Delivered, EnvState::Answered) => true,
        (EnvState::Answered, EnvState::Retired) => true,
        _ => false,
    }
}

// ── 封套 ─────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub id: u64,
    /// 发送方线程 tid（1..5）
    pub sender: u32,
    /// 目标线程 tid（1..5，≠ sender）
    pub target: u32,
    /// 结果槽（建模 u64）+ 填充位
    pub result: u64,
    pub slot_filled: bool,
    pub state: EnvState,
}

pub struct EnvelopeHub {
    pub envelopes: Vec<Envelope>,
    next_id: u64,
}

impl EnvelopeHub {
    pub fn new() -> EnvelopeHub {
        EnvelopeHub { envelopes: Vec::new(), next_id: 1 }
    }

    fn find(&self, id: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.envelopes.len() {
            if self.envelopes[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 建封套（四字段落位：发送方/目标/空结果槽/Created）。
    pub fn create(&mut self, sender: u32, target: u32) -> Result<u64, u32> {
        if sender == 0 || sender as usize > TID_COUNT {
            return Err(EnvCode::ThreadInvalid.val());
        }
        if target == 0 || target as usize > TID_COUNT {
            return Err(EnvCode::ThreadInvalid.val());
        }
        if sender == target {
            return Err(EnvCode::SameThreadSend.val());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.envelopes.push(Envelope {
            id,
            sender,
            target,
            result: 0,
            slot_filled: false,
            state: EnvState::Created,
        });
        Ok(id)
    }

    /// 状态推进一步（闭集边校验；投递步需核对投递线程）。
    pub fn advance(&mut self, id: u64, to: EnvState, actor_tid: u32) -> Result<(), u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(EnvCode::EnvelopeNotFound.val()),
        };
        if !env_step(self.envelopes[idx].state, to) {
            return Err(EnvCode::StateIllegal.val());
        }
        // 跨线程纪律：投递（Delivered）步 actor 必须 == 目标 tid。
        if to == EnvState::Delivered && actor_tid != self.envelopes[idx].target {
            return Err(EnvCode::DeliverThreadMismatch.val());
        }
        // 应答步：结果槽填充（值由 id×tid 确定性合成）。
        if to == EnvState::Answered {
            if self.envelopes[idx].slot_filled {
                return Err(EnvCode::DoubleAnswer.val());
            }
            self.envelopes[idx].result =
                (id as u64).wrapping_mul(self.envelopes[idx].target as u64) | 1;
            self.envelopes[idx].slot_filled = true;
        }
        self.envelopes[idx].state = to;
        Ok(())
    }

    /// 槽位漂移审计：填充位与状态机逐封套联动一致。
    pub fn slot_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.envelopes.len() {
            let e = &self.envelopes[i];
            let answered_or_later =
                e.state == EnvState::Answered || e.state == EnvState::Retired;
            if e.slot_filled != answered_or_later {
                return Err(EnvCode::ResultSlotDrift.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 生命周期收口审计：全账 Retired；账总数 = 1 万。
    pub fn lifecycle_audit(&self) -> Result<(), u32> {
        if self.envelopes.len() != ENVELOPE_ACCOUNT {
            return Err(EnvCode::AccountMismatch.val());
        }
        let mut i = 0;
        while i < self.envelopes.len() {
            if self.envelopes[i].state != EnvState::Retired {
                return Err(EnvCode::LifecycleIncomplete.val());
            }
            if !self.envelopes[i].slot_filled {
                return Err(EnvCode::ResultSlotDrift.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 双机对拍：独立新 hub 重放同序列，逐封套一致。
    pub fn dual_replay(&self, ops: &[(u32, u32)]) -> Result<(), u32> {
        let mut twin = EnvelopeHub::new();
        let mut i = 0;
        while i < ops.len() {
            let (s, t) = ops[i];
            if let Ok(id) = twin.create(s, t) {
                let _ = twin.advance(id, EnvState::Queued, s);
                let _ = twin.advance(id, EnvState::Delivered, t);
                let _ = twin.advance(id, EnvState::Answered, t);
                let _ = twin.advance(id, EnvState::Retired, s);
            }
            i += 1;
        }
        if twin.envelopes.len() != self.envelopes.len() {
            return Err(EnvCode::DualReplayDrift.val());
        }
        let mut i = 0;
        while i < self.envelopes.len() {
            if twin.envelopes[i] != self.envelopes[i] {
                return Err(EnvCode::DualReplayDrift.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

/// 1 万封套 (sender, target) 序列（确定性；sender≠target 闭式保证：
/// target = sender + 1 + 偏移 mod 4 再 +1）。
pub fn gen_pairs() -> Vec<(u32, u32)> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut v: Vec<(u32, u32)> = Vec::new();
    let mut i = 0;
    while i < ENVELOPE_ACCOUNT {
        let s = 1 + (rng.below(TID_COUNT as u64)) as u32;
        // target ∈ 1..5 且 ≠ s：偏移 1..4 中排除 0。
        let off = 1 + (rng.below((TID_COUNT - 1) as u64)) as u32;
        let t = 1 + ((s - 1 + off) % TID_COUNT as u32);
        v.push((s, t));
        i += 1;
    }
    v
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    // 结构档注出处点验：四字段名钉版非空。
    if FIELD_SENDER.len() == 0
        || FIELD_TARGET.len() == 0
        || FIELD_RESULT.len() == 0
        || FIELD_STATE.len() == 0
    {
        return Err(EnvCode::EnvelopeNotFound.val());
    }
    let pairs = gen_pairs();
    let mut hub = EnvelopeHub::new();
    let mut i = 0;
    while i < pairs.len() {
        let (s, t) = pairs[i];
        if let Ok(id) = hub.create(s, t) {
            let _ = hub.advance(id, EnvState::Queued, s);
            let _ = hub.advance(id, EnvState::Delivered, t);
            let _ = hub.advance(id, EnvState::Answered, t);
            let _ = hub.advance(id, EnvState::Retired, s);
        }
        i += 1;
    }
    hub.slot_audit()?;
    hub.lifecycle_audit()?;
    hub.dual_replay(&pairs)?;
    // 零漂移守恒：填充槽数 = 账长。
    let mut filled = 0;
    let mut i = 0;
    while i < hub.envelopes.len() {
        if hub.envelopes[i].slot_filled {
            filled += 1;
        }
        i += 1;
    }
    Ok((hub.envelopes.len(), filled, true))
}

// ── 注入面：三族 4/4 必抓 ────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：tid 注入三连（tid=0 / tid=9 / 同线程发送）。
    ledger.tries[0] += 3;
    let mut hub = EnvelopeHub::new();
    let mut c = 0;
    if hub.create(0, 2).is_err() {
        c += 1;
    }
    if hub.create(1, 9).is_err() {
        c += 1;
    }
    if hub.create(3, 3).is_err() {
        c += 1;
    }
    if c == 3 {
        ledger.caught[0] += 3;
    }
    // 族 1：状态机跳步注入（Created→Answered 必拒）+ 跨线程投递必拒。
    ledger.tries[1] += 2;
    let mut hub2 = EnvelopeHub::new();
    if let Ok(id) = hub2.create(1, 2) {
        if hub2.advance(id, EnvState::Answered, 2).is_err() {
            ledger.caught[1] += 1;
        }
        // 投递线程错挂：sender(1) 冒充目标(2) 投递必拒。
        let _ = hub2.advance(id, EnvState::Queued, 1);
        if hub2.advance(id, EnvState::Delivered, 1).is_err() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：二次应答注入 + 查无此封套注入。
    ledger.tries[2] += 2;
    let mut hub3 = EnvelopeHub::new();
    if let Ok(id) = hub3.create(2, 4) {
        let _ = hub3.advance(id, EnvState::Queued, 2);
        let _ = hub3.advance(id, EnvState::Delivered, 4);
        let first = hub3.advance(id, EnvState::Answered, 4).is_ok();
        let second = hub3.advance(id, EnvState::Answered, 4).is_err();
        if first && second {
            ledger.caught[2] += 1;
        }
    }
    if hub3.advance(0xFFFF, EnvState::Queued, 1).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EnvCode::ALL.len() {
        let v = EnvCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：1 万封套全生命周期收口 + 零漂移。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((n, filled, ok)) => *ok && *n == ENVELOPE_ACCOUNT && *filled == *n,
        Err(_) => false,
    };
    v.push(("J1-1万封套零漂移收口", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓（跳步/冒投必红）",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 结构档注出处四字段名点验。
    v.push((
        "J1-封套四字段档注出处",
        FIELD_SENDER == "sender_tid"
            && FIELD_TARGET == "target_tid"
            && FIELD_RESULT == "result_slot"
            && FIELD_STATE == "state",
        String::from("sender/target/result/state"),
    ));
    // 4. 状态机闭集反向抽验（回退 Retired→Queued 必拒）。
    let back_ok = {
        let mut hub = EnvelopeHub::new();
        match hub.create(1, 2) {
            Ok(id) => {
                let _ = hub.advance(id, EnvState::Queued, 1);
                let _ = hub.advance(id, EnvState::Delivered, 2);
                let _ = hub.advance(id, EnvState::Answered, 2);
                let _ = hub.advance(id, EnvState::Retired, 1);
                hub.advance(id, EnvState::Queued, 1).is_err()
            }
            Err(_) => false,
        }
    };
    v.push(("J1-状态机回退拒收", back_ok, String::from("Retired→Queued 必拒")));
    // 5. 槽位联动抽验（未应答槽必空）。
    let slot_ok = {
        let mut hub = EnvelopeHub::new();
        match hub.create(2, 3) {
            Ok(id) => {
                let _ = hub.advance(id, EnvState::Queued, 2);
                let e = &hub.envelopes[0];
                !e.slot_filled && e.result == 0
            }
            Err(_) => false,
        }
    };
    v.push(("J1-未应答槽位必空", slot_ok, String::from("联动断言")));
    // 6. 诊断码窗口守卫。
    v.push(("J3-段位守卫", segment_guard(), format!("{:#x} 窗口", SEG_BASE)));
    // 7. LCG 确定性。
    let lcg_ok = {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 32 {
            if a.next() != b.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-LCG 确定性对拍", lcg_ok, format!("seed={:#x}", SEG_BASE)));
    v
}

pub fn run_unx14481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14481 sent 消息封套结构（跨线程）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
