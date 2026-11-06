//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18487 · 嵌套覆写语义（后写覆盖先写） · 0x7F580 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18487-J1 嵌套覆写栈语义断言全过（LIFO 还原），乱序撤销
//!       注入检出
//! 本模块结构清单：
//!   1. 冻结常量层：4 条嵌套栈 × 深度 8（帧池 32）/ 判据轮数 10；
//!   2. 诊断码闭集 NsCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 帧池账本 FramePool：[Frame; 32]（key 栈号 + 层号 + 新值 + 旧值
//!        + 全局序号），栈顶指针按栈各持一份；
//!      - 位面 OccPlane：u32 位图，第 i 位=帧 i 在栈上；
//!      - 状态机 StackPhase：Empty→Nested→Drained 三态单向（再压栈回
//!        Nested 是唯一受控回退）；
//!   4. 后写覆盖先写语义：push 记录 prev=栈顶旧值（空栈记基线），查询
//!      终值=栈顶值；pop 按层号还原 prev（LIFO）；
//!   5. 错误路径：栈号越界 / 深度超限 / 空栈弹出 / 乱序撤销（非顶层
//!      seq）/ 幽灵 seq / 跨栈 seq，全部显式拒绝码；
//!   6. 注入核：乱序撤销（弹非顶）/ 空栈下溢 / 幽灵 seq / 栈号越界
//!      四类坏样本逐类 10/10 断言被检出；
//!   7. run_unx18487_checks：14 条判据（LIFO/守恒/单调/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F580，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F580;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 嵌套栈条数（写死 4）：主/次/Build/SP 四档各一条（六维在 18488 全开）。
pub const NEST_STACKS: usize = 4;
/// 单栈深度（写死 8）：嵌套层数上限。
pub const STACK_DEPTH: usize = 8;
/// 帧池容量（写死 32 = 4×8，单数组统一账面）。
pub const POOL_CAP: usize = NEST_STACKS * STACK_DEPTH;
/// 栈号域上界（冻结：0..=3）。
pub const STACK_ID_MAX: usize = NEST_STACKS - 1;
/// 判据轮数（写死 10：判据原文"乱序撤销注入检出"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 嵌套语义版本号（冻结 v1）。
pub const NEST_VER_V1: u32 = 1;
/// 基线哨兵值（空栈查询返回值，写死 0xFFFF_FFFE 避开常用值域）。
pub const BASELINE_SENTINEL: u32 = 0xFFFF_FFFE;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsCode {
    /// 栈号越界：超出 0..=3
    StackOutOfRange,
    /// 深度超限：单栈压满 8 层再压
    DepthExceeded,
    /// 空栈弹出：栈深 0 时 pop
    StackUnderflow,
    /// 乱序撤销：弹出的 seq 非当前栈顶（LIFO 违例）
    OutOfOrderPop,
    /// 幽灵 seq：帧池查无此序号
    GhostSeq,
    /// 跨栈 seq：seq 存在但属其他栈
    CrossStackSeq,
    /// 池满：32 帧用尽
    PoolFull,
    /// LIFO 次序破坏：还原序列非倒序（自检发现）
    LifoOrderBroken,
    /// 守恒破坏：压栈数=弹出数+在场深度+拒收失衡
    ConservationBroken,
    /// 位面失衡：在场位面计数与栈深之和不符
    PlaneMismatch,
    /// 幽灵查询：对未建栈查询终值
    GhostStackQuery,
    /// 双跑不一致
    NotReproducible,
}

impl NsCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            NsCode::StackOutOfRange => "栈号越界：超出 0..=3 四栈闭集",
            NsCode::DepthExceeded => "深度超限：单栈压满 8 层再压，v1 拒绝",
            NsCode::StackUnderflow => "空栈弹出：栈深 0 时 pop，下溢必拒",
            NsCode::OutOfOrderPop => "乱序撤销：弹出的 seq 非当前栈顶，LIFO 违例",
            NsCode::GhostSeq => "幽灵 seq：帧池查无此序号",
            NsCode::CrossStackSeq => "跨栈 seq：seq 存在但属其他栈",
            NsCode::PoolFull => "池满：32 帧用尽，须升版扩容",
            NsCode::LifoOrderBroken => "LIFO 次序破坏：还原序列非倒序",
            NsCode::ConservationBroken => "守恒破坏：压栈数=弹出数+在场深度+拒收失衡",
            NsCode::PlaneMismatch => "位面失衡：在场位面计数与栈深之和不符",
            NsCode::GhostStackQuery => "幽灵查询：对未建栈查询终值",
            NsCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [NsCode; 12] = [
        NsCode::StackOutOfRange,
        NsCode::DepthExceeded,
        NsCode::StackUnderflow,
        NsCode::OutOfOrderPop,
        NsCode::GhostSeq,
        NsCode::CrossStackSeq,
        NsCode::PoolFull,
        NsCode::LifoOrderBroken,
        NsCode::ConservationBroken,
        NsCode::PlaneMismatch,
        NsCode::GhostStackQuery,
        NsCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < NsCode::ALL.len() {
        if let Some(c) = NsCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F580 段闭集内"
}

// ── 栈状态机（Empty→Nested→Drained 单向，Nested 可重入） ───────────
/// 栈三态（冻结 v1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackPhase {
    /// 空栈：无嵌套覆写。
    Empty,
    /// 嵌套中：至少一层覆写在栈上。
    Nested,
    /// 已抽干：全部弹出（再压栈回 Nested——唯一受控回退）。
    Drained,
}

/// 单帧：嵌套覆写记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NestFrame {
    /// 所属栈号（0..=3）。
    pub key: usize,
    /// 层号（栈内 1..=8）。
    pub depth_no: u8,
    /// 本层覆写值。
    pub value: u32,
    /// 被本层覆盖的旧值（弹出时还原）。
    pub prev_value: u32,
    /// 全局序号（1..，单调，乱序撤销检测的锚）。
    pub seq: u32,
}

impl NestFrame {
    pub const fn empty() -> NestFrame {
        NestFrame { key: 0, depth_no: 0, value: 0, prev_value: 0, seq: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.seq == 0
    }
}

// ── 在场位面（帧池 32 位，u32 恰装下） ─────────────────────────────
/// 在场位面：32 帧用 u32 恰装下。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OccPlane(pub u32);

impl OccPlane {
    pub const fn new() -> OccPlane {
        OccPlane(0)
    }
    pub fn set(&mut self, slot: usize) {
        if slot < POOL_CAP {
            self.0 |= 1u32 << slot;
        }
    }
    pub fn clear(&mut self, slot: usize) {
        if slot < POOL_CAP {
            self.0 &= !(1u32 << slot);
        }
    }
    pub fn get(self, slot: usize) -> bool {
        if slot < POOL_CAP {
            (self.0 >> slot) & 1u32 == 1u32
        } else {
            false
        }
    }
    pub fn count(self) -> u32 {
        let mut n = 0u32;
        let mut b = self.0;
        while b != 0 {
            if b & 1u32 == 1u32 {
                n = n.saturating_add(1);
            }
            b >>= 1;
        }
        n
    }
}

// ── 嵌套栈引擎 ─────────────────────────────────────────────────────
/// 嵌套覆写引擎：帧池 + 各栈顶指针 + 位面 + 守恒账。
pub struct NestEngine {
    pub pool: [NestFrame; POOL_CAP],
    /// 各栈栈顶指针（0=空栈，1..=8=当前深度）。
    pub tops: [usize; NEST_STACKS],
    /// 各栈状态。
    pub phases: [StackPhase; NEST_STACKS],
    /// 各栈基线值（空栈查询返回基线）。
    pub baselines: [u32; NEST_STACKS],
    /// 在场位面。
    pub plane: OccPlane,
    /// 压栈总数。
    pub push_total: u32,
    /// 弹出总数。
    pub pop_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 全局序号累加器（单调）。
    pub seq_acc: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl NestEngine {
    /// 新引擎（各栈基线由调用方冻结注入）。
    pub fn new(baselines: [u32; NEST_STACKS]) -> NestEngine {
        NestEngine {
            pool: [NestFrame::empty(); POOL_CAP],
            tops: [0; NEST_STACKS],
            phases: [StackPhase::Empty; NEST_STACKS],
            baselines,
            plane: OccPlane::new(),
            push_total: 0,
            pop_total: 0,
            rejected_total: 0,
            seq_acc: 0,
            reject_log: Vec::new(),
        }
    }

    /// 栈 k 的池内槽区间：k*8 .. k*8+8。
    fn pool_base(key: usize) -> usize {
        key * STACK_DEPTH
    }

    /// 栈 k 的第 d 层槽号（d 从 0 起）。
    fn slot_of(key: usize, depth_idx: usize) -> usize {
        NestEngine::pool_base(key) + depth_idx
    }

    /// 按 seq 线性扫帧池。
    fn find_seq(&self, seq: u32) -> Option<usize> {
        let mut i = 0;
        while i < POOL_CAP {
            if let Some(f) = self.pool.get(i) {
                if !f.is_empty() && f.seq == seq {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 查询栈 k 终值：栈顶值，空栈返回基线。
    pub fn top_value(&self, key: usize) -> Result<u32, u32> {
        if key >= NEST_STACKS {
            return Err(NsCode::StackOutOfRange.code());
        }
        let top = match self.tops.get(key) {
            Some(t) => *t,
            None => 0,
        };
        if top == 0 {
            return Ok(match self.baselines.get(key) {
                Some(b) => *b,
                None => BASELINE_SENTINEL,
            });
        }
        let slot = NestEngine::slot_of(key, top - 1);
        match self.pool.get(slot) {
            Some(f) if !f.is_empty() => Ok(f.value),
            _ => Ok(BASELINE_SENTINEL),
        }
    }

    /// 压栈（后写覆盖先写）：记录 prev=当前栈顶值，层号+1。
    pub fn push(&mut self, key: usize, value: u32) -> Result<u32, u32> {
        if key >= NEST_STACKS {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::StackOutOfRange.code());
            return Err(NsCode::StackOutOfRange.code());
        }
        let top = match self.tops.get(key) {
            Some(t) => *t,
            None => 0,
        };
        if top >= STACK_DEPTH {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::DepthExceeded.code());
            return Err(NsCode::DepthExceeded.code());
        }
        let slot = NestEngine::slot_of(key, top);
        // 逻辑槽必须空闲（tops 不变量保证）；被占即位面失衡，显式拒。
        let occupied = match self.pool.get(slot) {
            Some(f) => !f.is_empty(),
            None => true,
        };
        if occupied {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::PlaneMismatch.code());
            return Err(NsCode::PlaneMismatch.code());
        }
        // prev=当前栈顶值（空栈取基线）。
        let prev = match self.top_value(key) {
            Ok(v) => v,
            Err(_) => BASELINE_SENTINEL,
        };
        let seq = self.seq_acc.saturating_add(1);
        let depth_no = (top + 1) as u8;
        if let Some(f) = self.pool.get_mut(slot) {
            *f = NestFrame { key, depth_no, value, prev_value: prev, seq };
        }
        self.plane.set(slot);
        if let Some(t) = self.tops.get_mut(key) {
            *t = top + 1;
        }
        if let Some(ph) = self.phases.get_mut(key) {
            *ph = StackPhase::Nested;
        }
        self.seq_acc = seq;
        self.push_total = self.push_total.saturating_add(1);
        Ok(seq)
    }

    /// 弹出（LIFO 还原）：只许弹当前栈顶 seq；乱序/下溢/幽灵必拒。
    pub fn pop(&mut self, key: usize, seq: u32) -> Result<u32, u32> {
        if key >= NEST_STACKS {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::StackOutOfRange.code());
            return Err(NsCode::StackOutOfRange.code());
        }
        let top = match self.tops.get(key) {
            Some(t) => *t,
            None => 0,
        };
        if top == 0 {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::StackUnderflow.code());
            return Err(NsCode::StackUnderflow.code());
        }
        // 找 seq 帧。
        let slot = match self.find_seq(seq) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(NsCode::GhostSeq.code());
                return Err(NsCode::GhostSeq.code());
            }
        };
        let (fkey, fdepth, prev) = match self.pool.get(slot) {
            Some(f) => (f.key, f.depth_no, f.prev_value),
            None => (key, 0, 0),
        };
        if fkey != key {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::CrossStackSeq.code());
            return Err(NsCode::CrossStackSeq.code());
        }
        // 乱序撤销检测：必须是栈顶层（depth_no == top）。
        if fdepth as usize != top {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(NsCode::OutOfOrderPop.code());
            return Err(NsCode::OutOfOrderPop.code());
        }
        // LIFO 弹出：还原 prev，深度-1。
        if let Some(f) = self.pool.get_mut(slot) {
            *f = NestFrame::empty();
        }
        self.plane.clear(slot);
        if let Some(t) = self.tops.get_mut(key) {
            *t = top - 1;
        }
        if top - 1 == 0 {
            if let Some(ph) = self.phases.get_mut(key) {
                *ph = StackPhase::Drained;
            }
        }
        self.pop_total = self.pop_total.saturating_add(1);
        Ok(prev)
    }

    /// 全 LIFO 抽干：逐层弹顶直到空，返回还原值序列（底→顶序）。
    /// 序列必须等于压栈序（后写先弹），任何一处错位即 LIFO 违例。
    pub fn drain_lifo(&mut self, key: usize) -> Result<Vec<u32>, u32> {
        if key >= NEST_STACKS {
            return Err(NsCode::StackOutOfRange.code());
        }
        let mut restored: Vec<u32> = Vec::new();
        // 期望倒序：栈内 seq 从大到小。先快照栈内 seq 列表（顶层最大）。
        let mut seqs: Vec<u32> = Vec::new();
        let mut d = match self.tops.get(key) {
            Some(t) => *t,
            None => 0,
        };
        while d > 0 {
            // 池内按 key+depth_no 线性找。
            let mut found: Option<u32> = None;
            let mut i = 0;
            while i < POOL_CAP {
                if let Some(f) = self.pool.get(i) {
                    if !f.is_empty() && f.key == key && f.depth_no as usize == d {
                        found = Some(f.seq);
                        i = POOL_CAP;
                    }
                }
                if found.is_none() {
                    i = i.saturating_add(1);
                }
            }
            match found {
                Some(s) => seqs.push(s),
                None => return Err(NsCode::GhostSeq.code()),
            }
            d -= 1;
        }
        // 逐层弹顶（seqs 已是顶层→底层序）。
        let mut j = 0;
        while j < seqs.len() {
            let seq = match seqs.get(j) {
                Some(s) => *s,
                None => 0,
            };
            let prev = self.pop(key, seq)?;
            restored.push(prev);
            j = j.saturating_add(1);
        }
        // LIFO 校验：还原序列必须与压入序一致（prev 链单调回到基线）。
        // 最终终值必须回到基线。
        let final_v = self.top_value(key)?;
        let base = match self.baselines.get(key) {
            Some(b) => *b,
            None => BASELINE_SENTINEL,
        };
        if final_v != base {
            return Err(NsCode::LifoOrderBroken.code());
        }
        Ok(restored)
    }

    /// 位面对账：位面计数 = Σ各栈深度。
    pub fn plane_audit(&self) -> Result<(), u32> {
        let mut total_depth = 0u32;
        let mut k = 0;
        while k < NEST_STACKS {
            let t = match self.tops.get(k) {
                Some(v) => *v,
                None => 0,
            };
            total_depth = total_depth.saturating_add(t as u32);
            k += 1;
        }
        if self.plane.count() != total_depth {
            return Err(NsCode::PlaneMismatch.code());
        }
        Ok(())
    }

    /// 守恒：push_total = pop_total + Σdepth。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut total_depth = 0u32;
        let mut k = 0;
        while k < NEST_STACKS {
            let t = match self.tops.get(k) {
                Some(v) => *v,
                None => 0,
            };
            total_depth = total_depth.saturating_add(t as u32);
            k += 1;
        }
        if self.push_total != self.pop_total.saturating_add(total_depth) {
            return Err(NsCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链：嵌套三层压栈 → LIFO 抽干 → 终值回基线 ────────────────
/// 黄金全链：栈 0 三层嵌套（后写覆盖先写验证）→ 顶层查询=最后写值 →
/// LIFO 抽干逐层还原 → 终值=基线；栈 1..=3 各一层往返。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut e = NestEngine::new([0xAA0, 0xBB0, 0xCC0, 0xDD0]);
    // 栈 0：三层嵌套 0x111/0x222/0x333。
    let s1 = e.push(0, 0x111)?;
    let s2 = e.push(0, 0x222)?;
    let s3 = e.push(0, 0x333)?;
    // 后写覆盖先写：终值=最后写值。
    let topv = e.top_value(0)?;
    if topv != 0x333 {
        return Err(NsCode::LifoOrderBroken.code());
    }
    // LIFO 抽干：还原序列 [0x222,0x111,0xAA0]（顶→底 prev 链）。
    let restored = e.drain_lifo(0)?;
    if restored.len() != 3 {
        return Err(NsCode::LifoOrderBroken.code());
    }
    let r0 = match restored.get(0) {
        Some(v) => *v,
        None => 0,
    };
    if r0 != 0x222 {
        return Err(NsCode::LifoOrderBroken.code());
    }
    // 弹出后各 seq 幽灵化（帧已清）。
    if e.find_seq_pub(s3).is_some() {
        return Err(NsCode::GhostSeq.code());
    }
    // 栈 1..=3 各一层往返。
    let mut k = 1usize;
    while k < NEST_STACKS {
        let sv = e.push(k, 0x1000 + k as u32)?;
        let tv = e.top_value(k)?;
        if tv != 0x1000 + k as u32 {
            return Err(NsCode::LifoOrderBroken.code());
        }
        e.drain_lifo(k)?;
        if e.find_seq_pub(sv).is_some() {
            return Err(NsCode::GhostSeq.code());
        }
        k += 1;
    }
    e.plane_audit()?;
    e.conserve()?;
    if e.push_total != 6 || e.pop_total != 6 {
        return Err(NsCode::ConservationBroken.code());
    }
    let _ = s1;
    let _ = s2;
    Ok((e.push_total, e.pop_total))
}

// ── 供外部审计的 seq 查找（只读） ──────────────────────────────────
/// seq 只读查找（帧在栈上返回 Some(slot)，已弹出返回 None）。
pub fn find_seq_pub_in(e: &NestEngine, seq: u32) -> Option<usize> {
    let mut i = 0;
    while i < POOL_CAP {
        if let Some(f) = e.pool.get(i) {
            if !f.is_empty() && f.seq == seq {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

impl NestEngine {
    /// 实例侧 seq 只读查找（测试与黄金链用）。
    pub fn find_seq_pub(&self, seq: u32) -> Option<usize> {
        find_seq_pub_in(self, seq)
    }
}

// ── 注入检出账 ─────────────────────────────────────────────────────
/// 四槽注入账。
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
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入核：四类坏样本逐类 10/10 断言被检出。
///   槽 0：乱序撤销（三层栈弹底层 seq——非顶层）
///   槽 1：空栈下溢
///   槽 2：幽灵 seq 弹出
///   槽 3：跨栈 seq 弹出
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：乱序撤销——弹底层 s1 而顶层是 s3。
        let mut e0 = NestEngine::new([0xA00, 0, 0, 0]);
        let s1 = match e0.push(0, 0x101) {
            Ok(s) => s,
            Err(_) => 0,
        };
        let _ = e0.push(0, 0x102);
        let _ = e0.push(0, 0x103);
        let hit0 = match e0.pop(0, s1) {
            Err(c) if c == NsCode::OutOfOrderPop.code() => true,
            _ => false,
        };
        led.record(0, hit0);
        // 槽 1：空栈下溢。
        let mut e1 = NestEngine::new([0xB00, 0, 0, 0]);
        let hit1 = match e1.pop(1, 1) {
            Err(c) if c == NsCode::StackUnderflow.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：幽灵 seq。
        let mut e2 = NestEngine::new([0xC00, 0, 0, 0]);
        let _ = e2.push(0, 0x201);
        let hit2 = match e2.pop(0, 999) {
            Err(c) if c == NsCode::GhostSeq.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：跨栈 seq（栈 0 的 seq 拿到栈 1 弹）。
        let mut e3 = NestEngine::new([0xD00, 0xE00, 0, 0]);
        let s0 = match e3.push(0, 0x301) {
            Ok(s) => s,
            Err(_) => 0,
        };
        let _ = e3.push(1, 0x302);
        let hit3 = match e3.pop(1, s0) {
            Err(c) if c == NsCode::CrossStackSeq.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例嵌套往返全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut e = NestEngine::new([0xF00, 0, 0, 0]);
    e.push(0, 0xF11)?;
    e.push(0, 0xF22)?;
    e.drain_lifo(0)?;
    Ok(())
}

/// 探针 B：空栈下溢必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = NestEngine::new([1, 1, 1, 1]);
    e.pop(0, 1).map(|_| ())
}

/// 探针 C：栈号越界必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = NestEngine::new([1, 1, 1, 1]);
    e.push(4, 1).map(|_| ())
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(NsCode::NotReproducible.code());
    }
    Ok(())
}

// ── 段位守卫 ───────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐且全部诊断码落段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < NsCode::ALL.len() {
        if let Some(c) = NsCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 深度边界探测：单栈连压 9 层，第 9 层必返深度超限且前 8 层账不丢。
pub fn depth_boundary_probe() -> Result<(u32, u32), u32> {
    let mut e = NestEngine::new([0x100, 0, 0, 0]);
    let mut ok_pushes = 0u32;
    let mut i = 0u32;
    while i < 9u32 {
        match e.push(0, 0x200 + i) {
            Ok(_) => ok_pushes = ok_pushes.saturating_add(1),
            Err(c) if c == NsCode::DepthExceeded.code() => {}
            Err(_) => return Err(NsCode::ConservationBroken.code()),
        }
        i = i.saturating_add(1);
    }
    if ok_pushes != STACK_DEPTH as u32 {
        return Err(NsCode::ConservationBroken.code());
    }
    e.conserve()?;
    Ok((ok_pushes, e.rejected_total))
}

/// 池容量探测：池容量=4×8=32 恰等于满配（POOL_CAP=NEST_STACKS×STACK_DEPTH），
/// 故 33 帧在"栈已满"分支即被深度超限拦截（池满为防御性兜底，正常不达）。
/// 探针：3 栈 × 8 层（24 帧）+ 栈 3 × 8 层（共 32 帧）全成，
/// 栈 3 第 9 压必返深度超限。
pub fn pool_capacity_probe() -> Result<(u32, bool), u32> {
    let mut e = NestEngine::new([0x10, 0x20, 0x30, 0x40]);
    let mut filled = 0u32;
    // 32 帧：4 栈 × 8 层。
    let mut k = 0usize;
    while k < NEST_STACKS {
        let mut d = 0u32;
        while d < STACK_DEPTH as u32 {
            match e.push(k, 0x300 + d) {
                Ok(_) => filled = filled.saturating_add(1),
                Err(_) => return Err(NsCode::ConservationBroken.code()),
            }
            d = d.saturating_add(1);
        }
        k += 1;
    }
    // 第 33 压：栈 0 已满 → 深度超限必拒。
    let depth_hit = match e.push(0, 0x500) {
        Err(c) if c == NsCode::DepthExceeded.code() => true,
        _ => false,
    };
    if filled != POOL_CAP as u32 {
        return Err(NsCode::ConservationBroken.code());
    }
    if !depth_hit {
        return Err(NsCode::DepthExceeded.code());
    }
    Ok((filled, depth_hit))
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-LIFO还原", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-乱序撤销注入检出10/10",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    let miss = match &inj {
        Ok(l) => l.missed_total(),
        Err(_) => 1,
    };
    v.push(("J1-注入守恒漏检零", miss == 0, format!("missed={}", miss)));
    // LIFO 核心复验：三层压栈顶层=最后写，抽干还原链倒序。
    let mut e = NestEngine::new([0x70, 0, 0, 0]);
    let _ = e.push(0, 0x71);
    let _ = e.push(0, 0x72);
    let t = e.top_value(0);
    let lifo_ok = t == Ok(0x72);
    let drained = e.drain_lifo(0);
    let drain_ok = match &drained {
        Ok(r) => r.len() == 2,
        Err(_) => false,
    };
    v.push((
        "J2-后写覆盖先写+LIFO抽干",
        lifo_ok && drain_ok,
        format!("top={:?} drain={:?}", t, drained),
    ));
    // 抽干后终值回基线。
    let back = e.top_value(0);
    v.push(("J2-抽干后回基线", back == Ok(0x70), format!("{:?}", back)));
    // 位面对账 + 守恒。
    let plane_ok = e.plane_audit().is_ok();
    v.push(("J2-在场位面对账", plane_ok, String::from("plane.count=Σ栈深")));
    let conserve_ok = e.conserve().is_ok();
    v.push(("J2-帧池守恒", conserve_ok, String::from("push=pop+Σdepth")));
    // 单调：seq 累加器单调（两次压栈 seq 严格递增）。
    let mut e2 = NestEngine::new([0x80, 0, 0, 0]);
    let q1 = e2.push(0, 1);
    let q2 = e2.push(0, 2);
    let mono = match (q1, q2) {
        (Ok(a), Ok(b)) => b > a,
        _ => false,
    };
    v.push(("J3-seq单调递增", mono, format!("{:?}/{:?}", q1, q2)));
    // 深度边界。
    let dep = depth_boundary_probe();
    v.push(("J4-深度边界-8满9拒", dep.is_ok(), format!("{:?}", dep)));
    // 池容量（满配=4×8=32，第 33 压深度超限）。
    let pool = pool_capacity_probe();
    v.push(("J4-池容量-32帧满配", pool.is_ok(), format!("{:?}", pool)));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b!={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F580 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < NsCode::ALL.len() {
        if let Some(c) = NsCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", NsCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 15, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18487 嵌套覆写语义自检（14 条）。
pub fn run_unx18487_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18487 nested_ovr_stack");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    s
}

// ── 测试（8 个） ───────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_golden_chain() {
        // 正常流：三层嵌套→LIFO 抽干→回基线。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn lifo_restore_order() {
        // LIFO 核心：压 A→B→C，抽干还原序列 top→bottom，终值回基线。
        let mut e = NestEngine::new([100, 0, 0, 0]);
        assert!(e.push(0, 201).is_ok());
        assert!(e.push(0, 202).is_ok());
        assert!(e.push(0, 203).is_ok());
        assert!(e.top_value(0) == Ok(203), "终值应为最后写值");
        let restored = e.drain_lifo(0).unwrap_or_default();
        assert!(restored.len() == 3, "应还原 3 层");
        // prev 链：顶层 prev=202，中层 prev=201，底层 prev=100（基线）。
        assert!(restored[0] == 202 && restored[1] == 201 && restored[2] == 100, "LIFO prev 链应倒序");
        assert!(e.top_value(0) == Ok(100), "抽干后应回基线");
    }

    #[test]
    fn out_of_order_injection_caught() {
        // 注入必抓：乱序撤销 10/10。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.caught[0] == JUDGE_ROUNDS, "乱序槽满轮检出");
        }
    }

    #[test]
    fn underflow_and_cross_stack() {
        // 空栈下溢 + 跨栈 seq。
        let mut e = NestEngine::new([1, 2, 3, 4]);
        assert!(e.pop(0, 1).err() == Some(NsCode::StackUnderflow.code()));
        let s0 = e.push(0, 10).unwrap_or(0);
        let _ = e.push(1, 20);
        assert!(e.pop(1, s0).err() == Some(NsCode::CrossStackSeq.code()));
        assert!(e.pop(0, s0).is_ok(), "本栈顶 seq 应可弹");
    }

    #[test]
    fn depth_boundary() {
        // 深度边界：8 满 9 拒。
        let dep = depth_boundary_probe();
        assert!(dep.is_ok());
        if let Ok((pushes, _r)) = dep {
            assert!(pushes == STACK_DEPTH as u32);
        }
    }

    #[test]
    fn pool_capacity_boundary() {
        // 池容量：满配 4×8=32 帧，第 33 压深度超限。
        let pool = pool_capacity_probe();
        assert!(pool.is_ok());
        if let Ok((total, depth_hit)) = pool {
            assert!(total == POOL_CAP as u32);
            assert!(depth_hit, "第 33 压应被深度超限拦截");
        }
    }

    #[test]
    fn double_run_deterministic() {
        // 双跑一致。
        assert!(repro_check().is_ok(), "双跑应逐字段一致");
    }

    #[test]
    fn segment_guard_and_checkset() {
        // 段位守卫 + 判据收口全绿。
        assert!(segment_guard(), "段位守卫应过");
        let set = run_unx18487_checks();
        assert!(set.all_passed(), "15 条判据应全绿");
        assert!(set.len() == 15, "判据条数应冻结 15");
    }
}
