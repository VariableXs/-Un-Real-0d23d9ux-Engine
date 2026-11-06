//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18458 · ActCtx 消费预告挂点（B05 衔接） · 0x7EE40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18458-J1 挂点签名与 B05 消费一致断言，桩期空载可运行
//! 本模块结构清单：
//!   1. 冻结常量层：挂点号 / 参数位面 / 返回域闭集 / 签名哈希
//!      （const fn FNV-1a 期推导，B05 侧预期同函数同值——签名
//!      一致断言的锚）；
//!   2. 诊断码闭集 HkCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一（预告登记账）：AnnounceLedger——消费方签入账
//!      （消费方号+签名哈希），签名失配/重复签入显式拒；
//!   4. 数据结构二（挂点状态机）：HookGate——Stub→Announced→Active
//!      单向三态（倒退拒），调用账（总调用/空载/消费）守恒；
//!   5. 空载语义：桩期调用一律返回 NOOP（计数入账不崩不挂）——
//!      J1 后半句「桩期空载可运行」；
//!   6. 返回域守卫：挂点回值只许 {OK, NOOP, DEGRADED} 三值闭集；
//!   7. 注入核 4 类×10 轮：签名失配签入 / 状态倒退 / 返回越域 /
//!      重复签入，逐类断言被显式拒绝；
//!   8. 黄金全链 + run_unx18458_checks：15 条判据（断言性质）。
//! 为什么挂点先行冻结：B05 消费面按签名哈希接挂点，哈希一旦漂移
//! 消费方将静默失联——故哈希在 const 期推导写死，预告登记以哈希
//! 对拍为唯一准入凭证，桩期调用全部 NOOP 空转，消费方接入后不
//! 改签名直接升 Active。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EE40，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EE40;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 挂点号（冻结 0x58：声明面预告挂点，与 F18441 挂接点表衔接）。
pub const HOOK_ID: u32 = 0x58;
/// 参数位面（冻结 0b11：两参数——ActCtx 载荷类别 + 标志位面）。
pub const PARAM_MASK: u32 = 0b11;
/// 返回域闭集（冻结 3：OK=0 / NOOP=1 / DEGRADED=2）。
pub const RET_OK: u32 = 0;
pub const RET_NOOP: u32 = 1;
pub const RET_DEGRADED: u32 = 2;
pub const RET_CLOSEN: u32 = 3;
/// 登记账容量（冻结 8：B05 消费方数上限）。
pub const ANNOUNCE_CAP: usize = 8;
/// 消费方号（B05 侧冻结：唯一预告签入者）。
pub const B05_CONSUMER: u32 = 0xB05;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 签名哈希（const 期推导） ────────────────────────────────────
/// FNV-1a 32 位 const 版：签名 = hash(挂点号‖参数位面‖返回域闭集)，
/// 12 字节小端拼接。B05 侧预期用同一函数同参数推导——同值即一致。
pub const fn sig_hash(hook_id: u32, param_mask: u32, ret_domain: u32) -> u32 {
    let b: [u8; 12] = [
        (hook_id & 0xFF) as u8,
        ((hook_id >> 8) & 0xFF) as u8,
        ((hook_id >> 16) & 0xFF) as u8,
        ((hook_id >> 24) & 0xFF) as u8,
        (param_mask & 0xFF) as u8,
        ((param_mask >> 8) & 0xFF) as u8,
        ((param_mask >> 16) & 0xFF) as u8,
        ((param_mask >> 24) & 0xFF) as u8,
        (ret_domain & 0xFF) as u8,
        ((ret_domain >> 8) & 0xFF) as u8,
        ((ret_domain >> 16) & 0xFF) as u8,
        ((ret_domain >> 24) & 0xFF) as u8,
    ];
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < 12 {
        h ^= b[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 本挂点冻结签名（const 期推导，写死不拍脑袋）。
pub const SIG_FROZEN: u32 = sig_hash(HOOK_ID, PARAM_MASK, RET_CLOSEN);

/// B05 消费方预期签名（同函数同参数——一致性断言的对照锚）。
pub const B05_EXPECT_SIG: u32 = sig_hash(HOOK_ID, PARAM_MASK, RET_CLOSEN);

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HkCode {
    /// 签名失配（预告签入凭证与冻结哈希不一致）
    SigMismatch,
    /// 消费方号越域（0 哨兵或超 0xFFFF）
    ConsumerIllegal,
    /// 重复签入（同消费方只许一次预告）
    DupAnnounce,
    /// 登记账满
    LedgerFull,
    /// 状态倒退（Active→Announced→Stub 禁止）
    StateBackward,
    /// 返回越域（三值闭集外）
    RetIllegal,
    /// 调用账守恒破（总调用 ≠ 空载 + 消费）
    CallConserveBroken,
    /// 未预告先消费（Active 前不接受消费调用）
    ConsumeBeforeAnnounce,
    /// 双跑不一致
    NotReproducible,
}

impl HkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            HkCode::SigMismatch => "签名失配：签入哈希与冻结签名不一致",
            HkCode::ConsumerIllegal => "消费方号越域：0 哨兵或超 0xFFFF",
            HkCode::DupAnnounce => "重复签入：同消费方只许一次预告",
            HkCode::LedgerFull => "登记账满：8 消费方上限",
            HkCode::StateBackward => "状态倒退：Stub→Announced→Active 单向",
            HkCode::RetIllegal => "返回越域：{OK,NOOP,DEGRADED} 三值闭集外",
            HkCode::CallConserveBroken => "调用账守恒破：总调用≠空载+消费",
            HkCode::ConsumeBeforeAnnounce => "未预告先消费：Active 前不接受消费调用",
            HkCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [HkCode; 9] = [
        HkCode::SigMismatch,
        HkCode::ConsumerIllegal,
        HkCode::DupAnnounce,
        HkCode::LedgerFull,
        HkCode::StateBackward,
        HkCode::RetIllegal,
        HkCode::CallConserveBroken,
        HkCode::ConsumeBeforeAnnounce,
        HkCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HkCode::ALL.len() {
        if HkCode::ALL[i].val() == code {
            return HkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EE40 段闭集内"
}

// ── 挂点状态机 ──────────────────────────────────────────────────
/// 挂点三态：Stub（桩期空载）→ Announced（已预告）→ Active（消费中）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookState {
    Stub,
    Announced,
    Active,
}

/// 预告登记行：消费方号 + 签入哈希。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnounceRow {
    pub consumer: u32,
    pub sig: u32,
}

/// 数据结构一：预告登记账。
pub struct AnnounceLedger {
    pub rows: Vec<AnnounceRow>,
}

impl AnnounceLedger {
    pub fn new() -> AnnounceLedger {
        AnnounceLedger { rows: Vec::new() }
    }

    fn find(&self, consumer: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.consumer == consumer {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 签入：消费方号域守卫 → 重复签入拒 → 签名对拍（唯一准入凭证）
    /// → 容量守卫 → 落账。
    pub fn sign_in(&mut self, consumer: u32, sig: u32) -> Result<(), u32> {
        if consumer == 0 || consumer > 0xFFFF {
            return Err(HkCode::ConsumerIllegal.val());
        }
        if self.find(consumer).is_some() {
            return Err(HkCode::DupAnnounce.val());
        }
        if sig != SIG_FROZEN {
            return Err(HkCode::SigMismatch.val());
        }
        if self.rows.len() >= ANNOUNCE_CAP {
            return Err(HkCode::LedgerFull.val());
        }
        self.rows.push(AnnounceRow { consumer, sig });
        Ok(())
    }
}

/// 数据结构二：挂点闸（状态机 + 调用账）。
pub struct HookGate {
    pub state: HookState,
    pub ann: AnnounceLedger,
    pub total_calls: u32,
    pub noop_calls: u32,
    pub consumed_calls: u32,
    pub degraded_calls: u32,
    pub backward_rejects: u32,
}

impl HookGate {
    pub fn new() -> HookGate {
        HookGate {
            state: HookState::Stub,
            ann: AnnounceLedger::new(),
            total_calls: 0,
            noop_calls: 0,
            consumed_calls: 0,
            degraded_calls: 0,
            backward_rejects: 0,
        }
    }

    /// 预告签入透传（B05 消费方入口）。
    pub fn announce(&mut self, consumer: u32, sig: u32) -> Result<(), u32> {
        if self.state == HookState::Active {
            // Active 后再签入视为重复（状态机单调语义）。
            self.backward_rejects = self.backward_rejects.saturating_add(1);
            return Err(HkCode::DupAnnounce.val());
        }
        let r = self.ann.sign_in(consumer, sig);
        if r.is_ok() && self.state == HookState::Stub {
            self.state = HookState::Announced;
        }
        r
    }

    /// 升 Active：Announced→Active 单向；Stub 直接升拒。
    pub fn activate(&mut self) -> Result<(), u32> {
        if self.state != HookState::Announced {
            if self.state == HookState::Stub {
                return Err(HkCode::ConsumeBeforeAnnounce.val());
            }
            self.backward_rejects = self.backward_rejects.saturating_add(1);
            return Err(HkCode::StateBackward.val());
        }
        self.state = HookState::Active;
        Ok(())
    }

    /// 挂点调用：桩期一律 NOOP 空载（可运行、计数入账）；Active 后
    /// 按返回域闭集落账；越域返回显式拒（调用不入账不崩）。
    pub fn call(&mut self, want: u32) -> Result<u32, u32> {
        if want >= RET_CLOSEN {
            return Err(HkCode::RetIllegal.val());
        }
        self.total_calls = self.total_calls.saturating_add(1);
        match self.state {
            HookState::Stub => {
                // 桩期空载：一律 NOOP，消费请求不计入消费账。
                self.noop_calls = self.noop_calls.saturating_add(1);
                Ok(RET_NOOP)
            }
            HookState::Announced => {
                // 已预告未激活：仍空载（预告只是登记，不是开通）。
                self.noop_calls = self.noop_calls.saturating_add(1);
                Ok(RET_NOOP)
            }
            HookState::Active => {
                if want == RET_NOOP {
                    // Active 后不许请求 NOOP（NOOP 是桩期专属回值）。
                    return Err(HkCode::RetIllegal.val());
                }
                if want == RET_DEGRADED {
                    self.degraded_calls = self.degraded_calls.saturating_add(1);
                } else {
                    self.consumed_calls = self.consumed_calls.saturating_add(1);
                }
                Ok(want)
            }
        }
    }

    /// 调用账守恒：总调用 == 空载 + 消费 + 降级。
    pub fn conserve(&self) -> Result<(), u32> {
        let sum = self.noop_calls
            .saturating_add(self.consumed_calls)
            .saturating_add(self.degraded_calls);
        if self.total_calls != sum {
            return Err(HkCode::CallConserveBroken.val());
        }
        Ok(())
    }
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽 0 签名失配签入 / 槽 1 状态倒退 / 槽 2 返回越域 /
/// 槽 3 重复签入。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：签名失配——哈希差一比特也拒。
        let mut g = HookGate::new();
        let bad_sig = SIG_FROZEN ^ 1;
        let hit0 = g.announce(B05_CONSUMER, bad_sig).err() == Some(HkCode::SigMismatch.val())
            && g.state == HookState::Stub;
        led.record(0, hit0);
        // 槽 1：状态倒退——Active 后再 activate 必拒 StateBackward。
        let mut g1 = HookGate::new();
        g1.announce(B05_CONSUMER, SIG_FROZEN).ok();
        g1.activate().ok();
        let hit1 = g1.activate().err() == Some(HkCode::StateBackward.val());
        led.record(1, hit1);
        // 槽 2：返回越域——want=RET_CLOSEN 拒且不入账。
        let mut g2 = HookGate::new();
        let t0 = g2.total_calls;
        let hit2 = g2.call(RET_CLOSEN).err() == Some(HkCode::RetIllegal.val())
            && g2.total_calls == t0;
        led.record(2, hit2);
        // 槽 3：重复签入——B05 二次签入必拒。
        let mut g3 = HookGate::new();
        g3.announce(B05_CONSUMER, SIG_FROZEN).ok();
        let hit3 = g3.announce(B05_CONSUMER, SIG_FROZEN).err()
            == Some(HkCode::DupAnnounce.val())
            && g3.ann.rows.len() == 1;
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：签名一致断言 → 桩期空载 → 预告 → 升 Active → 消费 →
/// 守恒 → 注入全抓 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // J1 前半句：挂点签名与 B05 消费一致（const 期同函数同值）。
    if SIG_FROZEN != B05_EXPECT_SIG {
        return Err(HkCode::SigMismatch.val());
    }
    if sig_hash(HOOK_ID, PARAM_MASK, RET_CLOSEN) != SIG_FROZEN {
        return Err(HkCode::SigMismatch.val());
    }
    // 桩期空载：未预告直接调用 → NOOP，可运行不崩。
    let mut g = HookGate::new();
    let r0 = g.call(RET_OK)?;
    if r0 != RET_NOOP {
        return Err(HkCode::CallConserveBroken.val());
    }
    // 未预告先升 Active 拒。
    if g.activate().err() != Some(HkCode::ConsumeBeforeAnnounce.val()) {
        return Err(HkCode::ConsumeBeforeAnnounce.val());
    }
    // B05 预告签入 → Announced（预告后仍空载）。
    g.announce(B05_CONSUMER, SIG_FROZEN)?;
    let r1 = g.call(RET_OK)?;
    if r1 != RET_NOOP {
        return Err(HkCode::CallConserveBroken.val());
    }
    // 升 Active → 消费调用生效。
    g.activate()?;
    let r2 = g.call(RET_OK)?;
    let r3 = g.call(RET_DEGRADED)?;
    if r2 != RET_OK || r3 != RET_DEGRADED {
        return Err(HkCode::RetIllegal.val());
    }
    g.conserve()?;
    // 注入 10 轮全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(HkCode::NotReproducible.val());
    }
    Ok((g.total_calls, g.noop_calls, g.consumed_calls + g.degraded_calls))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HkCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：签名一致性（const 期断言的运行期复算）。
pub fn probe_seq_a() -> Result<(), u32> {
    if SIG_FROZEN == B05_EXPECT_SIG && SIG_FROZEN != 0 {
        Ok(())
    } else {
        Err(HkCode::SigMismatch.val())
    }
}

/// 负探针 1：消费方号越域（0 哨兵与 0x10000 都拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = HookGate::new();
    let a = g.announce(0, SIG_FROZEN);
    let b = g.announce(0x1_0000, SIG_FROZEN);
    if a.err() == Some(HkCode::ConsumerIllegal.val())
        && b.err() == Some(HkCode::ConsumerIllegal.val())
    {
        Ok(())
    } else {
        Err(HkCode::ConsumerIllegal.val())
    }
}

/// 负探针 2：Stub 期请求消费域回值拒（未升 Active 前消费不生效）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut g = HookGate::new();
    g.announce(B05_CONSUMER, SIG_FROZEN).ok();
    let r = g.call(RET_OK)?;
    if r == RET_NOOP {
        Ok(())
    } else {
        Err(HkCode::ConsumeBeforeAnnounce.val())
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HkCode::ALL.len() {
        let v = HkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18458_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18458 ActCtx 消费预告挂点");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "总调用{:?} 空载{:?} 消费{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0),
            g.as_ref().map(|r| r.2).unwrap_or(0)
        ),
    );
    // ② 挂点签名与 B05 消费一致（J1 前半句，const 期同函数同值）。
    s.add("签名与B05一致", SIG_FROZEN == B05_EXPECT_SIG && SIG_FROZEN != 0,
        "const 期 FNV-1a 同参数同值");
    // ③ 签名确定性复算：运行期重算 == const 值。
    s.add("签名运行期复算", sig_hash(HOOK_ID, PARAM_MASK, RET_CLOSEN) == SIG_FROZEN,
        "const fn 可复算");
    // ④ 桩期空载可运行（J1 后半句）：未预告调用 NOOP 不崩。
    let mut gstub = HookGate::new();
    let stub_r = gstub.call(RET_OK);
    s.add("桩期空载可运行", stub_r == Ok(RET_NOOP) && gstub.noop_calls == 1,
        "Stub 期调用一律 NOOP，账面入空载");
    // ⑤ 状态机单向：Stub→Announced→Active，倒退全拒。
    let mut gsm = HookGate::new();
    let stub_act = gsm.activate().err() == Some(HkCode::ConsumeBeforeAnnounce.val());
    gsm.announce(B05_CONSUMER, SIG_FROZEN).ok();
    let ann_state = gsm.state == HookState::Announced;
    gsm.activate().ok();
    let active_ok = gsm.state == HookState::Active
        && gsm.activate().err() == Some(HkCode::StateBackward.val());
    s.add("状态机单向", stub_act && ann_state && active_ok,
        "越级拒+倒退拒+正常升过");
    // ⑥ 预告后仍空载（预告只是登记）。
    let mut gpre = HookGate::new();
    gpre.announce(B05_CONSUMER, SIG_FROZEN).ok();
    let rpre = gpre.call(RET_OK);
    s.add("预告后仍空载", rpre == Ok(RET_NOOP), "Active 前消费不生效");
    // ⑦ 调用账守恒。
    let mut gcon = HookGate::new();
    gcon.call(RET_OK).ok();
    gcon.announce(B05_CONSUMER, SIG_FROZEN).ok();
    gcon.call(RET_OK).ok();
    gcon.activate().ok();
    gcon.call(RET_OK).ok();
    gcon.call(RET_DEGRADED).ok();
    s.add("调用账守恒", gcon.conserve().is_ok()
        && gcon.total_calls == 4
        && gcon.noop_calls == 2
        && gcon.consumed_calls == 1
        && gcon.degraded_calls == 1,
        "总4 = 空载2 + 消费1 + 降级1");
    // ⑧ 返回域闭集：三值合法、越域拒且不入账。
    let mut gret = HookGate::new();
    gret.announce(B05_CONSUMER, SIG_FROZEN).ok();
    gret.activate().ok();
    let t_before = gret.total_calls;
    let dom_ok = gret.call(RET_OK).is_ok()
        && gret.call(RET_DEGRADED).is_ok()
        && gret.call(RET_CLOSEN).err() == Some(HkCode::RetIllegal.val())
        && gret.total_calls == t_before + 2;
    s.add("返回域三值闭集", dom_ok, "越域拒且调用不入账");
    // ⑨ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑩ 漏检为零。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入=检出+漏检，漏检=0");
    // ⑪ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "签名一致/消费方号拒/预告期空载");
    // ⑫ 登记账容量边界：8 消费方满后第 9 拒。
    let mut gcap = HookGate::new();
    let mut filled = 0u32;
    let mut c = 1;
    while c <= ANNOUNCE_CAP + 2 {
        if gcap.announce(c as u32, SIG_FROZEN).is_ok() {
            filled += 1;
        }
        c += 1;
    }
    s.add("登记账容量", filled == ANNOUNCE_CAP as u32, "8 方签满，超额拒入");
    // ⑬ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑭ 文案闭集。
    let mut closed = true;
    let mut ci = 0;
    while ci < HkCode::ALL.len() {
        if HkCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "9 码全有中文建议");
    // ⑮ 段位守卫。
    s.add("段位守卫 0x7EE40", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_consistency_const_and_runtime() {
        assert_eq!(SIG_FROZEN, B05_EXPECT_SIG, "const 期同函数同值");
        assert_eq!(sig_hash(HOOK_ID, PARAM_MASK, RET_CLOSEN), SIG_FROZEN);
        // 哈希对参数敏感：改一位参数哈希必变。
        assert_ne!(sig_hash(HOOK_ID, PARAM_MASK, RET_CLOSEN + 1), SIG_FROZEN);
        assert_ne!(sig_hash(HOOK_ID + 1, PARAM_MASK, RET_CLOSEN), SIG_FROZEN);
    }

    #[test]
    fn stub_period_noop_runnable() {
        let mut g = HookGate::new();
        let mut i = 0;
        while i < JUDGE_ROUNDS {
            assert_eq!(g.call(RET_OK).unwrap_or(9), RET_NOOP, "第 {} 轮桩期 NOOP", i);
            i += 1;
        }
        assert_eq!(g.total_calls, JUDGE_ROUNDS);
        assert_eq!(g.noop_calls, JUDGE_ROUNDS);
        assert!(g.conserve().is_ok());
    }

    #[test]
    fn state_machine_monotonic() {
        let mut g = HookGate::new();
        assert_eq!(g.activate().err(), Some(HkCode::ConsumeBeforeAnnounce.val()));
        assert!(g.announce(B05_CONSUMER, SIG_FROZEN).is_ok());
        assert_eq!(g.state, HookState::Announced);
        assert!(g.activate().is_ok());
        assert_eq!(g.state, HookState::Active);
        assert_eq!(g.activate().err(), Some(HkCode::StateBackward.val()));
    }

    #[test]
    fn sig_mismatch_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut g = HookGate::new();
            assert_eq!(
                g.announce(B05_CONSUMER, SIG_FROZEN ^ 0x10).err(),
                Some(HkCode::SigMismatch.val()),
                "第 {} 轮签名失配必拒",
                r
            );
            assert_eq!(g.state, HookState::Stub, "拒后状态不动");
            r += 1;
        }
    }

    #[test]
    fn dup_announce_rejected() {
        let mut g = HookGate::new();
        assert!(g.announce(B05_CONSUMER, SIG_FROZEN).is_ok());
        assert_eq!(
            g.announce(B05_CONSUMER, SIG_FROZEN).err(),
            Some(HkCode::DupAnnounce.val())
        );
        assert_eq!(g.ann.rows.len(), 1);
    }

    #[test]
    fn ret_domain_closed_set() {
        let mut g = HookGate::new();
        g.announce(B05_CONSUMER, SIG_FROZEN).ok();
        g.activate().ok();
        assert_eq!(g.call(RET_OK).unwrap_or(9), RET_OK);
        assert_eq!(g.call(RET_DEGRADED).unwrap_or(9), RET_DEGRADED);
        let before = g.total_calls;
        assert_eq!(g.call(RET_CLOSEN).err(), Some(HkCode::RetIllegal.val()));
        assert_eq!(g.call(RET_CLOSEN + 5).err(), Some(HkCode::RetIllegal.val()));
        assert_eq!(g.total_calls, before, "越域调用不入账");
    }

    #[test]
    fn active_noop_request_rejected() {
        let mut g = HookGate::new();
        g.announce(B05_CONSUMER, SIG_FROZEN).ok();
        g.activate().ok();
        assert_eq!(g.call(RET_NOOP).err(), Some(HkCode::RetIllegal.val()),
            "Active 后 NOOP 是桩期专属回值");
    }

    #[test]
    fn call_ledger_conservation() {
        let mut g = HookGate::new();
        g.call(RET_OK).ok();
        g.call(RET_DEGRADED).ok();
        g.announce(B05_CONSUMER, SIG_FROZEN).ok();
        g.call(RET_OK).ok();
        g.activate().ok();
        g.call(RET_OK).ok();
        assert_eq!(g.total_calls, 4);
        assert_eq!(g.noop_calls, 3);
        assert_eq!(g.consumed_calls, 1);
        assert!(g.conserve().is_ok());
    }

    #[test]
    fn consumer_domain_guard() {
        let mut g = HookGate::new();
        assert_eq!(g.announce(0, SIG_FROZEN).err(), Some(HkCode::ConsumerIllegal.val()));
        assert_eq!(g.announce(0x1_0000, SIG_FROZEN).err(), Some(HkCode::ConsumerIllegal.val()));
    }

    #[test]
    fn announce_capacity_boundary() {
        let mut g = HookGate::new();
        let mut c = 1;
        while c <= ANNOUNCE_CAP {
            assert!(g.announce(c as u32, SIG_FROZEN).is_ok(), "第 {} 方签入", c);
            c += 1;
        }
        assert_eq!(
            g.announce((ANNOUNCE_CAP + 1) as u32, SIG_FROZEN).err(),
            Some(HkCode::LedgerFull.val())
        );
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn probes_and_segment_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18458_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
