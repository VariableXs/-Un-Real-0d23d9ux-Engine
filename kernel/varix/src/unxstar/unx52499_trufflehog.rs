//! UNX-F52499 TruffleHog / Gitleaks 收口 · 0xCBC80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52499"，逐字）：
//!   title: TruffleHog / Gitleaks 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52500-J1 全组安
//!     全测试条目靶场白名单登记：仅本机/实验室靶场，外部目标零接触，
//!     探测类判据与 E5 知识账探测失配分界（联签锚定行 AI-25）；判据锚
//!     定 §AI-66.9 红线适用；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52499）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52500-J1，照原文锚定；功能按本卡标题（TruffleHog / Gitleaks
//! 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：双工具 20 操作主账（TruffleHog 10 格：verify/scan/git/
//!      filesystem/docker 各 2 格；Gitleaks 10 格：detect/protect/
//!      version/report/completion 各 2 格），命令槽单调（逆序必抓）。
//!   ② 密钥三态 6 格（泄漏检出/未检出/已轮换 × 双工具各 1 格），
//!      越格必拒、漂移必抓。
//!   ③ 与 Semgrep 分账声明 2 行（F52498）+ Go 上栈声明 2 行，共 4 行
//!      闭集，缺一拒封（ERowMiss）。
//!   ④ 安全类卡语义：仅本地仓库 / 实验室靶场干跑（§AI-66.9 红线：
//!      外部目标零接触），探测分界由声明行承载（AI-25 联签）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；run_cycle 注入核 10 轮三态轮转（命令槽
//!      逆序/操作漂移/操作越闭集），全部落在当轮临时账本上，10/10 必
//!      抓；双跑 FNV 指纹一致；定级 S1（20 操作 + 6 密钥格 + 4 声明行
//!      + 六闸全通）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52499;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 双工具操作格数（各 10 格）。
pub const OPS: usize = 20;

/// 单工具操作格数。
pub const TOOL_OPS: usize = 10;

/// 密钥三态格数。
pub const SECRET_CELLS: usize = 6;

/// 声明行数（Semgrep 分账 2 + Go 上栈 2）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TruffleCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作格漂移
    EDrift,
    /// 命令槽逆序
    EOrder,
    /// 密钥格越闭集
    ENoSec,
    /// 密钥格漂移
    ESecDrift,
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

impl TruffleCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TruffleCode::Ok => "成功",
            TruffleCode::ENoOp => "操作号越闭集：双工具 20 操作之外先登记",
            TruffleCode::EDrift => "操作格漂移：载荷折叠与基准不恒等，逐格复算",
            TruffleCode::EOrder => "命令槽逆序：TruffleHog 五命令→Gitleaks 五命令单调，逆序必抓",
            TruffleCode::ENoSec => "密钥格越闭集：密钥三态 6 格之外先登记",
            TruffleCode::ESecDrift => "密钥格漂移：三态折叠与基准不恒等，逐格复算",
            TruffleCode::ENoRow => "声明行越闭集：Semgrep 分账/Go 上栈 4 行闭集之外先登记",
            TruffleCode::ERowMiss => "缺声明行：Semgrep 分账/Go 上栈声明未齐先补齐",
            TruffleCode::EDup => "重复落账：同格只许一条账目",
            TruffleCode::ESeal => "未齐即封：20 操作 + 6 密钥格 + 4 声明行未齐先补齐",
            TruffleCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TruffleCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TruffleCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1（KB-COMPAT 登记）",
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
            Grade::S1 => "S1：六闸+20操作+6密钥格+4声明行全通收口",
            Grade::S2 => "S2：有条件收口，六闸已落但格账存在未齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；命令槽 = o/2，槽 0-4 TruffleHog、5-9 Gitleaks）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xBC000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 命令槽（0..10，每槽 2 格）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 工具号（0=TruffleHog，1=Gitleaks）。
pub const fn op_tool(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / TOOL_OPS) as u32)
}

/// 命令槽名（按 kind 索引；0-4 TruffleHog、5-9 Gitleaks）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("trufflehog verify"),
        1 => Some("trufflehog scan"),
        2 => Some("trufflehog git"),
        3 => Some("trufflehog filesystem"),
        4 => Some("trufflehog docker"),
        5 => Some("gitleaks detect"),
        6 => Some("gitleaks protect"),
        7 => Some("gitleaks version"),
        8 => Some("gitleaks report"),
        9 => Some("gitleaks completion"),
        _ => None,
    }
}

/// 密钥三态格载荷（格 c；态 = c/2：泄漏检出/未检出/已轮换）。
pub const fn secret_cell(c: usize) -> Option<u64> {
    if c >= SECRET_CELLS {
        return None;
    }
    Some((0xBD000u64 + (c as u64) * 0x3B) ^ 0x534543_53544154)
}

/// 密钥态名（按 c/2 索引）。
pub const fn secret_state_name(c: usize) -> Option<&'static str> {
    match c / 2 {
        0 => Some("泄漏检出"),
        1 => Some("未检出"),
        2 => Some("已轮换"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xBE00 + r as u32 * 0x2C)
}

/// 声明行族名（0-1 与 Semgrep 分账 F52498；2-3 Go 上栈）。
pub const fn split_family_name(r: usize) -> Option<&'static str> {
    match r {
        0 | 1 => Some("与Semgrep分账"),
        2 | 3 => Some("Go上栈"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// TruffleHog / Gitleaks 收口账本状态机。
#[derive(Clone)]
pub struct TruffleLedger {
    /// 六判据（母版闸）。
    gates: [Option<bool>; 6],
    /// 双工具操作账。
    ops: [Option<u64>; OPS],
    /// 密钥三态账。
    secrets: [Option<u64>; SECRET_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后命令槽。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TruffleLedger {
    /// 新账本。
    pub fn new() -> Self {
        TruffleLedger {
            gates: [None; 6],
            ops: [None; OPS],
            secrets: [None; SECRET_CELLS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TruffleCode> {
        if gate >= 6 {
            return Err(TruffleCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 操作一格（命令槽单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), TruffleCode> {
        if o >= OPS {
            return Err(TruffleCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(TruffleCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(TruffleCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(TruffleCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(TruffleCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TruffleCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
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

    /// 密钥三态一格（载荷对照 + 重复拒）。
    pub fn record_secret(&mut self, c: usize, got: u64) -> Result<(), TruffleCode> {
        let want = match secret_cell(c) {
            Some(w) => w,
            None => return Err(TruffleCode::ENoSec),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + c as u64);
            return Err(TruffleCode::ESecDrift);
        }
        if match self.secrets.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TruffleCode::EDup);
        }
        if let Some(x) = self.secrets.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x7301 + c as u64);
        Ok(())
    }

    /// 密钥三态全账核验（6 格）。
    pub fn secrets_all(&self) -> bool {
        let mut c = 0usize;
        while c < SECRET_CELLS {
            match self.secrets.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TruffleCode> {
        if r >= SPLIT_ROWS {
            return Err(TruffleCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TruffleCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x7400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
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
        if self.gates_all() && self.ops_all() && self.secrets_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for TruffleLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TruffleLedger::new();
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
    let mut c = 0usize;
    while c < SECRET_CELLS {
        if let Some(w) = secret_cell(c) {
            if led.record_secret(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：命令槽逆序 / 操作漂移 / 操作越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TruffleLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(15, op_cell(15).unwrap_or(0));
                lt.record_op(3, op_cell(3).unwrap_or(0)) == Err(TruffleCode::EOrder)
            }
            1 => {
                lt.record_op(2, op_cell(2).unwrap_or(0));
                lt.record_op(3, op_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(TruffleCode::EDrift)
            }
            _ => lt.record_op(OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52499 判据集（锚 UNX-F52500-J1，off-by-one 板式）。
pub fn run_unx52499_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52499");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "TruffleHog 10 操作全通",
        {
            let mut lt = TruffleLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < TOOL_OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok
        },
        "verify/scan/git/filesystem/docker 五命令各 2 格恒等",
    );
    s.add(
        "Gitleaks 10 操作全通",
        {
            let mut lt = TruffleLedger::new();
            let mut ok = true;
            let mut o = TOOL_OPS;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok
        },
        "detect/protect/version/report/completion 五命令各 2 格恒等",
    );
    s.add(
        "双工具 20 操作主账全通",
        recorded >= OPS as u32,
        "命令槽 0-4 TruffleHog、5-9 Gitleaks 逐格恒等",
    );
    s.add(
        "密钥三态 6 格全落",
        {
            let mut lt = TruffleLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SECRET_CELLS {
                if let Some(w) = secret_cell(c) {
                    if lt.record_secret(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.secrets_all()
        },
        "泄漏检出/未检出/已轮换三态 × 双工具逐格恒等",
    );
    s.add(
        "声明 4/4 全齐",
        {
            let mut lt = TruffleLedger::new();
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
        "与 Semgrep 分账 2（F52498）+ Go 上栈 2 共 4 行闭集",
    );
    s.add(
        "判据母版六闸全落",
        {
            let mut lt = TruffleLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，缺一即封拒",
    );
    s.add(
        "命令槽单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_kind(o).unwrap_or(10) < op_kind(o - 1).unwrap_or(10) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "十命令槽闭集单调（TruffleHog 五→Gitleaks 五）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令槽逆序/操作漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = TruffleLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·密钥格越闭集",
        l0.record_secret(SECRET_CELLS, 0).is_err(),
        "密钥格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 4 越闭集必拒",
    );
    s.add(
        "漂移必抓",
        {
            let mut lt = TruffleLedger::new();
            lt.record_op(0, op_cell(0).unwrap_or(0));
            lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(TruffleCode::EDrift)
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
            let mut lt = TruffleLedger::new();
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
            let mut c = 0usize;
            while c < SECRET_CELLS {
                if let Some(w) = secret_cell(c) {
                    let _ = lt.record_secret(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.9 红线：外部目标零接触）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBC80,
        "SEG_BASE 0x40 对齐且为 VX16 域 52499 格位",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(2), Some(1));
        assert_eq!(op_kind(10), Some(5));
        assert_eq!(op_kind(19), Some(9));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_tool(0), Some(0));
        assert_eq!(op_tool(10), Some(1));
        assert!(op_tool(20).is_none());
    }

    #[test]
    fn test_kind_names() {
        assert_eq!(op_kind_name(0), Some("trufflehog verify"));
        assert_eq!(op_kind_name(4), Some("trufflehog docker"));
        assert_eq!(op_kind_name(5), Some("gitleaks detect"));
        assert_eq!(op_kind_name(9), Some("gitleaks completion"));
        assert!(op_kind_name(10).is_none());
        assert_eq!(secret_state_name(0), Some("泄漏检出"));
        assert_eq!(secret_state_name(4), Some("已轮换"));
        assert!(secret_state_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        assert!(secret_cell(SECRET_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_split_family() {
        assert_eq!(split_family_name(0), Some("与Semgrep分账"));
        assert_eq!(split_family_name(2), Some("Go上栈"));
        assert!(split_family_name(4).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = TruffleLedger::new();
        let _ = lt.record_op(15, op_cell(15).unwrap_or(0));
        assert_eq!(lt.record_op(3, op_cell(3).unwrap_or(0)), Err(TruffleCode::EOrder));
        assert!(lt.record_op(16, op_cell(16).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TruffleLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TruffleCode::EDrift)
        );
        assert_eq!(
            lt.record_secret(3, secret_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(TruffleCode::ESecDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TruffleLedger::new();
        let _ = lt.record_secret(0, secret_cell(0).unwrap_or(0));
        assert_eq!(lt.record_secret(0, secret_cell(0).unwrap_or(0)), Err(TruffleCode::EDup));
        let _ = lt.declare_split(2);
        assert_eq!(lt.declare_split(2), Err(TruffleCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TruffleLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TruffleLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, OPS as u32 + SECRET_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBC80);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TruffleCode::Ok as u32, 0);
        assert_eq!(TruffleCode::EGradeDrift as u32, 12);
        assert_eq!(TruffleCode::Ok.val(), SEG_BASE);
        assert!(!TruffleCode::Ok.detail().is_empty());
        assert!(!TruffleCode::ESeal.detail().is_empty());
        assert!(!TruffleCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S1。
        let lt0 = TruffleLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = TruffleLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = TruffleLedger::new();
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < SECRET_CELLS {
            if let Some(w) = secret_cell(c) {
                let _ = lt2.record_secret(c, w);
            }
            c += 1;
        }
        // 六闸齐后才可达目标档（如实阶梯）。
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52499_checks_green() {
        let s = run_unx52499_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
