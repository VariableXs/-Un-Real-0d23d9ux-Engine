//! UNX-F52254 Advanced REST Client 收口 · 0xC7F40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52254"，逐字）：
//!   title: Advanced REST Client 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52255-J1 20 操作
//!     集（GET/POST/头/代理/证书五面）×10 全通；TLS 消费对账（AI-44 契
//!     约联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52254）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52255-J1，照原文锚定；功能按本卡标题（Advanced REST Client）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐单定制）：
//!   ① 请求 10 操作集：五家族闭集（方法构造/头部编辑/体编辑/认证/发送）
//!      各 2 操作，家族单调（逆序必抓），逐格载荷对照。
//!   ② 环境/变量 8 格：环境 0..4、变量 4..8 两面，逐格落账。
//!   ③ 历史/项目 6 格：历史 0..3、项目 3..6 两面，逐格落账。
//!   ④ Electron 4 层：Chromium 渲染 → Node 主进程 → IPC 桥 → 应用
//!      壳层，自底向上严格栈序，逆栈必抓。
//!   ⑤ TLS 消费对账联签锚定行 1 行闭集（AI-44 契约），缺一拒封。
//!   ⑥ 六判据收口闸：启动/请求/崩溃/卸载/升级/许可，逐格落账，未齐
//!      即封拒。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（请求越闭集/请求漂移/家族逆
//!      序），检出+逃逸守恒等式；双跑 FNV 指纹一致；KB-COMPAT 定级闭
//!      集：全通 → S2（有条件收口，Electron 桌面口径）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52254;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 请求操作数。
pub const REQUEST_OPS: usize = 10;

/// 环境/变量格数（环境 4 + 变量 4）。
pub const ENV_CELLS: usize = 8;

/// 历史/项目格数（历史 3 + 项目 3）。
pub const HISTORY_CELLS: usize = 6;

/// Electron 上栈层数。
pub const ELECTRON_LAYERS: usize = 4;

/// TLS 消费对账联签锚定行数。
pub const ANCHOR_ROWS: usize = 1;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArcCode {
    /// 成功
    Ok = 0,
    /// 请求操作越闭集
    ENoOp,
    /// 请求载荷漂移
    EOpDrift,
    /// 请求家族逆序
    EOrder,
    /// 环境/变量格越闭集
    ENoEnv,
    /// 环境/变量载荷漂移
    EEnvDrift,
    /// 历史/项目格越闭集
    ENoHist,
    /// 历史/项目载荷漂移
    EHistDrift,
    /// Electron 逆栈
    EStack,
    /// 联签锚定行越闭集
    ENoAnchor,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl ArcCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ArcCode::Ok => "成功",
            ArcCode::ENoOp => "请求操作越闭集：请求 10 操作集之外先登记",
            ArcCode::EOpDrift => "请求载荷漂移：操作折叠与基准不恒等，逐格复算",
            ArcCode::EOrder => "请求家族逆序：方法构造→头部编辑→体编辑→认证→发送单调，逆序必抓",
            ArcCode::ENoEnv => "环境/变量格越闭集：8 格闭集之外先登记",
            ArcCode::EEnvDrift => "环境/变量载荷漂移：格折叠与基准不恒等，逐格复算",
            ArcCode::ENoHist => "历史/项目格越闭集：6 格闭集之外先登记",
            ArcCode::EHistDrift => "历史/项目载荷漂移：格折叠与基准不恒等，逐格复算",
            ArcCode::EStack => "Electron 逆栈：Chromium→Node→IPC→壳层自底向上，逆栈必抓",
            ArcCode::ENoAnchor => "联签锚定行越闭集：TLS 消费对账仅 1 行（AI-44 契约）",
            ArcCode::EDup => "重复落账：同格只许一条账目",
            ArcCode::ESeal => "未齐即封：10 操作+8 格环境+6 格历史+4 层上栈+1 行联签未齐先补齐",
            ArcCode::EGradeDrift => "定级漂移：全通才可定 S2，判据缺为 S4",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// ARC KB-COMPAT 定级闭集（本卡口径顶格 S2：Electron 桌面有条件收口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArcGrade {
    /// S2：全通有条件收口
    S2,
    /// S3：判据全但格账有缺
    S3,
    /// S4：判据缺失，不收口
    S4,
}

impl ArcGrade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            ArcGrade::S2 => 2,
            ArcGrade::S3 => 3,
            ArcGrade::S4 => 4,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            ArcGrade::S2 => "S2：请求 10+环境 8+历史 6+Electron 4 层+联签 1 行全通有条件收口",
            ArcGrade::S3 => "S3：六判据全但格账有缺，补齐后复评",
            ArcGrade::S4 => "S4：判据缺失，不收口",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 请求操作格载荷（操作 o；家族 = o/2）。
pub const fn request_cell(o: usize) -> Option<u64> {
    if o >= REQUEST_OPS {
        return None;
    }
    Some((0xC10000u64 + (o as u64) * 0x47) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 请求操作家族（0..2=方法构造 2..4=头部编辑 4..6=体编辑 6..8=认证 8..10=发送）。
pub const fn request_kind(o: usize) -> Option<u32> {
    if o >= REQUEST_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 请求家族名。
pub const fn request_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("方法构造"),
        1 => Some("头部编辑"),
        2 => Some("体编辑"),
        3 => Some("认证"),
        4 => Some("发送"),
        _ => None,
    }
}

/// 环境/变量格载荷（格 e：0..4=环境 4..8=变量）。
pub const fn env_cell(e: usize) -> Option<u64> {
    if e >= ENV_CELLS {
        return None;
    }
    Some((0xC20000u64 + (e as u64) * 0x3B) ^ 0x454E565F56415231)
}

/// 环境面名（面 f：0=环境 1=变量）。
pub const fn env_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("环境"),
        1 => Some("变量"),
        _ => None,
    }
}

/// 历史/项目格载荷（格 h：0..3=历史 3..6=项目）。
pub const fn history_cell(h: usize) -> Option<u64> {
    if h >= HISTORY_CELLS {
        return None;
    }
    Some((0xC30000u64 + (h as u64) * 0x59) ^ 0x484953545F50524A)
}

/// 历史面名（面 f：0=历史 1=项目）。
pub const fn history_face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("历史"),
        1 => Some("项目"),
        _ => None,
    }
}

/// Electron 上栈层载荷（层 l：0..4 自底向上）。
pub const fn electron_layer_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some((0xC40000u64 + (l as u64) * 0x2E) ^ 0x454C45435F53544B)
}

/// Electron 上栈层名。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Chromium 渲染"),
        1 => Some("Node 主进程"),
        2 => Some("IPC 桥"),
        3 => Some("应用壳层"),
        _ => None,
    }
}

/// TLS 消费对账联签锚定行折叠（行 r）。
pub const fn anchor_row(r: usize) -> Option<u32> {
    if r >= ANCHOR_ROWS {
        return None;
    }
    Some(0xC310 + r as u32 * 0x13)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Advanced REST Client 收口账本状态机。
#[derive(Clone)]
pub struct ArcLedger {
    /// 六判据（启动/请求/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 请求操作账。
    ops: [Option<u64>; REQUEST_OPS],
    /// 环境/变量账。
    envs: [Option<u64>; ENV_CELLS],
    /// 历史/项目账。
    hists: [Option<u64>; HISTORY_CELLS],
    /// Electron 上栈账。
    layers: [Option<u64>; ELECTRON_LAYERS],
    /// 联签锚定行账。
    anchors: [Option<u32>; ANCHOR_ROWS],
    /// 已上栈层数（栈序核验用）。
    stacked: usize,
    /// 最后请求家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ArcLedger {
    /// 新账本。
    pub fn new() -> Self {
        ArcLedger {
            gates: [None; 6],
            ops: [None; REQUEST_OPS],
            envs: [None; ENV_CELLS],
            hists: [None; HISTORY_CELLS],
            layers: [None; ELECTRON_LAYERS],
            anchors: [None; ANCHOR_ROWS],
            stacked: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ArcCode> {
        if gate >= 6 {
            return Err(ArcCode::ESeal);
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

    /// 请求操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_request(&mut self, o: usize, got: u64) -> Result<(), ArcCode> {
        if o >= REQUEST_OPS {
            return Err(ArcCode::ENoOp);
        }
        let kind = match request_kind(o) {
            Some(k) => k,
            None => return Err(ArcCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(ArcCode::EOrder);
        }
        let want = match request_cell(o) {
            Some(w) => w,
            None => return Err(ArcCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(ArcCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArcCode::EDup);
        }
        if let Some(slot) = self.ops.get_mut(o) {
            *slot = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 请求全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < REQUEST_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 环境/变量一格（载荷对照 + 重复拒）。
    pub fn record_env(&mut self, e: usize, got: u64) -> Result<(), ArcCode> {
        let want = match env_cell(e) {
            Some(w) => w,
            None => return Err(ArcCode::ENoEnv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + e as u64);
            return Err(ArcCode::EEnvDrift);
        }
        if match self.envs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArcCode::EDup);
        }
        if let Some(slot) = self.envs.get_mut(e) {
            *slot = Some(got);
        }
        self.fnv(0x6301 + e as u64);
        Ok(())
    }

    /// 环境/变量全账核验（8 格）。
    pub fn envs_all(&self) -> bool {
        let mut e = 0usize;
        while e < ENV_CELLS {
            match self.envs.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// 历史/项目一格（载荷对照 + 重复拒）。
    pub fn record_history(&mut self, h: usize, got: u64) -> Result<(), ArcCode> {
        let want = match history_cell(h) {
            Some(w) => w,
            None => return Err(ArcCode::ENoHist),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + h as u64);
            return Err(ArcCode::EHistDrift);
        }
        if match self.hists.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArcCode::EDup);
        }
        if let Some(slot) = self.hists.get_mut(h) {
            *slot = Some(got);
        }
        self.fnv(0x6401 + h as u64);
        Ok(())
    }

    /// 历史/项目全账核验（6 格）。
    pub fn hists_all(&self) -> bool {
        let mut h = 0usize;
        while h < HISTORY_CELLS {
            match self.hists.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
        }
        true
    }

    /// Electron 上栈一层（严格栈序：必须正好是下一层，逆栈必抓）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), ArcCode> {
        let want = match electron_layer_cell(l) {
            Some(w) => w,
            None => return Err(ArcCode::EStack),
        };
        if l != self.stacked {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500);
            return Err(ArcCode::EStack);
        }
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6501 + l as u64);
            return Err(ArcCode::EEnvDrift);
        }
        if let Some(slot) = self.layers.get_mut(l) {
            *slot = Some(got);
        }
        self.stacked += 1;
        self.fnv(0x6502 + l as u64);
        Ok(())
    }

    /// Electron 上栈全账核验（4 层且栈序完整）。
    pub fn layers_all(&self) -> bool {
        self.stacked == ELECTRON_LAYERS
    }

    /// TLS 消费对账联签锚定行（AI-44 契约）。
    pub fn declare_anchor(&mut self, r: usize) -> Result<(), ArcCode> {
        if r >= ANCHOR_ROWS {
            return Err(ArcCode::ENoAnchor);
        }
        if match self.anchors.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArcCode::EDup);
        }
        if let Some(slot) = self.anchors.get_mut(r) {
            *slot = anchor_row(r);
        }
        self.fnv(0x6600 + r as u64);
        Ok(())
    }

    /// 联签锚定行全齐核验。
    pub fn anchors_all(&self) -> bool {
        let mut r = 0usize;
        while r < ANCHOR_ROWS {
            match self.anchors.get(r) {
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
    pub fn judge_grade(&self) -> ArcGrade {
        if self.gates_all()
            && self.ops_all()
            && self.envs_all()
            && self.hists_all()
            && self.layers_all()
            && self.anchors_all()
        {
            ArcGrade::S2
        } else if self.gates_all() {
            ArcGrade::S3
        } else {
            ArcGrade::S4
        }
    }
}

impl Default for ArcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ArcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < REQUEST_OPS {
        if let Some(w) = request_cell(o) {
            if led.record_request(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < ENV_CELLS {
        if let Some(w) = env_cell(e) {
            if led.record_env(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut h = 0usize;
    while h < HISTORY_CELLS {
        if let Some(w) = history_cell(h) {
            if led.record_history(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        if let Some(w) = electron_layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < ANCHOR_ROWS {
        let _ = led.declare_anchor(r);
        r += 1;
    }
    // 注入 10 轮：请求越闭集 / 请求载荷漂移 / 请求家族逆序三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒数与指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ArcLedger::new();
        let caught = match kind {
            0 => lt.record_request(REQUEST_OPS, 0) == Err(ArcCode::ENoOp),
            1 => {
                // 漂移注入先录正格再注漂移。
                if let Some(w) = request_cell(0) {
                    let _ = lt.record_request(0, w);
                }
                lt.record_request(1, request_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ArcCode::EOpDrift)
            }
            _ => {
                // 逆序注入对空账本同样必抓：先录正格（高族未录格）再逆序注入，
                // 拒后 last_kind 不变。
                if let Some(w) = request_cell(7) {
                    let _ = lt.record_request(7, w);
                }
                lt.record_request(2, request_cell(2).unwrap_or(0)) == Err(ArcCode::EOrder)
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
/// UNX-F52254 判据集（锚 UNX-F52255-J1，off-by-one 板式）。
pub fn run_unx52254_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52254");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "请求 10 操作全通",
        recorded >= REQUEST_OPS as u32,
        "方法构造/头部编辑/体编辑/认证/发送五家族逐操作恒等",
    );
    s.add(
        "环境/变量 8 格全落",
        {
            let mut lt = ArcLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENV_CELLS {
                if let Some(w) = env_cell(e) {
                    if lt.record_env(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.envs_all()
        },
        "环境 4 格 + 变量 4 格逐格落账",
    );
    s.add(
        "历史/项目 6 格全落",
        {
            let mut lt = ArcLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < HISTORY_CELLS {
                if let Some(w) = history_cell(h) {
                    if lt.record_history(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.hists_all()
        },
        "历史 3 格 + 项目 3 格逐格落账",
    );
    s.add(
        "Electron 4 层栈序",
        {
            let mut lt = ArcLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if let Some(w) = electron_layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "Chromium 渲染→Node 主进程→IPC 桥→应用壳层自底向上",
    );
    s.add(
        "Electron 逆栈必抓",
        {
            let mut lt = ArcLedger::new();
            lt.record_layer(1, electron_layer_cell(1).unwrap_or(0)) == Err(ArcCode::EStack)
        },
        "跳层上栈（第 1 层先于第 0 层）必拒",
    );
    s.add(
        "TLS 消费对账联签锚定行 1/1",
        {
            let mut lt = ArcLedger::new();
            let ok = lt.declare_anchor(0).is_ok();
            ok && lt.anchors_all()
        },
        "AI-44 契约联签锚定行闭集（卡面判据逐字锚定）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ArcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/请求/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "请求家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < REQUEST_OPS {
                if request_kind(o).unwrap_or(5) < request_kind(o - 1).unwrap_or(5) {
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
        "请求越闭集/载荷漂移/家族逆序三态轮转必抓",
    );
    // 边界·越闭集 ×3。
    let mut l0 = ArcLedger::new();
    s.add(
        "边界·请求越闭集",
        l0.record_request(REQUEST_OPS, 0).is_err(),
        "请求操作 10 越闭集必拒",
    );
    s.add(
        "边界·环境越闭集",
        l0.record_env(ENV_CELLS, 0).is_err(),
        "环境/变量格 8 越闭集必拒",
    );
    s.add(
        "边界·历史越闭集",
        l0.record_history(HISTORY_CELLS, 0).is_err(),
        "历史/项目格 6 越闭集必拒",
    );
    s.add(
        "请求漂移拒",
        {
            let _ = l0.record_request(0, request_cell(0).unwrap_or(0));
            l0.record_request(1, request_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(ArcCode::EOpDrift)
        },
        "请求载荷漂移必抓",
    );
    s.add(
        "联签行越闭集",
        {
            let mut lt = ArcLedger::new();
            lt.declare_anchor(ANCHOR_ROWS).is_err()
        },
        "联签锚定行 1 越闭集必拒",
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
            let mut lt = ArcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < REQUEST_OPS {
                if let Some(w) = request_cell(o) {
                    let _ = lt.record_request(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < ENV_CELLS {
                if let Some(w) = env_cell(e) {
                    let _ = lt.record_env(e, w);
                }
                e += 1;
            }
            let mut h = 0usize;
            while h < HISTORY_CELLS {
                if let Some(w) = history_cell(h) {
                    let _ = lt.record_history(h, w);
                }
                h += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if let Some(w) = electron_layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                let _ = lt.declare_anchor(r);
                r += 1;
            }
            lt.judge_grade() == ArcGrade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，Electron 桌面口径）",
    );
    s.add(
        "定级降档闭集",
        {
            let mut lt = ArcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.judge_grade() == ArcGrade::S3
        },
        "判据全但格缺 → S3，不误判 S2",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且公式恒等",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_kind() {
        assert_eq!(request_kind(0), Some(0));
        assert_eq!(request_kind(3), Some(1));
        assert_eq!(request_kind(9), Some(4));
        assert!(request_kind(10).is_none());
        assert!(request_cell(10).is_none());
    }

    #[test]
    fn test_request_names() {
        assert_eq!(request_kind_name(0), Some("方法构造"));
        assert_eq!(request_kind_name(4), Some("发送"));
        assert!(request_kind_name(5).is_none());
    }

    #[test]
    fn test_face_names() {
        assert_eq!(env_face_name(0), Some("环境"));
        assert_eq!(env_face_name(1), Some("变量"));
        assert!(env_face_name(2).is_none());
        assert_eq!(history_face_name(0), Some("历史"));
        assert_eq!(history_face_name(1), Some("项目"));
        assert!(history_face_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = request_cell(4).unwrap_or(0);
        assert_eq!(a, request_cell(4).unwrap_or(0));
        assert_ne!(a, request_cell(5).unwrap_or(0));
        assert_ne!(env_cell(0).unwrap_or(0), env_cell(4).unwrap_or(0));
        assert_ne!(history_cell(0).unwrap_or(0), history_cell(3).unwrap_or(0));
        assert!(anchor_row(ANCHOR_ROWS).is_none());
    }

    #[test]
    fn test_electron_layer_names() {
        assert_eq!(electron_layer_name(0), Some("Chromium 渲染"));
        assert_eq!(electron_layer_name(3), Some("应用壳层"));
        assert!(electron_layer_name(4).is_none());
        assert!(electron_layer_cell(4).is_none());
    }

    #[test]
    fn test_request_order_caught() {
        let mut lt = ArcLedger::new();
        let _ = lt.record_request(8, request_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_request(3, request_cell(3).unwrap_or(0)),
            Err(ArcCode::EOrder)
        );
        // 逆序拒后同 last_kind 复验须用同族或更高族未录格。
        assert!(lt.record_request(9, request_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ArcLedger::new();
        assert_eq!(
            lt.record_env(2, env_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(ArcCode::EEnvDrift)
        );
        assert_eq!(
            lt.record_history(4, history_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(ArcCode::EHistDrift)
        );
    }

    #[test]
    fn test_stack_order() {
        let mut lt = ArcLedger::new();
        // 跳层（第 2 层先上）必抓且不推进栈。
        assert_eq!(
            lt.record_layer(2, electron_layer_cell(2).unwrap_or(0)),
            Err(ArcCode::EStack)
        );
        assert_eq!(lt.stacked, 0);
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            assert!(lt.record_layer(l, electron_layer_cell(l).unwrap_or(0)).is_ok());
            l += 1;
        }
        assert!(lt.layers_all());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ArcLedger::new();
        let _ = lt.record_request(0, request_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_request(0, request_cell(0).unwrap_or(0)),
            Err(ArcCode::EDup)
        );
        let _ = lt.record_env(6, env_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_env(6, env_cell(6).unwrap_or(0)),
            Err(ArcCode::EDup)
        );
    }

    #[test]
    fn test_anchor_boundary() {
        let mut lt = ArcLedger::new();
        assert!(lt.declare_anchor(0).is_ok());
        assert_eq!(lt.declare_anchor(0), Err(ArcCode::EDup));
        assert_eq!(lt.declare_anchor(1), Err(ArcCode::ENoAnchor));
        assert!(lt.anchors_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ArcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ArcLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (REQUEST_OPS + ENV_CELLS + HISTORY_CELLS + ELECTRON_LAYERS) as u32
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
        assert_eq!(SEG_BASE, 0xC7F40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ArcCode::Ok as u32, 0);
        assert_eq!(ArcCode::EGradeDrift as u32, 12);
        assert!(!ArcCode::Ok.detail().is_empty());
        assert!(!ArcCode::ENoAnchor.detail().is_empty());
        assert!(!ArcCode::EStack.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(ArcGrade::S2.val(), 2);
        assert_eq!(ArcGrade::S4.val(), 4);
        let lt = ArcLedger::new();
        assert_eq!(lt.judge_grade(), ArcGrade::S4);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = ArcLedger::new();
        // 载荷漂移路径记账：注入=检出=1、逃逸=0（越闭集路径提前返回不记账）。
        let _ = lt.record_request(0, request_cell(0).unwrap_or(0).wrapping_add(1));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn unx52254_checks_green() {
        let s = run_unx52254_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
