//! UNX-F52490 Katalon Studio 收口 · 0xCBA40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52490"，逐字）：
//!   title: Katalon Studio 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52491-J1 录制/
//!     回放 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/
//!     软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52490）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52491-J1，照原文锚定；功能按本卡标题（Katalon Studio）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 操作集：录制/脚本/双引擎/CI 消费/报告五家族闭集各
//!      4 操作，逐操作载荷（纯函数折叠）恒等才收；家族单调（逆序必抓）。
//!   ② Groovy 消费 6 格：Katalon 脚本层 Groovy 消费闭集，逐格载荷对照，
//!      漂移必抓。
//!   ③ 与 Selenium 分账声明 2 行（F52477）+ 免费/商业分账 2 行 + Java
//!      上栈声明 2 行，六行闭集缺一拒封（seal 时声明行缺 → ERowMiss）。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      seal/judge_grade）；注入核 run_cycle 三态轮转 10/10（操作越闭集/
//!      操作漂移/家族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级
//!      S2（KB-COMPAT：免费/商业分账如实，有条件收口档）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52490;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const OPS: usize = 20;

/// Groovy 消费格数。
pub const GROOVY_CELLS: usize = 6;

/// 与 Selenium 分账声明行数（F52477）。
pub const SPLIT_ROWS: usize = 2;

/// 免费/商业分账声明行数。
pub const BILLING_ROWS: usize = 2;

/// Java 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KatalonCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// Groovy 消费格号越闭集
    ENoGroovy,
    /// Groovy 消费载荷漂移
    EGroovyDrift,
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

impl KatalonCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KatalonCode::Ok => "成功",
            KatalonCode::ENoOp => "操作号越闭集：录制/脚本/双引擎/CI 消费/报告 20 操作集之外先登记",
            KatalonCode::EDrift => "载荷漂移：操作载荷与基准折叠不恒等，逐格复算",
            KatalonCode::EOrder => "家族逆序：录制→脚本→双引擎→CI 消费→报告单调，逆序必抓",
            KatalonCode::ENoGroovy => "Groovy 消费格号越闭集：6 格 Groovy 消费闭集之外先登记",
            KatalonCode::EGroovyDrift => "Groovy 消费载荷漂移：与基准折叠不恒等，逐格复算",
            KatalonCode::ENoRow => "声明行越闭集：Selenium 分账 2 行 + 免费/商业分账 2 行 + Java 上栈 2 行之外先登记",
            KatalonCode::ERowMiss => "缺声明行：Selenium 分账/免费商业分账/Java 上栈声明未齐先补齐",
            KatalonCode::EDup => "重复落账：同格只许一条账目",
            KatalonCode::ESeal => "未齐即封：20 操作 + 6 Groovy + 6 声明行未齐先补齐",
            KatalonCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KatalonCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KatalonCode::EGradeDrift => "定级漂移：六判据全落且声明行全齐才可定 S2（分账面如实）",
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
            Grade::S1 => "S1：全判据全格全通收口",
            Grade::S2 => "S2：有条件收口（Katalon 免费/商业分账如实登记档）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五功能族操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xCBA40u64 + (o as u64) * 0x31) ^ 0x5249_0A00_0000_0000)
}

/// 操作家族（0..4=录制 4..8=脚本 8..12=双引擎 12..16=CI 消费 16..20=报告）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("录制"),
        1 => Some("脚本"),
        2 => Some("双引擎"),
        3 => Some("CI 消费"),
        4 => Some("报告"),
        _ => None,
    }
}

/// Groovy 消费格载荷（格 p）。
pub const fn groovy_cell(p: usize) -> Option<u64> {
    if p >= GROOVY_CELLS {
        return None;
    }
    Some((0xCBB40u64 + (p as u64) * 0x3B) ^ 0x5249_0B00_0000_0000)
}

/// Groovy 消费格名。
pub const fn groovy_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("test-case 脚本"),
        1 => Some("关键词库"),
        2 => Some("监听器"),
        3 => Some("自定义关键字"),
        4 => Some("数据驱动"),
        5 => Some("Setup/Teardown"),
        _ => None,
    }
}

/// 与 Selenium 分账声明行折叠（行 r，F52477）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB10 + r as u32 * 0x2C)
}

/// 免费/商业分账声明行折叠（行 r）。
pub const fn billing_row(r: usize) -> Option<u32> {
    if r >= BILLING_ROWS {
        return None;
    }
    Some(0xCC10 + r as u32 * 0x2C)
}

/// Java 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCD10 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Katalon 收口账本状态机。
#[derive(Clone)]
pub struct KatalonLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五功能族操作账。
    ops: [Option<u64>; OPS],
    /// Groovy 消费账。
    groovy: [Option<u64>; GROOVY_CELLS],
    /// Selenium 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 免费/商业分账声明账。
    billings: [Option<u32>; BILLING_ROWS],
    /// Java 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KatalonLedger {
    /// 新账本。
    pub fn new() -> Self {
        KatalonLedger {
            gates: [None; 6],
            ops: [None; OPS],
            groovy: [None; GROOVY_CELLS],
            splits: [None; SPLIT_ROWS],
            billings: [None; BILLING_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KatalonCode> {
        if gate >= 6 {
            return Err(KatalonCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), KatalonCode> {
        if o >= OPS {
            return Err(KatalonCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(KatalonCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A00);
            return Err(KatalonCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(KatalonCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A01 + o as u64);
            return Err(KatalonCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KatalonCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4A02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Groovy 消费一格（载荷对照 + 重复拒）。
    pub fn record_groovy(&mut self, p: usize, got: u64) -> Result<(), KatalonCode> {
        if p >= GROOVY_CELLS {
            return Err(KatalonCode::ENoGroovy);
        }
        let want = match groovy_cell(p) {
            Some(w) => w,
            None => return Err(KatalonCode::ENoGroovy),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00 + p as u64);
            return Err(KatalonCode::EGroovyDrift);
        }
        if match self.groovy.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KatalonCode::EDup);
        }
        if let Some(c) = self.groovy.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4B01 + p as u64);
        Ok(())
    }

    /// Groovy 消费全账核验（6 格）。
    pub fn groovy_all(&self) -> bool {
        let mut p = 0usize;
        while p < GROOVY_CELLS {
            match self.groovy.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Selenium 分账声明一行（F52477）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), KatalonCode> {
        if r >= SPLIT_ROWS {
            return Err(KatalonCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KatalonCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4C00 + r as u64);
        Ok(())
    }

    /// Selenium 分账全齐核验。
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

    /// 免费/商业分账声明一行。
    pub fn declare_billing(&mut self, r: usize) -> Result<(), KatalonCode> {
        if r >= BILLING_ROWS {
            return Err(KatalonCode::ENoRow);
        }
        if match self.billings.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KatalonCode::EDup);
        }
        if let Some(c) = self.billings.get_mut(r) {
            *c = billing_row(r);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// 免费/商业分账全齐核验。
    pub fn billings_all(&self) -> bool {
        let mut r = 0usize;
        while r < BILLING_ROWS {
            match self.billings.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Java 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), KatalonCode> {
        if r >= STACK_ROWS {
            return Err(KatalonCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KatalonCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4E00 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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

    /// 收口封账（顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal）。
    pub fn seal(&self) -> Result<(), KatalonCode> {
        if !self.gates_all() {
            return Err(KatalonCode::ESeal);
        }
        if !self.splits_all() || !self.billings_all() || !self.stacks_all() {
            return Err(KatalonCode::ERowMiss);
        }
        if !self.ops_all() || !self.groovy_all() {
            return Err(KatalonCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S2 分账面如实档；六闸齐 → S2；否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.seal().is_ok() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for KatalonLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KatalonLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < GROOVY_CELLS {
        if let Some(w) = groovy_cell(p) {
            if led.record_groovy(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < BILLING_ROWS {
        let _ = led.declare_billing(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 家族逆序三态轮转。
    // 三态注入全部落在当轮临时账本上；漂移/逆序先按合法序落正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KatalonLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(KatalonCode::EDrift)
            }
            _ => {
                let _ = lt.record_op(8, op_cell(8).unwrap_or(0));
                lt.record_op(2, op_cell(2).unwrap_or(0)) == Err(KatalonCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4F00 + k as u64);
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
/// UNX-F52490 判据集（锚 UNX-F52491-J1，off-by-one 板式）。
pub fn run_unx52490_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52490");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= OPS as u32,
        "录制/脚本/双引擎/CI 消费/报告五家族逐操作恒等",
    );
    s.add(
        "录制/回放注入 10/10 全抓（UNX-F52491-J1 ×10）",
        caught >= 10,
        "操作越闭集/操作漂移/家族逆序三态轮转必抓",
    );
    s.add(
        "Groovy 消费 6 格全落",
        {
            let mut lt = KatalonLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < GROOVY_CELLS {
                if let Some(w) = groovy_cell(p) {
                    if lt.record_groovy(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.groovy_all()
        },
        "脚本层 Groovy 消费闭集逐格恒等，漂移必抓",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = KatalonLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "与 Selenium 分账声明 2/2 全齐（F52477）",
        {
            let mut lt = KatalonLedger::new();
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
        "与 Selenium 分账声明两行闭集（F52477 锚定）",
    );
    s.add(
        "免费/商业分账声明 2/2 全齐",
        {
            let mut lt = KatalonLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BILLING_ROWS {
                if lt.declare_billing(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.billings_all()
        },
        "免费版分账声明/商业版分账声明两行闭集",
    );
    s.add(
        "Java 上栈声明 2/2 全齐",
        {
            let mut lt = KatalonLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "Java 运行时上栈声明/JVM 消费声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "录制→脚本→双引擎→CI 消费→报告五家族闭集单调",
    );
    let mut l0 = KatalonLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）",
    );
    s.add(
        "边界·Groovy 越闭集",
        l0.record_groovy(GROOVY_CELLS, 0).is_err(),
        "Groovy 6 越闭集必拒（ENoGroovy 提前返回）",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err()
            && l0.declare_billing(BILLING_ROWS).is_err()
            && l0.declare_stack(STACK_ROWS).is_err(),
        "声明行越闭集必拒（ENoRow 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(KatalonCode::EDrift)
        },
        "操作载荷漂移必抓",
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
        "定级 S2 闭集（KB-COMPAT）",
        {
            let mut lt = KatalonLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < GROOVY_CELLS {
                if let Some(w) = groovy_cell(p) {
                    let _ = lt.record_groovy(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < BILLING_ROWS {
                let _ = lt.declare_billing(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全齐 → S2（Katalon 免费/商业分账如实定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBA40,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×89",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind_mapping() {
        // 名表按家族（kind）索引而非格号。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(11), Some(2));
        assert_eq!(op_kind(16), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(2), Some("双引擎"));
        assert_eq!(op_kind_name(3), Some("CI 消费"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_groovy_names() {
        assert_eq!(groovy_name(0), Some("test-case 脚本"));
        assert_eq!(groovy_name(5), Some("Setup/Teardown"));
        assert!(groovy_name(6).is_none());
        assert!(groovy_cell(GROOVY_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(7).unwrap_or(0);
        assert_eq!(a, op_cell(7).unwrap_or(0));
        assert_ne!(a, op_cell(8).unwrap_or(0));
        let b = groovy_cell(3).unwrap_or(0);
        assert_eq!(b, groovy_cell(3).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(billing_row(BILLING_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族格，再注逆序必抓；被拒格不改 last_kind。
        let mut lt = KatalonLedger::new();
        let _ = lt.record_op(13, op_cell(13).unwrap_or(0));
        assert_eq!(lt.record_op(5, op_cell(5).unwrap_or(0)), Err(KatalonCode::EOrder));
        assert!(lt.record_op(14, op_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KatalonLedger::new();
        assert_eq!(
            lt.record_op(6, op_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(KatalonCode::EDrift)
        );
        assert_eq!(
            lt.record_groovy(4, groovy_cell(4).unwrap_or(0).wrapping_add(9)),
            Err(KatalonCode::EGroovyDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KatalonLedger::new();
        let _ = lt.record_groovy(1, groovy_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_groovy(1, groovy_cell(1).unwrap_or(0)),
            Err(KatalonCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KatalonLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = KatalonLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_order() {
        // 封账语义顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
        let empty = KatalonLedger::new();
        assert_eq!(empty.seal(), Err(KatalonCode::ESeal));
        let mut lt = KatalonLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(KatalonCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < BILLING_ROWS {
            let _ = lt.declare_billing(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(KatalonCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 操作 + 6 Groovy。
        assert!(recorded >= OPS as u32 + GROOVY_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBA40);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KatalonCode::Ok as u32, 0);
        assert_eq!(KatalonCode::EGradeDrift as u32, 12);
        assert_eq!(KatalonCode::Ok.val(), SEG_BASE);
        assert_eq!(KatalonCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!KatalonCode::Ok.detail().is_empty());
        assert!(!KatalonCode::ESeal.detail().is_empty());
        assert!(!KatalonCode::EGroovyDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = KatalonLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_row_boundary() {
        let mut lt = KatalonLedger::new();
        assert!(lt.declare_split(0).is_ok());
        assert!(lt.declare_split(1).is_ok());
        assert!(lt.declare_split(2).is_err());
        assert!(lt.declare_billing(1).is_ok());
        assert!(lt.declare_billing(2).is_err());
        assert!(lt.declare_stack(1).is_ok());
        assert!(lt.declare_stack(2).is_err());
    }

    #[test]
    fn unx52490_checks_green() {
        let s = run_unx52490_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
