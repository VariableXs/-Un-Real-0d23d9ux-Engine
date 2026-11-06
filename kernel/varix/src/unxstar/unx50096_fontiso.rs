//! UNX-F50096 字体下载会话隔离（任务间零字体泄漏）（0x43820 段）
//!
//! 锚点：UNX-F50096-J1 隔离 ×10 断言；判据锚定 §AI-63 §2 专题二。
//!
//! 字体下载是多任务共享链路里最容易串味的一环：A 任务下载的授权字体若被
//! B 任务解析到，不只是显示错字，而是许可越界。隔离的做法不是「约定不要
//! 互相看」，而是让所有权表成为唯一事实源——每个字体摘要只能挂在一个
//! 任务名下，跨任务解析直接返回 `Leak`，关闭会话时只释放本任务的所有权。
//! 判据用 10 个任务两两交叉解析（100 次）验证「零泄漏」，并检查关闭后
//! 所有权计数严格下降、释放的字体可被新任务重新取得（说明没有残留引用）。
//!
//! 结构清单：
//! * 会话表 [`IsolationHub`] + 会话 [`IsoSession`] + 所有权 [`Ownership`]；
//! * 状态机 Open→Active→Closed（关闭幂等拒绝，代次递增）；
//! * 审计 [`IsolationHub::audit`]：双向核对所有权与会话字体集合；
//! * 快照 [`IsolationHub::snapshot`]：跨机比对隔离状态；
//! * 注入核 [`injection_core`]：10 类坏样本逐类断言具体诊断码。
//!
//! 诊断码闭集：`SEG_BASE + n`，n ∈ [1,10]。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43820 独占 0x20）。
pub const SEG_BASE: u32 = 0x43820;

pub const E_EMPTY: u16 = 1;
pub const E_DUPTASK: u16 = 2;
pub const E_NOTFOUND: u16 = 3;
pub const E_NOTACTIVE: u16 = 4;
pub const E_CLOSED: u16 = 5;
pub const E_LEAK: u16 = 6;
pub const E_CAP: u16 = 7;
pub const E_DUPFONT: u16 = 8;
pub const E_UNKNOWNFONT: u16 = 9;
pub const E_STATE: u16 = 10;

const FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x1000_0000_01b3;

/// 隔离面错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsoError {
    Empty,
    DupTask,
    NotFound,
    NotActive,
    Closed,
    Leak,
    Cap,
    DupFont,
    UnknownFont,
    State,
}

impl IsoError {
    /// 段内序号（闭集 1..=10）。
    pub fn ordinal(self) -> u16 {
        match self {
            IsoError::Empty => E_EMPTY,
            IsoError::DupTask => E_DUPTASK,
            IsoError::NotFound => E_NOTFOUND,
            IsoError::NotActive => E_NOTACTIVE,
            IsoError::Closed => E_CLOSED,
            IsoError::Leak => E_LEAK,
            IsoError::Cap => E_CAP,
            IsoError::DupFont => E_DUPFONT,
            IsoError::UnknownFont => E_UNKNOWNFONT,
            IsoError::State => E_STATE,
        }
    }

    /// 全仓诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            IsoError::Empty => "task id zero is reserved",
            IsoError::DupTask => "task already has a session",
            IsoError::NotFound => "task has no session",
            IsoError::NotActive => "session has no fonts yet",
            IsoError::Closed => "session already closed",
            IsoError::Leak => "font owned by another task",
            IsoError::Cap => "session table full",
            IsoError::DupFont => "font already attached to this task",
            IsoError::UnknownFont => "font id zero is invalid",
            IsoError::State => "ownership table inconsistent",
        }
    }
}

/// 会话档位。Closed 是终态，代次加一供审计区分「重开」与「旧会话」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Open,
    Active,
    Closed,
}

impl SessionState {
    /// 档位码（0..2）。
    pub fn code(self) -> u8 {
        match self {
            SessionState::Open => 0,
            SessionState::Active => 1,
            SessionState::Closed => 2,
        }
    }

    /// 档位标签。
    pub fn label(self) -> &'static str {
        match self {
            SessionState::Open => "open",
            SessionState::Active => "active",
            SessionState::Closed => "closed",
        }
    }
}

/// 一条所有权记录：字体摘要 → 任务。全表内字体摘要唯一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ownership {
    pub font: u64,
    pub task: u16,
}

/// 任务会话：只记录本任务拥有的字体摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoSession {
    pub task: u16,
    pub state: SessionState,
    pub fonts: Vec<u64>,
    pub generation: u16,
}

/// 隔离中枢。所有查询都走 `owners`，会话里的 `fonts` 只是反向索引，
/// 审计会双向核对，防止两张表各自漂移。
pub struct IsolationHub {
    pub sessions: Vec<IsoSession>,
    pub owners: Vec<Ownership>,
    cap: usize,
}

impl IsolationHub {
    /// 新建中枢。
    pub fn new(cap: usize) -> IsolationHub {
        IsolationHub {
            sessions: Vec::with_capacity(cap),
            owners: Vec::new(),
            cap,
        }
    }

    /// 当前所有权条数。
    pub fn count(&self) -> usize {
        self.owners.len()
    }

    /// 会话数。
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    fn index_of(&self, task: u16) -> Result<usize, IsoError> {
        let mut i = 0usize;
        while let Some(s) = self.sessions.get(i) {
            if s.task == task {
                return Ok(i);
            }
            i += 1;
        }
        Err(IsoError::NotFound)
    }

    /// 只读取会话。
    pub fn session(&self, task: u16) -> Result<&IsoSession, IsoError> {
        let idx = self.index_of(task)?;
        match self.sessions.get(idx) {
            Some(s) => Ok(s),
            None => Err(IsoError::NotFound),
        }
    }

    /// 开会话。task 0 保留；同任务重复开显式拒绝。
    pub fn open(&mut self, task: u16) -> Result<(), IsoError> {
        if task == 0 {
            return Err(IsoError::Empty);
        }
        if self.index_of(task).is_ok() {
            return Err(IsoError::DupTask);
        }
        if self.sessions.len() >= self.cap {
            return Err(IsoError::Cap);
        }
        self.sessions.push(IsoSession {
            task,
            state: SessionState::Open,
            fonts: Vec::new(),
            generation: 0,
        });
        Ok(())
    }

    /// 挂载字体。跨任务撞车返回 Leak（而不是静默共享）。
    pub fn attach(&mut self, task: u16, font: u64) -> Result<(), IsoError> {
        if font == 0 {
            return Err(IsoError::UnknownFont);
        }
        let idx = self.index_of(task)?;
        let closed = match self.sessions.get(idx) {
            Some(s) => s.state == SessionState::Closed,
            None => return Err(IsoError::NotFound),
        };
        if closed {
            return Err(IsoError::Closed);
        }
        let mut i = 0usize;
        while let Some(o) = self.owners.get(i) {
            if o.font == font {
                if o.task == task {
                    return Err(IsoError::DupFont);
                }
                return Err(IsoError::Leak);
            }
            i += 1;
        }
        self.owners.push(Ownership { font, task });
        if let Some(s) = self.sessions.get_mut(idx) {
            s.fonts.push(font);
            s.state = SessionState::Active;
        }
        Ok(())
    }

    /// 解析字体：本任务拥有→true；他人拥有→Leak；无人拥有→false。
    /// Open 会话还没有字体，查询返回 NotActive 以区分「没下过」与「没有」。
    pub fn resolve(&self, task: u16, font: u64) -> Result<bool, IsoError> {
        let idx = self.index_of(task)?;
        let state = match self.sessions.get(idx) {
            Some(s) => s.state,
            None => return Err(IsoError::NotFound),
        };
        match state {
            SessionState::Open => return Err(IsoError::NotActive),
            SessionState::Closed => return Err(IsoError::Closed),
            SessionState::Active => {}
        }
        let mut i = 0usize;
        while let Some(o) = self.owners.get(i) {
            if o.font == font {
                return if o.task == task {
                    Ok(true)
                } else {
                    Err(IsoError::Leak)
                };
            }
            i += 1;
        }
        Ok(false)
    }

    /// 关闭会话：只摘除本任务的所有权，代次加一。重复关闭显式拒绝。
    pub fn close(&mut self, task: u16) -> Result<u16, IsoError> {
        let idx = self.index_of(task)?;
        let already = match self.sessions.get(idx) {
            Some(s) => s.state == SessionState::Closed,
            None => return Err(IsoError::NotFound),
        };
        if already {
            return Err(IsoError::Closed);
        }
        self.owners.retain(|o| o.task != task);
        match self.sessions.get_mut(idx) {
            Some(s) => {
                s.fonts.clear();
                s.state = SessionState::Closed;
                s.generation = s.generation.saturating_add(1);
                Ok(s.generation)
            }
            None => Err(IsoError::NotFound),
        }
    }

    /// 双向审计：每个所有权都能在对应会话里找到；每个会话字体都有且仅有
    /// 一条同任务所有权；两侧计数相等。
    pub fn audit(&self) -> Result<(), IsoError> {
        let mut i = 0usize;
        while let Some(o) = self.owners.get(i) {
            let mut found = false;
            let mut j = 0usize;
            while let Some(s) = self.sessions.get(j) {
                if s.task == o.task {
                    let mut k = 0usize;
                    while let Some(f) = s.fonts.get(k) {
                        if *f == o.font {
                            found = true;
                        }
                        k += 1;
                    }
                }
                j += 1;
            }
            if !found {
                return Err(IsoError::State);
            }
            i += 1;
        }
        let mut total = 0usize;
        let mut i = 0usize;
        while let Some(s) = self.sessions.get(i) {
            let mut k = 0usize;
            while let Some(f) = s.fonts.get(k) {
                let mut hits = 0usize;
                let mut j = 0usize;
                while let Some(o) = self.owners.get(j) {
                    if o.font == *f {
                        if o.task != s.task {
                            return Err(IsoError::State);
                        }
                        hits += 1;
                    }
                    j += 1;
                }
                if hits != 1 {
                    return Err(IsoError::State);
                }
                let mut m = k + 1;
                while let Some(g) = s.fonts.get(m) {
                    if g == f {
                        return Err(IsoError::State);
                    }
                    m += 1;
                }
                total += 1;
                k += 1;
            }
            i += 1;
        }
        if total != self.owners.len() {
            return Err(IsoError::State);
        }
        Ok(())
    }

    /// 状态快照：会话（任务/档位/代次/字体表）与所有权按序折叠。
    pub fn snapshot(&self) -> u64 {
        let mut h = (FNV_SEED ^ self.sessions.len() as u64).wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while let Some(s) = self.sessions.get(i) {
            h = (h ^ s.task as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ s.state.code() as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ s.generation as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ s.fonts.len() as u64).wrapping_mul(FNV_PRIME);
            let mut k = 0usize;
            while let Some(f) = s.fonts.get(k) {
                h = (h ^ *f).wrapping_mul(FNV_PRIME);
                k += 1;
            }
            i += 1;
        }
        let mut i = 0usize;
        while let Some(o) = self.owners.get(i) {
            h = (h ^ o.font).wrapping_mul(FNV_PRIME);
            h = (h ^ o.task as u64).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h
    }
}

fn built_hub(n: u16) -> IsolationHub {
    let mut hub = IsolationHub::new(16);
    let mut i = 1u16;
    while i <= n {
        if hub.open(i).is_ok() {
            let _ = hub.attach(i, 1000 + i as u64);
        }
        i += 1;
    }
    hub
}

/// 10 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    let mut empty = IsolationHub::new(4);
    if empty.open(0).err() == Some(IsoError::Empty) {
        caught += 1;
    }
    let mut dup_task = IsolationHub::new(4);
    if dup_task.open(1).is_ok() && dup_task.open(1).err() == Some(IsoError::DupTask) {
        caught += 1;
    }
    let mut missing = IsolationHub::new(4);
    if missing.attach(7, 100).err() == Some(IsoError::NotFound) {
        caught += 1;
    }
    let mut not_active = IsolationHub::new(4);
    if not_active.open(3).is_ok() && not_active.resolve(3, 100).err() == Some(IsoError::NotActive) {
        caught += 1;
    }
    let mut closed = IsolationHub::new(4);
    let mut closed_hit = false;
    if closed.open(4).is_ok() && closed.attach(4, 100).is_ok() && closed.close(4).is_ok() {
        closed_hit = closed.attach(4, 101).err() == Some(IsoError::Closed);
    }
    if closed_hit {
        caught += 1;
    }
    let mut leak = IsolationHub::new(4);
    let mut leak_hit = false;
    if leak.open(1).is_ok()
        && leak.open(2).is_ok()
        && leak.attach(1, 10).is_ok()
        && leak.attach(2, 20).is_ok()
    {
        leak_hit = leak.attach(2, 10).err() == Some(IsoError::Leak);
    }
    if leak_hit {
        caught += 1;
    }
    let mut cap = IsolationHub::new(1);
    if cap.open(1).is_ok() && cap.open(2).err() == Some(IsoError::Cap) {
        caught += 1;
    }
    let mut dup_font = IsolationHub::new(4);
    let mut dup_hit = false;
    if dup_font.open(1).is_ok() && dup_font.attach(1, 10).is_ok() {
        dup_hit = dup_font.attach(1, 10).err() == Some(IsoError::DupFont);
    }
    if dup_hit {
        caught += 1;
    }
    let mut unknown = IsolationHub::new(4);
    let mut unknown_hit = false;
    if unknown.open(1).is_ok() {
        unknown_hit = unknown.attach(1, 0).err() == Some(IsoError::UnknownFont);
    }
    if unknown_hit {
        caught += 1;
    }
    let mut state = IsolationHub::new(4);
    let mut state_hit = false;
    if state.open(5).is_ok() {
        state.owners.push(Ownership {
            font: 42,
            task: 5,
        });
        state_hit = state.audit().err() == Some(IsoError::State);
    }
    if state_hit {
        caught += 1;
    }
    (caught, total)
}

/// 运行 UNX-F50096 的 16 条判据。
pub fn run_unx50096_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50096");
    let mut hub = built_hub(10);

    let ok = hub.session_count() == 10 && hub.count() == 10 && hub.audit().is_ok();
    let det = format!("sessions={} owners={}", hub.session_count(), hub.count());
    s.add_dyn("50096-J1-十字会话建立", ok, &det);

    let mut own_all = true;
    let mut i = 1u16;
    while i <= 10 {
        if hub.resolve(i, 1000 + i as u64) != Ok(true) {
            own_all = false;
        }
        i += 1;
    }
    s.add("50096-J1-自解析全部命中", own_all, if own_all { "" } else { "自解析缺失" });

    let mut cross_leaks = 0usize;
    let mut pairs = 0usize;
    let mut i = 1u16;
    while i <= 10 {
        let mut j = 1u16;
        while j <= 10 {
            if i != j {
                pairs += 1;
                if hub.resolve(i, 1000 + j as u64).err() == Some(IsoError::Leak) {
                    cross_leaks += 1;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let ok = cross_leaks == pairs && pairs == 90;
    let det = format!("leaks={}/{}", cross_leaks, pairs);
    s.add_dyn("50096-J1-交叉零泄漏", ok, &det);

    let mut absorbed = 0usize;
    let mut i = 1u16;
    while i <= 10 {
        if hub.resolve(i, 1000 + i as u64) == Ok(true) {
            absorbed += 1;
        }
        i += 1;
    }
    let ok = absorbed == 10;
    let det = format!("absorbed={}", absorbed);
    s.add_dyn("50096-J1-隔离十次断言", ok, &det);

    let closed = hub.close(5);
    let ok = closed == Ok(1) && hub.count() == 9 && hub.session(5).map(|x| x.state) == Ok(SessionState::Closed);
    let det = format!("closed={:?} left={}", closed, hub.count());
    s.add_dyn("50096-J1-单任务关闭", ok, &det);

    let mut freed_visible = true;
    let mut i = 1u16;
    while i <= 10 {
        if i != 5 {
            if hub.resolve(i, 1005) != Ok(false) {
                freed_visible = false;
            }
        }
        i += 1;
    }
    let ok = freed_visible && hub.audit().is_ok();
    s.add("50096-J1-释放后不可见", ok, if ok { "" } else { "释放字体仍可见" });

    let mut all_closed = true;
    let mut i = 1u16;
    while i <= 10 {
        if i != 5 {
            if hub.close(i).is_err() {
                all_closed = false;
            }
        }
        i += 1;
    }
    let ok = all_closed && hub.count() == 0 && hub.audit().is_ok();
    let det = format!("owners={}", hub.count());
    s.add_dyn("50096-J1-全关归零", ok, &det);

    let ok = hub.attach(5, 2005).err() == Some(IsoError::Closed)
        && hub.resolve(5, 2005).err() == Some(IsoError::Closed);
    s.add("50096-J1-关闭后拒操作", ok, if ok { "" } else { "关闭后仍可操作" });

    let mut dup_task = IsolationHub::new(4);
    let ok = dup_task.open(1).is_ok() && dup_task.open(1).err() == Some(IsoError::DupTask);
    s.add("50096-J1-重复会话必拒", ok, if ok { "" } else { "重复会话放行" });

    let mut dup_font = IsolationHub::new(4);
    let ok = dup_font.open(1).is_ok()
        && dup_font.attach(1, 10).is_ok()
        && dup_font.attach(1, 10).err() == Some(IsoError::DupFont);
    s.add("50096-J1-重复挂载必拒", ok, if ok { "" } else { "重复挂载放行" });

    let mut edge = IsolationHub::new(4);
    let ok = edge.open(0).err() == Some(IsoError::Empty);
    let ok = ok && edge.open(1).is_ok() && edge.attach(1, 0).err() == Some(IsoError::UnknownFont);
    s.add("50096-J1-零值边界", ok, if ok { "" } else { "零值未拒" });

    let mut cap = IsolationHub::new(1);
    let ok = cap.open(1).is_ok() && cap.open(2).err() == Some(IsoError::Cap);
    s.add("50096-J1-容量必拒", ok, if ok { "" } else { "容量未拒" });

    let mut idle = IsolationHub::new(4);
    let ok = idle.open(1).is_ok() && idle.resolve(1, 10).err() == Some(IsoError::NotActive);
    s.add("50096-J1-未挂载先查", ok, if ok { "" } else { "Open 会话被查询" });

    let mut reuse = IsolationHub::new(4);
    let mut flow = reuse.open(1).is_ok()
        && reuse.open(2).is_ok()
        && reuse.attach(1, 77).is_ok()
        && reuse.attach(2, 88).is_ok();
    if flow {
        flow = reuse.resolve(2, 77).err() == Some(IsoError::Leak) && reuse.close(1).is_ok();
    }
    let ok = flow
        && reuse.attach(2, 77).is_ok()
        && reuse.resolve(2, 77) == Ok(true)
        && reuse.resolve(2, 88) == Ok(true);
    s.add("50096-J1-关闭后可复用", ok, if ok { "" } else { "字体残留不可复用" });

    let ok = hub.session(99).err() == Some(IsoError::NotFound)
        && hub.attach(99, 1).err() == Some(IsoError::NotFound);
    s.add("50096-J1-缺会话必拒", ok, if ok { "" } else { "缺会话放行" });

    let a = built_hub(6);
    let b = built_hub(6);
    let c = built_hub(5);
    let ok = a.snapshot() == b.snapshot() && a.snapshot() != c.snapshot();
    s.add("50096-J1-快照确定性", ok, if ok { "" } else { "快照不稳定" });

    let mut broken = IsolationHub::new(4);
    let ok = broken.open(1).is_ok();
    broken.owners.push(Ownership { font: 9, task: 1 });
    let ok = ok && broken.audit().err() == Some(IsoError::State);
    s.add("50096-J1-审计可检漂移", ok, if ok { "" } else { "漂移漏检" });

    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50096-J1-注入必抓", ok, &det);

    let all = [
        IsoError::Empty,
        IsoError::DupTask,
        IsoError::NotFound,
        IsoError::NotActive,
        IsoError::Closed,
        IsoError::Leak,
        IsoError::Cap,
        IsoError::DupFont,
        IsoError::UnknownFont,
        IsoError::State,
    ];
    let mut unique = true;
    let mut i = 0usize;
    while let Some(a) = all.get(i) {
        if a.code() < SEG_BASE + 1 || a.code() > SEG_BASE + 10 {
            unique = false;
        }
        let mut j = i + 1;
        while let Some(b) = all.get(j) {
            if a.code() == b.code() {
                unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = unique;
    s.add("50096-J1-码闭集", ok, if ok { "" } else { "码值越界或重复" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50096_checks_green() {
        let set = run_unx50096_checks();
        assert!(set.all_passed(), "UNX-F50096 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50096_ten_way_isolation() {
        let hub = built_hub(10);
        let mut i = 1u16;
        while i <= 10 {
            let mut j = 1u16;
            while j <= 10 {
                if i != j {
                    assert_eq!(hub.resolve(i, 1000 + j as u64), Err(IsoError::Leak));
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(hub.count(), 10);
    }
}
