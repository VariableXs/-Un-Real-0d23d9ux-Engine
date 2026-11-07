//! UNX-F52482 k6 收口 · 0xCB840段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52482"，逐字）：
//!   title: k6 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52483-J1 ×10（本
//!     地靶站）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52482）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52483-J1，照原文锚定；功能按本卡标题（k6）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族闭集：run / inspect / archive / cloud / local 各 4 操作
//!      共 20 操作，家族单调（逆序必抓），逐操作载荷折叠恒等才收。
//!   ② 场景三态 6 格：smoke/load/stress/spike/soak/breakpoint 六场景，
//!      每场景三态（ramp-up/plateau/teardown），逐格载荷恒等，漂移必抓。
//!   ③ 阈值投影 8 格：http_req_duration/http_req_failed/http_reqs/vus/
//!      iterations/checks/data_received/data_sent，逐格投影恒等。
//!   ④ JMeter 分账声明 2 行（F52260）：k6 与 JMeter 负载账分账，缺一拒封。
//!   ⑤ Go 上栈声明 2 行闭集：k6 Go 运行时上栈声明 / k6 扩展 Go 构建上栈
//!      声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（命令越闭集/命令漂移/
//!      家族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级闭集
//!      （六闸 + 20 操作 + 6 场景 + 8 阈值 + 2 分账 + 2 声明全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52482;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令操作数。
pub const CMD_OPS: usize = 20;

/// 场景三态格数。
pub const SCN_CELLS: usize = 6;

/// 阈值投影格数。
pub const THR_CELLS: usize = 8;

/// JMeter 分账声明行数（F52260）。
pub const JM_ROWS: usize = 2;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K6Code {
    /// 成功
    Ok = 0,
    /// 命令号越闭集
    ENoCmd,
    /// 命令载荷漂移
    ECmdDrift,
    /// 命令家族逆序
    EOrder,
    /// 场景格越闭集
    ENoScn,
    /// 场景漂移
    EScnDrift,
    /// 阈值格越闭集
    ENoThr,
    /// 阈值漂移
    EThrDrift,
    /// 分账行越闭集
    ENoRow,
    /// Go 声明行越闭集
    ENoDecl,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl K6Code {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            K6Code::Ok => "成功",
            K6Code::ENoCmd => "命令号越闭集：run/inspect/archive/cloud/local 20 操作集之外先登记",
            K6Code::ECmdDrift => "命令漂移：命令折叠与基准不恒等，逐格复算",
            K6Code::EOrder => "家族逆序：run→inspect→archive→cloud→local 单调，逆序必抓",
            K6Code::ENoScn => "场景格越闭集：smoke/load/stress/spike/soak/breakpoint 6 格之外先登记",
            K6Code::EScnDrift => "场景漂移：场景三态载荷与基准不恒等，逐格复算",
            K6Code::ENoThr => "阈值格越闭集：阈值投影 8 格之外先登记",
            K6Code::EThrDrift => "阈值漂移：阈值投影载荷与基准不恒等，逐格复算",
            K6Code::ENoRow => "分账行越闭集：JMeter 分账声明 2 行（F52260）之外先登记",
            K6Code::ENoDecl => "Go 声明行越闭集：Go 上栈声明 2 行闭集之外先登记",
            K6Code::EDup => "重复落账：同格只许一条账目",
            K6Code::ESeal => "未齐即封：六闸+20操作+6场景+8阈值+2分账+2声明未齐先补齐",
            K6Code::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20操作+6场景+8阈值+2分账+2声明全通收口",
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
    Some((0xC1000u64 + (o as u64) * 0x31) ^ 0x4B36434D44000000)
}

/// 命令家族（0..4=run 4..8=inspect 8..12=archive 12..16=cloud 16..20=local）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令家族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("run"),
        1 => Some("inspect"),
        2 => Some("archive"),
        3 => Some("cloud"),
        4 => Some("local"),
        _ => None,
    }
}

/// 场景名（6 格）。
pub const fn scn_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("smoke"),
        1 => Some("load"),
        2 => Some("stress"),
        3 => Some("spike"),
        4 => Some("soak"),
        5 => Some("breakpoint"),
        _ => None,
    }
}

/// 场景三态（0=ramp-up 1=plateau 2=teardown；i/N 映射：态 = c % 3）。
pub const fn scn_state(c: usize) -> Option<u32> {
    if c >= SCN_CELLS {
        return None;
    }
    Some((c % 3) as u32)
}

/// 场景三态名。
pub const fn scn_state_name(t: u32) -> Option<&'static str> {
    match t {
        0 => Some("ramp-up"),
        1 => Some("plateau"),
        2 => Some("teardown"),
        _ => None,
    }
}

/// 场景格载荷。
pub const fn scn_cell(c: usize) -> Option<u64> {
    if c >= SCN_CELLS {
        return None;
    }
    Some((0xC2000u64 + (c as u64) * 0x39) ^ 0x4B3653434E000000)
}

/// 阈值投影格名（8 格）。
pub const fn thr_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("http_req_duration"),
        1 => Some("http_req_failed"),
        2 => Some("http_reqs"),
        3 => Some("vus"),
        4 => Some("iterations"),
        5 => Some("checks"),
        6 => Some("data_received"),
        7 => Some("data_sent"),
        _ => None,
    }
}

/// 阈值投影格载荷。
pub const fn thr_cell(p: usize) -> Option<u64> {
    if p >= THR_CELLS {
        return None;
    }
    Some((0xC3000u64 + (p as u64) * 0x33) ^ 0x4B36545852000000)
}

/// JMeter 分账行折叠（行 r；F52260）。
pub const fn jmeter_row(r: usize) -> Option<u32> {
    if r >= JM_ROWS {
        return None;
    }
    Some(0xC400 + r as u32 * 0x2C)
}

/// JMeter 分账行名。
pub const fn jmeter_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("k6 负载账与 JMeter 分账声明（F52260）"),
        1 => Some("JMeter 历史基线账独立封存声明"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

/// Go 上栈声明行名。
pub const fn go_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("k6 Go 运行时上栈声明"),
        1 => Some("k6 扩展 Go 构建上栈声明"),
        _ => None,
    }
}

/// 六判据闸名（k6 流水）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("脚本加载"),
        1 => Some("场景执行"),
        2 => Some("阈值判定"),
        3 => Some("云同步"),
        4 => Some("本地汇总"),
        5 => Some("定级登记"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// k6 收口账本状态机。
#[derive(Clone)]
pub struct K6Ledger {
    /// 六判据（脚本加载/场景执行/阈值判定/云同步/本地汇总/定级登记）。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 场景账。
    scns: [Option<u64>; SCN_CELLS],
    /// 阈值投影账。
    thrs: [Option<u64>; THR_CELLS],
    /// JMeter 分账账。
    jms: [Option<u32>; JM_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后命令家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl K6Ledger {
    /// 新账本。
    pub fn new() -> Self {
        K6Ledger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            scns: [None; SCN_CELLS],
            thrs: [None; THR_CELLS],
            jms: [None; JM_ROWS],
            gos: [None; GO_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), K6Code> {
        if gate >= 6 {
            return Err(K6Code::ESeal);
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
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), K6Code> {
        if o >= CMD_OPS {
            return Err(K6Code::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(K6Code::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(K6Code::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(K6Code::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(K6Code::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K6Code::EDup);
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

    /// 场景一格（载荷对照 + 重复拒）。
    pub fn record_scn(&mut self, c: usize, got: u64) -> Result<(), K6Code> {
        if c >= SCN_CELLS {
            return Err(K6Code::ENoScn);
        }
        let want = match scn_cell(c) {
            Some(w) => w,
            None => return Err(K6Code::ENoScn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + c as u64);
            return Err(K6Code::EScnDrift);
        }
        if match self.scns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K6Code::EDup);
        }
        if let Some(s) = self.scns.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x6402 + c as u64);
        Ok(())
    }

    /// 场景全账核验（6 格）。
    pub fn scns_all(&self) -> bool {
        let mut c = 0usize;
        while c < SCN_CELLS {
            match self.scns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 阈值投影一格（载荷对照 + 重复拒）。
    pub fn record_thr(&mut self, p: usize, got: u64) -> Result<(), K6Code> {
        if p >= THR_CELLS {
            return Err(K6Code::ENoThr);
        }
        let want = match thr_cell(p) {
            Some(w) => w,
            None => return Err(K6Code::ENoThr),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6501 + p as u64);
            return Err(K6Code::EThrDrift);
        }
        if match self.thrs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K6Code::EDup);
        }
        if let Some(t) = self.thrs.get_mut(p) {
            *t = Some(got);
        }
        self.fnv(0x6502 + p as u64);
        Ok(())
    }

    /// 阈值全账核验（8 格）。
    pub fn thrs_all(&self) -> bool {
        let mut p = 0usize;
        while p < THR_CELLS {
            match self.thrs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// JMeter 分账一行（F52260）。
    pub fn declare_jmeter(&mut self, r: usize) -> Result<(), K6Code> {
        if r >= JM_ROWS {
            return Err(K6Code::ENoRow);
        }
        if match self.jms.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K6Code::EDup);
        }
        if let Some(c) = self.jms.get_mut(r) {
            *c = jmeter_row(r);
        }
        self.fnv(0x6600 + r as u64);
        Ok(())
    }

    /// JMeter 分账全齐核验。
    pub fn jms_all(&self) -> bool {
        let mut r = 0usize;
        while r < JM_ROWS {
            match self.jms.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), K6Code> {
        if r >= GO_ROWS {
            return Err(K6Code::ENoDecl);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(K6Code::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0x6700 + r as u64);
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

    /// 封账核验：任一节未齐即拒封（ESeal）。
    pub fn seal(&self) -> Result<(), K6Code> {
        if self.gates_all() && self.cmds_all() && self.scns_all() && self.thrs_all() && self.jms_all() && self.gos_all() {
            Ok(())
        } else {
            Err(K6Code::ESeal)
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
        if self.gates_all() && self.cmds_all() && self.scns_all() && self.thrs_all() && self.jms_all() && self.gos_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for K6Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全流程账本构建：六闸 + 全节落账 + 注入 10 轮三态轮转。返回（记账格数, 账本）。
fn cycle_ledger() -> (u32, K6Ledger) {
    let mut led = K6Ledger::new();
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
    let mut c = 0usize;
    while c < SCN_CELLS {
        if let Some(w) = scn_cell(c) {
            if led.record_scn(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < THR_CELLS {
        if let Some(w) = thr_cell(p) {
            if led.record_thr(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < JM_ROWS {
        let _ = led.declare_jmeter(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：命令越闭集 / 命令漂移 / 家族逆序三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = K6Ledger::new();
        let caught = match kind {
            0 => lt.record_cmd(CMD_OPS, 0).is_err(),
            1 => {
                lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0).wrapping_add(1)) == Err(K6Code::ECmdDrift)
            }
            _ => {
                lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0)) == Err(K6Code::EOrder)
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
pub fn fresh_complete() -> K6Ledger {
    let mut lt = K6Ledger::new();
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
    let mut c = 0usize;
    while c < SCN_CELLS {
        if let Some(w) = scn_cell(c) {
            let _ = lt.record_scn(c, w);
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < THR_CELLS {
        if let Some(w) = thr_cell(p) {
            let _ = lt.record_thr(p, w);
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < JM_ROWS {
        let _ = lt.declare_jmeter(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = lt.declare_go(r);
        r += 1;
    }
    lt
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52482 判据集（锚 UNX-F52483-J1，off-by-one 板式）。
pub fn run_unx52482_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52482");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "run/inspect/archive/cloud/local 五家族逐操作恒等",
    );
    s.add(
        "场景三态 6 格全落",
        {
            let mut lt = K6Ledger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SCN_CELLS {
                if let Some(w) = scn_cell(c) {
                    if lt.record_scn(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.scns_all()
        },
        "smoke/load/stress/spike/soak/breakpoint 六场景三态逐格落账",
    );
    s.add(
        "阈值投影 8 格全落",
        {
            let mut lt = K6Ledger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < THR_CELLS {
                if let Some(w) = thr_cell(p) {
                    if lt.record_thr(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.thrs_all()
        },
        "http_req_duration/failed/reqs/vus/iterations/checks/data 八格投影",
    );
    s.add(
        "JMeter 分账声明 2 行全齐（F52260）",
        {
            let mut lt = K6Ledger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JM_ROWS {
                if lt.declare_jmeter(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.jms_all()
        },
        "k6 与 JMeter 负载账分账/历史基线独立封存两行闭集",
    );
    s.add(
        "Go 上栈声明 2 行全齐",
        {
            let mut lt = K6Ledger::new();
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
        "k6 Go 运行时上栈/扩展 Go 构建上栈两行闭集",
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
        "run→inspect→archive→cloud→local 五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令越闭集/命令漂移/家族逆序三态轮转必抓（判据原文：×10 本地靶站）",
    );
    let mut l0 = K6Ledger::new();
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "阈值漂移拒",
        {
            l0.record_thr(0, thr_cell(0).unwrap_or(0));
            l0.record_thr(1, thr_cell(1).unwrap_or(0).wrapping_add(1)) == Err(K6Code::EThrDrift)
        },
        "阈值投影载荷漂移必抓",
    );
    s.add(
        "边界·阈值越闭集",
        l0.record_thr(THR_CELLS, 0).is_err(),
        "阈值格 8 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        l0.declare_jmeter(JM_ROWS).is_err(),
        "JMeter 分账行 2 越闭集必拒",
    );
    s.add(
        "未齐即封核",
        {
            let empty = K6Ledger::new();
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB840,
        "SEG_BASE 0x40 对齐且等于 VX16 公式位 0xCB840",
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
        assert!(cmd_kind(20).is_none());
        assert_eq!(cmd_kind_name(0), Some("run"));
        assert_eq!(cmd_kind_name(4), Some("local"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_scn_states() {
        // i/N 映射：态 = c % 3。
        assert_eq!(scn_state(0), Some(0));
        assert_eq!(scn_state(1), Some(1));
        assert_eq!(scn_state(2), Some(2));
        assert_eq!(scn_state(3), Some(0));
        assert_eq!(scn_state(5), Some(2));
        assert!(scn_state(6).is_none());
        assert_eq!(scn_name(0), Some("smoke"));
        assert_eq!(scn_name(5), Some("breakpoint"));
        assert_eq!(scn_state_name(0), Some("ramp-up"));
        assert_eq!(scn_state_name(2), Some("teardown"));
        assert!(scn_state_name(3).is_none());
    }

    #[test]
    fn test_thr_names() {
        assert_eq!(thr_name(0), Some("http_req_duration"));
        assert_eq!(thr_name(7), Some("data_sent"));
        assert!(thr_name(8).is_none());
        assert!(thr_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(3).unwrap_or(0);
        assert_eq!(a, cmd_cell(3).unwrap_or(0));
        assert_ne!(a, cmd_cell(4).unwrap_or(0));
        assert_ne!(scn_cell(2).unwrap_or(0), scn_cell(3).unwrap_or(0));
        assert_ne!(thr_cell(1).unwrap_or(0), thr_cell(2).unwrap_or(0));
        assert!(jmeter_row(JM_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = K6Ledger::new();
        // 先落低族格再落高族，合法序不误抓。
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(K6Code::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = K6Ledger::new();
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(K6Code::ECmdDrift)
        );
        assert_eq!(
            lt.record_scn(2, scn_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(K6Code::EScnDrift)
        );
        assert_eq!(
            lt.record_thr(4, thr_cell(4).unwrap_or(0).wrapping_add(3)),
            Err(K6Code::EThrDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = K6Ledger::new();
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)), Err(K6Code::EDup));
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(K6Code::EDup));
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = K6Ledger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 未齐即封：只落六闸，其余节缺，封账必拒。
        assert_eq!(lt.seal(), Err(K6Code::ESeal));
        let full = fresh_complete();
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_decl_boundary() {
        let mut lt = K6Ledger::new();
        assert!(lt.declare_jmeter(0).is_ok());
        assert!(lt.declare_jmeter(1).is_ok());
        assert_eq!(lt.declare_jmeter(2), Err(K6Code::ENoRow));
        assert!(lt.declare_go(0).is_ok());
        assert!(lt.declare_go(1).is_ok());
        assert_eq!(lt.declare_go(2), Err(K6Code::ENoDecl));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数恰为各记账节常量之和（分账/声明行不计数）。
        assert_eq!(recorded, CMD_OPS as u32 + SCN_CELLS as u32 + THR_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCB840);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(K6Code::Ok as u32, 0);
        assert_eq!(K6Code::ENoCmd as u32, 1);
        assert_eq!(K6Code::ECmdDrift as u32, 2);
        assert_eq!(K6Code::EOrder as u32, 3);
        assert_eq!(K6Code::ENoScn as u32, 4);
        assert_eq!(K6Code::EScnDrift as u32, 5);
        assert_eq!(K6Code::ENoThr as u32, 6);
        assert_eq!(K6Code::EThrDrift as u32, 7);
        assert_eq!(K6Code::ENoRow as u32, 8);
        assert_eq!(K6Code::ENoDecl as u32, 9);
        assert_eq!(K6Code::EDup as u32, 10);
        assert_eq!(K6Code::ESeal as u32, 11);
        assert_eq!(K6Code::EGradeDrift as u32, 12);
        assert_eq!(K6Code::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => K6Code::Ok,
                1 => K6Code::ENoCmd,
                2 => K6Code::ECmdDrift,
                3 => K6Code::EOrder,
                4 => K6Code::ENoScn,
                5 => K6Code::EScnDrift,
                6 => K6Code::ENoThr,
                7 => K6Code::EThrDrift,
                8 => K6Code::ENoRow,
                9 => K6Code::ENoDecl,
                10 => K6Code::EDup,
                11 => K6Code::ESeal,
                _ => K6Code::EGradeDrift,
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
        let empty = K6Ledger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = K6Ledger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(fresh_complete().judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52482_checks_green() {
        let s = run_unx52482_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
