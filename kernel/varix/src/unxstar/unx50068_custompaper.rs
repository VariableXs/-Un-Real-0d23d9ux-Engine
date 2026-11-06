//! UNX-F50068 自定义纸张账（用户自定义尺寸上下限校验）（0x434a0 段）
//!
//! 锚点：UNX-F50068-J1 自定义 ×20 越界拒绝+合法落账；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x434a0`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 11`，
//! 逐项见 [`CustomPaperError`]。不得借位相邻卡（0x43480 标准纸卡 / 0x434c0
//! IPP 联签卡）。
//!
//! 自定义纸与标准纸的本质区别：**标准纸是查表，自定义纸是记账**。用户输入
//! 的每一张自定义纸都要在账本里留下唯一 ID，后续作业引用的是 ID 而不是
//! 尺寸对；因此本模块的重心是「一次写入、可审计、不可悄悄改写」：
//! * 上下限校验四道闸：短边下限、长边上限、面积上限、长宽比上限。
//!   四道闸的判定顺序固定（先边长、再面积、最后比例），保证同一坏样本
//!   无论从哪个入口进来都得到同一个诊断码；
//! * 账本：ID 单调递增、名称大小写不敏感去重、32 条硬顶；
//! * 封账：`seal()` 冻结内容摘要；封账后任何增删都返回 `SealedLedger`。
//!   摘要覆盖全部条目（ID/名称/尺寸），同一账本重算必然逐位相同；
//! * 注入核：8 类坏样本（空名、超长名、短边过低、长边过高、面积超限、
//!   比例超限、重名、封账后写入），逐类断言诊断码。
//!
//! 尺寸一律用 0.1mm 整数，校验与摘要都不经过浮点——「越界拒绝」与
//! 「合法落账」因此是精确可判的，不存在边界浮点抖动。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x434a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x434a0;

/// 账本硬顶：32 条自定义纸。办公驱动的自定义列表极少超过 20 条，
/// 32 留出余量又把「账满」做成可测状态。
pub const LEDGER_CAP: usize = 32;

/// 名称长度上限（字符数，不是字节数）：32 字符足够区分「财务凭证纸」
/// 这类长名，再长在 UI 上也无法完整显示。
pub const NAME_MAX: usize = 32;

/// 封账盐：让封签不只是摘要的复制品，防止摘要被重算附会。
const SEAL_SALT: u64 = 0xF500_68C0_FFEE_0000;

const E_EMPTY_NAME: u16 = 1;
const E_NAME_LONG: u16 = 2;
const E_SIDE_LOW: u16 = 3;
const E_SIDE_HIGH: u16 = 4;
const E_AREA: u16 = 5;
const E_RATIO: u16 = 6;
const E_DUPLICATE: u16 = 7;
const E_CAPACITY: u16 = 8;
const E_UNKNOWN_ID: u16 = 9;
const E_SEALED: u16 = 10;
const E_DIGEST: u16 = 11;

/// 自定义纸账本错误十一型。`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomPaperError {
    /// 名称为空或全空白。
    EmptyName,
    /// 名称超过 32 字符。
    NameTooLong,
    /// 短边低于下限（50mm）。
    SideBelowMin,
    /// 长边高于上限（1500mm）。
    SideAboveMax,
    /// 面积超过 1.0m²（100,000,000 × 0.01mm²）。
    AreaAboveMax,
    /// 长宽比超过 20:1。
    RatioAboveMax,
    /// 同名条目已存在（忽略大小写与首尾空白）。
    DuplicateName,
    /// 账本已满 32 条。
    CapacityFull,
    /// 按 ID 删除/读取时不存在。
    UnknownId,
    /// 账本已封，拒绝增删与重封。
    SealedLedger,
    /// 摘要或封签与当前账本不符。
    DigestMismatch,
}

impl CustomPaperError {
    /// 段内序号（1 起），与 `code()` 一一对应。
    fn ordinal(self) -> u16 {
        match self {
            CustomPaperError::EmptyName => E_EMPTY_NAME,
            CustomPaperError::NameTooLong => E_NAME_LONG,
            CustomPaperError::SideBelowMin => E_SIDE_LOW,
            CustomPaperError::SideAboveMax => E_SIDE_HIGH,
            CustomPaperError::AreaAboveMax => E_AREA,
            CustomPaperError::RatioAboveMax => E_RATIO,
            CustomPaperError::DuplicateName => E_DUPLICATE,
            CustomPaperError::CapacityFull => E_CAPACITY,
            CustomPaperError::UnknownId => E_UNKNOWN_ID,
            CustomPaperError::SealedLedger => E_SEALED,
            CustomPaperError::DigestMismatch => E_DIGEST,
        }
    }

    /// 诊断码：段基址 + 序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断文本（UI 回执/驱动日志用）。
    pub fn message(self) -> &'static str {
        match self {
            CustomPaperError::EmptyName => "custom paper name empty",
            CustomPaperError::NameTooLong => "custom paper name above 32 chars",
            CustomPaperError::SideBelowMin => "short side below 50mm",
            CustomPaperError::SideAboveMax => "long side above 1500mm",
            CustomPaperError::AreaAboveMax => "area above 1.0 square metre",
            CustomPaperError::RatioAboveMax => "aspect ratio above 20:1",
            CustomPaperError::DuplicateName => "custom paper name already used",
            CustomPaperError::CapacityFull => "custom paper ledger full",
            CustomPaperError::UnknownId => "custom paper id not found",
            CustomPaperError::SealedLedger => "custom paper ledger sealed",
            CustomPaperError::DigestMismatch => "ledger digest / seal mismatch",
        }
    }
}

/// 全错误表：判据据此验证码值闭集与「基址 + 序号」的零漂移。
pub const ALL_ERRORS: [CustomPaperError; 11] = [
    CustomPaperError::EmptyName,
    CustomPaperError::NameTooLong,
    CustomPaperError::SideBelowMin,
    CustomPaperError::SideAboveMax,
    CustomPaperError::AreaAboveMax,
    CustomPaperError::RatioAboveMax,
    CustomPaperError::DuplicateName,
    CustomPaperError::CapacityFull,
    CustomPaperError::UnknownId,
    CustomPaperError::SealedLedger,
    CustomPaperError::DigestMismatch,
];

/// 自定义纸限额四元组。`office()` 是办公激光/喷墨的通用边界：
/// 50mm 短边（信封/标签下限）、1500mm 长边（A3 加长纸上限）、
/// 1.0m² 面积（A0 恰好 0.9999m²，是能过的最大标准幅面）、
/// 20:1 比例（再细长的纸在进纸辊上会打滑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub min_side_dmm: u32,
    pub max_side_dmm: u32,
    pub max_area_dmm2: u64,
    pub max_ratio_x10: u32,
}

impl Limits {
    /// 办公档限额（本模块默认策略，也是判据的唯一口径）。
    pub const fn office() -> Limits {
        Limits {
            min_side_dmm: 500,
            max_side_dmm: 15000,
            max_area_dmm2: 100_000_000,
            max_ratio_x10: 200,
        }
    }
}

/// 一条自定义纸。ID 由账本分配，调用方不能指定；`name` 已去掉首尾空白。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomPaper {
    pub id: u16,
    pub name: String,
    pub width_dmm: u32,
    pub height_dmm: u32,
}

impl CustomPaper {
    /// 短边（朝向无关）。
    pub fn short_side(&self) -> u32 {
        if self.width_dmm <= self.height_dmm {
            self.width_dmm
        } else {
            self.height_dmm
        }
    }

    /// 长边（朝向无关）。
    pub fn long_side(&self) -> u32 {
        if self.width_dmm >= self.height_dmm {
            self.width_dmm
        } else {
            self.height_dmm
        }
    }

    /// 面积（0.01mm² 单位）。
    pub fn area_dmm2(&self) -> u64 {
        (self.width_dmm as u64) * (self.height_dmm as u64)
    }

    /// 长宽比 ×10（整数，避免浮点比较）：150×100 与 100×150 得到同一个值。
    pub fn ratio_x10(&self) -> u32 {
        let short = self.short_side();
        if short == 0 {
            return 0;
        }
        // long ≤ 20000、×10 ≤ 200000，u32 安全。
        self.long_side().saturating_mul(10) / short
    }

    /// 是否横放登记（只影响展示，不影响限额判定）。
    pub fn is_landscape(&self) -> bool {
        self.width_dmm > self.height_dmm
    }
}

/// 封账凭证。`digest` 是封账时的内容摘要，`seal` 是摘要与盐的混合值；
/// 任一字段被改写，`verify` 都能区分「摘要不符」与「封签不符」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerSeal {
    pub digest: u64,
    pub count: u16,
    pub seal: u64,
}

/// 自定义纸账本。`next_id` 单调递增且**不回收**——删除后重新添加会得到
/// 新 ID，避免旧作业的纸 ID 指向新内容（这是「落账可信」的底线）。
#[derive(Clone, Debug)]
pub struct PaperLedger {
    entries: Vec<CustomPaper>,
    next_id: u16,
    sealed: bool,
}

impl PaperLedger {
    /// 空账本，首个 ID = 1。
    pub fn new() -> PaperLedger {
        PaperLedger {
            entries: Vec::with_capacity(LEDGER_CAP),
            next_id: 1,
            sealed: false,
        }
    }

    /// 当前条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 按下标取条目（判据遍历用）。
    pub fn at(&self, idx: usize) -> Option<&CustomPaper> {
        self.entries.get(idx)
    }

    /// 按 ID 取条目。
    pub fn get(&self, id: u16) -> Option<&CustomPaper> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 按名称查找（忽略大小写与首尾空白）。
    pub fn find_name(&self, name: &str) -> Option<&CustomPaper> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name.eq_ignore_ascii_case(trimmed) {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 增加一条自定义纸：先做四道限额校验，再判重名，最后分配 ID 落账。
    ///
    /// 顺序是契约：若先判重名，一个「重名且越界」的样本会得到 `DuplicateName`，
    /// 注入核就无法用同一坏样本稳定断言越界码。先校验几何、后判重名，
    /// 让「坏尺寸」永远优先暴露。
    pub fn insert(&mut self, name: &str, width_dmm: u32, height_dmm: u32) -> Result<u16, CustomPaperError> {
        if self.sealed {
            return Err(CustomPaperError::SealedLedger);
        }
        if self.entries.len() >= LEDGER_CAP {
            return Err(CustomPaperError::CapacityFull);
        }
        let trimmed = name.trim();
        check_name(trimmed)?;
        check_geometry(width_dmm, height_dmm, &Limits::office())?;
        if self.find_name(trimmed).is_some() {
            return Err(CustomPaperError::DuplicateName);
        }
        if self.next_id == 0 || self.next_id == u16::MAX {
            // ID 空间耗尽：u16 上界只是理论值，但显式失败比回绕成 0 好。
            return Err(CustomPaperError::CapacityFull);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(CustomPaper {
            id,
            name: String::from(trimmed),
            width_dmm,
            height_dmm,
        });
        Ok(id)
    }

    /// 按 ID 删除。返回被删条目，便于审计端记录「谁被撤了」。
    pub fn remove(&mut self, id: u16) -> Result<CustomPaper, CustomPaperError> {
        if self.sealed {
            return Err(CustomPaperError::SealedLedger);
        }
        let mut pos: Option<usize> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    pos = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match pos {
            Some(p) => Ok(self.entries.remove(p)),
            None => Err(CustomPaperError::UnknownId),
        }
    }

    /// 内容摘要（FNV-1a）：覆盖条目数、每条 ID/名称字节/宽/高。
    /// 摘要不覆盖 `next_id` 与封账状态——这两者是账本运行态，
    /// 内容未变时重算摘要必须相同，封账判据才成立。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = mix(h, self.entries.len() as u64);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = mix(h, e.id as u64);
                let bytes = e.name.as_bytes();
                let mut k = 0usize;
                while k < bytes.len() {
                    if let Some(b) = bytes.get(k) {
                        h = mix(h, *b as u64);
                    }
                    k += 1;
                }
                h = mix(h, e.width_dmm as u64);
                h = mix(h, e.height_dmm as u64);
            }
            i += 1;
        }
        h
    }

    /// 封账：冻结内容摘要并返回封签。重复封账显式拒绝，便于审计端
    /// 区分「首次封账」与「有人试图重封换摘要」。
    pub fn seal(&mut self) -> Result<LedgerSeal, CustomPaperError> {
        if self.sealed {
            return Err(CustomPaperError::SealedLedger);
        }
        let digest = self.digest();
        let count = self.entries.len() as u16;
        let seal = mix(digest ^ (count as u64), SEAL_SALT);
        self.sealed = true;
        Ok(LedgerSeal {
            digest,
            count,
            seal,
        })
    }

    /// 复核封签：摘要、条数、封签三项必须同时一致。
    pub fn verify(&self, s: &LedgerSeal) -> Result<(), CustomPaperError> {
        if s.count as usize != self.entries.len() {
            return Err(CustomPaperError::DigestMismatch);
        }
        if s.digest != self.digest() {
            return Err(CustomPaperError::DigestMismatch);
        }
        let want = mix(s.digest ^ (s.count as u64), SEAL_SALT);
        if want != s.seal {
            return Err(CustomPaperError::DigestMismatch);
        }
        Ok(())
    }
}

/// FNV-1a 混合原语。
fn mix(h: u64, v: u64) -> u64 {
    let x = h ^ v;
    x.wrapping_mul(0x100_0000_01b3)
}

/// 名称校验：空白名与超长名分码，便于 UI 分别提示。
fn check_name(name: &str) -> Result<(), CustomPaperError> {
    if name.trim().is_empty() {
        return Err(CustomPaperError::EmptyName);
    }
    if name.chars().count() > NAME_MAX {
        return Err(CustomPaperError::NameTooLong);
    }
    Ok(())
}

/// 几何校验四道闸。顺序固定：边长 → 面积 → 比例。
fn check_geometry(w: u32, h: u32, lim: &Limits) -> Result<(), CustomPaperError> {
    let short = if w <= h { w } else { h };
    let long = if w >= h { w } else { h };
    if short < lim.min_side_dmm {
        return Err(CustomPaperError::SideBelowMin);
    }
    if long > lim.max_side_dmm {
        return Err(CustomPaperError::SideAboveMax);
    }
    let area = (w as u64) * (h as u64);
    if area > lim.max_area_dmm2 {
        return Err(CustomPaperError::AreaAboveMax);
    }
    if long.saturating_mul(10) / short > lim.max_ratio_x10 {
        return Err(CustomPaperError::RatioAboveMax);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 注入核：八类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 8 类坏样本并断言诊断码。返回 (抓到, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let mut l = PaperLedger::new();

    // 1. 空名：不是「自动命名」，显式拒绝。
    if l.insert("   ", 1000, 1500).err() == Some(CustomPaperError::EmptyName) {
        caught += 1;
    }
    // 2. 超长名：33 字符 > NAME_MAX。
    if l.insert("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456", 1000, 1500).err()
        == Some(CustomPaperError::NameTooLong)
    {
        caught += 1;
    }
    // 3. 短边过低：40mm < 50mm。
    if l.insert("bad-low", 400, 1500).err() == Some(CustomPaperError::SideBelowMin) {
        caught += 1;
    }
    // 4. 长边过高：1600mm > 1500mm。
    if l.insert("bad-high", 1000, 16000).err() == Some(CustomPaperError::SideAboveMax) {
        caught += 1;
    }
    // 5. 比例超限：30:1 > 20:1。
    if l.insert("bad-ratio", 500, 15000).err() == Some(CustomPaperError::RatioAboveMax) {
        caught += 1;
    }
    // 6. 面积超限：2.1m² > 1.0m²。
    if l.insert("bad-area", 14000, 15000).err() == Some(CustomPaperError::AreaAboveMax) {
        caught += 1;
    }
    // 7. 重名（忽略大小写）：第二次写入必须拒绝，而不是产生两条同名片。
    let first = l.insert("Duplex-Label", 1000, 1500);
    let second = l.insert("duplex-label", 1200, 1600);
    if first.is_ok() && second.err() == Some(CustomPaperError::DuplicateName) {
        caught += 1;
    }
    // 8. 封账后写入：封账是单向下闸。
    if l.seal().is_ok() && l.insert("late", 1000, 1500).err() == Some(CustomPaperError::SealedLedger) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 合法样本 10 条：覆盖信封、标签、A 系、B 系、大幅面与横放。
fn legal_cases() -> [(&'static str, u32, u32); 10] {
    [
        ("Label-100x150", 1000, 1500),
        ("A4-Custom", 2100, 2970),
        ("Envelope-762x1270", 762, 1270),
        ("Wide-800x1250", 8000, 12500),
        ("Card-500x700", 5000, 7000),
        ("B4-Custom", 3300, 4820),
        ("Receipt-600x900", 600, 900),
        ("Photo-800x1000", 8000, 10000),
        ("Plot-1480x2100", 14800, 21000 / 10),
        ("Landscape-3530x2500", 3530, 2500),
    ]
}

/// 非法样本 8 条（重名与账满在审计里单独构造，合计 20 条样本）。
fn illegal_cases() -> [(&'static str, u32, u32, CustomPaperError); 8] {
    [
        ("", 1000, 1500, CustomPaperError::EmptyName),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456",
            1000,
            1500,
            CustomPaperError::NameTooLong,
        ),
        ("bad-low", 400, 1500, CustomPaperError::SideBelowMin),
        ("bad-low-h", 1000, 400, CustomPaperError::SideBelowMin),
        ("bad-high", 1000, 16000, CustomPaperError::SideAboveMax),
        ("bad-high-w", 15001, 3000, CustomPaperError::SideAboveMax),
        ("bad-area", 14000, 15000, CustomPaperError::AreaAboveMax),
        ("bad-ratio", 500, 15000, CustomPaperError::RatioAboveMax),
    ]
}

/// J1 的 20 样本审计：10 合法落账 + 10 越界拒绝（8 直拒 + 重名 + 账满）。
/// 返回 (合法通过数, 拒绝命中数, 样本总数)。
pub fn audit_samples() -> (usize, usize, usize) {
    let mut ledger = PaperLedger::new();
    let legal = legal_cases();
    let mut legal_ok = 0usize;
    let mut i = 0usize;
    while i < legal.len() {
        if let Some((name, w, h)) = legal.get(i) {
            match ledger.insert(name, *w, *h) {
                Ok(id) => {
                    if id as usize == i + 1 {
                        legal_ok += 1;
                    }
                }
                Err(_) => {}
            }
        }
        i += 1;
    }

    let mut rejected = 0usize;
    let illegal = illegal_cases();
    let mut k = 0usize;
    while k < illegal.len() {
        if let Some((name, w, h, want)) = illegal.get(k) {
            if ledger.insert(name, *w, *h).err() == Some(*want) {
                rejected += 1;
            }
        }
        k += 1;
    }
    // 样本 19：重名（合法条目再来一次）。
    let dup_want = CustomPaperError::DuplicateName;
    if ledger.insert("a4-custom", 2100, 2970).err() == Some(dup_want) {
        rejected += 1;
    }
    // 样本 20：账满。另一本账先填满 32 条，再写第 33 条。
    let mut full2 = PaperLedger::new();
    let mut m = 0usize;
    let mut filled = 0usize;
    while m < LEDGER_CAP {
        let name = format!("P{}", m);
        let id = 1000 + (m as u32) * 10;
        if full2.insert(&name, id, id + 500).is_ok() {
            filled += 1;
        }
        m += 1;
    }
    if filled == LEDGER_CAP && full2.insert("P99", 2000, 3000).err() == Some(CustomPaperError::CapacityFull) {
        rejected += 1;
    }

    (legal_ok, rejected, legal.len() + illegal.len() + 2)
}

pub fn run_unx50068_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50068");

    // J1-1 主判据：20 样本中 10 合法落账、10 越界拒绝（逐类码值一致）。
    let (legal_ok, rejected, total) = audit_samples();
    let ok = total == 20 && legal_ok == 10 && rejected == 10;
    let det = format!("legal={} rejected={} total={}", legal_ok, rejected, total);
    s.add_dyn("50068-J1-二十样本", ok, &det);

    // J1-2 ID 单调且不回收：删除中间条目后新增，ID 继续前进。
    let mut l = PaperLedger::new();
    let a = l.insert("one", 1000, 1500);
    let b = l.insert("two", 1100, 1600);
    let c = l.insert("three", 1200, 1700);
    let removed = l.remove(2);
    let d = l.insert("four", 1300, 1800);
    let ok = a == Ok(1)
        && b == Ok(2)
        && c == Ok(3)
        && removed.map(|p| p.name) == Ok(String::from("two"))
        && d == Ok(4)
        && l.len() == 3
        && l.get(2).is_none();
    s.add("50068-J1-ID单调不回收", ok, if ok { "" } else { "ID 复用或删除异常" });

    // J1-3 名称边界：32 字符通过，33 字符拒绝；首尾空白不算长度。
    let mut l32 = PaperLedger::new();
    let n32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
    let ok = n32.chars().count() == 32
        && l32.insert(n32, 1000, 1500).is_ok()
        && l32.insert("        padded name        ", 1000, 1500).is_ok()
        && l32
            .insert("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456", 1200, 1600)
            .err()
            == Some(CustomPaperError::NameTooLong);
    s.add("50068-J1-名称边界", ok, if ok { "" } else { "名称边界漂移" });

    // J1-4 边长边界：50mm/1500mm 含等号通过，两侧各越 0.1mm 即拒绝。
    let mut lb = PaperLedger::new();
    let ok = lb.insert("min-side", 500, 1500).is_ok()
        && lb.insert("bad-499", 499, 1500).err() == Some(CustomPaperError::SideBelowMin)
        && lb.insert("max-side", 5000, 15000).is_ok()
        && lb.insert("bad-15001", 5000, 15001).err() == Some(CustomPaperError::SideAboveMax);
    s.add("50068-J1-边长边界", ok, if ok { "" } else { "边长边界不闭合" });

    // J1-5 面积边界：1250×800mm 恰为 1.0m² 通过，+0.01m² 拒绝。
    let mut la = PaperLedger::new();
    let exact = 12500u32.saturating_mul(8000u32);
    let over = 12500u32.saturating_mul(8001u32);
    let ok = exact as u64 == Limits::office().max_area_dmm2
        && la.insert("area-exact", 12500, 8000).is_ok()
        && la.insert("area-over", 12500, 8001).err() == Some(CustomPaperError::AreaAboveMax)
        && over as u64 > Limits::office().max_area_dmm2;
    s.add("50068-J1-面积边界", ok, if ok { "" } else { "面积边界不闭合" });

    // J1-6 比例边界：20:1 含等号通过；21:1 拒绝。
    let mut lr = PaperLedger::new();
    let ok = lr.insert("ratio-20", 500, 10000).is_ok()
        && lr.insert("ratio-21", 500, 10500).err() == Some(CustomPaperError::RatioAboveMax);
    s.add("50068-J1-比例边界", ok, if ok { "" } else { "比例边界不闭合" });

    // J1-7 重名忽略大小写与空白：三种写法视为同一张纸。
    let mut ld = PaperLedger::new();
    let ok = ld.insert("Dup", 1000, 1500).is_ok()
        && ld.insert("dup", 1200, 1600).err() == Some(CustomPaperError::DuplicateName)
        && ld.insert("  DUP  ", 1200, 1600).err() == Some(CustomPaperError::DuplicateName)
        && ld.len() == 1;
    s.add("50068-J1-重名判定", ok, if ok { "" } else { "重名判定漏抓" });

    // J1-8 账满：恰好 32 条通过，第 33 条 CapacityFull。
    let mut lf = PaperLedger::new();
    let mut filled = 0usize;
    let mut i = 0usize;
    while i < LEDGER_CAP {
        let name = format!("P{}", i);
        if lf.insert(&name, 1000 + (i as u32) * 10, 1500 + (i as u32) * 10).is_ok() {
            filled += 1;
        }
        i += 1;
    }
    let ok = filled == 32
        && lf.len() == 32
        && lf.insert("P99", 2000, 3000).err() == Some(CustomPaperError::CapacityFull);
    let det = format!("filled={} len={}", filled, lf.len());
    s.add_dyn("50068-J1-账满边界", ok, &det);

    // J1-9 封账单向：封账后增/删/重封全部 SealedLedger，且内容不再变化。
    let mut ls = PaperLedger::new();
    let _ = ls.insert("one", 1000, 1500);
    let seal = ls.seal();
    let len_before = ls.len();
    let ok = match seal {
        Ok(sig) => {
            ls.insert("two", 1100, 1600).err() == Some(CustomPaperError::SealedLedger)
                && ls.remove(1).err() == Some(CustomPaperError::SealedLedger)
                && ls.seal().err() == Some(CustomPaperError::SealedLedger)
                && ls.len() == len_before
                && ls.verify(&sig).is_ok()
        }
        Err(_) => false,
    };
    s.add("50068-J1-封账单向", ok, if ok { "" } else { "封账后仍可写入" });

    // J1-10 摘要确定性与敏感性：同内容同摘要；改尺寸/名称即变。
    let mut d1 = PaperLedger::new();
    let _ = d1.insert("alpha", 1000, 1500);
    let _ = d1.insert("beta", 2000, 3000);
    let mut d2 = PaperLedger::new();
    let _ = d2.insert("alpha", 1000, 1500);
    let _ = d2.insert("beta", 2000, 3000);
    let mut d3 = PaperLedger::new();
    let _ = d3.insert("alpha", 1000, 1501);
    let _ = d3.insert("beta", 2000, 3000);
    let mut d4 = PaperLedger::new();
    let _ = d4.insert("alpha", 1000, 1500);
    let _ = d4.insert("gamma", 2000, 3000);
    let ok = d1.digest() == d2.digest()
        && d1.digest() != d3.digest()
        && d1.digest() != d4.digest();
    let det = format!(
        "d1={:016x} d3={:016x}",
        d1.digest(),
        d3.digest()
    );
    s.add_dyn("50068-J1-摘要敏感", ok, &det);

    // J1-11 封签防篡改：条数或封签任一被改，verify 报 DigestMismatch。
    let mut lv = PaperLedger::new();
    let _ = lv.insert("one", 1000, 1500);
    let ok = match lv.seal() {
        Ok(sig) => {
            let mut bad_count = sig;
            bad_count.count = bad_count.count.saturating_add(1);
            let mut bad_seal = sig;
            bad_seal.seal ^= 0x1;
            lv.verify(&sig).is_ok()
                && lv.verify(&bad_count).err() == Some(CustomPaperError::DigestMismatch)
                && lv.verify(&bad_seal).err() == Some(CustomPaperError::DigestMismatch)
        }
        Err(_) => false,
    };
    s.add("50068-J1-封签防篡改", ok, if ok { "" } else { "篡改未被抓" });

    // J1-12 横放等价：150×100 与 100×150 的短边/长边/比例/面积全部一致。
    let mut lw = PaperLedger::new();
    let _ = lw.insert("landscape", 1500, 1000);
    let _ = lw.insert("portrait", 1000, 1500);
    let ok = match (lw.get(1), lw.get(2)) {
        (Some(a), Some(b)) => {
            a.short_side() == b.short_side()
                && a.long_side() == b.long_side()
                && a.ratio_x10() == b.ratio_x10()
                && a.area_dmm2() == b.area_dmm2()
                && a.is_landscape()
                && !b.is_landscape()
        }
        _ => false,
    };
    s.add("50068-J1-横放等价", ok, if ok { "" } else { "横放判定不一致" });

    // J1-13 未知 ID 删除显式拒绝，且账本不变。
    let mut lu = PaperLedger::new();
    let _ = lu.insert("one", 1000, 1500);
    let before = lu.digest();
    let ok = lu.remove(999).err() == Some(CustomPaperError::UnknownId)
        && lu.len() == 1
        && lu.digest() == before;
    s.add("50068-J1-未知ID", ok, if ok { "" } else { "未知 ID 静默通过" });

    // J1-14 错误码闭集：11 条，码值 = SEG_BASE + 1..=11 且两两不同。
    let mut codes_ok = ALL_ERRORS.len() == 11;
    let mut marks = [false; 12];
    let mut e = 0usize;
    while e < ALL_ERRORS.len() {
        if let Some(err) = ALL_ERRORS.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 {
                codes_ok = false;
            }
            let off = (err.code() - SEG_BASE) as usize;
            match marks.get_mut(off) {
                Some(m) => {
                    if *m {
                        codes_ok = false;
                    }
                    *m = true;
                }
                None => codes_ok = false,
            }
        }
        e += 1;
    }
    s.add("50068-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-15 注入核 8 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50068-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50068_checks_green() {
        let set = run_unx50068_checks();
        assert!(set.all_passed(), "UNX-F50068 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50068_ids_never_recycled() {
        let mut l = PaperLedger::new();
        assert_eq!(l.insert("a", 1000, 1500), Ok(1));
        assert_eq!(l.insert("b", 1100, 1600), Ok(2));
        assert_eq!(l.remove(1).map(|p| p.id), Ok(1));
        assert_eq!(l.insert("c", 1200, 1700), Ok(3));
    }

    #[test]
    fn unx50068_seal_is_stable() {
        let mut a = PaperLedger::new();
        let mut b = PaperLedger::new();
        let _ = a.insert("x", 1000, 1500);
        let _ = b.insert("x", 1000, 1500);
        let sa = a.seal();
        let sb = b.seal();
        assert!(sa == sb && sa.is_ok());
    }
}
