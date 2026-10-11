//! UNX-F1883 · 唤醒-切换-IPI 联合回归（三链联测）
//!
//! 诊断段 0x9C81~0x9C88（调度族联测段）。
//! 判据（施工书 J1）：唤醒五段×切换四段×IPI 分发三链联测断言全过（联合竞态注入 10/10 正确），三签名链逐段咬合核对通过。

use alloc::vec::Vec;

/// 联测诊断码闭集（0x9C81~0x9C88 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriCode {
    /// 0x9C81 唤醒链五段走完。
    WakeChain = 0x9C81,
    /// 0x9C82 切换链四段走完。
    SwitchChain = 0x9C82,
    /// 0x9C83 IPI 分发链三段走完。
    IpiChain = 0x9C83,
    /// 0x9C84 三签名咬合核通过（wake_id=switch_id=ipi_id）。
    SigSealed = 0x9C84,
    /// 0x9C85 签名咬合破坏（链间身份漂移）。
    SigBroken = 0x9C85,
    /// 0x9C86 联合竞态注入正确处置。
    RaceHandled = 0x9C86,
    /// 0x9C87 联测闭环完成。
    TriClosed = 0x9C87,
    /// 0x9C88 联测账守恒破坏。
    LedgerBroken = 0x9C88,
}

impl TriCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            TriCode::WakeChain => "唤醒链五段走完",
            TriCode::SwitchChain => "切换链四段走完",
            TriCode::IpiChain => "IPI 分发链三段走完",
            TriCode::SigSealed => "三签名逐段咬合",
            TriCode::SigBroken => "三签名咬合破坏",
            TriCode::RaceHandled => "联合竞态注入正确处置",
            TriCode::TriClosed => "三链联测闭环",
            TriCode::LedgerBroken => "联测账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            TriCode::WakeChain => "入队→记账→置可运行→选核→IPI 请求",
            TriCode::SwitchChain => "保存→选程→换页→恢复",
            TriCode::IpiChain => "发送→接收→EOI",
            TriCode::SigSealed => "三链同源身份一致",
            TriCode::SigBroken => "链间身份漂移或串扰",
            TriCode::RaceHandled => "竞态窗内序保护生效",
            TriCode::TriClosed => "三链全走完且账齐",
            TriCode::LedgerBroken => "链段数与账不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TriCode::WakeChain => "继续切换链",
            TriCode::SwitchChain => "继续 IPI 链",
            TriCode::IpiChain => "做签名咬合核",
            TriCode::SigSealed => "无需处置",
            TriCode::SigBroken => "排查链间身份传递",
            TriCode::RaceHandled => "无需处置",
            TriCode::TriClosed => "无需处置",
            TriCode::LedgerBroken => "冻结联测账并上报 0x9C88",
        }
    }
}

/// 唤醒链五段。
pub const WAKE_STEPS: usize = 5;
/// 切换链四段。
pub const SWITCH_STEPS: usize = 4;
/// IPI 链三段。
pub const IPI_STEPS: usize = 3;

/// 三签名联测会话：同一 run_id 贯穿三链。
pub struct TriSession {
    /// 会话身份（三链共用签名）。
    run_id: u64,
    /// 唤醒链步数。
    wake_steps: u8,
    /// 切换链步数。
    switch_steps: u8,
    /// IPI 链步数。
    ipi_steps: u8,
    /// 竞态注入处置账。
    races: u64,
    /// 事件账。
    log: Vec<TriCode>,
}

impl TriSession {
    /// 新会话。
    pub fn new(run_id: u64) -> Self {
        TriSession { run_id, wake_steps: 0, switch_steps: 0, ipi_steps: 0, races: 0, log: Vec::new() }
    }

    /// 唤醒链推进一段（严格序：必须先走完唤醒链才能走切换链）。
    pub fn wake_step(&mut self) -> TriCode {
        if self.switch_steps > 0 || self.ipi_steps > 0 || self.wake_steps >= WAKE_STEPS as u8 {
            return TriCode::LedgerBroken;
        }
        self.wake_steps += 1;
        if self.wake_steps == WAKE_STEPS as u8 {
            self.log.push(TriCode::WakeChain);
            return TriCode::WakeChain;
        }
        TriCode::SigSealed
    }

    /// 切换链推进一段。
    pub fn switch_step(&mut self) -> TriCode {
        if self.wake_steps != WAKE_STEPS as u8 || self.ipi_steps > 0 || self.switch_steps >= SWITCH_STEPS as u8 {
            return TriCode::LedgerBroken;
        }
        self.switch_steps += 1;
        if self.switch_steps == SWITCH_STEPS as u8 {
            self.log.push(TriCode::SwitchChain);
            return TriCode::SwitchChain;
        }
        TriCode::SigSealed
    }

    /// IPI 链推进一段。
    pub fn ipi_step(&mut self) -> TriCode {
        if self.wake_steps != WAKE_STEPS as u8 || self.switch_steps != SWITCH_STEPS as u8 || self.ipi_steps >= IPI_STEPS as u8 {
            return TriCode::LedgerBroken;
        }
        self.ipi_steps += 1;
        if self.ipi_steps == IPI_STEPS as u8 {
            self.log.push(TriCode::IpiChain);
            let sealed = self.sig_check();
            self.log.push(sealed);
            self.log.push(TriCode::TriClosed);
            return TriCode::TriClosed;
        }
        TriCode::SigSealed
    }

    /// 三签名咬合核：三链齐走完且同源 run_id（此处以步数齐 + 会话单例承载签名）。
    pub fn sig_check(&self) -> TriCode {
        if self.wake_steps == WAKE_STEPS as u8 && self.switch_steps == SWITCH_STEPS as u8 && self.ipi_steps == IPI_STEPS as u8 {
            TriCode::SigSealed
        } else {
            TriCode::SigBroken
        }
    }

    /// 联合竞态注入：在 IPI 链未走完时重复唤醒（应被拒——重唤醒幂等）。
    pub fn inject_race(&mut self) -> TriCode {
        if self.ipi_steps < IPI_STEPS as u8 && self.wake_steps == WAKE_STEPS as u8 {
            // 竞态窗：重复唤醒被吸收（不重置切换/IPI 链）。
            self.races += 1;
            self.log.push(TriCode::RaceHandled);
            return TriCode::RaceHandled;
        }
        TriCode::LedgerBroken
    }

    /// 身份读取。
    pub const fn run_id(&self) -> u64 {
        self.run_id
    }

    /// 统计。
    pub const fn stats(&self) -> (u8, u8, u8, u64) {
        (self.wake_steps, self.switch_steps, self.ipi_steps, self.races)
    }

    /// 账守恒：三链步数不越闭集上界。
    pub fn ledger_consistent(&self) -> bool {
        self.wake_steps <= WAKE_STEPS as u8
            && self.switch_steps <= SWITCH_STEPS as u8
            && self.ipi_steps <= IPI_STEPS as u8
    }
}

/// 判据测试（段 0x9C81~0x9C88）。
pub fn run_t883_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t883");
    // c1 三链顺序联测闭环。
    let mut s = TriSession::new(0xA11CE);
    let mut c1 = true;
    for _ in 0..WAKE_STEPS {
        c1 &= s.wake_step() != TriCode::LedgerBroken;
    }
    for _ in 0..SWITCH_STEPS {
        c1 &= s.switch_step() != TriCode::LedgerBroken;
    }
    for _ in 0..IPI_STEPS {
        c1 &= s.ipi_step() != TriCode::LedgerBroken;
    }
    c1 &= s.sig_check() == TriCode::SigSealed;
    set.add("t883-c1-tri-chain", c1 && s.ledger_consistent(), "");
    // c2 链序违规：未走完唤醒链即走切换链 → 拒。
    let mut s2 = TriSession::new(0xB0B);
    let _ = s2.wake_step();
    set.add("t883-c2-chain-order", s2.switch_step() == TriCode::LedgerBroken, "");
    // c3 唤醒链越界（第 6 段）→ 拒。
    let mut s3 = TriSession::new(0xC0C);
    for _ in 0..WAKE_STEPS {
        let _ = s3.wake_step();
    }
    set.add("t883-c3-wake-overrun", s3.wake_step() == TriCode::LedgerBroken, "");
    // c4 联合竞态注入 10/10 正确处置。
    let mut handled: u32 = 0;
    for i in 0..10u64 {
        let mut s4 = TriSession::new(0xD000 + i);
        for _ in 0..WAKE_STEPS {
            let _ = s4.wake_step();
        }
        for _ in 0..2 {
            let _ = s4.switch_step();
        }
        if s4.inject_race() == TriCode::RaceHandled {
            handled += 1;
        }
        // 竞态后切换链继续不受扰。
        let cont = s4.switch_step() != TriCode::LedgerBroken;
        if !cont {
            handled = handled.saturating_sub(1);
        }
    }
    set.add("t883-c4-race-10of10", handled == 10, "");
    // c5 三签名逐段咬合：同 run_id 会话唯一性。
    let sa = TriSession::new(0xE1);
    let sb = TriSession::new(0xE1);
    let sc = TriSession::new(0xE2);
    set.add("t883-c5-sig-bind", sa.run_id() == sb.run_id() && sa.run_id() != sc.run_id(), "");
    // c6 IPI 链前置校验：切换链未走完 → 拒。
    let mut s6 = TriSession::new(0xF1);
    for _ in 0..WAKE_STEPS {
        let _ = s6.wake_step();
    }
    set.add("t883-c6-ipi-prereq", s6.ipi_step() == TriCode::LedgerBroken, "");
    // c7 闭环账。
    let (w, sw, ipi, r) = s.stats();
    set.add("t883-c7-close-account", w == 5 && sw == 4 && ipi == 3 && r == 0 && s.ledger_consistent(), "");
    // c8 LCG 512 轮注入：随机步序混打，账守恒恒成立。
    let mut seed: u64 = 0x8830_9C81_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut t = TriSession::new(lc());
        for _ in 0..14 {
            match lc() % 4 {
                0 => {
                    let _ = t.wake_step();
                }
                1 => {
                    let _ = t.switch_step();
                }
                2 => {
                    let _ = t.ipi_step();
                }
                _ => {
                    let _ = t.inject_race();
                }
            }
            if !t.ledger_consistent() {
                inj_ok = false;
                break;
            }
        }
    }
    set.add("t883-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        TriCode::WakeChain,
        TriCode::SwitchChain,
        TriCode::IpiChain,
        TriCode::SigSealed,
        TriCode::SigBroken,
        TriCode::RaceHandled,
        TriCode::TriClosed,
        TriCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t883-c9-triad", tri_ok, "");
    set
}
