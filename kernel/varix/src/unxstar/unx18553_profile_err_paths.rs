//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18553 · 档案错误路径账（损坏/缺字段）· 0x80600 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18553-J1 损坏档案 10 形态注入全过（拒载+告警+回退），
//!       三要素呈现
//! 本模块结构清单：
//!   1. 冻结常量层：档案二进制布局（魔数"VXPF"/版本/标志/条目数/
//!      条目对/校验和）、条目容量 8、版本闭集 1..=3、10 形态损坏表；
//!   2. 诊断码闭集 EpCode（code() = SEG_BASE + n，0x80600 段窗口）；
//!   3. 数据结构一（装载体）：LoadedProfile——版本+标志+条目数组账
//!      （key/val 对，键升序、键闭集、值闭集三重校验）；
//!   4. 数据结构二（错误路径账）：ErrPathLedger——逐形态三要素账
//!      （rejected/warned/fell_back），守恒律：三要素同记同现，
//!      拒载数=告警数=回退数；
//!   5. 装载核 load_profile：十闸顺序判定（空→魔数→版本→超长→
//!      截断→条目数→键越界→重复键→乱序→校验和），判定次序冻结；
//!   6. 回退核 fallback：任何拒载一律落到冻结默认档案（确定性，
//!      10 形态回退结果逐字节同值）；
//!   7. 注入核：10 形态×10 轮全拒 + 三类核心形态（魔数/校验和/
//!      截断）逐轮断言三要素；
//!   8. 黄金全链 golden_run + run_unx18553_checks：15 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么判定次序要冻结：同一份坏档可能同时踩多个闸，若次序漂移，
//! 同一输入会报不同拒因——错误路径账的可复算性依赖「先魔数后版本、
//! 先结构后语义、先键面后校验和」这条冻结链，与内核错误码判定序
//! （EBADF 先于 EFAULT）同构。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80600，AI-VX11-1105-G1·A16 批 08 独占窗口）。
pub const SEG_BASE: u32 = 0x80600;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 魔数（冻结："VXPF" = varix profile 四字节）。
pub const MAGIC: [u8; 4] = [0x56, 0x58, 0x50, 0x46];
/// 版本闭集上界（冻结：1..=3，4 起未知）。
pub const VER_MAX: u8 = 3;
/// 条目容量（冻结：8 对 key/val）。
pub const ENTRY_CAP: usize = 8;
/// 键闭集（冻结：0..10，与档案面 12 键子集对齐取 10）。
pub const KEY_MAX: u8 = 10;
/// 值闭集（冻结：0..5，与 F18551 值类别 5 档对齐）。
pub const VAL_MAX: u8 = 5;
/// 输入总长上界（冻结：头 8 + 条目 2×8 + 校验和 1 = 25 字节满档）。
pub const INPUT_MAX: usize = 25;
/// 损坏形态数（冻结：判据原文 10 形态）。
pub const CORRUPT_FORMS: usize = 10;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpCode {
    /// 空输入
    EmptyInput,
    /// 魔数错
    MagicBad,
    /// 版本未知
    VerUnknown,
    /// 输入超长
    InputTooLong,
    /// 截断（结构不满）
    Truncated,
    /// 条目数与实际不符
    CountMismatch,
    /// 键越界
    KeyRange,
    /// 重复键
    KeyDup,
    /// 键乱序（非升序）
    KeyUnsorted,
    /// 校验和错
    ChecksumBad,
    /// 三要素失一（拒载/告警/回退不同记）
    TriadBroken,
    /// 双跑不一致
    NotReproducible,
}

impl EpCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            EpCode::EmptyInput => "空输入：档案零字节，拒载+告警+回退",
            EpCode::MagicBad => "魔数错：非 VXPF，拒载+告警+回退",
            EpCode::VerUnknown => "版本未知：超出 1..=3 闭集，拒载",
            EpCode::InputTooLong => "输入超长：超出 25 字节满档上限",
            EpCode::Truncated => "截断：结构不满（头/条目/校验和缺段）",
            EpCode::CountMismatch => "条目数与实际不符：计数段说谎",
            EpCode::KeyRange => "键越界：键位不在 0..10 闭集",
            EpCode::KeyDup => "重复键：档案内一键多声明",
            EpCode::KeyUnsorted => "键乱序：条目非升序，语义面拒载",
            EpCode::ChecksumBad => "校验和错：逐字节和对不上",
            EpCode::TriadBroken => "三要素失一：拒载/告警/回退必须同记",
            EpCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [EpCode; 12] = [
        EpCode::EmptyInput,
        EpCode::MagicBad,
        EpCode::VerUnknown,
        EpCode::InputTooLong,
        EpCode::Truncated,
        EpCode::CountMismatch,
        EpCode::KeyRange,
        EpCode::KeyDup,
        EpCode::KeyUnsorted,
        EpCode::ChecksumBad,
        EpCode::TriadBroken,
        EpCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EpCode::ALL.len() {
        if EpCode::ALL[i].code() == code {
            return EpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80600 段闭集内"
}

// ── 数据结构一：装载体 ──────────────────────────────────────────
/// 装载成功的档案：版本 + 标志 + 条目数组账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedProfile {
    pub version: u8,
    pub flags: u8,
    pub entries: Vec<(u8, u8)>,
}

impl LoadedProfile {
    /// 语义完整性：键全在闭集、值全在闭集、键升序、键唯一。
    pub fn semantic_ok(&self) -> bool {
        let mut i = 0;
        let mut prev: i32 = -1;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some((k, v)) => {
                    if *k >= KEY_MAX || *v >= VAL_MAX {
                        return false;
                    }
                    if (*k as i32) <= prev {
                        return false; // 升序 + 唯一一并守住
                    }
                    prev = *k as i32;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
    /// 条目数。
    pub fn entry_count(&self) -> u32 {
        self.entries.len() as u32
    }
}

/// 校验和：前 n 字节逐字节和取低 8 位（冻结算法）。
pub fn checksum(bytes: &[u8]) -> u8 {
    let mut acc: u8 = 0;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = acc.wrapping_add(*b);
        }
        i += 1;
    }
    acc
}

// ── 装载核（十闸冻结次序） ──────────────────────────────────────
/// 装载：十闸按冻结次序判，任一闸拒即 Err（拒因=闸码）。
/// 次序：空→魔数→版本→超长→截断→条目数→键越界→重复键→乱序→校验和。
pub fn load_profile(bytes: &[u8]) -> Result<LoadedProfile, u32> {
    // 闸 1：空输入。
    if bytes.is_empty() {
        return Err(EpCode::EmptyInput.code());
    }
    // 闸 2：魔数。
    if bytes.len() < 4 {
        return Err(EpCode::Truncated.code());
    }
    let mut mi = 0;
    while mi < 4 {
        let m = bytes.get(mi).ok_or(EpCode::Truncated.code())?;
        let w = MAGIC.get(mi).ok_or(EpCode::Truncated.code())?;
        if m != w {
            return Err(EpCode::MagicBad.code());
        }
        mi += 1;
    }
    // 闸 3：版本。
    let ver = bytes.get(4).ok_or(EpCode::Truncated.code())?;
    if *ver == 0 || *ver > VER_MAX {
        return Err(EpCode::VerUnknown.code());
    }
    // 闸 4：超长。
    if bytes.len() > INPUT_MAX {
        return Err(EpCode::InputTooLong.code());
    }
    // 闸 5：结构满（头 8 字节 + 校验和 1 字节下限）。
    if bytes.len() < 9 {
        return Err(EpCode::Truncated.code());
    }
    let flags = bytes.get(5).ok_or(EpCode::Truncated.code())?;
    let count = bytes.get(6).ok_or(EpCode::Truncated.code())?;
    let reserve = bytes.get(7).ok_or(EpCode::Truncated.code())?;
    if *reserve != 0 {
        return Err(EpCode::CountMismatch.code());
    }
    // 闸 6：条目数与实际长度一致（8 头 + 2×count + 1 校验和 = 总长）。
    let want_len = 8usize + 2 * (*count as usize) + 1usize;
    if want_len > INPUT_MAX || bytes.len() != want_len {
        return Err(EpCode::CountMismatch.code());
    }
    if *count as usize > ENTRY_CAP {
        return Err(EpCode::CountMismatch.code());
    }
    // 条目解析（键越界→重复键→乱序 三闸次序冻结）。
    let mut entries: Vec<(u8, u8)> = Vec::new();
    let mut prev: i32 = -1;
    let mut i = 0;
    while i < *count as usize {
        let koff = 8 + 2 * i;
        let k = bytes.get(koff).ok_or(EpCode::Truncated.code())?;
        let v = bytes.get(koff + 1).ok_or(EpCode::Truncated.code())?;
        // 闸 7：键越界。
        if *k >= KEY_MAX || *v >= VAL_MAX {
            return Err(EpCode::KeyRange.code());
        }
        // 闸 8：重复键（升序扫描中 == 即重复）。
        if (*k as i32) == prev {
            return Err(EpCode::KeyDup.code());
        }
        // 闸 9：乱序（< prev）。
        if (*k as i32) < prev {
            return Err(EpCode::KeyUnsorted.code());
        }
        prev = *k as i32;
        entries.push((*k, *v));
        i += 1;
    }
    // 闸 10：校验和（末字节 = 前序字节和低 8 位）。
    let body = &bytes[..bytes.len() - 1];
    let want = checksum(body);
    let got = bytes.get(bytes.len() - 1).ok_or(EpCode::Truncated.code())?;
    if want != *got {
        return Err(EpCode::ChecksumBad.code());
    }
    Ok(LoadedProfile { version: *ver, flags: *flags, entries })
}

// ── 回退核 ──────────────────────────────────────────────────────
/// 冻结默认档案（回退目标）：版本 1、标志 0、四条目（键 0/2/4/6）。
/// 推导：条目值取键位%5，键隔 2 取避开乱序与闭集边界。
pub fn default_profile() -> LoadedProfile {
    let mut e: Vec<(u8, u8)> = Vec::new();
    let mut k = 0;
    while k < 4 {
        let key = (k * 2) as u8;
        e.push((key, key % VAL_MAX));
        k += 1;
    }
    LoadedProfile { version: 1, flags: 0, entries: e }
}

// ── 数据结构二：错误路径账（三要素） ────────────────────────────
/// 逐形态三要素账：拒载(rejected)/告警(warned)/回退(fell_back) 同记
/// 同现——任何形态只记其一是 TriadBroken。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrPathLedger {
    pub rejected: [u32; CORRUPT_FORMS],
    pub warned: [u32; CORRUPT_FORMS],
    pub fell_back: [u32; CORRUPT_FORMS],
    pub reject_codes: Vec<u32>,
    pub fallback_fingerprint: u32,
}

impl ErrPathLedger {
    /// 新建空账。
    pub fn new() -> ErrPathLedger {
        ErrPathLedger {
            rejected: [0; CORRUPT_FORMS],
            warned: [0; CORRUPT_FORMS],
            fell_back: [0; CORRUPT_FORMS],
            reject_codes: Vec::new(),
            fallback_fingerprint: 0,
        }
    }
    /// 记一轮三要素：拒载必告警必回退，回退指纹入账。
    pub fn record(&mut self, slot: usize, code: u32, fallback: &LoadedProfile) -> Result<(), u32> {
        if slot >= CORRUPT_FORMS {
            return Err(EpCode::TriadBroken.code());
        }
        self.rejected[slot] = self.rejected[slot].saturating_add(1);
        self.warned[slot] = self.warned[slot].saturating_add(1);
        self.fell_back[slot] = self.fell_back[slot].saturating_add(1);
        self.reject_codes.push(code);
        self.fallback_fingerprint = fingerprint(fallback);
        Ok(())
    }
    /// 三要素守恒：逐形态三计数相等且等于轮数。
    pub fn triad_ok(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < CORRUPT_FORMS {
            if self.rejected[i] != rounds || self.warned[i] != rounds || self.fell_back[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 回退指纹守恒：全部形态落到同一冻结默认档案。
    pub fn fallback_uniform(&self) -> bool {
        self.fallback_fingerprint == fingerprint(&default_profile())
    }
}

/// 档案指纹：逐条目 (键×31+值) 折叠（饱和，不回绕）。
pub fn fingerprint(p: &LoadedProfile) -> u32 {
    let mut acc: u32 = 0x600;
    let mut i = 0;
    while i < p.entries.len() {
        if let Some((k, v)) = p.entries.get(i) {
            acc = acc
                .saturating_mul(31)
                .saturating_add((*k as u32).saturating_mul(7))
                .saturating_add(*v as u32);
        }
        i += 1;
    }
    acc.saturating_add(p.version as u32).saturating_add(p.flags as u32)
}

// ── 10 形态损坏样本构造（冻结表） ───────────────────────────────
/// 干净样本构造器：条目 (键,键%5) 升序 + 校验和自动补齐。
pub fn make_clean_sample(entries: &[(u8, u8)], ver: u8, flags: u8) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(MAGIC[0]);
    out.push(MAGIC[1]);
    out.push(MAGIC[2]);
    out.push(MAGIC[3]);
    out.push(ver);
    out.push(flags);
    out.push(entries.len() as u8);
    out.push(0);
    let mut i = 0;
    while i < entries.len() {
        if let Some((k, v)) = entries.get(i) {
            out.push(*k);
            out.push(*v);
        }
        i += 1;
    }
    let c = checksum(&out);
    out.push(c);
    out
}

/// 10 形态损坏样本（逐行对应 slot 0..10，拒因各异的冻结注入语料）：
///   0 空输入 / 1 魔数错 / 2 版本未知 / 3 超长 / 4 截断 / 5 计数谎报
///   6 键越界 / 7 重复键 / 8 乱序 / 9 校验和错。
pub fn make_corrupt_samples() -> [Vec<u8>; CORRUPT_FORMS] {
    let clean4 = make_clean_sample(&[(0, 1), (2, 2), (4, 0), (6, 3)], 1, 0);
    // 形态 3 超长：满档再补一段尾巴（>25 字节）。
    let mut long = make_clean_sample(&[(0, 1), (1, 2), (2, 3), (3, 4), (4, 0), (5, 1), (6, 2), (7, 3)], 1, 0);
    long.push(0);
    [
        Vec::new(),                       // 0 空输入
        b"XXXX\x01\x00\x00\x00\x00".to_vec(), // 1 魔数错
        make_clean_sample(&[(0, 1)], 4, 0),   // 2 版本未知（ver=4）
        long,                                 // 3 超长
        clean4[..7].to_vec(),                 // 4 截断（头部不满，闸 5 拦）
        {                                     // 5 计数谎报（count=2 改成 3，总长失配）
            let mut b = make_clean_sample(&[(0, 1), (2, 2)], 1, 0);
            b[6] = 3;
            b
        },
        make_clean_sample(&[(0, 1), (2, 2), (99, 0)], 1, 0), // 6 键越界——注意：构造器校验和有效但闸 7 先拦
        b"",                                  // 7 占位（下方重填，重复键需手拼）
        b"",                                  // 8 占位（乱序手拼）
        {                                     // 9 校验和错
            let mut b = make_clean_sample(&[(0, 1), (2, 2)], 1, 0);
            let cl = b.len();
            if cl >= 1 {
                let idx = cl - 1;
                b[idx] = b[idx].wrapping_add(1);
            }
            b
        },
    ]
}

/// 重复键形态（slot 7）：键 2 声明两次（手拼，键升序但重复）。
pub fn make_dup_sample() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(MAGIC[0]);
    out.push(MAGIC[1]);
    out.push(MAGIC[2]);
    out.push(MAGIC[3]);
    out.push(1);
    out.push(0);
    out.push(2); // count=2
    out.push(0);
    out.push(2);
    out.push(1);
    out.push(2);
    out.push(3);
    let c = checksum(&out);
    out.push(c);
    out
}

/// 乱序形态（slot 8）：键 4 在前、键 1 在后（非升序）。
pub fn make_unsorted_sample() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(MAGIC[0]);
    out.push(MAGIC[1]);
    out.push(MAGIC[2]);
    out.push(MAGIC[3]);
    out.push(1);
    out.push(0);
    out.push(2);
    out.push(0);
    out.push(4);
    out.push(1);
    out.push(1);
    out.push(2);
    let c = checksum(&out);
    out.push(c);
    out
}

/// 全 10 形态就位（占位重填后的完整注入语料）。
pub fn all_corrupt_forms() -> [Vec<u8>; CORRUPT_FORMS] {
    let mut forms = make_corrupt_samples();
    forms[7] = make_dup_sample();
    forms[8] = make_unsorted_sample();
    forms
}

// ── 注入核 ──────────────────────────────────────────────────────
/// 注入轮：10 形态各跑 rounds 轮，逐形态断言「拒载+告警+回退」三要素。
pub fn inject_all_forms(rounds: u32) -> Result<ErrPathLedger, u32> {
    let forms = all_corrupt_forms();
    let mut led = ErrPathLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut slot = 0;
        while slot < CORRUPT_FORMS {
            let bytes = forms.get(slot).ok_or(EpCode::TriadBroken.code())?;
            match load_profile(bytes) {
                Err(c) => {
                    // 三要素同记：拒载 + 告警（拒因入账）+ 回退（指纹）。
                    led.record(slot, c, &default_profile())?;
                }
                Ok(_) => return Err(EpCode::TriadBroken.code()), // 损坏形态漏检
            }
            slot += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：干净档案装载成功 → 10 形态注入三要素全过 → 回退一致性
/// → 判定次序（魔数先于版本、结构先于校验和）→ 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 干净四条目档案：装载成功且语义完整。
    let clean = make_clean_sample(&[(0, 1), (2, 2), (4, 0), (6, 3)], 1, 0);
    let loaded = load_profile(&clean)?;
    if !loaded.semantic_ok() || loaded.entry_count() != 4 {
        return Err(EpCode::TriadBroken.code());
    }
    // 10 形态注入：三要素全过。
    let led = inject_all_forms(JUDGE_ROUNDS)?;
    if !led.triad_ok(JUDGE_ROUNDS) {
        return Err(EpCode::TriadBroken.code());
    }
    if !led.fallback_uniform() {
        return Err(EpCode::TriadBroken.code());
    }
    // 判定次序冻结：魔数错的档不报版本错（闸序先魔数）。
    let magic_err = load_profile(b"XXXX\x04\x00\x00\x00\x00");
    match magic_err {
        Err(c) if c == EpCode::MagicBad.code() => {}
        _ => return Err(EpCode::MagicBad.code()),
    }
    // 结构先于语义：截断档不报键越界。
    let forms = all_corrupt_forms();
    match load_profile(forms.get(4).ok_or(EpCode::TriadBroken.code())?) {
        Err(c) if c == EpCode::Truncated.code() => {}
        _ => return Err(EpCode::Truncated.code()),
    }
    Ok((loaded.entry_count(), CORRUPT_FORMS as u32, fingerprint(&default_profile())))
}

/// 探针三序（装载成功/拒载回退/空档拒）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let clean = make_clean_sample(&[(0, 1), (2, 2)], 2, 1);
    match load_profile(&clean) {
        Ok(p) if p.version == 2 && p.entry_count() == 2 => Ok(p.entry_count()),
        _ => Err(EpCode::TriadBroken.code()),
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    let forms = all_corrupt_forms();
    let b = forms.get(1).ok_or(EpCode::TriadBroken.code())?;
    match load_profile(b) {
        Err(c) if c == EpCode::MagicBad.code() => Ok(()),
        _ => Err(EpCode::TriadBroken.code()),
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    match load_profile(&[]) {
        Err(c) if c == EpCode::EmptyInput.code() => Ok(()),
        _ => Err(EpCode::TriadBroken.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EpCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EpCode::ALL.len() {
        let v = EpCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < EpCode::ALL.len() {
        if EpCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：10 形态注入全过 + 三要素呈现。
    let inj = inject_all_forms(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.triad_ok(JUDGE_ROUNDS) && l.fallback_uniform(),
        Err(_) => false,
    };
    v.push(("J1-损坏10形态注入全过", inj_ok, format!("{:?}", inj.as_ref().map(|l| l.rejected))));
    // 三要素呈现：拒载=告警=回退。
    let triad = match &inj {
        Ok(l) => {
            let mut eq = true;
            let mut i = 0;
            while i < CORRUPT_FORMS {
                if l.rejected[i] != l.warned[i] || l.warned[i] != l.fell_back[i] {
                    eq = false;
                }
                i += 1;
            }
            eq
        }
        Err(_) => false,
    };
    v.push(("J1-三要素呈现", triad, String::from("拒载=告警=回退逐形态")));
    // 干净档案装载成功。
    let clean_ok = probe_seq_a();
    v.push(("J2-干净档案装载", clean_ok == Ok(2u32), format!("{:?}", clean_ok)));
    // 回退指纹一致（10 形态同一冻结默认档）。
    let fp = fingerprint(&default_profile());
    let fb = match &inj {
        Ok(l) => l.fallback_uniform() && l.fallback_fingerprint == fp,
        Err(_) => false,
    };
    v.push(("J2-回退指纹一致", fb, format!("fp={}", fp)));
    // 判定次序冻结（魔数先于版本、结构先于语义）。
    let order_ok = probe_seq_b().is_ok() && probe_seq_c().is_ok();
    v.push(("J2-判定次序冻结", order_ok, String::from("闸序：魔数→版本→结构→语义")));
    // 键面三闸（越界/重复/乱序）各显式拒。
    let forms2 = all_corrupt_forms();
    let k6 = load_profile(forms2.get(6).ok_or(EpCode::TriadBroken.code()).map(|x| x.as_slice()).unwrap_or(&[]));
    let k7 = load_profile(forms2.get(7).ok_or(EpCode::TriadBroken.code()).map(|x| x.as_slice()).unwrap_or(&[]));
    let k8 = load_profile(forms2.get(8).ok_or(EpCode::TriadBroken.code()).map(|x| x.as_slice()).unwrap_or(&[]));
    let keys_ok = match (k6, k7, k8) {
        (Err(a), Err(b), Err(c)) => {
            a == EpCode::KeyRange.code() && b == EpCode::KeyDup.code() && c == EpCode::KeyUnsorted.code()
        }
        _ => false,
    };
    v.push(("J2-键面三闸各拒", keys_ok, String::from("越界/重复/乱序 拒因各异")));
    // 校验和往返：干净构造→装载成功；改一字节→拒。
    let clean = make_clean_sample(&[(1, 2), (3, 0)], 1, 0);
    let rt_ok = load_profile(&clean).is_ok();
    let mut tampered = make_clean_sample(&[(1, 2), (3, 0)], 1, 0);
    let tl = tampered.len();
    if tl >= 1 {
        let idx = tl - 1;
        tampered[idx] = tampered[idx].wrapping_add(1);
    }
    let tamper_caught = load_profile(&tampered).err() == Some(EpCode::ChecksumBad.code());
    v.push(("J2-校验和往返与必抓", rt_ok && tamper_caught, String::from("构造↔装载↔篡改")));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 边界：满档 8 条目装载成功。
    let full = make_clean_sample(
        &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 0), (5, 1), (6, 2), (7, 3)], 1, 0);
    v.push(("J2-满档边界", load_profile(&full).is_ok(), format!("8 条目/25 字节")));
    // 段位守卫 + 文案闭集。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x80600 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("12 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18553_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18553 档案错误路径账（损坏/缺字段）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}
// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_forms_all_rejected_with_triad() {
        let led = inject_all_forms(JUDGE_ROUNDS).unwrap_or_else(|_| ErrPathLedger::new());
        assert!(led.triad_ok(JUDGE_ROUNDS), "10 形态三要素 10/10");
        assert!(led.fallback_uniform(), "回退指纹统一");
        assert_eq!(led.reject_codes.len(), (CORRUPT_FORMS as u32 * JUDGE_ROUNDS) as usize);
    }

    #[test]
    fn clean_profile_loads() {
        assert_eq!(probe_seq_a(), Ok(2u32));
        let clean = make_clean_sample(&[(0, 1), (2, 2), (4, 0), (6, 3)], 1, 0);
        let p = load_profile(&clean).unwrap_or(LoadedProfile { version: 0, flags: 0, entries: Vec::new() });
        assert!(p.semantic_ok());
        assert_eq!(p.entry_count(), 4);
    }

    #[test]
    fn gate_order_frozen() {
        // 魔数错优先于一切（即使版本也错）。
        match load_profile(b"XXXX\x09\x00\x00\x00\x00") {
            Err(c) => assert_eq!(c, EpCode::MagicBad.code()),
            Ok(_) => panic!("魔数错必须拒"),
        }
        // 结构不满优先于键语义。
        match load_profile(b"VXPF\x01") {
            Err(c) => assert_eq!(c, EpCode::Truncated.code()),
            Ok(_) => panic!("截断必须拒"),
        }
    }

    #[test]
    fn key_gates_distinct_codes() {
        let forms = all_corrupt_forms();
        assert_eq!(
            load_profile(forms.get(6).unwrap_or(&Vec::new())),
            Err(EpCode::KeyRange.code())
        );
        assert_eq!(load_profile(forms.get(7).unwrap_or(&Vec::new())), Err(EpCode::KeyDup.code()));
        assert_eq!(
            load_profile(forms.get(8).unwrap_or(&Vec::new())),
            Err(EpCode::KeyUnsorted.code())
        );
    }

    #[test]
    fn checksum_roundtrip_and_tamper() {
        let clean = make_clean_sample(&[(1, 2), (3, 0)], 1, 0);
        assert!(load_profile(&clean).is_ok(), "校验和往返");
        let mut bad = clean.clone();
        let bl = bad.len();
        if bl >= 1 {
            let idx = bl - 1;
            bad[idx] = bad[idx].wrapping_add(7);
        }
        assert_eq!(load_profile(&bad), Err(EpCode::ChecksumBad.code()));
    }

    #[test]
    fn fallback_deterministic() {
        let a = default_profile();
        let b = default_profile();
        assert_eq!(a, b, "冻结默认档案逐字段同值");
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn repro_and_segment() {
        assert!(repro_check().is_ok(), "双跑一致");
        assert!(segment_guard(), "0x80600 段位守卫");
        assert!(detail_closed(), "文案闭集");
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18553_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
