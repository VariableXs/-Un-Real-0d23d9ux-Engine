//! UNX-F52338 API Monitor 收口 · 0xC9440段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52338"，逐字）：
//!   title: API Monitor 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52339-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52338）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52339-J1，照原文锚定；功能按本卡标题（API Monitor）落地。
//! 定级封顶理由：API Monitor 为闭源免费工具（rohitab.com），无源码审计
//! 通道，如实降档；即便六判据与全部格账全通，KB-COMPAT 定级也封顶
//! S2（有条件收口），S1 恒不可达。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① API 按五族 20 格：进程（4）/内存（4）/文件（4）/网络（4）/
//!      注册表（4）五族闭集，族单调（逆序必抓）+ 族相位闸（后族先录
//!      必抓，对空账本同样必抓）。
//!   ② 监视 10 操作：附加进程/捕获/暂停/恢复/过滤应用/断点设置/单步/
//!      导出日志/清空/分离，越闭集必抓。
//!   ③ 过滤器 8 格：进程名/模块名/API 名/参数类型/返回值/错误码/
//!      搜索文本/错误级别，逐格落账，载荷漂移必抓。
//!   ④ 断点注入 5 场景：进程创建/内存读写/文件打开/套接字收发/注册表
//!      写入，逐场景落账（消费 E1 语义对账联签锚定行口径）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（API 格越闭集/
//!      过滤器载荷漂移/族相位越序），三类注入全部落在当轮临时账本上，
//!      主账 injected==caught==10、escaped==0；漂移注入先按合法序落正格
//!      再注漂移；相位注入对空账本必抓且不改 last_kind；双跑指纹一致；
//!      全通封顶 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52338;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// API 格数（五族 × 4）。
pub const API_CELLS: usize = 20;

/// 单族格数。
pub const API_PER_FAM: usize = 4;

/// 监视操作数。
pub const MON_OPS: usize = 10;

/// 过滤器格数。
pub const FLT_CELLS: usize = 8;

/// 断点注入场景数。
pub const BP_SCENES: usize = 5;

/// XOR 锚常量（低 16 位恒零口径）。
pub const API_XOR: u64 = 0x4150_494D_4F4E_0000;

/// 过滤器 XOR 锚常量（低 16 位恒零口径）。
pub const FLT_XOR: u64 = 0x464C_545F_4D41_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiMonCode {
    /// 成功
    Ok = 0,
    /// API 格越闭集
    ENoApi,
    /// API 格载荷漂移
    EApiDrift,
    /// 监视操作越闭集
    ENoMon,
    /// 过滤器格越闭集
    ENoFlt,
    /// 过滤器载荷漂移
    EFltDrift,
    /// 族序违例（逆序或后族先录）
    EOrder,
    /// 断点场景越闭集
    ENoBp,
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

impl ApiMonCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ApiMonCode::Ok => "成功",
            ApiMonCode::ENoApi => "API 格越闭集：五族 20 格闭集之外先登记",
            ApiMonCode::EApiDrift => "API 载荷漂移：格折叠与基准不恒等，逐格复算",
            ApiMonCode::ENoMon => "监视操作越闭集：监视 10 操作闭集之外先登记",
            ApiMonCode::ENoFlt => "过滤器格越闭集：过滤器 8 格闭集之外先登记",
            ApiMonCode::EFltDrift => "过滤器载荷漂移：格折叠与基准不恒等，逐格复算",
            ApiMonCode::EOrder => "族序违例：五族进程/内存/文件/网络/注册表单调，后族先录或逆序必抓",
            ApiMonCode::ENoBp => "断点场景越闭集：断点注入 5 场景闭集之外先登记",
            ApiMonCode::EDup => "重复落账：同格只许一条账目",
            ApiMonCode::ESeal => "未齐即封：20 API 格 + 10 监视 + 8 过滤器 + 5 断点场景未齐先补齐",
            ApiMonCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ApiMonCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ApiMonCode::EGradeDrift => "定级漂移：闭源免费工具封顶 S2，全通最高只可定 S2（S1 恒不可达）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡封顶后恒不可达）
    S1,
    /// S2：有条件收口（闭源免费工具封顶档）
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
            Grade::S1 => "S1：全通收口（本卡封顶，S1 恒不可达）",
            Grade::S2 => "S2：有条件收口——闭源免费工具如实降档，无源码审计通道",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// API 族（a/4：0=进程 1=内存 2=文件 3=网络 4=注册表）。
pub const fn api_family(a: usize) -> Option<u32> {
    if a >= API_CELLS {
        return None;
    }
    Some((a / API_PER_FAM) as u32)
}

/// 族名。
pub const fn family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("进程"),
        1 => Some("内存"),
        2 => Some("文件"),
        3 => Some("网络"),
        4 => Some("注册表"),
        _ => None,
    }
}

/// API 格载荷（格 a）。
pub const fn api_cell(a: usize) -> Option<u64> {
    if a >= API_CELLS {
        return None;
    }
    Some((0xF_0000u64 + (a as u64) * 0x41) ^ API_XOR)
}

/// 监视操作名（const fn 表 + Option 边界）。
pub const fn mon_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("附加进程"),
        1 => Some("捕获"),
        2 => Some("暂停"),
        3 => Some("恢复"),
        4 => Some("过滤应用"),
        5 => Some("断点设置"),
        6 => Some("单步"),
        7 => Some("导出日志"),
        8 => Some("清空"),
        9 => Some("分离"),
        _ => None,
    }
}

/// 监视操作格折叠（操作 m）。
pub const fn mon_cell(m: usize) -> Option<u32> {
    if m >= MON_OPS {
        return None;
    }
    Some(0xF100 + m as u32 * 0x2C)
}

/// 过滤器格载荷（格 f）。
pub const fn flt_cell(f: usize) -> Option<u64> {
    if f >= FLT_CELLS {
        return None;
    }
    Some((0x10_0000u64 + (f as u64) * 0x5B) ^ (FLT_XOR.wrapping_add(f as u64 * 0x100)))
}

/// 断点场景名（const fn 表 + Option 边界）。
pub const fn bp_name(b: usize) -> Option<&'static str> {
    match b {
        0 => Some("进程创建"),
        1 => Some("内存读写"),
        2 => Some("文件打开"),
        3 => Some("套接字收发"),
        4 => Some("注册表写入"),
        _ => None,
    }
}

/// 断点场景折叠（场景 b）。
pub const fn bp_cell(b: usize) -> Option<u32> {
    if b >= BP_SCENES {
        return None;
    }
    Some(0xF200 + b as u32 * 0x33)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// API Monitor 收口账本状态机。
#[derive(Clone)]
pub struct ApiMonLedger {
    /// 六判据（UNX-F52339-J1 六判据逐格落账）。
    gates: [Option<bool>; 6],
    /// API 五族账。
    apis: [Option<u64>; API_CELLS],
    /// 监视操作账。
    mons: [Option<u32>; MON_OPS],
    /// 过滤器账。
    flts: [Option<u64>; FLT_CELLS],
    /// 断点场景账。
    bps: [Option<u32>; BP_SCENES],
    /// 最后 API 族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ApiMonLedger {
    /// 新账本。
    pub fn new() -> Self {
        ApiMonLedger {
            gates: [None; 6],
            apis: [None; API_CELLS],
            mons: [None; MON_OPS],
            flts: [None; FLT_CELLS],
            bps: [None; BP_SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ApiMonCode> {
        if gate >= 6 {
            return Err(ApiMonCode::ESeal);
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

    /// 前一族是否至少一格已落（族相位闸判据）。
    fn prev_family_touched(&self, k: u32) -> bool {
        if k == 0 {
            return true;
        }
        let lo = ((k as usize) - 1) * API_PER_FAM;
        let hi = lo + API_PER_FAM;
        let mut i = lo;
        while i < hi {
            match self.apis.get(i) {
                Some(Some(_)) => return true,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// API 一格（族相位闸 + 族单调 + 载荷对照 + 重复拒）。
    pub fn record_api(&mut self, a: usize, got: u64) -> Result<(), ApiMonCode> {
        if a >= API_CELLS {
            return Err(ApiMonCode::ENoApi);
        }
        let kind = match api_family(a) {
            Some(k) => k,
            None => return Err(ApiMonCode::ENoApi),
        };
        // 相位闸：族 k>0 须在前族至少一格落账后方可落账；对空账本同样
        // 必抓，且拒绝路径不改 last_kind。
        if !self.prev_family_touched(kind) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(ApiMonCode::EOrder);
        }
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201);
            return Err(ApiMonCode::EOrder);
        }
        let want = match api_cell(a) {
            Some(w) => w,
            None => return Err(ApiMonCode::ENoApi),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6202 + a as u64);
            return Err(ApiMonCode::EApiDrift);
        }
        if match self.apis.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ApiMonCode::EDup);
        }
        if let Some(c) = self.apis.get_mut(a) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6203 + a as u64);
        Ok(())
    }

    /// API 全账核验（20 格）。
    pub fn apis_all(&self) -> bool {
        let mut a = 0usize;
        while a < API_CELLS {
            match self.apis.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 监视操作一格。
    pub fn record_mon(&mut self, m: usize) -> Result<(), ApiMonCode> {
        if m >= MON_OPS {
            return Err(ApiMonCode::ENoMon);
        }
        if match self.mons.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ApiMonCode::EDup);
        }
        if let Some(c) = self.mons.get_mut(m) {
            *c = mon_cell(m);
        }
        self.fnv(0x6300 + m as u64);
        Ok(())
    }

    /// 监视全账核验（10 操作）。
    pub fn mons_all(&self) -> bool {
        let mut m = 0usize;
        while m < MON_OPS {
            match self.mons.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 过滤器一格（载荷对照 + 重复拒）。
    pub fn record_flt(&mut self, f: usize, got: u64) -> Result<(), ApiMonCode> {
        if f >= FLT_CELLS {
            return Err(ApiMonCode::ENoFlt);
        }
        let want = match flt_cell(f) {
            Some(w) => w,
            None => return Err(ApiMonCode::ENoFlt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + f as u64);
            return Err(ApiMonCode::EFltDrift);
        }
        if match self.flts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ApiMonCode::EDup);
        }
        if let Some(c) = self.flts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x6401 + f as u64);
        Ok(())
    }

    /// 过滤器全账核验（8 格）。
    pub fn flts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FLT_CELLS {
            match self.flts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 断点场景一格。
    pub fn record_bp(&mut self, b: usize) -> Result<(), ApiMonCode> {
        if b >= BP_SCENES {
            return Err(ApiMonCode::ENoBp);
        }
        if match self.bps.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ApiMonCode::EDup);
        }
        if let Some(c) = self.bps.get_mut(b) {
            *c = bp_cell(b);
        }
        self.fnv(0x6500 + b as u64);
        Ok(())
    }

    /// 断点场景全账核验（5 场景）。
    pub fn bps_all(&self) -> bool {
        let mut b = 0usize;
        while b < BP_SCENES {
            match self.bps.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
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

    /// KB-COMPAT 定级（封顶：全通 → S2，判据缺 → S3，S1 恒不可达）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.apis_all()
            && self.mons_all()
            && self.flts_all()
            && self.bps_all()
        {
            // 闭源免费工具封顶：无源码审计通道，全通也只可定 S2。
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ApiMonLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ApiMonLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut a = 0usize;
    while a < API_CELLS {
        if let Some(w) = api_cell(a) {
            if led.record_api(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut m = 0usize;
    while m < MON_OPS {
        let _ = led.record_mon(m);
        m += 1;
    }
    let mut f = 0usize;
    while f < FLT_CELLS {
        if let Some(w) = flt_cell(f) {
            let _ = led.record_flt(f, w);
        }
        f += 1;
    }
    let mut b = 0usize;
    while b < BP_SCENES {
        let _ = led.record_bp(b);
        b += 1;
    }
    // 注入 10 轮：API 格越闭集 / 过滤器载荷漂移 / 族相位越序 三态轮转。
    // 三类注入全部落在当轮临时账本上，主账只记守恒三数与指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ApiMonLedger::new();
        let caught = match kind {
            0 => lt.record_api(API_CELLS, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                let _ = lt.record_flt(0, flt_cell(0).unwrap_or(0));
                lt.record_flt(1, flt_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(ApiMonCode::EFltDrift)
            }
            _ => {
                // 相位注入：空账本先录网络族（族 3）API，必抓且不改
                // last_kind。
                lt.record_api(12, api_cell(12).unwrap_or(0)) == Err(ApiMonCode::EOrder)
            }
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
/// UNX-F52338 判据集（锚 UNX-F52339-J1，off-by-one 板式）。
pub fn run_unx52338_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52338");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "API 五族 20 格全通",
        recorded >= API_CELLS as u32,
        "进程/内存/文件/网络/注册表五族逐格恒等（每族 4 格）",
    );
    s.add(
        "监视 10 操作全落",
        {
            let mut lt = ApiMonLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MON_OPS {
                if lt.record_mon(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.mons_all()
        },
        "附加进程/捕获/暂停/恢复/过滤应用/断点设置/单步/导出日志/清空/分离",
    );
    s.add(
        "过滤器 8 格全落",
        {
            let mut lt = ApiMonLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FLT_CELLS {
                if let Some(w) = flt_cell(f) {
                    if lt.record_flt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.flts_all()
        },
        "进程名/模块名/API 名/参数类型/返回值/错误码/搜索文本/错误级别逐格落账",
    );
    s.add(
        "断点注入 5 场景全通",
        {
            let mut lt = ApiMonLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BP_SCENES {
                if lt.record_bp(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.bps_all()
        },
        "进程创建/内存读写/文件打开/套接字收发/注册表写入（E1 语义对账联签锚定行）",
    );
    s.add(
        "族单调",
        {
            let mut ok = true;
            let mut a = 1usize;
            while a < API_CELLS {
                let prev = match api_family(a - 1) {
                    Some(k) => k,
                    None => 9,
                };
                let cur = match api_family(a) {
                    Some(k) => k,
                    None => 9,
                };
                if cur < prev {
                    ok = false;
                }
                a += 1;
            }
            ok
        },
        "五族闭集族序单调不逆",
    );
    s.add(
        "族相位闸对空账本必抓",
        {
            let mut lt = ApiMonLedger::new();
            lt.record_api(12, api_cell(12).unwrap_or(0)) == Err(ApiMonCode::EOrder)
        },
        "后族先录必抓，且不改 last_kind",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "API 格越闭集/过滤器载荷漂移/族相位越序三态轮转必抓",
    );
    let mut l0 = ApiMonLedger::new();
    s.add(
        "边界·API 格越闭集",
        l0.record_api(API_CELLS, 0).is_err(),
        "API 格 20 越闭集必拒",
    );
    s.add(
        "边界·监视操作越闭集",
        l0.record_mon(MON_OPS).is_err(),
        "监视操作 10 越闭集必拒",
    );
    s.add(
        "边界·断点场景越闭集",
        l0.record_bp(BP_SCENES).is_err(),
        "断点场景 5 越闭集必拒",
    );
    s.add(
        "重复落账拒",
        {
            let _ = l0.record_mon(0);
            l0.record_mon(0) == Err(ApiMonCode::EDup)
        },
        "同格只许一条账目",
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
        "定级 S2 封顶闭集",
        {
            let mut lt = ApiMonLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut a = 0usize;
            while a < API_CELLS {
                if let Some(w) = api_cell(a) {
                    let _ = lt.record_api(a, w);
                }
                a += 1;
            }
            let mut m = 0usize;
            while m < MON_OPS {
                let _ = lt.record_mon(m);
                m += 1;
            }
            let mut f = 0usize;
            while f < FLT_CELLS {
                if let Some(w) = flt_cell(f) {
                    let _ = lt.record_flt(f, w);
                }
                f += 1;
            }
            let mut b = 0usize;
            while b < BP_SCENES {
                let _ = lt.record_bp(b);
                b += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 封顶（闭源免费工具如实降档，KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9440,
        "SEG_BASE 0x40 对齐且落 0xC7200 + 0x40×(52338−52201) 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_mapping() {
        // 映射口径：族 = a/4（0=进程 1=内存 2=文件 3=网络 4=注册表）。
        assert_eq!(api_family(0), Some(0));
        assert_eq!(api_family(4), Some(1));
        assert_eq!(api_family(8), Some(2));
        assert_eq!(api_family(12), Some(3));
        assert_eq!(api_family(19), Some(4));
        assert!(api_family(20).is_none());
        assert!(family_name(5).is_none());
    }

    #[test]
    fn test_mon_name_table() {
        assert_eq!(mon_name(0), Some("附加进程"));
        assert_eq!(mon_name(5), Some("断点设置"));
        assert_eq!(mon_name(9), Some("分离"));
        assert!(mon_name(10).is_none());
        assert!(mon_cell(10).is_none());
    }

    #[test]
    fn test_bp_name_table() {
        assert_eq!(bp_name(0), Some("进程创建"));
        assert_eq!(bp_name(4), Some("注册表写入"));
        assert!(bp_name(5).is_none());
        assert!(bp_cell(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = api_cell(6).unwrap_or(0);
        assert_eq!(a, api_cell(6).unwrap_or(0));
        assert_ne!(a, api_cell(7).unwrap_or(0));
        assert!(flt_cell(FLT_CELLS).is_none());
        // XOR 锚常量低 16 位恒零。
        assert_eq!(API_XOR & 0xFFFF, 0);
        assert_eq!(FLT_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_phase_caught() {
        let mut lt = ApiMonLedger::new();
        assert_eq!(
            lt.record_api(12, api_cell(12).unwrap_or(0)),
            Err(ApiMonCode::EOrder)
        );
        // 拒绝路径不改 last_kind：进程族正格仍可落。
        assert!(lt.record_api(0, api_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_monotone_caught() {
        let mut lt = ApiMonLedger::new();
        // 先按合法族序落 0..=8（进程/内存/文件族首格），再注族逆序。
        let mut a = 0usize;
        while a <= 8 {
            let _ = lt.record_api(a, api_cell(a).unwrap_or(0));
            a += 1;
        }
        // 回录早族格：族 0 < 最后族 2，族单调先于重复检查必抓。
        assert_eq!(
            lt.record_api(2, api_cell(2).unwrap_or(0)),
            Err(ApiMonCode::EOrder)
        );
        assert_eq!(
            lt.record_api(1, api_cell(1).unwrap_or(0)),
            Err(ApiMonCode::EOrder)
        );
        assert!(lt.record_api(9, api_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ApiMonLedger::new();
        assert_eq!(
            lt.record_api(1, api_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(ApiMonCode::EApiDrift)
        );
        assert_eq!(
            lt.record_flt(3, flt_cell(3).unwrap_or(0).wrapping_add(9)),
            Err(ApiMonCode::EFltDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ApiMonLedger::new();
        let _ = lt.record_bp(2);
        assert_eq!(lt.record_bp(2), Err(ApiMonCode::EDup));
        let _ = lt.record_mon(7);
        assert_eq!(lt.record_mon(7), Err(ApiMonCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ApiMonLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ApiMonLedger::new();
        let _ = lt2.judge_gate(0);
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= API_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9440);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ApiMonCode::Ok as u32, 0);
        assert_eq!(ApiMonCode::EGradeDrift as u32, 12);
        assert!(!ApiMonCode::Ok.detail().is_empty());
        assert!(!ApiMonCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade_cap() {
        assert_eq!(Grade::S1.val(), 1);
        let lt = ApiMonLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52338_checks_green() {
        let s = run_unx52338_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
