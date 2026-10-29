#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0788 · 判据 3 终验（冷启动 ≤3 秒凭证落账）
//! 域/批：CoRun·UNX A1 收官域（入册/六判据终验面 · W7 清单第 15 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0788-J1 终验过（判据 3 凭证包四件：
//!   终核账/耗时账/画像报告/快照——齐 4/4）且 ≤3,000ms 定格；③ 完成时 API/
//!   软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① T0–T3 四锚点建模（固件交接/内核进入/子系统就绪/用户面就绪），
//!   锚点单调（t0 ≤ t1 ≤ t2 ≤ t3），总耗时 = t3 − t0（整数毫秒）。
//! ② 三机复算：三机总耗时取中位值（三值排序取中，禁浮点），中位值 ≤3,000ms
//!   才定格（判据原文写死）；任一机锚点不单调即红。
//! ③ 快照冻结：首定格后（锚点阵, 中位值）指纹冻结，重定格红；耗时账双路复算
//!   （正序/逆序 Σ）必须一致。
//! ④ 凭证包四件闭集：终核账/耗时账/画像报告/快照，齐 4/4 落账。
//! ⑤ 状态机 Intake→Recounted→Packed→Pinned；诊断码定位到（机, 锚点）。
//! 诊断段声明：0x60C0–0x60DF 本单独占（清单 <SEG 0x60c0>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x60C0–0x60DF）
pub const SEG_BASE: u16 = 0x60C0; // 诊断码段基数，来源：W7 清单 UNX-F0788 行 <SEG 0x60c0>
pub const SEG_TOP: u16 = 0x60DF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终验状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空锚点批
pub const XC_DUP_MACHINE: u16 = SEG_BASE + 3; // 机号重复
pub const XC_MACHINE_BAD: u16 = SEG_BASE + 4; // 机号越出 1..=3
pub const XC_ANCHOR_ORDER: u16 = SEG_BASE + 5; // 锚点不单调（t0≤t1≤t2≤t3 红）
pub const XC_RUN_SHORT: u16 = SEG_BASE + 6; // 机数不足 3
pub const XC_SLOW: u16 = SEG_BASE + 7; // 中位值 >3,000ms（判据红）
pub const XC_RECOUNT_DIFF: u16 = SEG_BASE + 8; // 耗时账双路复算不一致
pub const XC_PACK_SHORT: u16 = SEG_BASE + 9; // 凭证包不足 4/4
pub const XC_DUP_PACK: u16 = SEG_BASE + 10; // 凭证件型重复
pub const XC_HASH_ZERO: u16 = SEG_BASE + 11; // 凭证哈希为 0
pub const XC_ALREADY_PINNED: u16 = SEG_BASE + 12; // 已定格再定格
pub const XC_SNAP_DIFF: u16 = SEG_BASE + 13; // 快照与现账不一致
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 14; // 画像桶和 ≠ 机数
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 终验常量（写死，来源=判据原文/本单口径头注假设）
pub const MACHINE_COUNT: usize = 3; // 三机，来源：批册口径（与 F0764 同源）
pub const ANCHORS: usize = 4; // T0–T3 四锚点，来源：判据原文（F0764 口径）
pub const DEADLINE_MS: u32 = 3_000; // ≤3,000ms，来源：判据原文「≤3,000ms」
pub const PACK_ITEMS: usize = 4; // 凭证包四件，来源：判据原文

/// 一台机的四锚点记录。
#[derive(Clone, Copy, Debug)]
pub struct BootAnchors {
    pub machine: u8,  // 机号 1..=3
    pub t0: u32,      // 固件交接（ms）
    pub t1: u32,      // 内核进入
    pub t2: u32,      // 子系统就绪
    pub t3: u32,      // 用户面就绪
}
impl BootAnchors {
    pub fn monotonic(&self) -> bool {  // 锚点单调性：t0 ≤ t1 ≤ t2 ≤ t3。
        self.t0 <= self.t1 && self.t1 <= self.t2 && self.t2 <= self.t3
    }
    pub fn total_ms(&self) -> u32 {  // 总耗时 = t3 − t0（单调前提下的整数差）。
        self.t3 - self.t0
    }
    pub fn segments(&self) -> [u32; 3] {  // 分段耗时 [T0→T1, T1→T2, T2→T3]（整数差）。
        [self.t1 - self.t0, self.t2 - self.t1, self.t3 - self.t2]
    }
}

/// 凭证件。
#[derive(Clone, Debug)]
pub struct Piece {
    pub kind: u8,    // 0=终核账 1=耗时账 2=画像报告 3=快照
    pub hash: u64,   // 非 0
    pub note: String,
}

/// 画像报告。
#[derive(Clone, Debug)]
pub struct ProfileReport {
    pub totals: [u32; MACHINE_COUNT],   // 三机总耗时
    pub median: u32,                    // 中位值
    pub within: bool,                   // 中位值 ≤3,000
    pub seg_max: [u32; 3],              // 三段的最大耗时（跨机）
}

/// 定格回执。
#[derive(Clone, Debug)]
pub struct V3Pin {
    pub median_ms: u32,   // 中位值（定格）
    pub within: bool,     // ≤3,000 结论
    pub pack: u32,        // 凭件数（4）
    pub summary: String,  // 摘要（非空）
    pub fingerprint: u64, // 账面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V3Stage {
    Intake = 0,
    Recounted = 1,
    Packed = 2,
    Pinned = 3,
}
impl V3Stage {
    pub fn can_step(self, next: V3Stage) -> bool {
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
pub struct Verdict3 {  // 判据 3 终验机。
    stage: V3Stage,
    rows: Vec<BootAnchors>,
    seen: Vec<u8>,
    pack: Vec<Piece>,
    frozen_snapshot: Option<u64>, // 定格快照指纹
    pinned: Option<V3Pin>,
    events: Vec<String>,
}
impl Verdict3 {
    pub fn new() -> Self {
        Verdict3 {
            stage: V3Stage::Intake,
            rows: Vec::new(),
            seen: Vec::new(),
            pack: Vec::new(),
            frozen_snapshot: None,
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> V3Stage {
        self.stage
    }
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn pack_len(&self) -> usize {
        self.pack.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_anchors(&mut self, batch: &[BootAnchors]) -> Result<u32, u16> {  // 喂入锚点批：机闭集唯一、锚点单调守卫。
        if self.stage != V3Stage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for a in batch.iter() {
            if a.machine < 1 || a.machine as usize > MACHINE_COUNT {
                return Err(XC_MACHINE_BAD);
            }
            if !a.monotonic() {
                return Err(XC_ANCHOR_ORDER);
            }
            let mut i = 0usize;
            while i < self.seen.len() {
                if self.seen[i] == a.machine {
                    return Err(XC_DUP_MACHINE);
                }
                i += 1;
            }
            self.seen.push(a.machine);
            self.rows.push(*a);
            added += 1;
        }
        self.events.push(alloc::format!("anchors +{}", added));
        Ok(added)
    }
    pub fn recount(&mut self) -> Result<(u32, u32, u32), u16> {  // 耗时账复算：正序/逆序 Σ(t3−t0) 双路一致 + 三机齐。
        if !self.stage.can_step(V3Stage::Recounted) {
            return Err(if self.rows.len() != MACHINE_COUNT {
                XC_RUN_SHORT
            } else {
                XC_STATE_INVALID
            });
        }
        let mut fwd: u64 = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            fwd += self.rows[i].total_ms() as u64;
            i += 1;
        }
        let mut rev: u64 = 0;
        let mut j = self.rows.len();
        while j > 0 {
            j -= 1;
            rev += self.rows[j].total_ms() as u64;
        }
        if fwd != rev {
            return Err(XC_RECOUNT_DIFF);
        }
        let mut t0 = 0usize;
        let mut t1 = 1usize;
        let mut t2 = 2usize;
        // 三值排序（升序，取中）。确定性交换排序。
        if self.rows[t0].total_ms() > self.rows[t1].total_ms() {
            let tmp = t0;
            t0 = t1;
            t1 = tmp;
        }
        if self.rows[t1].total_ms() > self.rows[t2].total_ms() {
            let tmp = t1;
            t1 = t2;
            t2 = tmp;
        }
        if self.rows[t0].total_ms() > self.rows[t1].total_ms() {
            let tmp = t0;
            t0 = t1;
            t1 = tmp;
        }
        let median = self.rows[t1].total_ms();
        let min = self.rows[t0].total_ms();
        let max = self.rows[t2].total_ms();
        self.stage = V3Stage::Recounted;
        self.events
            .push(alloc::format!("recount median={} min={} max={}", median, min, max));
        Ok((min, median, max))
    }
    pub fn profile(&self) -> Result<ProfileReport, u16> {  // 画像报告：三机总耗时 + 中位值 + 三段跨机最大值（桶守恒）。
        if self.rows.len() != MACHINE_COUNT {
            return Err(XC_RUN_SHORT);
        }
        let mut totals = [0u32; MACHINE_COUNT];
        let mut seg_max = [0u32; 3];
        let mut i = 0usize;
        while i < self.rows.len() {
            totals[i] = self.rows[i].total_ms();
            let segs = self.rows[i].segments();
            let mut s = 0usize;
            while s < 3 {
                if segs[s] > seg_max[s] {
                    seg_max[s] = segs[s];
                }
                s += 1;
            }
            i += 1;
        }
        // 中位值（三值排序）。
        let mut v = [totals[0], totals[1], totals[2]];
        if v[0] > v[1] {
            let t = v[0];
            v[0] = v[1];
            v[1] = t;
        }
        if v[1] > v[2] {
            let t = v[1];
            v[1] = v[2];
            v[2] = t;
        }
        if v[0] > v[1] {
            let t = v[0];
            v[0] = v[1];
            v[1] = t;
        }
        let median = v[1];
        Ok(ProfileReport { totals, median, within: median <= DEADLINE_MS, seg_max })
    }
    pub fn register_pack(&mut self, pieces: &[Piece]) -> Result<u32, u16> {  // 凭证包登记：件型闭集、哈希非 0、型唯一。
        if !self.stage.can_step(V3Stage::Packed) {
            return Err(XC_STATE_INVALID);
        }
        if pieces.is_empty() {
            return Err(XC_PACK_SHORT);
        }
        let mut added: u32 = 0;
        for p in pieces.iter() {
            if p.kind as usize >= PACK_ITEMS {
                return Err(XC_DUP_PACK);
            }
            if p.hash == 0 {
                return Err(XC_HASH_ZERO);
            }
            let mut i = 0usize;
            while i < self.pack.len() {
                if self.pack[i].kind == p.kind {
                    return Err(XC_DUP_PACK);
                }
                i += 1;
            }
            self.pack.push(p.clone());
            added += 1;
        }
        if self.pack.len() > PACK_ITEMS {
            return Err(XC_DUP_PACK);
        }
        self.events.push(alloc::format!("pack +{}", added));
        Ok(added)
    }
    pub fn pack_assert(&mut self) -> Result<u32, u16> {  // 凭包齐断言：4/4 件型到位。
        if self.pack.len() != PACK_ITEMS {
            return Err(XC_PACK_SHORT);
        }
        let mut kinds = [false; PACK_ITEMS];
        let mut i = 0usize;
        while i < self.pack.len() {
            kinds[self.pack[i].kind as usize] = true;
            i += 1;
        }
        let mut k = 0usize;
        while k < PACK_ITEMS {
            if !kinds[k] {
                return Err(XC_PACK_SHORT);
            }
            k += 1;
        }
        self.stage = V3Stage::Packed;
        self.events.push(String::from("pack 4/4"));
        Ok(PACK_ITEMS as u32)
    }
    pub fn pin(&mut self) -> Result<V3Pin, u16> {  // 定格：中位值 ≤3,000 + 凭包 4/4 → 快照指纹冻结 + 摘要定格。
        if self.pinned.is_some() {
            return Err(XC_ALREADY_PINNED);
        }
        if self.stage != V3Stage::Packed {
            return Err(XC_STATE_INVALID);
        }
        let prof = self.profile()?;
        if !prof.within {
            return Err(XC_SLOW);
        }
        let fp = self.fingerprint();
        if let Some(fz) = self.frozen_snapshot {
            if fz != fp {
                return Err(XC_SNAP_DIFF);
            }
        }
        self.frozen_snapshot = Some(fp);
        let summary = alloc::format!(
            "UNX-F0788-J1 终验过：冷启动三机中位值 {}ms ≤3,000ms 定格，凭证包 4/4（终核账/耗时账/画像报告/快照）落账",
            prof.median
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = V3Pin {
            median_ms: prof.median,
            within: true,
            pack: self.pack.len() as u32,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = V3Stage::Pinned;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&V3Pin> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = fnv1a(h, r.machine as u64);
            h = fnv1a(h, r.t0 as u64);
            h = fnv1a(h, r.t1 as u64);
            h = fnv1a(h, r.t2 as u64);
            h = fnv1a(h, r.t3 as u64);
            i += 1;
        }
        h
    }
    pub fn row_of(&self, machine: u8) -> Option<BootAnchors> {  // 机号查询。
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].machine == machine {
                return Some(self.rows[i]);
            }
            i += 1;
        }
        None
    }
    pub fn deadline_margin(&self) -> Result<u32, u16> {  // 截止余量：DEADLINE_MS − 中位值（超时钳位为 0）。
        let prof = self.profile()?;
        if prof.median > DEADLINE_MS {
            return Ok(0);
        }
        Ok(DEADLINE_MS - prof.median)
    }

    pub fn slowest_machine(&self) -> Result<u8, u16> {  // 最慢机定位：总耗时最大的机号（并列取小号，确定性）。
        if self.rows.len() != MACHINE_COUNT {
            return Err(XC_RUN_SHORT);
        }
        let mut best = self.rows[0];
        let mut i = 1usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.total_ms() > best.total_ms() {
                best = r;
            }
            i += 1;
        }
        Ok(best.machine)
    }

    pub fn segment_matrix(&self) -> Result<[[u32; 3]; MACHINE_COUNT], u16> {  // 分段画像（逐机 × 逐段）：3×3 矩阵，段和 == 总耗时（守恒）。
        if self.rows.len() != MACHINE_COUNT {
            return Err(XC_RUN_SHORT);
        }
        let mut out = [[0u32; 3]; MACHINE_COUNT];
        let mut i = 0usize;
        while i < self.rows.len() {
            let segs = self.rows[i].segments();
            out[i] = segs;
            if segs[0] + segs[1] + segs[2] != self.rows[i].total_ms() {
                return Err(XC_RECOUNT_DIFF);
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn piece_name(kind: u8) -> Option<&'static str> {  // 件型名表（闭集 0..=3 中文标签）。
        match kind {
            0 => Some("终核账"),
            1 => Some("耗时账"),
            2 => Some("画像报告"),
            3 => Some("快照"),
            _ => None,
        }
    }
}
pub fn gen_anchors(seed: u64, spans: [u32; MACHINE_COUNT]) -> Vec<BootAnchors> {  // 样本发生器：三机锚点（确定性；base 为 t0 基线，span 为总时长增量）。
    let mut s = seed;
    let mut out: Vec<BootAnchors> = Vec::new();
    let mut m = 1u8;
    while m as usize <= MACHINE_COUNT {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let base = (s >> 33) as u32 % 100;
        let span = spans[(m - 1) as usize];
        // 四锚点等分递增（单调保证）。
        let t0 = base;
        let t1 = base + span / 4;
        let t2 = base + span / 2;
        let t3 = base + span;
        out.push(BootAnchors { machine: m, t0, t1, t2, t3 });
        m += 1;
    }
    out
}
pub fn gen_pack(seed: u64) -> Vec<Piece> {  // 凭证包四件。
    let names = ["终核账", "耗时账", "画像报告", "快照"];
    let mut out: Vec<Piece> = Vec::new();
    let mut k = 0u8;
    while k < 4 {
        out.push(Piece {
            kind: k,
            hash: fnv1a(0x60C0_0000, seed ^ (k as u64)) | 1,
            note: alloc::format!("判据3凭证·{}", names[k as usize]),
        });
        k += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    // —— 主路径：三机 [2800, 2900, 2950] → 中位 2900 ≤3000 ——
    let runs = gen_anchors(0x60C0_0000_0000_0001, [2_800, 2_900, 2_950]);
    let mut m = Verdict3::new();
    m.feed_anchors(&runs)?;
    if m.row_count() != 3 {
        return Err(XC_RUN_SHORT);
    }
    let (min, med, max) = m.recount()?;
    if !(min <= med && med <= max) || med != 2_900 {
        return Err(XC_RECOUNT_DIFF);
    }
    let prof = m.profile()?;
    if !prof.within || prof.median != 2_900 {
        return Err(XC_SLOW);
    }
    if prof.totals[0] != 2_800 || prof.totals[1] != 2_900 || prof.totals[2] != 2_950 {
        return Err(XC_COUNT_MISMATCH);
    }
    m.register_pack(&gen_pack(0x71))?;
    if m.pack_assert()? != 4 {
        return Err(XC_PACK_SHORT);
    }
    let rc = m.pin()?;
    if rc.median_ms != 2_900 || !rc.within || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.pin(), Err(XC_ALREADY_PINNED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：超时（中位 3,200 >3,000）——
    let mut m2 = Verdict3::new();
    m2.feed_anchors(&gen_anchors(0x60C0_0000_0000_0002, [3_100, 3_200, 3_300]))?;
    m2.recount()?;
    m2.register_pack(&gen_pack(0x72))?;
    m2.pack_assert()?;
    if !matches!(m2.pin(), Err(XC_SLOW)) {
        return Err(XC_SLOW);
    }
    // —— 错误路径 2：锚点不单调 ——
    let mut m3 = Verdict3::new();
    let mut bad = gen_anchors(0x60C0_0000_0000_0003, [2_500, 2_600, 2_700]);
    bad[1].t2 = bad[1].t1 - 1; // t2 < t1
    if !matches!(m3.feed_anchors(&bad), Err(XC_ANCHOR_ORDER)) {
        return Err(XC_ANCHOR_ORDER);
    }
    // —— 错误路径 3：机号重复 / 越界 / 机数不足 ——
    let mut m4 = Verdict3::new();
    let mut dup = gen_anchors(0x60C0_0000_0000_0004, [2_500, 2_600, 2_700]);
    dup[2].machine = dup[0].machine;
    if !matches!(m4.feed_anchors(&dup), Err(XC_DUP_MACHINE)) {
        return Err(XC_DUP_MACHINE);
    }
    let mut m5 = Verdict3::new();
    let mut badm = gen_anchors(0x60C0_0000_0000_0005, [2_500, 2_600, 2_700]);
    badm[0].machine = 9;
    if !matches!(m5.feed_anchors(&badm), Err(XC_MACHINE_BAD)) {
        return Err(XC_MACHINE_BAD);
    }
    let mut m6 = Verdict3::new();
    m6.feed_anchors(&runs[..2].to_vec())?;
    if !matches!(m6.recount(), Err(XC_RUN_SHORT)) {
        return Err(XC_RUN_SHORT);
    }
    // —— 错误路径 4：凭包缺件 / 件型重复 / 哈希零 ——
    let mut m7 = Verdict3::new();
    m7.feed_anchors(&gen_anchors(0x60C0_0000_0000_0006, [2_500, 2_600, 2_700]))?;
    m7.recount()?;
    m7.register_pack(&gen_pack(0x73)[..3].to_vec())?;
    if !matches!(m7.pack_assert(), Err(XC_PACK_SHORT)) {
        return Err(XC_PACK_SHORT);
    }
    let mut m8 = Verdict3::new();
    m8.feed_anchors(&gen_anchors(0x60C0_0000_0000_0007, [2_500, 2_600, 2_700]))?;
    m8.recount()?;
    let mut dupk = gen_pack(0x74);
    dupk[3].kind = 1;
    if !matches!(m8.register_pack(&dupk), Err(XC_DUP_PACK)) {
        return Err(XC_DUP_PACK);
    }
    // —— 查询 API：机号可解引 / 件型名表闭集 / 分段单调 ——
    match m.row_of(2) {
        Some(r) if r.total_ms() == 2_900 => {}
        _ => return Err(XC_RECOUNT_DIFF),
    }
    if m.row_of(9).is_some() {
        return Err(XC_MACHINE_BAD);
    }
    let mut pk = 0u8;
    while pk < 4 {
        if Verdict3::piece_name(pk).is_none() {
            return Err(XC_DUP_PACK);
        }
        pk += 1;
    }
    if Verdict3::piece_name(4).is_some() {
        return Err(XC_DUP_PACK);
    }
    let segs = m.row_of(1).unwrap_or(BootAnchors { machine: 0, t0: 0, t1: 0, t2: 0, t3: 0 }).segments();
    if segs[0] + segs[1] + segs[2] != m.row_of(1).map(|r| r.total_ms()).unwrap_or(0) {
        return Err(XC_RECOUNT_DIFF);
    }
    // —— 扩展 API 面：余量 / 最慢机 / 分段矩阵 ——
    if m.deadline_margin()? != 100 {
        return Err(XC_COUNT_MISMATCH);
    }
    if m2.deadline_margin()? != 0 {
        return Err(XC_SLOW);
    }
    if m.slowest_machine()? != 3 {
        return Err(XC_COUNT_MISMATCH);
    }
    let sm = m.segment_matrix()?;
    let mut si = 0usize;
    while si < MACHINE_COUNT {
        let s3 = sm[si];
        if s3[0] + s3[1] + s3[2] != prof.totals[si] {
            return Err(XC_RECOUNT_DIFF);
        }
        si += 1;
    }
    if !matches!(m6.segment_matrix(), Err(XC_RUN_SHORT)) {
        return Err(XC_RUN_SHORT);
    }
    Ok(0)
}
