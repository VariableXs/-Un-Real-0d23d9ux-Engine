//! UNX-F21592 · 随闸门补测池终登记（R-F2-001/002 清单）
//!
//! 诊断段 0x91059~0x91060（F2-B40 域收官段 12/100）。
//!
//! 判据原文：补测池 40 项登记（实机/QEMU 批队列，零 QEMU 纪律无违规）。
//!
//! 设计要点（为什么一个补测池要分两列，而不是一列排队）：
//! 补测有两种代价完全不同的执行环境：实机要走烧录-上电-观测，QEMU 只是仿真。
//! 若混在一列 FIFO 里，一个卡住的实机项会让后面所有 QEMU 项一起等——这正是
//! 「两列互不阻塞」要解决的问题。所以队列按环境分列，各自 FIFO，出队互不干涉。
//!
//! 但分列带来一个新风险：**串列**。一条声明「必须在实机验」的项如果被塞进
//! QEMU 列，它照样会跑完、照样打绿，而它证明的其实是仿真行为——这就是「零
//! QEMU 纪律」要守的线。故每条项同时记 `declared`（清单声明）与 `scheduled`
//! （实际所在列），纪律复核比对两者，比的是「声明与事实」，不是「队列自述」。
//!
//! 码段只分到两枚（0x91059/0x91060），故故障用「码 + 因由」二元组定位：
//! 码管「登记面/纪律面」大类，因由枚举管具体动作。因由不占码段，避免与
//! F21593 的证据段撞车。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 补测池容量（施工书：40 项登记）。
///
/// 容量与号段刻意不相等：若号段恰好也是 40，容量闸门就永远不可达（第 41 条
/// 一定先被号段拦下），成为一条永不触发的死码。号段给到 099 是编号预案
/// （后续批次还会追加 R-F2 项），容量 40 才是闸门的真实约束。
pub const POOL_CAPACITY: usize = 40;
/// 补测编号下界（R-F2-001）。
pub const CODE_MIN: u16 = 1;
/// 补测编号上界（R-F2-099，编号预案口径）。
pub const CODE_MAX: u16 = 99;

/// 池诊断码（本单只分到两枚，五位数段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolCode {
    /// 0x91059 登记面清账（回执用）。
    Clear = 0x91059,
    /// 0x91060 池面阻断（容量/重复/纪律/守恒）。
    Blocked = 0x91060,
}

impl PoolCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            PoolCode::Clear => "补测池登记面清账",
            PoolCode::Blocked => "补测池操作被阻断",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            PoolCode::Clear => "四十项齐、两列分离、纪律复核零违规",
            PoolCode::Blocked => "容量已满、重复登记、跨环境串列，或队列为空",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PoolCode::Clear => "封池并移交闸门批队列",
            PoolCode::Blocked => "按因由分派：补登记号、去重、或把项挪回声明环境",
        }
    }
    /// 数值形式（u32：五位数码段）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 阻断因由（比码更细的一层定位；不占码段，故可自由增补）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolReason {
    /// 编号越出 R-F2-001~040。
    BadCode,
    /// 同号二次登记。
    Duplicate,
    /// 池已满 40 项。
    Capacity,
    /// 该环境列已空，无可出队项。
    QueueEmpty,
    /// 声明环境与实际所在列不一致。
    EnvDrift,
    /// 编号未在池中登记。
    NotRegistered,
    /// 池已封，不再接受登记。
    Sealed,
    /// 项已完成，不能再改队列位置。
    AlreadyDone,
}

/// 池故障。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolFault {
    /// 诊断码（0x91059 / 0x91060）。
    pub code: u32,
    /// 因由。
    pub reason: PoolReason,
    /// 定位编号（0 = 与具体项无关）。
    pub at: u16,
}

impl PoolFault {
    const fn new(reason: PoolReason, at: u16) -> PoolFault {
        PoolFault { code: PoolCode::Blocked.code(), reason, at }
    }
    /// 码是否落在本单码段内。
    pub const fn is_known(self) -> bool {
        self.code == 0x91059 || self.code == 0x91060
    }
}

/// 执行环境（实机 / 仿真）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Env {
    /// 实机（烧录-上电-观测）。
    Metal,
    /// QEMU 仿真。
    Qemu,
}

impl Env {
    /// 报告用短名。
    pub const fn label(self) -> &'static str {
        match self { Env::Metal => "metal", Env::Qemu => "qemu" }
    }
}

/// 触发条件（决定这项在闸门的哪个动作点跑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// 开机路径。
    Boot,
    /// 关机路径。
    Shutdown,
    /// 故障注入路径。
    Fault,
    /// 人工手动。
    Manual,
}

/// 项状态。只有两态：列里排队（Pending）或已出队（Done）。
///
/// 不设 Running/Failed：补测池只管「排没排到」，执行结论由证据三件套承担；
/// 在这里再存一份结论就会出现两个真相。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// 待执行。
    Pending,
    /// 已出队执行。
    Done,
}

/// 单个补测项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestItem {
    /// 编号 R-F2-001~040。
    pub code: u16,
    /// 清单声明的环境（不可被调度改写）。
    pub declared: Env,
    /// 实际所在列（`reschedule` 可改）。
    pub scheduled: Env,
    /// 触发条件。
    pub trigger: Trigger,
    /// 状态。
    pub status: Status,
}

impl RetestItem {
    /// 声明与实际是否同列（纪律复核的最小判据）。
    pub const fn env_agrees(&self) -> bool {
        self.declared as u8 == self.scheduled as u8
    }
}

/// 守恒读数。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Conservation {
    /// 登记尝试总数。
    pub offered: u16,
    /// 入池数。
    pub enqueued: u16,
    /// 已出队数。
    pub done: u16,
    /// 待执行数。
    pub pending: u16,
    /// 被拒数。
    pub rejected: u16,
}

impl Conservation {
    /// 双恒等式：尝试 = 入池 + 被拒；入池 = 完成 + 待执行。
    ///
    /// 两条都要：只有第一条会漏掉「入池了但既没完成也没待执行」的丢项，
    /// 只有第二条会漏掉「被拒了但计数没落账」的假拒。
    pub const fn balanced(&self) -> bool {
        self.offered == self.enqueued.saturating_add(self.rejected)
            && self.enqueued == self.done.saturating_add(self.pending)
    }
}

/// 封池回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SealReceipt {
    /// 清账码（0x91059）。
    pub code: u32,
    /// 池内项数。
    pub items: u16,
    /// 实机列项数（含已完成）。
    pub metal: u16,
    /// QEMU 列项数（含已完成）。
    pub qemu: u16,
}

/// 补测池（两列 FIFO + 登记册 + 守恒计数）。
pub struct RetestPool {
    items: Vec<RetestItem>,
    metal_q: Vec<u16>,
    qemu_q: Vec<u16>,
    offered: u16,
    done: u16,
    rejected: u16,
    sealed: bool,
}

impl RetestPool {
    /// 新建空池。
    pub const fn new() -> RetestPool {
        RetestPool {
            items: Vec::new(),
            metal_q: Vec::new(),
            qemu_q: Vec::new(),
            offered: 0,
            done: 0,
            rejected: 0,
            sealed: false,
        }
    }

    /// 登记一项。`offered` 先加：被拒的尝试也是尝试，不记就会让守恒式永远
    /// 成立却毫无信息（分母被悄悄改小）。
    pub fn register(&mut self, code: u16, env: Env, trigger: Trigger) -> Result<(), PoolFault> {
        self.offered = self.offered.saturating_add(1);
        match self.try_register(code) {
            Ok(()) => {
                let item = RetestItem { code, declared: env, scheduled: env, trigger, status: Status::Pending };
                self.items.push(item);
                self.column_mut(env).push(code);
                Ok(())
            }
            Err(f) => {
                self.rejected = self.rejected.saturating_add(1);
                Err(f)
            }
        }
    }

    /// 只读判定：封池 → 号段 → 去重 → 容量。
    ///
    /// 去重先于容量：容量满时重复登记应报「重复」而不是「满」，否则运维会去
    /// 扩容而不是去查重——同一个现象两种处置，报错顺序就决定了处置方向。
    fn try_register(&self, code: u16) -> Result<(), PoolFault> {
        if self.sealed {
            return Err(PoolFault::new(PoolReason::Sealed, code));
        }
        if code < CODE_MIN || code > CODE_MAX {
            return Err(PoolFault::new(PoolReason::BadCode, code));
        }
        if self.items.iter().any(|i| i.code == code) {
            return Err(PoolFault::new(PoolReason::Duplicate, code));
        }
        if self.items.len() >= POOL_CAPACITY {
            return Err(PoolFault::new(PoolReason::Capacity, code));
        }
        Ok(())
    }

    fn column_mut(&mut self, env: Env) -> &mut Vec<u16> {
        match env { Env::Metal => &mut self.metal_q, Env::Qemu => &mut self.qemu_q }
    }

    fn column(&self, env: Env) -> &Vec<u16> {
        match env { Env::Metal => &self.metal_q, Env::Qemu => &self.qemu_q }
    }

    /// 出队执行（该环境列头）。
    ///
    /// 只动本列：另一列的队头与顺序完全不受影响，这就是「互不阻塞」的可观测
    /// 形式——不靠文档承诺，靠两列各自持有自己的 Vec。
    pub fn advance(&mut self, env: Env) -> Result<u16, PoolFault> {
        let code = match self.column(env).first() {
            Some(c) => *c,
            None => return Err(PoolFault::new(PoolReason::QueueEmpty, 0)),
        };
        self.column_mut(env).remove(0);
        if let Some(it) = self.items.iter_mut().find(|i| i.code == code) {
            it.status = Status::Done;
        }
        self.done = self.done.saturating_add(1);
        Ok(code)
    }

    /// 改列（人工重排：把项挪到另一环境的队尾）。
    ///
    /// 允许挪动是必要的（实机排队两周时可以先上仿真摸底），但不改写 `declared`：
    /// 声明是清单原件，改了就没有比对基准，纪律复核会跟着一起失真。
    pub fn reschedule(&mut self, code: u16, to: Env) -> Result<(), PoolFault> {
        let idx = match self.items.iter().position(|i| i.code == code) {
            Some(i) => i,
            None => return Err(PoolFault::new(PoolReason::NotRegistered, code)),
        };
        if self.items[idx].status != Status::Pending {
            return Err(PoolFault::new(PoolReason::AlreadyDone, code));
        }
        let from = self.items[idx].scheduled;
        if from == to {
            return Ok(());
        }
        if let Some(p) = self.column(from).iter().position(|c| *c == code) {
            self.column_mut(from).remove(p);
        }
        self.column_mut(to).push(code);
        self.items[idx].scheduled = to;
        Ok(())
    }

    /// 零 QEMU 纪律复核：返回复核过的项数（全合规才有 Ok）。
    ///
    /// 三件事一起查：声明与所在列一致、列内成员与该列待执行项一一对应、
    /// 列内顺序为登记序。第三件常被忽略——列序错乱不会让任何项跑不起来，
    /// 但会让「先到先跑」的批次叙事失真，事后复盘追不到真实因果。
    pub fn audit_discipline(&self) -> Result<u16, PoolFault> {
        for it in self.items.iter() {
            if !it.env_agrees() {
                return Err(PoolFault::new(PoolReason::EnvDrift, it.code));
            }
        }
        for it in self.items.iter() {
            let hits = self.column(it.scheduled).iter().filter(|c| **c == it.code).count();
            let want = if it.status == Status::Pending { 1 } else { 0 };
            if hits != want {
                return Err(PoolFault::new(PoolReason::EnvDrift, it.code));
            }
        }
        for env in [Env::Metal, Env::Qemu] {
            let mut prev = 0u16;
            for c in self.column(env).iter() {
                if *c <= prev {
                    return Err(PoolFault::new(PoolReason::EnvDrift, *c));
                }
                prev = *c;
            }
        }
        Ok(self.items.len() as u16)
    }

    /// 守恒读数。
    pub fn conserve(&self) -> Conservation {
        let pending = self.items.iter().filter(|i| i.status == Status::Pending).count();
        Conservation {
            offered: self.offered,
            enqueued: self.items.len() as u16,
            done: self.done,
            pending: pending as u16,
            rejected: self.rejected,
        }
    }

    /// 按触发条件筛待执行项（闸门某个动作点只跑自己那一批）。
    pub fn pending_with(&self, trigger: Trigger) -> Vec<u16> {
        self.items
            .iter()
            .filter(|i| i.status == Status::Pending && i.trigger == trigger)
            .map(|i| i.code)
            .collect()
    }

    /// 某环境列的长度。
    pub fn queue_len(&self, env: Env) -> usize {
        self.column(env).len()
    }

    /// 已登项数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否已封池。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 被拒次数。
    pub const fn rejected(&self) -> u16 {
        self.rejected
    }

    /// 封池：必须先过纪律复核。
    ///
    /// 「终登记」的语义就在这里——封池不是加锁，而是「承诺这份池子已经过交叉
    /// 复核」；故复核失败时拒绝封池，回执码用清账码而非阻断码。
    pub fn seal(&mut self) -> Result<SealReceipt, PoolFault> {
        if self.sealed {
            return Err(PoolFault::new(PoolReason::Sealed, 0));
        }
        self.audit_discipline()?;
        self.sealed = true;
        Ok(SealReceipt {
            code: PoolCode::Clear.code(),
            items: self.items.len() as u16,
            metal: self.items.iter().filter(|i| i.declared == Env::Metal).count() as u16,
            qemu: self.items.iter().filter(|i| i.declared == Env::Qemu).count() as u16,
        })
    }

    /// 队列导出（两列顺序 + 计数，供闸门脚本直接比对）。
    pub fn export(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("pool items=");
        out.push_str(&self.items.len().to_string());
        out.push(' ');
        out.push_str(Env::Metal.label());
        out.push('=');
        out.push_str(&self.items.iter().filter(|i| i.declared == Env::Metal).count().to_string());
        out.push(' ');
        out.push_str(Env::Qemu.label());
        out.push('=');
        out.push_str(&self.items.iter().filter(|i| i.declared == Env::Qemu).count().to_string());
        out.push_str(" done=");
        out.push_str(&self.done.to_string());
        out.push_str(" sealed=");
        out.push_str(if self.sealed { "1" } else { "0" });
        for env in [Env::Metal, Env::Qemu] {
            out.push(' ');
            out.push_str(env.label());
            out.push_str("_q=");
            let mut first = true;
            for code in self.column(env).iter() {
                if !first {
                    out.push(',');
                }
                out.push_str(&label_of(*code));
                first = false;
            }
        }
        out
    }
}

/// 编号标签（自由函数：导出时手里只有编号，没有整条项）。
pub fn label_of(code: u16) -> String {
    let mut s = String::with_capacity(9);
    s.push_str("R-F2-");
    if code < 100 {
        s.push('0');
    }
    if code < 10 {
        s.push('0');
    }
    s.push_str(&code.to_string());
    s
}

/// 标准 40 项补测池（判据夹具）。
///
/// 环境按 `code % 4 == 0` 切（10 项 QEMU / 30 项实机），触发条件按 `code % 5`
/// 切——两个模数互质，故「QEMU 项恰好都是某类触发」这种耦合不会出现；
/// 否则「按触发筛」那条判据会被环境分布顺带满足，等于没测。
pub fn standard_pool() -> Result<RetestPool, PoolFault> {
    let mut pool = RetestPool::new();
    let mut code: u16 = CODE_MIN;
    // 上界取池容量而非号段上界：号段是 099 的预案，塞满它必然撞容量闸门。
    while code <= POOL_CAPACITY as u16 {
        let env = if code % 4 == 0 { Env::Qemu } else { Env::Metal };
        let trigger = match code % 5 {
            1 => Trigger::Boot,
            2 => Trigger::Shutdown,
            3 => Trigger::Fault,
            4 => Trigger::Manual,
            _ => Trigger::Boot,
        };
        pool.register(code, env, trigger)?;
        code += 1;
    }
    Ok(pool)
}

/// 夹具失败时统一返回 false（夹具由第一条判据单独证明）。
fn on_pool<F: FnOnce(&mut RetestPool) -> bool>(f: F) -> bool {
    match standard_pool() {
        Ok(mut p) => f(&mut p),
        Err(_) => false,
    }
}

/// 判据入口（UNX-F21592-J1 及配套十三条）。
pub fn run_unx21592_retestpool_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21592");
    let standard = standard_pool();

    // 1) 夹具可构造。
    cs.add("f21592.fixture_buildable", standard.is_ok(), "标准 40 项补测池构造失败");

    // 2) 登记总量与环境分列（正向）。
    let counts_ok = standard.as_ref().map(|p| {
        p.len() == 40 && p.queue_len(Env::Metal) == 30 && p.queue_len(Env::Qemu) == 10
            && p.pending_with(Trigger::Boot).len() == 16
            && p.pending_with(Trigger::Fault).len() == 8
    });
    cs.add(
        "f21592.registration_counts",
        counts_ok.unwrap_or(false),
        "项数/环境分列/触发分布不符（应 40 = 30+10，Boot 16、Fault 8）",
    );

    // 3) 容量上限 +1：满 40 后再登记必拒，且池内不增。
    let cap_ok = on_pool(|p| {
        let before = p.len();
        let f = p.register(41, Env::Metal, Trigger::Boot).err();
        f.map(|f| f.code) == Some(0x91060)
            && f.map(|f| f.reason) == Some(PoolReason::Capacity)
            && p.len() == before
            && p.rejected() == 1
    });
    cs.add("f21592.capacity_overflow", cap_ok, "第 41 项未被拒，或拒绝码/因由不符");

    // 4) 重复登记必拒（反向）：同号二次登记，池内只留一条。
    let dup_ok = on_pool(|p| {
        let f = p.register(5, Env::Qemu, Trigger::Fault).err();
        f.map(|f| f.reason) == Some(PoolReason::Duplicate) && p.len() == 40 && p.queue_len(Env::Qemu) == 10
    });
    cs.add("f21592.duplicate_rejected", dup_ok, "重号未被拒，或第二列被写入");

    // 5) 编号越界必拒（反向，边界 0 与 100）。
    let code_ok = on_pool(|p| {
        let low = p.register(0, Env::Metal, Trigger::Boot).err();
        let high = p.register(100, Env::Metal, Trigger::Boot).err();
        low.map(|f| f.reason) == Some(PoolReason::BadCode)
            && high.map(|f| f.reason) == Some(PoolReason::BadCode)
            && p.len() == 40
    });
    cs.add("f21592.bad_code_rejected", code_ok, "号段 0 / 100 未被拒，或因由不符");

    // 6) FIFO：同列出队顺序等于登记顺序。
    let fifo_ok = {
        let mut p = RetestPool::new();
        let mut ok = true;
        for c in 1u16..=3 {
            ok = ok && p.register(c, Env::Metal, Trigger::Boot).is_ok();
        }
        let a = p.advance(Env::Metal);
        let b = p.advance(Env::Metal);
        let c = p.advance(Env::Metal);
        ok && a == Ok(1) && b == Ok(2) && c == Ok(3) && p.queue_len(Env::Metal) == 0
    };
    cs.add("f21592.fifo_order", fifo_ok, "同列出队顺序不是登记序");

    // 7) 两列互不阻塞：出队 QEMU 不影响实机队头与长度。
    let isolation_ok = {
        let mut p = match standard_pool() {
            Ok(p) => p,
            Err(_) => RetestPool::new(),
        };
        let metal_head = p.queue_len(Env::Metal);
        let q1 = p.advance(Env::Qemu);
        let q2 = p.advance(Env::Qemu);
        q1 == Ok(4) && q2 == Ok(8) && p.queue_len(Env::Metal) == metal_head && p.queue_len(Env::Qemu) == 8
    };
    cs.add("f21592.env_isolation", isolation_ok, "出队 QEMU 影响了实机列，或 QEMU 列序不符");

    // 8) 纪律复核（正向）：标准池零违规，全 40 项被复核。
    let clean_ok = standard.as_ref().map(|p| p.audit_discipline() == Ok(40)).unwrap_or(false);
    cs.add("f21592.discipline_clean", clean_ok, "标准池纪律复核未通过或复核项数不为 40");

    // 9) 纪律串列必拒（反向）：把实机项挪进 QEMU 列。
    let drift_ok = on_pool(|p| {
        let moved = p.reschedule(2, Env::Qemu).is_ok();
        let f = p.audit_discipline().err();
        moved
            && f.map(|f| f.code) == Some(0x91060)
            && f.map(|f| f.reason) == Some(PoolReason::EnvDrift)
            && f.map(|f| f.at) == Some(2)
            && p.seal().is_err()
    });
    cs.add(
        "f21592.discipline_drift",
        drift_ok,
        "串列未被纪律复核发现，或封池未随之被拒",
    );

    // 10) 守恒：尝试 = 入池 + 被拒，入池 = 完成 + 待执行（含被拒样本）。
    let conserve_ok = on_pool(|p| {
        let _ = p.register(1, Env::Metal, Trigger::Boot);
        let _ = p.register(99, Env::Metal, Trigger::Boot);
        let _ = p.advance(Env::Metal);
        let c = p.conserve();
        c.offered == 42
            && c.enqueued == 40
            && c.done == 1
            && c.pending == 39
            && c.rejected == 2
            && c.balanced()
    });
    cs.add("f21592.conservation", conserve_ok, "守恒读数不符（应 42 = 40+2，40 = 1+39）");

    // 11) 封池：回执用清账码，封后登记被拒且计数只加一次。
    let seal_ok = on_pool(|p| {
        let r = p.seal();
        let receipt_ok = r.map(|r| r.code == 0x91059 && r.items == 40 && r.metal == 30 && r.qemu == 10);
        let before = p.rejected();
        let again = p.register(7, Env::Metal, Trigger::Boot).err();
        receipt_ok == Ok(true)
            && again.map(|f| f.reason) == Some(PoolReason::Sealed)
            && p.rejected() == before + 1
            && p.is_sealed()
            && p.seal().is_err()
    });
    cs.add("f21592.seal_lock", seal_ok, "封池回执不符，或封后仍可登记");

    // 12) 导出可归档 + 确定性：同一输入两次导出逐字节相同，两次独立构造亦然。
    let export_ok = standard
        .as_ref()
        .map(|p| {
            let e = p.export();
            let same = e == p.export() && standard_pool().map(|q| q.export()) == Ok(e.clone());
            same
                && e.contains("items=40")
                && e.contains("metal=30")
                && e.contains("qemu=10")
                && e.contains("metal_q=R-F2-001,R-F2-002")
                && e.contains("qemu_q=R-F2-004,R-F2-008")
        })
        .unwrap_or(false);
    cs.add(
        "f21592.export_fields",
        export_ok,
        "导出不确定、缺字段，或两列队头编号不符",
    );

    // 13) 空列与未登记（反向边界）：空池出队、未知号改列各报其因由。
    let empty_ok = {
        let mut p = RetestPool::new();
        let a = p.advance(Env::Qemu).err();
        let b = p.reschedule(3, Env::Metal).err();
        a.map(|f| f.reason) == Some(PoolReason::QueueEmpty)
            && b.map(|f| f.reason) == Some(PoolReason::NotRegistered)
            && p.len() == 0
    };
    cs.add("f21592.empty_and_unknown", empty_ok, "空池出队/未知号改列未被拒，或因由不符");

    // 14) 已完成项不可再改列（反向）：出队后再挪列被拒，且另一列不被写入。
    let done_ok = {
        let mut p = match standard_pool() {
            Ok(p) => p,
            Err(_) => RetestPool::new(),
        };
        let ran = p.advance(Env::Metal);
        let f = p.reschedule(1, Env::Qemu).err();
        ran == Ok(1) && f.map(|f| f.reason) == Some(PoolReason::AlreadyDone) && p.queue_len(Env::Qemu) == 10
    };
    cs.add("f21592.done_immutable", done_ok, "已完成项仍被改列，或被拒因由不符");

    // 15) 诊断码闭集：两码值正确、三要素非空、未知码被识别。
    let codes_ok = {
        let all = [PoolCode::Clear, PoolCode::Blocked];
        let mut ok = all[0].code() == 0x91059 && all[1].code() == 0x91060;
        for c in all.iter() {
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                ok = false;
            }
        }
        let unknown = PoolFault { code: 0x91058, reason: PoolReason::BadCode, at: 0 };
        ok && !unknown.is_known() && PoolFault::new(PoolReason::Capacity, 0).is_known()
    };
    cs.add("f21592.code_closed_set", codes_ok, "码值/三要素/闭集识别不符");

    cs
}
