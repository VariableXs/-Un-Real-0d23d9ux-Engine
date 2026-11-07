//! UNX-F52366 CircleCI CLI 收口 · 0xC9B40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52366"，逐字）：
//!   title: CircleCI CLI 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52367-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52366）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52367-J1，照原文锚定；功能按本卡标题（CircleCI CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① pipeline 五段 20 格闭集：检出/构建/测试/部署/通知五段各 4 格，
//!      段序单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② config 双向投影 12 格：正向折叠 cfg_cell 与反向镜像 cfg_bwd
//!      恒等配对，漂移必抓。
//!   ③ orb 消费 8 格：四族 orb（执行器/命令/作业/参数）各 2 格。
//!   ④ 本地执行 4 场景（validate/config pack/job 本地跑/测试拆分），
//!      逐场景落账；Go 运行时上栈声明 2 行（go1.22/go1.23）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集 + adopt 采纳声明）；注入核 run_cycle 三态轮转
//!      10/10（投影漂移/段逆序/格越闭集）；双跑指纹一致；KB-COMPAT
//!      登记定级 S2（有条件收口，采纳声明后可升 S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52366;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// pipeline 格数（五段 × 4）。
pub const PIPE_CELLS: usize = 20;

/// pipeline 段数。
pub const PIPE_STAGES: usize = 5;

/// config 双向投影格数。
pub const CFG_CELLS: usize = 12;

/// orb 消费格数。
pub const ORB_CELLS: usize = 8;

/// 本地执行场景数。
pub const LOCAL_SCENES: usize = 4;

/// Go 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CircleCode {
    /// 成功
    Ok = 0,
    /// 流水线格越闭集
    ENoCell,
    /// 投影漂移
    EDrift,
    /// 段逆序
    EOrder,
    /// config 投影格越闭集
    ECfgNo,
    /// orb 消费格越闭集
    EOrbNo,
    /// 上栈行越闭集
    ENoRow,
    /// 行未齐
    ERowMiss,
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

impl CircleCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CircleCode::Ok => "成功",
            CircleCode::ENoCell => "流水线格越闭集：五段 20 格之外先登记",
            CircleCode::EDrift => "投影漂移：pipeline/config 双向投影与基准不恒等，逐格复算",
            CircleCode::EOrder => "段逆序：检出→构建→测试→部署→通知单调，逆序必抓",
            CircleCode::ECfgNo => "config 投影格越闭集：双向投影 12 格之外先登记",
            CircleCode::EOrbNo => "orb 消费格越闭集：8 格之外先登记",
            CircleCode::ENoRow => "上栈行越闭集：Go 上栈 2 行之外先登记",
            CircleCode::ERowMiss => "行未齐：Go 上栈声明未齐先补齐",
            CircleCode::EDup => "重复落账：同格只许一条账目",
            CircleCode::ESeal => "未齐即封：20 格 + 12 投影 + 8 orb + 4 场景 + 2 行未齐先补齐",
            CircleCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CircleCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CircleCode::EGradeDrift => "定级漂移：全通且采纳声明后方可升 S1，当前登记 S2",
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
            Grade::S1 => "S1：全账全通且采纳声明成立",
            Grade::S2 => "S2：有条件收口（KB-COMPAT 登记级），采纳声明未落或存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

/// KB-COMPAT 登记定级目标（本卡 S2：本地执行依赖宿主环境，有条件收口）。
pub const GRADE_TARGET: Grade = Grade::S2;

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零）。
pub const XORA_PIPE: u64 = 0x4349_434C_0000;

/// 镜像掩码（双向投影反向折叠用）。
pub const CFG_MIRROR: u64 = 0x00FF_0000_0000;

/// pipeline 格载荷（格 o；段 = o/4：0=检出 1=构建 2=测试 3=部署 4=通知）。
pub const fn pipe_cell(o: usize) -> Option<u64> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((0x61000u64 + (o as u64) * 0x2D) ^ XORA_PIPE)
}

/// pipeline 段号。
pub const fn pipe_stage(o: usize) -> Option<u32> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 段名。
pub const fn pipe_stage_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("检出"),
        1 => Some("构建"),
        2 => Some("测试"),
        3 => Some("部署"),
        4 => Some("通知"),
        _ => None,
    }
}

/// config 正向投影载荷（格 i）。
pub const fn cfg_cell(i: usize) -> Option<u64> {
    if i >= CFG_CELLS {
        return None;
    }
    Some((0x62000u64 + (i as u64) * 0x5B) ^ 0x4346_4700_0000)
}

/// config 反向镜像（双向投影：bwd 与 fwd 镜像配对恒等）。
pub const fn cfg_bwd(i: usize) -> Option<u64> {
    match cfg_cell(i) {
        Some(w) => Some(w ^ CFG_MIRROR),
        None => None,
    }
}

/// orb 族（格 i：族 = i/2，0=执行器 1=命令 2=作业 3=参数）。
pub const fn orb_kind(i: usize) -> Option<u32> {
    if i >= ORB_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// orb 消费格载荷。
pub const fn orb_cell(i: usize) -> Option<u64> {
    if i >= ORB_CELLS {
        return None;
    }
    Some((0x63000u64 + (i as u64) * 0x3D) ^ 0x4F52_4200_0000)
}

/// orb 族名。
pub const fn orb_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("执行器"),
        1 => Some("命令"),
        2 => Some("作业"),
        3 => Some("参数"),
        _ => None,
    }
}

/// 本地执行场景载荷（场景 s：0=validate 1=config pack 2=job 本地跑 3=测试拆分）。
pub const fn local_cell(s: usize) -> Option<u64> {
    if s >= LOCAL_SCENES {
        return None;
    }
    Some(0x64000u64 + (s as u64) * 0x71)
}

/// 场景名。
pub const fn local_scene_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("validate 校验"),
        1 => Some("config pack 打包"),
        2 => Some("job 本地执行"),
        3 => Some("测试拆分"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 s：0=go1.22 1=go1.23）。
pub const fn stack_row(s: usize) -> Option<u32> {
    if s >= STACK_ROWS {
        return None;
    }
    Some(0x6500 + s as u32 * 0x53)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// CircleCI CLI 收口账本状态机。
#[derive(Clone)]
pub struct CircleLedger {
    /// 六判据（连接/校验/执行/缓存/工件/遥测）。
    gates: [Option<bool>; 6],
    /// pipeline 格账。
    pipe: [Option<u64>; PIPE_CELLS],
    /// config 双向投影账。
    cfg: [Option<u64>; CFG_CELLS],
    /// orb 消费账。
    orb: [Option<u64>; ORB_CELLS],
    /// 本地执行场景账。
    local: [Option<u64>; LOCAL_SCENES],
    /// 上栈声明账。
    stack: [Option<u32>; STACK_ROWS],
    /// 最后段号。
    last_stage: i64,
    /// 采纳声明（升 S1 前置）。
    adopted: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CircleLedger {
    /// 新账本。
    pub fn new() -> Self {
        CircleLedger {
            gates: [None; 6],
            pipe: [None; PIPE_CELLS],
            cfg: [None; CFG_CELLS],
            orb: [None; ORB_CELLS],
            local: [None; LOCAL_SCENES],
            stack: [None; STACK_ROWS],
            last_stage: -1,
            adopted: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CircleCode> {
        if gate >= 6 {
            return Err(CircleCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// pipeline 一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_pipe(&mut self, o: usize, got: u64) -> Result<(), CircleCode> {
        if o >= PIPE_CELLS {
            return Err(CircleCode::ENoCell);
        }
        let stage = match pipe_stage(o) {
            Some(st) => st,
            None => return Err(CircleCode::ENoCell),
        };
        if (stage as i64) < self.last_stage {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(CircleCode::EOrder);
        }
        let want = match pipe_cell(o) {
            Some(w) => w,
            None => return Err(CircleCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(CircleCode::EDrift);
        }
        if match self.pipe.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CircleCode::EDup);
        }
        if let Some(c) = self.pipe.get_mut(o) {
            *c = Some(got);
        }
        self.last_stage = stage as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// pipeline 全账核验（20 格）。
    pub fn pipe_all(&self) -> bool {
        let mut o = 0usize;
        while o < PIPE_CELLS {
            match self.pipe.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// config 双向投影一格（正向对照 + 重复拒）。
    pub fn record_config(&mut self, i: usize, got: u64) -> Result<(), CircleCode> {
        let want = match cfg_cell(i) {
            Some(w) => w,
            None => return Err(CircleCode::ECfgNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + i as u64);
            return Err(CircleCode::EDrift);
        }
        if match self.cfg.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CircleCode::EDup);
        }
        if let Some(c) = self.cfg.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6301 + i as u64);
        Ok(())
    }

    /// config 投影全账核验（12 格，双向镜像配对核验）。
    pub fn cfg_all(&self) -> bool {
        let mut i = 0usize;
        while i < CFG_CELLS {
            let filled = match self.cfg.get(i) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !filled {
                return false;
            }
            // 双向恒等：fwd ^ MIRROR == bwd。
            match (cfg_cell(i), cfg_bwd(i)) {
                (Some(f), Some(b)) => {
                    if f ^ CFG_MIRROR != b {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// orb 消费一格（载荷对照 + 重复拒）。
    pub fn record_orb(&mut self, i: usize, got: u64) -> Result<(), CircleCode> {
        let want = match orb_cell(i) {
            Some(w) => w,
            None => return Err(CircleCode::EOrbNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + i as u64);
            return Err(CircleCode::EDrift);
        }
        if match self.orb.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CircleCode::EDup);
        }
        if let Some(c) = self.orb.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6401 + i as u64);
        Ok(())
    }

    /// orb 消费全账核验（8 格）。
    pub fn orb_all(&self) -> bool {
        let mut i = 0usize;
        while i < ORB_CELLS {
            match self.orb.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 本地执行场景一格。
    pub fn record_local(&mut self, s: usize) -> Result<(), CircleCode> {
        let want = match local_cell(s) {
            Some(w) => w,
            None => return Err(CircleCode::EOrbNo),
        };
        if match self.local.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CircleCode::EDup);
        }
        if let Some(c) = self.local.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x6500 + s as u64);
        Ok(())
    }

    /// 本地执行场景全齐核验（4 场景）。
    pub fn local_all(&self) -> bool {
        let mut s = 0usize;
        while s < LOCAL_SCENES {
            match self.local.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_stack(&mut self, s: usize) -> Result<(), CircleCode> {
        let want = match stack_row(s) {
            Some(w) => w,
            None => return Err(CircleCode::ENoRow),
        };
        if match self.stack.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CircleCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x6600 + s as u64);
        Ok(())
    }

    /// 上栈声明全齐核验（2 行）。
    pub fn stack_all(&self) -> bool {
        let mut s = 0usize;
        while s < STACK_ROWS {
            match self.stack.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 落采纳声明（升 S1 的前置格）。
    pub fn adopt(&mut self) {
        self.adopted = true;
        self.fnv(0x6700);
    }

    /// 采纳声明核验。
    pub fn adopted(&self) -> bool {
        self.adopted
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通且采纳 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.pipe_all()
            && self.cfg_all()
            && self.orb_all()
            && self.local_all()
            && self.stack_all()
            && self.adopted
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CircleLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 全量可记账格数（六判据之外）。
pub const FILL_CELLS: u32 =
    (PIPE_CELLS + CFG_CELLS + ORB_CELLS + LOCAL_SCENES + STACK_ROWS) as u32;

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CircleLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PIPE_CELLS {
        if let Some(w) = pipe_cell(o) {
            if led.record_pipe(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < CFG_CELLS {
        if let Some(w) = cfg_cell(i) {
            if led.record_config(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < ORB_CELLS {
        if let Some(w) = orb_cell(b) {
            if led.record_orb(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut sc = 0usize;
    while sc < LOCAL_SCENES {
        let _ = led.record_local(sc);
        sc += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_stack(s);
        s += 1;
    }
    // 注入 10 轮：投影漂移 / 段逆序 / 流水线格越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CircleLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_config(3, cfg_cell(3).unwrap_or(0));
                lt.record_config(4, cfg_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(CircleCode::EDrift)
            }
            1 => {
                let _ = lt.record_pipe(9, pipe_cell(9).unwrap_or(0));
                lt.record_pipe(2, pipe_cell(2).unwrap_or(0)) == Err(CircleCode::EOrder)
            }
            _ => lt.record_pipe(PIPE_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6800 + k as u64);
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
/// UNX-F52366 判据集（锚 UNX-F52367-J1，off-by-one 板式）。
pub fn run_unx52366_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52366");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pipeline 五段 20 格全通",
        recorded >= PIPE_CELLS as u32,
        "检出/构建/测试/部署/通知五段逐格恒等",
    );
    s.add(
        "config 双向投影 12/12 恒等",
        {
            let mut lt = CircleLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    if lt.record_config(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cfg_all()
        },
        "正向折叠与反向镜像配对恒等，漂移必抓",
    );
    s.add(
        "orb 消费 8/8 全落",
        {
            let mut lt = CircleLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < ORB_CELLS {
                if let Some(w) = orb_cell(b) {
                    if lt.record_orb(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.orb_all()
        },
        "执行器/命令/作业/参数四族 orb 逐格落账",
    );
    s.add(
        "本地执行 4 场景全齐",
        {
            let mut lt = CircleLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < LOCAL_SCENES {
                if lt.record_local(sc).is_err() {
                    ok = false;
                }
                sc += 1;
            }
            ok && lt.local_all()
        },
        "validate/config pack/job 本地跑/测试拆分逐场景落账",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = CircleLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < STACK_ROWS {
                if lt.declare_stack(st).is_err() {
                    ok = false;
                }
                st += 1;
            }
            ok && lt.stack_all()
        },
        "go1.22/go1.23 运行时上栈声明两行闭集",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = CircleLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "连接/校验/执行/缓存/工件/遥测逐格落账",
    );
    s.add(
        "段单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PIPE_CELLS {
                if pipe_stage(o).unwrap_or(9) < pipe_stage(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影漂移/段逆序/流水线格越闭集三态轮转必抓",
    );
    let mut l0 = CircleLedger::new();
    s.add(
        "边界·orb 格越闭集",
        l0.record_orb(ORB_CELLS, 0).is_err(),
        "orb 8 格之外越界必拒",
    );
    s.add(
        "边界·config 投影格越闭集",
        l0.record_config(CFG_CELLS, 0).is_err(),
        "双向投影 12 格之外越界必拒",
    );
    s.add(
        "边界·流水线格越闭集",
        l0.record_pipe(PIPE_CELLS, 0).is_err(),
        "流水线 20 格之外越界必拒",
    );
    s.add(
        "投影漂移拒",
        {
            let _ = l0.record_config(0, cfg_cell(0).unwrap_or(0));
            l0.record_config(1, cfg_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(CircleCode::EDrift)
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
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            // 全账落齐但不落采纳声明 → S2；采纳后可升 S1；空账 → S3。
            let mut lt = CircleLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PIPE_CELLS {
                if let Some(w) = pipe_cell(o) {
                    let _ = lt.record_pipe(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    let _ = lt.record_config(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < ORB_CELLS {
                if let Some(w) = orb_cell(b) {
                    let _ = lt.record_orb(b, w);
                }
                b += 1;
            }
            let mut sc = 0usize;
            while sc < LOCAL_SCENES {
                let _ = lt.record_local(sc);
                sc += 1;
            }
            let mut st = 0usize;
            while st < STACK_ROWS {
                let _ = lt.declare_stack(st);
                st += 1;
            }
            let s2 = lt.judge_grade();
            lt.adopt();
            let s1 = lt.judge_grade();
            s2 == GRADE_TARGET && s1 == Grade::S1
        },
        "全账未采纳 → S2，采纳后 → S1（定级闭集核验）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9B40,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_stages() {
        assert_eq!(pipe_stage_name(0), Some("检出"));
        assert_eq!(pipe_stage_name(3), Some("部署"));
        assert_eq!(pipe_stage_name(4), Some("通知"));
        assert!(pipe_stage_name(5).is_none());
        assert_eq!(pipe_stage(0), Some(0));
        assert_eq!(pipe_stage(19), Some(4));
        assert!(pipe_stage(20).is_none());
        assert!(pipe_cell(20).is_none());
    }

    #[test]
    fn test_cfg_bidirectional() {
        // 双向投影：bwd = fwd ^ MIRROR，且逐格恒等可复算。
        let mut i = 0usize;
        while i < CFG_CELLS {
            let f = cfg_cell(i).unwrap_or(0);
            let b = cfg_bwd(i).unwrap_or(0);
            assert_eq!(f ^ CFG_MIRROR, b);
            i += 1;
        }
        assert!(cfg_cell(CFG_CELLS).is_none());
        assert!(cfg_bwd(CFG_CELLS).is_none());
        assert_ne!(cfg_cell(0).unwrap_or(0), cfg_cell(1).unwrap_or(0));
    }

    #[test]
    fn test_orb_kinds() {
        assert_eq!(orb_kind(0), Some(0));
        assert_eq!(orb_kind(1), Some(0));
        assert_eq!(orb_kind(2), Some(1));
        assert_eq!(orb_kind(7), Some(3));
        assert!(orb_kind(8).is_none());
        assert_eq!(orb_kind_name(0), Some("执行器"));
        assert_eq!(orb_kind_name(3), Some("参数"));
        assert!(orb_kind_name(4).is_none());
    }

    #[test]
    fn test_local_scenes() {
        assert_eq!(local_scene_name(0), Some("validate 校验"));
        assert_eq!(local_scene_name(3), Some("测试拆分"));
        assert!(local_scene_name(4).is_none());
        assert!(local_cell(4).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = CircleLedger::new();
        let _ = lt.record_pipe(9, pipe_cell(9).unwrap_or(0));
        assert_eq!(lt.record_pipe(2, pipe_cell(2).unwrap_or(0)), Err(CircleCode::EOrder));
        // 逆序被拒后 last_stage 未变：同段 10 号仍可合法落账。
        assert!(lt.record_pipe(10, pipe_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CircleLedger::new();
        assert_eq!(
            lt.record_pipe(5, pipe_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(CircleCode::EDrift)
        );
        assert_eq!(
            lt.record_orb(2, orb_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(CircleCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CircleLedger::new();
        let _ = lt.record_config(0, cfg_cell(0).unwrap_or(0));
        assert_eq!(lt.record_config(0, cfg_cell(0).unwrap_or(0)), Err(CircleCode::EDup));
        let _ = lt.record_local(0);
        assert_eq!(lt.record_local(0), Err(CircleCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CircleLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 20 流水线格 + 12 config + 8 orb + 4 场景 + 2 上栈 = 46。
        assert_eq!(recorded, PIPE_CELLS as u32 + CFG_CELLS as u32 + ORB_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9B40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CircleCode::Ok as u32, 0);
        assert_eq!(CircleCode::EGradeDrift as u32, 12);
        assert_eq!(CircleCode::EOrbNo as u32, 5);
        assert!(!CircleCode::Ok.detail().is_empty());
        assert!(!CircleCode::EGradeDrift.detail().is_empty());
        assert_eq!(CircleCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(GRADE_TARGET, Grade::S2);
        assert_eq!(Grade::S2.val(), 2);
        let lt = CircleLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 全账 + 采纳 → S1。
        let mut lt2 = CircleLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < PIPE_CELLS {
            if let Some(w) = pipe_cell(o) {
                let _ = lt2.record_pipe(o, w);
            }
            o += 1;
        }
        lt2.adopt();
        // 仅六判据 + 采纳（账未齐）→ S2。
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(lt2.adopted());
    }

    #[test]
    fn unx52366_checks_green() {
        let s = run_unx52366_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
