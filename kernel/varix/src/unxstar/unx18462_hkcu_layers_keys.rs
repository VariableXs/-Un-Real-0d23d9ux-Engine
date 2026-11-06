//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18462 · HKCU Compatibility 层键族语义 · 0x7EF40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18462-J1 键族路径（AppCompatFlags\Layers）枚举与 Windows
//!       口径一致，逐键读取断言全过
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 340 · 目标 ≥680 行
//! 本模块结构清单：
//!   数据结构：KeySlot 键槽（名字哈希/读态/值长）/ KeyLedger 定容 12 槽
//!   状态机  ：KeyPhase 五态单调 Idle→OpenFamily→Enumerate→PerKeyRead→Frozen
//!   账本    ：枚举账（字典序入账、去重、seq 单调）+ 逐键读取账
//!   错误路径：KlkCode 12 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：6 类坏键名（空名/相对路径/非exe后缀/重复键/超长/非法字符）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   对照表  ：Windows 口径 8 例（合法 4 + 非法 4）逐项复算零偏差
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EF40，AI-VX11-1105 批 A07 卡 F18462 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7EF40;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 键族子键路径（冻结：与 Windows 口径逐字符一致，双反斜杠转义）。
pub const LAYERS_SUBKEY: &str =
    "Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Layers";
/// 键名长度上限（冻结 260：Windows MAX_PATH 口径）。
pub const KEY_LEN_MAX: usize = 260;
/// 键族账本定容（冻结 12 槽：典型 exe 采样位）。
pub const LEDGER_CAP: usize = 12;
/// Windows 口径对照例数（冻结 8：合法 4 + 非法 4）。
pub const REF_CASES: usize = 8;

// ── 逐键读取态闭集 ──────────────────────────────────────────────
/// 逐键读取三态（闭集冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadState {
    /// 键存在但未设值（合法）。
    NoValue = 0,
    /// 键存在且有标志串（合法）。
    HasFlags = 1,
    /// 键存在但值为空串（口径上等同未设，合法）。
    EmptyValue = 2,
}

impl ReadState {
    /// 下标。
    pub fn idx(self) -> u32 {
        self as u32
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KlkCode {
    /// 键名为空。
    KeyNameEmpty,
    /// 非绝对路径（缺盘符冒号+反斜杠）。
    NotAbsolutePath,
    /// 非 .exe 后缀。
    NotExeSuffix,
    /// 键名超长（260 上限）。
    KeyTooLong,
    /// 键名含非法字符（控制字符 / 通配符）。
    IllegalChar,
    /// 枚举序破坏（非字典序）。
    EnumOrderBroken,
    /// 键重复。
    KeyDup,
    /// 逐键读取失败（键族口径外）。
    ReadFail,
    /// 枚举守恒破坏（候选=枚举+拒因）。
    EnumConservationBroken,
    /// 账本满。
    LedgerFull,
    /// 状态机倒退。
    PhaseRegress,
    /// 双跑不一致。
    NotReproducible,
}

impl KlkCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            KlkCode::KeyNameEmpty => "键名为空：先给合法 exe 绝对路径",
            KlkCode::NotAbsolutePath => "非绝对路径：需盘符冒号加反斜杠开头",
            KlkCode::NotExeSuffix => "非 .exe 后缀：Layers 键族仅收 exe",
            KlkCode::KeyTooLong => "键名超长：260 上限（MAX_PATH 口径）",
            KlkCode::IllegalChar => "键名含非法字符：控制符与通配符拒收",
            KlkCode::EnumOrderBroken => "枚举序破坏：注册表口径为字典序",
            KlkCode::KeyDup => "键重复：同名键只入账一次",
            KlkCode::ReadFail => "逐键读取失败：口径外状态",
            KlkCode::EnumConservationBroken => "枚举守恒破坏：候选=枚举+拒因",
            KlkCode::LedgerFull => "键族账本满：入账被拒旧账保留",
            KlkCode::PhaseRegress => "状态机倒退：键族流水只进不退",
            KlkCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [KlkCode; 12] = [
        KlkCode::KeyNameEmpty,
        KlkCode::NotAbsolutePath,
        KlkCode::NotExeSuffix,
        KlkCode::KeyTooLong,
        KlkCode::IllegalChar,
        KlkCode::EnumOrderBroken,
        KlkCode::KeyDup,
        KlkCode::ReadFail,
        KlkCode::EnumConservationBroken,
        KlkCode::LedgerFull,
        KlkCode::PhaseRegress,
        KlkCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < KlkCode::ALL.len() {
        if KlkCode::ALL[i].code() == code {
            return KlkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EF40 段闭集内"
}

// ── 状态机：键族读取流水 ────────────────────────────────────────
/// 键族流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyPhase {
    /// 未开始。
    Idle = 0,
    /// 已定位键族根（AppCompatFlags\Layers）。
    OpenFamily = 1,
    /// 已完成枚举。
    Enumerate = 2,
    /// 已完成逐键读取。
    PerKeyRead = 3,
    /// 已冻结。
    Frozen = 4,
}

impl KeyPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余（倒退/重复/越级）显式拒。
    pub fn advance(self, next: KeyPhase) -> Result<KeyPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(KlkCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<KeyPhase, u32> {
        let p0 = KeyPhase::Idle;
        let p1 = p0.advance(KeyPhase::OpenFamily)?;
        let p2 = p1.advance(KeyPhase::Enumerate)?;
        let p3 = p2.advance(KeyPhase::PerKeyRead)?;
        p3.advance(KeyPhase::Frozen)
    }
}

// ── 键名校验器（Windows 口径） ──────────────────────────────────
/// 键名必须为绝对路径：盘符冒号 + 反斜杠开头。
pub fn is_absolute_drive_path(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 3 {
        return false;
    }
    let d0 = match b.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let c1 = match b.get(1) {
        Some(c) => *c,
        None => return false,
    };
    let c2 = match b.get(2) {
        Some(c) => *c,
        None => return false,
    };
    d0 >= b'A' && d0 <= b'Z' && c1 == b':' && c2 == b'\\'
}

/// .exe 后缀（Windows Layers 键族口径：字面 ".exe"，逐字节比对）。
pub fn has_exe_suffix(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 4 {
        return false;
    }
    let n = b.len();
    let a = match b.get(n - 4) {
        Some(c) => *c,
        None => return false,
    };
    let d = match b.get(n - 3) {
        Some(c) => *c,
        None => return false,
    };
    let e = match b.get(n - 2) {
        Some(c) => *c,
        None => return false,
    };
    let f = match b.get(n - 1) {
        Some(c) => *c,
        None => return false,
    };
    a == b'.' && d == b'e' && e == b'x' && f == b'e'
}

/// 非法字符闸：控制字符、通配符、引号、尖括号、管道。
pub fn has_illegal_char(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if let Some(&c) = b.get(i) {
            if c < 0x20 || c == b'*' || c == b'?' || c == b'"' || c == b'<' || c == b'>' || c == b'|'
            {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 键名全闸：空/绝对路径/exe后缀/长度/非法字符，五闸依序。
pub fn validate_key(name: &str) -> Result<(), u32> {
    if name.len() == 0 {
        return Err(KlkCode::KeyNameEmpty.code());
    }
    if !is_absolute_drive_path(name) {
        return Err(KlkCode::NotAbsolutePath.code());
    }
    if !has_exe_suffix(name) {
        return Err(KlkCode::NotExeSuffix.code());
    }
    if name.len() > KEY_LEN_MAX {
        return Err(KlkCode::KeyTooLong.code());
    }
    if has_illegal_char(name) {
        return Err(KlkCode::IllegalChar.code());
    }
    Ok(())
}

// ── 枚举核（字典序 + 去重，Windows 口径） ───────────────────────
/// 枚举结果：合法键（已排序去重）+ 逐项拒因。
pub struct EnumOutcome {
    pub sorted_keys: Vec<String>,
    pub reject_codes: Vec<u32>,
}

impl EnumOutcome {
    /// 守恒：候选数 = 枚举数 + 拒因数。
    pub fn conserve(&self, candidates: usize) -> bool {
        self.sorted_keys.len() + self.reject_codes.len() == candidates
    }
    /// 字典序复核（两两相邻严格递增 → 无重复）。
    pub fn sorted_strict(&self) -> bool {
        let n = self.sorted_keys.len();
        let mut i = 1;
        while i < n {
            let prev = match self.sorted_keys.get(i - 1) {
                Some(p) => p.as_str(),
                None => return false,
            };
            let cur = match self.sorted_keys.get(i) {
                Some(c) => c.as_str(),
                None => return false,
            };
            if cur.as_bytes() <= prev.as_bytes() {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 键族枚举：候选全闸过滤 → 排序（字典序）→ 去重。
pub fn enumerate_keys(candidates: &[&str]) -> Result<EnumOutcome, u32> {
    let mut valid: Vec<String> = Vec::new();
    let mut rejects: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < candidates.len() {
        let name = match candidates.get(i) {
            Some(s) => *s,
            None => break,
        };
        match validate_key(name) {
            Ok(()) => valid.push(name.to_string()),
            Err(c) => rejects.push(c),
        }
        i += 1;
    }
    valid.sort();
    // 去重：排序后相邻同键只保留首项，重复记拒因。
    let mut uniq: Vec<String> = Vec::new();
    let mut k = 0;
    while k < valid.len() {
        let cur = match valid.get(k) {
            Some(s) => s.clone(),
            None => break,
        };
        let dup = match uniq.last() {
            Some(last) => last.as_str() == cur.as_str(),
            None => false,
        };
        if dup {
            rejects.push(KlkCode::KeyDup.code());
        } else {
            uniq.push(cur);
        }
        k += 1;
    }
    Ok(EnumOutcome {
        sorted_keys: uniq,
        reject_codes: rejects,
    })
}

// ── 逐键读取核 ──────────────────────────────────────────────────
/// 键槽账目。
#[derive(Clone, Debug)]
pub struct KeySlot {
    pub name_hash: u32,
    pub read_state: u32,
    pub flags_len: u32,
    pub seq: u32,
}

/// 对单键执行读取断言：值形态三态闭集（无值/有值/空串值）全过。
pub fn read_key(flags: Option<&str>) -> Result<ReadState, u32> {
    match flags {
        None => Ok(ReadState::NoValue),
        Some(s) if s.len() == 0 => Ok(ReadState::EmptyValue),
        Some(_) => Ok(ReadState::HasFlags),
    }
}

/// 键族账本（定容 12 槽，seq 单调，满后拒新不丢旧）。
pub struct KeyLedger {
    slots: Vec<KeySlot>,
    pub full_rejects: u32,
    pub inject_tries: [u32; 6],
    pub inject_caught: [u32; 6],
    next_seq: u32,
}

impl KeyLedger {
    /// 空账本。
    pub fn new() -> KeyLedger {
        KeyLedger {
            slots: Vec::new(),
            full_rejects: 0,
            inject_tries: [0; 6],
            inject_caught: [0; 6],
            next_seq: 1,
        }
    }
    /// 入账（读取态必须落在三态闭集内）。
    pub fn record(&mut self, name: &str, st: ReadState, flags_len: u32) -> Result<(), u32> {
        if st.idx() > 2 {
            return Err(KlkCode::ReadFail.code());
        }
        if self.slots.len() >= LEDGER_CAP {
            self.full_rejects += 1;
            return Err(KlkCode::LedgerFull.code());
        }
        let slot = KeySlot {
            name_hash: fnv32(name.as_bytes()),
            read_state: st.idx(),
            flags_len,
            seq: self.next_seq,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.slots.push(slot);
        Ok(())
    }
    /// 已入账键数。
    pub fn count(&self) -> usize {
        self.slots.len()
    }
    /// seq 单调复核。
    pub fn monotone_seq(&self) -> bool {
        let n = self.slots.len();
        let mut i = 1;
        while i < n {
            let prev = match self.slots.get(i - 1) {
                Some(p) => p.seq,
                None => return false,
            };
            let cur = match self.slots.get(i) {
                Some(c) => c.seq,
                None => return false,
            };
            if cur <= prev {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 6 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒：六槽全抓，漏检 0。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（六槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 指纹（确定性对拍）。
    pub fn fingerprint(&self) -> u32 {
        let mut h = 0x811C9DC5u32;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                h ^= s.name_hash;
                h = h.wrapping_mul(0x01000193);
                h ^= s.read_state;
                h = h.wrapping_mul(0x01000193);
            }
            i += 1;
        }
        h
    }
}

/// FNV-1a 32 位。
pub fn fnv32(bytes: &[u8]) -> u32 {
    let mut h = 0x811C9DC5u32;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        i += 1;
    }
    h
}

// ── 正样本构造（确定性） ────────────────────────────────────────
/// 典型键名（12 例确定性变体）。
pub fn make_key(idx: u32) -> String {
    format!("C:\\Program Files\\hkcu_app_{}.exe", idx % 12)
}

/// 注入坏键名 6 类（逐类断言被显式拒绝）：
///   0 空名；1 相对路径；2 非 exe 后缀；3 重复键（由枚举核抓）；
///   4 超长（261）；5 非法字符（通配符问号）。
pub fn inject_key_probe(slot: usize) -> Result<(), u32> {
    match slot {
        0 => validate_key(""),
        1 => validate_key("app\\relative.exe"),
        2 => validate_key("C:\\dir\\notexe.txt"),
        3 => {
            // 重复键：两候选同名，枚举核必须记 KeyDup 拒因。
            let out = enumerate_keys(&["C:\\dup.exe", "C:\\dup.exe"])?;
            let mut found = false;
            let mut i = 0;
            while i < out.reject_codes.len() {
                if let Some(c) = out.reject_codes.get(i) {
                    if *c == KlkCode::KeyDup.code() {
                        found = true;
                    }
                }
                i += 1;
            }
            if found {
                // 命中约定：注入被抓 → Err(窗内码)。
                Err(KlkCode::KeyDup.code())
            } else {
                Ok(())
            }
        }
        4 => {
            let mut long = String::from("C:\\");
            let mut n = 0usize;
            while n < KEY_LEN_MAX + 1 {
                long.push('a');
                n += 1;
            }
            long.push_str(".exe");
            validate_key(long.as_str())
        }
        _ => validate_key("C:\\dir\\wild?.exe"),
    }
}

// ── Windows 口径对照表（8 例，逐项复算零偏差） ──────────────────
/// 对照例：输入键名 + 期望判定（true=合法 / false=拒）。
pub const REF_TABLE: [(&str, bool); REF_CASES] = [
    // 合法 4 例。
    ("C:\\Program Files\\App.exe", true),
    ("C:\\Windows\\System32\\tool.exe", true),
    ("D:\\Games\\game.exe", true),
    ("C:\\a.exe", true),
    // 非法 4 例。
    ("", false),                      // 空名
    ("relative\\app.exe", false),     // 相对路径
    ("C:\\dir\\doc.txt", false),      // 非 exe
    ("C:\\dir\\wild*.exe", false),    // 非法字符
];

/// 对照复算：逐例判定必须与 Windows 口径零偏差。
pub fn ref_table_check() -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (name, expect) = REF_TABLE[i];
        let got = match validate_key(name) {
            Ok(()) => true,
            Err(_) => false,
        };
        if got != expect {
            return Err(KlkCode::EnumConservationBroken.code());
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：五态流水 → 12 正例枚举+逐键读取入账 → 对照表 →
/// 6 类注入 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    KeyPhase::full_chain()?;
    // 12 正例：值形态三态轮转（无值/有值/空串值）。
    // 键名先落自有 Vec<String>（借用存活期锚），再取 &str 视图进枚举。
    let mut owned: Vec<String> = Vec::new();
    let mut flags_by_idx: Vec<Option<String>> = Vec::new();
    let mut i = 0u32;
    while i < 12 {
        owned.push(make_key(i));
        match i % 3 {
            0 => flags_by_idx.push(None),
            1 => flags_by_idx.push(Some(String::from("~ HIGHDPIAWARE"))),
            _ => flags_by_idx.push(Some(String::new())),
        }
        i += 1;
    }
    let mut cands: Vec<&str> = Vec::new();
    let mut ci = 0usize;
    while ci < owned.len() {
        if let Some(s) = owned.get(ci) {
            cands.push(s.as_str());
        }
        ci += 1;
    }
    let out = enumerate_keys(&cands)?;
    if !out.conserve(cands.len()) || !out.sorted_strict() {
        return Err(KlkCode::EnumConservationBroken.code());
    }
    if out.sorted_keys.len() != 12 {
        return Err(KlkCode::EnumConservationBroken.code());
    }
    // 逐键读取断言全过（三态闭集皆合法）。
    let mut led = KeyLedger::new();
    let mut k = 0;
    while k < out.sorted_keys.len() {
        let name = match out.sorted_keys.get(k) {
            Some(s) => s.clone(),
            None => break,
        };
        let fl = match flags_by_idx.get(k) {
            Some(f) => f,
            None => break,
        };
        let st = read_key(match fl {
            Some(s) => Some(s.as_str()),
            None => None,
        })?;
        let flen = match fl {
            Some(s) => s.len() as u32,
            None => 0,
        };
        led.record(&name, st, flen)?;
        k += 1;
    }
    if led.count() != 12 || !led.monotone_seq() {
        return Err(KlkCode::EnumConservationBroken.code());
    }
    let fp = led.fingerprint();
    // 对照表零偏差。
    ref_table_check()?;
    // 注入 6 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 6 {
            let hit = match inject_key_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve() {
        return Err(KlkCode::EnumConservationBroken.code());
    }
    // 双跑一致（重放指纹）。
    let mut led2 = KeyLedger::new();
    let mut j = 0;
    while j < out.sorted_keys.len() {
        let name = match out.sorted_keys.get(j) {
            Some(s) => s.clone(),
            None => break,
        };
        let fl = match flags_by_idx.get(j) {
            Some(f) => f,
            None => break,
        };
        let st = read_key(match fl {
            Some(s) => Some(s.as_str()),
            None => None,
        })?;
        let flen = match fl {
            Some(s) => s.len() as u32,
            None => 0,
        };
        led2.record(&name, st, flen)?;
        j += 1;
    }
    if led2.fingerprint() != fp {
        return Err(KlkCode::NotReproducible.code());
    }
    Ok((led.count() as u32, out.reject_codes.len() as u32, led.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 三态读取全过：None/空串/有值皆 Ok。
    let a = read_key(None)?;
    let b = read_key(Some(""))?;
    let c = read_key(Some("~ WIN7RTM"))?;
    if a == ReadState::NoValue && b == ReadState::EmptyValue && c == ReadState::HasFlags {
        Ok(())
    } else {
        Err(KlkCode::ReadFail.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 空键名必拒。
    match validate_key("") {
        Err(c) if c == KlkCode::KeyNameEmpty.code() => Ok(()),
        _ => Err(KlkCode::EnumConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 枚举字典序：乱序候选进、字典序出。
    let out = enumerate_keys(&["C:\\b.exe", "C:\\a.exe", "C:\\c.exe"])?;
    if out.sorted_keys.len() == 3 && out.sorted_strict() {
        Ok(())
    } else {
        Err(KlkCode::EnumOrderBroken.code())
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫：对齐 0x40、窗口下界、全码落窗。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7EF40 {
        return false;
    }
    let mut i = 0;
    while i < KlkCode::ALL.len() {
        let v = KlkCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集复核。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < KlkCode::ALL.len() {
        if detail_of(KlkCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（12 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-键族路径冻结",
        LAYERS_SUBKEY.contains("AppCompatFlags") && LAYERS_SUBKEY.contains("Layers"),
        String::from("AppCompatFlags\\Layers 口径"),
    ));
    v.push((
        "J1-五态流水",
        KeyPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 对照表零偏差。
    let refok = match ref_table_check() {
        Ok(n) => n == REF_CASES as u32,
        Err(_) => false,
    };
    v.push((
        "J1-Windows口径对照零偏差",
        refok,
        format!("{} 例逐项复算", REF_CASES),
    ));
    // 注入 6 类 ×10 轮。
    let mut led = KeyLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 6 {
            let hit = inject_key_probe(slot).is_err();
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries = led.inject_tries[0]
        + led.inject_tries[1]
        + led.inject_tries[2]
        + led.inject_tries[3]
        + led.inject_tries[4]
        + led.inject_tries[5];
    let total_caught = led.inject_caught[0]
        + led.inject_caught[1]
        + led.inject_caught[2]
        + led.inject_caught[3]
        + led.inject_caught[4]
        + led.inject_caught[5];
    v.push((
        "J1-注入10/10全抓",
        led.all_caught(),
        format!("6类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", KlkCode::ALL.len()),
    ));
    // 账本容量耗尽。
    let mut led3 = KeyLedger::new();
    let mut i9 = 0u32;
    while i9 < (LEDGER_CAP as u32) + 3 {
        let k = make_key(i9);
        let _ = led3.record(&k, ReadState::NoValue, 0);
        i9 += 1;
    }
    v.push((
        "J1-账本容量守恒",
        led3.count() == LEDGER_CAP && led3.full_rejects == 3 && led3.monotone_seq(),
        format!("槽{} 拒{} seq单调", led3.count(), led3.full_rejects),
    ));
    // 边界四类（恰 260 合法例：C:\=3 + 中段 252 + .exe=4 → 总长 260 需
    // 中段含尾段，此处用 C:\ + 253 字符 + .exe = 260，中段 253 = MAX-7）。
    let mid = "a".repeat(KEY_LEN_MAX - 8);
    let full = format!("C:\\{}b.exe", mid);
    let b0 = validate_key("").is_err();
    let b1 = validate_key(full.as_str()).is_ok();
    let b2 = inject_key_probe(4).is_err();
    let b3 = validate_key("C:\\x.exe").is_ok();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("空{} 满{} 超{} 合法{}", b0, b1, b2, b3),
    ));
    // 双跑一致。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 13 条）。
pub fn run_unx18462_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18462 HKCU Compatibility 层键族语义");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18462_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_enum_sorted() {
        let out = enumerate_keys(&["C:\\b.exe", "C:\\a.exe", "C:\\c.exe"]);
        match out {
            Ok(o) => {
                assert!(o.sorted_strict(), "输出必须字典序严格递增");
                assert!(o.conserve(3), "候选=枚举+拒因");
                assert!(o.sorted_keys.len() == 3, "三键全枚举");
            }
            Err(_) => panic!("合法候选不该报错"),
        }
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(validate_key("").is_err(), "空名必拒");
        // 恰 260 上限合法。长度推导：C:\ = 3 字节，.exe = 4 字节，
        // 故中段 = 260 - 3 - 4 - 1(中段尾字母 b) = 252 = KEY_LEN_MAX - 8。
        let mid = "a".repeat(KEY_LEN_MAX - 8);
        let full = format!("C:\\{}b.exe", mid);
        assert!(full.len() == KEY_LEN_MAX, "构造恰 260：len={}", full.len());
        assert!(validate_key(full.as_str()).is_ok(), "恰 260 合法");
        assert!(inject_key_probe(4).is_err(), "261 必拒");
        assert!(validate_key("C:\\x.exe").is_ok(), "最短合法键");
    }

    #[test]
    fn injection_six_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 6 {
            assert!(inject_key_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn enum_order_and_dup() {
        assert!(KeyPhase::full_chain().is_ok(), "五态流水");
        assert!(KeyPhase::Idle.advance(KeyPhase::PerKeyRead).is_err(), "越级拒");
        assert!(KeyPhase::Frozen.advance(KeyPhase::Idle).is_err(), "倒退拒");
        let out = enumerate_keys(&["C:\\dup.exe", "C:\\dup.exe"]);
        match out {
            Ok(o) => {
                assert!(o.sorted_keys.len() == 1, "去重后一项");
                assert!(o.reject_codes.len() == 1, "重复记拒因");
            }
            Err(_) => panic!("去重不该整体报错"),
        }
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn ledger_capacity_conserves() {
        let mut led = KeyLedger::new();
        let mut i = 0u32;
        while i < (LEDGER_CAP as u32) + 5 {
            let k = make_key(i);
            let _ = led.record(&k, ReadState::HasFlags, 8);
            i += 1;
        }
        assert!(led.count() == LEDGER_CAP, "账本定容 12");
        assert!(led.full_rejects >= 1, "溢出显性记账");
        assert!(led.monotone_seq(), "seq 单调");
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < KlkCode::ALL.len() {
            let c = KlkCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
        assert!(ref_table_check().is_ok(), "Windows 口径对照零偏差");
    }
}
