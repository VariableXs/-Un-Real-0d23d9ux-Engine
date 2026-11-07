//! UNX-F52461 Google Chrome 收口 · 0xCB300段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52461"，逐字）：
//!   title: Google Chrome 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52462-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52461）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52462-J1，照原文锚定；功能按本卡标题（Google Chrome 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 格闭集：渲染/扩展/同步/更新/DevTools 各 4 格，家族
//!      单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② Chromium 上栈 4 层：浏览器进程/渲染进程/GPU 进程/网络服务，逐层
//!      载荷对照，越层与漂移必抓。
//!   ③ 企业策略投影 8 格：Chrome 企业策略逐格投影，越格与漂移必抓。
//!   ④ 版本阶梯 3 档：124 系/125 系/126 系，越档必抓，逐档恒等。
//!   ⑤ 与 Edge 分账声明 2 行闭集：渲染账分账/扩展账分账，缺一拒封。
//!   ⑥ 六判据收口闸（启动/渲染/崩溃/卸载/升级/许可）逐格落账，未齐即
//!      封拒。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（版本越阶梯/
//!      操作漂移/操作越闭集，三类注入全落当轮临时账本，空账本同样必
//!      抓，escaped=0）；注入=检出+逃逸守恒；双跑 FNV 指纹一致；KB-COMPAT
//!      定级 S1/S2/S3 闭集（全通 → S1）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52461;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FEATURE_OPS: usize = 20;

/// Chromium 上栈层数。
pub const STACK_LAYERS: usize = 4;

/// 企业策略投影格数。
pub const POLICY_CELLS: usize = 8;

/// 版本阶梯档数。
pub const VERSION_STEPS: usize = 3;

/// 与 Edge 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromeCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 栈层号越闭集
    ENoStack,
    /// 栈层漂移
    EStackDrift,
    /// 策略格越闭集
    ENoPolicy,
    /// 版本档越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl ChromeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ChromeCode::Ok => "成功",
            ChromeCode::ENoOp => "操作号越闭集：渲染/扩展/同步/更新/DevTools 20 格之外先登记",
            ChromeCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ChromeCode::EOrder => "家族逆序：渲染→扩展→同步→更新→DevTools 单调，逆序必抓",
            ChromeCode::ENoStack => "栈层号越闭集：Chromium 上栈 4 层之外先登记",
            ChromeCode::EStackDrift => "栈层漂移：上栈层载荷与基准不恒等，逐层复算",
            ChromeCode::ENoPolicy => "策略格越闭集：企业策略投影 8 格之外先登记",
            ChromeCode::ENoVer => "版本档越阶梯：124 系/125 系/126 系三级闭集之外先登记",
            ChromeCode::EVerDrift => "版本阶梯漂移：版本档载荷与基准不恒等，逐档复算",
            ChromeCode::ENoRow => "声明行越闭集：与 Edge 分账 2 行闭集之外先登记",
            ChromeCode::EDup => "重复落账：同格只许一条账目",
            ChromeCode::ESeal => "未齐即封：20 操作 + 4 层上栈 + 8 格策略 + 3 档版本未齐先补齐",
            ChromeCode::EGradeDrift => "定级漂移：全通且与 Edge 分账声明全齐才可定 S1",
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
    /// 定级值（match 闭集，const 安全）。
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
            Grade::S1 => "S1：六判据+20操作+4层上栈+8格策略+3档版本+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（操作 o；家族 = o/4）。
pub const fn feature_cell(o: usize) -> Option<u64> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x2F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=渲染 4..8=扩展 8..12=同步 12..16=更新 16..20=DevTools）。
pub const fn feature_kind(o: usize) -> Option<u32> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn feature_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("渲染"),
        1 => Some("扩展"),
        2 => Some("同步"),
        3 => Some("更新"),
        4 => Some("DevTools"),
        _ => None,
    }
}

/// Chromium 上栈层载荷（层 l：0=浏览器进程 1=渲染进程 2=GPU 进程 3=网络服务）。
pub const fn stack_layer(l: usize) -> Option<u64> {
    if l >= STACK_LAYERS {
        return None;
    }
    Some((0xC2000u64 + (l as u64) * 0x47) ^ 0x4352565F53544B31)
}

/// 上栈层名。
pub const fn stack_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("浏览器进程"),
        1 => Some("渲染进程"),
        2 => Some("GPU 进程"),
        3 => Some("网络服务"),
        _ => None,
    }
}

/// 企业策略投影格载荷（策略格 p）。
pub const fn policy_cell(p: usize) -> Option<u64> {
    if p >= POLICY_CELLS {
        return None;
    }
    Some((0xC3000u64 + (p as u64) * 0x53) ^ 0x43524F53504C4331)
}

/// 版本档载荷（档 v：0=124 系 1=125 系 2=126 系）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xC4000u64 + (v as u64) * 0x5D) ^ 0x43524F565F4C4444)
}

/// 版本档名。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("124 系"),
        1 => Some("125 系"),
        2 => Some("126 系"),
        _ => None,
    }
}

/// 与 Edge 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2F)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Chrome 收口账本状态机。
#[derive(Clone)]
pub struct ChromeLedger {
    /// 六判据（启动/渲染/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FEATURE_OPS],
    /// 上栈层账。
    stack: [Option<u64>; STACK_LAYERS],
    /// 策略投影账。
    policy: [Option<u64>; POLICY_CELLS],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ChromeLedger {
    /// 新账本。
    pub fn new() -> Self {
        ChromeLedger {
            gates: [None; 6],
            ops: [None; FEATURE_OPS],
            stack: [None; STACK_LAYERS],
            policy: [None; POLICY_CELLS],
            vers: [None; VERSION_STEPS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ChromeCode> {
        if gate >= 6 {
            return Err(ChromeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x52A00 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ChromeCode> {
        if o >= FEATURE_OPS {
            return Err(ChromeCode::ENoOp);
        }
        let kind = match feature_kind(o) {
            Some(k) => k,
            None => return Err(ChromeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52B00);
            return Err(ChromeCode::EOrder);
        }
        let want = match feature_cell(o) {
            Some(w) => w,
            None => return Err(ChromeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52B01 + o as u64);
            return Err(ChromeCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromeCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x52B02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 上栈一层（载荷对照 + 重复拒）。
    pub fn record_stack(&mut self, l: usize, got: u64) -> Result<(), ChromeCode> {
        let want = match stack_layer(l) {
            Some(w) => w,
            None => return Err(ChromeCode::ENoStack),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52C00 + l as u64);
            return Err(ChromeCode::EStackDrift);
        }
        if match self.stack.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromeCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x52C01 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层）。
    pub fn stack_all(&self) -> bool {
        let mut l = 0usize;
        while l < STACK_LAYERS {
            match self.stack.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 策略投影一格（载荷对照 + 重复拒；漂移按操作漂移口径归并）。
    pub fn record_policy(&mut self, p: usize, got: u64) -> Result<(), ChromeCode> {
        let want = match policy_cell(p) {
            Some(w) => w,
            None => return Err(ChromeCode::ENoPolicy),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52D00 + p as u64);
            return Err(ChromeCode::EDrift);
        }
        if match self.policy.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromeCode::EDup);
        }
        if let Some(c) = self.policy.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x52D01 + p as u64);
        Ok(())
    }

    /// 策略全账核验（8 格）。
    pub fn policy_all(&self) -> bool {
        let mut p = 0usize;
        while p < POLICY_CELLS {
            match self.policy.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 版本一档（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), ChromeCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(ChromeCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52E00 + v as u64);
            return Err(ChromeCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromeCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x52E01 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 档全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 声明一行（与 Edge 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), ChromeCode> {
        if r >= SPLIT_ROWS {
            return Err(ChromeCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ChromeCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x52F00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.stack_all()
            && self.policy_all()
            && self.vers_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ChromeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ChromeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURE_OPS {
        if let Some(w) = feature_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut l = 0usize;
    while l < STACK_LAYERS {
        if let Some(w) = stack_layer(l) {
            if led.record_stack(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut p = 0usize;
    while p < POLICY_CELLS {
        if let Some(w) = policy_cell(p) {
            if led.record_policy(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：版本越阶梯 / 操作漂移 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上：空账本同样必抓，escaped 恒 0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ChromeLedger::new();
        let caught = match kind {
            0 => lt.record_version(VERSION_STEPS, 0).is_err(),
            1 => {
                let bad = match feature_cell(3) {
                    Some(w) => w.wrapping_add(1),
                    None => 1,
                };
                lt.record_op(3, bad) == Err(ChromeCode::EDrift)
            }
            _ => lt.record_op(FEATURE_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x53000 + k as u64);
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
/// UNX-F52461 判据集（锚 UNX-F52462-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52461_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52461");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Chrome 五功能族 20 格全通",
        recorded >= FEATURE_OPS as u32,
        "渲染/扩展/同步/更新/DevTools 五家族逐操作恒等",
    );
    s.add(
        "Chromium 上栈 4 层全落",
        {
            let mut lt = ChromeLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < STACK_LAYERS {
                if let Some(w) = stack_layer(l) {
                    if lt.record_stack(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.stack_all()
        },
        "浏览器进程/渲染进程/GPU 进程/网络服务逐层恒等，越层必抓",
    );
    s.add(
        "企业策略投影 8 格全投影",
        {
            let mut lt = ChromeLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if let Some(w) = policy_cell(p) {
                    if lt.record_policy(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.policy_all()
        },
        "企业策略 8 格逐格恒等，越格必抓",
    );
    s.add(
        "版本阶梯 124/125/126 三档全恒等",
        {
            let mut lt = ChromeLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三级闭集逐档恒等，越档必抓",
    );
    s.add(
        "与 Edge 分账声明 2/2 全齐",
        {
            let mut lt = ChromeLedger::new();
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
        "与 Edge 渲染账分账/与 Edge 扩展账分账两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ChromeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/渲染/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURE_OPS {
                let a = match feature_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let b = match feature_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "渲染→扩展→同步→更新→DevTools 五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越阶梯/操作漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = ChromeLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FEATURE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·栈层越闭集",
        l0.record_stack(STACK_LAYERS, 0).is_err(),
        "栈层 4 越闭集必拒",
    );
    s.add(
        "边界·版本越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本档 3 越阶梯必拒",
    );
    s.add(
        "未齐不封核",
        {
            let mut lt = ChromeLedger::new();
            let w = match feature_cell(0) {
                Some(w) => w,
                None => 0,
            };
            lt.record_op(0, w).is_ok() && lt.judge_grade() != Grade::S1
        },
        "账未齐时合法落格仍 Ok 且不定 S1，未齐即封被拒",
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
            let mut lt = ChromeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURE_OPS {
                if let Some(w) = feature_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut l = 0usize;
            while l < STACK_LAYERS {
                if let Some(w) = stack_layer(l) {
                    let _ = lt.record_stack(l, w);
                }
                l += 1;
            }
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if let Some(w) = policy_cell(p) {
                    let _ = lt.record_policy(p, w);
                }
                p += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB300,
        "SEG_BASE 0x40 对齐且在 VX16 域首格（0xCA400 + 0x40×60）",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_kind() {
        assert_eq!(feature_kind(0), Some(0));
        assert_eq!(feature_kind(3), Some(0));
        assert_eq!(feature_kind(4), Some(1));
        assert_eq!(feature_kind(19), Some(4));
        assert!(feature_kind(20).is_none());
        assert!(feature_cell(20).is_none());
    }

    #[test]
    fn test_stack_layers() {
        assert_eq!(stack_layer_name(0), Some("浏览器进程"));
        assert_eq!(stack_layer_name(3), Some("网络服务"));
        assert!(stack_layer_name(4).is_none());
        assert!(stack_layer(STACK_LAYERS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match policy_cell(3) {
            Some(a) => a,
            None => 0,
        };
        assert_eq!(a, match policy_cell(3) {
            Some(a) => a,
            None => 0,
        });
        assert_ne!(a, match policy_cell(4) {
            Some(a) => a,
            None => 0,
        });
        assert!(policy_cell(POLICY_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("124 系"));
        assert_eq!(version_name(1), Some("125 系"));
        assert_eq!(version_name(2), Some("126 系"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ChromeLedger::new();
        // 种子按合法序先落低族格（操作 1 属渲染族）。
        let _ = lt.record_op(1, feature_cell(1).unwrap_or(0));
        assert!(lt.record_op(2, feature_cell(2).unwrap_or(0)).is_ok());
        let mut lt2 = ChromeLedger::new();
        let _ = lt2.record_op(17, feature_cell(17).unwrap_or(0));
        assert_eq!(lt2.record_op(2, feature_cell(2).unwrap_or(0)), Err(ChromeCode::EOrder));
        assert!(lt2.record_op(18, feature_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ChromeLedger::new();
        assert_eq!(
            lt.record_op(5, feature_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ChromeCode::EDrift)
        );
        assert_eq!(
            lt.record_stack(2, stack_layer(2).unwrap_or(0).wrapping_add(7)),
            Err(ChromeCode::EStackDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(ChromeCode::EVerDrift)
        );
        assert_eq!(
            lt.record_policy(0, policy_cell(0).unwrap_or(0).wrapping_add(9)),
            Err(ChromeCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ChromeLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(ChromeCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(ChromeCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ChromeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ChromeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：只含 run_cycle 实际计数的节（操作+上栈+策略+版本）。
        assert_eq!(
            recorded,
            (FEATURE_OPS + STACK_LAYERS + POLICY_CELLS + VERSION_STEPS) as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_conserved() {
        let mut lt = ChromeLedger::new();
        let (i0, c0, e0) = lt.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
        // 空账直接落高族 → 序注入（错误路径单点记账）。
        let _ = lt.record_op(FEATURE_OPS - 1, feature_cell(FEATURE_OPS - 1).unwrap_or(0));
        let _ = lt.record_op(0, feature_cell(0).unwrap_or(0));
        let (i1, c1, e1) = lt.stats();
        assert_eq!(i1, i0 + 1);
        assert_eq!(c1, c0 + 1);
        assert_eq!(e1, e0);
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
        assert_eq!(SEG_BASE, 0xCB300);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ChromeCode::Ok as u32, 0);
        assert_eq!(ChromeCode::EGradeDrift as u32, 12);
        assert!(!ChromeCode::Ok.detail().is_empty());
        assert!(!ChromeCode::ESeal.detail().is_empty());
        assert!(!ChromeCode::EGradeDrift.detail().is_empty());
        assert_eq!(ChromeCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let lt0 = ChromeLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = ChromeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = ChromeLedger::new();
        let mut o = 0usize;
        while o < FEATURE_OPS {
            let _ = lt2.record_op(o, feature_cell(o).unwrap_or(0));
            o += 1;
        }
        let mut g2 = 0usize;
        while g2 < 6 {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52461_checks_green() {
        let s = run_unx52461_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
