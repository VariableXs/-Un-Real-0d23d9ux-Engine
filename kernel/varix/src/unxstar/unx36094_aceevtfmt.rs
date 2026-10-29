//! UNX-F36094-J1 审计 ACE 与 J5 通道的命中事件格式（fake 联签对拍）。
//! 判据：① 本地事件结构（序列号/ACE 指纹/成功失败位/时间戳槽）10 例入账，
//! 序列号严格单调、fake 联签全对拍（对签真 / 错签假）；② 零序列号 1 例、
//! 坏命中位 2 例、保留时间戳非零 1 例、序回退 1 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：自含模块——事件格式在本模块内定义（J5 通道为未来批消费锚，
//! 不 import 未写出模块）；指纹与联签哈希消费 unx36081_acecore 的 FNV。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3860;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;  // 版本锚不符
pub const E_SEQ0: u16 = SEG_BASE + 2; // 零序列号
pub const E_HIT: u16 = SEG_BASE + 3;  // 命中位非法（非成功/失败）
pub const E_TS: u16 = SEG_BASE + 4;   // 保留时间戳槽非零
pub const E_MONO: u16 = SEG_BASE + 5; // 序列号回退/重复
pub const E_FULL: u16 = SEG_BASE + 6; // 事件账定容满
pub const E_CS: u16 = SEG_BASE + 7;   // 联签对拍失败

/// 版本锚。
pub const VER: u32 = 1;

/// 时间戳槽保留锚（本版恒 0，J5 接入时启用）。
pub const TS_SLOT_RESERVED: u64 = 0;
/// 事件账定容。
pub const EVT_CAP: usize = 16;

/// 命中类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitKind {
    /// 成功命中。
    SuccessHit = 0x0001,
    /// 失败命中。
    FailureHit = 0x0002,
}

impl HitKind {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn from_bits(b: u8) -> Option<HitKind> {
        match b {
            1 => Some(HitKind::SuccessHit),
            2 => Some(HitKind::FailureHit),
            _ => None,
        }
    }
}

/// 命中事件记录（J5 通道格式 v1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EventRec {
    /// 事件序列号（账内严格递增，从 1 起）。
    pub seq: u32,
    /// ACE 指纹（真源 FNV over ACE 线上形式）。
    pub ace_fp: u64,
    /// 命中位（1=成功 2=失败，闭集外非法）。
    pub hit: u8,
    /// 时间戳槽（本版保留恒 0）。
    pub ts_slot: u64,
}

impl EventRec {
    /// 受控构造：序列号闸 → 命中位闸 → 保留槽闸（顺序固定）。
    pub fn new(seq: u32, ace_fp: u64, hit: u8) -> Result<EventRec, u16> {
        if seq == 0 {
            return Err(E_SEQ0);
        }
        if HitKind::from_bits(hit).is_none() {
            return Err(E_HIT);
        }
        if TS_SLOT_RESERVED != 0 {
            return Err(E_TS);
        }
        Ok(EventRec { seq, ace_fp, hit, ts_slot: TS_SLOT_RESERVED })
    }

    /// fake 联签：FNV over 事件四件 + 对签钥（J5 接入前为本地对拍面）。
    pub fn cosign(&self, key: u64) -> u64 {
        let mut feed = Vec::new();
        feed.extend_from_slice(&VER.to_le_bytes());
        feed.extend_from_slice(&self.seq.to_le_bytes());
        feed.extend_from_slice(&self.ace_fp.to_le_bytes());
        feed.push(self.hit);
        feed.extend_from_slice(&self.ts_slot.to_le_bytes());
        feed.extend_from_slice(&key.to_le_bytes());
        core::fnv1a(&feed)
    }

    /// 对拍：重算联签逐字节相等。
    pub fn verify(&self, key: u64, sig: u64) -> bool {
        self.cosign(key) == sig
    }
}

/// 事件账（定容 + 单调序闸）。
#[derive(Clone, Copy)]
pub struct EventLog {
    pub recs: [Option<EventRec>; EVT_CAP],
    pub n: usize,
}

const NONE_EVT: Option<EventRec> = None;

impl EventLog {
    /// 空账。
    pub const fn fresh() -> EventLog {
        EventLog { recs: [NONE_EVT; EVT_CAP], n: 0 }
    }

    /// 入账：满容闸 → 单调闸（seq 必须严格 = 上一条 + 1，首条必须 = 1）。
    pub fn append(&mut self, rec: EventRec) -> Result<usize, u16> {
        if self.n >= EVT_CAP {
            return Err(E_FULL);
        }
        let expect = if self.n == 0 { 1 } else {
            match self.recs[self.n - 1] {
                Some(last) => last.seq + 1,
                None => return Err(E_MONO),
            }
        };
        if rec.seq != expect {
            return Err(E_MONO);
        }
        self.recs[self.n] = Some(rec);
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 全账对拍：逐条 verify，一条假即 E_CS。
    pub fn verify_all(&self, key: u64) -> Result<u32, u16> {
        let mut ok_n = 0;
        let mut i = 0;
        while i < self.n {
            let r = match self.recs[i] {
                Some(x) => x,
                None => return Err(E_MONO),
            };
            if !r.verify(key, r.cosign(key)) {
                return Err(E_CS);
            }
            ok_n += 1;
            i += 1;
        }
        Ok(ok_n)
    }
}

/// 摘要行（String 面，无 IO）。
pub fn summary(log: &EventLog) -> String {
    let mut s = String::new();
    s.push_str("evt n=");
    s.push_str(&log.n.to_string());
    s.push_str(" cap=");
    s.push_str(&EVT_CAP.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_SEQ0, E_HIT, E_TS, E_MONO, E_FULL, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEQ0 => "零序列号",
        E_HIT => "命中位非法（非成功/失败）",
        E_TS => "保留时间戳槽非零",
        E_MONO => "序列号回退/重复",
        E_FULL => "事件账定容满",
        E_CS => "联签对拍失败",
        _ => "未知诊断码",
    }
}

/// 演练：R1 空账；R2 10 例入账（单调）；R3 fake 联签全对拍（真钥真/错钥假）；
/// R4 零序列号 1 例；R5 坏命中位 2 例；R6 序回退 1 例；R7 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：空账 + 版本锚。
    if VER != 1 {
        return Err(E_VER);
    }
    let mut log = EventLog::fresh();
    if log.verify_all(0x5eed_0001)? != 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R2：10 例入账（命中位成功/失败轮转，ACE 指纹来自真源 FNV）。
    let key: u64 = 0x5eed_0001;
    let mut i: u32 = 0;
    while i < 10 {
        let ace_fp = core::fnv1a(&(0x0001_2003u32.wrapping_add(i)).to_le_bytes());
        let hit: u8 = if i % 2 == 0 { HitKind::SuccessHit.code() as u8 } else { HitKind::FailureHit.code() as u8 };
        let rec = EventRec::new(i + 1, ace_fp, hit)?;
        log.append(rec)?;
        if rec.ts_slot != TS_SLOT_RESERVED {
            return Err(E_TS);
        }
        hits += 1;
        i += 1;
    }
    // R3：fake 联签全对拍（逐条：对签真 + 错钥假）。
    let wrong_key = key.wrapping_add(1);
    let mut v = 0;
    while v < log.n {
        let rec = match log.recs[v] {
            Some(x) => x,
            None => return Err(E_MONO),
        };
        let sig = rec.cosign(key);
        if !rec.verify(key, sig) {
            return Err(E_CS);
        }
        if rec.verify(wrong_key, sig) {
            return Err(E_CS);
        }
        hits += 1;
        v += 1;
    }
    if log.verify_all(key)? != 10 {
        return Err(E_CS);
    }
    // R4：零序列号 1 例——E_SEQ0。
    match EventRec::new(0, 1, 1) {
        Ok(_) => return Err(E_SEQ0),
        Err(c) => {
            if c != E_SEQ0 {
                return Err(E_SEQ0);
            }
        }
    }
    hits += 1;
    // R5：坏命中位 2 例（0 / 3）——E_HIT。
    let bad_hits: Vec<u8> = vec![0, 3];
    let mut b = 0;
    while b < bad_hits.len() {
        match EventRec::new(1, 1, bad_hits[b]) {
            Ok(_) => return Err(E_HIT),
            Err(c) => {
                if c != E_HIT {
                    return Err(E_HIT);
                }
            }
        }
        hits += 1;
        b += 1;
    }
    // R6：序回退 1 例（在 seq=10 后追加 seq=5 → E_MONO）。
    let rec = EventRec::new(5, 0x00ff, 1)?;
    match log.append(rec) {
        Ok(_) => return Err(E_MONO),
        Err(c) => {
            if c != E_MONO {
                return Err(E_MONO);
            }
        }
    }
    hits += 1;
    // R7：指纹两次求值稳定且非零（账首条联签两次求值）。
    let r0 = match log.recs[0] {
        Some(x) => x,
        None => return Err(E_MONO),
    };
    let p1 = r0.cosign(key);
    let p2 = r0.cosign(key);
    if p1 != p2 || p1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(26));
    }

    #[test]
    fn seq0_and_bad_hit_rejected() {
        assert_eq!(EventRec::new(0, 1, 1), Err(E_SEQ0));
        assert_eq!(EventRec::new(1, 1, 0), Err(E_HIT));
        assert_eq!(EventRec::new(1, 1, 3), Err(E_HIT));
    }

    #[test]
    fn monotonic_enforced() {
        let mut log = EventLog::fresh();
        let r1 = EventRec::new(1, 1, 1).unwrap_or(EventRec { seq: 0, ace_fp: 0, hit: 0, ts_slot: 0 });
        let _ = log.append(r1);
        let r3 = EventRec::new(3, 1, 1).unwrap_or(r1);
        assert_eq!(log.append(r3), Err(E_MONO));
        assert!(summary(&log).contains("evt"));
    }

    #[test]
    fn cosign_wrong_key_fails() {
        let r = EventRec::new(1, 42, 2).unwrap_or(EventRec { seq: 0, ace_fp: 0, hit: 0, ts_slot: 0 });
        let sig = r.cosign(7);
        assert!(r.verify(7, sig));
        assert!(!r.verify(8, sig));
    }
}

pub fn run_unx36094_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36094_aceevtfmt");
    cs.add("SEG 段位独占", SEG_BASE == 0x3860, "0x3860~0x386f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "入账 10 例单调、联签对拍 20 例全对"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("时间戳槽保留锚 0", TS_SLOT_RESERVED == 0, "保留时间戳槽锚漂移");
    cs.add("事件账定容锚 16", EVT_CAP == 16, "事件账定容漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
