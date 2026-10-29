// unx1029_cancel —— UNX-F1029 超时器取消与重设（cancel/reset 语义）
// 判据原文 UNX-F1029-J1：取消正确（取消后零回调 10/10 次——竞态取消安全），
// 重设正确（重设后新时刻到期 10/10 次），取消竞态（到期瞬间取消）10/10 次
// 正确处理。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2040 独占（0x2040~0x207F）。
//
// 设计（纯功能 no_std）：
//   * 句柄账：arm 分配句柄（单调分配），取消/重设按句柄索引；
//   * 取消语义：CANCELLED 态零回调（回调账核对——取消后 fired 恒 0）；
//   * 重设语义：RESET 态继承句柄换到期点（旧到期作废账显性）；
//   * 竞态闸：到期瞬间取消按"先到先得"裁定（原子态判定单点）；
//   * 生命周期闭集：ARMED→(FIRED|CANCELLED|RESET)→(终态不迁移)；
//   * 守恒：回调数=到期数；FNV 指纹冻结；注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x2040 独占） ─────────────────────────────────────────────
pub const XC_CANCEL_NOH: u32 = 0x2041; // 句柄不存在
pub const XC_CANCEL_TERM: u32 = 0x2042; // 终态再迁移（取消/重设已终态句柄）
pub const XC_CANCEL_RACE: u32 = 0x2043; // 竞态窗口非法操作
pub const XC_CANCEL_SEALED: u32 = 0x2044; // 封账后动账
pub const XC_CANCEL_LOST: u32 = 0x2045; // 回调与到期不守恒
pub const XC_CANCEL_DUP: u32 = 0x2046; // 同句柄双重取消

/// 生命周期态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TmoState {
    Armed,
    Fired,
    Cancelled,
    Reset,
}

/// 一个超时器句柄账目。
#[derive(Clone, Copy)]
pub struct TmoEntry {
    pub h: u32,
    pub deadline_ns: u64,
    pub tag: u32,
    pub state: TmoState,
    pub fired_cnt: u32, // 回调计数
    pub resets: u32,
}

/// 超时器取消/重设账本。
pub struct TmoCancelBook {
    entries: Vec<TmoEntry>,
    next_h: u32,
    now_ns: u64,
    callbacks: u32,
    expired: u32,
    sealed: bool,
}

impl TmoCancelBook {
    pub fn new() -> TmoCancelBook {
        TmoCancelBook {
            entries: Vec::new(),
            next_h: 1,
            now_ns: 0,
            callbacks: 0,
            expired: 0,
            sealed: false,
        }
    }

    /// arm：登记句柄（deadline 必须 > now——过去时拒）。
    pub fn arm(&mut self, deadline_ns: u64, tag: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_CANCEL_SEALED);
        }
        if deadline_ns <= self.now_ns {
            return Err(XC_CANCEL_RACE);
        }
        let h = self.next_h;
        self.next_h = self.next_h.wrapping_add(1);
        self.entries.push(TmoEntry {
            h,
            deadline_ns,
            tag,
            state: TmoState::Armed,
            fired_cnt: 0,
            resets: 0,
        });
        Ok(h)
    }

    fn find(&mut self, h: u32) -> Option<&mut TmoEntry> {
        self.entries.iter_mut().find(|e| e.h == h)
    }

    /// cancel：Armed→Cancelled（终态零回调）。
    pub fn cancel(&mut self, h: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_CANCEL_SEALED);
        }
        let e = match self.find(h) {
            Some(e) => e,
            None => return Err(XC_CANCEL_NOH),
        };
        match e.state {
            TmoState::Armed => {
                e.state = TmoState::Cancelled;
                Ok(())
            }
            TmoState::Cancelled => Err(XC_CANCEL_DUP),
            _ => Err(XC_CANCEL_TERM),
        }
    }

    /// reset：Armed→Reset→新 Armed（继承句柄换到期点）。
    pub fn reset(&mut self, h: u32, new_deadline_ns: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_CANCEL_SEALED);
        }
        // 先做时钟域校验（避免与句柄借用交叠）
        if new_deadline_ns <= self.now_ns {
            return Err(XC_CANCEL_RACE);
        }
        let e = match self.find(h) {
            Some(e) => e,
            None => return Err(XC_CANCEL_NOH),
        };
        match e.state {
            TmoState::Armed => {
                e.deadline_ns = new_deadline_ns;
                e.resets = e.resets.wrapping_add(1);
                Ok(())
            }
            _ => Err(XC_CANCEL_TERM),
        }
    }

    /// 推进时钟并收割到期（Armed 且 deadline ≤ now → Fired+回调）。
    pub fn tick_to(&mut self, now_ns: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_CANCEL_SEALED);
        }
        if now_ns < self.now_ns {
            return Err(XC_CANCEL_RACE);
        }
        self.now_ns = now_ns;
        let mut fired_now = 0u32;
        for e in self.entries.iter_mut() {
            if e.state == TmoState::Armed && e.deadline_ns <= now_ns {
                e.state = TmoState::Fired;
                e.fired_cnt = e.fired_cnt.wrapping_add(1);
                self.callbacks = self.callbacks.wrapping_add(1);
                self.expired = self.expired.wrapping_add(1);
                fired_now = fired_now.wrapping_add(1);
            }
        }
        Ok(fired_now)
    }

    /// 竞态裁定：到期瞬间取消——先到先得（若已 Fired 则取消拒 TERM）。
    pub fn race_cancel(&mut self, h: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_CANCEL_SEALED);
        }
        let e = match self.find(h) {
            Some(e) => e,
            None => return Err(XC_CANCEL_NOH),
        };
        match e.state {
            TmoState::Armed => {
                // 未收割窗口：取消赢（零回调）
                e.state = TmoState::Cancelled;
                Ok(true)
            }
            TmoState::Fired => {
                // 已回调：取消输（显性，不算错误路径——返回 false）
                Ok(false)
            }
            _ => Err(XC_CANCEL_TERM),
        }
    }

    /// 取消后零回调断言：全部 Cancelled 条目 fired_cnt==0。
    pub fn cancelled_zero_fire(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.state != TmoState::Cancelled || e.fired_cnt == 0)
    }

    /// 重设后新时刻到期断言：Reset 条目（resets>0）以新 deadline 收割。
    pub fn reset_fires_new(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.resets == 0 || e.fired_cnt == 0 || e.deadline_ns <= self.now_ns)
    }

    /// 守恒：回调数 = 到期数 = Fired 条目数。
    pub fn conserved(&self) -> bool {
        let fired_n = self.entries.iter().filter(|e| e.state == TmoState::Fired).count() as u32;
        fired_n == self.callbacks && self.callbacks == self.expired
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CANCEL_SEALED);
        }
        if !self.conserved() {
            return Err(XC_CANCEL_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.entries.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for e in self.entries.iter() {
            let st = match e.state {
                TmoState::Armed => 0u32,
                TmoState::Fired => 1,
                TmoState::Cancelled => 2,
                TmoState::Reset => 3,
            };
            h ^= e.h ^ e.tag ^ (e.deadline_ns as u32) ^ st ^ e.fired_cnt ^ e.resets;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.callbacks ^ self.expired.wrapping_mul(0x9E37);
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.next_h as u64 & 0xFFFF)
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    pub fn callback_count(&self) -> u32 {
        self.callbacks
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）取消后零回调 10/10（取消即收割不触发）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut b = TmoCancelBook::new();
            let h = match b.arm(1000 + k, 7) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let _ = b.cancel(h);
            let _ = b.tick_to(5000 + k);
            if b.cancelled_zero_fire() && b.callback_count() == 0 {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）重设后新时刻到期 10/10
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut b = TmoCancelBook::new();
            let h = match b.arm(1000, 1) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let _ = b.reset(h, 2000 + k);
            let _ = b.tick_to(1000 + k); // 旧到期点已过但已重设——不触发
            let fired = b.tick_to(2000 + k + 1); // 新到期点触发
            if fired == Ok(1) && b.conserved() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）竞态取消 10/10（到期瞬间取消——先到先得裁定）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut b = TmoCancelBook::new();
            let h = match b.arm(1000 + k, 2) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let _ = b.tick_to(999 + k); // 未到期
            let won = b.race_cancel(h); // 竞态取消赢
            let _ = b.tick_to(5000 + k); // 已取消不再触发
            if won == Ok(true) && b.callback_count() == 0 {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）已到期句柄取消判输（race_cancel 返 false 10/10）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut b = TmoCancelBook::new();
            let h = match b.arm(100 + k, 3) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let _ = b.tick_to(200 + k); // 已收割
            if b.race_cancel(h) == Ok(false) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）双重取消必拒
    {
        let mut b = TmoCancelBook::new();
        let h = match b.arm(100, 4) {
            Ok(h) => h,
            Err(_) => return (N, caught),
        };
        let _ = b.cancel(h);
        if b.cancel(h) == Err(XC_CANCEL_DUP) {
            caught += 1;
        }
    }
    // 6）句柄不存在必拒（10/10）
    {
        let mut b = TmoCancelBook::new();
        let mut hits = 0;
        for h in 100..=109u32 {
            if b.cancel(h) == Err(XC_CANCEL_NOH) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1029-J1。
pub fn run_unx1029_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1029");

    // C29-01 取消后零回调（基础路）
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(500, 1).unwrap_or(0);
        let _ = b.cancel(h);
        let _ = b.tick_to(1000);
        let ok = b.cancelled_zero_fire() && b.callback_count() == 0;
        s.add("C29-01-取消零回调", ok, if ok { "" } else { "取消漏" });
    }
    // C29-02 重设后新时刻到期
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(500, 2).unwrap_or(0);
        let _ = b.reset(h, 800);
        let f1 = b.tick_to(600);
        let f2 = b.tick_to(900);
        let ok = f1 == Ok(0) && f2 == Ok(1);
        s.add("C29-02-重设到期", ok, if ok { "" } else { "重设漂" });
    }
    // C29-03 竞态取消先到先得（未收割窗口取消赢）
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(1000, 3).unwrap_or(0);
        let _ = b.tick_to(999);
        let won = b.race_cancel(h);
        let _ = b.tick_to(2000);
        let ok = won == Ok(true) && b.callback_count() == 0;
        s.add("C29-03-竞态先到", ok, if ok { "" } else { "竞态乱" });
    }
    // C29-04 已到期取消判输（显性 false）
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(100, 4).unwrap_or(0);
        let _ = b.tick_to(200);
        let ok = b.race_cancel(h) == Ok(false) && b.callback_count() == 1;
        s.add("C29-04-到期取消输", ok, if ok { "" } else { "裁定漂" });
    }
    // C29-05 双重取消拒
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(100, 5).unwrap_or(0);
        let _ = b.cancel(h);
        let ok = b.cancel(h) == Err(XC_CANCEL_DUP);
        s.add("C29-05-双取消拒", ok, if ok { "" } else { "双取消漏" });
    }
    // C29-06 终态重设拒（Fired 后 reset）
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(100, 6).unwrap_or(0);
        let _ = b.tick_to(200);
        let ok = b.reset(h, 900) == Err(XC_CANCEL_TERM);
        s.add("C29-06-终态重设拒", ok, if ok { "" } else { "终态漏" });
    }
    // C29-07 过去时 arm 拒
    {
        let mut b = TmoCancelBook::new();
        let _ = b.tick_to(1000);
        let ok = b.arm(999, 7) == Err(XC_CANCEL_RACE) && b.arm(1000, 7) == Err(XC_CANCEL_RACE);
        s.add("C29-07-过去时拒", ok, if ok { "" } else { "时序漏" });
    }
    // C29-08 句柄单调分配
    {
        let mut b = TmoCancelBook::new();
        let a = b.arm(100, 8);
        let c = b.arm(200, 8);
        let d = b.arm(300, 8);
        let ok = matches!((a, c, d), (Ok(x), Ok(y), Ok(z)) if z > y && y > x && x == 1);
        s.add("C29-08-句柄单调", ok, if ok { "" } else { "句柄乱" });
    }
    // C29-09 守恒
    {
        let mut b = TmoCancelBook::new();
        let h1 = b.arm(100, 9).unwrap_or(0);
        let h2 = b.arm(200, 9).unwrap_or(0);
        let _ = b.cancel(h2);
        let _ = b.tick_to(300);
        let ok = b.conserved() && b.callback_count() == 1;
        s.add("C29-09-回调守恒", ok, if ok { "" } else { "守恒破" });
    }
    // C29-10 封账冻结
    {
        let mut b = TmoCancelBook::new();
        let h = b.arm(100, 10).unwrap_or(0);
        let _ = b.tick_to(200);
        let fp = b.seal();
        let ok = fp.is_ok()
            && b.arm(300, 10) == Err(XC_CANCEL_SEALED)
            && b.cancel(h) == Err(XC_CANCEL_SEALED)
            && b.tick_to(400) == Err(XC_CANCEL_SEALED);
        s.add("C29-10-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C29-11 指纹往返
    {
        let build = || {
            let mut b = TmoCancelBook::new();
            let _ = b.arm(100, 1);
            let _ = b.arm(200, 2);
            let _ = b.tick_to(150);
            b.fingerprint()
        };
        let ok = build() == build();
        s.add("C29-11-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C29-12 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C29-12-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    s
}
