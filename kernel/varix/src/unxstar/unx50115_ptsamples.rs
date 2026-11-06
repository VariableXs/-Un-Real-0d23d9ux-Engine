//! UNX-F50115 直通样本页账（×3 样本入账）（0x43a80 段）
//!
//! 锚点：UNX-F50115-J1 样本哈希账可复跑；判据锚定 §AI-63 §3。
//!
//! 直通回归用三页固定样本：文本页、矢量页、位图页。三页各有独立槽位
//! （1/2/3），入账后按槽位序折叠出账链。链只依赖「槽位 → 页面摘要」，
//! 与登记先后无关——这让并行回放（三页乱序登记）得到同一账根，
//! 而同一槽位内容被替换则必然改变链，篡改无从隐藏。
//!
//! 结构清单：
//! * 页面模型 `SamplePage` + 绘制算子 `Op`：槽位/类别/尺寸/算子序列；
//! * 槽位账 `SampleLedger`：定长 3 槽（Option 数组，无动态扩容），
//!   入账/复核/缺页掩码/账链按槽序重算；
//! * 文本编解码 `encode_ledger` / `parse_ledger`：逐行重建三槽并重算链；
//! * 注入核：页面坏样本 6 类 + 账文本坏样本 4 类 + 重复槽/缺页/篡改 3 类，
//!   共 13 类，逐类断言具体诊断码。
//!
//! 错误纪律：诊断码闭集 `0x43a80 + 1..=11`；内核路径零 panic——槽位访问
//! 一律先 `get`/`get_mut`，算子容量先判后写。

use crate::alloc::format;
use crate::alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43a80 独占 0x20）。
pub const SEG_BASE: u32 = 0x43a80;

pub const E_DUP: u16 = 1;
pub const E_ID: u16 = 2;
pub const E_CLASS: u16 = 3;
pub const E_EMPTY: u16 = 4;
pub const E_RANGE: u16 = 5;
pub const E_TAMPER: u16 = 6;
pub const E_MISSING: u16 = 7;
pub const E_FIELDS: u16 = 8;
pub const E_HEX: u16 = 9;
pub const E_CHECKSUM: u16 = 10;
pub const E_COUNT: u16 = 11;

/// 样本页账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleErr {
    Dup,
    Id,
    Class,
    Empty,
    Range,
    Tamper,
    Missing,
    Fields,
    Hex,
    Checksum,
    Count,
}

impl SampleErr {
    /// 诊断码（段基址 + 闭集内偏移）。
    pub fn code(self) -> u32 {
        let n = match self {
            SampleErr::Dup => E_DUP,
            SampleErr::Id => E_ID,
            SampleErr::Class => E_CLASS,
            SampleErr::Empty => E_EMPTY,
            SampleErr::Range => E_RANGE,
            SampleErr::Tamper => E_TAMPER,
            SampleErr::Missing => E_MISSING,
            SampleErr::Fields => E_FIELDS,
            SampleErr::Hex => E_HEX,
            SampleErr::Checksum => E_CHECKSUM,
            SampleErr::Count => E_COUNT,
        };
        SEG_BASE + n as u32
    }

    /// 稳定文案。
    pub fn message(self) -> &'static str {
        match self {
            SampleErr::Dup => "slot already registered",
            SampleErr::Id => "slot outside 1..=3",
            SampleErr::Class => "page class does not match slot",
            SampleErr::Empty => "page has no draw op",
            SampleErr::Range => "value outside legal range",
            SampleErr::Tamper => "page digest differs from registered digest",
            SampleErr::Missing => "slot not registered",
            SampleErr::Fields => "ledger line field count mismatch",
            SampleErr::Hex => "digest field malformed",
            SampleErr::Checksum => "ledger chain mismatch",
            SampleErr::Count => "declared slot count mismatch",
        }
    }
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_u64(h: u64, x: u64) -> u64 {
    let mut acc = h;
    let mut shift = 0u32;
    while shift < 64 {
        let b = ((x >> shift) & 0xff) as u8;
        acc ^= b as u64;
        acc = acc.wrapping_mul(FNV_PRIME);
        shift += 8;
    }
    acc
}

fn parse_hex(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < s.len() {
        let c = s.get(i..i + 1)?;
        let ch = c.chars().next()?;
        v = (v << 4) | ch.to_digit(16)? as u64;
        i += 1;
    }
    Some(v)
}

/// 样本页三类。槽位约定：1 文本、2 矢量、3 位图——这是三页固定样本的
/// 身份，不可互换（互换会让「缺哪一页」的掩码失去意义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleClass {
    Text,
    Vector,
    Image,
}

impl SampleClass {
    /// 类码（1..=3，与槽位一致）。
    pub fn code(self) -> u8 {
        match self {
            SampleClass::Text => 1,
            SampleClass::Vector => 2,
            SampleClass::Image => 3,
        }
    }

    /// 类码反解。
    pub fn from_code(c: u8) -> Result<SampleClass, SampleErr> {
        match c {
            1 => Ok(SampleClass::Text),
            2 => Ok(SampleClass::Vector),
            3 => Ok(SampleClass::Image),
            _ => Err(SampleErr::Class),
        }
    }

    /// 类码 → 槽位。
    pub fn slot(self) -> u8 {
        self.code()
    }

    /// 稳定文本标签。
    pub fn tag(self) -> &'static str {
        match self {
            SampleClass::Text => "text",
            SampleClass::Vector => "vector",
            SampleClass::Image => "image",
        }
    }

    /// 标签反解。
    pub fn from_tag(t: &str) -> Result<SampleClass, SampleErr> {
        match t {
            "text" => Ok(SampleClass::Text),
            "vector" => Ok(SampleClass::Vector),
            "image" => Ok(SampleClass::Image),
            _ => Err(SampleErr::Class),
        }
    }
}

/// 单条绘制算子（码 + 16 位参数）。码 0 保留，不得出现在真实页里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub code: u8,
    pub arg: u16,
}

impl Op {
    /// 构造并校验（码必须非 0）。
    pub fn new(code: u8, arg: u16) -> Result<Op, SampleErr> {
        if code == 0 {
            return Err(SampleErr::Range);
        }
        Ok(Op { code, arg })
    }
}

/// 单页最多 64 个算子：三页样本要的是形态覆盖（填充/描边/图像拼接），
/// 不是复杂版面；超过 64 说明有人在拿真实页面当样本。
pub const OPS_CAP: usize = 64;

/// 样本页。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplePage {
    pub slot: u8,
    pub class: SampleClass,
    pub width: u16,
    pub height: u16,
    pub ops: Vec<Op>,
}

impl SamplePage {
    /// 构造并校验。
    pub fn new(
        slot: u8,
        class: SampleClass,
        width: u16,
        height: u16,
        ops: Vec<Op>,
    ) -> Result<SamplePage, SampleErr> {
        let p = SamplePage {
            slot,
            class,
            width,
            height,
            ops,
        };
        p.validate()?;
        Ok(p)
    }

    /// 校验顺序：槽位 → 类别一致 → 算子数 → 尺寸 → 算子码。
    pub fn validate(&self) -> Result<(), SampleErr> {
        if self.slot < 1 || self.slot > 3 {
            return Err(SampleErr::Id);
        }
        if self.class.slot() != self.slot {
            return Err(SampleErr::Class);
        }
        if self.ops.is_empty() {
            return Err(SampleErr::Empty);
        }
        if self.ops.len() > OPS_CAP {
            return Err(SampleErr::Range);
        }
        if self.width == 0 || self.width > 20000 || self.height == 0 || self.height > 20000 {
            return Err(SampleErr::Range);
        }
        let mut i = 0usize;
        while i < self.ops.len() {
            match self.ops.get(i) {
                Some(o) => {
                    if o.code == 0 {
                        return Err(SampleErr::Range);
                    }
                }
                None => return Err(SampleErr::Range),
            }
            i += 1;
        }
        Ok(())
    }

    /// 页面估算字节数（算子 3 字节 + 8 字节页头；账本只记账，不驻留页面）。
    pub fn bytes_len(&self) -> u32 {
        self.ops.len() as u32 * 3 + 8
    }

    /// 页面摘要：槽位/类别/尺寸/算子数/逐算子。唯一口径，账与复核共用。
    pub fn digest(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.slot as u64);
        h = fnv_u64(h, self.class.code() as u64);
        h = fnv_u64(h, self.width as u64);
        h = fnv_u64(h, self.height as u64);
        h = fnv_u64(h, self.ops.len() as u64);
        let mut i = 0usize;
        while i < self.ops.len() {
            if let Some(o) = self.ops.get(i) {
                h = fnv_u64(h, o.code as u64);
                h = fnv_u64(h, o.arg as u64);
            }
            i += 1;
        }
        h
    }
}

/// 账内记录（只存摘要与估算字节，页面对象不驻留，避免样本页把账撑大）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRecord {
    pub slot: u8,
    pub class: SampleClass,
    pub digest: u64,
    pub bytes: u32,
}

impl SampleRecord {
    /// 记录摘要（账链折叠单元）。
    pub fn chain_digest(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.slot as u64);
        h = fnv_u64(h, self.class.code() as u64);
        h = fnv_u64(h, self.bytes as u64);
        fnv_u64(h, self.digest)
    }

    /// 审计文本行：`slot,class_tag,bytes,digest_hex`。
    pub fn to_line(&self) -> String {
        format!(
            "{},{},{:x},{:016x}",
            self.slot,
            self.class.tag(),
            self.bytes,
            self.digest
        )
    }
}

/// 三槽样本账。
#[derive(Clone, Debug)]
pub struct SampleLedger {
    slots: [Option<SampleRecord>; 3],
    chain: u64,
}

impl SampleLedger {
    /// 空账（链为 FNV 基值，区别于「脏链」）。
    pub fn new() -> SampleLedger {
        SampleLedger {
            slots: [None; 3],
            chain: FNV_BASIS,
        }
    }

    /// 当前账链。
    pub fn chain(&self) -> u64 {
        self.chain
    }

    /// 已入账槽数。
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if let Some(Some(_)) = self.slots.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 是否为空账。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 三页是否齐备。
    pub fn complete(&self) -> bool {
        self.len() == 3
    }

    /// 缺页掩码：bit0/bit1/bit2 对应槽 1/2/3，置位表示该槽缺页。
    pub fn missing_mask(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < 3 {
            match self.slots.get(i) {
                Some(None) => m |= 1 << i,
                _ => {}
            }
            i += 1;
        }
        m
    }

    /// 按槽位取记录。
    pub fn record(&self, slot: u8) -> Option<&SampleRecord> {
        if slot < 1 || slot > 3 {
            return None;
        }
        match self.slots.get((slot - 1) as usize) {
            Some(Some(r)) => Some(r),
            _ => None,
        }
    }

    /// 从三槽按槽序重算账链：登记先后因此不影响账根。
    fn recompute(&mut self) {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < 3 {
            if let Some(Some(r)) = self.slots.get(i) {
                h = fnv_u64(h, r.chain_digest());
            }
            i += 1;
        }
        self.chain = h;
    }

    /// 入账一页：校验 → 槽位占用 → 落记录 → 重算链。
    /// 占用检查在写入前，拒绝路径不改账。
    pub fn register(&mut self, p: &SamplePage) -> Result<u64, SampleErr> {
        p.validate()?;
        let idx = (p.slot - 1) as usize;
        match self.slots.get(idx) {
            Some(Some(_)) => return Err(SampleErr::Dup),
            Some(None) => {}
            None => return Err(SampleErr::Id),
        }
        let rec = SampleRecord {
            slot: p.slot,
            class: p.class,
            digest: p.digest(),
            bytes: p.bytes_len(),
        };
        if let Some(cell) = self.slots.get_mut(idx) {
            *cell = Some(rec);
        } else {
            return Err(SampleErr::Id);
        }
        self.recompute();
        Ok(rec.digest)
    }

    /// 复核一页：未入账 `Missing`，摘要不符 `Tamper`，一致返回摘要。
    pub fn verify(&self, p: &SamplePage) -> Result<u64, SampleErr> {
        p.validate()?;
        let r = match self.record(p.slot) {
            Some(v) => v,
            None => return Err(SampleErr::Missing),
        };
        let d = p.digest();
        if d != r.digest {
            return Err(SampleErr::Tamper);
        }
        Ok(d)
    }
}

/// 导出账文本：首行 `chain <hex> <count>`，其后按槽序三行。
pub fn encode_ledger(l: &SampleLedger) -> String {
    let mut out = format!("chain {:016x} {}\n", l.chain(), l.len());
    let mut slot = 1u8;
    while slot <= 3 {
        if let Some(r) = l.record(slot) {
            out.push_str(&r.to_line());
            out.push('\n');
        }
        slot += 1;
    }
    out
}

/// 导入账文本：逐行重建三槽（校验槽位/类别），再核对条数与链。
pub fn parse_ledger(text: &str) -> Result<SampleLedger, SampleErr> {
    if text.is_empty() {
        return Err(SampleErr::Fields);
    }
    let mut lines = text.split('\n');
    let head = match lines.next() {
        Some(h) => h,
        None => return Err(SampleErr::Fields),
    };
    let mut hp = head.split(' ');
    let tag = hp.next().unwrap_or("");
    let chain_s = hp.next().unwrap_or("");
    let count_s = hp.next().unwrap_or("");
    if tag != "chain" || hp.next().is_some() || chain_s.is_empty() || count_s.is_empty() {
        return Err(SampleErr::Fields);
    }
    let declared_chain = parse_hex(chain_s).ok_or(SampleErr::Hex)?;
    let declared_count = parse_hex(count_s).ok_or(SampleErr::Hex)? as usize;
    let mut l = SampleLedger::new();
    let mut seen = 0usize;
    loop {
        let line = match lines.next() {
            Some(x) => x,
            None => break,
        };
        if line.is_empty() {
            continue;
        }
        let mut f = line.split(',');
        let slot_s = f.next().unwrap_or("");
        let class_s = f.next().unwrap_or("");
        let bytes_s = f.next().unwrap_or("");
        let dig_s = f.next().unwrap_or("");
        if f.next().is_some()
            || slot_s.is_empty()
            || class_s.is_empty()
            || bytes_s.is_empty()
            || dig_s.is_empty()
        {
            return Err(SampleErr::Fields);
        }
        let slot = parse_hex(slot_s).ok_or(SampleErr::Hex)?;
        let bytes = parse_hex(bytes_s).ok_or(SampleErr::Hex)?;
        let digest = parse_hex(dig_s).ok_or(SampleErr::Hex)?;
        if slot < 1 || slot > 3 {
            return Err(SampleErr::Id);
        }
        if digest == 0 || bytes < 8 || bytes > 200 {
            return Err(SampleErr::Range);
        }
        let class = SampleClass::from_tag(class_s)?;
        if class.slot() != slot as u8 {
            return Err(SampleErr::Class);
        }
        let idx = (slot - 1) as usize;
        match l.slots.get(idx) {
            Some(Some(_)) => return Err(SampleErr::Dup),
            Some(None) => {}
            None => return Err(SampleErr::Id),
        }
        if let Some(cell) = l.slots.get_mut(idx) {
            *cell = Some(SampleRecord {
                slot: slot as u8,
                class,
                digest,
                bytes: bytes as u32,
            });
        }
        seen += 1;
    }
    if seen != declared_count {
        return Err(SampleErr::Count);
    }
    l.recompute();
    if l.chain() != declared_chain {
        return Err(SampleErr::Checksum);
    }
    Ok(l)
}

/// 构造固定三页样本（文本 4 算子 / 矢量 6 算子 / 位图 3 算子）。
pub fn sample_page(slot: u8) -> Result<SamplePage, SampleErr> {
    let class = SampleClass::from_code(slot)?;
    let n = match slot {
        1 => 4usize,
        2 => 6usize,
        _ => 3usize,
    };
    let mut ops = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        ops.push(Op {
            code: (i as u8) + slot,
            arg: (slot as u16) * 100 + i as u16,
        });
        i += 1;
    }
    SamplePage::new(slot, class, 595, 842, ops)
}

fn populated() -> SampleLedger {
    let mut l = SampleLedger::new();
    let mut s = 1u8;
    while s <= 3 {
        if let Ok(p) = sample_page(s) {
            let _ = l.register(&p);
        }
        s += 1;
    }
    l
}

/// 注入核：13 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 13usize;

    // 1 槽位 0 → Id
    if SamplePage::new(0, SampleClass::Text, 100, 100, Vec::new()).err() == Some(SampleErr::Id) {
        caught += 1;
    }
    // 2 槽位 4 → Id
    if SamplePage::new(4, SampleClass::Image, 100, 100, Vec::new()).err() == Some(SampleErr::Id) {
        caught += 1;
    }
    // 3 类别与槽位不一致 → Class
    let mut ops = Vec::new();
    if let Ok(o) = Op::new(1, 1) {
        ops.push(o);
    }
    if SamplePage::new(1, SampleClass::Image, 100, 100, ops.clone()).err()
        == Some(SampleErr::Class)
    {
        caught += 1;
    }
    // 4 空算子 → Empty
    if SamplePage::new(1, SampleClass::Text, 100, 100, Vec::new()).err() == Some(SampleErr::Empty) {
        caught += 1;
    }
    // 5 算子超 64 → Range
    let mut many = Vec::with_capacity(OPS_CAP + 1);
    let mut i = 0usize;
    while i < OPS_CAP + 1 {
        many.push(Op {
            code: 1,
            arg: i as u16,
        });
        i += 1;
    }
    if SamplePage::new(1, SampleClass::Text, 100, 100, many).err() == Some(SampleErr::Range) {
        caught += 1;
    }
    // 6 尺寸 0 → Range
    if SamplePage::new(1, SampleClass::Text, 0, 100, ops.clone()).err() == Some(SampleErr::Range) {
        caught += 1;
    }
    // 7 重复槽入账 → Dup
    let mut l = SampleLedger::new();
    let p1 = match sample_page(1) {
        Ok(v) => v,
        Err(_) => SamplePage {
            slot: 1,
            class: SampleClass::Text,
            width: 1,
            height: 1,
            ops: Vec::new(),
        },
    };
    if l.register(&p1).is_ok() && l.register(&p1).err() == Some(SampleErr::Dup) {
        caught += 1;
    }
    // 8 复核未入账槽 → Missing
    let empty = SampleLedger::new();
    if empty.verify(&p1).err() == Some(SampleErr::Missing) {
        caught += 1;
    }
    // 9 同槽同身份但内容改一算子 → Tamper
    let mut tampered = p1.clone();
    if let Some(o) = tampered.ops.get_mut(0) {
        o.arg ^= 0x7f;
    }
    if l.verify(&tampered).err() == Some(SampleErr::Tamper) {
        caught += 1;
    }
    // 10 账文本字段数错 → Fields
    if parse_ledger("chain 1 1\n1,text,8\n").err() == Some(SampleErr::Fields) {
        caught += 1;
    }
    // 11 摘要非十六进制 → Hex
    if parse_ledger("chain 1 1\n1,text,8,zz\n").err() == Some(SampleErr::Hex) {
        caught += 1;
    }
    // 12 账链不符 → Checksum
    let full = populated();
    let good = encode_ledger(&full);
    let mut bad = format!("chain 0000000000000000 {}", full.len());
    if let Some(pos) = good.find('\n') {
        if let Some(rest) = good.get(pos..) {
            bad.push_str(rest);
        }
    }
    if parse_ledger(&bad).err() == Some(SampleErr::Checksum) {
        caught += 1;
    }
    // 13 账文本条数不符 → Count（3 行账头改成 2 但链保持真实值）
    let mut short = format!("chain {:016x} 2\n", full.chain());
    if let Some(pos) = good.find('\n') {
        if let Some(rest) = good.get(pos + 1..) {
            short.push_str(rest);
        }
    }
    if parse_ledger(&short).err() == Some(SampleErr::Count) {
        caught += 1;
    }
    (caught, total)
}

/// J1 判据：样本哈希账可复跑（13 条）。
pub fn run_unx50115_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50115");

    // J1-1 三样本入账齐备，账链非基值。
    let full = populated();
    let ok = full.complete()
        && full.len() == 3
        && full.missing_mask() == 0
        && full.chain() != FNV_BASIS;
    let det = format!("len={} chain={:016x}", full.len(), full.chain());
    s.add_dyn("50115-J1-三页入账", ok, &det);

    // J1-2 可复跑：同一组样本重建账 → 链与逐页摘要逐位一致。
    let again = populated();
    let mut same = full.chain() == again.chain() && full.len() == again.len();
    let mut slot = 1u8;
    while slot <= 3 {
        let a = full.record(slot);
        let b = again.record(slot);
        if a != b {
            same = false;
        }
        slot += 1;
    }
    let det = format!("c1={:016x} c2={:016x}", full.chain(), again.chain());
    s.add_dyn("50115-J1-账链复跑", same, &det);

    // J1-3 登记顺序无关：3,1,2 与 1,2,3 得到同一链（按槽序折叠）。
    let mut l312 = SampleLedger::new();
    let order = [3u8, 1, 2];
    let mut i = 0usize;
    while i < 3 {
        if let Some(sl) = order.get(i) {
            if let Ok(p) = sample_page(*sl) {
                let _ = l312.register(&p);
            }
        }
        i += 1;
    }
    let ok = l312.complete() && l312.chain() == full.chain();
    let det = format!("c312={:016x} c123={:016x}", l312.chain(), full.chain());
    s.add_dyn("50115-J1-顺序无关", ok, &det);

    // J1-4 重复槽拒绝且链/长度守恒。
    let mut l = populated();
    let before = l.chain();
    let p2 = match sample_page(2) {
        Ok(v) => v,
        Err(_) => sample_page(1).unwrap_or(SamplePage {
            slot: 1,
            class: SampleClass::Text,
            width: 1,
            height: 1,
            ops: Vec::new(),
        }),
    };
    let dup = l.register(&p2);
    let ok = dup.err() == Some(SampleErr::Dup) && l.chain() == before && l.len() == 3;
    s.add("50115-J1-重复槽守恒", ok, if ok { "" } else { "重复槽改了账" });

    // J1-5 篡改必抓：同槽改一个算子参数 → Tamper。
    let mut t = match sample_page(2) {
        Ok(v) => v,
        Err(_) => p2.clone(),
    };
    if let Some(o) = t.ops.get_mut(0) {
        o.arg ^= 1;
    }
    let ok = full.verify(&t).err() == Some(SampleErr::Tamper);
    s.add("50115-J1-篡改必抓", ok, if ok { "" } else { "篡改未检出" });

    // J1-6 缺页：只入两页 → 不齐备，缺页掩码精确指向槽 3。
    let mut partial = SampleLedger::new();
    let mut k = 1u8;
    while k <= 2 {
        if let Ok(p) = sample_page(k) {
            let _ = partial.register(&p);
        }
        k += 1;
    }
    let ok = !partial.complete() && partial.len() == 2 && partial.missing_mask() == 0b100;
    let det = format!("mask={:03b}", partial.missing_mask());
    s.add_dyn("50115-J1-缺页掩码", ok, &det);

    // J1-7 槽位/类别校验：槽 0/4 → Id，类不符 → Class。
    let bad_slot0 = SamplePage::new(0, SampleClass::Text, 10, 10, Vec::new());
    let bad_slot4 = SamplePage::new(4, SampleClass::Image, 10, 10, Vec::new());
    let mut one_op = Vec::new();
    if let Ok(o) = Op::new(2, 2) {
        one_op.push(o);
    }
    let bad_class = SamplePage::new(2, SampleClass::Text, 10, 10, one_op);
    let ok = bad_slot0.err() == Some(SampleErr::Id)
        && bad_slot4.err() == Some(SampleErr::Id)
        && bad_class.err() == Some(SampleErr::Class);
    s.add("50115-J1-槽类校验", ok, if ok { "" } else { "槽/类校验漏判" });

    // J1-8 算子边界：空算子 Empty，65 个 Range，码 0 Range。
    let mut many = Vec::with_capacity(OPS_CAP + 1);
    let mut n = 0usize;
    while n < OPS_CAP + 1 {
        many.push(Op {
            code: 3,
            arg: n as u16,
        });
        n += 1;
    }
    let empty = SamplePage::new(3, SampleClass::Image, 10, 10, Vec::new());
    let over = SamplePage::new(3, SampleClass::Image, 10, 10, many);
    let zero_op = Op::new(0, 7);
    let ok = empty.err() == Some(SampleErr::Empty)
        && over.err() == Some(SampleErr::Range)
        && zero_op.err() == Some(SampleErr::Range);
    s.add("50115-J1-算子边界", ok, if ok { "" } else { "算子边界漏判" });

    // J1-9 三页摘要互不相同（槽位/类别参与摘要，不会互相碰撞）。
    let d1 = full.record(1).map(|r| r.digest);
    let d2 = full.record(2).map(|r| r.digest);
    let d3 = full.record(3).map(|r| r.digest);
    let ok = match (d1, d2, d3) {
        (Some(a), Some(b), Some(c)) => a != b && b != c && a != c && a != 0 && b != 0 && c != 0,
        _ => false,
    };
    s.add("50115-J1-摘要互异", ok, if ok { "" } else { "三页摘要碰撞" });

    // J1-10 账文本往返：链与三槽记录逐位一致。
    let text = encode_ledger(&full);
    let back = parse_ledger(&text);
    let ok = match back {
        Ok(b) => {
            b.chain() == full.chain()
                && b.len() == full.len()
                && b.record(1) == full.record(1)
                && b.record(2) == full.record(2)
                && b.record(3) == full.record(3)
        }
        Err(_) => false,
    };
    let det = format!("chain={:016x}", full.chain());
    s.add_dyn("50115-J1-账本往返", ok, &det);

    // J1-11 复核三页全部通过（账链与页面摘要自洽）。
    let mut all = true;
    let mut s2 = 1u8;
    while s2 <= 3 {
        match sample_page(s2) {
            Ok(p) => {
                if full.verify(&p).is_err() {
                    all = false;
                }
            }
            Err(_) => all = false,
        }
        s2 += 1;
    }
    s.add("50115-J1-全量复核", all, if all { "" } else { "有页复核失败" });

    // J1-12 空账语义：链为基值、缺页掩码 0b111、complete=false。
    let empty = SampleLedger::new();
    let ok = empty.chain() == FNV_BASIS
        && empty.missing_mask() == 0b111
        && !empty.complete()
        && empty.is_empty();
    let det = format!("mask={:03b}", empty.missing_mask());
    s.add_dyn("50115-J1-空账语义", ok, &det);

    // J1-13 注入核 13/13。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50115-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50115_checks_green() {
        let set = run_unx50115_checks();
        assert!(set.all_passed(), "UNX-F50115 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50115_ledger_roundtrip() {
        let l = populated();
        let text = encode_ledger(&l);
        match parse_ledger(&text) {
            Ok(b) => assert_eq!(b.chain(), l.chain()),
            Err(_) => panic!("ledger rejected"),
        }
    }

    #[test]
    fn unx50115_order_independent() {
        let mut a = SampleLedger::new();
        let mut b = SampleLedger::new();
        if let (Ok(p1), Ok(p2), Ok(p3)) = (sample_page(1), sample_page(2), sample_page(3)) {
            assert!(a.register(&p1).is_ok());
            assert!(a.register(&p2).is_ok());
            assert!(a.register(&p3).is_ok());
            assert!(b.register(&p3).is_ok());
            assert!(b.register(&p1).is_ok());
            assert!(b.register(&p2).is_ok());
        }
        assert_eq!(a.chain(), b.chain());
    }
}
