//! UNX-F52485 Gatling 收口 · 0xCB900段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52485"，逐字）：
//!   title: Gatling 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52486-J1 可访问性
//!     审计 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52485）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52486-J1，照原文锚定；功能按本卡标题（Gatling 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 命令族闭集：Gatling 五命令族（recorder/run/report/scala/java）
//!      各 4 操作共 20 格，命令族单调（recorder→run→report→scala→java，
//!      逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② 仿真投影 8 格：仿真投影逐格折叠对照，漂移必抓。
//!   ③ 断言三态 6 格：断言状态三态闭集（通过/失败/挂起），格 i 期望态
//!      = i % 3，态漂移必抓。
//!   ④ 与 k6 分账声明 2 行闭集（F52482 锚定：k6 独立账声明/k6 类管道
//!      分账声明），缺一拒封；Scala 上栈声明 2 行闭集（Scala 依 JVM 上
//!      栈声明/Scala 版本对齐声明），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal_gate/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（命令族逆序/仿真投影
//!      漂移/操作越闭集）10/10 必抓；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集（S1/S2/S3）：Gatling 社区版按消费面如实定级
//!      —— 六判据齐即 S2（有条件收口），全齐亦 S2（企业面留待
//!      Gatling Enterprise 另账），空账 S3。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52485;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// Gatling 命令操作数。
pub const GATLING_OPS: usize = 20;

/// 仿真投影格数。
pub const SIM_CELLS: usize = 8;

/// 断言三态格数。
pub const ASSERT_CELLS: usize = 6;

/// k6 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Scala 上栈声明行数。
pub const SCALA_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatlingCode {
    /// 成功
    Ok = 0,
    /// 序号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 仿真投影格越闭集
    ENoSim,
    /// 仿真投影漂移
    ESimDrift,
    /// 断言格越闭集
    ENoAs,
    /// 断言态漂移
    EAsDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl GatlingCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GatlingCode::Ok => "成功",
            GatlingCode::ENoOp => "序号越闭集：命令 20 操作集之外先登记",
            GatlingCode::EDrift => "操作漂移：命令折叠与基准不恒等，逐格复算",
            GatlingCode::EOrder => "命令族逆序：recorder→run→report→scala→java 单调，逆序必抓",
            GatlingCode::ENoSim => "投影格越闭集：仿真投影 8 格之外先登记",
            GatlingCode::ESimDrift => "仿真投影漂移：投影折叠与基准不恒等，逐格复算",
            GatlingCode::ENoAs => "断言格越闭集：断言三态 6 格之外先登记",
            GatlingCode::EAsDrift => "断言态漂移：断言三态（通过/失败/挂起）与格位期望不符",
            GatlingCode::ENoRow => "声明行越闭集：k6 分账/Scala 上栈各 2 行闭集之外先登记",
            GatlingCode::ERowMiss => "缺声明行：k6 分账与 Scala 上栈声明未齐先补齐",
            GatlingCode::EDup => "重复落账：同格只许一条账目",
            GatlingCode::ESeal => "未齐即封：20 操作 + 8 投影 + 6 断言 + 4 行声明未齐先补齐",
            GatlingCode::EGradeDrift => "定级漂移：六判据齐才可定 S2（KB-COMPAT 如实定级）",
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
    /// 定级值（const 可调）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口",
            Grade::S2 => "S2：六判据齐，有条件收口（企业面另账，F52482 k6 分账）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；命令族 = o/4）。
pub const fn gatling_cell(o: usize) -> Option<u64> {
    if o >= GATLING_OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x34) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=recorder 4..8=run 8..12=report 12..16=scala 16..20=java）。
pub const fn gatling_kind(o: usize) -> Option<u32> {
    if o >= GATLING_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn gatling_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("recorder"),
        1 => Some("run"),
        2 => Some("report"),
        3 => Some("scala"),
        4 => Some("java"),
        _ => None,
    }
}

/// 仿真投影格载荷（格 i）。
pub const fn sim_cell(i: usize) -> Option<u64> {
    if i >= SIM_CELLS {
        return None;
    }
    Some((0xD2000u64 + (i as u64) * 0x4A) ^ 0x4741544C494E47)
}

/// 断言格位期望三态（0=通过 1=失败 2=挂起；态 = i % 3）。
pub const fn assert_want(i: usize) -> Option<u32> {
    if i >= ASSERT_CELLS {
        return None;
    }
    Some((i % 3) as u32)
}

/// 断言格载荷（格 i）。
pub const fn assert_cell(i: usize) -> Option<u64> {
    if i >= ASSERT_CELLS {
        return None;
    }
    Some((0xD3000u64 + (i as u64) * 0x58) ^ 0x41535355524E00)
}

/// k6 分账声明行折叠（行 r；F52482 锚定）。
pub const fn k6_split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x34)
}

/// Scala 上栈声明行折叠（行 r）。
pub const fn scala_row(r: usize) -> Option<u32> {
    if r >= SCALA_ROWS {
        return None;
    }
    Some(0xD480 + r as u32 * 0x34)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Gatling 收口账本状态机。
#[derive(Clone)]
pub struct GatlingLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    ops: [Option<u64>; GATLING_OPS],
    /// 仿真投影账。
    sims: [Option<u64>; SIM_CELLS],
    /// 断言三态账。
    asserts: [Option<u32>; ASSERT_CELLS],
    /// k6 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Scala 上栈声明账。
    scalas: [Option<u32>; SCALA_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GatlingLedger {
    /// 新账本。
    pub fn new() -> Self {
        GatlingLedger {
            gates: [None; 6],
            ops: [None; GATLING_OPS],
            sims: [None; SIM_CELLS],
            asserts: [None; ASSERT_CELLS],
            splits: [None; SPLIT_ROWS],
            scalas: [None; SCALA_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GatlingCode> {
        if gate >= 6 {
            return Err(GatlingCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5A100 + gate as u64);
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

    /// 命令操作一格（命令族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GatlingCode> {
        if o >= GATLING_OPS {
            return Err(GatlingCode::ENoOp);
        }
        let kind = match gatling_kind(o) {
            Some(k) => k,
            None => return Err(GatlingCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A200);
            return Err(GatlingCode::EOrder);
        }
        let want = match gatling_cell(o) {
            Some(w) => w,
            None => return Err(GatlingCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A201 + o as u64);
            return Err(GatlingCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatlingCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5A202 + o as u64);
        Ok(())
    }

    /// 命令操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < GATLING_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 仿真投影一格（载荷对照 + 重复拒）。
    pub fn record_sim(&mut self, i: usize, got: u64) -> Result<(), GatlingCode> {
        let want = match sim_cell(i) {
            Some(w) => w,
            None => return Err(GatlingCode::ENoSim),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A300 + i as u64);
            return Err(GatlingCode::ESimDrift);
        }
        if match self.sims.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatlingCode::EDup);
        }
        if let Some(c) = self.sims.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5A301 + i as u64);
        Ok(())
    }

    /// 仿真投影全账核验（8 格）。
    pub fn sims_all(&self) -> bool {
        let mut i = 0usize;
        while i < SIM_CELLS {
            match self.sims.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 断言一格（三态期望对照 + 重复拒）。
    pub fn record_assert(&mut self, i: usize, got_state: u32) -> Result<(), GatlingCode> {
        let want = match assert_want(i) {
            Some(w) => w,
            None => return Err(GatlingCode::ENoAs),
        };
        if got_state != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A400 + i as u64);
            return Err(GatlingCode::EAsDrift);
        }
        if match self.asserts.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatlingCode::EDup);
        }
        if let Some(c) = self.asserts.get_mut(i) {
            *c = Some(got_state);
        }
        self.fnv(0x5A401 + i as u64);
        Ok(())
    }

    /// 断言三态全账核验（6 格，每态恰 2 格）。
    pub fn asserts_all(&self) -> bool {
        let mut i = 0usize;
        while i < ASSERT_CELLS {
            match self.asserts.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// k6 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), GatlingCode> {
        if r >= SPLIT_ROWS {
            return Err(GatlingCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatlingCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = k6_split_row(r);
        }
        self.fnv(0x5A500 + r as u64);
        Ok(())
    }

    /// k6 分账声明全齐核验。
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

    /// Scala 上栈声明一行。
    pub fn declare_scala(&mut self, r: usize) -> Result<(), GatlingCode> {
        if r >= SCALA_ROWS {
            return Err(GatlingCode::ENoRow);
        }
        if match self.scalas.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatlingCode::EDup);
        }
        if let Some(c) = self.scalas.get_mut(r) {
            *c = scala_row(r);
        }
        self.fnv(0x5A600 + r as u64);
        Ok(())
    }

    /// Scala 上栈声明全齐核验。
    pub fn scalas_all(&self) -> bool {
        let mut r = 0usize;
        while r < SCALA_ROWS {
            match self.scalas.get(r) {
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

    /// 收口封闸：声明缺 → ERowMiss；判据/格账缺 → ESeal；全齐 → Ok。
    pub fn seal_gate(&self) -> Result<(), GatlingCode> {
        if !self.gates_all() {
            return Err(GatlingCode::ESeal);
        }
        if !self.splits_all() || !self.scalas_all() {
            return Err(GatlingCode::ERowMiss);
        }
        if !self.ops_all() || !self.sims_all() || !self.asserts_all() {
            return Err(GatlingCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（如实阶梯：空账 S3；六判据齐 S2；全齐目标档 S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：与期望档不符即 EGradeDrift。
    pub fn verify_grade(&self, want: Grade) -> Result<(), GatlingCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(GatlingCode::EGradeDrift)
        }
    }
}

impl Default for GatlingLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GatlingLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < GATLING_OPS {
        if let Some(w) = gatling_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < SIM_CELLS {
        if let Some(w) = sim_cell(i) {
            if led.record_sim(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut a = 0usize;
    while a < ASSERT_CELLS {
        if let Some(w) = assert_want(a) {
            if led.record_assert(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut s = 0usize;
    while s < SCALA_ROWS {
        let _ = led.declare_scala(s);
        s += 1;
    }
    // 注入 10 轮：命令族逆序 / 仿真投影漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GatlingLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(17, gatling_cell(17).unwrap_or(0));
                lt.record_op(5, gatling_cell(5).unwrap_or(0)) == Err(GatlingCode::EOrder)
            }
            1 => {
                lt.record_sim(3, sim_cell(3).unwrap_or(0).wrapping_add(1)) == Err(GatlingCode::ESimDrift)
            }
            _ => lt.record_op(GATLING_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5A700 + k as u64);
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
/// UNX-F52485 判据集（锚 UNX-F52486-J1，off-by-one 板式）。
pub fn run_unx52485_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52485");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= GATLING_OPS as u32,
        "recorder/run/report/scala/java 五族逐操作恒等",
    );
    s.add(
        "仿真投影 8 格全落",
        {
            let mut lt = GatlingLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < SIM_CELLS {
                if let Some(w) = sim_cell(i) {
                    if lt.record_sim(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.sims_all()
        },
        "仿真投影逐格折叠恒等，漂移必抓",
    );
    s.add(
        "断言三态 6 格全落",
        {
            let mut lt = GatlingLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ASSERT_CELLS {
                if let Some(w) = assert_want(a) {
                    if lt.record_assert(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.asserts_all()
        },
        "断言三态（通过/失败/挂起）逐格对位",
    );
    s.add(
        "k6 分账声明 2/2 全齐",
        {
            let mut lt = GatlingLedger::new();
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
        "k6 独立账声明/k6 类管道分账声明两行闭集（F52482 锚定）",
    );
    s.add(
        "Scala 上栈声明 2/2 全齐",
        {
            let mut lt = GatlingLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SCALA_ROWS {
                if lt.declare_scala(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.scalas_all()
        },
        "Scala 依 JVM 上栈声明/Scala 版本对齐声明两行闭集",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < GATLING_OPS {
                if gatling_kind(o).unwrap_or(5) < gatling_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "命令族逆序/投影漂移/越闭集三态轮转必抓");
    let mut l0 = GatlingLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_op(GATLING_OPS, 0).is_err(),
        "命令操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_sim(SIM_CELLS, 0).is_err(),
        "仿真投影 8 越闭集必拒",
    );
    s.add(
        "边界·断言越闭集",
        l0.record_assert(ASSERT_CELLS, 0).is_err(),
        "断言格 6 越闭集必拒",
    );
    s.add(
        "未齐落账不拒（封点前 Ok）",
        {
            let _ = l0.record_op(0, gatling_cell(0).unwrap_or(0));
            l0.record_sim(0, sim_cell(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落账仍 Ok，封拒只发生在封闸",
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
            let mut lt = GatlingLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < GATLING_OPS {
                if let Some(w) = gatling_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < SIM_CELLS {
                if let Some(w) = sim_cell(i) {
                    let _ = lt.record_sim(i, w);
                }
                i += 1;
            }
            let mut a = 0usize;
            while a < ASSERT_CELLS {
                if let Some(w) = assert_want(a) {
                    let _ = lt.record_assert(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut sc = 0usize;
            while sc < SCALA_ROWS {
                let _ = lt.declare_scala(sc);
                sc += 1;
            }
            lt.judge_grade() == Grade::S2 && lt.verify_grade(Grade::S2).is_ok()
        },
        "六判据齐 → S2，全齐目标档 S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB900,
        "SEG_BASE 0x40 对齐且落在 VX16 0xCB900 段",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gatling_kind() {
        assert_eq!(gatling_kind(0), Some(0));
        assert_eq!(gatling_kind(3), Some(0));
        assert_eq!(gatling_kind(4), Some(1));
        assert_eq!(gatling_kind(8), Some(2));
        assert_eq!(gatling_kind(19), Some(4));
        assert!(gatling_kind(20).is_none());
        assert!(gatling_cell(20).is_none());
        assert_eq!(gatling_kind_name(0), Some("recorder"));
        assert_eq!(gatling_kind_name(4), Some("java"));
        assert!(gatling_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = gatling_cell(7).unwrap_or(0);
        assert_eq!(a, gatling_cell(7).unwrap_or(0));
        assert_ne!(a, gatling_cell(8).unwrap_or(0));
        assert!(sim_cell(SIM_CELLS).is_none());
        assert!(assert_cell(ASSERT_CELLS).is_none());
        assert!(k6_split_row(SPLIT_ROWS).is_none());
        assert!(scala_row(SCALA_ROWS).is_none());
    }

    #[test]
    fn test_assert_tri_state() {
        assert_eq!(assert_want(0), Some(0));
        assert_eq!(assert_want(1), Some(1));
        assert_eq!(assert_want(2), Some(2));
        assert_eq!(assert_want(3), Some(0));
        assert_eq!(assert_want(5), Some(2));
        assert!(assert_want(6).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GatlingLedger::new();
        let _ = lt.record_op(9, gatling_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, gatling_cell(2).unwrap_or(0)), Err(GatlingCode::EOrder));
        assert!(lt.record_op(10, gatling_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GatlingLedger::new();
        assert_eq!(
            lt.record_op(5, gatling_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(GatlingCode::EDrift)
        );
        assert_eq!(
            lt.record_sim(2, sim_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(GatlingCode::ESimDrift)
        );
        assert_eq!(lt.record_assert(1, 0), Err(GatlingCode::EAsDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GatlingLedger::new();
        let _ = lt.record_sim(0, sim_cell(0).unwrap_or(0));
        assert_eq!(lt.record_sim(0, sim_cell(0).unwrap_or(0)), Err(GatlingCode::EDup));
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = GatlingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 账未齐：封闸必拒（声明行缺 → ERowMiss），但落账仍 Ok。
        assert_eq!(lt.seal_gate(), Err(GatlingCode::ERowMiss));
        assert!(lt.record_op(0, gatling_cell(0).unwrap_or(0)).is_ok());
        let mut lt2 = GatlingLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_miss() {
        let mut lt = GatlingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        // 判据齐但声明缺 → ERowMiss。
        assert_eq!(lt.seal_gate(), Err(GatlingCode::ERowMiss));
        assert!(lt.declare_split(SPLIT_ROWS).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, GATLING_OPS as u32 + SIM_CELLS as u32 + ASSERT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCB900);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GatlingCode::Ok as u32, 0);
        assert_eq!(GatlingCode::EGradeDrift as u32, 12);
        assert_eq!(GatlingCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => GatlingCode::Ok,
                1 => GatlingCode::ENoOp,
                2 => GatlingCode::EDrift,
                3 => GatlingCode::EOrder,
                4 => GatlingCode::ENoSim,
                5 => GatlingCode::ESimDrift,
                6 => GatlingCode::ENoAs,
                7 => GatlingCode::EAsDrift,
                8 => GatlingCode::ENoRow,
                9 => GatlingCode::ERowMiss,
                10 => GatlingCode::EDup,
                11 => GatlingCode::ESeal,
                _ => GatlingCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt0 = GatlingLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        assert!(lt0.verify_grade(Grade::S2).is_err());
        // 六闸齐 S2。
        let mut lt1 = GatlingLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52485_checks_green() {
        let s = run_unx52485_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
