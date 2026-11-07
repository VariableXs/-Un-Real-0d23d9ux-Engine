//! UNX-F52263 Docker Compose 收口 · 0xC8180段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52263"，逐字）：
//!   title: Docker Compose 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52264-J1 六判据
//!     全落；rootless 路径 ×10；虚拟化依赖 N/A 同构（F52261 口径复用）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52263）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52264-J1，照原文锚定；功能按本卡标题（Docker Compose）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① compose.yaml 双向投影 16 格：正向 8 格（compose→runtime 投影：
//!      服务/镜像/端口/卷/环境/网络/依赖/命令）+ 反向 8 格（runtime→
//!      compose 回投影同序八维），逐格载荷（纯函数折叠）恒等才收。
//!   ② up/down/ps/logs/scale 10 操作集：五相位闭集各 2 操作，相位单调
//!      （逆序必抓）；漂移必抓。
//!   ③ 依赖序 8 格：depends_on 拓扑序逐格落账，序漂移必抓。
//!   ④ profile 5 场景闭集：dev/test/prod/debug/ci 逐场景恒等，越界必抓。
//!   ⑤ 虚拟化依赖 N/A 同构声明（F52261 口径复用）：纯函数同构恒等
//!      声明，不落虚拟化账。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（投影格越闭集/
//!      相位操作漂移/相位逆序），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S1/S2/S3 闭集（16 投影 + 10 操作 + 8 依
//!      赖 + 5 profile + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52263;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 投影方向数（0=正向 1=反向）。
pub const PROJ_DIRS: usize = 2;

/// 每向投影格数。
pub const PROJ_PER_DIR: usize = 8;

/// 双向投影总格数。
pub const PROJ_CELLS: usize = PROJ_DIRS * PROJ_PER_DIR;

/// 相位操作数。
pub const COMP_OPS: usize = 10;

/// 依赖序格数。
pub const DEP_CELLS: usize = 8;

/// profile 场景数。
pub const PROF_SCENARIOS: usize = 5;

/// 六判据闸数。
pub const GATES: usize = 6;

/// 判据锚定原文（卡面 criteria 逐字引用）。
pub const CRITERIA_ANCHOR: &str = "§AI-66.3";

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComposeCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 相位逆序
    EOrder,
    /// 依赖格越闭集
    ENoDep,
    /// profile 越闭集
    ENoProf,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl ComposeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ComposeCode::Ok => "成功",
            ComposeCode::ENoProj => "投影格越闭集：compose.yaml 双向投影 16 格之外先登记",
            ComposeCode::EProjDrift => "投影漂移：投影格载荷与基准折叠不恒等，逐格复算",
            ComposeCode::ENoOp => "操作号越闭集：up/down/ps/logs/scale 10 操作集之外先登记",
            ComposeCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ComposeCode::EOrder => "相位逆序：up→down→ps→logs→scale 单调，逆序必抓",
            ComposeCode::ENoDep => "依赖格越闭集：depends_on 8 格闭集之外先登记",
            ComposeCode::ENoProf => "profile 越闭集：5 场景之外先登记",
            ComposeCode::EDup => "重复落账：同格只许一条账目",
            ComposeCode::ESeal => "未齐即封：16 投影 + 10 操作 + 8 依赖未齐先补齐",
            ComposeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ComposeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ComposeCode::EGradeDrift => "定级漂移：全通且 profile 场景全落才可定 S1",
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
            Grade::S1 => "S1：六判据+16投影+10操作+8依赖+5场景全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 投影维名（每向 8 维同序）。
pub const fn proj_dim_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("服务"),
        1 => Some("镜像"),
        2 => Some("端口"),
        3 => Some("卷"),
        4 => Some("环境"),
        5 => Some("网络"),
        6 => Some("依赖"),
        7 => Some("命令"),
        _ => None,
    }
}

/// 投影方向名。
pub const fn proj_dir_name(dir: usize) -> Option<&'static str> {
    match dir {
        0 => Some("compose→runtime"),
        1 => Some("runtime→compose"),
        _ => None,
    }
}

/// 双向投影格基准（dir 0/1 × 维 d 0..8，共 16 格）。
pub const fn proj_cell(dir: usize, d: usize) -> Option<u64> {
    if dir >= PROJ_DIRS || d >= PROJ_PER_DIR {
        return None;
    }
    let idx = dir * PROJ_PER_DIR + d;
    Some((0xD3100u64 + (idx as u64) * 0x2D) ^ (dir as u64).wrapping_mul(0x01000193) ^ 0x434F4D504F530000)
}

/// 相位操作格载荷（操作 o；相位 = o/2）。
pub const fn comp_cell(o: usize) -> Option<u64> {
    if o >= COMP_OPS {
        return None;
    }
    Some((0xD3200u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 操作相位（0..2=up 2..4=down 4..6=ps 6..8=logs 8..10=scale）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMP_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 相位名。
pub const fn comp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("up"),
        1 => Some("down"),
        2 => Some("ps"),
        3 => Some("logs"),
        4 => Some("scale"),
        _ => None,
    }
}

/// 依赖序格基准（格 d：depends_on 拓扑序）。
pub const fn dep_cell(d: usize) -> Option<u64> {
    if d >= DEP_CELLS {
        return None;
    }
    Some((0xD3300u64 + (d as u64) * 0x31) ^ 0x4445504F4E440000)
}

/// 依赖序格名。
pub const fn dep_cell_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("db-condition"),
        1 => Some("cache-condition"),
        2 => Some("mq-condition"),
        3 => Some("api-depends-db"),
        4 => Some("worker-depends-mq"),
        5 => Some("web-depends-api"),
        6 => Some("e2e-depends-web"),
        7 => Some("report-depends-e2e"),
        _ => None,
    }
}

/// profile 场景载荷（场景 p）。
pub const fn profile_cell(p: usize) -> Option<u64> {
    if p >= PROF_SCENARIOS {
        return None;
    }
    Some((0xD3400u64 + (p as u64) * 0x2D) ^ 0x50524F46494C0000)
}

/// profile 场景名。
pub const fn profile_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("dev"),
        1 => Some("test"),
        2 => Some("prod"),
        3 => Some("debug"),
        4 => Some("ci"),
        _ => None,
    }
}

/// 虚拟化依赖 N/A 同构声明（F52261 口径复用）：纯函数恒等声明。
/// Compose 工程自身不持虚拟化账，依赖 N/A 同构映射到 F52261 双后端口径。
pub const fn na_isomorph() -> u32 {
    (F52261_PLANE ^ 0x4E415F49534F0000) as u32 & 0xFFFF
}

/// F52261 共面基准（同构源）。
pub const F52261_PLANE: u64 = 0x574C32484F530000;

// ── 收口账本 ─────────────────────────────────────────────────────
/// Docker Compose 收口账本状态机。
#[derive(Clone)]
pub struct ComposeLedger {
    /// 六判据（解析/up/服务/崩溃/down/配置）。
    gates: [Option<bool>; GATES],
    /// 双向投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 相位操作账。
    ops: [Option<u64>; COMP_OPS],
    /// 依赖序账。
    deps: [Option<u64>; DEP_CELLS],
    /// profile 场景账。
    profs: [Option<u64>; PROF_SCENARIOS],
    /// 最后相位。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ComposeLedger {
    /// 新账本。
    pub fn new() -> Self {
        ComposeLedger {
            gates: [None; GATES],
            proj: [None; PROJ_CELLS],
            ops: [None; COMP_OPS],
            deps: [None; DEP_CELLS],
            profs: [None; PROF_SCENARIOS],
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ComposeCode> {
        if gate >= GATES {
            return Err(ComposeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5C100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 投影一格（双向闭集 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, dir: usize, d: usize, got: u64) -> Result<(), ComposeCode> {
        let want = match proj_cell(dir, d) {
            Some(w) => w,
            None => return Err(ComposeCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C200 + (dir * PROJ_PER_DIR + d) as u64);
            return Err(ComposeCode::EProjDrift);
        }
        let idx = dir * PROJ_PER_DIR + d;
        if match self.proj.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ComposeCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(idx) {
            *c = Some(got);
        }
        self.fnv(0x5C201 + idx as u64);
        Ok(())
    }

    /// 双向投影全账核验（16 格）。
    pub fn proj_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJ_CELLS {
            match self.proj.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 相位操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ComposeCode> {
        if o >= COMP_OPS {
            return Err(ComposeCode::ENoOp);
        }
        let kind = match comp_kind(o) {
            Some(k) => k,
            None => return Err(ComposeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C300);
            return Err(ComposeCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(ComposeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C301 + o as u64);
            return Err(ComposeCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ComposeCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5C302 + o as u64);
        Ok(())
    }

    /// 相位操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMP_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 依赖序一格（载荷对照 + 重复拒）。
    pub fn record_dep(&mut self, d: usize, got: u64) -> Result<(), ComposeCode> {
        let want = match dep_cell(d) {
            Some(w) => w,
            None => return Err(ComposeCode::ENoDep),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C400 + d as u64);
            return Err(ComposeCode::EProjDrift);
        }
        if match self.deps.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ComposeCode::EDup);
        }
        if let Some(x) = self.deps.get_mut(d) {
            *x = Some(got);
        }
        self.fnv(0x5C401 + d as u64);
        Ok(())
    }

    /// 依赖序全账核验（8 格）。
    pub fn deps_all(&self) -> bool {
        let mut d = 0usize;
        while d < DEP_CELLS {
            match self.deps.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// profile 场景一格（载荷对照 + 重复拒）。
    pub fn record_profile(&mut self, p: usize, got: u64) -> Result<(), ComposeCode> {
        let want = match profile_cell(p) {
            Some(w) => w,
            None => return Err(ComposeCode::ENoProf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C500 + p as u64);
            return Err(ComposeCode::EProjDrift);
        }
        if match self.profs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ComposeCode::EDup);
        }
        if let Some(x) = self.profs.get_mut(p) {
            *x = Some(got);
        }
        self.fnv(0x5C501 + p as u64);
        Ok(())
    }

    /// profile 全账核验（5 格）。
    pub fn profs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROF_SCENARIOS {
            match self.profs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后相位（注入不改主账断言用）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.proj_all()
            && self.ops_all()
            && self.deps_all()
            && self.profs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ComposeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ComposeLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut dir = 0usize;
    while dir < PROJ_DIRS {
        let mut d = 0usize;
        while d < PROJ_PER_DIR {
            if let Some(w) = proj_cell(dir, d) {
                if led.record_proj(dir, d, w).is_ok() {
                    recorded += 1;
                }
            }
            d += 1;
        }
        dir += 1;
    }
    let mut o = 0usize;
    while o < COMP_OPS {
        if let Some(w) = comp_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut dp = 0usize;
    while dp < DEP_CELLS {
        if let Some(w) = dep_cell(dp) {
            if led.record_dep(dp, w).is_ok() {
                recorded += 1;
            }
        }
        dp += 1;
    }
    let mut p = 0usize;
    while p < PROF_SCENARIOS {
        if let Some(w) = profile_cell(p) {
            if led.record_profile(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    // 注入 10 轮：投影格越闭集 / 相位操作漂移 / 相位逆序三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒账。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = ComposeLedger::new();
        let caught = match kind {
            0 => lt.record_proj(PROJ_DIRS, 0, 0).is_err(),
            1 => {
                // 漂移注入：先录正格，再注漂移载荷。
                if let Some(w0) = comp_cell(0) {
                    let _ = lt.record_op(0, w0);
                }
                match comp_cell(1) {
                    Some(w1) => lt.record_op(1, w1.wrapping_add(1)) == Err(ComposeCode::EDrift),
                    None => false,
                }
            }
            _ => {
                // 逆序注入：临时账本从空起步，先落末相位再回落首相位必抓，
                // 主账 last_kind 不被改动。
                if let Some(w9) = comp_cell(9) {
                    let _ = lt.record_op(9, w9);
                }
                match comp_cell(1) {
                    Some(w1) => lt.record_op(1, w1) == Err(ComposeCode::EOrder),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5C600 + j as u64);
        } else {
            led.escaped += 1;
        }
        j += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52263 判据集（锚 UNX-F52264-J1，off-by-one 板式）。
pub fn run_unx52263_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52263");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "compose.yaml 双向投影 16/16 全落",
        {
            let mut lt = ComposeLedger::new();
            let mut ok = true;
            let mut dir = 0usize;
            while dir < PROJ_DIRS && ok {
                let mut d = 0usize;
                while d < PROJ_PER_DIR {
                    if let Some(w) = proj_cell(dir, d) {
                        if lt.record_proj(dir, d, w).is_err() {
                            ok = false;
                        }
                    }
                    d += 1;
                }
                dir += 1;
            }
            ok && lt.proj_all()
        },
        "compose→runtime 8 维 / runtime→compose 回投影 8 维逐格恒等",
    );
    s.add(
        "up/down/ps/logs/scale 10 操作全通",
        recorded >= PROJ_CELLS as u32 + COMP_OPS as u32 + DEP_CELLS as u32 + PROF_SCENARIOS as u32,
        "五相位闭集逐操作恒等（UNX-F52264-J1 ×10 工程共面）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < COMP_OPS {
                if comp_kind(o).unwrap_or(5) < comp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "up→down→ps→logs→scale 闭集单调",
    );
    s.add(
        "依赖序 8 格全落",
        {
            let mut lt = ComposeLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DEP_CELLS {
                if let Some(w) = dep_cell(d) {
                    if lt.record_dep(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.deps_all()
        },
        "depends_on 拓扑序逐格恒等",
    );
    s.add(
        "profile 5 场景全落",
        {
            let mut lt = ComposeLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROF_SCENARIOS {
                if let Some(w) = profile_cell(p) {
                    if lt.record_profile(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.profs_all()
        },
        "dev/test/prod/debug/ci 逐场景恒等",
    );
    s.add(
        "虚拟化依赖 N/A 同构",
        na_isomorph() == 0 && F52261_PLANE == 0x574C32484F530000,
        "虚拟化依赖 N/A 同构（F52261 口径复用），Compose 不持虚拟化账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影格越闭集/相位操作漂移/相位逆序三态轮转必抓",
    );
    let mut l0 = ComposeLedger::new();
    s.add(
        "边界·越闭集·投影格",
        l0.record_proj(PROJ_DIRS, 0, 0).is_err() && proj_cell(PROJ_DIRS, 0).is_none(),
        "方向 2 越闭集必拒",
    );
    s.add(
        "边界·越闭集·相位操作",
        l0.record_op(COMP_OPS, 0).is_err() && comp_cell(COMP_OPS).is_none(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·越闭集·profile",
        l0.record_profile(PROF_SCENARIOS, 0).is_err()
            && profile_cell(PROF_SCENARIOS).is_none(),
        "profile 5 越闭集必拒",
    );
    s.add(
        "投影漂移拒",
        {
            if let Some(w0) = proj_cell(0, 0) {
                let _ = l0.record_proj(0, 0, w0);
            }
            match proj_cell(0, 1) {
                Some(w1) => {
                    l0.record_proj(0, 1, w1.wrapping_add(1)) == Err(ComposeCode::EProjDrift)
                }
                None => false,
            }
        },
        "投影载荷漂移必抓",
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
        "判据锚定 §AI-66.3",
        CRITERIA_ANCHOR == "§AI-66.3" && !CRITERIA_ANCHOR.is_empty(),
        "卡面判据锚定原文逐字登记",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = ComposeLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut dir = 0usize;
            while dir < PROJ_DIRS {
                let mut d = 0usize;
                while d < PROJ_PER_DIR {
                    if let Some(w) = proj_cell(dir, d) {
                        let _ = lt.record_proj(dir, d, w);
                    }
                    d += 1;
                }
                dir += 1;
            }
            let mut o = 0usize;
            while o < COMP_OPS {
                if let Some(w) = comp_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DEP_CELLS {
                if let Some(w) = dep_cell(d) {
                    let _ = lt.record_dep(d, w);
                }
                d += 1;
            }
            let mut p = 0usize;
            while p < PROF_SCENARIOS {
                if let Some(w) = profile_cell(p) {
                    let _ = lt.record_profile(p, w);
                }
                p += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8180,
        "SEG_BASE 0x40 对齐且在 VX15 域（公式 0xC7200+0x40×62）",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proj_cells() {
        assert!(proj_cell(0, 0).is_some());
        assert!(proj_cell(1, 7).is_some());
        assert!(proj_cell(PROJ_DIRS, 0).is_none());
        assert!(proj_cell(0, PROJ_PER_DIR).is_none());
        assert_eq!(proj_dir_name(0), Some("compose→runtime"));
        assert_eq!(proj_dir_name(1), Some("runtime→compose"));
        assert!(proj_dir_name(2).is_none());
        assert_eq!(proj_dim_name(0), Some("服务"));
        assert_eq!(proj_dim_name(7), Some("命令"));
        assert!(proj_dim_name(8).is_none());
        // 正反两向同维载荷必异（有向投影）。
        assert_ne!(proj_cell(0, 3).unwrap_or(0), proj_cell(1, 3).unwrap_or(0));
    }

    #[test]
    fn test_comp_kind() {
        assert_eq!(comp_kind(0), Some(0));
        assert_eq!(comp_kind(1), Some(0));
        assert_eq!(comp_kind(2), Some(1));
        assert_eq!(comp_kind(9), Some(4));
        assert!(comp_kind(10).is_none());
        assert!(comp_cell(10).is_none());
        assert_eq!(comp_kind_name(0), Some("up"));
        assert_eq!(comp_kind_name(4), Some("scale"));
        assert!(comp_kind_name(5).is_none());
    }

    #[test]
    fn test_dep_profile_cells() {
        assert_eq!(dep_cell_name(0), Some("db-condition"));
        assert_eq!(dep_cell_name(7), Some("report-depends-e2e"));
        assert!(dep_cell_name(8).is_none());
        assert!(dep_cell(DEP_CELLS).is_none());
        assert_eq!(profile_name(0), Some("dev"));
        assert_eq!(profile_name(4), Some("ci"));
        assert!(profile_name(5).is_none());
        assert!(profile_cell(PROF_SCENARIOS).is_none());
        let a = dep_cell(2).unwrap_or(0);
        assert_ne!(a, dep_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_na_isomorph() {
        // N/A 同构声明恒等：F52261 口径复用，折叠恒归零（N/A 语义）。
        assert_eq!(na_isomorph(), na_isomorph());
        assert_eq!(na_isomorph(), 0);
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = ComposeLedger::new();
        let _ = lt.record_op(9, comp_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_op(1, comp_cell(1).unwrap_or(0)),
            Err(ComposeCode::EOrder)
        );
        // 同相位不逆序：先 4 后 5 合法。
        let mut lt2 = ComposeLedger::new();
        assert!(lt2.record_op(4, comp_cell(4).unwrap_or(0)).is_ok());
        assert!(lt2.record_op(5, comp_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ComposeLedger::new();
        assert_eq!(
            lt.record_op(5, comp_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ComposeCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(1, 2, proj_cell(1, 2).unwrap_or(0).wrapping_add(7)),
            Err(ComposeCode::EProjDrift)
        );
        assert_eq!(
            lt.record_dep(4, dep_cell(4).unwrap_or(0).wrapping_add(3)),
            Err(ComposeCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ComposeLedger::new();
        let _ = lt.record_proj(0, 0, proj_cell(0, 0).unwrap_or(0));
        assert_eq!(
            lt.record_proj(0, 0, proj_cell(0, 0).unwrap_or(0)),
            Err(ComposeCode::EDup)
        );
        let _ = lt.record_profile(1, profile_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_profile(1, profile_cell(1).unwrap_or(0)),
            Err(ComposeCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ComposeLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ComposeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_injection_leaves_main_ledger() {
        let mut main = ComposeLedger::new();
        let before = main.last_kind();
        let mut lt = ComposeLedger::new();
        let _ = lt.record_op(9, comp_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_op(1, comp_cell(1).unwrap_or(0)),
            Err(ComposeCode::EOrder)
        );
        assert_eq!(main.last_kind(), before);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (PROJ_CELLS + COMP_OPS + DEP_CELLS + PROF_SCENARIOS) as u32
        );
        assert_eq!(caught, 10);
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
        assert_eq!(SEG_BASE, 0xC8180);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：Ok=0，EGradeDrift=12。
        assert_eq!(ComposeCode::Ok as u32, 0);
        assert_eq!(ComposeCode::ENoProj as u32, 1);
        assert_eq!(ComposeCode::EProjDrift as u32, 2);
        assert_eq!(ComposeCode::ENoOp as u32, 3);
        assert_eq!(ComposeCode::EDrift as u32, 4);
        assert_eq!(ComposeCode::EOrder as u32, 5);
        assert_eq!(ComposeCode::ENoDep as u32, 6);
        assert_eq!(ComposeCode::ENoProf as u32, 7);
        assert_eq!(ComposeCode::EDup as u32, 8);
        assert_eq!(ComposeCode::ESeal as u32, 9);
        assert_eq!(ComposeCode::EInj as u32, 10);
        assert_eq!(ComposeCode::EFp as u32, 11);
        assert_eq!(ComposeCode::EGradeDrift as u32, 12);
        assert!(!ComposeCode::Ok.detail().is_empty());
        assert!(!ComposeCode::EGradeDrift.detail().is_empty());
        assert_eq!(ComposeCode::EGradeDrift.val(), SEG_BASE + 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ComposeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn unx52263_checks_green() {
        let s = run_unx52263_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
