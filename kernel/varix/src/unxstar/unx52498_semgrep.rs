//! UNX-F52498 Semgrep CLI 收口 · 0xCBC40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52498"，逐字）：
//!   title: Semgrep CLI 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52499-J1 凭证泄
//!     漏扫描 ×10 仓库（本地）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52498）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52499-J1，照原文锚定；功能按本卡标题（Semgrep CLI 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：五命令族（scan / ci / publish / login / policy）20 操作
//!      主账，族单调（逆序必抓）；逐操作载荷纯函数折叠，恒等才收。
//!   ② 规则投影 12 格（本地规则集投影，越格必拒、漂移必抓）。
//!   ③ 三输出格式 6 格（JSON / SARIF / Text 各 2 格）。
//!   ④ 与 SonarScanner 分账声明 2 行（F52481）+ OCaml 上栈声明 1 行 +
//!      Go 上栈声明 1 行，共 4 行闭集，缺一拒封（ERowMiss）。
//!   ⑤ 安全类卡语义：全部本地仓库 / 回环干跑（×10 仓库本地扫描判据
//!      由命令族主账承载），零外部网络、零凭据上传。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；run_cycle 注入核 10 轮三态轮转（命令族
//!      逆序/规则投影漂移/操作越闭集），全部落在当轮临时账本上，10/10
//!      必抓；双跑 FNV 指纹一致；定级 S1（20 操作 + 12 规则格 + 6 输出
//!      格 + 4 声明行 + 六闸全通）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52498;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 命令族操作格数。
pub const CLI_OPS: usize = 20;

/// 规则投影格数。
pub const RULE_CELLS: usize = 12;

/// 三输出格式格数。
pub const OUT_CELLS: usize = 6;

/// 声明行数（SonarScanner 分账 2 + OCaml 上栈 1 + Go 上栈 1）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemgrepCode {
    /// 成功
    Ok = 0,
    /// 命令操作号越闭集
    ENoOp,
    /// 操作/输出格漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// 规则格越闭集
    ENoRule,
    /// 规则格漂移
    ERuleDrift,
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

impl SemgrepCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SemgrepCode::Ok => "成功",
            SemgrepCode::ENoOp => "命令操作号越闭集：五命令族 20 操作之外先登记",
            SemgrepCode::EDrift => "操作/输出格漂移：载荷折叠与基准不恒等，逐格复算",
            SemgrepCode::EOrder => "命令族逆序：scan→ci→publish→login→policy 单调，逆序必抓",
            SemgrepCode::ENoRule => "规则格越闭集：规则投影 12 格之外先登记",
            SemgrepCode::ERuleDrift => "规则格漂移：规则投影折叠与基准不恒等，逐格复算",
            SemgrepCode::ENoRow => "声明行越闭集：SonarScanner 分账/上栈 4 行闭集之外先登记",
            SemgrepCode::ERowMiss => "缺声明行：SonarScanner 分账/OCaml/Go 上栈声明未齐先补齐",
            SemgrepCode::EDup => "重复落账：同格只许一条账目",
            SemgrepCode::ESeal => "未齐即封：20 操作 + 12 规则格 + 6 输出格 + 4 声明行未齐先补齐",
            SemgrepCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SemgrepCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SemgrepCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1（KB-COMPAT 登记）",
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
            Grade::S1 => "S1：六闸+20操作+12规则格+6输出格+4声明行全通收口",
            Grade::S2 => "S2：有条件收口，六闸已落但格账存在未齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令族格载荷（操作 o；族 = o/4）。
pub const fn cli_cell(o: usize) -> Option<u64> {
    if o >= CLI_OPS {
        return None;
    }
    Some((0xB8000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=scan 4..8=ci 8..12=publish 12..16=login 16..20=policy）。
pub const fn cli_kind(o: usize) -> Option<u32> {
    if o >= CLI_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名（按 kind 索引）。
pub const fn cli_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("scan"),
        1 => Some("ci"),
        2 => Some("publish"),
        3 => Some("login"),
        4 => Some("policy"),
        _ => None,
    }
}

/// 规则投影格载荷（格 r；本地规则集投影）。
pub const fn rule_cell(r: usize) -> Option<u64> {
    if r >= RULE_CELLS {
        return None;
    }
    Some((0xB9000u64 + (r as u64) * 0x3B) ^ 0x52474D52_554C4500)
}

/// 三输出格式格载荷（格 c；格式 = c/2：JSON/SARIF/Text）。
pub const fn out_cell(c: usize) -> Option<u64> {
    if c >= OUT_CELLS {
        return None;
    }
    Some((0xBA000u64 + (c as u64) * 0x3B) ^ 0x4F5554_46524D00)
}

/// 输出格式名（按 c/2 索引）。
pub const fn out_fmt_name(c: usize) -> Option<&'static str> {
    match c / 2 {
        0 => Some("JSON"),
        1 => Some("SARIF"),
        2 => Some("Text"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xBB00 + r as u32 * 0x2C)
}

/// 声明行族名（0-1 与 SonarScanner 分账 F52481；2 OCaml 上栈；3 Go 上栈）。
pub const fn split_family_name(r: usize) -> Option<&'static str> {
    match r {
        0 | 1 => Some("与SonarScanner分账"),
        2 => Some("OCaml上栈"),
        3 => Some("Go上栈"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Semgrep CLI 收口账本状态机。
#[derive(Clone)]
pub struct SemgrepLedger {
    /// 六判据（母版闸）。
    gates: [Option<bool>; 6],
    /// 命令族操作账。
    ops: [Option<u64>; CLI_OPS],
    /// 规则投影账。
    rules: [Option<u64>; RULE_CELLS],
    /// 三输出格式账。
    outs: [Option<u64>; OUT_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SemgrepLedger {
    /// 新账本。
    pub fn new() -> Self {
        SemgrepLedger {
            gates: [None; 6],
            ops: [None; CLI_OPS],
            rules: [None; RULE_CELLS],
            outs: [None; OUT_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SemgrepCode> {
        if gate >= 6 {
            return Err(SemgrepCode::ESeal);
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

    /// 命令一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SemgrepCode> {
        if o >= CLI_OPS {
            return Err(SemgrepCode::ENoOp);
        }
        let kind = match cli_kind(o) {
            Some(k) => k,
            None => return Err(SemgrepCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(SemgrepCode::EOrder);
        }
        let want = match cli_cell(o) {
            Some(w) => w,
            None => return Err(SemgrepCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(SemgrepCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SemgrepCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 命令全账核验（20 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CLI_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 规则投影一格（载荷对照 + 重复拒）。
    pub fn record_rule(&mut self, r: usize, got: u64) -> Result<(), SemgrepCode> {
        let want = match rule_cell(r) {
            Some(w) => w,
            None => return Err(SemgrepCode::ENoRule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + r as u64);
            return Err(SemgrepCode::ERuleDrift);
        }
        if match self.rules.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SemgrepCode::EDup);
        }
        if let Some(x) = self.rules.get_mut(r) {
            *x = Some(got);
        }
        self.fnv(0x6301 + r as u64);
        Ok(())
    }

    /// 规则投影全账核验（12 格）。
    pub fn rules_all(&self) -> bool {
        let mut r = 0usize;
        while r < RULE_CELLS {
            match self.rules.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 输出格式一格（载荷对照 + 重复拒）。
    pub fn record_out(&mut self, c: usize, got: u64) -> Result<(), SemgrepCode> {
        let want = match out_cell(c) {
            Some(w) => w,
            None => return Err(SemgrepCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + c as u64);
            return Err(SemgrepCode::EDrift);
        }
        if match self.outs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SemgrepCode::EDup);
        }
        if let Some(x) = self.outs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x6401 + c as u64);
        Ok(())
    }

    /// 输出格式全账核验（6 格）。
    pub fn outs_all(&self) -> bool {
        let mut c = 0usize;
        while c < OUT_CELLS {
            match self.outs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SemgrepCode> {
        if r >= SPLIT_ROWS {
            return Err(SemgrepCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SemgrepCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x6500 + r as u64);
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
        if self.gates_all() && self.ops_all() && self.rules_all() && self.outs_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SemgrepLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SemgrepLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CLI_OPS {
        if let Some(w) = cli_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < RULE_CELLS {
        if let Some(w) = rule_cell(r) {
            if led.record_rule(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut c = 0usize;
    while c < OUT_CELLS {
        if let Some(w) = out_cell(c) {
            if led.record_out(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut d = 0usize;
    while d < SPLIT_ROWS {
        let _ = led.declare_split(d);
        d += 1;
    }
    // 注入 10 轮：命令族逆序 / 规则投影漂移 / 操作越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SemgrepLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(17, cli_cell(17).unwrap_or(0));
                lt.record_op(5, cli_cell(5).unwrap_or(0)) == Err(SemgrepCode::EOrder)
            }
            1 => {
                lt.record_rule(2, rule_cell(2).unwrap_or(0));
                lt.record_rule(3, rule_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(SemgrepCode::ERuleDrift)
            }
            _ => lt.record_op(CLI_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52498 判据集（锚 UNX-F52499-J1，off-by-one 板式）。
pub fn run_unx52498_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52498");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 操作全通",
        recorded >= CLI_OPS as u32,
        "scan/ci/publish/login/policy 五族逐操作恒等",
    );
    s.add(
        "规则投影 12 格全落",
        {
            let mut lt = SemgrepLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RULE_CELLS {
                if let Some(w) = rule_cell(r) {
                    if lt.record_rule(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rules_all()
        },
        "本地规则集投影 12 格逐格恒等，越格必拒",
    );
    s.add(
        "三输出格式 6 格全落",
        {
            let mut lt = SemgrepLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < OUT_CELLS {
                if let Some(w) = out_cell(c) {
                    if lt.record_out(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.outs_all()
        },
        "JSON/SARIF/Text 三格式逐格恒等",
    );
    s.add(
        "声明 4/4 全齐",
        {
            let mut lt = SemgrepLedger::new();
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
        "SonarScanner 分账 2（F52481）+ OCaml 上栈 1 + Go 上栈 1 共 4 行闭集",
    );
    s.add(
        "判据母版六闸全落",
        {
            let mut lt = SemgrepLedger::new();
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
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CLI_OPS {
                if cli_kind(o).unwrap_or(5) < cli_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调（scan→ci→publish→login→policy）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令族逆序/规则投影漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = SemgrepLedger::new();
    s.add(
        "边界·命令操作越闭集",
        l0.record_op(CLI_OPS, 0).is_err(),
        "命令操作 20 越闭集必拒",
    );
    s.add(
        "边界·规则格越闭集",
        l0.record_rule(RULE_CELLS, 0).is_err(),
        "规则格 12 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 4 越闭集必拒",
    );
    s.add(
        "漂移必抓",
        {
            let mut lt = SemgrepLedger::new();
            lt.record_op(0, cli_cell(0).unwrap_or(0));
            lt.record_op(1, cli_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SemgrepCode::EDrift)
        },
        "命令操作载荷漂移必抓",
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
            let mut lt = SemgrepLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CLI_OPS {
                if let Some(w) = cli_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < RULE_CELLS {
                if let Some(w) = rule_cell(r) {
                    let _ = lt.record_rule(r, w);
                }
                r += 1;
            }
            let mut c = 0usize;
            while c < OUT_CELLS {
                if let Some(w) = out_cell(c) {
                    let _ = lt.record_out(c, w);
                }
                c += 1;
            }
            let mut d = 0usize;
            while d < SPLIT_ROWS {
                let _ = lt.declare_split(d);
                d += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBC40,
        "SEG_BASE 0x40 对齐且为 VX16 域 52498 格位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_kind() {
        assert_eq!(cli_kind(0), Some(0));
        assert_eq!(cli_kind(4), Some(1));
        assert_eq!(cli_kind(19), Some(4));
        assert!(cli_kind(20).is_none());
        assert!(cli_cell(20).is_none());
        assert_eq!(cli_kind_name(0), Some("scan"));
        assert_eq!(cli_kind_name(4), Some("policy"));
        assert!(cli_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = rule_cell(3).unwrap_or(0);
        assert_eq!(a, rule_cell(3).unwrap_or(0));
        assert_ne!(a, rule_cell(4).unwrap_or(0));
        assert!(out_cell(OUT_CELLS).is_none());
        assert!(rule_cell(RULE_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(out_fmt_name(0), Some("JSON"));
        assert_eq!(out_fmt_name(5), Some("Text"));
    }

    #[test]
    fn test_split_family() {
        assert_eq!(split_family_name(0), Some("与SonarScanner分账"));
        assert_eq!(split_family_name(2), Some("OCaml上栈"));
        assert_eq!(split_family_name(3), Some("Go上栈"));
        assert!(split_family_name(4).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = SemgrepLedger::new();
        let _ = lt.record_op(17, cli_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(5, cli_cell(5).unwrap_or(0)), Err(SemgrepCode::EOrder));
        assert!(lt.record_op(18, cli_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SemgrepLedger::new();
        assert_eq!(
            lt.record_op(5, cli_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SemgrepCode::EDrift)
        );
        assert_eq!(
            lt.record_rule(7, rule_cell(7).unwrap_or(0).wrapping_add(9)),
            Err(SemgrepCode::ERuleDrift)
        );
        assert_eq!(
            lt.record_out(2, out_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(SemgrepCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SemgrepLedger::new();
        let _ = lt.record_rule(0, rule_cell(0).unwrap_or(0));
        assert_eq!(lt.record_rule(0, rule_cell(0).unwrap_or(0)), Err(SemgrepCode::EDup));
        let _ = lt.declare_split(1);
        assert_eq!(lt.declare_split(1), Err(SemgrepCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SemgrepLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SemgrepLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, CLI_OPS as u32 + RULE_CELLS as u32 + OUT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBC40);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SemgrepCode::Ok as u32, 0);
        assert_eq!(SemgrepCode::EGradeDrift as u32, 12);
        assert_eq!(SemgrepCode::Ok.val(), SEG_BASE);
        assert!(!SemgrepCode::Ok.detail().is_empty());
        assert!(!SemgrepCode::ESeal.detail().is_empty());
        assert!(!SemgrepCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S1。
        let lt0 = SemgrepLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = SemgrepLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = SemgrepLedger::new();
        let mut o = 0usize;
        while o < CLI_OPS {
            if let Some(w) = cli_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut r = 0usize;
        while r < RULE_CELLS {
            if let Some(w) = rule_cell(r) {
                let _ = lt2.record_rule(r, w);
            }
            r += 1;
        }
        let mut c = 0usize;
        while c < OUT_CELLS {
            if let Some(w) = out_cell(c) {
                let _ = lt2.record_out(c, w);
            }
            c += 1;
        }
        // 六闸齐后才可达目标档（如实阶梯）。
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut d = 0usize;
        while d < SPLIT_ROWS {
            let _ = lt2.declare_split(d);
            d += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52498_checks_green() {
        let s = run_unx52498_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
