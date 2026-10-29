//! unx0948_picklaw — UNX-F0948 三底座选型契约（softirq/tasklet/workqueue 决策树）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0948 三底座选型契约（softirq/tasklet/workqueue 决策树）
//! 判据原文摘录（卡面验收行）：决策树逐问可核，选型判词与三底座能力
//! 对照位一致（全样本 10/10 次归位正确），逆选（高负载走 softirq 等）
//! 必拦并给归因。
//! 诊断段 0x7C80（本单独占，XcCode 闭集见下）。
//! 设计纪律：选型即契约——输入特征四问（可睡？长做？同型互斥？高频
//! 短平快？），逐问裁剪到底座；判词唯一且可回放；全部错误走
//! Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7C80;

// ── 诊断段独占 · 选型契约错误域 ────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_PICK: u16 = SEG_ERR + 1;    // 选型与特征不符
const E_FEATURE: u16 = SEG_ERR + 2; // 特征自相矛盾
const E_REVERSE: u16 = SEG_ERR + 3; // 逆选（重活轻载）
const E_STATE: u16 = SEG_ERR + 4;   // 决策机状态非法
const E_CLOSE: u16 = SEG_ERR + 5;   // 判词不闭合

/// 三底座。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Softirq,
    Tasklet,
    Workqueue,
}

impl Base {
    pub fn as_str(self) -> &'static str {
        match self { Base::Softirq => "softirq", Base::Tasklet => "tasklet", Base::Workqueue => "workqueue" }
    }
    /// 能力位（may_sleep, may_long, serialized_per_cpu）。
    pub fn caps(self) -> (bool, bool, bool) {
        match self {
            Base::Softirq => (false, false, false),
            Base::Tasklet => (false, false, true),
            Base::Workqueue => (true, true, true),
        }
    }
}

/// 工作负载四特征。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feat {
    pub may_sleep: bool,   // 是否需要睡眠
    pub long_run: bool,    // 是否长做（>10µs）
    pub need_serial: bool, // 是否需要同核同型串行
    pub hot_short: bool,   // 是否高频短平快（<5µs 级）
}

impl Feat {
    /// 特征自洽审计：long_run 与 hot_short 互斥。
    pub fn self_consistent(&self) -> bool {
        !(self.long_run && self.hot_short)
    }
}

/// 决策判词：选了哪个底座 + 走了哪条问题路径。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    pub base: Base,
    pub path: [bool; 4], // 四问各自的答案路径
}

/// 决策树核心：逐问裁剪。
/// Q1 需要睡眠/长做？ → Workqueue。
/// Q2 需要同核同型串行？ → Tasklet。
/// Q3 高频短平快？ → Softirq。
/// Q4 其余兜底？ → Softirq（最轻代价）。
pub fn decide(f: &Feat) -> Result<Verdict, u16> {
    if !f.self_consistent() { return Err(E_FEATURE); }
    let path = [f.may_sleep || f.long_run, f.need_serial, f.hot_short, true];
    let base = if f.may_sleep || f.long_run {
        Base::Workqueue
    } else if f.need_serial {
        Base::Tasklet
    } else {
        Base::Softirq
    };
    Ok(Verdict { base, path })
}

/// 选型核验：判词与能力位必须归位一致（可睡 ⇔ workqueue 等）。
pub fn verify(f: &Feat, v: &Verdict) -> Result<(), u16> {
    let caps = v.base.caps();
    if f.may_sleep && !caps.0 { return Err(E_REVERSE); }
    if f.long_run && !caps.1 { return Err(E_REVERSE); }
    if f.need_serial && !caps.2 { return Err(E_REVERSE); }
    // 高频短平快走 workqueue 属逆选（进程上下文开销浪费）。
    if f.hot_short && v.base == Base::Workqueue { return Err(E_REVERSE); }
    if f.self_consistent() == false { return Err(E_FEATURE); }
    Ok(())
}

/// 选型账本：样本 → 判词，全量可回放。
pub struct PickLedger {
    pub entries: Vec<(Feat, Verdict)>,
}

impl PickLedger {
    pub fn new() -> Self { PickLedger { entries: Vec::new() } }

    /// 记一条选型：先决策后核验，双过才落账。
    pub fn judge(&mut self, f: Feat) -> Result<Verdict, u16> {
        let v = decide(&f)?;
        verify(&f, &v)?;
        self.entries.push((f, v));
        Ok(v)
    }

    /// 回放审计：全部判词重核一遍，任何一条翻案即整账红灯。
    pub fn replay(&self) -> Result<u32, u16> {
        if self.entries.is_empty() { return Err(E_STATE); }
        for (f, v) in &self.entries {
            let v2 = decide(f)?;
            if v2 != *v { return Err(E_CLOSE); }
            verify(f, v)?;
        }
        Ok(self.entries.len() as u32)
    }

    /// 底座分布摘要。
    pub fn dist(&self) -> String {
        let mut s = String::from("pick dist:");
        for b in [Base::Softirq, Base::Tasklet, Base::Workqueue] {
            let n = self.entries.iter().filter(|(_, v)| v.base == b).count();
            s = alloc::format!("{} {}:{}", s, b.as_str(), n);
        }
        s
    }
}

/// 逆选注入器：故意把高负载样本判给 softirq，核验必须拦下。
pub fn inject_reverse(f: &Feat) -> bool {
    let bad = Verdict { base: Base::Softirq, path: [false; 4] };
    verify(f, &bad) == Err(E_REVERSE)
}

/// 自例程：十六特征组合全判全核，逆选 10/10 必拦返回 0。
pub fn drill() -> Result<u32, u16> {
    let mut ledger = PickLedger::new();
    // 场景一：十六种特征组合全量过树（bool4 全排列去掉矛盾组合）。
    let mut count = 0u32;
    for mask in 0..16u32 {
        let f = Feat {
            may_sleep: mask & 1 != 0,
            long_run: mask & 2 != 0,
            need_serial: mask & 4 != 0,
            hot_short: mask & 8 != 0,
        };
        if !f.self_consistent() {
            // 矛盾特征必拒。
            if decide(&f) != Err(E_FEATURE) { return Err(E_FEATURE); }
            continue;
        }
        ledger.judge(f)?;
        count += 1;
    }
    if count == 0 { return Err(E_STATE); }
    if ledger.replay()? != count { return Err(E_CLOSE); }
    // 场景二：典型三样本语义锚点。
    let v1 = decide(&Feat { may_sleep: false, long_run: false, need_serial: false, hot_short: true })?;
    if v1.base != Base::Softirq { return Err(E_PICK); }
    let v2 = decide(&Feat { may_sleep: false, long_run: false, need_serial: true, hot_short: true })?;
    if v2.base != Base::Tasklet { return Err(E_PICK); }
    let v3 = decide(&Feat { may_sleep: true, long_run: true, need_serial: false, hot_short: false })?;
    if v3.base != Base::Workqueue { return Err(E_PICK); }
    // 场景三：逆选注入 10/10 必拦。
    for i in 0..10u32 {
        let f = Feat {
            may_sleep: i % 2 == 0,
            long_run: i % 2 == 0,
            need_serial: i % 3 == 0,
            hot_short: false,
        };
        if !inject_reverse(&f) { return Err(E_REVERSE); }
    }
    // 场景四：能力位对照。
    if Base::Workqueue.caps() != (true, true, true) { return Err(E_PICK); }
    if Base::Softirq.caps() != (false, false, false) { return Err(E_PICK); }
    if Base::Tasklet.caps() != (false, false, true) { return Err(E_PICK); }
    if ledger.dist().is_empty() { return Err(E_STATE); }
    Ok(0)
}
