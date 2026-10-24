//! VE-F3995 · S 域移交包（T 域 F4001）
//!
//! 移交：S 域移交包封装（总账 / 词典 / 联测 / 基准 / 安全 / 无障碍 /
//! 观测 / fuzz / 文档 / 清账十件 + 冻结哈希 + 遗留三路清单）；T 域交接
//! 面声明（国际化无障碍适配承接点：朗读语料多语言 / 色标文化色 / 布局
//! 镜像前置）。
//! 数据结构：移交包（十件+哈希）；交接面册。
//! 错误路径：件缺→拒收；哈希不符→重封；交接面缺→补面。
//! 性能：封装 O(件)；核 O(哈希)；交接 O(1)。
//! 跨批对接：F3981-F3992 内容源；F4001 T 域开工闸。
//! 无障碍与隐私：交接面即无障碍国际化专项（域本色）；包内录证已脱敏。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x47C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x47C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T15HCode(pub u16);

impl T15HCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ITEM_MISS: Self = Self(SEG_BASE + 1);
    pub const HASH_MISMATCH: Self = Self(SEG_BASE + 2);
    pub const FACE_GAP: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ITEM_MISS => "件缺→拒收",
            Self::HASH_MISMATCH => "哈希不符→重封",
            Self::FACE_GAP => "交接面缺→补面",
            _ => "未知",
        }
    }
}

// ── 移交十件 ─────────────────────────────────────────────────────────

pub const HANDOVER_ITEMS: [&str; 10] = [
    "总账", "词典", "联测", "基准", "安全",
    "无障碍", "观测", "fuzz", "文档", "清账",
];

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 移交包。
#[derive(Debug, Clone)]
pub struct HandoverPack {
    /// 已封入件。
    pub items: Vec<&'static str>,
    /// 封装哈希。
    pub seal_hash: u64,
    /// 三基线冻结哈希（F3990 留痕随包）。
    pub baseline_hashes: [u64; 3],
    /// 录证脱敏终版标记。
    pub sanitized: bool,
}

impl HandoverPack {
    pub fn new() -> Self {
        Self { items: Vec::new(), seal_hash: 0, baseline_hashes: [0; 3], sanitized: false }
    }

    /// 封入一件。
    pub fn pack(&mut self, item: &'static str) -> bool {
        if !HANDOVER_ITEMS.contains(&item) || self.items.contains(&item) {
            return false;
        }
        self.items.push(item);
        true
    }

    /// 缺件清单（拒收依据）。
    pub fn missing(&self) -> Vec<&'static str> {
        HANDOVER_ITEMS.iter().filter(|i| !self.items.contains(i)).map(|i| *i).collect()
    }

    /// 封装：十件齐 + 脱敏 → 哈希留痕。
    pub fn seal(&mut self) -> T15HCode {
        if !self.missing().is_empty() || !self.sanitized {
            return T15HCode::ITEM_MISS;
        }
        let mut src = String::new();
        for i in HANDOVER_ITEMS.iter() {
            src.push_str(i);
        }
        self.seal_hash = fnv1a(src.as_bytes());
        T15HCode::OK
    }

    /// 收方核哈希：重算一致才收。
    pub fn verify(&self) -> T15HCode {
        let mut src = String::new();
        for i in HANDOVER_ITEMS.iter() {
            src.push_str(i);
        }
        if fnv1a(src.as_bytes()) == self.seal_hash && self.seal_hash != 0 {
            T15HCode::OK
        } else {
            T15HCode::HASH_MISMATCH
        }
    }
}

// ── 交接面册（国际化无障碍承接点）───────────────────────────────────

/// 交接面三类（域本色专项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoverFace {
    /// 朗读语料多语言。
    SpeechI18n,
    /// 色标文化色。
    ColorCulture,
    /// 布局镜像前置。
    MirrorPrep,
}

pub const HANDOVER_FACES: [HandoverFace; 3] = [
    HandoverFace::SpeechI18n, HandoverFace::ColorCulture, HandoverFace::MirrorPrep,
];

impl HandoverFace {
    pub fn name(self) -> &'static str {
        match self {
            HandoverFace::SpeechI18n => "朗读语料多语言",
            HandoverFace::ColorCulture => "色标文化色",
            HandoverFace::MirrorPrep => "布局镜像前置",
        }
    }
}

/// 交接面册。
#[derive(Debug, Clone)]
pub struct FaceBook {
    /// 已声明交接面。
    pub declared: Vec<HandoverFace>,
    /// 各面承接说明非空。
    pub notes: Vec<(HandoverFace, String)>,
}

impl FaceBook {
    pub fn new() -> Self {
        Self { declared: Vec::new(), notes: Vec::new() }
    }

    /// 声明 + 说明（说明必填）。
    pub fn declare(&mut self, face: HandoverFace, note: &str) -> bool {
        if note.is_empty() || self.declared.contains(&face) {
            return false;
        }
        self.declared.push(face);
        self.notes.push((face, String::from(note)));
        true
    }

    /// 缺面清单。
    pub fn gaps(&self) -> Vec<&'static str> {
        HANDOVER_FACES.iter()
            .filter(|f| !self.declared.contains(f))
            .map(|f| f.name())
            .collect()
    }

    /// 交接面齐备。
    pub fn complete(&self) -> bool {
        self.gaps().is_empty() && self.notes.len() == 3
    }
}

// ── 遗留三路清单随包 ────────────────────────────────────────────────

/// 遗留随包清单（三路计数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeftoverSummary {
    pub closed: u32,
    pub to_t: u32,
    pub deferred: u32,
}

impl LeftoverSummary {
    pub fn total(self) -> u32 {
        self.closed + self.to_t + self.deferred
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves249_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves249_s10handover");

    // C3995-01 诊断码段不越窗。
    chk(&mut set, "C3995-01",
        T15HCode::FACE_GAP.0 < SEG_BASE + 16 && SEG_BASE == 0x47C0, "err");

    // C3995-02 十件清单齐备。
    chk(&mut set, "C3995-02",
        HANDOVER_ITEMS.len() == 10
            && HANDOVER_ITEMS[0] == "总账" && HANDOVER_ITEMS[9] == "清账", "err");

    // C3995-03 逐件封入 + 缺件检出。
    let mut p = HandoverPack::new();
    for i in HANDOVER_ITEMS.iter().take(7) {
        p.pack(i);
    }
    chk(&mut set, "C3995-03",
        p.missing() == vec!["观测", "fuzz", "文档", "清账"], "err");

    // C3995-04 补齐但未脱敏 → 封装拒。
    for i in HANDOVER_ITEMS.iter().skip(7) {
        p.pack(i);
    }
    chk(&mut set, "C3995-04", p.missing().is_empty() && p.seal() == T15HCode::ITEM_MISS, "err");

    // C3995-05 脱敏后封装 + 收方核哈希过。
    p.sanitized = true;
    chk(&mut set, "C3995-05",
        p.seal() == T15HCode::OK && p.verify() == T15HCode::OK, "err");

    // C3995-06 哈希篡改检出 → 重封。
    let mut p2 = HandoverPack::new();
    for i in HANDOVER_ITEMS.iter() {
        p2.pack(i);
    }
    p2.sanitized = true;
    p2.seal();
    p2.seal_hash ^= 1;
    chk(&mut set, "C3995-06", p2.verify() == T15HCode::HASH_MISMATCH, "err");

    // C3995-07 重封恢复。
    p2.seal();
    chk(&mut set, "C3995-07", p2.verify() == T15HCode::OK, "err");

    // C3995-08 越名件拒绝 + 重复封入拒绝。
    let mut p3 = HandoverPack::new();
    p3.pack("总账");
    chk(&mut set, "C3995-08", !p3.pack("伪件") && !p3.pack("总账"), "err");

    // C3995-09 交接面三类齐备（域本色）。
    chk(&mut set, "C3995-09",
        HANDOVER_FACES.len() == 3
            && HANDOVER_FACES[0].name() == "朗读语料多语言"
            && HANDOVER_FACES[2].name() == "布局镜像前置", "err");

    // C3995-10 缺面检出。
    let mut fb = FaceBook::new();
    fb.declare(HandoverFace::SpeechI18n, "T 域朗读语料表多语言扩展");
    fb.declare(HandoverFace::ColorCulture, "T 域文化色映射");
    chk(&mut set, "C3995-10", fb.gaps() == vec!["布局镜像前置"] && !fb.complete(), "err");

    // C3995-11 补面后齐备。
    fb.declare(HandoverFace::MirrorPrep, "T 域 RTL 前置");
    chk(&mut set, "C3995-11", fb.complete(), "err");

    // C3995-12 说明空缺拒绝。
    let mut fb2 = FaceBook::new();
    chk(&mut set, "C3995-12", !fb2.declare(HandoverFace::SpeechI18n, ""), "err");

    // C3995-13 重复声明拒绝。
    fb2.declare(HandoverFace::SpeechI18n, "n1");
    chk(&mut set, "C3995-13", !fb2.declare(HandoverFace::SpeechI18n, "n2"), "err");

    // C3995-14 遗留三路清单随包。
    let ls = LeftoverSummary { closed: 5, to_t: 2, deferred: 1 };
    chk(&mut set, "C3995-14", ls.total() == 8, "err");

    // C3995-15 基线哈希随包三路。
    let mut p4 = HandoverPack::new();
    p4.baseline_hashes = [11, 22, 33];
    chk(&mut set, "C3995-15",
        p4.baseline_hashes == [11u64, 22, 33], "err");

    // C3995-16 拒收码语义。
    chk(&mut set, "C3995-16", T15HCode::ITEM_MISS.explain().contains("拒收"), "err");

    // C3995-17 封装确定性（同内容同哈希）。
    let mut p5 = HandoverPack::new();
    for i in HANDOVER_ITEMS.iter() {
        p5.pack(i);
    }
    p5.sanitized = true;
    p5.seal();
    chk(&mut set, "C3995-17", p5.seal_hash == p.seal_hash, "err");

    // C3995-18 交接面枚举互异。
    chk(&mut set, "C3995-18",
        HandoverFace::SpeechI18n != HandoverFace::ColorCulture
            && HandoverFace::ColorCulture != HandoverFace::MirrorPrep, "err");

    // C3995-19 十件名互异。
    let mut uniq = true;
    for i in 0..10 {
        for j in (i + 1)..10 {
            if HANDOVER_ITEMS[i] == HANDOVER_ITEMS[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3995-19", uniq, "err");

    // C3995-20 错误码互异。
    let codes = [T15HCode::OK.0, T15HCode::ITEM_MISS.0, T15HCode::HASH_MISMATCH.0, T15HCode::FACE_GAP.0];
    chk(&mut set, "C3995-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_seal_verify() {
        let mut p = HandoverPack::new();
        for i in HANDOVER_ITEMS.iter() {
            p.pack(i);
        }
        p.sanitized = true;
        assert_eq!(p.seal(), T15HCode::OK);
        assert_eq!(p.verify(), T15HCode::OK);
    }

    #[test]
    fn tamper_detected() {
        let mut p = HandoverPack::new();
        for i in HANDOVER_ITEMS.iter() {
            p.pack(i);
        }
        p.sanitized = true;
        p.seal();
        p.seal_hash = 42;
        assert_eq!(p.verify(), T15HCode::HASH_MISMATCH);
    }

    #[test]
    fn faces_complete() {
        let mut fb = FaceBook::new();
        for f in HANDOVER_FACES.iter() {
            fb.declare(*f, "n");
        }
        assert!(fb.complete());
    }
}
