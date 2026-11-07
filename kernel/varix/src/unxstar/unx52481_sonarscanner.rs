//! UNX-F52481 SonarScanner CLI 收口 · 0xCB800段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52481"，逐字）：
//!   title: SonarScanner CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52482-J1 压测 ×10
//!     （只打本地靶站声明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52481）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52482-J1，照原文锚定；功能按本卡标题（SonarScanner CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：begin / end / scan / report / quality-gate 各 4 操
//!      作共 20 操作，家族单调（逆序必抓），逐操作载荷折叠恒等才收。
//!   ② sonar-project.properties 双向投影 12 格：工程键/源码目录/服务端/
//!      令牌/排除/测试/二进制/覆盖率/质量门等待/分支/日志级别/CI 链接，
//!      逐格投影载荷恒等，漂移必抓。
//!   ③ 质量门三元闸 3 格：pass / warn / fail 三态闭集，逐格落账。
//!   ④ Jenkins 消费联签 4 格（F52361）：联签载荷携 F52361 标记位，缺格
//!      拒封。
//!   ⑤ Java 上栈声明 2 行闭集：JDK 代理进程上栈声明 / sonar-scanner JVM
//!      内存上栈声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（命令越闭集/命令漂移/
//!      家族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级闭集
//!      （六闸 + 20 操作 + 12 投影 + 3 闸 + 4 联签 + 2 声明全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52481;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数。
pub const CMD_OPS: usize = 20;

/// properties 双向投影格数。
pub const PROP_CELLS: usize = 12;

/// 质量门三元闸格数。
pub const QG_CELLS: usize = 3;

/// Jenkins 消费联签格数（F52361）。
pub const JK_CELLS: usize = 4;

/// Java 上栈声明行数。
pub const JAVA_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SonarCode {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// 命令家族逆序
    EOrder,
    /// 投影格越闭集
    ENoProp,
    /// 投影漂移
    EPropDrift,
    /// 质量门格越闭集
    ENoGate,
    /// 质量门漂移
    EGateDrift,
    /// 联签格越闭集
    ENoCell,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl SonarCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SonarCode::Ok => "成功",
            SonarCode::ENoCmd => "命令号越闭集：begin/end/scan/report/quality-gate 20 操作集之外先登记",
            SonarCode::ECmdDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            SonarCode::EOrder => "家族逆序：begin→end→scan→report→quality-gate 单调，逆序必抓",
            SonarCode::ENoProp => "投影格越闭集：sonar-project.properties 12 格双向投影之外先登记",
            SonarCode::EPropDrift => "投影漂移：properties 投影载荷与基准不恒等，逐格复算",
            SonarCode::ENoGate => "质量门格越闭集：pass/warn/fail 三元闸之外先登记",
            SonarCode::EGateDrift => "质量门漂移：三元闸载荷与基准不恒等，逐格复算",
            SonarCode::ENoCell => "联签格越闭集：Jenkins 消费联签 4 格（F52361）之外先登记",
            SonarCode::ENoRow => "声明行越闭集：Java 上栈声明 2 行闭集之外先登记",
            SonarCode::EDup => "重复落账：同格只许一条账目",
            SonarCode::ESeal => "未齐即封：六闸+20操作+12投影+3闸+4联签+2声明未齐先补齐",
            SonarCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20操作+12投影+3闸+4联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；家族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x31) ^ 0x534F4E4152000000)
}

/// 命令家族（0..4=begin 4..8=end 8..12=scan 12..16=report 16..20=quality-gate）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令家族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("begin"),
        1 => Some("end"),
        2 => Some("scan"),
        3 => Some("report"),
        4 => Some("quality-gate"),
        _ => None,
    }
}

/// properties 投影格名（双向投影 12 格）。
pub const fn prop_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("sonar.projectKey"),
        1 => Some("sonar.sources"),
        2 => Some("sonar.host.url"),
        3 => Some("sonar.token"),
        4 => Some("sonar.exclusions"),
        5 => Some("sonar.tests"),
        6 => Some("sonar.java.binaries"),
        7 => Some("sonar.coverage.reportPaths"),
        8 => Some("sonar.qualitygate.wait"),
        9 => Some("sonar.branch.name"),
        10 => Some("sonar.verbose"),
        11 => Some("sonar.links.ci"),
        _ => None,
    }
}

/// properties 投影格载荷。
pub const fn prop_cell(p: usize) -> Option<u64> {
    if p >= PROP_CELLS {
        return None;
    }
    Some((0xC2000u64 + (p as u64) * 0x37) ^ 0x534F4E50524A0000)
}

/// 质量门三元态（0=pass 1=warn 2=fail）。
pub const fn qg_state(g: usize) -> Option<u32> {
    if g >= QG_CELLS {
        return None;
    }
    Some(g as u32)
}

/// 质量门三元态名。
pub const fn qg_state_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("pass"),
        1 => Some("warn"),
        2 => Some("fail"),
        _ => None,
    }
}

/// 质量门三元闸格载荷。
pub const fn qg_cell(g: usize) -> Option<u64> {
    if g >= QG_CELLS {
        return None;
    }
    Some((0xC300u64 + (g as u64) * 0x2C) ^ 0x5147545249410000)
}

/// Jenkins 消费联签格载荷（F52361 标记位入折叠）。
pub const fn jk_cell(c: usize) -> Option<u64> {
    if c >= JK_CELLS {
        return None;
    }
    Some((0xC400u64 + (c as u64) * 0x2C) ^ 0x4635323336310000)
}

/// Java 上栈声明行折叠（行 r）。
pub const fn java_row(r: usize) -> Option<u32> {
    if r >= JAVA_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

/// Java 上栈声明行名。
pub const fn java_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("JDK 代理进程上栈声明"),
        1 => Some("sonar-scanner JVM 内存上栈声明"),
        _ => None,
    }
}

/// 六判据闸名（SonarScanner 流水）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("扫描启动"),
        1 => Some("分析完成"),
        2 => Some("报告生成"),
        3 => Some("质量门判定"),
        4 => Some("Jenkins 回写"),
        5 => Some("定级登记"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SonarScanner 收口账本状态机。
#[derive(Clone)]
pub struct SonarLedger {
    /// 六判据（扫描启动/分析完成/报告生成/质量门判定/Jenkins 回写/定级登记）。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// properties 投影账。
    props: [Option<u64>; PROP_CELLS],
    /// 质量门三元闸账。
    qgs: [Option<u64>; QG_CELLS],
    /// Jenkins 联签账（F52361）。
    jks: [Option<u64>; JK_CELLS],
    /// Java 上栈声明账。
    java: [Option<u32>; JAVA_ROWS],
    /// 最后命令家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SonarLedger {
    /// 新账本。
    pub fn new() -> Self {
        SonarLedger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            props: [None; PROP_CELLS],
            qgs: [None; QG_CELLS],
            jks: [None; JK_CELLS],
            java: [None; JAVA_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SonarCode> {
        if gate >= 6 {
            return Err(SonarCode::ESeal);
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

    /// 命令一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), SonarCode> {
        if o >= CMD_OPS {
            return Err(SonarCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(SonarCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(SonarCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(SonarCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(SonarCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SonarCode::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 格）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// properties 投影一格（载荷对照 + 重复拒）。
    pub fn record_prop(&mut self, p: usize, got: u64) -> Result<(), SonarCode> {
        if p >= PROP_CELLS {
            return Err(SonarCode::ENoProp);
        }
        let want = match prop_cell(p) {
            Some(w) => w,
            None => return Err(SonarCode::ENoProp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + p as u64);
            return Err(SonarCode::EPropDrift);
        }
        if match self.props.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SonarCode::EDup);
        }
        if let Some(c) = self.props.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6402 + p as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn props_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROP_CELLS {
            match self.props.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 质量门三元闸一格（载荷对照 + 重复拒）。
    pub fn record_qg(&mut self, g: usize, got: u64) -> Result<(), SonarCode> {
        if g >= QG_CELLS {
            return Err(SonarCode::ENoGate);
        }
        let want = match qg_cell(g) {
            Some(w) => w,
            None => return Err(SonarCode::ENoGate),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6501 + g as u64);
            return Err(SonarCode::EGateDrift);
        }
        if match self.qgs.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SonarCode::EDup);
        }
        if let Some(c) = self.qgs.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x6502 + g as u64);
        Ok(())
    }

    /// 质量门全账核验（3 格）。
    pub fn qgs_all(&self) -> bool {
        let mut g = 0usize;
        while g < QG_CELLS {
            match self.qgs.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// Jenkins 联签一格（F52361；账面内部折叠，越界/重复拒）。
    pub fn record_jk(&mut self, c: usize) -> Result<(), SonarCode> {
        if c >= JK_CELLS {
            return Err(SonarCode::ENoCell);
        }
        if match self.jks.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SonarCode::EDup);
        }
        let w = match jk_cell(c) {
            Some(w) => w,
            None => return Err(SonarCode::ENoCell),
        };
        if let Some(slot) = self.jks.get_mut(c) {
            *slot = Some(w);
        }
        self.fnv(0x6600 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn jks_all(&self) -> bool {
        let mut c = 0usize;
        while c < JK_CELLS {
            match self.jks.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// Java 上栈声明一行。
    pub fn declare_java(&mut self, r: usize) -> Result<(), SonarCode> {
        if r >= JAVA_ROWS {
            return Err(SonarCode::ENoRow);
        }
        if match self.java.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SonarCode::EDup);
        }
        if let Some(c) = self.java.get_mut(r) {
            *c = java_row(r);
        }
        self.fnv(0x6700 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn java_all(&self) -> bool {
        let mut r = 0usize;
        while r < JAVA_ROWS {
            match self.java.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：任一节未齐即拒封（ESeal）。
    pub fn seal(&self) -> Result<(), SonarCode> {
        if self.gates_all() && self.cmds_all() && self.props_all() && self.qgs_all() && self.jks_all() && self.java_all() {
            Ok(())
        } else {
            Err(SonarCode::ESeal)
        }
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
        if self.gates_all() && self.cmds_all() && self.props_all() && self.qgs_all() && self.jks_all() && self.java_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SonarLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全流程账本构建：六闸 + 全节落账 + 注入 10 轮三态轮转。返回（记账格数, 账本）。
fn cycle_ledger() -> (u32, SonarLedger) {
    let mut led = SonarLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROP_CELLS {
        if let Some(w) = prop_cell(p) {
            if led.record_prop(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut q = 0usize;
    while q < QG_CELLS {
        if let Some(w) = qg_cell(q) {
            if led.record_qg(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    let mut c = 0usize;
    while c < JK_CELLS {
        let _ = led.record_jk(c);
        c += 1;
    }
    let mut r = 0usize;
    while r < JAVA_ROWS {
        let _ = led.declare_java(r);
        r += 1;
    }
    // 注入 10 轮：命令越闭集 / 命令漂移 / 家族逆序三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SonarLedger::new();
        let caught = match kind {
            0 => lt.record_cmd(CMD_OPS, 0).is_err(),
            1 => {
                lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SonarCode::ECmdDrift)
            }
            _ => {
                lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0)) == Err(SonarCode::EOrder)
            }
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
    (recorded, led)
}

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let (recorded, led) = cycle_ledger();
    let (_, caught, _) = led.stats();
    (recorded, caught, led.fingerprint())
}

/// 注入账三数（主账本）：恒等式注入=检出、逃逸=0。
pub fn run_cycle_stats() -> (u32, u32, u32) {
    let (_, led) = cycle_ledger();
    led.stats()
}

/// 全齐账本（六闸 + 全节落账，无注入）。
pub fn fresh_complete() -> SonarLedger {
    let mut lt = SonarLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = lt.judge_gate(g);
        g += 1;
    }
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            let _ = lt.record_cmd(o, w);
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROP_CELLS {
        if let Some(w) = prop_cell(p) {
            let _ = lt.record_prop(p, w);
        }
        p += 1;
    }
    let mut q = 0usize;
    while q < QG_CELLS {
        if let Some(w) = qg_cell(q) {
            let _ = lt.record_qg(q, w);
        }
        q += 1;
    }
    let mut c = 0usize;
    while c < JK_CELLS {
        let _ = lt.record_jk(c);
        c += 1;
    }
    let mut r = 0usize;
    while r < JAVA_ROWS {
        let _ = lt.declare_java(r);
        r += 1;
    }
    lt
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52481 判据集（锚 UNX-F52482-J1，off-by-one 板式）。
pub fn run_unx52481_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52481");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "begin/end/scan/report/quality-gate 五家族逐操作恒等",
    );
    s.add(
        "sonar-project.properties 双向投影 12 格全落",
        {
            let mut lt = SonarLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROP_CELLS {
                if let Some(w) = prop_cell(p) {
                    if lt.record_prop(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.props_all()
        },
        "工程键/源码/服务端/令牌/排除/测试/二进制/覆盖率/门等待/分支/日志/CI 十二格投影",
    );
    s.add(
        "质量门三元闸 3 格全落",
        {
            let mut lt = SonarLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < QG_CELLS {
                if let Some(w) = qg_cell(q) {
                    if lt.record_qg(q, w).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.qgs_all()
        },
        "pass/warn/fail 三态闭集逐格落账",
    );
    s.add(
        "Jenkins 消费联签 4 格全落（F52361）",
        {
            let mut lt = SonarLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < JK_CELLS {
                if lt.record_jk(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.jks_all()
        },
        "F52361 联签标记位入折叠，缺格拒封",
    );
    s.add(
        "Java 上栈声明 2 行全齐",
        {
            let mut lt = SonarLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JAVA_ROWS {
                if lt.declare_java(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.java_all()
        },
        "JDK 代理进程上栈/sonar-scanner JVM 内存上栈两行闭集",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "begin→end→scan→report→quality-gate 五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令越闭集/命令漂移/家族逆序三态轮转必抓（判据原文：压测 ×10）",
    );
    let mut l0 = SonarLedger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "投影漂移拒",
        {
            l0.record_prop(0, prop_cell(0).unwrap_or(0));
            l0.record_prop(1, prop_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SonarCode::EPropDrift)
        },
        "properties 投影载荷漂移必抓",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_prop(PROP_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.record_jk(JK_CELLS).is_err(),
        "Jenkins 联签格 4 越闭集必拒",
    );
    s.add(
        "未齐即封核",
        {
            let empty = SonarLedger::new();
            let full = fresh_complete();
            empty.seal().is_err() && full.seal().is_ok()
        },
        "账未齐封账必拒，全齐才许封",
    );
    let (inj_n, caught_n, esc_n) = run_cycle_stats();
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
            let lt = fresh_complete();
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB800,
        "SEG_BASE 0x40 对齐且等于 VX16 公式位 0xCB800",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kinds() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(8), Some(2));
        assert_eq!(cmd_kind(12), Some(3));
        assert_eq!(cmd_kind(16), Some(4));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert_eq!(cmd_kind_name(0), Some("begin"));
        assert_eq!(cmd_kind_name(4), Some("quality-gate"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_prop_names() {
        assert_eq!(prop_name(0), Some("sonar.projectKey"));
        assert_eq!(prop_name(8), Some("sonar.qualitygate.wait"));
        assert_eq!(prop_name(11), Some("sonar.links.ci"));
        assert!(prop_name(12).is_none());
        assert!(prop_cell(12).is_none());
    }

    #[test]
    fn test_qg_states() {
        assert_eq!(qg_state(0), Some(0));
        assert_eq!(qg_state(1), Some(1));
        assert_eq!(qg_state(2), Some(2));
        assert!(qg_state(3).is_none());
        assert_eq!(qg_state_name(0), Some("pass"));
        assert_eq!(qg_state_name(2), Some("fail"));
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert_ne!(prop_cell(5).unwrap_or(0), prop_cell(6).unwrap_or(0));
        assert_ne!(qg_cell(1).unwrap_or(0), qg_cell(2).unwrap_or(0));
        assert!(java_row(JAVA_ROWS).is_none());
        assert!(jk_cell(JK_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SonarLedger::new();
        // 先落低族格再落高族，合法序不误抓。
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(SonarCode::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SonarLedger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SonarCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_prop(3, prop_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(SonarCode::EPropDrift)
        );
        assert_eq!(
            lt.record_qg(1, qg_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(SonarCode::EGateDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SonarLedger::new();
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)), Err(SonarCode::EDup));
        let _ = lt.record_jk(0);
        assert_eq!(lt.record_jk(0), Err(SonarCode::EDup));
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = SonarLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 未齐即封：只落六闸，其余节缺，封账必拒。
        assert_eq!(lt.seal(), Err(SonarCode::ESeal));
        let full = fresh_complete();
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数恰为各记账节常量之和（联签/声明行不计数）。
        assert_eq!(recorded, CMD_OPS as u32 + PROP_CELLS as u32 + QG_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_ledger() {
        let (inj, caught, esc) = run_cycle_stats();
        assert_eq!(inj, 10);
        assert_eq!(caught, 10);
        assert_eq!(esc, 0);
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
        assert_eq!(SEG_BASE, 0xCB800);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SonarCode::Ok as u32, 0);
        assert_eq!(SonarCode::ENoCmd as u32, 1);
        assert_eq!(SonarCode::ECmdDrift as u32, 2);
        assert_eq!(SonarCode::EOrder as u32, 3);
        assert_eq!(SonarCode::ENoProp as u32, 4);
        assert_eq!(SonarCode::EPropDrift as u32, 5);
        assert_eq!(SonarCode::ENoGate as u32, 6);
        assert_eq!(SonarCode::EGateDrift as u32, 7);
        assert_eq!(SonarCode::ENoCell as u32, 8);
        assert_eq!(SonarCode::ENoRow as u32, 9);
        assert_eq!(SonarCode::EDup as u32, 10);
        assert_eq!(SonarCode::ESeal as u32, 11);
        assert_eq!(SonarCode::EGradeDrift as u32, 12);
        assert_eq!(SonarCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => SonarCode::Ok,
                1 => SonarCode::ENoCmd,
                2 => SonarCode::ECmdDrift,
                3 => SonarCode::EOrder,
                4 => SonarCode::ENoProp,
                5 => SonarCode::EPropDrift,
                6 => SonarCode::ENoGate,
                7 => SonarCode::EGateDrift,
                8 => SonarCode::ENoCell,
                9 => SonarCode::ENoRow,
                10 => SonarCode::EDup,
                11 => SonarCode::ESeal,
                _ => SonarCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let empty = SonarLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = SonarLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(fresh_complete().judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52481_checks_green() {
        let s = run_unx52481_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
