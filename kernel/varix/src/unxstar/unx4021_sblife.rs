// unx4021 —— UNX-F4021 Superblock 生命周期三态（allocating/active/kill）
// 判据原文 UNX-F4021-J1：sb 三态迁移（allocating→active→kill）状态机全路径
// 断言通过，非法迁移注入（active→allocating）100/100 次被拒绝并留账。
// 落位：kernel/varix/src/unxstar/；诊断段 0xD000 独占（0xD000~0xD07F）。
//
// 设计（纯功能 no_std）：
//   * 三态闭集：Allocating → Active → Kill（单向生命线）；
//   * 合法迁移闸：仅两条前向边；回退/跳段全部拒绝留账；
//   * 100/100 注入：active→allocating 非法迁移百次全拒 + 每次留痕；
//   * 注入核：回退迁移/跳段/kill 后操作/未初始化迁移 四类必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xD000 独占） ─────────────────────────────────────────────
pub const XC_SB_BACK: u32 = 0xD001; // 回退迁移
pub const XC_SB_SKIP: u32 = 0xD002; // 跳段迁移
pub const XC_SB_DEAD: u32 = 0xD003; // kill 后操作
pub const XC_SB_INIT: u32 = 0xD004; // 未初始化迁移

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// sb 三态闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SbState {
    Allocating,
    Active,
    Kill,
}

impl SbState {
    pub fn name(self) -> &'static str {
        match self {
            SbState::Allocating => "allocating",
            SbState::Active => "active",
            SbState::Kill => "kill",
        }
    }
    pub fn idx(self) -> usize {
        match self {
            SbState::Allocating => 0,
            SbState::Active => 1,
            SbState::Kill => 2,
        }
    }
}

/// Superblock 生命周期状态机。
pub struct Superblock {
    state: SbState,
    events: Vec<(u8, &'static str)>, // (迁移码, 说明)
    rejects: Vec<(u32, &'static str)>,
}

impl Superblock {
    pub fn new() -> Self {
        Superblock {
            state: SbState::Allocating,
            events: Vec::new(),
            rejects: Vec::new(),
        }
    }

    /// 前向迁移（合法边闸：allocating→active, active→kill）。
    pub fn advance(&mut self) -> Result<SbState, u32> {
        match self.state {
            SbState::Allocating => {
                self.state = SbState::Active;
                self.events.push((1, "allocating->active"));
                Ok(self.state)
            }
            SbState::Active => {
                self.state = SbState::Kill;
                self.events.push((2, "active->kill"));
                Ok(self.state)
            }
            SbState::Kill => {
                self.rejects.push((XC_SB_DEAD, "kill 后操作"));
                Err(XC_SB_DEAD)
            }
        }
    }

    /// 非法迁移注入面：试图回退到 allocating（active→allocating 拒）。
    pub fn try_regress(&mut self) -> Result<(), u32> {
        if self.state == SbState::Allocating {
            self.rejects.push((XC_SB_INIT, "allocating 无回退语义"));
            return Err(XC_SB_INIT);
        }
        self.rejects.push((XC_SB_BACK, "回退迁移"));
        Err(XC_SB_BACK)
    }

    /// 跳段注入面：试图 allocating→kill 直跳（拒）。
    pub fn try_skip(&mut self) -> Result<(), u32> {
        if self.state != SbState::Allocating {
            self.rejects.push((XC_SB_SKIP, "非 allocating 态跳段"));
            return Err(XC_SB_SKIP);
        }
        self.rejects.push((XC_SB_SKIP, "跳段迁移"));
        Err(XC_SB_SKIP)
    }

    /// 全路径断言：allocating→active→kill 全走通。
    pub fn full_path(&mut self) -> bool {
        self.state == SbState::Allocating
            && self.advance() == Ok(SbState::Active)
            && self.advance() == Ok(SbState::Kill)
    }

    pub fn state(&self) -> SbState {
        self.state
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 100/100 非法迁移注入（active→allocating 百次全拒留账）。
pub fn regress_100() -> (bool, u64) {
    let mut all_rej = true;
    let mut rej_total: u64 = 0;
    for _ in 0..100 {
        let mut sb = Superblock::new();
        let _ = sb.advance(); // → Active
        if sb.try_regress().is_ok() {
            all_rej = false;
        }
        rej_total += sb.reject_count() as u64;
    }
    (all_rej, rej_total)
}

/// 注入核：回退/跳段/kill 后操作/重复 kill 四类必抓。
pub struct SbInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl SbInjectCore {
    pub fn new() -> Self {
        SbInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_regress(&mut self) {
        self.total += 1;
        let mut sb = Superblock::new();
        let _ = sb.advance();
        if sb.try_regress().is_err() && sb.state() == SbState::Active {
            // 拒绝后状态不变
            self.caught += 1;
        }
    }
    pub fn inject_skip(&mut self) {
        self.total += 1;
        let mut sb = Superblock::new();
        let r = sb.try_skip(); // allocating→kill 直跳
        if r.is_err() && sb.state() == SbState::Allocating {
            self.caught += 1;
        }
    }
    pub fn inject_after_kill(&mut self) {
        self.total += 1;
        let mut sb = Superblock::new();
        let _ = sb.advance();
        let _ = sb.advance(); // → Kill
        let a = sb.advance();
        let r = sb.try_regress();
        if a.is_err() && r.is_err() && sb.reject_count() == 2 {
            self.caught += 1;
        }
    }
    pub fn inject_double_regress_trail(&mut self) {
        self.total += 1;
        let mut sb = Superblock::new();
        let _ = sb.advance();
        let _ = sb.try_regress();
        let _ = sb.try_regress();
        // 两次回退各留一账
        if sb.reject_count() == 2 && sb.event_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx4021_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx4021");

    // C21-01 三态全路径断言
    {
        let mut sb = Superblock::new();
        let ok = sb.full_path() && sb.state() == SbState::Kill;
        s.add("C21-01-全路径", ok, if ok { "" } else { "路径断" });
    }
    // C21-02 三态闭集语义
    {
        let ok = SbState::Allocating.idx() == 0
            && SbState::Active.idx() == 1
            && SbState::Kill.idx() == 2
            && SbState::Active.name() == "active";
        s.add("C21-02-三态闭集", ok, if ok { "" } else { "态集漂移" });
    }
    // C21-03 非法迁移 100/100 全拒
    {
        let (ok, rej) = regress_100();
        s.add(
            "C21-03-回退100拒",
            ok && rej == 100,
            if ok { "" } else { "回退放行" },
        );
    }
    // C21-04 迁移留痕（全路径恰两事件）
    {
        let mut sb = Superblock::new();
        let _ = sb.full_path();
        let ok = sb.event_count() == 2 && sb.reject_count() == 0;
        s.add("C21-04-留痕", ok, if ok { "" } else { "留痕断" });
    }
    // C21-05 状态机拒绝后状态不变
    {
        let mut sb = Superblock::new();
        let _ = sb.advance();
        let _ = sb.try_skip();
        let ok = sb.state() == SbState::Active;
        s.add("C21-05-拒后不变", ok, if ok { "" } else { "拒后漂移" });
    }
    // C21-06 注入核 4/4
    {
        let mut inj = SbInjectCore::new();
        inj.inject_regress();
        inj.inject_skip();
        inj.inject_after_kill();
        inj.inject_double_regress_trail();
        s.add(
            "C21-06-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
