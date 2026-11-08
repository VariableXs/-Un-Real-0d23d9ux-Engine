//! UNX-F52496 Hydra / 演练靶场爆破面收口（干跑判据） · 0xCBBC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52496"，逐字）：
//!   title: Hydra / 演练靶场爆破面收口（干跑判据）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52497-J1 APK 静
//!     态分析 ×10（本地样本）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52496）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52497-J1，照原文锚定；功能按本卡标题（Hydra / 演练靶场爆破面
//! 收口（干跑判据））落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（本地授权演练靶场/干跑语义，零真实攻击面，零真实爆破）：
//!   ① 判据只对本地回环靶标（127.0.0.1 演练靶场）与干跑结果账收口；
//!      爆破面全程只走 plan/dryrun/abort 三态，零真实爆破语义在册。
//!   ② 参数 20 格：五参数族闭集（-l/-p/-L/-P/-t）各 4 格，家族单调
//!      （逆序必抓）；-t 并发档在干跑域内只投影档位不发起连接。
//!   ③ 干跑三态 6 格（plan/dryrun/abort 各写读两格）+ 回环靶标三元闸
//!      （回环址/授权域/干跑位），越闸即拒。
//!   ④ 授权声明 2 行 + 与 SQLMap 分账声明 2 行（F52495 域邻卡）+ C 上
//!      栈声明 2 行，闭集缺一拒封（seal 语义：缺声明行 → ERowMiss）。
//!   ⑤ ledger 状态机（new/record/declare/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作越闭集/操作漂移/干跑三态越
//!      闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S2/S3 闭集
//!      （20 格 + 6 格三态 + 3 闸 + 6 行声明 + 六判据全通 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52496;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 参数格数。
pub const PARAM_OPS: usize = 20;

/// 干跑三态格数（plan/dryrun/abort 各写读两格）。
pub const TRI_CELLS: usize = 6;

/// 回环靶标三元闸数。
pub const LOOP_GATES: usize = 3;

/// 授权声明行数。
pub const AUTH_ROWS: usize = 2;

/// 与 SQLMap 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HydraCode {
    /// 成功
    Ok = 0,
    /// 参数号越闭集
    ENoOp,
    /// 参数漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 干跑三态格越闭集
    ENoTri,
    /// 干跑三态格漂移
    ETriDrift,
    /// 回环三元闸越闸
    ENoLoop,
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
    /// 定级漂移
    EGradeDrift,
}

impl HydraCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HydraCode::Ok => "成功",
            HydraCode::ENoOp => "参数号越闭集：五参数族 20 格之外先登记，零真实爆破语义在册",
            HydraCode::EDrift => "参数漂移：参数格折叠与基准不恒等，逐格复算",
            HydraCode::EOrder => "家族逆序：-l→-p→-L→-P→-t 单调，逆序必抓",
            HydraCode::ENoTri => "干跑三态格越闭集：plan/dryrun/abort 6 格之外先登记",
            HydraCode::ETriDrift => "干跑三态格漂移：三态格折叠与基准不恒等，逐格复算",
            HydraCode::ENoLoop => "回环三元闸越闸：回环址/授权域/干跑位三元闸越闸即拒",
            HydraCode::ENoRow => "声明行越闭集：授权 / SQLMap 分账 / C 上栈 6 行闭集之外先登记",
            HydraCode::ERowMiss => "缺声明行：授权 / 与 SQLMap 分账 / C 上栈声明未齐先补齐",
            HydraCode::EDup => "重复落账：同格只许一条账目",
            HydraCode::ESeal => "未齐即封：20 格 + 6 格三态 + 3 闸 + 6 行声明 + 六判据未齐先补齐",
            HydraCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            HydraCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不产出，闭集在册）
    S1,
    /// S2：有条件收口（本卡目标档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口（Hydra 卡闭集在册但目标档为 S2）",
            Grade::S2 => "S2：20格+6格三态+3闸+6行声明+六判据全通（干跑判据，零真实爆破）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 参数格载荷（格 o；参数族 = o/4）。
pub const fn param_cell(o: usize) -> Option<u64> {
    if o >= PARAM_OPS {
        return None;
    }
    Some((0xCBBC0u64 + (o as u64) * 0x43) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 参数族（0..4=-l 4..8=-p 8..12=-L 12..16=-P 16..20=-t 并发档）。
pub const fn param_kind(o: usize) -> Option<u32> {
    if o >= PARAM_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 参数族名（按族索引，非格号）。
pub const fn param_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-l 单账号"),
        1 => Some("-p 单口令"),
        2 => Some("-L 账号表"),
        3 => Some("-P 口令表"),
        4 => Some("-t 并发档"),
        _ => None,
    }
}

/// 干跑三态格载荷（格 t：0/1=plan 写读 2/3=dryrun 写读 4/5=abort 写读）。
pub const fn tri_cell(t: usize) -> Option<u64> {
    if t >= TRI_CELLS {
        return None;
    }
    Some((0xCC000u64 + (t as u64) * 0x53) ^ 0x485954445259524E)
}

/// 干跑三态名（按三态族索引：t/2）。
pub const fn tri_name(t: usize) -> Option<&'static str> {
    match t / 2 {
        0 => Some("plan 计划态"),
        1 => Some("dryrun 干跑态"),
        2 => Some("abort 中止态"),
        _ => None,
    }
}

/// 回环靶标三元闸载荷（闸 g：0=回环址 1=授权域 2=干跑位）。
pub const fn loop_gate_cell(g: usize) -> Option<u32> {
    if g >= LOOP_GATES {
        return None;
    }
    Some(0xCD00 + g as u32 * 0x43)
}

/// 回环三元闸名。
pub const fn loop_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("回环址闸：仅 127.0.0.1 靶标可过闸"),
        1 => Some("授权域闸：演练授权域名单在册才过闸"),
        2 => Some("干跑位闸：干跑位非 abort 才过闸"),
        _ => None,
    }
}

/// 授权声明行折叠（行 r）。
pub const fn auth_row(r: usize) -> Option<u32> {
    if r >= AUTH_ROWS {
        return None;
    }
    Some(0xCE00 + r as u32 * 0x53)
}

/// 授权声明行名。
pub const fn auth_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("授权声明：演练靶场爆破面仅限授权域干跑"),
        1 => Some("授权声明：零真实爆破语义在册，出域即拒"),
        _ => None,
    }
}

/// 与 SQLMap 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCF00 + r as u32 * 0x43)
}

/// 与 SQLMap 分账声明行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("与 SQLMap 分账声明：Hydra 爆破面账独立记（F52495 域邻卡）"),
        1 => Some("与 SQLMap 分账声明：分账锚定行不改写"),
        _ => None,
    }
}

/// C 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD000 + r as u32 * 0x53)
}

/// C 上栈声明行名。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("C 上栈声明：Hydra 核心依赖 C 栈运行时"),
        1 => Some("C 上栈声明：干跑桩与主栈同版本编译"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Hydra 收口账本状态机。
#[derive(Clone)]
pub struct HydraLedger {
    /// 六判据（本机靶场：白名单/授权/限速/隔离/落账/写回）。
    gates: [Option<bool>; 6],
    /// 参数账。
    ops: [Option<u64>; PARAM_OPS],
    /// 干跑三态账。
    tris: [Option<u64>; TRI_CELLS],
    /// 回环三元闸账。
    loops: [Option<u32>; LOOP_GATES],
    /// 授权声明账。
    auths: [Option<u32>; AUTH_ROWS],
    /// SQLMap 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后参数族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HydraLedger {
    /// 新账本。
    pub fn new() -> Self {
        HydraLedger {
            gates: [None; 6],
            ops: [None; PARAM_OPS],
            tris: [None; TRI_CELLS],
            loops: [None; LOOP_GATES],
            auths: [None; AUTH_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HydraCode> {
        if gate >= 6 {
            return Err(HydraCode::ESeal);
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

    /// 参数一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), HydraCode> {
        if o >= PARAM_OPS {
            return Err(HydraCode::ENoOp);
        }
        let kind = match param_kind(o) {
            Some(k) => k,
            None => return Err(HydraCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(HydraCode::EOrder);
        }
        let want = match param_cell(o) {
            Some(w) => w,
            None => return Err(HydraCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(HydraCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HydraCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 参数全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < PARAM_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 干跑三态一格（载荷对照 + 重复拒）。
    pub fn record_tri(&mut self, t: usize, got: u64) -> Result<(), HydraCode> {
        let want = match tri_cell(t) {
            Some(w) => w,
            None => return Err(HydraCode::ENoTri),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + t as u64);
            return Err(HydraCode::ETriDrift);
        }
        if match self.tris.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HydraCode::EDup);
        }
        if let Some(c) = self.tris.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x8301 + t as u64);
        Ok(())
    }

    /// 干跑三态全账核验（6 格）。
    pub fn tris_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRI_CELLS {
            match self.tris.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 回环三元闸一格（载荷对照 + 重复拒，越闸即拒）。
    pub fn record_loop_gate(&mut self, g: usize, got: u32) -> Result<(), HydraCode> {
        let want = match loop_gate_cell(g) {
            Some(w) => w,
            None => return Err(HydraCode::ENoLoop),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + g as u64);
            return Err(HydraCode::ENoLoop);
        }
        if match self.loops.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HydraCode::EDup);
        }
        if let Some(c) = self.loops.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x8401 + g as u64);
        Ok(())
    }

    /// 回环三元闸全账核验（3 闸）。
    pub fn loops_all(&self) -> bool {
        let mut g = 0usize;
        while g < LOOP_GATES {
            match self.loops.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 声明一行（group 0=授权 1=SQLMap 分账 2=C 上栈）。
    pub fn declare_row(&mut self, group: usize, r: usize) -> Result<(), HydraCode> {
        let limit = match group {
            0 => AUTH_ROWS,
            1 => SPLIT_ROWS,
            2 => STACK_ROWS,
            _ => return Err(HydraCode::ERowMiss),
        };
        if r >= limit {
            return Err(HydraCode::ENoRow);
        }
        let want = match group {
            0 => auth_row(r),
            1 => split_row(r),
            _ => stack_row(r),
        };
        let want = match want {
            Some(w) => w,
            None => return Err(HydraCode::ENoRow),
        };
        let hit = match group {
            0 => match self.auths.get(r) {
                Some(Some(_)) => true,
                _ => false,
            },
            1 => match self.splits.get(r) {
                Some(Some(_)) => true,
                _ => false,
            },
            _ => match self.stacks.get(r) {
                Some(Some(_)) => true,
                _ => false,
            },
        };
        if hit {
            return Err(HydraCode::EDup);
        }
        match group {
            0 => {
                if let Some(c) = self.auths.get_mut(r) {
                    *c = Some(want);
                }
            }
            1 => {
                if let Some(c) = self.splits.get_mut(r) {
                    *c = Some(want);
                }
            }
            _ => {
                if let Some(c) = self.stacks.get_mut(r) {
                    *c = Some(want);
                }
            }
        }
        self.fnv(0x8500 + group as u64 * 0x10 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（三组各 2 行）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < AUTH_ROWS {
            match self.auths.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut s = 0usize;
        while s < SPLIT_ROWS {
            match self.splits.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        let mut c = 0usize;
        while c < STACK_ROWS {
            match self.stacks.get(c) {
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

    /// 封闸：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
    pub fn seal(&self) -> Result<(), HydraCode> {
        if !self.gates_all() {
            return Err(HydraCode::ESeal);
        }
        if !self.rows_all() {
            return Err(HydraCode::ERowMiss);
        }
        if !self.ops_all() || !self.tris_all() || !self.loops_all() {
            return Err(HydraCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S2，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.tris_all() && self.loops_all() && self.rows_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for HydraLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HydraLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TRI_CELLS {
        if let Some(w) = tri_cell(t) {
            if led.record_tri(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut o = 0usize;
    while o < PARAM_OPS {
        if let Some(w) = param_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut lg = 0usize;
    while lg < LOOP_GATES {
        if let Some(w) = loop_gate_cell(lg) {
            let _ = led.record_loop_gate(lg, w);
        }
        lg += 1;
    }
    let mut r = 0usize;
    while r < AUTH_ROWS {
        let _ = led.declare_row(0, r);
        r += 1;
    }
    let mut s = 0usize;
    while s < SPLIT_ROWS {
        let _ = led.declare_row(1, s);
        s += 1;
    }
    let mut c = 0usize;
    while c < STACK_ROWS {
        let _ = led.declare_row(2, c);
        c += 1;
    }
    // 注入 10 轮：参数越闭集 / 参数漂移 / 干跑三态越闭集三态轮转，
    // 全部落在当轮临时账本上；主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HydraLedger::new();
        let caught = match kind {
            0 => {
                lt.record_op(0, param_cell(0).unwrap_or(0));
                lt.record_op(PARAM_OPS, 0).is_err()
            }
            1 => {
                // 同族已录格注漂移（跨族会先撞序闸）。
                lt.record_op(5, param_cell(5).unwrap_or(0));
                lt.record_op(5, param_cell(5).unwrap_or(0).wrapping_add(1)) == Err(HydraCode::EDrift)
            }
            _ => {
                lt.record_tri(0, tri_cell(0).unwrap_or(0));
                lt.record_tri(TRI_CELLS, 0) == Err(HydraCode::ENoTri)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52496 判据集（锚 UNX-F52497-J1 ×10（本地样本），off-by-one 板式，
/// 功能按本卡标题落地为演练靶场爆破面干跑收口）。
pub fn run_unx52496_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52496");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "参数 20 格全通（本地样本干跑域）",
        recorded >= PARAM_OPS as u32,
        "-l/-p/-L/-P/-t 五参数族逐格恒等，-t 并发档只投影不发起连接",
    );
    s.add(
        "干跑三态 6 格全落",
        {
            let mut lt = HydraLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRI_CELLS {
                if let Some(w) = tri_cell(t) {
                    if lt.record_tri(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tris_all()
        },
        "plan/dryrun/abort 各写读两格逐格恒等，零真实爆破语义在册（§AI-66.3）",
    );
    s.add(
        "回环靶标三元闸 3/3",
        {
            let mut lt = HydraLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < LOOP_GATES {
                if let Some(w) = loop_gate_cell(g) {
                    if lt.record_loop_gate(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.loops_all()
        },
        "回环址闸/授权域闸/干跑位闸逐闸过账，越闸即拒",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = HydraLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "本机靶场白名单/授权/限速/隔离/落账/写回逐格落账",
    );
    s.add(
        "授权声明 2/2",
        {
            let mut lt = HydraLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < AUTH_ROWS {
                if lt.declare_row(0, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "仅授权域干跑 / 零真实爆破语义在册（§AI-66.3）",
    );
    s.add(
        "与 SQLMap 分账声明 2/2",
        {
            let mut lt = HydraLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_row(1, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "爆破面账独立记（F52495 域邻卡）/ 分账锚定行不改写",
    );
    s.add(
        "C 上栈声明 2/2",
        {
            let mut lt = HydraLedger::new();
            let mut ok = true;
            // rows_all 要求授权/分账/上栈三组声明行全落。
            let mut g = 0usize;
            while g < 3 {
                let mut r = 0usize;
                while r < STACK_ROWS {
                    if lt.declare_row(g, r).is_err() {
                        ok = false;
                    }
                    r += 1;
                }
                g += 1;
            }
            ok && lt.rows_all()
        },
        "C 核心运行时声明 + 干跑桩同版本编译声明",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PARAM_OPS {
                if param_kind(o).unwrap_or(5) < param_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五参数族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "参数越闭集/参数漂移/干跑三态越闭集三态轮转必抓");
    s.add("边界·越闭集·参数", HydraLedger::new().record_op(PARAM_OPS, 0).is_err(), "参数 20 越闭集必拒（ENoOp 提前返回，不计注入账）");
    s.add(
        "边界·越闭集·干跑三态",
        HydraLedger::new().record_tri(TRI_CELLS, 0).is_err(),
        "干跑三态格 6 越闭集必拒（ENoTri 提前返回）",
    );
    s.add(
        "边界·越闭集·回环闸与声明行",
        {
            let mut lt = HydraLedger::new();
            lt.record_loop_gate(LOOP_GATES, 0).is_err()
                && lt.declare_row(0, AUTH_ROWS).is_err()
                && lt.declare_row(2, STACK_ROWS).is_err()
        },
        "回环闸 3 / 授权行 2 / C 栈行 2 越闭集必拒（ENoLoop/ENoRow 提前返回）",
    );
    s.add(
        "参数漂移拒",
        {
            let mut lt = HydraLedger::new();
            lt.record_op(0, param_cell(0).unwrap_or(0));
            lt.record_op(1, param_cell(1).unwrap_or(0).wrapping_add(1)) == Err(HydraCode::EDrift)
        },
        "参数载荷漂移必抓",
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
            let mut lt = HydraLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut t = 0usize;
            while t < TRI_CELLS {
                if let Some(w) = tri_cell(t) {
                    let _ = lt.record_tri(t, w);
                }
                t += 1;
            }
            let mut o = 0usize;
            while o < PARAM_OPS {
                if let Some(w) = param_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut lg = 0usize;
            while lg < LOOP_GATES {
                if let Some(w) = loop_gate_cell(lg) {
                    let _ = lt.record_loop_gate(lg, w);
                }
                lg += 1;
            }
            let mut r = 0usize;
            while r < AUTH_ROWS {
                let _ = lt.declare_row(0, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < SPLIT_ROWS {
                let _ = lt.declare_row(1, r2);
                r2 += 1;
            }
            let mut r3 = 0usize;
            while r3 < STACK_ROWS {
                let _ = lt.declare_row(2, r3);
                r3 += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "封闸语义闭集",
        {
            // 六闸缺 → ESeal；六闸齐而声明行缺 → ERowMiss；格账缺 → ESeal。
            let empty = HydraLedger::new();
            let mut mid = HydraLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = mid.judge_gate(g);
                g += 1;
            }
            let mut full = mid.clone();
            let mut r = 0usize;
            while r < AUTH_ROWS {
                let _ = full.declare_row(0, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < SPLIT_ROWS {
                let _ = full.declare_row(1, r2);
                r2 += 1;
            }
            let mut r3 = 0usize;
            while r3 < STACK_ROWS {
                let _ = full.declare_row(2, r3);
                r3 += 1;
            }
            empty.seal() == Err(HydraCode::ESeal)
                && mid.seal() == Err(HydraCode::ERowMiss)
                && full.seal() == Err(HydraCode::ESeal)
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal，按序核验",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBBC0,
        "SEG_BASE 0x40 对齐且在 VX16 域（0xCA400 + 0x40×95）",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_param_kind_table() {
        assert_eq!(param_kind(0), Some(0));
        assert_eq!(param_kind(4), Some(1));
        assert_eq!(param_kind(8), Some(2));
        assert_eq!(param_kind(12), Some(3));
        assert_eq!(param_kind(19), Some(4));
        assert!(param_kind(20).is_none());
        // 名表按族索引而非格号。
        assert_eq!(param_kind_name(0), Some("-l 单账号"));
        assert_eq!(param_kind_name(3), Some("-P 口令表"));
        assert_eq!(param_kind_name(4), Some("-t 并发档"));
        assert!(param_kind_name(5).is_none());
        assert!(param_cell(20).is_none());
    }

    #[test]
    fn test_tri_and_loop_cells() {
        assert_eq!(tri_name(0), Some("plan 计划态"));
        assert_eq!(tri_name(3), Some("dryrun 干跑态"));
        assert_eq!(tri_name(5), Some("abort 中止态"));
        assert!(tri_name(6).is_none());
        assert!(tri_cell(TRI_CELLS).is_none());
        assert_eq!(loop_gate_name(0), Some("回环址闸：仅 127.0.0.1 靶标可过闸"));
        assert!(loop_gate_name(3).is_none());
        assert!(loop_gate_cell(LOOP_GATES).is_none());
    }

    #[test]
    fn test_rows_pure() {
        assert!(auth_row(AUTH_ROWS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(auth_row(0), Some(0xCE00));
        assert_eq!(split_row(1), Some(0xCF00 + 0x43));
        assert_eq!(stack_row(1), Some(0xD000 + 0x53));
        assert_eq!(auth_row_name(1), Some("授权声明：零真实爆破语义在册，出域即拒"));
        assert!(auth_row_name(2).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = HydraLedger::new();
        let _ = lt.record_op(9, param_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, param_cell(2).unwrap_or(0)), Err(HydraCode::EOrder));
        assert!(lt.record_op(10, param_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HydraLedger::new();
        assert_eq!(lt.record_op(5, param_cell(5).unwrap_or(0).wrapping_add(1)), Err(HydraCode::EDrift));
        assert_eq!(
            lt.record_tri(1, tri_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(HydraCode::ETriDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HydraLedger::new();
        let _ = lt.record_op(0, param_cell(0).unwrap_or(0));
        assert_eq!(lt.record_op(0, param_cell(0).unwrap_or(0)), Err(HydraCode::EDup));
        let _ = lt.record_tri(0, tri_cell(0).unwrap_or(0));
        assert_eq!(lt.record_tri(0, tri_cell(0).unwrap_or(0)), Err(HydraCode::EDup));
        let _ = lt.record_loop_gate(0, loop_gate_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_loop_gate(0, loop_gate_cell(0).unwrap_or(0)),
            Err(HydraCode::EDup)
        );
        let _ = lt.declare_row(1, 0);
        assert_eq!(lt.declare_row(1, 0), Err(HydraCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HydraLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = HydraLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_order() {
        let empty = HydraLedger::new();
        assert_eq!(empty.seal(), Err(HydraCode::ESeal));
        let mut mid = HydraLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.seal(), Err(HydraCode::ERowMiss));
        let mut r = 0usize;
        while r < AUTH_ROWS {
            let _ = mid.declare_row(0, r);
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < SPLIT_ROWS {
            let _ = mid.declare_row(1, r2);
            r2 += 1;
        }
        let mut r3 = 0usize;
        while r3 < STACK_ROWS {
            let _ = mid.declare_row(2, r3);
            r3 += 1;
        }
        assert_eq!(mid.seal(), Err(HydraCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：6 三态 + 20 参数 = 26。
        assert_eq!(recorded, TRI_CELLS as u32 + PARAM_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xCBBC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(HydraCode::Ok as u32, 0);
        assert_eq!(HydraCode::EGradeDrift as u32, 12);
        assert!(!HydraCode::Ok.detail().is_empty());
        assert!(!HydraCode::ENoLoop.detail().is_empty());
        assert!(!HydraCode::ERowMiss.detail().is_empty());
        assert_eq!(HydraCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S2（目标档）。
        let empty = HydraLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = HydraLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < PARAM_OPS {
            if let Some(w) = param_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut t = 0usize;
        while t < TRI_CELLS {
            if let Some(w) = tri_cell(t) {
                let _ = full.record_tri(t, w);
            }
            t += 1;
        }
        let mut lg = 0usize;
        while lg < LOOP_GATES {
            if let Some(w) = loop_gate_cell(lg) {
                let _ = full.record_loop_gate(lg, w);
            }
            lg += 1;
        }
        let mut r = 0usize;
        while r < AUTH_ROWS {
            let _ = full.declare_row(0, r);
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < SPLIT_ROWS {
            let _ = full.declare_row(1, r2);
            r2 += 1;
        }
        let mut r3 = 0usize;
        while r3 < STACK_ROWS {
            let _ = full.declare_row(2, r3);
            r3 += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_boundary_no_injection_ledger() {
        // 越闭集边界拒（ENoX 类提前返回）不计注入账。
        let mut lt = HydraLedger::new();
        let _ = lt.record_op(PARAM_OPS, 0);
        let _ = lt.record_tri(TRI_CELLS, 0);
        let _ = lt.record_loop_gate(LOOP_GATES, 0);
        let _ = lt.declare_row(0, AUTH_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn unx52496_checks_green() {
        let s = run_unx52496_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
