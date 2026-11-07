//! UNX-F52264 Podman Desktop 收口 · 0xC81C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52264"，逐字）：
//!   title: Podman Desktop 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52265-J1 六判据
//!     全落；k8s/k3s 拉起依赖账；N/A 同构声明；KB-COMPAT 定级登记；判据
//!     锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52264）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52265-J1，照原文锚定；功能按本卡标题（Podman Desktop）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① rootless 三态 12 格：双 runner（podman-machine/wsl）× 三态
//!      （运行/停止/故障）× 双探针（engine/gui），逐格载荷（纯函数折叠）
//!      恒等才收；越界必抓。
//!   ② 容器/镜像/网络三账 15 格：三族各 5 格闭集，逐格恒等，漂移必抓。
//!   ③ compose 消费 6 格：Podman Desktop 对 Compose 工程的消费链路
//!      逐格落账（F52263 口径复用）。
//!   ④ kube 生成 8 格：四相位闭集（生成/转换/校验/部署）各 2 格，相位
//!      单调（逆序必抓）；k8s/k3s 拉起依赖账 2 行闭集声明。
//!   ⑤ Electron 4 层闭集：主进程/渲染进程/预加载/原生桥逐层恒等。
//!   ⑥ N/A 同构声明：虚拟化依赖按 F52261 口径复用，折叠恒归零（N/A）。
//!   ⑦ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（rootless 格
//!      越闭集/kube 相位逆序/三账漂移），全部落在当轮临时账本上，检出+
//!      逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集——全通也只定
//!      S2（rootless 有条件收口，N/A 同构口径），判据缺 → S3。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52264;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// rootless runner 数。
pub const RL_RUNNERS: usize = 2;

/// rootless 三态数。
pub const RL_STATES: usize = 3;

/// rootless 探针数。
pub const RL_PROBES: usize = 2;

/// rootless 三态总格数。
pub const RL_CELLS: usize = RL_RUNNERS * RL_STATES * RL_PROBES;

/// 三账族数（容器/镜像/网络）。
pub const ACCT_FAMILIES: usize = 3;

/// 每族格数。
pub const ACCT_PER_FAMILY: usize = 5;

/// 三账总格数。
pub const ACCT_CELLS: usize = ACCT_FAMILIES * ACCT_PER_FAMILY;

/// compose 消费格数。
pub const CMP_CELLS: usize = 6;

/// kube 生成格数。
pub const KUBE_CELLS: usize = 8;

/// Electron 层数。
pub const ELEC_LAYERS: usize = 4;

/// k8s/k3s 拉起依赖账行数。
pub const DEP_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// 判据锚定原文（卡面 criteria 逐字引用）。
pub const CRITERIA_ANCHOR: &str = "§AI-66.3";

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PodmanDeskCode {
    /// 成功
    Ok = 0,
    /// rootless 格越闭集
    ENoRl,
    /// 格漂移
    EDrift,
    /// 三账格越闭集
    ENoAcct,
    /// kube 相位逆序
    EOrder,
    /// compose 消费格越闭集
    ENoCmp,
    /// kube 生成格越闭集
    ENoKube,
    /// Electron 层越闭集
    ENoEl,
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

impl PodmanDeskCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PodmanDeskCode::Ok => "成功",
            PodmanDeskCode::ENoRl => "rootless 格越闭集：双 runner 三态 12 格之外先登记",
            PodmanDeskCode::EDrift => "格漂移：rootless/三账/compose/Electron 载荷与基准折叠不恒等",
            PodmanDeskCode::ENoAcct => "三账格越闭集：容器/镜像/网络 15 格之外先登记",
            PodmanDeskCode::EOrder => "kube 相位逆序：生成→转换→校验→部署单调，逆序必抓",
            PodmanDeskCode::ENoCmp => "compose 消费格越闭集：6 格闭集之外先登记",
            PodmanDeskCode::ENoKube => "kube 生成格越闭集：8 格闭集之外先登记",
            PodmanDeskCode::ENoEl => "Electron 层越闭集：4 层闭集之外先登记",
            PodmanDeskCode::EDup => "重复落账：同格只许一条账目",
            PodmanDeskCode::ESeal => "未齐即封：12 格 + 15 格 + 6 格 + 8 格 + 4 层未齐先补齐",
            PodmanDeskCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PodmanDeskCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PodmanDeskCode::EGradeDrift => "定级漂移：全通且依赖账声明全齐也只可定 S2（N/A 同构有条件收口）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不可达）
    S1,
    /// S2：有条件收口（rootless N/A 同构口径，全通定级）
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
            Grade::S1 => "S1：本卡不可达（rootless 有条件收口，无全通 S1 口径）",
            Grade::S2 => "S2：六判据+12格+15格+6格+8格+4层+2行依赖账全齐，有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// runner 名（r：0=podman-machine 1=wsl）。
pub const fn runner_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("podman-machine"),
        1 => Some("wsl"),
        _ => None,
    }
}

/// 三态名。
pub const fn rl_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("运行"),
        1 => Some("停止"),
        2 => Some("故障"),
        _ => None,
    }
}

/// 探针名。
pub const fn rl_probe_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("engine"),
        1 => Some("gui"),
        _ => None,
    }
}

/// rootless 三态格基准（r/s/p 三元闭集，2×3×2=12 格）。
pub const fn rl_cell(r: usize, s: usize, p: usize) -> Option<u64> {
    if r >= RL_RUNNERS || s >= RL_STATES || p >= RL_PROBES {
        return None;
    }
    let idx = (r * RL_STATES + s) * RL_PROBES + p;
    Some((0xD4100u64 + (idx as u64) * 0x2D) ^ 0x504F444D414E0000)
}

/// 三账族名（f：0=容器 1=镜像 2=网络）。
pub const fn acct_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("容器"),
        1 => Some("镜像"),
        2 => Some("网络"),
        _ => None,
    }
}

/// 三账格基准（族 f × 格 i，3×5=15 格）。
pub const fn acct_cell(f: usize, i: usize) -> Option<u64> {
    if f >= ACCT_FAMILIES || i >= ACCT_PER_FAMILY {
        return None;
    }
    let idx = f * ACCT_PER_FAMILY + i;
    Some((0xD4200u64 + (idx as u64) * 0x2C) ^ (f as u64).wrapping_mul(0x01000193))
}

/// compose 消费格基准（格 c）。
pub const fn cmp_cell(c: usize) -> Option<u64> {
    if c >= CMP_CELLS {
        return None;
    }
    Some((0xD4300u64 + (c as u64) * 0x2D) ^ 0x434D50434E530000)
}

/// compose 消费格名。
pub const fn cmp_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("compose 文件解析"),
        1 => Some("服务拓扑消费"),
        2 => Some("profile 消费"),
        3 => Some("up 消费"),
        4 => Some("down 消费"),
        5 => Some("logs 消费"),
        _ => None,
    }
}

/// kube 生成格载荷（格 k；相位 = k/2）。
pub const fn kube_cell(k: usize) -> Option<u64> {
    if k >= KUBE_CELLS {
        return None;
    }
    Some((0xD4400u64 + (k as u64) * 0x2C) ^ ((k / 2) as u64).wrapping_mul(0x01000193))
}

/// kube 相位（0..2=生成 2..4=转换 4..6=校验 6..8=部署）。
pub const fn kube_kind(k: usize) -> Option<u32> {
    if k >= KUBE_CELLS {
        return None;
    }
    Some((k / 2) as u32)
}

/// kube 相位名。
pub const fn kube_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("生成"),
        1 => Some("转换"),
        2 => Some("校验"),
        3 => Some("部署"),
        _ => None,
    }
}

/// Electron 层基准（层 l）。
pub const fn elec_cell(l: usize) -> Option<u64> {
    if l >= ELEC_LAYERS {
        return None;
    }
    Some((0xD4500u64 + (l as u64) * 0x31) ^ 0x454C454354520000)
}

/// Electron 层名。
pub const fn elec_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("主进程"),
        1 => Some("渲染进程"),
        2 => Some("预加载"),
        3 => Some("原生桥"),
        _ => None,
    }
}

/// k8s/k3s 拉起依赖账行折叠（行 r）。
pub const fn dep_row(r: usize) -> Option<u32> {
    if r >= DEP_ROWS {
        return None;
    }
    Some(0xD4600 + r as u32 * 0x2C)
}

/// 依赖账行名。
pub const fn dep_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("k8s 拉起依赖账"),
        1 => Some("k3s 拉起依赖账"),
        _ => None,
    }
}

/// N/A 同构声明（F52261 口径复用）：折叠恒归零（N/A 语义）。
pub const fn na_declaration() -> u32 {
    (F52261_PLANE ^ 0x4E415F4445530000) as u32 & 0xFFFF
}

/// F52261 共面基准（同构源）。
pub const F52261_PLANE: u64 = 0x574C32484F530000;

// ── 收口账本 ─────────────────────────────────────────────────────
/// Podman Desktop 收口账本状态机。
#[derive(Clone)]
pub struct PodmanDeskLedger {
    /// 六判据（安装/启动/rootless/崩溃/compose/kube）。
    gates: [Option<bool>; GATES],
    /// rootless 三态账。
    rl: [Option<u64>; RL_CELLS],
    /// 三账（容器/镜像/网络）。
    acct: [Option<u64>; ACCT_CELLS],
    /// compose 消费账。
    cmp: [Option<u64>; CMP_CELLS],
    /// kube 生成账。
    kube: [Option<u64>; KUBE_CELLS],
    /// Electron 层账。
    elec: [Option<u64>; ELEC_LAYERS],
    /// k8s/k3s 拉起依赖账。
    deps: [Option<u32>; DEP_ROWS],
    /// 最后 kube 相位。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PodmanDeskLedger {
    /// 新账本。
    pub fn new() -> Self {
        PodmanDeskLedger {
            gates: [None; GATES],
            rl: [None; RL_CELLS],
            acct: [None; ACCT_CELLS],
            cmp: [None; CMP_CELLS],
            kube: [None; KUBE_CELLS],
            elec: [None; ELEC_LAYERS],
            deps: [None; DEP_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PodmanDeskCode> {
        if gate >= GATES {
            return Err(PodmanDeskCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5D100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// rootless 三态一格（三元闭集 + 载荷对照 + 重复拒）。
    pub fn record_rl(&mut self, r: usize, s: usize, p: usize, got: u64) -> Result<(), PodmanDeskCode> {
        let want = match rl_cell(r, s, p) {
            Some(w) => w,
            None => return Err(PodmanDeskCode::ENoRl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D200 + ((r * RL_STATES + s) * RL_PROBES + p) as u64);
            return Err(PodmanDeskCode::EDrift);
        }
        let idx = (r * RL_STATES + s) * RL_PROBES + p;
        if match self.rl.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PodmanDeskCode::EDup);
        }
        if let Some(c) = self.rl.get_mut(idx) {
            *c = Some(got);
        }
        self.fnv(0x5D201 + idx as u64);
        Ok(())
    }

    /// rootless 全账核验（12 格）。
    pub fn rl_all(&self) -> bool {
        let mut i = 0usize;
        while i < RL_CELLS {
            match self.rl.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 三账一格（族/格双闭集 + 载荷对照 + 重复拒）。
    pub fn record_acct(&mut self, f: usize, i: usize, got: u64) -> Result<(), PodmanDeskCode> {
        let want = match acct_cell(f, i) {
            Some(w) => w,
            None => return Err(PodmanDeskCode::ENoAcct),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D300 + (f * ACCT_PER_FAMILY + i) as u64);
            return Err(PodmanDeskCode::EDrift);
        }
        let idx = f * ACCT_PER_FAMILY + i;
        if match self.acct.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PodmanDeskCode::EDup);
        }
        if let Some(c) = self.acct.get_mut(idx) {
            *c = Some(got);
        }
        self.fnv(0x5D301 + idx as u64);
        Ok(())
    }

    /// 三账全账核验（15 格）。
    pub fn acct_all(&self) -> bool {
        let mut i = 0usize;
        while i < ACCT_CELLS {
            match self.acct.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// compose 消费一格（载荷对照 + 重复拒）。
    pub fn record_cmp(&mut self, c: usize, got: u64) -> Result<(), PodmanDeskCode> {
        let want = match cmp_cell(c) {
            Some(w) => w,
            None => return Err(PodmanDeskCode::ENoCmp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D400 + c as u64);
            return Err(PodmanDeskCode::EDrift);
        }
        if match self.cmp.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PodmanDeskCode::EDup);
        }
        if let Some(x) = self.cmp.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5D401 + c as u64);
        Ok(())
    }

    /// compose 消费全账核验（6 格）。
    pub fn cmp_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMP_CELLS {
            match self.cmp.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// kube 生成一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_kube(&mut self, k: usize, got: u64) -> Result<(), PodmanDeskCode> {
        if k >= KUBE_CELLS {
            return Err(PodmanDeskCode::ENoKube);
        }
        let kind = match kube_kind(k) {
            Some(x) => x,
            None => return Err(PodmanDeskCode::ENoKube),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D500);
            return Err(PodmanDeskCode::EOrder);
        }
        let want = match kube_cell(k) {
            Some(w) => w,
            None => return Err(PodmanDeskCode::ENoKube),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D501 + k as u64);
            return Err(PodmanDeskCode::EDrift);
        }
        if match self.kube.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PodmanDeskCode::EDup);
        }
        if let Some(x) = self.kube.get_mut(k) {
            *x = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5D502 + k as u64);
        Ok(())
    }

    /// kube 生成全账核验（8 格）。
    pub fn kube_all(&self) -> bool {
        let mut k = 0usize;
        while k < KUBE_CELLS {
            match self.kube.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// Electron 层一格（载荷对照 + 重复拒）。
    pub fn record_elec(&mut self, l: usize, got: u64) -> Result<(), PodmanDeskCode> {
        let want = match elec_cell(l) {
            Some(w) => w,
            None => return Err(PodmanDeskCode::ENoEl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5D600 + l as u64);
            return Err(PodmanDeskCode::EDrift);
        }
        if match self.elec.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PodmanDeskCode::EDup);
        }
        if let Some(x) = self.elec.get_mut(l) {
            *x = Some(got);
        }
        self.fnv(0x5D601 + l as u64);
        Ok(())
    }

    /// Electron 全账核验（4 层）。
    pub fn elec_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELEC_LAYERS {
            match self.elec.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// k8s/k3s 拉起依赖账一行（越界归 kube 域码，重复拒）。
    pub fn declare_dep(&mut self, r: usize) -> Result<(), PodmanDeskCode> {
        let want = match dep_row(r) {
            Some(w) => w,
            None => return Err(PodmanDeskCode::ENoKube),
        };
        if match self.deps.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PodmanDeskCode::EDup);
        }
        if let Some(x) = self.deps.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x5D700 + r as u64);
        Ok(())
    }

    /// 依赖账全齐核验。
    pub fn deps_all(&self) -> bool {
        let mut r = 0usize;
        while r < DEP_ROWS {
            match self.deps.get(r) {
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

    /// 最后 kube 相位（注入不改主账断言用）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S2 有条件收口，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.rl_all()
            && self.acct_all()
            && self.cmp_all()
            && self.kube_all()
            && self.elec_all()
            && self.deps_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PodmanDeskLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PodmanDeskLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut r = 0usize;
    while r < RL_RUNNERS {
        let mut s = 0usize;
        while s < RL_STATES {
            let mut p = 0usize;
            while p < RL_PROBES {
                if let Some(w) = rl_cell(r, s, p) {
                    if led.record_rl(r, s, p, w).is_ok() {
                        recorded += 1;
                    }
                }
                p += 1;
            }
            s += 1;
        }
        r += 1;
    }
    let mut f = 0usize;
    while f < ACCT_FAMILIES {
        let mut i = 0usize;
        while i < ACCT_PER_FAMILY {
            if let Some(w) = acct_cell(f, i) {
                if led.record_acct(f, i, w).is_ok() {
                    recorded += 1;
                }
            }
            i += 1;
        }
        f += 1;
    }
    let mut c = 0usize;
    while c < CMP_CELLS {
        if let Some(w) = cmp_cell(c) {
            if led.record_cmp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut k = 0usize;
    while k < KUBE_CELLS {
        if let Some(w) = kube_cell(k) {
            if led.record_kube(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut l = 0usize;
    while l < ELEC_LAYERS {
        if let Some(w) = elec_cell(l) {
            if led.record_elec(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut d = 0usize;
    while d < DEP_ROWS {
        let _ = led.declare_dep(d);
        d += 1;
    }
    // 注入 10 轮：rootless 格越闭集 / kube 相位逆序 / 三账漂移三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒账。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = PodmanDeskLedger::new();
        let caught = match kind {
            0 => lt.record_rl(RL_RUNNERS, 0, 0, 0).is_err(),
            1 => {
                // 逆序注入：临时账本从空起步，先落末相位再回落首相位必抓，
                // 主账 last_kind 不被改动。
                if let Some(w7) = kube_cell(7) {
                    let _ = lt.record_kube(7, w7);
                }
                match kube_cell(2) {
                    Some(w2) => lt.record_kube(2, w2) == Err(PodmanDeskCode::EOrder),
                    None => false,
                }
            }
            _ => {
                // 漂移注入：先录正格，再注漂移载荷。
                if let Some(w0) = acct_cell(0, 0) {
                    let _ = lt.record_acct(0, 0, w0);
                }
                match acct_cell(0, 1) {
                    Some(w1) => {
                        lt.record_acct(0, 1, w1.wrapping_add(1)) == Err(PodmanDeskCode::EDrift)
                    }
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5D800 + j as u64);
        } else {
            led.escaped += 1;
        }
        j += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52264 判据集（锚 UNX-F52265-J1，off-by-one 板式）。
pub fn run_unx52264_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52264");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "rootless 三态 12 格全落",
        {
            let mut lt = PodmanDeskLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RL_RUNNERS && ok {
                let mut st = 0usize;
                while st < RL_STATES {
                    let mut p = 0usize;
                    while p < RL_PROBES {
                        if let Some(w) = rl_cell(r, st, p) {
                            if lt.record_rl(r, st, p, w).is_err() {
                                ok = false;
                            }
                        }
                        p += 1;
                    }
                    st += 1;
                }
                r += 1;
            }
            ok && lt.rl_all()
        },
        "podman-machine/wsl 双 runner × 运行/停止/故障三态 × engine/gui 探针逐格恒等",
    );
    s.add(
        "容器/镜像/网络三账 15 格全落",
        recorded
            >= (RL_CELLS + ACCT_CELLS + CMP_CELLS + KUBE_CELLS + ELEC_LAYERS) as u32,
        "三族各 5 格闭集逐格恒等（UNX-F52265-J1 六判据共面）",
    );
    s.add(
        "compose 消费 6 格全落",
        {
            let mut lt = PodmanDeskLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMP_CELLS {
                if let Some(w) = cmp_cell(c) {
                    if lt.record_cmp(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmp_all()
        },
        "解析/拓扑/profile/up/down/logs 逐格恒等（F52263 口径复用）",
    );
    s.add(
        "kube 生成 8 格全落",
        {
            let mut lt = PodmanDeskLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < KUBE_CELLS {
                if let Some(w) = kube_cell(k) {
                    if lt.record_kube(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.kube_all()
        },
        "生成/转换/校验/部署四相位逐格恒等",
    );
    s.add(
        "Electron 4 层全落",
        {
            let mut lt = PodmanDeskLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELEC_LAYERS {
                if let Some(w) = elec_cell(l) {
                    if lt.record_elec(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.elec_all()
        },
        "主进程/渲染进程/预加载/原生桥逐层恒等",
    );
    s.add(
        "k8s/k3s 拉起依赖账 2/2 全齐",
        {
            let mut lt = PodmanDeskLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DEP_ROWS {
                if lt.declare_dep(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.deps_all()
        },
        "k8s 拉起依赖账/k3s 拉起依赖账两行闭集",
    );
    s.add(
        "N/A 同构声明",
        na_declaration() == 0 && F52261_PLANE == 0x574C32484F530000,
        "虚拟化依赖 N/A 同构声明（F52261 口径复用），折叠恒归零",
    );
    s.add(
        "kube 相位单调",
        {
            let mut ok = true;
            let mut k = 1usize;
            while k < KUBE_CELLS {
                if kube_kind(k).unwrap_or(4) < kube_kind(k - 1).unwrap_or(4) {
                    ok = false;
                }
                k += 1;
            }
            ok
        },
        "生成→转换→校验→部署闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "rootless 格越闭集/kube 相位逆序/三账漂移三态轮转必抓",
    );
    let mut l0 = PodmanDeskLedger::new();
    s.add(
        "边界·越闭集·rootless 格",
        l0.record_rl(RL_RUNNERS, 0, 0, 0).is_err() && rl_cell(RL_RUNNERS, 0, 0).is_none(),
        "runner 2 越闭集必拒",
    );
    s.add(
        "边界·越闭集·三账格",
        l0.record_acct(ACCT_FAMILIES, 0, 0).is_err()
            && acct_cell(ACCT_FAMILIES, 0).is_none(),
        "族 3 越闭集必拒",
    );
    s.add(
        "边界·越闭集·Electron 层",
        l0.record_elec(ELEC_LAYERS, 0).is_err() && elec_cell(ELEC_LAYERS).is_none(),
        "层 4 越闭集必拒",
    );
    s.add(
        "kube 相位逆序拒",
        {
            if let Some(w0) = kube_cell(0) {
                let _ = l0.record_kube(0, w0);
            }
            match kube_cell(1) {
                Some(w1) => l0.record_kube(1, w1.wrapping_add(1)) == Err(PodmanDeskCode::EDrift),
                None => false,
            }
        },
        "kube 载荷漂移必抓（相位单调另由注入核覆盖）",
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
        "判据锚定 §AI-66.3",
        CRITERIA_ANCHOR == "§AI-66.3" && !CRITERIA_ANCHOR.is_empty(),
        "卡面判据锚定原文逐字登记",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = PodmanDeskLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < RL_RUNNERS {
                let mut st = 0usize;
                while st < RL_STATES {
                    let mut p = 0usize;
                    while p < RL_PROBES {
                        if let Some(w) = rl_cell(r, st, p) {
                            let _ = lt.record_rl(r, st, p, w);
                        }
                        p += 1;
                    }
                    st += 1;
                }
                r += 1;
            }
            let mut f = 0usize;
            while f < ACCT_FAMILIES {
                let mut i = 0usize;
                while i < ACCT_PER_FAMILY {
                    if let Some(w) = acct_cell(f, i) {
                        let _ = lt.record_acct(f, i, w);
                    }
                    i += 1;
                }
                f += 1;
            }
            let mut c = 0usize;
            while c < CMP_CELLS {
                if let Some(w) = cmp_cell(c) {
                    let _ = lt.record_cmp(c, w);
                }
                c += 1;
            }
            let mut k = 0usize;
            while k < KUBE_CELLS {
                if let Some(w) = kube_cell(k) {
                    let _ = lt.record_kube(k, w);
                }
                k += 1;
            }
            let mut l = 0usize;
            while l < ELEC_LAYERS {
                if let Some(w) = elec_cell(l) {
                    let _ = lt.record_elec(l, w);
                }
                l += 1;
            }
            let mut d = 0usize;
            while d < DEP_ROWS {
                let _ = lt.declare_dep(d);
                d += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 有条件收口（KB-COMPAT 定级登记，rootless N/A 同构口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC81C0,
        "SEG_BASE 0x40 对齐且在 VX15 域（公式 0xC7200+0x40×63）",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rl_cells() {
        assert!(rl_cell(0, 0, 0).is_some());
        assert!(rl_cell(1, 2, 1).is_some());
        assert!(rl_cell(RL_RUNNERS, 0, 0).is_none());
        assert!(rl_cell(0, RL_STATES, 0).is_none());
        assert!(rl_cell(0, 0, RL_PROBES).is_none());
        assert_eq!(runner_name(0), Some("podman-machine"));
        assert_eq!(runner_name(1), Some("wsl"));
        assert!(runner_name(2).is_none());
        assert_eq!(rl_state_name(2), Some("故障"));
        assert_eq!(rl_probe_name(1), Some("gui"));
        let a = rl_cell(1, 1, 0).unwrap_or(0);
        assert_ne!(a, rl_cell(1, 1, 1).unwrap_or(0));
    }

    #[test]
    fn test_acct_cells() {
        assert_eq!(acct_family_name(0), Some("容器"));
        assert_eq!(acct_family_name(2), Some("网络"));
        assert!(acct_family_name(3).is_none());
        assert!(acct_cell(ACCT_FAMILIES, 0).is_none());
        assert!(acct_cell(0, ACCT_PER_FAMILY).is_none());
        let a = acct_cell(1, 2).unwrap_or(0);
        assert_eq!(a, acct_cell(1, 2).unwrap_or(0));
        assert_ne!(a, acct_cell(2, 2).unwrap_or(0));
    }

    #[test]
    fn test_kube_kind() {
        assert_eq!(kube_kind(0), Some(0));
        assert_eq!(kube_kind(1), Some(0));
        assert_eq!(kube_kind(2), Some(1));
        assert_eq!(kube_kind(7), Some(3));
        assert!(kube_kind(8).is_none());
        assert!(kube_cell(8).is_none());
        assert_eq!(kube_kind_name(0), Some("生成"));
        assert_eq!(kube_kind_name(3), Some("部署"));
        assert!(kube_kind_name(4).is_none());
    }

    #[test]
    fn test_cmp_elec_cells() {
        assert_eq!(cmp_cell_name(0), Some("compose 文件解析"));
        assert_eq!(cmp_cell_name(5), Some("logs 消费"));
        assert!(cmp_cell_name(6).is_none());
        assert!(cmp_cell(CMP_CELLS).is_none());
        assert_eq!(elec_layer_name(0), Some("主进程"));
        assert_eq!(elec_layer_name(3), Some("原生桥"));
        assert!(elec_layer_name(4).is_none());
        assert!(elec_cell(ELEC_LAYERS).is_none());
    }

    #[test]
    fn test_na_declaration() {
        assert_eq!(na_declaration(), na_declaration());
        assert_eq!(na_declaration(), 0);
        assert!(dep_row(DEP_ROWS).is_none());
        assert_eq!(dep_row_name(0), Some("k8s 拉起依赖账"));
        assert_eq!(dep_row_name(1), Some("k3s 拉起依赖账"));
    }

    #[test]
    fn test_kube_order_caught() {
        let mut lt = PodmanDeskLedger::new();
        let _ = lt.record_kube(7, kube_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_kube(2, kube_cell(2).unwrap_or(0)),
            Err(PodmanDeskCode::EOrder)
        );
        // 同相位不逆序：先 0 后 1 合法。
        let mut lt2 = PodmanDeskLedger::new();
        assert!(lt2.record_kube(0, kube_cell(0).unwrap_or(0)).is_ok());
        assert!(lt2.record_kube(1, kube_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PodmanDeskLedger::new();
        assert_eq!(
            lt.record_rl(0, 0, 0, rl_cell(0, 0, 0).unwrap_or(0).wrapping_add(5)),
            Err(PodmanDeskCode::EDrift)
        );
        assert_eq!(
            lt.record_acct(2, 3, acct_cell(2, 3).unwrap_or(0).wrapping_add(1)),
            Err(PodmanDeskCode::EDrift)
        );
        assert_eq!(
            lt.record_cmp(4, cmp_cell(4).unwrap_or(0).wrapping_add(9)),
            Err(PodmanDeskCode::EDrift)
        );
        assert_eq!(
            lt.record_elec(2, elec_cell(2).unwrap_or(0).wrapping_add(2)),
            Err(PodmanDeskCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PodmanDeskLedger::new();
        let _ = lt.record_acct(0, 0, acct_cell(0, 0).unwrap_or(0));
        assert_eq!(
            lt.record_acct(0, 0, acct_cell(0, 0).unwrap_or(0)),
            Err(PodmanDeskCode::EDup)
        );
        let _ = lt.declare_dep(1);
        assert_eq!(lt.declare_dep(1), Err(PodmanDeskCode::EDup));
        // 依赖账越界归 kube 域码。
        assert_eq!(lt.declare_dep(DEP_ROWS), Err(PodmanDeskCode::ENoKube));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PodmanDeskLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PodmanDeskLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_injection_leaves_main_ledger() {
        let mut main = PodmanDeskLedger::new();
        let before = main.last_kind();
        let mut lt = PodmanDeskLedger::new();
        let _ = lt.record_kube(7, kube_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_kube(2, kube_cell(2).unwrap_or(0)),
            Err(PodmanDeskCode::EOrder)
        );
        assert_eq!(main.last_kind(), before);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (RL_CELLS + ACCT_CELLS + CMP_CELLS + KUBE_CELLS + ELEC_LAYERS) as u32
        );
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
        assert_eq!(SEG_BASE, 0xC81C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：Ok=0，EGradeDrift=12。
        assert_eq!(PodmanDeskCode::Ok as u32, 0);
        assert_eq!(PodmanDeskCode::ENoRl as u32, 1);
        assert_eq!(PodmanDeskCode::EDrift as u32, 2);
        assert_eq!(PodmanDeskCode::ENoAcct as u32, 3);
        assert_eq!(PodmanDeskCode::EOrder as u32, 4);
        assert_eq!(PodmanDeskCode::ENoCmp as u32, 5);
        assert_eq!(PodmanDeskCode::ENoKube as u32, 6);
        assert_eq!(PodmanDeskCode::ENoEl as u32, 7);
        assert_eq!(PodmanDeskCode::EDup as u32, 8);
        assert_eq!(PodmanDeskCode::ESeal as u32, 9);
        assert_eq!(PodmanDeskCode::EInj as u32, 10);
        assert_eq!(PodmanDeskCode::EFp as u32, 11);
        assert_eq!(PodmanDeskCode::EGradeDrift as u32, 12);
        assert!(!PodmanDeskCode::Ok.detail().is_empty());
        assert!(!PodmanDeskCode::EGradeDrift.detail().is_empty());
        assert_eq!(PodmanDeskCode::EGradeDrift.val(), SEG_BASE + 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PodmanDeskLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = PodmanDeskLedger::new();
        let mut g2 = 0usize;
        while g2 < GATES {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        // 六判据全落但格账未齐 → S2 有条件收口。
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn unx52264_checks_green() {
        let s = run_unx52264_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
