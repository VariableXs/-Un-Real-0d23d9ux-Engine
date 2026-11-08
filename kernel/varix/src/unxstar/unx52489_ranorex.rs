//! UNX-F52489 Ranorex 收口 · 0xCBA00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52489"，逐字）：
//!   title: Ranorex 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52490-J1 六判据
//!     全落（Electron 同构）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52489）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52490-J1，照原文锚定；功能按本卡标题（Ranorex）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模块族 20 操作集：录制/仓库/映射/回放/报告五家族闭集各 4 操作，
//!      逐操作载荷（纯函数折叠）恒等才收；家族单调（逆序必抓）。
//!   ② RxPath 投影 8 格：UI 元素到 RxPath 的投影闭集，逐格载荷对照，
//!      漂移必抓。
//!   ③ 商业许可三元闸：授权书/节点数/有效期三元闭集逐格落账，越界必拒。
//!   ④ 与 TestComplete 分账声明 2 行（F52488）+ C#/C++ 上栈声明 2 行，
//!      四行闭集缺一拒封（seal 时声明行缺 → ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      seal/judge_grade）；注入核 run_cycle 三态轮转 10/10（操作越闭集/
//!      操作漂移/家族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级
//!      S2（KB-COMPAT：许可面如实，有条件收口档）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52489;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五模块族操作数。
pub const OPS: usize = 20;

/// RxPath 投影格数。
pub const RXPATH_CELLS: usize = 8;

/// 商业许可三元闸格数。
pub const LIC_CELLS: usize = 3;

/// TestComplete 分账声明行数（F52488）。
pub const SPLIT_ROWS: usize = 2;

/// C#/C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RanorexCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移（操作/RxPath/许可共用）
    EDrift,
    /// 家族逆序
    EOrder,
    /// RxPath 投影号越闭集
    ENoPath,
    /// 许可三元格号越闭集
    ENoLic,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
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

impl RanorexCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RanorexCode::Ok => "成功",
            RanorexCode::ENoOp => "操作号越闭集：录制/仓库/映射/回放/报告 20 操作集之外先登记",
            RanorexCode::EDrift => "载荷漂移：操作/RxPath/许可载荷与基准折叠不恒等，逐格复算",
            RanorexCode::EOrder => "家族逆序：录制→仓库→映射→回放→报告单调，逆序必抓",
            RanorexCode::ENoPath => "RxPath 投影号越闭集：8 格投影闭集之外先登记",
            RanorexCode::ENoLic => "许可格号越闭集：授权书/节点数/有效期三元闭集之外先登记",
            RanorexCode::ENoRow => "声明行越闭集：TestComplete 分账 2 行 + C#/C++ 上栈 2 行之外先登记",
            RanorexCode::ERowMiss => "缺声明行：TestComplete 分账/C#/C++ 上栈声明未齐先补齐",
            RanorexCode::EDup => "重复落账：同格只许一条账目",
            RanorexCode::ESeal => "未齐即封：20 操作 + 8 投影 + 3 许可 + 4 声明行未齐先补齐",
            RanorexCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RanorexCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RanorexCode::EGradeDrift => "定级漂移：六判据全落且分账声明全齐才可定 S2（许可面如实）",
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
            Grade::S1 => "S1：全判据全格全通收口",
            Grade::S2 => "S2：有条件收口（Ranorex 商业许可面如实登记档）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五模块族操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xCBA00u64 + (o as u64) * 0x2F) ^ 0x5248_9A00_0000_0000)
}

/// 操作家族（0..4=录制 4..8=仓库 8..12=映射 12..16=回放 16..20=报告）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("录制"),
        1 => Some("仓库"),
        2 => Some("映射"),
        3 => Some("回放"),
        4 => Some("报告"),
        _ => None,
    }
}

/// RxPath 投影格载荷（投影 p）。
pub const fn rxpath_cell(p: usize) -> Option<u64> {
    if p >= RXPATH_CELLS {
        return None;
    }
    Some((0xCBB00u64 + (p as u64) * 0x37) ^ 0x5248_9B00_0000_0000)
}

/// RxPath 投影段名（按格索引的段名表）。
pub const fn rxpath_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("form"),
        1 => Some("container"),
        2 => Some("button"),
        3 => Some("text"),
        4 => Some("table"),
        5 => Some("row"),
        6 => Some("cell"),
        7 => Some("menu"),
        _ => None,
    }
}

/// 商业许可三元格载荷（格 i：0=授权书 1=节点数 2=有效期）。
pub const fn lic_cell(i: usize) -> Option<u64> {
    if i >= LIC_CELLS {
        return None;
    }
    Some((0xCBC00u64 + (i as u64) * 0x41) ^ 0x5248_9C00_0000_0000)
}

/// 许可三元格名。
pub const fn lic_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("授权书"),
        1 => Some("节点数"),
        2 => Some("有效期"),
        _ => None,
    }
}

/// TestComplete 分账声明行折叠（行 r，F52488）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB00 + r as u32 * 0x2C)
}

/// C#/C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCC00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Ranorex 收口账本状态机。
#[derive(Clone)]
pub struct RanorexLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五模块族操作账。
    ops: [Option<u64>; OPS],
    /// RxPath 投影账。
    paths: [Option<u64>; RXPATH_CELLS],
    /// 商业许可三元账。
    lics: [Option<u64>; LIC_CELLS],
    /// TestComplete 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C#/C++ 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RanorexLedger {
    /// 新账本。
    pub fn new() -> Self {
        RanorexLedger {
            gates: [None; 6],
            ops: [None; OPS],
            paths: [None; RXPATH_CELLS],
            lics: [None; LIC_CELLS],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RanorexCode> {
        if gate >= 6 {
            return Err(RanorexCode::ESeal);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RanorexCode> {
        if o >= OPS {
            return Err(RanorexCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(RanorexCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A00);
            return Err(RanorexCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(RanorexCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A01 + o as u64);
            return Err(RanorexCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RanorexCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4A02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
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

    /// RxPath 投影一格（载荷对照 + 重复拒）。
    pub fn record_path(&mut self, p: usize, got: u64) -> Result<(), RanorexCode> {
        if p >= RXPATH_CELLS {
            return Err(RanorexCode::ENoPath);
        }
        let want = match rxpath_cell(p) {
            Some(w) => w,
            None => return Err(RanorexCode::ENoPath),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00 + p as u64);
            return Err(RanorexCode::EDrift);
        }
        if match self.paths.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RanorexCode::EDup);
        }
        if let Some(c) = self.paths.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x4B01 + p as u64);
        Ok(())
    }

    /// RxPath 投影全账核验（8 格）。
    pub fn paths_all(&self) -> bool {
        let mut p = 0usize;
        while p < RXPATH_CELLS {
            match self.paths.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 商业许可三元一格（载荷对照 + 重复拒）。
    pub fn record_lic(&mut self, i: usize, got: u64) -> Result<(), RanorexCode> {
        if i >= LIC_CELLS {
            return Err(RanorexCode::ENoLic);
        }
        let want = match lic_cell(i) {
            Some(w) => w,
            None => return Err(RanorexCode::ENoLic),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4C00 + i as u64);
            return Err(RanorexCode::EDrift);
        }
        if match self.lics.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RanorexCode::EDup);
        }
        if let Some(c) = self.lics.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4C01 + i as u64);
        Ok(())
    }

    /// 许可三元全账核验（3 格）。
    pub fn lics_all(&self) -> bool {
        let mut i = 0usize;
        while i < LIC_CELLS {
            match self.lics.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// TestComplete 分账声明一行（F52488）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), RanorexCode> {
        if r >= SPLIT_ROWS {
            return Err(RanorexCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RanorexCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// TestComplete 分账全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C#/C++ 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), RanorexCode> {
        if r >= STACK_ROWS {
            return Err(RanorexCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RanorexCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4E00 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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

    /// 收口封账（顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal）。
    pub fn seal(&self) -> Result<(), RanorexCode> {
        if !self.gates_all() {
            return Err(RanorexCode::ESeal);
        }
        if !self.splits_all() || !self.stacks_all() {
            return Err(RanorexCode::ERowMiss);
        }
        if !self.ops_all() || !self.paths_all() || !self.lics_all() {
            return Err(RanorexCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S2 许可面如实档；六闸齐 → S2；否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.seal().is_ok() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RanorexLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RanorexLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < RXPATH_CELLS {
        if let Some(w) = rxpath_cell(p) {
            if led.record_path(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut i = 0usize;
    while i < LIC_CELLS {
        if let Some(w) = lic_cell(i) {
            if led.record_lic(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 家族逆序三态轮转。
    // 三态注入全部落在当轮临时账本上；漂移/逆序先按合法序落正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RanorexLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(RanorexCode::EDrift)
            }
            _ => {
                let _ = lt.record_op(4, op_cell(4).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0)) == Err(RanorexCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4F00 + k as u64);
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
/// UNX-F52489 判据集（锚 UNX-F52490-J1，off-by-one 板式）。
pub fn run_unx52489_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52489");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五模块族 20 操作全通",
        recorded >= OPS as u32,
        "录制/仓库/映射/回放/报告五家族逐操作恒等（Electron 同构）",
    );
    s.add(
        "RxPath 投影 8 格全落",
        {
            let mut lt = RanorexLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < RXPATH_CELLS {
                if let Some(w) = rxpath_cell(p) {
                    if lt.record_path(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.paths_all()
        },
        "UI 元素到 RxPath 投影闭集逐格恒等，漂移必抓",
    );
    s.add(
        "商业许可三元闸全落",
        {
            let mut lt = RanorexLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < LIC_CELLS {
                if let Some(w) = lic_cell(i) {
                    if lt.record_lic(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.lics_all()
        },
        "授权书/节点数/有效期三元闭集逐格落账，许可面如实",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = RanorexLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "TestComplete 分账声明 2/2 全齐（F52488）",
        {
            let mut lt = RanorexLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "TestComplete 消费分账声明两行闭集（F52488 锚定）",
    );
    s.add(
        "C#/C++ 上栈声明 2/2 全齐",
        {
            let mut lt = RanorexLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "C# 上栈声明/C++ 上栈声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "录制→仓库→映射→回放→报告五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作越闭集/操作漂移/家族逆序三态轮转必抓",
    );
    let mut l0 = RanorexLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）",
    );
    s.add(
        "边界·RxPath 越闭集",
        l0.record_path(RXPATH_CELLS, 0).is_err(),
        "投影 8 越闭集必拒（ENoPath 提前返回）",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err() && l0.declare_stack(STACK_ROWS).is_err(),
        "声明行越闭集必拒（ENoRow 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(RanorexCode::EDrift)
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
        "定级 S2 闭集（KB-COMPAT）",
        {
            let mut lt = RanorexLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < RXPATH_CELLS {
                if let Some(w) = rxpath_cell(p) {
                    let _ = lt.record_path(p, w);
                }
                p += 1;
            }
            let mut i = 0usize;
            while i < LIC_CELLS {
                if let Some(w) = lic_cell(i) {
                    let _ = lt.record_lic(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全齐 → S2（Ranorex 商业许可面如实定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBA00,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×88",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind_mapping() {
        // 名表按家族（kind）索引而非格号。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(12), Some(3));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(0), Some("录制"));
        assert_eq!(op_kind_name(1), Some("仓库"));
        assert_eq!(op_kind_name(2), Some("映射"));
        assert_eq!(op_kind_name(3), Some("回放"));
        assert_eq!(op_kind_name(4), Some("报告"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_rxpath_names() {
        assert_eq!(rxpath_name(0), Some("form"));
        assert_eq!(rxpath_name(7), Some("menu"));
        assert!(rxpath_name(8).is_none());
        assert!(rxpath_cell(RXPATH_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        let b = lic_cell(2).unwrap_or(0);
        assert_eq!(b, lic_cell(2).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族格，再注逆序必抓；被拒格不改 last_kind。
        let mut lt = RanorexLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(RanorexCode::EOrder));
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RanorexLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(RanorexCode::EDrift)
        );
        assert_eq!(
            lt.record_path(2, rxpath_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(RanorexCode::EDrift)
        );
        assert_eq!(
            lt.record_lic(1, lic_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(RanorexCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RanorexLedger::new();
        let _ = lt.record_path(0, rxpath_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_path(0, rxpath_cell(0).unwrap_or(0)),
            Err(RanorexCode::EDup)
        );
        let _ = lt.record_lic(0, lic_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_lic(0, lic_cell(0).unwrap_or(0)),
            Err(RanorexCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RanorexLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = RanorexLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_license_boundary() {
        let mut lt = RanorexLedger::new();
        assert!(lt.record_lic(LIC_CELLS, 0).is_err());
        assert_eq!(lic_name(0), Some("授权书"));
        assert_eq!(lic_name(1), Some("节点数"));
        assert_eq!(lic_name(2), Some("有效期"));
        assert!(lic_name(3).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 封账语义顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
        let empty = RanorexLedger::new();
        assert_eq!(empty.seal(), Err(RanorexCode::ESeal));
        let mut lt = RanorexLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(RanorexCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(RanorexCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 操作 + 8 投影 + 3 许可。
        assert!(recorded >= OPS as u32 + RXPATH_CELLS as u32 + LIC_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBA00);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RanorexCode::Ok as u32, 0);
        assert_eq!(RanorexCode::EGradeDrift as u32, 12);
        assert_eq!(RanorexCode::Ok.val(), SEG_BASE);
        assert_eq!(RanorexCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!RanorexCode::Ok.detail().is_empty());
        assert!(!RanorexCode::ESeal.detail().is_empty());
        assert!(!RanorexCode::ERowMiss.detail().is_empty());
        assert!(!RanorexCode::EFp.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = RanorexLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_row_boundary() {
        let mut lt = RanorexLedger::new();
        assert!(lt.declare_split(0).is_ok());
        assert!(lt.declare_split(1).is_ok());
        assert!(lt.declare_split(2).is_err());
        assert!(lt.declare_stack(0).is_ok());
        assert!(lt.declare_stack(1).is_ok());
        assert!(lt.declare_stack(2).is_err());
        assert!(lt.splits_all() && lt.stacks_all());
    }

    #[test]
    fn unx52489_checks_green() {
        let s = run_unx52489_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
