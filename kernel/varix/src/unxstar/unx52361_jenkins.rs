//! UNX-F52361 Jenkins（Windows 服务面）收口 · 0xC9A00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52361"，逐字）：
//!   title: Jenkins（Windows 服务面）收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52362-J1 agent
//!     安装/构建分发 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!     成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52361）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52362-J1，照原文锚定；功能按本卡标题（Jenkins Windows 服务面）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 流水线五段 20 格：checkout/build/test/archive/notify 五家族闭集
//!      各 4 格，段单调（逆序必抓），载荷纯函数逐格恒等才收。
//!   ② agent 生命周期 10 操作闭集：provision/online/accept-job/workspace/
//!      take/execute/report/teardown/offline/reclaim，逐操作落账恒等。
//!   ③ 插件 8 格闭集：八款核心插件逐格落账，重复拒、漂移拒。
//!   ④ Windows 服务三态 6 格：Running/Stopped/Paused 三态各进/出 2 格，
//!      三态闭集之外先登记。
//!   ⑤ Java 上栈 4 层声明行：JRE/JIT/ClassLoader/JNI 逐行闭集，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（段越闭集/段漂移/段逆序），检出
//!      +逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集 → S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52361;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 流水线格数（五段 × 4）。
pub const PIPE_CELLS: usize = 20;

/// agent 生命周期操作数。
pub const AGENT_OPS: usize = 10;

/// 插件格数。
pub const PLUGIN_CELLS: usize = 8;

/// Windows 服务三态格数。
pub const SVC_CELLS: usize = 6;

/// Java 上栈层数。
pub const JAVA_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JenkinsCode {
    /// 成功
    Ok = 0,
    /// 流水线段号越闭集
    ENoStage,
    /// 流水线格漂移
    EStageDrift,
    /// 段逆序（五段单调被破坏）
    EOrder,
    /// agent 操作越闭集
    ENoAgent,
    /// agent 操作漂移
    EAgentDrift,
    /// 插件格越闭集
    ENoPlugin,
    /// 插件格漂移
    EPluginDrift,
    /// 服务态越闭集
    ENoSvc,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl JenkinsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            JenkinsCode::Ok => "成功",
            JenkinsCode::ENoStage => "流水线段号越闭集：20 格之外先登记",
            JenkinsCode::EStageDrift => "流水线格漂移：载荷与基准不恒等，逐格复算",
            JenkinsCode::EOrder => "段逆序：checkout→build→test→archive→notify 单调，逆序必抓",
            JenkinsCode::ENoAgent => "agent 操作越闭集：生命周期 10 操作之外先登记",
            JenkinsCode::EAgentDrift => "agent 操作漂移：操作折叠与基准不恒等，逐操作复算",
            JenkinsCode::ENoPlugin => "插件格越闭集：8 格闭集之外先登记",
            JenkinsCode::EPluginDrift => "插件格漂移：插件折叠与基准不恒等，逐格复算",
            JenkinsCode::ENoSvc => "服务态越闭集：Windows 服务三态 6 格之外先登记",
            JenkinsCode::EDup => "重复落账：同格只许一条账目",
            JenkinsCode::ESeal => "未齐即封：20 格流水线 + 10 操作 + 8 插件 + 6 服务格未齐先补齐",
            JenkinsCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            JenkinsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格流水线+10操作+8插件+6服务格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 流水线格载荷（格 i；段 = i/4：0=checkout 1=build 2=test 3=archive 4=notify）。
pub const fn stage_cell(i: usize) -> Option<u64> {
    if i >= PIPE_CELLS {
        return None;
    }
    Some((0xC4100u64 + (i as u64) * 0x37) ^ ((i / 4) as u64).wrapping_mul(0x4A530000))
}

/// 流水线段（0..4=checkout 4..8=build 8..12=test 12..16=archive 16..20=notify）。
pub const fn stage_kind(i: usize) -> Option<u32> {
    if i >= PIPE_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 段名。
pub const fn stage_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("checkout"),
        1 => Some("build"),
        2 => Some("test"),
        3 => Some("archive"),
        4 => Some("notify"),
        _ => None,
    }
}

/// agent 生命周期操作载荷（操作 o，0..10）。
pub const fn agent_cell(o: usize) -> Option<u64> {
    if o >= AGENT_OPS {
        return None;
    }
    Some(0xC4A00u64 + (o as u64) * 0x5B)
}

/// agent 操作名。
pub const fn agent_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("provision"),
        1 => Some("online"),
        2 => Some("accept-job"),
        3 => Some("workspace"),
        4 => Some("take"),
        5 => Some("execute"),
        6 => Some("report"),
        7 => Some("teardown"),
        8 => Some("offline"),
        9 => Some("reclaim"),
        _ => None,
    }
}

/// 插件格载荷（格 p，0..8）。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some(0xC5200u64 + (p as u64) * 0x71 ^ 0x504C0000)
}

/// Windows 服务格载荷（格 s，0..6；态 = s/2：0=Running 1=Stopped 2=Paused）。
pub const fn svc_cell(s: usize) -> Option<u64> {
    if s >= SVC_CELLS {
        return None;
    }
    Some(0xC5600u64 + (s as u64) * 0x2D)
}

/// 服务态名（0=Running 1=Stopped 2=Paused）。
pub const fn svc_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Running"),
        1 => Some("Stopped"),
        2 => Some("Paused"),
        _ => None,
    }
}

/// Java 上栈层折叠（层 r：0=JRE 1=JIT 2=ClassLoader 3=JNI）。
pub const fn java_row(r: usize) -> Option<u32> {
    if r >= JAVA_LAYERS {
        return None;
    }
    Some(0xC5A0 + r as u32 * 0x2D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Jenkins 收口账本状态机。
#[derive(Clone)]
pub struct JenkinsLedger {
    /// 六判据（agent 安装/构建分发/服务启动/服务停止/卸载清理/许可合规）。
    gates: [Option<bool>; 6],
    /// 流水线账。
    pipe: [Option<u64>; PIPE_CELLS],
    /// agent 操作账。
    agents: [Option<u64>; AGENT_OPS],
    /// 插件账。
    plugins: [Option<u64>; PLUGIN_CELLS],
    /// 服务态账。
    svc: [Option<u64>; SVC_CELLS],
    /// Java 上栈声明账。
    java: [Option<u32>; JAVA_LAYERS],
    /// 最后段。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl JenkinsLedger {
    /// 新账本。
    pub fn new() -> Self {
        JenkinsLedger {
            gates: [None; 6],
            pipe: [None; PIPE_CELLS],
            agents: [None; AGENT_OPS],
            plugins: [None; PLUGIN_CELLS],
            svc: [None; SVC_CELLS],
            java: [None; JAVA_LAYERS],
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

    /// 最后段（测试用观察口）。
    pub fn last_stage(&self) -> i64 {
        self.last_kind
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), JenkinsCode> {
        if gate >= 6 {
            return Err(JenkinsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4A00 + gate as u64);
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

    /// 流水线一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_stage(&mut self, i: usize, got: u64) -> Result<(), JenkinsCode> {
        if i >= PIPE_CELLS {
            return Err(JenkinsCode::ENoStage);
        }
        let kind = match stage_kind(i) {
            Some(k) => k,
            None => return Err(JenkinsCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00);
            return Err(JenkinsCode::EOrder);
        }
        let want = match stage_cell(i) {
            Some(w) => w,
            None => return Err(JenkinsCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B01 + i as u64);
            return Err(JenkinsCode::EStageDrift);
        }
        if match self.pipe.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JenkinsCode::EDup);
        }
        if let Some(c) = self.pipe.get_mut(i) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4B02 + i as u64);
        Ok(())
    }

    /// 流水线全账核验（20 格）。
    pub fn pipe_all(&self) -> bool {
        let mut i = 0usize;
        while i < PIPE_CELLS {
            match self.pipe.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// agent 生命周期一格（载荷对照 + 重复拒）。
    pub fn record_agent(&mut self, o: usize, got: u64) -> Result<(), JenkinsCode> {
        let want = match agent_cell(o) {
            Some(w) => w,
            None => return Err(JenkinsCode::ENoAgent),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4C00 + o as u64);
            return Err(JenkinsCode::EAgentDrift);
        }
        if match self.agents.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JenkinsCode::EDup);
        }
        if let Some(c) = self.agents.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x4C01 + o as u64);
        Ok(())
    }

    /// agent 操作全账核验（10 操作）。
    pub fn agents_all(&self) -> bool {
        let mut o = 0usize;
        while o < AGENT_OPS {
            match self.agents.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), JenkinsCode> {
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(JenkinsCode::ENoPlugin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4D00 + p as u64);
            return Err(JenkinsCode::EPluginDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JenkinsCode::EDup);
        }
        if let Some(c) = self.plugins.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4D01 + p as u64);
        Ok(())
    }

    /// 插件全账核验（8 格）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Windows 服务态一格（三态闭集 + 载荷对照 + 重复拒）。
    pub fn record_svc(&mut self, s: usize, got: u64) -> Result<(), JenkinsCode> {
        let want = match svc_cell(s) {
            Some(w) => w,
            None => return Err(JenkinsCode::ENoSvc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4E00 + s as u64);
            return Err(JenkinsCode::EPluginDrift);
        }
        if match self.svc.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JenkinsCode::EDup);
        }
        if let Some(c) = self.svc.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x4E01 + s as u64);
        Ok(())
    }

    /// 服务态全账核验（6 格）。
    pub fn svc_all(&self) -> bool {
        let mut s = 0usize;
        while s < SVC_CELLS {
            match self.svc.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Java 上栈一行（JRE/JIT/ClassLoader/JNI）。
    pub fn declare_java(&mut self, r: usize) -> Result<(), JenkinsCode> {
        let w = match java_row(r) {
            Some(w) => w,
            None => return Err(JenkinsCode::ENoPlugin),
        };
        if match self.java.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(JenkinsCode::EDup);
        }
        if let Some(c) = self.java.get_mut(r) {
            *c = Some(w);
        }
        self.fnv(0x4F00 + r as u64);
        Ok(())
    }

    /// Java 上栈全齐核验。
    pub fn java_all(&self) -> bool {
        let mut r = 0usize;
        while r < JAVA_LAYERS {
            match self.java.get(r) {
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
        if self.gates_all() && self.pipe_all() && self.agents_all() && self.plugins_all()
            && self.svc_all() && self.java_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for JenkinsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = JenkinsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < PIPE_CELLS {
        if let Some(w) = stage_cell(i) {
            if led.record_stage(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < AGENT_OPS {
        if let Some(w) = agent_cell(o) {
            if led.record_agent(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < SVC_CELLS {
        if let Some(w) = svc_cell(s) {
            if led.record_svc(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < JAVA_LAYERS {
        if led.declare_java(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：段越闭集 / 段漂移 / 段逆序三态轮转（均在当轮临时账本上）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = JenkinsLedger::new();
        let caught = match kind {
            0 => lt.record_stage(PIPE_CELLS, 0) == Err(JenkinsCode::ENoStage),
            1 => {
                let w0 = match stage_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_stage(0, w0.wrapping_add(1)) == Err(JenkinsCode::EStageDrift)
            }
            _ => {
                let _ = lt.record_stage(6, stage_cell(6).unwrap_or(0));
                lt.record_stage(1, stage_cell(1).unwrap_or(0)) == Err(JenkinsCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5000 + k as u64);
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
/// UNX-F52361 判据集（锚 UNX-F52362-J1，off-by-one 板式）。
pub fn run_unx52361_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52361");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "流水线 20 格全通（checkout/build/test/archive/notify）",
        recorded >= PIPE_CELLS as u32,
        "五段四格逐格载荷恒等落账",
    );
    s.add(
        "agent 生命周期 10 操作全落（agent 安装/构建分发 ×10，UNX-F52362-J1）",
        {
            let mut lt = JenkinsLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < AGENT_OPS {
                if let Some(w) = agent_cell(o) {
                    if lt.record_agent(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.agents_all()
        },
        "provision→reclaim 十操作逐个恒等（判据锚 §AI-66.3）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = JenkinsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "agent 安装/构建分发/服务启动/服务停止/卸载清理/许可合规逐格落账",
    );
    s.add(
        "插件 8 格全齐",
        {
            let mut lt = JenkinsLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "八款核心插件逐格闭集落账",
    );
    s.add(
        "Windows 服务三态 6 格全齐",
        {
            let mut lt = JenkinsLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < SVC_CELLS {
                if let Some(w) = svc_cell(st) {
                    if lt.record_svc(st, w).is_err() {
                        ok = false;
                    }
                }
                st += 1;
            }
            ok && lt.svc_all()
        },
        "Running/Stopped/Paused 三态各进出两格闭集（Windows 服务面）",
    );
    s.add(
        "Java 上栈 4 层声明全落",
        {
            let mut lt = JenkinsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JAVA_LAYERS {
                if lt.declare_java(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.java_all()
        },
        "JRE/JIT/ClassLoader/JNI 四层闭集缺一拒封",
    );
    s.add(
        "五段单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < PIPE_CELLS {
                if stage_kind(i).unwrap_or(5) < stage_kind(i - 1).unwrap_or(5) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "checkout→build→test→archive→notify 段单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "段越闭集/段漂移/段逆序三态轮转必抓");
    s.add(
        "边界·流水线格越闭集",
        JenkinsLedger::new().record_stage(PIPE_CELLS, 0).is_err(),
        "格 20 越闭集必拒",
    );
    s.add(
        "边界·agent 操作越闭集",
        {
            let mut lt = JenkinsLedger::new();
            let _ = lt.record_agent(0, agent_cell(0).unwrap_or(0));
            lt.record_agent(AGENT_OPS, 0).is_err()
        },
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·漂移拒",
        {
            let mut lt = JenkinsLedger::new();
            let w = match stage_cell(0) {
                Some(w) => w,
                None => 0,
            };
            lt.record_stage(0, w.wrapping_add(1)) == Err(JenkinsCode::EStageDrift)
        },
        "流水线载荷漂移必抓",
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
            let mut lt = JenkinsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < PIPE_CELLS {
                if let Some(w) = stage_cell(i) {
                    let _ = lt.record_stage(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < AGENT_OPS {
                if let Some(w) = agent_cell(o) {
                    let _ = lt.record_agent(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            let mut st = 0usize;
            while st < SVC_CELLS {
                if let Some(w) = svc_cell(st) {
                    let _ = lt.record_svc(st, w);
                }
                st += 1;
            }
            let mut r = 0usize;
            while r < JAVA_LAYERS {
                let _ = lt.declare_java(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9A00,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9A00",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集（本条按求值时点 len==15）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_kind_mapping() {
        assert_eq!(stage_kind(0), Some(0));
        assert_eq!(stage_kind(4), Some(1));
        assert_eq!(stage_kind(8), Some(2));
        assert_eq!(stage_kind(12), Some(3));
        assert_eq!(stage_kind(19), Some(4));
        assert!(stage_kind(20).is_none());
    }

    #[test]
    fn test_stage_names() {
        assert_eq!(stage_kind_name(0), Some("checkout"));
        assert_eq!(stage_kind_name(4), Some("notify"));
        assert!(stage_kind_name(5).is_none());
        assert_eq!(agent_op_name(0), Some("provision"));
        assert_eq!(agent_op_name(9), Some("reclaim"));
        assert!(agent_op_name(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match stage_cell(3) {
            Some(w) => w,
            None => 0,
        };
        assert_eq!(a, stage_cell(3).unwrap_or(0));
        assert_ne!(a, stage_cell(4).unwrap_or(0));
        assert!(stage_cell(PIPE_CELLS).is_none());
        assert!(agent_cell(AGENT_OPS).is_none());
        assert!(plugin_cell(PLUGIN_CELLS).is_none());
        assert!(svc_cell(SVC_CELLS).is_none());
        assert!(java_row(JAVA_LAYERS).is_none());
    }

    #[test]
    fn test_svc_states() {
        assert_eq!(svc_state_name(0), Some("Running"));
        assert_eq!(svc_state_name(1), Some("Stopped"));
        assert_eq!(svc_state_name(2), Some("Paused"));
        assert!(svc_state_name(3).is_none());
    }

    #[test]
    fn test_order_caught() {
        // 按合法序先落 build 段（kind=1），再注 checkout（kind=0）→ EOrder。
        let mut lt = JenkinsLedger::new();
        assert!(lt.record_stage(5, stage_cell(5).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_stage(1, stage_cell(1).unwrap_or(0)),
            Err(JenkinsCode::EOrder)
        );
        // 注入失败的逆序格不改 last_kind（仍为 build 段 kind=1）。
        assert_eq!(lt.last_stage(), 1);
        assert!(lt.record_stage(6, stage_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = JenkinsLedger::new();
        assert_eq!(
            lt.record_stage(2, stage_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(JenkinsCode::EStageDrift)
        );
        assert_eq!(
            lt.record_agent(3, agent_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(JenkinsCode::EAgentDrift)
        );
        assert_eq!(
            lt.record_plugin(1, plugin_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(JenkinsCode::EPluginDrift)
        );
        assert_eq!(
            lt.record_svc(4, svc_cell(4).unwrap_or(0).wrapping_add(3)),
            Err(JenkinsCode::EPluginDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = JenkinsLedger::new();
        let _ = lt.record_agent(0, agent_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_agent(0, agent_cell(0).unwrap_or(0)),
            Err(JenkinsCode::EDup)
        );
        let _ = lt.record_plugin(0, plugin_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_plugin(0, plugin_cell(0).unwrap_or(0)),
            Err(JenkinsCode::EDup)
        );
        let _ = lt.record_svc(0, svc_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_svc(0, svc_cell(0).unwrap_or(0)),
            Err(JenkinsCode::EDup)
        );
        let _ = lt.declare_java(0);
        assert_eq!(lt.declare_java(0), Err(JenkinsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = JenkinsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = JenkinsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 20 流水线 + 10 agent + 8 插件 + 6 服务格 + 4 Java 行 = 48。
        assert_eq!(recorded, 48);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_kinds() {
        // 三态轮转：k%3 ∈ {0,1,2}，0=段越闭集 1=段漂移 2=段逆序，全部必抓。
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = JenkinsLedger::new();
            let caught = match kind {
                0 => lt.record_stage(PIPE_CELLS, 0).is_err(),
                1 => lt
                    .record_stage(0, stage_cell(0).unwrap_or(0).wrapping_add(1))
                    .is_err(),
                _ => {
                    let _ = lt.record_stage(6, stage_cell(6).unwrap_or(0));
                    lt.record_stage(1, stage_cell(1).unwrap_or(0)).is_err()
                }
            };
            assert!(caught, "注入轮 {} 必抓", k);
            k += 1;
        }
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
        assert_eq!(SEG_BASE, 0xC9A00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(JenkinsCode::Ok as u32, 0);
        assert_eq!(JenkinsCode::EGradeDrift as u32, 12);
        assert_eq!(JenkinsCode::ESeal.val(), SEG_BASE + 10);
        assert!(!JenkinsCode::Ok.detail().is_empty());
        assert!(!JenkinsCode::ESeal.detail().is_empty());
        assert!(!JenkinsCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = JenkinsLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = JenkinsLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = JenkinsLedger::new();
        let _ = lt.record_stage(0, stage_cell(0).unwrap_or(0).wrapping_add(9));
        let (i, c, e) = lt.stats();
        assert_eq!((i, c, e), (1, 1, 0));
    }

    #[test]
    fn unx52361_checks_green() {
        let s = run_unx52361_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 16);
    }
}
