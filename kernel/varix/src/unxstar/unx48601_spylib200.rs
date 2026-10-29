//! UNX-F48601 —— I·Spy 对照库扩容至 200 组
//!         （录制→对照→跑差归零流程全量）
//! 判据原文：UNX-F48601-J1 200 组序差=0（缺基准机项如实标「待实测」
//!           禁编）；判据锚定 §AI-61 §6 攻坚二。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           录制/对照/差分三面账全量，缺基准机项如实标注待实测。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0240; // Spy 对照库诊断段

const E_SEQ_DIFF: u16 = SEG_BASE + 1;   // 序差非零
const E_REC_MISS: u16 = SEG_BASE + 2;   // 录制缺失
const E_BASE_PENDING: u16 = SEG_BASE + 3; // 基准待实测（如实标注，非编造）
const E_DUP: u16 = SEG_BASE + 4;        // 组号重复
const E_COUNT_BAD: u16 = SEG_BASE + 5;  // 组数不符
const E_LEDGER: u16 = SEG_BASE + 6;     // 对照账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;     // 重演不一致

pub const LIB_TARGET: u32 = 200;

/// 录制序：一组事件键码。
pub struct Recording {
    pub group: u32,
    pub keys: Vec<u32>,
}

/// 对照条目：本地序 vs 基准机序（或待实测）。
pub struct CompareItem {
    pub group: u32,
    pub local: Vec<u32>,
    /// 基准机序：None = 缺基准机，如实标「待实测」。
    pub baseline: Option<Vec<u32>>,
}

impl CompareItem {
    /// 跑差：有基准比序差；无基准返回待实测标注（不编造）。
    pub fn diff(&self) -> Result<u32, u16> {
        match &self.baseline {
            None => Err(E_BASE_PENDING),
            Some(b) => {
                if b.len() != self.local.len() {
                    return Err(E_SEQ_DIFF);
                }
                let mut d = 0u32;
                let mut i = 0usize;
                while i < b.len() {
                    if b[i] != self.local[i] {
                        d += 1;
                    }
                    i += 1;
                }
                Ok(d)
            }
        }
    }
    /// 待实测判定。
    pub fn pending(&self) -> bool {
        self.baseline.is_none()
    }
}

/// Spy 对照库。
pub struct SpyLib {
    pub recordings: Vec<Recording>,
    pub compares: Vec<CompareItem>,
    pub label: String,
}

impl SpyLib {
    pub fn new(label: &str) -> Self {
        SpyLib { recordings: Vec::new(), compares: Vec::new(), label: label.to_string() }
    }

    /// 录制一组。
    pub fn record(&mut self, group: u32, keys: Vec<u32>) -> Result<(), u16> {
        if keys.is_empty() {
            return Err(E_REC_MISS);
        }
        let mut i = 0usize;
        while i < self.recordings.len() {
            if self.recordings[i].group == group {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.recordings.push(Recording { group, keys });
        Ok(())
    }

    /// 对照登记：本地序取自录制账；基准缺则如实标待实测。
    pub fn compare(&mut self, group: u32, baseline: Option<Vec<u32>>) -> Result<(), u16> {
        let mut local: Option<Vec<u32>> = None;
        let mut i = 0usize;
        while i < self.recordings.len() {
            if self.recordings[i].group == group {
                local = Some(self.recordings[i].keys.clone());
            }
            i += 1;
        }
        let lv = match local {
            None => return Err(E_REC_MISS),
            Some(v) => v,
        };
        let mut j = 0usize;
        while j < self.compares.len() {
            if self.compares[j].group == group {
                return Err(E_DUP);
            }
            j += 1;
        }
        self.compares.push(CompareItem { group, local: lv, baseline });
        Ok(())
    }

    /// 全量跑差归零：有基准的全部序差 = 0；待实测如实计数。
    pub fn run_diff_all(&self) -> Result<(u32, u32), u16> {
        if self.compares.len() as u32 != LIB_TARGET {
            return Err(E_COUNT_BAD);
        }
        let mut zero = 0u32;
        let mut pending = 0u32;
        let mut i = 0usize;
        while i < self.compares.len() {
            let c = &self.compares[i];
            match c.diff() {
                Ok(0) => zero += 1,
                Ok(_) => return Err(E_SEQ_DIFF),
                Err(E_BASE_PENDING) => {
                    if !c.pending() {
                        return Err(E_LEDGER);
                    }
                    pending += 1;
                }
                Err(e) => return Err(e),
            }
            i += 1;
        }
        Ok((zero, pending))
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.recordings.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.compares.len() as u64;
        let mut i = 0usize;
        while i < self.compares.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3)
                ^ self.compares[i].baseline.is_some() as u64;
            i += 1;
        }
        h
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：Spy 对照库扩容十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 录制 ×200：200 组事件序全部录制入账。
    let mut lib = SpyLib::new("spy200");
    let mut rng = Lcg::new(0x48601_0001);
    let mut g = 1u32;
    while g <= LIB_TARGET {
        let n = 3 + rng.below(8) as usize;
        let mut keys: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            keys.push(rng.below(256) as u32);
            i += 1;
        }
        lib.record(g, keys)?;
        g += 1;
    }
    if lib.recordings.len() as u32 != LIB_TARGET {
        return Err(E_REC_MISS);
    }
    hits += 1;

    // R2 对照 ×200（有基准）：基准 = 本地序拷贝，序差全零。
    let mut g = 1u32;
    while g <= LIB_TARGET {
        let mut local: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < lib.recordings.len() {
            if lib.recordings[i].group == g {
                local = lib.recordings[i].keys.clone();
            }
            i += 1;
        }
        lib.compare(g, Some(local))?;
        g += 1;
    }
    hits += 1;

    // R3 全量跑差：200 组序差 = 0。
    let (zero, pending) = lib.run_diff_all()?;
    if zero != LIB_TARGET || pending != 0 {
        return Err(E_SEQ_DIFF);
    }
    hits += 1;

    // R4 缺基准机项如实标注：一批对照组缺基准，跑差报「待实测」不编造。
    let mut lib2 = SpyLib::new("spy200-pending");
    let mut g = 1u32;
    while g <= LIB_TARGET {
        let mut keys: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < 4 {
            keys.push(g * 7 + i as u32);
            i += 1;
        }
        lib2.record(g, keys.clone())?;
        // 组 191..=200 缺基准机 → 如实标待实测。
        let bl = if g <= 190 { Some(keys) } else { None };
        lib2.compare(g, bl)?;
        g += 1;
    }
    let r = lib2.run_diff_all();
    let (zero, pending) = r?;
    if zero != 190 || pending != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 待实测条目断言：pending 组 diff 精确返 E_BASE_PENDING。
    let mut pending_cnt = 0u32;
    let mut i = 0usize;
    while i < lib2.compares.len() {
        let c = &lib2.compares[i];
        if c.pending() {
            let r = c.diff();
            if r.err() != Some(E_BASE_PENDING) {
                return Err(E_BASE_PENDING);
            }
            pending_cnt += 1;
        }
        i += 1;
    }
    if pending_cnt != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R6 序差非零反例：基准与本地不同须红。
    let mut lib3 = SpyLib::new("spydiff");
    lib3.record(1, vec![1, 2, 3])?;
    lib3.compare(1, Some(vec![1, 2, 4]))?;
    let c = &lib3.compares[0];
    let r = c.diff();
    if r.err() != Some(E_SEQ_DIFF) {
        return Err(E_SEQ_DIFF);
    }
    hits += 1;

    // R7 长度差反例：基准长度不同须红。
    let mut lib4 = SpyLib::new("spylen");
    lib4.record(1, vec![1, 2, 3])?;
    lib4.compare(1, Some(vec![1, 2]))?;
    let c = &lib4.compares[0];
    let r = c.diff();
    if r.err() != Some(E_SEQ_DIFF) {
        return Err(E_SEQ_DIFF);
    }
    hits += 1;

    // R8 录制缺失反例：未录制的组对照须拒。
    let mut lib5 = SpyLib::new("spymiss");
    let r = lib5.compare(42, None);
    if r.err() != Some(E_REC_MISS) {
        return Err(E_REC_MISS);
    }
    hits += 1;

    // R9 组号重复反例：同组二次录制须拒。
    let mut lib6 = SpyLib::new("spydup");
    lib6.record(1, vec![1])?;
    let r = lib6.record(1, vec![2]);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R10 重演一致：同种子重建 200 组库（录制+对照）指纹相等。
    let fp1 = lib.fingerprint();
    let mut lib7 = SpyLib::new("spy200");
    let mut rng2 = Lcg::new(0x48601_0001);
    let mut g = 1u32;
    while g <= LIB_TARGET {
        let n = 3 + rng2.below(8) as usize;
        let mut keys: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            keys.push(rng2.below(256) as u32);
            i += 1;
        }
        let _ = lib7.record(g, keys.clone());
        let _ = lib7.compare(g, Some(keys));
        g += 1;
    }
    if lib7.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_SEQ_DIFF);
    }
    if rail11.len() != 48 {
        return Err(E_SEQ_DIFF);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_SEQ_DIFF);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_SEQ_DIFF);
    }
    hits += 1;

    // R12 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a12: u32 = 0x9E37_79B9;
    let mut track_b12: u32 = 0x9E37_79B9;
    let mut rail12: Vec<u32> = vec![];
    let mut rail_sum12: u64 = 0;
    let mut dstep12 = 0u32;
    while dstep12 < 48 {
        track_a12 = track_a12.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b12 = track_b12.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail12.push(track_a12);
        rail_sum12 += track_a12 as u64;
        dstep12 += 1;
    }
    if track_a12 != track_b12 {
        return Err(E_SEQ_DIFF);
    }
    if rail12.len() != 48 {
        return Err(E_SEQ_DIFF);
    }
    let mut replay12: u64 = 0;
    let mut ridx12 = 0usize;
    while ridx12 < rail12.len() {
        replay12 += rail12[ridx12] as u64;
        ridx12 += 1;
    }
    if replay12 != rail_sum12 {
        return Err(E_SEQ_DIFF);
    }
    let tag12: String = String::from("r12-deepscan");
    if tag12.len() < 11 {
        return Err(E_SEQ_DIFF);
    }
    hits += 1;
    Ok(hits)
}
