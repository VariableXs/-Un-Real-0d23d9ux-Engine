//! UNX-F3409 断言失败不中断模式（continue 模式与严格模式） · 0x79580段
//!
//! 判据原文（UNX-F3409 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：两模式（continue=记录续跑/strict=首败即停）语义明确；M-168 两模式行为对照。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 两模式闭集：continue=失败记录后继续跑完全程（失败册全收），
//!    strict=首败即停（停点=首败位，余步显性计未执行）——语义在报告
//!    字段上一锤定音，无第三态；
//! 2. 行为对照 M-168：同语料双模式各跑一遍，对照矩阵五面（无败同账/
//!    有败 strict 停点=首败/strict 未执行数=余步/continue 失败册=全败位/
//!    strict 执行数 ≤ continue 执行数）逐面对账；
//! 3. 执行账面：每跑一遍落一份运行报告（执行数/停点/失败册/未执行数），
//!    同语料重跑报告必须逐字段一致（确定性判据）；
//! 4. 脚本封口：起跑前脚本封口（封口后加步显性拒），空脚本显性拒——
//!    防空跑与半截脚本混入对照面；
//! 5. 注入矩阵 8 项：空脚本/越界执行/strict 有败未停/continue 丢败/
//!    停点漂移/未执行计数错/无败对照破/封口后加步逐种构造，逐种必抓；
//! 6. 诊断码闭集 0x79581..0x7958C，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x79580;

/// M-168 对照语料档数。
pub const M168_CASES: usize = 4;

/// 两模式诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 空脚本：无步可跑。
    EEmptyScript,
    /// 越界执行：执行数超总步数。
    EExecOver,
    /// strict 语义破：有败未停在首败位。
    EStrictBroken,
    /// continue 丢败：失败册不满全败位。
    EContinueLost,
    /// 停点漂移：停点 ≠ 首败位。
    EStopMismatch,
    /// 未执行计数错：未执行数 ≠ 总步数-执行数。
    EUnexecMismatch,
    /// 无败对照破：无败语料两模式必须同账。
    ENoFailMismatch,
    /// 取步越位：步下标越出脚本。
    ERange,
    /// 步名重复：脚本内步名互异。
    EDupName,
    /// 步名空：步名必填。
    EEmptyName,
    /// 对照账序断裂：对照账序号不连续。
    ESeq,
    /// 相位倒退：装配→封口→已跑 单向（封口后加步、未封口起跑均拒）。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EEmptyScript => 0x01,
                Diag::EExecOver => 0x02,
                Diag::EStrictBroken => 0x03,
                Diag::EContinueLost => 0x04,
                Diag::EStopMismatch => 0x05,
                Diag::EUnexecMismatch => 0x06,
                Diag::ENoFailMismatch => 0x07,
                Diag::ERange => 0x08,
                Diag::EDupName => 0x09,
                Diag::EEmptyName => 0x0a,
                Diag::ESeq => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EEmptyScript => "空脚本：无步可跑，先加步再起跑",
            Diag::EExecOver => "越界执行：执行数超总步数，运行器内部违约，回报修者",
            Diag::EStrictBroken => "strict 语义破：首败后未停，检查运行器停点逻辑",
            Diag::EContinueLost => "continue 丢败：失败册不满全败位，逐败位对账补册",
            Diag::EStopMismatch => "停点漂移：停点≠首败位，重跑对照定位停点逻辑",
            Diag::EUnexecMismatch => "未执行计数错：未执行数≠总步数-执行数，重算报告",
            Diag::ENoFailMismatch => "无败对照破：无败语料两模式同账，检查模式分派",
            Diag::ERange => "取步越位：下标越出脚本，按脚本长度取步",
            Diag::EDupName => "步名重复：脚本内步名互异，换名再加步",
            Diag::EEmptyName => "步名空：步名必填，回填步名再加步",
            Diag::ESeq => "对照账序断裂：对照账序号须从一起连续递增，重建对照账",
            Diag::EPhase => "相位倒退：装配→封口→已跑 单向，封口后加步或未封口起跑均拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EEmptyScript,
    Diag::EExecOver,
    Diag::EStrictBroken,
    Diag::EContinueLost,
    Diag::EStopMismatch,
    Diag::EUnexecMismatch,
    Diag::ENoFailMismatch,
    Diag::ERange,
    Diag::EDupName,
    Diag::EEmptyName,
    Diag::ESeq,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79581..0x7958C 闭集内"
}

/// 运行模式闭集（两模式语义在报告字段上一锤定音）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 记录续跑：失败记录后继续跑完全程。
    Continue,
    /// 首败即停：首败位停，余步计未执行。
    Strict,
}

impl Mode {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Mode::Continue => "续跑",
            Mode::Strict => "首败即停",
        }
    }

    /// 序数（对照账序）。
    pub fn ord(self) -> u8 {
        match self {
            Mode::Continue => 0,
            Mode::Strict => 1,
        }
    }
}

/// 脚本相位（单向：装配→封口→已跑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 装配：可加步。
    Building,
    /// 封口：只读，可起跑。
    Sealed,
    /// 已跑：报告落定。
    Ran,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Building => 0,
            Phase::Sealed => 1,
            Phase::Ran => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Building => "装配",
            Phase::Sealed => "封口",
            Phase::Ran => "已跑",
        }
    }
}

/// 一条脚本步（步名 + 钦定判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// 步名（脚本内互异）。
    pub name: &'static str,
    /// 钦定判定（true=过）。
    pub will_pass: bool,
}

/// 运行报告（执行数/停点/失败册/未执行数——两模式语义载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// 运行模式。
    pub mode: Mode,
    /// 实际执行步数。
    pub executed: usize,
    /// 停点位（continue 恒 None；strict 有败=首败位，无败=None）。
    pub stopped_at: Option<usize>,
    /// 失败步位册（continue=全败位；strict=恰首败位）。
    pub failures: Vec<usize>,
    /// 未执行步数（总步数-执行数）。
    pub unexecuted: usize,
}

impl RunReport {
    /// 报告语义自检（对一册脚本五面逐面核——语义一锤定音处）。
    pub fn semantics_ok(&self, script: &[Step]) -> bool {
        let n = script.len();
        // 越界执行面。
        if self.executed > n || self.executed == 0 && n > 0 {
            return false;
        }
        // 未执行计数面。
        if self.unexecuted != n - self.executed {
            return false;
        }
        // 失败册面：册内位次必须真是败步。
        for &pos in &self.failures {
            match script.get(pos) {
                Some(s) => {
                    if s.will_pass {
                        return false;
                    }
                }
                None => return false,
            }
        }
        match self.mode {
            Mode::Continue => {
                // 续跑语义：跑满全程、无停点、失败册=全败位。
                if self.executed != n || self.stopped_at.is_some() {
                    return false;
                }
                let mut want = Vec::new();
                let mut i = 0usize;
                while i < n {
                    match script.get(i) {
                        Some(s) => {
                            if !s.will_pass {
                                want.push(i);
                            }
                        }
                        None => return false,
                    }
                    i += 1;
                }
                self.failures == want
            }
            Mode::Strict => {
                // 首败即停语义：无败跑满；有败停在首败位、失败册恰一位。
                let first_fail = {
                    let mut f = None;
                    let mut i = 0usize;
                    while i < n {
                        match script.get(i) {
                            Some(s) => {
                                if !s.will_pass {
                                    f = Some(i);
                                    break;
                                }
                            }
                            None => return false,
                        }
                        i += 1;
                    }
                    f
                };
                match first_fail {
                    None => self.executed == n && self.stopped_at.is_none() && self.failures.is_empty(),
                    Some(k) => {
                        self.executed == k + 1
                            && self.stopped_at == Some(k)
                            && self.failures == vec![k]
                    }
                }
            }
        }
    }

    /// 单行摘要（写回面，非空）。
    pub fn report_line(&self) -> String {
        let stop = match self.stopped_at {
            Some(k) => format!("停{}", k),
            None => String::from("未停"),
        };
        format!(
            "模式{} 执行{} 失败{} {} 未执行{}",
            self.mode.name(),
            self.executed,
            self.failures.len(),
            stop,
            self.unexecuted
        )
    }
}

/// 两模式运行器（脚本 + 模式 + 报告）。
#[derive(Clone, Debug)]
pub struct ModeRunner {
    mode: Mode,
    phase: Phase,
    script: Vec<Step>,
    last: Option<RunReport>,
    rejects: u32,
}

impl ModeRunner {
    /// 新运行器（装配相位）。
    pub fn new(mode: Mode) -> ModeRunner {
        ModeRunner {
            mode,
            phase: Phase::Building,
            script: Vec::new(),
            last: None,
            rejects: 0,
        }
    }

    /// 模式。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 脚本步数。
    pub fn script_len(&self) -> usize {
        self.script.len()
    }

    /// 取步（越位返回 None）。
    pub fn step_at(&self, idx: usize) -> Option<Step> {
        self.script.get(idx).copied()
    }

    /// 上次报告。
    pub fn last_report(&self) -> Option<RunReport> {
        self.last.clone()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 加步（封口后显性拒；步名互异且非空）。
    pub fn add_step(&mut self, name: &'static str, will_pass: bool) -> Result<(), u32> {
        if self.phase != Phase::Building {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if name.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmptyName.val());
        }
        for s in &self.script {
            if s.name == name {
                self.rejects += 1;
                return Err(Diag::EDupName.val());
            }
        }
        self.script.push(Step { name, will_pass });
        Ok(())
    }

    /// 封口（装配→封口，单向；空脚本显性拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Building {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.script.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmptyScript.val());
        }
        self.phase = Phase::Sealed;
        Ok(())
    }

    /// 起跑：按模式语义执行脚本，落运行报告（未封口显性拒）。
    pub fn run(&mut self) -> Result<RunReport, u32> {
        if self.phase != Phase::Sealed && self.phase != Phase::Ran {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.script.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmptyScript.val());
        }
        let n = self.script.len();
        let mut executed = 0usize;
        let mut stopped_at: Option<usize> = None;
        let mut failures: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < n {
            let s = match self.script.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ERange.val()),
            };
            executed += 1;
            if !s.will_pass {
                failures.push(i);
                if self.mode == Mode::Strict {
                    stopped_at = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let report = RunReport {
            mode: self.mode,
            executed,
            stopped_at,
            failures,
            unexecuted: n - executed,
        };
        if !report.semantics_ok(&self.script) {
            self.rejects += 1;
            return Err(match self.mode {
                Mode::Continue => Diag::EContinueLost.val(),
                Mode::Strict => Diag::EStrictBroken.val(),
            });
        }
        self.last = Some(report.clone());
        self.phase = Phase::Ran;
        Ok(report)
    }
}

/// M-168 对照语料档：四档（无败/首步败/中段双败/尾步单败）。
pub struct M168Case {
    /// 档名。
    pub name: &'static str,
    /// 脚本（步名, 钦定判定）。
    pub script: [(&'static str, bool); 5],
}

/// 构造 M-168 四档语料。
pub fn m168_corpus() -> Vec<M168Case> {
    let mut out = Vec::new();
    out.push(M168Case {
        name: "无败",
        script: [
            ("甲", true),
            ("乙", true),
            ("丙", true),
            ("丁", true),
            ("戊", true),
        ],
    });
    out.push(M168Case {
        name: "首步败",
        script: [
            ("甲", false),
            ("乙", true),
            ("丙", true),
            ("丁", true),
            ("戊", true),
        ],
    });
    out.push(M168Case {
        name: "中段双败",
        script: [
            ("甲", true),
            ("乙", false),
            ("丙", true),
            ("丁", false),
            ("戊", true),
        ],
    });
    out.push(M168Case {
        name: "尾步单败",
        script: [
            ("甲", true),
            ("乙", true),
            ("丙", true),
            ("丁", true),
            ("戊", false),
        ],
    });
    out
}

/// 装一册语料进运行器（五步按语料钦定判定）。
pub fn build_runner(mode: Mode, script: &[(&'static str, bool); 5]) -> ModeRunner {
    let mut r = ModeRunner::new(mode);
    for (name, will_pass) in script.iter() {
        let _ = r.add_step(name, *will_pass);
    }
    let _ = r.seal();
    r
}

/// 一条对照账（档名 + 续跑报告 + 首败即停报告 + 五面对账结论）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareRec {
    /// 账序号（从一起连续递增）。
    pub seq: u32,
    /// 档名。
    pub case: &'static str,
    /// 续跑报告。
    pub cont: RunReport,
    /// 首败即停报告。
    pub strict: RunReport,
    /// 五面对账结论。
    pub agree: bool,
}

/// M-168 两模式行为对照：四档逐档双跑对账，五面全对才落账。
pub fn m168_compare() -> (bool, Vec<CompareRec>) {
    let corpus = m168_corpus();
    let mut recs: Vec<CompareRec> = Vec::new();
    if corpus.len() != M168_CASES {
        return (false, recs);
    }
    let mut all = true;
    let mut seq = 1u32;
    for c in &corpus {
        let mut cont = build_runner(Mode::Continue, &c.script);
        let mut strict = build_runner(Mode::Strict, &c.script);
        let rc = cont.run();
        let rs = strict.run();
        let agree = match (rc, rs) {
            (Ok(a), Ok(b)) => {
                let n = c.script.len();
                // 面1：无败档两模式同账（执行数/失败册/停点全同）。
                let no_fail = a.failures.is_empty();
                let f1 = if no_fail {
                    a.executed == b.executed && a.failures == b.failures && a.stopped_at == b.stopped_at
                } else {
                    // 面2：strict 停点=首败位。
                    let first_fail = a.failures.first().copied().unwrap_or(0);
                    b.stopped_at == Some(first_fail)
                };
                // 面3：strict 未执行数 = 总步数-执行数（报告自检已核，此处复核）。
                let f3 = b.unexecuted == n - b.executed;
                // 面4：continue 失败册 = 全败位（strict 失败册 ⊆ continue 失败册）。
                let f4 = b.failures.iter().all(|x| a.failures.contains(x));
                // 面5：strict 执行数 ≤ continue 执行数。
                let f5 = b.executed <= a.executed;
                f1 && f3 && f4 && f5
            }
            _ => false,
        };
        if !agree {
            all = false;
        }
        let cont_rep = cont.last_report();
        let strict_rep = strict.last_report();
        match (cont_rep, strict_rep) {
            (Some(a), Some(b)) => recs.push(CompareRec {
                seq,
                case: c.name,
                cont: a,
                strict: b,
                agree,
            }),
            _ => {
                all = false;
            }
        }
        seq += 1;
    }
    (all && recs.len() == M168_CASES, recs)
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：空脚本起跑（封口即拒）。
    let mut empty = ModeRunner::new(Mode::Continue);
    mark(&mut out, 0, empty.seal() == Err(Diag::EEmptyScript.val()));
    // 注入2：strict 有败未停（伪造报告语义自检必点出）。
    let fake_strict = RunReport {
        mode: Mode::Strict,
        executed: 5,
        stopped_at: None,
        failures: vec![1],
        unexecuted: 0,
    };
    let script5: [(&'static str, bool); 5] = [
        ("甲", true),
        ("乙", false),
        ("丙", true),
        ("丁", true),
        ("戊", true),
    ];
    let steps: Vec<Step> = script5
        .iter()
        .map(|(n, p)| Step {
            name: n,
            will_pass: *p,
        })
        .collect();
    mark(&mut out, 1, !fake_strict.semantics_ok(&steps));
    // 注入3：continue 丢败（失败册缺一位必点出）。
    let fake_cont = RunReport {
        mode: Mode::Continue,
        executed: 5,
        stopped_at: None,
        failures: vec![1],
        unexecuted: 0,
    };
    let script_two_fail: [(&'static str, bool); 5] = [
        ("甲", true),
        ("乙", false),
        ("丙", true),
        ("丁", false),
        ("戊", true),
    ];
    let steps2: Vec<Step> = script_two_fail
        .iter()
        .map(|(n, p)| Step {
            name: n,
            will_pass: *p,
        })
        .collect();
    mark(&mut out, 2, !fake_cont.semantics_ok(&steps2));
    // 注入4：停点漂移（strict 停点 ≠ 首败位必点出）。
    let fake_stop = RunReport {
        mode: Mode::Strict,
        executed: 4,
        stopped_at: Some(3),
        failures: vec![1],
        unexecuted: 1,
    };
    mark(&mut out, 3, !fake_stop.semantics_ok(&steps));
    // 注入5：未执行计数错（未执行数与执行数不符必点出）。
    let fake_unexec = RunReport {
        mode: Mode::Strict,
        executed: 2,
        stopped_at: Some(1),
        failures: vec![1],
        unexecuted: 2,
    };
    mark(&mut out, 4, !fake_unexec.semantics_ok(&steps));
    // 注入6：越界执行（执行数超总步数必点出）。
    let fake_over = RunReport {
        mode: Mode::Continue,
        executed: 9,
        stopped_at: None,
        failures: vec![1, 3],
        unexecuted: 0,
    };
    mark(&mut out, 5, !fake_over.semantics_ok(&steps2));
    // 注入7：封口后加步。
    let mut sealed = build_runner(Mode::Continue, &script5);
    mark(
        &mut out,
        6,
        sealed.add_step("越权", true) == Err(Diag::EPhase.val()),
    );
    // 注入8：步名重复。
    let mut dup = ModeRunner::new(Mode::Continue);
    let _ = dup.add_step("甲", true);
    mark(&mut out, 7, dup.add_step("甲", true) == Err(Diag::EDupName.val()));
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x79580
}

/// J1 主判据：两模式语义明确（continue 记录续跑 / strict 首败即停）。
fn j1_main_ok() -> bool {
    let script: [(&'static str, bool); 5] = [
        ("甲", true),
        ("乙", false),
        ("丙", true),
        ("丁", false),
        ("戊", true),
    ];
    // 续跑：跑满全程、失败册全收。
    let mut cont = build_runner(Mode::Continue, &script);
    let rc = cont.run();
    let cont_ok = match rc {
        Ok(r) => {
            r.executed == 5
                && r.stopped_at.is_none()
                && r.failures == vec![1, 3]
                && r.unexecuted == 0
        }
        Err(_) => false,
    };
    // 首败即停：停点=1、执行 2 步、余 3 步未执行。
    let mut strict = build_runner(Mode::Strict, &script);
    let rs = strict.run();
    let strict_ok = match rs {
        Ok(r) => {
            r.executed == 2
                && r.stopped_at == Some(1)
                && r.failures == vec![1]
                && r.unexecuted == 3
        }
        Err(_) => false,
    };
    // 报告行非空且带模式名（写回面）。
    let line_ok = match cont.run() {
        Ok(r) => {
            let line = r.report_line();
            line.contains("续跑") && !line.is_empty()
        }
        Err(_) => false,
    };
    cont_ok && strict_ok && line_ok
}

/// M-168 两模式行为对照全绿。
fn m168_ok() -> bool {
    let (all, recs) = m168_compare();
    all && recs.len() == M168_CASES && recs.iter().all(|r| r.agree)
}

/// 对照账序连续。
fn compare_seq_ok() -> bool {
    let (_, recs) = m168_compare();
    let mut want = 1u32;
    for r in &recs {
        if r.seq != want {
            return false;
        }
        want += 1;
    }
    true
}

/// 确定性：同语料重跑报告逐字段一致。
fn determinism_ok() -> bool {
    let script: [(&'static str, bool); 5] = [
        ("甲", true),
        ("乙", false),
        ("丙", true),
        ("丁", true),
        ("戊", false),
    ];
    let mut a = build_runner(Mode::Continue, &script);
    let r1 = a.run();
    let r2 = a.run();
    if r1 != r2 {
        return false;
    }
    let mut b = build_runner(Mode::Strict, &script);
    let s1 = b.run();
    let s2 = b.run();
    s1 == s2 && r1.is_ok() && s1.is_ok()
}

/// 双跑一致（对照账两轮全同）。
fn dual_run_ok() -> bool {
    let (a, ra) = m168_compare();
    let (b, rb) = m168_compare();
    a && b && ra == rb
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3409 断言失败不中断模式。
pub fn run_unx3409_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3409");
    let script: [(&'static str, bool); 5] = [
        ("甲", true),
        ("乙", false),
        ("丙", true),
        ("丁", false),
        ("戊", true),
    ];
    let cont = build_runner(Mode::Continue, &script);
    let strict = build_runner(Mode::Strict, &script);

    s.add_dyn(
        "F3409-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 正常流：两模式语义明确（J1 主判据）。
    s.add_dyn(
        "F3409-02 两模式语义明确",
        j1_main_ok(),
        &format!(
            "cont={} strict={}",
            cont.last_report().map(|r| r.report_line()).unwrap_or_default(),
            strict.last_report().map(|r| r.report_line()).unwrap_or_default()
        ),
    );

    // M-168 两模式行为对照全绿。
    let (m_ok, recs) = m168_compare();
    s.add_dyn(
        "F3409-03 M168两模式行为对照",
        m_ok,
        &format!(
            "recs={} agree_all={}",
            recs.len(),
            recs.iter().all(|r| r.agree)
        ),
    );

    // 对照账序连续。
    s.add_dyn(
        "F3409-04 对照账序连续",
        compare_seq_ok(),
        &format!("seq={}", compare_seq_ok()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3409-05 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 边界：无败语料两模式同账（对照面零点）。
    let no_fail_script: [(&'static str, bool); 5] = [
        ("甲", true),
        ("乙", true),
        ("丙", true),
        ("丁", true),
        ("戊", true),
    ];
    let mut nf_cont = build_runner(Mode::Continue, &no_fail_script);
    let mut nf_strict = build_runner(Mode::Strict, &no_fail_script);
    let zero_ok = match (nf_cont.run(), nf_strict.run()) {
        (Ok(a), Ok(b)) => {
            a.executed == 5
                && b.executed == 5
                && a.failures.is_empty()
                && b.failures.is_empty()
                && a.stopped_at.is_none()
                && b.stopped_at.is_none()
        }
        _ => false,
    };
    s.add_dyn(
        "F3409-06 无败语料两模式同账",
        zero_ok,
        &format!("zero={}", zero_ok),
    );

    // 边界：全败语料——strict 恰执行 1 步，continue 全收。
    let all_fail: [(&'static str, bool); 5] = [
        ("甲", false),
        ("乙", false),
        ("丙", false),
        ("丁", false),
        ("戊", false),
    ];
    let mut af_cont = build_runner(Mode::Continue, &all_fail);
    let mut af_strict = build_runner(Mode::Strict, &all_fail);
    let all_fail_ok = match (af_cont.run(), af_strict.run()) {
        (Ok(a), Ok(b)) => {
            a.executed == 5
                && a.failures.len() == 5
                && b.executed == 1
                && b.stopped_at == Some(0)
                && b.unexecuted == 4
        }
        _ => false,
    };
    s.add_dyn(
        "F3409-07 全败语料两模式对照",
        all_fail_ok,
        &format!("all_fail={}", all_fail_ok),
    );

    // 边界：空脚本显性拒（装配相位起跑拒、封口拒）。
    let mut empty = ModeRunner::new(Mode::Strict);
    let empty_ok = empty.run() == Err(Diag::EPhase.val())
        && empty.seal() == Err(Diag::EEmptyScript.val())
        && empty.script_len() == 0;
    s.add_dyn(
        "F3409-08 空脚本显性拒",
        empty_ok,
        &format!("len={}", empty.script_len()),
    );

    // 边界：相位单向（装配→封口→已跑，倒退拒）。
    let mut phased = build_runner(Mode::Continue, &script);
    let _ = phased.run();
    let phase_ok = phased.phase() == Phase::Ran
        && phased.add_step("越权", true) == Err(Diag::EPhase.val())
        && phased.seal() == Err(Diag::EPhase.val())
        && phased.phase().ord() == 2;
    s.add_dyn(
        "F3409-09 相位单向倒退拒",
        phase_ok,
        &format!("phase={}", phased.phase().name()),
    );

    // 确定性：同语料重跑报告逐字段一致。
    s.add_dyn(
        "F3409-10 同语料重跑报告一致",
        determinism_ok(),
        &format!("det={}", determinism_ok()),
    );

    // 步名互异面：重复名与空名均显性拒。
    let mut names = ModeRunner::new(Mode::Continue);
    let _ = names.add_step("甲", true);
    let name_ok = names.add_step("甲", true) == Err(Diag::EDupName.val())
        && names.add_step("", true) == Err(Diag::EEmptyName.val())
        && names.script_len() == 1;
    s.add_dyn(
        "F3409-11 步名互异非空闸",
        name_ok,
        &format!("len={}", names.script_len()),
    );

    // 取步越位显性拒。
    let step_ok = cont.step_at(4).is_some()
        && cont.step_at(5).is_none()
        && cont.step_at(99).is_none();
    s.add_dyn(
        "F3409-12 取步越位显性拒",
        step_ok,
        &format!("len={}", cont.script_len()),
    );

    // 写回摘要：对照账含两模式报告行（判据③写回面）。
    let mut sum = String::new();
    for r in &recs {
        sum.push_str(&format!(
            "档{} 续[{}] 停[{}] 对{}",
            r.case,
            r.cont.report_line(),
            r.strict.report_line(),
            r.agree
        ));
    }
    s.add_dyn(
        "F3409-13 对照账摘要非空",
        !sum.is_empty() && sum.contains("续跑") && sum.contains("首败即停"),
        &format!("bytes={}", sum.len()),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F3409-14 双跑一致与诊断码闭集",
        dual_run_ok()
            && ALL.len() == 12
            && detail(SEG_BASE + 0x0C) == Diag::EPhase.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EEmptyScript.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79580);
    }

    #[test]
    fn continue_mode_collects_all_failures() {
        let script: [(&'static str, bool); 5] = [
            ("甲", true),
            ("乙", false),
            ("丙", true),
            ("丁", false),
            ("戊", true),
        ];
        let mut r = build_runner(Mode::Continue, &script);
        let rep = r.run();
        match rep {
            Ok(x) => {
                assert_eq!(x.executed, 5);
                assert_eq!(x.failures, vec![1, 3]);
                assert_eq!(x.stopped_at, None);
                assert_eq!(x.unexecuted, 0);
            }
            Err(_) => assert!(false, "续跑必须成功"),
        }
    }

    #[test]
    fn strict_mode_stops_at_first_failure() {
        let script: [(&'static str, bool); 5] = [
            ("甲", true),
            ("乙", true),
            ("丙", false),
            ("丁", false),
            ("戊", true),
        ];
        let mut r = build_runner(Mode::Strict, &script);
        let rep = r.run();
        match rep {
            Ok(x) => {
                assert_eq!(x.executed, 3);
                assert_eq!(x.stopped_at, Some(2));
                assert_eq!(x.failures, vec![2]);
                assert_eq!(x.unexecuted, 2);
            }
            Err(_) => assert!(false, "首败即停必须成功"),
        }
    }

    #[test]
    fn no_fail_same_behavior() {
        let script: [(&'static str, bool); 5] = [
            ("甲", true),
            ("乙", true),
            ("丙", true),
            ("丁", true),
            ("戊", true),
        ];
        let mut a = build_runner(Mode::Continue, &script);
        let mut b = build_runner(Mode::Strict, &script);
        let ra = a.run();
        let rb = b.run();
        match (ra, rb) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.executed, y.executed);
                assert_eq!(x.failures, y.failures);
                assert_eq!(x.stopped_at, y.stopped_at);
            }
            _ => assert!(false, "无败两模式都必须成功"),
        }
    }

    #[test]
    fn m168_four_cases_green() {
        assert_eq!(m168_corpus().len(), M168_CASES);
        assert!(m168_ok());
        let (_, recs) = m168_compare();
        assert_eq!(recs.len(), M168_CASES);
        assert!(recs.iter().all(|r| r.agree));
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn empty_script_rejected() {
        let mut r = ModeRunner::new(Mode::Continue);
        assert_eq!(r.seal(), Err(Diag::EEmptyScript.val()));
        assert_eq!(r.run(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn phase_one_way() {
        let mut r = ModeRunner::new(Mode::Continue);
        assert_eq!(r.phase(), Phase::Building);
        let _ = r.add_step("甲", true);
        assert_eq!(r.seal(), Ok(()));
        assert_eq!(r.phase(), Phase::Sealed);
        assert_eq!(r.add_step("乙", true), Err(Diag::EPhase.val()));
        let _ = r.run();
        assert_eq!(r.phase(), Phase::Ran);
        assert_eq!(r.seal(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn dup_and_empty_names_rejected() {
        let mut r = ModeRunner::new(Mode::Continue);
        assert!(r.add_step("甲", true).is_ok());
        assert_eq!(r.add_step("甲", true), Err(Diag::EDupName.val()));
        assert_eq!(r.add_step("", true), Err(Diag::EEmptyName.val()));
        assert_eq!(r.script_len(), 1);
    }

    #[test]
    fn fake_reports_caught_by_semantics() {
        let script: [(&'static str, bool); 5] = [
            ("甲", true),
            ("乙", false),
            ("丙", true),
            ("丁", true),
            ("戊", true),
        ];
        let steps: Vec<Step> = script
            .iter()
            .map(|(n, p)| Step {
                name: n,
                will_pass: *p,
            })
            .collect();
        // strict 有败未停必须被点出。
        let fake = RunReport {
            mode: Mode::Strict,
            executed: 5,
            stopped_at: None,
            failures: vec![1],
            unexecuted: 0,
        };
        assert!(!fake.semantics_ok(&steps));
        // 真报告必须过语义自检。
        let mut r = build_runner(Mode::Strict, &script);
        let rep = r.run();
        match rep {
            Ok(x) => assert!(x.semantics_ok(&steps)),
            Err(_) => assert!(false, "真报告必须过自检"),
        }
    }

    #[test]
    fn determinism_rerun_identical() {
        assert!(determinism_ok());
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3409_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
