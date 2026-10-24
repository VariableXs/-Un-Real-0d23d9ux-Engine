//! vcs03_session — CGPU-F2883 用户会话管理（生命周期/并发上限/超时回收）。
//!
//! 判据原文（施工书锚点 CGPU-F2883）：会话：会话（用户会话管理（会话生命
//! 周期/并发上限/超时回收——会话实现；状态机（会话完整状态机——状态复用；
//! 测试（生命周期/并发/回收三组。判据：三机制、状态复用、三组、判据。
//!
//! 职责定位：会话三机制+状态机复用。**状态复用**双层兑现：其一，会话状态
//! 机复用 F2882 [`UserState`] 的状态机纪律（闭集枚举+单向终态+迁移表全
//! 枚举显性合法/非法——cgr05 隔离格同款）；其二，开侧直接消费 [`UserState`]
//! （停权用户禁开会话——会话挂用户生命周期，生命周期不健康会话不开张）。
//! 并发上限（每用户 2——超限显性拒不静默挤掉旧会话）；超时回收（扫描式
//! sweep，idle 超时单向终态 Expired——回收是终态不是删除，账目可审计）。
//! 时间戳上游注入（零墙钟——同 P 域纪律）。诊断码段 0x52（字符串码）。
use alloc::format;
use alloc::string::String;

use alloc::vec::Vec;

use crate::checks::CheckSet;

// 状态复用（F2882 单源——判据"状态复用"：编译期 use 即证明）。
use crate::cgpu::vcs02_multiuser::UserState;

/// 模块版本。
pub const SESSION_VERSION: &str = "S03-session-v1";

/// 并发上限（每用户同时活跃会话数——判据"并发上限"）。
pub const CONCURRENCY_LIMIT: usize = 2;

/// 空闲超时（ns——超过即回收候选；恰值不回收，> 才回收）。
pub const IDLE_TIMEOUT_NS: u64 = 30_000_000_000;

/// 诊断码（字符串码五码闭集）。
pub const E_SS_CONCURRENCY: &str = "E_SS_CONCURRENCY";
pub const E_SS_SUSPENDED: &str = "E_SS_SUSPENDED";
pub const E_SS_UNKNOWN: &str = "E_SS_UNKNOWN";
pub const E_SS_TERMINAL: &str = "E_SS_TERMINAL";
pub const E_SS_UNREGISTERED: &str = "E_SS_UNREGISTERED";

/// 诊断详情（五码闭集逐码中文说明）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_SS_CONCURRENCY => "并发超限：每用户活跃会话已满上限，显性拒绝不静默挤掉旧会话",
        E_SS_SUSPENDED => "用户停权中：会话冻结语义——停权不开新张（复用 F2882 生命周期）",
        E_SS_UNKNOWN => "会话不存在：查询/操作落空——账里没有就是没有",
        E_SS_TERMINAL => "会话已终态：Expired/LoggedOut 单向不可逆，迁移显性拒",
        E_SS_UNREGISTERED => "用户未注册：会话必须挂在在册用户上",
        _ => "未知会话诊断码",
    }
}

/// 会话状态机（判据"状态复用"：复用 UserState 纪律——闭集+单向终态+
/// 迁移表全枚举显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    /// 活跃（刚有活动）。
    Active,
    /// 空闲（可被超时回收，也可被活动唤醒回 Active）。
    Idle,
    /// 超时回收（单向终态——回收不是删除，账目可审计）。
    Expired,
    /// 主动登出（单向终态）。
    LoggedOut,
}

impl SessionState {
    pub const ALL: [SessionState; 4] = [
        SessionState::Active,
        SessionState::Idle,
        SessionState::Expired,
        SessionState::LoggedOut,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            SessionState::Active => "活跃",
            SessionState::Idle => "空闲",
            SessionState::Expired => "超时回收",
            SessionState::LoggedOut => "已登出",
        }
    }

    /// 单向终态判定（复用 UserState 注销单向纪律——终态不可迁出）。
    pub const fn is_terminal(self) -> bool {
        matches!(self, SessionState::Expired | SessionState::LoggedOut)
    }

    /// 迁移表全枚举（显性合法 Some 新态 / 非法 None——判据"状态机"本体）。
    /// Active→Idle（超时）/Active→LoggedOut（登出）；
    /// Idle→Active（唤醒）/Idle→Expired（超时回收）/Idle→LoggedOut（登出）；
    /// 终态无出边。
    pub const fn transition(self, ev: Transition) -> Option<SessionState> {
        match (self, ev) {
            (SessionState::Active, Transition::GoIdle) => Some(SessionState::Idle),
            (SessionState::Active, Transition::Logout) => Some(SessionState::LoggedOut),
            (SessionState::Active, Transition::Wake) => Some(SessionState::Active),
            (SessionState::Idle, Transition::Wake) => Some(SessionState::Active),
            (SessionState::Idle, Transition::GoIdle) => Some(SessionState::Idle),
            (SessionState::Idle, Transition::Expire) => Some(SessionState::Expired),
            (SessionState::Idle, Transition::Logout) => Some(SessionState::LoggedOut),
            _ => None,
        }
    }
}

/// 迁移事件（触发状态机的外部事件闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transition {
    /// 活动（触活唤醒）。
    Wake,
    /// 空闲化（无活动）。
    GoIdle,
    /// 超时回收。
    Expire,
    /// 主动登出。
    Logout,
}

/// 会话（判据"会话实现"的最小完备行）。
#[derive(Clone, Copy, Debug)]
pub struct Session {
    /// 会话号（单调分配）。
    pub sid: u64,
    /// 归属用户。
    pub user: u32,
    /// 状态。
    pub state: SessionState,
    /// 最近活动时刻（ns——上游注入，零墙钟）。
    pub last_active_ns: u64,
    /// 开设时刻。
    pub opened_ns: u64,
}

/// 会话管理器（三机制本体：生命周期/并发上限/超时回收）。
pub struct SessionManager {
    /// 会话账（唯一权威——查询走账不走副本）。
    book: Vec<Session>,
    next_sid: u64,
    /// 计数（opened/closed/swept/rejected——四计数对账）。
    pub opened: u32,
    pub closed: u32,
    pub swept: u32,
    pub rejected: u32,
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager { book: Vec::new(), next_sid: 1, opened: 0, closed: 0, swept: 0, rejected: 0 }
    }

    /// 开会话（判据"生命周期"起点 + "并发上限" + "状态复用"消费侧）：
    /// 用户未注册拒 / 停权拒 / 并发满拒；通过则 Active 入账。
    pub fn open(&mut self, user: u32, user_state: Option<UserState>, now_ns: u64) -> Result<u64, &'static str> {
        let st = match user_state {
            None => {
                self.rejected += 1;
                return Err(E_SS_UNREGISTERED);
            }
            Some(st) => st,
        };
        if matches!(st, UserState::Suspended) {
            self.rejected += 1;
            return Err(E_SS_SUSPENDED);
        }
        if matches!(st, UserState::Removed) {
            // 注销终态（复用 UserState 单向纪律）：不开新张，重注册走新 id。
            self.rejected += 1;
            return Err(E_SS_UNREGISTERED);
        }
        let mut live = 0usize;
        for s in &self.book {
            if s.user == user && s.state == SessionState::Active {
                live += 1;
            }
        }
        if live >= CONCURRENCY_LIMIT {
            self.rejected += 1;
            return Err(E_SS_CONCURRENCY);
        }
        let sid = self.next_sid;
        self.next_sid += 1;
        self.book.push(Session {
            sid,
            user,
            state: SessionState::Active,
            last_active_ns: now_ns,
            opened_ns: now_ns,
        });
        self.opened += 1;
        Ok(sid)
    }

    /// 触活（Active 保持/Idle 唤醒——迁移表驱动，非法迁移显性拒）。
    pub fn touch(&mut self, sid: u64, now_ns: u64) -> Result<SessionState, &'static str> {
        let idx = self.find(sid).ok_or(E_SS_UNKNOWN)?;
        let cur = self.book[idx].state;
        let next = cur.transition(Transition::Wake).ok_or(E_SS_TERMINAL)?;
        self.book[idx].state = next;
        self.book[idx].last_active_ns = now_ns;
        Ok(next)
    }

    /// 主动登出（判据"生命周期"终点之一——单向终态）。
    pub fn close(&mut self, sid: u64) -> Result<SessionState, &'static str> {
        let idx = self.find(sid).ok_or(E_SS_UNKNOWN)?;
        let cur = self.book[idx].state;
        let next = cur.transition(Transition::Logout).ok_or(E_SS_TERMINAL)?;
        self.book[idx].state = next;
        if cur != SessionState::LoggedOut {
            self.closed += 1;
        }
        Ok(next)
    }

    /// 超时回收扫描（判据"超时回收"——O(会话数)；idle > 超时线即
    /// Expired 终态；恰值不回收 > 才回收；Active 也直接超时回收——
    /// 空闲超时的记账口径是 last_active 距今，与名义状态无关）。
    pub fn sweep(&mut self, now_ns: u64) -> u32 {
        let mut n = 0u32;
        for s in &mut self.book {
            if s.state.is_terminal() {
                continue;
            }
            let idle = now_ns.saturating_sub(s.last_active_ns);
            if idle > IDLE_TIMEOUT_NS {
                s.state = SessionState::Expired;
                n += 1;
            }
        }
        self.swept += n;
        n
    }

    /// 查会话（只读——账上没有就是没有）。
    pub fn get(&self, sid: u64) -> Option<Session> {
        let idx = self.find(sid)?;
        Some(self.book[idx])
    }

    /// 活跃会话数（并发上限的对账口）。
    pub fn active_of(&self, user: u32) -> usize {
        let mut n = 0usize;
        for s in &self.book {
            if s.user == user && s.state == SessionState::Active {
                n += 1;
            }
        }
        n
    }

    /// 账面会话总数（含终态——回收不是删除）。
    pub fn len(&self) -> usize {
        self.book.len()
    }

    pub fn is_empty(&self) -> bool {
        self.book.is_empty()
    }

    fn find(&self, sid: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].sid == sid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "会话{} 账{} 开{} 登出{} 回收{} 拒{}",
            SESSION_VERSION,
            self.book.len(),
            self.opened,
            self.closed,
            self.swept,
            self.rejected,
        )
    }
}

/// 判据主入口（注册于 cgpu/mod.rs blocks 数组，tag CGPU-F2883）。
pub fn run_vcs03_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-f2883");

    // --- 生命周期组（判据一）---
    // 命-01：完整生命周期——开（Active）→登出（LoggedOut 终态）；
    // 终态后 touch 显性拒 E_SS_TERMINAL（单向不可逆——复用 UserState
    // 注销单向纪律）；查无会话 E_SS_UNKNOWN。
    let mut m = SessionManager::new();
    let sid = m.open(1, Some(UserState::Active), 100);
    let sid = match sid {
        Ok(v) => v,
        Err(_) => 0,
    };
    let opened_ok = sid != 0 && m.get(sid).map(|x| x.state) == Some(SessionState::Active);
    let out1 = m.close(sid);
    let logout_ok = matches!(out1, Ok(SessionState::LoggedOut));
    let t1 = m.touch(sid, 200);
    let terminal_ok = matches!(t1, Err(E_SS_TERMINAL));
    let unknown_ok = matches!(m.touch(9999, 300), Err(E_SS_UNKNOWN));
    if opened_ok && logout_ok && terminal_ok && unknown_ok {
        s.ok("F2883-命-01-生命周期与单向终态");
    } else {
        s.fail("F2883-命-01-生命周期与单向终态", "生命周期失效");
    }

    // --- 生命周期组（判据二）---
    // 命-02：迁移表全枚举对拍——合法边（Active→Idle/Active→Logout/Active→Wake 自环保活/
    // Idle→Wake/Idle→Expire）Some 恰值；非法边（终态出边/Active→Wake
    // 恒等自环不设——Wake 只对 Idle）None；Idle→GoIdle 自环保留。
    let st = SessionState::Active;
    let t_active_idle = st.transition(Transition::GoIdle) == Some(SessionState::Idle);
    let t_active_logout = st.transition(Transition::Logout) == Some(SessionState::LoggedOut);
    let t_active_wake = st.transition(Transition::Wake) == Some(SessionState::Active);
    let t_active_expire = st.transition(Transition::Expire).is_none();
    let idle = SessionState::Idle;
    let t_idle_wake = idle.transition(Transition::Wake) == Some(SessionState::Active);
    let t_idle_expire = idle.transition(Transition::Expire) == Some(SessionState::Expired);
    let t_idle_logout = idle.transition(Transition::Logout) == Some(SessionState::LoggedOut);
    let t_idle_idle = idle.transition(Transition::GoIdle) == Some(SessionState::Idle);
    let t_expired_wake = SessionState::Expired.transition(Transition::Wake).is_none();
    let t_out_logout = SessionState::LoggedOut.transition(Transition::Logout).is_none();
    let term_flags = SessionState::Expired.is_terminal() && SessionState::LoggedOut.is_terminal()
        && !SessionState::Active.is_terminal() && !SessionState::Idle.is_terminal();
    let table_ok = t_active_idle
        && t_active_logout
        && t_active_wake
        && t_active_expire
        && t_idle_wake
        && t_idle_expire
        && t_idle_logout
        && t_idle_idle
        && t_expired_wake
        && t_out_logout
        && term_flags;
    if table_ok {
        s.ok("F2883-命-02-迁移表全枚举显性");
    } else {
        s.fail("F2883-命-02-迁移表全枚举显性", "迁移表失效");
    }

    // --- 并发组（判据三）---
    // 并-01：并发上限恰值——上限 2：第一二个成功、第三个显性拒
    // E_SS_CONCURRENCY（不静默挤掉旧会话——旧会话仍 Active）；
    // 登出一个后可再开（上限是活账不是死账）。
    let mut m2 = SessionManager::new();
    let a = m2.open(7, Some(UserState::Active), 0);
    let b = m2.open(7, Some(UserState::Active), 0);
    let c = m2.open(7, Some(UserState::Active), 0);
    let two_ok = a.is_ok() && b.is_ok() && matches!(c, Err(E_SS_CONCURRENCY));
    let active2 = m2.active_of(7) == 2;
    let _ = m2.close(a.unwrap_or(0));
    let d = m2.open(7, Some(UserState::Active), 10);
    let refill_ok = d.is_ok() && m2.active_of(7) == 2;
    if two_ok && active2 && refill_ok {
        s.ok("F2883-并-01-并发上限恰值与活账");
    } else {
        s.fail("F2883-并-01-并发上限恰值与活账", "并发上限失效");
    }

    // --- 并发组（判据四）---
    // 并-02：状态复用消费侧——未注册用户（None）拒 E_SS_UNREGISTERED；
    // 停权用户拒 E_SS_SUSPENDED（会话冻结语义——不开新张）；注销用户
    // （Removed 终态——重注册走新 id）按未注册口径拒；在册 Active 放行。
    let mut m3 = SessionManager::new();
    let r_unreg = m3.open(9, None, 0);
    let r_susp = m3.open(9, Some(UserState::Suspended), 0);
    let r_deact = m3.open(9, Some(UserState::Removed), 0);
    let r_act = m3.open(9, Some(UserState::Active), 0);
    let reuse_ok = matches!(r_unreg, Err(E_SS_UNREGISTERED))
        && matches!(r_susp, Err(E_SS_SUSPENDED))
        && matches!(r_deact, Err(E_SS_UNREGISTERED))
        && r_act.is_ok()
        && m3.rejected == 3;
    if reuse_ok {
        s.ok("F2883-并-02-用户生命周期复用消费");
    } else {
        s.fail("F2883-并-02-用户生命周期复用消费", "状态复用失效");
    }

    // --- 回收组（判据五）---
    // 收-01：超时回收——IDLE_TIMEOUT=30s：last_active=0 时 sweep(30s)
    // 恰值不回收（> 才回收）、sweep(30s+1) 回收 1 条 Expired 终态；
    // 账面仍在（回收不是删除——len 不减）；重复 sweep 不重复计数
    // （终态跳过）。
    let mut m4 = SessionManager::new();
    let s1 = m4.open(3, Some(UserState::Active), 0);
    let s1 = s1.unwrap_or(0);
    let edge0 = m4.sweep(30_000_000_000);
    let edge_ok = edge0 == 0 && m4.get(s1).map(|x| x.state) == Some(SessionState::Active);
    let edge1 = m4.sweep(30_000_000_001);
    let expired_ok = edge1 == 1
        && m4.get(s1).map(|x| x.state) == Some(SessionState::Expired)
        && m4.get(s1).map(|x| x.state).map(|st| st.is_terminal()) == Some(true)
        && m4.len() == 1;
    let again = m4.sweep(60_000_000_000);
    let idem_ok = again == 0 && m4.swept == 1;
    if edge_ok && expired_ok && idem_ok {
        s.ok("F2883-收-01-超时回收恰值与终态");
    } else {
        s.fail("F2883-收-01-超时回收恰值与终态", "回收失效");
    }

    // --- 回收组（判据六）---
    // 收-02：混合流回收对账——三条会话不同 last_active（0/10s/25s）+
    // 一条已登出：sweep(40s) 回收恰 3 条（0 与 10s 与 25s 均超 30s 线；
    // 登出终态跳过）——0/10s/25s 对 40s 的 idle=40/30/15s，15s 不超！
    // 重算：25s 会话 idle=15s 未超 → 回收恰 2 条；登出条不动。
    let mut m5 = SessionManager::new();
    let x1 = m5.open(5, Some(UserState::Active), 0);
    let x2 = m5.open(6, Some(UserState::Active), 10_000_000_000);
    let x3 = m5.open(8, Some(UserState::Active), 25_000_000_000);
    let _ = m5.close(x3.unwrap_or(0));
    // x3 已登出（last_active 仍是 25s）。
    let n = m5.sweep(40_000_000_000);
    // x1: idle 40s > 30s 回收；x2: idle 30s 恰值不回收；x3 终态跳过。
    let mixed_ok = n == 1
        && m5.get(x1.unwrap_or(0)).map(|x| x.state) == Some(SessionState::Expired)
        && m5.get(x2.unwrap_or(0)).map(|x| x.state) == Some(SessionState::Active)
        && m5.get(x3.unwrap_or(0)).map(|x| x.state) == Some(SessionState::LoggedOut)
        && m5.len() == 3;
    if mixed_ok {
        s.ok("F2883-收-02-混合流回收对账");
    } else {
        s.fail("F2883-收-02-混合流回收对账", "混合回收失效");
    }

    // --- 判据元（判据七）---
    // 元-01：五码互异 + 文案非空互异 + 缺省分支 + 版本锚定。
    let codes = [
        E_SS_CONCURRENCY,
        E_SS_SUSPENDED,
        E_SS_UNKNOWN,
        E_SS_TERMINAL,
        E_SS_UNREGISTERED,
    ];
    let mut uniq = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let mut det_uniq = true;
    let mut det_nonempty = true;
    for i in 0..5 {
        let d = reason_of(codes[i]);
        if d.is_empty() {
            det_nonempty = false;
        }
        for j in (i + 1)..5 {
            if reason_of(codes[i]) == reason_of(codes[j]) {
                det_uniq = false;
            }
        }
    }
    let unknown_ok = reason_of("E_UNKNOWN") == "未知会话诊断码";
    let ver_ok = SESSION_VERSION == "S03-session-v1"
        && CONCURRENCY_LIMIT == 2
        && IDLE_TIMEOUT_NS == 30_000_000_000;
    if uniq && det_uniq && det_nonempty && unknown_ok && ver_ok {
        s.ok("F2883-元-01-五码闭集与常量锚");
    } else {
        s.fail("F2883-元-01-五码闭集与常量锚", "判据元失效");
    }

    // --- 判据元（判据八）---
    // 元-02：零 panic 面——空账 sweep/touch/close/active_of 全平凡；
    // unwrap_or 兜底路径（判据自身零 unwrap——用 match/ok_or 语义）。
    let mut m6 = SessionManager::new();
    let empty_ok = m6.is_empty()
        && m6.len() == 0
        && m6.active_of(1) == 0
        && m6.sweep(1000) == 0
        && matches!(m6.touch(1, 0), Err(E_SS_UNKNOWN))
        && matches!(m6.close(1), Err(E_SS_UNKNOWN))
        && m6.get(1).is_none();
    if empty_ok {
        s.ok("F2883-元-02-零panic空账平凡");
    } else {
        s.fail("F2883-元-02-零panic空账平凡", "panic 面失效");
    }

    // --- 编排（判据九）---
    // 编-01：三机制全链路——时序手算：
    // u1 开 2 满（u1a/u1b）、u1 第三次并发拒、u2 开 1、停权拒 1；
    // u1a 登出（终态）；u1b touch(35s) 唤醒续命（last=35s）；
    // sweep(40s)：u1a 终态跳过、u1b idle=5s 不回收、u2a idle=40s>30s
    // 回收 → swept=1；touch(u1b,41s) 再唤醒 Ok(Active)。
    // 计数对账：opened=3 closed=1 swept=1 rejected=2。
    let mut m7 = SessionManager::new();
    let u1a = m7.open(1, Some(UserState::Active), 0);
    let u1b = m7.open(1, Some(UserState::Active), 0);
    let u1c = m7.open(1, Some(UserState::Active), 0);
    let u2a = m7.open(2, Some(UserState::Active), 0);
    let _ = m7.open(3, Some(UserState::Suspended), 0);
    let _ = m7.close(u1a.unwrap_or(0));
    let _ = m7.touch(u1b.unwrap_or(0), 35_000_000_000);
    let swept = m7.sweep(40_000_000_000);
    let wake = m7.touch(u1b.unwrap_or(0), 41_000_000_000);
    let chain_ok = u1a.is_ok()
        && u1b.is_ok()
        && matches!(u1c, Err(E_SS_CONCURRENCY))
        && u2a.is_ok()
        && swept == 1
        && matches!(wake, Ok(SessionState::Active))
        && m7.opened == 3
        && m7.closed == 1
        && m7.swept == 1
        && m7.rejected == 2
        && m7.get(u2a.unwrap_or(0)).map(|x| x.state) == Some(SessionState::Expired);
    let sum7 = m7.summary();
    let sum_ok = sum7.contains("S03-session-v1") && sum7.contains("开3");
    if chain_ok && sum_ok {
        s.ok("F2883-编-01-三机制全链路对账");
    } else {
        s.fail("F2883-编-01-三机制全链路对账", "全链路失效");
    }

    s
}
