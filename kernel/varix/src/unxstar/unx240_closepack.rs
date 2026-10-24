//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0240 · 波 01 A1 侧闭账物料包 · 诊断段 0xf010
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0240-J1 发布包 A1 侧件（报告/对账单/红线复核记录）齐套，
//!   缺件检出 100%。
//! 难点分析：
//!   发布包的 A1 侧物料是「域闭账」的最终凭证——报告、对账单、红线
//!   复核记录三类件缺一即发布作废。难点不在生成而在**齐套性判定**：
//!   物料清单是三层结构（类 → 件 → 份），件内可有多份（不同波次），
//!   「齐套」要求三层全绿；「缺件检出 100%」要求任意抽走一份都能
//!   被点名到「类-件-份」三级坐标。哈希校验再加一层：件在包内但
//!   内容被改 = 同样算缺件（缺的是「真件」）。
//! 纪律约束：
//!   - 清单驱动：齐套判定只认清单，不认包内实际有什么；
//!   - 三级坐标：缺件报告必须到份级，不许只报到类级；
//!   - 内容校验：FNV-1a 逐件哈希，改一字节即判缺；
//!   - 缺件检出 100%：任意单份抽走/改写必报。
//! 成本记账：清单 O(n)；齐套判定 O(n)；哈希 O(总字节)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf010;

/// FNV-1a 32 位（与全仓物料哈希口径同源）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ─────────────────────────────────────────────────────────────────────
// 一、物料模型：类 → 件 → 份 三层
// ─────────────────────────────────────────────────────────────────────

/// 物料大类（A1 侧闭账三件套）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackClass {
    /// 域进度报告。
    Report,
    /// 批次对账单。
    Recon,
    /// 红线复核记录。
    Redline,
}

impl PackClass {
    pub fn tag(self) -> &'static str {
        match self {
            PackClass::Report => "报告",
            PackClass::Recon => "对账单",
            PackClass::Redline => "红线复核",
        }
    }
}

/// 一份物料实体。
#[derive(Clone, Debug)]
pub struct Piece {
    /// 所属类。
    pub class: PackClass,
    /// 件名（类内唯一）。
    pub item: &'static str,
    /// 份号（件内唯一，波次号）。
    pub wave: u8,
    /// 内容。
    pub body: Vec<u8>,
}

impl Piece {
    /// 三级坐标（缺件报告的定位粒度）。
    pub fn coord(&self) -> String {
        alloc::format!("{}/{}/{}", self.class.tag(), self.item, self.wave)
    }

    pub fn hash(&self) -> u32 {
        fnv1a(&self.body)
    }
}

/// 清单条目：要求「某类某件至少存在到某波次」。
#[derive(Clone, Copy, Debug)]
pub struct ManifestEntry {
    pub class: PackClass,
    pub item: &'static str,
    /// 要求覆盖的最大波次号（1..=max_wave 全覆盖）。
    pub max_wave: u8,
}

/// A1 侧闭账标准清单（波 01 口径）。
pub fn wave01_manifest() -> Vec<ManifestEntry> {
    vec![
        ManifestEntry { class: PackClass::Report, item: "域进度报告", max_wave: 1 },
        ManifestEntry { class: PackClass::Recon, item: "批次对账单", max_wave: 1 },
        ManifestEntry { class: PackClass::Redline, item: "红线复核记录", max_wave: 1 },
    ]
}

// ─────────────────────────────────────────────────────────────────────
// 二、齐套判定：清单驱动 + 内容校验 + 三级坐标点名
// ─────────────────────────────────────────────────────────────────────

/// 缺件记录（三级坐标 + 缺因）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissKind {
    /// 件整体缺失。
    Absent,
    /// 波次份缺失。
    WaveMissing,
    /// 内容哈希不符（件在但内容不是真的）。
    ContentMismatch,
}

#[derive(Clone, Debug)]
pub struct MissRecord {
    pub class: PackClass,
    pub item: &'static str,
    pub wave: u8,
    pub kind: MissKind,
}

impl MissRecord {
    pub fn coord(&self) -> String {
        let kind = match self.kind {
            MissKind::Absent => "缺件",
            MissKind::WaveMissing => "缺份",
            MissKind::ContentMismatch => "内容不符",
        };
        alloc::format!("{}/{}/{} {}", self.class.tag(), self.item, self.wave, kind)
    }
}

/// 黄金哈希登记表：件坐标 → 应有哈希。
/// 发布前由「生成侧」登记；齐套判定只信这张表。
pub struct HashLedger {
    rows: Vec<((PackClass, &'static str, u8), u32)>,
}

impl HashLedger {
    pub fn new() -> Self {
        HashLedger { rows: Vec::new() }
    }

    pub fn register(&mut self, class: PackClass, item: &'static str, wave: u8, body: &[u8]) {
        let key = (class, item, wave);
        let h = fnv1a(body);
        match self.rows.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = h,
            None => self.rows.push((key, h)),
        }
    }

    pub fn lookup(&self, class: PackClass, item: &str, wave: u8) -> Option<u32> {
        self.rows
            .iter()
            .find(|((c, i, w), _)| *c == class && *i == item && *w == wave)
            .map(|(_, h)| *h)
    }

    /// 账目一致性：登记键无重复、波次号非零。
    pub fn coherent(&self) -> bool {
        let n = self.rows.len();
        for i in 0..n {
            if self.rows[i].0 .2 == 0 {
                return false;
            }
            for j in i + 1..n {
                if self.rows[i].0 .0 == self.rows[j].0 .0
                    && self.rows[i].0 .1 == self.rows[j].0 .1
                    && self.rows[i].0 .2 == self.rows[j].0 .2
                {
                    return false;
                }
            }
        }
        true
    }
}

/// 齐套判定入口。
pub fn audit(pack: &[Piece], manifest: &[ManifestEntry], ledger: &HashLedger) -> Vec<MissRecord> {
    let mut out = Vec::new();
    for m in manifest {
        for wave in 1..=m.max_wave {
            let hit = pack
                .iter()
                .find(|p| p.class == m.class && p.item == m.item && p.wave == wave);
            match hit {
                None => {
                    // 件整体缺失 vs 单份缺失：看同件其他波次在不在。
                    let item_exists = pack
                        .iter()
                        .any(|p| p.class == m.class && p.item == m.item);
                    out.push(MissRecord {
                        class: m.class,
                        item: m.item,
                        wave,
                        kind: if item_exists { MissKind::WaveMissing } else { MissKind::Absent },
                    });
                }
                Some(p) => {
                    if let Some(expect) = ledger.lookup(m.class, m.item, wave) {
                        if p.hash() != expect {
                            out.push(MissRecord {
                                class: m.class,
                                item: m.item,
                                wave,
                                kind: MissKind::ContentMismatch,
                            });
                        }
                    }
                    // 无登记 = 生成侧漏账，按内容不符判（宁可误报不放过）。
                    else {
                        out.push(MissRecord {
                            class: m.class,
                            item: m.item,
                            wave,
                            kind: MissKind::ContentMismatch,
                        });
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────
// 三、黄金包夹具：三类各一件（波 01）
// ─────────────────────────────────────────────────────────────────────

/// 黄金包：清单全齐 + 哈希账同源登记。
pub fn golden_pack() -> (Vec<Piece>, HashLedger) {
    let mut ledger = HashLedger::new();
    let mut pack = Vec::new();
    for m in wave01_manifest() {
        let body: Vec<u8> = {
            let mut b = Vec::new();
            b.extend_from_slice(m.item.as_bytes());
            b.extend_from_slice(b" wave-01 golden");
            b
        };
        ledger.register(m.class, m.item, 1, &body);
        pack.push(Piece { class: m.class, item: m.item, wave: 1, body });
    }
    (pack, ledger)
}

// ─────────────────────────────────────────────────────────────────────
// 四、闭账物料包：发布门（齐套才许闭账）
// ─────────────────────────────────────────────────────────────────────

/// 发布门判定结果。
#[derive(Debug)]
pub struct CloseVerdict {
    pub complete: bool,
    pub misses: Vec<MissRecord>,
}

/// 闭账发布门：零缺件才许闭。
pub fn close_gate(pack: &[Piece], manifest: &[ManifestEntry], ledger: &HashLedger) -> CloseVerdict {
    let misses = audit(pack, manifest, ledger);
    CloseVerdict { complete: misses.is_empty(), misses }
}

// ─────────────────────────────────────────────────────────────────────
// 五、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0240 判据全集。
pub fn run_unx240_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0240 波 01 A1 侧闭账物料包");
    let manifest = wave01_manifest();

    // J1 主判据：黄金包齐套
    let (pack, ledger) = golden_pack();
    cs.add("J1 账自洽", ledger.coherent(), "键无重复/波次非零");
    let v0 = close_gate(&pack, &manifest, &ledger);
    cs.add("J1 黄金包齐套", v0.complete && v0.misses.is_empty(), "三类全绿");

    // 注入 1：抽走整件（红线复核记录）→ 缺件 + 类级坐标
    let (p1, l1) = golden_pack();
    let thin: Vec<Piece> = p1
        .into_iter()
        .filter(|p| p.class != PackClass::Redline)
        .collect();
    let v1 = close_gate(&thin, &manifest, &l1);
    cs.add(
        "注入1 缺整件检出",
        !v1.complete
            && v1.misses.len() == 1
            && v1.misses[0].kind == MissKind::Absent
            && v1.misses[0].class == PackClass::Redline,
        "类-件-份三级坐标",
    );
    cs.add_dyn("注入1 坐标可读", v1.misses.len() == 1 && v1.misses[0].coord().contains("红线复核"), &format!(
        "坐标={}",
        if v1.misses.is_empty() { "?".to_string() } else { v1.misses[0].coord() }
    ));

    // 注入 2：内容改一字节 → 内容不符（件在但不是真的）
    let (mut p2, l2) = golden_pack();
    p2[0].body[0] ^= 0x01;
    let v2 = close_gate(&p2, &manifest, &l2);
    cs.add(
        "注入2 内容不符检出",
        !v2.complete && v2.misses.len() == 1 && v2.misses[0].kind == MissKind::ContentMismatch,
        "哈希异=缺真件",
    );

    // 注入 3：账漏登记（生成侧漏账）→ 宁误报不放过
    let (p3, mut l3) = golden_pack();
    l3.rows.pop();
    let v3 = close_gate(&p3, &manifest, &l3);
    cs.add("注入3 漏账检出", !v3.complete && v3.misses.len() == 1, "无登记按不符判");

    // 注入 4：抽走一件的两份中一份（双波次件）→ 缺份而非缺件
    let mut manifest2 = wave01_manifest();
    manifest2[0].max_wave = 2; // 报告要求两波
    let (mut p4, mut l4) = golden_pack();
    // 补登记第二波并入场
    let mut body2 = Vec::new();
    body2.extend_from_slice("域进度报告".as_bytes());
    body2.extend_from_slice(b" wave-02 golden");
    l4.register(PackClass::Report, "域进度报告", 2, &body2);
    p4.push(Piece { class: PackClass::Report, item: "域进度报告", wave: 2, body: body2 });
    // 抽走第二波
    let thin4: Vec<Piece> = p4.into_iter().filter(|p| p.wave != 2).collect();
    let v4 = close_gate(&thin4, &manifest2, &l4);
    cs.add(
        "注入4 缺份检出",
        !v4.complete
            && v4.misses.len() == 1
            && v4.misses[0].kind == MissKind::WaveMissing
            && v4.misses[0].wave == 2,
        "件在份缺",
    );

    // 注入 5：所有份全抽 → 齐套判定仍 100% 点名（三类全缺）
    let v5 = close_gate(&[], &manifest, &golden_pack().1);
    cs.add(
        "注入5 空包全检出",
        !v5.complete && v5.misses.len() == 3 && v5.misses.iter().all(|m| m.kind == MissKind::Absent),
        "100% 缺件检出",
    );

    // 诊断码闭集：缺因三分量互异
    cs.add(
        "缺因三分量互异",
        MissKind::Absent != MissKind::WaveMissing
            && MissKind::WaveMissing != MissKind::ContentMismatch
            && MissKind::Absent != MissKind::ContentMismatch,
        "三态闭集",
    );

    cs
}
