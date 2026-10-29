#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0797 · A1 域封域宣告（封域哈希 + 只读守卫挂载）
//! 域/批：CoRun·UNX A1 收官域（封域面 · W7 清单第 24 单）
//! 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥570 行；② 判据原文：UNX-F0797-J1 封域宣告过（全域账面封域
//!   哈希 + 只读守卫挂载声明）且封域后写尝试检出断言挂载在账；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 全域账面闭集：六张收官账（判据账/凭证账/交付账/复盘账/
//!   维护账/留痕账），每张账 = 账面名 + 账面哈希（FNV-1a-64）+ 定容行账。
//! ② 封域哈希：六账哈希按账面名规范序折叠为全域封域哈希（确定性，
//!   双跑一致）；任一账面变动 → 封域哈希变（可检）。
//! ③ 只读守卫挂载：封域宣告时挂载只读守卫于账；封域后任何写尝试
//!   （write 模拟）必须被检出——返回错误码并登记检出记录（检出账），
//!   检出断言挂载在账 = 检出账含逐次写尝试的账面定位。
//! ④ 状态机 Open→Hashed→Guarded→Sealed 单向闭集；守卫未挂载即写成功
//!   （合法写窗），挂载后写必红。
//! 诊断段声明：0x61E0–0x61FF 本单独占（清单 <SEG 0x61E0>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x61E0–0x61FF）
pub const SEG_BASE: u16 = 0x61E0; // 诊断码段基数，来源：W7 清单 UNX-F0797 行 <SEG 0x61E0>
pub const SEG_TOP: u16 = 0x61FF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 封域状态机非法边
pub const XC_DUP_BOOK: u16 = SEG_BASE + 2; // 账面名重复
pub const XC_BOOK_EMPTY: u16 = SEG_BASE + 3; // 账面名为空
pub const XC_BOOK_SHORT: u16 = SEG_BASE + 4; // 账面不足六张
pub const XC_ROW_EMPTY: u16 = SEG_BASE + 5; // 写入行文本为空
pub const XC_ROW_CAP: u16 = SEG_BASE + 6; // 账面行容量满
pub const XC_BOOK_UNKNOWN: u16 = SEG_BASE + 7; // 写目标账面不存在
pub const XC_NOT_HASHED: u16 = SEG_BASE + 8; // 未算封域哈希即挂守卫/封域
pub const XC_ALREADY_SEALED: u16 = SEG_BASE + 9; // 已封域再封域
pub const XC_WRITE_FROZEN: u16 = SEG_BASE + 10; // 封域后写尝试（守卫检出）
pub const XC_GUARD_DOUBLE: u16 = SEG_BASE + 11; // 守卫重复挂载
pub const XC_HASH_DIFF: u16 = SEG_BASE + 12; // 封域哈希双跑不一致
pub const XC_DET_MISSING: u16 = SEG_BASE + 13; // 检出断言未挂载在账
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 14; // 摘要为空
pub const XC_CONSERV_BREAK: u16 = SEG_BASE + 15; // 直方图守恒破

// 2. 封域常量（写死，来源=判据原文/本单口径头注假设）
pub const BOOK_N: usize = 6; // 全域六张收官账，来源：判据原文（全域账面）
pub const ROW_CAP: usize = 32; // 每账定容行数，头注假设

/// 六账名闭集（规范序 = 枚举序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookName {
    Criteria = 0, // 判据账
    Verdict = 1, // 凭证账
    Handover = 2, // 交付账
    Retro = 3, // 复盘账
    Maint = 4, // 维护账
    Trail = 5, // 留痕账
}
pub const BOOK_ALL: [BookName; 6] = [BookName::Criteria, BookName::Verdict, BookName::Handover, BookName::Retro, BookName::Maint, BookName::Trail];
impl BookName {
    pub fn view(self) -> &'static str {
        match self {
            BookName::Criteria => "criteria",
            BookName::Verdict => "verdict",
            BookName::Handover => "handover",
            BookName::Retro => "retro",
            BookName::Maint => "maint",
            BookName::Trail => "trail",
        }
    }
}

/// 一张账面。
#[derive(Clone, Debug)]
pub struct DomainBook {
    pub name: BookName, // 账面名（闭集，不重）
    pub rows: Vec<String>, // 定容行账（封域前可写）
    pub hash: u64, // 账面哈希（行规范序折叠）
}
impl DomainBook {
    pub fn new(n: BookName) -> Self {
        DomainBook { name: n, rows: Vec::new(), hash: 0 }
    }
    pub fn rehash(&mut self) {  // 账面哈希重算：逐行 FNV 折叠。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.rows.len() {
            let bs = self.rows[i].as_bytes();
            let mut j = 0usize;
            while j < bs.len() {
                h ^= bs[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
                j += 1;
            }
            h = fnv1a(h, self.rows[i].len() as u64);
            i += 1;
        }
        self.hash = h;
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步（字节混合）。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}

/// 一次写尝试检出记录。
#[derive(Clone, Debug)]
pub struct WriteCatch {
    pub book: BookName, // 写目标账面
    pub len: u32, // 尝试写入文本长度
    pub code: u16, // 检出码（恒为 XC_WRITE_FROZEN）
}

/// 封域回执。
#[derive(Clone, Debug)]
pub struct SealAttest {
    pub books: u32, // 账面数（6）
    pub rows: u32, // 六账总行数
    pub domain_hash: u64, // 全域封域哈希
    pub catches: u32, // 封域后写尝试检出数
    pub summary: String, // 摘要（非空）
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealStage {
    Open = 0,
    Hashed = 1,
    Guarded = 2,
    Sealed = 3,
}
impl SealStage {
    pub fn can_step(self, next: SealStage) -> bool {
        (self as u8) + 1 == next as u8
    }
    pub fn view(self) -> &'static str {
        match self {
            SealStage::Open => "open",
            SealStage::Hashed => "hashed",
            SealStage::Guarded => "guarded",
            SealStage::Sealed => "sealed",
        }
    }
}
pub struct SealGuard {  // A1 域封域守卫机。
    stage: SealStage,
    books: Vec<DomainBook>,
    catches: Vec<WriteCatch>,
    pinned: Option<SealAttest>,
    events: Vec<String>,
}
impl SealGuard {
    pub fn new() -> Self {
        SealGuard { stage: SealStage::Open, books: Vec::new(), catches: Vec::new(), pinned: None, events: Vec::new() }
    }
    pub fn stage(&self) -> SealStage {
        self.stage
    }
    pub fn book_len(&self) -> usize {
        self.books.len()
    }
    pub fn total_rows(&self) -> usize {  // 六账总行数（守恒基线）。
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.books.len() {
            n += self.books[i].rows.len();
            i += 1;
        }
        n
    }
    pub fn catch_len(&self) -> usize {
        self.catches.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn open_book(&mut self, n: BookName) -> Result<u32, u16> {  // 开账：名闭集不重。
        if self.stage != SealStage::Open {
            return Err(XC_STATE_INVALID);
        }
        let mut i = 0usize;
        while i < self.books.len() {
            if self.books[i].name == n {
                return Err(XC_DUP_BOOK);
            }
            i += 1;
        }
        self.books.push(DomainBook::new(n));
        self.events.push(alloc::format!("book +{}", self.books.len()));
        Ok(self.books.len() as u32)
    }
    pub fn write_row(&mut self, n: BookName, row: &str) -> Result<u32, u16> {  // 写账：守卫未挂载才可写；行非空、容量守卫；写后重算账面哈希。
        if self.stage == SealStage::Guarded || self.stage == SealStage::Sealed {
            self.catches.push(WriteCatch { book: n, len: row.len() as u32, code: XC_WRITE_FROZEN });
            return Err(XC_WRITE_FROZEN); // 封域后写尝试检出
        }
        if row.is_empty() {
            return Err(XC_ROW_EMPTY);
        }
        let mut i = 0usize;
        while i < self.books.len() {
            if self.books[i].name == n {
                if self.books[i].rows.len() >= ROW_CAP {
                    return Err(XC_ROW_CAP);
                }
                self.books[i].rows.push(String::from(row));
                self.books[i].rehash();
                self.events.push(alloc::format!("write {}", self.books[i].rows.len()));
                return Ok(self.books[i].rows.len() as u32);
            }
            i += 1;
        }
        Err(XC_BOOK_UNKNOWN)
    }
    pub fn domain_hash(&mut self) -> Result<u64, u16> {  // 全域封域哈希：六账哈希按账名规范序折叠（确定性）。
        if self.stage != SealStage::Open && self.stage != SealStage::Hashed {
            return Err(XC_STATE_INVALID);
        }
        if self.books.len() != BOOK_N {
            return Err(XC_BOOK_SHORT);
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut k = 0usize;
        while k < BOOK_N {
            let want = BOOK_ALL[k];
            let mut hit: Option<u64> = None;
            let mut i = 0usize;
            while i < self.books.len() {
                if self.books[i].name == want {
                    hit = Some(self.books[i].hash);
                    break;
                }
                i += 1;
            }
            h = fnv1a(h, want as u64);
            h = fnv1a(h, match hit {
                Some(v) => v,
                None => return Err(XC_BOOK_SHORT),
            });
            k += 1;
        }
        if self.stage == SealStage::Open {
            self.stage = SealStage::Hashed;
            self.events.push(String::from("hashed"));
        }
        Ok(h)
    }
    pub fn mount_guard(&mut self) -> Result<u32, u16> {  // 只读守卫挂载：Hashed→Guarded，重挂红。
        if self.stage == SealStage::Guarded || self.stage == SealStage::Sealed {
            return Err(XC_GUARD_DOUBLE);
        }
        if !self.stage.can_step(SealStage::Guarded) {
            return Err(XC_NOT_HASHED);
        }
        self.stage = SealStage::Guarded;
        self.events.push(String::from("guard-on"));
        Ok(1)
    }
    pub fn seal(&mut self) -> Result<SealAttest, u16> {  // 封域宣告：Guarded→Sealed，封域哈希定格 + 摘要（重封红）。
        if self.stage == SealStage::Sealed {
            return Err(XC_ALREADY_SEALED);
        }
        if !self.stage.can_step(SealStage::Sealed) {
            return Err(XC_STATE_INVALID);
        }
        if self.books.len() != BOOK_N {
            return Err(XC_BOOK_SHORT);
        }
        let dh = self.domain_hash()?;
        let summary = alloc::format!(
            "UNX-F0797-J1 封域宣告过：全域六账封域哈希定格（总行 {}），只读守卫已挂载，封域后写尝试检出 {} 次且断言挂载在账",
            self.total_rows(), self.catches.len()
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = SealAttest {
            books: self.books.len() as u32,
            rows: self.total_rows() as u32,
            domain_hash: dh,
            catches: self.catches.len() as u32,
            summary,
        };
        self.pinned = Some(rc.clone());
        self.stage = SealStage::Sealed;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&SealAttest> {
        self.pinned.as_ref()
    }
    pub fn catch_of(&self, k: usize) -> Option<&WriteCatch> {  // 检出账逐条查询（检出断言挂载在账）。
        if k >= self.catches.len() {
            return None;
        }
        Some(&self.catches[k])
    }
    pub fn catch_by_book(&self, n: BookName) -> u32 {  // 按账面统计检出数。
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.catches.len() {
            if self.catches[i].book == n {
                c += 1;
            }
            i += 1;
        }
        c
    }
    pub fn book_of(&self, n: BookName) -> Option<&DomainBook> {  // 账面查询。
        let mut i = 0usize;
        while i < self.books.len() {
            if self.books[i].name == n {
                return Some(&self.books[i]);
            }
            i += 1;
        }
        None
    }
    pub fn book_hash_view(&self) -> String {  // 六账哈希视图串（"name:hash" 行串，规范序）。
        let mut s = String::new();
        let mut k = 0usize;
        while k < BOOK_N {
            let want = BOOK_ALL[k];
            let mut i = 0usize;
            while i < self.books.len() {
                if self.books[i].name == want {
                    s.push_str(alloc::format!("{}:{:016x}\n", want.view(), self.books[i].hash).as_str());
                    break;
                }
                i += 1;
            }
            k += 1;
        }
        s
    }
    pub fn row_view(&self, n: BookName) -> Option<String> {  // 账面行视图串（逐行带序号，未开账 None）。
        let mut i = 0usize;
        while i < self.books.len() {
            if self.books[i].name == n {
                let mut s = String::new();
                let mut r = 0usize;
                while r < self.books[i].rows.len() {
                    s.push_str(alloc::format!("{}|{}\n", r + 1, self.books[i].rows[r]).as_str());
                    r += 1;
                }
                return Some(s);
            }
            i += 1;
        }
        None
    }
    pub fn catch_table_view(&self) -> String {  // 检出账表串："k|book|len|code" 行串（断言在账凭据）。
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.catches.len() {
            let c = &self.catches[i];
            s.push_str(alloc::format!("{}|{}|{}|{:04x}\n", i + 1, c.book.view(), c.len, c.code).as_str());
            i += 1;
        }
        s
    }
    pub fn row_count_histogram(&self) -> [u32; 6] {  // 六账行数直方图（规范序，守恒=sum=total_rows）。
        let mut h = [0u32; 6];
        let mut k = 0usize;
        while k < BOOK_N {
            let want = BOOK_ALL[k];
            let mut i = 0usize;
            while i < self.books.len() {
                if self.books[i].name == want {
                    h[k] = self.books[i].rows.len() as u32;
                    break;
                }
                i += 1;
            }
            k += 1;
        }
        h
    }
    pub fn guard_status_view(&self) -> String {  // 守卫状态视图串（"stage=…,guard=…,catches=…"）。
        let on = match self.stage {
            SealStage::Guarded | SealStage::Sealed => "on",
            _ => "off",
        };
        alloc::format!("stage={},guard={},catches={}", self.stage.view(), on, self.catches.len())
    }
    pub fn write_allowed(&self, n: BookName) -> Option<bool> {  // 写窗查询：账已开且守卫未挂载为 true；账未开 None；冻结 false。
        if self.stage == SealStage::Guarded || self.stage == SealStage::Sealed {
            return Some(false);
        }
        let mut i = 0usize;
        while i < self.books.len() {
            if self.books[i].name == n {
                return Some(true);
            }
            i += 1;
        }
        None
    }
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    // —— 主路径：开六账 → 合法写窗 → 封域哈希 → 挂守卫 → 写尝试检出 → 封域 ——
    let mut m = SealGuard::new();
    let mut k = 0usize;
    while k < BOOK_N {
        m.open_book(BOOK_ALL[k])?;
        k += 1;
    }
    if m.book_len() != BOOK_N {
        return Err(XC_BOOK_SHORT);
    }
    let mut i = 0usize;
    while i < BOOK_N {
        let b = BOOK_ALL[i];
        let mut r = 0usize;
        while r < 2 {
            m.write_row(b, &alloc::format!("{:?}_行{}_收官定格", b, r + 1))?;
            r += 1;
        }
        i += 1;
    }
    if m.total_rows() != 12 {
        return Err(XC_BOOK_SHORT);
    }
    let dh1 = m.domain_hash()?;
    // 双跑对拍：哈希重算一致（确定性）。
    let dh2 = m.domain_hash()?;
    if dh1 != dh2 {
        return Err(XC_HASH_DIFF);
    }
    // 账面变动 → 封域哈希变（可检）。
    m.write_row(BookName::Trail, "补录_终行")?;
    let dh3 = m.domain_hash()?;
    if dh3 == dh1 {
        return Err(XC_HASH_DIFF);
    }
    m.mount_guard()?;
    if !matches!(m.mount_guard(), Err(XC_GUARD_DOUBLE)) {
        return Err(XC_GUARD_DOUBLE);
    }
    // —— 封域后写尝试检出断言挂载在账 ——
    if m.write_row(BookName::Criteria, "越权写") != Err(XC_WRITE_FROZEN) {
        return Err(XC_WRITE_FROZEN);
    }
    if m.write_row(BookName::Maint, "越权写二") != Err(XC_WRITE_FROZEN) {
        return Err(XC_WRITE_FROZEN);
    }
    if m.catch_len() != 2 {
        return Err(XC_DET_MISSING);
    }
    let c0 = match m.catch_of(0) {
        Some(c) if c.code == XC_WRITE_FROZEN && c.book == BookName::Criteria => c.len,
        _ => return Err(XC_DET_MISSING),
    };
    if c0 != "越权写".len() as u32 {
        return Err(XC_DET_MISSING);
    }
    if m.catch_by_book(BookName::Maint) != 1 || m.catch_by_book(BookName::Retro) != 0 {
        return Err(XC_DET_MISSING);
    }
    let rc = m.seal()?;
    if rc.books != 6 || rc.rows != 13 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.seal(), Err(XC_ALREADY_SEALED)) {
        return Err(XC_STATE_INVALID);
    }
    if m.stage().view() != "sealed" {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：账面重名 / 未挂守卫即封域 / 未哈希即挂守卫 ——
    let mut m2 = SealGuard::new();
    m2.open_book(BookName::Criteria)?;
    if !matches!(m2.open_book(BookName::Criteria), Err(XC_DUP_BOOK)) {
        return Err(XC_DUP_BOOK);
    }
    if !matches!(m2.mount_guard(), Err(XC_NOT_HASHED)) {
        return Err(XC_NOT_HASHED);
    }
    if !matches!(m2.seal(), Err(XC_STATE_INVALID)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 2：账不足六张 / 写空行 / 写不存在的账 ——
    let mut m3 = SealGuard::new();
    k = 0;
    while k < BOOK_N - 1 {
        m3.open_book(BOOK_ALL[k])?;
        k += 1;
    }
    if !matches!(m3.domain_hash(), Err(XC_BOOK_SHORT)) {
        return Err(XC_BOOK_SHORT);
    }
    let mut m4 = SealGuard::new();
    k = 0;
    while k < BOOK_N {
        m4.open_book(BOOK_ALL[k])?;
        k += 1;
    }
    if m4.write_row(BookName::Retro, "") != Err(XC_ROW_EMPTY) {
        return Err(XC_ROW_EMPTY);
    }
    if m4.write_row(BookName::Verdict, "落入无主账") != Err(XC_BOOK_UNKNOWN) {
        return Err(XC_BOOK_UNKNOWN);
    }
    // 容量守卫：单账写满 32 行后第 33 行红。
    let mut r = 0usize;
    while r < ROW_CAP {
        m4.write_row(BookName::Retro, &alloc::format!("占位行{}", r))?;
        r += 1;
    }
    if m4.write_row(BookName::Retro, "超容行") != Err(XC_ROW_CAP) {
        return Err(XC_ROW_CAP);
    }
    // —— 查询 API：账面/哈希视图 ——
    match m.book_of(BookName::Handover) {
        Some(b) if b.rows.len() == 2 => {}
        _ => return Err(XC_BOOK_SHORT),
    }
    let hv = m.book_hash_view();
    if !hv.contains("criteria:") || !hv.contains("trail:") {
        return Err(XC_SUMMARY_EMPTY);
    }
    match m.pinned_of() {
        Some(a) if a.domain_hash != 0 => {}
        _ => return Err(XC_NOT_HASHED),
    }
    // —— 扩展互证：视图/查询 API 与核心账对拍 ——
    let rh = m.row_count_histogram();
    let mut sum = 0u32;
    let mut k2 = 0usize;
    while k2 < 6 {
        sum += rh[k2];
        k2 += 1;
    }
    if sum != m.total_rows() as u32 || sum != 13 {
        return Err(XC_CONSERV_BREAK); // 直方图守恒破
    }
    let rv = match m.row_view(BookName::Trail) {
        Some(v) => v,
        None => return Err(XC_BOOK_SHORT),
    };
    if !rv.contains("1|") || !rv.contains("终行") {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.row_view(BookName::Criteria).is_none() {
        return Err(XC_BOOK_SHORT);
    }
    let ctv = m.catch_table_view();
    if !ctv.contains("1|criteria|") || !ctv.contains("61ea") {
        return Err(XC_DET_MISSING); // 检出断言在账凭据缺失（0x61EA=XC_WRITE_FROZEN）
    }
    let gv = m.guard_status_view();
    if !gv.starts_with("stage=sealed") || !gv.contains("guard=on") {
        return Err(XC_STATE_INVALID);
    }
    // 写窗查询：封域后恒 false；未开账 None；未封域前 true。
    match m.write_allowed(BookName::Retro) {
        Some(false) => {}
        _ => return Err(XC_WRITE_FROZEN),
    }
    let mut m5 = SealGuard::new();
    if m5.write_allowed(BookName::Retro).is_some() {
        return Err(XC_BOOK_UNKNOWN); // 账未开不该给出写窗结论
    }
    m5.open_book(BookName::Retro)?;
    match m5.write_allowed(BookName::Retro) {
        Some(true) => {}
        _ => return Err(XC_STATE_INVALID),
    }
    if !matches!(m5.mount_guard(), Err(XC_NOT_HASHED)) {
        return Err(XC_NOT_HASHED); // Hashed 前挂守卫应红
    }
    m5.domain_hash()?;
    m5.mount_guard()?;
    match m5.write_allowed(BookName::Retro) {
        Some(false) => {}
        _ => return Err(XC_WRITE_FROZEN),
    }
    if m5.write_row(BookName::Retro, "冻结后写") != Err(XC_WRITE_FROZEN) {
        return Err(XC_WRITE_FROZEN);
    }
    if m5.catch_len() != 1 {
        return Err(XC_DET_MISSING);
    }
    Ok(0)
}
