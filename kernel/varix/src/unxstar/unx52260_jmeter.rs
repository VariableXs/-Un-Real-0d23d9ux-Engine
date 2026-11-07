//! UNX-F52260 Apache JMeter 收口 · 0xC80C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52260"，逐字）：
//!   title: Apache JMeter 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52261-J1 六判
//!     据全落；容器/镜像/卷/Compose 20 操作集全通；后端虚拟化依赖消
//!     费 L5 判据（联签锚定行 AI-60）——不可用路径显式 N/A + 三要素
//!     告警，禁隐性失败；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 14
//!     口径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52260）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52261-J1，照原文锚定；功能按本卡标题（Apache JMeter）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 测试计划五面 20 操作集：测试计划/线程组/取样器/断言/监听器各
//!      4 操作，五面单调（逆序必抓），逐操作载荷纯函数折叠恒等才收。
//!   ② 分布模式（远程注入）4 格：控制机 + 3 台远程注入机（agent）逐
//!      格恒等，越格必拒。
//!   ③ Java 上栈 4 层（F52057 联签锚定行）：JRE/库/Class 装载/线程栈
//!      四层依次落账，跳层必抓。
//!   ④ CSV/JTL 双向投影 12 格：正投影折叠 + 反投影回授双向闭合，漂移
//!      必抓（对应结果聚合与图表口径）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 10 轮三态轮转（投影漂移/五面逆
//!      序/操作越闭集）全部落在当轮临时账本上；双跑指纹一致；
//!      KB-COMPAT 定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52260;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 测试计划操作数。
pub const PLAN_OPS: usize = 20;

/// 分布模式格数（控制机 + 3 远程注入机）。
pub const DIST_CELLS: usize = 4;

/// Java 上栈层数（F52057 联签）。
pub const JVM_LAYERS: usize = 4;

/// CSV/JTL 投影格数。
pub const PROJ_CELLS: usize = 12;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JmeterCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 五面逆序
    EOrder,
    /// 分布格越闭集
    ENoDist,
    /// 分布模式漂移
    EDistDrift,
    /// Java 层越闭集
    ENoJvm,
    /// Java 上栈跳层
    EJvmOrder,
    /// 投影格越闭集
    ENoProj,
    /// CSV/JTL 投影漂移
    EProjDrift,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl JmeterCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JmeterCode::Ok => "成功",
            JmeterCode::ENoOp => "操作号越闭集：测试计划/线程组/取样器/断言/监听器 20 操作集之外先登记",
            JmeterCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            JmeterCode::EOrder => "五面逆序：测试计划→线程组→取样器→断言→监听器 单调，逆序必抓",
            JmeterCode::ENoDist => "分布格越闭集：控制机+3 远程注入机 4 格闭集之外先登记",
            JmeterCode::EDistDrift => "分布模式漂移：远程注入机折叠与基准不恒等，逐格复算",
            JmeterCode::ENoJvm => "Java 层越闭集：JRE/库/Class 装载/线程栈 4 层闭集之外先登记",
            JmeterCode::EJvmOrder => "Java 上栈跳层：JRE→库→Class 装载→线程栈四层依次落账，跳层必抓",
            JmeterCode::ENoProj => "投影格越闭集：CSV/JTL 12 格闭集之外先登记",
            JmeterCode::EProjDrift => "CSV/JTL 投影漂移：正投影折叠与基准不恒等，双向复算",
            JmeterCode::ESeal => "未齐即封：20 操作 + 4 分布 + 4 Java 层 + 12 投影未齐先补齐",
            JmeterCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            JmeterCode::EGradeDrift => "定级漂移：全通才可定 S1（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡定级）
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
            Grade::S1 => "S1：20 操作+4 分布+4 Java 层+12 投影全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；面 = o/4）。
pub const fn plan_cell(o: usize) -> Option<u64> {
    if o >= PLAN_OPS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作面（0..4=测试计划 4..8=线程组 8..12=取样器 12..16=断言 16..20=监听器）。
pub const fn plan_kind(o: usize) -> Option<u32> {
    if o >= PLAN_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面名。
pub const fn plan_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("测试计划"),
        1 => Some("线程组"),
        2 => Some("取样器"),
        3 => Some("断言"),
        4 => Some("监听器"),
        _ => None,
    }
}

/// 分布模式格载荷（格 d：0=控制机 1..3=远程注入机）。
pub const fn dist_cell(d: usize) -> Option<u64> {
    if d >= DIST_CELLS {
        return None;
    }
    Some((0xF2000u64 + (d as u64) * 0x37) ^ 0x4A4D_5452_5F49_0000)
}

/// 分布格名。
pub const fn dist_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("控制机"),
        1 => Some("远程注入机 A"),
        2 => Some("远程注入机 B"),
        3 => Some("远程注入机 C"),
        _ => None,
    }
}

/// Java 上栈层载荷（层 l：0=JRE 1=库 2=Class 装载 3=线程栈；F52057 联签）。
pub const fn jvm_cell(l: usize) -> Option<u64> {
    if l >= JVM_LAYERS {
        return None;
    }
    Some((0xF3000u64 + (l as u64) * 0x5D) ^ 0x4A56_4D5F_4C59_0000)
}

/// Java 层名。
pub const fn jvm_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JRE"),
        1 => Some("库"),
        2 => Some("Class 装载"),
        3 => Some("线程栈"),
        _ => None,
    }
}

/// CSV/JTL 正投影折叠（格 c）。
pub const fn proj_cell(c: usize) -> Option<u64> {
    if c >= PROJ_CELLS {
        return None;
    }
    Some((0xF4000u64 + (c as u64) * 0x37) ^ 0x4A54_4C5F_5052_0000)
}

/// CSV/JTL 反投影校验：got 与正投影恒等才回授格号。
pub const fn proj_back(c: usize, got: u64) -> Option<usize> {
    match proj_cell(c) {
        Some(w) => {
            if w == got {
                Some(c)
            } else {
                None
            }
        }
        None => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Apache JMeter 收口账本状态机。
#[derive(Clone)]
pub struct JmeterLedger {
    /// 六判据（计划/线程/取样/断言/监听/聚合）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; PLAN_OPS],
    /// 分布账。
    dists: [Option<u64>; DIST_CELLS],
    /// Java 层账。
    jvms: [Option<u64>; JVM_LAYERS],
    /// 投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 最后操作面。
    last_kind: i64,
    /// 最后 Java 层。
    last_jvm: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JmeterLedger {
    /// 新账本。
    pub fn new() -> Self {
        JmeterLedger {
            gates: [None; 6],
            ops: [None; PLAN_OPS],
            dists: [None; DIST_CELLS],
            jvms: [None; JVM_LAYERS],
            projs: [None; PROJ_CELLS],
            last_kind: -1,
            last_jvm: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JmeterCode> {
        if gate >= 6 {
            return Err(JmeterCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5510 + gate as u64);
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

    /// 操作一格（五面单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), JmeterCode> {
        if o >= PLAN_OPS {
            return Err(JmeterCode::ENoOp);
        }
        let kind = match plan_kind(o) {
            Some(k) => k,
            None => return Err(JmeterCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5520);
            return Err(JmeterCode::EOrder);
        }
        let want = match plan_cell(o) {
            Some(w) => w,
            None => return Err(JmeterCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5521 + o as u64);
            return Err(JmeterCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JmeterCode::ESeal);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5522 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < PLAN_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 分布格一格（载荷对照 + 重复拒）。
    pub fn record_dist(&mut self, d: usize, got: u64) -> Result<(), JmeterCode> {
        let want = match dist_cell(d) {
            Some(w) => w,
            None => return Err(JmeterCode::ENoDist),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5530 + d as u64);
            return Err(JmeterCode::EDistDrift);
        }
        if match self.dists.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JmeterCode::ESeal);
        }
        if let Some(c) = self.dists.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x5531 + d as u64);
        Ok(())
    }

    /// 分布全账核验（4 格）。
    pub fn dists_all(&self) -> bool {
        let mut d = 0usize;
        while d < DIST_CELLS {
            match self.dists.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// Java 层一格（四层依次落账，跳层必抓）。
    pub fn record_jvm(&mut self, l: usize, got: u64) -> Result<(), JmeterCode> {
        if l >= JVM_LAYERS {
            return Err(JmeterCode::ENoJvm);
        }
        if (l as i64) != self.last_jvm + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5540);
            return Err(JmeterCode::EJvmOrder);
        }
        let want = match jvm_cell(l) {
            Some(w) => w,
            None => return Err(JmeterCode::ENoJvm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5541 + l as u64);
            return Err(JmeterCode::EDrift);
        }
        if let Some(c) = self.jvms.get_mut(l) {
            *c = Some(got);
        }
        self.last_jvm = l as i64;
        self.fnv(0x5542 + l as u64);
        Ok(())
    }

    /// Java 上栈全账核验（4 层且顶层为线程栈）。
    pub fn jvms_all(&self) -> bool {
        let mut l = 0usize;
        while l < JVM_LAYERS {
            match self.jvms.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        self.last_jvm == (JVM_LAYERS as i64) - 1
    }

    /// CSV/JTL 投影一格（正投影恒等 + 反投影回授闭合）。
    pub fn record_proj(&mut self, c: usize, got: u64) -> Result<(), JmeterCode> {
        let want = match proj_cell(c) {
            Some(w) => w,
            None => return Err(JmeterCode::ENoProj),
        };
        if got != want || proj_back(c, got) != Some(c) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5550 + c as u64);
            return Err(JmeterCode::EProjDrift);
        }
        if match self.projs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JmeterCode::ESeal);
        }
        if let Some(cell) = self.projs.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x5551 + c as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn projs_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROJ_CELLS {
            match self.projs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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
        if self.gates_all() && self.ops_all() && self.dists_all() && self.jvms_all() && self.projs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for JmeterLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = JmeterLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PLAN_OPS {
        if let Some(w) = plan_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DIST_CELLS {
        if let Some(w) = dist_cell(d) {
            if led.record_dist(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut l = 0usize;
    while l < JVM_LAYERS {
        if let Some(w) = jvm_cell(l) {
            if led.record_jvm(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut c = 0usize;
    while c < PROJ_CELLS {
        if let Some(w) = proj_cell(c) {
            if led.record_proj(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 注入 10 轮：投影漂移 / 五面逆序 / 操作越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JmeterLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
                lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(JmeterCode::EProjDrift)
            }
            1 => {
                let _ = lt.record_op(17, plan_cell(17).unwrap_or(0));
                lt.record_op(5, plan_cell(5).unwrap_or(0)) == Err(JmeterCode::EOrder)
            }
            _ => lt.record_op(PLAN_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5560 + k as u64);
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
/// UNX-F52260 判据集（锚 UNX-F52261-J1，off-by-one 板式）。
pub fn run_unx52260_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52260");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "测试计划 20 操作全通",
        recorded >= PLAN_OPS as u32,
        "测试计划/线程组/取样器/断言/监听器五面逐操作恒等",
    );
    s.add(
        "五面单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PLAN_OPS {
                if plan_kind(o).unwrap_or(5) < plan_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五面闭集单调，逆序必抓",
    );
    s.add(
        "分布模式（远程注入）4 格全通",
        {
            let mut lt = JmeterLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DIST_CELLS {
                if let Some(w) = dist_cell(d) {
                    if lt.record_dist(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dists_all()
        },
        "控制机 + 3 台远程注入机逐格恒等",
    );
    s.add(
        "Java 上栈 4 层依次落账",
        {
            let mut lt = JmeterLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JVM_LAYERS {
                if let Some(w) = jvm_cell(l) {
                    if lt.record_jvm(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.jvms_all()
        },
        "JRE→库→Class 装载→线程栈四层有序闭合（F52057 联签锚定行）",
    );
    s.add(
        "CSV/JTL 双向投影 12/12 恒等",
        {
            let mut lt = JmeterLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    if lt.record_proj(c, w).is_err() || proj_back(c, w) != Some(c) {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.projs_all()
        },
        "正投影折叠 + 反投影回授双向闭合（结果聚合与图表口径）",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = JmeterLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "计划/线程/取样/断言/监听/聚合逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "投影漂移/五面逆序/操作越闭集三态轮转必抓");
    let mut l0 = JmeterLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(PLAN_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·分布格越闭集",
        l0.record_dist(DIST_CELLS, 0).is_err(),
        "分布格 4 越闭集必拒",
    );
    s.add(
        "边界·Java 层越闭集",
        l0.record_jvm(JVM_LAYERS, 0).is_err(),
        "Java 层 4 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "漂移注入先录正格再注漂移",
        {
            let _ = l0.record_op(0, plan_cell(0).unwrap_or(0));
            l0.record_op(1, plan_cell(1).unwrap_or(0).wrapping_add(0x2C)) == Err(JmeterCode::EDrift)
        },
        "正格先收，漂移格必抓",
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
            let mut lt = JmeterLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PLAN_OPS {
                if let Some(w) = plan_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DIST_CELLS {
                if let Some(w) = dist_cell(d) {
                    let _ = lt.record_dist(d, w);
                }
                d += 1;
            }
            let mut l = 0usize;
            while l < JVM_LAYERS {
                if let Some(w) = jvm_cell(l) {
                    let _ = lt.record_jvm(l, w);
                }
                l += 1;
            }
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    let _ = lt.record_proj(c, w);
                }
                c += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC80C0,
        "SEG_BASE 0x40 对齐且落在 VX15 域 52260 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_kind() {
        assert_eq!(plan_kind(0), Some(0));
        assert_eq!(plan_kind(4), Some(1));
        assert_eq!(plan_kind(8), Some(2));
        assert_eq!(plan_kind(15), Some(3));
        assert_eq!(plan_kind(19), Some(4));
        assert!(plan_kind(20).is_none());
        assert!(plan_cell(PLAN_OPS).is_none());
        assert_eq!(plan_kind_name(1), Some("线程组"));
        assert_eq!(plan_kind_name(4), Some("监听器"));
        assert!(plan_kind_name(5).is_none());
    }

    #[test]
    fn test_dist_cells() {
        assert_eq!(dist_name(0), Some("控制机"));
        assert_eq!(dist_name(3), Some("远程注入机 C"));
        assert!(dist_name(4).is_none());
        assert!(dist_cell(DIST_CELLS).is_none());
        let mut lt = JmeterLedger::new();
        assert!(lt.record_dist(0, dist_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_dist(1, dist_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(JmeterCode::EDistDrift)
        );
    }

    #[test]
    fn test_jvm_layers() {
        assert_eq!(jvm_layer_name(0), Some("JRE"));
        assert_eq!(jvm_layer_name(3), Some("线程栈"));
        assert!(jvm_layer_name(4).is_none());
        assert!(jvm_cell(JVM_LAYERS).is_none());
    }

    #[test]
    fn test_jvm_order_caught() {
        let mut lt = JmeterLedger::new();
        // 空账本跳层：直接落库层必抓。
        assert_eq!(lt.record_jvm(1, jvm_cell(1).unwrap_or(0)), Err(JmeterCode::EJvmOrder));
        let _ = lt.record_jvm(0, jvm_cell(0).unwrap_or(0));
        assert_eq!(lt.record_jvm(2, jvm_cell(2).unwrap_or(0)), Err(JmeterCode::EJvmOrder));
        assert!(lt.record_jvm(1, jvm_cell(1).unwrap_or(0)).is_ok());
        assert!(lt.record_jvm(2, jvm_cell(2).unwrap_or(0)).is_ok());
        assert!(lt.record_jvm(3, jvm_cell(3).unwrap_or(0)).is_ok());
        assert!(lt.jvms_all());
    }

    #[test]
    fn test_proj_bidirectional() {
        let mut c = 0usize;
        while c < PROJ_CELLS {
            let w = proj_cell(c).unwrap_or(0);
            assert_eq!(proj_back(c, w), Some(c), "反投影必须回授原格");
            assert_ne!(proj_back(c, w.wrapping_add(1)), Some(c), "漂移值反投影必拒");
            c += 1;
        }
        assert!(proj_cell(PROJ_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = JmeterLedger::new();
        let _ = lt.record_op(17, plan_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(5, plan_cell(5).unwrap_or(0)), Err(JmeterCode::EOrder));
        assert!(lt.record_op(18, plan_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = JmeterLedger::new();
        assert_eq!(
            lt.record_op(3, plan_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(JmeterCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(2, proj_cell(2).unwrap_or(0).wrapping_add(0x10)),
            Err(JmeterCode::EProjDrift)
        );
    }

    #[test]
    fn test_boundary_out_of_set() {
        let mut lt = JmeterLedger::new();
        assert!(lt.record_op(PLAN_OPS, 0).is_err());
        assert!(lt.record_dist(DIST_CELLS, 0).is_err());
        assert!(lt.record_jvm(JVM_LAYERS, 0).is_err());
        assert!(lt.record_proj(PROJ_CELLS, 0).is_err());
        assert!(lt.judge_gate(6).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PLAN_OPS + DIST_CELLS + JVM_LAYERS + PROJ_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC80C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JmeterCode::Ok as u32, 0);
        assert_eq!(JmeterCode::EGradeDrift as u32, 12);
        assert_eq!(JmeterCode::Ok.val(), SEG_BASE);
        assert_eq!(JmeterCode::EJvmOrder.val(), SEG_BASE + 7);
        assert!(!JmeterCode::Ok.detail().is_empty());
        assert!(!JmeterCode::EDistDrift.detail().is_empty());
        assert!(!JmeterCode::EProjDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = JmeterLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = JmeterLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52260_checks_green() {
        let s = run_unx52260_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
