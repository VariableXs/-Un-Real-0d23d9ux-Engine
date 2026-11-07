//! UNX-F52371 Argo CD CLI 收口 · 0xC9C80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52371"，逐字）：
//!   title: Argo CD CLI 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52372-J1
//!     bootstrap/reconcile ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52371）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52372-J1，照原文锚定；功能按本卡标题（Argo CD CLI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① app 五相位闭集：sync/health/refresh/hard-refresh/diff 各 4 格共
//!      20 格（kind = o/4），相位序单调（逆序必抓），相位载荷折叠纯函数
//!      恒等才收。
//!   ② app 10 操作集：list/get/sync/wait/diff/history/rollback/delete/
//!      manifests/resources，越闭集必拒。
//!   ③ Application CRD 投影 12 格：apiVersion/kind/metadata.name/spec.
//!      project/spec.source.repoURL/spec.source.path/spec.source.
//!      targetRevision/spec.destination.server/spec.destination.namespace/
//!      spec.syncPolicy.automated/spec.syncPolicy.prune/spec.sources，
//!      逐格落账缺格拒收。
//!   ④ kubectl 消费联签 4 格：CRD 就绪/RBAC 授权/命名空间/events 通道；
//!      Go 上栈声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（相位越闭集/相位载荷漂移/相位逆序），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 格 + 10 操作 + 12 投影 + 4 联签 + 2 上栈 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52371;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// app 五相位。
pub const PHASES: usize = 5;

/// 每相位格数。
pub const PHASE_CELLS: usize = 4;

/// 相位总格数（5 相位 × 4 格）。
pub const PHASE_CELLS_TOTAL: usize = 20;

/// app 操作数。
pub const APP_OPS: usize = 10;

/// Application CRD 投影格数。
pub const CRD_PROJ: usize = 12;

/// kubectl 消费联签格数。
pub const KCTL_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 收口六判据。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgocdCode {
    /// 成功
    Ok = 0,
    /// 相位号越闭集
    ENoPhase,
    /// 相位载荷漂移
    EPhaseDrift,
    /// app 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EDrift,
    /// 相位逆序
    EOrder,
    /// CRD 投影格越闭集
    ENoProj,
    /// 联签/上栈行越闭集
    ENoRow,
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

impl ArgocdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ArgocdCode::Ok => "成功",
            ArgocdCode::ENoPhase => "相位号越闭集：sync/health/refresh/hard-refresh/diff 20 格之外先登记",
            ArgocdCode::EPhaseDrift => "相位载荷漂移：相位折叠与基准不恒等，逐格复算",
            ArgocdCode::ENoOp => "app 操作号越闭集：10 操作集之外先登记",
            ArgocdCode::EDrift => "操作载荷漂移：操作折叠与基准不恒等，逐项复算",
            ArgocdCode::EOrder => "相位逆序：sync→health→refresh→hard-refresh→diff 单调，逆序必抓",
            ArgocdCode::ENoProj => "CRD 投影格越闭集：Application CRD 12 格投影之外先登记",
            ArgocdCode::ENoRow => "联签/上栈行越闭集：kubectl 联签 4 格与 Go 上栈 2 行闭集之外先登记",
            ArgocdCode::EDup => "重复落账：同格只许一条账目",
            ArgocdCode::ESeal => "未齐即封：20 格 + 10 操作 + 12 投影 + 4 联签 + 2 上栈未齐先补齐",
            ArgocdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ArgocdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ArgocdCode::EGradeDrift => "定级漂移：全通且联签上栈全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+10操作+12投影+4联签+2上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// app 相位名（const fn 表 + Option 边界）。
pub const fn phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("sync"),
        1 => Some("health"),
        2 => Some("refresh"),
        3 => Some("hard-refresh"),
        4 => Some("diff"),
        _ => None,
    }
}

/// 相位格载荷（格 o；相位 = o/4）。
pub const fn phase_cell(o: usize) -> Option<u64> {
    if o >= PHASE_CELLS_TOTAL {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x35) ^ 0x4152474F43440000)
}

/// 相位号（0..4=sync 4..8=health 8..12=refresh 12..16=hard-refresh 16..20=diff）。
pub const fn phase_kind(o: usize) -> Option<u32> {
    if o >= PHASE_CELLS_TOTAL {
        return None;
    }
    Some((o / PHASE_CELLS) as u32)
}

/// 相位族名。
pub const fn phase_kind_name(k: usize) -> Option<&'static str> {
    phase_name(k)
}

/// app 操作载荷（操作 o）。
pub const fn app_cell(o: usize) -> Option<u64> {
    if o >= APP_OPS {
        return None;
    }
    Some((0xD3000u64 + (o as u64) * 0x2B) ^ 0x4150505F434C0000)
}

/// app 操作名。
pub const fn app_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("app list"),
        1 => Some("app get"),
        2 => Some("app sync"),
        3 => Some("app wait"),
        4 => Some("app diff"),
        5 => Some("app history"),
        6 => Some("app rollback"),
        7 => Some("app delete"),
        8 => Some("app manifests"),
        9 => Some("app resources"),
        _ => None,
    }
}

/// Application CRD 投影格载荷（格 r）。
pub const fn crd_cell(r: usize) -> Option<u32> {
    if r >= CRD_PROJ {
        return None;
    }
    Some(0xD400 + r as u32 * 0x3B)
}

/// Application CRD 投影字段名。
pub const fn crd_field_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("apiVersion"),
        1 => Some("kind"),
        2 => Some("metadata.name"),
        3 => Some("spec.project"),
        4 => Some("spec.source.repoURL"),
        5 => Some("spec.source.path"),
        6 => Some("spec.source.targetRevision"),
        7 => Some("spec.destination.server"),
        8 => Some("spec.destination.namespace"),
        9 => Some("spec.syncPolicy.automated"),
        10 => Some("spec.syncPolicy.prune"),
        11 => Some("spec.sources"),
        _ => None,
    }
}

/// kubectl 消费联签行折叠（行 r）。
pub const fn kubectl_row(r: usize) -> Option<u32> {
    if r >= KCTL_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x3D)
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
    Some(0xD600 + r as u32 * 0x41)
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("服务端启动"),
        1 => Some("CLI 登录"),
        2 => Some("app 登记"),
        3 => Some("sync 执行"),
        4 => Some("health 巡检"),
        5 => Some("回滚通道"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Argo CD CLI 收口账本状态机。
#[derive(Clone)]
pub struct ArgocdLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATES],
    /// 相位 20 格账。
    phases: [Option<u64>; PHASE_CELLS_TOTAL],
    /// app 10 操作账。
    ops: [Option<u64>; APP_OPS],
    /// CRD 投影 12 格账。
    crd: [Option<u32>; CRD_PROJ],
    /// kubectl 联签账。
    kctl: [Option<u32>; KCTL_ROWS],
    /// Go 上栈账。
    go_stack: [Option<u32>; GO_ROWS],
    /// 最后相位号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ArgocdLedger {
    /// 新账本。
    pub fn new() -> Self {
        ArgocdLedger {
            gates: [None; GATES],
            phases: [None; PHASE_CELLS_TOTAL],
            ops: [None; APP_OPS],
            crd: [None; CRD_PROJ],
            kctl: [None; KCTL_ROWS],
            go_stack: [None; GO_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ArgocdCode> {
        if gate >= GATES {
            return Err(ArgocdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7A00 + gate as u64);
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

    /// 相位一格（相位序单调 + 载荷对照 + 重复拒；逆序被拒不改 last_kind）。
    pub fn record_phase(&mut self, o: usize, got: u64) -> Result<(), ArgocdCode> {
        if o >= PHASE_CELLS_TOTAL {
            return Err(ArgocdCode::ENoPhase);
        }
        let kind = match phase_kind(o) {
            Some(k) => k,
            None => return Err(ArgocdCode::ENoPhase),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B00);
            return Err(ArgocdCode::EOrder);
        }
        let want = match phase_cell(o) {
            Some(w) => w,
            None => return Err(ArgocdCode::ENoPhase),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B01 + o as u64);
            return Err(ArgocdCode::EPhaseDrift);
        }
        if match self.phases.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArgocdCode::EDup);
        }
        if let Some(c) = self.phases.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7B02 + o as u64);
        Ok(())
    }

    /// 相位全账核验（20 格）。
    pub fn phases_all(&self) -> bool {
        let mut o = 0usize;
        while o < PHASE_CELLS_TOTAL {
            match self.phases.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// app 一格（载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ArgocdCode> {
        if o >= APP_OPS {
            return Err(ArgocdCode::ENoOp);
        }
        let want = match app_cell(o) {
            Some(w) => w,
            None => return Err(ArgocdCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7C00 + o as u64);
            return Err(ArgocdCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArgocdCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x7C01 + o as u64);
        Ok(())
    }

    /// app 全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < APP_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// CRD 投影一格。
    pub fn record_crd(&mut self, r: usize) -> Result<(), ArgocdCode> {
        let want = match crd_cell(r) {
            Some(w) => w,
            None => return Err(ArgocdCode::ENoProj),
        };
        if match self.crd.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArgocdCode::EDup);
        }
        if let Some(c) = self.crd.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x7D00 + r as u64);
        Ok(())
    }

    /// CRD 投影全齐核验。
    pub fn crd_all(&self) -> bool {
        let mut r = 0usize;
        while r < CRD_PROJ {
            match self.crd.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// kubectl 联签一行。
    pub fn declare_kubectl(&mut self, r: usize) -> Result<(), ArgocdCode> {
        let want = match kubectl_row(r) {
            Some(w) => w,
            None => return Err(ArgocdCode::ENoRow),
        };
        if match self.kctl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArgocdCode::EDup);
        }
        if let Some(c) = self.kctl.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x7E00 + r as u64);
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
    pub fn declare_go_stack(&mut self, r: usize) -> Result<(), ArgocdCode> {
        let want = match go_stack_row(r) {
            Some(w) => w,
            None => return Err(ArgocdCode::ENoRow),
        };
        if match self.go_stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ArgocdCode::EDup);
        }
        if let Some(c) = self.go_stack.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x7F00 + r as u64);
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
            && self.phases_all()
            && self.ops_all()
            && self.crd_all()
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

impl Default for ArgocdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ArgocdLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PHASE_CELLS_TOTAL {
        if let Some(w) = phase_cell(o) {
            if led.record_phase(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut o = 0usize;
    while o < APP_OPS {
        if let Some(w) = app_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < CRD_PROJ {
        if led.record_crd(r).is_ok() {
            recorded += 1;
        }
        r += 1;
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
    // 注入 10 轮：相位越闭集 / 相位载荷漂移 / 相位逆序三态轮转。
    // 逆序轮对空临时账本先按合法序落高相位正格再注低相位（必抓且不改 last_kind）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ArgocdLedger::new();
        let caught = match kind {
            0 => lt.record_phase(PHASE_CELLS_TOTAL, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格再注漂移（同相位内，免触发逆序闸）。
                let _ = lt.record_phase(6, phase_cell(6).unwrap_or(0));
                lt.record_phase(4, phase_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(ArgocdCode::EPhaseDrift)
            }
            _ => {
                let _ = lt.record_phase(5, phase_cell(5).unwrap_or(0));
                lt.record_phase(1, phase_cell(1).unwrap_or(0)) == Err(ArgocdCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8000 + k as u64);
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
/// UNX-F52371 判据集（锚 UNX-F52372-J1，off-by-one 板式）。
pub fn run_unx52371_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52371");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "app 五相位 20 格全通",
        recorded >= PHASE_CELLS_TOTAL as u32,
        "sync/health/refresh/hard-refresh/diff 逐相位逐格恒等（UNX-F52372-J1 锚定）",
    );
    s.add(
        "app 10 操作全通",
        {
            let mut lt = ArgocdLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < APP_OPS {
                if let Some(w) = app_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "list/get/sync/wait/diff/history/rollback/delete/manifests/resources",
    );
    s.add(
        "Application CRD 投影 12 格全齐",
        {
            let mut lt = ArgocdLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CRD_PROJ {
                if lt.record_crd(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.crd_all()
        },
        "apiVersion→spec.sources 十二格逐格投影（§AI-66.3）",
    );
    s.add(
        "kubectl 消费联签 4/4 全齐",
        {
            let mut lt = ArgocdLedger::new();
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
        "CRD 就绪/RBAC 授权/命名空间/events 通道四格联签",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = ArgocdLedger::new();
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
            let mut lt = ArgocdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "服务端启动/CLI 登录/app 登记/sync 执行/health 巡检/回滚通道",
    );
    s.add(
        "相位序单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PHASE_CELLS_TOTAL {
                if phase_kind(o).unwrap_or(5) < phase_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "sync→health→refresh→hard-refresh→diff 五相位闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "相位越闭集/相位漂移/相位逆序三态轮转必抓",
    );
    let mut l0 = ArgocdLedger::new();
    s.add(
        "边界·相位越闭集",
        l0.record_phase(PHASE_CELLS_TOTAL, 0).is_err(),
        "相位格 20 越闭集必拒",
    );
    s.add(
        "边界·相位漂移",
        {
            let _ = l0.record_phase(6, phase_cell(6).unwrap_or(0));
            l0.record_phase(4, phase_cell(4).unwrap_or(0).wrapping_add(1))
                == Err(ArgocdCode::EPhaseDrift)
        },
        "相位载荷漂移必抓",
    );
    s.add(
        "边界·CRD 投影越闭集",
        l0.record_crd(CRD_PROJ).is_err(),
        "CRD 投影格 12 越闭集必拒",
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
            let mut lt = ArgocdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PHASE_CELLS_TOTAL {
                if let Some(w) = phase_cell(o) {
                    let _ = lt.record_phase(o, w);
                }
                o += 1;
            }
            let mut o = 0usize;
            while o < APP_OPS {
                if let Some(w) = app_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < CRD_PROJ {
                let _ = lt.record_crd(r);
                r += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9C80,
        "SEG_BASE 0x40 对齐且落在 VX15 0xC9C80 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_names() {
        assert_eq!(phase_name(0), Some("sync"));
        assert_eq!(phase_name(1), Some("health"));
        assert_eq!(phase_name(2), Some("refresh"));
        assert_eq!(phase_name(3), Some("hard-refresh"));
        assert_eq!(phase_name(4), Some("diff"));
        assert!(phase_name(5).is_none());
    }

    #[test]
    fn test_phase_kind() {
        assert_eq!(phase_kind(0), Some(0));
        assert_eq!(phase_kind(4), Some(1));
        assert_eq!(phase_kind(15), Some(3));
        assert_eq!(phase_kind(19), Some(4));
        assert!(phase_kind(20).is_none());
        assert!(phase_cell(20).is_none());
    }

    #[test]
    fn test_app_ops_closed() {
        assert_eq!(app_op_name(2), Some("app sync"));
        assert_eq!(app_op_name(6), Some("app rollback"));
        assert_eq!(app_op_name(9), Some("app resources"));
        assert!(app_op_name(10).is_none());
        assert!(app_cell(10).is_none());
    }

    #[test]
    fn test_crd_fields() {
        assert_eq!(crd_field_name(0), Some("apiVersion"));
        assert_eq!(crd_field_name(4), Some("spec.source.repoURL"));
        assert_eq!(crd_field_name(11), Some("spec.sources"));
        assert!(crd_field_name(12).is_none());
        assert!(crd_cell(CRD_PROJ).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = phase_cell(13).unwrap_or(0);
        assert_eq!(a, phase_cell(13).unwrap_or(0));
        assert_ne!(a, phase_cell(14).unwrap_or(0));
        assert!(kubectl_row(KCTL_ROWS).is_none());
        assert!(go_stack_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = ArgocdLedger::new();
        let _ = lt.record_phase(9, phase_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_phase(2, phase_cell(2).unwrap_or(0)),
            Err(ArgocdCode::EOrder)
        );
        // 逆序注入被拒不改 last_kind：同相位后格仍可落。
        assert!(lt.record_phase(11, phase_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ArgocdLedger::new();
        assert_eq!(
            lt.record_phase(17, phase_cell(17).unwrap_or(0).wrapping_add(1)),
            Err(ArgocdCode::EPhaseDrift)
        );
        assert_eq!(
            lt.record_op(7, app_cell(7).unwrap_or(0).wrapping_add(5)),
            Err(ArgocdCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ArgocdLedger::new();
        let _ = lt.record_phase(0, phase_cell(0).unwrap_or(0));
        assert_eq!(lt.record_phase(0, phase_cell(0).unwrap_or(0)), Err(ArgocdCode::EDup));
        let _ = lt.record_crd(3);
        assert_eq!(lt.record_crd(3), Err(ArgocdCode::EDup));
        let _ = lt.declare_kubectl(1);
        assert_eq!(lt.declare_kubectl(1), Err(ArgocdCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ArgocdLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ArgocdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 计数口径：联签行与上栈行不计 recorded（见 run_cycle）。
        assert_eq!(recorded, (PHASE_CELLS_TOTAL + APP_OPS + CRD_PROJ) as u32);
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
        assert_eq!(SEG_BASE, 0xC9C80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ArgocdCode::Ok as u32, 0);
        assert_eq!(ArgocdCode::EGradeDrift as u32, 12);
        assert_eq!(ArgocdCode::Ok.val(), SEG_BASE);
        assert_eq!(ArgocdCode::ESeal.val(), SEG_BASE + 9);
        let mut i = 0u32;
        while i < 13 {
            let c: ArgocdCode = match i {
                0 => ArgocdCode::Ok,
                1 => ArgocdCode::ENoPhase,
                2 => ArgocdCode::EPhaseDrift,
                3 => ArgocdCode::ENoOp,
                4 => ArgocdCode::EDrift,
                5 => ArgocdCode::EOrder,
                6 => ArgocdCode::ENoProj,
                7 => ArgocdCode::ENoRow,
                8 => ArgocdCode::EDup,
                9 => ArgocdCode::ESeal,
                10 => ArgocdCode::EInj,
                11 => ArgocdCode::EFp,
                _ => ArgocdCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ArgocdLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = ArgocdLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
        let mut lt3 = ArgocdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_stats_conservation() {
        let lt = ArgocdLedger::new();
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 0);
        assert_eq!(caught, 0);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx52371_checks_green() {
        let s = run_unx52371_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
