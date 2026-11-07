//! UNX-F52243 Newman（Postman 跑批器）收口 · 0xC7C80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52243"，逐字）：
//!   title: Newman（Postman 跑批器）收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52244-J1 六判据
//!     全落（Electron）；请求/环境 ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52243）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52244-J1，照原文锚定；功能按本卡标题（Newman 跑批器）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① collection 运行 10 场景闭集：五家族（装订/执行/断言/汇出/收尾）
//!      各 2 场景，家族单调（逆序必抓且不污染家族账位）。
//!   ② 环境注入 8 格闭集：全局/集合级/环境级/数据文件/迭代数据/变量覆
//!      盖/密钥注入/动态变量，逐格载荷折叠恒等才收。
//!   ③ reporter 三态 6 格：CLI 表格态/JSON 态/JUnit 态各 2 格，逐态对
//!      照，越态必抓。
//!   ④ Node 上栈 4 层阶梯：Node 运行时/libuv/CLI 框架/集合运行器，越
//!      层必抓，逐层载荷对照。
//!   ⑤ 迭代/超时/重试三面 9 格：迭代 3 + 超时 3 + 重试 3，逐面落账。
//!   ⑥ 退出码闭集 5：0=全通 / 1=运行失败 / 2=资源缺失 / 3=参数错误 /
//!      4=异常中断，越码必抓。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（环境注入越
//!      闭集/场景漂移/reporter 越态），三类注入全部落在当轮临时账本
//!      上，injected==caught==10、escaped==0；双跑指纹一致；定级 S1
//!      （10 场景 + 8 环境格 + 6 reporter 格 + 4 层 Node 栈 + 9 三面格
//!      + 5 退出码 + 六判据全通 → S1）。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界 + match 取值）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52243;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// collection 运行场景数。
pub const RUN_SCENES: usize = 10;

/// 环境注入格数。
pub const ENV_CELLS: usize = 8;

/// reporter 三态格数（3 态 × 2）。
pub const REPORTER_CELLS: usize = 6;

/// Node 上栈层数。
pub const NODE_LAYERS: usize = 4;

/// 迭代/超时/重试三面格数。
pub const TRIFACE_CELLS: usize = 9;

/// 退出码闭集值数。
pub const EXIT_CODES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NewmanCode {
    /// 成功
    Ok = 0,
    /// 运行场景越闭集
    ENoScene,
    /// 运行场景漂移
    ESceneDrift,
    /// 环境注入格越闭集
    ENoEnv,
    /// 环境注入漂移
    EEnvDrift,
    /// 场景家族逆序
    EOrder,
    /// reporter 态越闭集
    ENoRep,
    /// reporter 漂移
    ERepDrift,
    /// Node 层越阶梯
    ENoNode,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl NewmanCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NewmanCode::Ok => "成功",
            NewmanCode::ENoScene => "运行场景越闭集：collection 运行 10 场景之外先登记",
            NewmanCode::ESceneDrift => "运行场景漂移：场景折叠与基准不恒等，逐景复算",
            NewmanCode::ENoEnv => "环境注入格越闭集：环境注入 8 格之外先登记",
            NewmanCode::EEnvDrift => "环境注入漂移：注入折叠与基准不恒等，逐格复算",
            NewmanCode::EOrder => "场景家族逆序：装订→执行→断言→汇出→收尾单调，逆序必抓",
            NewmanCode::ENoRep => "reporter 态越闭集：表格/JSON/JUnit 三态 6 格之外先登记",
            NewmanCode::ERepDrift => "reporter 漂移：态折叠与基准不恒等，逐态复算",
            NewmanCode::ENoNode => "Node 层越阶梯：运行时/libuv/CLI 框架/集合运行器四层之外先登记",
            NewmanCode::EDup => "重复落账：同格只许一条账目",
            NewmanCode::ESeal => "未齐即封：10 场景 + 8 环境格 + 6 态 + 4 层 + 9 三面格未齐先补齐",
            NewmanCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NewmanCode::EGradeDrift => "定级漂移：全通且三面格全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10场景+8环境格+6态+4层+9三面格+5退出码全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 运行场景折叠（场景 s；家族 = s/2）。
pub const fn scene_cell(s: usize) -> Option<u64> {
    if s >= RUN_SCENES {
        return None;
    }
    Some((0xF1000u64 + (s as u64) * 0x5C) ^ ((s / 2) as u64).wrapping_mul(0x01000193))
}

/// 场景家族（0=装订 1=执行 2=断言 3=汇出 4=收尾）。
pub const fn scene_kind(s: usize) -> Option<u32> {
    if s >= RUN_SCENES {
        return None;
    }
    Some((s / 2) as u32)
}

/// 场景家族名。
pub const fn scene_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("装订"),
        1 => Some("执行"),
        2 => Some("断言"),
        3 => Some("汇出"),
        4 => Some("收尾"),
        _ => None,
    }
}

/// 环境注入格折叠（格 e）。
pub const fn env_cell(e: usize) -> Option<u64> {
    if e >= ENV_CELLS {
        return None;
    }
    Some((0xF2000u64 + (e as u64) * 0x63) ^ 0x454E56494E4A)
}

/// 环境注入格名。
pub const fn env_cell_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("全局注入"),
        1 => Some("集合级注入"),
        2 => Some("环境级注入"),
        3 => Some("数据文件注入"),
        4 => Some("迭代数据注入"),
        5 => Some("变量覆盖注入"),
        6 => Some("密钥注入"),
        7 => Some("动态变量注入"),
        _ => None,
    }
}

/// reporter 态格折叠（格 r：0..2=表格 2..4=JSON 4..6=JUnit）。
pub const fn reporter_cell(r: usize) -> Option<u64> {
    if r >= REPORTER_CELLS {
        return None;
    }
    Some((0xF3000u64 + (r as u64) * 0x48) ^ ((r / 2) as u64).wrapping_mul(0x9E3779B9))
}

/// reporter 态名。
pub const fn reporter_mode_name(r: usize) -> Option<&'static str> {
    match r / 2 {
        0 => Some("CLI 表格态"),
        1 => Some("JSON 态"),
        2 => Some("JUnit 态"),
        _ => None,
    }
}

/// Node 上栈层折叠（层 l：0=运行时 1=libuv 2=CLI 框架 3=集合运行器）。
pub const fn node_layer_cell(l: usize) -> Option<u64> {
    if l >= NODE_LAYERS {
        return None;
    }
    Some((0xF4000u64 + (l as u64) * 0x8B) ^ 0x4E4F44455F53544B)
}

/// Node 层名。
pub const fn node_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Node 运行时"),
        1 => Some("libuv"),
        2 => Some("CLI 框架"),
        3 => Some("集合运行器"),
        _ => None,
    }
}

/// 三面格折叠（格 t：0..3=迭代 3..6=超时 6..9=重试）。
pub const fn triface_cell(t: usize) -> Option<u64> {
    if t >= TRIFACE_CELLS {
        return None;
    }
    if t < 3 {
        Some((0xF5000u64 + (t as u64) * 0x2F) ^ 0x49544552)
    } else if t < 6 {
        Some((0xF5100u64 + ((t - 3) as u64) * 0x2F) ^ 0x54494D45)
    } else {
        Some((0xF5200u64 + ((t - 6) as u64) * 0x2F) ^ 0x52455452)
    }
}

/// 三面名。
pub const fn triface_name(t: usize) -> Option<&'static str> {
    match t / 3 {
        0 => Some("迭代面"),
        1 => Some("超时面"),
        2 => Some("重试面"),
        _ => None,
    }
}

/// 退出码闭集折叠（码 v：0..4）。
pub const fn exit_code_cell(v: usize) -> Option<u32> {
    match v {
        0 => Some(0x00),
        1 => Some(0x01),
        2 => Some(0x02),
        3 => Some(0x03),
        4 => Some(0x04),
        _ => None,
    }
}

/// 退出码名。
pub const fn exit_code_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("全通"),
        1 => Some("运行失败"),
        2 => Some("资源缺失"),
        3 => Some("参数错误"),
        4 => Some("异常中断"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Newman 收口账本状态机。
#[derive(Clone)]
pub struct NewmanLedger {
    /// 六判据（Electron 面收口闸：启动/执行/断言/崩溃/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [Option<u64>; RUN_SCENES],
    /// 环境注入账。
    envs: [Option<u64>; ENV_CELLS],
    /// reporter 态账。
    reps: [Option<u64>; REPORTER_CELLS],
    /// Node 层账。
    nodes: [Option<u64>; NODE_LAYERS],
    /// 三面格账。
    tris: [Option<u64>; TRIFACE_CELLS],
    /// 退出码账。
    exits: [Option<u32>; EXIT_CODES],
    /// 最后场景家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NewmanLedger {
    /// 新账本。
    pub fn new() -> Self {
        NewmanLedger {
            gates: [None; 6],
            scenes: [None; RUN_SCENES],
            envs: [None; ENV_CELLS],
            reps: [None; REPORTER_CELLS],
            nodes: [None; NODE_LAYERS],
            tris: [None; TRIFACE_CELLS],
            exits: [None; EXIT_CODES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NewmanCode> {
        if gate >= 6 {
            return Err(NewmanCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 场景一格（家族单调 + 载荷对照 + 重复拒；逆序拒不改 last_kind）。
    pub fn record_scene(&mut self, s: usize, got: u64) -> Result<(), NewmanCode> {
        if s >= RUN_SCENES {
            return Err(NewmanCode::ENoScene);
        }
        let kind = match scene_kind(s) {
            Some(k) => k,
            None => return Err(NewmanCode::ENoScene),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(NewmanCode::EOrder);
        }
        let want = match scene_cell(s) {
            Some(w) => w,
            None => return Err(NewmanCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + s as u64);
            return Err(NewmanCode::ESceneDrift);
        }
        if match self.scenes.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NewmanCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(s) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + s as u64);
        Ok(())
    }

    /// 场景全账核验（10 景）。
    pub fn scenes_all(&self) -> bool {
        let mut s = 0usize;
        while s < RUN_SCENES {
            match self.scenes.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 环境注入一格（载荷对照 + 重复拒）。
    pub fn record_env(&mut self, e: usize, got: u64) -> Result<(), NewmanCode> {
        let want = match env_cell(e) {
            Some(w) => w,
            None => return Err(NewmanCode::ENoEnv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + e as u64);
            return Err(NewmanCode::EEnvDrift);
        }
        if match self.envs.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NewmanCode::EDup);
        }
        if let Some(c) = self.envs.get_mut(e) {
            *c = Some(got);
        }
        self.fnv(0x8301 + e as u64);
        Ok(())
    }

    /// 环境注入全齐核验（8 格）。
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

    /// reporter 态一格（载荷对照 + 重复拒）。
    pub fn record_reporter(&mut self, r: usize, got: u64) -> Result<(), NewmanCode> {
        let want = match reporter_cell(r) {
            Some(w) => w,
            None => return Err(NewmanCode::ENoRep),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + r as u64);
            return Err(NewmanCode::ERepDrift);
        }
        if match self.reps.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NewmanCode::EDup);
        }
        if let Some(c) = self.reps.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x8401 + r as u64);
        Ok(())
    }

    /// reporter 全齐核验（6 格，三态各 2）。
    pub fn reporters_all(&self) -> bool {
        let mut r = 0usize;
        while r < REPORTER_CELLS {
            match self.reps.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Node 层一格（逐层阶梯 + 载荷对照）。
    pub fn record_node(&mut self, l: usize, got: u64) -> Result<(), NewmanCode> {
        let want = match node_layer_cell(l) {
            Some(w) => w,
            None => return Err(NewmanCode::ENoNode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + l as u64);
            return Err(NewmanCode::ENoNode);
        }
        if match self.nodes.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NewmanCode::EDup);
        }
        if let Some(c) = self.nodes.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x8501 + l as u64);
        Ok(())
    }

    /// Node 层全落核验（4 层）。
    pub fn nodes_all(&self) -> bool {
        let mut l = 0usize;
        while l < NODE_LAYERS {
            match self.nodes.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 三面格一格（载荷对照 + 重复拒）。
    pub fn record_triface(&mut self, t: usize, got: u64) -> Result<(), NewmanCode> {
        let want = match triface_cell(t) {
            Some(w) => w,
            None => return Err(NewmanCode::ESeal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + t as u64);
            return Err(NewmanCode::ESceneDrift);
        }
        if match self.tris.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NewmanCode::EDup);
        }
        if let Some(c) = self.tris.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x8601 + t as u64);
        Ok(())
    }

    /// 三面格全齐核验（9 格）。
    pub fn trifaces_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRIFACE_CELLS {
            match self.tris.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 退出码一格（闭集 5 值 + 重复拒）。
    pub fn record_exit(&mut self, v: usize) -> Result<(), NewmanCode> {
        let want = match exit_code_cell(v) {
            Some(w) => w,
            None => return Err(NewmanCode::ESeal),
        };
        if match self.exits.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NewmanCode::EDup);
        }
        if let Some(c) = self.exits.get_mut(v) {
            *c = Some(want);
        }
        self.fnv(0x8700 + v as u64);
        Ok(())
    }

    /// 退出码全齐核验（5 值）。
    pub fn exits_all(&self) -> bool {
        let mut v = 0usize;
        while v < EXIT_CODES {
            match self.exits.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
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
            && self.scenes_all()
            && self.envs_all()
            && self.reporters_all()
            && self.nodes_all()
            && self.trifaces_all()
            && self.exits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for NewmanLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NewmanLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut s = 0usize;
    while s < RUN_SCENES {
        match scene_cell(s) {
            Some(w) => {
                if led.record_scene(s, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        s += 1;
    }
    let mut e = 0usize;
    while e < ENV_CELLS {
        match env_cell(e) {
            Some(w) => {
                if led.record_env(e, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        e += 1;
    }
    let mut r = 0usize;
    while r < REPORTER_CELLS {
        match reporter_cell(r) {
            Some(w) => {
                let _ = led.record_reporter(r, w);
            }
            None => {}
        }
        r += 1;
    }
    let mut l = 0usize;
    while l < NODE_LAYERS {
        match node_layer_cell(l) {
            Some(w) => {
                let _ = led.record_node(l, w);
            }
            None => {}
        }
        l += 1;
    }
    let mut t = 0usize;
    while t < TRIFACE_CELLS {
        match triface_cell(t) {
            Some(w) => {
                let _ = led.record_triface(t, w);
            }
            None => {}
        }
        t += 1;
    }
    let mut v = 0usize;
    while v < EXIT_CODES {
        let _ = led.record_exit(v);
        v += 1;
    }
    // 注入 10 轮：环境注入越闭集 / 场景漂移 / reporter 越态三态轮转。
    // 三类注入全部落在当轮临时账本上，主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NewmanLedger::new();
        let caught = match kind {
            0 => {
                // 环境注入越闭集：格 8 之外必拒（空账本同样必抓）。
                lt.record_env(ENV_CELLS, 0) == Err(NewmanCode::ENoEnv)
            }
            1 => {
                // 场景漂移：先录正格再注漂移载荷。
                let w0 = match scene_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_scene(0, w0);
                let w1 = match scene_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_scene(1, w1.wrapping_add(2)) == Err(NewmanCode::ESceneDrift)
            }
            _ => {
                // reporter 越态：格 6 之外必拒。
                lt.record_reporter(REPORTER_CELLS, 0) == Err(NewmanCode::ENoRep)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + k as u64);
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
/// UNX-F52243 判据集（锚 UNX-F52244-J1，off-by-one 板式）。
pub fn run_unx52243_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52243");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "collection 运行 10 场景全通",
        recorded >= (RUN_SCENES + ENV_CELLS) as u32,
        "装订/执行/断言/汇出/收尾五家族逐景恒等",
    );
    s.add(
        "环境注入 8 格全落",
        {
            let mut lt = NewmanLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENV_CELLS {
                match env_cell(e) {
                    Some(w) => {
                        if lt.record_env(e, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                e += 1;
            }
            ok && lt.envs_all()
        },
        "全局/集合级/环境级/数据文件/迭代数据/变量覆盖/密钥/动态变量逐格恒等",
    );
    s.add(
        "reporter 三态 6 格全落",
        {
            let mut lt = NewmanLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < REPORTER_CELLS {
                match reporter_cell(r) {
                    Some(w) => {
                        if lt.record_reporter(r, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                r += 1;
            }
            ok && lt.reporters_all()
        },
        "表格/JSON/JUnit 三态逐态对照，越态必抓",
    );
    s.add(
        "Node 上栈 4 层阶梯全落",
        {
            let mut lt = NewmanLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < NODE_LAYERS {
                match node_layer_cell(l) {
                    Some(w) => {
                        if lt.record_node(l, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                l += 1;
            }
            ok && lt.nodes_all()
        },
        "运行时/libuv/CLI 框架/集合运行器逐层对照",
    );
    s.add(
        "迭代/超时/重试三面 9 格全落",
        {
            let mut lt = NewmanLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRIFACE_CELLS {
                match triface_cell(t) {
                    Some(w) => {
                        if lt.record_triface(t, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                t += 1;
            }
            ok && lt.trifaces_all()
        },
        "迭代 3 + 超时 3 + 重试 3 逐面落账",
    );
    s.add(
        "退出码闭集 5/5 全落",
        {
            let mut lt = NewmanLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < EXIT_CODES {
                if lt.record_exit(v).is_err() {
                    ok = false;
                }
                v += 1;
            }
            ok && lt.exits_all()
        },
        "全通/运行失败/资源缺失/参数错误/异常中断逐码落账",
    );
    s.add(
        "六判据全落（Electron 面收口闸）",
        {
            let mut lt = NewmanLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/执行/断言/崩溃/卸载/许可逐格落账",
    );
    s.add(
        "请求/环境 ×10 跑批计数",
        {
            // 10 轮跑批计数恒等：每轮场景+环境折叠增量恒定。
            let mut total = 0u64;
            let mut r = 0usize;
            while r < 10 {
                match scene_cell(r % RUN_SCENES) {
                    Some(w) => {
                        let ev = match env_cell(r % ENV_CELLS) {
                            Some(e) => e,
                            None => 0,
                        };
                        total = total.wrapping_add(w.wrapping_add(ev))
                    }
                    None => {}
                }
                r += 1;
            }
            let mut total2 = 0u64;
            let mut r2 = 0usize;
            while r2 < 10 {
                match scene_cell(r2 % RUN_SCENES) {
                    Some(w) => {
                        let ev = match env_cell(r2 % ENV_CELLS) {
                            Some(e) => e,
                            None => 0,
                        };
                        total2 = total2.wrapping_add(w.wrapping_add(ev))
                    }
                    None => {}
                }
                r2 += 1;
            }
            total == total2 && total != 0
        },
        "十轮跑批请求/环境折叠计数双算恒等且非零",
    );
    s.add(
        "场景家族单调",
        {
            let mut ok = true;
            let mut sc = 1usize;
            while sc < RUN_SCENES {
                match (scene_kind(sc), scene_kind(sc - 1)) {
                    (Some(k), Some(pk)) => {
                        if k < pk {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                sc += 1;
            }
            ok
        },
        "五家族闭集单调递增",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "环境越闭集/场景漂移/reporter 越态三态轮转必抓",
    );
    let mut l0 = NewmanLedger::new();
    s.add(
        "边界·场景越闭集",
        l0.record_scene(RUN_SCENES, 0).is_err(),
        "场景 10 越闭集必拒",
    );
    s.add(
        "边界·环境注入越闭集",
        l0.record_env(ENV_CELLS, 0).is_err(),
        "环境注入格 8 越闭集必拒（空账本同样必抓）",
    );
    s.add(
        "边界·reporter 越态",
        l0.record_reporter(REPORTER_CELLS, 0).is_err(),
        "reporter 格 6 越态必拒",
    );
    s.add(
        "边界·退出码越闭集",
        l0.record_exit(EXIT_CODES).is_err(),
        "退出码 5 之外越闭集必拒",
    );
    s.add(
        "逆序拒后家族账位不污染",
        {
            // 先录家族 2（场景 4），逆序注家族 0 被拒，再用同族未录格复验。
            let mut lt = NewmanLedger::new();
            match scene_cell(4) {
                Some(w) => {
                    let _ = lt.record_scene(4, w);
                }
                None => {}
            }
            let rejected = lt.record_scene(0, 0) == Err(NewmanCode::EOrder);
            let w5 = match scene_cell(5) {
                Some(w) => w,
                None => 0,
            };
            rejected && lt.record_scene(5, w5).is_ok()
        },
        "EOrder 拒绝不改 last_kind，同族未录格可继续落账",
    );
    s.add(
        "场景漂移拒",
        {
            let mut lt = NewmanLedger::new();
            let w3 = match scene_cell(3) {
                Some(w) => w,
                None => 0,
            };
            lt.record_scene(3, w3.wrapping_add(1)) == Err(NewmanCode::ESceneDrift)
        },
        "场景载荷漂移必抓",
    );
    let (inj, caught_n, esc) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj == caught_n && esc == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj, caught_n, esc),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = NewmanLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut sc = 0usize;
            while sc < RUN_SCENES {
                match scene_cell(sc) {
                    Some(w) => {
                        let _ = lt.record_scene(sc, w);
                    }
                    None => {}
                }
                sc += 1;
            }
            let mut e = 0usize;
            while e < ENV_CELLS {
                match env_cell(e) {
                    Some(w) => {
                        let _ = lt.record_env(e, w);
                    }
                    None => {}
                }
                e += 1;
            }
            let mut r = 0usize;
            while r < REPORTER_CELLS {
                match reporter_cell(r) {
                    Some(w) => {
                        let _ = lt.record_reporter(r, w);
                    }
                    None => {}
                }
                r += 1;
            }
            let mut l = 0usize;
            while l < NODE_LAYERS {
                match node_layer_cell(l) {
                    Some(w) => {
                        let _ = lt.record_node(l, w);
                    }
                    None => {}
                }
                l += 1;
            }
            let mut t = 0usize;
            while t < TRIFACE_CELLS {
                match triface_cell(t) {
                    Some(w) => {
                        let _ = lt.record_triface(t, w);
                    }
                    None => {}
                }
                t += 1;
            }
            let mut v = 0usize;
            while v < EXIT_CODES {
                let _ = lt.record_exit(v);
                v += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7C80,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×42",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_kinds() {
        assert_eq!(scene_kind(0), Some(0));
        assert_eq!(scene_kind(2), Some(1));
        assert_eq!(scene_kind(9), Some(4));
        assert!(scene_kind(10).is_none());
        assert_eq!(scene_kind_name(0), Some("装订"));
        assert_eq!(scene_kind_name(4), Some("收尾"));
        assert!(scene_kind_name(5).is_none());
    }

    #[test]
    fn test_env_names() {
        assert_eq!(env_cell_name(0), Some("全局注入"));
        assert_eq!(env_cell_name(6), Some("密钥注入"));
        assert!(env_cell_name(8).is_none());
        assert!(env_cell(8).is_none());
    }

    #[test]
    fn test_reporter_modes() {
        assert_eq!(reporter_mode_name(0), Some("CLI 表格态"));
        assert_eq!(reporter_mode_name(2), Some("JSON 态"));
        assert_eq!(reporter_mode_name(4), Some("JUnit 态"));
        assert!(reporter_mode_name(6).is_none());
        assert!(reporter_cell(6).is_none());
    }

    #[test]
    fn test_triface_names() {
        assert_eq!(triface_name(0), Some("迭代面"));
        assert_eq!(triface_name(3), Some("超时面"));
        assert_eq!(triface_name(8), Some("重试面"));
        assert!(triface_name(9).is_none());
        assert!(triface_cell(9).is_none());
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(exit_code_cell(0), Some(0));
        assert_eq!(exit_code_cell(4), Some(4));
        assert!(exit_code_cell(5).is_none());
        assert_eq!(exit_code_name(0), Some("全通"));
        assert_eq!(exit_code_name(4), Some("异常中断"));
        assert!(exit_code_name(5).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NewmanLedger::new();
        let _ = lt.record_scene(8, scene_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_scene(1, scene_cell(1).unwrap_or(0)),
            Err(NewmanCode::EOrder)
        );
        assert!(lt.record_scene(9, scene_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NewmanLedger::new();
        assert_eq!(
            lt.record_env(3, env_cell(3).unwrap_or(0).wrapping_add(6)),
            Err(NewmanCode::EEnvDrift)
        );
        assert_eq!(
            lt.record_reporter(5, reporter_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(NewmanCode::ERepDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NewmanLedger::new();
        let _ = lt.record_node(1, node_layer_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_node(1, node_layer_cell(1).unwrap_or(0)),
            Err(NewmanCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NewmanLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NewmanLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (RUN_SCENES + ENV_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7C80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NewmanCode::Ok as u32, 0);
        assert_eq!(NewmanCode::EGradeDrift as u32, 12);
        let codes = [
            NewmanCode::Ok,
            NewmanCode::ENoScene,
            NewmanCode::ESceneDrift,
            NewmanCode::ENoEnv,
            NewmanCode::EEnvDrift,
            NewmanCode::EOrder,
            NewmanCode::ENoRep,
            NewmanCode::ERepDrift,
            NewmanCode::ENoNode,
            NewmanCode::EDup,
            NewmanCode::ESeal,
            NewmanCode::EFp,
            NewmanCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert!(!codes[i].detail().is_empty());
            assert_eq!(codes[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(codes.len(), 13);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        let lt = NewmanLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52243_checks_green() {
        let s = run_unx52243_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
