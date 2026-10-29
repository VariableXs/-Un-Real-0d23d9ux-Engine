#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0789 · 判据 4 终验（8 核 50 次 AP 到齐凭证落账）
//! 域/批：CoRun·UNX A1 收官域（入册/六判据终验面 · W7 清单第 16 单）
//! 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥570 行；② 判据原文：UNX-F0789-J1 终验过（判据 4 凭证包四件：
//!   终核账/专项账/拓扑声明/LAPIC 账——齐 4/4）且 8/8×50 定格；③ 完成时 API/
//!   软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 到齐矩阵建模：50 轮 × 8 核位图（u64 位图，bit c = 核 c 到齐），
//!   位图满值 0xFF 才算该轮 8/8；50 轮全满才定格（判据原文写死）。
//! ② LAPIC 账：每轮每核一条 IPI 收讫记录（轮, 核, 向量号闭集 0x20..=0xFE），
//!   账条数 == 8×50（守恒）；（轮, 核）唯一；与位图逐位互证。
//! ③ 拓扑声明：核数声明 == 8 且核号闭集 0..=7 与位图编址一致（闭集断裂红）。
//! ④ 凭证包四件闭集：终核账/专项账/拓扑声明/LAPIC 账，齐 4/4 落账；定格单调。
//! ⑤ 状态机 Intake→Mapped→Verified→Packed→Pinned；诊断码定位到（轮, 核）。
//! 诊断段声明：0x60E0–0x60FF 本单独占（清单 <SEG 0x60e0>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x60E0–0x60FF）
pub const SEG_BASE: u16 = 0x60E0; // 诊断码段基数，来源：W7 清单 UNX-F0789 行 <SEG 0x60e0>
pub const SEG_TOP: u16 = 0x60FF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终验状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空位图批
pub const XC_DUP_ROUND: u16 = SEG_BASE + 3; // 轮号重复
pub const XC_ROUND_BAD: u16 = SEG_BASE + 4; // 轮号越出 1..=50
pub const XC_ROUND_SHORT: u16 = SEG_BASE + 5; // 轮数不足 50
pub const XC_NOT_FULL: u16 = SEG_BASE + 6; // 有轮未满 8/8（位图非 0xFF）
pub const XC_LAPIC_SHORT: u16 = SEG_BASE + 7; // LAPIC 账条数 ≠ 8×50
pub const XC_LAPIC_DUP: u16 = SEG_BASE + 8; // （轮, 核）重复
pub const XC_LAPIC_ORPHAN: u16 = SEG_BASE + 9; // LAPIC 记录对应位图未到齐
pub const XC_VECTOR_BAD: u16 = SEG_BASE + 10; // 向量号越出 0x20..=0xFE
pub const XC_TOPO_DIFF: u16 = SEG_BASE + 11; // 拓扑声明核数 ≠ 8
pub const XC_PACK_SHORT: u16 = SEG_BASE + 12; // 凭证包不足 4/4
pub const XC_DUP_PACK: u16 = SEG_BASE + 13; // 凭证件型重复
pub const XC_ALREADY_PINNED: u16 = SEG_BASE + 14; // 已定格再定格
pub const XC_HASH_ZERO: u16 = SEG_BASE + 15; // 凭证哈希为 0

// 2. 终验常量（写死，来源=判据原文/本单口径头注假设）
pub const CORE_COUNT: u8 = 8; // 8 核，来源：判据原文「8 核」
pub const CORE_MASK: u64 = 0xFF; // 8 核满值位图（bit0..=bit7）
pub const ROUNDS: usize = 50; // 50 次（轮），来源：判据原文「50 次」
pub const LAPIC_TOTAL: usize = CORE_COUNT as usize * ROUNDS; // 400 守恒口径
pub const VECTOR_MIN: u8 = 0x20; // LAPIC 向量号下限，来源：x86 常规向量区
pub const VECTOR_MAX: u8 = 0xFE; // LAPIC 向量号上限
pub const PACK_ITEMS: usize = 4; // 凭证包四件，来源：判据原文

/// 一轮到齐位图。
#[derive(Clone, Copy, Debug)]
pub struct RoundMask {
    pub round: u16,  // 轮号 1..=50
    pub mask: u64,   // 到齐位图（bit c = 核 c 到齐；满值 0xFF = 8/8）
}

/// LAPIC 收讫记录。
#[derive(Clone, Copy, Debug)]
pub struct LapicRow {
    pub round: u16,  // 轮号 1..=50
    pub core: u8,    // 核号 0..=7
    pub vector: u8,  // IPI 向量号 0x20..=0xFE
    pub ack: u64,    // 收讫确认码（非 0）
}

/// 拓扑声明。
#[derive(Clone, Copy, Debug)]
pub struct TopoDecl {
    pub cores: u8,     // 声明核数（须 == 8）
    pub core_mask: u64, // 核号编址位图（须 == 0xFF）
}

/// 凭证件。
#[derive(Clone, Debug)]
pub struct Piece {
    pub kind: u8,    // 0=终核账 1=专项账 2=拓扑声明 3=LAPIC 账
    pub hash: u64,   // 非 0
    pub note: String,
}

/// 定格回执。
#[derive(Clone, Debug)]
pub struct V4Pin {
    pub rounds: u32,      // 轮数（50）
    pub full_rounds: u32, // 满轮数（50）
    pub lapic_rows: u32,  // LAPIC 账条数（400）
    pub summary: String,  // 摘要（非空）
    pub fingerprint: u64, // 账面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V4Stage {
    Intake = 0,
    Mapped = 1,
    Verified = 2,
    Packed = 3,
    Pinned = 4,
}
impl V4Stage {
    pub fn can_step(self, next: V4Stage) -> bool {
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
pub struct Verdict4 {  // 判据 4 终验机。
    stage: V4Stage,
    masks: Vec<RoundMask>,
    lapic: Vec<LapicRow>,
    lapic_seen: Vec<(u16, u8)>,
    topo: Option<TopoDecl>,
    pack: Vec<Piece>,
    pinned: Option<V4Pin>,
    events: Vec<String>,
}
impl Verdict4 {
    pub fn new() -> Self {
        Verdict4 {
            stage: V4Stage::Intake,
            masks: Vec::new(),
            lapic: Vec::new(),
            lapic_seen: Vec::new(),
            topo: None,
            pack: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> V4Stage {
        self.stage
    }
    pub fn round_count(&self) -> usize {
        self.masks.len()
    }
    pub fn lapic_count(&self) -> usize {
        self.lapic.len()
    }
    pub fn pack_len(&self) -> usize {
        self.pack.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_masks(&mut self, batch: &[RoundMask]) -> Result<u32, u16> {  // 喂入位图批：轮闭集唯一、核号编址越 8 位即红（mask 高位禁用）。
        if self.stage != V4Stage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in batch.iter() {
            if self.masks.len() >= ROUNDS {
                return Err(XC_ROUND_SHORT);
            }
            if r.round < 1 || r.round as usize > ROUNDS {
                return Err(XC_ROUND_BAD);
            }
            if r.mask & !CORE_MASK != 0 {
                return Err(XC_TOPO_DIFF); // 8 核外编址 = 拓扑闭集断裂
            }
            let mut i = 0usize;
            while i < self.masks.len() {
                if self.masks[i].round == r.round {
                    return Err(XC_DUP_ROUND);
                }
                i += 1;
            }
            self.masks.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("masks +{}", added));
        Ok(added)
    }
    pub fn is_full(&self) -> bool {
        self.masks.len() == ROUNDS
    }
    pub fn declare_topo(&mut self, t: TopoDecl) -> Result<u32, u16> {  // 拓扑声明登记：核数 == 8 且编址位图 == 0xFF。
        if self.stage != V4Stage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if t.cores != CORE_COUNT || t.core_mask != CORE_MASK {
            return Err(XC_TOPO_DIFF);
        }
        self.topo = Some(t);
        self.events.push(String::from("topo 8 cores"));
        Ok(t.cores as u32)
    }
    pub fn feed_lapic(&mut self, rows: &[LapicRow]) -> Result<u32, u16> {  // LAPIC 账登记：（轮, 核）唯一、向量闭集、ack 非 0、条数守卫。
        if !self.stage.can_step(V4Stage::Mapped) {
            return Err(XC_STATE_INVALID);
        }
        if rows.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for r in rows.iter() {
            if self.lapic.len() >= LAPIC_TOTAL {
                return Err(XC_LAPIC_SHORT);
            }
            if r.round < 1 || r.round as usize > ROUNDS {
                return Err(XC_ROUND_BAD);
            }
            if r.core >= CORE_COUNT {
                return Err(XC_TOPO_DIFF);
            }
            if r.vector < VECTOR_MIN || r.vector > VECTOR_MAX {
                return Err(XC_VECTOR_BAD);
            }
            if r.ack == 0 {
                return Err(XC_HASH_ZERO);
            }
            let key = (r.round, r.core);
            let mut i = 0usize;
            while i < self.lapic_seen.len() {
                if self.lapic_seen[i] == key {
                    return Err(XC_LAPIC_DUP);
                }
                i += 1;
            }
            self.lapic_seen.push(key);
            self.lapic.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("lapic +{}", added));
        Ok(added)
    }
    pub fn verify(&mut self) -> Result<u32, u16> {  // 终核：轮数 50 + 全轮 8/8 + LAPIC 400 条 + LAPIC-位图逐位互证。
        if !self.stage.can_step(V4Stage::Verified) {
            return Err(XC_STATE_INVALID);
        }
        if self.masks.len() != ROUNDS {
            return Err(XC_ROUND_SHORT);
        }
        // 全轮满位图。
        let mut full = 0u32;
        let mut i = 0usize;
        while i < self.masks.len() {
            if self.masks[i].mask == CORE_MASK {
                full += 1;
            }
            i += 1;
        }
        if full != ROUNDS as u32 {
            return Err(XC_NOT_FULL);
        }
        // LAPIC 账条数守恒。
        if self.lapic.len() != LAPIC_TOTAL {
            return Err(XC_LAPIC_SHORT);
        }
        // LAPIC-位图互证：每条记录对应轮必须满位图。
        let mut i = 0usize;
        while i < self.lapic.len() {
            let r = &self.lapic[i];
            let mut j = 0usize;
            let mut mask = 0u64;
            while j < self.masks.len() {
                if self.masks[j].round == r.round {
                    mask = self.masks[j].mask;
                    break;
                }
                j += 1;
            }
            if mask != CORE_MASK {
                return Err(XC_LAPIC_ORPHAN);
            }
            i += 1;
        }
        self.stage = V4Stage::Verified;
        self.events
            .push(alloc::format!("verify full={} lapic={}", full, self.lapic.len()));
        Ok(full)
    }
    pub fn register_pack(&mut self, pieces: &[Piece]) -> Result<u32, u16> {  // 凭证包登记：件型闭集、哈希非 0、型唯一。
        if !self.stage.can_step(V4Stage::Packed) {
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
    pub fn pack_assert(&mut self) -> Result<u32, u16> {  // 凭包齐断言。
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
        self.stage = V4Stage::Packed;
        Ok(PACK_ITEMS as u32)
    }
    pub fn pin(&mut self) -> Result<V4Pin, u16> {  // 定格：8/8×50 + LAPIC 400 + 凭包 4/4 → 摘要定格。
        if self.pinned.is_some() {
            return Err(XC_ALREADY_PINNED);
        }
        if self.stage != V4Stage::Packed {
            return Err(XC_STATE_INVALID);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0789-J1 终验过：8 核 50 轮 AP 到齐 8/8×50 定格，LAPIC 账 {} 条守恒互证，凭证包 4/4（终核账/专项账/拓扑声明/LAPIC 账）落账",
            self.lapic.len()
        );
        if summary.is_empty() {
            return Err(XC_HASH_ZERO);
        }
        let rc = V4Pin {
            rounds: self.masks.len() as u32,
            full_rounds: ROUNDS as u32,
            lapic_rows: self.lapic.len() as u32,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = V4Stage::Pinned;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&V4Pin> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.masks.len() {
            h = fnv1a(h, self.masks[i].round as u64);
            h = fnv1a(h, self.masks[i].mask);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.lapic.len() {
            let r = &self.lapic[j];
            h = fnv1a(h, r.round as u64);
            h = fnv1a(h, r.core as u64);
            h = fnv1a(h, r.vector as u64);
            h = fnv1a(h, r.ack);
            j += 1;
        }
        h
    }
    pub fn mask_of(&self, round: u16) -> Option<u64> {  // 单轮位图查询。
        let mut i = 0usize;
        while i < self.masks.len() {
            if self.masks[i].round == round {
                return Some(self.masks[i].mask);
            }
            i += 1;
        }
        None
    }
    pub fn arrived_count(&self, round: u16) -> Result<u32, u16> {  // 单轮到齐核数（popcount，整数）。
        let m = match self.mask_of(round) {
            Some(x) => x,
            None => return Err(XC_ROUND_BAD),
        };
        Ok(m.count_ones())
    }
    /// 逐核到齐轮数直方图：core → 到齐轮数（全满时 8 桶各 50，守恒恒等式
    pub fn per_core_histogram(&self) -> Result<[u32; CORE_COUNT as usize], u16> {  // Σ桶 == LAPIC_TOTAL）。依据：LAPIC 账（轮, 核）对去重计数。
        if self.lapic.is_empty() {
            return Err(XC_LAPIC_SHORT);
        }
        let mut hist = [0u32; CORE_COUNT as usize];
        let mut i = 0usize;
        while i < self.lapic.len() {
            let r = &self.lapic[i];
            if r.core >= CORE_COUNT {
                return Err(XC_TOPO_DIFF);
            }
            hist[r.core as usize] += 1;
            i += 1;
        }
        let mut total = 0u32;
        let mut c = 0usize;
        while c < CORE_COUNT as usize {
            total += hist[c];
            c += 1;
        }
        if total != self.lapic.len() as u32 {
            return Err(XC_LAPIC_SHORT);
        }
        Ok(hist)
    }

        pub fn piece_name(kind: u8) -> Option<&'static str> {  // 件型名表（闭集 0..=3 中文标签）。
        match kind {
            0 => Some("终核账"),
            1 => Some("专项账"),
            2 => Some("拓扑声明"),
            3 => Some("LAPIC 账"),
            _ => None,
        }
    }
}
pub fn gen_data(seed: u64) -> (Vec<RoundMask>, Vec<LapicRow>) {  // 样本发生器：50 轮全满位图 + 400 条 LAPIC 记录（确定性）。
    let mut s = seed;
    let mut masks: Vec<RoundMask> = Vec::new();
    let mut round = 1u16;
    while round as usize <= ROUNDS {
        masks.push(RoundMask { round, mask: CORE_MASK });
        round += 1;
    }
    let mut lapic: Vec<LapicRow> = Vec::new();
    let mut round = 1u16;
    while round as usize <= ROUNDS {
        let mut core = 0u8;
        while core < CORE_COUNT {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            lapic.push(LapicRow {
                round,
                core,
                vector: VECTOR_MIN + ((s >> 33) as u8 % (VECTOR_MAX - VECTOR_MIN)),
                ack: 0x60E0_0000_0000_0000 | ((round as u64) << 8) | core as u64,
            });
            core += 1;
        }
        round += 1;
    }
    (masks, lapic)
}
pub fn gen_pack(seed: u64) -> Vec<Piece> {  // 凭证包四件。
    let names = ["终核账", "专项账", "拓扑声明", "LAPIC 账"];
    let mut out: Vec<Piece> = Vec::new();
    let mut k = 0u8;
    while k < 4 {
        out.push(Piece {
            kind: k,
            hash: fnv1a(0x60E0_0000, seed ^ (k as u64)) | 1,
            note: alloc::format!("判据4凭证·{}", names[k as usize]),
        });
        k += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let (masks, lapic) = gen_data(0x60E0_0000_0000_0001);
    // —— 主路径：拓扑 → 位图 50 轮 → LAPIC 400 → 终核 → 凭包 → 定格 ——
    let mut m = Verdict4::new();
    m.declare_topo(TopoDecl { cores: CORE_COUNT, core_mask: CORE_MASK })?;
    m.feed_masks(&masks)?;
    if !m.is_full() {
        return Err(XC_ROUND_SHORT);
    }
    m.feed_lapic(&lapic)?;
    if m.lapic_count() != LAPIC_TOTAL {
        return Err(XC_LAPIC_SHORT);
    }
    if m.verify()? != ROUNDS as u32 {
        return Err(XC_NOT_FULL);
    }
    m.register_pack(&gen_pack(0x81))?;
    if m.pack_assert()? != 4 {
        return Err(XC_PACK_SHORT);
    }
    let rc = m.pin()?;
    if rc.rounds != 50 || rc.full_rounds != 50 || rc.lapic_rows != 400 || rc.summary.is_empty() {
        return Err(XC_HASH_ZERO);
    }
    if !matches!(m.pin(), Err(XC_ALREADY_PINNED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：某轮缺 1 核（位图 0xFE）→ 终核红 ——
    let mut m2 = Verdict4::new();
    let mut badmasks = masks.clone();
    badmasks[17].mask = 0xFE; // 核 0 未到齐
    m2.declare_topo(TopoDecl { cores: CORE_COUNT, core_mask: CORE_MASK })?;
    m2.feed_masks(&badmasks)?;
    m2.feed_lapic(&lapic)?;
    if !matches!(m2.verify(), Err(XC_NOT_FULL)) {
        return Err(XC_NOT_FULL);
    }
    // —— 错误路径 2：LAPIC 短账 / 重（轮, 核）/ 向量越界 ——
    let mut m3 = Verdict4::new();
    m3.declare_topo(TopoDecl { cores: CORE_COUNT, core_mask: CORE_MASK })?;
    m3.feed_masks(&masks)?;
    m3.feed_lapic(&lapic[..399].to_vec())?;
    if !matches!(m3.verify(), Err(XC_LAPIC_SHORT)) {
        return Err(XC_LAPIC_SHORT);
    }
    let mut m4 = Verdict4::new();
    m4.declare_topo(TopoDecl { cores: CORE_COUNT, core_mask: CORE_MASK })?;
    m4.feed_masks(&masks)?;
    let mut dupl = lapic.clone();
    dupl[5].round = dupl[0].round;
    dupl[5].core = dupl[0].core;
    if !matches!(m4.feed_lapic(&dupl), Err(XC_LAPIC_DUP)) {
        return Err(XC_LAPIC_DUP);
    }
    let mut m5 = Verdict4::new();
    m5.declare_topo(TopoDecl { cores: CORE_COUNT, core_mask: CORE_MASK })?;
    m5.feed_masks(&masks)?;
    let mut badv = lapic.clone();
    badv[9].vector = 0x10;
    if !matches!(m5.feed_lapic(&badv), Err(XC_VECTOR_BAD)) {
        return Err(XC_VECTOR_BAD);
    }
    // —— 错误路径 3：拓扑声明不符 / 轮号越界 / 位图高位编址 ——
    let mut m6 = Verdict4::new();
    if m6.declare_topo(TopoDecl { cores: 4, core_mask: 0x0F }) != Err(XC_TOPO_DIFF) {
        return Err(XC_TOPO_DIFF);
    }
    let mut m7 = Verdict4::new();
    let mut badr = masks[..3].to_vec();
    badr[2].round = 51;
    if !matches!(m7.feed_masks(&badr), Err(XC_ROUND_BAD)) {
        return Err(XC_ROUND_BAD);
    }
    let mut m8 = Verdict4::new();
    let mut highbit = masks[..3].to_vec();
    highbit[1].mask = 0x1FF; // bit8 越出 8 核编址
    if !matches!(m8.feed_masks(&highbit), Err(XC_TOPO_DIFF)) {
        return Err(XC_TOPO_DIFF);
    }
    // —— 错误路径 4：凭包缺件 / 件型重复 ——
    let mut m9 = Verdict4::new();
    m9.declare_topo(TopoDecl { cores: CORE_COUNT, core_mask: CORE_MASK })?;
    m9.feed_masks(&masks)?;
    m9.feed_lapic(&lapic)?;
    m9.verify()?;
    m9.register_pack(&gen_pack(0x82)[..3].to_vec())?;
    if !matches!(m9.pack_assert(), Err(XC_PACK_SHORT)) {
        return Err(XC_PACK_SHORT);
    }
    // —— 查询 API：位图/到齐数/件型名表 ——
    if m.mask_of(50) != Some(CORE_MASK) || m.mask_of(51).is_some() {
        return Err(XC_ROUND_BAD);
    }
    if m.arrived_count(1)? != 8 || !matches!(m.arrived_count(99), Err(XC_ROUND_BAD)) {
        return Err(XC_ROUND_BAD);
    }
    let mut pk = 0u8;
    while pk < 4 {
        if Verdict4::piece_name(pk).is_none() {
            return Err(XC_DUP_PACK);
        }
        pk += 1;
    }
    if Verdict4::piece_name(4).is_some() {
        return Err(XC_DUP_PACK);
    }
    // —— 逐核直方图：8 桶各 50，和守恒 400 ——
    let hist = m.per_core_histogram()?;
    let mut hsum = 0u32;
    let mut hc = 0usize;
    while hc < CORE_COUNT as usize {
        if hist[hc] != ROUNDS as u32 {
            return Err(XC_LAPIC_SHORT);
        }
        hsum += hist[hc];
        hc += 1;
    }
    if hsum != LAPIC_TOTAL as u32 {
        return Err(XC_LAPIC_SHORT);
    }
    Ok(0)
}
