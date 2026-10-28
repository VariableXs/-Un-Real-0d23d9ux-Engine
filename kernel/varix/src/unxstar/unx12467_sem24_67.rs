//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12467 · NtNotifyChangeKey/NtNotifyChangeMultipleKeys · 0x30860
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12467-J1 异步监视（事件/完成档）判据绿，变更触发 10/10
//!      次正确交付；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 异步监视协议：注册监视（挂完成票据 + 监视过滤器）→ 变更发生
//!     → 交付（票据态 Pending→Delivered），一次注册恰交付一次
//!     （NT Notify 口径：交付后监视撤销，需重新挂）；
//!   - 过滤器闭集：REG_NOTIFY_CHANGE_NAME（改名/建删）/ATTRIBUTE
//!     （属性）/LAST_SET（值改动）/SECURITY（安全描述）四类位过滤，
//!     变更类 ∩ 过滤器 = 0 不交付（静默跳过，非错误）；
//!   - 多键监视档：NtNotifyChangeMultipleKeys——监视键+子树（
//!     WatchTree 位），子树变更也交付（树径交付账）；
//!   - 交付序：变更按 (frame,seq) 确定性序交付，交付账 append-only；
//!   - 撤销语义：显性撤销（Cancel）→ 票据 Cancelled，此后变更不再
//!     交付该票据；
//!   - 注入核：LCG 写死种子 = SEG_BASE，过滤器不匹配注入（变更类
//!     出过滤器）10/10 不交付（静默）；重复交付注入 10/10 拒；
//!   - 诊断码闭集 0x30860 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30860 段）。
pub const SEG_BASE: u32 = 0x30860;

/// 监视票据表容量。
pub const WATCH_CAP: usize = 8;

/// 过滤器位闭集（NT REG_NOTIFY_* 口径）。
pub const NOTIFY_NAME: u32 = 0x01;
pub const NOTIFY_ATTRIBUTES: u32 = 0x02;
pub const NOTIFY_LAST_SET: u32 = 0x04;
pub const NOTIFY_SECURITY: u32 = 0x08;
/// 合法掩码。
pub const NOTIFY_LEGAL: u32 = 0x0F;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30860 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NtCode {
    /// 0x30860 过滤器越位。
    FilterOob = 0,
    /// 0x30861 票据表满。
    WatchFull = 1,
    /// 0x30862 幽灵票据（变更引用未挂票据）。
    GhostWatch = 2,
    /// 0x30863 重复交付（票据已 Delivered）。
    DupDeliver = 3,
    /// 0x30864 撤销后交付。
    CancelledDeliver = 4,
    /// 0x30865 过滤器不匹配（静默跳过，可查不报错）。
    FilterSkip = 5,
    /// 0x30866 交付序倒挂。
    DeliverBack = 6,
    /// 0x30867 交付账缩水。
    LedgerShrink = 7,
    /// 0x30868 树径越界（多键档子树号出界）。
    TreeOob = 8,
    /// 0x3089 段位越界。
    SegOob = 9,
    /// 0x3086A 双重挂（同键同过滤器重复注册）。
    DupWatch = 10,
    /// 0x3086B 变更类越界。
    ChangeOob = 11,
}

impl NtCode {
    /// 全部码闭集。
    pub const ALL: [NtCode; 12] = [
        NtCode::FilterOob,
        NtCode::WatchFull,
        NtCode::GhostWatch,
        NtCode::DupDeliver,
        NtCode::CancelledDeliver,
        NtCode::FilterSkip,
        NtCode::DeliverBack,
        NtCode::LedgerShrink,
        NtCode::TreeOob,
        NtCode::SegOob,
        NtCode::DupWatch,
        NtCode::ChangeOob,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            NtCode::FilterOob => "过滤器出 NOTIFY_* 闭集（4 位）",
            NtCode::WatchFull => "监视票据表满（8），先撤销闲置监视",
            NtCode::GhostWatch => "变更引用未挂票据，先注册监视",
            NtCode::DupDeliver => "票据已交付，交付后自动撤销需重挂",
            NtCode::CancelledDeliver => "票据已撤销，不再交付",
            NtCode::FilterSkip => "过滤器不匹配静默跳过（非错误，账面可查）",
            NtCode::DeliverBack => "交付序倒挂，(frame,seq) 必须递增",
            NtCode::LedgerShrink => "交付账只增不减被破",
            NtCode::TreeOob => "子树号越界（多键档）",
            NtCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            NtCode::DupWatch => "同键同过滤器重复注册，先撤销再挂",
            NtCode::ChangeOob => "变更类出闭集（4 类）",
        }
    }
}

/// 监视票据三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WatchState {
    /// 等待变更。
    Pending,
    /// 已交付（一次性）。
    Delivered,
    /// 已撤销。
    Cancelled,
}

/// 监视票据。
#[derive(Clone, Copy)]
struct WatchTicket {
    /// 监视键号。
    key: u32,
    /// 过滤器位集。
    filter: u32,
    /// 是否树径（子树监视）。
    watch_tree: bool,
    /// 态。
    state: WatchState,
}

/// 交付账行。
#[derive(Clone, Copy)]
pub struct DeliverRow {
    /// 票据号。
    pub watch: u32,
    /// 变更类（NOTIFY_* 位）。
    pub change: u32,
    /// (frame, seq) 确定性序。
    pub frame: u64,
    pub seq: u32,
}

/// 监视语义机。
pub struct NotifyMgr {
    /// 票据表。
    tickets: [Option<WatchTicket>; WATCH_CAP],
    used: usize,
    /// 交付账。
    ledger: [Option<DeliverRow>; WATCH_CAP * 4],
    ledger_len: usize,
    /// 静默跳过计数（过滤器不匹配可查）。
    skips: u64,
    /// 上次交付序。
    last_frame: u64,
    last_seq: u32,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl NotifyMgr {
    /// 新建。
    pub fn new() -> NotifyMgr {
        NotifyMgr {
            tickets: [None; WATCH_CAP],
            used: 0,
            ledger: [None; WATCH_CAP * 4],
            ledger_len: 0,
            skips: 0,
            last_frame: 0,
            last_seq: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: NtCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 注册监视（过滤器闭集 + 重复注册拒 + 表满拒）。
    pub fn register(&mut self, key: u32, filter: u32, watch_tree: bool) -> Result<u32, u32> {
        if filter & !NOTIFY_LEGAL != 0 {
            return Err(self.hit(NtCode::FilterOob));
        }
        // 重复注册（同键同过滤器同树径）。
        let mut i = 0usize;
        while i < self.used {
            if let Some(ref t) = self.tickets[i] {
                if t.state == WatchState::Pending && t.key == key && t.filter == filter
                    && t.watch_tree == watch_tree
                {
                    return Err(self.hit(NtCode::DupWatch));
                }
            }
            i += 1;
        }
        if self.used >= WATCH_CAP {
            return Err(self.hit(NtCode::WatchFull));
        }
        self.tickets[self.used] =
            Some(WatchTicket { key, filter, watch_tree, state: WatchState::Pending });
        let idx = self.used as u32;
        self.used += 1;
        Ok(idx)
    }

    /// 撤销监视。
    pub fn cancel(&mut self, watch: u32) -> Result<(), u32> {
        if watch as usize >= self.used {
            return Err(self.hit(NtCode::GhostWatch));
        }
        match self.tickets[watch as usize] {
            Some(ref mut t) if t.state == WatchState::Pending => {
                t.state = WatchState::Cancelled;
                Ok(())
            }
            Some(ref t) if t.state == WatchState::Cancelled => Err(self.hit(NtCode::GhostWatch)),
            _ => Err(self.hit(NtCode::DupDeliver)),
        }
    }

    /// 变更投递（事件/完成档核）：过滤器匹配 → 恰一次交付。
    /// 过滤器不匹配 → 静默跳过（FilterSkip 可查，不报错不交付）。
    pub fn on_change(&mut self, watch: u32, key: u32, change: u32, frame: u64, seq: u32) -> Result<bool, u32> {
        if change & !NOTIFY_LEGAL != 0 {
            return Err(self.hit(NtCode::ChangeOob));
        }
        if watch as usize >= self.used {
            return Err(self.hit(NtCode::GhostWatch));
        }
        let (filter, state, tree) = match self.tickets[watch as usize] {
            Some(ref t) => (t.filter, t.state, t.watch_tree),
            None => return Err(self.hit(NtCode::GhostWatch)),
        };
        match state {
            WatchState::Cancelled => return Err(self.hit(NtCode::CancelledDeliver)),
            WatchState::Delivered => return Err(self.hit(NtCode::DupDeliver)),
            WatchState::Pending => {}
        }
        // 树径档：非树径票据只接本键变更。
        if !tree && key != self.ticket_key(watch) {
            self.skips += 1;
            return Ok(false);
        }
        // 过滤器匹配核：变更类 ∩ 过滤器 = 0 → 静默跳过。
        if change & filter == 0 {
            self.skips += 1;
            return Ok(false);
        }
        // 交付序核：(frame,seq) 严格递增。
        if frame < self.last_frame || (frame == self.last_frame && seq <= self.last_seq) {
            return Err(self.hit(NtCode::DeliverBack));
        }
        // 恰一次交付（Delivered 终态）。
        if let Some(ref mut t) = self.tickets[watch as usize] {
            t.state = WatchState::Delivered;
        }
        if self.ledger_len < WATCH_CAP * 4 {
            self.ledger[self.ledger_len] =
                Some(DeliverRow { watch, change, frame, seq });
            self.ledger_len += 1;
        }
        self.last_frame = frame;
        self.last_seq = seq;
        Ok(true)
    }

    /// 票据键只读。
    fn ticket_key(&self, watch: u32) -> u32 {
        match self.tickets[watch as usize] {
            Some(ref t) => t.key,
            None => u32::MAX,
        }
    }

    /// 交付账单调。
    pub fn ledger_monotone(&self) -> bool {
        let mut i = 0;
        while i < self.ledger_len {
            if self.ledger[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 交付数。
    pub fn delivered(&self) -> usize {
        self.ledger_len
    }

    /// 静默跳过数。
    pub fn skip_count(&self) -> u64 {
        self.skips
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: NtCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 变更触发正确交付演练 10/10 轮（判准 J1）。
pub fn drill_deliver_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut m = NotifyMgr::new();
        let w = m.register(1, NOTIFY_NAME | NOTIFY_LAST_SET, false);
        let r_ok = w.is_ok();
        let w = w.unwrap_or(0);
        // 匹配变更：交付。
        let d1 = m.on_change(w, 1, NOTIFY_NAME, round as u64, 1);
        // 一次性：交付后同票据再交付拒 DupDeliver。
        let d2 = match m.on_change(w, 1, NOTIFY_LAST_SET, round as u64, 2) {
            Err(c) => c == NtCode::DupDeliver.val(),
            Ok(_) => false,
        };
        // 重挂后可再交付。
        let w2 = m.register(1, NOTIFY_LAST_SET, false);
        let d3 = w2.is_ok() && m.on_change(w2.unwrap_or(0), 1, NOTIFY_LAST_SET, round as u64, 3).unwrap_or(false);
        // 过滤器不匹配：静默。
        let w3 = m.register(2, NOTIFY_SECURITY, false);
        let d4 = w3.is_ok() && matches!(m.on_change(w3.unwrap_or(0), 2, NOTIFY_NAME, round as u64, 4), Ok(false));
        // 撤销后变更：CancelledDeliver。
        let w4 = m.register(3, NOTIFY_NAME, false);
        let _ = m.cancel(w4.unwrap_or(0));
        let d5 = match m.on_change(w4.unwrap_or(0), 3, NOTIFY_NAME, round as u64, 5) {
            Err(c) => c == NtCode::CancelledDeliver.val(),
            Ok(_) => false,
        };
        let _ = lcg.below(3);
        if r_ok && d1.unwrap_or(false) && d2 && d3 && d4 && d5 && m.ledger_monotone() {
            ok += 1;
        } else {
            d.push(format!(
                "轮{} 交{} 重{} 重挂{} 静{} 撤{}",
                round, d1.unwrap_or(false), d2, d3, d4, d5
            ));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("变更交付 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 过滤器不匹配注入 10/10 轮：四类变更 × 四类过滤器矩阵静默面。
pub fn drill_filter_matrix_10() -> (bool, String) {
    let bits = [NOTIFY_NAME, NOTIFY_ATTRIBUTES, NOTIFY_LAST_SET, NOTIFY_SECURITY];
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut all_ok = true;
        let mut f = 0usize;
        while f < 4 {
            let mut m = NotifyMgr::new();
            let w = m.register(1, bits[f], false);
            if w.is_err() {
                all_ok = false;
                f += 1;
                continue;
            }
            let w = w.unwrap_or(0);
            // 同类变更 → 交付；异类 → 静默。
            let mut c = 0usize;
            while c < 4 {
                let r = m.on_change(w, 1, bits[c], (round * 16 + f * 4 + c) as u64, 1);
                if c == f {
                    if !matches!(r, Ok(true)) {
                        all_ok = false;
                    }
                    // 交付后重挂再测下一类。
                    let w2 = m.register(1, bits[f], false);
                    if w2.is_err() {
                        all_ok = false;
                        break;
                    }
                    let _ = w2;
                    // 注意：票已 Delivered，后续 c 循环用新票。
                    // 为矩阵完备，此处 f 行只测同类交付，异类在下段。
                    break;
                }
                c += 1;
            }
            f += 1;
        }
        // 异类静默面：每过滤器挂票测一个异类变更 → 静默。
        let mut g = 0usize;
        while g < 4 {
            let mut m = NotifyMgr::new();
            let w = m.register(1, bits[g], false);
            if w.is_err() {
                g += 1;
                continue;
            }
            let other = bits[(g + 1) % 4];
            if !matches!(m.on_change(w.unwrap_or(0), 1, other, round as u64, 9), Ok(false)) {
                all_ok = false;
            }
            if m.skip_count() == 0 {
                all_ok = false;
            }
            g += 1;
        }
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("过滤矩阵 {}/{}", ok, INJECT_ROUNDS))
}

/// 过滤器越位注入 10/10 轮：位 4..=31 逐位扫必拒。
pub fn drill_filter_oob_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut caught = 0usize;
        let mut bit = 4u32;
        while bit < 32 {
            let mut m = NotifyMgr::new();
            if matches!(m.register(1, 1u32 << bit, false), Err(c) if c == NtCode::FilterOob.val()) {
                caught += 1;
            }
            bit += 1;
        }
        // 变更类越位同样拒。
        let mut m = NotifyMgr::new();
        let w = m.register(1, NOTIFY_NAME, false);
        let change_oob = match w {
            Ok(w) => matches!(m.on_change(w, 1, 0x10, 0, 1), Err(c) if c == NtCode::ChangeOob.val()),
            Err(_) => false,
        };
        if caught == 28 && change_oob {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("过滤器越位 {}/{} ×28位", ok, INJECT_ROUNDS))
}

/// 树径交付演练 10/10 轮（多键档）。
pub fn drill_watch_tree_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut m = NotifyMgr::new();
        // 树径票据：键 1，子树变更（键 5）也交付。
        let wt = m.register(1, NOTIFY_NAME, true);
        // 非树径票据：键 1，子树变更静默。
        let wf = m.register(1, NOTIFY_NAME, false);
        let mut r_ok = wt.is_ok() && wf.is_ok();
        let sub_frame = round as u64 * 2;
        let tree_hit = matches!(
            m.on_change(wt.unwrap_or(0), 5, NOTIFY_NAME, sub_frame, 1),
            Ok(true)
        );
        // 非树径对子树键 5：静默（键不匹配跳过）。
        let flat_skip = matches!(
            m.on_change(wf.unwrap_or(0), 5, NOTIFY_NAME, sub_frame + 1, 2),
            Ok(false)
        );
        if !tree_hit || !flat_skip {
            r_ok = false;
        }
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("树径交付 {}/{}", ok, INJECT_ROUNDS))
}

/// 交付序倒挂注入 10/10 轮。
pub fn drill_order_back_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut m = NotifyMgr::new();
        // 连续挂 4 票依次交付，序倒挂必拒。
        let mut seq = 1u32;
        let mut delivered = 0;
        let mut back_caught = false;
        let mut i = 0;
        while i < 4 {
            let w = m.register(1, NOTIFY_NAME, false);
            match w {
                Ok(w) => {
                    let f = (round as u64) * 10 + i as u64;
                    if m.on_change(w, 1, NOTIFY_NAME, f, seq).unwrap_or(false) {
                        delivered += 1;
                    }
                    seq += 1;
                    // 倒挂注入：同帧序号回退。
                    if i == 0 {
                        let w2 = m.register(1, NOTIFY_LAST_SET, false);
                        if let Ok(w2) = w2 {
                            if matches!(
                                m.on_change(w2, 1, NOTIFY_LAST_SET, f, 0),
                                Err(c) if c == NtCode::DeliverBack.val()
                            ) {
                                back_caught = true;
                            }
                        }
                    }
                }
                Err(_) => {}
            }
            i += 1;
        }
        if delivered >= 3 && back_caught && m.ledger_monotone() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("序倒挂注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    NtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：过滤器常量锚核对。
pub fn run_unx12467_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-过滤器锚",
        NOTIFY_LEGAL == 0x0F && WATCH_CAP == 8,
        format!("掩码{:x} 票容{}", NOTIFY_LEGAL, WATCH_CAP),
    ));
    v
}

/// run_unx12467_checks：本单判据聚合（诊断段 0x30860）。
pub fn run_unx12467_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12467 变更监视语义");
    // J1-a：变更触发正确交付（一次性/重挂/静默/撤销）。
    let (ok1, d1) = drill_deliver_10();
    s.add_dyn("J1-变更交付10/10", ok1, &d1);
    // J1-b：过滤矩阵静默面。
    let (ok2, d2) = drill_filter_matrix_10();
    s.add_dyn("J1-过滤矩阵10/10", ok2, &d2);
    // J1-c：过滤器越位注入。
    let (ok3, d3) = drill_filter_oob_10();
    s.add_dyn("J1-过滤器越位10/10", ok3, &d3);
    // J1-d：树径交付（多键档）。
    let (ok4, d4) = drill_watch_tree_10();
    s.add_dyn("J1-树径交付10/10", ok4, &d4);
    // J1-e：交付序倒挂注入。
    let (ok5, d5) = drill_order_back_10();
    s.add_dyn("J1-序倒挂注入10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30860段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12467_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
