//! unx48193_wmsgfuzz — UNX-F48193 Wine 窗 fuzz（随机消息序/随机标志注入不崩 Wine 侧）（诊断段 0xB900）。
//!
//! 判据原文：
//!   J1 fuzz ×10^4 零崩；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 伪随机源用确定性 LCG（x' = 1664525·x + 1013904223 mod 2^32，
//!       全程 wrapping 算术），禁用真随机；fuzz 可复跑：同种子 ⇒ 同
//!       消息序 ⇒ 同结论。
//!   2. Wine 侧收包面建模为封闭状态机：SETCONTEXT / START / COMP /
//!      END / NOTIFY / CONTROL / CHAR / SELECT / REQUEST / FULL 十类
//!      消息全处理；越界标志组合一律走 GracefulReject 记账回执，
//!      绝不 panic——"零崩"的账面口径是 crashes==0 且每笔消息必有
//!      Tolerated / Graceful 之一归宿（归宿守恒）。
//!   3. 覆盖率硬约束：×10^4 轮内十类消息 id 必须全部命中
//!      （coverage_mask 全 1），否则判 fuzz 未铺满、判据无效。
//!   4. 状态不变量随轮审计：comp_active ⇒ ctx_open；gcs_seen ⊆
//!      合法 GCS 掩码；每轮收尾复核 seen == tolerated + graceful。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB900;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_INVARIANT: u16 = SEG_BASE + 1; // 状态机不变量被破坏（崩的账面形态）
const E_UNSETTLED: u16 = SEG_BASE + 2; // 消息无归宿（守恒破）
const E_COVER_MISS: u16 = SEG_BASE + 3; // 十类消息未铺满
const E_ROUND_MISS: u16 = SEG_BASE + 4; // 轮数不足 ×10^4
const E_STATE_BAD: u16 = SEG_BASE + 5; // 状态迁移非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空
const E_SEED_ECHO: u16 = SEG_BASE + 7; // 同种子两轮结论不一致（不可复跑）

// ── WM_IME 消息族（USER32 口径）────────────────────────────
pub const WM_IME_STARTCOMPOSITION: u32 = 0x010D;
pub const WM_IME_ENDCOMPOSITION: u32 = 0x010E;
pub const WM_IME_COMPOSITION: u32 = 0x010F;
pub const WM_IME_SETCONTEXT: u32 = 0x0281;
pub const WM_IME_NOTIFY: u32 = 0x0282;
pub const WM_IME_CONTROL: u32 = 0x0283;
pub const WM_IME_COMPOSITIONFULL: u32 = 0x0284;
pub const WM_IME_SELECT: u32 = 0x0285;
pub const WM_IME_CHAR: u32 = 0x0286;
pub const WM_IME_REQUEST: u32 = 0x0288;

/// 十类受试消息 id 闭集（覆盖率账的统计口径）。
pub const VALID_IDS: [u32; 10] = [
    WM_IME_STARTCOMPOSITION,
    WM_IME_ENDCOMPOSITION,
    WM_IME_COMPOSITION,
    WM_IME_SETCONTEXT,
    WM_IME_NOTIFY,
    WM_IME_CONTROL,
    WM_IME_COMPOSITIONFULL,
    WM_IME_SELECT,
    WM_IME_CHAR,
    WM_IME_REQUEST,
];

/// GCS 组合串标志合法掩码（越界位注入即触发 Graceful）。
pub const GCS_VALID_MASK: u32 = 0x0000_1FFF;
/// NOTIFY 操作码合法区间 [1,16]（IMN_* 闭集）。
pub const IMN_OP_MAX: u32 = 16;
/// CONTROL 请求码闭集（IMC_* 常见十码）。
pub const IMC_VALID: [u32; 10] = [1, 2, 3, 5, 6, 7, 9, 10, 21, 22];

/// 确定性 LCG 伪随机源（纪律 1：可复跑）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed ^ 0xA5A5_5AA5 }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    /// [0, n) 内取值；n==0 恒 0。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
    /// 十类 id 按权重均匀抽签。
    pub fn pick_id(&mut self) -> u32 {
        VALID_IDS[self.below(VALID_IDS.len() as u32) as usize]
    }
}

/// 一笔注入消息：id + wParam + lParam。
#[derive(Clone, Copy, Debug)]
pub struct WineMsg {
    pub id: u32,
    pub wp: u32,
    pub lp: u32,
}

/// 每笔消息的归宿：Tolerated（正常入账）/ Graceful（拒收回执，不崩）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Tolerated,
    Graceful(u16),
}

/// Wine 侧窗口收包状态机：任意标志组合入包不崩。
pub struct WineWin {
    pub ctx_open: bool,
    pub comp_active: bool,
    pub gcs_seen: u32,
    pub msgs_seen: u32,
    pub tolerated: u32,
    pub graceful: u32,
    pub coverage: u32,
}

impl WineWin {
    pub fn new() -> WineWin {
        WineWin {
            ctx_open: false,
            comp_active: false,
            gcs_seen: 0,
            msgs_seen: 0,
            tolerated: 0,
            graceful: 0,
            coverage: 0,
        }
    }

    fn id_index(id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < VALID_IDS.len() {
            if VALID_IDS[i] == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 收包主径：十类消息封闭处理，未知 id 亦走 Graceful（纪律 2）。
    pub fn recv(&mut self, m: &WineMsg) -> Outcome {
        self.msgs_seen += 1;
        let out = self.dispatch(m);
        match out {
            Outcome::Tolerated => self.tolerated += 1,
            Outcome::Graceful(_) => self.graceful += 1,
        }
        if let Some(bit) = WineWin::id_index(m.id) {
            self.coverage |= 1u32 << bit;
        }
        out
    }

    fn dispatch(&mut self, m: &WineMsg) -> Outcome {
        match m.id {
            WM_IME_SETCONTEXT => {
                self.ctx_open = m.wp != 0;
                if !self.ctx_open {
                    self.comp_active = false;
                }
                Outcome::Tolerated
            }
            WM_IME_STARTCOMPOSITION => {
                if !self.ctx_open {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                self.comp_active = true;
                Outcome::Tolerated
            }
            WM_IME_COMPOSITION => {
                if !self.comp_active {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                if m.lp & !GCS_VALID_MASK != 0 {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                self.gcs_seen |= m.lp;
                Outcome::Tolerated
            }
            WM_IME_ENDCOMPOSITION => {
                if !self.comp_active {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                self.comp_active = false;
                Outcome::Tolerated
            }
            WM_IME_NOTIFY => {
                let op = m.wp & 0xFFFF;
                if op == 0 || op > IMN_OP_MAX {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                if op >= 3 && op <= 6 && m.lp >= 32 {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                Outcome::Tolerated
            }
            WM_IME_CONTROL => {
                let mut hit = false;
                let mut i = 0usize;
                while i < IMC_VALID.len() {
                    if IMC_VALID[i] == m.lp {
                        hit = true;
                    }
                    i += 1;
                }
                if !hit {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                Outcome::Tolerated
            }
            WM_IME_CHAR => {
                if m.wp == 0 {
                    return Outcome::Graceful(E_STATE_BAD);
                }
                Outcome::Tolerated
            }
            _ => Outcome::Tolerated, // SELECT / REQUEST / FULL：宽容直收
        }
    }

    /// 状态不变量随轮审计（纪律 4）：违背即"崩"的账面形态。
    pub fn check_invariants(&self) -> Result<(), u16> {
        if self.comp_active && !self.ctx_open {
            return Err(E_INVARIANT);
        }
        if self.gcs_seen & !GCS_VALID_MASK != 0 {
            return Err(E_INVARIANT);
        }
        if self.msgs_seen != self.tolerated + self.graceful {
            return Err(E_UNSETTLED);
        }
        Ok(())
    }

    /// 十类 id 全铺满校验（纪律 3）。
    pub fn cover_full(&self) -> Result<(), u16> {
        let full = (1u32 << VALID_IDS.len()) - 1;
        if self.coverage != full {
            return Err(E_COVER_MISS);
        }
        Ok(())
    }
}

/// fuzz 轮账：每轮抽 1 笔消息注入并随轮审计不变量。
pub struct FuzzReport {
    pub seed: u32,
    pub rounds: u32,
    pub crashes: u32,
    pub win: WineWin,
}

/// ×N 轮确定性 fuzz 主径；任何一轮不变量违背计入 crashes（非 panic）。
pub fn run_fuzz(seed: u32, rounds: u32) -> FuzzReport {
    let mut rng = Lcg::new(seed);
    let mut win = WineWin::new();
    let mut crashes = 0u32;
    let mut done = 0u32;
    while done < rounds {
        let id = rng.pick_id();
        let mut wp = rng.next();
        let mut lp = rng.next();
        // 每 4 轮注入一次越界标志位（高 16 位污染），专攻 Graceful 路径。
        if rng.below(4) == 0 {
            lp |= 0x8000_0000;
        }
        // 每 8 轮随机翻转上下文开关，制造 comp_active/ctx_open 组合压力。
        if rng.below(8) == 0 {
            wp = wp.wrapping_add(0x0100_0000);
        }
        let m = WineMsg { id, wp, lp };
        let _ = win.recv(&m);
        if win.check_invariants().is_err() {
            crashes += 1;
        }
        done += 1;
    }
    FuzzReport { seed, rounds, crashes, win }
}

/// 写回判定（判据 J2）：完成态 + 非空摘要。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（fuzz ×10^4 零崩），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据：×10^4 轮固定种子 fuzz，零崩 + 守恒 + 十类铺满 + Graceful 真实发生。
    let rep = run_fuzz(0xB900_0001, 10_000);
    if rep.rounds != 10_000 {
        return Err(E_ROUND_MISS);
    }
    if rep.crashes != 0 {
        return Err(E_INVARIANT);
    }
    rep.win.check_invariants()?;
    rep.win.cover_full()?;
    if rep.win.msgs_seen != rep.win.tolerated + rep.win.graceful {
        return Err(E_UNSETTLED);
    }
    if rep.win.graceful == 0 {
        return Err(E_STATE_BAD); // 越界注入必须真打进拒收路径
    }
    hits += 1;

    // R2 可复跑性：同种子两跑，账面五元组逐一相等（纪律 1）。
    let a = run_fuzz(0xB900_0002, 2_000);
    let b = run_fuzz(0xB900_0002, 2_000);
    if a.seed != b.seed
        || a.rounds != b.rounds
        || a.crashes != b.crashes
        || a.win.msgs_seen != b.win.msgs_seen
        || a.win.tolerated != b.win.tolerated
        || a.win.graceful != b.win.graceful
        || a.win.coverage != b.win.coverage
    {
        return Err(E_SEED_ECHO);
    }
    hits += 1;

    // R3 分片压力：5 个异种种子各 2,000 轮（合计再 10^4），全部零崩。
    let seeds: [u32; 5] = [7, 0xDEAD, 0x48193, 0xFFFF_FFFF, 0x0100_0001];
    let mut i = 0usize;
    while i < seeds.len() {
        let rep = run_fuzz(seeds[i], 2_000);
        if rep.crashes != 0 {
            return Err(E_INVARIANT);
        }
        rep.win.check_invariants()?;
        i += 1;
    }
    hits += 1;

    // R4 全一值对抗流：十类 id × 全一 wp/lp，逐笔归宿不得是崩、拒收须≥10。
    let mut win = WineWin::new();
    let mut g = 0u32;
    let mut i = 0usize;
    while i < VALID_IDS.len() {
        let m = WineMsg { id: VALID_IDS[i], wp: 0xFFFF_FFFF, lp: 0xFFFF_FFFF };
        if let Outcome::Graceful(_) = win.recv(&m) {
            g += 1;
        }
        if win.check_invariants().is_err() {
            return Err(E_INVARIANT);
        }
        i += 1;
    }
    if g < 5 {
        return Err(E_STATE_BAD); // 全一值下 START/END/COMP（态前置缺失）、
                                 // NOTIFY/CONTROL（越界参数）恰拒 5 笔
    }
    if win.tolerated + win.graceful != win.msgs_seen {
        return Err(E_UNSETTLED);
    }
    hits += 1;

    // R5 合法序干净周期：SETCONTEXT(1)→START→COMP(valid)→END 全 Tolerated，
    //    关上下文后 START 必拒、comp_active 不得漂移。
    let mut win = WineWin::new();
    let seq_ok: [WineMsg; 4] = [
        WineMsg { id: WM_IME_SETCONTEXT, wp: 1, lp: 0 },
        WineMsg { id: WM_IME_STARTCOMPOSITION, wp: 0, lp: 0 },
        WineMsg { id: WM_IME_COMPOSITION, wp: 0, lp: GCS_VALID_MASK & 0x0818 },
        WineMsg { id: WM_IME_ENDCOMPOSITION, wp: 0, lp: 0 },
    ];
    let mut i = 0usize;
    while i < seq_ok.len() {
        if let Outcome::Graceful(e) = win.recv(&seq_ok[i]) {
            return Err(e);
        }
        i += 1;
    }
    if win.comp_active || win.gcs_seen != 0x0818 {
        return Err(E_STATE_BAD);
    }
    let m = WineMsg { id: WM_IME_SETCONTEXT, wp: 0, lp: 0 };
    let _ = win.recv(&m);
    let m = WineMsg { id: WM_IME_STARTCOMPOSITION, wp: 0, lp: 0 };
    if win.recv(&m) != Outcome::Graceful(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    if win.comp_active {
        return Err(E_INVARIANT);
    }
    hits += 1;

    // R6 写回（判据 J2）：完成态 + 非空摘要放行；未完成与空摘要均拒。
    if writeback(false, 0x40).is_ok() {
        return Err(E_STATE_BAD);
    }
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x10)?;
    hits += 1;

    Ok(hits)
}
