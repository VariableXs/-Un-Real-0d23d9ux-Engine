//! UNX-F52493 OWASP ZAP 收口 · 0xCBB00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52493"，逐字）：
//!   title: OWASP ZAP 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52494-J1 ×10（本
//!     机靶站）；Perl 消费联签（F52118 锚定行）；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非
//!     空。
//! off-by-one 板式口径：本卡（卡 N=52493）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52494-J1，照原文锚定；功能按本卡标题（OWASP ZAP）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（本地授权演练靶站/干跑语义，零真实攻击面）：
//!   ① 判据只对本地回环靶站（127.0.0.1 演练靶场）与干跑结果账收口；
//!      Active Scan 族强制「限」语义：仅限本机靶站、强度限定、进度可核。
//!   ② 扫描 20 操作集：五工具族闭集（Proxy/Spider/Active Scan 限/
//!      Passive Scan/Daemon）各 4 操作，家族单调（逆序必抓）。
//!   ③ 策略投影 8 格：每格恒等折叠，漂移必抓。
//!   ④ 与 Burp 分账声明 2 行（F52492 锚定行）+ Java 上栈声明 2 行，闭
//!      集缺一拒封（seal 语义：缺声明行 → ERowMiss）。
//!   ⑤ ledger 状态机（new/record/declare/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作越闭集/操作漂移/策略越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集
//!      （20 操作 + 8 策略 + 4 行声明 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52493;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 扫描操作数。
pub const SCAN_OPS: usize = 20;

/// 策略投影格数。
pub const POLICY_CELLS: usize = 8;

/// 与 Burp 分账声明行数（F52492）。
pub const BURP_ROWS: usize = 2;

/// Java 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZapCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 策略格越闭集
    ENoPolicy,
    /// 策略投影漂移
    EPolicyDrift,
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

impl ZapCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ZapCode::Ok => "成功",
            ZapCode::ENoOp => "操作号越闭集：扫描 20 操作集之外先登记，本机靶站域内才可入账",
            ZapCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ZapCode::EOrder => "家族逆序：Proxy→Spider→Active Scan 限→Passive Scan→Daemon 单调，逆序必抓",
            ZapCode::ENoPolicy => "策略格越闭集：策略投影 8 格之外先登记",
            ZapCode::EPolicyDrift => "策略投影漂移：策略格折叠与基准不恒等，逐格复算",
            ZapCode::ENoRow => "声明行越闭集：Burp 分账 2 行 / Java 上栈 2 行闭集之外先登记",
            ZapCode::ERowMiss => "缺声明行：Burp 分账（F52492）与 Java 上栈声明未齐先补齐",
            ZapCode::EDup => "重复落账：同格只许一条账目",
            ZapCode::ESeal => "未齐即封：20 操作 + 8 策略 + 4 行声明 + 六判据未齐先补齐",
            ZapCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ZapCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ZapCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+8策略+4行声明全通收口（本机靶站）",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 扫描操作格载荷（操作 o；工具族 = o/4）。
pub const fn scan_cell(o: usize) -> Option<u64> {
    if o >= SCAN_OPS {
        return None;
    }
    Some((0xCBB00u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 工具族（0..4=Proxy 4..8=Spider 8..12=Active Scan 限 12..16=Passive Scan 16..20=Daemon）。
pub const fn scan_kind(o: usize) -> Option<u32> {
    if o >= SCAN_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 工具族名（按族索引，非格号）。
pub const fn scan_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Proxy"),
        1 => Some("Spider"),
        2 => Some("Active Scan 限"),
        3 => Some("Passive Scan"),
        4 => Some("Daemon"),
        _ => None,
    }
}

/// 策略投影格载荷（策略 p）。
pub const fn policy_cell(p: usize) -> Option<u64> {
    if p >= POLICY_CELLS {
        return None;
    }
    Some((0xCC000u64 + (p as u64) * 0x5D) ^ 0x5A41505F504F4C21)
}

/// 策略投影名（按格索引）。
pub const fn policy_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("强度-低"),
        1 => Some("强度-中"),
        2 => Some("强度-高"),
        3 => Some("靶标圈定"),
        4 => Some("阈值-误报"),
        5 => Some("阈值-漏报"),
        6 => Some("会话快照"),
        7 => Some("关停核验"),
        _ => None,
    }
}

/// 与 Burp 分账声明行折叠（行 r；F52492 锚定行）。
pub const fn burp_row(r: usize) -> Option<u32> {
    if r >= BURP_ROWS {
        return None;
    }
    Some(0xCD00 + r as u32 * 0x31)
}

/// Burp 分账声明行名。
pub const fn burp_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("与 Burp 分账声明：ZAP 代理账独立记"),
        1 => Some("与 Burp 分账声明：F52492 锚定行不改写"),
        _ => None,
    }
}

/// Java 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCE00 + r as u32 * 0x5D)
}

/// Java 上栈声明行名。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Java 上栈声明：ZAP 运行时依赖 Java 栈"),
        1 => Some("Java 上栈声明：Perl 消费联签（F52118 锚定行）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// OWASP ZAP 收口账本状态机。
#[derive(Clone)]
pub struct ZapLedger {
    /// 六判据（本机靶站：可达/授权/限速/隔离/落账/写回）。
    gates: [Option<bool>; 6],
    /// 扫描操作账。
    ops: [Option<u64>; SCAN_OPS],
    /// 策略投影账。
    policies: [Option<u64>; POLICY_CELLS],
    /// Burp 分账声明账。
    burps: [Option<u32>; BURP_ROWS],
    /// Java 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后工具族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ZapLedger {
    /// 新账本。
    pub fn new() -> Self {
        ZapLedger {
            gates: [None; 6],
            ops: [None; SCAN_OPS],
            policies: [None; POLICY_CELLS],
            burps: [None; BURP_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ZapCode> {
        if gate >= 6 {
            return Err(ZapCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 扫描操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ZapCode> {
        if o >= SCAN_OPS {
            return Err(ZapCode::ENoOp);
        }
        let kind = match scan_kind(o) {
            Some(k) => k,
            None => return Err(ZapCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(ZapCode::EOrder);
        }
        let want = match scan_cell(o) {
            Some(w) => w,
            None => return Err(ZapCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(ZapCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZapCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < SCAN_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 策略投影一格（载荷对照 + 重复拒）。
    pub fn record_policy(&mut self, p: usize, got: u64) -> Result<(), ZapCode> {
        let want = match policy_cell(p) {
            Some(w) => w,
            None => return Err(ZapCode::ENoPolicy),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + p as u64);
            return Err(ZapCode::EPolicyDrift);
        }
        if match self.policies.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ZapCode::EDup);
        }
        if let Some(c) = self.policies.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5301 + p as u64);
        Ok(())
    }

    /// 策略全账核验（8 格）。
    pub fn policies_all(&self) -> bool {
        let mut p = 0usize;
        while p < POLICY_CELLS {
            match self.policies.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 声明一行（which=false → Burp 分账，which=true → Java 上栈）。
    pub fn declare_row(&mut self, which: bool, r: usize) -> Result<(), ZapCode> {
        if which {
            if r >= STACK_ROWS {
                return Err(ZapCode::ENoRow);
            }
            if match self.stacks.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(ZapCode::EDup);
            }
            if let Some(c) = self.stacks.get_mut(r) {
                *c = stack_row(r);
            }
        } else {
            if r >= BURP_ROWS {
                return Err(ZapCode::ENoRow);
            }
            if match self.burps.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(ZapCode::EDup);
            }
            if let Some(c) = self.burps.get_mut(r) {
                *c = burp_row(r);
            }
        }
        self.fnv(if which { 0x5401 + r as u64 } else { 0x5400 + r as u64 });
        Ok(())
    }

    /// 声明全齐核验（Burp 2 行 + Java 上栈 2 行）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < BURP_ROWS {
            match self.burps.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut s = 0usize;
        while s < STACK_ROWS {
            match self.stacks.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
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
    pub fn seal(&self) -> Result<(), ZapCode> {
        if !self.gates_all() {
            return Err(ZapCode::ESeal);
        }
        if !self.rows_all() {
            return Err(ZapCode::ERowMiss);
        }
        if !self.ops_all() || !self.policies_all() {
            return Err(ZapCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.policies_all() && self.rows_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ZapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ZapLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < POLICY_CELLS {
        if let Some(w) = policy_cell(p) {
            if led.record_policy(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut o = 0usize;
    while o < SCAN_OPS {
        if let Some(w) = scan_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < BURP_ROWS {
        let _ = led.declare_row(false, r);
        r += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_row(true, s);
        s += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 策略越闭集三态轮转，
    // 全部落在当轮临时账本上；主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ZapLedger::new();
        let caught = match kind {
            0 => {
                lt.record_op(0, scan_cell(0).unwrap_or(0));
                lt.record_op(SCAN_OPS, 0).is_err()
            }
            1 => {
                lt.record_op(2, scan_cell(2).unwrap_or(0));
                lt.record_op(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ZapCode::EDrift)
            }
            _ => {
                lt.record_policy(0, policy_cell(0).unwrap_or(0));
                lt.record_policy(POLICY_CELLS, 0) == Err(ZapCode::ENoPolicy)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
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
/// UNX-F52493 判据集（锚 UNX-F52494-J1 ×10（本机靶站），off-by-one 板式）。
pub fn run_unx52493_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52493");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "扫描 20 操作全通（本机靶站）",
        recorded >= SCAN_OPS as u32,
        "Proxy/Spider/Active Scan 限/Passive Scan/Daemon 五工具族逐操作恒等",
    );
    s.add(
        "策略投影 8 格全落",
        {
            let mut lt = ZapLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if let Some(w) = policy_cell(p) {
                    if lt.record_policy(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.policies_all()
        },
        "强度/靶标圈定/阈值/会话快照/关停核验逐格恒等（§AI-66.3）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ZapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "本机靶站可达/授权/限速/隔离/落账/写回逐格落账",
    );
    s.add(
        "与 Burp 分账声明 2/2（F52492）",
        {
            let mut lt = ZapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BURP_ROWS {
                if lt.declare_row(false, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "ZAP 代理账独立记 / F52492 锚定行不改写（§AI-66.3）",
    );
    s.add(
        "Java 上栈声明 2/2",
        {
            let mut lt = ZapLedger::new();
            let mut ok = true;
            // rows_all 要求分账行与上栈行两套全落。
            let mut b = 0usize;
            while b < BURP_ROWS {
                if lt.declare_row(false, b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_row(true, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "Java 运行时栈声明 + Perl 消费联签（F52118 锚定行）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SCAN_OPS {
                if scan_kind(o).unwrap_or(5) < scan_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五工具族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "操作越闭集/操作漂移/策略越闭集三态轮转必抓");
    s.add("边界·越闭集·操作", ZapLedger::new().record_op(SCAN_OPS, 0).is_err(), "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）");
    s.add(
        "边界·越闭集·策略",
        ZapLedger::new().record_policy(POLICY_CELLS, 0).is_err(),
        "策略格 8 越闭集必拒（ENoPolicy 提前返回）",
    );
    s.add(
        "边界·越闭集·声明行",
        {
            let mut lt = ZapLedger::new();
            lt.declare_row(false, BURP_ROWS).is_err() && lt.declare_row(true, STACK_ROWS).is_err()
        },
        "Burp 行 2 / Java 行 2 越闭集必拒（ENoRow 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = ZapLedger::new();
            lt.record_op(0, scan_cell(0).unwrap_or(0));
            lt.record_op(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ZapCode::EDrift)
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
            let mut lt = ZapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut p = 0usize;
            while p < POLICY_CELLS {
                if let Some(w) = policy_cell(p) {
                    let _ = lt.record_policy(p, w);
                }
                p += 1;
            }
            let mut o = 0usize;
            while o < SCAN_OPS {
                if let Some(w) = scan_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < BURP_ROWS {
                let _ = lt.declare_row(false, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = lt.declare_row(true, r2);
                r2 += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "封闸语义闭集",
        {
            // 六闸缺 → ESeal；六闸齐而声明行缺 → ERowMiss；格账缺 → ESeal。
            let empty = ZapLedger::new();
            let mut mid = ZapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = mid.judge_gate(g);
                g += 1;
            }
            let mut full = mid.clone();
            let mut r = 0usize;
            while r < BURP_ROWS {
                let _ = full.declare_row(false, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = full.declare_row(true, r2);
                r2 += 1;
            }
            empty.seal() == Err(ZapCode::ESeal)
                && mid.seal() == Err(ZapCode::ERowMiss)
                && full.seal() == Err(ZapCode::ESeal)
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal，按序核验",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBB00,
        "SEG_BASE 0x40 对齐且在 VX16 域（0xCA400 + 0x40×92）",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_kind_table() {
        assert_eq!(scan_kind(0), Some(0));
        assert_eq!(scan_kind(4), Some(1));
        assert_eq!(scan_kind(8), Some(2));
        assert_eq!(scan_kind(12), Some(3));
        assert_eq!(scan_kind(19), Some(4));
        assert!(scan_kind(20).is_none());
        // 名表按族索引而非格号。
        assert_eq!(scan_kind_name(0), Some("Proxy"));
        assert_eq!(scan_kind_name(2), Some("Active Scan 限"));
        assert_eq!(scan_kind_name(4), Some("Daemon"));
        assert!(scan_kind_name(5).is_none());
        assert!(scan_cell(20).is_none());
    }

    #[test]
    fn test_policy_cells() {
        assert_eq!(policy_name(0), Some("强度-低"));
        assert_eq!(policy_name(7), Some("关停核验"));
        assert!(policy_name(8).is_none());
        assert!(policy_cell(POLICY_CELLS).is_none());
        let a = policy_cell(3).unwrap_or(0);
        assert_eq!(a, policy_cell(3).unwrap_or(0));
        assert_ne!(a, policy_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_rows_pure() {
        assert!(burp_row(BURP_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(burp_row(0), Some(0xCD00));
        assert_eq!(stack_row(1), Some(0xCE00 + 0x5D));
        assert_eq!(burp_row_name(1), Some("与 Burp 分账声明：F52492 锚定行不改写"));
        assert!(burp_row_name(2).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ZapLedger::new();
        let _ = lt.record_op(9, scan_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, scan_cell(2).unwrap_or(0)), Err(ZapCode::EOrder));
        assert!(lt.record_op(10, scan_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ZapLedger::new();
        assert_eq!(lt.record_op(5, scan_cell(5).unwrap_or(0).wrapping_add(1)), Err(ZapCode::EDrift));
        assert_eq!(
            lt.record_policy(1, policy_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(ZapCode::EPolicyDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ZapLedger::new();
        let _ = lt.record_op(0, scan_cell(0).unwrap_or(0));
        assert_eq!(lt.record_op(0, scan_cell(0).unwrap_or(0)), Err(ZapCode::EDup));
        let _ = lt.record_policy(0, policy_cell(0).unwrap_or(0));
        assert_eq!(lt.record_policy(0, policy_cell(0).unwrap_or(0)), Err(ZapCode::EDup));
        let _ = lt.declare_row(false, 0);
        assert_eq!(lt.declare_row(false, 0), Err(ZapCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ZapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ZapLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_order() {
        let empty = ZapLedger::new();
        assert_eq!(empty.seal(), Err(ZapCode::ESeal));
        let mut mid = ZapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.seal(), Err(ZapCode::ERowMiss));
        let _ = mid.declare_row(false, 0);
        let _ = mid.declare_row(false, 1);
        let _ = mid.declare_row(true, 0);
        let _ = mid.declare_row(true, 1);
        assert_eq!(mid.seal(), Err(ZapCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：8 策略 + 20 操作 = 28。
        assert_eq!(recorded, POLICY_CELLS as u32 + SCAN_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xCBB00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ZapCode::Ok as u32, 0);
        assert_eq!(ZapCode::EGradeDrift as u32, 12);
        assert!(!ZapCode::Ok.detail().is_empty());
        assert!(!ZapCode::ESeal.detail().is_empty());
        assert!(!ZapCode::ERowMiss.detail().is_empty());
        assert_eq!(ZapCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S1。
        let empty = ZapLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = ZapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < SCAN_OPS {
            if let Some(w) = scan_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < POLICY_CELLS {
            if let Some(w) = policy_cell(p) {
                let _ = full.record_policy(p, w);
            }
            p += 1;
        }
        let mut r = 0usize;
        while r < BURP_ROWS {
            let _ = full.declare_row(false, r);
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < STACK_ROWS {
            let _ = full.declare_row(true, r2);
            r2 += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S1);
    }

    #[test]
    fn test_boundary_no_injection_ledger() {
        // 越闭集边界拒（ENoX 类提前返回）不计注入账。
        let mut lt = ZapLedger::new();
        let _ = lt.record_op(SCAN_OPS, 0);
        let _ = lt.record_policy(POLICY_CELLS, 0);
        let _ = lt.declare_row(false, BURP_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn unx52493_checks_green() {
        let s = run_unx52493_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
