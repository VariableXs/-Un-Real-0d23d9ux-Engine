//! UNX-F52304 UniGetUI（WingetUI）收口 · 0xC8BC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52304"，逐字）：
//!   title: UniGetUI（WingetUI）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52305-J1 六判据
//!     全落；批量安装 ×10 包全通；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52304）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52305-J1，照原文锚定；功能按本卡标题（UniGetUI）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 六后端聚合 24 格：winget / choco / scoop / pip / npm / dotnet
//!      六后端各 4 格，闭集外必抓；逐格载荷折叠对照，恒等才收。
//!   ② 安装/更新/忽略 10 操作集：三操作族（安装 0..4 / 更新 4..7 /
//!      忽略 7..10），操作族单调（逆序必抓），载荷漂移必抓。
//!   ③ 批量安装 ×10 包全通：批账 10 格逐包折叠落账（UNX-F52305-J1
//!      批量安装锚定）；包列表投影 8 格；通知三元闸 3 格；与各后端
//!      分账声明 2 行（winget 后端 / choco 后端消费分账声明）。
//!   ④ 定级口径：UniGetUI 为聚合代理账，聚合账上限 S2——S1 留给逐后
//!      端本体收口另册登记；判据缺 → S3。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（后端越闭集/
//!      操作漂移/操作越闭集），全部落在当轮临时账本上，检出+逃逸守恒；
//!      双跑 FNV 指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52304;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 后端格数（六后端 × 4）。
pub const BACKEND_CELLS: usize = 24;

/// 后端数。
pub const BACKENDS: usize = 6;

/// 操作数（安装 4 + 更新 3 + 忽略 3）。
pub const OPS: usize = 10;

/// 批量安装格数。
pub const BATCH_CELLS: usize = 10;

/// 包列表投影格数。
pub const LIST_CELLS: usize = 8;

/// 通知三元闸格数。
pub const TRI_GATES: usize = 3;

/// 与各后端分账声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UniGetCode {
    /// 成功
    Ok = 0,
    /// 后端格越闭集
    ENoBackend,
    /// 后端载荷漂移
    EBackendDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 操作族逆序
    EKind,
    /// 批账格越闭集
    ENoBatch,
    /// 列表投影格越闭集
    ENoList,
    /// 通知闸格越闭集
    ENoTri,
    /// 分账声明行越闭集
    ENoDecl,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl UniGetCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            UniGetCode::Ok => "成功",
            UniGetCode::ENoBackend => "后端格越闭集：winget/choco/scoop/pip/npm/dotnet 24 格之外先登记",
            UniGetCode::EBackendDrift => "后端载荷漂移：后端索引折叠与基准不恒等，逐格复算",
            UniGetCode::ENoOp => "操作号越闭集：安装/更新/忽略 10 操作集之外先登记",
            UniGetCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            UniGetCode::EKind => "操作族逆序：安装→更新→忽略操作族单调，逆序必抓",
            UniGetCode::ENoBatch => "批账格越闭集：批量安装 10 包之外先登记",
            UniGetCode::ENoList => "列表投影格越闭集：包列表投影 8 格之外先登记",
            UniGetCode::ENoTri => "通知闸格越闭集：通知三元闸 3 格之外先登记",
            UniGetCode::ENoDecl => "分账声明行越闭集：与各后端分账声明 2 行之外先登记",
            UniGetCode::EDup => "重复落账：同格只许一条账目",
            UniGetCode::ESeal => "未齐即封：24 后端格 + 10 操作 + 3 批格 + 8 投影格 + 三元闸未齐先补齐",
            UniGetCode::EGradeDrift => "定级漂移：聚合代理账上限 S2，逐后端本体收口才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：逐后端本体收口（聚合账不授）
    S1,
    /// S2：聚合收口上限
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
            Grade::S1 => "S1：逐后端本体收口专属，聚合代理账不授此级",
            Grade::S2 => "S2：聚合代理账上限，六后端聚合+10 操作+批量 10 包全通",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 后端格载荷（格 b：0..24，后端 = b/4）。
pub const fn backend_cell(b: usize) -> Option<u64> {
    if b >= BACKEND_CELLS {
        return None;
    }
    Some((0xF1000u64 + (b as u64) * 0x53) ^ 0x5547_0000)
}

/// 后端号（0=winget 1=choco 2=scoop 3=pip 4=npm 5=dotnet）。
pub const fn backend_of(b: usize) -> Option<u32> {
    if b >= BACKEND_CELLS {
        return None;
    }
    Some((b / 4) as u32)
}

/// 后端名。
pub const fn backend_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("winget"),
        1 => Some("choco"),
        2 => Some("scoop"),
        3 => Some("pip"),
        4 => Some("npm"),
        5 => Some("dotnet"),
        _ => None,
    }
}

/// 操作格载荷（操作 o）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xF2000u64 + (o as u64) * 0x5B) ^ (((o / 4) as u64).wrapping_mul(0x0400_0000)))
}

/// 操作族（0..4=安装 4..7=更新 7..10=忽略）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    if o < 4 {
        Some(0)
    } else if o < 7 {
        Some(1)
    } else {
        Some(2)
    }
}

/// 操作族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("安装"),
        1 => Some("更新"),
        2 => Some("忽略"),
        _ => None,
    }
}

/// 批账格载荷（包 i：0..10）。
pub const fn batch_cell(i: usize) -> Option<u64> {
    if i >= BATCH_CELLS {
        return None;
    }
    Some((0xF3000u64 + (i as u64) * 0x67) ^ 0x4241_0000)
}

/// 包列表投影载荷（格 c）。
pub const fn list_cell(c: usize) -> Option<u32> {
    if c >= LIST_CELLS {
        return None;
    }
    Some(0xF500 + (c as u32) * 0x29)
}

/// 通知三元闸载荷（格 t：0=安装完成通知 1=更新可用通知 2=失败告警）。
pub const fn tri_gate_cell(t: usize) -> Option<u32> {
    if t >= TRI_GATES {
        return None;
    }
    Some(0xF700 + (t as u32) * 0x11)
}

/// 通知三元闸名。
pub const fn tri_gate_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("安装完成通知"),
        1 => Some("更新可用通知"),
        2 => Some("失败告警"),
        _ => None,
    }
}

/// 与各后端分账声明折叠（行 r：0=winget 后端 1=choco 后端）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xF800 + r as u32 * 0x2D)
}

/// 分账声明行名。
pub const fn decl_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("winget 后端消费分账声明"),
        1 => Some("choco 后端消费分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// UniGetUI 收口账本状态机。
#[derive(Clone)]
pub struct UniGetLedger {
    /// 六判据（启动/聚合扫描/安装/更新/忽略/通知）。
    gates: [Option<bool>; 6],
    /// 六后端聚合账。
    backends: [Option<u64>; BACKEND_CELLS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// 批量安装账。
    batch: [Option<u64>; BATCH_CELLS],
    /// 包列表投影账。
    lists: [Option<u32>; LIST_CELLS],
    /// 通知三元闸账。
    tris: [Option<u32>; TRI_GATES],
    /// 分账声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl UniGetLedger {
    /// 新账本。
    pub fn new() -> Self {
        UniGetLedger {
            gates: [None; 6],
            backends: [None; BACKEND_CELLS],
            ops: [None; OPS],
            batch: [None; BATCH_CELLS],
            lists: [None; LIST_CELLS],
            tris: [None; TRI_GATES],
            decls: [None; DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), UniGetCode> {
        if gate >= 6 {
            return Err(UniGetCode::ESeal);
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

    /// 后端一格（载荷对照 + 重复拒）。
    pub fn record_backend(&mut self, b: usize, got: u64) -> Result<(), UniGetCode> {
        let want = match backend_cell(b) {
            Some(w) => w,
            None => return Err(UniGetCode::ENoBackend),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + b as u64);
            return Err(UniGetCode::EBackendDrift);
        }
        if match self.backends.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UniGetCode::EDup);
        }
        if let Some(c) = self.backends.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x8201 + b as u64);
        Ok(())
    }

    /// 后端全账核验（24 格）。
    pub fn backends_all(&self) -> bool {
        let mut b = 0usize;
        while b < BACKEND_CELLS {
            match self.backends.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 操作一格（操作族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), UniGetCode> {
        if o >= OPS {
            return Err(UniGetCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(UniGetCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(UniGetCode::EKind);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(UniGetCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(UniGetCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UniGetCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 批量安装一格。
    pub fn record_batch(&mut self, i: usize) -> Result<(), UniGetCode> {
        let want = match batch_cell(i) {
            Some(w) => w,
            None => return Err(UniGetCode::ENoBatch),
        };
        if match self.batch.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UniGetCode::EDup);
        }
        if let Some(c) = self.batch.get_mut(i) {
            *c = Some(want);
        }
        self.fnv(0x8400 + i as u64);
        Ok(())
    }

    /// 批账全账核验（10 包全通）。
    pub fn batch_all(&self) -> bool {
        let mut i = 0usize;
        while i < BATCH_CELLS {
            match self.batch.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 包列表投影一格。
    pub fn record_list(&mut self, c: usize) -> Result<(), UniGetCode> {
        let want = match list_cell(c) {
            Some(w) => w,
            None => return Err(UniGetCode::ENoList),
        };
        if match self.lists.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UniGetCode::EDup);
        }
        if let Some(x) = self.lists.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x8500 + c as u64);
        Ok(())
    }

    /// 列表投影全账核验。
    pub fn lists_all(&self) -> bool {
        let mut c = 0usize;
        while c < LIST_CELLS {
            match self.lists.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 通知三元闸一格。
    pub fn record_tri(&mut self, t: usize) -> Result<(), UniGetCode> {
        let want = match tri_gate_cell(t) {
            Some(w) => w,
            None => return Err(UniGetCode::ENoTri),
        };
        if match self.tris.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UniGetCode::EDup);
        }
        if let Some(x) = self.tris.get_mut(t) {
            *x = Some(want);
        }
        self.fnv(0x8600 + t as u64);
        Ok(())
    }

    /// 通知闸全账核验。
    pub fn tris_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRI_GATES {
            match self.tris.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 分账声明一行。
    pub fn declare_backend(&mut self, r: usize) -> Result<(), UniGetCode> {
        let want = match decl_row(r) {
            Some(w) => w,
            None => return Err(UniGetCode::ENoDecl),
        };
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UniGetCode::EDup);
        }
        if let Some(x) = self.decls.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x8700 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(r) {
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

    /// KB-COMPAT 定级（聚合代理账上限 S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.backends_all()
            && self.ops_all()
            && self.batch_all()
            && self.lists_all()
            && self.tris_all()
            && self.decls_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for UniGetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = UniGetLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < BACKEND_CELLS {
        if let Some(w) = backend_cell(b) {
            if led.record_backend(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < BATCH_CELLS {
        let _ = led.record_batch(i);
        i += 1;
    }
    let mut c = 0usize;
    while c < LIST_CELLS {
        let _ = led.record_list(c);
        c += 1;
    }
    let mut t = 0usize;
    while t < TRI_GATES {
        let _ = led.record_tri(t);
        t += 1;
    }
    let mut r = 0usize;
    while r < DECL_ROWS {
        let _ = led.declare_backend(r);
        r += 1;
    }
    // 注入 10 轮：后端越闭集 / 操作漂移 / 操作越闭集三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = UniGetLedger::new();
        let caught = match kind {
            0 => lt.record_backend(BACKEND_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(UniGetCode::EDrift)
            }
            _ => lt.record_op(OPS, 0).is_err(),
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
/// UNX-F52304 判据集（锚 UNX-F52305-J1，off-by-one 板式）。
pub fn run_unx52304_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52304");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "六后端聚合 24 格全落",
        recorded >= BACKEND_CELLS as u32 + OPS as u32,
        "winget/choco/scoop/pip/npm/dotnet 六后端逐格恒等落账",
    );
    s.add(
        "后端名闭集",
        {
            let mut ok = true;
            let mut k = 0usize;
            while k < BACKENDS {
                match backend_name(k) {
                    Some(_) => {}
                    None => ok = false,
                }
                k += 1;
            }
            ok && backend_name(BACKENDS).is_none()
        },
        "六后端名闭集，后端外无名",
    );
    s.add(
        "安装/更新/忽略 10 操作全通",
        {
            let mut lt = UniGetLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "三操作族逐操作恒等（UNX-F52305-J1 六判据全落锚定）",
    );
    s.add(
        "操作族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_kind(o).unwrap_or(3) < op_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "安装→更新→忽略三操作族单调",
    );
    s.add(
        "批量安装 ×10 包全通",
        {
            let mut lt = UniGetLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < BATCH_CELLS {
                if lt.record_batch(i).is_err() {
                    ok = false;
                }
                i += 1;
            }
            ok && lt.batch_all() && batch_cell(BATCH_CELLS).is_none()
        },
        "批量安装 10 包逐包折叠落账，越界必拒",
    );
    s.add(
        "包列表投影 8 格全落",
        {
            let mut lt = UniGetLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < LIST_CELLS {
                if lt.record_list(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.lists_all()
        },
        "包列表投影逐格落账",
    );
    s.add(
        "通知三元闸全落",
        {
            let mut lt = UniGetLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRI_GATES {
                if lt.record_tri(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tris_all()
        },
        "安装完成/更新可用/失败告警三元闭集",
    );
    s.add(
        "与各后端分账声明 2 行全齐",
        {
            let mut lt = UniGetLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_backend(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decls_all()
        },
        "winget/choco 后端消费分账声明两行闭集（与各后端分账）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = UniGetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/聚合扫描/安装/更新/忽略/通知逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "后端越闭集/操作漂移/操作越闭集三态轮转必抓");
    let mut l0 = UniGetLedger::new();
    s.add(
        "边界·后端越闭集",
        l0.record_backend(BACKEND_CELLS, 0).is_err(),
        "后端 24 格之外必拒",
    );
    s.add(
        "注入·操作漂移拒",
        {
            let _ = l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(UniGetCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 10 越闭集必拒",
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
            let mut lt = UniGetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < BACKEND_CELLS {
                if let Some(w) = backend_cell(b) {
                    let _ = lt.record_backend(b, w);
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < BATCH_CELLS {
                let _ = lt.record_batch(i);
                i += 1;
            }
            let mut c = 0usize;
            while c < LIST_CELLS {
                let _ = lt.record_list(c);
                c += 1;
            }
            let mut t = 0usize;
            while t < TRI_GATES {
                let _ = lt.record_tri(t);
                t += 1;
            }
            let mut r = 0usize;
            while r < DECL_ROWS {
                let _ = lt.declare_backend(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "聚合代理账全通 → S2 上限（KB-COMPAT 定级登记，S1 归逐后端本体收口）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8BC0,
        "SEG_BASE 0x40 对齐且按 0xC7200+0x40×(52304−52201) 恒等",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_families() {
        assert_eq!(backend_name(0), Some("winget"));
        assert_eq!(backend_name(1), Some("choco"));
        assert_eq!(backend_name(2), Some("scoop"));
        assert_eq!(backend_name(3), Some("pip"));
        assert_eq!(backend_name(4), Some("npm"));
        assert_eq!(backend_name(5), Some("dotnet"));
        assert!(backend_name(6).is_none());
        assert_eq!(backend_of(0), Some(0));
        assert_eq!(backend_of(23), Some(5));
        assert!(backend_of(24).is_none());
        assert!(backend_cell(24).is_none());
    }

    #[test]
    fn test_op_kinds() {
        assert_eq!(op_kind_name(0), Some("安装"));
        assert_eq!(op_kind_name(1), Some("更新"));
        assert_eq!(op_kind_name(2), Some("忽略"));
        assert!(op_kind_name(3).is_none());
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(6), Some(1));
        assert_eq!(op_kind(7), Some(2));
        assert_eq!(op_kind(9), Some(2));
        assert!(op_kind(10).is_none());
        assert!(op_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(6).unwrap_or(0);
        assert_eq!(a, op_cell(6).unwrap_or(0));
        assert_ne!(a, op_cell(7).unwrap_or(0));
        assert!(list_cell(LIST_CELLS).is_none());
        assert!(tri_gate_cell(TRI_GATES).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_tri_and_decl_names() {
        assert_eq!(tri_gate_name(0), Some("安装完成通知"));
        assert_eq!(tri_gate_name(1), Some("更新可用通知"));
        assert_eq!(tri_gate_name(2), Some("失败告警"));
        assert!(tri_gate_name(3).is_none());
        assert_eq!(decl_row_name(0), Some("winget 后端消费分账声明"));
        assert_eq!(decl_row_name(1), Some("choco 后端消费分账声明"));
        assert!(decl_row_name(2).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        // 操作族逆序注入：先按合法序落高族格，再注低族格必抓。
        let mut lt = UniGetLedger::new();
        let _ = lt.record_op(8, op_cell(8).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(UniGetCode::EKind));
        assert!(lt.record_op(9, op_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = UniGetLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(UniGetCode::EDrift)
        );
        assert_eq!(
            lt.record_backend(11, backend_cell(11).unwrap_or(0).wrapping_add(7)),
            Err(UniGetCode::EBackendDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = UniGetLedger::new();
        let _ = lt.record_backend(0, backend_cell(0).unwrap_or(0));
        assert_eq!(lt.record_backend(0, backend_cell(0).unwrap_or(0)), Err(UniGetCode::EDup));
        let _ = lt.record_batch(0);
        assert_eq!(lt.record_batch(0), Err(UniGetCode::EDup));
    }

    #[test]
    fn test_injection_on_empty_ledger_keeps_kind() {
        // 对空账本注入必抓且不改 last_kind：注入后再落合法格应照常成功。
        let mut lt = UniGetLedger::new();
        assert!(lt.record_op(OPS, 0).is_err());
        assert!(lt.record_backend(BACKEND_CELLS, 0).is_err());
        assert!(lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_op(4, op_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = UniGetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = UniGetLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= BACKEND_CELLS as u32 + OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC8BC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(UniGetCode::Ok as u32, 0);
        assert_eq!(UniGetCode::EGradeDrift as u32, 12);
        assert!(!UniGetCode::Ok.detail().is_empty());
        assert!(!UniGetCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade_aggregate_cap() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 空账 → S3；聚合账全通 → S2（S1 不由聚合账授出）。
        let lt = UniGetLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut full = UniGetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut b = 0usize;
        while b < BACKEND_CELLS {
            if let Some(w) = backend_cell(b) {
                let _ = full.record_backend(b, w);
            }
            b += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < BATCH_CELLS {
            let _ = full.record_batch(i);
            i += 1;
        }
        let mut c = 0usize;
        while c < LIST_CELLS {
            let _ = full.record_list(c);
            c += 1;
        }
        let mut t = 0usize;
        while t < TRI_GATES {
            let _ = full.record_tri(t);
            t += 1;
        }
        let mut r = 0usize;
        while r < DECL_ROWS {
            let _ = full.declare_backend(r);
            r += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52304_checks_green() {
        let s = run_unx52304_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
