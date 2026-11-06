//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18466 · RUNASADMIN 标志与提示联动 · 0x7F040段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18466-J1 RUNASADMIN 标志触发提权提示路径断言全过，与
//!       B26 虚拟化开关联动签名一致
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 300 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：ElevState 四态分叉状态机 / PromptRec 提示账（Vec 定容 8）
//!   状态机  ：Absent→Pending→(Accepted|Declined) 终态封闭，倒退必拒
//!   账本    ：提示账（同 exe 一次提示、seq 单调、满后拒新不丢旧）
//!   错误路径：RalCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：5 类坏样本（无标志触发/终态再转/虚拟化不一致/重复提示/签名错）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   联动核  ：B26 虚拟化开关联动签名 v1（提权进程禁虚拟化=开关 0）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F040，AI-VX11-1105 批 A07 卡 F18466 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7F040;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// B26 虚拟化开关联动签名 v1。推导：高 16 位 = B26 工单族码 0xB026，
/// 低 16 位 = 冻结版本 v1 = 0x0001。
pub const LINK_SIG_V1: u32 = 0xB026_0001;
/// 提示账定容（冻结 8 槽：典型进程采样位）。
pub const LEDGER_CAP: usize = 8;
/// exe 路径长度上限（冻结 128）。
pub const EXE_LEN_MAX: usize = 128;
/// 虚拟化开关联动值（冻结 0：提权进程必须禁用虚拟化）。
pub const VIRT_SWITCH_ELEVATED: u8 = 0;

// ── 提权状态机（四态分叉） ──────────────────────────────────────
/// 提权提示状态（分叉封闭：终态不可再转）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElevState {
    /// 无 RUNASADMIN 标志（无提示路径）。
    FlagAbsent = 0,
    /// 提示挂起。
    PromptPending = 1,
    /// 提示被接受（终态）。
    PromptAccepted = 2,
    /// 提示被拒绝（终态）。
    PromptDeclined = 3,
}

impl ElevState {
    /// 层级（Accepted/Declined 同层终态）。
    pub fn tier(self) -> u32 {
        match self {
            ElevState::FlagAbsent => 0,
            ElevState::PromptPending => 1,
            ElevState::PromptAccepted => 2,
            ElevState::PromptDeclined => 2,
        }
    }
    /// 是否终态。
    pub fn terminal(self) -> bool {
        self.tier() >= 2
    }
    /// 合法转移：Absent→Pending（无标志不触发提示路径）；Pending→Accepted
    /// 或 Pending→Declined（终态分叉）；终态再转/倒退/越级一律显式拒。
    pub fn advance(self, next: ElevState) -> Result<ElevState, u32> {
        if self.terminal() {
            return Err(RalCode::StateRegress.code());
        }
        let ok = match self {
            ElevState::FlagAbsent => next == ElevState::PromptPending,
            ElevState::PromptPending => {
                next == ElevState::PromptAccepted || next == ElevState::PromptDeclined
            }
            ElevState::PromptAccepted => false,
            ElevState::PromptDeclined => false,
        };
        if ok {
            Ok(next)
        } else {
            Err(RalCode::StateRegress.code())
        }
    }
    /// 全链：Absent→Pending→Accepted（接受路径）。
    pub fn full_chain_accept() -> Result<ElevState, u32> {
        let s0 = ElevState::FlagAbsent;
        let s1 = s0.advance(ElevState::PromptPending)?;
        s1.advance(ElevState::PromptAccepted)
    }
    /// 全链：Absent→Pending→Declined（拒绝路径）。
    pub fn full_chain_decline() -> Result<ElevState, u32> {
        let s0 = ElevState::FlagAbsent;
        let s1 = s0.advance(ElevState::PromptPending)?;
        s1.advance(ElevState::PromptDeclined)
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RalCode {
    /// exe 路径为空。
    ExeEmpty,
    /// exe 路径超长（128 上限）。
    ExeTooLong,
    /// 无标志触发提示（RUNASADMIN 未设却走提示路径）。
    PromptWithoutFlag,
    /// 状态机倒退/终态再转。
    StateRegress,
    /// 同 exe 重复提示。
    DupPrompt,
    /// 虚拟化开关联动不一致（提权进程虚拟化非 0）。
    VirtSwitchMismatch,
    /// B26 联动签名不匹配。
    LinkSigMismatch,
    /// 提示账满。
    LedgerFull,
    /// 状态机越级（同 StateRegress 之外的流水路径）。
    PhaseRegress,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl RalCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            RalCode::ExeEmpty => "exe 路径为空：提示路径拒收",
            RalCode::ExeTooLong => "exe 路径超长：128 上限",
            RalCode::PromptWithoutFlag => "无标志触发提示：RUNASADMIN 未设",
            RalCode::StateRegress => "状态机倒退：终态封闭只进不退",
            RalCode::DupPrompt => "同 exe 重复提示：一进程一提示",
            RalCode::VirtSwitchMismatch => "虚拟化开关联动不一致：提权必须禁虚拟化",
            RalCode::LinkSigMismatch => "B26 联动签名不匹配：仅 v1 可联动",
            RalCode::LedgerFull => "提示账满：入账被拒旧账保留",
            RalCode::PhaseRegress => "提示流水越级：只进不退",
            RalCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            RalCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [RalCode; 11] = [
        RalCode::ExeEmpty,
        RalCode::ExeTooLong,
        RalCode::PromptWithoutFlag,
        RalCode::StateRegress,
        RalCode::DupPrompt,
        RalCode::VirtSwitchMismatch,
        RalCode::LinkSigMismatch,
        RalCode::LedgerFull,
        RalCode::PhaseRegress,
        RalCode::ConservationBroken,
        RalCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < RalCode::ALL.len() {
        if RalCode::ALL[i].code() == code {
            return RalCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F040 段闭集内"
}

// ── 联动核（B26 虚拟化开关） ────────────────────────────────────
/// 联动签名校验。
pub fn link_sig_ok(sig: u32) -> bool {
    sig == LINK_SIG_V1
}

/// 提权后虚拟化开关联动闸：接受提权 → 开关必须为 0（禁虚拟化）。
pub fn link_check(virt_switch: u8) -> Result<(), u32> {
    if virt_switch == VIRT_SWITCH_ELEVATED {
        Ok(())
    } else {
        Err(RalCode::VirtSwitchMismatch.code())
    }
}

/// exe 路径校验（空/超长两闸）。
pub fn validate_exe(exe: &str) -> Result<(), u32> {
    if exe.len() == 0 {
        return Err(RalCode::ExeEmpty.code());
    }
    if exe.len() > EXE_LEN_MAX {
        return Err(RalCode::ExeTooLong.code());
    }
    Ok(())
}

// ── 提示账（定容 8，同 exe 一次提示） ───────────────────────────
/// 单条提示账目。
#[derive(Clone, Debug)]
pub struct PromptRec {
    pub exe_hash: u32,
    pub seq: u32,
    pub accepted: bool,
}

/// 提示账本。
pub struct PromptLedger {
    recs: Vec<PromptRec>,
    pub full_rejects: u32,
    pub inject_tries: [u32; 5],
    pub inject_caught: [u32; 5],
    next_seq: u32,
}

impl PromptLedger {
    /// 空账本。
    pub fn new() -> PromptLedger {
        PromptLedger {
            recs: Vec::new(),
            full_rejects: 0,
            inject_tries: [0; 5],
            inject_caught: [0; 5],
            next_seq: 1,
        }
    }
    /// 触发提示路径总装：标志闸 → exe 闸 → 状态机 → 联动闸 → 入账。
    /// 返回（终态, 已入账）。无标志 → Ok(FlagAbsent) 且不入账（合法路径）。
    pub fn trigger(
        &mut self,
        flag_present: bool,
        exe: &str,
        accept: bool,
        sig: u32,
        virt_switch: u8,
    ) -> Result<ElevState, u32> {
        validate_exe(exe)?;
        if !flag_present {
            // 无标志：不走提示路径（返回缺席态，显式合法）。
            return Ok(ElevState::FlagAbsent);
        }
        if !link_sig_ok(sig) {
            return Err(RalCode::LinkSigMismatch.code());
        }
        // 状态机推进：Absent→Pending→终态。
        let st = ElevState::FlagAbsent
            .advance(ElevState::PromptPending)?
            .advance(if accept {
                ElevState::PromptAccepted
            } else {
                ElevState::PromptDeclined
            })?;
        // 终态为接受 → 虚拟化联动闸。
        if st == ElevState::PromptAccepted {
            link_check(virt_switch)?;
        }
        // 同 exe 重复提示闸。
        let h = fnv32(exe.as_bytes());
        let mut i = 0;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.exe_hash == h {
                    return Err(RalCode::DupPrompt.code());
                }
            }
            i += 1;
        }
        // 容量闸。
        if self.recs.len() >= LEDGER_CAP {
            self.full_rejects += 1;
            return Err(RalCode::LedgerFull.code());
        }
        let rec = PromptRec {
            exe_hash: h,
            seq: self.next_seq,
            accepted: st == ElevState::PromptAccepted,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.recs.push(rec);
        Ok(st)
    }
    /// 已入账条数。
    pub fn count(&self) -> usize {
        self.recs.len()
    }
    /// seq 单调复核。
    pub fn monotone_seq(&self) -> bool {
        let n = self.recs.len();
        let mut i = 1;
        while i < n {
            let prev = match self.recs.get(i - 1) {
                Some(p) => p.seq,
                None => return false,
            };
            let cur = match self.recs.get(i) {
                Some(c) => c.seq,
                None => return false,
            };
            if cur <= prev {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 接受条数。
    pub fn accepted_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                if r.accepted {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 5 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒（五槽全抓）。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（五槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// FNV-1a 32 位。
pub fn fnv32(bytes: &[u8]) -> u32 {
    let mut h = 0x811C9DC5u32;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        i += 1;
    }
    h
}

// ── 正样本构造（确定性） ────────────────────────────────────────
/// 典型 exe（12 例确定性变体）。
pub fn make_exe(idx: u32) -> String {
    format!("C:\\Program Files\\elev_app_{}.exe", idx % 12)
}

// ── 注入核（5 类坏样本 ×10 轮） ─────────────────────────────────
/// 注入探针：
///   0 无标志触发提示（有标志才触发，这里构造"标志假但提示路径被走"）；
///   1 终态再转；2 虚拟化开关联动不一致；3 同 exe 重复提示；4 签名错。
pub fn inject_probe(slot: usize) -> Result<ElevState, u32> {
    let mut led = PromptLedger::new();
    match slot {
        0 => {
            // 无标志：trigger 必须返回 FlagAbsent（无提示路径），
            // 任何终态都算提示路径被非法触发。
            match led.trigger(false, "C:\\a.exe", true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED) {
                Ok(st) if st == ElevState::FlagAbsent => Err(RalCode::PromptWithoutFlag.code()),
                Ok(_) => Ok(ElevState::PromptAccepted), // 不该到这
                Err(_) => Ok(ElevState::PromptAccepted), // 不该到这
            }
        }
        1 => {
            // 终态再转：Accepted 后任何 advance 必拒（命中 → Err(窗内码)）。
            ElevState::full_chain_accept()?;
            match ElevState::PromptAccepted.advance(ElevState::PromptPending) {
                Err(c) if c == RalCode::StateRegress.code() => Err(c),
                _ => Ok(ElevState::PromptPending),
            }
        }
        2 => {
            // 提权接受 + 虚拟化开关非 0 → 联动不一致必拒。
            led.trigger(true, "C:\\v.exe", true, LINK_SIG_V1, 1)
        }
        3 => {
            // 同 exe 重复提示必拒。
            let _ = led.trigger(true, "C:\\d.exe", true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED)?;
            led.trigger(true, "C:\\d.exe", false, LINK_SIG_V1, VIRT_SWITCH_ELEVATED)
        }
        _ => {
            // 联动签名不匹配必拒。
            led.trigger(true, "C:\\s.exe", true, LINK_SIG_V1.wrapping_add(1), VIRT_SWITCH_ELEVATED)
        }
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：12 正例触发（接受/拒绝/无标志三态轮转）→ 5 类注入
/// 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = PromptLedger::new();
    let mut i = 0u32;
    while i < 12 {
        let exe = make_exe(i);
        let st = match i % 3 {
            0 => led.trigger(true, &exe, true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED)?,
            1 => led.trigger(true, &exe, false, LINK_SIG_V1, VIRT_SWITCH_ELEVATED)?,
            _ => led.trigger(false, &exe, true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED)?,
        };
        // 口径复核：无标志必须返回缺席态；有标志必须返回终态。
        if i % 3 == 2 {
            if st != ElevState::FlagAbsent {
                return Err(RalCode::PromptWithoutFlag.code());
            }
        } else {
            if !st.terminal() {
                return Err(RalCode::StateRegress.code());
            }
        }
        i += 1;
    }
    // 无标志例（4 个）不入账；入账 8 = 提示账恰满（8 例有标志）。
    if led.count() != 8 || !led.monotone_seq() {
        return Err(RalCode::ConservationBroken.code());
    }
    // 接受路径恰 4 条（i%3==0 的 0,3,6,9）。
    if led.accepted_count() != 4 {
        return Err(RalCode::ConservationBroken.code());
    }
    // 注入 5 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 5 {
            let hit = match inject_probe(slot) {
                // 槽 0 的"命中"以 Err 表达（无标志触发被抓后显式返回错）。
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve() {
        return Err(RalCode::ConservationBroken.code());
    }
    Ok((led.count() as u32, led.accepted_count(), led.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 接受路径全链 + 联动闸过。
    let st = ElevState::full_chain_accept()?;
    link_check(VIRT_SWITCH_ELEVATED)?;
    if st == ElevState::PromptAccepted {
        Ok(())
    } else {
        Err(RalCode::StateRegress.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 拒绝路径全链（Declined 终态不需要联动闸）。
    let st = ElevState::full_chain_decline()?;
    if st == ElevState::PromptDeclined && st.terminal() {
        Ok(())
    } else {
        Err(RalCode::StateRegress.code())
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 无标志触发提示必拒（提示路径口径）。
    let mut led = PromptLedger::new();
    match led.trigger(false, "C:\\x.exe", true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED) {
        Ok(st) if st == ElevState::FlagAbsent => Ok(()),
        _ => Err(RalCode::PromptWithoutFlag.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7F040 {
        return false;
    }
    let mut i = 0;
    while i < RalCode::ALL.len() {
        let v = RalCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集复核。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < RalCode::ALL.len() {
        if detail_of(RalCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（12 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-B26联动签名冻结v1",
        LINK_SIG_V1 == 0xB026_0001 && VIRT_SWITCH_ELEVATED == 0,
        format!("sig=0x{:08X}", LINK_SIG_V1),
    ));
    // 双路径状态机。
    let fa = ElevState::full_chain_accept().is_ok();
    let fd = ElevState::full_chain_decline().is_ok();
    v.push((
        "J1-接受/拒绝双路径",
        fa && fd,
        format!("接受{} 拒绝{}", fa, fd),
    ));
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 注入 5 类 ×10 轮。
    let mut led = PromptLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 5 {
            let hit = match inject_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries = led.inject_tries[0]
        + led.inject_tries[1]
        + led.inject_tries[2]
        + led.inject_tries[3]
        + led.inject_tries[4];
    let total_caught = led.inject_caught[0]
        + led.inject_caught[1]
        + led.inject_caught[2]
        + led.inject_caught[3]
        + led.inject_caught[4];
    v.push((
        "J1-注入10/10全抓",
        led.all_caught(),
        format!("5类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", RalCode::ALL.len()),
    ));
    // 容量耗尽：8 槽满后拒新不丢旧。
    let mut led2 = PromptLedger::new();
    let mut i9 = 0u32;
    while i9 < (LEDGER_CAP as u32) + 3 {
        let exe = make_exe(100 + i9);
        let _ = led2.trigger(true, &exe, false, LINK_SIG_V1, VIRT_SWITCH_ELEVATED);
        i9 += 1;
    }
    v.push((
        "J1-提示账容量守恒",
        led2.count() == LEDGER_CAP && led2.full_rejects == 3 && led2.monotone_seq(),
        format!("槽{} 拒{} seq单调", led2.count(), led2.full_rejects),
    ));
    // 边界四类：空/恰 128/129/合法。
    let b0 = validate_exe("").is_err();
    let b1 = validate_exe("x".repeat(EXE_LEN_MAX).as_str()).is_ok();
    let b2 = validate_exe("x".repeat(EXE_LEN_MAX + 1).as_str()).is_err();
    let b3 = link_check(VIRT_SWITCH_ELEVATED).is_ok() && link_check(1).is_err();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("空{} 满{} 超{} 联动{}", b0, b1, b2, b3),
    ));
    // 联动签名闭集：v1 过 / v1±1 拒。
    let b4 = link_sig_ok(LINK_SIG_V1) && !link_sig_ok(LINK_SIG_V1.wrapping_add(1));
    v.push((
        "J1-联动签名闭集",
        b4,
        String::from("v1 过 邻值拒"),
    ));
    // 双跑一致。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 13 条）。
pub fn run_unx18466_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18466 RUNASADMIN 标志与提示联动");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18466_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_accept_path() {
        let mut led = PromptLedger::new();
        let st = led.trigger(true, "C:\\ok.exe", true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED);
        match st {
            Ok(s) => assert!(s == ElevState::PromptAccepted, "接受终态"),
            Err(_) => panic!("合法提权不该报错"),
        }
        assert!(led.count() == 1, "入账一条");
    }

    #[test]
    fn no_flag_no_prompt() {
        let mut led = PromptLedger::new();
        let st = led.trigger(false, "C:\\n.exe", true, LINK_SIG_V1, VIRT_SWITCH_ELEVATED);
        match st {
            Ok(s) => assert!(s == ElevState::FlagAbsent, "无标志=缺席态"),
            Err(_) => panic!("无标志是合法路径"),
        }
        assert!(led.count() == 0, "无标志不入账");
    }

    #[test]
    fn state_machine_terminal_closed() {
        assert!(ElevState::full_chain_accept().is_ok(), "接受路径");
        assert!(ElevState::full_chain_decline().is_ok(), "拒绝路径");
        assert!(ElevState::FlagAbsent.advance(ElevState::PromptAccepted).is_err(), "越级拒");
        assert!(ElevState::PromptPending.advance(ElevState::FlagAbsent).is_err(), "倒退拒");
        assert!(ElevState::PromptDeclined.advance(ElevState::PromptAccepted).is_err(), "终态互转拒");
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(validate_exe("").is_err(), "空路径必拒");
        let full = "x".repeat(EXE_LEN_MAX);
        assert!(validate_exe(full.as_str()).is_ok(), "恰 128 合法");
        let over = "x".repeat(EXE_LEN_MAX + 1);
        assert!(validate_exe(over.as_str()).is_err(), "129 必拒");
        assert!(link_check(1).is_err(), "虚拟化非 0 必拒");
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 5 {
            assert!(inject_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < RalCode::ALL.len() {
            let c = RalCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
    }
}
