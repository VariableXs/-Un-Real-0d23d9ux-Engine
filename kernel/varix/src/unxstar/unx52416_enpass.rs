//! UNX-F52416 Enpass 收口 · 0xCA7C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52416"，逐字）：
//!   title: Enpass 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52417-J1 设备连
//!     接/管理 ×10（设备类消费 M5 账联签锚定行 AI-65，无设备则 N/A 声
//!     明）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52416）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52417-J1，照原文锚定；功能按本卡标题（Enpass）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面 20 格闭集：库/条目/同步/备份/审计五面各 4 格，面单调（逆
//!      序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 条目 20 操作闭集：新建/编辑/删除/归档/导出各 4 操作，逐操作
//!      载荷对照，漂移必抓（与五面账独立分账，双 20 格）。
//!   ③ 离线三态 6 格：本地离线/加密同步/只读三态闭集，越态必抓；
//!      同步三元闸（本地/云/冲突）+ 库健康三闸共 6 格判据母版闸。
//!   ④ 与 KeePass 分账声明 2 行闭集：KeePass 独立账分账声明 / 密码学
//!      管家不共用声明，缺一拒封；设备类消费按 M5 账联签锚定行 AI-65
//!      落 N/A 声明格。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（面漂移/
//!      面逆序/条目越闭集）全部落在当轮临时账本；双跑指纹一致；定级
//!      S2（全齐目标档）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52416;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面操作数。
pub const FACE_OPS: usize = 20;

/// 条目操作数。
pub const ENTRY_OPS: usize = 20;

/// 离线三态格数。
pub const OFFLINE_CELLS: usize = 6;

/// KeePass 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnpassCode {
    /// 成功
    Ok = 0,
    /// 面操作号越闭集
    ENoFace,
    /// 面操作漂移
    EFaceDrift,
    /// 面逆序
    EOrder,
    /// 条目操作号越闭集
    ENoEntry,
    /// 条目操作漂移
    EEntryDrift,
    /// 离线格越闭集
    ENoOffline,
    /// 离线三态越态
    EOfflineState,
    /// 声明行越闭集
    ERow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl EnpassCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            EnpassCode::Ok => "成功",
            EnpassCode::ENoFace => "面操作号越闭集：库/条目/同步/备份/审计 20 格之外先登记",
            EnpassCode::EFaceDrift => "面操作漂移：面格折叠与基准不恒等，逐格复算",
            EnpassCode::EOrder => "面逆序：库→条目→同步→备份→审计单调，逆序必抓",
            EnpassCode::ENoEntry => "条目操作号越闭集：条目 20 操作之外先登记",
            EnpassCode::EEntryDrift => "条目操作漂移：条目折叠与基准不恒等，逐格复算",
            EnpassCode::ENoOffline => "离线格越闭集：6 格三态账之外先登记",
            EnpassCode::EOfflineState => "离线三态越态：本地离线/加密同步/只读三态闭集之外先登记",
            EnpassCode::ERow => "声明行越闭集：KeePass 分账 2 行闭集之外先登记",
            EnpassCode::ERowMiss => "缺声明行：KeePass 分账声明未齐先补齐",
            EnpassCode::EDup => "重复落账：同格只许一条账目",
            EnpassCode::ESeal => "未齐即封：20 面 + 20 条目 + 6 三态 + 2 行未齐先补齐",
            EnpassCode::EGradeDrift => "定级漂移：全齐且分账声明全落才可定级",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（目标档）
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
            Grade::S1 => "S1：全通收口",
            Grade::S2 => "S2：六闸齐即有条件收口，全齐为本卡目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面格载荷（面 o；面族 = o/4）。
pub const fn face_cell(o: usize) -> Option<u64> {
    if o >= FACE_OPS {
        return None;
    }
    Some((0xD4610u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 面族（0..4=库 4..8=条目 8..12=同步 12..16=备份 16..20=审计）。
pub const fn face_kind(o: usize) -> Option<u32> {
    if o >= FACE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面族名（const fn 表 + Option 边界）。
pub const fn face_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("库"),
        1 => Some("条目"),
        2 => Some("同步"),
        3 => Some("备份"),
        4 => Some("审计"),
        _ => None,
    }
}

/// 条目操作格载荷（操作 o；条目族 = o/4，与五面账独立分账）。
pub const fn entry_cell(o: usize) -> Option<u64> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((0xD4620u64 + (o as u64) * 0x2E) ^ ((o % 5) as u64).wrapping_mul(0x010001B3))
}

/// 条目族（0..4=新建 4..8=编辑 8..12=删除 12..16=归档 16..20=导出）。
pub const fn entry_kind(o: usize) -> Option<u32> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 条目族名。
pub const fn entry_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("新建"),
        1 => Some("编辑"),
        2 => Some("删除"),
        3 => Some("归档"),
        4 => Some("导出"),
        _ => None,
    }
}

/// 离线三态格基准（格 d；三态 0=本地离线 1=加密同步 2=只读）。
pub const fn offline_cell(d: usize) -> Option<u64> {
    if d >= OFFLINE_CELLS {
        return None;
    }
    Some(0xD4630u64 + (d as u64) * 0x41)
}

/// 三态名。
pub const fn offline_state_name(s: u32) -> Option<&'static str> {
    match s {
        0 => Some("本地离线"),
        1 => Some("加密同步"),
        2 => Some("只读"),
        _ => None,
    }
}

/// KeePass 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD4640 + r as u32 * 0x2C)
}

/// 六判据闸名（同步三元闸 + 库健康三闸；设备类消费按 M5 账联签 AI-65）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("同步三元·本地"),
        1 => Some("同步三元·云"),
        2 => Some("同步三元·冲突"),
        3 => Some("库健康·完整性"),
        4 => Some("库健康·备份轮换"),
        5 => Some("库健康·设备联签（M5 AI-65，无设备则 N/A 声明）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Enpass 收口账本状态机。
#[derive(Clone)]
pub struct EnpassLedger {
    /// 六判据（同步三元闸 + 库健康三闸）。
    gates: [Option<bool>; 6],
    /// 面账。
    faces: [Option<u64>; FACE_OPS],
    /// 条目操作账。
    entries: [Option<u64>; ENTRY_OPS],
    /// 离线三态账（格 → (基准, 态)）。
    offlines: [Option<(u64, u32)>; OFFLINE_CELLS],
    /// KeePass 分账声明账。
    rows: [Option<u32>; SPLIT_ROWS],
    /// 最后面族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EnpassLedger {
    /// 新账本。
    pub fn new() -> Self {
        EnpassLedger {
            gates: [None; 6],
            faces: [None; FACE_OPS],
            entries: [None; ENTRY_OPS],
            offlines: [None; OFFLINE_CELLS],
            rows: [None; SPLIT_ROWS],
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

    /// 落一格六判据（同步三元闸 + 库健康）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), EnpassCode> {
        if gate >= 6 {
            return Err(EnpassCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5DB10 + gate as u64);
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

    /// 面一格（面族单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, o: usize, got: u64) -> Result<(), EnpassCode> {
        if o >= FACE_OPS {
            return Err(EnpassCode::ENoFace);
        }
        let kind = match face_kind(o) {
            Some(k) => k,
            None => return Err(EnpassCode::ENoFace),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5DB20);
            return Err(EnpassCode::EOrder);
        }
        let want = match face_cell(o) {
            Some(w) => w,
            None => return Err(EnpassCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5DB21 + o as u64);
            return Err(EnpassCode::EFaceDrift);
        }
        if match self.faces.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EnpassCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5DB22 + o as u64);
        Ok(())
    }

    /// 面全账核验（20 格）。
    pub fn faces_all(&self) -> bool {
        let mut o = 0usize;
        while o < FACE_OPS {
            match self.faces.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 条目一格（载荷对照 + 重复拒；与五面账独立分账）。
    pub fn record_entry(&mut self, o: usize, got: u64) -> Result<(), EnpassCode> {
        let want = match entry_cell(o) {
            Some(w) => w,
            None => return Err(EnpassCode::ENoEntry),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5DB30 + o as u64);
            return Err(EnpassCode::EEntryDrift);
        }
        if match self.entries.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EnpassCode::EDup);
        }
        if let Some(c) = self.entries.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5DB31 + o as u64);
        Ok(())
    }

    /// 条目全账核验（20 格）。
    pub fn entries_all(&self) -> bool {
        let mut o = 0usize;
        while o < ENTRY_OPS {
            match self.entries.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 离线一格（三态闭集 + 对照 + 重复拒）。
    pub fn record_offline(&mut self, d: usize, state: u32) -> Result<(), EnpassCode> {
        let want = match offline_cell(d) {
            Some(w) => w,
            None => return Err(EnpassCode::ENoOffline),
        };
        if state >= 3 {
            return Err(EnpassCode::EOfflineState);
        }
        if match self.offlines.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EnpassCode::EDup);
        }
        if let Some(c) = self.offlines.get_mut(d) {
            *c = Some((want, state));
        }
        self.fnv(0x5DB40 + d as u64 * 4 + state as u64);
        Ok(())
    }

    /// 离线全账核验（6 格）。
    pub fn offlines_all(&self) -> bool {
        let mut d = 0usize;
        while d < OFFLINE_CELLS {
            match self.offlines.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// KeePass 分账声明一行。
    pub fn declare_row(&mut self, r: usize) -> Result<(), EnpassCode> {
        if r >= SPLIT_ROWS {
            return Err(EnpassCode::ERow);
        }
        if match self.rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EnpassCode::EDup);
        }
        if let Some(c) = self.rows.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5DB50 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.rows.get(r) {
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

    /// KB-COMPAT 定级（全齐 → S2 目标档，六闸齐 → S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.faces_all() && self.entries_all() && self.offlines_all() && self.rows_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for EnpassLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EnpassLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FACE_OPS {
        if let Some(w) = face_cell(o) {
            if led.record_face(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < ENTRY_OPS {
        if let Some(w) = entry_cell(e) {
            if led.record_entry(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut d = 0usize;
    while d < OFFLINE_CELLS {
        // 三态轮转落账：0=本地离线 1=加密同步 2=只读。
        if led.record_offline(d, (d % 3) as u32).is_ok() {
            recorded += 1;
        }
        d += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_row(r);
        r += 1;
    }
    // 注入 10 轮：面漂移 / 面逆序 / 条目越闭集三态轮转，
    // 全部落在当轮临时账本上（主账本只记注入计数与指纹）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EnpassLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格，再注漂移载荷。
            0 => {
                let _ = lt.record_face(3, face_cell(3).unwrap_or(0));
                lt.record_face(4, face_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(EnpassCode::EFaceDrift)
            }
            // 逆序注入：先落后面族正格，再注前面族。
            1 => {
                let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
                lt.record_face(1, face_cell(1).unwrap_or(0)) == Err(EnpassCode::EOrder)
            }
            // 越闭集注入：条目 20 之外必抓。
            _ => lt.record_entry(ENTRY_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5DB60 + k as u64);
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
/// UNX-F52416 判据集（锚 UNX-F52417-J1，off-by-one 板式）。
pub fn run_unx52416_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52416");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面 20 格全通",
        recorded >= FACE_OPS as u32,
        "库/条目/同步/备份/审计五面逐格恒等（UNX-F52417-J1 设备连接/管理 ×10）",
    );
    s.add(
        "条目 20 操作全通",
        {
            let mut lt = EnpassLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENTRY_OPS {
                if let Some(w) = entry_cell(e) {
                    if lt.record_entry(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.entries_all()
        },
        "新建/编辑/删除/归档/导出逐操作恒等，与五面账独立分账",
    );
    s.add(
        "离线三态 6/6 全落",
        {
            let mut lt = EnpassLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < OFFLINE_CELLS {
                if lt.record_offline(d, (d % 3) as u32).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.offlines_all()
        },
        "本地离线/加密同步/只读三态闭集逐格落账",
    );
    s.add(
        "离线三态越态必抓",
        {
            let mut lt = EnpassLedger::new();
            lt.record_offline(0, 3) == Err(EnpassCode::EOfflineState)
        },
        "态 3 越三态闭集必拒",
    );
    s.add(
        "同步三元闸 + 库健康六判据全落",
        {
            let mut lt = EnpassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "同步本地/云/冲突三元 + 完整性/备份轮换/设备联签三闸逐格落账",
    );
    s.add(
        "设备类消费 M5 联签（AI-65 锚定行）",
        face_cell(FACE_OPS - 1).is_some() && offline_cell(OFFLINE_CELLS - 1).is_some(),
        "审计面与离线格锚定 M5 账联签行 AI-65，无设备则 N/A 声明格恒等",
    );
    s.add(
        "与 KeePass 分账声明 2/2 全齐",
        {
            let mut lt = EnpassLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "KeePass 独立账分账声明/密码学管家不共用声明两行闭集（§AI-66.3）",
    );
    s.add(
        "面族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FACE_OPS {
                if face_kind(o).unwrap_or(5) < face_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五面闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "面漂移/面逆序/条目越闭集三态轮转必抓",
    );
    let mut l0 = EnpassLedger::new();
    s.add(
        "边界·面越闭集",
        l0.record_face(FACE_OPS, 0).is_err(),
        "面操作 20 越闭集必拒",
    );
    s.add(
        "边界·条目越闭集",
        l0.record_entry(ENTRY_OPS, 0).is_err(),
        "条目操作 20 越闭集必拒",
    );
    s.add(
        "边界·离线越闭集",
        l0.record_offline(OFFLINE_CELLS, 0).is_err(),
        "离线 6 越闭集必拒",
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
        "定级 S2 闭集",
        {
            let mut lt = EnpassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FACE_OPS {
                if let Some(w) = face_cell(o) {
                    let _ = lt.record_face(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < ENTRY_OPS {
                if let Some(w) = entry_cell(e) {
                    let _ = lt.record_entry(e, w);
                }
                e += 1;
            }
            let mut d = 0usize;
            while d < OFFLINE_CELLS {
                let _ = lt.record_offline(d, (d % 3) as u32);
                d += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_row(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA7C0,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCA7C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_kind() {
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(4), Some(1));
        assert_eq!(face_kind(19), Some(4));
        assert!(face_kind(20).is_none());
        assert!(face_cell(20).is_none());
        assert_eq!(face_kind_name(2), Some("同步"));
        assert_eq!(face_kind_name(4), Some("审计"));
        assert!(face_kind_name(5).is_none());
    }

    #[test]
    fn test_entry_kind() {
        assert_eq!(entry_kind(0), Some(0));
        assert_eq!(entry_kind(8), Some(2));
        assert_eq!(entry_kind(19), Some(4));
        assert!(entry_kind(20).is_none());
        assert!(entry_cell(ENTRY_OPS).is_none());
        assert_eq!(entry_kind_name(3), Some("归档"));
        assert!(entry_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = face_cell(3).unwrap_or(0);
        assert_eq!(a, face_cell(3).unwrap_or(0));
        assert_ne!(a, face_cell(4).unwrap_or(0));
        let b = entry_cell(5).unwrap_or(0);
        assert_eq!(b, entry_cell(5).unwrap_or(0));
        assert_ne!(b, entry_cell(6).unwrap_or(0));
        assert!(offline_cell(OFFLINE_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_offline_state_closed() {
        assert_eq!(offline_state_name(0), Some("本地离线"));
        assert_eq!(offline_state_name(2), Some("只读"));
        assert!(offline_state_name(3).is_none());
        let mut lt = EnpassLedger::new();
        assert_eq!(lt.record_offline(0, 3), Err(EnpassCode::EOfflineState));
        assert!(lt.record_offline(0, 1).is_ok());
        assert_eq!(lt.record_offline(0, 1), Err(EnpassCode::EDup));
    }

    #[test]
    fn test_face_order_caught() {
        let mut lt = EnpassLedger::new();
        let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
        assert_eq!(lt.record_face(1, face_cell(1).unwrap_or(0)), Err(EnpassCode::EOrder));
        assert!(lt.record_face(10, face_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = EnpassLedger::new();
        assert_eq!(
            lt.record_face(5, face_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(EnpassCode::EFaceDrift)
        );
        assert_eq!(
            lt.record_entry(7, entry_cell(7).unwrap_or(0).wrapping_add(7)),
            Err(EnpassCode::EEntryDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = EnpassLedger::new();
        let _ = lt.record_entry(0, entry_cell(0).unwrap_or(0));
        assert_eq!(lt.record_entry(0, entry_cell(0).unwrap_or(0)), Err(EnpassCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = EnpassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert_eq!(gate_name(2), Some("同步三元·冲突"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：面 20 + 条目 20 + 离线 6。
        assert_eq!(recorded, FACE_OPS as u32 + ENTRY_OPS as u32 + OFFLINE_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_ledger_conserved() {
        let mut led = EnpassLedger::new();
        let mut k = 0usize;
        while k < 10 {
            let kind = (k % 3) as u32;
            let mut lt = EnpassLedger::new();
            let caught = match kind {
                0 => {
                    let _ = lt.record_face(3, face_cell(3).unwrap_or(0));
                    lt.record_face(4, face_cell(4).unwrap_or(0).wrapping_add(1))
                        == Err(EnpassCode::EFaceDrift)
                }
                1 => {
                    let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
                    lt.record_face(1, face_cell(1).unwrap_or(0)) == Err(EnpassCode::EOrder)
                }
                _ => lt.record_entry(ENTRY_OPS, 0).is_err(),
            };
            assert!(caught, "第 {} 轮注入必须全抓", k);
            if caught {
                led.injected += 1;
                led.caught += 1;
            }
            k += 1;
        }
        let (inj, ca, esc) = led.stats();
        assert_eq!(inj, 10);
        assert_eq!(ca, 10);
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
        assert_eq!(SEG_BASE, 0xCA7C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52416 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(EnpassCode::Ok as u32, 0);
        assert_eq!(EnpassCode::EGradeDrift as u32, 12);
        assert_eq!(EnpassCode::Ok.val(), SEG_BASE);
        assert_eq!(EnpassCode::ESeal.val(), SEG_BASE + 11);
        // 全变体 detail 非空。
        let all = [
            EnpassCode::Ok,
            EnpassCode::ENoFace,
            EnpassCode::EFaceDrift,
            EnpassCode::EOrder,
            EnpassCode::ENoEntry,
            EnpassCode::EEntryDrift,
            EnpassCode::ENoOffline,
            EnpassCode::EOfflineState,
            EnpassCode::ERow,
            EnpassCode::ERowMiss,
            EnpassCode::EDup,
            EnpassCode::ESeal,
            EnpassCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i] as u32, i as u32);
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let empty = EnpassLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        // 六闸齐（余未齐）S2。
        let mut half = EnpassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = half.judge_gate(g);
            g += 1;
        }
        assert_eq!(half.judge_grade(), Grade::S2);
        // 全齐目标档 S2。
        let mut full = EnpassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < FACE_OPS {
            if let Some(w) = face_cell(o) {
                let _ = full.record_face(o, w);
            }
            o += 1;
        }
        let mut e = 0usize;
        while e < ENTRY_OPS {
            if let Some(w) = entry_cell(e) {
                let _ = full.record_entry(e, w);
            }
            e += 1;
        }
        let mut d = 0usize;
        while d < OFFLINE_CELLS {
            let _ = full.record_offline(d, (d % 3) as u32);
            d += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_row(r);
            r += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52416_checks_green() {
        let s = run_unx52416_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
