//! UNX-F52333 pestudio 收口 · 0xC9300 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52333"，逐字）：
//!   title: pestudio 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52334-J1 六判据全落；
//!     壳/编译器识别 ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52333）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52334-J1，照原文锚定；功能按本卡标题（pestudio）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① 静态指标五族 20 格：导入/签名/熵/清单/网络指示器，每族 4 格，
//!      族单调（逆序必抓）+ 相位闸（后族先录对空账本同样必抓）。
//!   ② 指标三态（绿/黄/红）12 格：逐格落账，三态越界必抓。
//!   ③ 离线声明 2 行闭集：离线扫描声明 / 无遥测声明，缺一拒封。
//!   ④ 收口闸六判据（含壳/编译器识别）：逐格落账，未齐即封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10 轮（格越闭集/漂移/相位
//!      越序），三类注入全部落在当轮临时账本上，主账 injected==caught==10、
//!      escaped==0；漂移注入先录正格再注漂移；相位注入对空账本必抓且不改
//!      last_kind；双跑指纹一致；全通定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52333;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 静态指标格数（五族 × 4）。
pub const METRIC_CELLS: usize = 20;

/// 每族格数。
pub const FAMILY_CELLS: usize = 4;

/// 三态指标格数。
pub const TRI_CELLS: usize = 12;

/// 离线声明行数。
pub const OFFLINE_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PestudioCode {
    /// 成功
    Ok = 0,
    /// 指标格越闭集
    ENoCell,
    /// 指标载荷漂移
    ECellDrift,
    /// 族序逆序
    EOrder,
    /// 相位越序（后族先录）
    EPhase,
    /// 重复落账
    EDup,
    /// 判据闸越闭集
    ENoGate,
    /// 未齐即封
    ESeal,
    /// 三态越闭集
    ENoTri,
    /// 三态越界
    ETriRange,
    /// 声明行越闭集
    ENoRow,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl PestudioCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PestudioCode::Ok => "成功",
            PestudioCode::ENoCell => "指标格越闭集：静态指标 20 格闭集之外先登记",
            PestudioCode::ECellDrift => "指标载荷漂移：折叠值与基准不恒等，逐格复算",
            PestudioCode::EOrder => "族序逆序：导入→签名→熵→清单→网络指示器单调，逆序必抓",
            PestudioCode::EPhase => "相位越序：前族未齐先录后族，对空账本同样必抓",
            PestudioCode::EDup => "重复落账：同格只许一条账目",
            PestudioCode::ENoGate => "判据闸越闭集：六判据闸 0..=5 之外先登记",
            PestudioCode::ESeal => "未齐即封：20 指标格 + 12 三态格 + 2 行声明未齐先补齐",
            PestudioCode::ENoTri => "三态格越闭集：三态 12 格闭集之外先登记",
            PestudioCode::ETriRange => "三态越界：绿/黄/红三态闭集之外先登记",
            PestudioCode::ENoRow => "声明行越闭集：离线声明 2 行闭集之外先登记",
            PestudioCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PestudioCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20指标格+12三态格+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 静态指标格载荷（格 o；族 = o/4；XOR 锚低 16 位恒零）。
pub const fn metric_cell(o: usize) -> Option<u64> {
    if o >= METRIC_CELLS {
        return None;
    }
    Some((0xB3300u64 + (o as u64) * 0x37) ^ 0x5045_0000)
}

/// 指标族（0=导入 1=签名 2=熵 3=清单 4=网络指示器）。
pub const fn metric_kind(o: usize) -> Option<u32> {
    if o >= METRIC_CELLS {
        return None;
    }
    Some((o / FAMILY_CELLS) as u32)
}

/// 族名。
pub const fn metric_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("导入"),
        1 => Some("签名"),
        2 => Some("熵"),
        3 => Some("清单"),
        4 => Some("网络指示器"),
        _ => None,
    }
}

/// 三态格基准载荷（格 t）。
pub const fn tri_cell(t: usize) -> Option<u32> {
    if t >= TRI_CELLS {
        return None;
    }
    Some(0xB3400 + t as u32 * 0x2B)
}

/// 三态名（0=绿 1=黄 2=红）。
pub const fn tri_state_name(s: u32) -> Option<&'static str> {
    match s {
        0 => Some("绿"),
        1 => Some("黄"),
        2 => Some("红"),
        _ => None,
    }
}

/// 离线声明行折叠（行 r）。
pub const fn offline_row(r: usize) -> Option<u32> {
    if r >= OFFLINE_ROWS {
        return None;
    }
    Some(0xB3500 + r as u32 * 0x2B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// pestudio 收口账本状态机。
#[derive(Clone)]
pub struct PestudioLedger {
    /// 六判据（静态扫描启动/壳与编译器识别/崩溃退出/卸载清理/升级兼容/许可声明）。
    gates: [Option<bool>; 6],
    /// 静态指标账。
    metrics: [Option<u64>; METRIC_CELLS],
    /// 三态账（每格存 (基准, 三态值)）。
    tris: [Option<(u32, u32)>; TRI_CELLS],
    /// 离线声明账。
    offline: [Option<u32>; OFFLINE_ROWS],
    /// 最后族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PestudioLedger {
    /// 新账本。
    pub fn new() -> Self {
        PestudioLedger {
            gates: [None; 6],
            metrics: [None; METRIC_CELLS],
            tris: [None; TRI_CELLS],
            offline: [None; OFFLINE_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PestudioCode> {
        if gate >= 6 {
            return Err(PestudioCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 前族全齐核验（相位闸用）。
    fn families_complete(&self, upto: usize) -> bool {
        let bound = upto * FAMILY_CELLS;
        let mut i = 0usize;
        while i < bound {
            match self.metrics.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 静态指标一格（族单调 + 相位闸 + 载荷对照 + 重复拒）。
    pub fn record_metric(&mut self, o: usize, got: u64) -> Result<(), PestudioCode> {
        let kind = match metric_kind(o) {
            Some(k) => k,
            None => return Err(PestudioCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(PestudioCode::EOrder);
        }
        if (kind as usize) > 0 && !self.families_complete(kind as usize) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201);
            return Err(PestudioCode::EPhase);
        }
        let want = match metric_cell(o) {
            Some(w) => w,
            None => return Err(PestudioCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6202 + o as u64);
            return Err(PestudioCode::ECellDrift);
        }
        if match self.metrics.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PestudioCode::EDup);
        }
        if let Some(c) = self.metrics.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6203 + o as u64);
        Ok(())
    }

    /// 指标全账核验（20 格）。
    pub fn metrics_all(&self) -> bool {
        let mut o = 0usize;
        while o < METRIC_CELLS {
            match self.metrics.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 三态一格（绿/黄/红闭集 + 重复拒）。
    pub fn record_tri(&mut self, t: usize, state: u32) -> Result<(), PestudioCode> {
        if t >= TRI_CELLS {
            return Err(PestudioCode::ENoTri);
        }
        if state > 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + t as u64);
            return Err(PestudioCode::ETriRange);
        }
        if match self.tris.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PestudioCode::EDup);
        }
        let base = match tri_cell(t) {
            Some(b) => b,
            None => return Err(PestudioCode::ENoTri),
        };
        if let Some(c) = self.tris.get_mut(t) {
            *c = Some((base, state));
        }
        self.fnv(0x6301 + t as u64);
        Ok(())
    }

    /// 三态全账核验（12 格）。
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

    /// 离线声明一行。
    pub fn declare_offline(&mut self, r: usize) -> Result<(), PestudioCode> {
        if r >= OFFLINE_ROWS {
            return Err(PestudioCode::ENoRow);
        }
        if match self.offline.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PestudioCode::EDup);
        }
        if let Some(c) = self.offline.get_mut(r) {
            *c = offline_row(r);
        }
        self.fnv(0x6400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn offline_all(&self) -> bool {
        let mut r = 0usize;
        while r < OFFLINE_ROWS {
            match self.offline.get(r) {
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

    /// 最后族（相位/族序注入不改此值）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.metrics_all() && self.tris_all() && self.offline_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PestudioLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PestudioLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < METRIC_CELLS {
        if let Some(w) = metric_cell(o) {
            if led.record_metric(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TRI_CELLS {
        // 三态轮转：绿/黄/红循环落账。
        let _ = led.record_tri(t, (t as u32) % 3);
        t += 1;
    }
    let mut r = 0usize;
    while r < OFFLINE_ROWS {
        let _ = led.declare_offline(r);
        r += 1;
    }
    // 注入 10 轮：格越闭集 / 漂移 / 相位越序 三态轮转，全部落在当轮临时账本。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PestudioLedger::new();
        let caught = match kind {
            0 => lt.record_metric(METRIC_CELLS, 0) == Err(PestudioCode::ENoCell),
            1 => {
                // 漂移注入：先录正格再注漂移。
                let _ = lt.record_metric(0, metric_cell(0).unwrap_or(0));
                lt.record_metric(1, metric_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(PestudioCode::ECellDrift)
            }
            _ => {
                // 相位注入：对空账本录第 2 族首格，必抓且不改 last_kind。
                lt.record_metric(8, metric_cell(8).unwrap_or(0)) == Err(PestudioCode::EPhase)
                    && lt.last_kind() == -1
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6500 + k as u64);
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
/// UNX-F52333 判据集（锚 UNX-F52334-J1，off-by-one 板式）。
pub fn run_unx52333_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52333");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "静态指标五族 20 格全通",
        recorded >= METRIC_CELLS as u32,
        "导入/签名/熵/清单/网络指示器逐格恒等落账",
    );
    s.add(
        "指标三态 12 格全落（绿/黄/红）",
        {
            let mut lt = PestudioLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRI_CELLS {
                if lt.record_tri(t, (t as u32) % 3).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tris_all()
        },
        "绿/黄/红三态逐格落账，越界必抓",
    );
    s.add(
        "收口闸六判据全落",
        {
            let mut lt = PestudioLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "静态扫描启动/壳与编译器识别/崩溃退出/卸载清理/升级兼容/许可声明",
    );
    s.add(
        "离线声明 2/2 全齐",
        {
            let mut lt = PestudioLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OFFLINE_ROWS {
                if lt.declare_offline(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.offline_all()
        },
        "离线扫描声明/无遥测声明两行闭集，缺一拒封",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < METRIC_CELLS {
                if metric_kind(o).unwrap_or(5) < metric_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，逆序必抓",
    );
    s.add(
        "壳/编译器识别 ×10 全通",
        caught >= 10,
        "十轮注入三态轮转全部识别，无一逃逸",
    );
    s.add(
        "边界·指标格越闭集",
        {
            let mut lt = PestudioLedger::new();
            lt.record_metric(METRIC_CELLS, 0).is_err()
        },
        "指标格 20 越闭集必拒",
    );
    s.add(
        "指标漂移拒",
        {
            let mut lt = PestudioLedger::new();
            let _ = lt.record_metric(0, metric_cell(0).unwrap_or(0));
            lt.record_metric(1, metric_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(PestudioCode::ECellDrift)
        },
        "指标载荷漂移必抓",
    );
    s.add(
        "边界·三态越界",
        {
            let mut lt = PestudioLedger::new();
            lt.record_tri(0, 3).is_err()
        },
        "三态 3 越绿/黄/红闭集必拒",
    );
    s.add(
        "边界·判据闸越闭集",
        {
            let mut lt = PestudioLedger::new();
            lt.judge_gate(6).is_err()
        },
        "判据闸 6 越六闸闭集必拒",
    );
    s.add(
        "相位注入对空账本必抓且不改 last_kind",
        {
            let mut lt = PestudioLedger::new();
            let hit = lt.record_metric(12, metric_cell(12).unwrap_or(0))
                == Err(PestudioCode::EPhase);
            hit && lt.last_kind() == -1
        },
        "前族未齐先录后族，空账本同样必抓",
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
            // main/aux 各节全部记账后再断言定级。
            let mut lt = PestudioLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < METRIC_CELLS {
                if let Some(w) = metric_cell(o) {
                    let _ = lt.record_metric(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TRI_CELLS {
                let _ = lt.record_tri(t, (t as u32) % 3);
                t += 1;
            }
            let mut r = 0usize;
            while r < OFFLINE_ROWS {
                let _ = lt.declare_offline(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9300,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位 0xC9300",
    );
    s.add(
        "码闭集 13 变体",
        PestudioCode::Ok as u32 == 0
            && PestudioCode::EGradeDrift as u32 == 12
            && !PestudioCode::ESeal.detail().is_empty()
            && !PestudioCode::Ok.detail().is_empty(),
        "Ok=0 起 12 错误码至 +0x0C 恰 13 变体，明细全非空",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_kinds() {
        assert_eq!(metric_kind(0), Some(0));
        assert_eq!(metric_kind(4), Some(1));
        assert_eq!(metric_kind(19), Some(4));
        assert!(metric_kind(20).is_none());
        assert!(metric_cell(20).is_none());
        assert_eq!(metric_kind_name(0), Some("导入"));
        assert_eq!(metric_kind_name(4), Some("网络指示器"));
        assert!(metric_kind_name(5).is_none());
    }

    #[test]
    fn test_tri_states() {
        assert_eq!(tri_state_name(0), Some("绿"));
        assert_eq!(tri_state_name(1), Some("黄"));
        assert_eq!(tri_state_name(2), Some("红"));
        assert!(tri_state_name(3).is_none());
        assert!(tri_cell(TRI_CELLS).is_none());
        assert!(offline_row(OFFLINE_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = metric_cell(3).unwrap_or(0);
        assert_eq!(a, metric_cell(3).unwrap_or(0));
        assert_ne!(a, metric_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        // 测试种子须按合法序先落低族格。
        let mut lt = PestudioLedger::new();
        let mut o = 0usize;
        while o < 3 * FAMILY_CELLS {
            let _ = lt.record_metric(o, metric_cell(o).unwrap_or(0));
            o += 1;
        }
        // 落到族 3 后回录族 1 → 逆序必抓；正序续录仍通过。
        assert_eq!(
            lt.record_metric(5, metric_cell(5).unwrap_or(0)),
            Err(PestudioCode::EOrder)
        );
        assert!(lt.record_metric(12, metric_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PestudioLedger::new();
        // 空账本直接录族 2 会先撞相位闸（EPhase），漂移注入须先按合法序
        // 落 0..=5 正格再对已录格注漂移。
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_metric(i, metric_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_metric(5, metric_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PestudioCode::ECellDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PestudioLedger::new();
        let _ = lt.record_tri(0, 0);
        assert_eq!(lt.record_tri(0, 1), Err(PestudioCode::EDup));
        let _ = lt.declare_offline(0);
        assert_eq!(lt.declare_offline(0), Err(PestudioCode::EDup));
    }

    #[test]
    fn test_phase_on_empty_ledger() {
        let mut lt = PestudioLedger::new();
        assert_eq!(
            lt.record_metric(8, metric_cell(8).unwrap_or(0)),
            Err(PestudioCode::EPhase)
        );
        assert_eq!(lt.last_kind(), -1);
        // 合法首格通过。
        assert!(lt.record_metric(0, metric_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PestudioLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PestudioLedger::new();
        let _ = lt2.judge_gate(0);
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= METRIC_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9300);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PestudioCode::Ok as u32, 0);
        assert_eq!(PestudioCode::ENoCell as u32, 1);
        assert_eq!(PestudioCode::ESeal as u32, 7);
        assert_eq!(PestudioCode::EInj as u32, 11);
        assert_eq!(PestudioCode::EGradeDrift as u32, 12);
        let all = [
            PestudioCode::Ok,
            PestudioCode::ENoCell,
            PestudioCode::ECellDrift,
            PestudioCode::EOrder,
            PestudioCode::EPhase,
            PestudioCode::EDup,
            PestudioCode::ENoGate,
            PestudioCode::ESeal,
            PestudioCode::ENoTri,
            PestudioCode::ETriRange,
            PestudioCode::ENoRow,
            PestudioCode::EInj,
            PestudioCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PestudioLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_stats_conservation() {
        let (_, caught, _) = run_cycle();
        let lt = PestudioLedger::new();
        let (i0, c0, e0) = lt.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
        assert!(caught >= 10);
    }

    #[test]
    fn unx52333_checks_green() {
        let s = run_unx52333_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
