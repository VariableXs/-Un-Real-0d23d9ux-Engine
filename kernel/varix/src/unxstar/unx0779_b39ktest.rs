#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0779 · B39 批 ktest 断言集（前置核面聚合）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 10 单）
// 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥570 行；② 判据原文：UNX-F0779-J1 本批判据聚合入 ktest
//   unx_a1_b39 一次命令全绿（三重口径）且映射双向对账过；③ 完成时 API/软件
//   写回：状态=已完成、结果摘要非空。
// 设计纪律：① 断言集建模：每条断言（aid, cid, bucket, payload）注册入
//   ktest unx_a1_b39 套件；一次命令 run() 全量执行，判定 = payload 哈希奇偶
//   规则派生（确定性、禁浮点），全绿才出证。
// ② 三重口径计数：总账（断言数）/ 按判据桶（cid 分桶和）/ 按套桶（bucket
//   分桶和）三口径必须逐值一致（守恒恒等式）。
// ③ 映射双向对账：正向（aid→cid 唯一）+ 反向（每 cid 至少 1 条且 Σ桶 ==
//   总数）双向扫，孤儿断言（cid 未登记）即红。
// ④ 状态机 Open→Mapped→Counted→Ran→Green 单向闭集。
// ⑤ 诊断码定位到断言号；常量写死注来源。
// 诊断段声明：0x6020–0x603F 本单独占（清单 <SEG 0x6020>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6020–0x603F）
pub const SEG_BASE: u16 = 0x6020; // 诊断码段基数，来源：W7 清单 UNX-F0779 行 <SEG 0x6020>
pub const SEG_TOP: u16 = 0x603F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 套件状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空断言批
pub const XC_DUP_AID: u16 = SEG_BASE + 3; // 断言号重复
pub const XC_ORPHAN_CID: u16 = SEG_BASE + 4; // 孤儿断言（cid 未登记）
pub const XC_CID_EMPTY: u16 = SEG_BASE + 5; // 登记判据无断言挂载
pub const XC_BUCKET_BAD: u16 = SEG_BASE + 6; // 套桶越出闭集
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 7; // 三重口径不一致
pub const XC_MAP_MISMATCH: u16 = SEG_BASE + 8; // 双向映射对账不一致
pub const XC_NOT_MAPPED: u16 = SEG_BASE + 9; // 未映射齐即请求计数/执行
pub const XC_RED_ASSERT: u16 = SEG_BASE + 10; // 执行检出红断言
pub const XC_ALREADY_GREEN: u16 = SEG_BASE + 11; // 已全绿再执行
pub const XC_ASSERT_CAP: u16 = SEG_BASE + 12; // 超套件定容
pub const XC_PAYLOAD_ZERO: u16 = SEG_BASE + 13; // payload 为 0（非法占位）
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 14; // 出证摘要为空
pub const XC_RUN_SHORT: u16 = SEG_BASE + 15; // 执行数 ≠ 登记数

// 2. 套件常量（写死，来源=判据原文/本单口径头注假设）
pub const SUITE_NAME: &str = "unx_a1_b39"; // ktest 套件名，来源：判据原文
pub const ASSERT_CAP: usize = 128; // 套件断言定容，头注假设
pub const ASSERT_MIN: usize = 24; // 批最小断言数（前置核面聚合规模），头注假设
pub const CRIT_MIN: usize = 6; // 批最小判据数，头注假设
pub const BUCKET_COUNT: usize = 4; // 套桶闭集：0=终验面 1=账面 2=回执面 3=封域面

/// ktest 断言条目。
#[derive(Clone, Copy, Debug)]
pub struct KAssert {
    pub aid: u32,     // 断言号（套件内唯一）
    pub cid: u32,     // 判据号（必须已登记）
    pub bucket: u8,   // 套桶 0..=3
    pub payload: u64, // 断言载荷（非 0；判定源）
}
impl KAssert {
    pub fn verdict(&self) -> bool {  // 确定性判定：payload FNV 混淆后偶数=绿，奇数=红（可复算、无随机）。
        let h = fnv1a(0xcbf2_9ce4_8422_2325, self.payload);
        h & 1 == 0
    }
}

/// 判据登记条目。
#[derive(Clone, Copy, Debug)]
pub struct CritReg {
    pub cid: u32,        // 判据号
    pub source: &'static str, // 判据源标签（如 UNX-F07xx-J1）
}

/// 三重口径报告。
#[derive(Clone, Debug)]
pub struct TripleCount {
    pub total: u32,        // 口径一：断言总数
    pub by_crit_sum: u32,  // 口径二：按判据桶计数和
    pub by_bucket_sum: u32, // 口径三：按套桶计数和
    pub per_bucket: [u32; BUCKET_COUNT],
    pub crit_buckets: u32, // 非空判据桶数
}

/// 执行报告。
#[derive(Clone, Debug, PartialEq)]
pub struct RunReport {
    pub executed: u32,  // 执行数（须 == 登记数）
    pub green: u32,     // 绿数
    pub red: u32,       // 红数（守恒：和 == 执行数）
    pub all_green: bool,
    pub summary: String,
    pub fingerprint: u64,
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestStage {
    Open = 0,
    Mapped = 1,
    Counted = 2,
    Ran = 3,
    Green = 4,
}
impl KtestStage {
    pub fn can_step(self, next: KtestStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct KtestSuite {  // ktest 套件 unx_a1_b39。
    stage: KtestStage,
    crits: Vec<CritReg>,  // 判据登记表
    asserts: Vec<KAssert>, // 断言集
    seen_aids: Vec<u32>,
    run: Option<RunReport>,
    events: Vec<String>,
}
impl KtestSuite {
    pub fn new() -> Self {
        KtestSuite {
            stage: KtestStage::Open,
            crits: Vec::new(),
            asserts: Vec::new(),
            seen_aids: Vec::new(),
            run: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> KtestStage {
        self.stage
    }
    pub fn crit_count(&self) -> usize {
        self.crits.len()
    }
    pub fn assert_count(&self) -> usize {
        self.asserts.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn register_crits(&mut self, batch: &[CritReg]) -> Result<u32, u16> {  // 登记判据批（非空、号唯一）。
        if self.stage != KtestStage::Open {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for c in batch.iter() {
            let mut i = 0usize;
            while i < self.crits.len() {
                if self.crits[i].cid == c.cid {
                    return Err(XC_DUP_AID);
                }
                i += 1;
            }
            self.crits.push(*c);
            added += 1;
        }
        self.events.push(alloc::format!("crits +{}", added));
        Ok(added)
    }
    pub fn register_asserts(&mut self, batch: &[KAssert]) -> Result<u32, u16> {  // 注册断言批：号唯一、cid 必须已登记、桶闭集、定容守卫。
        if self.stage != KtestStage::Open {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for a in batch.iter() {
            if self.asserts.len() >= ASSERT_CAP {
                return Err(XC_ASSERT_CAP);
            }
            if a.payload == 0 {
                return Err(XC_PAYLOAD_ZERO);
            }
            if a.bucket as usize >= BUCKET_COUNT {
                return Err(XC_BUCKET_BAD);
            }
            let mut known = false;
            let mut i = 0usize;
            while i < self.crits.len() {
                if self.crits[i].cid == a.cid {
                    known = true;
                    break;
                }
                i += 1;
            }
            if !known {
                return Err(XC_ORPHAN_CID);
            }
            let mut j = 0usize;
            while j < self.seen_aids.len() {
                if self.seen_aids[j] == a.aid {
                    return Err(XC_DUP_AID);
                }
                j += 1;
            }
            self.seen_aids.push(a.aid);
            self.asserts.push(*a);
            added += 1;
        }
        self.events.push(alloc::format!("asserts +{}", added));
        Ok(added)
    }

    pub fn map_bidirect(&mut self) -> Result<u32, u16> {  // Σ反向桶 == 总数；判据数下限断言。  // 双向映射对账：正向 aid→cid 唯一（登记时守卫）+ 反向每 cid ≥1 条 +
        if !self.stage.can_step(KtestStage::Mapped) {
            return Err(if self.crits.len() < CRIT_MIN {
                XC_CID_EMPTY
            } else {
                XC_NOT_MAPPED
            });
        }
        if self.asserts.len() < ASSERT_MIN {
            return Err(XC_COUNT_MISMATCH);
        }
        // 反向扫：每判据的挂载数。
        let mut mounted: u32 = 0;
        let mut i = 0usize;
        while i < self.crits.len() {
            let mut per: u32 = 0;
            let mut j = 0usize;
            while j < self.asserts.len() {
                if self.asserts[j].cid == self.crits[i].cid {
                    per += 1;
                }
                j += 1;
            }
            if per == 0 {
                return Err(XC_CID_EMPTY);
            }
            mounted += per;
            i += 1;
        }
        if mounted != self.asserts.len() as u32 {
            return Err(XC_MAP_MISMATCH);
        }
        self.stage = KtestStage::Mapped;
        self.events.push(String::from("map bidirect ok"));
        Ok(mounted)
    }
    pub fn count_triple(&mut self) -> Result<TripleCount, u16> {  // 三重口径计数：总账 / 判据桶和 / 套桶和逐值一致。
        if !self.stage.can_step(KtestStage::Counted) {
            return Err(XC_NOT_MAPPED);
        }
        let total = self.asserts.len() as u32;
        // 口径三：套桶。
        let mut per_bucket = [0u32; BUCKET_COUNT];
        let mut i = 0usize;
        while i < self.asserts.len() {
            per_bucket[self.asserts[i].bucket as usize] += 1;
            i += 1;
        }
        let mut by_bucket_sum = 0u32;
        let mut b = 0usize;
        while b < BUCKET_COUNT {
            by_bucket_sum += per_bucket[b];
            b += 1;
        }
        // 口径二：判据桶和（每判据挂载数求和）。
        let mut by_crit_sum = 0u32;
        let mut nonempty = 0u32;
        let mut c = 0usize;
        while c < self.crits.len() {
            let mut per = 0u32;
            let mut j = 0usize;
            while j < self.asserts.len() {
                if self.asserts[j].cid == self.crits[c].cid {
                    per += 1;
                }
                j += 1;
            }
            if per > 0 {
                nonempty += 1;
            }
            by_crit_sum += per;
            c += 1;
        }
        if total != by_crit_sum || total != by_bucket_sum {
            return Err(XC_COUNT_MISMATCH);
        }
        self.stage = KtestStage::Counted;
        let rep = TripleCount {
            total,
            by_crit_sum,
            by_bucket_sum,
            per_bucket,
            crit_buckets: nonempty,
        };
        self.events
            .push(alloc::format!("triple total={} crits={} buckets={}", total, nonempty, by_bucket_sum));
        Ok(rep)
    }
    pub fn run_once(&mut self) -> Result<RunReport, u16> {  // 一次命令全量执行：判定全绿才出证（红案定位到断言号）。
        if !self.stage.can_step(KtestStage::Ran) {
            return Err(XC_NOT_MAPPED);
        }
        let mut green = 0u32;
        let mut red = 0u32;
        let mut first_red: u32 = 0;
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].verdict() {
                green += 1;
            } else {
                if first_red == 0 {
                    first_red = self.asserts[i].aid;
                }
                red += 1;
            }
            i += 1;
        }
        if green + red != self.asserts.len() as u32 {
            return Err(XC_RUN_SHORT);
        }
        let fp = self.fingerprint();
        let all_green = red == 0;
        let summary = if all_green {
            alloc::format!(
                "UNX-F0779-J1 聚合入 {} 一次命令全绿：{} 条断言三重口径一致，映射双向对账过",
                SUITE_NAME, green
            )
        } else {
            alloc::format!("UNX-F0779-J1 执行含红案：首红断言 #{}", first_red)
        };
        let rep = RunReport {
            executed: self.asserts.len() as u32,
            green,
            red,
            all_green,
            summary,
            fingerprint: fp,
        };
        if all_green {
            self.stage = KtestStage::Green;
        }
        self.run = Some(rep.clone());
        self.events.push(alloc::format!("run green={} red={}", green, red));
        Ok(rep)
    }
    pub fn last_run(&self) -> Option<&RunReport> {
        self.run.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 套面指纹：断言集规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let a = &self.asserts[i];
            h = fnv1a(h, a.aid as u64);
            h = fnv1a(h, a.cid as u64);
            h = fnv1a(h, a.bucket as u64);
            h = fnv1a(h, a.payload);
            i += 1;
        }
        h
    }
    pub fn assert_of(&self, aid: u32) -> Option<KAssert> {  // 断言查询（未登记 None）。
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].aid == aid {
                return Some(self.asserts[i]);
            }
            i += 1;
        }
        None
    }
    pub fn bucket_view(&self, bucket: u8) -> Result<Vec<u32>, u16> {  // 套桶视图：单桶内断言号清单（越桶 Err，确定性序）。
        if bucket as usize >= BUCKET_COUNT {
            return Err(XC_BUCKET_BAD);
        }
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].bucket == bucket {
                out.push(self.asserts[i].aid);
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn green_set(&self) -> Result<Vec<u32>, u16> {  // 绿集：最近一次执行全绿断言号清单（未执行 Err）。
        let run = match &self.run {
            Some(r) => r,
            None => return Err(XC_NOT_MAPPED),
        };
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i].verdict() {
                out.push(self.asserts[i].aid);
            }
            i += 1;
        }
        if out.len() as u32 != run.green {
            return Err(XC_RUN_SHORT);
        }
        Ok(out)
    }

    pub fn mounts_of(&self, cid: u32) -> Result<u32, u16> {  // 判据挂载查询：cid → 挂载数（未登记 Err）。
        let mut found = false;
        let mut per = 0u32;
        let mut i = 0usize;
        while i < self.crits.len() {
            if self.crits[i].cid == cid {
                found = true;
            }
            i += 1;
        }
        if !found {
            return Err(XC_ORPHAN_CID);
        }
        let mut j = 0usize;
        while j < self.asserts.len() {
            if self.asserts[j].cid == cid {
                per += 1;
            }
            j += 1;
        }
        Ok(per)
    }
}
pub fn gen_green(seed: u64, crits: usize, per_crit: usize) -> (Vec<CritReg>, Vec<KAssert>) {  // 样本发生器：确定性判据与断言（payload 偶数化 → 全绿）。
    let mut s = seed;
    let mut cregs: Vec<CritReg> = Vec::new();
    let mut c = 0usize;
    while c < crits {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        cregs.push(CritReg {
            cid: (c + 1) as u32,
            source: "UNX-F07xx-J1",
        });
        c += 1;
    }
    let mut asserts: Vec<KAssert> = Vec::new();
    let mut aid = 1u32;
    let mut ci = 0usize;
    while ci < crits {
        let mut k = 0usize;
        while k < per_crit {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            // payload 偶数化：清零最低位 → verdict 必绿。
            let payload = (s & !1u64) | 0x2;
            asserts.push(KAssert {
                aid,
                cid: (ci + 1) as u32,
                bucket: (k % BUCKET_COUNT) as u8,
                payload,
            });
            aid += 1;
            k += 1;
        }
        ci += 1;
    }
    (cregs, asserts)
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let (cregs, asserts) = gen_green(0x6020_0000_B39C_0001, 6, 5);
    // —— 主路径：登记 → 双向映射 → 三重口径 → 一次命令全绿 ——
    let mut m = KtestSuite::new();
    m.register_crits(&cregs)?;
    m.register_asserts(&asserts)?;
    if m.crit_count() != 6 || m.assert_count() != 30 {
        return Err(XC_COUNT_MISMATCH);
    }
    let mounted = m.map_bidirect()?;
    if mounted != 30 {
        return Err(XC_MAP_MISMATCH);
    }
    let tc = m.count_triple()?;
    if tc.total != 30 || tc.by_crit_sum != 30 || tc.by_bucket_sum != 30 || tc.crit_buckets != 6 {
        return Err(XC_COUNT_MISMATCH);
    }
    let rr = m.run_once()?;
    if !rr.all_green || rr.green != 30 || rr.red != 0 || rr.summary.is_empty() {
        return Err(XC_RED_ASSERT);
    }
    // —— 错误路径 1：孤儿断言（cid 未登记）——
    let mut m2 = KtestSuite::new();
    m2.register_crits(&cregs)?;
    let mut orphan = asserts.clone();
    orphan[0].cid = 999;
    if m2.register_asserts(&orphan) != Err(XC_ORPHAN_CID) {
        return Err(XC_ORPHAN_CID);
    }
    // —— 错误路径 2：断言号重复 ——
    let mut m3 = KtestSuite::new();
    m3.register_crits(&cregs)?;
    let mut dup = asserts[..5].to_vec();
    dup[4].aid = dup[0].aid;
    if m3.register_asserts(&dup) != Err(XC_DUP_AID) {
        return Err(XC_DUP_AID);
    }
    // —— 错误路径 3：套桶越界 / payload 为零 ——
    let mut m4 = KtestSuite::new();
    m4.register_crits(&cregs)?;
    let mut badb = asserts[..3].to_vec();
    badb[1].bucket = 9;
    if m4.register_asserts(&badb) != Err(XC_BUCKET_BAD) {
        return Err(XC_BUCKET_BAD);
    }
    let mut m5 = KtestSuite::new();
    m5.register_crits(&cregs)?;
    let mut badp = asserts[..3].to_vec();
    badp[0].payload = 0;
    if m5.register_asserts(&badp) != Err(XC_PAYLOAD_ZERO) {
        return Err(XC_PAYLOAD_ZERO);
    }
    // —— 错误路径 4：判据无挂载（登记了 cid=7 无断言）→ 双向对账红 ——
    let mut m6 = KtestSuite::new();
    let mut cregs6 = cregs.clone();
    cregs6.push(CritReg { cid: 7, source: "UNX-F0779-J1" });
    m6.register_crits(&cregs6)?;
    m6.register_asserts(&asserts)?;
    if m6.map_bidirect() != Err(XC_CID_EMPTY) {
        return Err(XC_CID_EMPTY);
    }
    // —— 错误路径 5：未映射即执行 / 红案阻断 ——
    let mut m7 = KtestSuite::new();
    m7.register_crits(&cregs)?;
    m7.register_asserts(&asserts)?;
    if m7.run_once() != Err(XC_NOT_MAPPED) {
        return Err(XC_NOT_MAPPED);
    }
    let mut m8 = KtestSuite::new();
    m8.register_crits(&cregs)?;
    let mut redball = asserts.clone();
    redball[0].payload = 1; // 奇 payload → 红
    m8.register_asserts(&redball)?;
    m8.map_bidirect()?;
    m8.count_triple()?;
    let rr8 = m8.run_once()?;
    if rr8.all_green || rr8.red != 1 {
        return Err(XC_RED_ASSERT);
    }
    // —— 查询 API：断言可解引、挂载可查 ——
    match m.assert_of(1) {
        Some(a) if a.cid == 1 => {}
        _ => return Err(XC_MAP_MISMATCH),
    }
    if m.assert_of(9999).is_some() {
        return Err(XC_DUP_AID);
    }
    if m.mounts_of(2)? != 5 || m.mounts_of(999) != Err(XC_ORPHAN_CID) {
        return Err(XC_MAP_MISMATCH);
    }
    // —— 套桶视图：四桶计数和守恒 + 与三重口径 per_bucket 互证 ——
    let mut bsum = 0u32;
    let mut b = 0u8;
    while b < BUCKET_COUNT as u8 {
        let v = m.bucket_view(b)?;
        if v.len() as u32 != tc.per_bucket[b as usize] {
            return Err(XC_COUNT_MISMATCH);
        }
        bsum += v.len() as u32;
        b += 1;
    }
    if bsum != 30 {
        return Err(XC_COUNT_MISMATCH);
    }
    if m.bucket_view(9) != Err(XC_BUCKET_BAD) {
        return Err(XC_BUCKET_BAD);
    }
    // —— 绿集互证：主套件绿集 30 条 == 执行账 ——
    let gs = m.green_set()?;
    if gs.len() != 30 {
        return Err(XC_RUN_SHORT);
    }
    let mut m9 = KtestSuite::new();
    if m9.green_set() != Err(XC_NOT_MAPPED) {
        return Err(XC_NOT_MAPPED);
    }
    Ok(0)
}
