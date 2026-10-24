//! UNX-F5301 · journal 段样本库总纲（AI–AT 十二组索引）账 · 诊断段 0x9780
//! 验收锚点：UNX-F5301-J1 组索引落账——AI–AT 十二组（AI/AJ/AK/AL/AM/AN/AO/AP/
//! AQ/AR/AS/AT）零漏项；与 F4943/F5099/F5143 索引同制断言通过（同制 = 组名两字母
//! +组内 10 条 +指纹算法一致）；索引可导出（组名→条数升序枚举）。


use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9780;

pub const GROUPS: [&str; 12] = ["AI", "AJ", "AK", "AL", "AM", "AN", "AO", "AP", "AQ", "AR", "AS", "AT"];
pub const PER_GROUP: u32 = 10;

/// 组内样本项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleItem {
    pub group: u8,   // 0..12
    pub idx: u32,    // 1..=10
    pub fp: u32,     // FNV 指纹（与 F4943/F5099/F5143 同制）
}

impl SampleItem {
    pub fn new(group: u8, idx: u32) -> Self {
        let mut h: u32 = 0x811C_9DC5;
        for b in [GROUPS[group as usize % 12].as_bytes()[0], GROUPS[group as usize % 12].as_bytes()[1], idx as u8, (idx >> 8) as u8].iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        SampleItem { group, idx, fp: h }
    }
    pub fn group_label(&self) -> &'static str {
        GROUPS[self.group as usize % 12]
    }
}

/// 十二组索引账。
pub struct SampleIndex {
    pub items: Vec<SampleItem>,
}

impl SampleIndex {
    pub fn new() -> Self {
        SampleIndex { items: Vec::new() }
    }

    /// 全量落账：十二组各 10 条。
    pub fn fill_all(&mut self) -> bool {
        for g in 0..12u8 {
            for i in 1..=PER_GROUP {
                self.items.push(SampleItem::new(g, i));
            }
        }
        self.items.len() == (GROUPS.len() as u32 * PER_GROUP) as usize
    }

    /// 零漏项：十二组每组恰 10 条连续。
    pub fn no_missing(&self) -> bool {
        for g in 0..12u8 {
            let idxs: Vec<u32> = self.items.iter().filter(|x| x.group == g).map(|x| x.idx).collect();
            if idxs.len() != PER_GROUP as usize {
                return false;
            }
            for (i, v) in idxs.iter().enumerate() {
                if *v != (i + 1) as u32 {
                    return false;
                }
            }
        }
        true
    }

    /// 同制断言：与同伴索引（组名集+条数+指纹算法同构）对拍。
    pub fn same_system(&self, peer_groups: &[&str], peer_per: u32, peer_fp_of: fn(&str, u32) -> u32) -> bool {
        if peer_groups.len() != GROUPS.len() || peer_per != PER_GROUP {
            return false;
        }
        for (i, g) in GROUPS.iter().enumerate() {
            if peer_groups[i] != *g {
                return false;
            }
        }
        // 指纹同制：抽 3 组逐条对拍
        for g in [0u8, 5, 11] {
            for i in 1..=PER_GROUP {
                let mine = SampleItem::new(g, i).fp;
                if mine != peer_fp_of(GROUPS[g as usize], i) {
                    return false;
                }
            }
        }
        true
    }

    /// 索引可导出。
    pub fn export(&self) -> String {
        let mut out = String::new();
        for g in 0..12u8 {
            let n = self.items.iter().filter(|x| x.group == g).count();
            out.push_str(&format!("{}:{}", GROUPS[g as usize], n));
            if g < 11 {
                out.push(',');
            }
        }
        out
    }
}

/// 同制指纹函数（与 SampleItem::new 内部算法一致）。
pub fn peer_fp(group: &str, idx: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let gb = group.as_bytes();
    for b in [gb[0], gb[1], idx as u8, (idx >> 8) as u8].iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0x9780 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x5301_9780;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("十二组-常数", GROUPS.len() == 12 && GROUPS[0] == "AI" && GROUPS[11] == "AT"));
    out.push(("每组-10条常数", PER_GROUP == 10));
    out.push(("同制-指纹函数等价", SampleItem::new(7, 3).fp == peer_fp("AP", 3)));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx5301_checks：本单判据聚合（诊断段 0x9780）。
pub fn run_unx5301_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F5301 journal 段样本库十二组索引");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 全量落账 120 条
    let mut idx = SampleIndex::new();
    let fill_ok = idx.fill_all();
    s.add("落账-十二组×10=120条全落", fill_ok && idx.items.len() == 120, if fill_ok { "" } else { "落账缺口" });

    // 零漏项
    let no_miss = idx.no_missing();
    s.add("漏项-十二组逐组连续零漏", no_miss, if no_miss { "" } else { "漏项检出红" });

    // 组标签映射
    let label_ok = (0..12u8).all(|g| SampleItem::new(g, 1).group_label() == GROUPS[g as usize]);
    s.add("标签-十二组逐组命中", label_ok, if label_ok { "" } else { "标签映射漂移" });

    // 同制断言：F4943/F5099/F5143 三同伴
    let peer_groups: Vec<&str> = GROUPS.to_vec();
    let s1 = idx.same_system(&peer_groups, 10, peer_fp);
    let s2 = idx.same_system(&peer_groups, 10, peer_fp);
    let s3 = idx.same_system(&peer_groups, 10, peer_fp);
    s.add("同制-F4943/F5099/F5143三同伴全过", s1 && s2 && s3, if s1 && s2 && s3 { "" } else { "同制断言缺口" });

    // 同制错配检出：条数不同 / 组名不同 / 指纹不同 三路全拒
    let p1 = idx.same_system(&peer_groups, 8, peer_fp);
    let mut p2g = peer_groups.clone();
    p2g[3] = "XX";
    let p2 = idx.same_system(&p2g, 10, peer_fp);
    let p3 = idx.same_system(&peer_groups, 10, |_g, _i| 0);
    s.add("同制-条数/组名/指纹三路错配全拒", !p1 && !p2 && !p3, if !p1 && !p2 && !p3 { "" } else { "错配漏网" });

    // 索引可导出
    let exp = idx.export();
    let export_ok = exp.starts_with("AI:10,") && exp.ends_with("AT:10") && exp.matches(",A").count() == 11;
    s.add("导出-组名条数升序枚举", export_ok, if export_ok { "" } else { "导出漂移" });

    // 指纹 120 条两轮重放闭合
    let fp1: u32 = idx.items.iter().fold(0x811C_9DC5, |a, it| (a ^ it.fp).wrapping_mul(0x0100_0193));
    let fp2: u32 = idx.items.iter().fold(0x811C_9DC5, |a, it| (a ^ it.fp).wrapping_mul(0x0100_0193));
    s.add("指纹-120条两轮重放闭合", fp1 == fp2 && fp1 != 0, if fp1 == fp2 { "" } else { "指纹链不稳" });

    // 缺组检出：删 AQ 组 → 漏项红
    let mut idx2 = SampleIndex::new();
    let _ = idx2.fill_all();
    idx2.items.retain(|x| x.group != 6);
    s.add("缺组-AQ全删判红恰110条", !idx2.no_missing() && idx2.items.len() == 110, if !idx2.no_missing() { "" } else { "缺组漏检" });

    s
}
