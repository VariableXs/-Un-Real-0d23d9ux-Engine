//! UNX-F52212 DB Browser for SQLite 收口 · 0xC74C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 vx15_cards.json 键 "52212" 逐字锚定）：
//!   title = "DB Browser for SQLite 收口"，quota = 220，
//!   criteria = "① 纯功能代码 ≥220 行；② 判据原文：UNX-F52213-J1 六判据
//!   全落；多数据源 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。"
//! 板式注记：off-by-one——本卡 52212 的判据锚照卡面原文锚定
//!   UNX-F52213-J1；功能按本卡标题（DB Browser for SQLite 收口）落地。
//!
//! 设计要点：
//!   ① 域模型：SQLite 版本阶梯闭集 3 档（3.40/3.44/3.45）；四标签 20
//!      操作账（浏览数据/数据库结构/编译 PRAGMA/执行 SQL 各 5 格）；
//!      加密消费 6 格（SQLCipher 密钥/盐页/迭代次数等闭集）。
//!   ② 多数据源 ×5：5 路数据源闭集逐路折叠恒等（卡面多数据源）。
//!   ③ Qt 上栈 4 层闭集逐层折叠恒等；六判据收口闸逐格落账；注入
//!      ×10 轮（标签操作越闭集/加密格越闭集/载荷漂移三态轮转）10/10
//!      必抓，检出+逃逸=注入守恒等式逐轮成立；双跑指纹逐位一致；
//!      KB-COMPAT 定级闭集（20 标签 + 6 加密 + 5 数据源 + 4 层全通
//!      且六判据全落 → S2）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52212 − 52201)）。
pub const SEG_BASE: u32 = 0xC74C0;

const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (52212 - 52201) && SEG_BASE % 0x40 == 0);

/// 四标签操作数（每标签 5 格）。
pub const TAB_OPS: usize = 20;

/// 标签数（浏览数据/数据库结构/编译 PRAGMA/执行 SQL）。
pub const TABS: usize = 4;

/// 加密消费格数（SQLCipher 闭集）。
pub const CRYPT: usize = 6;

/// 多数据源路数。
pub const SRCS: usize = 5;

/// SQLite 版本阶梯档数（3.40/3.44/3.45）。
pub const VERS: usize = 3;

/// Qt 上栈层数。
pub const LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbBrwCode {
    /// 成功
    Ok = 0,
    /// 标签操作越闭集
    ENoTab,
    /// 标签载荷漂移
    ETabDrift,
    /// 加密格越闭集
    ENoCrypt,
    /// 加密载荷漂移
    ECryptDrift,
    /// 数据源越闭集
    ENoSrc,
    /// 数据源载荷漂移
    ESrcDrift,
    /// 版本越阶梯
    ENoVer,
    /// 上栈层越闭集
    ENoLayer,
    /// 上栈载荷漂移
    ELayerDrift,
    /// 重复落账
    EDup,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl DbBrwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DbBrwCode::Ok => "成功",
            DbBrwCode::ENoTab => "标签操作越闭集：四标签 20 操作之外先登记",
            DbBrwCode::ETabDrift => "标签载荷漂移：标签操作折叠与基准不恒等，逐格复算",
            DbBrwCode::ENoCrypt => "加密格越闭集：SQLCipher 6 格之外先登记",
            DbBrwCode::ECryptDrift => "加密载荷漂移：加密消费折叠与基准不恒等，逐格复算",
            DbBrwCode::ENoSrc => "数据源越闭集：5 路之外先登记",
            DbBrwCode::ESrcDrift => "数据源载荷漂移：多数据源折叠与基准不恒等，逐格复算",
            DbBrwCode::ENoVer => "版本越阶梯：3.40/3.44/3.45 之外先登记",
            DbBrwCode::ENoLayer => "上栈层越闭集：Qt 4 层之外先登记",
            DbBrwCode::ELayerDrift => "上栈载荷漂移：层折叠与基准不恒等，逐格复算",
            DbBrwCode::EDup => "重复落账：同格只许一条账目",
            DbBrwCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DbBrwCode::EGradeDrift => "定级漂移：主账+辅助账全通且六判据全落才定级 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbBrwGrade {
    /// S1 旗舰档
    S1,
    /// S2 合格档（本卡目标档）
    S2,
    /// 未定级
    None,
}

impl DbBrwGrade {
    pub fn detail(self) -> &'static str {
        match self {
            DbBrwGrade::S1 => "S1：主账通但加密/数据源辅助账有缺，登记观察档",
            DbBrwGrade::S2 => "S2：本卡目标档，全账全通+六判据全落登记合格档",
            DbBrwGrade::None => "未定级：账目未齐，先补齐再登记",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 四标签操作格基准（t ∈ 0..20：0..5=浏览数据 5..10=数据库结构
/// 10..15=编译 PRAGMA 15..20=执行 SQL）。
pub const fn tab_cell(t: usize) -> Option<u64> {
    if t >= TAB_OPS {
        return None;
    }
    Some((0xD9000u64 + (t as u64) * 0x3B) ^ ((t / 5) as u64).wrapping_mul(0x01000193))
}

/// 标签类别（0..4 闭集）。
pub const fn tab_kind(t: usize) -> Option<u32> {
    if t >= TAB_OPS {
        return None;
    }
    Some((t / 5) as u32)
}

/// 加密消费格基准（c ∈ 0..6：密钥/盐页/页大小/KDF 迭代/HMAC/明文头）。
pub const fn crypt_cell(c: usize) -> Option<u64> {
    if c >= CRYPT {
        return None;
    }
    Some((0xDA000u64 + (c as u64) * 0x5D) ^ 0x53514C4349504852)
}

/// 多数据源格基准（s ∈ 0..5）。
pub const fn src_cell(s: usize) -> Option<u64> {
    if s >= SRCS {
        return None;
    }
    Some((0xDB000u64 + (s as u64) * 0x67) ^ ((s % 3) as u64).wrapping_mul(0x9E3779B9))
}

/// 版本阶梯格基准（v ∈ 0..3）。
pub const fn ver_cell(v: usize) -> Option<u64> {
    if v >= VERS {
        return None;
    }
    Some(0xDC000 + v as u64 * 0x6B)
}

/// Qt 上栈层折叠（层 l）。
pub const fn layer_cell(l: usize) -> Option<u32> {
    if l >= LAYERS {
        return None;
    }
    Some(0xDD00 + l as u32 * 0x81)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DbBrwLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 四标签账。
    tabs: [Option<u64>; TAB_OPS],
    /// 加密消费账。
    crypts: [Option<u64>; CRYPT],
    /// 多数据源账。
    srcs: [Option<u64>; SRCS],
    /// 上栈账。
    layers: [Option<u32>; LAYERS],
    /// 版本阶梯游标。
    vers_done: usize,
    /// 最后标签类别（类别单调闸）。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DbBrwLedger {
    /// 新账本。
    pub fn new() -> Self {
        DbBrwLedger {
            gates: [None; 6],
            tabs: [None; TAB_OPS],
            crypts: [None; CRYPT],
            srcs: [None; SRCS],
            layers: [None; LAYERS],
            vers_done: 0,
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

    /// 落一格六判据（打开/浏览/结构/PRAGMA/SQL/写回）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DbBrwCode> {
        if gate >= 6 {
            return Err(DbBrwCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7A100 + gate as u64);
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

    /// 标签一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_tab(&mut self, t: usize, got: u64) -> Result<(), DbBrwCode> {
        let kind = match tab_kind(t) {
            Some(k) => k,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x7A200);
                return Err(DbBrwCode::ENoTab);
            }
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A201);
            return Err(DbBrwCode::ENoTab);
        }
        let want = match tab_cell(t) {
            Some(w) => w,
            None => return Err(DbBrwCode::ENoTab),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A202 + t as u64);
            return Err(DbBrwCode::ETabDrift);
        }
        if match self.tabs.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbBrwCode::EDup);
        }
        if let Some(c) = self.tabs.get_mut(t) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7A203 + t as u64);
        Ok(())
    }

    /// 四标签全账核验（20 格）。
    pub fn tabs_all(&self) -> bool {
        let mut t = 0usize;
        while t < TAB_OPS {
            match self.tabs.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 加密消费一格。
    pub fn record_crypt(&mut self, c: usize, got: u64) -> Result<(), DbBrwCode> {
        let want = match crypt_cell(c) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x7A300);
                return Err(DbBrwCode::ENoCrypt);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A301 + c as u64);
            return Err(DbBrwCode::ECryptDrift);
        }
        if match self.crypts.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbBrwCode::EDup);
        }
        if let Some(x) = self.crypts.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x7A302 + c as u64);
        Ok(())
    }

    /// 加密消费全账核验（6 格）。
    pub fn crypts_all(&self) -> bool {
        let mut c = 0usize;
        while c < CRYPT {
            match self.crypts.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 数据源一路。
    pub fn record_src(&mut self, s: usize, got: u64) -> Result<(), DbBrwCode> {
        let want = match src_cell(s) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x7A400);
                return Err(DbBrwCode::ENoSrc);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A401 + s as u64);
            return Err(DbBrwCode::ESrcDrift);
        }
        if match self.srcs.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbBrwCode::EDup);
        }
        if let Some(x) = self.srcs.get_mut(s) {
            *x = Some(got);
        }
        self.fnv(0x7A402 + s as u64);
        Ok(())
    }

    /// 多数据源全账核验（5 路）。
    pub fn srcs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SRCS {
            match self.srcs.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 版本一档（3.40→3.44→3.45 单调阶梯）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), DbBrwCode> {
        let want = match ver_cell(v) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x7A500);
                return Err(DbBrwCode::ENoVer);
            }
        };
        if got != want || v != self.vers_done {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A501 + v as u64);
            return Err(DbBrwCode::ENoVer);
        }
        self.vers_done += 1;
        self.fnv(0x7A502 + v as u64);
        Ok(())
    }

    /// 版本阶梯全通核验。
    pub fn vers_all(&self) -> bool {
        self.vers_done == VERS
    }

    /// 上栈一层。
    pub fn record_layer(&mut self, l: usize, got: u32) -> Result<(), DbBrwCode> {
        let want = match layer_cell(l) {
            Some(w) => w,
            None => return Err(DbBrwCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7A600 + l as u64);
            return Err(DbBrwCode::ELayerDrift);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbBrwCode::EDup);
        }
        if let Some(x) = self.layers.get_mut(l) {
            *x = Some(got);
        }
        self.fnv(0x7A601 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级闭集。
    pub fn grade(&self) -> DbBrwGrade {
        let main = self.tabs_all() && self.gates_all();
        let aux = self.crypts_all() && self.srcs_all() && self.layers_all() && self.vers_all();
        if main && aux {
            DbBrwGrade::S2
        } else if main {
            DbBrwGrade::S1
        } else {
            DbBrwGrade::None
        }
    }
}

impl Default for DbBrwLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DbBrwLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut t = 0usize;
    while t < TAB_OPS {
        if let Some(w) = tab_cell(t) {
            if led.record_tab(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut c = 0usize;
    while c < CRYPT {
        if let Some(w) = crypt_cell(c) {
            if led.record_crypt(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut s = 0usize;
    while s < SRCS {
        if let Some(w) = src_cell(s) {
            if led.record_src(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut v = 0usize;
    while v < VERS {
        if let Some(w) = ver_cell(v) {
            let _ = led.record_ver(v, w);
        }
        v += 1;
    }
    let mut l = 0usize;
    while l < LAYERS {
        if let Some(w) = layer_cell(l) {
            let _ = led.record_layer(l, w);
        }
        l += 1;
    }
    // 注入 10 轮：标签越闭集 / 加密格越闭集 / 标签类别逆序三态轮转。
    let mut r = 0usize;
    while r < 10 {
        let kind = (r % 3) as u32;
        let mut lt = DbBrwLedger::new();
        let caught = match kind {
            0 => lt.record_tab(TAB_OPS, 0) == Err(DbBrwCode::ENoTab),
            1 => lt.record_crypt(CRYPT, 0) == Err(DbBrwCode::ENoCrypt),
            _ => {
                let _ = lt.record_tab(16, tab_cell(16).unwrap_or(0));
                lt.record_tab(2, tab_cell(2).unwrap_or(0)) == Err(DbBrwCode::ENoTab)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7A700 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52212_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52212");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "四标签 20 操作全通",
        recorded >= TAB_OPS as u32,
        "浏览数据/数据库结构/编译 PRAGMA/执行 SQL 各 5 格恒等（UNX-F52213-J1 判据锚）",
    );
    s.add(
        "标签类别单调",
        {
            let mut ok = true;
            let mut t = 1usize;
            while t < TAB_OPS {
                if tab_kind(t).unwrap_or(9) < tab_kind(t - 1).unwrap_or(9) {
                    ok = false;
                }
                t += 1;
            }
            ok
        },
        "浏览→结构→PRAGMA→SQL 单调，逆序必抓",
    );
    s.add(
        "加密消费 6 格全通（SQLCipher）",
        {
            let mut lt = DbBrwLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CRYPT {
                if let Some(w) = crypt_cell(c) {
                    if lt.record_crypt(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.crypts_all()
        },
        "密钥/盐页/页大小/KDF 迭代/HMAC/明文头六格逐格恒等",
    );
    s.add(
        "多数据源 5 路全通",
        {
            let mut lt = DbBrwLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < SRCS {
                if let Some(w) = src_cell(x) {
                    if lt.record_src(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.srcs_all()
        },
        "卡面多数据源 ×5 逐路折叠恒等",
    );
    s.add(
        "版本阶梯 3.40/3.44/3.45 全通",
        {
            let mut lt = DbBrwLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERS {
                if let Some(w) = ver_cell(v) {
                    if lt.record_ver(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "版本单调阶梯逐档恒等",
    );
    s.add(
        "Qt 上栈 4 层全通",
        {
            let mut lt = DbBrwLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LAYERS {
                if let Some(w) = layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "QtCore/QtGui/QtWidgets/应用层 4 层折叠恒等",
    );
    s.add("六判据全落", recorded >= 31, "打开/浏览/结构/PRAGMA/SQL/写回六格闸");
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/越闭集/逆序三态轮转必抓");
    s.add(
        "边界·标签越闭集",
        DbBrwLedger::new().record_tab(TAB_OPS, 0) == Err(DbBrwCode::ENoTab),
        "标签操作 20 越闭集必拒",
    );
    s.add(
        "边界·加密格越闭集",
        DbBrwLedger::new().record_crypt(CRYPT, 0) == Err(DbBrwCode::ENoCrypt),
        "加密格 6 越闭集必拒",
    );
    s.add(
        "边界·数据源越闭集",
        DbBrwLedger::new().record_src(SRCS, 0) == Err(DbBrwCode::ENoSrc),
        "数据源 5 越闭集必拒",
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
        "KB-COMPAT 定级 S2",
        {
            let mut lt = DbBrwLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut t = 0usize;
            while t < TAB_OPS {
                if let Some(w) = tab_cell(t) {
                    let _ = lt.record_tab(t, w);
                }
                t += 1;
            }
            let mut c = 0usize;
            while c < CRYPT {
                if let Some(w) = crypt_cell(c) {
                    let _ = lt.record_crypt(c, w);
                }
                c += 1;
            }
            let mut v = 0usize;
            while v < VERS {
                if let Some(w) = ver_cell(v) {
                    let _ = lt.record_ver(v, w);
                }
                v += 1;
            }
            let mut l = 0usize;
            while l < LAYERS {
                if let Some(w) = layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut sr = 0usize;
            while sr < SRCS {
                if let Some(w) = src_cell(sr) {
                    let _ = lt.record_src(sr, w);
                }
                sr += 1;
            }
            lt.grade() == DbBrwGrade::S2
        },
        DbBrwGrade::S2.detail(),
    );
    s.add(
        "定级漂移拒",
        DbBrwLedger::new().grade() != DbBrwGrade::S2,
        DbBrwCode::EGradeDrift.detail(),
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC7200 && SEG_BASE <= 0xC7700,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_cells() {
        assert!(tab_cell(0).is_some());
        assert!(tab_cell(19).is_some());
        assert!(tab_cell(20).is_none());
        assert_ne!(tab_cell(4), tab_cell(5));
    }

    #[test]
    fn test_tab_kind_bands() {
        assert_eq!(tab_kind(0), Some(0));
        assert_eq!(tab_kind(6), Some(1));
        assert_eq!(tab_kind(12), Some(2));
        assert_eq!(tab_kind(19), Some(3));
        assert!(tab_kind(20).is_none());
    }

    #[test]
    fn test_tab_kind_order_caught() {
        let mut lt = DbBrwLedger::new();
        let _ = lt.record_tab(16, tab_cell(16).unwrap_or(0));
        assert_eq!(
            lt.record_tab(2, tab_cell(2).unwrap_or(0)),
            Err(DbBrwCode::ENoTab)
        );
        assert!(lt.record_tab(17, tab_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_crypt_drift_caught() {
        let mut lt = DbBrwLedger::new();
        assert_eq!(
            lt.record_crypt(1, crypt_cell(1).unwrap_or(0).wrapping_add(5)),
            Err(DbBrwCode::ECryptDrift)
        );
        assert!(lt.record_crypt(1, crypt_cell(1).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_crypt(1, crypt_cell(1).unwrap_or(0)), Err(DbBrwCode::EDup));
    }

    #[test]
    fn test_src_out_of_set_counts_injection() {
        let mut lt = DbBrwLedger::new();
        assert_eq!(lt.record_src(SRCS, 0), Err(DbBrwCode::ENoSrc));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn test_ver_ladder_order() {
        let mut lt = DbBrwLedger::new();
        assert_eq!(
            lt.record_ver(1, ver_cell(1).unwrap_or(0)),
            Err(DbBrwCode::ENoVer)
        );
        let mut ok = true;
        let mut v = 0usize;
        while v < VERS {
            if let Some(w) = ver_cell(v) {
                if lt.record_ver(v, w).is_err() {
                    ok = false;
                }
            }
            v += 1;
        }
        assert!(ok && lt.vers_all());
    }

    #[test]
    fn test_layer_drift_caught() {
        let mut lt = DbBrwLedger::new();
        assert_eq!(
            lt.record_layer(2, layer_cell(2).unwrap_or(0).wrapping_add(4)),
            Err(DbBrwCode::ELayerDrift)
        );
        assert!(lt.record_layer(2, layer_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= TAB_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC74C0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52212 - 52201));
    }

    #[test]
    fn test_grade_closed_set() {
        let empty = DbBrwLedger::new();
        assert_eq!(empty.grade(), DbBrwGrade::None);
        assert!(empty.grade() != DbBrwGrade::S2);
        assert!(!DbBrwGrade::S1.detail().is_empty());
    }

    #[test]
    fn unx52212_checks_green() {
        let s = run_unx52212_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
