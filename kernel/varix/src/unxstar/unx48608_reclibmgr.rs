//! UNX-F48608 —— I·对照库变更管理（录制库校验和+版本化）
//! 判据原文：UNX-F48608-J1 被改即判据红断言；判据锚定 §AI-61 §5 样张纪律。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           样张库校验和冻结 + 版本递增，任何篡改即红。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0320; // 对照库变更管理诊断段

const E_TAMPER: u16 = SEG_BASE + 1;   // 样张被改
const E_VER_BAD: u16 = SEG_BASE + 2;  // 版本违例
const E_DUP: u16 = SEG_BASE + 3;      // 样张重复
const E_SUM_MISS: u16 = SEG_BASE + 4; // 校验和缺失
const E_LEDGER: u16 = SEG_BASE + 5;   // 变更账不守恒
const E_REPLAY: u16 = SEG_BASE + 6;   // 重演不一致
const E_ROLLBACK_BAD: u16 = SEG_BASE + 7; // 回滚失效

/// 录制样张。
pub struct Clip {
    pub id: u32,
    pub keys: Vec<u32>,
    pub ver: u32,
}

impl Clip {
    /// 校验和（含版本）。
    pub fn checksum(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.ver as u64;
        let mut i = 0usize;
        while i < self.keys.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.keys[i] as u64;
            i += 1;
        }
        h
    }
}

/// 变更管理账：样张 + 冻结校验和 + 版本历史。
pub struct ChangeLedger {
    pub clips: Vec<Clip>,
    pub sums: Vec<u64>,
    pub versions: Vec<Vec<u32>>, // 每样张的版本历史
    pub label: String,
}

impl ChangeLedger {
    pub fn new(label: &str) -> Self {
        ChangeLedger {
            clips: Vec::new(),
            sums: Vec::new(),
            versions: Vec::new(),
            label: label.to_string(),
        }
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.clips.len() {
            if self.clips[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 入库：版本 1，冻结校验和。
    pub fn ingest(&mut self, id: u32, keys: Vec<u32>) -> Result<(), u16> {
        if keys.is_empty() {
            return Err(E_SUM_MISS);
        }
        if self.find(id).is_some() {
            return Err(E_DUP);
        }
        let c = Clip { id, keys, ver: 1 };
        let s = c.checksum();
        let mut hist = Vec::new();
        hist.push(1u32);
        self.clips.push(c);
        self.sums.push(s);
        self.versions.push(hist);
        Ok(())
    }

    /// 变更：版本 +1，重算校验和入账。
    pub fn revise(&mut self, id: u32, keys: Vec<u32>) -> Result<(), u16> {
        let idx = match self.find(id) {
            None => return Err(E_SUM_MISS),
            Some(i) => i,
        };
        if keys.is_empty() {
            return Err(E_SUM_MISS);
        }
        let nv = self.clips[idx].ver + 1;
        self.clips[idx].keys = keys;
        self.clips[idx].ver = nv;
        self.sums[idx] = self.clips[idx].checksum();
        self.versions[idx].push(nv);
        Ok(())
    }

    /// 防篡改机检：现值校验和与冻结账一致。
    pub fn anti_tamper(&self) -> Result<(), u16> {
        if self.clips.len() != self.sums.len() {
            return Err(E_LEDGER);
        }
        let mut i = 0usize;
        while i < self.clips.len() {
            if self.clips[i].checksum() != self.sums[i] {
                return Err(E_TAMPER);
            }
            i += 1;
        }
        Ok(())
    }

    /// 版本单调：历史严格递增。
    pub fn version_audit(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.versions.len() {
            let hist = &self.versions[i];
            if hist.is_empty() || hist[0] != 1 {
                return Err(E_VER_BAD);
            }
            let mut j = 1usize;
            while j < hist.len() {
                if hist[j] <= hist[j - 1] {
                    return Err(E_VER_BAD);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.sums.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.sums[i];
            i += 1;
        }
        h ^= self.clips.len() as u64;
        h
    }
}

/// 篡改注入探针：外部直改样张键序 → 防篡改机检必须红。
pub fn inject_tamper(led: &mut ChangeLedger, id: u32, fake: Vec<u32>) -> Result<(), u16> {
    let idx = match led.find(id) {
        None => return Err(E_SUM_MISS),
        Some(i) => i,
    };
    // 模拟越界直改（绕过 revise 的正常路径）。
    led.clips[idx].keys = fake;
    // 防篡改机检必须检出。
    let r = led.anti_tamper();
    match r {
        Err(E_TAMPER) => Ok(()),
        Ok(()) => Err(E_TAMPER), // 未检出即机检失守
        Err(e) => Err(e),
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

/// 主巡检：对照库变更管理十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 入库 ×8：八张样张版本 1 入账。
    let mut led = ChangeLedger::new("reclib");
    let mut id = 1u32;
    while id <= 8 {
        let mut keys = Vec::new();
        let mut i = 0u32;
        while i < 4 {
            keys.push(id * 10 + i);
            i += 1;
        }
        led.ingest(id, keys)?;
        id += 1;
    }
    led.anti_tamper()?;
    led.version_audit()?;
    hits += 1;

    // R2 变更 ×3：三张样张升版，版本历史单调。
    led.revise(2, vec![21, 22, 23, 24, 25])?;
    led.revise(3, vec![31, 32])?;
    led.revise(5, vec![51, 52, 53])?;
    led.version_audit()?;
    if led.versions[1].len() != 2 || led.versions[1][1] != 2 {
        return Err(E_VER_BAD);
    }
    hits += 1;

    // R3 防篡改红断言：直改键序后机检必须红。
    let mut led2 = ChangeLedger::new("tamper");
    led2.ingest(1, vec![1, 2, 3])?;
    let r = inject_tamper(&mut led2, 1, vec![9, 9, 9]);
    if r.err() != Some(E_TAMPER) {
        return Err(E_TAMPER);
    }
    hits += 1;

    // R4 未改样张白断言：注入与原值相同的「改」不触发红。
    let mut led3 = ChangeLedger::new("same");
    led3.ingest(1, vec![4, 5, 6])?;
    let r = inject_tamper(&mut led3, 1, vec![4, 5, 6]);
    if r.is_err() {
        return Err(E_TAMPER);
    }
    hits += 1;

    // R5 版本违例反例：版本历史回退不可达（revise 恒 +1）。
    let mut led4 = ChangeLedger::new("ver");
    led4.ingest(1, vec![1])?;
    led4.revise(1, vec![2])?;
    led4.revise(1, vec![3])?;
    led4.version_audit()?;
    if led4.versions[0].len() != 3 || led4.clips[0].ver != 3 {
        return Err(E_VER_BAD);
    }
    hits += 1;

    // R6 样张重复反例：同 ID 二次入库须拒。
    let mut led5 = ChangeLedger::new("dup");
    led5.ingest(1, vec![1])?;
    let r = led5.ingest(1, vec![2]);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R7 空键序反例：空样张入库/升版须拒。
    let mut led6 = ChangeLedger::new("empty");
    let r = led6.ingest(1, Vec::new());
    if r.err() != Some(E_SUM_MISS) {
        return Err(E_SUM_MISS);
    }
    led6.ingest(2, vec![1])?;
    let r = led6.revise(2, Vec::new());
    if r.err() != Some(E_SUM_MISS) {
        return Err(E_SUM_MISS);
    }
    hits += 1;

    // R8 未知样张反例：改/查不存在 ID 须拒。
    let mut led7 = ChangeLedger::new("miss");
    let r = led7.revise(42, vec![1]);
    if r.err() != Some(E_SUM_MISS) {
        return Err(E_SUM_MISS);
    }
    hits += 1;

    // R9 fuzz ×64 随机入库/升版：全账防篡改 + 版本单调绿。
    let mut rng = Lcg::new(0x48608_0001);
    let mut led8 = ChangeLedger::new("fuzz");
    let mut k = 0u32;
    while k < 64 {
        let id = 1 + rng.below(10) as u32;
        let n = 1 + rng.below(6) as usize;
        let mut keys = Vec::new();
        let mut i = 0usize;
        while i < n {
            keys.push(rng.below(256) as u32);
            i += 1;
        }
        if led8.find(id).is_some() {
            led8.revise(id, keys)?;
        } else {
            led8.ingest(id, keys)?;
        }
        led8.anti_tamper()?;
        led8.version_audit()?;
        k += 1;
    }
    hits += 1;

    // R10 重演一致：同种子重建 fuzz 账，指纹相等。
    let fp1 = led8.fingerprint();
    let mut rng2 = Lcg::new(0x48608_0001);
    let mut led9 = ChangeLedger::new("fuzz");
    let mut k = 0u32;
    while k < 64 {
        let id = 1 + rng2.below(10) as u32;
        let n = 1 + rng2.below(6) as usize;
        let mut keys = Vec::new();
        let mut i = 0usize;
        while i < n {
            keys.push(rng2.below(256) as u32);
            i += 1;
        }
        let has = {
            let mut f = false;
            let mut j = 0usize;
            while j < led9.clips.len() {
                if led9.clips[j].id == id {
                    f = true;
                }
                j += 1;
            }
            f
        };
        if has {
            let _ = led9.revise(id, keys);
        } else {
            let _ = led9.ingest(id, keys);
        }
        k += 1;
    }
    if led9.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
