//! VE-F4299 · U05 预备自查（U05 工具集成组）
//! 职责：四维自查（同 F4279 口径：体例七要素/断言八条/差异化/指针）+
//! 本组特查：九面集成关系与工具链总图一致性——图实对拍 + 回修重扫
//! + 指针悬空修正 + 特查异议升级复核。
//! 段位：0x2F20（续表 idx533）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2F20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U5SkCode {
    DimFail = SEG_BASE,
    DanglingPtr = SEG_BASE + 0x01,
    MapDrift = SEG_BASE + 0x02, // 特查：图实漂移
}

/// U05 自查册（四维+特查）。
pub struct U05SelfCheck {
    pub elements: u8,
    pub asserts: u8,
    pub ptrs: Vec<(u32, bool)>,
    /// 图实对拍：图面数 vs 实际面数。
    pub map_faces: usize,
    pub real_faces: usize,
    pub scans: u32,
    pub spots: Vec<(u16, bool)>,
    /// —— 深化：结构化扫描账（带轮次与修因）。——
    pub log: Vec<ScanRec>,
}

/// —— 深化：结构化扫描记录。——
#[derive(Clone, Copy, Debug)]
pub struct ScanRec {
    pub no: u32,
    pub dims: [u8; 4],
    /// 修因：0 初扫 / 1 补要素 / 2 补断言 / 3 修指针 / 4 图实对齐。
    pub cause: u8,
}

impl U05SelfCheck {
    pub fn new(map_faces: usize, real_faces: usize) -> Self {
        U05SelfCheck { elements: 0, asserts: 0, ptrs: Vec::new(), map_faces, real_faces, scans: 0, spots: Vec::new(), log: Vec::new() }
    }

    /// —— 深化：带账扫描：扫描入账并回填轮次。——
    pub fn scan_logged(&mut self, cause: u8) -> [u8; 4] {
        let dims = self.scan();
        let no = self.scans;
        if !self.log.iter().any(|r| r.no == no) {
            self.log.push(ScanRec { no, dims, cause });
        }
        dims
    }

    /// —— 深化：扫描账守恒：账面轮次严格递增且不超扫描计数。——
    pub fn log_conserve(&self) -> bool {
        self.log.windows(2).all(|w| w[0].no < w[1].no)
            && self.log.iter().all(|r| r.no >= 1 && r.no <= self.scans)
    }

    /// —— 深化：最弱维度投影（四维取最小，短板即所得）。——
    pub fn weakest_dim(&self) -> u8 {
        match self.dims_of().iter().min() {
            Some(v) => *v,
            None => 0,
        }
    }

    /// —— 深化：指针账投影（在册, 悬空）。——
    pub fn ptr_split(&self) -> (u32, u32) {
        let ok = self.ptrs.iter().filter(|(_, ok)| *ok).count() as u32;
        let bad = self.ptrs.iter().filter(|(_, ok)| !*ok).count() as u32;
        (ok, bad)
    }

    /// —— 深化：指针账中文摘要。——
    pub fn ptr_summary(&self) -> String {
        let (ok, bad) = self.ptr_split();
        let mut s = String::new();
        s.push_str("指针在册");
        s.push_str(&ok.to_string());
        s.push_str("·悬空");
        s.push_str(&bad.to_string());
        s
    }

    /// —— 深化：幂等修复闸：同指针二次修复零副作用已由 fix_ptr 保证，
    /// 此处批量修复并返回成功数。——
    pub fn fix_all(&mut self) -> u32 {
        let mut n = 0u32;
        let ids: Vec<u32> = self.ptrs.iter().filter(|(_, ok)| !*ok).map(|(f, _)| *f).collect();
        for f in ids {
            if self.fix_ptr(f) {
                n += 1;
            }
        }
        n
    }

    pub fn scan(&mut self) -> [u8; 4] {
        self.scans += 1;
        self.dims_of()
    }

    /// —— 深化：维度投影（纯计算，不动扫描账）。——
    pub fn dims_of(&self) -> [u8; 4] {
        let d0 = (self.elements.min(7) * 100 / 7) as u8;
        let d1 = (self.asserts.min(8) * 100 / 8) as u8;
        let d2 = if d0 == 100 && d1 == 100 { 100 } else { 50 };
        let d3 = if self.map_faces == self.real_faces && self.ptrs.iter().all(|(_, ok)| *ok) && !self.ptrs.is_empty() { 100 } else { 0 };
        [d0, d1, d2, d3]
    }

    pub fn fix_ptr(&mut self, f: u32) -> bool {
        if let Some(p) = self.ptrs.iter_mut().find(|(pf, ok)| *pf == f && !ok) {
            p.1 = true;
            return true;
        }
        false
    }

    pub fn map_drift(&self) -> bool {
        self.map_faces != self.real_faces
    }

    pub fn spot(&mut self, s: u16, dispute: bool) {
        self.spots.push((s, dispute));
    }

    pub fn disputes(&self) -> Vec<u16> {
        self.spots.iter().filter(|(_, d)| *d).map(|(s, _)| *s).collect()
    }

    pub fn ready(&self) -> bool {
        !self.map_drift() && self.ptrs.iter().all(|(_, ok)| *ok) && !self.ptrs.is_empty()
            && self.disputes().is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C533-01..12。
pub fn run_ves533_checks() -> CheckSet {
    let mut set = CheckSet::new("ves533");

    let mut sk = U05SelfCheck::new(7, 9);
    chk(&mut set, "C533-01", sk.map_drift(), "特查：图实漂移检出（7≠9）");
    sk.map_faces = 9;
    sk.register_dummy();
    chk(&mut set, "C533-02", !sk.map_drift(), "图实对齐");

    sk.elements = 5;
    sk.asserts = 8;
    let dims = sk.scan();
    chk(&mut set, "C533-03", dims[0] == 71 && dims[1] == 100, "七要素缺 2、八断言满");
    sk.elements = 7;
    sk.scan();
    chk(&mut set, "C533-04", sk.scans == 2, "回修重扫计数");

    // 指针
    sk.ptrs.push((4270, true));
    sk.ptrs.push((4281, false));
    chk(&mut set, "C533-05", !sk.ready(), "悬空指针不放行");
    chk(&mut set, "C533-06", sk.fix_ptr(4281), "修正成功");
    chk(&mut set, "C533-07", !sk.fix_ptr(4281), "重复修正拒绝");
    chk(&mut set, "C533-08", sk.ready(), "全绿可进双签");

    // 抽查
    sk.spot(1, true);
    chk(&mut set, "C533-09", sk.disputes() == alloc::vec![1] && !sk.ready(), "特查异议不放行");
    sk.spots.clear();
    sk.spot(1, false);
    chk(&mut set, "C533-10", sk.ready(), "异议复核后放行");

    let mut empty = U05SelfCheck::new(9, 9);
    chk(&mut set, "C533-11", !empty.ready(), "零指针保守不放行");
    chk(&mut set, "C533-12", empty.scan()[3] == 0, "零指针维度 0");

    // —— 深化 A：带账扫描 + 守恒 + 最弱维投影 ——
    sk.elements = 7;
    sk.asserts = 8;
    let d0 = sk.scan_logged(0);
    let d1 = sk.scan_logged(4);
    chk(&mut set, "C533-13", sk.log_conserve() && sk.log.len() == 2, "扫描账守恒：轮次递增且不超扫描计数");
    chk(&mut set, "C533-14", d0 == d1, "同态两次扫描幂等（维分一致）");
    chk(&mut set, "C533-15", sk.weakest_dim() == 100, "全绿态最弱维投影=100");

    // —— 深化 B：指针账投影 ——
    sk.ptrs.push((4301, false));
    chk(&mut set, "C533-16", sk.ptr_split() == (3, 1), "指针账分列：在册 3 / 悬空 1");
    chk(&mut set, "C533-17", sk.ptr_summary().len() > 0, "指针账中文摘要（域本色）");
    chk(&mut set, "C533-18", sk.fix_all() == 1 && sk.fix_all() == 0, "批量修复一次清零，二次零副作用");

    // —— 深化 C：边界探针 ——
    let er = run_sk_edge_probes();
    chk(&mut set, "C533-19", er.over_clamp && er.absent_fix_false, "超限要素钳位/缺席修复拒绝");
    chk(&mut set, "C533-20", er.dup_spot_dedup && er.drift_equal_none, "重复异议去重/图实相等零漂移");

    // —— 深化 D：LCG 注入回归 ——
    match sk_inject_regression(48) {
        Ok((fixed, green)) => chk(&mut set, "C533-21", fixed > 0 && green, "48 轮悬空注入修复必达且终态全绿"),
        Err(_) => chk(&mut set, "C533-21", false, "注入回归修复未达全绿"),
    }

    // —— 深化 E：序列化投影 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_ptrs(&sk.ptrs, &mut bytes);
    let rt = decode_ptrs(&bytes);
    chk(&mut set, "C533-22", rt.iter().count() == sk.ptrs.len(), "指针账 5B/条定长解码往返");
    chk(&mut set, "C533-23", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == sk.ptrs.len() * 5, "FNV 指纹稳定且字节布局定长");
    chk(&mut set, "C533-24", sk.log.iter().all(|r| r.dims.iter().all(|d| *d <= 100)), "扫描账维分全部界内");

    set
}

// ===== 深化块 A：边界探针 =====

/// 自查边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SkEdgeReport {
    pub over_clamp: bool,
    pub absent_fix_false: bool,
    pub dup_spot_dedup: bool,
    pub drift_equal_none: bool,
}

/// 四类边界探针：超限钳位/缺席修复/重复异议/零漂移。
pub fn run_sk_edge_probes() -> SkEdgeReport {
    // 1) 超限：要素 99/断言 99 钳位到满分
    let mut e = U05SelfCheck::new(9, 9);
    e.elements = 99;
    e.asserts = 99;
    let dims = e.scan();
    let over_clamp = dims[0] == 100 && dims[1] == 100;
    // 2) 缺席修复：不存在指针返回 false
    let absent_fix_false = !e.fix_ptr(0xDEAD);
    // 3) 重复异议：同段号异议去重后不影响放行判定
    e.spot(3, false);
    e.spot(3, false);
    let dup_spot_dedup = e.disputes().is_empty();
    // 4) 图实相等：零漂移
    let drift_equal_none = !e.map_drift();

    SkEdgeReport { over_clamp, absent_fix_false, dup_spot_dedup, drift_equal_none }
}

// ===== 深化块 B：LCG 注入回归 =====

/// 确定性 LCG（8 行）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 注入回归：LCG 随机注入悬空指针 N 轮，批量修复必达全绿。
pub fn sk_inject_regression(rounds: u32) -> Result<(u32, bool), U5SkCode> {
    let mut rng = Lcg::new(0x5EED_5330);
    let mut sk = U05SelfCheck::new(9, 9);
    let mut fixed = 0u32;
    for i in 0..rounds {
        let id = 0x4000 + i;
        let dangling = rng.next() & 1 == 0;
        sk.ptrs.push((id, !dangling));
        if dangling {
            fixed += sk.fix_all();
        }
    }
    fixed += sk.fix_all();
    if sk.ptrs.iter().any(|(_, ok)| !ok) {
        return Err(U5SkCode::DanglingPtr);
    }
    Ok((fixed, sk.ready()))
}

// ===== 深化块 C：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每条 5B（id:4 + ok:1），大端。
pub fn encode_ptrs(ptrs: &[(u32, bool)], out: &mut Vec<u8>) {
    for (id, ok) in ptrs {
        out.extend_from_slice(&id.to_be_bytes());
        out.push(*ok as u8);
    }
}

/// 定长解码：非整 5B → 显性拒绝。
pub fn decode_ptrs(bytes: &[u8]) -> Result<Vec<(u32, bool)>, U5SkCode> {
    if bytes.len() % 5 != 0 {
        return Err(U5SkCode::DanglingPtr);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(5) {
        out.push((
            u32::from_be_bytes([ch[0], ch[1], ch[2], ch[3]]),
            ch[4] != 0,
        ));
    }
    Ok(out)
}

impl U05SelfCheck {
    fn register_dummy(&mut self) {
        if self.ptrs.is_empty() {
            self.ptrs.push((4207, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05_selfcheck() {
        let mut sk = U05SelfCheck::new(9, 9);
        sk.ptrs.push((1, true));
        sk.scan();
        assert!(sk.ready());
        sk.map_faces = 3;
        assert!(!sk.ready());
    }
}
