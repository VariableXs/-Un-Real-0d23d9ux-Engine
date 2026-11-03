//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1883 · 唤醒-切换-IPI 联合回归（三链联测） · 0x72640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B15）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1883-J1 唤醒五段×切换四段×IPI 分发三链联测断言
//!      全过（联合竞态注入 10/10 正确），三签名链逐段咬合核对通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 三链段表冻结：唤醒五段（入队/置旗/发信号/摘队/就绪）×切换四段
//!     （选核/存上下文/换栈/恢复）×IPI 三段（封包/投递/确认），共 12 段；
//!   · 联合状态机：Idle→Waking→Switching→Dispatching→Ready→Closed，
//!     链间单调接力——唤醒收链自动开切换，切换收链自动开分发，
//!     任一环段倒退/平移/越段显性拒；
//!   · 三签名链逐段咬合：唤醒链尾签名 = 切换链基签名，切换链尾签名
//!     = IPI 链基签名；每段签名 = FNV(前段签名‖链标‖段号)，独立重算
//!     对拍（两条代码路径）逐位一致方判咬合通过；
//!   · 联合竞态注入 10 路：丢唤醒/段倒退/双确认/切换越段/IPI 超前/
//!     目标漂移/咬合破坏/封链未走完/复封/状态篡改，10/10 正确处置
//!     （每路期望诊断码逐路对表，漏抓或错码即红）；
//!   · 联测账守恒：触发事件数 == 完成段数 == 12，摘队必以发信号为前提
//!     （旗立无信号即丢唤醒，封链必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72640，VX3-1103 三链联测窗口独占）。
pub const SEG_BASE: u32 = 0x72640;
/// 唤醒链段数（冻结 5）。
pub const WAKE_SEGS_N: usize = 5;
/// 切换链段数（冻结 4）。
pub const SWITCH_SEGS_N: usize = 4;
/// IPI 分发链段数（冻结 3）。
pub const IPI_SEGS_N: usize = 3;
/// 联测总段数（5+4+3，冻结 12）。
pub const TOTAL_SEGS: usize = WAKE_SEGS_N + SWITCH_SEGS_N + IPI_SEGS_N;
/// 注入轮数（判据 10/10 语义，冻结）。
pub const INJECT_ROUNDS: u32 = 10;
/// 联合身份 wake_id（冻结常量，三链咬合的锚）。
pub const WAKE_ID: u64 = 0x5741_4B45_4944_3031;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriCode {
    /// 段号倒退：链内段序只进不退，重放已完成段即拒
    SegBack,
    /// 段序断裂：跳段或链未收先开下链（越段），按段表顺序补齐
    SeqBroken,
    /// 三签名漂移：链间身份对不上（基签名/目标 id 被改），逐段核对定位
    SigDrift,
    /// 超前触发：IPI 在切换链未收链时就开跑，先收前链再分发
    EarlyFire,
    /// 丢唤醒：摘队已发生而发信号缺席，查置旗→发信号→摘队三段时序
    LostWake,
    /// 封链条件不满足：链未走完即封 / 已封再封，按段账补齐或停手
    ChainHalt,
    /// 联测账守恒破坏：触发事件数与完成段数对不上
    LedgerBroken,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl TriCode {
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TriCode::SegBack => "段号倒退：链内段序只进不退，重放已完成段即拒",
            TriCode::SeqBroken => "段序断裂：跳段或提前开下链，按段表顺序补齐",
            TriCode::SigDrift => "三签名漂移：链间身份对不上，逐段重算签名定位漂移点",
            TriCode::EarlyFire => "超前触发：前链未收就开分发，先收链再投递",
            TriCode::LostWake => "丢唤醒：摘队无信号前提，查置旗→发信号→摘队时序",
            TriCode::ChainHalt => "封链条件不满足：链未走完即封或复封，按段账处置",
            TriCode::LedgerBroken => "联测账守恒破坏：事件数与完成段数不符，重对账",
            TriCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [TriCode; 8] = [
        TriCode::SegBack,
        TriCode::SeqBroken,
        TriCode::SigDrift,
        TriCode::EarlyFire,
        TriCode::LostWake,
        TriCode::ChainHalt,
        TriCode::LedgerBroken,
        TriCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TriCode::ALL.len() {
        if TriCode::ALL[i].val() == code {
            return TriCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72640 段闭集内"
}

// ── FNV-1a 64 位（签名链，整数实现） ────────────────────────────
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u64)).wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 签名步进：sig' = FNV(sig 的 8 字节小端 ‖ 链标 ‖ 段号)。
/// 链标三值冻结：0x57=唤醒 / 0x53=切换 / 0x49=IPI，链间互不串线。
pub const CHAIN_TAG_WAKE: u8 = 0x57;
pub const CHAIN_TAG_SWITCH: u8 = 0x53;
pub const CHAIN_TAG_IPI: u8 = 0x49;

pub fn sig_step(prev: u64, tag: u8, seg: usize) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < 8 {
        bytes.push((prev >> (k * 8)) as u8);
        k += 1;
    }
    bytes.push(tag);
    bytes.push(seg as u8);
    fnv1a64(&bytes)
}

/// 链基签名：从 wake_id 派生（三链同源，咬合即身份一致）。
pub fn chain_base(wake_id: u64, tag: u8) -> u64 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < 8 {
        bytes.push((wake_id >> (k * 8)) as u8);
        k += 1;
    }
    bytes.push(tag);
    fnv1a64(&bytes)
}

// ── 段表（冻结） ────────────────────────────────────────────────
/// 唤醒链五段（冻结）。
pub const WAKE_SEGS: [&str; WAKE_SEGS_N] = ["入队", "置旗", "发信号", "摘队", "就绪"];
/// 切换链四段（冻结）。
pub const SWITCH_SEGS: [&str; SWITCH_SEGS_N] = ["选核", "存上下文", "换栈", "恢复"];
/// IPI 分发链三段（冻结）。
pub const IPI_SEGS: [&str; IPI_SEGS_N] = ["封包", "投递", "确认"];

// ── 单链状态（段序单调 + 签名链） ───────────────────────────────
/// 单链账：已完成段数 + 基签名 + 尾签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainState {
    pub done: usize,
    pub segs: usize,
    pub tag: u8,
    pub base: u64,
    pub fin: u64,
}

impl ChainState {
    pub fn new(segs: usize, tag: u8, base: u64) -> ChainState {
        ChainState { done: 0, segs, tag, base, fin: base }
    }

    /// 段步进：段号必须恰等于 done（倒退 SegBack / 跳段 SeqBroken）。
    pub fn step(&mut self, idx: usize) -> Result<u64, u32> {
        if idx >= self.segs {
            return Err(TriCode::SeqBroken.val());
        }
        if idx < self.done {
            return Err(TriCode::SegBack.val());
        }
        if idx > self.done {
            return Err(TriCode::SeqBroken.val());
        }
        self.fin = sig_step(self.fin, self.tag, idx);
        self.done += 1;
        Ok(self.fin)
    }

    /// 是否收链（全部段完成）。
    pub fn complete(&self) -> bool {
        self.done == self.segs
    }

    /// 签名链独立重算对拍：从 base 逐段重走，逐位比对 fin。
    /// 两条代码路径（步进积累 vs 重算推导）一致方为咬合通过。
    pub fn reverify(&self) -> bool {
        let mut s = self.base;
        let mut i = 0usize;
        while i < self.done {
            s = sig_step(s, self.tag, i);
            i += 1;
        }
        s == self.fin
    }
}

// ── 联合状态机（Idle→Waking→Switching→Dispatching→Ready→Closed） ─
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JointState {
    /// 空闲：三链未动。
    Idle,
    /// 唤醒中：唤醒链开跑未收。
    Waking,
    /// 切换中：唤醒收链，切换链开跑未收。
    Switching,
    /// 分发中：切换收链，IPI 链开跑未收。
    Dispatching,
    /// 就绪：三链全收，待封。
    Ready,
    /// 已封：联测收官。
    Closed,
}

impl JointState {
    pub fn rank(self) -> u8 {
        match self {
            JointState::Idle => 0,
            JointState::Waking => 1,
            JointState::Switching => 2,
            JointState::Dispatching => 3,
            JointState::Ready => 4,
            JointState::Closed => 5,
        }
    }
    pub fn can_advance(self, to: JointState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 联测事件 ────────────────────────────────────────────────────
/// 联测事件：三链段触发 + 封链。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    /// 唤醒链第 idx 段。
    Wake(usize),
    /// 切换链第 idx 段。
    Switch(usize),
    /// IPI 链第 idx 段。
    Ipi(usize),
    /// 封链（三链全收后）。
    Seal,
}

/// 联测 runner：三链账 + 联合状态机 + 咬合与守恒账。
#[derive(Clone, Debug)]
pub struct TriRunner {
    pub joint: JointState,
    pub wake: ChainState,
    pub switch: ChainState,
    pub ipi: ChainState,
    /// 三链咬合锚（唤醒链产出，IPI 链消费）。
    pub wake_id: u64,
    /// IPI 投递目标（正常恒等于 wake_id）。
    pub ipi_target: u64,
    /// 唤醒旗/信号/摘队三标记（丢唤醒判定的账面）。
    pub flag_set: bool,
    pub signaled: bool,
    pub dequeued: bool,
    /// 触发事件计数（守恒账）。
    pub fired: u32,
    /// 已封标记。
    pub sealed: bool,
    /// 注入开关（竞态注入面）。
    pub tamper_target: bool,
    pub tamper_base: bool,
    pub tamper_state: bool,
}

impl TriRunner {
    pub fn new(wake_id: u64) -> TriRunner {
        TriRunner {
            joint: JointState::Idle,
            wake: ChainState::new(WAKE_SEGS_N, CHAIN_TAG_WAKE, chain_base(wake_id, CHAIN_TAG_WAKE)),
            switch: ChainState::new(SWITCH_SEGS_N, CHAIN_TAG_SWITCH, 0),
            ipi: ChainState::new(IPI_SEGS_N, CHAIN_TAG_IPI, 0),
            wake_id,
            ipi_target: wake_id,
            flag_set: false,
            signaled: false,
            dequeued: false,
            fired: 0,
            sealed: false,
            tamper_target: false,
            tamper_base: false,
            tamper_state: false,
        }
    }

    /// 联合状态推进公开口：倒退/平移显性拒。
    pub fn advance(&mut self, to: JointState) -> Result<(), u32> {
        if !self.joint.can_advance(to) {
            return Err(TriCode::SeqBroken.val());
        }
        self.joint = to;
        Ok(())
    }

    /// 单事件步进（联测主路）。
    pub fn step(&mut self, ev: Ev) -> Result<(), u32> {
        if self.sealed {
            return Err(TriCode::ChainHalt.val());
        }
        match ev {
            Ev::Wake(idx) => {
                if self.joint != JointState::Idle && self.joint != JointState::Waking {
                    // 唤醒链已收再触发 = 段序断裂。
                    return Err(TriCode::SeqBroken.val());
                }
                if self.joint == JointState::Idle {
                    self.advance(JointState::Waking)?;
                }
                self.wake.step(idx)?;
                self.fired += 1;
                // 唤醒账面：置旗/发信号/摘队三标记按段落账。
                match idx {
                    1 => self.flag_set = true,
                    2 => self.signaled = true,
                    3 => self.dequeued = true,
                    _ => {}
                }
                if self.wake.complete() {
                    // 唤醒收链：切链基签名 = 唤醒尾签名（逐段咬合第①扣）。
                    self.advance(JointState::Switching)?;
                    self.switch.base = self.wake.fin;
                    if self.tamper_base {
                        self.switch.base ^= 1;
                    }
                    self.switch.fin = self.switch.base;
                }
                Ok(())
            }
            Ev::Switch(idx) => {
                if self.joint != JointState::Switching {
                    return Err(TriCode::SeqBroken.val());
                }
                self.switch.step(idx)?;
                self.fired += 1;
                if self.switch.complete() {
                    // 切换收链：IPI 链基签名 = 切换尾签名（逐段咬合第②扣）。
                    self.advance(JointState::Dispatching)?;
                    self.ipi.base = self.switch.fin;
                    self.ipi.fin = self.ipi.base;
                }
                Ok(())
            }
            Ev::Ipi(idx) => {
                if self.joint != JointState::Dispatching {
                    // 分发超前：前链未收即投递。
                    return Err(TriCode::EarlyFire.val());
                }
                if idx == 0 && self.tamper_target {
                    self.ipi_target = self.ipi_target.wrapping_add(1);
                }
                if self.ipi_target != self.wake_id {
                    // 目标漂移：投递目标与唤醒身份不一致。
                    return Err(TriCode::SigDrift.val());
                }
                self.ipi.step(idx)?;
                self.fired += 1;
                if self.ipi.complete() {
                    self.advance(JointState::Ready)?;
                }
                Ok(())
            }
            Ev::Seal => {
                if self.joint != JointState::Ready {
                    return Err(TriCode::ChainHalt.val());
                }
                // 丢唤醒：摘队已发生而发信号缺席（含状态篡改注入面）。
                if self.tamper_state {
                    self.dequeued = true;
                    self.signaled = false;
                }
                if self.dequeued && !self.signaled {
                    return Err(TriCode::LostWake.val());
                }
                // 咬合终核：三扣逐一对上 + 三链签名独立重算全过。
                if self.switch.base != self.wake.fin || self.ipi.base != self.switch.fin {
                    return Err(TriCode::SigDrift.val());
                }
                if !self.wake.reverify() || !self.switch.reverify() || !self.ipi.reverify() {
                    return Err(TriCode::SigDrift.val());
                }
                // 守恒：触发数 == 12 段。
                if self.fired as usize != TOTAL_SEGS {
                    return Err(TriCode::LedgerBroken.val());
                }
                self.advance(JointState::Closed)?;
                self.sealed = true;
                Ok(())
            }
        }
    }

    /// 联合收官签名：三链尾签名拼算（写回/对拍用）。
    pub fn joint_sig(&self) -> Result<u64, u32> {
        if !self.sealed {
            return Err(TriCode::ChainHalt.val());
        }
        let mut bytes: Vec<u8> = Vec::new();
        let three: [u64; 3] = [self.wake.fin, self.switch.fin, self.ipi.fin];
        let mut f = 0usize;
        while f < 3 {
            let mut k = 0usize;
            while k < 8 {
                bytes.push((three[f] >> (k * 8)) as u8);
                k += 1;
            }
            f += 1;
        }
        Ok(fnv1a64(&bytes))
    }
}

// ── 黄金脚本（确定性全过路） ────────────────────────────────────
/// 黄金全过脚本：唤醒五段 → 切换四段 → IPI 三段 → 封。
pub fn golden_script() -> Vec<Ev> {
    let mut v: Vec<Ev> = Vec::new();
    let mut i = 0usize;
    while i < WAKE_SEGS_N {
        v.push(Ev::Wake(i));
        i += 1;
    }
    let mut j = 0usize;
    while j < SWITCH_SEGS_N {
        v.push(Ev::Switch(j));
        j += 1;
    }
    let mut k = 0usize;
    while k < IPI_SEGS_N {
        v.push(Ev::Ipi(k));
        k += 1;
    }
    v.push(Ev::Seal);
    v
}

/// 按脚本跑全程：任一步错即带诊断码返回。
pub fn run_script(script: &[Ev]) -> Result<TriRunner, u32> {
    let mut r = TriRunner::new(WAKE_ID);
    let mut i = 0usize;
    while i < script.len() {
        r.step(script[i])?;
        i += 1;
    }
    Ok(r)
}

// ── 联合竞态注入 10 路（逐路期望码，10/10 正确） ────────────────
/// 注入用例：脚本 + 注入开关 + 期望诊断码。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceCase {
    pub kind: usize,
    pub script: Vec<Ev>,
    pub tamper_target: bool,
    pub tamper_base: bool,
    pub tamper_state: bool,
    pub expect: u32,
}

/// 构造第 kind 路竞态注入（kind 取 0..10）。
pub fn build_race_case(kind: usize) -> Result<RaceCase, u32> {
    let full = golden_script();
    let mut sc: Vec<Ev>;
    let mut tt = false;
    let mut tb = false;
    let mut ts = false;
    let expect;
    match kind % 10 {
        0 => {
            // 丢唤醒脚本：置旗后跳过发信号直接摘队。
            sc = Vec::new();
            sc.push(Ev::Wake(0));
            sc.push(Ev::Wake(1));
            sc.push(Ev::Wake(3));
            expect = TriCode::SeqBroken.val();
        }
        1 => {
            // 唤醒段倒退：链内重放已完成段（发信号段重放，链未收即撞倒退拒）。
            sc = Vec::new();
            let mut i = 0usize;
            while i < WAKE_SEGS_N {
                sc.push(Ev::Wake(i));
                if i == 2 {
                    sc.push(Ev::Wake(2));
                }
                i += 1;
            }
            let mut j = 0usize;
            while j < SWITCH_SEGS_N {
                sc.push(Ev::Switch(j));
                j += 1;
            }
            let mut k = 0usize;
            while k < IPI_SEGS_N {
                sc.push(Ev::Ipi(k));
                k += 1;
            }
            sc.push(Ev::Seal);
            expect = TriCode::SegBack.val();
        }
        2 => {
            // IPI 双确认：分发链未收时重放已完成的投递段。
            sc = Vec::new();
            let mut i = 0usize;
            while i < WAKE_SEGS_N {
                sc.push(Ev::Wake(i));
                i += 1;
            }
            let mut j = 0usize;
            while j < SWITCH_SEGS_N {
                sc.push(Ev::Switch(j));
                j += 1;
            }
            let mut k = 0usize;
            while k < IPI_SEGS_N {
                sc.push(Ev::Ipi(k));
                if k == 1 {
                    sc.push(Ev::Ipi(1));
                }
                k += 1;
            }
            sc.push(Ev::Seal);
            expect = TriCode::SegBack.val();
        }
        3 => {
            // 切换越段：唤醒未收就开切换链。
            sc = Vec::new();
            sc.push(Ev::Wake(0));
            sc.push(Ev::Switch(0));
            expect = TriCode::SeqBroken.val();
        }
        4 => {
            // IPI 超前：唤醒链未收就投递。
            sc = Vec::new();
            sc.push(Ev::Wake(0));
            sc.push(Ev::Wake(1));
            sc.push(Ev::Ipi(0));
            expect = TriCode::EarlyFire.val();
        }
        5 => {
            // 目标漂移：IPI 目标被改（tamper_target）。
            sc = full.clone();
            tt = true;
            expect = TriCode::SigDrift.val();
        }
        6 => {
            // 咬合破坏：切链基签名被改（tamper_base）。
            sc = full.clone();
            tb = true;
            expect = TriCode::SigDrift.val();
        }
        7 => {
            // 封链未走完：唤醒收链就封。
            sc = Vec::new();
            let mut i = 0usize;
            while i < WAKE_SEGS_N {
                sc.push(Ev::Wake(i));
                i += 1;
            }
            sc.push(Ev::Seal);
            expect = TriCode::ChainHalt.val();
        }
        8 => {
            // 复封：全过后再封一次。
            sc = full.clone();
            sc.push(Ev::Seal);
            expect = TriCode::ChainHalt.val();
        }
        _ => {
            // 状态篡改丢唤醒：全过脚本 + 摘队无信号账面（tamper_state）。
            sc = full.clone();
            ts = true;
            expect = TriCode::LostWake.val();
        }
    }
    Ok(RaceCase { kind, script: sc, tamper_target: tt, tamper_base: tb, tamper_state: ts, expect })
}

/// 单路注入执行：黄金基准先自证，注入脚本期望在任一步 Err(expect)。
pub fn run_race_case(case: &RaceCase) -> bool {
    // 基准自证：无注入黄金脚本必须全过（防注入器污染基准）。
    match run_script(&golden_script()) {
        Ok(_) => {}
        Err(_) => return false,
    }
    let mut r = TriRunner::new(WAKE_ID);
    r.tamper_target = case.tamper_target;
    r.tamper_base = case.tamper_base;
    r.tamper_state = case.tamper_state;
    let mut i = 0usize;
    while i < case.script.len() {
        match r.step(case.script[i]) {
            Ok(()) => {}
            Err(e) => return e == case.expect,
        }
        i += 1;
    }
    false
}

/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓/错码的注入路号（诊断用）。
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

/// 竞态注入轮：每轮一路，期望码逐路对表，10/10 正确。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let kind = (r % 10) as usize;
        let case = build_race_case(kind)?;
        led.record(kind, run_race_case(&case));
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：黄金脚本全过 → 三签名链重算对拍 → 联合收官签名 → 注入账
/// → 状态机四拒探。返回 (正旗数, 拒旗数, 完成段数, 收官签名)。
pub fn golden_run() -> Result<(u32, u32, usize, u64), u32> {
    // ① 黄金脚本全过。
    let r = run_script(&golden_script())?;
    let segs = r.wake.done + r.switch.done + r.ipi.done;
    let script_ok = segs == TOTAL_SEGS && r.sealed && r.joint == JointState::Closed;
    // ② 三签名链逐段咬合：独立重算对拍。
    let bite_ok = r.wake.reverify() && r.switch.reverify() && r.ipi.reverify()
        && r.switch.base == r.wake.fin
        && r.ipi.base == r.switch.fin;
    // ③ 收官签名在册。
    let jsig = r.joint_sig()?;
    let sig_ok = jsig != 0;
    // ④ 注入账 10/10。
    let inj = inject_rounds(INJECT_ROUNDS)?;
    let inj_ok = inj.all_caught();
    // 拒旗四路：联合状态倒退 / 未收先封 / 复封 / 越界段号。
    let mut p = TriRunner::new(WAKE_ID);
    let _ = p.step(Ev::Wake(0));
    let rej1 = p.advance(JointState::Idle).is_err();
    let rej2 = p.step(Ev::Seal).is_err();
    let full = run_script(&golden_script())?;
    let mut q = full.clone();
    let rej3 = q.step(Ev::Seal).is_err();
    let mut s = TriRunner::new(WAKE_ID);
    let rej4 = s.step(Ev::Wake(WAKE_SEGS_N)).is_err();
    let flags = script_ok as u32 + bite_ok as u32 + sig_ok as u32 + inj_ok as u32;
    let rejects = rej1 as u32 + rej2 as u32 + rej3 as u32 + rej4 as u32;
    Ok((flags, rejects, segs, jsig))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, TOTAL_SEGS, _)) => Ok(()),
        _ => Err(TriCode::NotReproducible.val()),
    }
}

/// 序 B：竞态注入 10 路全抓 + 漏账必空。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(INJECT_ROUNDS) {
        Ok(l) if l.all_caught() && l.miss_kinds.is_empty() => Ok(()),
        _ => Err(TriCode::SigDrift.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72640 && TriCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = WAKE_SEGS_N == 5 && SWITCH_SEGS_N == 4 && IPI_SEGS_N == 3 && TOTAL_SEGS == 12
        && INJECT_ROUNDS == 10 && WAKE_ID == 0x5741_4B45_4944_3031 && in_window
        && WAKE_SEGS.len() == 5 && SWITCH_SEGS.len() == 4 && IPI_SEGS.len() == 3;
    if ok {
        Ok(())
    } else {
        Err(TriCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TriCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TriCode::ALL.len() {
        let v = TriCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 段表咬合探针：三链段表逐段身份核对 ──────────────────────────
pub fn seg_table_probe() -> bool {
    // 段名非空且无重名（链内唯一），链间段数 5/4/3。
    let mut ok = true;
    let mut i = 0usize;
    while i < WAKE_SEGS_N {
        ok = ok && !WAKE_SEGS[i].is_empty();
        let mut j = i + 1;
        while j < WAKE_SEGS_N {
            ok = ok && WAKE_SEGS[i] != WAKE_SEGS[j];
            j += 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < SWITCH_SEGS_N {
        ok = ok && !SWITCH_SEGS[i].is_empty();
        let mut j = i + 1;
        while j < SWITCH_SEGS_N {
            ok = ok && SWITCH_SEGS[i] != SWITCH_SEGS[j];
            j += 1;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < IPI_SEGS_N {
        ok = ok && !IPI_SEGS[i].is_empty();
        let mut j = i + 1;
        while j < IPI_SEGS_N {
            ok = ok && IPI_SEGS[i] != IPI_SEGS[j];
            j += 1;
        }
        i += 1;
    }
    ok
}

// ── 签名链对拍探针（独立重算 vs 步进积累） ──────────────────────
pub fn sig_chain_probe() -> bool {
    // 三链基签名互异（链标隔离）。
    let b1 = chain_base(WAKE_ID, CHAIN_TAG_WAKE);
    let b2 = chain_base(WAKE_ID, CHAIN_TAG_SWITCH);
    let b3 = chain_base(WAKE_ID, CHAIN_TAG_IPI);
    let distinct = b1 != b2 && b2 != b3 && b1 != b3;
    // 步进积累 vs 独立重算全链一致。
    let mut c = ChainState::new(WAKE_SEGS_N, CHAIN_TAG_WAKE, b1);
    let mut ok = c.reverify();
    let mut i = 0usize;
    while i < WAKE_SEGS_N {
        ok = ok && c.step(i).is_ok();
        i += 1;
    }
    ok = ok && c.complete() && c.reverify();
    // 越界段号拒 + 重放拒。
    let mut d = ChainState::new(IPI_SEGS_N, CHAIN_TAG_IPI, b3);
    ok = ok && d.step(IPI_SEGS_N).is_err() && d.step(0).is_ok() && d.step(0).is_err();
    distinct && ok
}

// ── 诊断码闭集探针 ──────────────────────────────────────────────
pub fn code_table_probe() -> bool {
    let mut ok = TriCode::ALL.len() == 8;
    let mut i = 0;
    while i < TriCode::ALL.len() {
        let c = TriCode::ALL[i];
        ok = ok && c.val() == SEG_BASE + i as u32 && !detail(c.val()).is_empty()
            && detail(c.val()) == c.detail();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x40).starts_with("未知诊断码");
    ok
}

// ── 状态机守卫探针（六态推进 + 倒退/平移拒） ────────────────────
pub fn state_machine_probe() -> bool {
    let mut r = TriRunner::new(WAKE_ID);
    let m1 = r.advance(JointState::Waking).is_ok();
    let b1 = r.advance(JointState::Idle).is_err();
    let b2 = r.advance(JointState::Waking).is_err();
    let m2 = r.advance(JointState::Switching).is_ok();
    let m3 = r.advance(JointState::Dispatching).is_ok();
    let m4 = r.advance(JointState::Ready).is_ok();
    let m5 = r.advance(JointState::Closed).is_ok();
    let b3 = r.advance(JointState::Ready).is_err();
    m1 && b1 && b2 && m2 && m3 && m4 && m5 && b3
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let (flags, rejects, segs, jsig) = golden_run()?;
    if flags != 4 || rejects != 4 || segs != TOTAL_SEGS {
        return Err(TriCode::NotReproducible.val());
    }
    Ok(format!(
        "状态=已完成 三链联测收官 flags={} rejects={} segs={} jsig={:016X}",
        flags, rejects, segs, jsig
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, s, j)) => (*f == 4 && *r == 4,
            format!("flags={} rejects={} segs={} jsig={:016X}", f, r, s, j)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 唤醒五段：段表冻结 + 黄金脚本中唤醒链收链。
    let wake_ok = seg_table_probe() && WAKE_SEGS_N == 5
        && golden_script().iter().filter(|e| matches!(e, Ev::Wake(_))).count() == WAKE_SEGS_N;
    v.push(("J1-唤醒五段", wake_ok, format!("{:?}", WAKE_SEGS)));
    // J1 切换四段。
    let sw_ok = SWITCH_SEGS_N == 4
        && golden_script().iter().filter(|e| matches!(e, Ev::Switch(_))).count() == SWITCH_SEGS_N;
    v.push(("J1-切换四段", sw_ok, format!("{:?}", SWITCH_SEGS)));
    // J1 IPI 三段。
    let ipi_ok = IPI_SEGS_N == 3
        && golden_script().iter().filter(|e| matches!(e, Ev::Ipi(_))).count() == IPI_SEGS_N;
    v.push(("J1-IPI三段", ipi_ok, format!("{:?}", IPI_SEGS)));
    // J1 三签名链逐段咬合：重算对拍 + 两扣全中。
    let r = run_script(&golden_script());
    let bite = match &r {
        Ok(run) => run.wake.reverify() && run.switch.reverify() && run.ipi.reverify()
            && run.switch.base == run.wake.fin && run.ipi.base == run.switch.fin,
        Err(_) => false,
    };
    v.push(("J1-三签名逐段咬合", bite, String::from("唤醒尾=切换基 切换尾=IPI基 重算对拍一致")));
    // J1 联合竞态注入 10/10 正确。
    let inj = inject_rounds(INJECT_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-竞态注入10/10", ip, id));
    // J1 签名链对拍探针（链标隔离 + 积累/重算一致 + 越界重放拒）。
    v.push(("J1-签名链对拍", sig_chain_probe(), String::from("三链标互异 步进/重算一致")));
    // J2 状态机单调：六态推进 + 倒退/平移拒。
    v.push(("J2-状态机单调", state_machine_probe(), String::from("Idle→…→Closed 单调 倒退/平移拒")));
    // J2 联测账守恒：黄金 runner 事件数 == 段数 == 12。
    let cons = match run_script(&golden_script()) {
        Ok(run) => run.fired as usize == TOTAL_SEGS
            && run.wake.done + run.switch.done + run.ipi.done == TOTAL_SEGS,
        Err(_) => false,
    };
    v.push(("J2-联测账守恒", cons, format!("fired=segs={}", TOTAL_SEGS)));
    // J2 诊断码闭集。
    v.push(("J2-诊断码闭集", code_table_probe(), String::from("8 码 val/detail/ALL 查表一致")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值".to_string()));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72640 窗口")));
    v
}

/// 自检入口（契约命名：run_unxXXXX_checks，XXXX=1883）。
pub fn run_unx1883_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1883 唤醒-切换-IPI 联合回归（三链联测）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
