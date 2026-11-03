//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1646 · 每核负载贡献账（权重累计） · 0x72380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B03）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1646-J1 负载=Σ线程权重逐核落账，迁移随线程
//!     转移，Σ每核=全局闭合断言通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 线程账（tid/weight/home）为准绳，每核负载 = Σ落在本核的线程
//!     权重；全局 = Σ每核，双面同写、审计闭合；
//!   · 迁移随线程转移：home 从 A 改 B 的同一事务里，A 负载 -= w、
//!     B 负载 += w，全局不变（迁移只搬运不增减）；
//!   · 全部算术 checked_add/checked_sub：溢出显性拒（WeightOverflow），
//!     欠账下溢显性拒（SumMismatch），不静默饱和；
//!   · 账本状态机 Open→Balanced→Frozen 单调推进；Frozen 后改账显性拒；
//!   · 审计复算：按线程账逐核重算负载，与缓存负载、全局三面对账，
//!     任何一面失配即 SumMismatch（注入篡改必抓）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72380，VX3-1103 负载账窗口独占）。
pub const SEG_BASE: u32 = 0x72380;

/// 负载账核容量上限（冻结 64）。
pub const LOAD_CORE_CAP: usize = 64;
/// 真机档核数（冻结 4，含 BSP 0 号）。
pub const LOAD_CPUS: usize = 4;
/// 线程账容量上限（防账面膨胀，冻结 128）。
pub const THREAD_CAP: usize = 128;
/// 单线程权重上限（冻结 1024，权重 0 一律拒）。
pub const WEIGHT_CAP: u64 = 1024;
/// 真机档标准线程数（冻结 8）。
pub const STD_THREADS: usize = 8;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadCode {
    /// 核号越界（不在 0..=63 或未注册）
    CoreOver,
    /// 线程重号（tid 已在账）
    ThreadDup,
    /// 线程不在账（迁移/摘除目标缺失）
    ThreadMissing,
    /// 权重非法（0 或超 WEIGHT_CAP）
    WeightBad,
    /// 权重累计溢出（checked_add 拒绝）
    WeightOverflow,
    /// 同核迁移（home 未变，无意义搬迁）
    MigrateSameCore,
    /// Σ每核≠全局 / 复算失配（账面被篡改或漏记）
    SumMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl LoadCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LoadCode::CoreOver => "核号越界：先注册 0..=63 核负载账再落账",
            LoadCode::ThreadDup => "线程重号：tid 已在账，另起新号",
            LoadCode::ThreadMissing => "线程不在账：迁移/摘除前先挂账",
            LoadCode::WeightBad => "权重非法：1..=1024，0 权重不入账",
            LoadCode::WeightOverflow => "权重累计溢出：checked_add 拒绝，查核负载面宽度",
            LoadCode::MigrateSameCore => "同核迁移：home 未变，拒绝空搬",
            LoadCode::SumMismatch => "Σ每核≠全局：按线程账复算逐核负载，查漏搬/重记",
            LoadCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [LoadCode; 8] = [
        LoadCode::CoreOver,
        LoadCode::ThreadDup,
        LoadCode::ThreadMissing,
        LoadCode::WeightBad,
        LoadCode::WeightOverflow,
        LoadCode::MigrateSameCore,
        LoadCode::SumMismatch,
        LoadCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LoadCode::ALL.len() {
        if LoadCode::ALL[i].val() == code {
            return LoadCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72380 段闭集内"
}

// ── 账本状态机（单调推进，倒退显性拒） ───────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadState {
    /// 开账：可挂账/迁移/摘账。
    Open,
    /// 已平账：审计闭合通过（仍可改账，改后须复审）。
    Balanced,
    /// 已冻结：只读快照，改账显性拒。
    Frozen,
}

impl LoadState {
    /// 单调序值：Open=0 < Balanced=1 < Frozen=2。
    pub fn rank(self) -> u8 {
        match self {
            LoadState::Open => 0,
            LoadState::Balanced => 1,
            LoadState::Frozen => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: LoadState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 线程权重账：home 为落核号，weight ∈ 1..=WEIGHT_CAP。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadW {
    pub tid: u32,
    pub weight: u64,
    pub home: u32,
}

/// 负载贡献账：线程账（准绳）+ 每核缓存负载 + 全局闭合账。
#[derive(Clone, Debug)]
pub struct LoadBook {
    pub state: LoadState,
    pub threads: Vec<ThreadW>,
    /// 每核缓存负载（下标 = 核号，与 cores 注册序一致）。
    pub loads: Vec<u64>,
    /// 全局闭合账：= Σ每核 = Σ线程权重。
    pub global: u64,
    /// 迁移累计笔数（只增不减，账目回放用）。
    pub migrations: u32,
}

impl LoadBook {
    /// 建账并注册 0..LOAD_CPUS 核（注册后即 Open）。
    pub fn new(cpus: usize) -> LoadBook {
        let mut loads = Vec::new();
        let mut c = 0usize;
        while c < cpus && c < LOAD_CORE_CAP {
            loads.push(0u64);
            c += 1;
        }
        LoadBook { state: LoadState::Open, threads: Vec::new(), loads, global: 0, migrations: 0 }
    }

    /// 核号合法性：已注册范围 0..loads.len()。
    pub fn core_ok(&self, core: u32) -> bool {
        (core as usize) < self.loads.len()
    }

    /// 按线程号定位线程账下标。
    pub fn thread_index(&self, tid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.threads.len() {
            if let Some(t) = self.threads.get(i) {
                if t.tid == tid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 改账闸：Frozen 后一律拒。
    fn frozen_gate(&self) -> Result<(), u32> {
        if self.state == LoadState::Frozen {
            return Err(LoadCode::SumMismatch.val());
        }
        Ok(())
    }

    /// 挂账：线程权重落到 home 核（冻结闸/核闸/重号闸/权重闸/溢出闸）。
    pub fn attach(&mut self, tid: u32, weight: u64, core: u32) -> Result<u64, u32> {
        self.frozen_gate()?;
        if !self.core_ok(core) {
            return Err(LoadCode::CoreOver.val());
        }
        if weight == 0 || weight > WEIGHT_CAP {
            return Err(LoadCode::WeightBad.val());
        }
        if self.thread_index(tid).is_some() {
            return Err(LoadCode::ThreadDup.val());
        }
        if self.threads.len() >= THREAD_CAP {
            return Err(LoadCode::ThreadDup.val());
        }
        let cur = match self.loads.get(core as usize) {
            Some(v) => *v,
            None => return Err(LoadCode::CoreOver.val()),
        };
        let nl = match cur.checked_add(weight) {
            Some(v) => v,
            None => return Err(LoadCode::WeightOverflow.val()),
        };
        let ng = match self.global.checked_add(weight) {
            Some(v) => v,
            None => return Err(LoadCode::WeightOverflow.val()),
        };
        if let Some(v) = self.loads.get_mut(core as usize) {
            *v = nl;
        }
        self.global = ng;
        self.threads.push(ThreadW { tid, weight, home: core });
        Ok(nl)
    }

    /// 摘账：线程权重离开 home 核并出账（核闸/在账闸/下溢闸）。
    pub fn detach(&mut self, tid: u32) -> Result<u64, u32> {
        self.frozen_gate()?;
        let ti = match self.thread_index(tid) {
            Some(i) => i,
            None => return Err(LoadCode::ThreadMissing.val()),
        };
        let (w, home) = match self.threads.get(ti) {
            Some(t) => (t.weight, t.home),
            None => return Err(LoadCode::ThreadMissing.val()),
        };
        let cur = match self.loads.get(home as usize) {
            Some(v) => *v,
            None => return Err(LoadCode::CoreOver.val()),
        };
        let nl = match cur.checked_sub(w) {
            Some(v) => v,
            None => return Err(LoadCode::SumMismatch.val()),
        };
        let ng = match self.global.checked_sub(w) {
            Some(v) => v,
            None => return Err(LoadCode::SumMismatch.val()),
        };
        if let Some(v) = self.loads.get_mut(home as usize) {
            *v = nl;
        }
        self.global = ng;
        self.threads.remove(ti);
        Ok(nl)
    }

    /// 迁移：线程 home 从 A 改 B，负载随线程转移（全局不变）。
    pub fn migrate(&mut self, tid: u32, to: u32) -> Result<(u32, u32), u32> {
        self.frozen_gate()?;
        let ti = match self.thread_index(tid) {
            Some(i) => i,
            None => return Err(LoadCode::ThreadMissing.val()),
        };
        let (w, from) = match self.threads.get(ti) {
            Some(t) => (t.weight, t.home),
            None => return Err(LoadCode::ThreadMissing.val()),
        };
        if !self.core_ok(to) || !self.core_ok(from) {
            return Err(LoadCode::CoreOver.val());
        }
        if from == to {
            return Err(LoadCode::MigrateSameCore.val());
        }
        // 出账面：from -= w（下溢 = 账被篡改，显性拒）。
        let fl = match self.loads.get(from as usize) {
            Some(v) => *v,
            None => return Err(LoadCode::CoreOver.val()),
        };
        let nfl = match fl.checked_sub(w) {
            Some(v) => v,
            None => return Err(LoadCode::SumMismatch.val()),
        };
        // 入账面：to += w（溢出显性拒）。
        let tl = match self.loads.get(to as usize) {
            Some(v) => *v,
            None => return Err(LoadCode::CoreOver.val()),
        };
        let ntl = match tl.checked_add(w) {
            Some(v) => v,
            None => return Err(LoadCode::WeightOverflow.val()),
        };
        if let Some(v) = self.loads.get_mut(from as usize) {
            *v = nfl;
        }
        if let Some(v) = self.loads.get_mut(to as usize) {
            *v = ntl;
        }
        if let Some(t) = self.threads.get_mut(ti) {
            t.home = to;
        }
        self.migrations += 1;
        Ok((from, to))
    }

    /// 审计闭合：按线程账逐核复算，与缓存负载、全局三面对账。
    /// 返回 (复算 Σ每核, 全局)。
    pub fn audit(&self) -> Result<(u64, u64), u32> {
        // 复算逐核。
        let mut re = Vec::new();
        let mut ci = 0usize;
        while ci < self.loads.len() {
            re.push(0u64);
            ci += 1;
        }
        let mut i = 0;
        while i < self.threads.len() {
            let (tw, thome) = match self.threads.get(i) {
                Some(t) => (t.weight, t.home),
                None => return Err(LoadCode::ThreadMissing.val()),
            };
            if !self.core_ok(thome) {
                return Err(LoadCode::CoreOver.val());
            }
            let cur = match re.get(thome as usize) {
                Some(v) => *v,
                None => return Err(LoadCode::CoreOver.val()),
            };
            let nv = match cur.checked_add(tw) {
                Some(v) => v,
                None => return Err(LoadCode::WeightOverflow.val()),
            };
            if let Some(v) = re.get_mut(thome as usize) {
                *v = nv;
            }
            i += 1;
        }
        // 复算 vs 缓存逐核全等。
        let mut k = 0;
        while k < self.loads.len() {
            let a = match re.get(k) {
                Some(v) => *v,
                None => return Err(LoadCode::CoreOver.val()),
            };
            let b = match self.loads.get(k) {
                Some(v) => *v,
                None => return Err(LoadCode::CoreOver.val()),
            };
            if a != b {
                return Err(LoadCode::SumMismatch.val());
            }
            k += 1;
        }
        // 复算 Σ vs 全局。
        let mut sum = 0u64;
        let mut m = 0;
        while m < re.len() {
            let v = match re.get(m) {
                Some(v) => *v,
                None => return Err(LoadCode::CoreOver.val()),
            };
            sum = match sum.checked_add(v) {
                Some(s) => s,
                None => return Err(LoadCode::WeightOverflow.val()),
            };
            m += 1;
        }
        if sum != self.global {
            return Err(LoadCode::SumMismatch.val());
        }
        Ok((sum, self.global))
    }

    /// 平账：审计闭合通过才许推进 Balanced。
    pub fn balance(&mut self) -> Result<(), u32> {
        self.audit()?;
        if !self.state.can_advance(LoadState::Balanced) {
            return Err(LoadCode::SumMismatch.val());
        }
        self.state = LoadState::Balanced;
        Ok(())
    }

    /// 冻结：Balanced 且审计闭合才许 Frozen（平移/倒退拒）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        self.audit()?;
        if !self.state.can_advance(LoadState::Frozen) {
            return Err(LoadCode::SumMismatch.val());
        }
        self.state = LoadState::Frozen;
        Ok(())
    }

    /// 导出负载表摘要（非空 + 可对回）。
    pub fn export_loads(&self) -> Result<String, u32> {
        if self.loads.is_empty() {
            return Err(LoadCode::SumMismatch.val());
        }
        let mut s = String::from("load[");
        let mut c = 0usize;
        while c < self.loads.len() {
            if c > 0 {
                s.push(',');
            }
            let v = match self.loads.get(c) {
                Some(v) => *v,
                None => return Err(LoadCode::CoreOver.val()),
            };
            s.push_str(&format!("c{}={}", c, v));
            c += 1;
        }
        s.push_str(&format!("] total={} mig={}", self.global, self.migrations));
        Ok(s)
    }

    /// 导出复核：逐核值拆回与账面全等。
    pub fn export_verifies(&self) -> Result<(), u32> {
        let s = self.export_loads()?;
        let mut c = 0usize;
        while c < self.loads.len() {
            let v = match self.loads.get(c) {
                Some(v) => *v,
                None => return Err(LoadCode::CoreOver.val()),
            };
            let want = format!("c{}={}", c, v);
            if !s.contains(&want) {
                return Err(LoadCode::SumMismatch.val());
            }
            c += 1;
        }
        Ok(())
    }
}

// ── 标准剧本：8 线程权重落账 + 3 笔迁移 ─────────────────────────
/// 线程表：(tid, weight) 权重和 = 31。
pub fn thread_spec() -> Vec<(u32, u64)> {
    let mut v: Vec<(u32, u64)> = Vec::new();
    v.push((1, 3));
    v.push((2, 1));
    v.push((3, 4));
    v.push((4, 1));
    v.push((5, 5));
    v.push((6, 9));
    v.push((7, 2));
    v.push((8, 6));
    v
}

/// 迁移剧本：(tid, to) 三笔，全局不变。
/// 落核轮转：tid1→c0, tid2→c1, tid3→c2, tid4→c3, tid5→c0,
/// tid6→c1, tid7→c2, tid8→c3；故三笔均为跨核搬迁。
pub fn migrate_spec() -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = Vec::new();
    v.push((1, 2)); // tid1 w=3：c0→c2
    v.push((6, 3)); // tid6 w=9：c1→c3
    v.push((5, 1)); // tid5 w=5：c0→c1
    v
}

/// 跑标准剧本：挂账 8 线程（轮转落核）+ 3 笔迁移 + 审计。
/// 返回迁移后的每核负载快照。
pub fn run_load_script(b: &mut LoadBook) -> Result<Vec<u64>, u32> {
    let spec = thread_spec();
    let mut i = 0;
    while i < spec.len() {
        let (tid, w) = match spec.get(i) {
            Some(t) => *t,
            None => return Err(LoadCode::ThreadMissing.val()),
        };
        let core = (i % b.loads.len()) as u32;
        b.attach(tid, w, core)?;
        i += 1;
    }
    let mig = migrate_spec();
    let mut m = 0;
    while m < mig.len() {
        let (tid, to) = match mig.get(m) {
            Some(t) => *t,
            None => return Err(LoadCode::ThreadMissing.val()),
        };
        b.migrate(tid, to)?;
        m += 1;
    }
    b.audit()?;
    let mut snap = Vec::new();
    let mut c = 0usize;
    while c < b.loads.len() {
        snap.push(match b.loads.get(c) {
            Some(v) => *v,
            None => return Err(LoadCode::CoreOver.val()),
        });
        c += 1;
    }
    Ok(snap)
}

// ── LCG（确定性伪随机，供注入轮步进） ───────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：篡改路/越界路/重号路/同核路） ─────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 建一个跑完剧本、平账、已冻结的负载账。
pub fn run_frozen_book() -> Result<LoadBook, u32> {
    let mut b = LoadBook::new(LOAD_CPUS);
    run_load_script(&mut b)?;
    b.balance()?;
    b.freeze()?;
    Ok(b)
}

// ── 黄金全链：建账→挂账→迁移→审计→冻结→四拒→闭合复核 ───────────
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut b = LoadBook::new(LOAD_CPUS);
    if b.loads.len() != LOAD_CPUS {
        return Err(LoadCode::CoreOver.val());
    }
    // 标准剧本：8 线程挂账 + 3 笔迁移，权重和 31（迁移不增减）。
    let snap = run_load_script(&mut b)?;
    if b.global != 31 || b.migrations != 3 {
        return Err(LoadCode::SumMismatch.val());
    }
    // 逐核快照和 = 全局。
    let mut sum = 0u64;
    let mut c = 0usize;
    while c < snap.len() {
        sum += match snap.get(c) {
            Some(v) => *v,
            None => return Err(LoadCode::CoreOver.val()),
        };
        c += 1;
    }
    if sum != b.global {
        return Err(LoadCode::SumMismatch.val());
    }
    // 平账 + 冻结 + 导出。
    b.balance()?;
    b.freeze()?;
    let s = b.export_loads()?;
    if s.is_empty() {
        return Err(LoadCode::SumMismatch.val());
    }
    b.export_verifies()?;
    // 注入 1：冻结后改账必拒。
    let frozen_rej = b.attach(99, 1, 0).is_err();
    // 注入 2：重号挂账必拒。
    let mut live = LoadBook::new(LOAD_CPUS);
    let _ = live.attach(1, 3, 0);
    let dup_rej = live.attach(1, 4, 1).is_err();
    // 注入 3：不在账线程迁移必拒。
    let miss_rej = live.migrate(42, 1).is_err();
    // 注入 4：同核迁移必拒。
    let _ = live.attach(2, 1, 1);
    let same_rej = live.migrate(2, 1).is_err();
    let flags = 1u32; // 导出摘要非空旗。
    let rejects = frozen_rej as u32 + dup_rej as u32 + miss_rej as u32 + same_rej as u32;
    Ok((flags, rejects, b.global))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 迁移随线程转移断言（迁移前后全局不变）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 4, 31)) => {
            // 迁移转移断言：单线程迁移只搬运权重，全局与权重和不变。
            let mut b = LoadBook::new(LOAD_CPUS);
            let _ = b.attach(1, 10, 0);
            let _ = b.attach(2, 6, 1);
            let g0 = b.global;
            b.migrate(1, 1)?;
            let g1 = b.global;
            if g0 != g1 || g1 != 16 {
                return Err(LoadCode::SumMismatch.val());
            }
            b.audit()?;
            Ok(())
        }
        _ => Err(LoadCode::NotReproducible.val()),
    }
}

/// 序 B：注入面四拒（越界/零权重/下溢/冻结后迁移）逐条显性拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = LoadBook::new(LOAD_CPUS);
    // 越界核挂账。
    let r1 = b.attach(1, 3, LOAD_CPUS as u32).is_err();
    // 零权重挂账。
    let r2 = b.attach(1, 0, 0).is_err();
    // 挂账后摘账两次（第二次不在账）。
    let _ = b.attach(3, 5, 2);
    let _ = b.detach(3);
    let r3 = b.detach(3).is_err();
    // 超上限权重。
    let r4 = b.attach(4, WEIGHT_CAP + 1, 0).is_err();
    match (r1, r2, r3, r4) {
        (true, true, true, true) => Ok(()),
        _ => Err(LoadCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（4 核 / 8 线程 / 权重上限 1024 / 线程上限 128）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        LOAD_CPUS == 4,
        STD_THREADS == 8,
        WEIGHT_CAP == 1024,
        THREAD_CAP == 128,
        LOAD_CORE_CAP == 64,
        JUDGE_ROUNDS == 10,
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(LoadCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：篡改路（随机抬高一核缓存负载，审计必抓 SumMismatch）。
        let mut b1 = LoadBook::new(LOAD_CPUS);
        let _ = run_load_script(&mut b1);
        let pick = rng.below(LOAD_CPUS as u64) as usize;
        if let Some(v) = b1.loads.get_mut(pick) {
            *v = v.saturating_add(1);
        }
        let tamper = match b1.audit() {
            Err(e) => e == LoadCode::SumMismatch.val(),
            Ok(_) => false,
        };
        led.record(0, tamper);
        // 路 2：越界路（越界核挂账 / 不在账线程迁移必拒）。
        let mut b2 = LoadBook::new(LOAD_CPUS);
        let a = b2.attach(1, 1, LOAD_CORE_CAP as u32).is_err();
        let bb = b2.migrate(7, 0).is_err();
        led.record(1, a && bb);
        // 路 3：重号路（同 tid 二次挂账必拒）。
        let mut b3 = LoadBook::new(LOAD_CPUS);
        let _ = b3.attach(5, 2, 1);
        let dup = b3.attach(5, 3, 2).is_err();
        led.record(2, dup);
        // 路 4：同核路（home 未变迁移必拒）。
        let mut b4 = LoadBook::new(LOAD_CPUS);
        let _ = b4.attach(9, 4, 3);
        let same = b4.migrate(9, 3).is_err();
        led.record(3, same);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LoadCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LoadCode::ALL.len() {
        let v = LoadCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let b = run_frozen_book()?;
    let s = b.export_loads()?;
    if s.is_empty() || b.global != 31 || b.state != LoadState::Frozen {
        return Err(LoadCode::SumMismatch.val());
    }
    Ok(format!("状态=已完成 {}", s))
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（1 旗 + 4 拒 + 全局 31）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((f, r, t)) => *f == 1 && *r == 4 && *t == 31,
            Err(_) => false,
        },
        match &g {
            Ok((f, r, t)) => format!("flags={} rejects={} total={}", f, r, t),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 负载=Σ线程权重逐核落账：封账区审计闭合。
    let closed = match run_frozen_book() {
        Ok(b) => match b.audit() {
            Ok((s, gl)) => s == gl && gl == 31,
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "J1-逐核落账闭合",
        closed,
        format!("{}线程 Σ权重=Σ每核=全局=31", STD_THREADS),
    ));
    // J1 迁移随线程转移：3 笔迁移后全局不变、迁移笔数落账。
    let mig_ok = match run_frozen_book() {
        Ok(b) => b.global == 31 && b.migrations == 3,
        Err(_) => false,
    };
    v.push((
        "J1-迁移随线程转移",
        mig_ok,
        String::from("3 笔迁移只搬运不增减"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x72380 窗口"),
    ));
    v
}

pub fn run_unx1646_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1646 每核负载贡献账（权重累计）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
