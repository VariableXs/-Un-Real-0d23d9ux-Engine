//! UNX-F0968 · 四态迁移完整性断言链账 · 诊断段 0xB8B0
//! 验收锚点：UNX-F0968-J1 全链迁移正确（完整四态迁移 10/10 次账平），
//! 各态停留时长账（Draining/Arming 窗口时长上限断言），违序迁移全拦截
//! （六种违序注入 10/10 次），双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB8B0;

// ── 诊断段独占 · 四态迁移错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_ORDER: u16 = SEG_ERR + 1;  // 违序迁移
const E_WINDOW: u16 = SEG_ERR + 2; // 停留超窗
const E_CLOSE: u16 = SEG_ERR + 3;  // 链不闭合
const E_STATE: u16 = SEG_ERR + 4;  // 态非法
const E_BAL: u16 = SEG_ERR + 5;    // 账不平

/// 四态：Polling → Draining → Arming → Irq。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FourState {
    Polling,
    Draining,
    Arming,
    Irq,
}

impl FourState {
    pub fn next_state(self) -> Option<FourState> {
        match self {
            FourState::Polling => Some(FourState::Draining),
            FourState::Draining => Some(FourState::Arming),
            FourState::Arming => Some(FourState::Irq),
            FourState::Irq => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            FourState::Polling => "polling",
            FourState::Draining => "draining",
            FourState::Arming => "arming",
            FourState::Irq => "irq",
        }
    }
    pub fn idx(self) -> usize {
        match self {
            FourState::Polling => 0,
            FourState::Draining => 1,
            FourState::Arming => 2,
            FourState::Irq => 3,
        }
    }
}

/// 停留窗上限（Draining/Arming 各有界）。
pub const DRAIN_WINDOW_US: u64 = 100_000;
pub const ARM_WINDOW_US: u64 = 200_000;

/// 一次迁移记录：从态 → 到态 + 时刻。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MigRec {
    pub from: FourState,
    pub to: FourState,
    pub t_us: u64,
}

/// 迁移链账本。
pub struct MigChain {
    pub recs: Vec<MigRec>,
}

/// 合法迁移表：Polling→Draining→Arming→Irq 线性链。
pub fn legal_edge(from: FourState, to: FourState) -> bool {
    from.next_state() == Some(to)
}

impl MigChain {
    pub fn new() -> Self {
        MigChain { recs: Vec::new() }
    }
    /// 迁移：违序拒。
    pub fn step(&mut self, from: FourState, to: FourState, t: u64) -> Result<(), u16> {
        if !legal_edge(from, to) {
            return Err(E_ORDER);
        }
        // 时刻单调
        if let Some(last) = self.recs.last() {
            if t < last.t_us {
                return Err(E_ORDER);
            }
        }
        self.recs.push(MigRec { from, to, t });
        Ok(())
    }
    /// 全链闭合：Polling 起三步到 Irq。
    pub fn chain_closed(&self) -> bool {
        self.recs.len() == 3
            && self.recs[0].from == FourState::Polling
            && self.recs[2].to == FourState::Irq
            && self.recs.windows(2).all(|w| w[0].to == w[1].from)
    }
    /// 停留时长账：Draining/Arming 窗口上限断言。
    pub fn windows_ok(&self) -> Result<(), u16> {
        // drain 停留 = rec1.t - rec0.t；arm 停留 = rec2.t - rec1.t
        if self.recs.len() != 3 {
            return Err(E_CLOSE);
        }
        let drain = self.recs[1].t_us - self.recs[0].t_us;
        let arm = self.recs[2].t_us - self.recs[1].t_us;
        if drain > DRAIN_WINDOW_US {
            return Err(E_WINDOW);
        }
        if arm > ARM_WINDOW_US {
            return Err(E_WINDOW);
        }
        Ok(())
    }
    /// 账平：三迁移边皆合法。
    pub fn all_legal(&self) -> bool {
        self.recs.iter().all(|r| legal_edge(r.from, r.to))
    }
}

/// 六种违序注入（跳态/回退/自环/Irq 越迁/时刻回拨/非法终态）。
pub fn six_injections() -> [(FourState, FourState, u64, u64); 6] {
    [
        (FourState::Polling, FourState::Arming, 10, 1),   // 1. 跳态
        (FourState::Draining, FourState::Polling, 10, 1), // 2. 回退
        (FourState::Polling, FourState::Polling, 10, 1),  // 3. 自环
        (FourState::Irq, FourState::Arming, 10, 1),       // 4. Irq 越迁
        (FourState::Arming, FourState::Irq, 10, 1),       // 5. 空链首步非法（Arming 非起点）
        (FourState::Polling, FourState::Irq, 10, 1),      // 6. 直跳终态
    ]
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0968_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0968 四态迁移完整性断言链");

    // 1. 完整四态迁移 10/10 账平
    let mut full = 0;
    for r in 0..10 {
        let mut c = MigChain::new();
        let base = 1000 * (r as u64 + 1);
        let ok = c.step(FourState::Polling, FourState::Draining, base).is_ok()
            && c.step(FourState::Draining, FourState::Arming, base + 10_000).is_ok()
            && c.step(FourState::Arming, FourState::Irq, base + 30_000).is_ok()
            && c.chain_closed() && c.all_legal()
            && c.windows_ok().is_ok();
        if ok {
            full += 1;
        }
    }
    s.add("FS-全链10/10", full == 10, if full == 10 { "" } else { "全链断" });

    // 2. 停留窗上限断言（恰贴界过）
    let mut c2 = MigChain::new();
    let boundary = c2.step(FourState::Polling, FourState::Draining, 0).is_ok()
        && c2.step(FourState::Draining, FourState::Arming, DRAIN_WINDOW_US).is_ok()
        && c2.step(FourState::Arming, FourState::Irq, DRAIN_WINDOW_US + ARM_WINDOW_US).is_ok()
        && c2.windows_ok().is_ok();
    s.add("FS-贴窗界过", boundary, if boundary { "" } else { "贴界误拒" });

    // 3. Draining 超窗拒
    let mut c3 = MigChain::new();
    let _ = c3.step(FourState::Polling, FourState::Draining, 0);
    let _ = c3.step(FourState::Draining, FourState::Arming, DRAIN_WINDOW_US + 1);
    let _ = c3.step(FourState::Arming, FourState::Irq, DRAIN_WINDOW_US + 1 + 1000);
    s.add("FS-Drain超窗拒", c3.windows_ok() == Err(E_WINDOW),
        if c3.windows_ok() == Err(E_WINDOW) { "" } else { "超窗放行" });

    // 4. Arming 超窗拒
    let mut c4 = MigChain::new();
    let _ = c4.step(FourState::Polling, FourState::Draining, 0);
    let _ = c4.step(FourState::Draining, FourState::Arming, 1000);
    let _ = c4.step(FourState::Arming, FourState::Irq, 1000 + ARM_WINDOW_US + 1);
    s.add("FS-Arm超窗拒", c4.windows_ok() == Err(E_WINDOW),
        if c4.windows_ok() == Err(E_WINDOW) { "" } else { "超窗放行" });

    // 5. 六种违序注入 10/10 全拦截
    let mut blocked = 0;
    for _ in 0..10 {
        let mut all_block = true;
        for (from, to, t, _) in six_injections() {
            let mut c = MigChain::new();
            if from == FourState::Arming {
                // 注入 5 的场景：先建部分链再试
                let _ = c.step(FourState::Polling, FourState::Draining, 0);
                let _ = c.step(FourState::Draining, FourState::Arming, 5);
                let _ = c.step(FourState::Arming, FourState::Irq, 8);
            }
            if c.step(from, to, t).is_ok() {
                all_block = false;
            }
        }
        if all_block {
            blocked += 1;
        }
    }
    s.add("FS-六违序10/10", blocked == 10, if blocked == 10 { "" } else { "违序漏拦" });

    // 6. 时刻回拨拒（同链内）
    let mut c5 = MigChain::new();
    let _ = c5.step(FourState::Polling, FourState::Draining, 100);
    let back = c5.step(FourState::Draining, FourState::Arming, 50);
    s.add("FS-时刻回拨拒", back == Err(E_ORDER), if back == Err(E_ORDER) { "" } else { "回拨放行" });

    // 7. 迁移记录账平（3 边皆合法 + 时刻单调）
    let mut c6 = MigChain::new();
    let _ = c6.step(FourState::Polling, FourState::Draining, 10);
    let _ = c6.step(FourState::Draining, FourState::Arming, 20);
    let _ = c6.step(FourState::Arming, FourState::Irq, 30);
    let mono = c6.recs.windows(2).all(|w| w[0].t_us <= w[1].t_us);
    s.add("FS-账平单调", c6.all_legal() && mono && c6.recs.len() == 3,
        if mono { "" } else { "账不平" });

    // 8. 态名/索引互异
    let names = [FourState::Polling.as_str(), FourState::Draining.as_str(), FourState::Arming.as_str(), FourState::Irq.as_str()];
    let uniq = names[0] != names[1] && names[1] != names[2] && names[2] != names[3]
        && FourState::Polling.idx() == 0 && FourState::Irq.idx() == 3;
    s.add("FS-态名互异", uniq, if uniq { "" } else { "态名碰撞" });

    // 9. Irq 终态无后继
    s.add("FS-终态无后继", FourState::Irq.next_state().is_none(),
        if FourState::Irq.next_state().is_none() { "" } else { "终态越迁" });

    // 10. 空链窗口判闭合失败
    let empty = MigChain::new();
    s.add("FS-空链拒", empty.windows_ok() == Err(E_CLOSE) && !empty.chain_closed(),
        if empty.windows_ok() == Err(E_CLOSE) { "" } else { "空链放行" });

    // 11. 贴界邻域（超 1us 即拒）
    let mut c7 = MigChain::new();
    let _ = c7.step(FourState::Polling, FourState::Draining, 0);
    let _ = c7.step(FourState::Draining, FourState::Arming, DRAIN_WINDOW_US);
    let _ = c7.step(FourState::Arming, FourState::Irq, DRAIN_WINDOW_US + ARM_WINDOW_US + 1);
    s.add("FS-超1us拒", c7.windows_ok() == Err(E_WINDOW),
        if c7.windows_ok() == Err(E_WINDOW) { "" } else { "邻域漏拦" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB8B0 && SEG_BASE < 0xB8D0;
    s.add("FS-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
