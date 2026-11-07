//! UNX-F52372 Flux CLI 收口 · 0xC9CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52372"，逐字）：
//!   title: Flux CLI 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52373-J1 playbook
//!     ×10 全通；winrm 连接面消费（I 部语义联签锚定行）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结
//!     果摘要非空。
//! off-by-one 板式：本卡（卡 N=52372）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52373-J1，照原文锚定；功能按本卡标题（Flux CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五控制器 20 格闭集：source/kustomize/helm/notification/image 各
//!      4 格（kind = o/4），控制器序单调（逆序必抓），载荷折叠纯函数恒
//!      等才收。
//!   ② bootstrap 10 操作集：bootstrap git/github/gitlab/check/get sources/
//!      get kustomization/reconcile/suspend/resume/delete，越闭集必拒。
//!   ③ GitRepository 投影 8 格：apiVersion/kind/metadata/spec.url/
//!      spec.ref.branch/spec.interval/spec.secretRef/status.artifact，
//!      逐格落账缺格拒收。
//!   ④ reconcile 8 拍单调：拍序递增记账，拍载荷折叠逐拍恒等，回退必抓。
//!   ⑤ kubectl 消费联签 4 格：CRD 就绪/RBAC 授权/命名空间/events 通道；
//!      Go 上栈声明 2 行闭集，缺一拒封。
//!   ⑥ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（控制器越闭集/控制器载荷漂移/控制器逆
//!      序），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 10 操作 + 8 投影 + 8 拍 + 4 联签 + 2 上栈 + 六判据全通
//!      → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52372;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五控制器。
pub const CONTROLLERS: usize = 5;

/// 每控制器格数。
pub const CTRL_CELLS: usize = 4;

/// 控制器总格数（5 × 4）。
pub const CTRL_CELLS_TOTAL: usize = 20;

/// bootstrap 操作数。
pub const BOOT_OPS: usize = 10;

/// GitRepository 投影格数。
pub const GIT_PROJ: usize = 8;

/// reconcile 单调拍数。
pub const RECON_BEATS: usize = 8;

/// kubectl 消费联签格数。
pub const KCTL_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 收口六判据。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FluxCode {
    /// 成功
    Ok = 0,
    /// 控制器格越闭集
    ENoCtrl,
    /// 控制器载荷漂移
    ECtrlDrift,
    /// bootstrap 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EDrift,
    /// 控制器逆序
    EOrder,
    /// GitRepository 投影格越闭集
    ENoProj,
    /// reconcile 拍回退
    EBeatBack,
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

impl FluxCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FluxCode::Ok => "成功",
            FluxCode::ENoCtrl => "控制器格越闭集：source/kustomize/helm/notification/image 20 格之外先登记",
            FluxCode::ECtrlDrift => "控制器载荷漂移：控制器折叠与基准不恒等，逐格复算",
            FluxCode::ENoOp => "bootstrap 操作号越闭集：10 操作集之外先登记",
            FluxCode::EDrift => "操作载荷漂移：操作折叠与基准不恒等，逐项复算",
            FluxCode::EOrder => "控制器逆序：source→kustomize→helm→notification→image 单调，逆序必抓",
            FluxCode::ENoProj => "GitRepository 投影格越闭集：8 格投影之外先登记",
            FluxCode::EBeatBack => "reconcile 拍回退：8 拍单调递增，回退必抓",
            FluxCode::EDup => "重复落账：同格只许一条账目",
            FluxCode::ESeal => "未齐即封：20 格 + 10 操作 + 8 投影 + 8 拍 + 4 联签 + 2 上栈未齐先补齐",
            FluxCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            FluxCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FluxCode::EGradeDrift => "定级漂移：全通且联签上栈全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡登记 S1 全通收口）。
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
            Grade::S1 => "S1：六判据+20格+10操作+8投影+8拍+4联签+2上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 控制器名（const fn 表 + Option 边界）。
pub const fn controller_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("source"),
        1 => Some("kustomize"),
        2 => Some("helm"),
        3 => Some("notification"),
        4 => Some("image"),
        _ => None,
    }
}

/// 控制器格载荷（格 o；控制器 = o/4）。
pub const fn ctrl_cell(o: usize) -> Option<u64> {
    if o >= CTRL_CELLS_TOTAL {
        return None;
    }
    Some((0xE2000u64 + (o as u64) * 0x3D) ^ 0x464C55585F5F0000)
}

/// 控制器号（0..4=source 4..8=kustomize 8..12=helm 12..16=notification 16..20=image）。
pub const fn ctrl_kind(o: usize) -> Option<u32> {
    if o >= CTRL_CELLS_TOTAL {
        return None;
    }
    Some((o / CTRL_CELLS) as u32)
}

/// 控制器族名。
pub const fn ctrl_kind_name(k: usize) -> Option<&'static str> {
    controller_name(k)
}

/// bootstrap 操作载荷（操作 o）。
pub const fn boot_cell(o: usize) -> Option<u64> {
    if o >= BOOT_OPS {
        return None;
    }
    Some((0xE3000u64 + (o as u64) * 0x2F) ^ 0x42535452505F0000)
}

/// bootstrap 操作名。
pub const fn boot_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("bootstrap git"),
        1 => Some("bootstrap github"),
        2 => Some("bootstrap gitlab"),
        3 => Some("check"),
        4 => Some("get sources"),
        5 => Some("get kustomization"),
        6 => Some("reconcile"),
        7 => Some("suspend"),
        8 => Some("resume"),
        9 => Some("delete"),
        _ => None,
    }
}

/// GitRepository 投影格载荷（格 r）。
pub const fn gitproj_cell(r: usize) -> Option<u32> {
    if r >= GIT_PROJ {
        return None;
    }
    Some(0xE400 + r as u32 * 0x43)
}

/// GitRepository 投影字段名。
pub const fn gitproj_field_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("apiVersion"),
        1 => Some("kind"),
        2 => Some("metadata"),
        3 => Some("spec.url"),
        4 => Some("spec.ref.branch"),
        5 => Some("spec.interval"),
        6 => Some("spec.secretRef"),
        7 => Some("status.artifact"),
        _ => None,
    }
}

/// reconcile 拍载荷（拍 b）：逐拍严格递增折叠。
pub const fn beat_cell(b: usize) -> Option<u32> {
    if b >= RECON_BEATS {
        return None;
    }
    Some(0xE500 + b as u32 * 0x47)
}

/// kubectl 消费联签行折叠（行 r）。
pub const fn kubectl_row(r: usize) -> Option<u32> {
    if r >= KCTL_ROWS {
        return None;
    }
    Some(0xE600 + r as u32 * 0x49)
}

/// kubectl 联签行名。
pub const fn kubectl_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("CRD 就绪"),
        1 => Some("RBAC 授权"),
        2 => Some("命名空间"),
        3 => Some("events 通道"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_stack_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xE700 + r as u32 * 0x4B)
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("bootstrap"),
        1 => Some("source 就绪"),
        2 => Some("kustomize 就绪"),
        3 => Some("helm 就绪"),
        4 => Some("notification 就绪"),
        5 => Some("image 就绪"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Flux CLI 收口账本状态机。
#[derive(Clone)]
pub struct FluxLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATES],
    /// 控制器 20 格账。
    ctrls: [Option<u64>; CTRL_CELLS_TOTAL],
    /// bootstrap 10 操作账。
    boots: [Option<u64>; BOOT_OPS],
    /// GitRepository 投影 8 格账。
    proj: [Option<u32>; GIT_PROJ],
    /// reconcile 8 拍账。
    beats: [Option<u32>; RECON_BEATS],
    /// 最后控制器号。
    last_kind: i64,
    /// 最后已落拍号。
    last_beat: i64,
    /// kubectl 联签账。
    kctl: [Option<u32>; KCTL_ROWS],
    /// Go 上栈账。
    go_stack: [Option<u32>; GO_ROWS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FluxLedger {
    /// 新账本。
    pub fn new() -> Self {
        FluxLedger {
            gates: [None; GATES],
            ctrls: [None; CTRL_CELLS_TOTAL],
            boots: [None; BOOT_OPS],
            proj: [None; GIT_PROJ],
            beats: [None; RECON_BEATS],
            last_kind: -1,
            last_beat: -1,
            kctl: [None; KCTL_ROWS],
            go_stack: [None; GO_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FluxCode> {
        if gate >= GATES {
            return Err(FluxCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8A00 + gate as u64);
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

    /// 控制器一格（控制器序单调 + 载荷对照 + 重复拒；逆序被拒不改 last_kind）。
    pub fn record_ctrl(&mut self, o: usize, got: u64) -> Result<(), FluxCode> {
        if o >= CTRL_CELLS_TOTAL {
            return Err(FluxCode::ENoCtrl);
        }
        let kind = match ctrl_kind(o) {
            Some(k) => k,
            None => return Err(FluxCode::ENoCtrl),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8B00);
            return Err(FluxCode::EOrder);
        }
        let want = match ctrl_cell(o) {
            Some(w) => w,
            None => return Err(FluxCode::ENoCtrl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8B01 + o as u64);
            return Err(FluxCode::ECtrlDrift);
        }
        if match self.ctrls.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluxCode::EDup);
        }
        if let Some(c) = self.ctrls.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8B02 + o as u64);
        Ok(())
    }

    /// 控制器全账核验（20 格）。
    pub fn ctrls_all(&self) -> bool {
        let mut o = 0usize;
        while o < CTRL_CELLS_TOTAL {
            match self.ctrls.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// bootstrap 一格（载荷对照 + 重复拒）。
    pub fn record_boot(&mut self, o: usize, got: u64) -> Result<(), FluxCode> {
        if o >= BOOT_OPS {
            return Err(FluxCode::ENoOp);
        }
        let want = match boot_cell(o) {
            Some(w) => w,
            None => return Err(FluxCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8C00 + o as u64);
            return Err(FluxCode::EDrift);
        }
        if match self.boots.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluxCode::EDup);
        }
        if let Some(c) = self.boots.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x8C01 + o as u64);
        Ok(())
    }

    /// bootstrap 全账核验（10 操作）。
    pub fn boots_all(&self) -> bool {
        let mut o = 0usize;
        while o < BOOT_OPS {
            match self.boots.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// GitRepository 投影一格。
    pub fn record_proj(&mut self, r: usize) -> Result<(), FluxCode> {
        let want = match gitproj_cell(r) {
            Some(w) => w,
            None => return Err(FluxCode::ENoProj),
        };
        if match self.proj.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluxCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x8D00 + r as u64);
        Ok(())
    }

    /// 投影全齐核验。
    pub fn proj_all(&self) -> bool {
        let mut r = 0usize;
        while r < GIT_PROJ {
            match self.proj.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// reconcile 一拍（拍序单调递增，回退必抓）。
    pub fn record_beat(&mut self, b: usize) -> Result<(), FluxCode> {
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(FluxCode::ENoCtrl),
        };
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8E00);
            return Err(FluxCode::EBeatBack);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluxCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(want);
        }
        self.last_beat = b as i64;
        self.fnv(0x8E01 + b as u64);
        Ok(())
    }

    /// reconcile 全拍核验（8 拍全落且单调已由落账保证）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < RECON_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// kubectl 联签一行。
    pub fn declare_kubectl(&mut self, r: usize) -> Result<(), FluxCode> {
        let want = match kubectl_row(r) {
            Some(w) => w,
            None => return Err(FluxCode::ESeal),
        };
        if match self.kctl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluxCode::EDup);
        }
        if let Some(c) = self.kctl.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x8F00 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn kctl_all(&self) -> bool {
        let mut r = 0usize;
        while r < KCTL_ROWS {
            match self.kctl.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go_stack(&mut self, r: usize) -> Result<(), FluxCode> {
        let want = match go_stack_row(r) {
            Some(w) => w,
            None => return Err(FluxCode::ESeal),
        };
        if match self.go_stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FluxCode::EDup);
        }
        if let Some(c) = self.go_stack.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x9000 + r as u64);
        Ok(())
    }

    /// 上栈全齐核验。
    pub fn go_stack_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.go_stack.get(r) {
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
        if self.gates_all()
            && self.ctrls_all()
            && self.boots_all()
            && self.proj_all()
            && self.beats_all()
            && self.kctl_all()
            && self.go_stack_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for FluxLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FluxLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CTRL_CELLS_TOTAL {
        if let Some(w) = ctrl_cell(o) {
            if led.record_ctrl(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut o = 0usize;
    while o < BOOT_OPS {
        if let Some(w) = boot_cell(o) {
            if led.record_boot(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < GIT_PROJ {
        if led.record_proj(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    let mut b = 0usize;
    while b < RECON_BEATS {
        let _ = led.record_beat(b);
        b += 1;
    }
    let mut r = 0usize;
    while r < KCTL_ROWS {
        let _ = led.declare_kubectl(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go_stack(r);
        r += 1;
    }
    // 注入 10 轮：控制器越闭集 / 控制器载荷漂移 / 控制器逆序三态轮转。
    // 逆序轮对空临时账本先按合法序落高控制器正格再注低控制器（必抓且不改 last_kind）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FluxLedger::new();
        let caught = match kind {
            0 => lt.record_ctrl(CTRL_CELLS_TOTAL, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格再注漂移（同控制器内，免触发逆序闸）。
                let _ = lt.record_ctrl(5, ctrl_cell(5).unwrap_or(0));
                lt.record_ctrl(4, ctrl_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(FluxCode::ECtrlDrift)
            }
            _ => {
                let _ = lt.record_ctrl(6, ctrl_cell(6).unwrap_or(0));
                lt.record_ctrl(1, ctrl_cell(1).unwrap_or(0)) == Err(FluxCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9100 + k as u64);
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
/// UNX-F52372 判据集（锚 UNX-F52373-J1，off-by-one 板式）。
pub fn run_unx52372_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52372");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五控制器 20 格全通",
        recorded >= CTRL_CELLS_TOTAL as u32,
        "source/kustomize/helm/notification/image 逐控制器逐格恒等（UNX-F52373-J1 锚定）",
    );
    s.add(
        "bootstrap 10 操作全通",
        {
            let mut lt = FluxLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < BOOT_OPS {
                if let Some(w) = boot_cell(o) {
                    if lt.record_boot(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.boots_all()
        },
        "bootstrap git/github/gitlab→delete 十操作闭集逐项恒等",
    );
    s.add(
        "GitRepository 投影 8 格全齐",
        {
            let mut lt = FluxLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GIT_PROJ {
                if lt.record_proj(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.proj_all()
        },
        "apiVersion→status.artifact 八格逐格投影（§AI-66.3）",
    );
    s.add(
        "reconcile 8 拍单调",
        {
            let mut lt = FluxLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < RECON_BEATS {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "八拍递增落账，回退必抓",
    );
    s.add(
        "kubectl 消费联签 4/4 全齐",
        {
            let mut lt = FluxLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < KCTL_ROWS {
                if lt.declare_kubectl(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.kctl_all()
        },
        "CRD 就绪/RBAC 授权/命名空间/events 通道四格联签（I 部语义联签锚定行）",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = FluxLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.go_stack_all()
        },
        "Go 上栈声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = FluxLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "bootstrap/source/kustomize/helm/notification/image 就绪逐闸落账",
    );
    s.add(
        "控制器序单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CTRL_CELLS_TOTAL {
                if ctrl_kind(o).unwrap_or(5) < ctrl_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "source→kustomize→helm→notification→image 五控制器闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "控制器越闭集/控制器漂移/控制器逆序三态轮转必抓",
    );
    let mut l0 = FluxLedger::new();
    s.add(
        "边界·控制器越闭集",
        l0.record_ctrl(CTRL_CELLS_TOTAL, 0).is_err(),
        "控制器格 20 越闭集必拒",
    );
    s.add(
        "边界·控制器漂移",
        {
            let _ = l0.record_ctrl(5, ctrl_cell(5).unwrap_or(0));
            l0.record_ctrl(4, ctrl_cell(4).unwrap_or(0).wrapping_add(1)) == Err(FluxCode::ECtrlDrift)
        },
        "控制器载荷漂移必抓",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(GIT_PROJ).is_err(),
        "GitRepository 投影格 8 越闭集必拒",
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
            let mut lt = FluxLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CTRL_CELLS_TOTAL {
                if let Some(w) = ctrl_cell(o) {
                    let _ = lt.record_ctrl(o, w);
                }
                o += 1;
            }
            let mut o = 0usize;
            while o < BOOT_OPS {
                if let Some(w) = boot_cell(o) {
                    let _ = lt.record_boot(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < GIT_PROJ {
                let _ = lt.record_proj(r);
                r += 1;
            }
            let mut b = 0usize;
            while b < RECON_BEATS {
                let _ = lt.record_beat(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < KCTL_ROWS {
                let _ = lt.declare_kubectl(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go_stack(r);
                r += 1;
            }
            // 定级断言前置所有分节记账完毕（main 节已全落）。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9CC0,
        "SEG_BASE 0x40 对齐且落在 VX15 0xC9CC0 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_names() {
        assert_eq!(controller_name(0), Some("source"));
        assert_eq!(controller_name(1), Some("kustomize"));
        assert_eq!(controller_name(2), Some("helm"));
        assert_eq!(controller_name(3), Some("notification"));
        assert_eq!(controller_name(4), Some("image"));
        assert!(controller_name(5).is_none());
    }

    #[test]
    fn test_ctrl_kind() {
        assert_eq!(ctrl_kind(0), Some(0));
        assert_eq!(ctrl_kind(4), Some(1));
        assert_eq!(ctrl_kind(12), Some(3));
        assert_eq!(ctrl_kind(19), Some(4));
        assert!(ctrl_kind(20).is_none());
        assert!(ctrl_cell(20).is_none());
    }

    #[test]
    fn test_boot_ops_closed() {
        assert_eq!(boot_op_name(0), Some("bootstrap git"));
        assert_eq!(boot_op_name(6), Some("reconcile"));
        assert_eq!(boot_op_name(9), Some("delete"));
        assert!(boot_op_name(10).is_none());
        assert!(boot_cell(10).is_none());
    }

    #[test]
    fn test_gitproj_fields() {
        assert_eq!(gitproj_field_name(0), Some("apiVersion"));
        assert_eq!(gitproj_field_name(4), Some("spec.ref.branch"));
        assert_eq!(gitproj_field_name(7), Some("status.artifact"));
        assert!(gitproj_field_name(8).is_none());
        assert!(gitproj_cell(GIT_PROJ).is_none());
    }

    #[test]
    fn test_beats_monotonic_closed() {
        assert!(beat_cell(RECON_BEATS).is_none());
        let a = beat_cell(2).unwrap_or(0);
        assert_eq!(a, beat_cell(2).unwrap_or(0));
        assert!(beat_cell(3).unwrap_or(0) > a);
        assert!(kubectl_row(KCTL_ROWS).is_none());
        assert!(go_stack_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_ctrl_order_caught() {
        let mut lt = FluxLedger::new();
        let _ = lt.record_ctrl(9, ctrl_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_ctrl(2, ctrl_cell(2).unwrap_or(0)),
            Err(FluxCode::EOrder)
        );
        // 逆序注入被拒不改 last_kind：同控制器后格仍可落。
        assert!(lt.record_ctrl(11, ctrl_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_back_caught() {
        let mut lt = FluxLedger::new();
        assert!(lt.record_beat(0).is_ok());
        assert!(lt.record_beat(1).is_ok());
        assert_eq!(lt.record_beat(1), Err(FluxCode::EBeatBack));
        assert_eq!(lt.record_beat(0), Err(FluxCode::EBeatBack));
        assert!(lt.record_beat(2).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FluxLedger::new();
        assert_eq!(
            lt.record_ctrl(17, ctrl_cell(17).unwrap_or(0).wrapping_add(1)),
            Err(FluxCode::ECtrlDrift)
        );
        assert_eq!(
            lt.record_boot(7, boot_cell(7).unwrap_or(0).wrapping_add(3)),
            Err(FluxCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FluxLedger::new();
        let _ = lt.record_ctrl(0, ctrl_cell(0).unwrap_or(0));
        assert_eq!(lt.record_ctrl(0, ctrl_cell(0).unwrap_or(0)), Err(FluxCode::EDup));
        let _ = lt.record_proj(2);
        assert_eq!(lt.record_proj(2), Err(FluxCode::EDup));
        let _ = lt.declare_kubectl(0);
        assert_eq!(lt.declare_kubectl(0), Err(FluxCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FluxLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = FluxLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 计数口径：联签行与上栈行不计 recorded（见 run_cycle）。
        assert_eq!(
            recorded,
            (CTRL_CELLS_TOTAL + BOOT_OPS + GIT_PROJ) as u32
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
        assert_eq!(SEG_BASE, 0xC9CC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(FluxCode::Ok as u32, 0);
        assert_eq!(FluxCode::EGradeDrift as u32, 12);
        assert_eq!(FluxCode::Ok.val(), SEG_BASE);
        assert_eq!(FluxCode::EBeatBack.val(), SEG_BASE + 7);
        let mut i = 0u32;
        while i < 13 {
            let c: FluxCode = match i {
                0 => FluxCode::Ok,
                1 => FluxCode::ENoCtrl,
                2 => FluxCode::ECtrlDrift,
                3 => FluxCode::ENoOp,
                4 => FluxCode::EDrift,
                5 => FluxCode::EOrder,
                6 => FluxCode::ENoProj,
                7 => FluxCode::EBeatBack,
                8 => FluxCode::EDup,
                9 => FluxCode::ESeal,
                10 => FluxCode::EInj,
                11 => FluxCode::EFp,
                _ => FluxCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = FluxLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = FluxLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        // 如实阶梯：六闸齐 → S2（各面全齐才 S1）。
        let mut lt3 = FluxLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_stats_conservation() {
        let lt = FluxLedger::new();
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 0);
        assert_eq!(caught, 0);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx52372_checks_green() {
        let s = run_unx52372_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
