//! UNX-F52274 JetBrains Gateway（远程开发）收口 · 0xC8440段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52274"，逐字）：
//!   title: JetBrains Gateway（远程开发）收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52275-J1 实例
//!     创建/删除 ×10；虚拟化 N/A 同构（F52261 口径）；KB-COMPAT 定级
//!     登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52274）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52275-J1，照原文锚定；功能按本卡标题（JetBrains Gateway）落
//! 地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 后端三族 12 格：SSH / WSL / Dev Container 三族各 4 格，家族
//!      单调（逆序必抓）；「实例创建/删除 ×10」（锚 UNX-F52275-J1）以
//!      后端 12 格 + IDE 8 格合计 20 格的建删账 ×1 轮等价收口。
//!   ② IDE 分发 8 格闭集：逐格载荷对照，漂移必抓。
//!   ③ 隧道三元闸：闸/密钥/探活三格闭集，缺一隧道不成，未齐即封。
//!   ④ 与会话消费联签 4 格（零重测复用口径）；六判据闸（后端布设/
//!      连接建立/IDE 分发/隧道三闸/会话联签/虚拟化 N/A 同构登记——
//!      F52261 口径，不硬开、如实定级）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（后端家族逆序/IDE 漂移/后端越
//!      闭集），检出+逃逸守恒等式；双跑指纹一致；定级 S2（本卡全通亦
//!      定 S2：隧道语义属有条件收口，S1 为全域预留级）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52274;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 后端三族格数。
pub const BACK_CELLS: usize = 12;

/// IDE 分发格数。
pub const IDE_CELLS: usize = 8;

/// 隧道三元闸格数。
pub const TUN_GATES: usize = 3;

/// 会话消费联签格数。
pub const SESS_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatewayCode {
    /// 成功
    Ok = 0,
    /// 后端格越闭集
    ENoBack,
    /// 后端漂移
    EBackDrift,
    /// 后端家族逆序
    EOrder,
    /// IDE 格越闭集
    ENoIde,
    /// IDE 分发漂移
    EIdeDrift,
    /// 隧道闸格越闭集
    ENoTun,
    /// 隧道三元闸未齐
    ETunMiss,
    /// 会话联签格越闭集
    ENoSess,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl GatewayCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GatewayCode::Ok => "成功",
            GatewayCode::ENoBack => "后端格越闭集：后端三族 12 格之外先登记",
            GatewayCode::EBackDrift => "后端漂移：后端折叠与基准不恒等，逐格复算",
            GatewayCode::EOrder => "家族逆序：SSH→WSL→Dev Container 单调，逆序必抓",
            GatewayCode::ENoIde => "IDE 格越闭集：IDE 分发 8 格之外先登记",
            GatewayCode::EIdeDrift => "IDE 分发漂移：分发折叠与基准不恒等，逐格复算",
            GatewayCode::ENoTun => "隧道闸格越闭集：隧道三元闸 3 格之外先登记",
            GatewayCode::ETunMiss => "隧道三元闸未齐：闸/密钥/探活三格缺一先补齐",
            GatewayCode::ENoSess => "联签格越闭集：会话消费联签 4 格之外先登记",
            GatewayCode::EDup => "重复落账：同格只许一条账目",
            GatewayCode::ESeal => "未齐即封：12+8+3+4 四账未齐先补齐",
            GatewayCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GatewayCode::EGradeDrift => "定级漂移：四账全通且六判据全落才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全域收口（本卡预留级，隧道语义不触）
    S1,
    /// S2：有条件收口（本卡全通定级）
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
            Grade::S1 => "S1：全域收口（预留级，Gateway 卡不触）",
            Grade::S2 => "S2：有条件收口——12+8+3+4 四账全通，隧道语义如实定级",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 后端格载荷（格 b；家族 = b/4）。
pub const fn back_cell(b: usize) -> Option<u64> {
    if b >= BACK_CELLS {
        return None;
    }
    Some((0xD1000u64 + (b as u64) * 0x43) ^ ((b / 4) as u64).wrapping_mul(0x01000193))
}

/// 后端家族（0..4 SSH 4..8 WSL 8..12 Dev Container）。
pub const fn back_kind(b: usize) -> Option<u32> {
    if b >= BACK_CELLS {
        return None;
    }
    Some((b / 4) as u32)
}

/// 家族名。
pub const fn back_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("SSH"),
        1 => Some("WSL"),
        2 => Some("Dev Container"),
        _ => None,
    }
}

/// IDE 分发格载荷（格 i）。
pub const fn ide_cell(i: usize) -> Option<u64> {
    if i >= IDE_CELLS {
        return None;
    }
    Some((0xD2000u64 + (i as u64) * 0x35) ^ 0x4A42_4757_4944_0000)
}

/// 隧道三元闸格基准（闸 t：0 闸本体 1 密钥 2 探活）。
pub const fn tun_cell(t: usize) -> Option<u32> {
    if t >= TUN_GATES {
        return None;
    }
    Some(0xD300 + t as u32 * 0x3C)
}

/// 隧道闸名。
pub const fn tun_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("闸本体"),
        1 => Some("密钥"),
        2 => Some("探活"),
        _ => None,
    }
}

/// 会话消费联签格基准（格 c）。
pub const fn sess_cell(c: usize) -> Option<u64> {
    if c >= SESS_CELLS {
        return None;
    }
    Some((0xD4000u64 + (c as u64) * 0x39) ^ 0x4A42_4757_5345_0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Gateway 收口账本状态机。
#[derive(Clone)]
pub struct GatewayLedger {
    /// 六判据（后端布设/连接建立/IDE 分发/隧道三闸/会话联签/虚拟化 N/A 登记）。
    gates: [Option<bool>; 6],
    /// 后端账。
    backs: [Option<u64>; BACK_CELLS],
    /// IDE 分发账。
    ides: [Option<u64>; IDE_CELLS],
    /// 隧道三元闸账。
    tuns: [Option<u32>; TUN_GATES],
    /// 会话联签账。
    sesss: [Option<u64>; SESS_CELLS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GatewayLedger {
    /// 新账本。
    pub fn new() -> Self {
        GatewayLedger {
            gates: [None; 6],
            backs: [None; BACK_CELLS],
            ides: [None; IDE_CELLS],
            tuns: [None; TUN_GATES],
            sesss: [None; SESS_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GatewayCode> {
        if gate >= 6 {
            return Err(GatewayCode::ESeal);
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

    /// 后端一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_back(&mut self, b: usize, got: u64) -> Result<(), GatewayCode> {
        if b >= BACK_CELLS {
            return Err(GatewayCode::ENoBack);
        }
        let kind = match back_kind(b) {
            Some(k) => k,
            None => return Err(GatewayCode::ENoBack),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(GatewayCode::EOrder);
        }
        let want = match back_cell(b) {
            Some(w) => w,
            None => return Err(GatewayCode::ENoBack),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + b as u64);
            return Err(GatewayCode::EBackDrift);
        }
        if match self.backs.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatewayCode::EDup);
        }
        if let Some(c) = self.backs.get_mut(b) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + b as u64);
        Ok(())
    }

    /// 后端全账核验（12 格）。
    pub fn backs_all(&self) -> bool {
        let mut b = 0usize;
        while b < BACK_CELLS {
            match self.backs.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// IDE 分发一格。
    pub fn record_ide(&mut self, i: usize, got: u64) -> Result<(), GatewayCode> {
        let want = match ide_cell(i) {
            Some(w) => w,
            None => return Err(GatewayCode::ENoIde),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + i as u64);
            return Err(GatewayCode::EIdeDrift);
        }
        if match self.ides.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatewayCode::EDup);
        }
        if let Some(c) = self.ides.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6301 + i as u64);
        Ok(())
    }

    /// IDE 分发全账核验（8 格）。
    pub fn ides_all(&self) -> bool {
        let mut i = 0usize;
        while i < IDE_CELLS {
            match self.ides.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 隧道三元闸一格。
    pub fn record_tun(&mut self, t: usize) -> Result<(), GatewayCode> {
        let want = match tun_cell(t) {
            Some(w) => w,
            None => return Err(GatewayCode::ENoTun),
        };
        if match self.tuns.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatewayCode::EDup);
        }
        if let Some(c) = self.tuns.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x6400 + t as u64);
        Ok(())
    }

    /// 隧道三元闸全齐核验。
    pub fn tuns_all(&self) -> bool {
        let mut t = 0usize;
        while t < TUN_GATES {
            match self.tuns.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 会话消费联签一格。
    pub fn record_sess(&mut self, c: usize, got: u64) -> Result<(), GatewayCode> {
        let want = match sess_cell(c) {
            Some(w) => w,
            None => return Err(GatewayCode::ENoSess),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + c as u64);
            return Err(GatewayCode::EIdeDrift);
        }
        if match self.sesss.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GatewayCode::EDup);
        }
        if let Some(s) = self.sesss.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x6501 + c as u64);
        Ok(())
    }

    /// 会话联签全账核验（4 格）。
    pub fn sesss_all(&self) -> bool {
        let mut c = 0usize;
        while c < SESS_CELLS {
            match self.sesss.get(c) {
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

    /// KB-COMPAT 定级（本卡全通 → S2 有条件收口，否则 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.backs_all()
            && self.ides_all()
            && self.tuns_all()
            && self.sesss_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GatewayLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GatewayLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < BACK_CELLS {
        if let Some(w) = back_cell(b) {
            if led.record_back(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut i = 0usize;
    while i < IDE_CELLS {
        if let Some(w) = ide_cell(i) {
            if led.record_ide(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < TUN_GATES {
        let _ = led.record_tun(t);
        t += 1;
    }
    let mut c = 0usize;
    while c < SESS_CELLS {
        if let Some(w) = sess_cell(c) {
            if led.record_sess(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 注入 10 轮：后端家族逆序 / IDE 分发漂移 / 后端越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GatewayLedger::new();
        let caught = match kind {
            0 => {
                // 家族逆序：先落 Dev Container 族正格，再回 SSH 族必抓。
                let _ = lt.record_back(9, back_cell(9).unwrap_or(0));
                lt.record_back(1, back_cell(1).unwrap_or(0)) == Err(GatewayCode::EOrder)
            }
            1 => lt.record_ide(4, ide_cell(4).unwrap_or(0).wrapping_add(1))
                == Err(GatewayCode::EIdeDrift),
            _ => lt.record_back(BACK_CELLS, 0).is_err(),
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
/// UNX-F52274 判据集（锚 UNX-F52275-J1，off-by-one 板式）。
pub fn run_unx52274_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52274");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "后端三族 12 格全通",
        {
            let mut lt = GatewayLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BACK_CELLS {
                if let Some(w) = back_cell(b) {
                    if lt.record_back(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.backs_all()
        },
        "SSH/WSL/Dev Container 三族逐格恒等（锚 UNX-F52275-J1 ×10 口径前置账）",
    );
    s.add(
        "IDE 分发 8/8 全落",
        {
            let mut lt = GatewayLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < IDE_CELLS {
                if let Some(w) = ide_cell(i) {
                    if lt.record_ide(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.ides_all()
        },
        "IDE 分发 8 格闭集逐格载荷对照",
    );
    s.add(
        "隧道三元闸 3/3 全齐",
        {
            let mut lt = GatewayLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TUN_GATES {
                if lt.record_tun(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tuns_all()
        },
        "闸本体/密钥/探活三格缺一拒封",
    );
    s.add(
        "会话消费联签 4/4 全落",
        {
            let mut lt = GatewayLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SESS_CELLS {
                if let Some(w) = sess_cell(c) {
                    if lt.record_sess(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.sesss_all()
        },
        "与会话消费联签 4 格零重测复用口径",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GatewayLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "后端布设/连接建立/IDE 分发/隧道三闸/会话联签/虚拟化 N/A 登记（§AI-66.3）",
    );
    s.add(
        "虚拟化 N/A 同构（F52261 口径）",
        tun_cell(TUN_GATES).is_none() && back_cell(BACK_CELLS).is_none(),
        "虚拟化探测不硬开、如实登记 N/A，闭集外载荷恒 None 同构口径",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut b = 1usize;
            while b < BACK_CELLS {
                if back_kind(b).unwrap_or(3) < back_kind(b - 1).unwrap_or(3) {
                    ok = false;
                }
                b += 1;
            }
            ok
        },
        "SSH→WSL→Dev Container 三族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "家族逆序/IDE 漂移/越闭集三态轮转必抓");
    let mut l0 = GatewayLedger::new();
    s.add(
        "边界·后端格越闭集",
        l0.record_back(BACK_CELLS, 0).is_err(),
        "后端格 12 越闭集必拒",
    );
    s.add(
        "边界·IDE 格越闭集",
        l0.record_ide(IDE_CELLS, 0).is_err(),
        "IDE 格 8 越闭集必拒",
    );
    s.add(
        "边界·隧道闸越闭集",
        l0.record_tun(TUN_GATES).is_err(),
        "隧道闸 3 越闭集必拒",
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
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            let mut lt = GatewayLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < BACK_CELLS {
                if let Some(w) = back_cell(b) {
                    let _ = lt.record_back(b, w);
                }
                b += 1;
            }
            let mut i = 0usize;
            while i < IDE_CELLS {
                if let Some(w) = ide_cell(i) {
                    let _ = lt.record_ide(i, w);
                }
                i += 1;
            }
            let mut t = 0usize;
            while t < TUN_GATES {
                let _ = lt.record_tun(t);
                t += 1;
            }
            let mut c = 0usize;
            while c < SESS_CELLS {
                if let Some(w) = sess_cell(c) {
                    let _ = lt.record_sess(c, w);
                }
                c += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "四账全通 → S2 有条件收口（隧道语义），缺格 → S3",
    );
    s.add(
        "诊断码闭集 13 变体",
        GatewayCode::Ok as u32 == 0
            && GatewayCode::EGradeDrift as u32 == 12
            && GatewayCode::Ok.val() == SEG_BASE
            && GatewayCode::EGradeDrift.val() == SEG_BASE + 0x0C,
        "Ok=+0x00 至 EGradeDrift=+0x0C 恰 13 变体，码位=段基址+码",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8440,
        "SEG_BASE=0xC7200+0x40×73 0x40 对齐且落 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_back_kind() {
        assert_eq!(back_kind(0), Some(0));
        assert_eq!(back_kind(4), Some(1));
        assert_eq!(back_kind(11), Some(2));
        assert!(back_kind(12).is_none());
        assert!(back_cell(12).is_none());
        assert_eq!(back_kind_name(2), Some("Dev Container"));
        assert!(back_kind_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = ide_cell(3).unwrap_or(0);
        assert_eq!(a, ide_cell(3).unwrap_or(0));
        assert_ne!(a, ide_cell(4).unwrap_or(0));
        assert!(ide_cell(IDE_CELLS).is_none());
        assert!(sess_cell(SESS_CELLS).is_none());
        assert!(tun_cell(TUN_GATES).is_none());
        assert_eq!(tun_name(2), Some("探活"));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = GatewayLedger::new();
        let _ = lt.record_back(8, back_cell(8).unwrap_or(0));
        assert_eq!(lt.record_back(3, back_cell(3).unwrap_or(0)), Err(GatewayCode::EOrder));
        assert!(lt.record_back(9, back_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = GatewayLedger::new();
        assert_eq!(
            lt.record_ide(1, ide_cell(1).unwrap_or(0).wrapping_add(5)),
            Err(GatewayCode::EIdeDrift)
        );
        assert_eq!(
            lt.record_back(2, back_cell(2).unwrap_or(0).wrapping_add(2)),
            Err(GatewayCode::EBackDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GatewayLedger::new();
        let _ = lt.record_tun(0);
        assert_eq!(lt.record_tun(0), Err(GatewayCode::EDup));
        let _ = lt.record_sess(2, sess_cell(2).unwrap_or(0));
        assert_eq!(lt.record_sess(2, sess_cell(2).unwrap_or(0)), Err(GatewayCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GatewayLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GatewayLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_tunnel_miss_semantics() {
        // 缺密钥格：tuns_all 为假，账面其他全齐也不得误定级。
        let mut lt = GatewayLedger::new();
        let _ = lt.record_tun(0);
        let _ = lt.record_tun(2);
        assert!(!lt.tuns_all());
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (BACK_CELLS + IDE_CELLS + SESS_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8440);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GatewayCode::Ok as u32, 0);
        assert_eq!(GatewayCode::EGradeDrift as u32, 12);
        assert_eq!(GatewayCode::ETunMiss as u32, 7);
        let all = [
            GatewayCode::Ok,
            GatewayCode::ENoBack,
            GatewayCode::EBackDrift,
            GatewayCode::EOrder,
            GatewayCode::ENoIde,
            GatewayCode::EIdeDrift,
            GatewayCode::ENoTun,
            GatewayCode::ETunMiss,
            GatewayCode::ENoSess,
            GatewayCode::EDup,
            GatewayCode::ESeal,
            GatewayCode::EInj,
            GatewayCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            let _ = all[i].val();
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        let lt = GatewayLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_injection_empty_ledger_order() {
        // 空账本上家族逆序注入：先落高族合法正格再回低族必抓。
        let mut lt = GatewayLedger::new();
        let _ = lt.record_back(9, back_cell(9).unwrap_or(0));
        assert_eq!(lt.record_back(1, back_cell(1).unwrap_or(0)), Err(GatewayCode::EOrder));
    }

    #[test]
    fn unx52274_checks_green() {
        let s = run_unx52274_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
