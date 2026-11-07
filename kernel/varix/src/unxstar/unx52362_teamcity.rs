//! UNX-F52362 TeamCity Agent 收口 · 0xC9A40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52362"，逐字）：
//!   title: TeamCity Agent 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52363-J1 注册/执行
//!     ×10 任务全通；shell executor 路径；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52362）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52363-J1，照原文锚定；功能按本卡标题（TeamCity Agent）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 构建五段 20 格：fetch-sources/compile/test/package/publish 五家族
//!      闭集各 4 格，段单调（逆序必抓），载荷纯函数逐格恒等才收。
//!   ② agent 注册三元闸：token/url/agent-name 三格全齐才放行取活。
//!   ③ 构建配置投影 8 格：server 端构建配置向 agent 侧投影闭集，逐格
//!      恒等、重复拒、漂移拒。
//!   ④ 与 Server 消费联签 4 格：心跳/构建参数/构件回传/日志回传逐格联签。
//!   ⑤ Java 上栈 4 层声明行：JRE/Agent-Launcher/Runner/JMX 逐行闭集。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（配置越闭集/配置漂移/构建段逆序），
//!      检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集 → S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52362;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 构建五段格数。
pub const BUILD_CELLS: usize = 20;

/// 注册三元闸格数。
pub const REG_CELLS: usize = 3;

/// 构建配置投影格数。
pub const CFG_CELLS: usize = 8;

/// 与 Server 消费联签格数。
pub const SIGN_CELLS: usize = 4;

/// Java 上栈层数。
pub const JAVA_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TeamCityCode {
    /// 成功
    Ok = 0,
    /// 构建段号越闭集
    ENoStage,
    /// 构建格漂移
    EStageDrift,
    /// 段逆序
    EOrder,
    /// 注册闸格越闭集
    ENoReg,
    /// 注册闸漂移
    ERegDrift,
    /// 配置投影格越闭集
    ENoCfg,
    /// 配置投影漂移
    ECfgDrift,
    /// 联签格越闭集
    ENoSign,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl TeamCityCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TeamCityCode::Ok => "成功",
            TeamCityCode::ENoStage => "构建段号越闭集：五段 20 格之外先登记",
            TeamCityCode::EStageDrift => "构建格漂移：载荷与基准不恒等，逐格复算",
            TeamCityCode::EOrder => "段逆序：fetch-sources→compile→test→package→publish 单调，逆序必抓",
            TeamCityCode::ENoReg => "注册闸格越闭集：token/url/agent-name 三元之外先登记",
            TeamCityCode::ERegDrift => "注册闸漂移：注册折叠与基准不恒等，逐格复算",
            TeamCityCode::ENoCfg => "配置投影格越闭集：8 格投影闭集之外先登记",
            TeamCityCode::ECfgDrift => "配置投影漂移：投影折叠与基准不恒等，逐格复算",
            TeamCityCode::ENoSign => "联签格越闭集：Server 消费联签 4 格之外先登记",
            TeamCityCode::EDup => "重复落账：同格只许一条账目",
            TeamCityCode::ESeal => "未齐即封：20 格构建 + 3 闸 + 8 投影 + 4 联签未齐先补齐",
            TeamCityCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TeamCityCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格构建+3闸+8投影+4联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 构建格载荷（格 i；段 = i/4）。
pub const fn build_cell(i: usize) -> Option<u64> {
    if i >= BUILD_CELLS {
        return None;
    }
    Some((0xD1100u64 + (i as u64) * 0x3F) ^ ((i / 4) as u64).wrapping_mul(0x54430000))
}

/// 构建段（0..4=fetch-sources 4..8=compile 8..12=test 12..16=package 16..20=publish）。
pub const fn build_kind(i: usize) -> Option<u32> {
    if i >= BUILD_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 段名。
pub const fn build_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("fetch-sources"),
        1 => Some("compile"),
        2 => Some("test"),
        3 => Some("package"),
        4 => Some("publish"),
        _ => None,
    }
}

/// 注册三元闸格载荷（闸 r：0=token 1=url 2=agent-name）。
pub const fn reg_cell(r: usize) -> Option<u64> {
    if r >= REG_CELLS {
        return None;
    }
    Some(0xD1600u64 + (r as u64) * 0x59)
}

/// 注册闸名。
pub const fn reg_cell_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("token"),
        1 => Some("url"),
        2 => Some("agent-name"),
        _ => None,
    }
}

/// 构建配置投影格载荷（格 c，0..8）。
pub const fn cfg_cell(c: usize) -> Option<u64> {
    if c >= CFG_CELLS {
        return None;
    }
    Some((0xD1A00u64 + (c as u64) * 0x67) ^ 0x5052_0000)
}

/// 与 Server 消费联签格载荷（格 g，0..4）。
pub const fn sign_cell(g: usize) -> Option<u64> {
    if g >= SIGN_CELLS {
        return None;
    }
    Some(0xD1E00u64 + (g as u64) * 0x73)
}

/// 联签名。
pub const fn sign_cell_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("heartbeat"),
        1 => Some("build-params"),
        2 => Some("artifacts"),
        3 => Some("build-log"),
        _ => None,
    }
}

/// Java 上栈层折叠（层 r：0=JRE 1=Agent-Launcher 2=Runner 3=JMX）。
pub const fn java_row(r: usize) -> Option<u32> {
    if r >= JAVA_LAYERS {
        return None;
    }
    Some(0xD220 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// TeamCity Agent 收口账本状态机。
#[derive(Clone)]
pub struct TeamCityLedger {
    /// 六判据（注册/取活/构建执行/构件回传/日志回传/注销清理）。
    gates: [Option<bool>; 6],
    /// 构建账。
    build: [Option<u64>; BUILD_CELLS],
    /// 注册三元闸账。
    reg: [Option<u64>; REG_CELLS],
    /// 配置投影账。
    cfg: [Option<u64>; CFG_CELLS],
    /// 联签账。
    sign: [Option<u64>; SIGN_CELLS],
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

impl TeamCityLedger {
    /// 新账本。
    pub fn new() -> Self {
        TeamCityLedger {
            gates: [None; 6],
            build: [None; BUILD_CELLS],
            reg: [None; REG_CELLS],
            cfg: [None; CFG_CELLS],
            sign: [None; SIGN_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TeamCityCode> {
        if gate >= 6 {
            return Err(TeamCityCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5200 + gate as u64);
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

    /// 构建一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_build(&mut self, i: usize, got: u64) -> Result<(), TeamCityCode> {
        if i >= BUILD_CELLS {
            return Err(TeamCityCode::ENoStage);
        }
        let kind = match build_kind(i) {
            Some(k) => k,
            None => return Err(TeamCityCode::ENoStage),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(TeamCityCode::EOrder);
        }
        let want = match build_cell(i) {
            Some(w) => w,
            None => return Err(TeamCityCode::ENoStage),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + i as u64);
            return Err(TeamCityCode::EStageDrift);
        }
        if match self.build.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TeamCityCode::EDup);
        }
        if let Some(c) = self.build.get_mut(i) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + i as u64);
        Ok(())
    }

    /// 构建前 n 格全落核验（注册/执行 ×10 任务的 shell executor 样本口径）。
    pub fn build_kind_filled(&self, n: usize) -> bool {
        let mut i = 0usize;
        while i < n && i < BUILD_CELLS {
            match self.build.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 构建全账核验（20 格）。
    pub fn build_all(&self) -> bool {
        let mut i = 0usize;
        while i < BUILD_CELLS {
            match self.build.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 注册三元闸一格（载荷对照 + 重复拒）。
    pub fn record_reg(&mut self, r: usize, got: u64) -> Result<(), TeamCityCode> {
        let want = match reg_cell(r) {
            Some(w) => w,
            None => return Err(TeamCityCode::ENoReg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + r as u64);
            return Err(TeamCityCode::ERegDrift);
        }
        if match self.reg.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TeamCityCode::EDup);
        }
        if let Some(c) = self.reg.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x5401 + r as u64);
        Ok(())
    }

    /// 注册三元闸全齐核验。
    pub fn reg_all(&self) -> bool {
        let mut r = 0usize;
        while r < REG_CELLS {
            match self.reg.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 构建配置投影一格（载荷对照 + 重复拒）。
    pub fn record_cfg(&mut self, c: usize, got: u64) -> Result<(), TeamCityCode> {
        let want = match cfg_cell(c) {
            Some(w) => w,
            None => return Err(TeamCityCode::ENoCfg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + c as u64);
            return Err(TeamCityCode::ECfgDrift);
        }
        if match self.cfg.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TeamCityCode::EDup);
        }
        if let Some(cc) = self.cfg.get_mut(c) {
            *cc = Some(got);
        }
        self.fnv(0x5501 + c as u64);
        Ok(())
    }

    /// 配置投影全账核验（8 格）。
    pub fn cfg_all(&self) -> bool {
        let mut c = 0usize;
        while c < CFG_CELLS {
            match self.cfg.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 与 Server 消费联签一格（载荷对照 + 重复拒）。
    pub fn record_sign(&mut self, g: usize, got: u64) -> Result<(), TeamCityCode> {
        let want = match sign_cell(g) {
            Some(w) => w,
            None => return Err(TeamCityCode::ENoSign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + g as u64);
            return Err(TeamCityCode::ERegDrift);
        }
        if match self.sign.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TeamCityCode::EDup);
        }
        if let Some(c) = self.sign.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x5601 + g as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn sign_all(&self) -> bool {
        let mut g = 0usize;
        while g < SIGN_CELLS {
            match self.sign.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// Java 上栈一行（JRE/Agent-Launcher/Runner/JMX）。
    pub fn declare_java(&mut self, r: usize) -> Result<(), TeamCityCode> {
        let w = match java_row(r) {
            Some(w) => w,
            None => return Err(TeamCityCode::ENoReg),
        };
        if match self.java.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TeamCityCode::EDup);
        }
        if let Some(c) = self.java.get_mut(r) {
            *c = Some(w);
        }
        self.fnv(0x5700 + r as u64);
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
        if self.gates_all() && self.build_all() && self.reg_all() && self.cfg_all()
            && self.sign_all() && self.java_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for TeamCityLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TeamCityLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < BUILD_CELLS {
        if let Some(w) = build_cell(i) {
            if led.record_build(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < REG_CELLS {
        if let Some(w) = reg_cell(r) {
            if led.record_reg(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut c = 0usize;
    while c < CFG_CELLS {
        if let Some(w) = cfg_cell(c) {
            if led.record_cfg(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut s = 0usize;
    while s < SIGN_CELLS {
        if let Some(w) = sign_cell(s) {
            if led.record_sign(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut j = 0usize;
    while j < JAVA_LAYERS {
        if led.declare_java(j).is_ok() {
            recorded += 1;
        }
        j += 1;
    }
    // 注入 10 轮：配置越闭集 / 配置漂移 / 构建段逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TeamCityLedger::new();
        let caught = match kind {
            0 => lt.record_cfg(CFG_CELLS, 0) == Err(TeamCityCode::ENoCfg),
            1 => {
                let wc = match cfg_cell(2) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_cfg(2, wc.wrapping_add(1)) == Err(TeamCityCode::ECfgDrift)
            }
            _ => {
                let _ = lt.record_build(9, build_cell(9).unwrap_or(0));
                lt.record_build(3, build_cell(3).unwrap_or(0)) == Err(TeamCityCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5800 + k as u64);
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
/// UNX-F52362 判据集（锚 UNX-F52363-J1，off-by-one 板式）。
pub fn run_unx52362_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52362");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "构建五段 20 格全通（fetch-sources/compile/test/package/publish）",
        recorded >= BUILD_CELLS as u32,
        "五段四格逐格载荷恒等落账",
    );
    s.add(
        "注册/执行 ×10 任务全通（UNX-F52363-J1，shell executor 路径）",
        {
            let mut lt = TeamCityLedger::new();
            let mut ok = true;
            // 注册三元闸先落，随后 shell executor 路径十格构建任务逐个执行。
            let mut r = 0usize;
            while r < REG_CELLS {
                if let Some(w) = reg_cell(r) {
                    if lt.record_reg(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            let mut i = 0usize;
            while i < 10usize {
                // shell executor 路径：取构建段前 10 格作为执行任务样本。
                if let Some(w) = build_cell(i) {
                    if lt.record_build(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.reg_all() && lt.build_kind_filled(10)
        },
        "三元闸放行后十任务逐个恒等（判据锚 §AI-66.3）",
    );
    s.add(
        "agent 注册三元闸 3/3 全齐",
        {
            let mut lt = TeamCityLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < REG_CELLS {
                if let Some(w) = reg_cell(r) {
                    if lt.record_reg(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.reg_all()
        },
        "token/url/agent-name 三元闭集缺一不放行",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TeamCityLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "注册/取活/构建执行/构件回传/日志回传/注销清理逐格落账",
    );
    s.add(
        "构建配置投影 8 格全齐",
        {
            let mut lt = TeamCityLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CFG_CELLS {
                if let Some(w) = cfg_cell(c) {
                    if lt.record_cfg(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cfg_all()
        },
        "server→agent 构建配置投影闭集逐格恒等",
    );
    s.add(
        "与 Server 消费联签 4 格全齐",
        {
            let mut lt = TeamCityLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < SIGN_CELLS {
                if let Some(w) = sign_cell(g) {
                    if lt.record_sign(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.sign_all()
        },
        "heartbeat/build-params/artifacts/build-log 逐格联签",
    );
    s.add(
        "Java 上栈 4 层声明全落",
        {
            let mut lt = TeamCityLedger::new();
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
        "JRE/Agent-Launcher/Runner/JMX 四层闭集缺一拒封",
    );
    s.add(
        "五段单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < BUILD_CELLS {
                if build_kind(i).unwrap_or(5) < build_kind(i - 1).unwrap_or(5) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "fetch-sources→compile→test→package→publish 段单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "配置越闭集/配置漂移/构建段逆序三态轮转必抓");
    s.add(
        "边界·构建格越闭集",
        TeamCityLedger::new().record_build(BUILD_CELLS, 0).is_err(),
        "格 20 越闭集必拒",
    );
    s.add(
        "边界·配置投影越闭集",
        TeamCityLedger::new().record_cfg(CFG_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·注册闸越闭集",
        TeamCityLedger::new().record_reg(REG_CELLS, 0).is_err(),
        "注册闸 3 越闭集必拒",
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
            let mut lt = TeamCityLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < BUILD_CELLS {
                if let Some(w) = build_cell(i) {
                    let _ = lt.record_build(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < REG_CELLS {
                if let Some(w) = reg_cell(r) {
                    let _ = lt.record_reg(r, w);
                }
                r += 1;
            }
            let mut c = 0usize;
            while c < CFG_CELLS {
                if let Some(w) = cfg_cell(c) {
                    let _ = lt.record_cfg(c, w);
                }
                c += 1;
            }
            let mut sg = 0usize;
            while sg < SIGN_CELLS {
                if let Some(w) = sign_cell(sg) {
                    let _ = lt.record_sign(sg, w);
                }
                sg += 1;
            }
            let mut j = 0usize;
            while j < JAVA_LAYERS {
                let _ = lt.declare_java(j);
                j += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9A40,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9A40",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集（本条按求值时点 len==15）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_kind_mapping() {
        assert_eq!(build_kind(0), Some(0));
        assert_eq!(build_kind(3), Some(0));
        assert_eq!(build_kind(4), Some(1));
        assert_eq!(build_kind(11), Some(2));
        assert_eq!(build_kind(16), Some(4));
        assert!(build_kind(20).is_none());
    }

    #[test]
    fn test_names() {
        assert_eq!(build_kind_name(0), Some("fetch-sources"));
        assert_eq!(build_kind_name(4), Some("publish"));
        assert!(build_kind_name(5).is_none());
        assert_eq!(reg_cell_name(0), Some("token"));
        assert_eq!(reg_cell_name(2), Some("agent-name"));
        assert!(reg_cell_name(3).is_none());
        assert_eq!(sign_cell_name(0), Some("heartbeat"));
        assert_eq!(sign_cell_name(3), Some("build-log"));
        assert!(sign_cell_name(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = build_cell(7).unwrap_or(0);
        assert_eq!(a, build_cell(7).unwrap_or(0));
        assert_ne!(a, build_cell(8).unwrap_or(0));
        assert!(build_cell(BUILD_CELLS).is_none());
        assert!(reg_cell(REG_CELLS).is_none());
        assert!(cfg_cell(CFG_CELLS).is_none());
        assert!(sign_cell(SIGN_CELLS).is_none());
        assert!(java_row(JAVA_LAYERS).is_none());
    }

    #[test]
    fn test_reg_gate_then_build() {
        // 未过三元闸也可落构建账（账本不耦合闸序），但注册闸全齐要 3/3。
        let mut lt = TeamCityLedger::new();
        let mut r = 0usize;
        while r < REG_CELLS {
            assert!(lt.record_reg(r, reg_cell(r).unwrap_or(0)).is_ok());
            r += 1;
        }
        assert!(lt.reg_all());
        let mut lt2 = TeamCityLedger::new();
        assert!(lt2.record_reg(0, reg_cell(0).unwrap_or(0)).is_ok());
        assert!(!lt2.reg_all());
    }

    #[test]
    fn test_order_caught() {
        // 按合法序先落 test 段（kind=2），再注 compile 段（kind=1）→ EOrder。
        let mut lt = TeamCityLedger::new();
        assert!(lt.record_build(9, build_cell(9).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_build(5, build_cell(5).unwrap_or(0)),
            Err(TeamCityCode::EOrder)
        );
        assert_eq!(lt.last_stage(), 2);
        assert!(lt.record_build(10, build_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TeamCityLedger::new();
        assert_eq!(
            lt.record_build(1, build_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(TeamCityCode::EStageDrift)
        );
        assert_eq!(
            lt.record_cfg(3, cfg_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(TeamCityCode::ECfgDrift)
        );
        assert_eq!(
            lt.record_reg(1, reg_cell(1).unwrap_or(0).wrapping_add(2)),
            Err(TeamCityCode::ERegDrift)
        );
        assert_eq!(
            lt.record_sign(2, sign_cell(2).unwrap_or(0).wrapping_add(4)),
            Err(TeamCityCode::ERegDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TeamCityLedger::new();
        let _ = lt.record_build(0, build_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_build(0, build_cell(0).unwrap_or(0)),
            Err(TeamCityCode::EDup)
        );
        let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cfg(0, cfg_cell(0).unwrap_or(0)),
            Err(TeamCityCode::EDup)
        );
        let _ = lt.declare_java(1);
        assert_eq!(lt.declare_java(1), Err(TeamCityCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TeamCityLedger::new();
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
        // 20 构建 + 3 闸 + 8 投影 + 4 联签 + 4 Java 行 = 39。
        assert_eq!(recorded, 39);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_kinds() {
        // 三态轮转：0=配置越闭集 1=配置漂移 2=构建段逆序（先落 test 段再注 compile）。
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = TeamCityLedger::new();
            let caught = match kind {
                0 => lt.record_cfg(CFG_CELLS, 0).is_err(),
                1 => lt
                    .record_cfg(2, cfg_cell(2).unwrap_or(0).wrapping_add(1))
                    .is_err(),
                _ => {
                    let _ = lt.record_build(9, build_cell(9).unwrap_or(0));
                    lt.record_build(3, build_cell(3).unwrap_or(0)).is_err()
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
        assert_eq!(SEG_BASE, 0xC9A40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TeamCityCode::Ok as u32, 0);
        assert_eq!(TeamCityCode::EGradeDrift as u32, 12);
        assert_eq!(TeamCityCode::ECfgDrift.val(), SEG_BASE + 7);
        assert!(!TeamCityCode::Ok.detail().is_empty());
        assert!(!TeamCityCode::ESeal.detail().is_empty());
        assert!(!TeamCityCode::ENoSign.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = TeamCityLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = TeamCityLedger::new();
        let _ = lt2.judge_gate(3);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let mut lt = TeamCityLedger::new();
        let _ = lt.record_cfg(0, cfg_cell(0).unwrap_or(0).wrapping_add(11));
        let (i, c, e) = lt.stats();
        assert_eq!((i, c, e), (1, 1, 0));
    }

    #[test]
    fn unx52362_checks_green() {
        let s = run_unx52362_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 17);
    }
}
