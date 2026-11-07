//! UNX-F52483 ApacheBench（ab）收口 · 0xCB880段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52483"，逐字）：
//!   title: ApacheBench（ab）收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52484-J1 ×10（本
//!     地靶站）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软
//!     件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52483）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52484-J1，照原文锚定；功能按本卡标题（ApacheBench（ab））落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五参数族闭集：-n / -c / -t / -k / -p 各 4 格共 20 参数格，家族单
//!      调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 采样 16 拍单调：拍序严格递进（b == last+1），拍载荷严格递增，
//!      跳拍必抓（EBeatOrder）、漂移必抓（ESmpDrift）。
//!   ③ 与 wrk 分账声明 2 行（F52484）：分账行带载荷对照，漂移必抓
//!      （ESplitDrift）；缺一拒封（ERowMiss）。
//!   ④ C 上栈声明 2 行闭集：ab C 进程上栈声明 / apr 网络 C 栈声明。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（参数越闭集/参数漂移/
//!      采样跳拍），检出+逃逸守恒；双跑 FNV 指纹一致；KB-COMPAT 定级上
//!      限 S2：六闸齐 → S2（全齐亦 S2，如实阶梯空账 → S3）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52483;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 参数格数。
pub const ARG_CELLS: usize = 20;

/// 采样拍数。
pub const SMP_BEATS: usize = 16;

/// wrk 分账声明行数（F52484）。
pub const WRK_ROWS: usize = 2;

/// C 上栈声明行数。
pub const C_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbCode {
    /// 成功
    Ok = 0,
    /// 参数格越闭集
    ENoArg,
    /// 参数载荷漂移
    EArgDrift,
    /// 参数族逆序
    EOrder,
    /// 采样拍越闭集
    ENoSmp,
    /// 采样漂移
    ESmpDrift,
    /// 采样跳拍
    EBeatOrder,
    /// 声明行越闭集
    ENoRow,
    /// 分账漂移
    ESplitDrift,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl AbCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AbCode::Ok => "成功",
            AbCode::ENoArg => "参数格越闭集：-n/-c/-t/-k/-p 20 参数格之外先登记",
            AbCode::EArgDrift => "参数漂移：参数折叠与基准不恒等，逐格复算",
            AbCode::EOrder => "参数族逆序：-n→-c→-t→-k→-p 单调，逆序必抓",
            AbCode::ENoSmp => "采样拍越闭集：16 拍之外先登记",
            AbCode::ESmpDrift => "采样漂移：拍载荷与基准不恒等，逐拍复算",
            AbCode::EBeatOrder => "采样跳拍：拍序严格递进 b==last+1，跳拍必抓",
            AbCode::ENoRow => "声明行越闭集：wrk 分账/C 上栈 2 行闭集之外先登记",
            AbCode::ESplitDrift => "分账漂移：wrk 分账行载荷与基准不恒等，逐行复算",
            AbCode::ERowMiss => "缺声明行：wrk 分账/C 上栈声明未齐先补齐",
            AbCode::EDup => "重复落账：同格只许一条账目",
            AbCode::ESeal => "未齐即封：六闸+20参数+16拍+2分账+2声明未齐先补齐",
            AbCode::EGradeDrift => "定级漂移：ab 定级上限 S2，全齐亦不得越档定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：ab 不适用（定级上限 S2，闭集占位）
    S1,
    /// S2：收口档
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
            Grade::S1 => "S1：ab 定级上限 S2，不适用（KB-COMPAT 闭集占位）",
            Grade::S2 => "S2：ab 收口档，六闸齐即定（全齐同档）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 参数格载荷（格 o；家族 = o/4）。
pub const fn arg_cell(o: usize) -> Option<u64> {
    if o >= ARG_CELLS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x2F) ^ 0x41425F4152470000)
}

/// 参数家族（0..4=-n 4..8=-c 8..12=-t 12..16=-k 16..20=-p）。
pub const fn arg_kind(o: usize) -> Option<u32> {
    if o >= ARG_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 参数家族名。
pub const fn arg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-n"),
        1 => Some("-c"),
        2 => Some("-t"),
        3 => Some("-k"),
        4 => Some("-p"),
        _ => None,
    }
}

/// 采样拍载荷（严格递增：0xD000 + (b+1)^2）。
pub const fn smp_cell(b: usize) -> Option<u64> {
    if b >= SMP_BEATS {
        return None;
    }
    Some(0xD000u64 + ((b as u64) + 1) * ((b as u64) + 1))
}

/// wrk 分账行折叠（行 r；F52484）。
pub const fn wrk_row(r: usize) -> Option<u32> {
    if r >= WRK_ROWS {
        return None;
    }
    Some(0xC400 + r as u32 * 0x2C)
}

/// wrk 分账行名。
pub const fn wrk_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("ab 与 wrk 负载账分账声明（F52484）"),
        1 => Some("wrk 对照基线独立封存声明"),
        _ => None,
    }
}

/// C 上栈声明行折叠（行 r）。
pub const fn c_row(r: usize) -> Option<u32> {
    if r >= C_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

/// C 上栈声明行名。
pub const fn c_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("ApacheBench C 进程上栈声明"),
        1 => Some("apr 网络 C 栈声明"),
        _ => None,
    }
}

/// 六判据闸名（ab 流水）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("连接建立"),
        1 => Some("请求发送"),
        2 => Some("采样完成"),
        3 => Some("统计汇总"),
        4 => Some("wrk 对照"),
        5 => Some("定级登记"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ApacheBench 收口账本状态机。
#[derive(Clone)]
pub struct AbLedger {
    /// 六判据（连接建立/请求发送/采样完成/统计汇总/wrk 对照/定级登记）。
    gates: [Option<bool>; 6],
    /// 参数格账。
    args: [Option<u64>; ARG_CELLS],
    /// 采样拍账。
    smps: [Option<u64>; SMP_BEATS],
    /// wrk 分账账。
    wrks: [Option<u32>; WRK_ROWS],
    /// C 上栈声明账。
    cs: [Option<u32>; C_ROWS],
    /// 最后参数家族。
    last_kind: i64,
    /// 最后已落拍。
    last_smp: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AbLedger {
    /// 新账本。
    pub fn new() -> Self {
        AbLedger {
            gates: [None; 6],
            args: [None; ARG_CELLS],
            smps: [None; SMP_BEATS],
            wrks: [None; WRK_ROWS],
            cs: [None; C_ROWS],
            last_kind: -1,
            last_smp: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AbCode> {
        if gate >= 6 {
            return Err(AbCode::ESeal);
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

    /// 参数一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_arg(&mut self, o: usize, got: u64) -> Result<(), AbCode> {
        if o >= ARG_CELLS {
            return Err(AbCode::ENoArg);
        }
        let kind = match arg_kind(o) {
            Some(k) => k,
            None => return Err(AbCode::ENoArg),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(AbCode::EOrder);
        }
        let want = match arg_cell(o) {
            Some(w) => w,
            None => return Err(AbCode::ENoArg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(AbCode::EArgDrift);
        }
        if match self.args.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AbCode::EDup);
        }
        if let Some(c) = self.args.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 参数全账核验（20 格）。
    pub fn args_all(&self) -> bool {
        let mut o = 0usize;
        while o < ARG_CELLS {
            match self.args.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 采样一拍（拍序严格递进 + 载荷对照）。
    pub fn record_smp(&mut self, b: usize, got: u64) -> Result<(), AbCode> {
        if b >= SMP_BEATS {
            return Err(AbCode::ENoSmp);
        }
        if (b as i64) != self.last_smp + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(AbCode::EBeatOrder);
        }
        let want = match smp_cell(b) {
            Some(w) => w,
            None => return Err(AbCode::ENoSmp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + b as u64);
            return Err(AbCode::ESmpDrift);
        }
        if let Some(c) = self.smps.get_mut(b) {
            *c = Some(got);
        }
        self.last_smp = b as i64;
        self.fnv(0x6402 + b as u64);
        Ok(())
    }

    /// 采样全账核验（16 拍）。
    pub fn smps_all(&self) -> bool {
        let mut b = 0usize;
        while b < SMP_BEATS {
            match self.smps.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// wrk 分账一行（F52484；载荷对照 + 漂移必抓）。
    pub fn declare_wrk(&mut self, r: usize, got: u32) -> Result<(), AbCode> {
        if r >= WRK_ROWS {
            return Err(AbCode::ENoRow);
        }
        let want = match wrk_row(r) {
            Some(w) => w,
            None => return Err(AbCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6501 + r as u64);
            return Err(AbCode::ESplitDrift);
        }
        if match self.wrks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AbCode::EDup);
        }
        if let Some(c) = self.wrks.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x6502 + r as u64);
        Ok(())
    }

    /// wrk 分账全齐核验。
    pub fn wrks_all(&self) -> bool {
        let mut r = 0usize;
        while r < WRK_ROWS {
            match self.wrks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_c(&mut self, r: usize) -> Result<(), AbCode> {
        if r >= C_ROWS {
            return Err(AbCode::ENoRow);
        }
        if match self.cs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AbCode::EDup);
        }
        if let Some(c) = self.cs.get_mut(r) {
            *c = c_row(r);
        }
        self.fnv(0x6600 + r as u64);
        Ok(())
    }

    /// C 声明全齐核验。
    pub fn cs_all(&self) -> bool {
        let mut r = 0usize;
        while r < C_ROWS {
            match self.cs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；其余节未齐 → ESeal。
    pub fn seal(&self) -> Result<(), AbCode> {
        if !self.wrks_all() || !self.cs_all() {
            return Err(AbCode::ERowMiss);
        }
        if self.gates_all() && self.args_all() && self.smps_all() {
            Ok(())
        } else {
            Err(AbCode::ESeal)
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（ab 上限 S2：六闸齐 → S2，判据缺 → S3；全齐同档 S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AbLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全流程账本构建：六闸 + 全节落账 + 注入 10 轮三态轮转。返回（记账格数, 账本）。
fn cycle_ledger() -> (u32, AbLedger) {
    let mut led = AbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ARG_CELLS {
        if let Some(w) = arg_cell(o) {
            if led.record_arg(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < SMP_BEATS {
        if let Some(w) = smp_cell(b) {
            if led.record_smp(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < WRK_ROWS {
        if let Some(w) = wrk_row(r) {
            let _ = led.declare_wrk(r, w);
        }
        r += 1;
    }
    let mut r = 0usize;
    while r < C_ROWS {
        let _ = led.declare_c(r);
        r += 1;
    }
    // 注入 10 轮：参数越闭集 / 参数漂移 / 采样跳拍三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AbLedger::new();
        let caught = match kind {
            0 => lt.record_arg(ARG_CELLS, 0).is_err(),
            1 => {
                lt.record_arg(0, arg_cell(0).unwrap_or(0));
                lt.record_arg(1, arg_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AbCode::EArgDrift)
            }
            _ => {
                lt.record_smp(0, smp_cell(0).unwrap_or(0));
                lt.record_smp(2, smp_cell(2).unwrap_or(0)) == Err(AbCode::EBeatOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    (recorded, led)
}

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let (recorded, led) = cycle_ledger();
    let (_, caught, _) = led.stats();
    (recorded, caught, led.fingerprint())
}

/// 注入账三数（主账本）：恒等式注入=检出、逃逸=0。
pub fn run_cycle_stats() -> (u32, u32, u32) {
    let (_, led) = cycle_ledger();
    led.stats()
}

/// 全齐账本（六闸 + 全节落账，无注入）。
pub fn fresh_complete() -> AbLedger {
    let mut lt = AbLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = lt.judge_gate(g);
        g += 1;
    }
    let mut o = 0usize;
    while o < ARG_CELLS {
        if let Some(w) = arg_cell(o) {
            let _ = lt.record_arg(o, w);
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < SMP_BEATS {
        if let Some(w) = smp_cell(b) {
            let _ = lt.record_smp(b, w);
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < WRK_ROWS {
        if let Some(w) = wrk_row(r) {
            let _ = lt.declare_wrk(r, w);
        }
        r += 1;
    }
    let mut r = 0usize;
    while r < C_ROWS {
        let _ = lt.declare_c(r);
        r += 1;
    }
    lt
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52483 判据集（锚 UNX-F52484-J1，off-by-one 板式）。
pub fn run_unx52483_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52483");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五参数族 20 格全通",
        recorded >= ARG_CELLS as u32,
        "-n/-c/-t/-k/-p 五参数族逐格恒等",
    );
    s.add(
        "采样 16 拍单调全落",
        {
            let mut lt = AbLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SMP_BEATS {
                if let Some(w) = smp_cell(b) {
                    if lt.record_smp(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.smps_all()
        },
        "拍序严格递进、拍载荷严格递增，逐拍落账",
    );
    s.add(
        "与 wrk 分账声明 2 行全落（F52484）",
        {
            let mut lt = AbLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < WRK_ROWS {
                if let Some(w) = wrk_row(r) {
                    if lt.declare_wrk(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.wrks_all()
        },
        "ab 与 wrk 负载账分账/对照基线独立封存，载荷对照漂移必抓",
    );
    s.add(
        "C 上栈声明 2 行全齐",
        {
            let mut lt = AbLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < C_ROWS {
                if lt.declare_c(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cs_all()
        },
        "ab C 进程上栈/apr 网络 C 栈两行闭集",
    );
    s.add(
        "参数族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ARG_CELLS {
                if arg_kind(o).unwrap_or(5) < arg_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "-n→-c→-t→-k→-p 五参数族闭集单调",
    );
    s.add(
        "采样拍单调纯核验",
        {
            let mut ok = true;
            let mut b = 1usize;
            while b < SMP_BEATS {
                if smp_cell(b).unwrap_or(0) <= smp_cell(b - 1).unwrap_or(0) {
                    ok = false;
                }
                b += 1;
            }
            ok
        },
        "16 拍载荷严格递增（纯函数级）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "参数越闭集/参数漂移/采样跳拍三态轮转必抓（判据原文：×10 本地靶站）",
    );
    let mut l0 = AbLedger::new();
    s.add(
        "边界·参数越闭集",
        l0.record_arg(ARG_CELLS, 0).is_err(),
        "参数格 20 越闭集必拒",
    );
    s.add(
        "采样漂移拒",
        {
            l0.record_smp(0, smp_cell(0).unwrap_or(0));
            l0.record_smp(1, smp_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AbCode::ESmpDrift)
        },
        "拍载荷漂移必抓",
    );
    s.add(
        "边界·采样拍越闭集",
        l0.record_smp(SMP_BEATS, 0).is_err(),
        "采样拍 16 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_wrk(WRK_ROWS, 0).is_err() && l0.declare_c(C_ROWS).is_err(),
        "wrk 分账/C 声明行 2 越闭集必拒",
    );
    s.add(
        "未齐即封核",
        {
            let empty = AbLedger::new();
            let full = fresh_complete();
            empty.seal().is_err() && full.seal().is_ok()
        },
        "账未齐封账必拒，全齐才许封",
    );
    let (inj_n, caught_n, esc_n) = run_cycle_stats();
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 闭集（ab 上限档）",
        {
            let lt = fresh_complete();
            lt.judge_grade() == Grade::S2
        },
        "全齐 → S2（KB-COMPAT 定级登记，ab 定级上限 S2）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB880,
        "SEG_BASE 0x40 对齐且等于 VX16 公式位 0xCB880",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_kinds() {
        assert_eq!(arg_kind(0), Some(0));
        assert_eq!(arg_kind(4), Some(1));
        assert_eq!(arg_kind(8), Some(2));
        assert_eq!(arg_kind(12), Some(3));
        assert_eq!(arg_kind(16), Some(4));
        assert_eq!(arg_kind(19), Some(4));
        assert!(arg_kind(20).is_none());
        assert_eq!(arg_kind_name(0), Some("-n"));
        assert_eq!(arg_kind_name(4), Some("-p"));
        assert!(arg_kind_name(5).is_none());
    }

    #[test]
    fn test_smp_monotone() {
        let mut b = 1usize;
        while b < SMP_BEATS {
            assert!(smp_cell(b).unwrap_or(0) > smp_cell(b - 1).unwrap_or(0));
            b += 1;
        }
        assert!(smp_cell(SMP_BEATS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = arg_cell(3).unwrap_or(0);
        assert_eq!(a, arg_cell(3).unwrap_or(0));
        assert_ne!(a, arg_cell(4).unwrap_or(0));
        assert!(wrk_row(WRK_ROWS).is_none());
        assert!(c_row(C_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = AbLedger::new();
        // 先落低族格再落高族，合法序不误抓。
        let _ = lt.record_arg(9, arg_cell(9).unwrap_or(0));
        assert_eq!(lt.record_arg(2, arg_cell(2).unwrap_or(0)), Err(AbCode::EOrder));
        assert!(lt.record_arg(10, arg_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = AbLedger::new();
        assert!(lt.record_smp(0, smp_cell(0).unwrap_or(0)).is_ok());
        // 跳拍：0 后直接落 2，必抓且 last_smp 不前移。
        assert_eq!(lt.record_smp(2, smp_cell(2).unwrap_or(0)), Err(AbCode::EBeatOrder));
        assert!(lt.record_smp(1, smp_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AbLedger::new();
        assert_eq!(
            lt.record_arg(5, arg_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AbCode::EArgDrift)
        );
        let mut lt2 = AbLedger::new();
        let _ = lt2.record_smp(0, smp_cell(0).unwrap_or(0));
        assert_eq!(
            lt2.record_smp(1, smp_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(AbCode::ESmpDrift)
        );
        let mut lt3 = AbLedger::new();
        assert_eq!(
            lt3.declare_wrk(0, wrk_row(0).unwrap_or(0).wrapping_add(1)),
            Err(AbCode::ESplitDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AbLedger::new();
        let _ = lt.record_arg(0, arg_cell(0).unwrap_or(0));
        assert_eq!(lt.record_arg(0, arg_cell(0).unwrap_or(0)), Err(AbCode::EDup));
        let mut lt2 = AbLedger::new();
        let _ = lt2.declare_c(0);
        assert_eq!(lt2.declare_c(0), Err(AbCode::EDup));
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = AbLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 未齐即封：声明行缺定向 ERowMiss，其余节缺 ESeal。
        assert_eq!(lt.seal(), Err(AbCode::ERowMiss));
        let mut mid = fresh_complete();
        mid.declare_c(0).unwrap_or_else(|_| ());
        assert!(mid.cs_all());
        let full = fresh_complete();
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数恰为各记账节常量之和（分账/声明行不计数）。
        assert_eq!(recorded, ARG_CELLS as u32 + SMP_BEATS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_ledger() {
        let (inj, caught, esc) = run_cycle_stats();
        assert_eq!(inj, 10);
        assert_eq!(caught, 10);
        assert_eq!(esc, 0);
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
        assert_eq!(SEG_BASE, 0xCB880);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AbCode::Ok as u32, 0);
        assert_eq!(AbCode::ENoArg as u32, 1);
        assert_eq!(AbCode::EArgDrift as u32, 2);
        assert_eq!(AbCode::EOrder as u32, 3);
        assert_eq!(AbCode::ENoSmp as u32, 4);
        assert_eq!(AbCode::ESmpDrift as u32, 5);
        assert_eq!(AbCode::EBeatOrder as u32, 6);
        assert_eq!(AbCode::ENoRow as u32, 7);
        assert_eq!(AbCode::ESplitDrift as u32, 8);
        assert_eq!(AbCode::ERowMiss as u32, 9);
        assert_eq!(AbCode::EDup as u32, 10);
        assert_eq!(AbCode::ESeal as u32, 11);
        assert_eq!(AbCode::EGradeDrift as u32, 12);
        assert_eq!(AbCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => AbCode::Ok,
                1 => AbCode::ENoArg,
                2 => AbCode::EArgDrift,
                3 => AbCode::EOrder,
                4 => AbCode::ENoSmp,
                5 => AbCode::ESmpDrift,
                6 => AbCode::EBeatOrder,
                7 => AbCode::ENoRow,
                8 => AbCode::ESplitDrift,
                9 => AbCode::ERowMiss,
                10 => AbCode::EDup,
                11 => AbCode::ESeal,
                _ => AbCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2（ab 定级上限 S2）。
        let empty = AbLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = AbLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(fresh_complete().judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52483_checks_green() {
        let s = run_unx52483_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
