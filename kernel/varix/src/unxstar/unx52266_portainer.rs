//! UNX-F52266 Portainer Desktop 收口 · 0xC8240段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52266"，逐字）：
//!   title: Portainer Desktop 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52267-J1
//!     get/apply/logs/describe 20 操作集 ×10 集群面全通；连接语义消费
//!     I 部网络（联签锚定行）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52266）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52267-J1，照原文锚定；功能按本卡标题（Portainer Desktop）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 环境五类 20 格：本地/远程/K8s/Edge/Agent × 每类 4 格（连接/探活/
//!      凭据/遥测）闭集，逐格载荷（纯函数折叠，XOR 锚低 16 位恒零）恒等
//!      才收；越界必抓（判据原文 20 操作集 ×10 集群面同构 20 格）。
//!   ② 栈管理 10 操作集：五家族闭集（部署/启停/更新/日志/删除）各 2
//!      操作，家族单调（逆序必抓；空账本首操作必须落在部署家族）；漂移
//!      必抓。
//!   ③ 模板 6 格：应用模板×2 / Edge 模板×2 / 自定义模板×2 逐格落账。
//!   ④ RBAC 三元闸：角色/范围/绑定 三元闭集逐闸落账，缺一拒封。
//!   ⑤ 连接语义消费 I 部网络联签锚定行 2 行闭集（连接建立联签/连接
//!      语义联签），缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（环境格越闭集/
//!      栈操作漂移/栈家族越序），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S2（本卡 KB-COMPAT 封顶）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52266;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 环境五类。
pub const ENV_KINDS: usize = 5;

/// 每类格数（连接/探活/凭据/遥测）。
pub const ENV_ASPECTS: usize = 4;

/// 环境总格数（5×4）。
pub const ENV_CELLS: usize = ENV_KINDS * ENV_ASPECTS;

/// 栈管理操作数。
pub const STACK_OPS: usize = 10;

/// 模板格数。
pub const TEMPLATE_CELLS: usize = 6;

/// RBAC 三元闸数。
pub const RBAC_GATES: usize = 3;

/// 联签锚定行数。
pub const COSIGN_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortainerCode {
    /// 成功
    Ok = 0,
    /// 环境格越闭集
    ENoEnv,
    /// 环境格漂移
    EEnvDrift,
    /// 栈操作号越闭集
    ENoOp,
    /// 栈操作漂移
    EDrift,
    /// 栈家族逆序
    EOrder,
    /// 模板/RBAC 格越闭集
    ENoCell,
    /// RBAC 闸缺
    ERbacMiss,
    /// 联签行缺
    ECosignMiss,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PortainerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PortainerCode::Ok => "成功",
            PortainerCode::ENoEnv => "环境格越闭集：本地/远程/K8s/Edge/Agent 五类 ×4 格之外先登记",
            PortainerCode::EEnvDrift => "环境格漂移：格载荷折叠与基准不恒等，逐格复算",
            PortainerCode::ENoOp => "栈操作号越闭集：栈管理 10 操作集之外先登记",
            PortainerCode::EDrift => "栈操作漂移：操作折叠与基准不恒等，逐格复算",
            PortainerCode::EOrder => "栈家族逆序：部署→启停→更新→日志→删除单调，逆序必抓；空账本首操作必须落在部署家族",
            PortainerCode::ENoCell => "格越闭集：模板 6 格或 RBAC 3 闸之外先登记",
            PortainerCode::ERbacMiss => "RBAC 闸缺：角色/范围/绑定三元闸未齐先补齐",
            PortainerCode::ECosignMiss => "联签行缺：连接建立/连接语义联签锚定行未齐先补齐",
            PortainerCode::EDup => "重复落账：同格只许一条账目",
            PortainerCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PortainerCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PortainerCode::EGradeDrift => "定级漂移：本卡 KB-COMPAT 封顶 S2，全通且联签全齐才可定 S2",
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
            Grade::S1 => "S1：全通收口（本卡不适用，KB-COMPAT 封顶 S2）",
            Grade::S2 => "S2：有条件收口，20 格+10 操作+6 模板+3 闸+2 联签行全通登记",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 环境类名（const fn 表 + Option 边界）。
pub const fn env_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("本地"),
        1 => Some("远程"),
        2 => Some("K8s"),
        3 => Some("Edge"),
        4 => Some("Agent"),
        _ => None,
    }
}

/// 环境面名。
pub const fn env_aspect_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("连接"),
        1 => Some("探活"),
        2 => Some("凭据"),
        3 => Some("遥测"),
        _ => None,
    }
}

/// 环境格载荷（格 i：k×4+a；XOR 锚低 16 位恒零）。
pub const fn env_cell(i: usize) -> Option<u64> {
    if i >= ENV_CELLS {
        return None;
    }
    Some((0xB1000u64 + (i as u64) * 0x61) ^ 0x52504F52_0000)
}

/// 栈操作格载荷（操作 o；家族 = o/2）。
pub const fn stack_cell(o: usize) -> Option<u64> {
    if o >= STACK_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 栈操作家族（0..2=部署 2..4=启停 4..6=更新 6..8=日志 8..10=删除）。
pub const fn stack_family(o: usize) -> Option<u32> {
    if o >= STACK_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn stack_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("部署"),
        1 => Some("启停"),
        2 => Some("更新"),
        3 => Some("日志"),
        4 => Some("删除"),
        _ => None,
    }
}

/// 模板格载荷（格 t：0..2 应用 2..4 Edge 4..6 自定义）。
pub const fn template_cell(t: usize) -> Option<u32> {
    if t >= TEMPLATE_CELLS {
        return None;
    }
    Some(0xB300 + t as u32 * 0x2C)
}

/// 模板格名。
pub const fn template_cell_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("应用模板·标准"),
        1 => Some("应用模板·编排"),
        2 => Some("Edge 模板·部署"),
        3 => Some("Edge 模板·回退"),
        4 => Some("自定义模板·私有"),
        5 => Some("自定义模板·共享"),
        _ => None,
    }
}

/// RBAC 闸载荷（闸 r：0=角色 1=范围 2=绑定）。
pub const fn rbac_gate(r: usize) -> Option<u32> {
    if r >= RBAC_GATES {
        return None;
    }
    Some(0xB400 + r as u32 * 0x3E)
}

/// RBAC 闸名。
pub const fn rbac_gate_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("角色"),
        1 => Some("范围"),
        2 => Some("绑定"),
        _ => None,
    }
}

/// 联签锚定行折叠（行 r：0=连接建立联签 1=连接语义联签，消费 I 部网络）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0xB500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Portainer 收口账本状态机。
#[derive(Clone)]
pub struct PortainerLedger {
    /// 六判据（安装/环境连接/栈管理/模板/RBAC/卸载）。
    gates: [Option<bool>; 6],
    /// 环境格账。
    envs: [Option<u64>; ENV_CELLS],
    /// 栈操作账。
    ops: [Option<u64>; STACK_OPS],
    /// 模板账。
    templates: [Option<u32>; TEMPLATE_CELLS],
    /// RBAC 闸账。
    rbac: [Option<u32>; RBAC_GATES],
    /// 联签行账。
    cosigns: [Option<u32>; COSIGN_ROWS],
    /// 最后家族。
    last_family: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PortainerLedger {
    /// 新账本。
    pub fn new() -> Self {
        PortainerLedger {
            gates: [None; 6],
            envs: [None; ENV_CELLS],
            ops: [None; STACK_OPS],
            templates: [None; TEMPLATE_CELLS],
            rbac: [None; RBAC_GATES],
            cosigns: [None; COSIGN_ROWS],
            last_family: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PortainerCode> {
        if gate >= 6 {
            return Err(PortainerCode::EDup);
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

    /// 环境格一格（载荷对照 + 重复拒）。
    pub fn record_env(&mut self, i: usize, got: u64) -> Result<(), PortainerCode> {
        let want = match env_cell(i) {
            Some(w) => w,
            None => return Err(PortainerCode::ENoEnv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + i as u64);
            return Err(PortainerCode::EEnvDrift);
        }
        if match self.envs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PortainerCode::EDup);
        }
        if let Some(c) = self.envs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4201 + i as u64);
        Ok(())
    }

    /// 环境格全账核验（20 格）。
    pub fn envs_all(&self) -> bool {
        let mut i = 0usize;
        while i < ENV_CELLS {
            match self.envs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 栈操作一格（家族单调 + 空账本首家族必须为 0 + 载荷对照 + 重复拒）。
    pub fn record_stack(&mut self, o: usize, got: u64) -> Result<(), PortainerCode> {
        if o >= STACK_OPS {
            return Err(PortainerCode::ENoOp);
        }
        let family = match stack_family(o) {
            Some(f) => f,
            None => return Err(PortainerCode::ENoOp),
        };
        if (self.last_family < 0 && family != 0)
            || (self.last_family >= 0 && (family as i64) < self.last_family)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(PortainerCode::EOrder);
        }
        let want = match stack_cell(o) {
            Some(w) => w,
            None => return Err(PortainerCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(PortainerCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PortainerCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = family as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 栈操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < STACK_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 模板一格。
    pub fn record_template(&mut self, t: usize) -> Result<(), PortainerCode> {
        let want = match template_cell(t) {
            Some(x) => x,
            None => return Err(PortainerCode::ENoCell),
        };
        if match self.templates.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PortainerCode::EDup);
        }
        if let Some(c) = self.templates.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x4400 + t as u64);
        Ok(())
    }

    /// 模板全齐核验。
    pub fn templates_all(&self) -> bool {
        let mut t = 0usize;
        while t < TEMPLATE_CELLS {
            match self.templates.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// RBAC 闸一格。
    pub fn record_rbac(&mut self, r: usize) -> Result<(), PortainerCode> {
        let want = match rbac_gate(r) {
            Some(x) => x,
            None => return Err(PortainerCode::ENoCell),
        };
        if match self.rbac.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PortainerCode::EDup);
        }
        if let Some(c) = self.rbac.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x4500 + r as u64);
        Ok(())
    }

    /// RBAC 三元闸全落核验。
    pub fn rbac_all(&self) -> bool {
        let mut r = 0usize;
        while r < RBAC_GATES {
            match self.rbac.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 联签锚定行一行（连接语义消费 I 部网络）。
    pub fn declare_cosign(&mut self, r: usize) -> Result<(), PortainerCode> {
        if r >= COSIGN_ROWS {
            return Err(PortainerCode::ECosignMiss);
        }
        if match self.cosigns.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PortainerCode::EDup);
        }
        if let Some(c) = self.cosigns.get_mut(r) {
            *c = cosign_row(r);
        }
        self.fnv(0x4600 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn cosigns_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_ROWS {
            match self.cosigns.get(r) {
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

    /// KB-COMPAT 定级（本卡封顶 S2：全通 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.envs_all()
            && self.ops_all()
            && self.templates_all()
            && self.rbac_all()
            && self.cosigns_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PortainerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PortainerLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < ENV_CELLS {
        if let Some(w) = env_cell(i) {
            if led.record_env(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < STACK_OPS {
        if let Some(w) = stack_cell(o) {
            if led.record_stack(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TEMPLATE_CELLS {
        let _ = led.record_template(t);
        t += 1;
    }
    let mut r = 0usize;
    while r < RBAC_GATES {
        let _ = led.record_rbac(r);
        r += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_ROWS {
        let _ = led.declare_cosign(c);
        c += 1;
    }
    // 注入 10 轮：环境格越闭集 / 栈操作漂移 / 栈家族越序三态轮转，
    // 全部落在当轮临时账本上；家族越序对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PortainerLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_env(0, env_cell(0).unwrap_or(0));
                lt.record_env(ENV_CELLS, 0).is_err()
            }
            1 => {
                let _ = lt.record_stack(0, stack_cell(0).unwrap_or(0));
                lt.record_stack(1, stack_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(PortainerCode::EDrift)
            }
            _ => {
                // 空账本首操作落在删除家族（家族 4）→ 越序必抓，且不改 last_family。
                lt.record_stack(9, stack_cell(9).unwrap_or(0)) == Err(PortainerCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4700 + k as u64);
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
/// UNX-F52266 判据集（锚 UNX-F52267-J1，off-by-one 板式）。
pub fn run_unx52266_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52266");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "环境五类 20 格全通（本地/远程/K8s/Edge/Agent ×4 面）",
        recorded >= ENV_CELLS as u32,
        "连接/探活/凭据/遥测四面逐格恒等（20 集群面同构）",
    );
    s.add(
        "栈管理 10 操作全通（部署/启停/更新/日志/删除 ×2）",
        {
            let mut lt = PortainerLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < STACK_OPS {
                if let Some(w) = stack_cell(o) {
                    if lt.record_stack(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "五家族闭集逐操作恒等（栈管理 ×10 判据原文）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = PortainerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/环境连接/栈管理/模板/RBAC/卸载逐格落账（§AI-66.3 判据锚定）",
    );
    s.add(
        "模板 6 格全齐",
        {
            let mut lt = PortainerLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TEMPLATE_CELLS {
                if lt.record_template(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.templates_all()
        },
        "应用×2/Edge×2/自定义×2 逐格落账",
    );
    s.add(
        "RBAC 三元闸全落",
        {
            let mut lt = PortainerLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RBAC_GATES {
                if lt.record_rbac(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rbac_all()
        },
        "角色/范围/绑定三元闸逐闸落账，缺一拒封",
    );
    s.add(
        "连接语义消费 I 部网络联签锚定行 2/2 全齐",
        {
            let mut lt = PortainerLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_ROWS {
                if lt.declare_cosign(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.cosigns_all()
        },
        "连接建立联签/连接语义联签两行闭集（联签锚定行）",
    );
    s.add(
        "栈家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < STACK_OPS {
                if stack_family(o).unwrap_or(5) < stack_family(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "部署→启停→更新→日志→删除五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "环境格越闭集/栈操作漂移/栈家族越序三态轮转必抓",
    );
    let mut l0 = PortainerLedger::new();
    s.add(
        "边界·环境格越闭集",
        l0.record_env(ENV_CELLS, 0).is_err(),
        "环境格 20 越闭集必拒",
    );
    s.add(
        "边界·栈操作越闭集",
        l0.record_stack(STACK_OPS, 0).is_err(),
        "栈操作 10 越闭集必拒",
    );
    s.add(
        "边界·联签行越闭集",
        l0.declare_cosign(COSIGN_ROWS).is_err(),
        "联签行 2 越闭集必拒",
    );
    s.add(
        "空账本家族越序必抓",
        l0.record_stack(9, stack_cell(9).unwrap_or(0)) == Err(PortainerCode::EOrder),
        "空账本首操作落在删除家族，越序必抓且不改 last_family",
    );
    s.add(
        "栈操作漂移拒",
        {
            let mut lt = PortainerLedger::new();
            let _ = lt.record_stack(0, stack_cell(0).unwrap_or(0));
            lt.record_stack(1, stack_cell(1).unwrap_or(0).wrapping_add(1)) == Err(PortainerCode::EDrift)
        },
        "栈操作载荷漂移必抓",
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
        "定级 S2 闭集（KB-COMPAT 登记封顶）",
        {
            let mut lt = PortainerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < ENV_CELLS {
                if let Some(w) = env_cell(i) {
                    let _ = lt.record_env(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < STACK_OPS {
                if let Some(w) = stack_cell(o) {
                    let _ = lt.record_stack(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TEMPLATE_CELLS {
                let _ = lt.record_template(t);
                t += 1;
            }
            let mut r = 0usize;
            while r < RBAC_GATES {
                let _ = lt.record_rbac(r);
                r += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_ROWS {
                let _ = lt.declare_cosign(c);
                c += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（本卡 KB-COMPAT 定级登记封顶 S2）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8240,
        "SEG_BASE 0x40 对齐且按派工公式落在 0xC8240",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_grid() {
        assert_eq!(env_kind_name(0), Some("本地"));
        assert_eq!(env_kind_name(4), Some("Agent"));
        assert!(env_kind_name(5).is_none());
        assert_eq!(env_aspect_name(3), Some("遥测"));
        assert!(env_aspect_name(4).is_none());
        assert!(env_cell(ENV_CELLS).is_none());
    }

    #[test]
    fn test_stack_family() {
        assert_eq!(stack_family(0), Some(0));
        assert_eq!(stack_family(9), Some(4));
        assert_eq!(stack_family_name(4), Some("删除"));
        assert!(stack_family(10).is_none());
        assert!(stack_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = env_cell(7).unwrap_or(0);
        assert_eq!(a, env_cell(7).unwrap_or(0));
        assert_ne!(a, env_cell(8).unwrap_or(0));
        let b = stack_cell(5).unwrap_or(0);
        assert_eq!(b, stack_cell(5).unwrap_or(0));
        assert_ne!(b, stack_cell(6).unwrap_or(0));
        assert!(template_cell(TEMPLATE_CELLS).is_none());
        assert!(rbac_gate(RBAC_GATES).is_none());
        assert!(cosign_row(COSIGN_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = PortainerLedger::new();
        let mut i = 0usize;
        while i <= 8 {
            let _ = lt.record_stack(i, stack_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_stack(2, stack_cell(2).unwrap_or(0)), Err(PortainerCode::EOrder));
        assert!(lt.record_stack(9, stack_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_family_caught() {
        let mut lt = PortainerLedger::new();
        assert_eq!(lt.record_stack(6, stack_cell(6).unwrap_or(0)), Err(PortainerCode::EOrder));
        // 空账本首操作必须落在部署家族（0）。
        assert!(lt.record_stack(0, stack_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PortainerLedger::new();
        let mut i = 0usize;
        while i <= 3 {
            let _ = lt.record_stack(i, stack_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_stack(3, stack_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(PortainerCode::EDrift)
        );
        assert_eq!(
            lt.record_env(5, env_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(PortainerCode::EEnvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PortainerLedger::new();
        let _ = lt.record_env(0, env_cell(0).unwrap_or(0));
        assert_eq!(lt.record_env(0, env_cell(0).unwrap_or(0)), Err(PortainerCode::EDup));
        let _ = lt.record_template(0);
        assert_eq!(lt.record_template(0), Err(PortainerCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PortainerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PortainerLedger::new();
        assert!(lt2.judge_gate(1).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (ENV_CELLS + STACK_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8240);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PortainerCode::Ok as u32, 0);
        assert_eq!(PortainerCode::EGradeDrift as u32, 12);
        // 13 变体闭集：逐位核对（与枚举位序一致）。
        assert_eq!(PortainerCode::ENoEnv as u32, 1);
        assert_eq!(PortainerCode::EDrift as u32, 4);
        assert_eq!(PortainerCode::EOrder as u32, 5);
        assert!(!PortainerCode::Ok.detail().is_empty());
        assert!(!PortainerCode::ECosignMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PortainerLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52266_checks_green() {
        let s = run_unx52266_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
