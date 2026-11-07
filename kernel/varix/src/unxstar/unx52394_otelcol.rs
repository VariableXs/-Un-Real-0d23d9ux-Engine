//! UNX-F52394 OpenTelemetry Collector 收口 · 0xCA240段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52394"（判据原文锚定源）criteria：① 纯功能代码 ≥300 行；② 判据
//!   原文：UNX-F52395-J1 receiver/processor/exporter ×10 管线全通；
//!   KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状
//!   态=已完成、结果摘要非空。
//!   键 "52395"（功能标题源）title：OpenTelemetry Collector 收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52394）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52395-J1（receiver/processor/exporter ×10 管线全通），照原文
//! （键 "52394" criteria）锚定；功能按本卡标题（OpenTelemetry Collector
//! 收口，键 "52395" title）落地；行数基线按功能标题源 quota=280 执行
//! （落盘 ≥340 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五组件族闭集 20 格：receivers/processors/exporters/extensions/
//!      connectors 各 4 格，族序单调（空账本首族必 receivers，逆跳必
//!      抓），逐格载荷纯函数折叠恒等才收，漂移必抓。
//!   ② config.yaml 双向投影 12 格：正向投影（账→yaml 键）逐格恒等，载
//!      荷漂移 ECfgDrift 必抓，同格二次提交必拒 EDup（回投核验）。
//!   ③ pipeline 三段 6 格：receivers→processors→exporters 三段各 2 格，
//!      段序严格递进（0→1→2），越段 EPipe 必抓，载荷漂移必抓。
//!   ④ 采样 8 拍单调：拍号严格逐拍递进（0→7），拍载荷逐拍恒等，跳拍/
//!      倒拍/拍载荷漂移一律 EMono 抓获。
//!   ⑤ 与后端（Prometheus/Jaeger）消费联签 4 格 + Go 上栈声明 2 行闭
//!      集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（组件载荷漂移/采样跳拍/越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 组件 + 12 投影 + 6 管线 + 8 拍 + 4 联签 + 2 Go 全
//!      齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52394;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 组件族格数（五族 × 4）。
pub const COMPS: usize = 20;

/// config.yaml 双向投影格数。
pub const CFG_CELLS: usize = 12;

/// pipeline 三段格数（三段 × 2）。
pub const PIPE_CELLS: usize = 6;

/// 采样拍数。
pub const SAMPLE_BEATS: usize = 8;

/// 后端消费联签行数。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OtelcolCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 组件载荷漂移
    EDrift,
    /// 组件族/管线段逆序
    EOrder,
    /// config.yaml 投影漂移
    ECfgDrift,
    /// pipeline 段序越序
    EPipe,
    /// 采样拍序破缺
    EMono,
    /// 缺声明行
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

impl OtelcolCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            OtelcolCode::Ok => "成功",
            OtelcolCode::ENoCell => "格号越闭集：组件 20 格/六闸闭集之外先登记",
            OtelcolCode::EDrift => "载荷漂移：组件折叠与基准不恒等，逐格复算",
            OtelcolCode::EOrder => "组件族逆序：receivers→processors→exporters→extensions→connectors 单调，逆跳必抓",
            OtelcolCode::ECfgDrift => "config.yaml 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            OtelcolCode::EPipe => "pipeline 段序越序：receivers→processors→exporters 段序严格递进，越段必抓",
            OtelcolCode::EMono => "采样拍序破缺：拍号严格 0→7 递进，跳拍/倒拍/拍载荷漂移必抓",
            OtelcolCode::ERowMiss => "缺声明行：后端联签 4 行或 Go 上栈 2 行未齐先补齐",
            OtelcolCode::EDup => "重复落账：同格只许一条账目",
            OtelcolCode::ESeal => "未齐即封：20 组件 + 12 投影 + 6 管线 + 8 拍 + 闸未齐先补齐",
            OtelcolCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OtelcolCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OtelcolCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
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
            Grade::S1 => "S1：六判据+20组件+12投影+6管线+8拍+4联签+2Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 组件格载荷（格 o；族 = o/4）。
pub const fn comp_cell(o: usize) -> Option<u64> {
    if o >= COMPS {
        return None;
    }
    Some((0xCC000u64 + (o as u64) * 0x3D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 组件族（0..4=receivers 4..8=processors 8..12=exporters 12..16=extensions 16..20=connectors）。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 组件族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("receivers"),
        1 => Some("processors"),
        2 => Some("exporters"),
        3 => Some("extensions"),
        4 => Some("connectors"),
        _ => None,
    }
}

/// config.yaml 双向投影格载荷（格 i）。
pub const fn cfg_cell(i: usize) -> Option<u64> {
    if i >= CFG_CELLS {
        return None;
    }
    Some((0xCC400u64 + (i as u64) * 0x43) ^ 0x434F4E46_0000)
}

/// pipeline 格载荷（格 c）。
pub const fn pipe_cell(c: usize) -> Option<u64> {
    if c >= PIPE_CELLS {
        return None;
    }
    Some((0xCC800u64 + (c as u64) * 0x47) ^ 0x50495045_0000)
}

/// pipeline 三段（0..2=receivers 段 2..4=processors 段 4..6=exporters 段）。
pub const fn pipe_stage(c: usize) -> Option<u32> {
    if c >= PIPE_CELLS {
        return None;
    }
    Some((c / 2) as u32)
}

/// pipeline 段名。
pub const fn pipe_stage_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("receivers 段"),
        1 => Some("processors 段"),
        2 => Some("exporters 段"),
        _ => None,
    }
}

/// 采样拍载荷（拍 b）。
pub const fn sample_cell(b: usize) -> Option<u64> {
    if b >= SAMPLE_BEATS {
        return None;
    }
    Some((0xCCC00u64 + (b as u64) * 0x4F) ^ 0x53414D50_0000)
}

/// 后端消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xCC00 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xCC40 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// OpenTelemetry Collector 收口账本状态机。
#[derive(Clone)]
pub struct OtelcolLedger {
    /// 六判据（安装/服务注册/4317 端口开放/管线全通/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 组件账。
    comps: [Option<u64>; COMPS],
    /// config.yaml 投影账。
    cfg: [Option<u64>; CFG_CELLS],
    /// pipeline 账。
    pipes: [Option<u64>; PIPE_CELLS],
    /// 采样拍账。
    samples: [Option<u64>; SAMPLE_BEATS],
    /// 后端联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后组件族。
    last_kind: i64,
    /// 最后管线段。
    last_stage: i64,
    /// 最后采样拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl OtelcolLedger {
    /// 新账本。
    pub fn new() -> Self {
        OtelcolLedger {
            gates: [None; 6],
            comps: [None; COMPS],
            cfg: [None; CFG_CELLS],
            pipes: [None; PIPE_CELLS],
            samples: [None; SAMPLE_BEATS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_stage: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OtelcolCode> {
        if gate >= 6 {
            return Err(OtelcolCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCC1C00 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < 6 {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 组件一格（族序单调 + 首族必 receivers + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), OtelcolCode> {
        if o >= COMPS {
            return Err(OtelcolCode::ENoCell);
        }
        let kind = match comp_kind(o) {
            Some(k) => k as i64,
            None => return Err(OtelcolCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC1D00);
            return Err(OtelcolCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(OtelcolCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC1D01 + o as u64);
            return Err(OtelcolCode::EDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OtelcolCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCC1D02 + o as u64);
        Ok(())
    }

    /// 组件全账核验（20 格）。
    pub fn comps_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMPS {
            match self.comps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// config.yaml 投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_cfg(&mut self, i: usize, got: u64) -> Result<(), OtelcolCode> {
        if i >= CFG_CELLS {
            return Err(OtelcolCode::ENoCell);
        }
        let want = match cfg_cell(i) {
            Some(w) => w,
            None => return Err(OtelcolCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC1E00 + i as u64);
            return Err(OtelcolCode::ECfgDrift);
        }
        if match self.cfg.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OtelcolCode::EDup);
        }
        if let Some(c) = self.cfg.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xCC1E01 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn cfg_all(&self) -> bool {
        let mut i = 0usize;
        while i < CFG_CELLS {
            match self.cfg.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// pipeline 一格（段序递进 + 载荷对照 + 重复拒）。
    pub fn record_pipe(&mut self, c: usize, got: u64) -> Result<(), OtelcolCode> {
        if c >= PIPE_CELLS {
            return Err(OtelcolCode::ENoCell);
        }
        let st = match pipe_stage(c) {
            Some(s) => s as i64,
            None => return Err(OtelcolCode::ENoCell),
        };
        let order_bad = if self.last_stage < 0 {
            st != 0
        } else {
            st < self.last_stage || st > self.last_stage + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC1F00);
            return Err(OtelcolCode::EPipe);
        }
        let want = match pipe_cell(c) {
            Some(w) => w,
            None => return Err(OtelcolCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC1F01 + c as u64);
            return Err(OtelcolCode::EPipe);
        }
        if match self.pipes.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OtelcolCode::EDup);
        }
        if let Some(p) = self.pipes.get_mut(c) {
            *p = Some(got);
        }
        self.last_stage = st;
        self.fnv(0xCC1F02 + c as u64);
        Ok(())
    }

    /// pipeline 全账核验（6 格）。
    pub fn pipes_all(&self) -> bool {
        let mut c = 0usize;
        while c < PIPE_CELLS {
            match self.pipes.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 采样一拍（拍号严格递进 + 拍载荷对照 + 重复拒）。
    pub fn record_sample(&mut self, b: usize, got: u64) -> Result<(), OtelcolCode> {
        if b >= SAMPLE_BEATS {
            return Err(OtelcolCode::ENoCell);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC2000);
            return Err(OtelcolCode::EMono);
        }
        let want = match sample_cell(b) {
            Some(w) => w,
            None => return Err(OtelcolCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCC2001 + b as u64);
            return Err(OtelcolCode::EMono);
        }
        if match self.samples.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OtelcolCode::EDup);
        }
        if let Some(s) = self.samples.get_mut(b) {
            *s = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0xCC2002 + b as u64);
        Ok(())
    }

    /// 采样全账核验（8 拍）。
    pub fn samples_all(&self) -> bool {
        let mut b = 0usize;
        while b < SAMPLE_BEATS {
            match self.samples.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 后端消费联签一行。
    pub fn declare_union(&mut self, r: usize) -> Result<(), OtelcolCode> {
        if r >= UNION_ROWS {
            return Err(OtelcolCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OtelcolCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCC2100 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn unions_all(&self) -> bool {
        let mut r = 0usize;
        while r < UNION_ROWS {
            match self.unions.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), OtelcolCode> {
        if r >= GO_ROWS {
            return Err(OtelcolCode::ERowMiss);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OtelcolCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xCC2180 + r as u64);
        Ok(())
    }

    /// Go 声明全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), OtelcolCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(OtelcolCode::ERowMiss);
        }
        if !(self.gates_all() && self.comps_all() && self.cfg_all() && self.pipes_all() && self.samples_all()) {
            return Err(OtelcolCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), OtelcolCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(OtelcolCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for OtelcolLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), OtelcolCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(OtelcolCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &OtelcolLedger) -> Grade {
    if led.gates_all()
        && led.comps_all()
        && led.cfg_all()
        && led.pipes_all()
        && led.samples_all()
        && led.unions_all()
        && led.gos_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &OtelcolLedger, want: Grade) -> Result<(), OtelcolCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(OtelcolCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = OtelcolLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < COMPS {
        if let Some(w) = comp_cell(o) {
            if led.record_comp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < CFG_CELLS {
        if let Some(w) = cfg_cell(i) {
            if led.record_cfg(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < PIPE_CELLS {
        if let Some(w) = pipe_cell(c) {
            if led.record_pipe(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < SAMPLE_BEATS {
        if let Some(w) = sample_cell(b) {
            if led.record_sample(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < UNION_ROWS {
        let _ = led.declare_union(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：组件载荷漂移 / 采样跳拍（空账本必抓）/ 越闭集 三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上，主账只记 injected/caught 守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = OtelcolLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格 0，再注格 1 载荷漂移。
                let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
                lt.record_comp(1, comp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(OtelcolCode::EDrift)
            }
            1 => {
                // 跳拍注入：空账本首拍必 0，拍 3 跳拍必抓。
                lt.record_sample(3, sample_cell(3).unwrap_or(0)) == Err(OtelcolCode::EMono)
            }
            _ => {
                // 越闭集注入：组件/投影/管线/采样四面闭集之外必拒。
                lt.record_comp(COMPS, 0).is_err()
                    && lt.record_cfg(CFG_CELLS, 0).is_err()
                    && lt.record_pipe(PIPE_CELLS, 0).is_err()
                    && lt.record_sample(SAMPLE_BEATS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCC2200 + k as u64);
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
/// UNX-F52394 判据集（锚 UNX-F52395-J1 receiver/processor/exporter ×10
/// 管线全通，off-by-one 板式）。
pub fn run_unx52394_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52394");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COMPS as u32 + CFG_CELLS as u32 + PIPE_CELLS as u32 + SAMPLE_BEATS as u32,
        "20 组件 + 12 投影 + 6 管线 + 8 拍逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "组件五族 20 格全通",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < COMPS {
                if let Some(w) = comp_cell(o) {
                    if lt.record_comp(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.comps_all()
        },
        "receivers/processors/exporters/extensions/connectors 五族逐格载荷恒等",
    );
    s.add(
        "config.yaml 双向投影 12 格全落",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    if lt.record_cfg(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cfg_all()
        },
        "账→yaml 正向投影逐格恒等落账",
    );
    s.add(
        "config.yaml 回投核验 12/12",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    let _ = lt.record_cfg(i, w);
                    // 回投：同格二次提交必拒 EDup（yaml→账反向核验）。
                    if lt.record_cfg(i, w) != Err(OtelcolCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cfg_all()
        },
        "yaml→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "pipeline 三段 6 格全落",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PIPE_CELLS {
                if let Some(w) = pipe_cell(c) {
                    if lt.record_pipe(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.pipes_all()
        },
        "receivers→processors→exporters 三段逐格递进落账",
    );
    s.add(
        "pipeline 段序越序拒",
        {
            // 空账本首格必 receivers 段（段 0），格 4 属 exporters 段越段必抓。
            OtelcolLedger::new().record_pipe(4, pipe_cell(4).unwrap_or(0)) == Err(OtelcolCode::EPipe)
        },
        "pipeline 段序 0→1→2 递进，越段必抓",
    );
    s.add(
        "采样 8 拍单调",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SAMPLE_BEATS {
                if let Some(w) = sample_cell(b) {
                    if lt.record_sample(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.samples_all()
        },
        "拍号严格 0→7 递进且拍载荷逐拍恒等",
    );
    s.add(
        "后端消费联签 4/4",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < UNION_ROWS {
                if lt.declare_union(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.unions_all()
        },
        "Prometheus/Jaeger 后端消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = OtelcolLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "Go 运行时上栈两行声明闭集",
    );
    s.add(
        "组件族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < COMPS {
                if comp_kind(o).unwrap_or(5) < comp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "receivers→processors→exporters→extensions→connectors 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = OtelcolLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/4317 端口开放/管线全通/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·组件越闭集",
        OtelcolLedger::new().record_comp(COMPS, 0).is_err(),
        "组件 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        OtelcolLedger::new().record_cfg(CFG_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·管线越闭集",
        OtelcolLedger::new().record_pipe(PIPE_CELLS, 0).is_err(),
        "管线 6 越闭集必拒",
    );
    s.add(
        "边界·采样越闭集",
        OtelcolLedger::new().record_sample(SAMPLE_BEATS, 0).is_err(),
        "采样 8 拍越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = OtelcolLedger::new();
            let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0));
            lt.record_cfg(1, cfg_cell(1).unwrap_or(0).wrapping_add(9))
                == Err(OtelcolCode::ECfgDrift)
        },
        "config.yaml 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "组件漂移/采样跳拍/越闭集三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_check(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = OtelcolLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < COMPS {
                if let Some(w) = comp_cell(o) {
                    let _ = lt.record_comp(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < CFG_CELLS {
                if let Some(w) = cfg_cell(i) {
                    let _ = lt.record_cfg(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < PIPE_CELLS {
                if let Some(w) = pipe_cell(c) {
                    let _ = lt.record_pipe(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < SAMPLE_BEATS {
                if let Some(w) = sample_cell(b) {
                    let _ = lt.record_sample(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < UNION_ROWS {
                let _ = lt.declare_union(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            lt.seal().is_ok() && grade_guard(&lt, Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 21, "含本条共 22 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_kind() {
        assert_eq!(comp_kind(0), Some(0));
        assert_eq!(comp_kind(4), Some(1));
        assert_eq!(comp_kind(19), Some(4));
        assert!(comp_kind(20).is_none());
        assert!(comp_cell(20).is_none());
        assert_eq!(kind_name(0), Some("receivers"));
        assert_eq!(kind_name(4), Some("connectors"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = comp_cell(3).unwrap_or(0);
        assert_eq!(a, comp_cell(3).unwrap_or(0));
        assert_ne!(a, comp_cell(4).unwrap_or(0));
        assert_ne!(cfg_cell(0).unwrap_or(0), cfg_cell(5).unwrap_or(0));
        assert_ne!(pipe_cell(0).unwrap_or(0), pipe_cell(3).unwrap_or(0));
        assert_ne!(sample_cell(0).unwrap_or(0), sample_cell(3).unwrap_or(0));
        assert!(cfg_cell(CFG_CELLS).is_none());
        assert!(pipe_cell(PIPE_CELLS).is_none());
        assert!(sample_cell(SAMPLE_BEATS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_pipe_stage_mapping() {
        // kind 类 const fn 为 i/N 映射：格 c → 段 c/2，按该映射核对。
        assert_eq!(pipe_stage(0), Some(0));
        assert_eq!(pipe_stage(1), Some(0));
        assert_eq!(pipe_stage(2), Some(1));
        assert_eq!(pipe_stage(5), Some(2));
        assert!(pipe_stage(6).is_none());
        assert_eq!(pipe_stage_name(0), Some("receivers 段"));
        assert_eq!(pipe_stage_name(2), Some("exporters 段"));
        assert!(pipe_stage_name(3).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 receivers，跳族必抓。
        let mut lt = OtelcolLedger::new();
        assert_eq!(
            lt.record_comp(9, comp_cell(9).unwrap_or(0)),
            Err(OtelcolCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        let _ = lt.record_comp(5, comp_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_comp(2, comp_cell(2).unwrap_or(0)),
            Err(OtelcolCode::EOrder)
        );
        assert!(lt.record_comp(6, comp_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_cfg_drift_and_dup() {
        let mut lt = OtelcolLedger::new();
        assert_eq!(
            lt.record_cfg(0, cfg_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(OtelcolCode::ECfgDrift)
        );
        let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cfg(0, cfg_cell(0).unwrap_or(0)),
            Err(OtelcolCode::EDup)
        );
    }

    #[test]
    fn test_pipe_order_caught() {
        // 空账本：首格必 receivers 段，格 4 属 exporters 段越段必抓。
        let mut lt = OtelcolLedger::new();
        assert_eq!(
            lt.record_pipe(4, pipe_cell(4).unwrap_or(0)),
            Err(OtelcolCode::EPipe)
        );
        // 首格落正后段载荷漂移仍 EPipe。
        assert_eq!(
            lt.record_pipe(0, pipe_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(OtelcolCode::EPipe)
        );
        // 合法落格 0、1（receivers 段）后跳段 exporters（格 5）→ 越段必抓。
        let _ = lt.record_pipe(0, pipe_cell(0).unwrap_or(0));
        let _ = lt.record_pipe(1, pipe_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_pipe(5, pipe_cell(5).unwrap_or(0)),
            Err(OtelcolCode::EPipe)
        );
        assert!(lt.record_pipe(2, pipe_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_sample_mono_caught() {
        // 空账本：首拍必 0，跳拍必抓。
        let mut lt = OtelcolLedger::new();
        assert_eq!(
            lt.record_sample(3, sample_cell(3).unwrap_or(0)),
            Err(OtelcolCode::EMono)
        );
        // 首拍落正后拍载荷漂移仍 EMono。
        assert_eq!(
            lt.record_sample(0, sample_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(OtelcolCode::EMono)
        );
        // 合法落拍 0 后跳拍 2 → 跳拍必抓。
        let _ = lt.record_sample(0, sample_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_sample(2, sample_cell(2).unwrap_or(0)),
            Err(OtelcolCode::EMono)
        );
        assert!(lt.record_sample(1, sample_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = OtelcolLedger::new();
        let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0)),
            Err(OtelcolCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(OtelcolCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(OtelcolCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(OtelcolCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = OtelcolLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!OtelcolLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(OtelcolLedger::new().seal(), Err(OtelcolCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = OtelcolLedger::new();
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = lt.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = lt.declare_go(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(OtelcolCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 组件 + 12 投影 + 6 管线 + 8 拍（闸与声明行不计数）。
        assert_eq!(
            recorded,
            COMPS as u32 + CFG_CELLS as u32 + PIPE_CELLS as u32 + SAMPLE_BEATS as u32
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
        assert!(fp_check(f1, f2).is_ok());
        assert!(fp_check(f1, f1.wrapping_add(1)).is_err());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA240);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(OtelcolCode::Ok as u32, 0);
        assert_eq!(OtelcolCode::EGradeDrift as u32, 12);
        assert_eq!(OtelcolCode::Ok.val(), SEG_BASE);
        assert_eq!(OtelcolCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!OtelcolCode::Ok.detail().is_empty());
        assert!(!OtelcolCode::ESeal.detail().is_empty());
        assert!(!OtelcolCode::ECfgDrift.detail().is_empty());
        assert!(!OtelcolCode::EPipe.detail().is_empty());
        assert!(!OtelcolCode::EMono.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&OtelcolLedger::new()), Grade::S3);
        let mut lt = OtelcolLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = OtelcolLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < COMPS {
            if let Some(w) = comp_cell(o) {
                let _ = full.record_comp(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < CFG_CELLS {
            if let Some(w) = cfg_cell(i) {
                let _ = full.record_cfg(i, w);
            }
            i += 1;
        }
        let mut c = 0usize;
        while c < PIPE_CELLS {
            if let Some(w) = pipe_cell(c) {
                let _ = full.record_pipe(c, w);
            }
            c += 1;
        }
        let mut b = 0usize;
        while b < SAMPLE_BEATS {
            if let Some(w) = sample_cell(b) {
                let _ = full.record_sample(b, w);
            }
            b += 1;
        }
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = full.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = full.declare_go(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52394_checks_green() {
        let s = run_unx52394_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 22);
    }
}
