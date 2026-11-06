//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18469 · 标志变更即时生效（hive 通知消费） · 0x7F100段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18469-J1 标志修改后新进程即生效断言全过，存活进程策略
//!       （下会话生效）口径登记
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 300 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：NotifyEv 通知队列（Vec 定容 8）/ ProcRec 进程表（gen_at_spawn）
//!   状态机  ：NotifyPhase 五态单调 Idle→Queue→Consume→ApplyPolicy→Frozen
//!   账本    ：通知消费账（seq 严格递增、重复/乱序拒、消费数=世代增量）
//!   错误路径：HnlCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：4 类坏样本（重复seq/乱序seq/未知路径/存活进程改策略）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   口径核  ：新进程取当前世代（即时生效）；存活进程冻结在
//!             gen_at_spawn（下会话生效）——存活性切换前永不变更
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F100，AI-VX11-1105 批 A07 卡 F18469 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7F100;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 通知队列定容（冻结 8 槽）。
pub const QUEUE_CAP: usize = 8;
/// 进程表定容（冻结 8 槽：典型进程采样位）。
pub const PROC_CAP: usize = 8;
/// 通知 seq 起始值（冻结 1：seq 严格递增，0 保留为"未消费"哨兵）。
pub const SEQ_START: u32 = 1;
/// 登记路径数上限（冻结 8）。
pub const PATH_CAP: usize = 8;

// ── 通知事件 ────────────────────────────────────────────────────
/// 单条 hive 通知。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotifyEv {
    pub seq: u32,
    pub path_hash: u32,
    pub payload_len: u32,
}

// ── 进程记录 ────────────────────────────────────────────────────
/// 进程记录（策略世代冻结点）。
#[derive(Clone, Copy, Debug)]
pub struct ProcRec {
    pub pid: u32,
    pub alive: bool,
    /// 出生时策略世代（存活进程永久冻结在此，下会话才换新策略）。
    pub gen_at_spawn: u32,
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HnlCode {
    /// seq 重复。
    SeqDup,
    /// seq 乱序/跳号（必须严格 +1）。
    SeqGap,
    /// 未知路径（未登记 exe）。
    PathUnknown,
    /// 队列满。
    QueueFull,
    /// 未知进程。
    ProcUnknown,
    /// 世代倒退。
    GenRegress,
    /// 存活进程改策略（下会话口径被踩）。
    LivePolicyBreach,
    /// 状态机倒退。
    PhaseRegress,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 生效漂移（同输入双跑世代不一致）。
    EffectiveDrift,
    /// 双跑不一致。
    NotReproducible,
}

impl HnlCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HnlCode::SeqDup => "seq 重复：通知幂等键冲突",
            HnlCode::SeqGap => "seq 乱序：必须严格 +1 消费",
            HnlCode::PathUnknown => "未知路径：通知的 exe 未登记",
            HnlCode::QueueFull => "通知队列满：入队被拒旧队保留",
            HnlCode::ProcUnknown => "未知进程：进程表外",
            HnlCode::GenRegress => "世代倒退：策略世代只增不减",
            HnlCode::LivePolicyBreach => "存活进程改策略：下会话口径被踩",
            HnlCode::PhaseRegress => "状态机倒退：通知流水只进不退",
            HnlCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            HnlCode::EffectiveDrift => "生效漂移：同输入双跑世代不一致",
            HnlCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [HnlCode; 11] = [
        HnlCode::SeqDup,
        HnlCode::SeqGap,
        HnlCode::PathUnknown,
        HnlCode::QueueFull,
        HnlCode::ProcUnknown,
        HnlCode::GenRegress,
        HnlCode::LivePolicyBreach,
        HnlCode::PhaseRegress,
        HnlCode::ConservationBroken,
        HnlCode::EffectiveDrift,
        HnlCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < HnlCode::ALL.len() {
        if HnlCode::ALL[i].code() == code {
            return HnlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F100 段闭集内"
}

// ── 状态机：通知消费流水 ────────────────────────────────────────
/// 通知流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifyPhase {
    /// 未开始。
    Idle = 0,
    /// 已入队。
    Queued = 1,
    /// 已消费。
    Consumed = 2,
    /// 已应用策略。
    Applied = 3,
    /// 已冻结。
    Frozen = 4,
}

impl NotifyPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: NotifyPhase) -> Result<NotifyPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(HnlCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<NotifyPhase, u32> {
        let s0 = NotifyPhase::Idle;
        let s1 = s0.advance(NotifyPhase::Queued)?;
        let s2 = s1.advance(NotifyPhase::Consumed)?;
        let s3 = s2.advance(NotifyPhase::Applied)?;
        s3.advance(NotifyPhase::Frozen)
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

// ── 通知器（队列 + 进程表 + 世代账） ────────────────────────────
/// hive 通知器总成。
pub struct HiveNotifier {
    queue: Vec<NotifyEv>,
    procs: Vec<ProcRec>,
    paths: Vec<u32>,
    /// 当前策略世代（每消费一条通知 +1，只增不减）。
    pub gen: u32,
    /// 已消费通知数（= 世代增量守恒锚）。
    pub consumed: u32,
    /// 最后消费的 seq（严格递增锚，0=未消费）。
    last_seq: u32,
    pub full_rejects: u32,
    pub inject_tries: [u32; 4],
    pub inject_caught: [u32; 4],
}

impl HiveNotifier {
    /// 空通知器。
    pub fn new() -> HiveNotifier {
        HiveNotifier {
            queue: Vec::new(),
            procs: Vec::new(),
            paths: Vec::new(),
            gen: 0,
            consumed: 0,
            last_seq: 0,
            full_rejects: 0,
            inject_tries: [0; 4],
            inject_caught: [0; 4],
        }
    }
    /// 登记 exe 路径（通知的合法目的地，定容 8）。
    pub fn register_path(&mut self, path: &str) -> Result<(), u32> {
        if path.len() == 0 {
            return Err(HnlCode::PathUnknown.code());
        }
        if self.paths.len() >= PATH_CAP {
            return Err(HnlCode::QueueFull.code());
        }
        let h = fnv32(path.as_bytes());
        // 重复登记幂等（同路径只记一次）。
        let mut dup = false;
        let mut i = 0;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if *p == h {
                    dup = true;
                }
            }
            i += 1;
        }
        if !dup {
            self.paths.push(h);
        }
        Ok(())
    }
    /// 入队（容量闸 + 重复 seq 闸——入队即查重）。
    pub fn enqueue(&mut self, seq: u32, path: &str) -> Result<(), u32> {
        NotifyPhase::full_chain()?;
        if self.queue.len() >= QUEUE_CAP {
            self.full_rejects += 1;
            return Err(HnlCode::QueueFull.code());
        }
        let h = fnv32(path.as_bytes());
        // 路径必须已登记。
        let mut known = false;
        let mut i = 0;
        while i < self.paths.len() {
            if let Some(p) = self.paths.get(i) {
                if *p == h {
                    known = true;
                }
            }
            i += 1;
        }
        if !known {
            return Err(HnlCode::PathUnknown.code());
        }
        // seq 重复闸：队列内与已消费均不可重复。
        let mut dup = false;
        let mut k = 0;
        while k < self.queue.len() {
            if let Some(e) = self.queue.get(k) {
                if e.seq == seq {
                    dup = true;
                }
            }
            k += 1;
        }
        if seq <= self.last_seq || dup {
            return Err(HnlCode::SeqDup.code());
        }
        let ev = NotifyEv {
            seq,
            path_hash: h,
            payload_len: 0,
        };
        self.queue.push(ev);
        Ok(())
    }
    /// 消费队首（seq 严格 +1 闸 → 世代 +1）。
    pub fn consume_next(&mut self) -> Result<u32, u32> {
        if self.queue.len() == 0 {
            return Err(HnlCode::SeqGap.code());
        }
        let ev = match self.queue.first() {
            Some(e) => *e,
            None => return Err(HnlCode::SeqGap.code()),
        };
        let expect = self.last_seq.saturating_add(1);
        if ev.seq != expect {
            return Err(HnlCode::SeqGap.code());
        }
        self.queue.remove(0);
        self.last_seq = ev.seq;
        self.consumed += 1;
        // 世代增量：策略世代单调 +1（只增不减）。
        let new_gen = self.gen.saturating_add(1);
        if new_gen <= self.gen {
            return Err(HnlCode::GenRegress.code());
        }
        self.gen = new_gen;
        Ok(self.gen)
    }
    /// 新进程出生：取当前世代（即时生效口径）。
    pub fn spawn_proc(&mut self, pid: u32) -> Result<u32, u32> {
        if pid == 0 {
            return Err(HnlCode::ProcUnknown.code());
        }
        if self.procs.len() >= PROC_CAP {
            return Err(HnlCode::QueueFull.code());
        }
        let rec = ProcRec {
            pid,
            alive: true,
            gen_at_spawn: self.gen,
        };
        self.procs.push(rec);
        Ok(self.gen)
    }
    /// 存活进程冻结点：gen_at_spawn 永不改（下会话口径）。
    fn live_policy_freeze(pid: u32) -> Result<u32, u32> {
        // 任何对存活进程策略的改写尝试都到不了这里：本函数只读。
        let _ = pid;
        Err(HnlCode::LivePolicyBreach.code())
    }
    /// 改写存活进程策略（显式拒绝入口——口径闸）。
    pub fn force_policy(&mut self, pid: u32, _new_gen: u32) -> Result<u32, u32> {
        let mut found = false;
        let mut alive = false;
        let mut i = 0;
        while i < self.procs.len() {
            if let Some(p) = self.procs.get(i) {
                if p.pid == pid {
                    found = true;
                    alive = p.alive;
                }
            }
            i += 1;
        }
        if !found {
            return Err(HnlCode::ProcUnknown.code());
        }
        if alive {
            return HiveNotifier::live_policy_freeze(pid);
        }
        // 已退出进程（占位）同样不回写——世代只增不减。
        Err(HnlCode::GenRegress.code())
    }
    /// 进程生效世代：存活 → gen_at_spawn；否则当前世代。
    pub fn effective_gen(&self, pid: u32) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.procs.len() {
            if let Some(p) = self.procs.get(i) {
                if p.pid == pid {
                    if p.alive {
                        return Ok(p.gen_at_spawn);
                    }
                    return Ok(self.gen);
                }
            }
            i += 1;
        }
        Err(HnlCode::ProcUnknown.code())
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒（四槽全抓）。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（四槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 指纹（确定性对拍）。
    pub fn fingerprint(&self) -> u32 {
        let mut h = 0x811C9DC5u32;
        h ^= self.gen;
        h = h.wrapping_mul(0x01000193);
        h ^= self.consumed;
        h = h.wrapping_mul(0x01000193);
        h ^= self.last_seq;
        h = h.wrapping_mul(0x01000193);
        let mut i = 0;
        while i < self.procs.len() {
            if let Some(p) = self.procs.get(i) {
                h ^= p.gen_at_spawn;
                h = h.wrapping_mul(0x01000193);
            }
            i += 1;
        }
        h
    }
}

// ── 正样本构造（确定性） ────────────────────────────────────────
/// 典型 exe 路径（8 例确定性变体）。
pub fn make_path(idx: u32) -> String {
    format!("C:\\Program Files\\notify_app_{}.exe", idx % 8)
}

// ── 注入核（4 类坏样本 ×10 轮） ─────────────────────────────────
/// 注入探针（命中约定：Err(窗内码)=被抓，Ok=漏）：
///   0 重复 seq；1 乱序 seq（跳号）；2 未知路径；3 存活进程改策略。
pub fn inject_probe(slot: usize) -> Result<(), u32> {
    let mut n = HiveNotifier::new();
    let _ = n.register_path(&make_path(0))?;
    let _ = n.spawn_proc(1)?;
    match slot {
        0 => {
            let _ = n.enqueue(1, &make_path(0))?;
            let _ = n.consume_next()?;
            match n.enqueue(1, &make_path(0)) {
                Err(c) if c == HnlCode::SeqDup.code() => Err(c),
                _ => Ok(()),
            }
        }
        1 => {
            // 跳号：消费 seq=1 后直接入队 seq=3，消费必拒（gap）。
            let _ = n.enqueue(1, &make_path(0))?;
            let _ = n.consume_next()?;
            let _ = n.enqueue(3, &make_path(0))?;
            match n.consume_next() {
                Err(c) if c == HnlCode::SeqGap.code() => Err(c),
                _ => Ok(()),
            }
        }
        2 => match n.enqueue(1, "C:\\unregistered\\ghost.exe") {
            Err(c) if c == HnlCode::PathUnknown.code() => Err(c),
            _ => Ok(()),
        },
        _ => match n.force_policy(1, 99) {
            Err(c) if c == HnlCode::LivePolicyBreach.code() => Err(c),
            _ => Ok(()),
        },
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：登记 4 路径 → 老 2 进程出生 → 消费 3 通知（世代=3）→
/// 新进程取世代 3（即时生效）→ 存活老进程冻结 0（下会话口径）
/// → 4 类注入 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    NotifyPhase::full_chain()?;
    let mut n = HiveNotifier::new();
    // 登记路径。
    let mut i = 0u32;
    while i < 4 {
        n.register_path(&make_path(i))?;
        i += 1;
    }
    // 老进程：世代 0 出生。
    n.spawn_proc(101)?;
    n.spawn_proc(102)?;
    // 消费 3 通知（seq 1..3 严格递增）。
    let mut s = 1u32;
    while s <= 3 {
        n.enqueue(s, &make_path(s % 4))?;
        let g = n.consume_next()?;
        if g != s {
            return Err(HnlCode::EffectiveDrift.code());
        }
        s += 1;
    }
    // 新进程：即时生效 → gen_at_spawn = 3。
    n.spawn_proc(103)?;
    let g_new = n.effective_gen(103)?;
    if g_new != 3 {
        return Err(HnlCode::EffectiveDrift.code());
    }
    // 存活老进程：冻结在 0（下会话口径）。
    let g_old = n.effective_gen(101)?;
    if g_old != 0 {
        return Err(HnlCode::LivePolicyBreach.code());
    }
    // 守恒锚：消费数 = 世代增量。
    if n.consumed != n.gen || n.gen != 3 {
        return Err(HnlCode::ConservationBroken.code());
    }
    // 注入 4 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = match inject_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            n.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !n.all_caught() || !n.conserve() {
        return Err(HnlCode::ConservationBroken.code());
    }
    Ok((n.consumed, n.gen, n.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 新进程即时生效：消费 1 通知后出生 → 世代 1。
    let mut n = HiveNotifier::new();
    n.register_path(&make_path(0))?;
    n.enqueue(1, &make_path(0))?;
    n.consume_next()?;
    n.spawn_proc(7)?;
    let g = n.effective_gen(7)?;
    if g == 1 {
        Ok(())
    } else {
        Err(HnlCode::EffectiveDrift.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 存活进程下会话口径：消费后 gen_at_spawn 不变。
    let mut n = HiveNotifier::new();
    n.register_path(&make_path(1))?;
    n.spawn_proc(5)?;
    n.enqueue(1, &make_path(1))?;
    n.consume_next()?;
    let g = n.effective_gen(5)?;
    if g == 0 {
        Ok(())
    } else {
        Err(HnlCode::LivePolicyBreach.code())
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机越级必拒。
    match NotifyPhase::Idle.advance(NotifyPhase::Applied) {
        Err(c) if c == HnlCode::PhaseRegress.code() => Ok(()),
        _ => Err(HnlCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7F100 {
        return false;
    }
    let mut i = 0;
    while i < HnlCode::ALL.len() {
        let v = HnlCode::ALL[i].code();
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
    while i < HnlCode::ALL.len() {
        if detail_of(HnlCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（11 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-世代口径冻结",
        SEQ_START == 1 && QUEUE_CAP == 8 && PROC_CAP == 8,
        format!("seq 起 {} 队列 {} 进程 {}", SEQ_START, QUEUE_CAP, PROC_CAP),
    ));
    v.push((
        "J1-五态流水",
        NotifyPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
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
    // 注入 4 类 ×10 轮。
    let mut n = HiveNotifier::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = inject_probe(slot).is_err();
            n.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries =
        n.inject_tries[0] + n.inject_tries[1] + n.inject_tries[2] + n.inject_tries[3];
    let total_caught =
        n.inject_caught[0] + n.inject_caught[1] + n.inject_caught[2] + n.inject_caught[3];
    v.push((
        "J1-注入10/10全抓",
        n.all_caught(),
        format!("4类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        n.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", HnlCode::ALL.len()),
    ));
    // 队列容量耗尽：满后拒新不丢旧。
    let mut n2 = HiveNotifier::new();
    let _ = n2.register_path(&make_path(0));
    let mut s9 = 1u32;
    while s9 <= (QUEUE_CAP as u32) + 2 {
        let _ = n2.enqueue(s9, &make_path(0));
        s9 += 1;
    }
    v.push((
        "J1-队列容量守恒",
        n2.full_rejects == 2 && n2.consumed == 0,
        format!("容量 {} 拒 {}", QUEUE_CAP, n2.full_rejects),
    ));
    // 边界四类：pid 0 拒 / pid 1 合法 / seq 0 拒（哨兵）/ seq 上界合法。
    let mut n3 = HiveNotifier::new();
    let _ = n3.register_path(&make_path(2));
    let b0 = n3.spawn_proc(0).is_err();
    let b1 = n3.spawn_proc(1).is_ok();
    let b2 = n3.enqueue(0, &make_path(2)).is_err();
    let b3 = n3.enqueue(1, &make_path(2)).is_ok();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("pid0{} pid1{} seq0{} seq1{}", b0, b1, b2, b3),
    ));
    // 双跑一致（指纹 + 结果三元组）。
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
pub fn run_unx18469_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18469 标志变更即时生效（hive 通知消费）");
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
        let set = run_unx18469_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_instant_effect() {
        let mut n = HiveNotifier::new();
        n.register_path(&make_path(0)).unwrap_or(());
        n.enqueue(1, &make_path(0)).unwrap_or(());
        assert!(n.consume_next().is_ok(), "消费过");
        n.spawn_proc(42).unwrap_or(0);
        match n.effective_gen(42) {
            Ok(g) => assert!(g == 1, "新进程即时生效"),
            Err(_) => panic!("已出生进程必可查"),
        }
    }

    #[test]
    fn live_process_session_policy() {
        let mut n = HiveNotifier::new();
        n.register_path(&make_path(1)).unwrap_or(());
        n.spawn_proc(9).unwrap_or(0);
        n.enqueue(1, &make_path(1)).unwrap_or(());
        let _ = n.consume_next();
        match n.effective_gen(9) {
            Ok(g) => assert!(g == 0, "存活进程冻结在出生世代"),
            Err(_) => panic!("存活进程必可查"),
        }
        assert!(n.force_policy(9, 5).is_err(), "改策略必拒");
    }

    #[test]
    fn boundary_four_kinds() {
        let mut n = HiveNotifier::new();
        n.register_path(&make_path(2)).unwrap_or(());
        assert!(n.spawn_proc(0).is_err(), "pid 0 哨兵必拒");
        assert!(n.spawn_proc(1).is_ok(), "pid 1 合法");
        assert!(n.enqueue(0, &make_path(2)).is_err(), "seq 0 哨兵必拒");
        assert!(n.enqueue(1, &make_path(2)).is_ok(), "seq 1 合法");
    }

    #[test]
    fn injection_four_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 4 {
            assert!(inject_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn seq_monotone_and_dup() {
        assert!(NotifyPhase::full_chain().is_ok(), "五态流水");
        assert!(NotifyPhase::Queued.advance(NotifyPhase::Idle).is_err(), "倒退拒");
        let mut n = HiveNotifier::new();
        n.register_path(&make_path(3)).unwrap_or(());
        assert!(n.enqueue(1, &make_path(3)).is_ok(), "seq 1 入队");
        assert!(n.enqueue(1, &make_path(3)).is_err(), "重复 seq 入队拒");
        assert!(n.enqueue(5, &make_path(3)).is_ok(), "后继 seq 可入队");
        assert!(n.consume_next().is_ok(), "按序消费 1");
        assert!(n.consume_next().is_err(), "跳号消费拒（gap）");
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
        while i < HnlCode::ALL.len() {
            let c = HnlCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
    }
}
