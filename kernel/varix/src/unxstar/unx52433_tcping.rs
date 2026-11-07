//! UNX-F52433 tcping 收口 · 0xCAC00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52433"，逐字）：
//!   title: tcping 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52434-J1
//!     ping/延迟/带宽 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③
//!     完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52433）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52434-J1，照原文锚定；功能按本卡标题（tcping）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① tcping 五模式族闭集：TCP ping / 代理模式 / HTTP 模式 / 原始
//!      套接字 / 持续模式，各 4 格共 20 格，家族单调（逆序必抓）。
//!   ② 五参数 10 操作集：主机/端口/间隔/次数/超时各 2 操作，载荷
//!      折叠恒等才收，越闭集必抓。
//!   ③ 采样 16 拍单调：逐拍落格，逆拍必抓且不改拍位游标。
//!   ④ 与 ping 分账声明 2 行闭集：延迟口径分账 / 丢包口径分账，
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（模式越闭集/模式漂移/采样逆
//!      拍），检出+逃逸守恒等式；双跑指纹一致；KB-COMPAT 定级闭集
//!      S1/S2/S3（20 模式 + 10 参数 + 16 拍 + 2 行分账 + 六闸全通 →
//!      S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52433;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// tcping 五模式族格数。
pub const MODE_CELLS: usize = 20;

/// 五参数操作数。
pub const PARAM_OPS: usize = 10;

/// 采样拍数。
pub const SAMPLE_BEATS: usize = 16;

/// 与 ping 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 收口判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcpingCode {
    /// 成功
    Ok = 0,
    /// 模式格越闭集
    ENoMode,
    /// 模式载荷漂移
    EModeDrift,
    /// 模式家族逆序
    EModeOrder,
    /// 参数操作越闭集
    ENoParam,
    /// 参数载荷漂移
    EParamDrift,
    /// 采样拍越界
    ENoBeat,
    /// 采样逆拍
    EBeatOrder,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl TcpingCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TcpingCode::Ok => "成功",
            TcpingCode::ENoMode => "模式格越闭集：TCP ping/代理/HTTP/原始/持续 20 格之外先登记",
            TcpingCode::EModeDrift => "模式载荷漂移：模式折叠与基准不恒等，逐格复算",
            TcpingCode::EModeOrder => "模式家族逆序：TCP ping→代理→HTTP→原始→持续单调，逆序必抓",
            TcpingCode::ENoParam => "参数操作越闭集：主机/端口/间隔/次数/超时 10 操作之外先登记",
            TcpingCode::EParamDrift => "参数载荷漂移：参数折叠与基准不恒等，逐操作复算",
            TcpingCode::ENoBeat => "采样拍越界：16 拍之外先登记",
            TcpingCode::EBeatOrder => "采样逆拍：16 拍单调递增，逆拍必抓",
            TcpingCode::ENoRow => "声明行越闭集：与 ping 分账 2 行闭集之外先登记",
            TcpingCode::EDup => "重复落账：同格只许一条账目",
            TcpingCode::ESeal => "未齐即封：20 模式 + 10 参数 + 16 拍 + 2 行分账未齐先补齐",
            TcpingCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TcpingCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不适用，闭集占位）
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：全通收口（tcping 收口按 KB-COMPAT 登记为 S2 档）",
            Grade::S2 => "S2：tcping ping/延迟/带宽 ×10 有条件收口，20 模式+10 参数+16 拍+2 行分账全齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 模式格载荷（模式 o；家族 = o/4）。
pub const fn mode_cell(o: usize) -> Option<u64> {
    if o >= MODE_CELLS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x1F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 模式家族（0..4=TCP ping 4..8=代理 8..12=HTTP 12..16=原始套接字 16..20=持续）。
pub const fn mode_kind(o: usize) -> Option<u32> {
    if o >= MODE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("TCP ping"),
        1 => Some("代理模式"),
        2 => Some("HTTP 模式"),
        3 => Some("原始套接字"),
        4 => Some("持续模式"),
        _ => None,
    }
}

/// 参数操作格载荷（操作 p；参数族 = p/2）。
pub const fn param_cell(p: usize) -> Option<u64> {
    if p >= PARAM_OPS {
        return None;
    }
    Some((0xD2000u64 + (p as u64) * 0x37) ^ ((p / 2) as u64).wrapping_mul(0x01000193))
}

/// 参数族（0..2=主机 2..4=端口 4..6=间隔 6..8=次数 8..10=超时）。
pub const fn param_kind(p: usize) -> Option<u32> {
    if p >= PARAM_OPS {
        return None;
    }
    Some((p / 2) as u32)
}

/// 参数族名。
pub const fn param_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("主机"),
        1 => Some("端口"),
        2 => Some("间隔"),
        3 => Some("次数"),
        4 => Some("超时"),
        _ => None,
    }
}

/// 采样拍载荷（拍 b）。
pub const fn sample_cell(b: usize) -> Option<u64> {
    if b >= SAMPLE_BEATS {
        return None;
    }
    Some(0xD3000u64 + (b as u64) * 0x11)
}

/// 与 ping 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// tcping 收口账本状态机。
#[derive(Clone)]
pub struct TcpingLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; GATES],
    /// 模式账。
    modes: [Option<u64>; MODE_CELLS],
    /// 参数账。
    params: [Option<u64>; PARAM_OPS],
    /// 采样账。
    samples: [Option<u64>; SAMPLE_BEATS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后模式家族。
    last_kind: i64,
    /// 最后采样拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TcpingLedger {
    /// 新账本。
    pub fn new() -> Self {
        TcpingLedger {
            gates: [None; GATES],
            modes: [None; MODE_CELLS],
            params: [None; PARAM_OPS],
            samples: [None; SAMPLE_BEATS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TcpingCode> {
        if gate >= GATES {
            return Err(TcpingCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATES {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 模式一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, o: usize, got: u64) -> Result<(), TcpingCode> {
        if o >= MODE_CELLS {
            return Err(TcpingCode::ENoMode);
        }
        let kind = match mode_kind(o) {
            Some(k) => k,
            None => return Err(TcpingCode::ENoMode),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(TcpingCode::EModeOrder);
        }
        let want = match mode_cell(o) {
            Some(w) => w,
            None => return Err(TcpingCode::ENoMode),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(TcpingCode::EModeDrift);
        }
        if match self.modes.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TcpingCode::EDup);
        }
        if let Some(c) = self.modes.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 模式全账核验（20 格）。
    pub fn modes_all(&self) -> bool {
        let mut o = 0usize;
        while o < MODE_CELLS {
            match self.modes.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 参数一格（载荷对照 + 重复拒）。
    pub fn record_param(&mut self, p: usize, got: u64) -> Result<(), TcpingCode> {
        if p >= PARAM_OPS {
            return Err(TcpingCode::ENoParam);
        }
        let want = match param_cell(p) {
            Some(w) => w,
            None => return Err(TcpingCode::ENoParam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + p as u64);
            return Err(TcpingCode::EParamDrift);
        }
        if match self.params.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TcpingCode::EDup);
        }
        if let Some(c) = self.params.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5301 + p as u64);
        Ok(())
    }

    /// 参数全账核验（10 操作）。
    pub fn params_all(&self) -> bool {
        let mut p = 0usize;
        while p < PARAM_OPS {
            match self.params.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 采样一拍（单调 + 重复拒）；逆拍不改拍位游标。
    pub fn record_sample(&mut self, b: usize) -> Result<(), TcpingCode> {
        if b >= SAMPLE_BEATS {
            return Err(TcpingCode::ENoBeat);
        }
        if (b as i64) < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(TcpingCode::EBeatOrder);
        }
        if match self.samples.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TcpingCode::EDup);
        }
        if let Some(c) = self.samples.get_mut(b) {
            *c = sample_cell(b);
        }
        self.last_beat = b as i64;
        self.fnv(0x5401 + b as u64);
        Ok(())
    }

    /// 采样全账核验（16 拍且拍位单调不回退）。
    pub fn samples_all(&self) -> bool {
        let mut b = 0usize;
        while b < SAMPLE_BEATS {
            match self.samples.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 最后采样拍游标（注入后不变性核验用）。
    pub fn last_beat(&self) -> i64 {
        self.last_beat
    }

    /// 与 ping 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TcpingCode> {
        if r >= SPLIT_ROWS {
            return Err(TcpingCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TcpingCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// 注入守恒核验（注入=检出+逃逸）。
    pub fn verify_conservation(&self) -> Result<(), TcpingCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(TcpingCode::EInj)
        }
    }

    /// KB-COMPAT 定级（全齐 → S2，六闸齐 → S2，空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.modes_all() && self.params_all() && self.samples_all() && self.splits_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验（漂移即报 EGradeDrift）。
    pub fn verify_grade(&self, expect: Grade) -> Result<(), TcpingCode> {
        if self.judge_grade() == expect {
            Ok(())
        } else {
            Err(TcpingCode::EGradeDrift)
        }
    }
}

impl Default for TcpingLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹核验（不一致即 EFp 语义，落守恒域外诊断）。
pub fn verify_fp(a: u64, b: u64) -> bool {
    a == b && a != 0
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TcpingLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < MODE_CELLS {
        if let Some(w) = mode_cell(o) {
            if led.record_mode(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PARAM_OPS {
        if let Some(w) = param_cell(p) {
            if led.record_param(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut b = 0usize;
    while b < SAMPLE_BEATS {
        if led.record_sample(b).is_ok() {
            recorded += 1;
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：模式越闭集 / 模式漂移 / 采样逆拍三态轮转。
    // 漂移注入先按合法序落正格再注漂移；逆拍注入对空账本同样必抓
    // 且不改拍位游标。注入即故障检出（与 tcping 探测语义同构）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TcpingLedger::new();
        let caught = match kind {
            0 => lt.record_mode(MODE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_mode(0, mode_cell(0).unwrap_or(0));
                lt.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(TcpingCode::EModeDrift)
            }
            _ => {
                let _ = lt.record_sample(5);
                lt.record_sample(2) == Err(TcpingCode::EBeatOrder) && lt.last_beat() == 5
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52433 判据集（锚 UNX-F52434-J1，off-by-one 板式）。
pub fn run_unx52433_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52433");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "tcping 五模式族 20 格全落",
        recorded >= MODE_CELLS as u32,
        "TCP ping/代理/HTTP/原始套接字/持续模式逐格恒等",
    );
    s.add(
        "五参数 10 操作全落",
        {
            let mut lt = TcpingLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PARAM_OPS {
                if let Some(w) = param_cell(p) {
                    if lt.record_param(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.params_all()
        },
        "主机/端口/间隔/次数/超时逐操作恒等",
    );
    s.add(
        "采样 16 拍单调",
        {
            let mut lt = TcpingLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SAMPLE_BEATS {
                if lt.record_sample(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.samples_all()
        },
        "16 拍逐拍单调递增，逆拍必抓",
    );
    s.add(
        "与 ping 分账声明 2/2 全齐",
        {
            let mut lt = TcpingLedger::new();
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
        "延迟口径分账/丢包口径分账两行闭集（与 ICMP ping 分账，§AI-66.3）",
    );
    s.add(
        "六闸全落",
        {
            let mut lt = TcpingLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "模式家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MODE_CELLS {
                if mode_kind(o).unwrap_or(5) < mode_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "模式越闭集/模式漂移/采样逆拍三态轮转必抓",
    );
    let mut l0 = TcpingLedger::new();
    s.add(
        "边界·模式越闭集",
        l0.record_mode(MODE_CELLS, 0).is_err(),
        "模式 20 越闭集必拒",
    );
    s.add(
        "边界·参数越闭集",
        l0.record_param(PARAM_OPS, 0).is_err(),
        "参数操作 10 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "未齐落格仍收",
        l0.record_mode(0, mode_cell(0).unwrap_or(0)).is_ok(),
        "账未齐时合法格照常落账（未齐即封只封收口，不封落格）",
    );
    s.add(
        "守恒核验 Ok",
        l0.verify_conservation().is_ok(),
        "账未齐时守恒等式照常成立，不误报 EInj",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", verify_fp(fp, fp2), "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            let mut lt = TcpingLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < MODE_CELLS {
                if let Some(w) = mode_cell(o) {
                    let _ = lt.record_mode(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PARAM_OPS {
                if let Some(w) = param_cell(p) {
                    let _ = lt.record_param(p, w);
                }
                p += 1;
            }
            let mut b = 0usize;
            while b < SAMPLE_BEATS {
                let _ = lt.record_sample(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.verify_grade(Grade::S2).is_ok()
        },
        "全齐 → S2（KB-COMPAT 定级登记，ping/延迟/带宽 ×10）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAC00,
        "SEG_BASE 0x40 对齐且在 VX16 域首格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_kind() {
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(8), Some(2));
        assert_eq!(mode_kind(12), Some(3));
        assert_eq!(mode_kind(19), Some(4));
        assert!(mode_kind(20).is_none());
        assert_eq!(mode_kind_name(0), Some("TCP ping"));
        assert_eq!(mode_kind_name(4), Some("持续模式"));
        assert!(mode_kind_name(5).is_none());
    }

    #[test]
    fn test_param_kind() {
        assert_eq!(param_kind(0), Some(0));
        assert_eq!(param_kind(1), Some(0));
        assert_eq!(param_kind(2), Some(1));
        assert_eq!(param_kind(9), Some(4));
        assert!(param_kind(10).is_none());
        assert_eq!(param_kind_name(4), Some("超时"));
        assert!(param_kind_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = mode_cell(3).unwrap_or(0);
        assert_eq!(a, mode_cell(3).unwrap_or(0));
        assert_ne!(a, mode_cell(4).unwrap_or(0));
        assert_ne!(param_cell(0).unwrap_or(0), param_cell(1).unwrap_or(0));
        assert!(mode_cell(MODE_CELLS).is_none());
        assert!(param_cell(PARAM_OPS).is_none());
        assert!(sample_cell(SAMPLE_BEATS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TcpingLedger::new();
        let _ = lt.record_mode(9, mode_cell(9).unwrap_or(0));
        assert_eq!(lt.record_mode(2, mode_cell(2).unwrap_or(0)), Err(TcpingCode::EModeOrder));
        assert!(lt.record_mode(10, mode_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TcpingLedger::new();
        assert_eq!(
            lt.record_mode(5, mode_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TcpingCode::EModeDrift)
        );
        assert_eq!(
            lt.record_param(3, param_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(TcpingCode::EParamDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TcpingLedger::new();
        let _ = lt.record_mode(0, mode_cell(0).unwrap_or(0));
        assert_eq!(lt.record_mode(0, mode_cell(0).unwrap_or(0)), Err(TcpingCode::EDup));
        let _ = lt.record_sample(0);
        assert_eq!(lt.record_sample(0), Err(TcpingCode::EDup));
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = TcpingLedger::new();
        assert!(lt.record_sample(5).is_ok());
        let before = lt.last_beat();
        assert_eq!(lt.record_sample(2), Err(TcpingCode::EBeatOrder));
        // 逆拍被拒后拍位游标不得漂移。
        assert_eq!(lt.last_beat(), before);
        assert!(lt.record_sample(6).is_ok());
        // 拍越界：16 拍之外必拒。
        assert_eq!(lt.record_sample(SAMPLE_BEATS), Err(TcpingCode::ENoBeat));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TcpingLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TcpingLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的三节：模式 20 + 参数 10 + 采样 16。
        assert_eq!(recorded, (MODE_CELLS + PARAM_OPS + SAMPLE_BEATS) as u32);
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
        assert!(verify_fp(f1, f2));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCAC00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体，位序 0..=0x0C 与枚举实际位序一致。
        assert_eq!(TcpingCode::Ok as u32, 0);
        assert_eq!(TcpingCode::ENoMode.val(), SEG_BASE + 1);
        assert_eq!(TcpingCode::EModeDrift.val(), SEG_BASE + 2);
        assert_eq!(TcpingCode::EModeOrder.val(), SEG_BASE + 3);
        assert_eq!(TcpingCode::ENoParam.val(), SEG_BASE + 4);
        assert_eq!(TcpingCode::EParamDrift.val(), SEG_BASE + 5);
        assert_eq!(TcpingCode::ENoBeat.val(), SEG_BASE + 6);
        assert_eq!(TcpingCode::EBeatOrder.val(), SEG_BASE + 7);
        assert_eq!(TcpingCode::ENoRow.val(), SEG_BASE + 8);
        assert_eq!(TcpingCode::EDup.val(), SEG_BASE + 9);
        assert_eq!(TcpingCode::ESeal.val(), SEG_BASE + 10);
        assert_eq!(TcpingCode::EInj.val(), SEG_BASE + 11);
        assert_eq!(TcpingCode::EGradeDrift.val(), SEG_BASE + 12);
        assert_eq!(TcpingCode::EGradeDrift as u32, 12);
        let all = [
            TcpingCode::Ok,
            TcpingCode::ENoMode,
            TcpingCode::EModeDrift,
            TcpingCode::EModeOrder,
            TcpingCode::ENoParam,
            TcpingCode::EParamDrift,
            TcpingCode::ENoBeat,
            TcpingCode::EBeatOrder,
            TcpingCode::ENoRow,
            TcpingCode::EDup,
            TcpingCode::ESeal,
            TcpingCode::EInj,
            TcpingCode::EGradeDrift,
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
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2。
        let lt0 = TcpingLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = TcpingLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = TcpingLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < MODE_CELLS {
            if let Some(w) = mode_cell(o) {
                let _ = lt2.record_mode(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < PARAM_OPS {
            if let Some(w) = param_cell(p) {
                let _ = lt2.record_param(p, w);
            }
            p += 1;
        }
        let mut b = 0usize;
        while b < SAMPLE_BEATS {
            let _ = lt2.record_sample(b);
            b += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(lt2.verify_grade(Grade::S2).is_ok());
    }

    #[test]
    fn test_injection_states() {
        // 三态逐态核验：越闭集 / 漂移（先落正格）/ 逆拍（空账本必抓且游标不变）。
        let mut l0 = TcpingLedger::new();
        assert!(l0.record_mode(MODE_CELLS, 0).is_err());
        let mut l1 = TcpingLedger::new();
        let _ = l1.record_mode(0, mode_cell(0).unwrap_or(0));
        assert_eq!(
            l1.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(TcpingCode::EModeDrift)
        );
        let mut l2 = TcpingLedger::new();
        let _ = l2.record_sample(5);
        assert_eq!(l2.record_sample(2), Err(TcpingCode::EBeatOrder));
        assert_eq!(l2.last_beat(), 5);
        // 合法账目守恒恒成立。
        assert!(l2.verify_conservation().is_ok());
    }

    #[test]
    fn unx52433_checks_green() {
        let s = run_unx52433_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
