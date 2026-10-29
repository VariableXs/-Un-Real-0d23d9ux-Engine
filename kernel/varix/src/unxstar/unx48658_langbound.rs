//! UNX-F48658 —— I·出厂语言清单联签（中文预装与其他语言边界声明）
//! 判据原文：UNX-F48658-J1 声明入册+越界注入 ×10 零波及；
//! 判据锚定 §AI-61 §1。
//! 判据落地（drill 场景）：R1 清单入册；R2 中文预装在位；R3 他语零预装；
//! R4 越界注入 ×10 拒收；R5 注入后账面零波及；R6 语言码闭集；
//! R7 名称非空；R8 码唯一；R9 呈现行；R10 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0960;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_MANIFEST: u16 = SEG_BASE + 1; // 清单未入册
const E_PRESET: u16 = SEG_BASE + 2; // 预装位与声明不符
const E_INJECT: u16 = SEG_BASE + 3; // 越界注入未被拒
const E_SPILL: u16 = SEG_BASE + 4; // 注入后账面被波及
const E_CODE: u16 = SEG_BASE + 5; // 语言码越出闭集
const E_NAME: u16 = SEG_BASE + 6; // 名称非法
const E_DUP: u16 = SEG_BASE + 7; // 语言码重复
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_INJN: u16 = SEG_BASE + 10; // 注入轮次不足 10

/// 语言码闭集。
pub const L_ZH_CN: u16 = 0x0804; // 简体中文（出厂预装）
pub const L_ZH_TW: u16 = 0x0404; // 繁体中文
pub const L_EN_US: u16 = 0x0409; // 美式英语
pub const L_JA_JP: u16 = 0x0411; // 日语
pub const L_KO_KR: u16 = 0x0412; // 韩语
/// 越界注入轮次。
pub const INJ_N: u32 = 10;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 语言码闭集判定。
pub fn code_ok(c: u16) -> bool {
    c == L_ZH_CN || c == L_ZH_TW || c == L_EN_US || c == L_JA_JP || c == L_KO_KR
}

/// 一条语言清单项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LangEntry {
    pub code: u16,
    pub name: String,
    pub preset: bool, // 出厂预装位
}

impl LangEntry {
    pub fn new(code: u16, name: &str, preset: bool) -> LangEntry {
        LangEntry { code, name: name.to_string(), preset }
    }

    pub fn name_ok(&self) -> bool {
        !self.name.is_empty() && self.name.len() >= 2
    }
}

/// 出厂语言清单：声明入册（中文预装、其余不预装）。
pub struct LangManifest {
    pub entries: Vec<LangEntry>,
}

impl LangManifest {
    pub fn new() -> LangManifest {
        LangManifest { entries: vec![] }
    }

    /// 入册：码闭集 + 名称非空 + 码唯一。
    pub fn register(&mut self, e: LangEntry) -> Result<(), u16> {
        if !code_ok(e.code) {
            return Err(E_CODE);
        }
        if !e.name_ok() {
            return Err(E_NAME);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].code == e.code {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.entries.push(e);
        Ok(())
    }

    /// 出厂声明表（固定序）。
    pub fn factory() -> LangManifest {
        let mut m = LangManifest::new();
        let _ = m.register(LangEntry::new(L_ZH_CN, "zh-CN", true));
        let _ = m.register(LangEntry::new(L_ZH_TW, "zh-TW", false));
        let _ = m.register(LangEntry::new(L_EN_US, "en-US", false));
        let _ = m.register(LangEntry::new(L_JA_JP, "ja-JP", false));
        let _ = m.register(LangEntry::new(L_KO_KR, "ko-KR", false));
        m
    }

    /// 预装声明核验：仅 zh-CN 预装，其余零预装。
    pub fn preset_decl_ok(&self) -> Result<(), u16> {
        if self.entries.len() != 5 {
            return Err(E_MANIFEST);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            let want = self.entries[i].code == L_ZH_CN;
            if self.entries[i].preset != want {
                return Err(E_PRESET);
            }
            i += 1;
        }
        Ok(())
    }

    /// 越界注入：强行预装非中文码 → 拒收，账面不动。
    pub fn inject_preset(&mut self, code: u16) -> Result<(), u16> {
        if code == L_ZH_CN {
            return Err(E_INJECT);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].code == code {
                // 声明位只读：拒绝改写。
                return Err(E_INJECT);
            }
            i += 1;
        }
        Err(E_INJECT)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].code.to_le_bytes());
            buf.extend_from_slice(self.entries[i].name.as_bytes());
            buf.push(self.entries[i].preset as u8);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 呈现行：langs=<n> preset=<预装数>。
pub fn manifest_line(m: &LangManifest) -> String {
    let mut preset: u32 = 0;
    let mut i = 0usize;
    while i < m.entries.len() {
        if m.entries[i].preset {
            preset += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("langs=");
    out.push_str(&m.entries.len().to_string());
    out.push_str(" preset=");
    out.push_str(&preset.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_58);

    // R1 清单入册：出厂声明表 5 语全部登记。
    let mut m = LangManifest::factory();
    if m.entries.len() != 5 {
        return Err(E_MANIFEST);
    }
    hits += 1;

    // R2 中文预装在位。
    m.preset_decl_ok()?;
    let zh = m.entries[0].clone();
    if zh.code != L_ZH_CN || !zh.preset {
        return Err(E_PRESET);
    }
    hits += 1;

    // R3 他语零预装：其余四语 preset 全 false。
    let mut i = 1usize;
    while i < m.entries.len() {
        if m.entries[i].preset {
            return Err(E_PRESET);
        }
        i += 1;
    }
    hits += 1;

    // R4 越界注入 ×10 拒收：外部码与在册他语码交替强推。
    let mut round: u32 = 0;
    let foreign = [L_ZH_TW, L_EN_US, L_JA_JP, L_KO_KR, 0x0407];
    while round < INJ_N {
        let pick = foreign[(round % 5) as usize];
        match m.inject_preset(pick) {
            Err(E_INJECT) => {}
            _ => return Err(E_INJECT),
        }
        round += 1;
    }
    hits += 1;

    // R5 注入后账面零波及：指纹与注入前一致。
    let before = m.fp();
    let mut m2 = LangManifest::factory();
    let _ = m2.inject_preset(L_JA_JP);
    let mut probe = LangManifest::factory();
    let _ = probe.inject_preset(L_KO_KR);
    if probe.fp() != before {
        return Err(E_SPILL);
    }
    hits += 1;

    // R6 语言码闭集：0x0407 越界拒收。
    let mut probe2 = LangManifest::new();
    let bad = LangEntry::new(0x0407, "de-DE", false);
    match probe2.register(bad) {
        Err(E_CODE) => hits += 1,
        _ => return Err(E_CODE),
    }

    // R7 名称非法拒收。
    let noname = LangEntry::new(L_EN_US, "", false);
    match probe2.register(noname) {
        Err(E_NAME) => hits += 1,
        _ => return Err(E_NAME),
    }

    // R8 码重复拒收。
    probe2.register(LangEntry::new(L_ZH_CN, "zh-CN", true))?;
    let dup = LangEntry::new(L_ZH_CN, "zh-CN-alt", false);
    match probe2.register(dup) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R9 呈现行：langs=5 preset=1。
    let line = manifest_line(&m);
    if !has_kw(line.as_bytes(), b"langs=5") || !has_kw(line.as_bytes(), b"preset=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 指纹稳定。
    let f1 = m.fp();
    let f2 = m.fp();
    if f1 != f2 || f1 == 0 || g.below(1000) > 999 {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}
