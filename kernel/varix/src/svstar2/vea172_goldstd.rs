//! VE-F0172 · 金标准图像库管理（VE-Test · 目标 340 行）
//!
//! 铁律：金标可登记，版本可追溯，晋升可审，退役可查。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8F2;
pub const XC_GOLD_MISSING: u16 = SEG_BASE | 0x01;
pub const XC_GOLD_DUPLICATE: u16 = SEG_BASE | 0x02;
pub const XC_PROMOTE_REJECTED: u16 = SEG_BASE | 0x03;
pub const XC_VERSION_REGRESSED: u16 = SEG_BASE | 0x04;
pub const XC_GOLD_RETIRED: u16 = SEG_BASE | 0x05;
pub const XC_LIB_EMPTY: u16 = SEG_BASE | 0x06;

/// 金标条目：一景一版一 hash。
#[derive(Clone, Copy)]
pub struct GoldEntry {
    pub scene_id: u32,
    pub version: u32,
    pub hash: u64,
    pub retired: bool,
    pub trust_bps: u32,
}

/// 候选帧：待晋升的实测结果。
#[derive(Clone, Copy)]
pub struct Candidate {
    pub scene_id: u32,
    pub hash: u64,
    pub measured_us: u32,
}

/// 金标准图像库。
pub struct GoldLibrary {
    entries: Vec<GoldEntry>,
    promotions: u32,
    retires: u32,
    rejections: u32,
    min_trust_bps: u32,
    last_code: u16,
}

impl GoldLibrary {
    pub fn new(min_trust_bps: u32) -> Self {
        Self {
            entries: Vec::new(),
            promotions: 0,
            retires: 0,
            rejections: 0,
            min_trust_bps,
            last_code: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 登记金标；同 scene+version 二次登记判重。
    pub fn add_golden(&mut self, scene_id: u32, version: u32, hash: u64) -> Result<usize, u16> {
        if self
            .entries
            .iter()
            .any(|e| e.scene_id == scene_id && e.version == version)
        {
            self.last_code = XC_GOLD_DUPLICATE;
            return Err(XC_GOLD_DUPLICATE);
        }
        self.entries.push(GoldEntry {
            scene_id,
            version,
            hash,
            retired: false,
            trust_bps: 10_000,
        });
        self.last_code = 0;
        Ok(self.entries.len())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 找某景某版金标（含已退役，供审计）。
    pub fn find(&self, scene_id: u32, version: u32) -> Option<&GoldEntry> {
        self.entries
            .iter()
            .find(|e| e.scene_id == scene_id && e.version == version)
    }

    /// 找某景当前有效最高版；全退役则 None。
    pub fn latest(&self, scene_id: u32) -> Option<&GoldEntry> {
        self.entries
            .iter()
            .filter(|e| e.scene_id == scene_id && !e.retired)
            .max_by_key(|e| e.version)
    }

    /// 某景版本总数（含退役）。
    pub fn version_count(&self, scene_id: u32) -> usize {
        self.entries.iter().filter(|e| e.scene_id == scene_id).count()
    }

    /// 候选晋升：trust 不足拒；接受则以 version+1 入库。
    pub fn promote(&mut self, cand: Candidate, trust_bps: u32) -> Result<u32, u16> {
        if trust_bps < self.min_trust_bps {
            self.rejections += 1;
            self.last_code = XC_PROMOTE_REJECTED;
            return Err(XC_PROMOTE_REJECTED);
        }
        let next = self
            .latest(cand.scene_id)
            .map(|e| e.version + 1)
            .unwrap_or(1);
        if self
            .entries
            .iter()
            .any(|e| e.scene_id == cand.scene_id && e.version == next)
        {
            self.last_code = XC_VERSION_REGRESSED;
            return Err(XC_VERSION_REGRESSED);
        }
        self.entries.push(GoldEntry {
            scene_id: cand.scene_id,
            version: next,
            hash: cand.hash,
            retired: false,
            trust_bps,
        });
        self.promotions += 1;
        self.last_code = 0;
        Ok(next)
    }

    /// 退役指定版本；缺失或已退役报错。
    pub fn retire(&mut self, scene_id: u32, version: u32) -> Result<u64, u16> {
        for e in self.entries.iter_mut() {
            if e.scene_id == scene_id && e.version == version {
                if e.retired {
                    self.last_code = XC_GOLD_RETIRED;
                    return Err(XC_GOLD_RETIRED);
                }
                e.retired = true;
                self.retires += 1;
                self.last_code = 0;
                return Ok(e.hash);
            }
        }
        self.last_code = XC_GOLD_MISSING;
        Err(XC_GOLD_MISSING)
    }

    /// 实测帧对齐当前金标：hash 一致为过。
    pub fn diff_latest(&mut self, scene_id: u32, actual_hash: u64) -> Result<bool, u16> {
        match self.latest(scene_id) {
            Some(e) => {
                let h = e.hash;
                self.last_code = 0;
                Ok(h == actual_hash)
            }
            None => {
                self.last_code = if self.entries.is_empty() {
                    XC_LIB_EMPTY
                } else {
                    XC_GOLD_MISSING
                };
                Err(self.last_code)
            }
        }
    }

    /// 某景有效（未退役）版本数。
    pub fn active_count(&self, scene_id: u32) -> usize {
        self.entries
            .iter()
            .filter(|e| e.scene_id == scene_id && !e.retired)
            .count()
    }

    /// 低信任度条目数（trust < 门槛）。
    pub fn low_trust_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.trust_bps < self.min_trust_bps)
            .count()
    }

    pub fn promotions(&self) -> u32 {
        self.promotions
    }

    pub fn retires(&self) -> u32 {
        self.retires
    }

    pub fn rejections(&self) -> u32 {
        self.rejections
    }

    /// 库健康度：有有效金标且无低信任条目为绿。
    pub fn healthy(&self) -> bool {
        !self.entries.is_empty() && self.low_trust_count() == 0
    }

    /// 批量对齐：返回 (过, 总) 计数。
    pub fn diff_batch(&mut self, scene_id: u32, hashes: &[u64]) -> Result<(u32, u32), u16> {
        if hashes.is_empty() {
            self.last_code = XC_LIB_EMPTY;
            return Err(XC_LIB_EMPTY);
        }
        let mut pass = 0u32;
        for &h in hashes {
            if self.diff_latest(scene_id, h)? {
                pass += 1;
            }
        }
        self.last_code = 0;
        Ok((pass, hashes.len() as u32))
    }
}

pub fn run_vea172_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("vea172_goldstd");
    {
        let g = GoldLibrary::new(8_000);
        chk(&mut set, "C172-01", g.is_empty() && g.promotions() == 0, "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        let r = g.add_golden(1, 1, 0xF00D);
        chk(&mut set, "C172-02", r.is_ok() && g.len() == 1, "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        g.add_golden(1, 1, 0xF00D);
        let r = g.add_golden(1, 1, 0xBEEF);
        chk(&mut set, "C172-03", r.err() == Some(XC_GOLD_DUPLICATE), "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        g.add_golden(1, 1, 0xF00D);
        let v = g.latest(1).map(|e| e.version);
        chk(&mut set, "C172-04", v == Some(1), "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        let cand = Candidate { scene_id: 9, hash: 0xABC, measured_us: 100 };
        let r = g.promote(cand, 9_000);
        chk(&mut set, "C172-05", r == Ok(1) && g.promotions() == 1, "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        let cand = Candidate { scene_id: 9, hash: 0xABC, measured_us: 100 };
        let r = g.promote(cand, 100);
        chk(&mut set, "C172-06", r.err() == Some(XC_PROMOTE_REJECTED) && g.rejections() == 1, "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        g.add_golden(1, 1, 0xF00D);
        let r = g.retire(1, 1);
        chk(&mut set, "C172-07", r == Ok(0xF00D) && g.active_count(1) == 0, "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        let r = g.retire(1, 1);
        chk(&mut set, "C172-08", r.err() == Some(XC_GOLD_MISSING), "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        g.add_golden(1, 1, 0xF00D);
        let ok = g.diff_latest(1, 0xF00D);
        chk(&mut set, "C172-09", ok == Ok(true), "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        let ok = g.diff_latest(1, 0xF00D);
        chk(&mut set, "C172-10", ok.err() == Some(XC_LIB_EMPTY), "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        g.add_golden(1, 1, 0xF00D);
        let r = g.diff_batch(1, &[0xF00D, 0xBAD]);
        chk(&mut set, "C172-11", r == Ok((1, 2)), "err");
    }
    {
        let mut g = GoldLibrary::new(8_000);
        g.add_golden(1, 1, 0xF00D);
        let r = g.retire(1, 1);
        let r2 = g.retire(1, 1);
        chk(&mut set, "C172-12", r.is_ok() && r2.err() == Some(XC_GOLD_RETIRED), "err");
    }
    set
}
