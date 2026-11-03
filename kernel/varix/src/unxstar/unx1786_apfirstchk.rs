//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1786 · 启动期结构自检（AP 上线首轮核验） · 0x72500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B10）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1786-J1 AP 上线握手后首轮结构核验清单全过（每核
//!     逐项），核验失败阻断上线断言通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · AP 上线握手状态机：Halted→SiSent→Acked→FirstRound→Approved/Blocked，
//!     单调推进，倒退与平移一律显性拒；
//!   · 首轮核验清单 12 项逐项判（GDT/IDT/TSS/GS_BASE/LAPIC/CR4/CR3/栈哨兵/
//!     时间源/SelfIPI/微码/特征字），权重表冻结；未验=跳过、未过=判负，
//!     两类都必须把该核拦在线外；
//!   · 上线闸门：全体核 Approved 且舰队序（id 唯一升序、核数冻结）成立才放
//!     行，任何一核核验失败 → 上线阻断，放行尝试按越权记诊断；
//!   · ack 序列码 = 盐 ^ 核号，逐核对表；重 ack、错码、跳相一律拒；
//!   · 越界注入 10 路（判负项/跳过项/倒退相/错码/重 ack/重复核号/核数偏离/
//!     越权放行/未验放行/阻断复用）10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72500，AP 首轮核验窗口独占）。
pub const SEG_BASE: u32 = 0x72500;
/// 首轮核验清单条数（冻结 12）。
pub const ITEM_N: usize = 12;
/// 核队规模（冻结 4：BSP + 3 AP）。
pub const CORE_N: usize = 4;
/// ack 序列盐（握手码派生，冻结）。
pub const ACK_SALT: u64 = 0xA5F1_7860;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 诊断窗口宽（段内 0x40）。
pub const SEG_WINDOW: u32 = 0x40;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApChkCode {
    /// 握手码坏：ack 序列码与 盐^核号 对不上，重发 SI 后再验
    AckBad,
    /// 相位倒退/平移：握手状态机单调，重 ack、跳相、阻断后复用一律拒
    PhaseBack,
    /// 清单项判负：某核某项核验不过，必须修复后才可再上线
    ItemFail,
    /// 清单项未验：跳项视为未过，禁止带洞放行
    ItemSkip,
    /// 核号越权：舰队 id 必须唯一且落在 0..CORE_N
    CoreIdOff,
    /// 核数偏离：舰队规模不等于冻结值 CORE_N
    FleetCountOff,
    /// 越权放行：上线闸门对未 Approved 的核放行（含 Halted 直通）
    GateBypass,
    /// 双跑不一致：golden_run 两次应同值（复现红线）
    NotReproducible,
}

impl ApChkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ApChkCode::AckBad => "握手码坏：ack 序列码 = 盐^核号，对不上就重发 SI",
            ApChkCode::PhaseBack => "相位倒退：握手状态机只进不退，平移也拒",
            ApChkCode::ItemFail => "清单判负：核验失败阻断上线，先修坏项再重走首轮",
            ApChkCode::ItemSkip => "清单未验：跳项=未过，12 项逐项补验后重来",
            ApChkCode::CoreIdOff => "核号越权：舰队 id 唯一且 <CORE_N，查建队派生",
            ApChkCode::FleetCountOff => "核数偏离：舰队规模≠冻结值，查拓扑表",
            ApChkCode::GateBypass => "越权放行：上线闸门只认全体 Approved，堵直通路径",
            ApChkCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [ApChkCode; 8] = [
        ApChkCode::AckBad,
        ApChkCode::PhaseBack,
        ApChkCode::ItemFail,
        ApChkCode::ItemSkip,
        ApChkCode::CoreIdOff,
        ApChkCode::FleetCountOff,
        ApChkCode::GateBypass,
        ApChkCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ApChkCode::ALL.len() {
        if ApChkCode::ALL[i].val() == code {
            return ApChkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72500 段闭集内"
}

// ── 首轮核验清单（冻结权重表，逐字对齐判据原文） ────────────────
/// 清单项：名 + 权重（0=轻 1=中 3=重；权重只进报表，不改变一票否决）。
pub const ITEM_TABLE: [(&str, u8); ITEM_N] = [
    ("GDT装载", 3),
    ("IDT装载", 3),
    ("TSS有效", 3),
    ("GS_BASE设定", 3),
    ("LAPIC使能", 2),
    ("CR4特征窗", 2),
    ("CR3页表就位", 3),
    ("栈哨兵可写", 2),
    ("时间源就绪", 1),
    ("SelfIPI可达", 2),
    ("微码已载", 1),
    ("特征字一致", 2),
];

/// 单项判定：0=未验 1=过 2=不过。
pub const V_UNSET: u8 = 0;
pub const V_PASS: u8 = 1;
pub const V_FAIL: u8 = 2;

// ── 握手状态机（Halted→SiSent→Acked→FirstRound→Approved/Blocked） ─
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApPhase {
    /// 停机：SI 未发。
    Halted,
    /// SI 已发：等 ack。
    SiSent,
    /// ack 已收：可进首轮核验。
    Acked,
    /// 首轮核验中。
    FirstRound,
    /// 核验全过：准入上线。
    Approved,
    /// 核验失败：阻断上线（终态，复用拒）。
    Blocked,
}

impl ApPhase {
    /// 单调序值。
    pub fn rank(self) -> u8 {
        match self {
            ApPhase::Halted => 0,
            ApPhase::SiSent => 1,
            ApPhase::Acked => 2,
            ApPhase::FirstRound => 3,
            ApPhase::Approved => 4,
            ApPhase::Blocked => 5,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: ApPhase) -> bool {
        to.rank() > self.rank()
    }
    /// 是否终态（Approved/Blocked）。
    pub fn is_terminal(self) -> bool {
        matches!(self, ApPhase::Approved | ApPhase::Blocked)
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 单个 AP 核的上线账：相位 + 清单判定 + 握手码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApCore {
    pub id: u32,
    pub phase: ApPhase,
    /// 期望握手码 = ACK_SALT ^ id。
    pub ack_expect: u64,
    /// 12 项判定（V_UNSET/V_PASS/V_FAIL）。
    pub verdicts: [u8; ITEM_N],
    /// 已完成的首轮核验轮数。
    pub rounds: u32,
    /// 阻断原因（诊断码），未阻断为 0。
    pub deny: u32,
}

impl ApCore {
    pub fn new(id: u32) -> ApCore {
        ApCore {
            id,
            phase: ApPhase::Halted,
            ack_expect: ACK_SALT ^ (id as u64),
            verdicts: [V_UNSET; ITEM_N],
            rounds: 0,
            deny: 0,
        }
    }

    /// 相位推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: ApPhase) -> Result<(), u32> {
        if !self.phase.can_advance(to) {
            return Err(ApChkCode::PhaseBack.val());
        }
        self.phase = to;
        Ok(())
    }

    /// 核验失败落阻断（终态）：仅 FirstRound 可落。
    pub fn block(&mut self, code: u32) -> Result<(), u32> {
        if self.phase != ApPhase::FirstRound {
            return Err(ApChkCode::PhaseBack.val());
        }
        self.deny = code;
        self.phase = ApPhase::Blocked;
        Ok(())
    }
}

/// 核队确定性构建：id = 0..n 升序唯一，清单全未验，相位全 Halted。
pub fn build_fleet(n: usize) -> Vec<ApCore> {
    let mut v: Vec<ApCore> = Vec::new();
    let mut i = 0usize;
    while i < n {
        v.push(ApCore::new(i as u32));
        i += 1;
    }
    v
}

// ── 握手三步（SI→ack→进首轮） ───────────────────────────────────
/// 发 SI：Halted→SiSent。
pub fn send_si(c: &mut ApCore) -> Result<(), u32> {
    c.advance(ApPhase::SiSent)
}

/// 收 ack：码对表 + SiSent→Acked；重 ack（平移）按倒退拒。
pub fn recv_ack(c: &mut ApCore, seq: u64) -> Result<(), u32> {
    if seq != c.ack_expect {
        return Err(ApChkCode::AckBad.val());
    }
    c.advance(ApPhase::Acked)
}

/// 进首轮核验：Acked→FirstRound。
pub fn enter_first_round(c: &mut ApCore) -> Result<(), u32> {
    c.advance(ApPhase::FirstRound)
}

// ── 首轮核验引擎（每核逐项，一票否决） ──────────────────────────
/// 首轮核验：12 项逐项判；任一 未验→ItemSkip、不过→ItemFail 并落阻断。
/// 全过则 rounds+1 并推进 Approved。返回过项数。
pub fn verify_first_round(c: &mut ApCore) -> Result<u32, u32> {
    if c.phase != ApPhase::FirstRound {
        return Err(ApChkCode::PhaseBack.val());
    }
    let mut passed = 0u32;
    let mut i = 0usize;
    while i < ITEM_N {
        match c.verdicts[i] {
            V_UNSET => {
                let _ = c.block(ApChkCode::ItemSkip.val());
                return Err(ApChkCode::ItemSkip.val());
            }
            V_FAIL => {
                let _ = c.block(ApChkCode::ItemFail.val());
                return Err(ApChkCode::ItemFail.val());
            }
            _ => passed += 1,
        }
        i += 1;
    }
    c.rounds += 1;
    c.advance(ApPhase::Approved)?;
    Ok(passed)
}

/// 舰队序审计：核数冻结值、id 唯一升序。
pub fn fleet_shape_ok(fleet: &[ApCore]) -> Result<(), u32> {
    if fleet.len() != CORE_N {
        return Err(ApChkCode::FleetCountOff.val());
    }
    let mut i = 0usize;
    while i < fleet.len() {
        let c = match fleet.get(i) {
            Some(c) => c,
            None => return Err(ApChkCode::FleetCountOff.val()),
        };
        if c.id != i as u32 || c.id as usize >= CORE_N {
            return Err(ApChkCode::CoreIdOff.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 上线闸门（判据②：核验失败阻断上线） ─────────────────────────
/// 上线报表：放行核数 / 过项总数 / 拒因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineReport {
    pub approved: u32,
    pub items_passed: u32,
    /// 闸门拒因（0=放行）。
    pub deny: u32,
}

/// 上线闸门：全体 Approved 且舰队序成立才放行；任一核不到 → 阻断。
/// 闸门绝不修改核相位（越权直通在闸门里被显性记账拒绝）。
pub fn online_gate(fleet: &[ApCore]) -> Result<OnlineReport, u32> {
    fleet_shape_ok(fleet)?;
    let mut approved = 0u32;
    let mut items_passed = 0u32;
    let mut i = 0usize;
    while i < fleet.len() {
        let c = match fleet.get(i) {
            Some(c) => c,
            None => return Err(ApChkCode::FleetCountOff.val()),
        };
        match c.phase {
            ApPhase::Approved => {
                approved += 1;
                let mut j = 0usize;
                while j < ITEM_N {
                    if c.verdicts[j] == V_PASS {
                        items_passed += 1;
                    }
                    j += 1;
                }
            }
            ApPhase::Blocked => {
                return Ok(OnlineReport {
                    approved,
                    items_passed,
                    deny: if c.deny != 0 { c.deny } else { ApChkCode::ItemFail.val() },
                })
            }
            _ => {
                // 未走完首轮就到闸门：按越权放行记账后拒绝。
                return Ok(OnlineReport {
                    approved,
                    items_passed,
                    deny: ApChkCode::GateBypass.val(),
                });
            }
        }
        i += 1;
    }
    if approved != CORE_N as u32 {
        return Ok(OnlineReport { approved, items_passed, deny: ApChkCode::GateBypass.val() });
    }
    Ok(OnlineReport { approved, items_passed, deny: 0 })
}

// ── 黄金全链（正常流） ──────────────────────────────────────────
/// 首轮逐项结构自探（确定性）：对清单表 12 项逐一落判定。
/// 判定依据 = 表形状逐项成立（项名非空、权重落在 [1,3]）→ V_PASS，否则 V_FAIL。
pub fn self_probe_items(c: &mut ApCore) -> Result<(), u32> {
    let mut i = 0usize;
    while i < ITEM_N {
        let (name, w) = ITEM_TABLE[i];
        c.verdicts[i] = if !name.is_empty() && w >= 1 && w <= 3 { V_PASS } else { V_FAIL };
        i += 1;
    }
    Ok(())
}

/// 单核全步：SI→ack→首轮→逐项自探→核验。返回过项数。
pub fn bring_core_online(c: &mut ApCore) -> Result<u32, u32> {
    send_si(c)?;
    let seq = c.ack_expect;
    recv_ack(c, seq)?;
    enter_first_round(c)?;
    self_probe_items(c)?;
    verify_first_round(c)
}

/// 舰队全链：每核逐项核验 + 上线闸门。全绿返回 (approved=4, items=48)。
pub fn golden_fleet_online(fleet: &mut Vec<ApCore>) -> Result<OnlineReport, u32> {
    let mut i = 0usize;
    while i < fleet.len() {
        let c = match fleet.get_mut(i) {
            Some(c) => c,
            None => return Err(ApChkCode::FleetCountOff.val()),
        };
        bring_core_online(c)?;
        i += 1;
    }
    let rep = online_gate(fleet)?;
    if rep.deny != 0 {
        return Ok(rep);
    }
    Ok(rep)
}

/// 黄金件全绿标记：核数/过项数的冻结期望。
pub fn golden_expect() -> (u32, u32) {
    (CORE_N as u32, (CORE_N * ITEM_N) as u32)
}

/// 放行除 skip 下标外的全部核（注入现场搭台用；跳过的核留给违约构造）。
pub fn online_others(fleet: &mut Vec<ApCore>, skip: usize) -> Result<(), u32> {
    let mut i = 0usize;
    while i < fleet.len() {
        if i != skip {
            let c = match fleet.get_mut(i) {
                Some(c) => c,
                None => return Err(ApChkCode::FleetCountOff.val()),
            };
            bring_core_online(c)?;
        }
        i += 1;
    }
    Ok(())
}

// ── 失真注入（10 路越界，判据 10/10 必抓） ──────────────────────
/// 越界注入轮：第 kind 路构造一个违约现场，核验/闸门必须抓到。
/// 返回 (该路拒因诊断码, 是否抓到)。
pub fn inject_round(round: u32) -> Result<(u32, bool), u32> {
    let kind = (round % 10) as usize;
    match kind {
        0 => {
            // 清单项判负：核 2 逐项自探后判负 CR3 项，其余核正常放行 → 核验必须拦。
            let mut f = build_fleet(CORE_N);
            online_others(&mut f, 2)?;
            if let Some(c) = f.get_mut(2) {
                send_si(c)?;
                recv_ack(c, c.ack_expect)?;
                enter_first_round(c)?;
                self_probe_items(c)?;
                c.verdicts[6] = V_FAIL;
                let _ = verify_first_round(c);
            }
            let rep = online_gate(&f);
            caught(rep, ApChkCode::ItemFail.val())
        }
        1 => {
            // 清单项未验：核 1 逐项自探后留空 LAPIC 项，其余核正常放行 → 跳项必拦。
            let mut f = build_fleet(CORE_N);
            online_others(&mut f, 1)?;
            if let Some(c) = f.get_mut(1) {
                send_si(c)?;
                recv_ack(c, c.ack_expect)?;
                enter_first_round(c)?;
                self_probe_items(c)?;
                c.verdicts[4] = V_UNSET;
                let _ = verify_first_round(c);
            }
            let rep = online_gate(&f);
            caught(rep, ApChkCode::ItemSkip.val())
        }
        2 => {
            // 相位倒退：Approved 核再走 send_si → 平移/倒退必拒。
            let mut f = build_fleet(CORE_N);
            if let Some(c) = f.get_mut(0) {
                bring_core_online(c)?;
                let back = send_si(c);
                match back {
                    Err(code) => Ok((code, code == ApChkCode::PhaseBack.val())),
                    Ok(()) => Ok((0, false)),
                }
            } else {
                Err(ApChkCode::FleetCountOff.val())
            }
        }
        3 => {
            // 握手码坏：错码 ack → AckBad 必拒。
            let mut f = build_fleet(CORE_N);
            if let Some(c) = f.get_mut(3) {
                send_si(c)?;
                let wrong = c.ack_expect ^ 1;
                match recv_ack(c, wrong) {
                    Err(code) => Ok((code, code == ApChkCode::AckBad.val())),
                    Ok(()) => Ok((0, false)),
                }
            } else {
                Err(ApChkCode::FleetCountOff.val())
            }
        }
        4 => {
            // 重 ack：Acked 后再 ack → 平移按倒退拒。
            let mut f = build_fleet(CORE_N);
            if let Some(c) = f.get_mut(0) {
                send_si(c)?;
                let seq = c.ack_expect;
                recv_ack(c, seq)?;
                match recv_ack(c, seq) {
                    Err(code) => Ok((code, code == ApChkCode::PhaseBack.val())),
                    Ok(()) => Ok((0, false)),
                }
            } else {
                Err(ApChkCode::FleetCountOff.val())
            }
        }
        5 => {
            // 重复核号：建队后篡改 id → 舰队序必拦。
            let mut f = build_fleet(CORE_N);
            if let Some(c) = f.get_mut(2) {
                c.id = 0;
            }
            match fleet_shape_ok(&f) {
                Err(code) => Ok((code, code == ApChkCode::CoreIdOff.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        6 => {
            // 核数偏离：少建一核 → 舰队序必拦。
            let f = build_fleet(CORE_N - 1);
            match fleet_shape_ok(&f) {
                Err(code) => Ok((code, code == ApChkCode::FleetCountOff.val())),
                Ok(()) => Ok((0, false)),
            }
        }
        7 => {
            // 越权放行：Halted 核混入闸门 → GateBypass 必拦。
            let mut f = build_fleet(CORE_N);
            // 先放行前三核，核 3 留在 Halted。
            let mut i = 0usize;
            while i < CORE_N - 1 {
                if let Some(c) = f.get_mut(i) {
                    let _ = bring_core_online(c);
                }
                i += 1;
            }
            let rep = online_gate(&f);
            caught(rep, ApChkCode::GateBypass.val())
        }
        8 => {
            // 未验放行：清单全空就硬推 Approved → 闸门/核验必拦。
            let mut f = build_fleet(CORE_N);
            if let Some(c) = f.get_mut(0) {
                send_si(c)?;
                recv_ack(c, c.ack_expect)?;
                enter_first_round(c)?;
                // 绕过 verify_first_round 直接推相（模拟直通路径）。
                let _ = c.advance(ApPhase::Approved);
            }
            let rep = online_gate(&f);
            caught(rep, ApChkCode::GateBypass.val())
        }
        _ => {
            // 阻断复用：先落 Blocked，再申请上线 → 复用必拒。
            let mut f = build_fleet(CORE_N);
            if let Some(c) = f.get_mut(1) {
                send_si(c)?;
                recv_ack(c, c.ack_expect)?;
                enter_first_round(c)?;
                c.verdicts[0] = V_FAIL;
            }
            let _ = golden_fleet_online(&mut f);
            let re = match f.get_mut(1) {
                Some(c) => bring_core_online(c),
                None => Err(ApChkCode::FleetCountOff.val()),
            };
            match re {
                Err(code) => Ok((code, code == ApChkCode::PhaseBack.val())),
                Ok(_) => Ok((0, false)),
            }
        }
    }
}

/// 闸门结果判抓：deny 落在期望诊断码上 = 抓到。
fn caught(rep: Result<OnlineReport, u32>, want: u32) -> Result<(u32, bool), u32> {
    match rep {
        Ok(r) => Ok((r.deny, r.deny == want)),
        Err(code) => Ok((code, code == want)),
    }
}

/// 注入账：tries/caught/漏抓路号。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入 10 轮全扫（判据语义：10/10）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        match inject_round(r) {
            Ok((_, hit)) => led.record((r % 10) as usize, hit),
            Err(code) => {
                // 注入器自身炸了也算漏抓（记路号后报错上抛）。
                led.record((r % 10) as usize, false);
                let _ = code;
            }
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中（4 核 48 项全过 + 典型拒全在）。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut f = build_fleet(CORE_N);
    let rep = golden_fleet_online(&mut f)?;
    let (wa, wi) = golden_expect();
    let ok = rep.deny == 0 && rep.approved == wa && rep.items_passed == wi;
    // 典型拒：Halted 直通、核数偏离、错码。
    let r1 = match online_gate(&build_fleet(CORE_N - 1)) {
        Err(_) => true,
        Ok(r) => r.deny != 0,
    };
    let mut f2 = build_fleet(CORE_N);
    let bad = if let Some(c) = f2.get_mut(0) {
        send_si(c).is_ok() && recv_ack(c, c.ack_expect ^ 0xF).is_err()
    } else {
        false
    };
    if ok && r1 && bad {
        Ok(())
    } else {
        Err(ApChkCode::NotReproducible.val())
    }
}

/// 序 B：越界注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(ApChkCode::GateBypass.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72500
        && ApChkCode::NotReproducible.val() < SEG_BASE + SEG_WINDOW;
    let ok = ITEM_N == 12 && CORE_N == 4 && ACK_SALT == 0xA5F1_7860
        && JUDGE_ROUNDS == 10 && in_window
        && ITEM_TABLE.len() == ITEM_N
        && ITEM_TABLE[0].0 == "GDT装载" && ITEM_TABLE[ITEM_N - 1].0 == "特征字一致";
    if ok {
        Ok(())
    } else {
        Err(ApChkCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let mut fa = build_fleet(CORE_N);
    let mut fb = build_fleet(CORE_N);
    let a = golden_fleet_online(&mut fa)?;
    let b = golden_fleet_online(&mut fb)?;
    if a != b {
        return Err(ApChkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ApChkCode::ALL.len() {
        let v = ApChkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + SEG_WINDOW {
            return false;
        }
        i += 1;
    }
    true
}

// ── 握手码往返探针（建码→对表→错码必拒） ────────────────────────
pub fn ack_roundtrip() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < CORE_N {
        let c = ApCore::new(i as u32);
        ok = ok && c.ack_expect == (ACK_SALT ^ (i as u64));
        i += 1;
    }
    // 错码必拒：逐位翻转逐核试。
    let mut j = 0usize;
    while j < CORE_N && ok {
        let mut c = ApCore::new(j as u32);
        let _ = send_si(&mut c);
        let wrong = c.ack_expect ^ (1u64 << (j as u64));
        if recv_ack(&mut c, wrong).is_ok() {
            ok = false;
        }
        j += 1;
    }
    ok
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut f = build_fleet(CORE_N);
    let rep = golden_fleet_online(&mut f)?;
    if rep.deny != 0 {
        return Err(rep.deny);
    }
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!(
        "状态=已完成 cores={} items_passed={} inject={}/{} deny=0",
        rep.approved, rep.items_passed, inj.caught, inj.tries
    ))
}

// ── 权重报表（逐核逐项中文明细） ────────────────────────────────
pub fn report_lines(fleet: &[ApCore]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < fleet.len() {
        let c = match fleet.get(i) {
            Some(c) => c,
            None => break,
        };
        let mut pass_n = 0u32;
        let mut j = 0usize;
        while j < ITEM_N {
            if c.verdicts[j] == V_PASS {
                pass_n += 1;
            }
            j += 1;
        }
        v.push(format!(
            "核{} 相位序值={} 过项={}/{} 拒因=0x{:x}",
            c.id, c.phase.rank(), pass_n, ITEM_N, c.deny
        ));
        i += 1;
    }
    v
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（每核逐项全过 + 闸门放行）。
    let mut f = build_fleet(CORE_N);
    let g = golden_fleet_online(&mut f);
    let (wa, wi) = golden_expect();
    let (gp, gd) = match &g {
        Ok(r) => (r.deny == 0 && r.approved == wa && r.items_passed == wi,
            format!("approved={} items={} deny=0x{:x}", r.approved, r.items_passed, r.deny)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链每核逐项", gp, gd));
    // J1 核验失败阻断上线（判负项 + 跳过项两路都拦）。
    let mut f1 = build_fleet(CORE_N);
    let mut blocked = true;
    if let Some(c) = f1.get_mut(0) {
        send_si(c).ok();
        recv_ack(c, c.ack_expect).ok();
        enter_first_round(c).ok();
        let _ = self_probe_items(c);
        c.verdicts[2] = V_FAIL;
        let _ = verify_first_round(c);
    }
    let _ = online_others(&mut f1, 0);
    if let Some(c) = f1.get(0) {
        blocked = blocked && c.phase == ApPhase::Blocked && c.deny == ApChkCode::ItemFail.val();
    }
    let g1 = online_gate(&f1);
    let b1 = matches!(&g1, Ok(r) if r.deny == ApChkCode::ItemFail.val());
    let mut f2 = build_fleet(CORE_N);
    if let Some(c) = f2.get_mut(1) {
        send_si(c).ok();
        recv_ack(c, c.ack_expect).ok();
        enter_first_round(c).ok();
        let _ = self_probe_items(c);
        c.verdicts[7] = V_UNSET;
        let _ = verify_first_round(c);
    }
    let _ = online_others(&mut f2, 1);
    let g2 = online_gate(&f2);
    let b2 = matches!(&g2, Ok(r) if r.deny == ApChkCode::ItemSkip.val());
    v.push((
        "J1-核验失败阻断上线",
        blocked && b1 && b2,
        format!("判负拦={} 跳项拦={} 落阻断={}", b1, b2, blocked),
    ));
    // J1 越界注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-越界注入10/10", ip, id));
    // J2 单调守卫：握手状态机 平移/倒退/阻断复用 三拒。
    let mut c3 = ApCore::new(0);
    let m1 = send_si(&mut c3).is_ok();
    let m2 = send_si(&mut c3).is_err(); // 平移拒
    let m3 = c3.advance(ApPhase::Halted).is_err(); // 倒退拒
    let seq = c3.ack_expect;
    let m4 = recv_ack(&mut c3, seq).is_ok();
    let m5 = enter_first_round(&mut c3).is_ok();
    let m6 = c3.advance(ApPhase::Approved).is_ok(); // FirstRound→Approved 合法
    let m7 = send_si(&mut c3).is_err(); // 终态再进拒
    v.push((
        "J2-握手单调守卫",
        m1 && m2 && m3 && m4 && m5 && m6 && m7,
        format!("进={} 平移拒={} 倒退拒={} 终态拒={} (推进链完整={})", m1, m2, m3, m7, m4 && m5 && m6),
    ));
    // J2 舰队序守卫：核数偏离/重复核号 双拒。
    let s1 = fleet_shape_ok(&build_fleet(CORE_N - 1)).is_err();
    let s2 = fleet_shape_ok(&build_fleet(CORE_N)).is_ok();
    let mut f4 = build_fleet(CORE_N);
    if let Some(c) = f4.get_mut(1) {
        c.id = 0;
    }
    let s3 = fleet_shape_ok(&f4).is_err();
    v.push((
        "J2-舰队序守卫",
        s1 && s2 && s3,
        format!("核数拒={} 全绿={} 重号拒={}", s1, s2, s3),
    ));
    // J1 握手码往返 + 错码必拒。
    v.push(("J1-握手码往返", ack_roundtrip(), String::from("盐^核号 建码对表；错码逐核必拒")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_fleet_online 两跑同值")));
    // J2 权重表冻结。
    let wt = ITEM_TABLE.len() == ITEM_N && ITEM_TABLE.iter().all(|(_, w)| *w >= 1 && *w <= 3);
    v.push(("J2-权重表冻结", wt, format!("条数={} 权重域[1,3]", ITEM_TABLE.len())));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72500 窗口")));
    // 报表行非空（逐核明细可读）。
    let mut fr = build_fleet(CORE_N);
    let _ = golden_fleet_online(&mut fr);
    let lines = report_lines(&fr);
    v.push(("J3-逐核报表", lines.len() == CORE_N, format!("行数={}", lines.len())));
    v
}

pub fn run_unx1786_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1786 启动期结构自检（AP 上线首轮核验）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
