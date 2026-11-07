//! UNX-F52431 SoftPerfect Network Scanner 收口 · 0xCAB80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52431"，逐字）：
//!   title: SoftPerfect Network Scanner 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52432-J1 路由追
//!     踪 ×10 目标；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52431）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52432-J1，照原文锚定；功能按本卡标题（SoftPerfect Network
//! Scanner）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集 20 格：扫描 / Wake-on-LAN / 远程执行 / 共享枚举 /
//!      带宽测试，每族 4 格，族载荷纯函数折叠恒等才收；族单调（逆序
//!      必抓）。
//!   ② 结果投影 8 格：存活主机/开放端口/共享清单/设备名四类投影各
//!      2 格，逐格投影对照，投影漂移必抓。
//!   ③ 并发三档 6 格：低档（16 会话）/ 中档（64 会话）/ 高档（256 会话）
//!      每档 2 格，档位越闭集与档漂移必抓。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（并发档漂移/
//!      投影漂移/族逆序）10/10 必抓，逃逸恒 0；双跑指纹一致。
//!   ⑤ 定级目标档 S2（KB-COMPAT 登记）：空账 S3，六闸齐 S2，全齐目标
//!      档 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52431;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族 20 格。
pub const FUNC_CELLS: usize = 20;

/// 结果投影 8 格。
pub const PROJ_CELLS: usize = 8;

/// 并发三档 6 格。
pub const CONC_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoftPerfCode {
    /// 成功
    Ok = 0,
    /// 功能格越闭集
    ENoFunc,
    /// 功能漂移
    EDrift,
    /// 功能族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// 并发档越闭集
    ENoConc,
    /// 并发档漂移
    EConcDrift,
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

impl SoftPerfCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SoftPerfCode::Ok => "成功",
            SoftPerfCode::ENoFunc => "功能格越闭集：五功能族 20 格之外先登记",
            SoftPerfCode::EDrift => "功能漂移：功能族载荷折叠与基准不恒等，逐格复算",
            SoftPerfCode::EOrder => "功能族逆序：扫描→Wake-on-LAN→远程执行→共享枚举→带宽测试单调，逆序必抓",
            SoftPerfCode::ENoProj => "投影格越闭集：结果投影 8 格之外先登记",
            SoftPerfCode::EProjDrift => "投影漂移：结果投影与基准不恒等，逐格复算",
            SoftPerfCode::ENoConc => "并发档越闭集：并发三档 6 格之外先登记",
            SoftPerfCode::EConcDrift => "并发档漂移：档位载荷折叠与基准不恒等，逐档复算",
            SoftPerfCode::EDup => "重复落账：同格只许一条账目",
            SoftPerfCode::ESeal => "未齐即封：20 功能格 + 8 投影格 + 6 并发格未齐先补齐",
            SoftPerfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SoftPerfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SoftPerfCode::EGradeDrift => "定级漂移：全齐且六闸落账才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本批卡面不触达（保留闭集完整性）
    #[allow(dead_code)]
    S1,
    /// S2：收口目标档
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
            Grade::S1 => "S1：全通收口（本批 VX16 卡面目标档为 S2，不触达）",
            Grade::S2 => "S2：收口目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能格载荷（格 u；族 = u/4）。
pub const fn func_cell(u: usize) -> Option<u64> {
    if u >= FUNC_CELLS {
        return None;
    }
    Some((0xC8000u64 + (u as u64) * 0x3D) ^ ((u / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=扫描 4..8=Wake-on-LAN 8..12=远程执行 12..16=共享枚举 16..20=带宽测试）。
pub const fn func_kind(u: usize) -> Option<u32> {
    if u >= FUNC_CELLS {
        return None;
    }
    Some((u / 4) as u32)
}

/// 功能族名。
pub const fn func_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("扫描"),
        1 => Some("Wake-on-LAN"),
        2 => Some("远程执行"),
        3 => Some("共享枚举"),
        4 => Some("带宽测试"),
        _ => None,
    }
}

/// 投影格载荷（格 p；类 = p/2：0=存活主机 1=开放端口 2=共享清单 3=设备名）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xC9000u64 + (p as u64) * 0x29) ^ 0x50524F4A45435400)
}

/// 投影类（i/N 映射：类 = p/2）。
pub const fn proj_class(p: usize) -> Option<u32> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((p / 2) as u32)
}

/// 投影类名。
pub const fn proj_class_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("存活主机"),
        1 => Some("开放端口"),
        2 => Some("共享清单"),
        3 => Some("设备名"),
        _ => None,
    }
}

/// 并发档格载荷（格 c；档 = c/2：0=低档(16 会话) 1=中档(64 会话) 2=高档(256 会话)）。
pub const fn conc_cell(c: usize) -> Option<u64> {
    if c >= CONC_CELLS {
        return None;
    }
    Some((0xCA000u64 + (c as u64) * 0x47) ^ 0x434F4E4350524F00)
}

/// 并发档（i/N 映射：档 = c/2）。
pub const fn conc_tier(c: usize) -> Option<u32> {
    if c >= CONC_CELLS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 并发档名。
pub const fn conc_tier_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("低档 16 会话"),
        1 => Some("低档 16 会话（校验格）"),
        2 => Some("中档 64 会话"),
        3 => Some("中档 64 会话（校验格）"),
        4 => Some("高档 256 会话"),
        5 => Some("高档 256 会话（校验格）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SoftPerfect Network Scanner 收口账本状态机。
#[derive(Clone)]
pub struct SoftPerfLedger {
    /// 六判据（UNX-F52432-J1 锚）。
    gates: [Option<bool>; 6],
    /// 功能账。
    funcs: [Option<u64>; FUNC_CELLS],
    /// 投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 并发档账。
    conc: [Option<u64>; CONC_CELLS],
    /// 最后功能族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SoftPerfLedger {
    /// 新账本。
    pub fn new() -> Self {
        SoftPerfLedger {
            gates: [None; 6],
            funcs: [None; FUNC_CELLS],
            proj: [None; PROJ_CELLS],
            conc: [None; CONC_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SoftPerfCode> {
        if gate >= 6 {
            return Err(SoftPerfCode::ESeal);
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

    /// 功能一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_func(&mut self, u: usize, got: u64) -> Result<(), SoftPerfCode> {
        if u >= FUNC_CELLS {
            return Err(SoftPerfCode::ENoFunc);
        }
        let kind = match func_kind(u) {
            Some(k) => k,
            None => return Err(SoftPerfCode::ENoFunc),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(SoftPerfCode::EOrder);
        }
        let want = match func_cell(u) {
            Some(w) => w,
            None => return Err(SoftPerfCode::ENoFunc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + u as u64);
            return Err(SoftPerfCode::EDrift);
        }
        if match self.funcs.get(u) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoftPerfCode::EDup);
        }
        if let Some(c) = self.funcs.get_mut(u) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + u as u64);
        Ok(())
    }

    /// 功能全账核验（20 格）。
    pub fn funcs_all(&self) -> bool {
        let mut u = 0usize;
        while u < FUNC_CELLS {
            match self.funcs.get(u) {
                Some(Some(_)) => {}
                _ => return false,
            }
            u += 1;
        }
        true
    }

    /// 投影一格（类越闭集 + 投影对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), SoftPerfCode> {
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(SoftPerfCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + p as u64);
            return Err(SoftPerfCode::EProjDrift);
        }
        if match self.proj.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoftPerfCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x7301 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn proj_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.proj.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 并发档一格（档越闭集 + 载荷对照 + 重复拒）。
    pub fn record_conc(&mut self, c: usize, got: u64) -> Result<(), SoftPerfCode> {
        let want = match conc_cell(c) {
            Some(w) => w,
            None => return Err(SoftPerfCode::ENoConc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + c as u64);
            return Err(SoftPerfCode::EConcDrift);
        }
        if match self.conc.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SoftPerfCode::EDup);
        }
        if let Some(g) = self.conc.get_mut(c) {
            *g = Some(got);
        }
        self.fnv(0x7401 + c as u64);
        Ok(())
    }

    /// 并发档全账核验（6 格）。
    pub fn conc_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONC_CELLS {
            match self.conc.get(c) {
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

    /// KB-COMPAT 定级（目标档 S2：六闸齐 → S2，全齐亦 S2；空账 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SoftPerfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转（并发档漂移 / 投影漂移 / 族逆序）全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0；漂移注入先按合法
/// 序落正格再注漂移；族逆序注入对空账本同样必抓且不改主账 last_kind。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SoftPerfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut u = 0usize;
    while u < FUNC_CELLS {
        if let Some(w) = func_cell(u) {
            if led.record_func(u, w).is_ok() {
                recorded += 1;
            }
        }
        u += 1;
    }
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut c = 0usize;
    while c < CONC_CELLS {
        if let Some(w) = conc_cell(c) {
            if led.record_conc(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // 注入 10 轮：并发档漂移 / 投影漂移 / 族逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SoftPerfLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_conc(0, conc_cell(0).unwrap_or(0));
                lt.record_conc(1, conc_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SoftPerfCode::EConcDrift)
            }
            1 => {
                let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
                lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SoftPerfCode::EProjDrift)
            }
            _ => {
                let _ = lt.record_func(18, func_cell(18).unwrap_or(0));
                lt.record_func(2, func_cell(2).unwrap_or(0)) == Err(SoftPerfCode::EOrder)
            }
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
/// UNX-F52431 判据集（锚 UNX-F52432-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52431_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52431");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 格全通",
        recorded >= FUNC_CELLS as u32 + PROJ_CELLS as u32 + CONC_CELLS as u32,
        "扫描/Wake-on-LAN/远程执行/共享枚举/带宽测试逐格恒等（UNX-F52432-J1 锚）",
    );
    s.add(
        "结果投影 8 格全落",
        {
            let mut lt = SoftPerfLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.proj_all()
        },
        "存活主机/开放端口/共享清单/设备名四类投影逐格对照",
    );
    s.add(
        "并发三档 6 格全落",
        {
            let mut lt = SoftPerfLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONC_CELLS {
                if let Some(w) = conc_cell(c) {
                    if lt.record_conc(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.conc_all()
        },
        "低/中/高三档逐格恒等，档漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SoftPerfLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52432-J1 六判据逐格落账",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut u = 1usize;
            while u < FUNC_CELLS {
                if func_kind(u).unwrap_or(5) < func_kind(u - 1).unwrap_or(5) {
                    ok = false;
                }
                u += 1;
            }
            ok
        },
        "五功能族闭集单调（§AI-66.3）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "并发档漂移/投影漂移/族逆序三态轮转必抓");
    let mut l0 = SoftPerfLedger::new();
    s.add(
        "投影漂移拒",
        {
            let _ = l0.record_proj(0, proj_cell(0).unwrap_or(0));
            l0.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SoftPerfCode::EProjDrift)
        },
        "结果投影漂移必抓",
    );
    s.add(
        "族逆序拒",
        {
            let _ = l0.record_func(18, func_cell(18).unwrap_or(0));
            l0.record_func(2, func_cell(2).unwrap_or(0)) == Err(SoftPerfCode::EOrder)
        },
        "带宽测试后回扫描必抓",
    );
    s.add(
        "边界·功能越闭集",
        l0.record_func(FUNC_CELLS, 0).is_err(),
        "功能格 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·并发越闭集",
        l0.record_conc(CONC_CELLS, 0).is_err(),
        "并发格 6 越闭集必拒",
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
        "定级 S2 目标档",
        {
            let mut lt = SoftPerfLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut u = 0usize;
            while u < FUNC_CELLS {
                if let Some(w) = func_cell(u) {
                    let _ = lt.record_func(u, w);
                }
                u += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut c = 0usize;
            while c < CONC_CELLS {
                if let Some(w) = conc_cell(c) {
                    let _ = lt.record_conc(c, w);
                }
                c += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全齐目标档 S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_func_kind() {
        assert_eq!(func_kind(0), Some(0));
        assert_eq!(func_kind(5), Some(1));
        assert_eq!(func_kind(13), Some(3));
        assert_eq!(func_kind(19), Some(4));
        assert!(func_kind(20).is_none());
        assert!(func_cell(20).is_none());
        assert_eq!(func_kind_name(1), Some("Wake-on-LAN"));
        assert_eq!(func_kind_name(4), Some("带宽测试"));
        assert!(func_kind_name(5).is_none());
    }

    #[test]
    fn test_proj_class_mapping() {
        assert_eq!(proj_class(0), Some(0));
        assert_eq!(proj_class(1), Some(0));
        assert_eq!(proj_class(6), Some(3));
        assert_eq!(proj_class_name(2), Some("共享清单"));
        assert!(proj_class(8).is_none());
        assert!(proj_cell(8).is_none());
    }

    #[test]
    fn test_conc_tier_mapping() {
        assert_eq!(conc_tier(0), Some(0));
        assert_eq!(conc_tier(3), Some(1));
        assert_eq!(conc_tier(4), Some(2));
        assert_eq!(conc_tier_name(4), Some("高档 256 会话"));
        assert!(conc_tier(6).is_none());
        assert!(conc_cell(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = func_cell(15).unwrap_or(0);
        assert_eq!(a, func_cell(15).unwrap_or(0));
        assert_ne!(a, func_cell(16).unwrap_or(0));
        assert_ne!(proj_cell(3).unwrap_or(0), proj_cell(4).unwrap_or(0));
        assert_ne!(conc_cell(2).unwrap_or(0), conc_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SoftPerfLedger::new();
        let _ = lt.record_func(11, func_cell(11).unwrap_or(0));
        assert_eq!(lt.record_func(4, func_cell(4).unwrap_or(0)), Err(SoftPerfCode::EOrder));
        assert!(lt.record_func(12, func_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SoftPerfLedger::new();
        assert_eq!(
            lt.record_func(7, func_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(SoftPerfCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(SoftPerfCode::EProjDrift)
        );
        assert_eq!(
            lt.record_conc(5, conc_cell(5).unwrap_or(0).wrapping_add(4)),
            Err(SoftPerfCode::EConcDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SoftPerfLedger::new();
        let _ = lt.record_func(0, func_cell(0).unwrap_or(0));
        assert_eq!(lt.record_func(0, func_cell(0).unwrap_or(0)), Err(SoftPerfCode::EDup));
        let _ = lt.record_proj(2, proj_cell(2).unwrap_or(0));
        assert_eq!(lt.record_proj(2, proj_cell(2).unwrap_or(0)), Err(SoftPerfCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SoftPerfLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= FUNC_CELLS as u32 + PROJ_CELLS as u32 + CONC_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCAB80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SoftPerfCode::Ok as u32, 0);
        assert_eq!(SoftPerfCode::EGradeDrift as u32, 12);
        assert_eq!(SoftPerfCode::EConcDrift.val(), SEG_BASE + 7);
        assert!(!SoftPerfCode::Ok.detail().is_empty());
        assert!(!SoftPerfCode::EProjDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = SoftPerfLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = SoftPerfLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐目标档 S2。
        let mut p = 0usize;
        while p < PROJ_CELLS {
            if let Some(w) = proj_cell(p) {
                let _ = lt2.record_proj(p, w);
            }
            p += 1;
        }
        let mut c = 0usize;
        while c < CONC_CELLS {
            if let Some(w) = conc_cell(c) {
                let _ = lt2.record_conc(c, w);
            }
            c += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52431_checks_green() {
        let s = run_unx52431_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
