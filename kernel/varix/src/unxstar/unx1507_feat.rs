//! UNX-F1507 回归结果账（绿/黄/红三态与滚动） · 0x78040段
//!
//! 判据原文（UNX-F1507-J1 摘录）：「回归结果账生效 100%（逐场景三态：
//! 绿=全过/黄=超时过/红=失败，三态枚举闭合），结果与执行器留痕对拍一致率
//! 100%，结果篡改注入 10/10 必抓，滚动汇总随批落档」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三态枚举闭合：绿（全过且未超预算）/黄（通过但超预算）/红（失败），
//!    枚举外的任何取值在登记口即被拒；
//! 2. 执行器留痕独立入账：登记时按留痕（状态字+耗时+预算）复导出期望三态，
//!    与登记态不符即对拍拒收——结果不能自说自话；
//! 3. 篡改注入 10/10：逐场景替换登记态，篡改必被对拍门抓住，十路全数命中；
//! 4. 滚动汇总随批落档：批封账生成汇总（绿/黄/红计数+折叠根）只追加不
//!    改写，红未清禁止封口；
//! 5. 状态机 采集中→已封口 单向，倒退显性拒；诊断码闭集 0x78041..0x7804C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78040;

/// 三态结果账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 三态取值非法。
    EState,
    /// 场景编号重复。
    EDup,
    /// 缺执行器留痕。
    ENoTrace,
    /// 结果与留痕对拍不符。
    EMismatch,
    /// 登记乱序。
    EOrder,
    /// 批已封口。
    ESealed,
    /// 容量已满。
    ECap,
    /// 空场景样本。
    EEmpty,
    /// 结果篡改检出。
    ETamper,
    /// 滚动汇总缺失。
    ENoRoll,
    /// 红账未清禁止封口。
    ERedBlock,
    /// 封口重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EState => 0x01,
                Diag::EDup => 0x02,
                Diag::ENoTrace => 0x03,
                Diag::EMismatch => 0x04,
                Diag::EOrder => 0x05,
                Diag::ESealed => 0x06,
                Diag::ECap => 0x07,
                Diag::EEmpty => 0x08,
                Diag::ETamper => 0x09,
                Diag::ENoRoll => 0x0a,
                Diag::ERedBlock => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EState => "三态取值非法：只认绿/黄/红，其余取值一律拒收",
            Diag::EDup => "场景编号重复：每场景只落一态，先查重再入账",
            Diag::ENoTrace => "缺执行器留痕：结果必须挂留痕，先补跑再登记",
            Diag::EMismatch => "对拍不符：登记态与留痕复导出态不一致，以留痕为准",
            Diag::EOrder => "登记乱序：按场景序 0,1,2.. 顺序补齐，禁止跳格",
            Diag::ESealed => "批已封口：封口后写入拒绝，改动走下一批滚动",
            Diag::ECap => "容量已满：本批场景已齐，多余场景滚动进下一批",
            Diag::EEmpty => "空场景样本：编号与留痕全零不构成结果",
            Diag::ETamper => "篡改检出：登记态被改，须整批重跑复核",
            Diag::ENoRoll => "滚动汇总缺失：封口前必须落汇总行",
            Diag::ERedBlock => "红账未清：存在红态场景，先修复再封口",
            Diag::ESeal => "封口重放不符：汇总复算与票面不一致，账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EState,
    Diag::EDup,
    Diag::ENoTrace,
    Diag::EMismatch,
    Diag::EOrder,
    Diag::ESealed,
    Diag::ECap,
    Diag::EEmpty,
    Diag::ETamper,
    Diag::ENoRoll,
    Diag::ERedBlock,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78041..0x7804C 闭集内"
}

/// 三态：绿=全过。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriState {
    /// 绿：全过且未超预算。
    Green,
    /// 黄：通过但超时（超预算）。
    Yellow,
    /// 红：失败。
    Red,
}

/// 三态闭集数组。
pub const TRI_ALL: &[TriState] = &[TriState::Green, TriState::Yellow, TriState::Red];

impl TriState {
    /// 态序号（0/1/2，入折叠）。
    pub fn ord(self) -> u64 {
        match self {
            TriState::Green => 0,
            TriState::Yellow => 1,
            TriState::Red => 2,
        }
    }

    /// 中文态名。
    pub fn name(self) -> &'static str {
        match self {
            TriState::Green => "绿",
            TriState::Yellow => "黄",
            TriState::Red => "红",
        }
    }
}

/// 执行器留痕状态字：执行通过。
pub const EXEC_OK: u8 = 0;
/// 执行器留痕状态字：执行失败。
pub const EXEC_FAIL: u8 = 1;

/// 一条执行器留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    /// 场景编号。
    pub sid: u16,
    /// 执行器状态字（0=过，1=败）。
    pub exec_status: u8,
    /// 耗时微秒。
    pub usec: u32,
    /// 预算微秒。
    pub budget: u32,
}

impl TraceEntry {
    /// 按留痕复导出期望三态（对拍的权威侧）。
    pub fn expect_state(&self) -> Option<TriState> {
        if self.exec_status == EXEC_FAIL {
            return Some(TriState::Red);
        }
        if self.exec_status != EXEC_OK {
            return None;
        }
        if self.usec > self.budget {
            return Some(TriState::Yellow);
        }
        Some(TriState::Green)
    }
}

/// 一条场景结果登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioResult {
    /// 场景编号（1..=10）。
    pub sid: u16,
    /// 登记三态。
    pub state: TriState,
}

impl ScenarioResult {
    /// 空样本判定（编号零即空）。
    pub fn is_blank(&self) -> bool {
        self.sid == 0
    }
}

/// FNV 折叠辅助。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 本批场景数。
pub const SCENARIOS: usize = 10;

/// 批生命周期：采集中 → 已封口（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 采集中。
    Collecting,
    /// 已封口。
    Closed,
}

/// 回归结果账（一批十场景）。
pub struct ResultLedger {
    results: Vec<ScenarioResult>,
    traces: Vec<TraceEntry>,
    phase: Phase,
    root: u64,
    rejects: u32,
    /// 滚动汇总（随批只追加）。
    rolling: Vec<String>,
}

impl ResultLedger {
    /// 空账。
    pub fn new() -> ResultLedger {
        ResultLedger {
            results: Vec::new(),
            traces: Vec::new(),
            phase: Phase::Collecting,
            root: 0,
            rejects: 0,
            rolling: Vec::new(),
        }
    }

    /// 挂执行器留痕（每场景一条，按序）。
    pub fn attach_trace(&mut self, t: TraceEntry) -> Result<(), u32> {
        if self.phase == Phase::Closed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if t.sid == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if t.sid as usize != self.traces.len() + 1 {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        self.traces.push(t);
        Ok(())
    }

    /// 登记一条结果（对拍不符即拒）。
    pub fn register(&mut self, sid: u16, state: TriState) -> Result<TriState, u32> {
        if self.phase == Phase::Closed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.results.len() >= SCENARIOS {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if sid == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        for r in &self.results {
            if r.sid == sid {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        if sid as usize != self.results.len() + 1 {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        let t = match self.traces.get((sid - 1) as usize) {
            Some(x) => *x,
            None => {
                self.rejects += 1;
                return Err(Diag::ENoTrace.val());
            }
        };
        if t.sid != sid {
            self.rejects += 1;
            return Err(Diag::ENoTrace.val());
        }
        let expect = match t.expect_state() {
            Some(x) => x,
            None => {
                self.rejects += 1;
                return Err(Diag::EState.val());
            }
        };
        if state != expect {
            self.rejects += 1;
            return Err(Diag::EMismatch.val());
        }
        self.results.push(ScenarioResult { sid, state });
        Ok(state)
    }

    /// 已登记场景数。
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// 取登记态。
    pub fn state_at(&self, i: usize) -> Option<TriState> {
        self.results.get(i).map(|r| r.state)
    }

    /// 取留痕。
    pub fn trace_at(&self, i: usize) -> Option<TraceEntry> {
        self.traces.get(i).copied()
    }

    /// 当前相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 滚动汇总行数。
    pub fn rolling_len(&self) -> usize {
        self.rolling.len()
    }

    /// 取第 k 行滚动汇总。
    pub fn rolling_at(&self, k: usize) -> Option<String> {
        self.rolling.get(k).cloned()
    }

    /// 某态计数。
    pub fn count_state(&self, st: TriState) -> u32 {
        let mut n = 0u32;
        for r in &self.results {
            if r.state == st {
                n += 1;
            }
        }
        n
    }

    /// 对拍一致率（千分比）：逐场景登记态 == 留痕复导出态。
    pub fn agree_permille(&self) -> u32 {
        if self.results.is_empty() {
            return 0;
        }
        let mut ok = 0u32;
        for r in &self.results {
            let t = match self.traces.get((r.sid - 1) as usize) {
                Some(x) => *x,
                None => continue,
            };
            if t.expect_state() == Some(r.state) {
                ok += 1;
            }
        }
        (ok * 1000) / self.results.len() as u32
    }

    /// 本批折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.results.len() as u64);
        for r in &self.results {
            h = fold_word(h, r.sid as u64);
            h = fold_word(h, r.state.ord());
        }
        h = fold_word(h, self.rolling.len() as u64);
        h
    }

    /// 批绿：十场景齐 + 全绿（零黄零红）。
    pub fn batch_green(&self) -> bool {
        self.results.len() == SCENARIOS
            && self.count_state(TriState::Red) == 0
            && self.count_state(TriState::Yellow) == 0
    }

    /// 封口：先落滚动汇总再转相位；红未清拒绝；重复封口拒绝。
    pub fn close(&mut self) -> Result<String, u32> {
        if self.phase == Phase::Closed {
            return Err(Diag::ESealed.val());
        }
        if self.count_state(TriState::Red) > 0 {
            return Err(Diag::ERedBlock.val());
        }
        if self.results.is_empty() {
            return Err(Diag::ENoRoll.val());
        }
        let sum = format!(
            "roll g={} y={} r={} root={:016x}",
            self.count_state(TriState::Green),
            self.count_state(TriState::Yellow),
            self.count_state(TriState::Red),
            self.fold(),
        );
        self.rolling.push(sum.clone());
        self.phase = Phase::Closed;
        self.root = self.fold();
        Ok(sum)
    }

    /// 封口根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 封口后重放：逐场景复对拍。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Closed {
            return Err(Diag::ESeal.val());
        }
        let mut ok = 0u32;
        for r in &self.results {
            let t = match self.traces.get((r.sid - 1) as usize) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if t.expect_state() != Some(r.state) {
                return Err(Diag::ETamper.val());
            }
            ok += 1;
        }
        if self.root != self.fold() {
            return Err(Diag::ESeal.val());
        }
        Ok(ok)
    }
}

/// 滚动汇总拼接（多批落档面）。
pub fn rolling_text(l: &ResultLedger) -> String {
    let mut out = String::new();
    let mut k = 0usize;
    while k < l.rolling_len() {
        if let Some(line) = l.rolling_at(k) {
            out.push_str(&line);
            out.push('\n');
        }
        k += 1;
    }
    out
}

/// 标准批：8 绿 + 2 黄（超时过）+ 0 红。
pub fn build_batch() -> ResultLedger {
    let mut l = ResultLedger::new();
    for sid in 1..=10u16 {
        // 场景 9、10 超预算走黄，其余绿。
        let (status, usec, budget) = if sid == 9 {
            (EXEC_OK, 1500u32, 1000u32)
        } else if sid == 10 {
            (EXEC_OK, 2200u32, 1000u32)
        } else {
            (EXEC_OK, 300u32 + sid as u32 * 40, 1000u32)
        };
        let _ = l.attach_trace(TraceEntry { sid, exec_status: status, usec, budget });
    }
    let mut sid = 1u16;
    while sid <= 10 {
        let st = if sid == 9 || sid == 10 {
            TriState::Yellow
        } else {
            TriState::Green
        };
        let _ = l.register(sid, st);
        sid += 1;
    }
    l
}

/// 篡改注入 10/10：逐场景把登记态改成与留痕矛盾的取值，必须全数被抓。
pub fn tamper_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let mut i = 0usize;
    while i < SCENARIOS {
        let mut l = build_batch();
        // 撤销已登记的第 i 场景：以整账重建后改写——直接构造矛盾登记。
        // 重建：先取基线留痕，再以错误态登记（对拍门必须拒绝）。
        let mut probe = ResultLedger::new();
        let mut sid = 1u16;
        while sid <= 10 {
            if let Some(t) = build_batch().trace_at((sid - 1) as usize) {
                let _ = probe.attach_trace(t);
            }
            sid += 1;
        }
        let target = (i + 1) as u16;
        // 篡改：把绿场景登记成红（或黄登记成绿），对拍必须拒。
        let wrong = match build_batch().state_at(i) {
            Some(TriState::Green) => TriState::Red,
            Some(TriState::Yellow) => TriState::Green,
            _ => TriState::Red,
        };
        // 场景按序登记，目标之前的照常，目标处登记篡改态。
        let mut ok_seq = true;
        let mut sid2 = 1u16;
        while sid2 <= 10 {
            if sid2 == target {
                let r = probe.register(sid2, wrong);
                if r != Err(Diag::EMismatch.val()) {
                    ok_seq = false;
                }
                break;
            }
            let st = match build_batch().state_at((sid2 - 1) as usize) {
                Some(x) => x,
                None => TriState::Red,
            };
            if probe.register(sid2, st) == Err(Diag::EMismatch.val()) {
                ok_seq = false;
            }
            sid2 += 1;
        }
        // 已封口的账再登记也必须拒（第二道门）。
        let mut sealed = build_batch();
        let _ = sealed.close();
        let sealed_reject = sealed.register(1, TriState::Red) == Err(Diag::ESealed.val());
        out[i] = ok_seq && sealed_reject;
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x78040
}

/// 三态枚举闭合。
fn tri_closed_ok() -> bool {
    if TRI_ALL.len() != 3 {
        return false;
    }
    let mut i = 0;
    while i < 3 {
        let a = match TRI_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < 3 {
            let b = match TRI_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b || a.ord() == b.ord() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 结果账生效：十场景齐、对拍一致率 1000‰。
fn ledger_ok(l: &ResultLedger) -> bool {
    if l.len() != SCENARIOS {
        return false;
    }
    l.agree_permille() == 1000
}

/// 篡改注入 10/10 必抓。
fn tamper_ok() -> bool {
    tamper_matrix() == [true; 10]
}

/// 滚动汇总随批落档。
fn rolling_ok(l: &ResultLedger) -> bool {
    if l.rolling_len() != 1 {
        return false;
    }
    let t = rolling_text(l);
    if t.is_empty() {
        return false;
    }
    t.contains("roll g=8 y=2 r=0")
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_batch();
    let b = build_batch();
    a.fold() == b.fold() && a.fold() != 0
}

/// 十三项判据。
pub fn run_unx1507_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1507");
    let mut l = build_batch();

    s.add_dyn(
        "F1507-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1507-02 三态枚举闭合",
        tri_closed_ok(),
        &format!(
            "n={} names={}{}/{}",
            TRI_ALL.len(),
            TriState::Green.name(),
            TriState::Yellow.name(),
            TriState::Red.name()
        ),
    );

    s.add_dyn(
        "F1507-03 结果账十场景齐备",
        ledger_ok(&l) && !l.is_empty(),
        &format!("scen={} agree={}", l.len(), l.agree_permille()),
    );

    s.add_dyn(
        "F1507-04 三态分布绿八黄二",
        l.count_state(TriState::Green) == 8
            && l.count_state(TriState::Yellow) == 2
            && l.count_state(TriState::Red) == 0,
        &format!(
            "g={} y={} r={}",
            l.count_state(TriState::Green),
            l.count_state(TriState::Yellow),
            l.count_state(TriState::Red)
        ),
    );

    // 对拍门：登记态与留痕复导出态矛盾必须被拒（三向各用独立账本，
    // 被拒格不入账，后续格继续按序登记）。
    let mut a = ResultLedger::new();
    let _ = a.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 100, budget: 1000 });
    let good = a.register(1, TriState::Green) == Ok(TriState::Green);
    let mut r1 = ResultLedger::new();
    let _ = r1.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_FAIL, usec: 100, budget: 1000 });
    let red_as_green = r1.register(1, TriState::Green) == Err(Diag::EMismatch.val());
    let mut r2 = ResultLedger::new();
    let _ = r2.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 5000, budget: 1000 });
    let slow_as_green = r2.register(1, TriState::Green) == Err(Diag::EMismatch.val());
    let total_rejects = a.rejects() + r1.rejects() + r2.rejects();
    s.add_dyn(
        "F1507-05 对拍门三向拦截",
        good && red_as_green && slow_as_green && total_rejects == 2,
        &format!("rejects={}", total_rejects),
    );

    // 结构拒绝：乱序/重复/缺留痕/空样本。
    let mut r = ResultLedger::new();
    let _ = r.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 100, budget: 1000 });
    let _ = r.register(1, TriState::Green);
    let order = r.register(3, TriState::Green) == Err(Diag::EOrder.val());
    let dup = r.register(1, TriState::Green) == Err(Diag::EDup.val());
    let notrace = r.register(2, TriState::Green) == Err(Diag::ENoTrace.val());
    let empty = r.register(0, TriState::Green) == Err(Diag::EEmpty.val());
    s.add_dyn(
        "F1507-06 结构拒绝四门",
        order && dup && notrace && empty && r.rejects() == 4 && r.len() == 1,
        &format!("rejects={} len={}", r.rejects(), r.len()),
    );

    s.add_dyn(
        "F1507-07 篡改注入十路必抓",
        tamper_ok(),
        &format!("tamper10={:?}", tamper_matrix()),
    );

    // 红未清禁止封口。
    let mut red = ResultLedger::new();
    let _ = red.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_FAIL, usec: 10, budget: 1000 });
    let _ = red.register(1, TriState::Red);
    s.add_dyn(
        "F1507-08 红账未清封口被拒",
        red.close() == Err(Diag::ERedBlock.val()) && red.phase() == Phase::Collecting,
        &format!("close={:?}", red.close()),
    );

    // 正常封口：汇总落档、相位单向、重复封口拒绝。
    let sum = l.close();
    let sum_ok = match sum {
        Ok(t) => t.contains("g=8") && t.contains("y=2") && t.contains("r=0"),
        Err(_) => false,
    };
    let re_close = l.close() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1507-09 封口汇总落档单向",
        sum_ok && re_close && l.phase() == Phase::Closed && l.rolling_len() == 1,
        &rolling_text(&l),
    );

    s.add_dyn(
        "F1507-10 封口后写入拒绝",
        l.register(11, TriState::Green) == Err(Diag::ESealed.val())
            && l.attach_trace(TraceEntry { sid: 11, exec_status: EXEC_OK, usec: 1, budget: 1 })
                == Err(Diag::ESealed.val()),
        &format!("rejects={}", l.rejects()),
    );

    s.add_dyn(
        "F1507-11 封口重放逐场景一致",
        l.replay() == Ok(SCENARIOS as u32) && l.root() != 0,
        &format!("replay={:?}", l.replay()),
    );

    s.add_dyn(
        "F1507-12 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", build_batch().fold()),
    );

    // 黄态语义：超时过不算绿，不得计入全绿判据。
    let yellow_ok = {
        let t = TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 900, budget: 800 };
        t.expect_state() == Some(TriState::Yellow)
            && t.expect_state() != Some(TriState::Green)
    };
    s.add_dyn(
        "F1507-13 超时过判黄不入绿",
        yellow_ok && !l.batch_green() && rolling_ok(&l),
        &format!("green={} rolling={}", l.batch_green(), l.rolling_len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_state_closed() {
        assert_eq!(TRI_ALL.len(), 3);
        assert_ne!(TriState::Green, TriState::Yellow);
        assert_ne!(TriState::Yellow, TriState::Red);
        assert_eq!(TriState::Green.ord(), 0);
        assert_eq!(TriState::Red.ord(), 2);
    }

    #[test]
    fn expect_state_mapping() {
        let g = TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 100, budget: 200 };
        let y = TraceEntry { sid: 2, exec_status: EXEC_OK, usec: 300, budget: 200 };
        let r = TraceEntry { sid: 3, exec_status: EXEC_FAIL, usec: 10, budget: 200 };
        assert_eq!(g.expect_state(), Some(TriState::Green));
        assert_eq!(y.expect_state(), Some(TriState::Yellow));
        assert_eq!(r.expect_state(), Some(TriState::Red));
    }

    #[test]
    fn batch_distribution_and_agree() {
        let l = build_batch();
        assert_eq!(l.len(), 10);
        assert_eq!(l.count_state(TriState::Green), 8);
        assert_eq!(l.count_state(TriState::Yellow), 2);
        assert_eq!(l.agree_permille(), 1000);
        // 黄态不算全绿：批有超时过场景，batch_green 必须为假。
        assert!(!l.batch_green());
    }

    #[test]
    fn mismatch_rejected() {
        let mut l = ResultLedger::new();
        let _ = l.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_FAIL, usec: 10, budget: 100 });
        assert_eq!(l.register(1, TriState::Green), Err(Diag::EMismatch.val()));
        assert_eq!(l.register(1, TriState::Red), Ok(TriState::Red));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn gate_three_way_block() {
        let mut a = ResultLedger::new();
        let _ = a.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 100, budget: 1000 });
        assert_eq!(a.register(1, TriState::Green), Ok(TriState::Green));
        let mut r1 = ResultLedger::new();
        let _ = r1.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_FAIL, usec: 100, budget: 1000 });
        assert_eq!(r1.register(1, TriState::Green), Err(Diag::EMismatch.val()));
        assert_eq!(r1.register(1, TriState::Red), Ok(TriState::Red));
        let mut r2 = ResultLedger::new();
        let _ = r2.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 5000, budget: 1000 });
        assert_eq!(r2.register(1, TriState::Green), Err(Diag::EMismatch.val()));
        assert_eq!(r2.register(1, TriState::Yellow), Ok(TriState::Yellow));
        assert_eq!(a.rejects() + r1.rejects() + r2.rejects(), 2);
    }

    #[test]
    fn order_dup_trace_gates() {
        let mut l = ResultLedger::new();
        assert_eq!(
            l.register(1, TriState::Green),
            Err(Diag::ENoTrace.val()),
            "未挂留痕先拒"
        );
        let _ = l.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 10, budget: 100 });
        assert_eq!(l.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_OK, usec: 10, budget: 100 }), Err(Diag::EOrder.val()));
        assert_eq!(l.register(2, TriState::Green), Err(Diag::EOrder.val()));
        let _ = l.register(1, TriState::Green);
        assert_eq!(l.register(1, TriState::Green), Err(Diag::EDup.val()));
        assert_eq!(l.register(0, TriState::Green), Err(Diag::EEmpty.val()));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn close_gates_and_rolling() {
        let mut red = ResultLedger::new();
        let _ = red.attach_trace(TraceEntry { sid: 1, exec_status: EXEC_FAIL, usec: 10, budget: 100 });
        let _ = red.register(1, TriState::Red);
        assert_eq!(red.close(), Err(Diag::ERedBlock.val()));

        let mut l = build_batch();
        let s = l.close();
        assert!(s.is_ok());
        assert_eq!(l.rolling_len(), 1);
        assert!(rolling_text(&l).contains("g=8 y=2 r=0"));
        assert_eq!(l.close(), Err(Diag::ESealed.val()));
        assert_eq!(l.phase(), Phase::Closed);
    }

    #[test]
    fn tamper_ten_of_ten() {
        let m = tamper_matrix();
        assert_eq!(m, [true; 10]);
        assert!(tamper_ok());
    }

    #[test]
    fn replay_after_close() {
        let mut l = build_batch();
        let _ = l.close();
        assert_eq!(l.replay(), Ok(10));
        assert_ne!(l.root(), 0);
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EState.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x20), "未知诊断码：不在 0x78041..0x7804C 闭集内");
    }

    #[test]
    fn checks_all_green_thirteen() {
        let s = run_unx1507_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
