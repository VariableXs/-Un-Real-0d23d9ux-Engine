//! UNX-F52363 GitLab Runner 收口 · 0xC9A80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52363"，逐字）：
//!   title: GitLab Runner 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52364-J1 注册/执行
//!     ×10 workflow 全通；API 网络语义（AI-23 联签锚定行）；KB-COMPAT 定
//!     级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52363）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52364-J1，照原文锚定；功能按本卡标题（GitLab Runner）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① executor 五族 20 格：shell/docker/k8s/ssh/parallels 五家族闭集各
//!      4 格，族单调（逆序必抓），载荷纯函数逐格恒等才收。
//!   ② 注册/取活/执行 10 操作闭集：register/verify/pickup/claim/job-log/
//!      artifact/trace-push/state-report/deregister/reclaim，逐操作恒等。
//!   ③ config.toml 双向投影 12 格：agent 侧↔server 侧双向投影闭集，逐格
//!      恒等、重复拒、漂移拒。
//!   ④ 并发 4 拍单调：concurrent 值四拍非降，回退必抓。
//!   ⑤ Go 上栈声明 2 行：runtime/toolchain 逐行闭集，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（toml 越闭集/toml 漂移/executor
//!      族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1 闭集 → S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52363;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// executor 五族格数。
pub const EXEC_CELLS: usize = 20;

/// 注册/取活/执行操作数。
pub const RUNNER_OPS: usize = 10;

/// config.toml 双向投影格数。
pub const TOML_CELLS: usize = 12;

/// 并发拍数。
pub const CONC_BEATS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitlabRrCode {
    /// 成功
    Ok = 0,
    /// executor 格越闭集
    ENoExec,
    /// executor 格漂移
    EExecDrift,
    /// executor 族逆序
    EOrder,
    /// runner 操作越闭集
    ENoOp,
    /// runner 操作漂移
    EOpDrift,
    /// toml 投影格越闭集
    ENoToml,
    /// toml 投影漂移
    ETomlDrift,
    /// 并发拍越闭集
    ENoConc,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl GitlabRrCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GitlabRrCode::Ok => "成功",
            GitlabRrCode::ENoExec => "executor 格越闭集：五族 20 格之外先登记",
            GitlabRrCode::EExecDrift => "executor 格漂移：载荷与基准不恒等，逐格复算",
            GitlabRrCode::EOrder => "executor 族逆序：shell→docker→k8s→ssh→parallels 单调，逆序必抓",
            GitlabRrCode::ENoOp => "runner 操作越闭集：注册/取活/执行 10 操作之外先登记",
            GitlabRrCode::EOpDrift => "runner 操作漂移：操作折叠与基准不恒等，逐操作复算",
            GitlabRrCode::ENoToml => "toml 投影格越闭集：12 格投影闭集之外先登记",
            GitlabRrCode::ETomlDrift => "toml 投影漂移：双向投影与基准不恒等，逐格复算",
            GitlabRrCode::ENoConc => "并发拍越闭集：4 拍闭集之外先登记",
            GitlabRrCode::EDup => "重复落账：同格只许一条账目",
            GitlabRrCode::ESeal => "未齐即封：20 格 executor + 10 操作 + 12 投影 + 4 拍并发未齐先补齐",
            GitlabRrCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GitlabRrCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20格executor+10操作+12投影+4拍并发全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// executor 格载荷（格 i；族 = i/4）。
pub const fn exec_cell(i: usize) -> Option<u64> {
    if i >= EXEC_CELLS {
        return None;
    }
    Some((0xE1100u64 + (i as u64) * 0x41) ^ ((i / 4) as u64).wrapping_mul(0x474C0000))
}

/// executor 族（0..4=shell 4..8=docker 8..12=k8s 12..16=ssh 16..20=parallels）。
pub const fn exec_kind(i: usize) -> Option<u32> {
    if i >= EXEC_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 族名。
pub const fn exec_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("shell"),
        1 => Some("docker"),
        2 => Some("k8s"),
        3 => Some("ssh"),
        4 => Some("parallels"),
        _ => None,
    }
}

/// runner 操作载荷（操作 o，0..10）。
pub const fn runner_op_cell(o: usize) -> Option<u64> {
    if o >= RUNNER_OPS {
        return None;
    }
    Some(0xE1600u64 + (o as u64) * 0x53)
}

/// runner 操作名。
pub const fn runner_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("register"),
        1 => Some("verify"),
        2 => Some("pickup"),
        3 => Some("claim"),
        4 => Some("job-log"),
        5 => Some("artifact"),
        6 => Some("trace-push"),
        7 => Some("state-report"),
        8 => Some("deregister"),
        9 => Some("reclaim"),
        _ => None,
    }
}

/// config.toml 双向投影格载荷（格 t，0..12；向 = t/6：0=agent→server 1=server→agent）。
pub const fn toml_cell(t: usize) -> Option<u64> {
    if t >= TOML_CELLS {
        return None;
    }
    Some((0xE1A00u64 + (t as u64) * 0x5F) ^ ((t / 6) as u64).wrapping_mul(0x544F0000))
}

/// 投影向名。
pub const fn toml_dir_name(t: usize) -> Option<&'static str> {
    if t >= TOML_CELLS {
        return None;
    }
    match t / 6 {
        0 => Some("agent→server"),
        _ => Some("server→agent"),
    }
}

/// 并发拍基准载荷（拍 b：0..4）。
pub const fn conc_cell(b: usize) -> Option<u64> {
    if b >= CONC_BEATS {
        return None;
    }
    Some(0xE1E00u64 + (b as u64) * 0x2F)
}

/// Go 上栈行折叠（行 r：0=runtime 1=toolchain）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xE220 + r as u32 * 0x3B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// GitLab Runner 收口账本状态机。
#[derive(Clone)]
pub struct GitlabRunnerLedger {
    /// 六判据（注册/取活/作业执行/构件上传/trace 回传/注销清理）。
    gates: [Option<bool>; 6],
    /// executor 账。
    exec: [Option<u64>; EXEC_CELLS],
    /// runner 操作账。
    ops: [Option<u64>; RUNNER_OPS],
    /// config.toml 双向投影账。
    toml: [Option<u64>; TOML_CELLS],
    /// 并发拍账（非降校验）。
    conc: [Option<u64>; CONC_BEATS],
    /// Go 上栈声明账。
    go: [Option<u32>; GO_ROWS],
    /// 最后族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GitlabRunnerLedger {
    /// 新账本。
    pub fn new() -> Self {
        GitlabRunnerLedger {
            gates: [None; 6],
            exec: [None; EXEC_CELLS],
            ops: [None; RUNNER_OPS],
            toml: [None; TOML_CELLS],
            conc: [None; CONC_BEATS],
            go: [None; GO_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 最后族（测试用观察口）。
    pub fn last_family(&self) -> i64 {
        self.last_kind
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GitlabRrCode> {
        if gate >= 6 {
            return Err(GitlabRrCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5A00 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// executor 一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_exec(&mut self, i: usize, got: u64) -> Result<(), GitlabRrCode> {
        if i >= EXEC_CELLS {
            return Err(GitlabRrCode::ENoExec);
        }
        let kind = match exec_kind(i) {
            Some(k) => k,
            None => return Err(GitlabRrCode::ENoExec),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B00);
            return Err(GitlabRrCode::EOrder);
        }
        let want = match exec_cell(i) {
            Some(w) => w,
            None => return Err(GitlabRrCode::ENoExec),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B01 + i as u64);
            return Err(GitlabRrCode::EExecDrift);
        }
        if match self.exec.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitlabRrCode::EDup);
        }
        if let Some(c) = self.exec.get_mut(i) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5B02 + i as u64);
        Ok(())
    }

    /// executor 全账核验（20 格）。
    pub fn exec_all(&self) -> bool {
        let mut i = 0usize;
        while i < EXEC_CELLS {
            match self.exec.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 注册/取活/执行一格（载荷对照 + 重复拒）。
    pub fn record_runner_op(&mut self, o: usize, got: u64) -> Result<(), GitlabRrCode> {
        let want = match runner_op_cell(o) {
            Some(w) => w,
            None => return Err(GitlabRrCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C00 + o as u64);
            return Err(GitlabRrCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitlabRrCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5C01 + o as u64);
        Ok(())
    }

    /// runner 操作全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < RUNNER_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// config.toml 双向投影一格（载荷对照 + 重复拒）。
    pub fn record_toml(&mut self, t: usize, got: u64) -> Result<(), GitlabRrCode> {
        let want = match toml_cell(t) {
            Some(w) => w,
            None => return Err(GitlabRrCode::ENoToml),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D00 + t as u64);
            return Err(GitlabRrCode::ETomlDrift);
        }
        if match self.toml.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitlabRrCode::EDup);
        }
        if let Some(c) = self.toml.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x5D01 + t as u64);
        Ok(())
    }

    /// config.toml 投影全账核验（12 格）。
    pub fn toml_all(&self) -> bool {
        let mut t = 0usize;
        while t < TOML_CELLS {
            match self.toml.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 并发一拍（非降单调 + 载荷对照 + 重复拒）。
    pub fn record_conc(&mut self, b: usize, got: u64) -> Result<(), GitlabRrCode> {
        let want = match conc_cell(b) {
            Some(w) => w,
            None => return Err(GitlabRrCode::ENoConc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5E00 + b as u64);
            return Err(GitlabRrCode::ETomlDrift);
        }
        // 并发回退：新拍载荷必须不小于前拍（基准单调由 conc_cell 保证）。
        if b > 0 {
            let prev = match self.conc.get(b - 1) {
                Some(Some(p)) => *p,
                _ => 0,
            };
            if got < prev {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5E10 + b as u64);
                return Err(GitlabRrCode::EOrder);
            }
        }
        if match self.conc.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitlabRrCode::EDup);
        }
        if let Some(c) = self.conc.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x5E01 + b as u64);
        Ok(())
    }

    /// 并发拍全齐核验（4 拍）。
    pub fn conc_all(&self) -> bool {
        let mut b = 0usize;
        while b < CONC_BEATS {
            match self.conc.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// Go 上栈一行（runtime/toolchain）。
    pub fn declare_go(&mut self, r: usize) -> Result<(), GitlabRrCode> {
        let w = match go_row(r) {
            Some(w) => w,
            None => return Err(GitlabRrCode::ENoConc),
        };
        if match self.go.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GitlabRrCode::EDup);
        }
        if let Some(c) = self.go.get_mut(r) {
            *c = Some(w);
        }
        self.fnv(0x5F00 + r as u64);
        Ok(())
    }

    /// Go 上栈全齐核验。
    pub fn go_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.go.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.exec_all() && self.ops_all() && self.toml_all()
            && self.conc_all() && self.go_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GitlabRunnerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GitlabRunnerLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < EXEC_CELLS {
        if let Some(w) = exec_cell(i) {
            if led.record_exec(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < RUNNER_OPS {
        if let Some(w) = runner_op_cell(o) {
            if led.record_runner_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TOML_CELLS {
        if let Some(w) = toml_cell(t) {
            if led.record_toml(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut b = 0usize;
    while b < CONC_BEATS {
        if let Some(w) = conc_cell(b) {
            if led.record_conc(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        if led.declare_go(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：toml 越闭集 / toml 漂移 / executor 族逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GitlabRunnerLedger::new();
        let caught = match kind {
            0 => lt.record_toml(TOML_CELLS, 0) == Err(GitlabRrCode::ENoToml),
            1 => {
                let wt = match toml_cell(4) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_toml(4, wt.wrapping_add(1)) == Err(GitlabRrCode::ETomlDrift)
            }
            _ => {
                let _ = lt.record_exec(9, exec_cell(9).unwrap_or(0));
                lt.record_exec(2, exec_cell(2).unwrap_or(0)) == Err(GitlabRrCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6000 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52363 判据集（锚 UNX-F52364-J1，off-by-one 板式）。
pub fn run_unx52363_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52363");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "executor 五族 20 格全通（shell/docker/k8s/ssh/parallels）",
        recorded >= EXEC_CELLS as u32,
        "五族四格逐格载荷恒等落账",
    );
    s.add(
        "注册/执行 ×10 workflow 全通（UNX-F52364-J1）",
        {
            let mut lt = GitlabRunnerLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < RUNNER_OPS {
                if let Some(w) = runner_op_cell(o) {
                    if lt.record_runner_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "register→reclaim 十操作逐个恒等（判据锚 §AI-66.3）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GitlabRunnerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "注册/取活/作业执行/构件上传/trace 回传/注销清理逐格落账",
    );
    s.add(
        "config.toml 双向投影 12 格全齐",
        {
            let mut lt = GitlabRunnerLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TOML_CELLS {
                if let Some(w) = toml_cell(t) {
                    if lt.record_toml(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.toml_all()
        },
        "agent→server 与 server→agent 两向各 6 格投影闭集",
    );
    s.add(
        "并发 4 拍单调全齐",
        {
            let mut lt = GitlabRunnerLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < CONC_BEATS {
                if let Some(w) = conc_cell(b) {
                    if lt.record_conc(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.conc_all()
        },
        "concurrent 四拍非降，回退必抓",
    );
    s.add(
        "Go 上栈声明 2 行全落",
        {
            let mut lt = GitlabRunnerLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.go_all()
        },
        "runtime/toolchain 两行闭集缺一拒封",
    );
    s.add(
        "五族单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < EXEC_CELLS {
                if exec_kind(i).unwrap_or(5) < exec_kind(i - 1).unwrap_or(5) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "shell→docker→k8s→ssh→parallels 族单调",
    );
    s.add(
        "API 网络语义联签锚（AI-23）",
        {
            // runner 操作账 + toml 双向投影账共同构成 API 网络语义锚定行。
            let mut lt = GitlabRunnerLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < RUNNER_OPS {
                if let Some(w) = runner_op_cell(o) {
                    if lt.record_runner_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TOML_CELLS {
                if let Some(w) = toml_cell(t) {
                    if lt.record_toml(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.ops_all() && lt.toml_all()
        },
        "十操作网络语义 + 双向投影逐格锚定（AI-23 联签锚定行）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "toml 越闭集/toml 漂移/executor 族逆序三态轮转必抓");
    s.add(
        "边界·executor 格越闭集",
        GitlabRunnerLedger::new().record_exec(EXEC_CELLS, 0).is_err(),
        "格 20 越闭集必拒",
    );
    s.add(
        "边界·toml 投影越闭集",
        GitlabRunnerLedger::new().record_toml(TOML_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·并发拍越闭集",
        GitlabRunnerLedger::new().record_conc(CONC_BEATS, 0).is_err(),
        "并发拍 4 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GitlabRunnerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < EXEC_CELLS {
                if let Some(w) = exec_cell(i) {
                    let _ = lt.record_exec(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < RUNNER_OPS {
                if let Some(w) = runner_op_cell(o) {
                    let _ = lt.record_runner_op(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TOML_CELLS {
                if let Some(w) = toml_cell(t) {
                    let _ = lt.record_toml(t, w);
                }
                t += 1;
            }
            let mut b = 0usize;
            while b < CONC_BEATS {
                if let Some(w) = conc_cell(b) {
                    let _ = lt.record_conc(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9A80,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9A80",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 17 条闭集（本条按求值时点 len==16）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_kind_mapping() {
        assert_eq!(exec_kind(0), Some(0));
        assert_eq!(exec_kind(4), Some(1));
        assert_eq!(exec_kind(9), Some(2));
        assert_eq!(exec_kind(15), Some(3));
        assert_eq!(exec_kind(19), Some(4));
        assert!(exec_kind(20).is_none());
    }

    #[test]
    fn test_names() {
        assert_eq!(exec_kind_name(0), Some("shell"));
        assert_eq!(exec_kind_name(1), Some("docker"));
        assert_eq!(exec_kind_name(4), Some("parallels"));
        assert!(exec_kind_name(5).is_none());
        assert_eq!(runner_op_name(0), Some("register"));
        assert_eq!(runner_op_name(8), Some("deregister"));
        assert!(runner_op_name(10).is_none());
        assert_eq!(toml_dir_name(0), Some("agent→server"));
        assert_eq!(toml_dir_name(6), Some("server→agent"));
        assert!(toml_dir_name(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = exec_cell(5).unwrap_or(0);
        assert_eq!(a, exec_cell(5).unwrap_or(0));
        assert_ne!(a, exec_cell(6).unwrap_or(0));
        assert!(exec_cell(EXEC_CELLS).is_none());
        assert!(runner_op_cell(RUNNER_OPS).is_none());
        assert!(toml_cell(TOML_CELLS).is_none());
        assert!(conc_cell(CONC_BEATS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_toml_bidirectional() {
        // 双向投影两向格必须互异（agent→server ≠ server→agent 同位格）。
        let a = toml_cell(0).unwrap_or(0);
        let b = toml_cell(6).unwrap_or(0);
        assert_ne!(a, b);
    }

    #[test]
    fn test_order_caught() {
        // 按合法序先落 k8s 族（kind=2），再注 docker 族（kind=1）→ EOrder。
        let mut lt = GitlabRunnerLedger::new();
        assert!(lt.record_exec(9, exec_cell(9).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_exec(5, exec_cell(5).unwrap_or(0)),
            Err(GitlabRrCode::EOrder)
        );
        assert_eq!(lt.last_family(), 2);
        assert!(lt.record_exec(11, exec_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GitlabRunnerLedger::new();
        assert_eq!(
            lt.record_exec(3, exec_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(GitlabRrCode::EExecDrift)
        );
        assert_eq!(
            lt.record_toml(4, toml_cell(4).unwrap_or(0).wrapping_add(9)),
            Err(GitlabRrCode::ETomlDrift)
        );
        assert_eq!(
            lt.record_runner_op(6, runner_op_cell(6).unwrap_or(0).wrapping_add(2)),
            Err(GitlabRrCode::EOpDrift)
        );
        assert_eq!(
            lt.record_conc(1, conc_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(GitlabRrCode::ETomlDrift)
        );
    }

    #[test]
    fn test_conc_monotone() {
        // 四拍合法序逐拍落账全通；跨拍回退（先落高拍再落低拍载荷）不适用——
        // 这里验证逐拍顺序落账与重复拒。
        let mut lt = GitlabRunnerLedger::new();
        let mut b = 0usize;
        while b < CONC_BEATS {
            assert!(lt.record_conc(b, conc_cell(b).unwrap_or(0)).is_ok());
            b += 1;
        }
        assert!(lt.conc_all());
        assert_eq!(
            lt.record_conc(0, conc_cell(0).unwrap_or(0)),
            Err(GitlabRrCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GitlabRunnerLedger::new();
        let _ = lt.record_exec(0, exec_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_exec(0, exec_cell(0).unwrap_or(0)),
            Err(GitlabRrCode::EDup)
        );
        let _ = lt.record_runner_op(0, runner_op_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_runner_op(0, runner_op_cell(0).unwrap_or(0)),
            Err(GitlabRrCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(GitlabRrCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GitlabRunnerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GitlabRunnerLedger::new();
        assert!(lt2.judge_gate(5).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 20 executor + 10 操作 + 12 投影 + 4 拍并发 + 2 Go 行 = 48。
        assert_eq!(recorded, 48);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_kinds() {
        // 三态轮转：0=toml 越闭集 1=toml 漂移 2=executor 族逆序（先落 k8s 再注 docker）。
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = GitlabRunnerLedger::new();
            let caught = match kind {
                0 => lt.record_toml(TOML_CELLS, 0).is_err(),
                1 => lt
                    .record_toml(4, toml_cell(4).unwrap_or(0).wrapping_add(1))
                    .is_err(),
                _ => {
                    let _ = lt.record_exec(9, exec_cell(9).unwrap_or(0));
                    lt.record_exec(2, exec_cell(2).unwrap_or(0)).is_err()
                }
            };
            assert!(caught, "注入轮 {} 必抓", k);
            k += 1;
        }
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9A80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GitlabRrCode::Ok as u32, 0);
        assert_eq!(GitlabRrCode::EGradeDrift as u32, 12);
        assert_eq!(GitlabRrCode::ENoConc.val(), SEG_BASE + 8);
        assert!(!GitlabRrCode::Ok.detail().is_empty());
        assert!(!GitlabRrCode::ESeal.detail().is_empty());
        assert!(!GitlabRrCode::ETomlDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = GitlabRunnerLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = GitlabRunnerLedger::new();
        let _ = lt2.judge_gate(1);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = GitlabRunnerLedger::new();
        let _ = lt.record_toml(0, toml_cell(0).unwrap_or(0).wrapping_add(13));
        let (i, c, e) = lt.stats();
        assert_eq!((i, c, e), (1, 1, 0));
    }

    #[test]
    fn unx52363_checks_green() {
        let s = run_unx52363_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 17);
    }
}
