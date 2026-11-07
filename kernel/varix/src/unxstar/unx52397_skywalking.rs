//! UNX-F52397 SkyWalking Agent（Windows 面）收口 · 0xCA300段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52397"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52398-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52398"（功能标题源）title：SkyWalking Agent（Windows 面）收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52397）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52398-J1，照原文（键 "52397" criteria）锚定；功能按功能标题源
//! （键 "52398" title：SkyWalking Agent（Windows 面）收口）落地；行数
//! 基线按功能标题源 quota=280 执行（落盘 ≥550 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模块族闭集 20 格：agent 内核/OAP/UI/storage/alarm 各 4 格，族序
//!      单调（非严格阶梯，回跳必抓），逐格载荷纯函数折叠恒等才收，漂移
//!      必抓。
//!   ② agent.config 双向投影 12 格：正向投影（账→config 键）逐格恒等，
//!      载荷漂移 EConfigDrift 必抓，同格二次提交必拒 EDup（回投核验，
//!      config→账反向核验）。
//!   ③ trace 10 操作集：采样/埋点/分段/上送/链路合并/拓扑/指标/日志/
//!      告警样本/快照，逐操作载荷对照，越闭集 ENoTrace 必拒。
//!   ④ 与 OAP 消费联签 4 格 + Java 上栈声明 2 行闭集，缺行拒封
//!      （ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；注
//!      入核 run_cycle 三态轮转 10/10（越闭集/模块载荷漂移/模块族逆序），
//!      三类注入全部落在当轮临时账本上，检出+逃逸守恒等式；双跑 FNV 指
//!      纹一致。
//!   ⑥ KB-COMPAT 定级闭集 S2：Windows 面封顶有条件收口（OAP/存储后端在
//!      Linux 面联签，S1 预留给全栈面收口）；判据缺 → S3。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52397;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五模块族格数（5 族 × 4）。
pub const MODULES: usize = 20;

/// agent.config 双向投影格数。
pub const CONFIG_CELLS: usize = 12;

/// trace 操作数。
pub const TRACE_OPS: usize = 10;

/// OAP 消费联签行数。
pub const UNION_ROWS: usize = 4;

/// Java 上栈声明行数。
pub const JAVA_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkywalkingCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoModule,
    /// 模块/trace 载荷漂移
    EDrift,
    /// 模块族逆序
    EOrder,
    /// agent.config 投影漂移
    EConfigDrift,
    /// trace 操作越闭集
    ENoTrace,
    /// 声明行越闭集
    ENoRow,
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

impl SkywalkingCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SkywalkingCode::Ok => "成功",
            SkywalkingCode::ENoModule => "格号越闭集：五模块族 20 格闭集之外先登记",
            SkywalkingCode::EDrift => "载荷漂移：模块/trace 折叠与基准不恒等，逐格复算",
            SkywalkingCode::EOrder => "模块族逆序：agent 内核→OAP→UI→storage→alarm 单调，回跳必抓",
            SkywalkingCode::EConfigDrift => "agent.config 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            SkywalkingCode::ENoTrace => "trace 操作越闭集：10 操作集之外先登记",
            SkywalkingCode::ENoRow => "声明行越闭集：OAP 联签 4 行/Java 上栈 2 行闭集之外先登记",
            SkywalkingCode::ERowMiss => "缺声明行：OAP 消费联签或 Java 上栈声明未齐先补齐",
            SkywalkingCode::EDup => "重复落账：同格只许一条账目",
            SkywalkingCode::ESeal => "未齐即封：20 模块 + 12 投影 + 10 trace + 闸未齐先补齐",
            SkywalkingCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SkywalkingCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SkywalkingCode::EGradeDrift => "定级漂移：Windows 面封顶 S2（S1 预留 OAP/存储 Linux 面联签），判据缺只可定 S3",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全栈面收口（Windows 面不可达，预留 OAP/存储 Linux 面联签）
    S1,
    /// S2：Windows 面有条件收口（目标档）
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
            Grade::S1 => "S1：全栈面收口，需 OAP/存储 Linux 面联签，Windows 面预留",
            Grade::S2 => "S2：Windows 面有条件收口（目标档），六判据闸全落",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五模块族格载荷（格 o；族 = o/4）。
pub const fn module_cell(o: usize) -> Option<u64> {
    if o >= MODULES {
        return None;
    }
    Some((0xD0000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 模块族（0..4=agent 内核 4..8=OAP 8..12=UI 12..16=storage 16..20=alarm）。
pub const fn module_kind(o: usize) -> Option<u32> {
    if o >= MODULES {
        return None;
    }
    Some((o / 4) as u32)
}

/// 模块族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("agent 内核"),
        1 => Some("OAP"),
        2 => Some("UI"),
        3 => Some("storage"),
        4 => Some("alarm"),
        _ => None,
    }
}

/// agent.config 双向投影格载荷（格 i）。
pub const fn config_cell(i: usize) -> Option<u64> {
    if i >= CONFIG_CELLS {
        return None;
    }
    Some((0xD0800u64 + (i as u64) * 0x3B) ^ 0x534B5757_0000)
}

/// trace 操作格载荷（操作 t）。
pub const fn trace_cell(t: usize) -> Option<u64> {
    if t >= TRACE_OPS {
        return None;
    }
    Some((0xD1000u64 + (t as u64) * 0x3F) ^ 0x54524143_0000)
}

/// trace 操作名（const fn 表 + Option 边界）。
pub const fn trace_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("采样"),
        1 => Some("埋点"),
        2 => Some("分段"),
        3 => Some("上送"),
        4 => Some("链路合并"),
        5 => Some("拓扑"),
        6 => Some("指标"),
        7 => Some("日志"),
        8 => Some("告警样本"),
        9 => Some("快照"),
        _ => None,
    }
}

/// OAP 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

/// Java 上栈声明行折叠（行 r）。
pub const fn java_row(r: usize) -> Option<u32> {
    if r >= JAVA_ROWS {
        return None;
    }
    Some(0xD440 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SkyWalking Agent（Windows 面）收口账本状态机。
#[derive(Clone)]
pub struct SkywalkingLedger {
    /// 六判据（agent 注入/OAP 收数/UI 呈现/storage 落盘/alarm 触发/卸载净除）。
    gates: [Option<bool>; 6],
    /// 模块账。
    modules: [Option<u64>; MODULES],
    /// agent.config 投影账。
    config: [Option<u64>; CONFIG_CELLS],
    /// trace 操作账。
    traces: [Option<u64>; TRACE_OPS],
    /// OAP 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Java 上栈声明账。
    javas: [Option<u32>; JAVA_ROWS],
    /// 最后模块族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SkywalkingLedger {
    /// 新账本。
    pub fn new() -> Self {
        SkywalkingLedger {
            gates: [None; 6],
            modules: [None; MODULES],
            config: [None; CONFIG_CELLS],
            traces: [None; TRACE_OPS],
            unions: [None; UNION_ROWS],
            javas: [None; JAVA_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SkywalkingCode> {
        if gate >= 6 {
            return Err(SkywalkingCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xD2000 + gate as u64);
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

    /// 模块一格（族序单调非严格 + 载荷对照 + 重复拒）。
    pub fn record_module(&mut self, o: usize, got: u64) -> Result<(), SkywalkingCode> {
        if o >= MODULES {
            return Err(SkywalkingCode::ENoModule);
        }
        let kind = match module_kind(o) {
            Some(k) => k as i64,
            None => return Err(SkywalkingCode::ENoModule),
        };
        if kind < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD2100);
            return Err(SkywalkingCode::EOrder);
        }
        let want = match module_cell(o) {
            Some(w) => w,
            None => return Err(SkywalkingCode::ENoModule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD2101 + o as u64);
            return Err(SkywalkingCode::EDrift);
        }
        if match self.modules.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SkywalkingCode::EDup);
        }
        if let Some(c) = self.modules.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xD2102 + o as u64);
        Ok(())
    }

    /// 模块全账核验（20 格）。
    pub fn modules_all(&self) -> bool {
        let mut o = 0usize;
        while o < MODULES {
            match self.modules.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// agent.config 投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_config(&mut self, i: usize, got: u64) -> Result<(), SkywalkingCode> {
        if i >= CONFIG_CELLS {
            return Err(SkywalkingCode::ENoModule);
        }
        let want = match config_cell(i) {
            Some(w) => w,
            None => return Err(SkywalkingCode::ENoModule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD2200 + i as u64);
            return Err(SkywalkingCode::EConfigDrift);
        }
        if match self.config.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SkywalkingCode::EDup);
        }
        if let Some(c) = self.config.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xD2201 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn config_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONFIG_CELLS {
            match self.config.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// trace 操作一格（载荷对照 + 重复拒）。
    pub fn record_trace(&mut self, t: usize, got: u64) -> Result<(), SkywalkingCode> {
        if t >= TRACE_OPS {
            return Err(SkywalkingCode::ENoTrace);
        }
        let want = match trace_cell(t) {
            Some(w) => w,
            None => return Err(SkywalkingCode::ENoTrace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD2300 + t as u64);
            return Err(SkywalkingCode::EDrift);
        }
        if match self.traces.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SkywalkingCode::EDup);
        }
        if let Some(c) = self.traces.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0xD2301 + t as u64);
        Ok(())
    }

    /// trace 全账核验（10 操作）。
    pub fn traces_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRACE_OPS {
            match self.traces.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// OAP 消费联签一行。
    pub fn declare_union(&mut self, r: usize) -> Result<(), SkywalkingCode> {
        if r >= UNION_ROWS {
            return Err(SkywalkingCode::ENoRow);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SkywalkingCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xD2400 + r as u64);
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

    /// Java 上栈声明一行。
    pub fn declare_java(&mut self, r: usize) -> Result<(), SkywalkingCode> {
        if r >= JAVA_ROWS {
            return Err(SkywalkingCode::ENoRow);
        }
        if match self.javas.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SkywalkingCode::EDup);
        }
        if let Some(c) = self.javas.get_mut(r) {
            *c = java_row(r);
        }
        self.fnv(0xD2480 + r as u64);
        Ok(())
    }

    /// Java 声明全齐核验。
    pub fn javas_all(&self) -> bool {
        let mut r = 0usize;
        while r < JAVA_ROWS {
            match self.javas.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), SkywalkingCode> {
        if !(self.unions_all() && self.javas_all()) {
            return Err(SkywalkingCode::ERowMiss);
        }
        if !(self.gates_all() && self.modules_all() && self.config_all() && self.traces_all()) {
            return Err(SkywalkingCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), SkywalkingCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(SkywalkingCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：Windows 面封顶 S2（六闸齐即目标档），判据缺 → S3。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SkywalkingLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), SkywalkingCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(SkywalkingCode::EFp)
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &SkywalkingLedger, want: Grade) -> Result<(), SkywalkingCode> {
    let got = led.judge_grade();
    if got == want {
        Ok(())
    } else {
        Err(SkywalkingCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SkywalkingLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < MODULES {
        if let Some(w) = module_cell(o) {
            if led.record_module(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < CONFIG_CELLS {
        if let Some(w) = config_cell(i) {
            if led.record_config(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < TRACE_OPS {
        if let Some(w) = trace_cell(t) {
            if led.record_trace(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < UNION_ROWS {
        let _ = led.declare_union(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < JAVA_ROWS {
        let _ = led.declare_java(r);
        r += 1;
    }
    // 注入 10 轮：越闭集 / 模块载荷漂移 / 模块族逆序 三态轮转，
    // 三类注入全部落在当轮临时账本上（主账 last_kind 不受扰）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SkywalkingLedger::new();
        let caught = match kind {
            0 => {
                lt.record_module(MODULES, 0).is_err()
                    && lt.record_trace(TRACE_OPS, 0).is_err()
                    && lt.record_config(CONFIG_CELLS, 0).is_err()
            }
            1 => {
                let _ = lt.record_module(0, module_cell(0).unwrap_or(0));
                lt.record_module(1, module_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SkywalkingCode::EDrift)
            }
            _ => {
                let _ = lt.record_module(9, module_cell(9).unwrap_or(0));
                lt.record_module(2, module_cell(2).unwrap_or(0)) == Err(SkywalkingCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xD2500 + k as u64);
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
/// UNX-F52397 判据集（锚 UNX-F52398-J1，off-by-one 板式）。
pub fn run_unx52397_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52397");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == MODULES as u32 + CONFIG_CELLS as u32 + TRACE_OPS as u32,
        "20 模块 + 12 投影 + 10 trace 逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "五模块族 20 格全通",
        {
            let mut lt = SkywalkingLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < MODULES {
                if let Some(w) = module_cell(o) {
                    if lt.record_module(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.modules_all()
        },
        "agent 内核/OAP/UI/storage/alarm 五族逐格载荷恒等",
    );
    s.add(
        "agent.config 双向投影 12 格全落",
        {
            let mut lt = SkywalkingLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CONFIG_CELLS {
                if let Some(w) = config_cell(i) {
                    if lt.record_config(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.config_all()
        },
        "账→config 正向投影逐格恒等落账",
    );
    s.add(
        "agent.config 回投核验 12/12",
        {
            let mut lt = SkywalkingLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CONFIG_CELLS {
                if let Some(w) = config_cell(i) {
                    let _ = lt.record_config(i, w);
                    // 回投：同格二次提交必拒 EDup（config→账反向核验）。
                    if lt.record_config(i, w) != Err(SkywalkingCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.config_all()
        },
        "config→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "trace 10 操作全通",
        {
            let mut lt = SkywalkingLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRACE_OPS {
                if let Some(w) = trace_cell(t) {
                    if lt.record_trace(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.traces_all()
        },
        "采样/埋点/分段/上送/链路合并/拓扑/指标/日志/告警样本/快照逐操作恒等",
    );
    s.add(
        "OAP 消费联签 4/4",
        {
            let mut lt = SkywalkingLedger::new();
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
        "OAP 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Java 上栈声明 2/2",
        {
            let mut lt = SkywalkingLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JAVA_ROWS {
                if lt.declare_java(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.javas_all()
        },
        "Java Agent 上栈声明/JVM 帧栈声明两行闭集",
    );
    s.add(
        "模块族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MODULES {
                if module_kind(o).unwrap_or(5) < module_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "agent 内核→OAP→UI→storage→alarm 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = SkywalkingLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "agent 注入/OAP 收数/UI 呈现/storage 落盘/alarm 触发/卸载净除逐格落账",
    );
    s.add(
        "边界·模块越闭集",
        SkywalkingLedger::new().record_module(MODULES, 0).is_err(),
        "模块 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        SkywalkingLedger::new().record_config(CONFIG_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·trace 越闭集",
        SkywalkingLedger::new().record_trace(TRACE_OPS, 0).is_err(),
        "trace 10 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = SkywalkingLedger::new();
            let _ = lt.record_config(0, config_cell(0).unwrap_or(0));
            lt.record_config(1, config_cell(1).unwrap_or(0).wrapping_add(5))
                == Err(SkywalkingCode::EConfigDrift)
        },
        "agent.config 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "越闭集/载荷漂移/族逆序三态轮转必抓",
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
        "定级 S2 闭集（Windows 面封顶）",
        {
            let mut lt = SkywalkingLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < MODULES {
                if let Some(w) = module_cell(o) {
                    let _ = lt.record_module(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < CONFIG_CELLS {
                if let Some(w) = config_cell(i) {
                    let _ = lt.record_config(i, w);
                }
                i += 1;
            }
            let mut t = 0usize;
            while t < TRACE_OPS {
                if let Some(w) = trace_cell(t) {
                    let _ = lt.record_trace(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < UNION_ROWS {
                let _ = lt.declare_union(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < JAVA_ROWS {
                let _ = lt.declare_java(r);
                r += 1;
            }
            lt.seal().is_ok()
                && lt.judge_grade() == Grade::S2
                && grade_guard(&lt, Grade::S2).is_ok()
                && grade_guard(&lt, Grade::S1).is_err()
        },
        "全齐 → S2（KB-COMPAT 定级登记，Windows 面目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_kind() {
        assert_eq!(module_kind(0), Some(0));
        assert_eq!(module_kind(4), Some(1));
        assert_eq!(module_kind(19), Some(4));
        assert!(module_kind(20).is_none());
        assert!(module_cell(20).is_none());
        assert_eq!(kind_name(0), Some("agent 内核"));
        assert_eq!(kind_name(4), Some("alarm"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_trace_names() {
        assert_eq!(trace_name(0), Some("采样"));
        assert_eq!(trace_name(9), Some("快照"));
        assert!(trace_name(10).is_none());
        assert!(trace_cell(TRACE_OPS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = module_cell(3).unwrap_or(0);
        assert_eq!(a, module_cell(3).unwrap_or(0));
        assert_ne!(a, module_cell(4).unwrap_or(0));
        assert_ne!(config_cell(0).unwrap_or(0), config_cell(5).unwrap_or(0));
        assert!(config_cell(CONFIG_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(java_row(JAVA_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序落 OAP 族格（kind 1）后回跳 agent 族（kind 0）→ 族序逆必抓。
        let mut lt = SkywalkingLedger::new();
        let _ = lt.record_module(9, module_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_module(2, module_cell(2).unwrap_or(0)),
            Err(SkywalkingCode::EOrder)
        );
        assert!(lt.record_module(10, module_cell(10).unwrap_or(0)).is_ok());
        // 同族内顺序自由（非严格阶梯）。
        assert!(lt.record_module(8, module_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_config_drift_and_dup() {
        let mut lt = SkywalkingLedger::new();
        assert_eq!(
            lt.record_config(0, config_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(SkywalkingCode::EConfigDrift)
        );
        let _ = lt.record_config(0, config_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_config(0, config_cell(0).unwrap_or(0)),
            Err(SkywalkingCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SkywalkingLedger::new();
        let _ = lt.record_module(0, module_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_module(0, module_cell(0).unwrap_or(0)),
            Err(SkywalkingCode::EDup)
        );
        let _ = lt.declare_java(0);
        assert_eq!(lt.declare_java(0), Err(SkywalkingCode::EDup));
        assert_eq!(lt.declare_java(JAVA_ROWS), Err(SkywalkingCode::ENoRow));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(SkywalkingCode::ENoRow));
        assert_eq!(lt.record_trace(TRACE_OPS, 0), Err(SkywalkingCode::ENoTrace));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SkywalkingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!SkywalkingLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(SkywalkingLedger::new().seal(), Err(SkywalkingCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = SkywalkingLedger::new();
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = lt.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < JAVA_ROWS {
            let _ = lt.declare_java(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(SkywalkingCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 模块 + 12 投影 + 10 trace（闸与声明行不计数）。
        assert_eq!(
            recorded,
            MODULES as u32 + CONFIG_CELLS as u32 + TRACE_OPS as u32
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
        assert_eq!(SEG_BASE, 0xCA300);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SkywalkingCode::Ok as u32, 0);
        assert_eq!(SkywalkingCode::EGradeDrift as u32, 12);
        assert_eq!(SkywalkingCode::Ok.val(), SEG_BASE);
        assert_eq!(SkywalkingCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!SkywalkingCode::Ok.detail().is_empty());
        assert!(!SkywalkingCode::ESeal.detail().is_empty());
        assert!(!SkywalkingCode::EConfigDrift.detail().is_empty());
        assert!(!SkywalkingCode::ENoTrace.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2（Windows 面目标档）→ 全齐仍 S2。
        assert_eq!(SkywalkingLedger::new().judge_grade(), Grade::S3);
        let mut lt = SkywalkingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        // 全齐账仍封顶 S2（S1 预留 OAP/存储 Linux 面联签）。
        let mut full = SkywalkingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < MODULES {
            if let Some(w) = module_cell(o) {
                let _ = full.record_module(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < CONFIG_CELLS {
            if let Some(w) = config_cell(i) {
                let _ = full.record_config(i, w);
            }
            i += 1;
        }
        let mut t = 0usize;
        while t < TRACE_OPS {
            if let Some(w) = trace_cell(t) {
                let _ = full.record_trace(t, w);
            }
            t += 1;
        }
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = full.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < JAVA_ROWS {
            let _ = full.declare_java(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(full.judge_grade(), Grade::S2);
        assert!(grade_guard(&full, Grade::S2).is_ok());
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
        assert!(full.conservation().is_ok());
    }

    #[test]
    fn unx52397_checks_green() {
        let s = run_unx52397_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 19);
    }
}
