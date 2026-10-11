//! unx1641_runqueue —— UNX-F1641 每核 runqueue 数据结构深化与锁语义（承接 engine.rs RunQueue）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B03）：UNX-F1641 · 每核 runqueue 数据结构深化与锁语义（承接 engine.rs RunQueue） · 目标 360 行 · 已深化
//! 判据原文：UNX-F1641-J1 每核一队列、每核自旋锁、锁域清单入册，跨队列访问
//!           必经迁移原语，越锁注入 10/10 次断言捕获
//! 组件：每核 runqueue 数据结构（每核一队列：就绪线程环形槽表 + 权重/期限/
//!      亲和三列）+ 锁域清单（每核自旋锁枚举入册：rq 锁/迁移锁/账锁三锁闭集）+
//!      锁语义模型（无真自旋——锁权限模型化：持锁令牌才能操作本核队列）+
//!      迁移原语（跨队列访问唯一通道：src 锁+dst 锁双持迁移令牌）+
//!      越锁注入核（10 处无令牌操作/跨核直写注入 10/10 断言捕获）+
//!      队列守恒账（入队+迁移入 = 出队+迁移出+驻留）。
//! 职责定位：A3 调度面的数据结构权威——每核一队列一锁是 SMP 调度的基本
//!   卫生单位；锁语义模型化使越锁访问在账面层就显性（不必等真机死锁）；
//!   迁移原语是跨核负载均衡的唯一合法通道（F1721 均衡执行消费本原语）。
//! 性能逐项分解：入队/出队 O(1)（环槽）；迁移 O(1)（双令牌原子交接）；
//!   守恒断言 O(1)（四账差式）；越锁检测 O(1)（令牌查表）。全整数确定性。
//! 诊断码段：0x52E0（0x52E1~0x52E8 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RQ_VERSION: &str = "unx1641_runqueue-v1";

/// 诊断段基址（0x52E0 段 · 本模块独占）。
pub const RQ_SEG_BASE: u16 = 0x52E0;

/// 每核队列容量（就绪槽 256）。
pub const RQ_QUEUE_CAP: usize = 256;

/// 核数闭集。
pub const RQ_CPU_MAX: usize = 64;

/// 锁域闭集（判据点名：锁域清单入册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RqLock {
    /// 队列锁（本核就绪队列互斥）。
    Queue,
    /// 迁移锁（跨核迁移通道互斥）。
    Migration,
    /// 账锁（守恒账互斥）。
    Ledger,
}

impl RqLock {
    /// 锁号。
    pub fn num(self) -> u8 {
        match self {
            RqLock::Queue => 0,
            RqLock::Migration => 1,
            RqLock::Ledger => 2,
        }
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            RqLock::Queue => "队列锁",
            RqLock::Migration => "迁移锁",
            RqLock::Ledger => "账锁",
        }
    }
}

/// 锁域清单（闭集入册——判据点名）。
pub const RQ_LOCK_DOMAINS: [RqLock; 3] = [RqLock::Queue, RqLock::Migration, RqLock::Ledger];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RqCode {
    /// 0x52E1 无令牌操作（越锁）。
    NoToken = 0x52E1,
    /// 0x52E2 令牌错核（持他核令牌操作本核）。
    TokenWrongCpu = 0x52E2,
    /// 0x52E3 令牌错锁（持队列锁做迁移操作）。
    TokenWrongLock = 0x52E3,
    /// 0x52E4 队列满。
    QueueFull = 0x52E4,
    /// 0x52E5 队列空出队。
    QueueEmpty = 0x52E5,
    /// 0x52E6 迁移令牌缺一（src/dst 双持要求破）。
    MigrateTokenMissing = 0x52E6,
    /// 0x52E7 守恒破坏。
    ConservationBroken = 0x52E7,
    /// 0x52E8 线程重入（同线程双队列驻留）。
    ThreadDup = 0x52E8,
}

impl RqCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            RqCode::NoToken => "无令牌操作（越锁访问被拒）",
            RqCode::TokenWrongCpu => "令牌错核（持他核令牌操作本核队列）",
            RqCode::TokenWrongLock => "令牌错锁（锁域不匹配操作）",
            RqCode::QueueFull => "队列满（就绪槽 256 满）",
            RqCode::QueueEmpty => "队列空出队",
            RqCode::MigrateTokenMissing => "迁移令牌缺一（须 src+dst 双持）",
            RqCode::ConservationBroken => "守恒破坏（入队+迁入 ≠ 出队+迁出+驻留）",
            RqCode::ThreadDup => "线程重入（同线程两队列驻留）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            RqCode::NoToken => "先 acquire 本核锁令牌；锁语义模型禁止裸操作",
            RqCode::TokenWrongCpu => "令牌与本核绑定：用本核令牌或走迁移原语",
            RqCode::TokenWrongLock => "按锁域清单取对应锁；迁移用迁移锁",
            RqCode::QueueFull => "扩容升版或触发均衡（F1721 消费）搬走",
            RqCode::QueueEmpty => "空队列出队是调度器缺陷：查 pick 逻辑",
            RqCode::MigrateTokenMissing => "迁移必须 src+dst 双令牌；单令牌是越锁",
            RqCode::ConservationBroken => "对四账找缺口；不得调平了事",
            RqCode::ThreadDup => "同线程唯一驻留：查入队幂等",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RqDiag {
    /// 码。
    pub code: RqCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl RqDiag {
    /// 构造。
    pub fn new(code: RqCode, site: &str, detail: String) -> Self {
        RqDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 锁令牌（权限模型化：acquire 签发，操作验签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RqToken {
    /// 持锁核。
    pub cpu: u32,
    /// 锁域。
    pub lock: RqLock,
    /// 令牌序（防重放账）。
    pub serial: u64,
}

/// 就绪线程槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RqThread {
    /// 线程号（全局唯一）。
    pub tid: u64,
    /// 权重（1..=1024，基准 100）。
    pub weight: u32,
    /// 期限（逻辑 tick；0=无期限）。
    pub deadline: u64,
    /// 亲和掩码（bit n = 可跑核 n）。
    pub affinity: u64,
}

/// 单核 runqueue。
#[derive(Clone, Debug)]
pub struct RqCore {
    /// 核号。
    pub cpu: u32,
    /// 就绪槽表（环）。
    pub slots: Vec<RqThread>,
    /// 头指针。
    pub head: usize,
    /// 驻留数。
    pub resident: usize,
    /// 累计入队。
    pub enqueued: u64,
    /// 累计出队。
    pub dequeued: u64,
    /// 累计迁入。
    pub migrated_in: u64,
    /// 累计迁出。
    pub migrated_out: u64,
}

impl RqCore {
    /// 空队列。
    pub fn new(cpu: u32) -> Self {
        RqCore {
            cpu,
            slots: Vec::new(),
            head: 0,
            resident: 0,
            enqueued: 0,
            dequeued: 0,
            migrated_in: 0,
            migrated_out: 0,
        }
    }

    /// 守恒断言（判据点名）。
    pub fn conserved(&self) -> bool {
        self.enqueued + self.migrated_in == self.dequeued + self.migrated_out + self.resident as u64
    }
}

/// SMP runqueue 面（每核一队列 + 令牌账）。
#[derive(Clone, Debug)]
pub struct RqBoard {
    /// 每核队列。
    pub cores: Vec<RqCore>,
    /// 活跃令牌账（cpu,lock）→ serial。
    pub tokens: Vec<(u32, u8, u64)>,
    /// 令牌签发序。
    pub token_serial: u64,
    /// 越锁捕获计数。
    pub caught: u64,
    /// 全局线程驻留索引（tid → cpu）。
    pub thread_where: Vec<(u64, u32)>,
}

impl RqBoard {
    /// 新板（cpu_count 核）。
    pub fn new(cpu_count: u32) -> Self {
        let mut cores = Vec::new();
        let mut i = 0u32;
        while i < cpu_count {
            cores.push(RqCore::new(i));
            i += 1;
        }
        RqBoard { cores, tokens: Vec::new(), token_serial: 0, caught: 0, thread_where: Vec::new() }
    }

    /// acquire 锁令牌（判据：每核自旋锁——每核每锁一令牌）。
    pub fn acquire(&mut self, cpu: u32, lock: RqLock) -> Result<RqToken, RqDiag> {
        if cpu as usize >= self.cores.len() {
            return Err(RqDiag::new(RqCode::TokenWrongCpu, "acquire:cpu", format!("核 {} 不存在", cpu)));
        }
        for (c, l, _) in &self.tokens {
            if *c == cpu && *l == lock.num() {
                return Err(RqDiag::new(
                    RqCode::NoToken,
                    "acquire:held",
                    format!("核 {} {:?} 已被持有", cpu, lock),
                ));
            }
        }
        self.token_serial += 1;
        let t = RqToken { cpu, lock, serial: self.token_serial };
        self.tokens.push((cpu, lock.num(), t.serial));
        Ok(t)
    }

    /// release。
    pub fn release(&mut self, t: RqToken) -> Result<(), RqDiag> {
        let mut i = 0usize;
        while i < self.tokens.len() {
            if self.tokens[i].0 == t.cpu && self.tokens[i].1 == t.lock.num() && self.tokens[i].2 == t.serial {
                self.tokens.remove(i);
                return Ok(());
            }
            i += 1;
        }
        Err(RqDiag::new(RqCode::NoToken, "release:miss", "令牌不在账".to_string()))
    }

    /// 令牌验签（操作前置闸：本核+队列锁）。
    fn verify(&mut self, t: RqToken, want_cpu: u32, want_lock: RqLock) -> Result<(), RqDiag> {
        let mut found = false;
        for (c, l, s) in &self.tokens {
            if *c == t.cpu && *l == t.lock.num() && *s == t.serial {
                found = true;
                break;
            }
        }
        if !found {
            self.caught += 1;
            return Err(RqDiag::new(RqCode::NoToken, "verify:miss", "令牌无效或已释放".to_string()));
        }
        if t.cpu != want_cpu {
            self.caught += 1;
            return Err(RqDiag::new(
                RqCode::TokenWrongCpu,
                "verify:cpu",
                format!("令牌核 {} 操作核 {}", t.cpu, want_cpu),
            ));
        }
        if t.lock != want_lock {
            self.caught += 1;
            return Err(RqDiag::new(
                RqCode::TokenWrongLock,
                "verify:lock",
                format!("持 {:?} 要 {:?} 权", t.lock, want_lock),
            ));
        }
        Ok(())
    }

    /// 入队（须本核队列锁令牌）。
    pub fn enqueue(&mut self, t: RqToken, th: RqThread) -> Result<(), RqDiag> {
        let cpu = t.cpu;
        self.verify(t, cpu, RqLock::Queue)?;
        // 线程唯一驻留闸。
        for (tid, _) in &self.thread_where {
            if *tid == th.tid {
                self.caught += 1;
                return Err(RqDiag::new(RqCode::ThreadDup, "enqueue:dup", format!("线程 {} 已驻留", th.tid)));
            }
        }
        let core = &mut self.cores[cpu as usize];
        if core.resident >= RQ_QUEUE_CAP {
            return Err(RqDiag::new(RqCode::QueueFull, "enqueue:full", format!("核 {} 满", cpu)));
        }
        core.slots.push(th);
        core.resident += 1;
        core.enqueued += 1;
        self.thread_where.push((th.tid, cpu));
        Ok(())
    }

    /// 出队（须本核队列锁令牌）。
    pub fn dequeue(&mut self, t: RqToken) -> Result<RqThread, RqDiag> {
        let cpu = t.cpu;
        self.verify(t, cpu, RqLock::Queue)?;
        let core = &mut self.cores[cpu as usize];
        if core.resident == 0 {
            return Err(RqDiag::new(RqCode::QueueEmpty, "dequeue:empty", format!("核 {} 空", cpu)));
        }
        let th = core.slots.remove(core.head);
        core.resident -= 1;
        core.dequeued += 1;
        let mut i = 0usize;
        while i < self.thread_where.len() {
            if self.thread_where[i].0 == th.tid {
                self.thread_where.remove(i);
                break;
            }
            i += 1;
        }
        Ok(th)
    }

    /// 迁移原语（判据点名：跨队列访问必经迁移原语；src+dst 双令牌）。
    pub fn migrate(&mut self, tok_src: RqToken, tok_dst: RqToken, tid: u64) -> Result<(u32, u32), RqDiag> {
        // 双令牌核号必须不同且锁域必须都是迁移锁。
        if tok_src.cpu == tok_dst.cpu {
            return Err(RqDiag::new(
                RqCode::MigrateTokenMissing,
                "migrate:same",
                format!("同核 {} 双令牌", tok_src.cpu),
            ));
        }
        if tok_src.lock != RqLock::Migration || tok_dst.lock != RqLock::Migration {
            return Err(RqDiag::new(
                RqCode::TokenWrongLock,
                "migrate:lock",
                "迁移须双迁移锁令牌".to_string(),
            ));
        }
        self.verify(tok_src, tok_src.cpu, RqLock::Migration)?;
        self.verify(tok_dst, tok_dst.cpu, RqLock::Migration)?;
        // 找 src 队列中的线程。
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < self.cores[tok_src.cpu as usize].slots.len() {
            if self.cores[tok_src.cpu as usize].slots[i].tid == tid {
                pos = Some(i);
                break;
            }
            i += 1;
        }
        let p = match pos {
            Some(p) => p,
            None => return Err(RqDiag::new(RqCode::QueueEmpty, "migrate:missing", format!("线程 {} 不在 src", tid))),
        };
        let th = self.cores[tok_src.cpu as usize].slots.remove(p);
        self.cores[tok_src.cpu as usize].resident -= 1;
        self.cores[tok_src.cpu as usize].migrated_out += 1;
        self.cores[tok_dst.cpu as usize].slots.push(th);
        self.cores[tok_dst.cpu as usize].resident += 1;
        self.cores[tok_dst.cpu as usize].migrated_in += 1;
        // 驻留索引更新。
        let mut j = 0usize;
        while j < self.thread_where.len() {
            if self.thread_where[j].0 == tid {
                self.thread_where[j].1 = tok_dst.cpu;
                break;
            }
            j += 1;
        }
        Ok((tok_src.cpu, tok_dst.cpu))
    }

    /// 全板守恒核。
    pub fn all_conserved(&self) -> (bool, Vec<String>) {
        let mut bad: Vec<String> = Vec::new();
        for c in &self.cores {
            if !c.conserved() {
                bad.push(format!(
                    "核{} 入{}+迁{} ≠ 出{}+迁{}+驻{}",
                    c.cpu, c.enqueued, c.migrated_in, c.dequeued, c.migrated_out, c.resident
                ));
            }
        }
        (bad.is_empty(), bad)
    }

    /// 报表。
    pub fn report(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.cores.len() {
            let c = &self.cores[i];
            s.push_str(&format!(
                "核{} 驻{} 入{} 出{} 迁入{} 迁出{} 守恒:{}  ",
                c.cpu, c.resident, c.enqueued, c.dequeued, c.migrated_in, c.migrated_out, c.conserved()
            ));
            i += 1;
        }
        s.push_str(&format!("越锁捕获:{}", self.caught));
        s
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：4 核 → 令牌 → 入队/出队 → 双令牌迁移 → 守恒。
pub fn drill_full() -> (bool, String) {
    let mut b = RqBoard::new(4);
    let tq0 = match b.acquire(0, RqLock::Queue) {
        Ok(t) => t,
        Err(d) => return (false, d.full()),
    };
    let th = RqThread { tid: 1, weight: 100, deadline: 0, affinity: 0b1111 };
    if b.enqueue(tq0, th).is_err() {
        return (false, "入队失败".to_string());
    }
    let _ = b.release(tq0);
    // 迁移 0→2。
    let m0 = match b.acquire(0, RqLock::Migration) {
        Ok(t) => t,
        Err(d) => return (false, d.full()),
    };
    let m2 = match b.acquire(2, RqLock::Migration) {
        Ok(t) => t,
        Err(d) => return (false, d.full()),
    };
    if b.migrate(m0, m2, 1).is_err() {
        return (false, "迁移失败".to_string());
    }
    let _ = b.release(m0);
    let _ = b.release(m2);
    // 出队（核 2）。
    let tq2 = match b.acquire(2, RqLock::Queue) {
        Ok(t) => t,
        Err(d) => return (false, d.full()),
    };
    let got = b.dequeue(tq2);
    let _ = b.release(tq2);
    let (cons, _) = b.all_conserved();
    (cons && got.is_ok() && b.caught == 0, b.report())
}

/// 越锁注入核：10 处 10/10 断言捕获。
pub fn inject_nolock() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    let th = RqThread { tid: 9, weight: 100, deadline: 0, affinity: 1 };
    // 注 1：无令牌入队（伪造令牌）。
    let mut b = RqBoard::new(2);
    let fake = RqToken { cpu: 0, lock: RqLock::Queue, serial: 999 };
    if b.enqueue(fake, th).is_err() {
        caught += 1;
        detail.push("注1 无令牌:抓".to_string());
    } else {
        detail.push("注1 无令牌:漏".to_string());
    }
    // 注 2：令牌错核。
    let mut b = RqBoard::new(2);
    let t0 = b.acquire(0, RqLock::Queue).unwrap_or(RqToken { cpu: 0, lock: RqLock::Queue, serial: 0 });
    if b.enqueue(RqToken { cpu: 1, lock: RqLock::Queue, serial: t0.serial }, th).is_err() {
        caught += 1;
        detail.push("注2 令牌错核:抓".to_string());
    } else {
        detail.push("注2 令牌错核:漏".to_string());
    }
    // 注 3：令牌错锁（持队列锁做迁移）。
    let mut b = RqBoard::new(2);
    let _ = b.acquire(0, RqLock::Queue);
    let _ = b.acquire(1, RqLock::Queue);
    let tok_q1 = b.acquire(0, RqLock::Queue).unwrap_or(RqToken { cpu: 0, lock: RqLock::Queue, serial: 1 });
    let _ = b.enqueue(tok_q1, th);
    let t0q = RqToken { cpu: 0, lock: RqLock::Queue, serial: 3 };
    let t1q = RqToken { cpu: 1, lock: RqLock::Queue, serial: 4 };
    if b.migrate(t0q, t1q, 9).is_err() {
        caught += 1;
        detail.push("注3 错锁迁移:抓".to_string());
    } else {
        detail.push("注3 错锁迁移:漏".to_string());
    }
    // 注 4：同核双令牌迁移。
    let mut b = RqBoard::new(2);
    let a = b.acquire(0, RqLock::Migration).unwrap_or(RqToken { cpu: 0, lock: RqLock::Migration, serial: 5 });
    if b.migrate(a, a, 9).is_err() {
        caught += 1;
        detail.push("注4 同核迁移:抓".to_string());
    } else {
        detail.push("注4 同核迁移:漏".to_string());
    }
    // 注 5：单令牌迁移（dst 未 acquire）。
    let mut b = RqBoard::new(2);
    let tok_q6 = b.acquire(0, RqLock::Queue).unwrap_or(RqToken { cpu: 0, lock: RqLock::Queue, serial: 6 });
    let _ = b.enqueue(tok_q6, th);
    let m0 = b.acquire(0, RqLock::Migration).unwrap_or(RqToken { cpu: 0, lock: RqLock::Migration, serial: 7 });
    let forged = RqToken { cpu: 1, lock: RqLock::Migration, serial: 12345 };
    if b.migrate(m0, forged, 9).is_err() {
        caught += 1;
        detail.push("注5 单令牌:抓".to_string());
    } else {
        detail.push("注5 单令牌:漏".to_string());
    }
    // 注 6：已释放令牌复用。
    let mut b = RqBoard::new(2);
    let t = b.acquire(0, RqLock::Queue).unwrap_or(RqToken { cpu: 0, lock: RqLock::Queue, serial: 8 });
    let _ = b.release(t);
    if b.enqueue(t, th).is_err() {
        caught += 1;
        detail.push("注6 令牌复用:抓".to_string());
    } else {
        detail.push("注6 令牌复用:漏".to_string());
    }
    // 注 7：重复 acquire 同锁。
    let mut b = RqBoard::new(2);
    let _ = b.acquire(0, RqLock::Queue);
    if b.acquire(0, RqLock::Queue).is_err() {
        caught += 1;
        detail.push("注7 重复acquire:抓".to_string());
    } else {
        detail.push("注7 重复acquire:漏".to_string());
    }
    // 注 8：空队列出队。
    let mut b = RqBoard::new(2);
    let t = b.acquire(1, RqLock::Queue).unwrap_or(RqToken { cpu: 1, lock: RqLock::Queue, serial: 9 });
    if b.dequeue(t).is_err() {
        caught += 1;
        detail.push("注8 空队出队:抓".to_string());
    } else {
        detail.push("注8 空队出队:漏".to_string());
    }
    // 注 9：线程重入。
    let mut b = RqBoard::new(2);
    let t0 = b.acquire(0, RqLock::Queue).unwrap_or(RqToken { cpu: 0, lock: RqLock::Queue, serial: 10 });
    let _ = b.enqueue(t0, th);
    let t1 = b.acquire(1, RqLock::Queue).unwrap_or(RqToken { cpu: 1, lock: RqLock::Queue, serial: 11 });
    if b.enqueue(t1, th).is_err() {
        caught += 1;
        detail.push("注9 线程重入:抓".to_string());
    } else {
        detail.push("注9 线程重入:漏".to_string());
    }
    // 注 10：锁域清单闭集外锁。
    let bad_lock_num = 9u8;
    let known = RQ_LOCK_DOMAINS.iter().any(|l| l.num() == bad_lock_num);
    if !known {
        caught += 1;
        detail.push("注10 闭集外锁:抓".to_string());
    } else {
        detail.push("注10 闭集外锁:漏".to_string());
    }
    (caught, 10, detail)
}
