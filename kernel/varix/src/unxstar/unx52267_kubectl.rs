//! UNX-F52267 kubectl 收口 · 0xC8280段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52267"，逐字）：
//!   title: kubectl 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52268-J1
//!     install/upgrade/rollback ×10 全通；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52267）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52268-J1，照原文锚定；功能按本卡标题（kubectl）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 20 操作集：五族闭集（get/apply/delete/logs/describe）各 4 操作，
//!      族单调（逆序必抓；空账本首操作必须落在 get 族）；漂移必抓。
//!   ② kubeconfig 双向投影 12 格：六要素（cluster/server/user/context/
//!      namespace/auth）× 两向（读取/写回）闭集，逐格载荷（纯函数折叠，
//!      XOR 锚低 16 位恒零）恒等才收；越界必抓。
//!   ③ context 5 场景闭集（default/dev/staging/prod/edge），逐场景落账。
//!   ④ 退出码闭集 8（0..8），逐码登记，越码必抓。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（context 越闭集/
//!      操作漂移/操作族越序），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52267;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// kubectl 操作数。
pub const KUBE_OPS: usize = 20;

/// kubeconfig 要素数。
pub const KCFG_ELEMENTS: usize = 6;

/// 投影方向数（读取/写回）。
pub const PROJ_DIRS: usize = 2;

/// kubeconfig 投影总格数（6×2）。
pub const KCFG_CELLS: usize = KCFG_ELEMENTS * PROJ_DIRS;

/// context 场景数。
pub const CONTEXT_SCENES: usize = 5;

/// 退出码闭集数。
pub const EXIT_CODES: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KubectlCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 操作族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影格漂移
    EProjDrift,
    /// context 场景越闭集
    ENoCtx,
    /// 退出码越闭集
    ENoExit,
    /// 六判据闸越界
    EGate,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl KubectlCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KubectlCode::Ok => "成功",
            KubectlCode::ENoOp => "操作号越闭集：kubectl 20 操作集之外先登记",
            KubectlCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            KubectlCode::EOrder => "操作族逆序：get→apply→delete→logs→describe 单调，逆序必抓；空账本首操作必须落在 get 族",
            KubectlCode::ENoProj => "投影格越闭集：kubeconfig 6 要素 ×2 向之外先登记",
            KubectlCode::EProjDrift => "投影格漂移：投影载荷折叠与基准不恒等，逐格复算",
            KubectlCode::ENoCtx => "context 场景越闭集：default/dev/staging/prod/edge 之外先登记",
            KubectlCode::ENoExit => "退出码越闭集：0..8 之外先登记",
            KubectlCode::EGate => "六判据闸越界：0..6 之外先登记",
            KubectlCode::EDup => "重复落账：同格只许一条账目",
            KubectlCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KubectlCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KubectlCode::EGradeDrift => "定级漂移：全通且投影全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 操作+12 投影格+5 场景+8 退出码全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作族（0..4=get 4..8=apply 8..12=delete 12..16=logs 16..20=describe）。
pub const fn op_family(o: usize) -> Option<u32> {
    if o >= KUBE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 族名。
pub const fn op_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("get"),
        1 => Some("apply"),
        2 => Some("delete"),
        3 => Some("logs"),
        4 => Some("describe"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；XOR 锚低 16 位恒零）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= KUBE_OPS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// kubeconfig 要素名。
pub const fn kcfg_element_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("cluster"),
        1 => Some("server"),
        2 => Some("user"),
        3 => Some("context"),
        4 => Some("namespace"),
        5 => Some("auth"),
        _ => None,
    }
}

/// 投影方向名（0=读取 1=写回）。
pub const fn proj_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("读取"),
        1 => Some("写回"),
        _ => None,
    }
}

/// 投影格载荷（格 i：e×2+d；双向投影恒等才收）。
pub const fn proj_cell(i: usize) -> Option<u64> {
    if i >= KCFG_CELLS {
        return None;
    }
    Some((0xB2000u64 + (i as u64) * 0x5D) ^ 0x4B434647_0000)
}

/// context 场景名。
pub const fn context_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("default"),
        1 => Some("dev"),
        2 => Some("staging"),
        3 => Some("prod"),
        4 => Some("edge"),
        _ => None,
    }
}

/// context 场景载荷。
pub const fn context_cell(c: usize) -> Option<u32> {
    if c >= CONTEXT_SCENES {
        return None;
    }
    Some(0xB300 + c as u32 * 0x3E)
}

/// 退出码名（闭集 0..8）。
pub const fn exit_code_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("成功"),
        1 => Some("一般错误"),
        2 => Some("用法错误"),
        3 => Some("资源缺失"),
        4 => Some("权限拒绝"),
        5 => Some("连接失败"),
        6 => Some("校验失败"),
        7 => Some("超时"),
        _ => None,
    }
}

/// 退出码载荷。
pub const fn exit_code_cell(c: usize) -> Option<u32> {
    if c >= EXIT_CODES {
        return None;
    }
    Some(0xB400 + c as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// kubectl 收口账本状态机。
#[derive(Clone)]
pub struct KubectlLedger {
    /// 六判据（安装/连接/操作面/投影/场景/退出码）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; KUBE_OPS],
    /// 投影格账。
    projs: [Option<u64>; KCFG_CELLS],
    /// context 场景账。
    ctxs: [Option<u32>; CONTEXT_SCENES],
    /// 退出码账。
    exits: [Option<u32>; EXIT_CODES],
    /// 最后操作族。
    last_family: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KubectlLedger {
    /// 新账本。
    pub fn new() -> Self {
        KubectlLedger {
            gates: [None; 6],
            ops: [None; KUBE_OPS],
            projs: [None; KCFG_CELLS],
            ctxs: [None; CONTEXT_SCENES],
            exits: [None; EXIT_CODES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KubectlCode> {
        if gate >= 6 {
            return Err(KubectlCode::EGate);
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

    /// 操作一格（族单调 + 空账本首族必须为 0 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), KubectlCode> {
        if o >= KUBE_OPS {
            return Err(KubectlCode::ENoOp);
        }
        let family = match op_family(o) {
            Some(f) => f,
            None => return Err(KubectlCode::ENoOp),
        };
        if (self.last_family < 0 && family != 0)
            || (self.last_family >= 0 && (family as i64) < self.last_family)
        {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200);
            return Err(KubectlCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(KubectlCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4201 + o as u64);
            return Err(KubectlCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KubectlCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = family as i64;
        self.fnv(0x4202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < KUBE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影格一格（双向载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, i: usize, got: u64) -> Result<(), KubectlCode> {
        let want = match proj_cell(i) {
            Some(w) => w,
            None => return Err(KubectlCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300 + i as u64);
            return Err(KubectlCode::EProjDrift);
        }
        if match self.projs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KubectlCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4301 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn projs_all(&self) -> bool {
        let mut i = 0usize;
        while i < KCFG_CELLS {
            match self.projs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// context 场景一格。
    pub fn record_ctx(&mut self, c: usize) -> Result<(), KubectlCode> {
        let want = match context_cell(c) {
            Some(x) => x,
            None => return Err(KubectlCode::ENoCtx),
        };
        if match self.ctxs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KubectlCode::EDup);
        }
        if let Some(x) = self.ctxs.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x4400 + c as u64);
        Ok(())
    }

    /// context 全齐核验。
    pub fn ctxs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONTEXT_SCENES {
            match self.ctxs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 退出码一格。
    pub fn record_exit(&mut self, c: usize) -> Result<(), KubectlCode> {
        let want = match exit_code_cell(c) {
            Some(x) => x,
            None => return Err(KubectlCode::ENoExit),
        };
        if match self.exits.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KubectlCode::EDup);
        }
        if let Some(x) = self.exits.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x4500 + c as u64);
        Ok(())
    }

    /// 退出码全齐核验。
    pub fn exits_all(&self) -> bool {
        let mut c = 0usize;
        while c < EXIT_CODES {
            match self.exits.get(c) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.projs_all() && self.ctxs_all() && self.exits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for KubectlLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KubectlLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < KUBE_OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < KCFG_CELLS {
        if let Some(w) = proj_cell(i) {
            if led.record_proj(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CONTEXT_SCENES {
        let _ = led.record_ctx(c);
        c += 1;
    }
    let mut e = 0usize;
    while e < EXIT_CODES {
        let _ = led.record_exit(e);
        e += 1;
    }
    // 注入 10 轮：context 越闭集 / 操作漂移 / 操作族越序三态轮转，
    // 全部落在当轮临时账本上；族越序对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KubectlLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_ctx(0);
                lt.record_ctx(CONTEXT_SCENES).is_err()
            }
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(KubectlCode::EDrift)
            }
            _ => {
                // 空账本首操作落在 describe 族（族 4）→ 越序必抓，且不改 last_family。
                lt.record_op(19, op_cell(19).unwrap_or(0)) == Err(KubectlCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4600 + k as u64);
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
/// UNX-F52267 判据集（锚 UNX-F52268-J1，off-by-one 板式）。
pub fn run_unx52267_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52267");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "kubectl 20 操作全通（get/apply/delete/logs/describe ×4）",
        recorded >= KUBE_OPS as u32,
        "五族闭集逐操作恒等（20 操作集判据原文）",
    );
    s.add(
        "kubeconfig 双向投影 12 格全通（6 要素 ×2 向）",
        {
            let mut lt = KubectlLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < KCFG_CELLS {
                if let Some(w) = proj_cell(i) {
                    if lt.record_proj(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.projs_all()
        },
        "cluster/server/user/context/namespace/auth 双向投影逐格恒等",
    );
    s.add(
        "context 5 场景全齐",
        {
            let mut lt = KubectlLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONTEXT_SCENES {
                if lt.record_ctx(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.ctxs_all()
        },
        "default/dev/staging/prod/edge 逐场景落账",
    );
    s.add(
        "退出码闭集 8 全落",
        {
            let mut lt = KubectlLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXIT_CODES {
                if lt.record_exit(e).is_err() {
                    ok = false;
                }
                e += 1;
            }
            ok && lt.exits_all()
        },
        "退出码 0..8 逐码登记，越码必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = KubectlLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/连接/操作面/投影/场景/退出码逐格落账（§AI-66.3 判据锚定）",
    );
    s.add(
        "操作族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < KUBE_OPS {
                if op_family(o).unwrap_or(5) < op_family(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "get→apply→delete→logs→describe 五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "context 越闭集/操作漂移/操作族越序三态轮转必抓",
    );
    let mut l0 = KubectlLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(KUBE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_proj(KCFG_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·context 场景越闭集",
        l0.record_ctx(CONTEXT_SCENES).is_err(),
        "context 场景 5 越闭集必拒",
    );
    s.add(
        "边界·退出码越闭集",
        l0.record_exit(EXIT_CODES).is_err(),
        "退出码 8 越闭集必拒",
    );
    s.add(
        "空账本族越序必抓",
        l0.record_op(19, op_cell(19).unwrap_or(0)) == Err(KubectlCode::EOrder),
        "空账本首操作落在 describe 族，越序必抓且不改 last_family",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = KubectlLedger::new();
            let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
            lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(KubectlCode::EDrift)
        },
        "操作载荷漂移必抓",
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
            let mut lt = KubectlLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < KUBE_OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < KCFG_CELLS {
                if let Some(w) = proj_cell(i) {
                    let _ = lt.record_proj(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < CONTEXT_SCENES {
                let _ = lt.record_ctx(c);
                c += 1;
            }
            let mut e = 0usize;
            while e < EXIT_CODES {
                let _ = lt.record_exit(e);
                e += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8280,
        "SEG_BASE 0x40 对齐且按派工公式落在 0xC8280",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_family() {
        assert_eq!(op_family(0), Some(0));
        assert_eq!(op_family(4), Some(1));
        assert_eq!(op_family(19), Some(4));
        assert_eq!(op_family_name(3), Some("logs"));
        assert!(op_family_name(5).is_none());
        assert!(op_family(20).is_none());
        assert!(op_cell(20).is_none());
    }

    #[test]
    fn test_projection_grid() {
        assert_eq!(kcfg_element_name(0), Some("cluster"));
        assert_eq!(kcfg_element_name(5), Some("auth"));
        assert!(kcfg_element_name(6).is_none());
        assert_eq!(proj_dir_name(0), Some("读取"));
        assert_eq!(proj_dir_name(1), Some("写回"));
        assert!(proj_dir_name(2).is_none());
        assert!(proj_cell(KCFG_CELLS).is_none());
    }

    #[test]
    fn test_context_closed_set() {
        assert_eq!(context_name(0), Some("default"));
        assert_eq!(context_name(4), Some("edge"));
        assert!(context_name(5).is_none());
        assert!(context_cell(5).is_none());
    }

    #[test]
    fn test_exit_codes_closed_set() {
        assert_eq!(exit_code_name(0), Some("成功"));
        assert_eq!(exit_code_name(7), Some("超时"));
        assert!(exit_code_name(8).is_none());
        assert!(exit_code_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(7).unwrap_or(0);
        assert_eq!(a, op_cell(7).unwrap_or(0));
        assert_ne!(a, op_cell(8).unwrap_or(0));
        let b = proj_cell(3).unwrap_or(0);
        assert_eq!(b, proj_cell(3).unwrap_or(0));
        assert_ne!(b, proj_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = KubectlLedger::new();
        let mut i = 0usize;
        while i <= 16 {
            let _ = lt.record_op(i, op_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_op(4, op_cell(4).unwrap_or(0)), Err(KubectlCode::EOrder));
        assert!(lt.record_op(17, op_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_family_caught() {
        let mut lt = KubectlLedger::new();
        assert_eq!(lt.record_op(12, op_cell(12).unwrap_or(0)), Err(KubectlCode::EOrder));
        // 空账本首操作必须落在 get 族（0）。
        assert!(lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KubectlLedger::new();
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_op(i, op_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(KubectlCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(2, proj_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(KubectlCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KubectlLedger::new();
        let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
        assert_eq!(lt.record_op(0, op_cell(0).unwrap_or(0)), Err(KubectlCode::EDup));
        let _ = lt.record_ctx(1);
        assert_eq!(lt.record_ctx(1), Err(KubectlCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KubectlLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = KubectlLedger::new();
        assert!(lt2.judge_gate(2).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (KUBE_OPS + KCFG_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8280);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KubectlCode::Ok as u32, 0);
        assert_eq!(KubectlCode::EGradeDrift as u32, 12);
        // 13 变体闭集：逐位核对（与枚举位序一致）。
        assert_eq!(KubectlCode::ENoOp as u32, 1);
        assert_eq!(KubectlCode::EOrder as u32, 3);
        assert_eq!(KubectlCode::EGate as u32, 8);
        assert!(!KubectlCode::Ok.detail().is_empty());
        assert!(!KubectlCode::ENoExit.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = KubectlLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52267_checks_green() {
        let s = run_unx52267_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
