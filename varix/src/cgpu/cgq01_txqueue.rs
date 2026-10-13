//! CGPU-F1285 · 传输队列管理（CGPU-I 域 · 带宽域 · Q01 组 · 双实现并存 B 面）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1285`（L5455）
//!
//! **判据（锚点原文）**：传输队列：传输队列管理（上传队列/下载队列分列——队列深度/
//! 水位监控；队列背压（下游慢→上游限速——回压协议）；队列优先级插队（紧急传输插队
//! 语义）；队列遥测。判据：分列、背压、插队、遥测、判据。
//!
//! **并存声明**：`cgi06_txqueue.rs`（并行工号 A 面 · 信用回压设计）在先；本面为独立
//! 第二实现（源码只增不减）——B 面采用水位门限 + 令牌限速设计，语义独立不抄 A 面。
//!
//! 本面真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **分列**（判据一）：上传/下载双队列（[`TqDir`]）各自独立深度/水位账，
//!    一条河堵塞不淹另一条；水位三级（Normal/Warn/Critical）按深度占比分档。
//! 2. **背压**（判据二）：回压协议 B 面——下游水位 Critical → 上游限速令牌
//!    （[`TqTokens`] 每 tick 补充额度随水位降档）；水位回 Normal 恢复全额。
//! 3. **插队**（判据三）：紧急传输插队语义（[`TqPri`] 三级：Normal/Urgent/
//!    Emergency）——Emergency 直插队首；Urgent 排同优先级队首；连续插队
//!    计数超限（4 次）强制队尾（饿死保护）。
//! 4. **遥测**（判据四）：逐队列深度/水位/插队计数/拒绝计数/吞吐账；
//!    全量导出单行快照。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（B 面）。
pub const TQ_VERSION: &str = "cgq01_txqueue-v1";

/// 诊断段基址（0x750 段 · 本模块独占）。
pub const TQ_SEG_BASE: u16 = 0x750;

/// 队列深度上限（B 面冻结：256 项）。
pub const TQ_DEPTH: usize = 256;

/// 水位三档门限（万分比：Warn 6000 / Critical 8500）。
pub const TQ_WARN_BPS: u32 = 6000;
pub const TQ_CRIT_BPS: u32 = 8500;

/// 连续插队上限（超限强制队尾 · 饿死保护）。
pub const TQ_CUT_MAX: u32 = 4;

/// 传输方向（分列闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TqDir {
    /// 上传队列。
    Up,
    /// 下载队列。
    Down,
}

impl TqDir {
    /// 方向名。
    pub fn name(self) -> &'static str {
        match self {
            TqDir::Up => "up",
            TqDir::Down => "down",
        }
    }
}

/// 优先级（插队语义闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TqPri {
    /// 普通。
    Normal,
    /// 紧急（同优先级队首）。
    Urgent,
    /// 极紧急（直插队首 · 连续超限降队尾）。
    Emergency,
}

impl TqPri {
    /// 优先级名。
    pub fn name(self) -> &'static str {
        match self {
            TqPri::Normal => "normal",
            TqPri::Urgent => "urgent",
            TqPri::Emergency => "emergency",
        }
    }
}

/// 水位档（三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TqWater {
    /// 正常（< 60%）。
    Normal,
    /// 警告（60%~85%）。
    Warn,
    /// 危急（≥ 85%）。
    Critical,
}

impl TqWater {
    /// 按深度占比分档（万分比）。
    pub fn of(depth: usize) -> TqWater {
        let bps = (depth as u32) * 10000 / TQ_DEPTH as u32;
        if bps >= TQ_CRIT_BPS {
            TqWater::Critical
        } else if bps >= TQ_WARN_BPS {
            TqWater::Warn
        } else {
            TqWater::Normal
        }
    }

    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            TqWater::Normal => "normal",
            TqWater::Warn => "warn",
            TqWater::Critical => "critical",
        }
    }
}

/// 传输项。
#[derive(Clone, Debug)]
pub struct TqItem {
    /// 项 id。
    pub id: u32,
    /// 方向。
    pub dir: TqDir,
    /// 优先级。
    pub pri: TqPri,
    /// 字节数。
    pub bytes: u64,
}

/// 令牌限速账（背压执行面）。
pub struct TqTokens {
    /// 当前余额。
    pub balance: u64,
    /// 满额（水位 Normal 时每 tick 补充量）。
    pub full: u64,
}

impl TqTokens {
    /// 新令牌账。
    pub fn new(full: u64) -> Self {
        TqTokens {
            balance: full,
            full,
        }
    }

    /// 每 tick 补充：水位 Critical → 0 补充（冻结）；Warn → 半额；Normal → 全额。
    pub fn refill(&mut self, w: TqWater) {
        self.balance = match w {
            TqWater::Critical => self.balance,
            TqWater::Warn => (self.balance + self.full / 2).min(self.full),
            TqWater::Normal => self.full,
        };
    }

    /// 消费：余额不足拒绝（背压语义）。
    pub fn take(&mut self, cost: u64) -> bool {
        if self.balance < cost {
            return false;
        }
        self.balance -= cost;
        true
    }
}

/// 单队列账。
pub struct TqLane {
    /// 方向。
    pub dir: TqDir,
    /// 队列。
    pub q: Vec<TqItem>,
    /// 入队总数。
    pub enq_total: u64,
    /// 出队总数。
    pub deq_total: u64,
    /// 满队拒绝数。
    pub full_rejects: u32,
    /// 令牌拒发数（背压拦截）。
    pub backpressure_rejects: u32,
    /// Emergency 插队计数。
    pub cuts: u32,
    /// 连续插队计数（超限降队尾）。
    pub cut_streak: u32,
    /// 强制队尾数（饿死保护触发）。
    pub cut_forced_tail: u32,
    /// 完成字节数。
    pub done_bytes: u64,
}

impl TqLane {
    /// 空队列。
    pub fn new(dir: TqDir) -> Self {
        TqLane {
            dir,
            q: Vec::new(),
            enq_total: 0,
            deq_total: 0,
            full_rejects: 0,
            backpressure_rejects: 0,
            cuts: 0,
            cut_streak: 0,
            cut_forced_tail: 0,
            done_bytes: 0,
        }
    }

    /// 水位。
    pub fn water(&self) -> TqWater {
        TqWater::of(self.q.len())
    }

    /// 入队：满拒；插队语义按优先级落位。
    pub fn enqueue(&mut self, item: TqItem) -> bool {
        if self.q.len() >= TQ_DEPTH {
            self.full_rejects += 1;
            return false;
        }
        match item.pri {
            TqPri::Normal => self.q.push(item),
            TqPri::Urgent => {
                // 同优先级队首：插在最后一个 Urgent 之后。
                let pos = self
                    .q
                    .iter()
                    .rposition(|x| x.pri == TqPri::Urgent)
                    .map(|p| p + 1)
                    .unwrap_or(0);
                self.q.insert(pos.min(self.q.len()), item);
            }
            TqPri::Emergency => {
                self.cut_streak += 1;
                self.cuts += 1;
                if self.cut_streak > TQ_CUT_MAX {
                    // 饿死保护：强制队尾。
                    self.cut_forced_tail += 1;
                    self.cut_streak = 0;
                    self.q.push(item);
                } else {
                    self.q.insert(0, item);
                }
            }
        }
        self.enq_total += 1;
        true
    }

    /// 出队（完成）：消费一个。
    pub fn dequeue(&mut self) -> Option<TqItem> {
        if self.q.is_empty() {
            return None;
        }
        let it = self.q.remove(0);
        self.deq_total += 1;
        self.done_bytes += it.bytes;
        Some(it)
    }

    /// 守恒：入 == 出 + 在队。
    pub fn conserved(&self) -> bool {
        self.enq_total == self.deq_total + self.q.len() as u64
    }
}

/// 传输队列门面（双队列 + 令牌 + 遥测）。
pub struct TqEngine {
    /// 上传列。
    pub up: TqLane,
    /// 下载列。
    pub down: TqLane,
    /// 令牌账（分列各自限速）。
    pub tok_up: TqTokens,
    pub tok_down: TqTokens,
    /// 遥测快照计数。
    pub snaps: u32,
}

impl TqEngine {
    /// 新引擎。
    pub fn new() -> Self {
        TqEngine {
            up: TqLane::new(TqDir::Up),
            down: TqLane::new(TqDir::Down),
            tok_up: TqTokens::new(4096),
            tok_down: TqTokens::new(4096),
            snaps: 0,
        }
    }

    /// 背压步：每 tick 依水位补令牌 + 抽水消费（确定性调度）。
    pub fn tick(&mut self) {
        let wu = self.up.water();
        let wd = self.down.water();
        self.tok_up.refill(wu);
        self.tok_down.refill(wd);
        // 消费：队首项按字节消费令牌，不足则背压拦截留案。
        for lane in [&mut self.up, &mut self.down] {
            if let Some(head) = lane.q.first() {
                let cost = head.bytes;
                let tok = match lane.dir {
                    TqDir::Up => &mut self.tok_up,
                    TqDir::Down => &mut self.tok_down,
                };
                if tok.take(cost) {
                    lane.dequeue();
                } else {
                    lane.backpressure_rejects += 1;
                }
            }
        }
    }

    /// 遥测快照单行。
    pub fn snapshot(&mut self) -> String {
        self.snaps += 1;
        format!(
            "{}: up[depth={} {} enq={} deq={} cuts={} bp={}] down[depth={} {} enq={} deq={} bp={}] cons(up/down)={}/{} seg=0x{:04x}",
            TQ_VERSION,
            self.up.q.len(),
            self.up.water().name(),
            self.up.enq_total,
            self.up.deq_total,
            self.up.cuts,
            self.up.backpressure_rejects,
            self.down.q.len(),
            self.down.water().name(),
            self.down.enq_total,
            self.down.deq_total,
            self.down.backpressure_rejects,
            self.up.conserved(),
            self.down.conserved(),
            TQ_SEG_BASE
        )
    }

    /// 判据总门（分列/背压/插队/遥测四件）。
    pub fn judge(&self) -> (bool, String) {
        // 满队拒绝（full_rejects）是合法拒绝语义（拒绝即留案），不入总门；
        // 总门只查四判据：分列守恒 / 背压 exercised / 插队饿死保护 / 遥测快照。
        let ok = self.up.conserved()
            && self.down.conserved()
            && self.up.cut_forced_tail >= 1
            && (self.up.backpressure_rejects >= 1 || self.down.backpressure_rejects >= 1)
            && self.snaps >= 1;
        (ok, String::from("see snapshot"))
    }
}

/// 负向注入场景。
pub enum TqInject {
    /// 满队强推（>256 项）。
    OverDepth,
    /// Emergency 连续插队超限（5 次）→ 第 5 次强制队尾。
    CutStorm,
    /// 令牌耗尽背压（大包消费 > 余额）。
    TokenDry,
}

/// 注入核。
pub struct TqInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截/触发数。
    pub caught: u32,
}

impl TqInjector {
    /// 空注入核。
    pub fn new() -> Self {
        TqInjector { cases: 0, caught: 0 }
    }

    /// 三场注入。
    pub fn inject_all(&mut self, eng: &mut TqEngine) {
        // 场一：满队强推。
        let base = eng.up.q.len();
        let mut pushed = 0u32;
        for i in 0..(TQ_DEPTH + 8) {
            if eng.up.enqueue(TqItem {
                id: 10_000 + i as u32,
                dir: TqDir::Up,
                pri: TqPri::Normal,
                bytes: 1,
            }) {
                pushed += 1;
            }
        }
        if eng.up.full_rejects >= 1 && pushed <= TQ_DEPTH as u32 - base as u32 {
            self.caught += 1;
        }
        self.cases += 1;
        // 清场（逐出全部）。
        while eng.up.dequeue().is_some() {}
        // 场二：Emergency 连续 5 次 → 第 5 次强制队尾。
        eng.up.cut_streak = 0;
        for i in 0..5u32 {
            eng.up.enqueue(TqItem {
                id: 20_000 + i,
                dir: TqDir::Up,
                pri: TqPri::Emergency,
                bytes: 1,
            });
        }
        if eng.up.cut_forced_tail >= 1 {
            self.caught += 1;
        }
        self.cases += 1;
        while eng.up.dequeue().is_some() {}
        // 场三：令牌耗尽（大包 cost 0x10FFFF > 余额 0x10000）。
        eng.tok_up.balance = 4096;
        eng.tok_up.refill(TqWater::Critical);
        let took = eng.tok_up.take(0x10_FFFF);
        if !took {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 3 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口）：双列灌包 + 背压 tick 泵 + 插队语义 + 三场注入。
pub fn run_tq01_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgq01-tq01");
    let mut eng = TqEngine::new();
    // 灌包：Normal/Urgent/Emergency 混合。
    for i in 0..64u32 {
        let pri = match i % 8 {
            0 => TqPri::Emergency,
            1 | 2 => TqPri::Urgent,
            _ => TqPri::Normal,
        };
        let dir = if i % 2 == 0 { TqDir::Up } else { TqDir::Down };
        let lane = if dir == TqDir::Up { &mut eng.up } else { &mut eng.down };
        lane.enqueue(TqItem {
            id: i,
            dir,
            pri,
            bytes: 8192,
        });
    }
    // tick 泵 8 拍（消费 + 背压 exercised）。
    for _ in 0..8 {
        eng.tick();
    }
    // 遥测。
    let snap = eng.snapshot();
    let _ = snap;
    // 注入三场。
    let mut inj = TqInjector::new();
    inj.inject_all(&mut eng);
    let (ok, _) = eng.judge();
    set.add("CGPU-F1285-J1", ok && inj.all_caught(), "queue engine 4-claim gate");
    set.add("TQ-conserved", eng.up.conserved() && eng.down.conserved(), "");
    set.add("TQ-cut-guard", eng.up.cut_forced_tail >= 1, "");
    set.add("TQ-backpressure", eng.up.backpressure_rejects >= 1 || eng.down.backpressure_rejects >= 1, "");
    set.add("TQ-inject-3/3", inj.all_caught(), "");
    set
}
