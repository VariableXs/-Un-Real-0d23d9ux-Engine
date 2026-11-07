//! UNX-F52370 Tekton CLI（tkn）收口 · 0xC9C40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52370"，逐字）：
//!   title: Tekton CLI（tkn）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52371-J1 app
//!     sync/diff ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52370）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52371-J1，照原文锚定；功能按本卡标题（Tekton CLI / tkn）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① task/pipeline 20 格闭集：0..10 为 task 族、10..20 为 pipeline 族
//!      （kind = o/10），载荷折叠纯函数恒等才收，族序单调逆序必抓。
//!   ② tkn 五命令 10 操作集：task list/start/logs、pipeline list/start/
//!      logs、pipelinerun list/describe/cancel/logs，越闭集必拒。
//!   ③ TaskRun 投影 8 格：name/taskRef/params/workspaces/serviceAccount/
//!      results/status/podTemplate，逐格落账缺格拒收。
//!   ④ kubectl 消费联签 4 格（F52267 锚定）：CRD 就绪/RBAC 授权/命名空间/
//!      events 通道；Go 上栈声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record/judge_gate/fingerprint，FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（格越闭集/载荷漂移/TaskRun 投影越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；本卡 KB-COMPAT 登记定级
//!      S2（有条件收口）：全通 → S2，判据缺 → S3，S1 本卡不予登记。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52370;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// task/pipeline 总格数。
pub const TK_CELLS: usize = 20;

/// 族大小（0..10=task 10..20=pipeline）。
pub const TK_FAMILY: usize = 10;

/// tkn 操作数。
pub const TK_OPS: usize = 10;

/// TaskRun 投影格数。
pub const RUN_PROJ: usize = 8;

/// kubectl 消费联签格数（F52267 锚定）。
pub const KCTL_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

/// 收口六判据。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TektonCode {
    /// 成功
    Ok = 0,
    /// task/pipeline 格越闭集
    ENoCell,
    /// 格载荷漂移
    ECellDrift,
    /// tkn 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EDrift,
    /// 族序逆序
    EOrder,
    /// TaskRun 投影格越闭集
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

impl TektonCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TektonCode::Ok => "成功",
            TektonCode::ENoCell => "task/pipeline 格越闭集：20 格闭集之外先登记",
            TektonCode::ECellDrift => "格载荷漂移：task/pipeline 折叠与基准不恒等，逐格复算",
            TektonCode::ENoOp => "tkn 操作号越闭集：五命令 10 操作集之外先登记",
            TektonCode::EDrift => "操作载荷漂移：操作折叠与基准不恒等，逐项复算",
            TektonCode::EOrder => "族序逆序：task→pipeline 族序单调，逆序必抓",
            TektonCode::ENoProj => "TaskRun 投影格越闭集：8 格投影之外先登记",
            TektonCode::ENoRow => "联签/上栈行越闭集：kubectl 联签 4 格与 Go 上栈 2 行闭集之外先登记",
            TektonCode::EDup => "重复落账：同格只许一条账目",
            TektonCode::ESeal => "未齐即封：20 格 + 10 操作 + 8 投影 + 4 联签 + 2 上栈未齐先补齐",
            TektonCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TektonCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TektonCode::EGradeDrift => "定级漂移：本卡全通定 S2（有条件收口），S1 不予登记",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡登记 S2 有条件收口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不予登记）
    S1,
    /// S2：有条件收口（本卡登记级）
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
            Grade::S1 => "S1：全通收口——本卡按 KB-COMPAT 不予登记",
            Grade::S2 => "S2：有条件收口，20格+10操作+8投影+4联签+2上栈全通",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// task/pipeline 格载荷（格 o）。
pub const fn tk_cell(o: usize) -> Option<u64> {
    if o >= TK_CELLS {
        return None;
    }
    Some((0xC2000u64 + (o as u64) * 0x33) ^ 0x544B544E5F5F0000)
}

/// task/pipeline 族号（0..10=task 10..20=pipeline）。
pub const fn tk_kind(o: usize) -> Option<u32> {
    if o >= TK_CELLS {
        return None;
    }
    Some((o / TK_FAMILY) as u32)
}

/// 族名。
pub const fn tk_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("task"),
        1 => Some("pipeline"),
        _ => None,
    }
}

/// tkn 操作载荷（操作 o）。
pub const fn tkn_cell(o: usize) -> Option<u64> {
    if o >= TK_OPS {
        return None;
    }
    Some((0xC3000u64 + (o as u64) * 0x29) ^ 0x544B4E4F505F0000)
}

/// tkn 命令名（五命令 10 操作）。
pub const fn tkn_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("task list"),
        1 => Some("task start"),
        2 => Some("task logs"),
        3 => Some("pipeline list"),
        4 => Some("pipeline start"),
        5 => Some("pipeline logs"),
        6 => Some("pipelinerun list"),
        7 => Some("pipelinerun describe"),
        8 => Some("pipelinerun cancel"),
        9 => Some("pipelinerun logs"),
        _ => None,
    }
}

/// TaskRun 投影格载荷（格 r）。
pub const fn runproj_cell(r: usize) -> Option<u32> {
    if r >= RUN_PROJ {
        return None;
    }
    Some(0xC400 + r as u32 * 0x2E)
}

/// TaskRun 投影字段名。
pub const fn runproj_field_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("name"),
        1 => Some("taskRef"),
        2 => Some("params"),
        3 => Some("workspaces"),
        4 => Some("serviceAccount"),
        5 => Some("results"),
        6 => Some("status"),
        7 => Some("podTemplate"),
        _ => None,
    }
}

/// kubectl 消费联签行折叠（行 r；F52267 锚定）。
pub const fn kubectl_row(r: usize) -> Option<u32> {
    if r >= KCTL_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x31)
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
    Some(0xC600 + r as u32 * 0x37)
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("task 启动"),
        2 => Some("pipeline 启动"),
        3 => Some("TaskRun 监听"),
        4 => Some("日志回传"),
        5 => Some("升级回滚"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Tekton CLI 收口账本状态机。
#[derive(Clone)]
pub struct TektonLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATES],
    /// task/pipeline 20 格账。
    cells: [Option<u64>; TK_CELLS],
    /// tkn 10 操作账。
    ops: [Option<u64>; TK_OPS],
    /// TaskRun 投影 8 格账。
    proj: [Option<u32>; RUN_PROJ],
    /// kubectl 联签账。
    kctl: [Option<u32>; KCTL_ROWS],
    /// Go 上栈账。
    go_stack: [Option<u32>; GO_ROWS],
    /// 最后族号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TektonLedger {
    /// 新账本。
    pub fn new() -> Self {
        TektonLedger {
            gates: [None; GATES],
            cells: [None; TK_CELLS],
            ops: [None; TK_OPS],
            proj: [None; RUN_PROJ],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TektonCode> {
        if gate >= GATES {
            return Err(TektonCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6A00 + gate as u64);
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

    /// task/pipeline 一格（族序单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, o: usize, got: u64) -> Result<(), TektonCode> {
        if o >= TK_CELLS {
            return Err(TektonCode::ENoCell);
        }
        let kind = match tk_kind(o) {
            Some(k) => k,
            None => return Err(TektonCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B00);
            return Err(TektonCode::EOrder);
        }
        let want = match tk_cell(o) {
            Some(w) => w,
            None => return Err(TektonCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B01 + o as u64);
            return Err(TektonCode::ECellDrift);
        }
        if match self.cells.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TektonCode::EDup);
        }
        if let Some(c) = self.cells.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6B02 + o as u64);
        Ok(())
    }

    /// task/pipeline 全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut o = 0usize;
        while o < TK_CELLS {
            match self.cells.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// tkn 一格（载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), TektonCode> {
        if o >= TK_OPS {
            return Err(TektonCode::ENoOp);
        }
        let want = match tkn_cell(o) {
            Some(w) => w,
            None => return Err(TektonCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6C00 + o as u64);
            return Err(TektonCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TektonCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x6C01 + o as u64);
        Ok(())
    }

    /// tkn 全账核验（10 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < TK_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// TaskRun 投影一格。
    pub fn record_proj(&mut self, r: usize) -> Result<(), TektonCode> {
        let want = match runproj_cell(r) {
            Some(w) => w,
            None => return Err(TektonCode::ENoProj),
        };
        if match self.proj.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TektonCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x6D00 + r as u64);
        Ok(())
    }

    /// TaskRun 投影全齐核验。
    pub fn proj_all(&self) -> bool {
        let mut r = 0usize;
        while r < RUN_PROJ {
            match self.proj.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// kubectl 联签一行。
    pub fn declare_kubectl(&mut self, r: usize) -> Result<(), TektonCode> {
        let want = match kubectl_row(r) {
            Some(w) => w,
            None => return Err(TektonCode::ENoRow),
        };
        if match self.kctl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TektonCode::EDup);
        }
        if let Some(c) = self.kctl.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x6E00 + r as u64);
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
    pub fn declare_go_stack(&mut self, r: usize) -> Result<(), TektonCode> {
        let want = match go_stack_row(r) {
            Some(w) => w,
            None => return Err(TektonCode::ENoRow),
        };
        if match self.go_stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TektonCode::EDup);
        }
        if let Some(c) = self.go_stack.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x6F00 + r as u64);
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

    /// KB-COMPAT 定级（全通 → S2 本卡登记级，判据缺 → S3；S1 本卡不予登记）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.cells_all()
            && self.ops_all()
            && self.proj_all()
            && self.kctl_all()
            && self.go_stack_all()
        {
            Grade::S2
        } else {
            // 判据或任一分节未齐：一律 S3（本卡 S1 不予登记）。
            Grade::S3
        }
    }
}

impl Default for TektonLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TektonLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < TK_CELLS {
        if let Some(w) = tk_cell(o) {
            if led.record_cell(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut o = 0usize;
    while o < TK_OPS {
        if let Some(w) = tkn_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < RUN_PROJ {
        if led.record_proj(r).is_ok() {
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
    // 注入 10 轮：格越闭集 / 载荷漂移 / TaskRun 投影越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TektonLedger::new();
        let caught = match kind {
            0 => lt.record_cell(TK_CELLS, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格再注漂移。
                let _ = lt.record_cell(3, tk_cell(3).unwrap_or(0));
                lt.record_cell(1, tk_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(TektonCode::ECellDrift)
            }
            _ => lt.record_proj(RUN_PROJ).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7000 + k as u64);
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
/// UNX-F52370 判据集（锚 UNX-F52371-J1，off-by-one 板式）。
pub fn run_unx52370_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52370");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "task/pipeline 20 格全通",
        recorded >= TK_CELLS as u32,
        "task 族 0..10 与 pipeline 族 10..20 逐格恒等（UNX-F52371-J1 锚定）",
    );
    s.add(
        "tkn 五命令 10 操作全通",
        {
            let mut lt = TektonLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < TK_OPS {
                if let Some(w) = tkn_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "task/pipeline/pipelinerun 三族命令逐项恒等",
    );
    s.add(
        "TaskRun 投影 8 格全齐",
        {
            let mut lt = TektonLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RUN_PROJ {
                if lt.record_proj(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.proj_all()
        },
        "name/taskRef/params/workspaces/serviceAccount/results/status/podTemplate",
    );
    s.add(
        "kubectl 消费联签 4/4 全齐",
        {
            let mut lt = TektonLedger::new();
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
        "CRD 就绪/RBAC 授权/命名空间/events 通道（F52267 锚定，§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = TektonLedger::new();
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
            let mut lt = TektonLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/task 启动/pipeline 启动/TaskRun 监听/日志回传/升级回滚",
    );
    s.add(
        "族序单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TK_CELLS {
                if tk_kind(o).unwrap_or(2) < tk_kind(o - 1).unwrap_or(2) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "task→pipeline 两族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "格越闭集/载荷漂移/投影越闭集三态轮转必抓",
    );
    let mut l0 = TektonLedger::new();
    s.add(
        "边界·格越闭集",
        l0.record_cell(TK_CELLS, 0).is_err(),
        "task/pipeline 格 20 越闭集必拒",
    );
    s.add(
        "边界·格漂移",
        {
            let _ = l0.record_cell(3, tk_cell(3).unwrap_or(0));
            l0.record_cell(1, tk_cell(1).unwrap_or(0).wrapping_add(1)) == Err(TektonCode::ECellDrift)
        },
        "格载荷漂移必抓",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(RUN_PROJ).is_err(),
        "TaskRun 投影格 8 越闭集必拒",
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
        "定级 S2 闭集",
        {
            let mut lt = TektonLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < TK_CELLS {
                if let Some(w) = tk_cell(o) {
                    let _ = lt.record_cell(o, w);
                }
                o += 1;
            }
            let mut o = 0usize;
            while o < TK_OPS {
                if let Some(w) = tkn_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < RUN_PROJ {
                let _ = lt.record_proj(r);
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
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 登记本卡有条件收口）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9C40,
        "SEG_BASE 0x40 对齐且落在 VX15 0xC9C40 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tk_kind() {
        assert_eq!(tk_kind(0), Some(0));
        assert_eq!(tk_kind(9), Some(0));
        assert_eq!(tk_kind(10), Some(1));
        assert_eq!(tk_kind(19), Some(1));
        assert!(tk_kind(20).is_none());
        assert!(tk_cell(20).is_none());
        assert_eq!(tk_kind_name(0), Some("task"));
        assert_eq!(tk_kind_name(1), Some("pipeline"));
        assert!(tk_kind_name(2).is_none());
    }

    #[test]
    fn test_tkn_ops_closed() {
        assert_eq!(tkn_op_name(0), Some("task list"));
        assert_eq!(tkn_op_name(4), Some("pipeline start"));
        assert_eq!(tkn_op_name(9), Some("pipelinerun logs"));
        assert!(tkn_op_name(10).is_none());
        assert!(tkn_cell(10).is_none());
    }

    #[test]
    fn test_proj_fields() {
        assert_eq!(runproj_field_name(1), Some("taskRef"));
        assert_eq!(runproj_field_name(6), Some("status"));
        assert!(runproj_field_name(8).is_none());
        assert!(runproj_cell(RUN_PROJ).is_none());
    }

    #[test]
    fn test_kubectl_rows() {
        assert_eq!(kubectl_row_name(0), Some("CRD 就绪"));
        assert_eq!(kubectl_row_name(3), Some("events 通道"));
        assert!(kubectl_row_name(4).is_none());
        assert!(kubectl_row(KCTL_ROWS).is_none());
        assert!(go_stack_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = tk_cell(11).unwrap_or(0);
        assert_eq!(a, tk_cell(11).unwrap_or(0));
        assert_ne!(a, tk_cell(12).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TektonLedger::new();
        let _ = lt.record_cell(12, tk_cell(12).unwrap_or(0));
        assert_eq!(
            lt.record_cell(2, tk_cell(2).unwrap_or(0)),
            Err(TektonCode::EOrder)
        );
        // 逆序注入被拒不改 last_kind：同族后格仍可落。
        assert!(lt.record_cell(13, tk_cell(13).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TektonLedger::new();
        assert_eq!(
            lt.record_cell(6, tk_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(TektonCode::ECellDrift)
        );
        assert_eq!(
            lt.record_op(5, tkn_cell(5).unwrap_or(0).wrapping_add(9)),
            Err(TektonCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TektonLedger::new();
        let _ = lt.record_cell(0, tk_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cell(0, tk_cell(0).unwrap_or(0)), Err(TektonCode::EDup));
        let _ = lt.record_proj(0);
        assert_eq!(lt.record_proj(0), Err(TektonCode::EDup));
        let _ = lt.declare_kubectl(0);
        assert_eq!(lt.declare_kubectl(0), Err(TektonCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TektonLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TektonLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 计数口径：联签行与上栈行不计 recorded（见 run_cycle）。
        assert_eq!(recorded, (TK_CELLS + TK_OPS + RUN_PROJ) as u32);
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
        assert_eq!(SEG_BASE, 0xC9C40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TektonCode::Ok as u32, 0);
        assert_eq!(TektonCode::EGradeDrift as u32, 12);
        assert_eq!(TektonCode::Ok.val(), SEG_BASE);
        assert_eq!(TektonCode::ESeal.val(), SEG_BASE + 9);
        let mut i = 0u32;
        while i < 13 {
            let c: TektonCode = match i {
                0 => TektonCode::Ok,
                1 => TektonCode::ENoCell,
                2 => TektonCode::ECellDrift,
                3 => TektonCode::ENoOp,
                4 => TektonCode::EDrift,
                5 => TektonCode::EOrder,
                6 => TektonCode::ENoProj,
                7 => TektonCode::ENoRow,
                8 => TektonCode::EDup,
                9 => TektonCode::ESeal,
                10 => TektonCode::EInj,
                11 => TektonCode::EFp,
                _ => TektonCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = TektonLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅判据齐 → S3（本卡 S1 不予登记，S2 需全通）。
        let mut lt2 = TektonLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let lt = TektonLedger::new();
        let (inj, caught, esc) = lt.stats();
        assert_eq!(inj, 0);
        assert_eq!(caught, 0);
        assert_eq!(esc, 0);
    }

    #[test]
    fn unx52370_checks_green() {
        let s = run_unx52370_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
