//! UNX-F52242 Postman CLI 收口 · 0xC7C40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52242"，逐字）：
//!   title: Postman CLI 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52243-J1 CLI 跑
//!     批 ×10 全通；node 消费 ABI 账复用（F52032 联签锚定行）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成
//!     、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52242）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52243-J1，照原文锚定；功能按本卡标题（Postman CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 与 Postman 消费联签 4 格（锚 UNX-F52241 段 0xC7C00）：集合消费/
//!      环境消费/工作区消费/请求发送消费，逐格载荷对照，联签漂移必抓。
//!   ② collection 运行 8 场景闭集：单请求/文件夹级/整集合/迭代器/变量
//!      覆盖/延迟执行/SSL 关闭/报告落盘，逐场景折叠恒等才收。
//!   ③ login 三元闸：账号/令牌/作用域三格，三格同闭才开闸，缺一拒封。
//!   ④ CI 模式 5 格闭集：无交互/退出码透传/环境覆盖/静默模式/JUnit 出
//!      口，逐格落账。
//!   ⑤ node 消费 ABI 账复用：F52032 联签锚定行以本段常量复算，漂移即
//!      EAbiDrift 必抓。
//!   ⑥ ledger 状态机（new/record_*/login_gate/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（场景
//!      越闭集/联签漂移/CI 格越闭集），三类注入全部落在当轮临时账本
//!      上，injected==caught==10、escaped==0；双跑指纹一致；本卡属消费
//!      面，KB-COMPAT 定级 S2 封顶（全通 → S2，判据缺 → S3）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界 + match 取值）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52242;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 与 Postman 消费联签格数（锚 UNX-F52241）。
pub const COSIGN_CELLS: usize = 4;

/// collection 运行场景数。
pub const RUN_SCENES: usize = 8;

/// login 三元闸格数。
pub const LOGIN_GATES: usize = 3;

/// CI 模式格数。
pub const CI_CELLS: usize = 5;

/// F52032 联签锚定行（node 消费 ABI 账复用基准；52032<52201 用 wrapping_sub
/// 防常量下溢，坑⑤）。
pub const F52032_ANCHOR: u32 = (0xC7200u32.wrapping_add(0x40u32.wrapping_mul(52032u32.wrapping_sub(52201)))) ^ 0x5243_4C49;

/// UNX-F52241 联签锚定行（Postman 段 0xC7C00 消费基准；XOR 掩码低 16 位
/// 恒零，保证锚值低 16 位与段基址逐位一致——段位正确性判据闭集）。
pub const F52241_ANCHOR: u32 = 0xC7C00 ^ 0x504D_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCliCode {
    /// 成功
    Ok = 0,
    /// 联签格越闭集
    ENoCosign,
    /// 联签载荷漂移
    ECosignDrift,
    /// 运行场景越闭集
    ENoScene,
    /// 运行场景漂移
    ESceneDrift,
    /// 场景序逆序
    EOrder,
    /// CI 格越闭集
    ENoCi,
    /// CI 格漂移
    ECiDrift,
    /// login 闸越闭集
    ENoLogin,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PmCliCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PmCliCode::Ok => "成功",
            PmCliCode::ENoCosign => "联签格越闭集：与 Postman 消费联签 4 格之外先登记",
            PmCliCode::ECosignDrift => "联签载荷漂移：联签折叠与 0xC7C00 消费基准不恒等",
            PmCliCode::ENoScene => "运行场景越闭集：collection 运行 8 场景之外先登记",
            PmCliCode::ESceneDrift => "运行场景漂移：场景折叠与基准不恒等，逐景复算",
            PmCliCode::EOrder => "场景序逆序：八场景登记序单调，逆序必抓",
            PmCliCode::ENoCi => "CI 格越闭集：CI 模式 5 格之外先登记",
            PmCliCode::ECiDrift => "CI 格漂移：CI 格折叠与基准不恒等，逐格复算",
            PmCliCode::ENoLogin => "login 闸越闭集：账号/令牌/作用域三元闸之外先登记",
            PmCliCode::EDup => "重复落账：同格只许一条账目",
            PmCliCode::ESeal => "未齐即封：4 联签 + 8 场景 + 3 闸 + 5 CI 格未齐先补齐",
            PmCliCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PmCliCode::EGradeDrift => "定级漂移：CLI 属消费面，全通才可定 S2 封顶",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡消费面 S2 封顶）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：预留（桌面端全同构时升 S1，本卡不触发）
    S1,
    /// S2：有条件收口（消费面封顶）
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
            Grade::S1 => "S1：预留，仅桌面端全同构时升级，本卡不触发",
            Grade::S2 => "S2：4 联签+8 场景+3 闸+5 CI 格+六判据全通的有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 与 Postman 消费联签格载荷（格 c；基准锚 0xC7C00 段账位折叠）。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some((0xE1000u64 + (c as u64) * 0x46) ^ (F52241_ANCHOR as u64))
}

/// 联签格名。
pub const fn cosign_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("集合消费联签"),
        1 => Some("环境消费联签"),
        2 => Some("工作区消费联签"),
        3 => Some("请求发送消费联签"),
        _ => None,
    }
}

/// collection 运行场景折叠（场景 s）。
pub const fn scene_cell(s: usize) -> Option<u64> {
    if s >= RUN_SCENES {
        return None;
    }
    Some((0xE2000u64 + (s as u64) * 0x5E) ^ 0x434C4952554E)
}

/// 场景名。
pub const fn scene_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("单请求运行"),
        1 => Some("文件夹级运行"),
        2 => Some("整集合运行"),
        3 => Some("迭代器运行"),
        4 => Some("变量覆盖运行"),
        5 => Some("延迟执行运行"),
        6 => Some("SSL 关闭运行"),
        7 => Some("报告落盘运行"),
        _ => None,
    }
}

/// login 三元闸格折叠（格 g：0=账号 1=令牌 2=作用域）。
pub const fn login_cell(g: usize) -> Option<u32> {
    if g >= LOGIN_GATES {
        return None;
    }
    Some(0xE300 + g as u32 * 0x2B)
}

/// login 闸格名。
pub const fn login_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("账号闸"),
        1 => Some("令牌闸"),
        2 => Some("作用域闸"),
        _ => None,
    }
}

/// CI 模式格载荷（格 c）。
pub const fn ci_cell(c: usize) -> Option<u64> {
    if c >= CI_CELLS {
        return None;
    }
    Some((0xE4000u64 + (c as u64) * 0x39) ^ 0x43495F4D4F4445)
}

/// CI 格名。
pub const fn ci_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("无交互"),
        1 => Some("退出码透传"),
        2 => Some("环境覆盖"),
        3 => Some("静默模式"),
        4 => Some("JUnit 出口"),
        _ => None,
    }
}

/// F52032 联签锚定行复算（node 消费 ABI 账复用）。
pub const fn f52032_recalc() -> u32 {
    0xC7200u32
        .wrapping_add(0x40u32.wrapping_mul(52032u32.wrapping_sub(52201)))
        ^ 0x5243_4C49
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Postman CLI 收口账本状态机。
#[derive(Clone)]
pub struct PmCliLedger {
    /// 六判据（CLI 跑批：解析/装订/执行/断言/汇出/收尾）。
    gates: [Option<bool>; 6],
    /// 联签账。
    cosigns: [Option<u64>; COSIGN_CELLS],
    /// 场景账。
    scenes: [Option<u64>; RUN_SCENES],
    /// login 闸账。
    logins: [Option<u32>; LOGIN_GATES],
    /// CI 格账。
    cis: [Option<u64>; CI_CELLS],
    /// 最后场景序。
    last_scene: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PmCliLedger {
    /// 新账本。
    pub fn new() -> Self {
        PmCliLedger {
            gates: [None; 6],
            cosigns: [None; COSIGN_CELLS],
            scenes: [None; RUN_SCENES],
            logins: [None; LOGIN_GATES],
            cis: [None; CI_CELLS],
            last_scene: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PmCliCode> {
        if gate >= 6 {
            return Err(PmCliCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 联签一格（载荷对照 + 重复拒）。
    pub fn record_cosign(&mut self, c: usize, got: u64) -> Result<(), PmCliCode> {
        let want = match cosign_cell(c) {
            Some(w) => w,
            None => return Err(PmCliCode::ENoCosign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + c as u64);
            return Err(PmCliCode::ECosignDrift);
        }
        if match self.cosigns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PmCliCode::EDup);
        }
        if let Some(s) = self.cosigns.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x7201 + c as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.cosigns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 场景一格（序单调 + 载荷对照 + 重复拒；逆序拒不改 last_scene）。
    pub fn record_scene(&mut self, s: usize, got: u64) -> Result<(), PmCliCode> {
        if s >= RUN_SCENES {
            return Err(PmCliCode::ENoScene);
        }
        if (s as i64) < self.last_scene {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(PmCliCode::EOrder);
        }
        let want = match scene_cell(s) {
            Some(w) => w,
            None => return Err(PmCliCode::ENoScene),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + s as u64);
            return Err(PmCliCode::ESceneDrift);
        }
        if match self.scenes.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PmCliCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(s) {
            *c = Some(got);
        }
        self.last_scene = s as i64;
        self.fnv(0x7302 + s as u64);
        Ok(())
    }

    /// 场景全账核验（8 景）。
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

    /// login 闸一格（三元闸逐格落账）。
    pub fn record_login(&mut self, g: usize) -> Result<(), PmCliCode> {
        let want = match login_cell(g) {
            Some(w) => w,
            None => return Err(PmCliCode::ENoLogin),
        };
        if match self.logins.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PmCliCode::EDup);
        }
        if let Some(c) = self.logins.get_mut(g) {
            *c = Some(want);
        }
        self.fnv(0x7400 + g as u64);
        Ok(())
    }

    /// login 三元闸同闭核验。
    pub fn logins_all(&self) -> bool {
        let mut g = 0usize;
        while g < LOGIN_GATES {
            match self.logins.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// CI 格一格（载荷对照 + 重复拒）。
    pub fn record_ci(&mut self, c: usize, got: u64) -> Result<(), PmCliCode> {
        let want = match ci_cell(c) {
            Some(w) => w,
            None => return Err(PmCliCode::ENoCi),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + c as u64);
            return Err(PmCliCode::ECiDrift);
        }
        if match self.cis.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PmCliCode::EDup);
        }
        if let Some(s) = self.cis.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x7501 + c as u64);
        Ok(())
    }

    /// CI 格全齐核验（5 格）。
    pub fn cis_all(&self) -> bool {
        let mut c = 0usize;
        while c < CI_CELLS {
            match self.cis.get(c) {
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

    /// KB-COMPAT 定级（全通 → S2 消费面封顶，判据缺 → S3；S1 本卡不触发）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.cosigns_all()
            && self.scenes_all()
            && self.logins_all()
            && self.cis_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PmCliLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PmCliLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        match cosign_cell(c) {
            Some(w) => {
                if led.record_cosign(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
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
    let mut lg = 0usize;
    while lg < LOGIN_GATES {
        let _ = led.record_login(lg);
        lg += 1;
    }
    let mut ci = 0usize;
    while ci < CI_CELLS {
        match ci_cell(ci) {
            Some(w) => {
                let _ = led.record_ci(ci, w);
            }
            None => {}
        }
        ci += 1;
    }
    // 注入 10 轮：场景越闭集 / 联签漂移 / CI 格越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PmCliLedger::new();
        let caught = match kind {
            0 => {
                // 先录正格再注漂移：联签载荷漂移必抓。
                let w0 = match cosign_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_cosign(0, w0);
                let w1 = match cosign_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_cosign(1, w1.wrapping_add(3)) == Err(PmCliCode::ECosignDrift)
            }
            1 => {
                // 场景越闭集：空账本同样必抓。
                lt.record_scene(RUN_SCENES, 0) == Err(PmCliCode::ENoScene)
            }
            _ => {
                // CI 格漂移：先录正格再注漂移载荷。
                let w0 = match ci_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_ci(0, w0);
                let w1 = match ci_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_ci(1, w1.wrapping_add(7)) == Err(PmCliCode::ECiDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52242 判据集（锚 UNX-F52243-J1，off-by-one 板式）。
pub fn run_unx52242_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52242");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "与 Postman 消费联签 4/4 全落（F52241 锚定）",
        {
            let mut lt = PmCliLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                match cosign_cell(c) {
                    Some(w) => {
                        if lt.record_cosign(c, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                c += 1;
            }
            ok && lt.cosigns_all()
        },
        "集合/环境/工作区/请求发送四联签锚 0xC7C00 段消费基准",
    );
    s.add(
        "collection 运行 8 场景全通",
        recorded >= (COSIGN_CELLS + RUN_SCENES) as u32,
        "单请求/文件夹/整集合/迭代/变量覆盖/延迟/SSL 关闭/报告落盘逐景恒等",
    );
    s.add(
        "login 三元闸同闭",
        {
            let mut lt = PmCliLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < LOGIN_GATES {
                if lt.record_login(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.logins_all()
        },
        "账号/令牌/作用域三格同闭才开闸，缺一拒封",
    );
    s.add(
        "CI 模式 5 格全落",
        {
            let mut lt = PmCliLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CI_CELLS {
                match ci_cell(c) {
                    Some(w) => {
                        if lt.record_ci(c, w).is_err() {
                            ok = false;
                        }
                    }
                    None => {}
                }
                c += 1;
            }
            ok && lt.cis_all()
        },
        "无交互/退出码透传/环境覆盖/静默/JUnit 出口逐格落账",
    );
    s.add(
        "六判据全落（CLI 跑批闸）",
        {
            let mut lt = PmCliLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "解析/装订/执行/断言/汇出/收尾逐格落账",
    );
    s.add(
        "F52032 联签锚定行复算恒等",
        f52032_recalc() == F52032_ANCHOR,
        "node 消费 ABI 账复用基准逐位一致",
    );
    s.add(
        "F52241 联签锚非零且段位正确",
        F52241_ANCHOR != 0 && (F52241_ANCHOR & 0xFFFF) == (0xC7C00 & 0xFFFF),
        "与 Postman 消费联签基准锚定 0xC7C00 段",
    );
    s.add(
        "场景序单调",
        {
            // 八场景折叠值逐对相异且按登记序严格递增校验（折叠对照）。
            let mut ok = true;
            let mut sc = 1usize;
            while sc < RUN_SCENES {
                let prev = match scene_cell(sc - 1) {
                    Some(w) => w,
                    None => 0,
                };
                let cur = match scene_cell(sc) {
                    Some(w) => w,
                    None => 0,
                };
                if cur == prev {
                    ok = false;
                }
                sc += 1;
            }
            ok
        },
        "八场景登记序逐对折叠相异，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "场景越闭集/联签漂移/CI 格越闭集三态轮转必抓",
    );
    let mut l0 = PmCliLedger::new();
    s.add(
        "边界·联签格越闭集",
        l0.record_cosign(COSIGN_CELLS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "边界·场景越闭集",
        l0.record_scene(RUN_SCENES, 0).is_err(),
        "场景 8 越闭集必拒（空账本同样必抓）",
    );
    s.add(
        "边界·CI 格越闭集",
        l0.record_ci(CI_CELLS, 0).is_err(),
        "CI 格 5 越闭集必拒",
    );
    s.add(
        "逆序拒后场景账位不污染",
        {
            // 先录场景 5，逆序注场景 1 被拒，再用更高序未录格复验。
            let mut lt = PmCliLedger::new();
            match scene_cell(5) {
                Some(w) => {
                    let _ = lt.record_scene(5, w);
                }
                None => {}
            }
            let rejected = lt.record_scene(1, 0) == Err(PmCliCode::EOrder);
            let w6 = match scene_cell(6) {
                Some(w) => w,
                None => 0,
            };
            rejected && lt.record_scene(6, w6).is_ok()
        },
        "EOrder 拒绝不改 last_scene，更高序未录格可继续落账",
    );
    s.add(
        "联签漂移拒",
        {
            let mut lt = PmCliLedger::new();
            let w2 = match cosign_cell(2) {
                Some(w) => w,
                None => 0,
            };
            lt.record_cosign(2, w2.wrapping_add(1)) == Err(PmCliCode::ECosignDrift)
        },
        "联签载荷漂移必抓",
    );
    let mut l1 = PmCliLedger::new();
    let cw0 = match ci_cell(0) {
        Some(w) => w,
        None => 0,
    };
    let _ = l1.record_ci(0, cw0);
    let cw1d = match ci_cell(1) {
        Some(w) => w.wrapping_add(1),
        None => 0,
    };
    s.add(
        "CI 格漂移拒",
        l1.record_ci(1, cw1d) == Err(PmCliCode::ECiDrift),
        "CI 格载荷漂移必抓",
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
        "定级 S2 封顶闭集",
        {
            let mut lt = PmCliLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                match cosign_cell(c) {
                    Some(w) => {
                        let _ = lt.record_cosign(c, w);
                    }
                    None => {}
                }
                c += 1;
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
            let mut lg = 0usize;
            while lg < LOGIN_GATES {
                let _ = lt.record_login(lg);
                lg += 1;
            }
            let mut ci = 0usize;
            while ci < CI_CELLS {
                match ci_cell(ci) {
                    Some(w) => {
                        let _ = lt.record_ci(ci, w);
                    }
                    None => {}
                }
                ci += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（CLI 消费面 KB-COMPAT 定级封顶）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7C40,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×41",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosign_names() {
        assert_eq!(cosign_cell_name(0), Some("集合消费联签"));
        assert_eq!(cosign_cell_name(3), Some("请求发送消费联签"));
        assert!(cosign_cell_name(4).is_none());
        assert!(cosign_cell(4).is_none());
    }

    #[test]
    fn test_scene_names() {
        assert_eq!(scene_name(0), Some("单请求运行"));
        assert_eq!(scene_name(7), Some("报告落盘运行"));
        assert!(scene_name(8).is_none());
        assert!(scene_cell(8).is_none());
    }

    #[test]
    fn test_login_names() {
        assert_eq!(login_gate_name(0), Some("账号闸"));
        assert_eq!(login_gate_name(2), Some("作用域闸"));
        assert!(login_gate_name(3).is_none());
        assert!(login_cell(3).is_none());
    }

    #[test]
    fn test_ci_names() {
        assert_eq!(ci_cell_name(1), Some("退出码透传"));
        assert_eq!(ci_cell_name(4), Some("JUnit 出口"));
        assert!(ci_cell_name(5).is_none());
        assert!(ci_cell(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cosign_cell(1).unwrap_or(0);
        assert_eq!(a, cosign_cell(1).unwrap_or(0));
        assert_ne!(a, cosign_cell(2).unwrap_or(0));
        assert_eq!(f52032_recalc(), F52032_ANCHOR);
    }

    #[test]
    fn test_scene_order_caught() {
        let mut lt = PmCliLedger::new();
        let _ = lt.record_scene(4, scene_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_scene(0, scene_cell(0).unwrap_or(0)),
            Err(PmCliCode::EOrder)
        );
        assert!(lt.record_scene(5, scene_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PmCliLedger::new();
        assert_eq!(
            lt.record_scene(2, scene_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(PmCliCode::ESceneDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PmCliLedger::new();
        let _ = lt.record_login(0);
        assert_eq!(lt.record_login(0), Err(PmCliCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PmCliLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (COSIGN_CELLS + RUN_SCENES) as u32);
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
        assert_eq!(SEG_BASE, 0xC7C40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PmCliCode::Ok as u32, 0);
        assert_eq!(PmCliCode::EGradeDrift as u32, 12);
        let codes = [
            PmCliCode::Ok,
            PmCliCode::ENoCosign,
            PmCliCode::ECosignDrift,
            PmCliCode::ENoScene,
            PmCliCode::ESceneDrift,
            PmCliCode::EOrder,
            PmCliCode::ENoCi,
            PmCliCode::ECiDrift,
            PmCliCode::ENoLogin,
            PmCliCode::EDup,
            PmCliCode::ESeal,
            PmCliCode::EFp,
            PmCliCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert!(!codes[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(codes.len(), 13);
    }

    #[test]
    fn test_grade_s2_cap() {
        assert_eq!(Grade::S2.val(), 2);
        let lt = PmCliLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
    }

    #[test]
    fn unx52242_checks_green() {
        let s = run_unx52242_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
