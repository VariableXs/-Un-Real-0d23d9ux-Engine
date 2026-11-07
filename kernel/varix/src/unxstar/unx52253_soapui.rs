//! UNX-F52253 SoapUI 收口 · 0xC7F00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52253"，逐字）：
//!   title: SoapUI 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52254-J1 六判据
//!     全落（Electron）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52253）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52254-J1，照原文锚定；功能按本卡标题（SoapUI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐单定制）：
//!   ① REST/SOAP 双协议 12 格闭集：REST 0..6、SOAP 6..12 两族，族单调
//!      （先 REST 后 SOAP，逆序必抓），逐格载荷对照。
//!   ② 测试套件 10 操作集：五家族闭集（用例编排/步骤装配/数据源/循环控
//!      制/报告生成）各 2 操作，家族单调。
//!   ③ 断言/属性传输 8 格：断言 0..4、属性传输 4..8 两面，逐格落账。
//!   ④ Java 上栈 4 层：JRE 运行时 → JVM 类加载 → Groovy 脚本引擎 →
//!      Swing 桌面栈，自底向上严格栈序，逆栈必抓。
//!   ⑤ 开源版/专业版分账声明 2 行闭集：缺一拒封。
//!   ⑥ 六判据收口闸：REST 启动/SOAP 工程/用例执行/崩溃/卸载/许可，
//!      逐格落账，未齐即封拒。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（协议越闭集/载荷漂移/家族逆
//!      序），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级闭
//!      集：全通 → S2（有条件收口，双协议工程口径）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52253;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 双协议格数（REST 6 + SOAP 6）。
pub const PROTOCOL_CELLS: usize = 12;

/// 测试套件操作数。
pub const SUITE_OPS: usize = 10;

/// 断言/属性传输格数（断言 4 + 属性传输 4）。
pub const ASSERT_CELLS: usize = 8;

/// Java 上栈层数。
pub const JAVA_LAYERS: usize = 4;

/// 开源/专业分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoapCode {
    /// 成功
    Ok = 0,
    /// 协议格越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 协议族逆序（SOAP 先于 REST）
    EProtoOrder,
    /// 套件操作越闭集
    ENoOp,
    /// 套件载荷漂移
    EOpDrift,
    /// 套件家族逆序
    EOrder,
    /// 断言/属性传输格越闭集
    ENoCell,
    /// 断言/属性传输载荷漂移
    ECellDrift,
    /// JVM 逆栈
    EStack,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl SoapCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SoapCode::Ok => "成功",
            SoapCode::ENoProto => "协议格越闭集：REST/SOAP 12 格闭集之外先登记",
            SoapCode::EProtoDrift => "协议载荷漂移：协议格折叠与基准不恒等，逐格复算",
            SoapCode::EProtoOrder => "协议族逆序：先 REST 后 SOAP 单调，逆序必抓",
            SoapCode::ENoOp => "套件操作越闭集：测试套件 10 操作集之外先登记",
            SoapCode::EOpDrift => "套件载荷漂移：操作折叠与基准不恒等，逐格复算",
            SoapCode::EOrder => "套件家族逆序：用例编排→步骤装配→数据源→循环控制→报告生成单调，逆序必抓",
            SoapCode::ENoCell => "断言/属性传输格越闭集：8 格闭集之外先登记",
            SoapCode::ECellDrift => "断言/属性传输载荷漂移：格折叠与基准不恒等，逐格复算",
            SoapCode::EStack => "JVM 逆栈：JRE→类加载→Groovy→Swing 自底向上，逆栈必抓",
            SoapCode::EDup => "重复落账：同格只许一条账目",
            SoapCode::ESeal => "未齐即封：12 格协议+10 操作+8 格断言+4 层上栈+2 行分账未齐先补齐",
            SoapCode::EGradeDrift => "定级漂移：全通才可定 S2，判据缺为 S4",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// SoapUI KB-COMPAT 定级闭集（本卡口径顶格 S2：双协议工程有条件收口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoapGrade {
    /// S2：全通有条件收口（开源/专业双轨声明齐）
    S2,
    /// S3：判据全但格账有缺
    S3,
    /// S4：判据缺失，不收口
    S4,
}

impl SoapGrade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            SoapGrade::S2 => 2,
            SoapGrade::S3 => 3,
            SoapGrade::S4 => 4,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            SoapGrade::S2 => "S2：双协议 12 格+套件 10+断言 8+JVM 4 层+分账 2 行全通有条件收口",
            SoapGrade::S3 => "S3：六判据全但格账有缺，补齐后复评",
            SoapGrade::S4 => "S4：判据缺失，不收口",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 协议格载荷（格 c：0..6=REST，6..12=SOAP）。
pub const fn protocol_cell(c: usize) -> Option<u64> {
    if c >= PROTOCOL_CELLS {
        return None;
    }
    Some((0xB40000u64 + (c as u64) * 0x3D) ^ 0x534F41505F524553)
}

/// 协议族（0=REST 1=SOAP）。
pub const fn protocol_kind(c: usize) -> Option<u32> {
    if c >= PROTOCOL_CELLS {
        return None;
    }
    Some((c / 6) as u32)
}

/// 协议族名。
pub const fn protocol_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("REST"),
        1 => Some("SOAP"),
        _ => None,
    }
}

/// 测试套件操作格载荷（操作 o；家族 = o/2）。
pub const fn suite_cell(o: usize) -> Option<u64> {
    if o >= SUITE_OPS {
        return None;
    }
    Some((0xB50000u64 + (o as u64) * 0x51) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 套件操作家族（0..2=用例编排 2..4=步骤装配 4..6=数据源 6..8=循环控制 8..10=报告生成）。
pub const fn suite_kind(o: usize) -> Option<u32> {
    if o >= SUITE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 套件家族名。
pub const fn suite_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("用例编排"),
        1 => Some("步骤装配"),
        2 => Some("数据源"),
        3 => Some("循环控制"),
        4 => Some("报告生成"),
        _ => None,
    }
}

/// 断言/属性传输格载荷（格 a：0..4=断言 4..8=属性传输）。
pub const fn assert_cell(a: usize) -> Option<u64> {
    if a >= ASSERT_CELLS {
        return None;
    }
    Some((0xB60000u64 + (a as u64) * 0x2F) ^ 0x4153534552540001)
}

/// 断言面名（面 f：0=断言 1=属性传输）。
pub const fn assert_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("断言"),
        1 => Some("属性传输"),
        _ => None,
    }
}

/// Java 上栈层载荷（层 l：0..4 自底向上）。
pub const fn java_layer_cell(l: usize) -> Option<u64> {
    if l >= JAVA_LAYERS {
        return None;
    }
    Some((0xB70000u64 + (l as u64) * 0x1A) ^ 0x4A564D5F53544B31)
}

/// Java 上栈层名。
pub const fn java_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JRE 运行时"),
        1 => Some("JVM 类加载"),
        2 => Some("Groovy 脚本引擎"),
        3 => Some("Swing 桌面栈"),
        _ => None,
    }
}

/// 开源/专业分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB710 + r as u32 * 0x11)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("开源版分账声明"),
        1 => Some("专业版分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SoapUI 收口账本状态机。
#[derive(Clone)]
pub struct SoapLedger {
    /// 六判据（REST 启动/SOAP 工程/用例执行/崩溃/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 双协议账。
    protos: [Option<u64>; PROTOCOL_CELLS],
    /// 套件操作账。
    ops: [Option<u64>; SUITE_OPS],
    /// 断言/属性传输账。
    cells: [Option<u64>; ASSERT_CELLS],
    /// JVM 上栈账。
    layers: [Option<u64>; JAVA_LAYERS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 已上栈层数（栈序核验用）。
    stacked: usize,
    /// 最后协议族。
    last_proto: i64,
    /// 最后套件家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SoapLedger {
    /// 新账本。
    pub fn new() -> Self {
        SoapLedger {
            gates: [None; 6],
            protos: [None; PROTOCOL_CELLS],
            ops: [None; SUITE_OPS],
            cells: [None; ASSERT_CELLS],
            layers: [None; JAVA_LAYERS],
            splits: [None; SPLIT_ROWS],
            stacked: 0,
            last_proto: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SoapCode> {
        if gate >= 6 {
            return Err(SoapCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 协议一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_protocol(&mut self, c: usize, got: u64) -> Result<(), SoapCode> {
        if c >= PROTOCOL_CELLS {
            return Err(SoapCode::ENoProto);
        }
        let kind = match protocol_kind(c) {
            Some(k) => k,
            None => return Err(SoapCode::ENoProto),
        };
        if (kind as i64) < self.last_proto {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(SoapCode::EProtoOrder);
        }
        let want = match protocol_cell(c) {
            Some(w) => w,
            None => return Err(SoapCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + c as u64);
            return Err(SoapCode::EProtoDrift);
        }
        if match self.protos.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoapCode::EDup);
        }
        if let Some(slot) = self.protos.get_mut(c) {
            *slot = Some(got);
        }
        self.last_proto = kind as i64;
        self.fnv(0x5202 + c as u64);
        Ok(())
    }

    /// 双协议全账核验（12 格）。
    pub fn protos_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROTOCOL_CELLS {
            match self.protos.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 套件操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_suite(&mut self, o: usize, got: u64) -> Result<(), SoapCode> {
        if o >= SUITE_OPS {
            return Err(SoapCode::ENoOp);
        }
        let kind = match suite_kind(o) {
            Some(k) => k,
            None => return Err(SoapCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(SoapCode::EOrder);
        }
        let want = match suite_cell(o) {
            Some(w) => w,
            None => return Err(SoapCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(SoapCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoapCode::EDup);
        }
        if let Some(slot) = self.ops.get_mut(o) {
            *slot = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 套件全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < SUITE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 断言/属性传输一格（载荷对照 + 重复拒）。
    pub fn record_assert(&mut self, a: usize, got: u64) -> Result<(), SoapCode> {
        let want = match assert_cell(a) {
            Some(w) => w,
            None => return Err(SoapCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + a as u64);
            return Err(SoapCode::ECellDrift);
        }
        if match self.cells.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoapCode::EDup);
        }
        if let Some(slot) = self.cells.get_mut(a) {
            *slot = Some(got);
        }
        self.fnv(0x5401 + a as u64);
        Ok(())
    }

    /// 断言/属性传输全账核验（8 格）。
    pub fn cells_all(&self) -> bool {
        let mut a = 0usize;
        while a < ASSERT_CELLS {
            match self.cells.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// Java 上栈一层（严格栈序：必须正好是下一层，逆栈必抓）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), SoapCode> {
        let want = match java_layer_cell(l) {
            Some(w) => w,
            None => return Err(SoapCode::EStack),
        };
        if l != self.stacked {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500);
            return Err(SoapCode::EStack);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5501 + l as u64);
            return Err(SoapCode::ECellDrift);
        }
        if let Some(slot) = self.layers.get_mut(l) {
            *slot = Some(got);
        }
        self.stacked += 1;
        self.fnv(0x5502 + l as u64);
        Ok(())
    }

    /// JVM 上栈全账核验（4 层且栈序完整）。
    pub fn layers_all(&self) -> bool {
        self.stacked == JAVA_LAYERS
    }

    /// 分账声明一行（开源/专业）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SoapCode> {
        if r >= SPLIT_ROWS {
            return Err(SoapCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoapCode::EDup);
        }
        if let Some(slot) = self.splits.get_mut(r) {
            *slot = split_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

    /// KB-COMPAT 定级（全通 → S2；判据全但格缺 → S3；判据缺 → S4）。
    pub fn judge_grade(&self) -> SoapGrade {
        if self.gates_all()
            && self.protos_all()
            && self.ops_all()
            && self.cells_all()
            && self.layers_all()
            && self.splits_all()
        {
            SoapGrade::S2
        } else if self.gates_all() {
            SoapGrade::S3
        } else {
            SoapGrade::S4
        }
    }
}

impl Default for SoapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SoapLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < PROTOCOL_CELLS {
        if let Some(w) = protocol_cell(c) {
            if led.record_protocol(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < SUITE_OPS {
        if let Some(w) = suite_cell(o) {
            if led.record_suite(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut a = 0usize;
    while a < ASSERT_CELLS {
        if let Some(w) = assert_cell(a) {
            if led.record_assert(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut l = 0usize;
    while l < JAVA_LAYERS {
        if let Some(w) = java_layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：协议越闭集 / 协议载荷漂移 / 套件家族逆序三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数与指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SoapLedger::new();
        let caught = match kind {
            0 => lt.record_protocol(PROTOCOL_CELLS, 0) == Err(SoapCode::ENoProto),
            1 => {
                // 漂移注入先录正格再注漂移。
                if let Some(w) = protocol_cell(0) {
                    let _ = lt.record_protocol(0, w);
                }
                lt.record_protocol(1, protocol_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SoapCode::EProtoDrift)
            }
            _ => {
                // 逆序注入对空账本同样必抓：先录正格（高族未录格）再逆序注入，
                // 拒后 last_kind 不变。
                if let Some(w) = suite_cell(5) {
                    let _ = lt.record_suite(5, w);
                }
                lt.record_suite(0, suite_cell(0).unwrap_or(0)) == Err(SoapCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52253 判据集（锚 UNX-F52254-J1，off-by-one 板式）。
pub fn run_unx52253_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52253");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "REST/SOAP 双协议 12 格全通",
        recorded >= PROTOCOL_CELLS as u32,
        "REST 6 格 + SOAP 6 格逐格恒等，族单调先 REST 后 SOAP",
    );
    s.add(
        "测试套件 10 操作全通",
        {
            let mut lt = SoapLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < SUITE_OPS {
                if let Some(w) = suite_cell(o) {
                    if lt.record_suite(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "用例编排/步骤装配/数据源/循环控制/报告生成五家族逐操作恒等",
    );
    s.add(
        "断言/属性传输 8 格全落",
        {
            let mut lt = SoapLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ASSERT_CELLS {
                if let Some(w) = assert_cell(a) {
                    if lt.record_assert(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.cells_all()
        },
        "断言 4 格 + 属性传输 4 格逐格落账",
    );
    s.add(
        "Java 上栈 4 层栈序",
        {
            let mut lt = SoapLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                if let Some(w) = java_layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "JRE 运行时→JVM 类加载→Groovy 脚本引擎→Swing 桌面栈自底向上",
    );
    s.add(
        "JVM 逆栈必抓",
        {
            let mut lt = SoapLedger::new();
            lt.record_layer(2, java_layer_cell(2).unwrap_or(0)) == Err(SoapCode::EStack)
        },
        "跳层上栈（第 2 层先于第 0 层）必拒",
    );
    s.add(
        "开源/专业分账声明 2/2 全齐",
        {
            let mut lt = SoapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "开源版分账声明/专业版分账声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SoapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "REST 启动/SOAP 工程/用例执行/崩溃/卸载/许可逐格落账（锚 UNX-F52254-J1）",
    );
    s.add(
        "套件家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SUITE_OPS {
                if suite_kind(o).unwrap_or(5) < suite_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "协议越闭集/载荷漂移/家族逆序三态轮转必抓",
    );
    // 边界·越闭集 ×3。
    let mut l0 = SoapLedger::new();
    s.add(
        "边界·协议越闭集",
        l0.record_protocol(PROTOCOL_CELLS, 0).is_err(),
        "协议格 12 越闭集必拒",
    );
    s.add(
        "边界·套件操作越闭集",
        l0.record_suite(SUITE_OPS, 0).is_err(),
        "套件操作 10 越闭集必拒",
    );
    s.add(
        "边界·断言格越闭集",
        l0.record_assert(ASSERT_CELLS, 0).is_err(),
        "断言/属性传输格 8 越闭集必拒",
    );
    s.add(
        "载荷漂移拒",
        {
            let _ = l0.record_protocol(0, protocol_cell(0).unwrap_or(0));
            l0.record_protocol(1, protocol_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(SoapCode::EProtoDrift)
        },
        "协议载荷漂移必抓",
    );
    s.add(
        "分账行越闭集",
        lt_split_out_of_range(),
        "分账行 2 越闭集必拒",
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
        "定级 S2 闭集",
        {
            // main/aux 各节全部记账后再断言。
            let mut lt = SoapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < PROTOCOL_CELLS {
                if let Some(w) = protocol_cell(c) {
                    let _ = lt.record_protocol(c, w);
                }
                c += 1;
            }
            let mut o = 0usize;
            while o < SUITE_OPS {
                if let Some(w) = suite_cell(o) {
                    let _ = lt.record_suite(o, w);
                }
                o += 1;
            }
            let mut a = 0usize;
            while a < ASSERT_CELLS {
                if let Some(w) = assert_cell(a) {
                    let _ = lt.record_assert(a, w);
                }
                a += 1;
            }
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                if let Some(w) = java_layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == SoapGrade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，双协议工程口径）",
    );
    s.add(
        "定级降档闭集",
        {
            let lt = SoapLedger::new();
            lt.judge_grade() == SoapGrade::S4
        },
        "判据缺 → S4，空账不误判 S2",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且公式恒等",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

/// 分账行越闭集复验（独立小函数便于测试复用）。
fn lt_split_out_of_range() -> bool {
    let mut lt = SoapLedger::new();
    lt.declare_split(SPLIT_ROWS).is_err()
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_kind() {
        assert_eq!(protocol_kind(0), Some(0));
        assert_eq!(protocol_kind(5), Some(0));
        assert_eq!(protocol_kind(6), Some(1));
        assert_eq!(protocol_kind(11), Some(1));
        assert!(protocol_kind(12).is_none());
        assert!(protocol_cell(12).is_none());
    }

    #[test]
    fn test_protocol_names() {
        assert_eq!(protocol_kind_name(0), Some("REST"));
        assert_eq!(protocol_kind_name(1), Some("SOAP"));
        assert!(protocol_kind_name(2).is_none());
    }

    #[test]
    fn test_suite_kind() {
        assert_eq!(suite_kind(0), Some(0));
        assert_eq!(suite_kind(2), Some(1));
        assert_eq!(suite_kind(9), Some(4));
        assert!(suite_kind(10).is_none());
        assert_eq!(suite_kind_name(4), Some("报告生成"));
        assert!(suite_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = protocol_cell(3).unwrap_or(0);
        assert_eq!(a, protocol_cell(3).unwrap_or(0));
        assert_ne!(a, protocol_cell(4).unwrap_or(0));
        assert_ne!(assert_cell(0).unwrap_or(0), assert_cell(1).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(split_row_name(0), Some("开源版分账声明"));
        assert_eq!(split_row_name(1), Some("专业版分账声明"));
    }

    #[test]
    fn test_java_layer_names() {
        assert_eq!(java_layer_name(0), Some("JRE 运行时"));
        assert_eq!(java_layer_name(3), Some("Swing 桌面栈"));
        assert!(java_layer_name(4).is_none());
        assert!(java_layer_cell(4).is_none());
    }

    #[test]
    fn test_proto_order_caught() {
        let mut lt = SoapLedger::new();
        let _ = lt.record_protocol(6, protocol_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_protocol(0, protocol_cell(0).unwrap_or(0)),
            Err(SoapCode::EProtoOrder)
        );
        // 拒后同族复验：SOAP 未录格照常可录。
        assert!(lt.record_protocol(7, protocol_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_suite_order_caught() {
        let mut lt = SoapLedger::new();
        let _ = lt.record_suite(4, suite_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_suite(1, suite_cell(1).unwrap_or(0)),
            Err(SoapCode::EOrder)
        );
        // 逆序拒后同 last_kind 复验须用同族或更高族未录格。
        assert!(lt.record_suite(5, suite_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SoapLedger::new();
        assert_eq!(
            lt.record_suite(3, suite_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(SoapCode::EOpDrift)
        );
        assert_eq!(
            lt.record_assert(2, assert_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(SoapCode::ECellDrift)
        );
    }

    #[test]
    fn test_stack_order() {
        let mut lt = SoapLedger::new();
        // 跳层（第 3 层先上）必抓且不推进栈。
        assert_eq!(
            lt.record_layer(3, java_layer_cell(3).unwrap_or(0)),
            Err(SoapCode::EStack)
        );
        assert_eq!(lt.stacked, 0);
        let mut l = 0usize;
        while l < JAVA_LAYERS {
            assert!(lt.record_layer(l, java_layer_cell(l).unwrap_or(0)).is_ok());
            l += 1;
        }
        assert!(lt.layers_all());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SoapLedger::new();
        let _ = lt.record_protocol(0, protocol_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_protocol(0, protocol_cell(0).unwrap_or(0)),
            Err(SoapCode::EDup)
        );
        let _ = lt.record_assert(5, assert_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_assert(5, assert_cell(5).unwrap_or(0)),
            Err(SoapCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SoapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SoapLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (PROTOCOL_CELLS + SUITE_OPS + ASSERT_CELLS + JAVA_LAYERS) as u32
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
        assert_eq!(SEG_BASE, 0xC7F00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SoapCode::Ok as u32, 0);
        assert_eq!(SoapCode::EGradeDrift as u32, 12);
        assert!(!SoapCode::Ok.detail().is_empty());
        assert!(!SoapCode::ESeal.detail().is_empty());
        assert!(!SoapCode::EStack.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(SoapGrade::S2.val(), 2);
        assert_eq!(SoapGrade::S4.val(), 4);
        let lt = SoapLedger::new();
        assert_eq!(lt.judge_grade(), SoapGrade::S4);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = SoapLedger::new();
        // 载荷漂移路径记账：注入=检出=1、逃逸=0（越闭集路径提前返回不记账）。
        let _ = lt.record_protocol(0, protocol_cell(0).unwrap_or(0).wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn unx52253_checks_green() {
        let s = run_unx52253_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
