//! UNX-F50008 色彩样本页（24 色卡样本入账）（0x42d20 段）
//!
//! 锚点：UNX-F50008-J1 色卡 ΔE 逐块落账（模拟器档）；判据锚定 §AI-63 §3 判据主轴。
//!
//! 数据段 SEG_BASE = 0x42d20，诊断码闭集为 Base+1..Base+11。
//! 24 块样本页取自标准反射色卡布局（6 行 × 4 列，含 4 级中性灰）：每块登记
//! 参考 sRGB 与参考 Lab（Lab 由 sRGB 经 D50 PCS 现算，不在源码里写死浮点）。
//! 入账按「块」结算：block = slot / 4，六块各 4 槽；块均值加权必须回到整卡
//! 均值（守恒判据），这样任何漏账/重账都会破坏守恒而不是被平均掉。
//!
//! 结构清单：
//! * `Patch`/`Card` 色卡注册表（24 槽、槽序严格、id 唯一、容量上限）；
//! * `DeltaLedger` 逐槽入账账本（(card, slot) 幂等拒重、块级聚合、判定分档）；
//! * `Verdict` 判定三档（Pass ≤2.0 / Warn ≤4.0 / Fail），NaN 显式判 Fail；
//! * `measure()` 模拟器档测量（profile 域往返，含裁剪）；
//! * 注入核 `injection_core()`：六类坏样本逐类断言显式拒绝。
//!
//! 错误纪律：内核路径零 panic，账本只按 `get()` 读行；浮点只用 `libm`。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{delta_e2000, rgb8_to_lab, IccProfile, Lab, Rgb8};

/// 诊断段（0x42d20 独占 0x20）。
pub const SEG_BASE: u32 = 0x42d20;

pub const E_EMPTY: u16 = 1;
pub const E_CARDLEN: u16 = 2;
pub const E_DUPID: u16 = 3;
pub const E_SLOT: u16 = 4;
pub const E_CAPACITY: u16 = 5;
pub const E_DUPBOOK: u16 = 6;
pub const E_NOCARD: u16 = 7;
pub const E_RANGE: u16 = 8;
pub const E_NAME: u16 = 9;
pub const E_ORDER: u16 = 10;
pub const E_PARTIAL: u16 = 11;

/// 色卡账本错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardError {
    Empty,
    CardLen,
    DuplicateId,
    SlotOrder,
    Capacity,
    DuplicateBooking,
    UnknownCard,
    Range,
    NameEmpty,
    Order,
    Partial,
}

impl CardError {
    pub fn code(self) -> u32 {
        let n = match self {
            CardError::Empty => E_EMPTY,
            CardError::CardLen => E_CARDLEN,
            CardError::DuplicateId => E_DUPID,
            CardError::SlotOrder => E_SLOT,
            CardError::Capacity => E_CAPACITY,
            CardError::DuplicateBooking => E_DUPBOOK,
            CardError::UnknownCard => E_NOCARD,
            CardError::Range => E_RANGE,
            CardError::NameEmpty => E_NAME,
            CardError::Order => E_ORDER,
            CardError::Partial => E_PARTIAL,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            CardError::Empty => "empty card or ledger",
            CardError::CardLen => "card does not carry exactly 24 patches",
            CardError::DuplicateId => "card id already registered",
            CardError::SlotOrder => "patch slots not in 0..23 order",
            CardError::Capacity => "registry or ledger capacity reached",
            CardError::DuplicateBooking => "slot already booked",
            CardError::UnknownCard => "card id not registered",
            CardError::Range => "slot outside 0..23",
            CardError::NameEmpty => "card name empty",
            CardError::Order => "booking order regression",
            CardError::Partial => "block incomplete",
        }
    }
}

/// 样本页规模：24 槽 = 6 块 × 4 槽。
pub const PATCH_COUNT: usize = 24;
pub const BLOCK_COUNT: usize = 6;
pub const PER_BLOCK: usize = 4;

/// 判定阈值。ΔE2000 = 2.0 是「训练有素观察者在 D50 灯箱下可辨」的保守上限，
/// 4.0 以上视为不可接受（工业验收口径），两阈值写死不留运行期配置。
pub const PASS_MAX: f32 = 2.0;
pub const WARN_MAX: f32 = 4.0;

/// 判定三档。NaN 一律 Fail——不可比较的 ΔE 不允许被当成合格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Warn,
    Fail,
}

impl Verdict {
    pub fn for_delta(d: f32) -> Verdict {
        if d != d {
            return Verdict::Fail;
        }
        if d <= PASS_MAX {
            Verdict::Pass
        } else if d <= WARN_MAX {
            Verdict::Warn
        } else {
            Verdict::Fail
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Verdict::Pass => 0,
            Verdict::Warn => 1,
            Verdict::Fail => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Warn => "warn",
            Verdict::Fail => "fail",
        }
    }
}

/// 24 块标准反射色卡 sRGB 码值（行优先：6 行 × 4 列，末行 4 级中性灰 + 黑）。
/// 数据是样本页本身的定义，不是被测结果。
pub const PATCH_SRGB: [(&str, u8, u8, u8); PATCH_COUNT] = [
    ("dark-skin", 115, 82, 68),
    ("light-skin", 194, 150, 130),
    ("blue-sky", 98, 122, 157),
    ("foliage", 87, 108, 67),
    ("blue-flower", 133, 128, 177),
    ("bluish-green", 103, 189, 170),
    ("orange", 214, 126, 44),
    ("purplish-blue", 80, 91, 166),
    ("moderate-red", 193, 90, 99),
    ("purple", 94, 60, 108),
    ("yellow-green", 157, 188, 64),
    ("orange-yellow", 224, 163, 46),
    ("blue", 56, 61, 150),
    ("green", 70, 148, 73),
    ("red", 175, 54, 60),
    ("yellow", 231, 199, 31),
    ("magenta", 187, 86, 149),
    ("cyan", 8, 133, 161),
    ("white-9.5", 243, 243, 242),
    ("neutral-8", 200, 200, 200),
    ("neutral-6.5", 160, 160, 160),
    ("neutral-5", 122, 122, 121),
    ("neutral-3.5", 85, 85, 85),
    ("black-2", 52, 52, 52),
];

/// 一块样本：槽号 + 名称 + 参考 sRGB + 参考 Lab（PCS D50）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub slot: u8,
    pub name: &'static str,
    pub rgb: Rgb8,
    pub reference: Lab,
}

/// 按标准布局构造 24 块。参考 Lab 现算，保证与 F50001 的转换口径同源。
pub fn standard_patches() -> Vec<Patch> {
    let mut out = Vec::with_capacity(PATCH_COUNT);
    let mut i = 0usize;
    while i < PATCH_COUNT {
        if let Some(row) = PATCH_SRGB.get(i) {
            let (name, r, g, b) = *row;
            let rgb = Rgb8::new(r, g, b);
            out.push(Patch {
                slot: i as u8,
                name,
                rgb,
                reference: rgb8_to_lab(rgb),
            });
        }
        i += 1;
    }
    out
}

/// 一张已注册的色卡。patches 恒为 24 条且槽序 = 下标。
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: u16,
    pub name: String,
    pub patches: Vec<Patch>,
}

impl Card {
    /// 按槽号取参考 Lab。槽序已被注册表保证，这里只做区间校验。
    pub fn reference(&self, slot: u8) -> Option<Lab> {
        if slot as usize >= self.patches.len() {
            return None;
        }
        match self.patches.get(slot as usize) {
            Some(p) => Some(p.reference),
            None => None,
        }
    }

    pub fn rgb(&self, slot: u8) -> Option<Rgb8> {
        match self.patches.get(slot as usize) {
            Some(p) => Some(p.rgb),
            None => None,
        }
    }
}

/// 色卡注册表。检查顺序固定为 名称 → 块数 → 槽序 → 容量 → 重号，
/// 保证每个坏样本只命中一个确定诊断码。
#[derive(Clone, Debug)]
pub struct CardBook {
    cards: Vec<Card>,
    cap: usize,
    rejections: u32,
}

impl CardBook {
    pub fn new(cap: usize) -> CardBook {
        CardBook {
            cards: Vec::new(),
            cap,
            rejections: 0,
        }
    }

    /// 注册一张卡。槽号必须逐位等于下标——乱序样本页会让「逐块」聚合错位，
    /// 因此宁可拒绝也不在注册期静默重排。
    pub fn register(
        &mut self,
        id: u16,
        name: &str,
        patches: Vec<Patch>,
    ) -> Result<(), CardError> {
        if name.is_empty() {
            self.rejections += 1;
            return Err(CardError::NameEmpty);
        }
        if patches.len() != PATCH_COUNT {
            self.rejections += 1;
            return Err(CardError::CardLen);
        }
        let mut i = 0usize;
        while i < PATCH_COUNT {
            match patches.get(i) {
                Some(p) => {
                    if p.slot as usize != i {
                        self.rejections += 1;
                        return Err(CardError::SlotOrder);
                    }
                }
                None => {
                    self.rejections += 1;
                    return Err(CardError::CardLen);
                }
            }
            i += 1;
        }
        if self.cards.len() >= self.cap {
            self.rejections += 1;
            return Err(CardError::Capacity);
        }
        let mut j = 0usize;
        while j < self.cards.len() {
            if self.cards.get(j).map(|c| c.id) == Some(id) {
                self.rejections += 1;
                return Err(CardError::DuplicateId);
            }
            j += 1;
        }
        self.cards.push(Card {
            id,
            name: name.to_string(),
            patches,
        });
        Ok(())
    }

    /// 一步注册标准样本页（id=1，名称 "UNX 24-patch"）。
    pub fn with_standard(&mut self) -> Result<(), CardError> {
        self.register(1, "UNX 24-patch", standard_patches())
    }

    pub fn get(&self, id: u16) -> Option<&Card> {
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.id == id {
                    return Some(c);
                }
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn rejections(&self) -> u32 {
        self.rejections
    }
}

/// 模拟器档测量：源 sRGB → Lab → 目标设备线性 RGB（裁剪）→ Lab。
/// 设备域越小，往返裁剪越狠，ΔE 越大——这是「模拟器档」的唯一来源。
pub fn measure(p: &IccProfile, rgb: Rgb8) -> Lab {
    let src = rgb8_to_lab(rgb);
    p.device_to_lab(p.lab_to_device(src))
}

/// 一条入账记录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Booking {
    pub seq: u32,
    pub card: u16,
    pub slot: u8,
    pub block: u8,
    pub delta_e: f32,
    pub verdict: Verdict,
}

/// 逐槽入账账本。(card, slot) 唯一，重复入账显式拒绝；
/// 入账顺序不强制槽序递增（现场测量顺序可能被打散），但同一槽只能有一条。
#[derive(Clone, Debug)]
pub struct DeltaLedger {
    rows: Vec<Booking>,
    cap: usize,
    dup: u32,
    rejections: u32,
}

impl DeltaLedger {
    pub fn new(cap: usize) -> DeltaLedger {
        DeltaLedger {
            rows: Vec::new(),
            cap,
            dup: 0,
            rejections: 0,
        }
    }

    /// 入账一块。返回该块的 ΔE2000。
    pub fn book(
        &mut self,
        book: &CardBook,
        card: u16,
        slot: u8,
        measured: Lab,
    ) -> Result<f32, CardError> {
        let c = match book.get(card) {
            Some(c) => c,
            None => {
                self.rejections += 1;
                return Err(CardError::UnknownCard);
            }
        };
        if slot as usize >= PATCH_COUNT {
            self.rejections += 1;
            return Err(CardError::Range);
        }
        let reference = match c.reference(slot) {
            Some(r) => r,
            None => {
                self.rejections += 1;
                return Err(CardError::Range);
            }
        };
        if self.rows.len() >= self.cap {
            self.rejections += 1;
            return Err(CardError::Capacity);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.card == card && r.slot == slot {
                    self.dup += 1;
                    self.rejections += 1;
                    return Err(CardError::DuplicateBooking);
                }
            }
            i += 1;
        }
        let d = delta_e2000(reference, measured);
        let seq = self.rows.len() as u32 + 1;
        self.rows.push(Booking {
            seq,
            card,
            slot,
            block: slot / PER_BLOCK as u8,
            delta_e: d,
            verdict: Verdict::for_delta(d),
        });
        Ok(d)
    }

    /// 整卡入账：24 槽逐块测量并入账，返回（均值, 最大值）。
    /// 中途失败即返回，已入账行保留（显式，不回滚，避免二次入账被误判重复）。
    pub fn book_card(
        &mut self,
        book: &CardBook,
        card: u16,
        p: &IccProfile,
    ) -> Result<(f32, f32), CardError> {
        let c = match book.get(card) {
            Some(c) => c,
            None => return Err(CardError::UnknownCard),
        };
        let mut i = 0usize;
        while i < PATCH_COUNT {
            let rgb = match c.rgb(i as u8) {
                Some(v) => v,
                None => return Err(CardError::Range),
            };
            self.book(book, card, i as u8, measure(p, rgb))?;
            i += 1;
        }
        match (self.mean(), self.max()) {
            (Some(m), Some(x)) => Ok((m, x)),
            _ => Err(CardError::Empty),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, i: usize) -> Option<&Booking> {
        self.rows.get(i)
    }

    pub fn duplicates(&self) -> u32 {
        self.dup
    }

    pub fn rejections(&self) -> u32 {
        self.rejections
    }

    /// 全卡均值。空账返回 None，不做「0 均值」伪值。
    pub fn mean(&self) -> Option<f32> {
        if self.rows.is_empty() {
            return None;
        }
        let mut sum = 0.0f32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                sum += r.delta_e;
            }
            i += 1;
        }
        Some(sum / self.rows.len() as f32)
    }

    pub fn max(&self) -> Option<f32> {
        let mut best = -1.0f32;
        let mut i = 0usize;
        let mut any = false;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if !any || r.delta_e > best {
                    best = r.delta_e;
                    any = true;
                }
            }
            i += 1;
        }
        if any {
            Some(best)
        } else {
            None
        }
    }

    /// 块计数（6 块各 4 槽）。
    pub fn block_counts(&self) -> [usize; BLOCK_COUNT] {
        let mut out = [0usize; BLOCK_COUNT];
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                let b = r.block as usize;
                if b < BLOCK_COUNT {
                    if let Some(c) = out.get_mut(b) {
                        *c += 1;
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// 块均值。该块不满 4 槽返回 None——不完整的块不得参与整卡结算。
    pub fn block_mean(&self, block: u8) -> Option<f32> {
        if block as usize >= BLOCK_COUNT {
            return None;
        }
        let mut sum = 0.0f32;
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.block == block {
                    sum += r.delta_e;
                    n += 1;
                }
            }
            i += 1;
        }
        if n == PER_BLOCK {
            Some(sum / n as f32)
        } else {
            None
        }
    }

    /// 块守恒间隙：|整卡均值 − 六块均值等权平均|。满账时必须趋零，
    /// 一旦有槽漏账/重账，块均值与整卡均值就会分叉。
    pub fn block_conservation_gap(&self) -> Option<f32> {
        let overall = self.mean()?;
        let mut sum = 0.0f32;
        let mut b = 0u8;
        while (b as usize) < BLOCK_COUNT {
            sum += self.block_mean(b)?;
            b += 1;
        }
        let blocks = sum / BLOCK_COUNT as f32;
        Some(fabsf(overall - blocks))
    }

    pub fn verdict_counts(&self) -> (usize, usize, usize) {
        let mut pass = 0usize;
        let mut warn = 0usize;
        let mut fail = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                match r.verdict {
                    Verdict::Pass => pass += 1,
                    Verdict::Warn => warn += 1,
                    Verdict::Fail => fail += 1,
                }
            }
            i += 1;
        }
        (pass, warn, fail)
    }

    /// 摘要（非空）。含块内最大 ΔE，便于写回结果摘要字段。
    pub fn summary(&self) -> String {
        let mean = match self.mean() {
            Some(v) => v,
            None => 0.0,
        };
        let max = match self.max() {
            Some(v) => v,
            None => 0.0,
        };
        let (pass, warn, fail) = self.verdict_counts();
        format!(
            "色卡 patches={} mean={:.3} max={:.3} pass={} warn={} fail={} dup={}",
            self.rows.len(),
            mean,
            max,
            pass,
            warn,
            fail,
            self.dup
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 六类坏样本 → 命中数。每条都必须返回确定诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;
    let mut book = CardBook::new(4);
    let patches = standard_patches();

    // ① 块数不足（23 块）
    let mut short = Vec::new();
    let mut i = 0usize;
    while i < PATCH_COUNT - 1 {
        if let Some(p) = patches.get(i) {
            short.push(*p);
        }
        i += 1;
    }
    if book.register(1, "short", short).err() == Some(CardError::CardLen) {
        caught += 1;
    }

    // ② 槽序跳跃（第 5 槽改成 6）
    let mut bad_slot = standard_patches();
    if let Some(p) = bad_slot.get_mut(5) {
        p.slot = 6;
    }
    if book.register(2, "bad-slot", bad_slot).err() == Some(CardError::SlotOrder) {
        caught += 1;
    }

    // ③ 注册后重号
    let ok = book.with_standard().is_ok()
        && book
            .register(1, "dup", standard_patches())
            .err()
            == Some(CardError::DuplicateId);
    if ok {
        caught += 1;
    }

    let mut led = DeltaLedger::new(64);
    // ④ 未知卡号
    if led.book(&book, 9, 0, Lab::new(50.0, 0.0, 0.0)).err() == Some(CardError::UnknownCard) {
        caught += 1;
    }
    // ⑤ 槽越界
    let ok = led.book(&book, 1, 99, Lab::new(50.0, 0.0, 0.0)).err() == Some(CardError::Range);
    if ok {
        caught += 1;
    }
    // ⑥ 重复入账同一槽
    let ok = led.book(&book, 1, 0, Lab::new(50.0, 0.0, 0.0)).is_ok()
        && led.book(&book, 1, 0, Lab::new(50.0, 0.0, 0.0)).err()
            == Some(CardError::DuplicateBooking);
    if ok {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50008");
    let patches = standard_patches();
    let srgb = IccProfile::builtin_srgb();

    // J1-1 样本页结构：24 槽、名称唯一、槽序等于下标
    let mut names_unique = true;
    let mut slots_ok = true;
    let mut i = 0usize;
    while i < patches.len() {
        if let Some(p) = patches.get(i) {
            if p.slot as usize != i {
                slots_ok = false;
            }
            let mut j = i + 1;
            while j < patches.len() {
                if let Some(q) = patches.get(j) {
                    if p.name == q.name {
                        names_unique = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    let ok = patches.len() == PATCH_COUNT && names_unique && slots_ok;
    let det = format!("n={} unique={} slots={}", patches.len(), names_unique, slots_ok);
    s.add_dyn("50008-J1-样本页结构", ok, &det);

    // J1-2 参考 Lab 合理：白块 L>90、黑块 L<50、逐块 L 有限
    let white_l = match patches.get(18) {
        Some(p) => p.reference.l,
        None => 0.0,
    };
    let black_l = match patches.get(23) {
        Some(p) => p.reference.l,
        None => 0.0,
    };
    let all_finite = patches.iter().all(|p| p.reference.l == p.reference.l);
    let ok = white_l > 90.0 && black_l < 50.0 && all_finite;
    let det = format!("Lwhite={:.2} Lblack={:.2}", white_l, black_l);
    s.add_dyn("50008-J1-参考Lab", ok, &det);

    // J1-3 槽序错误必拒
    let mut book = CardBook::new(4);
    let mut bad = standard_patches();
    if let Some(p) = bad.get_mut(3) {
        p.slot = 4;
    }
    let ok = book.register(1, "bad", bad).err() == Some(CardError::SlotOrder);
    s.add("50008-J1-槽序必拒", ok, if ok { "" } else { "乱序样本被接收" });

    // J1-4 块数错误必拒
    let mut book2 = CardBook::new(4);
    let mut short = Vec::new();
    let mut k = 0usize;
    while k < PATCH_COUNT - 1 {
        if let Some(p) = patches.get(k) {
            short.push(*p);
        }
        k += 1;
    }
    let ok = book2.register(1, "short", short).err() == Some(CardError::CardLen);
    s.add("50008-J1-块数必拒", ok, if ok { "" } else { "缺块样本被接收" });

    // J1-5 重号必拒 + 容量边界（两码必须可区分，不能都用容量兜住）
    let mut book3 = CardBook::new(4);
    let ok = book3.with_standard().is_ok()
        && book3
            .register(1, "again", standard_patches())
            .err()
            == Some(CardError::DuplicateId);
    let mut tiny = CardBook::new(1);
    let ok2 = tiny.with_standard().is_ok()
        && tiny.register(2, "second", standard_patches()).err() == Some(CardError::Capacity);
    let ok = ok && ok2;
    let det = if ok { "" } else { "重号/容量未拦" };
    s.add("50008-J1-重号容量", ok, det);

    // J1-6 整卡入账：24 行、每槽恰好一次、六块各 4 槽
    let mut book = CardBook::new(4);
    if book.with_standard().is_err() {
        s.fail("50008-J1-逐块入账", "标准卡注册失败");
        return s;
    }
    let mut led = DeltaLedger::new(64);
    let booked = led.book_card(&book, 1, &srgb);
    let counts = led.block_counts();
    let full_blocks = counts.iter().all(|c| *c == PER_BLOCK);
    let ok = booked.is_ok() && led.len() == PATCH_COUNT && full_blocks;
    let det = format!("rows={} counts={:?}", led.len(), counts);
    s.add_dyn("50008-J1-逐块入账", ok, &det);

    // J1-7 重复入账必拒，且重复计数 +1、行数不变
    let before = led.len();
    let dup = led.book(&book, 1, 7, Lab::new(50.0, 0.0, 0.0)).err();
    let ok = dup == Some(CardError::DuplicateBooking)
        && led.len() == before
        && led.duplicates() == 1;
    let det = format!("err={:?} rows={} dup={}", dup, led.len(), led.duplicates());
    s.add_dyn("50008-J1-幂等拒重", ok, &det);

    // J1-8 越界槽与未知卡显式区分
    let ok = led.book(&book, 1, 24, Lab::new(50.0, 0.0, 0.0)).err() == Some(CardError::Range)
        && led.book(&book, 5, 0, Lab::new(50.0, 0.0, 0.0)).err()
            == Some(CardError::UnknownCard);
    s.add("50008-J1-越界与未知", ok, if ok { "" } else { "越界/未知未区分" });

    // J1-9 自比零：测量值 = 参考值 → ΔE≈0 且 Pass
    let mut led2 = DeltaLedger::new(64);
    let mut zero_ok = true;
    let mut k = 0usize;
    while k < PATCH_COUNT {
        if let Some(p) = patches.get(k) {
            match led2.book(&book, 1, k as u8, p.reference) {
                Ok(d) => {
                    if fabsf(d) > 1e-4 {
                        zero_ok = false;
                    }
                }
                Err(_) => zero_ok = false,
            }
        }
        k += 1;
    }
    let counts = led2.verdict_counts();
    let ok = zero_ok && counts == (PATCH_COUNT, 0, 0);
    let det = format!("pass={} warn={} fail={}", counts.0, counts.1, counts.2);
    s.add_dyn("50008-J1-自比零", ok, &det);

    // J1-10 模拟器档：sRGB 自身往返均值趋零，缩小域出现可测降质（保序）
    let mut ident = DeltaLedger::new(64);
    let (ident_mean, _) = match ident.book_card(&book, 1, &srgb) {
        Ok(v) => v,
        Err(_) => (99.0, 99.0),
    };
    let printer = IccProfile::simulated_printer(0.70);
    let mut small = DeltaLedger::new(64);
    let (small_mean, small_max) = match small.book_card(&book, 1, &printer) {
        Ok(v) => v,
        Err(_) => (0.0, 0.0),
    };
    let ok = ident_mean <= 0.01 && small_mean > ident_mean + 0.1 && small_max > 1.0;
    let det = format!(
        "identity={:.4} printer={:.3} max={:.3}",
        ident_mean, small_mean, small_max
    );
    s.add_dyn("50008-J1-域收缩降质", ok, &det);

    // J1-11 块守恒：六块均值等权平均 = 整卡均值
    let gap = led.block_conservation_gap();
    let (ok, gv) = match gap {
        Some(g) => (g < 1e-4, g),
        None => (false, -1.0),
    };
    let det = format!("gap={:.6}", gv);
    s.add_dyn("50008-J1-块守恒", ok, &det);

    // J1-12 判定分档边界（阈值来自常量，不猜魔数）
    let ok = Verdict::for_delta(PASS_MAX) == Verdict::Pass
        && Verdict::for_delta(PASS_MAX + 0.01) == Verdict::Warn
        && Verdict::for_delta(WARN_MAX + 0.01) == Verdict::Fail
        && Verdict::for_delta(f32::NAN) == Verdict::Fail;
    s.add("50008-J1-判定分档", ok, if ok { "" } else { "阈值分档错误" });

    // J1-13 统计性质：max ≥ mean ≥ 0，且均值随域缩小不降
    let (mx, mn) = match (led.max(), led.mean()) {
        (Some(x), Some(m)) => (x, m),
        _ => (-1.0, -1.0),
    };
    let ok = mx >= mn && mn >= 0.0 && small_mean >= ident_mean;
    let det = format!("max={:.3} mean={:.3}", mx, mn);
    s.add_dyn("50008-J1-统计单调", ok, &det);

    // J1-14 确定性：同卡同 profile 两次入账逐位一致
    let mut a = DeltaLedger::new(64);
    let mut b = DeltaLedger::new(64);
    let ra = a.book_card(&book, 1, &printer).is_ok();
    let rb = b.book_card(&book, 1, &printer).is_ok();
    let mut same = ra && rb && a.len() == b.len();
    if same {
        let mut q = 0usize;
        while q < a.len() {
            match (a.row(q), b.row(q)) {
                (Some(x), Some(y)) => {
                    if x.delta_e != y.delta_e || x.slot != y.slot {
                        same = false;
                    }
                }
                _ => same = false,
            }
            q += 1;
        }
    }
    s.add("50008-J1-入账确定", same, if same { "" } else { "两次入账不一致" });

    // J1-15 空账不返回伪均值
    let empty = DeltaLedger::new(4);
    let ok = empty.mean().is_none() && empty.max().is_none() && empty.block_conservation_gap().is_none();
    s.add("50008-J1-空账无伪值", ok, if ok { "" } else { "空账返回了伪统计" });

    // J1-16 摘要非空且字段齐备
    let text = led.summary();
    let ok = !text.is_empty() && text.contains("patches=24") && text.contains("mean=");
    s.add("50008-J1-摘要非空", ok, if ok { "" } else { "摘要缺失" });

    // J1-17 六类注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50008-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50008_checks_green() {
        let set = run_unx50008_checks();
        assert!(set.all_passed(), "UNX-F50008 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50008_patch_count_and_blocks() {
        let patches = standard_patches();
        assert_eq!(patches.len(), 24);
        let mut book = CardBook::new(2);
        assert!(book.with_standard().is_ok());
        let mut led = DeltaLedger::new(32);
        assert!(led.book_card(&book, 1, &IccProfile::builtin_srgb()).is_ok());
        assert_eq!(led.block_counts(), [4, 4, 4, 4, 4, 4]);
        let gap = match led.block_conservation_gap() {
            Some(g) => g,
            None => 1.0,
        };
        assert!(gap < 1e-4);
    }

    #[test]
    fn unx50008_verdict_thresholds() {
        assert_eq!(Verdict::for_delta(0.0), Verdict::Pass);
        assert_eq!(Verdict::for_delta(3.0), Verdict::Warn);
        assert_eq!(Verdict::for_delta(9.0), Verdict::Fail);
        assert_eq!(Verdict::for_delta(f32::NAN), Verdict::Fail);
    }

    #[test]
    fn unx50008_error_codes_in_segment() {
        assert_eq!(CardError::Empty.code(), SEG_BASE + 1);
        assert_eq!(CardError::Partial.code(), SEG_BASE + 11);
    }
}
