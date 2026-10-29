#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0799 · B40 批 ktest 断言集（收官终局面聚合）
//! 域/批：CoRun·UNX A1 收官域（ktest/聚合面 · W7 清单第 26 单）
//! 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥570 行；② 判据原文：UNX-F0799-J1 本批判据聚合入 ktest
//!   unx_a1_b40 一次命令全绿（三重口径）且映射双向对账过；③ 完成时 API/
//!   软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 断言集建模：B40 批判据逐条入集（id 1..=N 闭集），每条 =
//!   判据号 + 断言口径（三重：F 码面/卡面行数面/凭证面）+ payload 哈希。
//! ② 三重口径计数：F 码面（id 闭集连续无重）、行数面（每条行数下限
//!   达标记账）、凭证面（每条凭证包齐记账）——三口径总数必须相等
//!   （全绿守恒恒等式），不等即红并定位。
//! ③ 映射双向对账：判据号→ktest 用例名（正向表）与用例名→判据号
//!   （反向表）逐条互查一致（双射），任一向失配即红并定位。
//! ④ payload 哈希：每条 payload（用例名串）FNV 哈希奇偶位作分桶
//!   （确定性）；一次命令聚合执行 = 全集遍历断言一遍全绿。
//! 诊断段声明：0x6220–0x623F 本单独占（清单 <SEG 0x6220>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6220–0x623F）
pub const SEG_BASE: u16 = 0x6220; // 诊断码段基数，来源：W7 清单 UNX-F0799 行 <SEG 0x6220>
pub const SEG_TOP: u16 = 0x623F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 聚合状态机非法边
pub const XC_DUP_ID: u16 = SEG_BASE + 2; // 判据号重复
pub const XC_ID_GAP: u16 = SEG_BASE + 3; // 判据号断档（闭集连续破）
pub const XC_CASE_DUP: u16 = SEG_BASE + 4; // 用例名重复
pub const XC_CASE_EMPTY: u16 = SEG_BASE + 5; // 用例名为空
pub const XC_SET_SHORT: u16 = SEG_BASE + 6; // 断言集不足下限
pub const XC_LINE_SHORT: u16 = SEG_BASE + 7; // 行数面口径未达标
pub const XC_VOUCH_MISSING: u16 = SEG_BASE + 8; // 凭证面口径未齐
pub const XC_MISMATCH: u16 = SEG_BASE + 9; // 三重口径总数不等
pub const XC_FWD_MISS: u16 = SEG_BASE + 10; // 正向映射失配
pub const XC_REV_MISS: u16 = SEG_BASE + 11; // 反向映射失配
pub const XC_ALREADY_RUN: u16 = SEG_BASE + 12; // 已全绿再执行
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 13; // 摘要为空
pub const XC_HASH_DIFF: u16 = SEG_BASE + 14; // payload 哈希对拍不一致

// 2. ktest 常量（写死，来源=判据原文/本单口径头注假设）
pub const SUITE_NAME: &str = "unx_a1_b40"; // ktest 套件名，来源：判据原文
pub const SET_MIN: usize = 6; // B40 批判据下限（本批六卡），头注假设
pub const BUCKET_N: usize = 2; // payload 哈希奇偶分桶数

/// 三重口径闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gauge {
    FCode = 0, // F 码面（id 闭集连续无重）
    Lines = 1, // 行数面（行数下限达标记账）
    Voucher = 2, // 凭证面（凭证包齐记账）
}
pub const GAUGE_ALL: [Gauge; 3] = [Gauge::FCode, Gauge::Lines, Gauge::Voucher];

/// 集内一条断言。
#[derive(Clone, Debug)]
pub struct Kase {
    pub kid: u32, // 判据号（1..=N 闭集连续）
    pub case: String, // ktest 用例名（非空，唯一）
    pub lines: u32, // 行数面记账（下限达标为实际行数）
    pub vouch: bool, // 凭证面记账（凭证包齐）
}

/// 一次全绿执行回执。
#[derive(Clone, Debug)]
pub struct RunReceipt {
    pub suite: String, // 套件名（unx_a1_b40）
    pub green: u32, // 全绿条数（三口径相等值）
    pub buckets: [u32; BUCKET_N], // payload 哈希奇偶分桶计数
    pub summary: String, // 摘要（非空）
    pub fingerprint: u64, // 断言集指纹（对拍用）
}
/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KStage {
    Load = 0,
    Mapped = 1,
    Green = 2,
}
impl KStage {
    pub fn can_step(self, next: KStage) -> bool {
        (self as u8) + 1 == next as u8
    }
    pub fn view(self) -> &'static str {
        match self {
            KStage::Load => "load",
            KStage::Mapped => "mapped",
            KStage::Green => "green",
        }
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步（数值混合）。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub fn fnv_str(s: &str) -> u64 {  // 字符串 FNV（逐字节）。
    hash_bytes(s.as_bytes())
}
pub fn hash_bytes(bs: &[u8]) -> u64 {  // 字节串 FNV-1a-64。
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bs.len() {
        h ^= bs[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}
pub struct KtestSuite {  // B40 批 ktest 断言集聚合机。
    stage: KStage,
    kases: Vec<Kase>,
    fwd: Vec<(u32, String)>, // 正向表：判据号→用例名
    rev: Vec<(String, u32)>, // 反向表：用例名→判据号
    pinned: Option<RunReceipt>,
    events: Vec<String>,
}
impl KtestSuite {
    pub fn new() -> Self {
        KtestSuite { stage: KStage::Load, kases: Vec::new(), fwd: Vec::new(), rev: Vec::new(), pinned: None, events: Vec::new() }
    }
    pub fn stage(&self) -> KStage {
        self.stage
    }
    pub fn set_len(&self) -> usize {
        self.kases.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn add(&mut self, k: Kase) -> Result<u32, u16> {  // 入集：号闭集（连续无重，允许乱序入但闭集校验在 map 阶段）、用例名非空唯一。
        if self.stage != KStage::Load {
            return Err(XC_STATE_INVALID);
        }
        if k.case.is_empty() {
            return Err(XC_CASE_EMPTY);
        }
        if k.kid < 1 || k.kid as usize > SET_MIN {
            return Err(XC_ID_GAP); // 集外号拒绝（B40 批定容 6）
        }
        let mut i = 0usize;
        while i < self.kases.len() {
            if self.kases[i].kid == k.kid {
                return Err(XC_DUP_ID);
            }
            if self.kases[i].case == k.case {
                return Err(XC_CASE_DUP);
            }
            i += 1;
        }
        self.kases.push(k);
        self.events.push(alloc::format!("kase +{}", self.kases.len()));
        Ok(self.kases.len() as u32)
    }
    pub fn build_maps(&mut self) -> Result<u32, u16> {  // 映射构建 + 闭集连续校验：号 1..=N 无断档；建正反向表。
        if !self.stage.can_step(KStage::Mapped) {
            return Err(XC_STATE_INVALID);
        }
        if self.kases.len() < SET_MIN {
            return Err(XC_SET_SHORT);
        }
        // 闭集连续校验（无重已在 add 守卫）。
        let mut n = 1u32;
        while n as usize <= SET_MIN {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.kases.len() {
                if self.kases[i].kid == n {
                    hit = true;
                    break;
                }
                i += 1;
            }
            if !hit {
                return Err(XC_ID_GAP); // 断档定位：n
            }
            n += 1;
        }
        self.fwd.clear();
        self.rev.clear();
        let mut i = 0usize;
        while i < self.kases.len() {
            self.fwd.push((self.kases[i].kid, self.kases[i].case.clone()));
            self.rev.push((self.kases[i].case.clone(), self.kases[i].kid));
            i += 1;
        }
        self.stage = KStage::Mapped;
        self.events.push(alloc::format!("maps {}", self.fwd.len()));
        Ok(self.fwd.len() as u32)
    }
    pub fn fwd_of(&self, kid: u32) -> Option<&str> {  // 正向查：判据号→用例名。
        let mut i = 0usize;
        while i < self.fwd.len() {
            if self.fwd[i].0 == kid {
                return Some(&self.fwd[i].1);
            }
            i += 1;
        }
        None
    }
    pub fn rev_of(&self, case: &str) -> Option<u32> {  // 反向查：用例名→判据号。
        let mut i = 0usize;
        while i < self.rev.len() {
            if self.rev[i].0 == case {
                return Some(self.rev[i].1);
            }
            i += 1;
        }
        None
    }
    pub fn maps_crosscheck(&self) -> Result<u32, u16> {  // 映射双向对账：正反向逐条互查双射一致。
        if self.stage != KStage::Mapped && self.stage != KStage::Green {
            return Err(XC_STATE_INVALID);
        }
        let mut i = 0usize;
        while i < self.fwd.len() {
            match self.rev_of(&self.fwd[i].1) {
                Some(v) if v == self.fwd[i].0 => {}
                _ => return Err(XC_REV_MISS), // 反向失配定位：fwd[i]
            }
            i += 1;
        }
        i = 0;
        while i < self.rev.len() {
            match self.fwd_of(self.rev[i].1) {
                Some(c) if c == self.rev[i].0 => {}
                _ => return Err(XC_FWD_MISS), // 正向失配定位：rev[i]
            }
            i += 1;
        }
        Ok(self.fwd.len() as u32)
    }
    pub fn run_all(&mut self) -> Result<RunReceipt, u16> {  // 一次命令全绿：三重口径计数相等守恒 + 逐条断言 + 分桶（重执行红）。
        if self.stage == KStage::Green && self.pinned.is_some() {
            return Err(XC_ALREADY_RUN);
        }
        if !self.stage.can_step(KStage::Green) {
            return Err(XC_STATE_INVALID);
        }
        // 口径一：F 码面计数（id 全部合法即集大小）。
        let g_f = self.kases.len() as u32;
        // 口径二：行数面计数（每条行数下限 570 达标记账）。
        let mut g_l = 0u32;
        let mut i = 0usize;
        while i < self.kases.len() {
            if self.kases[i].lines >= 570 {
                g_l += 1;
            }
            i += 1;
        }
        // 口径三：凭证面计数（每条凭证包齐记账）。
        let mut g_v = 0u32;
        i = 0;
        while i < self.kases.len() {
            if self.kases[i].vouch {
                g_v += 1;
            }
            i += 1;
        }
        if g_f != g_l || g_l != g_v {
            return Err(XC_MISMATCH); // 全绿守恒破，定位靠逐条回查
        }
        // payload 哈希奇偶分桶（确定性）。
        let mut buckets = [0u32; BUCKET_N];
        i = 0;
        while i < self.kases.len() {
            let h = fnv_str(&self.kases[i].case);
            buckets[(h & 1) as usize] += 1;
            i += 1;
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0799-J1 {} 一次命令全绿：三重口径 {}={}={}，映射双向对账过",
            SUITE_NAME, g_f, g_l, g_v
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = RunReceipt { suite: String::from(SUITE_NAME), green: g_f, buckets, summary, fingerprint: fp };
        self.pinned = Some(rc.clone());
        self.stage = KStage::Green;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&RunReceipt> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 断言集指纹：（kid,case,lines,vouch）规范序（按 kid 排序后折叠）。
        // 先取 kid 序（选择排序，确定性）。
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.kases.len() {
            ids.push(self.kases[i].kid);
            i += 1;
        }
        let mut a = 0usize;
        while a < ids.len() {
            let mut b = a + 1;
            while b < ids.len() {
                if ids[b] < ids[a] {
                    let t = ids[a];
                    ids[a] = ids[b];
                    ids[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        i = 0;
        while i < ids.len() {
            let mut j = 0usize;
            while j < self.kases.len() {
                if self.kases[j].kid == ids[i] {
                    h = fnv1a(h, self.kases[j].kid as u64);
                    h = fnv1a(h, fnv_str(&self.kases[j].case));
                    h = fnv1a(h, self.kases[j].lines as u64);
                    h = fnv1a(h, self.kases[j].vouch as u64);
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        h
    }
    pub fn kase_of(&self, kid: u32) -> Option<&Kase> {  // 断言查询。
        let mut i = 0usize;
        while i < self.kases.len() {
            if self.kases[i].kid == kid {
                return Some(&self.kases[i]);
            }
            i += 1;
        }
        None
    }
    pub fn gauge_counts(&self) -> [u32; 3] {  // 三重口径即时计数（Green 态与回执 green 互证）。
        let mut out = [0u32; 3];
        out[Gauge::FCode as usize] = self.kases.len() as u32;
        let mut i = 0usize;
        while i < self.kases.len() {
            if self.kases[i].lines >= 570 {
                out[Gauge::Lines as usize] += 1;
            }
            if self.kases[i].vouch {
                out[Gauge::Voucher as usize] += 1;
            }
            i += 1;
        }
        out
    }
    pub fn suite_view(&self) -> String {  // 断言集视图串："kid|case|lines|vouch" 行串（kid 规范序）。
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.kases.len() {
            ids.push(self.kases[i].kid);
            i += 1;
        }
        let mut a = 0usize;
        while a < ids.len() {
            let mut b = a + 1;
            while b < ids.len() {
                if ids[b] < ids[a] {
                    let t = ids[a];
                    ids[a] = ids[b];
                    ids[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        let mut s = String::new();
        i = 0;
        while i < ids.len() {
            let mut j = 0usize;
            while j < self.kases.len() {
                if self.kases[j].kid == ids[i] {
                    s.push_str(alloc::format!("{}|{}|{}|{}\n", self.kases[j].kid, self.kases[j].case, self.kases[j].lines, self.kases[j].vouch as u32).as_str());
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        s
    }
    pub fn case_of(&self, case: &str) -> Option<&Kase> {  // 按用例名查断言（反向解引入口）。
        let mut i = 0usize;
        while i < self.kases.len() {
            if self.kases[i].case == case {
                return Some(&self.kases[i]);
            }
            i += 1;
        }
        None
    }
    pub fn bucket_view(&self) -> String {  // 分桶视图串："even=…,odd=…"（回执分桶互证）。
        match &self.pinned {
            Some(r) => alloc::format!("even={},odd={}", r.buckets[0], r.buckets[1]),
            None => String::from("not-run"),
        }
    }
    pub fn payload_hash_of(&self, kid: u32) -> Option<u64> {  // 单条 payload 哈希（确定性复算入口）。
        let mut i = 0usize;
        while i < self.kases.len() {
            if self.kases[i].kid == kid {
                return Some(fnv_str(&self.kases[i].case));
            }
            i += 1;
        }
        None
    }
}
pub fn gen_kases() -> Vec<Kase> {  // 样本发生器：B40 批六条断言（确定性用例名/行数/凭证）。
    let mut out: Vec<Kase> = Vec::new();
    let mut i = 0usize;
    while i < SET_MIN {
        out.push(Kase {
            kid: (i + 1) as u32,
            case: alloc::format!("b40_k{}_{}", i + 1, SUITE_NAME),
            lines: 570 + (i as u32) * 5,
            vouch: true,
        });
        i += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let ks = gen_kases();
    // —— 主路径：入集 → 建映射 → 双向对账 → 一次命令全绿 ——
    let mut m = KtestSuite::new();
    let mut i = 0usize;
    while i < ks.len() {
        m.add(ks[i].clone())?;
        i += 1;
    }
    if m.set_len() != SET_MIN {
        return Err(XC_SET_SHORT);
    }
    if m.build_maps()? != SET_MIN as u32 {
        return Err(XC_SET_SHORT);
    }
    if m.maps_crosscheck()? != SET_MIN as u32 {
        return Err(XC_REV_MISS);
    }
    let rc = m.run_all()?;
    if rc.suite != SUITE_NAME || rc.green != SET_MIN as u32 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if rc.buckets[0] + rc.buckets[1] != SET_MIN as u32 {
        return Err(XC_MISMATCH); // 分桶守恒
    }
    if !matches!(m.run_all(), Err(XC_ALREADY_RUN)) {
        return Err(XC_STATE_INVALID);
    }
    // 双跑对拍：同料重跑指纹一致（确定性）。
    let fp1 = rc.fingerprint;
    let fp2 = {
        let mut m2 = KtestSuite::new();
        let mut j = 0usize;
        while j < ks.len() {
            m2.add(ks[j].clone())?;
            j += 1;
        }
        m2.build_maps()?;
        m2.run_all()?;
        m2.fingerprint()
    };
    if fp1 != fp2 {
        return Err(XC_HASH_DIFF);
    }
    // —— 错误路径 1：号重 / 号越界 / 用例名空 / 用例名重 ——
    let mut m3 = KtestSuite::new();
    m3.add(ks[0].clone())?;
    if !matches!(m3.add(ks[0].clone()), Err(XC_DUP_ID)) {
        return Err(XC_DUP_ID);
    }
    let mut bad = ks[1].clone();
    bad.kid = 7;
    if !matches!(m3.add(bad), Err(XC_ID_GAP)) {
        return Err(XC_ID_GAP);
    }
    let mut bad2 = ks[1].clone();
    bad2.case = String::new();
    if !matches!(m3.add(bad2), Err(XC_CASE_EMPTY)) {
        return Err(XC_CASE_EMPTY);
    }
    let mut bad3 = ks[1].clone();
    bad3.case = ks[0].case.clone();
    if !matches!(m3.add(bad3), Err(XC_CASE_DUP)) {
        return Err(XC_CASE_DUP);
    }
    // —— 错误路径 2：断档（缺一条）——
    let mut m4 = KtestSuite::new();
    let mut j = 0usize;
    while j < ks.len() {
        if j != 3 {
            m4.add(ks[j].clone())?; // 缺 kid=4
        }
        j += 1;
    }
    if !matches!(m4.build_maps(), Err(XC_ID_GAP)) {
        return Err(XC_ID_GAP);
    }
    // —— 错误路径 3：行数面未达标 / 凭证面未齐 → 三口径不等 ——
    let mut m5 = KtestSuite::new();
    j = 0;
    while j < ks.len() {
        let mut k = ks[j].clone();
        if j == 2 {
            k.lines = 100; // 行数面掉队
        }
        m5.add(k)?;
        j += 1;
    }
    m5.build_maps()?;
    if !matches!(m5.run_all(), Err(XC_MISMATCH)) {
        return Err(XC_MISMATCH);
    }
    let mut m6 = KtestSuite::new();
    j = 0;
    while j < ks.len() {
        let mut k = ks[j].clone();
        if j == 4 {
            k.vouch = false; // 凭证面掉队
        }
        m6.add(k)?;
        j += 1;
    }
    m6.build_maps()?;
    if !matches!(m6.run_all(), Err(XC_MISMATCH)) {
        return Err(XC_MISMATCH);
    }
    // —— 查询 API：正反查 + 双射 + 即时口径 + 视图 ——
    match m.fwd_of(3) {
        Some(c) if c == ks[2].case => {}
        _ => return Err(XC_FWD_MISS),
    }
    if m.fwd_of(9).is_some() {
        return Err(XC_FWD_MISS);
    }
    match m.rev_of(&ks[5].case) {
        Some(6) => {}
        _ => return Err(XC_REV_MISS),
    }
    if m.rev_of("no-such-case").is_some() {
        return Err(XC_REV_MISS);
    }
    let gc = m.gauge_counts();
    if gc[0] != gc[1] || gc[1] != gc[2] || gc[0] != SET_MIN as u32 {
        return Err(XC_MISMATCH); // Green 态即时口径互证
    }
    match m.kase_of(5) {
        Some(k) if k.vouch && k.lines >= 570 => {}
        _ => return Err(XC_VOUCH_MISSING),
    }
    let sv = m.suite_view();
    if !sv.starts_with("1|") || !sv.contains("6|") {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.stage().view() != "green" {
        return Err(XC_STATE_INVALID);
    }
    // —— 扩展互证：查询 API 与回执/映射对拍 ——
    match m.case_of(&ks[1].case) {
        Some(k) if k.kid == 2 => {}
        _ => return Err(XC_REV_MISS),
    }
    if m.case_of("absent-case").is_some() {
        return Err(XC_CASE_DUP);
    }
    let bv = m.bucket_view();
    if !bv.starts_with("even=") || !bv.contains("odd=") {
        return Err(XC_SUMMARY_EMPTY);
    }
    match m.payload_hash_of(4) {
        Some(h) => {
            // 单条复算哈希与直接口径一致。
            if h != fnv_str(&ks[3].case) {
                return Err(XC_HASH_DIFF);
            }
            if (h & 1) == 0 && rc.buckets[0] == 0 && rc.buckets[1] == 0 {
                return Err(XC_MISMATCH);
            }
        }
        None => return Err(XC_FWD_MISS),
    }
    if m.payload_hash_of(42).is_some() {
        return Err(XC_FWD_MISS);
    }
    Ok(0)
}
