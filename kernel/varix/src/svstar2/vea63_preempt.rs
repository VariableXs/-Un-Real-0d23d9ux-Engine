//! VE-F0063 · 优先级抢占请求通路（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0063`
//!
//! **职责定位（锚点原文）**：优先级抢占请求通路——呈现优先级的抢占请求通路
//! （前台优先于后台），抢占的公平规则（不饿死被抢占者），通路权限
//! （只有合成器可发起）；含抢占请求的频率限制（滥用抢占防抖）。
//!
//! **数据结构（锚点原文）**：抢占通路。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 饿死 → 公平规则（同一受害者在一个窗口内被抢的次数有上限，超限**推迟**
//!   而不是拒绝——拒绝会让真正紧急的前台请求也没法办，推迟只让它换个时刻）；
//! - 越权抢占 → 拒绝（只有合成器可发起，且**越权尝试不消耗合法者的令牌**，
//!   否则一个坏客户端能把额度耗光）；
//! - 通路滥用 → 审计（限流与推迟都入账，条数与计数必须对得上）。
//!
//! **性能逐项分解（锚点原文）**：O(1)。三次表查（槽位/窗口/令牌），零分配。
//!
//! **跨批对接点（锚点原文）**：AC02 公平延续——本模块的窗口参数
//! `FAIR_WINDOW_NS` / `MAX_PREEMPT_PER_WINDOW` 是 AC02 公平调度的**输入**，
//! 两处若不一致，AC02 的「不饿死」承诺就是空话；`fair_params()` 导出这对值
//! 供 AC02 对拍。
//!
//! **无障碍与隐私（锚点原文）**：通路状态读屏可达——`status_line()` 输出
//! 纯文本「谁让位给谁 / 还剩多久 / 是否被限」，不含应用标识。
//!
//! **判据（锚点原文）**：优先抢占、公平规则、权限笼子、饿死防护、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数 ns 定点；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE770 段（0xE770~0xE77F 十六码）。

use alloc::string::String;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE770 段）
// ---------------------------------------------------------------------------

/// 非合成器发起抢占。
pub const XC_NOT_AUTHORIZED: u16 = 0xE770;
/// 同级抢占（前台抢前台）。
pub const XC_SAME_CLASS: u16 = 0xE771;
/// 优先级倒挂（低抢高）。
pub const XC_PRIORITY_INVERSION: u16 = 0xE772;
/// 档位数值越界。
pub const XC_UNKNOWN_CLASS: u16 = 0xE773;
/// 频率限制命中。
pub const XC_THROTTLED: u16 = 0xE774;
/// 公平规则推迟。
pub const XC_FAIR_DEFERRED: u16 = 0xE775;
/// 授予表满。
pub const XC_QUEUE_FULL: u16 = 0xE776;
/// 无有效授予（过期/释放空转）。
pub const XC_NO_ACTIVE_GRANT: u16 = 0xE777;
/// 时长非法。
pub const XC_BAD_DURATION: u16 = 0xE778;
/// 受害者非法（未知/自抢）。
pub const XC_BAD_VICTIM: u16 = 0xE779;
/// 审计满账。
pub const XC_AUDIT_FULL: u16 = 0xE77A;
/// 账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE77B;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 档位数。
pub const CLASS_COUNT: usize = 3;
/// 令牌桶容量（一次突发最多几次抢占请求）。
pub const BURST: u32 = 4;
/// 令牌补充间隔（10ms 补 1 枚 → 稳态 100 次/秒）。
pub const REFILL_NS: u64 = 10_000_000;
/// 公平窗口长度（100ms 一个窗口）。
pub const FAIR_WINDOW_NS: u64 = 100_000_000;
/// 单窗口内同一受害者最多被抢次数。
pub const MAX_PREEMPT_PER_WINDOW: u32 = 3;
/// 单次授予的最长时长（16ms，一帧）。
pub const MAX_DURATION_NS: u64 = 16_000_000;
/// 审计槽位。
pub const AUDIT_CAP: usize = 48;

// ---------------------------------------------------------------------------
// 发起方
// ---------------------------------------------------------------------------

/// 抢占请求的发起方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Requester {
    Compositor = 0,
    Renderer = 1,
    Decode = 2,
    Display = 3,
    Shell = 4,
}

impl Requester {
    /// 是否有权发起抢占（锚点：只有合成器可发起）。
    pub fn may_preempt(self) -> bool {
        matches!(self, Requester::Compositor)
    }

    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Requester::Compositor => "compositor",
            Requester::Renderer => "renderer",
            Requester::Decode => "decode",
            Requester::Display => "display",
            Requester::Shell => "shell",
        }
    }
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 呈现档位（值越小越优先）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Foreground = 0,
    Normal = 1,
    Background = 2,
}

impl Class {
    /// 全部档位（顺序即位序）。
    pub const ALL: [Class; CLASS_COUNT] = [Class::Foreground, Class::Normal, Class::Background];

    /// 优先级秩（越小越优先）。
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// 位序。
    pub fn idx(self) -> usize {
        self as usize
    }

    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Class::Foreground => "foreground",
            Class::Normal => "normal",
            Class::Background => "background",
        }
    }

    /// 由数值取档位（配置/用户态输入必经此门）。
    pub fn from_u8(v: u8) -> Option<Class> {
        match v {
            0 => Some(Class::Foreground),
            1 => Some(Class::Normal),
            2 => Some(Class::Background),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 授予与窗口
// ---------------------------------------------------------------------------

/// 一个受害档位上的授予槽。
#[derive(Clone, Copy)]
pub struct GrantSlot {
    /// 受害者档位（= 槽位键）。
    pub victim: Class,
    /// 受益档位。
    pub beneficiary: Class,
    /// 授予时刻。
    pub issued: u64,
    /// 到期时刻。
    pub expires: u64,
    /// 是否有效。
    pub active: bool,
    /// 该槽累计授予次数（含合并）。
    pub grants: u32,
}

impl GrantSlot {
    /// 空槽。
    pub const EMPTY: GrantSlot = GrantSlot {
        victim: Class::Foreground,
        beneficiary: Class::Background,
        issued: 0,
        expires: 0,
        active: false,
        grants: 0,
    };

    /// 剩余时长（无效时 0）。
    pub fn remaining(&self, now: u64) -> u64 {
        if self.active && self.expires > now {
            self.expires - now
        } else {
            0
        }
    }
}

/// 公平窗口。
#[derive(Clone, Copy)]
pub struct FairWindow {
    /// 窗口起点。
    pub start: u64,
    /// 窗口内已计次数。
    pub count: u32,
}

impl FairWindow {
    /// 空窗口。
    pub const EMPTY: FairWindow = FairWindow { start: 0, count: 0 };
}

/// 审计条目。
#[derive(Clone, Copy)]
pub struct AuditEntry {
    /// 诊断码。
    pub code: u16,
    /// 相关档位（受害者）。
    pub victim: u8,
    /// 发生时刻。
    pub tick: u64,
}

/// 请求结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 已授予（`merged` 为真表示并入既有授予而非新开）。
    Granted {
        victim: Class,
        beneficiary: Class,
        expires_tick: u64,
        merged: bool,
    },
    /// 公平推迟（不是拒绝：换个时刻还能办）。
    Deferred { retry_at: u64, code: u16 },
    /// 限流。
    Throttled { retry_at: u64 },
    /// 拒绝。
    Rejected { code: u16 },
}

// ---------------------------------------------------------------------------
// 抢占通路
// ---------------------------------------------------------------------------

/// 抢占请求通路（单桶、定容、无分配）。
#[derive(Clone, Copy)]
pub struct PreemptChannel {
    slots: [GrantSlot; CLASS_COUNT],
    fair: [FairWindow; CLASS_COUNT],
    tokens: u32,
    last_refill: u64,
    issued: u32,
    granted: u32,
    merged: u32,
    deferred: u32,
    throttled: u32,
    denied_perm: u32,
    denied_prio: u32,
    denied_other: u32,
    expired: u32,
    audit: [AuditEntry; AUDIT_CAP],
    ac: usize,
    audit_dropped: u32,
}

impl PreemptChannel {
    /// 空通路（令牌满桶）。
    pub fn new() -> PreemptChannel {
        PreemptChannel {
            slots: [GrantSlot::EMPTY; CLASS_COUNT],
            fair: [FairWindow::EMPTY; CLASS_COUNT],
            tokens: BURST,
            last_refill: 0,
            issued: 0,
            granted: 0,
            merged: 0,
            deferred: 0,
            throttled: 0,
            denied_perm: 0,
            denied_prio: 0,
            denied_other: 0,
            expired: 0,
            audit: [
                AuditEntry {
                    code: 0,
                    victim: 0,
                    tick: 0,
                };
                AUDIT_CAP
            ],
            ac: 0,
            audit_dropped: 0,
        }
    }

    /// 公平参数（AC02 对拍用）。
    pub fn fair_params() -> (u64, u32) {
        (FAIR_WINDOW_NS, MAX_PREEMPT_PER_WINDOW)
    }

    fn record(&mut self, code: u16, victim: u8, tick: u64) {
        if self.ac < AUDIT_CAP {
            self.audit[self.ac] = AuditEntry {
                code,
                victim,
                tick,
            };
            self.ac += 1;
        } else {
            self.audit_dropped = self.audit_dropped.saturating_add(1);
        }
    }

    /// 审计条数。
    pub fn audit_len(&self) -> usize {
        self.ac
    }

    /// 审计丢弃数。
    pub fn audit_dropped(&self) -> u32 {
        self.audit_dropped
    }

    /// 审计计数。
    pub fn audit_count(&self, code: u16) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.ac {
            if self.audit[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 补充令牌（幂等：只按整周期进位，余数留给下次）。
    fn refill(&mut self, now: u64) {
        if now <= self.last_refill {
            return;
        }
        let elapsed = now - self.last_refill;
        let gained = elapsed / REFILL_NS;
        if gained == 0 {
            return;
        }
        let add = if gained > BURST as u64 {
            BURST as u64
        } else {
            gained
        };
        self.tokens = if self.tokens.saturating_add(add as u32) > BURST {
            BURST
        } else {
            self.tokens + add as u32
        };
        self.last_refill += gained.saturating_mul(REFILL_NS);
    }

    /// 当前令牌数。
    pub fn tokens(&self) -> u32 {
        self.tokens
    }

    /// 惰性过期检查，返回本次过期数（幂等）。
    pub fn expire(&mut self, now: u64) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CLASS_COUNT {
            if self.slots[i].active && now >= self.slots[i].expires {
                self.slots[i].active = false;
                n += 1;
            }
            i += 1;
        }
        self.expired = self.expired.saturating_add(n);
        n
    }

    /// 有效授予数。
    pub fn active_grants(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CLASS_COUNT {
            if self.slots[i].active {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某档位的授予槽。
    pub fn slot(&self, class: Class) -> GrantSlot {
        self.slots[class.idx()]
    }

    /// 发起抢占请求（类型安全入口：档位只能来自 `Class`）。
    pub fn request(
        &mut self,
        who: Requester,
        victim: Class,
        beneficiary: Class,
        duration_ns: u64,
        now: u64,
    ) -> Outcome {
        self.issued = self.issued.saturating_add(1);
        // ① 权限笼子（越权不消耗令牌）
        if !who.may_preempt() {
            self.denied_perm = self.denied_perm.saturating_add(1);
            self.record(XC_NOT_AUTHORIZED, victim.rank(), now);
            return Outcome::Rejected {
                code: XC_NOT_AUTHORIZED,
            };
        }
        // ② 档位关系
        if victim.rank() == beneficiary.rank() {
            self.denied_prio = self.denied_prio.saturating_add(1);
            self.record(XC_SAME_CLASS, victim.rank(), now);
            return Outcome::Rejected {
                code: XC_SAME_CLASS,
            };
        }
        if beneficiary.rank() > victim.rank() {
            self.denied_prio = self.denied_prio.saturating_add(1);
            self.record(XC_PRIORITY_INVERSION, victim.rank(), now);
            return Outcome::Rejected {
                code: XC_PRIORITY_INVERSION,
            };
        }
        self.request_checked(who, victim, beneficiary, duration_ns, now)
    }

    /// 数值入口（用户态/配置文件必经）：档位越界先被挡。
    pub fn request_raw(
        &mut self,
        who: Requester,
        victim_rank: u8,
        beneficiary_rank: u8,
        duration_ns: u64,
        now: u64,
    ) -> Outcome {
        let v = match Class::from_u8(victim_rank) {
            Some(v) => v,
            None => {
                self.issued = self.issued.saturating_add(1);
                self.denied_other = self.denied_other.saturating_add(1);
                self.record(XC_UNKNOWN_CLASS, victim_rank, now);
                return Outcome::Rejected {
                    code: XC_UNKNOWN_CLASS,
                };
            }
        };
        let b = match Class::from_u8(beneficiary_rank) {
            Some(b) => b,
            None => {
                self.issued = self.issued.saturating_add(1);
                self.denied_other = self.denied_other.saturating_add(1);
                self.record(XC_UNKNOWN_CLASS, victim_rank, now);
                return Outcome::Rejected {
                    code: XC_UNKNOWN_CLASS,
                };
            }
        };
        self.request(who, v, b, duration_ns, now)
    }

    fn request_checked(
        &mut self,
        _who: Requester,
        victim: Class,
        beneficiary: Class,
        duration_ns: u64,
        now: u64,
    ) -> Outcome {
        // ③ 时长
        if duration_ns == 0 || duration_ns > MAX_DURATION_NS {
            self.denied_other = self.denied_other.saturating_add(1);
            self.record(XC_BAD_DURATION, victim.rank(), now);
            return Outcome::Rejected {
                code: XC_BAD_DURATION,
            };
        }
        // ④ 惰性过期（到点的授予先收掉，否则「合并」会把过期授予续命）
        let _ = self.expire(now);
        // ⑤ 限流
        self.refill(now);
        if self.tokens == 0 {
            self.throttled = self.throttled.saturating_add(1);
            self.record(XC_THROTTLED, victim.rank(), now);
            return Outcome::Throttled {
                retry_at: self.last_refill.saturating_add(REFILL_NS),
            };
        }
        // ⑥ 公平窗口
        let vi = victim.idx();
        let mut w = self.fair[vi];
        if w.count == 0 || now.saturating_sub(w.start) >= FAIR_WINDOW_NS {
            w.start = now;
            w.count = 0;
        }
        if w.count >= MAX_PREEMPT_PER_WINDOW {
            self.fair[vi] = w;
            self.deferred = self.deferred.saturating_add(1);
            self.record(XC_FAIR_DEFERRED, victim.rank(), now);
            return Outcome::Deferred {
                retry_at: w.start.saturating_add(FAIR_WINDOW_NS),
                code: XC_FAIR_DEFERRED,
            };
        }
        w.count += 1;
        self.fair[vi] = w;
        // ⑦ 授予（同槽已有活动授予则合并，而不是叠第二条）
        self.tokens -= 1;
        let slot = &mut self.slots[vi];
        let mut merged = false;
        let mut base = now;
        let mut expires = now.saturating_add(duration_ns);
        if slot.active {
            merged = true;
            // 合并保留**原始授予时刻**：一次授予的生命期是「授予起 MAX_DURATION_NS」，
            // 合并只能在这次生命期内延长，不能把生命期本身往后推——否则反复合并
            // 等于让被抢占者永远等不到让位（饥饿换了个方向）。
            base = slot.issued;
            if slot.expires > expires {
                expires = slot.expires;
            }
            let cap = base.saturating_add(MAX_DURATION_NS);
            if expires > cap {
                expires = cap;
            }
            self.merged = self.merged.saturating_add(1);
        }
        slot.victim = victim;
        slot.beneficiary = beneficiary;
        slot.issued = base;
        slot.expires = expires;
        slot.active = true;
        slot.grants = slot.grants.saturating_add(1);
        self.granted = self.granted.saturating_add(1);
        Outcome::Granted {
            victim,
            beneficiary,
            expires_tick: expires,
            merged,
        }
    }

    /// 计数快照 `(发起, 授予, 合并, 推迟, 限流, 越权拒, 档位拒, 其它拒, 过期)`。
    pub fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32, u32, u32) {
        (
            self.issued,
            self.granted,
            self.merged,
            self.deferred,
            self.throttled,
            self.denied_perm,
            self.denied_prio,
            self.denied_other,
            self.expired,
        )
    }

    /// 账平衡：发起 ≡ 授予 + 推迟 + 限流 + 三类拒绝。
    pub fn ledger_balanced(&self) -> bool {
        let rhs = self
            .granted
            .saturating_add(self.deferred)
            .saturating_add(self.throttled)
            .saturating_add(self.denied_perm)
            .saturating_add(self.denied_prio)
            .saturating_add(self.denied_other);
        self.issued == rhs
    }

    /// 账不平诊断码。
    pub fn ledger_code(&self) -> u16 {
        if self.ledger_balanced() {
            0
        } else {
            XC_LEDGER_DRIFT
        }
    }

    /// 读屏行：谁让位给谁 / 还剩多久 / 是否被限。
    pub fn status_line(&self, victim: Class, now: u64) -> String {
        let mut s = String::new();
        let slot = self.slots[victim.idx()];
        s.push_str("受害者 ");
        s.push_str(victim.name());
        if slot.active {
            s.push_str(" 让位给 ");
            s.push_str(slot.beneficiary.name());
            s.push_str(" 剩 ");
            push_u64(&mut s, slot.remaining(now));
            s.push_str("ns");
        } else {
            s.push_str(" 无授予");
        }
        let w = self.fair[victim.idx()];
        s.push_str(" 窗口内已抢 ");
        push_u32(&mut s, w.count);
        s.push_str(" 次 限 ");
        s.push_str(if w.count >= MAX_PREEMPT_PER_WINDOW {
            "是"
        } else {
            "否"
        });
        s.push_str(" 令牌 ");
        push_u32(&mut s, self.tokens);
        s
    }
}

// ---------------------------------------------------------------------------
// 文本辅助
// ---------------------------------------------------------------------------

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// VE-F0063 判据入口。
pub fn run_vea63_checks() -> CheckSet {
    let mut set = CheckSet::new("vea63_preempt");

    // C63-01 权限笼子：非合成器一律拒，且不消耗令牌
    {
        let mut c = PreemptChannel::new();
        let a = c.request(Requester::Renderer, Class::Background, Class::Foreground, 4_000_000, 0);
        let b = c.request(Requester::Decode, Class::Background, Class::Foreground, 4_000_000, 0);
        let d = c.request(Requester::Display, Class::Background, Class::Foreground, 4_000_000, 0);
        let e = c.request(Requester::Shell, Class::Background, Class::Foreground, 4_000_000, 0);
        let n = c.counters();
        let ok = a == Outcome::Rejected { code: XC_NOT_AUTHORIZED }
            && b == a
            && d == a
            && e == a
            && n.5 == 4
            && n.1 == 0
            && c.tokens() == BURST
            && c.audit_count(XC_NOT_AUTHORIZED) == 4;
        chk(&mut set, "C63-01 权限笼子", ok, "越权被授予或耗掉令牌");
    }

    // C63-02 前台抢后台：授予且带到期
    {
        let mut c = PreemptChannel::new();
        let r = c.request(Requester::Compositor, Class::Background, Class::Foreground, 4_000_000, 1000);
        let ok = r
            == Outcome::Granted {
                victim: Class::Background,
                beneficiary: Class::Foreground,
                expires_tick: 1000 + 4_000_000,
                merged: false,
            }
            && c.active_grants() == 1
            && c.slot(Class::Background).beneficiary == Class::Foreground;
        chk(&mut set, "C63-02 前台抢后台", ok, "授予内容或到期错");
    }

    // C63-03 中间档也能抢后台
    {
        let mut c = PreemptChannel::new();
        let r = c.request(Requester::Compositor, Class::Background, Class::Normal, 2_000_000, 0);
        let ok = matches!(r, Outcome::Granted { merged: false, .. })
            && c.slot(Class::Background).remaining(0) == 2_000_000;
        chk(&mut set, "C63-03 普通档抢后台", ok, "中间档被误拒");
    }

    // C63-04 同级被拒
    {
        let mut c = PreemptChannel::new();
        let r = c.request(Requester::Compositor, Class::Foreground, Class::Foreground, 1_000_000, 0);
        let n = c.counters();
        let ok = r == Outcome::Rejected { code: XC_SAME_CLASS }
            && n.6 == 1
            && c.active_grants() == 0
            && c.audit_count(XC_SAME_CLASS) == 1;
        chk(&mut set, "C63-04 同级拒绝", ok, "同级被授予");
    }

    // C63-05 低抢高被拒（绝不允许后台抢前台）
    {
        let mut c = PreemptChannel::new();
        let r = c.request(Requester::Compositor, Class::Foreground, Class::Background, 1_000_000, 0);
        let r2 = c.request(Requester::Compositor, Class::Normal, Class::Background, 1_000_000, 0);
        let ok = r == Outcome::Rejected { code: XC_PRIORITY_INVERSION }
            && r2 == r
            && c.counters().6 == 2
            && c.active_grants() == 0;
        chk(&mut set, "C63-05 优先级倒挂拒绝", ok, "低抢高被放行");
    }

    // C63-06 数值入口档位越界
    {
        let mut c = PreemptChannel::new();
        let a = c.request_raw(Requester::Compositor, 9, 0, 1_000_000, 0);
        let b = c.request_raw(Requester::Compositor, 2, 7, 1_000_000, 0);
        let good = c.request_raw(Requester::Compositor, 2, 0, 1_000_000, 0);
        let n = c.counters();
        let ok = a == Outcome::Rejected { code: XC_UNKNOWN_CLASS }
            && b == a
            && matches!(good, Outcome::Granted { .. })
            && n.7 == 2
            && n.1 == 1
            && c.tokens() == BURST - 1;
        chk(&mut set, "C63-06 档位数值校验", ok, "越界数值被当成合法档");
    }

    // C63-07 时长边界（0 与 MAX+1 拒，MAX 放行）
    {
        let mut c = PreemptChannel::new();
        let z = c.request(Requester::Compositor, Class::Background, Class::Foreground, 0, 0);
        let over = c.request(
            Requester::Compositor,
            Class::Background,
            Class::Foreground,
            MAX_DURATION_NS + 1,
            0,
        );
        let ok_max = c.request(
            Requester::Compositor,
            Class::Background,
            Class::Foreground,
            MAX_DURATION_NS,
            0,
        );
        let ok = z == Outcome::Rejected { code: XC_BAD_DURATION }
            && over == z
            && matches!(ok_max, Outcome::Granted { .. })
            && c.counters().7 == 2;
        chk(&mut set, "C63-07 时长边界", ok, "边界值误判");
    }

    // C63-08 令牌桶耗尽即限流并给重试时刻
    {
        let mut c = PreemptChannel::new();
        let mut granted = 0u32;
        let mut i = 0u32;
        while i < BURST {
            // 不同受害者轮流，避开公平窗口
            let v = if i % 2 == 0 { Class::Background } else { Class::Normal };
            if let Outcome::Granted { .. } = c.request(Requester::Compositor, v, Class::Foreground, 1_000_000, 0) {
                granted += 1;
            }
            i += 1;
        }
        let over = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, 0);
        let ok = granted == BURST
            && over == Outcome::Throttled { retry_at: REFILL_NS }
            && c.counters().4 == 1
            && c.audit_count(XC_THROTTLED) == 1;
        chk(&mut set, "C63-08 限流与重试时刻", ok, "限流值或重试时刻错");
    }

    // C63-09 补充令牌后恢复一次
    {
        let mut c = PreemptChannel::new();
        let mut i = 0u32;
        while i < BURST {
            let v = if i % 2 == 0 { Class::Background } else { Class::Normal };
            let _ = c.request(Requester::Compositor, v, Class::Foreground, 1_000_000, 0);
            i += 1;
        }
        let before = c.tokens();
        let after_one = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, REFILL_NS);
        let ok = before == 0
            && matches!(after_one, Outcome::Granted { .. })
            && c.tokens() == 0;
        chk(&mut set, "C63-09 令牌补充", ok, "补充时机或数量错");
    }

    // C63-10 补充不越过桶容量
    {
        let mut c = PreemptChannel::new();
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, 0);
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, 1_000_000_000);
        let ok = c.tokens() >= BURST - 1 && c.tokens() <= BURST;
        let mut d = PreemptChannel::new();
        let _ = d.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, 0);
        let _ = d.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, REFILL_NS * 100);
        let ok = ok && d.tokens() == BURST - 1;
        chk(&mut set, "C63-10 桶容量上限", ok, "令牌溢出桶容量");
    }

    // C63-11 滥用留痕与计数一致
    {
        let mut c = PreemptChannel::new();
        let mut i = 0u32;
        while i < BURST + 5 {
            let v = if i % 2 == 0 { Class::Background } else { Class::Normal };
            let _ = c.request(Requester::Compositor, v, Class::Foreground, 1_000_000, 0);
            i += 1;
        }
        let n = c.counters();
        let ok = n.4 == 5 && c.audit_count(XC_THROTTLED) == 5 && n.1 == BURST;
        chk(&mut set, "C63-11 滥用留痕", ok, "限流次数与审计不符");
    }

    // C63-12 公平规则：同受害者窗口内第 MAX+1 次被推迟
    {
        let mut c = PreemptChannel::new();
        let now = 50_000_000u64;
        let mut g = 0u32;
        let mut i = 0u32;
        while i < MAX_PREEMPT_PER_WINDOW {
            if let Outcome::Granted { .. } =
                c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, now)
            {
                g += 1;
            }
            i += 1;
        }
        let d = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, now);
        let ok = g == MAX_PREEMPT_PER_WINDOW
            && d
                == Outcome::Deferred {
                    retry_at: now + FAIR_WINDOW_NS,
                    code: XC_FAIR_DEFERRED,
                }
            && c.counters().3 == 1;
        chk(&mut set, "C63-12 公平推迟", ok, "饿死防护未生效");
    }

    // C63-13 推迟不占额度（令牌留给真正要办的请求）
    {
        let mut c = PreemptChannel::new();
        let mut i = 0u32;
        while i < MAX_PREEMPT_PER_WINDOW {
            let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, 0);
            i += 1;
        }
        let tokens_before = c.tokens();
        let d = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, 0);
        let ok = matches!(d, Outcome::Deferred { .. })
            && c.tokens() == tokens_before
            && c.counters().1 == MAX_PREEMPT_PER_WINDOW;
        chk(&mut set, "C63-13 推迟不占额度", ok, "推迟反而扣了令牌");
    }

    // C63-14 窗口滚动后恢复
    {
        let mut c = PreemptChannel::new();
        let now = 1_000_000_000u64;
        let mut i = 0u32;
        while i < MAX_PREEMPT_PER_WINDOW {
            let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, now);
            i += 1;
        }
        let d = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, now);
        let later = now + FAIR_WINDOW_NS;
        let r = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, later);
        let w = c.slot(Class::Background);
        let ok = matches!(d, Outcome::Deferred { .. })
            && matches!(r, Outcome::Granted { .. })
            && w.grants == MAX_PREEMPT_PER_WINDOW + 1;
        chk(&mut set, "C63-14 窗口滚动恢复", ok, "窗口不滚动即永久饿死");
    }

    // C63-15 不同受害者互不影响
    {
        let mut c = PreemptChannel::new();
        let now = 7_000_000u64;
        let mut i = 0u32;
        while i < MAX_PREEMPT_PER_WINDOW {
            let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, now);
            i += 1;
        }
        let blocked = c.request(Requester::Compositor, Class::Background, Class::Foreground, 1_000_000, now);
        let other = c.request(Requester::Compositor, Class::Normal, Class::Foreground, 1_000_000, now);
        let ok = matches!(blocked, Outcome::Deferred { .. }) && matches!(other, Outcome::Granted { .. });
        chk(&mut set, "C63-15 受害者互不影响", ok, "一个受害者拖垮全部");
    }

    // C63-16 同槽合并而非叠授予
    {
        let mut c = PreemptChannel::new();
        let a = c.request(Requester::Compositor, Class::Background, Class::Foreground, 2_000_000, 0);
        let b = c.request(Requester::Compositor, Class::Background, Class::Foreground, 3_000_000, 1_000_000);
        let ok = matches!(a, Outcome::Granted { merged: false, .. })
            && matches!(b, Outcome::Granted { merged: true, .. })
            && c.active_grants() == 1
            && c.slot(Class::Background).expires == 1_000_000 + 3_000_000
            && c.counters().2 == 1;
        chk(&mut set, "C63-16 同槽合并", ok, "同槽叠出两条授予");
    }

    // C63-17 合并不顺延生命期（反复合并不得让被抢占者永远等不到让位）
    {
        let mut c = PreemptChannel::new();
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, MAX_DURATION_NS, 0);
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, MAX_DURATION_NS, 1_000_000);
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, MAX_DURATION_NS, 2_000_000);
        let e = c.slot(Class::Background).expires;
        let exp = c.expire(MAX_DURATION_NS);
        let ok = e == MAX_DURATION_NS && exp == 1 && c.active_grants() == 0;
        chk(&mut set, "C63-17 合并不顺延生命期", ok, "合并把生命期往后推");
    }

    // C63-18 到期失效与幂等
    {
        let mut c = PreemptChannel::new();
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 2_000_000, 100);
        let early = c.expire(2_000_099);
        let late = c.expire(2_000_100);
        let again = c.expire(2_000_200);
        let ok = early == 0
            && late == 1
            && again == 0
            && c.active_grants() == 0
            && c.counters().8 == 1;
        chk(&mut set, "C63-18 到期失效幂等", ok, "重复过期计数或边界错");
    }

    // C63-19 状态行三段齐备
    {
        let mut c = PreemptChannel::new();
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 2_000_000, 0);
        let line = c.status_line(Class::Background, 500_000);
        let ok = line.contains("让位给") && line.contains("剩 ") && line.contains("限 ") && line.contains("令牌");
        chk(&mut set, "C63-19 状态行可读", ok, "读屏字段缺失");
    }

    // C63-20 账平衡（混合全路径）
    {
        let mut c = PreemptChannel::new();
        let _ = c.request(Requester::Shell, Class::Background, Class::Foreground, 1_000_000, 0);
        let _ = c.request(Requester::Compositor, Class::Foreground, Class::Foreground, 1_000_000, 0);
        let _ = c.request(Requester::Compositor, Class::Foreground, Class::Background, 1_000_000, 0);
        let _ = c.request_raw(Requester::Compositor, 5, 0, 1_000_000, 0);
        let _ = c.request(Requester::Compositor, Class::Background, Class::Foreground, 0, 0);
        let mut i = 0u32;
        while i < BURST + 2 {
            let v = if i % 2 == 0 { Class::Background } else { Class::Normal };
            let _ = c.request(Requester::Compositor, v, Class::Foreground, 1_000_000, 0);
            i += 1;
        }
        let n = c.counters();
        let ok = c.ledger_balanced()
            && c.ledger_code() == 0
            && n.0 == 5 + BURST + 2
            && n.6 == 2
            && c.audit_dropped() == 0;
        chk(&mut set, "C63-20 账平衡", ok, "计数与账不符");
    }

    set
}

/// 单点演练。
pub fn drill() -> (bool, String) {
    let mut c = PreemptChannel::new();
    let mut ok = true;
    let mut s = String::new();
    if !matches!(
        c.request(Requester::Compositor, Class::Background, Class::Foreground, MAX_DURATION_NS, 0),
        Outcome::Granted { .. }
    ) {
        ok = false;
        s.push_str("grant ");
    }
    if matches!(
        c.request(Requester::Compositor, Class::Background, Class::Foreground, MAX_DURATION_NS, 0),
        Outcome::Granted { .. }
    ) {
        ok = false;
        s.push_str("fair ");
    }
    if !c.ledger_balanced() {
        ok = false;
        s.push_str("ledger ");
    }
    if s.is_empty() {
        s.push_str("抢占通路正常：");
        s.push_str(&c.status_line(Class::Background, 0));
    }
    (ok, s)
}
