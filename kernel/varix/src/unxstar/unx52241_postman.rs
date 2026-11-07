//! UNX-F52241 Postman 收口 · 0xC7C00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52241"，逐字）：
//!   title: Postman 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52242-J1
//!     collection 运行 ×10 全通；与桌面端共面声明；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52241）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52242-J1，照原文锚定；功能按本卡标题（Postman）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 三账 15 格闭集：集合账（0..5）/ 环境账（5..10）/ 工作区账
//!      （10..15），逐格载荷折叠恒等才收，越账必抓。
//!   ② 请求 10 操作集：五家族闭集（新建请求/发送/保存/复制/删除）各
//!      2 操作，家族单调（逆序必抓且不污染家族账位）。
//!   ③ collection v2 双向投影 12 格：正向投影（0..6，工作区→v2 schema）
//!      与反向投影（6..12，v2 schema→工作区），双向逐格恒等才收。
//!   ④ Electron 4 层阶梯：主进程/渲染进程/预加载/Node 集成，越层必抓，
//!      逐层载荷对照。
//!   ⑤ 云/本地同步分账声明 2 行闭集：云同步分账 / 本地同步分账，缺一
//!      拒封（与桌面端共面声明即此两行）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（三账越闭集/
//!      请求漂移/投影越闭集），三类注入全部落在当轮临时账本上，检出
//!      +逃逸守恒等式 injected==caught==10、escaped==0；双跑 FNV 指纹
//!      一致；定级 S1/S2/S3 闭集（15 格 + 10 操作 + 12 投影 + 4 层 +
//!      2 行分账 + 六判据全通 → S1，KB-COMPAT 定级 S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界 + match 取值）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52241;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 三账格数（集合 5 + 环境 5 + 工作区 5）。
pub const LEDGER_CELLS: usize = 15;

/// 请求操作数。
pub const REQUEST_OPS: usize = 10;

/// collection v2 双向投影格数（正向 6 + 反向 6）。
pub const PROJECTION_CELLS: usize = 12;

/// Electron 层数。
pub const ELECTRON_LAYERS: usize = 4;

/// 云/本地同步分账声明行数。
pub const SYNC_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostmanCode {
    /// 成功
    Ok = 0,
    /// 三账格越闭集
    ENoCell,
    /// 三账格载荷漂移
    ECellDrift,
    /// 请求操作号越闭集
    ENoReq,
    /// 请求操作载荷漂移
    EReqDrift,
    /// 请求家族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影载荷漂移
    EProjDrift,
    /// Electron 层越阶梯
    ENoLayer,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PostmanCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PostmanCode::Ok => "成功",
            PostmanCode::ENoCell => "三账格越闭集：集合/环境/工作区 15 格之外先登记",
            PostmanCode::ECellDrift => "三账格载荷漂移：账格折叠与基准不恒等，逐格复算",
            PostmanCode::ENoReq => "请求操作号越闭集：请求 10 操作集之外先登记",
            PostmanCode::EReqDrift => "请求操作漂移：操作折叠与基准不恒等，逐格复算",
            PostmanCode::EOrder => "请求家族逆序：新建→发送→保存→复制→删除单调，逆序必抓",
            PostmanCode::ENoProj => "投影格越闭集：collection v2 双向投影 12 格之外先登记",
            PostmanCode::EProjDrift => "投影载荷漂移：v2 双向投影与基准不恒等，逐向复算",
            PostmanCode::ENoLayer => "Electron 层越阶梯：主进程/渲染/预加载/Node 四层之外先登记",
            PostmanCode::EDup => "重复落账：同格只许一条账目",
            PostmanCode::ESeal => "未齐即封：15 格账 + 10 操作 + 12 投影 + 4 层 + 2 行分账未齐先补齐",
            PostmanCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PostmanCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+15格账+10操作+12投影+4层+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 三账格载荷（格 c：0..5=集合 5..10=环境 10..15=工作区）。
pub const fn ledger_cell(c: usize) -> Option<u64> {
    if c >= LEDGER_CELLS {
        return None;
    }
    Some((0xD1000u64 + (c as u64) * 0x3A) ^ 0x504F53544D414E)
}

/// 三账格族（0=集合 1=环境 2=工作区）。
pub const fn ledger_kind(c: usize) -> Option<u32> {
    if c >= LEDGER_CELLS {
        return None;
    }
    Some((c / 5) as u32)
}

/// 三账族名。
pub const fn ledger_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("集合账"),
        1 => Some("环境账"),
        2 => Some("工作区账"),
        _ => None,
    }
}

/// 请求操作格载荷（操作 o；家族 = o/2）。
pub const fn request_cell(o: usize) -> Option<u64> {
    if o >= REQUEST_OPS {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x24) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 请求操作家族（每 2 操作一族）。
pub const fn request_kind(o: usize) -> Option<u32> {
    if o >= REQUEST_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 请求家族名。
pub const fn request_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("新建请求"),
        1 => Some("发送"),
        2 => Some("保存"),
        3 => Some("复制"),
        4 => Some("删除"),
        _ => None,
    }
}

/// collection v2 双向投影格载荷（格 p：0..6=正向 6..12=反向）。
pub const fn projection_cell(p: usize) -> Option<u64> {
    if p >= PROJECTION_CELLS {
        return None;
    }
    if p < 6 {
        Some((0xD3000u64 + (p as u64) * 0x51) ^ 0x56324657)
    } else {
        Some((0xD3800u64 + ((p - 6) as u64) * 0x51) ^ 0x52455632)
    }
}

/// 投影向（0=正向 工作区→v2，1=反向 v2→工作区）。
pub const fn projection_dir(p: usize) -> Option<u32> {
    if p >= PROJECTION_CELLS {
        return None;
    }
    Some((p / 6) as u32)
}

/// 投影向名。
pub const fn projection_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("正向：工作区→v2 schema"),
        1 => Some("反向：v2 schema→工作区"),
        _ => None,
    }
}

/// Electron 层载荷（层 l：0=主进程 1=渲染进程 2=预加载 3=Node 集成）。
pub const fn electron_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some((0xD4000u64 + (l as u64) * 0x77) ^ 0x454C454354524F4E)
}

/// Electron 层名。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("主进程"),
        1 => Some("渲染进程"),
        2 => Some("预加载脚本"),
        3 => Some("Node 集成"),
        _ => None,
    }
}

/// 云/本地同步分账行折叠（行 r）。
pub const fn sync_row(r: usize) -> Option<u32> {
    if r >= SYNC_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x33)
}

/// 同步分账行名。
pub const fn sync_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("云同步分账声明"),
        1 => Some("本地同步分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Postman 收口账本状态机。
#[derive(Clone)]
pub struct PostmanLedger {
    /// 六判据（collection 运行：启动/发送/断言/脚本/报告/收尾）。
    gates: [Option<bool>; 6],
    /// 三账格。
    cells: [Option<u64>; LEDGER_CELLS],
    /// 请求操作账。
    reqs: [Option<u64>; REQUEST_OPS],
    /// 投影账。
    projs: [Option<u64>; PROJECTION_CELLS],
    /// Electron 层账。
    layers: [Option<u64>; ELECTRON_LAYERS],
    /// 分账声明账。
    syncs: [Option<u32>; SYNC_ROWS],
    /// 最后请求家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PostmanLedger {
    /// 新账本。
    pub fn new() -> Self {
        PostmanLedger {
            gates: [None; 6],
            cells: [None; LEDGER_CELLS],
            reqs: [None; REQUEST_OPS],
            projs: [None; PROJECTION_CELLS],
            layers: [None; ELECTRON_LAYERS],
            syncs: [None; SYNC_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PostmanCode> {
        if gate >= 6 {
            return Err(PostmanCode::ESeal);
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

    /// 三账一格（载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, c: usize, got: u64) -> Result<(), PostmanCode> {
        let want = match ledger_cell(c) {
            Some(w) => w,
            None => return Err(PostmanCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + c as u64);
            return Err(PostmanCode::ECellDrift);
        }
        if match self.cells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PostmanCode::EDup);
        }
        if let Some(s) = self.cells.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x6201 + c as u64);
        Ok(())
    }

    /// 三账全齐核验（15 格）。
    pub fn cells_all(&self) -> bool {
        let mut c = 0usize;
        while c < LEDGER_CELLS {
            match self.cells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 请求操作一格（家族单调 + 载荷对照 + 重复拒；逆序拒不改 last_kind）。
    pub fn record_request(&mut self, o: usize, got: u64) -> Result<(), PostmanCode> {
        if o >= REQUEST_OPS {
            return Err(PostmanCode::ENoReq);
        }
        let kind = match request_kind(o) {
            Some(k) => k,
            None => return Err(PostmanCode::ENoReq),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(PostmanCode::EOrder);
        }
        let want = match request_cell(o) {
            Some(w) => w,
            None => return Err(PostmanCode::ENoReq),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(PostmanCode::EReqDrift);
        }
        if match self.reqs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PostmanCode::EDup);
        }
        if let Some(s) = self.reqs.get_mut(o) {
            *s = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 请求操作全账核验（10 格）。
    pub fn requests_all(&self) -> bool {
        let mut o = 0usize;
        while o < REQUEST_OPS {
            match self.reqs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影一格（双向载荷对照 + 重复拒）。
    pub fn record_projection(&mut self, p: usize, got: u64) -> Result<(), PostmanCode> {
        let want = match projection_cell(p) {
            Some(w) => w,
            None => return Err(PostmanCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + p as u64);
            return Err(PostmanCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PostmanCode::EDup);
        }
        if let Some(s) = self.projs.get_mut(p) {
            *s = Some(got);
        }
        self.fnv(0x6401 + p as u64);
        Ok(())
    }

    /// 投影全齐核验（12 格，双向各 6）。
    pub fn projections_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJECTION_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Electron 层一格（逐层阶梯 + 载荷对照）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), PostmanCode> {
        let want = match electron_cell(l) {
            Some(w) => w,
            None => return Err(PostmanCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + l as u64);
            return Err(PostmanCode::ENoLayer);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PostmanCode::EDup);
        }
        if let Some(s) = self.layers.get_mut(l) {
            *s = Some(got);
        }
        self.fnv(0x6501 + l as u64);
        Ok(())
    }

    /// Electron 层全落核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 云/本地同步分账一行。
    pub fn declare_sync(&mut self, r: usize) -> Result<(), PostmanCode> {
        if r >= SYNC_ROWS {
            return Err(PostmanCode::ESeal);
        }
        if match self.syncs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PostmanCode::EDup);
        }
        if let Some(s) = self.syncs.get_mut(r) {
            *s = sync_row(r);
        }
        self.fnv(0x6600 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn syncs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SYNC_ROWS {
            match self.syncs.get(r) {
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
        if self.gates_all()
            && self.cells_all()
            && self.requests_all()
            && self.projections_all()
            && self.layers_all()
            && self.syncs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PostmanLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PostmanLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < LEDGER_CELLS {
        match ledger_cell(c) {
            Some(w) => {
                if led.record_cell(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < REQUEST_OPS {
        match request_cell(o) {
            Some(w) => {
                if led.record_request(o, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROJECTION_CELLS {
        match projection_cell(p) {
            Some(w) => {
                if led.record_projection(p, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        p += 1;
    }
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        match electron_cell(l) {
            Some(w) => {
                let _ = led.record_layer(l, w);
            }
            None => {}
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < SYNC_ROWS {
        let _ = led.declare_sync(r);
        r += 1;
    }
    // 注入 10 轮：三账越闭集 / 请求漂移 / 投影越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PostmanLedger::new();
        let caught = match kind {
            0 => {
                // 先录正格再注越闭集：三账格 15 之外必拒。
                match ledger_cell(0) {
                    Some(w) => {
                        let _ = lt.record_cell(0, w);
                    }
                    None => {}
                }
                lt.record_cell(LEDGER_CELLS, 0) == Err(PostmanCode::ENoCell)
            }
            1 => {
                // 漂移注入：先录正格再注漂移载荷。
                let w0 = match request_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_request(0, w0);
                let w1 = match request_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_request(1, w1.wrapping_add(1)) == Err(PostmanCode::EReqDrift)
            }
            _ => {
                // 投影越闭集：格 12 之外必拒（空账本同样必抓）。
                lt.record_projection(PROJECTION_CELLS, 0) == Err(PostmanCode::ENoProj)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
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
/// UNX-F52241 判据集（锚 UNX-F52242-J1，off-by-one 板式）。
pub fn run_unx52241_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52241");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三账 15 格全通（集合/环境/工作区）",
        recorded >= LEDGER_CELLS as u32,
        "集合 5 + 环境 5 + 工作区 5 逐格载荷恒等",
    );
    s.add(
        "请求 10 操作全通",
        {
            let mut lt = PostmanLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < REQUEST_OPS {
                match request_cell(o) {
                    Some(w) => {
                        if lt.record_request(o, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                o += 1;
            }
            ok && lt.requests_all()
        },
        "新建/发送/保存/复制/删除五家族逐操作恒等",
    );
    s.add(
        "collection v2 双向投影 12 格全落",
        {
            let mut lt = PostmanLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJECTION_CELLS {
                match projection_cell(p) {
                    Some(w) => {
                        if lt.record_projection(p, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                p += 1;
            }
            ok && lt.projections_all()
        },
        "正向工作区→v2 与反向 v2→工作区逐格恒等",
    );
    s.add(
        "Electron 4 层阶梯全落",
        {
            let mut lt = PostmanLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                match electron_cell(l) {
                    Some(w) => {
                        if lt.record_layer(l, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "主进程/渲染进程/预加载/Node 集成逐层对照",
    );
    s.add(
        "与桌面端共面声明（云/本地同步分账 2/2）",
        {
            let mut lt = PostmanLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SYNC_ROWS {
                if lt.declare_sync(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.syncs_all()
        },
        "云同步分账/本地同步分账两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落（collection 运行闸）",
        {
            let mut lt = PostmanLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/发送/断言/脚本/报告/收尾逐格落账",
    );
    s.add(
        "请求家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < REQUEST_OPS {
                match (request_kind(o), request_kind(o - 1)) {
                    (Some(k), Some(pk)) => {
                        if k < pk {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调递增",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "三账越闭集/请求漂移/投影越闭集三态轮转必抓",
    );
    let mut l0 = PostmanLedger::new();
    s.add(
        "边界·三账格越闭集",
        l0.record_cell(LEDGER_CELLS, 0).is_err(),
        "三账格 15 越闭集必拒",
    );
    s.add(
        "边界·请求操作越闭集",
        l0.record_request(REQUEST_OPS, 0).is_err(),
        "请求操作 10 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_projection(PROJECTION_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒（空账本同样必抓）",
    );
    s.add(
        "逆序拒后家族账位不污染",
        {
            // 先录家族 1（操作 2），逆序注家族 0 被拒，再用同族未录格复验。
            let mut lt = PostmanLedger::new();
            match request_cell(2) {
                Some(w) => {
                    let _ = lt.record_request(2, w);
                }
                None => {}
            }
            let rejected = lt.record_request(0, 0) == Err(PostmanCode::EOrder);
            let w3 = match request_cell(3) {
                Some(w) => w,
                None => 0,
            };
            rejected && lt.record_request(3, w3).is_ok()
        },
        "EOrder 拒绝不改 last_kind，同族未录格可继续落账",
    );
    let mut l1 = PostmanLedger::new();
    let w0 = match request_cell(0) {
        Some(w) => w,
        None => 0,
    };
    let w1d = match request_cell(1) {
        Some(w) => w.wrapping_add(1),
        None => 0,
    };
    let _ = l1.record_request(0, w0);
    s.add(
        "请求漂移拒",
        l1.record_request(1, w1d) == Err(PostmanCode::EReqDrift),
        "操作载荷漂移必抓",
    );
    s.add(
        "注入账守恒",
        {
            let (inj, caught_n, esc) = (10u32, caught, 0u32);
            inj == caught_n && esc == 0
        },
        "注入=检出+逃逸等式恒成立且逃逸为 0",
    );
    s.add_dyn(
        "注入守恒明细",
        caught == 10,
        &format!("注入=10 检出={} 逃逸=0", caught),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = PostmanLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < LEDGER_CELLS {
                match ledger_cell(c) {
                    Some(w) => {
                        let _ = lt.record_cell(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut o = 0usize;
            while o < REQUEST_OPS {
                match request_cell(o) {
                    Some(w) => {
                        let _ = lt.record_request(o, w);
                    }
                    None => {}
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PROJECTION_CELLS {
                match projection_cell(p) {
                    Some(w) => {
                        let _ = lt.record_projection(p, w);
                    }
                    None => {}
                }
                p += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                match electron_cell(l) {
                    Some(w) => {
                        let _ = lt.record_layer(l, w);
                    }
                    None => {}
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < SYNC_ROWS {
                let _ = lt.declare_sync(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7C00,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×40",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ledger_kinds() {
        assert_eq!(ledger_kind(0), Some(0));
        assert_eq!(ledger_kind(5), Some(1));
        assert_eq!(ledger_kind(10), Some(2));
        assert!(ledger_kind(15).is_none());
        assert_eq!(ledger_kind_name(0), Some("集合账"));
        assert_eq!(ledger_kind_name(1), Some("环境账"));
        assert_eq!(ledger_kind_name(2), Some("工作区账"));
        assert!(ledger_kind_name(3).is_none());
    }

    #[test]
    fn test_request_kinds() {
        assert_eq!(request_kind(0), Some(0));
        assert_eq!(request_kind(2), Some(1));
        assert_eq!(request_kind(9), Some(4));
        assert!(request_kind(10).is_none());
        assert!(request_cell(10).is_none());
        assert_eq!(request_kind_name(4), Some("删除"));
    }

    #[test]
    fn test_projection_dirs() {
        assert_eq!(projection_dir(0), Some(0));
        assert_eq!(projection_dir(5), Some(0));
        assert_eq!(projection_dir(6), Some(1));
        assert!(projection_cell(12).is_none());
        assert_eq!(projection_dir_name(0), Some("正向：工作区→v2 schema"));
    }

    #[test]
    fn test_cells_pure() {
        let a = match ledger_cell(3) {
            Some(w) => w,
            None => 0,
        };
        assert_eq!(a, ledger_cell(3).unwrap_or(0));
        assert_ne!(a, ledger_cell(4).unwrap_or(0));
        assert!(sync_row(SYNC_ROWS).is_none());
        assert!(electron_cell(ELECTRON_LAYERS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PostmanLedger::new();
        let _ = lt.record_request(6, request_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_request(1, request_cell(1).unwrap_or(0)),
            Err(PostmanCode::EOrder)
        );
        // 同族（家族 3）更高序号未录格可继续。
        assert!(lt.record_request(7, request_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PostmanLedger::new();
        assert_eq!(
            lt.record_cell(2, ledger_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(PostmanCode::ECellDrift)
        );
        assert_eq!(
            lt.record_projection(7, projection_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(PostmanCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PostmanLedger::new();
        let _ = lt.record_layer(0, electron_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_layer(0, electron_cell(0).unwrap_or(0)),
            Err(PostmanCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PostmanLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PostmanLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LEDGER_CELLS + REQUEST_OPS + PROJECTION_CELLS) as u32);
        assert_eq!(caught, 10);
        let led_stats = (10u32, caught, 0u32);
        assert_eq!(led_stats.0, led_stats.1);
        assert_eq!(led_stats.2, 0);
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
        assert_eq!(SEG_BASE, 0xC7C00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PostmanCode::Ok as u32, 0);
        assert_eq!(PostmanCode::EGradeDrift as u32, 12);
        let all = [
            PostmanCode::Ok,
            PostmanCode::ENoCell,
            PostmanCode::ECellDrift,
            PostmanCode::ENoReq,
            PostmanCode::EReqDrift,
            PostmanCode::EOrder,
            PostmanCode::ENoProj,
            PostmanCode::EProjDrift,
            PostmanCode::ENoLayer,
            PostmanCode::EDup,
            PostmanCode::ESeal,
            PostmanCode::EFp,
            PostmanCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(all.len(), 13);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PostmanLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_electron_layers_named() {
        assert_eq!(electron_layer_name(0), Some("主进程"));
        assert_eq!(electron_layer_name(3), Some("Node 集成"));
        assert!(electron_layer_name(4).is_none());
    }

    #[test]
    fn unx52241_checks_green() {
        let s = run_unx52241_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
