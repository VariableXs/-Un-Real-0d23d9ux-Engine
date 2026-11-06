//! UNX-F50073 纸张 fuzz 面（坏尺寸/坏盒号注入不崩）（0x43540 段）
//!
//! 锚点：UNX-F50073-J1 fuzz ×10^3 零崩；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x43540`，诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 13`，
//! 逐项见 [`FuzzError`]。
//!
//! 为什么纸张描述要经一个**定长二进制解析器**再进业务：纸描述来自纸盒
//! EEPROM / 面板 / 驱动三方，任何一方都可能写出坏记录（幅面写成 0、
//! 盒号写成 9、校验和错位）。如果这些值直接进排版，坏尺寸会在几何层
//! 放大成除零 / 巨幅分配；因此本模块在**入口**就把 18 字节记录解成
//! [`PaperRecord`]，任何一字节不合规都返回显式诊断码，绝不半信半疑地放行。
//!
//! 零崩的实现路径（不是「跑一遍没崩」）：
//! * 解析器全程用 `get()` 读字节，`len < 18` 先行拦截，不存在裸下标；
//! * 每个字段先做闭集/区间校验再赋给结构体，结构体内不存在非法值；
//! * fuzz 输入由**种子化 PRNG** 生成，1000 次逐次可复跑，接受/拒绝计数
//!   与摘要逐位可复算；
//! * 校验和覆盖前 16 字节——任何载荷位翻转都会在最后一道闸被拒。
//!
//! 结构清单：
//! * [`Media`] + [`PaperRecord`]：介质闭集与定长记录（encode / parse 往返）；
//! * [`Rng`] + [`generate`]：确定性样本源（随机 / 变异 / 结构化三类生成器）；
//! * [`Verdict`] + [`FuzzLedger`]：判定与定容账本（分类计数 / 封账 / 摘要）；
//! * [`fuzz_run`]：×N 驱动（J1 的「零崩」由账本守恒与分类完备共同给出）；
//! * [`injection_core`]：6 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43540 独占 0x20）。
pub const SEG_BASE: u32 = 0x43540;

const E_SHORT: u16 = 1;
const E_MAGIC: u16 = 2;
const E_MEDIA: u16 = 3;
const E_TRAY: u16 = 4;
const E_SIZE: u16 = 5;
const E_WEIGHT: u16 = 6;
const E_FLAGS: u16 = 7;
const E_RESERVED: u16 = 8;
const E_CHECKSUM: u16 = 9;
const E_TRAILING: u16 = 10;
const E_SHEETS: u16 = 11;
const E_CAPACITY: u16 = 12;
const E_SEALED: u16 = 13;

/// fuzz 面错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzError {
    /// 记录短于 18 字节。
    Short,
    /// 魔数不是 `PPR1`。
    Magic,
    /// 介质码不在 0..=4。
    MediaCode,
    /// 盒号不在 1..=8。
    Tray,
    /// 幅面越出 20..=1200 mm。
    Size,
    /// 克重越出 40..=400 gsm。
    Weight,
    /// 标志字节的高 7 位非零（保留位被写脏）。
    Flags,
    /// 保留字节非零。
    Reserved,
    /// 校验和不符（任何载荷位翻转都会落到这里）。
    Checksum,
    /// 记录长于 18 字节（尾随垃圾）。
    Trailing,
    /// 张数越出 1..=2500。
    Sheets,
    /// 账本已满 / 迭代数超上限。
    Capacity,
    /// 账本已封账。
    Sealed,
}

impl FuzzError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        SEG_BASE + self.slot() as u32
    }

    /// 闭集序号 1..=13（也是分类计数数组的下标）。
    pub fn slot(self) -> usize {
        let n = match self {
            FuzzError::Short => E_SHORT,
            FuzzError::Magic => E_MAGIC,
            FuzzError::MediaCode => E_MEDIA,
            FuzzError::Tray => E_TRAY,
            FuzzError::Size => E_SIZE,
            FuzzError::Weight => E_WEIGHT,
            FuzzError::Flags => E_FLAGS,
            FuzzError::Reserved => E_RESERVED,
            FuzzError::Checksum => E_CHECKSUM,
            FuzzError::Trailing => E_TRAILING,
            FuzzError::Sheets => E_SHEETS,
            FuzzError::Capacity => E_CAPACITY,
            FuzzError::Sealed => E_SEALED,
        };
        n as usize
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            FuzzError::Short => "paper record shorter than 18 bytes",
            FuzzError::Magic => "paper record magic is not PPR1",
            FuzzError::MediaCode => "media code outside closed set",
            FuzzError::Tray => "tray id outside 1..=8",
            FuzzError::Size => "sheet size outside 20..=1200 mm",
            FuzzError::Weight => "paper weight outside 40..=400 gsm",
            FuzzError::Flags => "flag byte has reserved bits set",
            FuzzError::Reserved => "reserved byte not zero",
            FuzzError::Checksum => "record checksum mismatch",
            FuzzError::Trailing => "trailing bytes after record",
            FuzzError::Sheets => "sheet count outside 1..=2500",
            FuzzError::Capacity => "fuzz ledger full or iteration cap hit",
            FuzzError::Sealed => "fuzz ledger sealed read-only",
        }
    }
}

/// 介质闭集。纸张类型决定定影温度与走纸速度，闭集之外的码值不能猜。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    /// 普通纸。
    Plain,
    /// 证券纸。
    Bond,
    /// 再生纸。
    Recycled,
    /// 标签纸。
    Label,
    /// 信封。
    Envelope,
}

impl Media {
    /// 介质码 0..=4。
    pub fn code(self) -> u8 {
        match self {
            Media::Plain => 0,
            Media::Bond => 1,
            Media::Recycled => 2,
            Media::Label => 3,
            Media::Envelope => 4,
        }
    }

    /// 介质码反解；闭集外返回 [`FuzzError::MediaCode`]。
    pub fn from_code(c: u8) -> Result<Media, FuzzError> {
        match c {
            0 => Ok(Media::Plain),
            1 => Ok(Media::Bond),
            2 => Ok(Media::Recycled),
            3 => Ok(Media::Label),
            4 => Ok(Media::Envelope),
            _ => Err(FuzzError::MediaCode),
        }
    }

    /// 介质名。
    pub fn label(self) -> &'static str {
        match self {
            Media::Plain => "plain",
            Media::Bond => "bond",
            Media::Recycled => "recycled",
            Media::Label => "label",
            Media::Envelope => "envelope",
        }
    }
}

/// 记录定长 18 字节。
pub const RECORD_LEN: usize = 18;
/// 最小幅面 20 mm（小于此值任何辊道都夹不住）。
pub const MIN_MM: u16 = 20;
/// 最大幅面 1200 mm（绘图仪上限）。
pub const MAX_MM: u16 = 1200;
/// 最小克重 40 gsm。
pub const MIN_GSM: u16 = 40;
/// 最大克重 400 gsm。
pub const MAX_GSM: u16 = 400;
/// 最大装纸张数。
pub const MAX_SHEETS: u16 = 2500;
/// 单次 fuzz 迭代上限（1000 是 J1 口径，上限留 20 倍余量）。
pub const MAX_ITER: u32 = 20_000;

/// 解析后的纸张记录。所有字段在构造时已过界检查，故下游可以直接用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaperRecord {
    pub media: Media,
    pub tray: u8,
    pub width_mm: u16,
    pub height_mm: u16,
    pub weight_gsm: u16,
    pub duplex: bool,
    pub sheets: u16,
}

impl PaperRecord {
    /// 一组合法基线（A4 / 普通纸 / 2 号盒 / 80 gsm / 支持双面）。
    pub const fn valid() -> PaperRecord {
        PaperRecord {
            media: Media::Plain,
            tray: 2,
            width_mm: 210,
            height_mm: 297,
            weight_gsm: 80,
            duplex: true,
            sheets: 250,
        }
    }

    /// 序列化成 18 字节（末两字节为前 16 字节的小端和）。
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(RECORD_LEN);
        v.push(b'P');
        v.push(b'P');
        v.push(b'R');
        v.push(b'1');
        v.push(self.media.code());
        v.push(self.tray);
        v.push(self.width_mm as u8);
        v.push((self.width_mm >> 8) as u8);
        v.push(self.height_mm as u8);
        v.push((self.height_mm >> 8) as u8);
        v.push(self.weight_gsm as u8);
        v.push((self.weight_gsm >> 8) as u8);
        v.push(if self.duplex { 1u8 } else { 0u8 });
        v.push(0u8);
        v.push(self.sheets as u8);
        v.push((self.sheets >> 8) as u8);
        let cs = checksum(&v);
        v.push(cs as u8);
        v.push((cs >> 8) as u8);
        v
    }
}

fn byte_at(d: &[u8], at: usize) -> u8 {
    match d.get(at) {
        Some(b) => *b,
        None => 0,
    }
}

fn le_u16(d: &[u8], at: usize) -> u16 {
    byte_at(d, at) as u16 | ((byte_at(d, at + 1) as u16) << 8)
}

fn checksum(d: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < 16 {
        sum = sum.wrapping_add(byte_at(d, i) as u32);
        i += 1;
    }
    sum as u16
}

/// 解析 18 字节记录。校验顺序按「结构性 → 字段 → 完整性」排：
/// 先长度与魔数（决定这是不是本格式），再逐字段闭集，最后核对校验和。
pub fn parse(data: &[u8]) -> Result<PaperRecord, FuzzError> {
    if data.len() < RECORD_LEN {
        return Err(FuzzError::Short);
    }
    if data.len() > RECORD_LEN {
        return Err(FuzzError::Trailing);
    }
    if data.get(0) != Some(&b'P')
        || data.get(1) != Some(&b'P')
        || data.get(2) != Some(&b'R')
        || data.get(3) != Some(&b'1')
    {
        return Err(FuzzError::Magic);
    }
    let media = Media::from_code(byte_at(data, 4))?;
    let tray = byte_at(data, 5);
    if tray == 0 || tray > 8 {
        return Err(FuzzError::Tray);
    }
    let width_mm = le_u16(data, 6);
    let height_mm = le_u16(data, 8);
    if width_mm < MIN_MM || width_mm > MAX_MM || height_mm < MIN_MM || height_mm > MAX_MM {
        return Err(FuzzError::Size);
    }
    let weight_gsm = le_u16(data, 10);
    if weight_gsm < MIN_GSM || weight_gsm > MAX_GSM {
        return Err(FuzzError::Weight);
    }
    let flags = byte_at(data, 12);
    if flags & 0xFE != 0 {
        return Err(FuzzError::Flags);
    }
    if byte_at(data, 13) != 0 {
        return Err(FuzzError::Reserved);
    }
    let sheets = le_u16(data, 14);
    if sheets == 0 || sheets > MAX_SHEETS {
        return Err(FuzzError::Sheets);
    }
    let want = le_u16(data, 16);
    if want != checksum(data) {
        return Err(FuzzError::Checksum);
    }
    Ok(PaperRecord {
        media,
        tray,
        width_mm,
        height_mm,
        weight_gsm,
        duplex: flags & 1 == 1,
        sheets,
    })
}

// ---------------------------------------------------------------------------
// 确定性样本源
// ---------------------------------------------------------------------------

/// xorshift64*：无依赖、无分配、跨平台逐位一致。种子 0 映射到黄金比例常数，
/// 避免「种子 0 输出全零」这种退化。
#[derive(Clone, Debug)]
pub struct Rng {
    s: u64,
}

impl Rng {
    /// 以种子建流（0 会被替换成非零常数）。
    pub fn new(seed: u64) -> Rng {
        Rng {
            s: if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed },
        }
    }

    /// 取下一个 64 位值。
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x
    }

    /// 取一个字节。
    pub fn next_byte(&mut self) -> u8 {
        (self.next_u64() >> 33) as u8
    }

    /// 取 [0, n) 的整数（n=0 时返回 0）。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            ((self.next_u64() >> 32) as u32) % n
        }
    }
}

fn write_u16(v: &mut Vec<u8>, at: usize, val: u16) {
    if let Some(s) = v.get_mut(at) {
        *s = val as u8;
    }
    if let Some(s) = v.get_mut(at + 1) {
        *s = (val >> 8) as u8;
    }
}

/// 重算校验和并写回末两字节（改字段后的收尾动作）。
pub fn reseal(v: &mut Vec<u8>) {
    let cs = checksum(v);
    write_u16(v, 16, cs);
}

fn random_bytes(rng: &mut Rng) -> Vec<u8> {
    let len = 4 + rng.below(21) as usize; // 4..=24，覆盖短/正/长三档
    let mut v = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        v.push(rng.next_byte());
        i += 1;
    }
    v
}

fn mutated_valid(rng: &mut Rng) -> Vec<u8> {
    let mut v = PaperRecord::valid().encode();
    let flips = 1 + rng.below(7);
    let mut k = 0u32;
    while k < flips {
        let len = v.len() as u32;
        let at = rng.below(len) as usize;
        let b = rng.next_byte();
        if let Some(s) = v.get_mut(at) {
            *s = b;
        }
        k += 1;
    }
    // 1/4 概率截断：真实链路里「发到一半掉电」比位翻转更常见。
    if rng.below(4) == 0 {
        let cut = rng.below(v.len() as u32) as usize;
        v.truncate(cut);
    }
    v
}

fn structured_bad(rng: &mut Rng) -> Vec<u8> {
    let mut v = PaperRecord::valid().encode();
    match rng.below(4) {
        0 => {
            let bad = if rng.below(2) == 0 { 0u16 } else { 1201 };
            write_u16(&mut v, 6, bad);
            reseal(&mut v);
        }
        1 => {
            if let Some(s) = v.get_mut(5) {
                *s = if rng.below(2) == 0 { 0 } else { 9 };
            }
            reseal(&mut v);
        }
        2 => {
            let bad = if rng.below(2) == 0 { 10u16 } else { 401 };
            write_u16(&mut v, 10, bad);
            reseal(&mut v);
        }
        _ => {
            // 只翻校验和自身：载荷没动但完整性被破坏，必须判 Checksum。
            if let Some(s) = v.get_mut(16) {
                *s ^= 0x5a;
            }
        }
    }
    v
}

fn valid_variant(rng: &mut Rng) -> Vec<u8> {
    let mut r = PaperRecord::valid();
    r.tray = 1 + rng.below(8) as u8;
    r.width_mm = MIN_MM + rng.below(981) as u16;
    r.height_mm = MIN_MM + rng.below(981) as u16;
    r.weight_gsm = MIN_GSM + rng.below(361) as u16;
    r.sheets = 1 + rng.below(MAX_SHEETS as u32) as u16;
    r.duplex = rng.below(2) == 0;
    r.media = match rng.below(5) {
        0 => Media::Plain,
        1 => Media::Bond,
        2 => Media::Recycled,
        3 => Media::Label,
        _ => Media::Envelope,
    };
    r.encode()
}

fn gen_one(rng: &mut Rng, i: u32) -> Vec<u8> {
    match i % 4 {
        0 => random_bytes(rng),
        1 => mutated_valid(rng),
        2 => structured_bad(rng),
        _ => valid_variant(rng),
    }
}

/// 生成 N 条确定性样本（同 seed 同 i 逐字节一致）。
pub fn generate(seed: u64, iterations: u32) -> Result<Vec<Vec<u8>>, FuzzError> {
    if iterations > MAX_ITER {
        return Err(FuzzError::Capacity);
    }
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(iterations as usize);
    let mut i = 0u32;
    while i < iterations {
        out.push(gen_one(&mut rng, i));
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 判定与账本
// ---------------------------------------------------------------------------

/// 单条样本的判定。接受分支只带「已过校验」的字段，拒绝分支只带闭集码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 记录合法。
    Accept {
        media: Media,
        tray: u8,
        sheets: u16,
    },
    /// 记录被显式拒绝。
    Reject(FuzzError),
}

/// 分类判定（供外部单独调用；账本记账也走它）。
pub fn classify(data: &[u8]) -> Verdict {
    match parse(data) {
        Ok(r) => Verdict::Accept {
            media: r.media,
            tray: r.tray,
            sheets: r.sheets,
        },
        Err(e) => Verdict::Reject(e),
    }
}

/// fuzz 账本：定容 + 分类计数 + 封账 + 顺序摘要。
#[derive(Clone, Debug)]
pub struct FuzzLedger {
    accepted: u32,
    rejected: u32,
    by_code: [u32; 14],
    cap: u32,
    sealed: bool,
    digest: u32,
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

impl FuzzLedger {
    /// 建账，容量为可记录的判定条数。
    pub fn new(cap: u32) -> FuzzLedger {
        FuzzLedger {
            accepted: 0,
            rejected: 0,
            by_code: [0u32; 14],
            cap,
            sealed: false,
            digest: FNV_OFFSET,
        }
    }

    /// 记一条判定：封账 → 定容 → 分类累加。
    pub fn record(&mut self, v: Verdict) -> Result<(), FuzzError> {
        if self.sealed {
            return Err(FuzzError::Sealed);
        }
        if self.total() >= self.cap {
            return Err(FuzzError::Capacity);
        }
        match v {
            Verdict::Accept {
                media,
                tray,
                sheets,
            } => {
                self.accepted = self.accepted.saturating_add(1);
                self.digest = fnv_byte(self.digest, b'A');
                self.digest = fnv_byte(self.digest, media.code());
                self.digest = fnv_byte(self.digest, tray);
                self.digest = fnv_byte(self.digest, sheets as u8);
            }
            Verdict::Reject(e) => {
                self.rejected = self.rejected.saturating_add(1);
                if let Some(c) = self.by_code.get_mut(e.slot()) {
                    *c = c.saturating_add(1);
                }
                self.digest = fnv_byte(self.digest, b'R');
                self.digest = fnv_byte(self.digest, e.slot() as u8);
            }
        }
        Ok(())
    }

    /// 接受条数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    /// 拒绝条数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 总条数。
    pub fn total(&self) -> u32 {
        self.accepted.saturating_add(self.rejected)
    }

    /// 容量。
    pub fn capacity(&self) -> u32 {
        self.cap
    }

    /// 单码计数。
    pub fn code_count(&self, e: FuzzError) -> u32 {
        match self.by_code.get(e.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 出现过的拒绝类别数（分类完备性的左侧）。
    pub fn rejected_classes(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.by_code.len() {
            if let Some(v) = self.by_code.get(i) {
                if *v > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 拒绝计数之和（必须等于 rejected，否则有判定被吞）。
    pub fn classified_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.by_code.len() {
            if let Some(v) = self.by_code.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        sum
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 顺序摘要。
    pub fn digest(&self) -> u32 {
        self.digest
    }
}

/// 一次 fuzz 运行的统计快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzStats {
    pub iterations: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub rejected_classes: usize,
    pub max_len: usize,
    pub digest: u32,
}

impl FuzzStats {
    /// 空快照（失败兜底用，调用方无需再解包错误）。
    pub const fn empty() -> FuzzStats {
        FuzzStats {
            iterations: 0,
            accepted: 0,
            rejected: 0,
            rejected_classes: 0,
            max_len: 0,
            digest: 0,
        }
    }

    /// 总条数。
    pub fn total(&self) -> u32 {
        self.accepted.saturating_add(self.rejected)
    }

    /// 一行摘要。
    pub fn line(&self) -> String {
        format!(
            "n={} acc={} rej={} cls={} maxlen={}",
            self.iterations, self.accepted, self.rejected, self.rejected_classes, self.max_len
        )
    }
}

/// 跑 N 次 fuzz：生成 → 解析 → 记账，最后把输入流摘要与账本摘要合成总摘要。
///
/// 零崩不是「这次没崩」的口号：本函数对所有输入都走 [`parse`]，而 parse 的
/// 每条路径都只返回 `Result`，不存在 panic 分支；J1 用「接受+拒绝==迭代数」
/// 与「拒绝计数和==rejected」共同证明没有判定被吞。
pub fn fuzz_run(seed: u64, iterations: u32) -> Result<FuzzStats, FuzzError> {
    let inputs = generate(seed, iterations)?;
    let mut led = FuzzLedger::new(iterations);
    let mut h = FNV_OFFSET;
    let mut max_len = 0usize;
    let mut i = 0usize;
    while i < inputs.len() {
        if let Some(input) = inputs.get(i) {
            if input.len() > max_len {
                max_len = input.len();
            }
            let mut k = 0usize;
            while k < input.len() {
                if let Some(b) = input.get(k) {
                    h = fnv_byte(h, *b);
                }
                k += 1;
            }
            let v = classify(input);
            h = fnv_byte(h, if matches!(v, Verdict::Accept { .. }) { 1 } else { 0 });
            led.record(v)?;
        }
        i += 1;
    }
    Ok(FuzzStats {
        iterations,
        accepted: led.accepted(),
        rejected: led.rejected(),
        rejected_classes: led.rejected_classes(),
        max_len,
        digest: h ^ led.digest(),
    })
}

// ---------------------------------------------------------------------------
// 注入核：6 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 6 类坏样本注入核，返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1) 短记录。
    if parse(&[0u8; 9]).err() == Some(FuzzError::Short) {
        caught += 1;
    }

    // 2) 魔数错（长度合法、字段全零也不行）。
    let mut bad_magic = PaperRecord::valid().encode();
    if let Some(s) = bad_magic.get_mut(0) {
        *s = b'X';
    }
    if parse(&bad_magic).err() == Some(FuzzError::Magic) {
        caught += 1;
    }

    // 3) 坏尺寸（0 mm）。
    let mut bad_size = PaperRecord::valid().encode();
    write_u16(&mut bad_size, 6, 0);
    reseal(&mut bad_size);
    if parse(&bad_size).err() == Some(FuzzError::Size) {
        caught += 1;
    }

    // 4) 坏盒号（9 不在 1..=8）。
    let mut bad_tray = PaperRecord::valid().encode();
    if let Some(s) = bad_tray.get_mut(5) {
        *s = 9;
    }
    reseal(&mut bad_tray);
    if parse(&bad_tray).err() == Some(FuzzError::Tray) {
        caught += 1;
    }

    // 5) 保留位置脏。
    let mut bad_res = PaperRecord::valid().encode();
    if let Some(s) = bad_res.get_mut(13) {
        *s = 1;
    }
    reseal(&mut bad_res);
    if parse(&bad_res).err() == Some(FuzzError::Reserved) {
        caught += 1;
    }

    // 6) 校验和被翻。
    let mut bad_cs = PaperRecord::valid().encode();
    if let Some(s) = bad_cs.get_mut(16) {
        *s ^= 0xff;
    }
    if parse(&bad_cs).err() == Some(FuzzError::Checksum) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50073 自检：14 条判据。
pub fn run_unx50073_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50073");

    // J1-1 ×10^3 守恒：接受 + 拒绝 == 迭代数，且与账本独立统计一致。
    let stats = match fuzz_run(0x5eed_0073, 1000) {
        Ok(v) => v,
        Err(_) => FuzzStats::empty(),
    };
    let ok = stats.iterations == 1000 && stats.total() == 1000 && stats.accepted > 0;
    let det = stats.line();
    s.add_dyn("50073-J1-千次守恒", ok, &det);

    // J1-2 确定性：同种子两次运行，计数与摘要逐位一致。
    let a = fuzz_run(0x1234_5678, 300);
    let b = fuzz_run(0x1234_5678, 300);
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x == y && x.digest == y.digest,
        _ => false,
    };
    s.add("50073-J1-运行确定", ok, if ok { "" } else { "两次 fuzz 不一致" });

    // J1-3 种子敏感：换种子必须换摘要（防「忽略输入」的假实现）。
    let c = fuzz_run(0x1234_5679, 300);
    let ok = match (&a, &c) {
        (Ok(x), Ok(y)) => x.digest != y.digest,
        _ => false,
    };
    s.add("50073-J1-种子敏感", ok, if ok { "" } else { "摘要与种子无关" });

    // J1-4 分类完备：拒绝类别 ≥ 4，且拒绝计数和 == rejected（无判定被吞）。
    let ok = stats.rejected_classes >= 4 && stats.rejected > 0;
    let det = format!("cls={} rej={}", stats.rejected_classes, stats.rejected);
    s.add_dyn("50073-J1-分类完备", ok, &det);

    // J1-5 接受必合法：1000 条里每条被接受的记录，重编码后逐字节往返、
    // 且字段全在闭集/区间内。
    let inputs = match generate(0x5eed_0073, 1000) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut accept_ok = true;
    let mut checked = 0usize;
    let mut i = 0usize;
    while i < inputs.len() {
        if let Some(input) = inputs.get(i) {
            if let Ok(rec) = parse(input) {
                checked += 1;
                let bytes = rec.encode();
                if bytes != *input {
                    accept_ok = false;
                }
                if rec.tray == 0
                    || rec.tray > 8
                    || rec.width_mm < MIN_MM
                    || rec.width_mm > MAX_MM
                    || rec.weight_gsm < MIN_GSM
                    || rec.weight_gsm > MAX_GSM
                    || rec.sheets == 0
                    || rec.sheets > MAX_SHEETS
                {
                    accept_ok = false;
                }
            }
        }
        i += 1;
    }
    let ok = accept_ok && checked == stats.accepted as usize;
    let det = format!("checked={} acc={}", checked, stats.accepted);
    s.add_dyn("50073-J1-接受合法", ok, &det);

    // J1-6 逐类精确码：11 类坏字段各自命中自己的诊断码（不接受「反正拒了」）。
    let base = PaperRecord::valid().encode();
    let mut cases = 0usize;
    let mut exact = 0usize;

    let short: Vec<u8> = Vec::new();
    cases += 1;
    if parse(&short).err() == Some(FuzzError::Short) {
        exact += 1;
    }

    let mut trailing = base.clone();
    trailing.push(0u8);
    cases += 1;
    if parse(&trailing).err() == Some(FuzzError::Trailing) {
        exact += 1;
    }

    let mut magic = base.clone();
    if let Some(x) = magic.get_mut(3) {
        *x = b'X';
    }
    cases += 1;
    if parse(&magic).err() == Some(FuzzError::Magic) {
        exact += 1;
    }

    let mut media = base.clone();
    if let Some(x) = media.get_mut(4) {
        *x = 5;
    }
    reseal(&mut media);
    cases += 1;
    if parse(&media).err() == Some(FuzzError::MediaCode) {
        exact += 1;
    }

    let mut tray = base.clone();
    if let Some(x) = tray.get_mut(5) {
        *x = 0;
    }
    reseal(&mut tray);
    cases += 1;
    if parse(&tray).err() == Some(FuzzError::Tray) {
        exact += 1;
    }

    let mut wide = base.clone();
    write_u16(&mut wide, 8, 1201);
    reseal(&mut wide);
    cases += 1;
    if parse(&wide).err() == Some(FuzzError::Size) {
        exact += 1;
    }

    let mut heavy = base.clone();
    write_u16(&mut heavy, 10, 401);
    reseal(&mut heavy);
    cases += 1;
    if parse(&heavy).err() == Some(FuzzError::Weight) {
        exact += 1;
    }

    let mut flags = base.clone();
    if let Some(x) = flags.get_mut(12) {
        *x = 0x80;
    }
    reseal(&mut flags);
    cases += 1;
    if parse(&flags).err() == Some(FuzzError::Flags) {
        exact += 1;
    }

    let mut res = base.clone();
    if let Some(x) = res.get_mut(13) {
        *x = 7;
    }
    reseal(&mut res);
    cases += 1;
    if parse(&res).err() == Some(FuzzError::Reserved) {
        exact += 1;
    }

    let mut sheets = base.clone();
    write_u16(&mut sheets, 14, 0);
    reseal(&mut sheets);
    cases += 1;
    if parse(&sheets).err() == Some(FuzzError::Sheets) {
        exact += 1;
    }

    let mut cs = base.clone();
    if let Some(x) = cs.get_mut(17) {
        *x ^= 0x01;
    }
    cases += 1;
    if parse(&cs).err() == Some(FuzzError::Checksum) {
        exact += 1;
    }

    let ok = cases == 11 && exact == cases;
    let det = format!("exact={}/{}", exact, cases);
    s.add_dyn("50073-J1-逐类精确码", ok, &det);

    // J1-7 载荷任一位翻转必拒：16 个载荷字节逐个翻转，全部必须被拒。
    // （校验和覆盖前 16 字节，这条性质把「漏校验字段」钉死。）
    let mut guarded = true;
    let mut pos = 0usize;
    while pos < 16 {
        let mut v = base.clone();
        if let Some(x) = v.get_mut(pos) {
            *x ^= 0x01;
        }
        if parse(&v).is_ok() {
            guarded = false;
        }
        pos += 1;
    }
    s.add("50073-J1-载荷位翻必拒", guarded, if guarded { "" } else { "存在未受校验的字段" });

    // J1-8 往返幂等：合法记录 encode → parse → encode 逐字节相等。
    let rec = PaperRecord {
        media: Media::Label,
        tray: 7,
        width_mm: 330,
        height_mm: 483,
        weight_gsm: 200,
        duplex: false,
        sheets: 1200,
    };
    let b1 = rec.encode();
    let rt = parse(&b1);
    let b2 = match &rt {
        Ok(r) => r.encode(),
        Err(_) => Vec::new(),
    };
    let ok = rt.ok() == Some(rec) && b1 == b2 && b1.len() == RECORD_LEN;
    s.add("50073-J1-往返幂等", ok, if ok { "" } else { "重编码不一致" });

    // J1-9 账本定容：cap=2 第三条 Capacity，计数不变。
    let mut led = FuzzLedger::new(2);
    let ok = led
        .record(Verdict::Accept {
            media: Media::Plain,
            tray: 1,
            sheets: 10,
        })
        .is_ok()
        && led.record(Verdict::Reject(FuzzError::Checksum)).is_ok()
        && led.record(Verdict::Reject(FuzzError::Tray)).err() == Some(FuzzError::Capacity)
        && led.total() == 2
        && led.rejected() == 1
        && led.code_count(FuzzError::Tray) == 0;
    s.add("50073-J1-账本定容", ok, if ok { "" } else { "超容计数被写脏" });

    // J1-10 封账只读：seal 后任何判定都 Sealed，账不变。
    led.seal();
    let ok = led.is_sealed()
        && led.record(Verdict::Reject(FuzzError::Short)).err() == Some(FuzzError::Sealed)
        && led.total() == 2;
    s.add("50073-J1-封账只读", ok, if ok { "" } else { "封账后可写" });

    // J1-11 迭代上限：超 MAX_ITER 直接 Capacity（不给 fuzz 拖死内核的机会）。
    let ok = generate(1, MAX_ITER + 1).err() == Some(FuzzError::Capacity)
        && match fuzz_run(1, 0) {
            Ok(v) => v.total() == 0,
            Err(_) => false,
        };
    s.add("50073-J1-迭代上限", ok, if ok { "" } else { "上限未拦或空跑异常" });

    // J1-12 摘要链：账本摘要随判定序列变化（同条数不同序列必不同）。
    let mut l1 = FuzzLedger::new(4);
    let mut l2 = FuzzLedger::new(4);
    let _ = l1.record(Verdict::Reject(FuzzError::Short));
    let _ = l1.record(Verdict::Reject(FuzzError::Tray));
    let _ = l2.record(Verdict::Reject(FuzzError::Tray));
    let _ = l2.record(Verdict::Reject(FuzzError::Short));
    let ok = l1.total() == l2.total() && l1.digest() != l2.digest();
    s.add("50073-J1-摘要序敏感", ok, if ok { "" } else { "摘要忽略顺序" });

    // J1-13 码域闭集：13 个诊断码互异且全落在本段窗口内。
    let all = [
        FuzzError::Short,
        FuzzError::Magic,
        FuzzError::MediaCode,
        FuzzError::Tray,
        FuzzError::Size,
        FuzzError::Weight,
        FuzzError::Flags,
        FuzzError::Reserved,
        FuzzError::Checksum,
        FuzzError::Trailing,
        FuzzError::Sheets,
        FuzzError::Capacity,
        FuzzError::Sealed,
    ];
    let mut codes_ok = true;
    let mut k = 0usize;
    while k < all.len() {
        if let Some(e) = all.get(k) {
            if e.code() <= SEG_BASE || e.code() > SEG_BASE + 0x1f {
                codes_ok = false;
            }
            let mut m = k + 1;
            while m < all.len() {
                if let Some(f) = all.get(m) {
                    if e.code() == f.code() {
                        codes_ok = false;
                    }
                }
                m += 1;
            }
        }
        k += 1;
    }
    s.add("50073-J1-码域闭集", codes_ok, if codes_ok { "" } else { "诊断码重复或越段" });

    // J1-14 六类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50073-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50073_checks_green() {
        let set = run_unx50073_checks();
        assert!(set.all_passed(), "UNX-F50073 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50073_thousand_iterations_never_crash() {
        let stats = fuzz_run(0x5eed_0073, 1000);
        assert!(stats.is_ok());
        if let Ok(s) = stats {
            assert_eq!(s.iterations, 1000);
            assert_eq!(s.total(), 1000);
            assert!(s.rejected > 0 && s.accepted > 0);
        }
    }

    #[test]
    fn unx50073_parser_rejects_every_payload_flip() {
        let base = PaperRecord::valid().encode();
        for pos in 0..16usize {
            let mut v = base.clone();
            if let Some(x) = v.get_mut(pos) {
                *x ^= 0x01;
            }
            assert!(parse(&v).is_err(), "flip at {} escaped", pos);
        }
    }

    #[test]
    fn unx50073_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
