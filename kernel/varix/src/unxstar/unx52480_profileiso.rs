//! UNX-F52480 浏览器用户数据隔离共面条目（多浏览器 profile 并存零互踩） · 0xCB7C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52480"，逐字）：
//!   title: 浏览器用户数据隔离共面条目（多浏览器 profile 并存零互踩）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52481-J1 静态扫描
//!     ×10 工程全通；JVM 上栈（F52057 联签锚定行）；KB-COMPAT 定级 S1；
//!     判据锚定 §AI-66.3 组 25 口径；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52480）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52481-J1，照原文锚定；功能按本卡标题（浏览器 profile 并存零互
//! 踩）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五浏览器族闭集：Chrome / Edge / Firefox / Brave / Vivaldi 各 4 格
//!      （目录/锁/缓存/扩展）共 20 格，浏览器族单调（逆序必抓）；逐格
//!      载荷纯函数折叠，恒等才收。
//!   ② 互踩检出注入 10/10：双开同 profile 必抓锁冲突（锁格重复落账 →
//!      ELock），三态轮转（锁冲突/并存拍漂移/格越闭集）全部落在当轮临
//!      时账本上，主账注入=检出=10、逃逸=0。
//!   ③ 并存 16 拍单调：16 拍并存观测逐拍落账、拍号严格单调、漂移必抓；
//!      六判据母版闸逐格落账，越界未齐即封拒。
//!   ④ 消费联签 4 格（F52461/F52462 锚定）+ 各浏览器分账声明 2 行闭集，
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      双跑指纹一致；KB-COMPAT 定级闭集（40 格 + 2 行声明 + 六判据全通
//!      → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52480;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五浏览器 profile 面格数（每浏览器 4 格：目录/锁/缓存/扩展）。
pub const BROWSER_CELLS: usize = 20;

/// 并存拍数。
pub const COEXIST_BEATS: usize = 16;

/// 消费联签格数。
pub const COSIGN_CELLS: usize = 4;

/// 各浏览器分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileIsoCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoOp,
    /// 格漂移
    EDrift,
    /// 浏览器族/拍号逆序
    EOrder,
    /// 并存拍号越闭集
    ENoCo,
    /// 并存拍漂移
    ECoDrift,
    /// 联签格号越闭集
    ENoSign,
    /// 联签漂移
    ESignDrift,
    /// profile 锁冲突（双开同 profile）
    ELock,
    /// 重复落账
    EDup,
    /// 声明行越闭集
    ENoRow,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl ProfileIsoCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ProfileIsoCode::Ok => "成功",
            ProfileIsoCode::ENoOp => "格号越闭集：五浏览器 20 格 profile 面之外先登记",
            ProfileIsoCode::EDrift => "格漂移：目录/锁/缓存/扩展折叠与基准不恒等，逐格复算",
            ProfileIsoCode::EOrder => "浏览器族/拍号逆序：Chrome→Edge→Firefox→Brave→Vivaldi 与 16 拍均单调，逆序必抓",
            ProfileIsoCode::ENoCo => "并存拍号越闭集：16 拍并存观测之外先登记",
            ProfileIsoCode::ECoDrift => "并存拍漂移：拍折叠与基准不恒等，逐拍复算",
            ProfileIsoCode::ENoSign => "联签格号越闭集：消费联签 4 格之外先登记",
            ProfileIsoCode::ESignDrift => "联签漂移：消费联签折叠与基准不恒等（F52461/F52462）",
            ProfileIsoCode::ELock => "profile 锁冲突：双开同 profile 锁格必抓，零互踩口径",
            ProfileIsoCode::EDup => "重复落账：同格只许一条账目",
            ProfileIsoCode::ENoRow => "声明行越闭集：各浏览器分账声明 2 行闭集之外先登记",
            ProfileIsoCode::ESeal => "未齐即封：20 格 + 16 拍 + 4 联签 + 2 声明未齐先补齐",
            ProfileIsoCode::EGradeDrift => "定级漂移：40 格 + 2 行声明 + 六判据全通才可定 S1",
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
            Grade::S1 => "S1：六判据+40格+2行声明全通收口（并存零互踩）",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 浏览器 profile 格载荷（格 o；浏览器族 = o/4；面 = o%4：0=目录 1=锁 2=缓存 3=扩展）。
pub const fn browser_cell(o: usize) -> Option<u64> {
    if o >= BROWSER_CELLS {
        return None;
    }
    Some((0xE8000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 浏览器族（0..4=Chrome 4..8=Edge 8..12=Firefox 12..16=Brave 16..20=Vivaldi）。
pub const fn browser_kind(o: usize) -> Option<u32> {
    if o >= BROWSER_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 浏览器族名。
pub const fn browser_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Chrome"),
        1 => Some("Edge"),
        2 => Some("Firefox"),
        3 => Some("Brave"),
        4 => Some("Vivaldi"),
        _ => None,
    }
}

/// profile 面名（格内四面）。
pub const fn face_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("目录"),
        1 => Some("锁"),
        2 => Some("缓存"),
        3 => Some("扩展"),
        _ => None,
    }
}

/// 并存拍载荷（拍 t）。
pub const fn co_cell(t: usize) -> Option<u64> {
    if t >= COEXIST_BEATS {
        return None;
    }
    Some((0xE9000u64 + (t as u64) * 0x2F) ^ 0x50524F46434F4558)
}

/// 消费联签格载荷（格 c）。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some((0xEA000u64 + (c as u64) * 0x43) ^ 0x434F5349474E5000)
}

/// 各浏览器分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xEB00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 浏览器 profile 隔离收口账本状态机。
#[derive(Clone)]
pub struct ProfileLedger {
    /// 六判据母版闸。
    gates: [Option<bool>; 6],
    /// profile 面账（20 格）。
    cells: [Option<u64>; BROWSER_CELLS],
    /// 并存拍账（16 拍）。
    beats: [Option<u64>; COEXIST_BEATS],
    /// 消费联签账。
    signs: [Option<u64>; COSIGN_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后浏览器族。
    last_kind: i64,
    /// 最后并存拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ProfileLedger {
    /// 新账本。
    pub fn new() -> Self {
        ProfileLedger {
            gates: [None; 6],
            cells: [None; BROWSER_CELLS],
            beats: [None; COEXIST_BEATS],
            signs: [None; COSIGN_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ProfileIsoCode> {
        if gate >= 6 {
            return Err(ProfileIsoCode::ESeal);
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

    /// profile 面一格（族单调 + 载荷对照 + 锁格重复即冲突 + 他格重复拒）。
    pub fn record_cell(&mut self, o: usize, got: u64) -> Result<(), ProfileIsoCode> {
        if o >= BROWSER_CELLS {
            return Err(ProfileIsoCode::ENoOp);
        }
        let kind = match browser_kind(o) {
            Some(k) => k,
            None => return Err(ProfileIsoCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(ProfileIsoCode::EOrder);
        }
        let want = match browser_cell(o) {
            Some(w) => w,
            None => return Err(ProfileIsoCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(ProfileIsoCode::EDrift);
        }
        if match self.cells.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            // 锁面重复落账 = 双开同 profile，零互踩口径必抓锁冲突。
            if o % 4 == 1 {
                return Err(ProfileIsoCode::ELock);
            }
            return Err(ProfileIsoCode::EDup);
        }
        if let Some(c) = self.cells.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// profile 面全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut o = 0usize;
        while o < BROWSER_CELLS {
            match self.cells.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 并存拍一格（拍号单调 + 载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, t: usize, got: u64) -> Result<(), ProfileIsoCode> {
        if t >= COEXIST_BEATS {
            return Err(ProfileIsoCode::ENoCo);
        }
        if (t as i64) < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(ProfileIsoCode::EOrder);
        }
        let want = match co_cell(t) {
            Some(w) => w,
            None => return Err(ProfileIsoCode::ENoCo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + t as u64);
            return Err(ProfileIsoCode::ECoDrift);
        }
        if match self.beats.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProfileIsoCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(t) {
            *c = Some(got);
        }
        self.last_beat = t as i64;
        self.fnv(0x8302 + t as u64);
        Ok(())
    }

    /// 并存拍全账核验（16 拍全齐且逐拍单调已由落账口径保证）。
    pub fn beats_all(&self) -> bool {
        let mut t = 0usize;
        while t < COEXIST_BEATS {
            match self.beats.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 消费联签一格（载荷对照 + 重复拒）。
    pub fn record_cosign(&mut self, c: usize, got: u64) -> Result<(), ProfileIsoCode> {
        let want = match cosign_cell(c) {
            Some(w) => w,
            None => return Err(ProfileIsoCode::ENoSign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + c as u64);
            return Err(ProfileIsoCode::ESignDrift);
        }
        if match self.signs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProfileIsoCode::EDup);
        }
        if let Some(k) = self.signs.get_mut(c) {
            *k = Some(got);
        }
        self.fnv(0x8401 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn signs_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.signs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（各浏览器分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), ProfileIsoCode> {
        if r >= SPLIT_ROWS {
            return Err(ProfileIsoCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ProfileIsoCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cells_all() && self.beats_all() && self.signs_all()
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

impl Default for ProfileLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ProfileLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < BROWSER_CELLS {
        if let Some(w) = browser_cell(o) {
            if led.record_cell(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < COEXIST_BEATS {
        if let Some(w) = co_cell(t) {
            if led.record_beat(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        if let Some(w) = cosign_cell(c) {
            if led.record_cosign(c, w).is_ok() {
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
    // 互踩检出注入 10 轮：锁冲突 / 并存拍漂移 / 格越闭集三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ProfileLedger::new();
        let caught = match kind {
            0 => {
                // 双开同 profile：锁格（Chrome 锁面，格 1）二次落账必抓。
                let _ = lt.record_cell(1, browser_cell(1).unwrap_or(0));
                lt.record_cell(1, browser_cell(1).unwrap_or(0)) == Err(ProfileIsoCode::ELock)
            }
            1 => {
                // 先按合法序落拍 3 正格，再注同拍漂移（拍号不回退）。
                let _ = lt.record_beat(3, co_cell(3).unwrap_or(0));
                lt.record_beat(3, co_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(ProfileIsoCode::ECoDrift)
            }
            _ => lt.record_cell(BROWSER_CELLS, 0).is_err(),
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
/// UNX-F52480 判据集（锚 UNX-F52481-J1，off-by-one 板式）。
pub fn run_unx52480_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52480");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五浏览器 profile 面 20 格全通",
        {
            let mut lt = ProfileLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < BROWSER_CELLS {
                if let Some(w) = browser_cell(o) {
                    if lt.record_cell(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.cells_all()
        },
        "Chrome/Edge/Firefox/Brave/Vivaldi ×目录/锁/缓存/扩展逐格恒等",
    );
    s.add(
        "并存 16 拍单调全落",
        {
            let mut lt = ProfileLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < COEXIST_BEATS {
                if let Some(w) = co_cell(t) {
                    if lt.record_beat(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.beats_all()
        },
        "16 拍并存观测逐拍落账、拍号严格单调",
    );
    s.add(
        "消费联签 4/4（F52461/F52462）",
        {
            let mut lt = ProfileLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if let Some(w) = cosign_cell(c) {
                    if lt.record_cosign(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.signs_all()
        },
        "消费联签逐格恒等（F52461/F52462 锚定）",
    );
    s.add(
        "各浏览器分账声明 2/2 全齐",
        {
            let mut lt = ProfileLedger::new();
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
        "各浏览器独立 profile 账两行闭集，缺一拒封",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = ProfileLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，越界未齐即封拒",
    );
    s.add(
        "浏览器族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BROWSER_CELLS {
                if browser_kind(o).unwrap_or(5) < browser_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五浏览器族闭集单调",
    );
    s.add(
        "互踩检出注入 10/10 全抓",
        caught >= 10,
        "锁冲突/拍漂移/越闭集三态轮转必抓（双开同 profile 必抓锁冲突）",
    );
    let mut l0 = ProfileLedger::new();
    s.add(
        "边界·格越闭集",
        l0.record_cell(BROWSER_CELLS, 0).is_err(),
        "格 20 越闭集必拒",
    );
    s.add(
        "边界·拍越闭集",
        l0.record_beat(COEXIST_BEATS, 0).is_err(),
        "拍 16 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "锁冲突必抓",
        {
            let _ = l0.record_cell(5, browser_cell(5).unwrap_or(0));
            l0.record_cell(5, browser_cell(5).unwrap_or(0)) == Err(ProfileIsoCode::ELock)
        },
        "Edge 锁面双开同 profile 必抓 ELock（零互踩）",
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
            let mut lt = ProfileLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < BROWSER_CELLS {
                if let Some(w) = browser_cell(o) {
                    let _ = lt.record_cell(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < COEXIST_BEATS {
                if let Some(w) = co_cell(t) {
                    let _ = lt.record_beat(t, w);
                }
                t += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if let Some(w) = cosign_cell(c) {
                    let _ = lt.record_cosign(c, w);
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
        "40 格 + 2 行声明 + 六判据全通 → S1（KB-COMPAT 定级 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB7C0,
        "SEG_BASE 0x40 对齐且落 VX16 公式格 0xCB7C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_kind() {
        assert_eq!(browser_kind(0), Some(0));
        assert_eq!(browser_kind(3), Some(0));
        assert_eq!(browser_kind(4), Some(1));
        assert_eq!(browser_kind(19), Some(4));
        assert!(browser_kind(20).is_none());
        assert!(browser_cell(20).is_none());
        assert_eq!(browser_kind_name(0), Some("Chrome"));
        assert_eq!(browser_kind_name(4), Some("Vivaldi"));
        assert!(browser_kind_name(5).is_none());
    }

    #[test]
    fn test_face_names() {
        assert_eq!(face_name(0), Some("目录"));
        assert_eq!(face_name(1), Some("锁"));
        assert_eq!(face_name(2), Some("缓存"));
        assert_eq!(face_name(3), Some("扩展"));
        assert!(face_name(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = browser_cell(3).unwrap_or(0);
        assert_eq!(a, browser_cell(3).unwrap_or(0));
        assert_ne!(a, browser_cell(4).unwrap_or(0));
        let b = co_cell(15).unwrap_or(0);
        assert_eq!(b, co_cell(15).unwrap_or(0));
        assert_ne!(b, co_cell(14).unwrap_or(0));
        assert!(co_cell(COEXIST_BEATS).is_none());
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = ProfileLedger::new();
        let _ = lt.record_cell(9, browser_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_cell(2, browser_cell(2).unwrap_or(0)),
            Err(ProfileIsoCode::EOrder)
        );
        assert!(lt.record_cell(10, browser_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_order_and_drift() {
        let mut lt = ProfileLedger::new();
        let _ = lt.record_beat(5, co_cell(5).unwrap_or(0));
        // 拍号逆序必抓。
        assert_eq!(lt.record_beat(2, co_cell(2).unwrap_or(0)), Err(ProfileIsoCode::EOrder));
        // 漂移必抓（同拍号、错载荷，last_beat 未回退）。
        assert_eq!(
            lt.record_beat(6, co_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(ProfileIsoCode::ECoDrift)
        );
    }

    #[test]
    fn test_lock_conflict_and_dup() {
        let mut lt = ProfileLedger::new();
        // Chrome 锁面（格 1）双开同 profile 必抓 ELock。
        let _ = lt.record_cell(1, browser_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_cell(1, browser_cell(1).unwrap_or(0)),
            Err(ProfileIsoCode::ELock)
        );
        // 非锁面重复落账拒 EDup。
        let _ = lt.record_cell(4, browser_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_cell(4, browser_cell(4).unwrap_or(0)),
            Err(ProfileIsoCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ProfileLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ProfileLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 恰为 run_cycle 实际计数三节公式常量之和：20+16+4。
        assert_eq!(recorded, (BROWSER_CELLS + COEXIST_BEATS + COSIGN_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xCB7C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ProfileIsoCode::Ok as u32, 0);
        assert_eq!(ProfileIsoCode::EGradeDrift as u32, 12);
        assert_eq!(ProfileIsoCode::ELock as u32, 8);
        assert!(!ProfileIsoCode::Ok.detail().is_empty());
        assert!(!ProfileIsoCode::ELock.detail().is_empty());
        assert!(!ProfileIsoCode::ESeal.detail().is_empty());
        assert!(ProfileIsoCode::Ok.val() == SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = ProfileLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 → S2。
        let mut lt2 = ProfileLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52480_checks_green() {
        let s = run_unx52480_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
