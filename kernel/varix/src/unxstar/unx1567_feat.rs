//! UNX-F1567 移交包五件终验（接口/账本/预案/文档/上手章） · 0x78CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-1104 B06 卡 4 · UNX-F1567 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1567-J1 移交包五件终验（五件逐件终验齐备：
//!     件在包内+哈希对平+版本终版三断言），缺件注入 10/10 必抓，
//!     包哈希落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五件闭集：接口册 / 账本册 / 预案册 / 文档册 / 上手章逐件登记
//!     （件名 / 席位 / 冻结哈希 / 终版版本号），缺一不可；
//!   - 逐件三断言：件在包内（席位占位）+ 哈希对平（现版哈希与冻结
//!     哈希逐位相等）+ 版本终版（版本号恰为冻结终版值）；
//!   - 缺件注入 10/10：十路缺件（逐件两路：抽件 / 清哈希）逐路必抓，
//!     检出计数恰 10；
//!   - 包哈希落档：五件哈希序贯合成 FNV-1a 包指纹，包摘要 9 行落档
//!     双跑逐位一致；
//!   - 状态机单调：登记→终验→落档，倒退跳级显性拒；
//!   - LCG 抽验复验，种子写死 = SEG_BASE；零浮点 / IO / 墙钟 /
//!     unwrap / panic；整数运算；中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78CC0，派工卡 VX8-1104 B06 卡 4 独占窗口）。
pub const SEG_BASE: u32 = 0x78CC0;

/// 移交包件数（判据 J1「五件」冻结）。
pub const PACK_COUNT: usize = 5;

/// 包摘要行数（头 + 5 件行 + 尾，冻结）。
pub const PACK_ROWS: usize = 7;

/// 终版版本号（五件统一冻结终版值）。
pub const FINAL_VERSION: u32 = 0x0001_1040;

/// 缺件注入路数（判据 J1「10/10」冻结）。
pub const MISS_ROUTES: usize = 10;

// ── 五件冻结表（件名 / 席位 / 冻结哈希） ────────────────────────
pub const PACK_TABLE: [(&str, usize, u32); 5] = [
    ("接口册", 0, 0x11A0_0001),
    ("账本册", 1, 0x11A0_0002),
    ("预案册", 2, 0x11A0_0003),
    ("文档册", 3, 0x11A0_0004),
    ("上手章", 4, 0x11A0_0005),
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pack67Code {
    /// 件越闭集：件名收进五件冻结表
    ItemUnknown,
    /// 席位越闭集：席位收进 0..=4
    SeatUnknown,
    /// 缺件：五件缺一即终验拒，补齐再验
    ItemMissing,
    /// 哈希失平：现版哈希与冻结哈希不一致，回滚冻结版
    HashMismatch,
    /// 版本非终版：版本号须恰为终版值
    VersionNotFinal,
    /// 件名为空：补件名再登记
    NameEmpty,
    /// 包件表长度违约：终验窗必须恰 5 件
    WindowBad,
    /// 缺件检出不足：十路注入逐路必抓
    MissMissed,
    /// 终验件状态倒退：登记→终验→落档单调推进
    PhaseRegress,
    /// 终验件状态跳级：未五件全过不得落档
    PhaseSkip,
    /// 包摘要行数违约：摘要必须恰 7 行
    PackRowsBad,
    /// 双跑不一致：排查登记序列随机性后复跑
    NotReproducible,
}

impl Pack67Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Pack67Code::ItemUnknown => "件越闭集：件名收进五件冻结表再登记",
            Pack67Code::SeatUnknown => "席位越闭集：席位收进 0..=4 再占位",
            Pack67Code::ItemMissing => "缺件：五件缺一即终验拒，补齐缺失件再验",
            Pack67Code::HashMismatch => "哈希失平：现版哈希与冻结哈希不一致，回滚冻结版再验",
            Pack67Code::VersionNotFinal => "版本非终版：版本号须恰为终版值再终验",
            Pack67Code::NameEmpty => "件名为空：补件名再登记",
            Pack67Code::WindowBad => "包件表长度违约：终验窗必须恰 5 件",
            Pack67Code::MissMissed => "缺件检出不足：十路注入逐路必抓再终验",
            Pack67Code::PhaseRegress => "终验件状态倒退：登记→终验→落档单调推进",
            Pack67Code::PhaseSkip => "终验件状态跳级：未五件全过不得落档",
            Pack67Code::PackRowsBad => "包摘要行数违约：摘要必须恰 7 行",
            Pack67Code::NotReproducible => "双跑不一致：排查登记序列随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Pack67Code; 12] = [
        Pack67Code::ItemUnknown,
        Pack67Code::SeatUnknown,
        Pack67Code::ItemMissing,
        Pack67Code::HashMismatch,
        Pack67Code::VersionNotFinal,
        Pack67Code::NameEmpty,
        Pack67Code::WindowBad,
        Pack67Code::MissMissed,
        Pack67Code::PhaseRegress,
        Pack67Code::PhaseSkip,
        Pack67Code::PackRowsBad,
        Pack67Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Pack67Code::ALL.len() {
        let c = match Pack67Code::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── LCG（种子固定 = SEG_BASE，零随机源） ────────────────────────
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    /// 逐步推进并取模返回。
    pub fn next_mod(&mut self, m: usize) -> usize {
        self.state = self
            .state
            .wrapping_mul(1_103_515_245)
            .wrapping_add(12_345);
        let v = (self.state >> 16) as usize;
        if m == 0 {
            return 0;
        }
        v % m
    }
}

// ── 终验状态机（登记 → 终验 → 落档，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackPhase {
    /// 登记期：五件逐件登记（件在包内占位）
    Enrolling,
    /// 终验期：五件三断言全过
    Passed,
    /// 落档期：包哈希落档封账
    Sealed,
}

impl PackPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            PackPhase::Enrolling => 0,
            PackPhase::Passed => 1,
            PackPhase::Sealed => 2,
        }
    }
}

/// 单件登记行。
#[derive(Clone, Debug)]
pub struct PackItem {
    pub name: String,
    pub seat: usize,
    pub hash: u32,
    pub version: u32,
}

/// 移交包终验件。
pub struct HandoverPack {
    phase: PackPhase,
    items: Vec<PackItem>,
    pack_digest: u32,
    summary: Option<String>,
}

impl HandoverPack {
    pub fn new() -> HandoverPack {
        HandoverPack {
            phase: PackPhase::Enrolling,
            items: Vec::new(),
            pack_digest: 0,
            summary: None,
        }
    }

    pub fn phase(&self) -> PackPhase {
        self.phase
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn pack_digest(&self) -> u32 {
        self.pack_digest
    }

    pub fn summary_text(&self) -> Option<&str> {
        match self.summary.as_ref() {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    /// 件登记：件名闭集 + 席位闭集 + 唯一 + 哈希非零。
    pub fn enroll(&mut self, name: &str, seat: usize, hash: u32) -> Result<(), u32> {
        if self.phase.rank() >= PackPhase::Passed.rank() {
            return Err(Pack67Code::PhaseRegress.val());
        }
        if name.is_empty() {
            return Err(Pack67Code::NameEmpty.val());
        }
        let mut known = false;
        let mut i = 0usize;
        while i < PACK_TABLE.len() {
            let row = match PACK_TABLE.get(i) {
                Some(r) => *r,
                None => break,
            };
            if row.0 == name {
                known = true;
            }
            i += 1;
        }
        if !known {
            return Err(Pack67Code::ItemUnknown.val());
        }
        if seat >= PACK_COUNT {
            return Err(Pack67Code::SeatUnknown.val());
        }
        let mut j = 0usize;
        while j < self.items.len() {
            let it = match self.items.get(j) {
                Some(it) => it,
                None => return Err(Pack67Code::ItemUnknown.val()),
            };
            if it.name == name || it.seat == seat {
                return Err(Pack67Code::ItemUnknown.val());
            }
            j += 1;
        }
        if hash == 0 {
            return Err(Pack67Code::HashMismatch.val());
        }
        self.items.push(PackItem {
            name: name.to_string(),
            seat,
            hash,
            version: FINAL_VERSION,
        });
        Ok(())
    }

    /// 逐件三断言终验：件在包内 + 哈希对平 + 版本终版。
    pub fn finalize(&mut self, current_hashes: &[u32]) -> Result<usize, u32> {
        if self.phase != PackPhase::Enrolling {
            return Err(Pack67Code::PhaseRegress.val());
        }
        if current_hashes.len() != PACK_COUNT {
            return Err(Pack67Code::WindowBad.val());
        }
        if self.items.len() != PACK_COUNT {
            return Err(Pack67Code::ItemMissing.val());
        }
        // 席位占位齐备：0..=4 逐席必有件。
        let mut seat_seen = [false; PACK_COUNT];
        let mut i = 0usize;
        while i < self.items.len() {
            let it = match self.items.get(i) {
                Some(it) => it,
                None => break,
            };
            if it.seat < PACK_COUNT {
                seat_seen[it.seat] = true;
            }
            i += 1;
        }
        let mut s = 0usize;
        while s < PACK_COUNT {
            let seen = match seat_seen.get(s) {
                Some(v) => *v,
                None => return Err(Pack67Code::SeatUnknown.val()),
            };
            if !seen {
                return Err(Pack67Code::ItemMissing.val());
            }
            s += 1;
        }
        // 哈希对平 + 版本终版：逐席比对。
        let mut passed = 0usize;
        let mut k = 0usize;
        while k < PACK_COUNT {
            let it = match self.items.get(k) {
                Some(it) => it,
                None => break,
            };
            let cur = match current_hashes.get(it.seat) {
                Some(v) => *v,
                None => return Err(Pack67Code::WindowBad.val()),
            };
            let frozen = match PACK_TABLE.get(k) {
                Some(t) => t.2,
                None => return Err(Pack67Code::ItemUnknown.val()),
            };
            if cur != frozen {
                return Err(Pack67Code::HashMismatch.val());
            }
            if it.version != FINAL_VERSION {
                return Err(Pack67Code::VersionNotFinal.val());
            }
            if it.hash != frozen {
                return Err(Pack67Code::HashMismatch.val());
            }
            passed += 1;
            k += 1;
        }
        self.phase = PackPhase::Passed;
        Ok(passed)
    }

    /// 包哈希落档：五件哈希序贯合成包指纹 + 7 行摘要 + 封账。
    pub fn seal_pack(&mut self) -> Result<u32, u32> {
        if self.phase != PackPhase::Passed {
            return Err(Pack67Code::PhaseSkip.val());
        }
        let mut digest: u32 = 0x811C_9DC5;
        let mut text = String::from("== UNX-F1567 移交包五件终验摘要 ==");
        text.push('\n');
        let mut i = 0usize;
        while i < self.items.len() {
            let it = match self.items.get(i) {
                Some(it) => it,
                None => break,
            };
            let line = format!("{} 席{} 终版一致 ●", it.name, it.seat);
            text.push_str(&line);
            text.push('\n');
            digest = digest.wrapping_mul(0x0100_0193).wrapping_add(it.hash);
            i += 1;
        }
        text.push_str("== 五件三断言全过 · 包哈希落档 ==");
        text.push('\n');
        let rows = text.matches('\n').count();
        if rows != PACK_ROWS {
            return Err(Pack67Code::PackRowsBad.val());
        }
        self.pack_digest = digest;
        self.summary = Some(text);
        self.phase = PackPhase::Sealed;
        Ok(digest)
    }

    /// LCG 抽验：抽 5 件复验版本终版位。
    pub fn sample_audit(&self) -> Result<usize, u32> {
        if self.items.len() != PACK_COUNT {
            return Err(Pack67Code::ItemMissing.val());
        }
        let mut lcg = Lcg::new(SEG_BASE);
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < PACK_COUNT {
            let idx = lcg.next_mod(PACK_COUNT);
            let it = match self.items.get(idx) {
                Some(it) => it,
                None => return Err(Pack67Code::ItemUnknown.val()),
            };
            if it.version != FINAL_VERSION {
                return Err(Pack67Code::VersionNotFinal.val());
            }
            ok += 1;
            k += 1;
        }
        Ok(ok)
    }
}

// ── 好包构建（五件全登 + 三断言终验 + 落档） ────────────────────
pub fn build_pack_ok() -> HandoverPack {
    let mut pk = HandoverPack::new();
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        let _ = pk.enroll(row.0, row.1, row.2);
        i += 1;
    }
    let hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    let _ = pk.finalize(&hashes);
    let _ = pk.seal_pack();
    pk
}

/// 缺件注入表：十路 (方式, 操作数)。方式 0=抽件（不登记该席），
/// 方式 1=清哈希（该席现版哈希清零致哈希失平）。
pub const MISS_ROUTES_TABLE: [(usize, usize); 10] = [
    (0, 0),
    (1, 0),
    (0, 1),
    (1, 1),
    (0, 2),
    (1, 2),
    (0, 3),
    (1, 3),
    (0, 4),
    (1, 4),
];

/// 缺件注入 10/10 检出复算：逐路必抓，返回检出路数。
fn miss_detect_count() -> usize {
    let mut detected = 0usize;
    let mut r = 0usize;
    while r < MISS_ROUTES_TABLE.len() {
        let route = match MISS_ROUTES_TABLE.get(r) {
            Some(t) => *t,
            None => break,
        };
        let mut pk = HandoverPack::new();
        let mut hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
        let mut i = 0usize;
        while i < PACK_TABLE.len() {
            let row = match PACK_TABLE.get(i) {
                Some(t) => *t,
                None => break,
            };
            if route.0 == 0 && row.1 == route.1 {
                // 抽件：该席不登记。
            } else {
                let _ = pk.enroll(row.0, row.1, row.2);
            }
            i += 1;
        }
        if route.0 == 1 {
            let slot = match hashes.get_mut(route.1) {
                Some(v) => v,
                None => break,
            };
            *slot = 0;
        }
        match pk.finalize(&hashes) {
            Ok(_) => {
                // 未检出：缺件漏网。
            }
            Err(_) => detected += 1,
        }
        r += 1;
    }
    detected
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78C00 && SEG_BASE <= 0x78DC0
}

/// 五件冻结表齐备：件名非空、席位在窗、哈希非零。
fn pack_table_ok() -> bool {
    if PACK_TABLE.len() != PACK_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.0.is_empty() || row.1 >= PACK_COUNT || row.2 == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 五件三断言全过：好包五件登记、终验 5/5、封账。
fn three_asserts_ok() -> bool {
    let pk = build_pack_ok();
    if pk.item_count() != PACK_COUNT {
        return false;
    }
    if pk.phase() != PackPhase::Sealed {
        return false;
    }
    if pk.pack_digest() == 0 {
        return false;
    }
    true
}

/// 包哈希落档：摘要 7 行、五件名齐、指纹非零。
fn pack_digest_ok() -> bool {
    let pk = build_pack_ok();
    let text = match pk.summary_text() {
        Some(t) => t,
        None => return false,
    };
    if text.matches('\n').count() != PACK_ROWS {
        return false;
    }
    if !text.contains("终验摘要") {
        return false;
    }
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if !text.contains(row.0) {
            return false;
        }
        i += 1;
    }
    if pk.pack_digest() == 0 {
        return false;
    }
    true
}

/// 缺件注入 10/10 检出：十路逐路必抓且检出计数恰 10。
fn miss_10_ok() -> bool {
    let detected = miss_detect_count();
    if detected != MISS_ROUTES {
        return false;
    }
    // 抽一路复验：直接路径报 ItemMissing。
    let route = match MISS_ROUTES_TABLE.get(0) {
        Some(t) => *t,
        None => return false,
    };
    let mut pk = HandoverPack::new();
    let hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(t) => *t,
            None => break,
        };
        if !(route.0 == 0 && row.1 == route.1) {
            let _ = pk.enroll(row.0, row.1, row.2);
        }
        i += 1;
    }
    match pk.finalize(&hashes) {
        Err(code) => {
            if code != Pack67Code::ItemMissing.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 哈希失平注入：首件现版哈希改值必抓（HashMismatch）。
fn inject_hash_ok() -> bool {
    let mut pk = HandoverPack::new();
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(t) => *t,
            None => break,
        };
        let _ = pk.enroll(row.0, row.1, row.2);
        i += 1;
    }
    let mut hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    let slot = match hashes.get_mut(0) {
        Some(v) => v,
        None => return false,
    };
    *slot = 0x9999_9999;
    match pk.finalize(&hashes) {
        Err(code) => {
            if code != Pack67Code::HashMismatch.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 版本注入：版本号漂移必抓（VersionNotFinal）。
fn inject_version_ok() -> bool {
    let mut pk = HandoverPack::new();
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(t) => *t,
            None => break,
        };
        let _ = pk.enroll(row.0, row.1, row.2);
        i += 1;
    }
    // 直接改一件版本号模拟版本漂移。
    let item = match pk.items.get_mut(2) {
        Some(it) => it,
        None => return false,
    };
    item.version = 0x0000_9999;
    let hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    match pk.finalize(&hashes) {
        Err(code) => {
            if code != Pack67Code::VersionNotFinal.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 窗长注入：4 件 / 6 件哈希窗必拒（WindowBad）。
fn inject_window_ok() -> bool {
    let mut fresh = HandoverPack::new();
    let mut i = 0usize;
    while i < PACK_TABLE.len() {
        let row = match PACK_TABLE.get(i) {
            Some(t) => *t,
            None => break,
        };
        let _ = fresh.enroll(row.0, row.1, row.2);
        i += 1;
    }
    let mut short: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    let _ = short.pop();
    match fresh.finalize(&short) {
        Err(code) => {
            if code != Pack67Code::WindowBad.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let mut long: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    long.push(0);
    match fresh.finalize(&long) {
        Err(code) => {
            if code != Pack67Code::WindowBad.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 登记闸门：越闭集件名 / 越界席位 / 重登 / 零哈希四路必抓。
fn enroll_guard_ok() -> bool {
    let mut pk = HandoverPack::new();
    // 越闭集件名。
    match pk.enroll("黑户册", 0, 0x1) {
        Err(code) => {
            if code != Pack67Code::ItemUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 越界席位。
    match pk.enroll("接口册", 9, 0x11A0_0001) {
        Err(code) => {
            if code != Pack67Code::SeatUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 零哈希。
    match pk.enroll("接口册", 0, 0) {
        Err(code) => {
            if code != Pack67Code::HashMismatch.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 重登（同名或同席）。
    if pk.enroll("接口册", 0, 0x11A0_0001).is_err() {
        return false;
    }
    match pk.enroll("接口册", 0, 0x11A0_0001) {
        Err(code) => {
            if code != Pack67Code::ItemUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if pk.item_count() != 1 {
        return false;
    }
    true
}

/// 状态机单调：未终验落档拒、封账后再登记拒。
fn phase_monotonic_ok() -> bool {
    let mut pk = HandoverPack::new();
    // 未终验先落档：跳级拒。
    match pk.seal_pack() {
        Err(code) => {
            if code != Pack67Code::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 好包封账后再登记：倒退拒。
    let mut sealed = build_pack_ok();
    match sealed.enroll("接口册", 0, 0x11A0_0001) {
        Err(code) => {
            if code != Pack67Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 封账后再终验：倒退拒。
    let hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
    match sealed.finalize(&hashes) {
        Err(code) => {
            if code != Pack67Code::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// LCG 抽验：好包抽 5 件全过，同种子序列逐位确定。
fn sample_audit_ok() -> bool {
    let pk = build_pack_ok();
    let n = match pk.sample_audit() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if n != PACK_COUNT {
        return false;
    }
    let mut a = Lcg::new(SEG_BASE);
    let mut b = Lcg::new(SEG_BASE);
    let mut i = 0usize;
    while i < PACK_COUNT {
        if a.next_mod(PACK_COUNT) != b.next_mod(PACK_COUNT) {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致：好包指纹与摘要两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_pack_ok().pack_digest();
    let d2 = build_pack_ok().pack_digest();
    if d1 != d2 || d1 == 0 {
        return false;
    }
    let t1 = match build_pack_ok().summary_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    let t2 = match build_pack_ok().summary_text() {
        Some(t) => t.to_string(),
        None => return false,
    };
    if t1 != t2 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Pack67Code::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match Pack67Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1567_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1567 移交包五件终验");
    s.add_dyn(
        "段位守卫",
        seg_guard_ok(),
        &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落本批段窗 0x78C00..=0x78DC0", SEG_BASE),
    );
    s.add_dyn("五件冻结表齐备", pack_table_ok(), "五件逐件登记席位与冻结哈希");
    s.add_dyn("逐件三断言", three_asserts_ok(), "件在包内+哈希对平+版本终版全过");
    s.add_dyn("包哈希落档", pack_digest_ok(), "7 行摘要五件名齐指纹落账");
    s.add_dyn("缺件注入10/10", miss_10_ok(), "十路缺件逐路必抓检出恰 10");
    s.add_dyn("哈希失平注入", inject_hash_ok(), "首件哈希改值必抓 HashMismatch");
    s.add_dyn("版本漂移注入", inject_version_ok(), "版本号漂移必抓 VersionNotFinal");
    s.add_dyn("窗长注入必抓", inject_window_ok(), "4/6 件窗长必拒 WindowBad");
    s.add_dyn("登记闸门", enroll_guard_ok(), "越闭集/越席/零哈希/重登四路必抓");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→终验→落档可达，倒退拒");
    s.add_dyn("LCG抽验5件", sample_audit_ok(), "好包抽 5 件全过序列确定");
    s.add_dyn("双跑一致", double_run_ok(), "指纹与摘要两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78CC0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1567_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn pack_table_shape() {
        assert_eq!(PACK_TABLE.len(), 5);
        assert_eq!(PACK_ROWS, 7);
        assert_eq!(FINAL_VERSION, 0x0001_1040);
        let mut i = 0usize;
        while i < PACK_TABLE.len() {
            let row = match PACK_TABLE.get(i) {
                Some(r) => *r,
                None => break,
            };
            assert_eq!(row.1, i, "席位必须与表序对齐");
            i += 1;
        }
    }

    #[test]
    fn full_path_seals() {
        let pk = build_pack_ok();
        assert_eq!(pk.item_count(), 5);
        assert_eq!(pk.phase(), PackPhase::Sealed);
        assert!(pk.summary_text().is_some());
        assert_ne!(pk.pack_digest(), 0);
    }

    #[test]
    fn missing_item_rejected() {
        let mut pk = HandoverPack::new();
        let hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
        match pk.finalize(&hashes) {
            Err(code) => assert_eq!(code, Pack67Code::ItemMissing.val()),
            Ok(_) => assert!(false, "零件终验必须被拒"),
        }
    }

    #[test]
    fn unknown_item_rejected() {
        let mut pk = HandoverPack::new();
        match pk.enroll("黑户册", 0, 0x11A0_0001) {
            Err(code) => assert_eq!(code, Pack67Code::ItemUnknown.val()),
            Ok(_) => assert!(false, "越闭集件名必须被拒"),
        }
    }

    #[test]
    fn duplicate_seat_rejected() {
        let mut pk = HandoverPack::new();
        assert!(pk.enroll("接口册", 0, 0x11A0_0001).is_ok());
        match pk.enroll("账本册", 0, 0x11A0_0002) {
            Err(code) => assert_eq!(code, Pack67Code::ItemUnknown.val()),
            Ok(_) => assert!(false, "同席重登必须被拒"),
        }
        assert_eq!(pk.item_count(), 1);
    }

    #[test]
    fn hash_mismatch_detected() {
        let mut pk = HandoverPack::new();
        let mut i = 0usize;
        while i < PACK_TABLE.len() {
            let row = match PACK_TABLE.get(i) {
                Some(t) => *t,
                None => break,
            };
            let _ = pk.enroll(row.0, row.1, row.2);
            i += 1;
        }
        let mut hashes: Vec<u32> = PACK_TABLE.iter().map(|t| t.2).collect();
        let slot = match hashes.get_mut(4) {
            Some(v) => v,
            None => return,
        };
        *slot = 0x1234_5678;
        match pk.finalize(&hashes) {
            Err(code) => assert_eq!(code, Pack67Code::HashMismatch.val()),
            Ok(_) => assert!(false, "哈希失平必须被拒"),
        }
    }

    #[test]
    fn miss_injection_ten_detected() {
        assert_eq!(miss_detect_count(), 10);
        assert_eq!(MISS_ROUTES_TABLE.len(), 10);
    }

    #[test]
    fn seal_before_finalize_rejected() {
        let mut pk = HandoverPack::new();
        match pk.seal_pack() {
            Err(code) => assert_eq!(code, Pack67Code::PhaseSkip.val()),
            Ok(_) => assert!(false, "未终验落档必须被拒"),
        }
    }

    #[test]
    fn sealed_rejects_reenroll() {
        let mut pk = build_pack_ok();
        match pk.enroll("接口册", 0, 0x11A0_0001) {
            Err(code) => assert_eq!(code, Pack67Code::PhaseRegress.val()),
            Ok(_) => assert!(false, "封账后再登记必须被拒"),
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let d1 = build_pack_ok().pack_digest();
        let d2 = build_pack_ok().pack_digest();
        assert_eq!(d1, d2);
        assert_ne!(d1, 0);
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE);
        let mut b = Lcg::new(SEG_BASE);
        assert_eq!(a.next_mod(5), b.next_mod(5));
        assert_eq!(a.next_mod(5), b.next_mod(5));
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Pack67Code::ALL.len(), 12);
        let first = match Pack67Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Pack67Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }
}
