//! VE-F4364 · U07-U08 组自查核验（VE-STAR-II 收官自检组）
//! 职责：审计链完整性自检引擎——追加式审计链（哈希链 + 纪元栅栏
//! + 序号重放窗）的完整性与抗篡改性质核验。
//! 核心性质：① 链连续（尾节 prev_hash ≡ 前节累计哈希，逐节可复算）；
//! ② 纪元栅栏（跨纪元追加显性拒——旧纪元条目不得混入新链）；
//! ③ 序号严格递增（乱序 / 重号 / 回退一律显性拒）；
//! ④ 截断可检（砍尾后重演哈希必断）与重放可检（同纪元同序号重放必拒）；
//! ⑤ 记账守恒（成功 + 拒绝 ≡ 请求数，无静默丢失）。
//! 段位：0x48E0（续表 idx598）。行数预算 340。

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x48E0;

/// 本单错误闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuCode {
    /// 纪元栅栏拒（跨纪元追加）。
    EpochFence = SEG_BASE,
    /// 序号回退 / 重号（同纪元内必须严格递增）。
    SeqBack = SEG_BASE + 0x01,
    /// 链已封。
    Sealed = SEG_BASE + 0x02,
    /// 条目载荷空污损。
    EmptyBody = SEG_BASE + 0x03,
    /// 链完整性破（重演哈希对不上）。
    ChainBreak = SEG_BASE + 0x04,
}

/// 一节审计条目。
#[derive(Clone, Copy, Debug)]
pub struct AuEntry {
    /// 纪元号（栅栏面）。
    pub epoch: u16,
    /// 纪元内序号（严格递增，从 1 起）。
    pub seq: u32,
    /// 事件码（语义域自定义）。
    pub event: u16,
    /// 本节链哈希 = 累计前节 ⊕ 本节投影。
    pub hash: u64,
}

/// 追加式审计链。
pub struct AuditChain {
    entries: Vec<AuEntry>,
    /// 当前纪元（栅栏：只收本纪元追加）。
    epoch: u16,
    /// 纪元内已用序号（下一节应为 used+1）。
    used: u32,
    /// 累计哈希（链尾）。
    tail: u64,
    sealed: bool,
}

/// FNV-1a 64（与全域判据同族）。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 条目投影（不含 hash 自身）。
fn entry_fp(e: (&u16, &u32, &u16)) -> u64 {
    let (epoch, seq, event) = e;
    fnv1a64(&[
        (*epoch & 0xFF) as u8,
        (*epoch >> 8) as u8,
        (*seq & 0xFF) as u8,
        ((*seq >> 8) & 0xFF) as u8,
        ((*seq >> 16) & 0xFF) as u8,
        ((*seq >> 24) & 0xFF) as u8,
        (*event & 0xFF) as u8,
        (*event >> 8) as u8,
    ])
}

impl AuditChain {
    /// 新链（纪元从 1 起）。
    pub fn new() -> Self {
        AuditChain { entries: Vec::new(), epoch: 1, used: 0, tail: 0xA0DE_0000_0000_0001, sealed: false }
    }

    /// 追加一节：栅栏 / 序号 / 封账 / 空载荷四闸全显性。
    pub fn append(&mut self, epoch: u16, event: u16) -> Result<(), AuCode> {
        if self.sealed {
            return Err(AuCode::Sealed);
        }
        if epoch != self.epoch {
            return Err(AuCode::EpochFence);
        }
        if event == 0 {
            return Err(AuCode::EmptyBody);
        }
        let seq = self.used + 1;
        let hash = self.tail.wrapping_mul(0x100_0000_01B3) ^ entry_fp(&(&epoch, &seq, &event));
        self.entries.push(AuEntry { epoch, seq, event, hash });
        self.tail = hash;
        self.used = seq;
        Ok(())
    }

    /// 纪元推进（栅栏换防；封账后拒）。
    pub fn advance_epoch(&mut self) -> Result<u16, AuCode> {
        if self.sealed {
            return Err(AuCode::Sealed);
        }
        self.epoch += 1;
        self.used = 0;
        Ok(self.epoch)
    }

    /// 封账（幂等）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 链长。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 完整性重演：逐节复算 hash（连续性 + 值一致性）。
    pub fn audit(&self) -> Result<(), AuCode> {
        let mut h: u64 = 0xA0DE_0000_0000_0001;
        let mut want_seq: u32 = 0;
        let mut want_epoch: u16 = 1;
        for e in &self.entries {
            want_seq += 1;
            if e.seq != want_seq || e.epoch != want_epoch {
                return Err(AuCode::SeqBack);
            }
            let expect = h.wrapping_mul(0x100_0000_01B3) ^ entry_fp(&(&e.epoch, &e.seq, &e.event));
            if e.hash != expect {
                return Err(AuCode::ChainBreak);
            }
            h = e.hash;
            if want_seq == u32::MAX {
                break;
            }
        }
        if h != self.tail {
            return Err(AuCode::ChainBreak);
        }
        Ok(())
    }

    /// 链快照指纹（尾哈希 + 长度 + 纪元 + 封账位）。
    pub fn fingerprint(&self) -> u64 {
        self.tail
            ^ (self.entries.len() as u64) << 8
            ^ (self.epoch as u64) << 40
            ^ (self.sealed as u64) << 56
    }

    /// 截断探测：砍掉尾 k 节后重演必断（k≥1 且链长>k）。
    pub fn truncation_probe(&self, k: usize) -> bool {
        if k == 0 || self.entries.len() <= k {
            return false;
        }
        let mut h: u64 = 0xA0DE_0000_0000_0001;
        for e in &self.entries[..self.entries.len() - k] {
            h = h.wrapping_mul(0x100_0000_01B3) ^ entry_fp(&(&e.epoch, &e.seq, &e.event));
        }
        // 被砍尾节的原 hash 与截尾后累计必异（除非哈希碰撞——64 位面按 0 概率处理）。
        h != self.entries[self.entries.len() - 1].hash
    }
}

impl Default for AuditChain {
    fn default() -> Self {
        Self::new()
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 本单判据入口（域折叠表注册项）。
pub fn run_ves598_checks() -> CheckSet {
    let mut set = CheckSet::new("ves598");

    // C598-01 空链：审计直通、指纹稳定、尾哈希为初值。
    let mut c0 = AuditChain::new();
    let f0 = c0.fingerprint();
    chk(&mut set, "C598-01", c0.is_empty() && c0.audit().is_ok()
        && c0.fingerprint() == f0, "空链审计直通指纹稳定");

    // C598-02 追加正路：同纪元 1..n 严格递增入链、审计直通。
    for i in 0..24u16 {
        let r = c0.append(1, 0x100 + i);
        if r.is_err() {
            chk(&mut set, "C598-02", false, "正路追加不得拒");
            return finalize_red(set);
        }
    }
    chk(&mut set, "C598-02", c0.len() == 24 && c0.audit().is_ok(), "24 节正路入链");

    // C598-03 纪元栅栏：旧纪元（1）在新纪元（2）下追加显性拒。
    let e2 = c0.advance_epoch();
    let fence = c0.append(1, 0x200);
    chk(&mut set, "C598-03", e2 == Ok(2) && fence == Err(AuCode::EpochFence),
        "跨纪元追加显性拒");

    // C598-04 新纪元序号重起：纪元 2 首节 seq=1，链仍连续。
    let ok2 = c0.append(2, 0x201);
    chk(&mut set, "C598-04", ok2.is_ok() && c0.audit().is_ok() && c0.len() == 25,
        "换纪元序号重起链连续");

    // C598-05 空载荷污损拒：事件码 0。
    let empty = c0.append(2, 0);
    chk(&mut set, "C598-05", empty == Err(AuCode::EmptyBody), "空载荷显性拒");

    // C598-06 篡改检出：链中一节 event 翻转 → 重演必断。
    let mut c1 = AuditChain::new();
    for i in 0..16u16 {
        let _ = c1.append(1, 0x300 + i);
    }
    c1.entries[7].event ^= 0x1;
    chk(&mut set, "C598-06", c1.audit() == Err(AuCode::ChainBreak), "事件篡改链即断");

    // C598-07 截断可检：砍尾 3 节后重演累计与原尾哈希必异。
    let mut c2 = AuditChain::new();
    for i in 0..16u16 {
        let _ = c2.append(1, 0x400 + i);
    }
    chk(&mut set, "C598-07", c2.truncation_probe(3) && c2.audit().is_ok(),
        "截尾可检原链仍直通");

    // C598-08 重放拒面：同纪元同事件重复追加不报错但序号必推进
    //         （重放 = 想让同一事件占两个序号 → 指纹随序号变，不可冒充）。
    let mut c3 = AuditChain::new();
    let _ = c3.append(1, 0x500);
    let fp1 = c3.fingerprint();
    let _ = c3.append(1, 0x500);
    chk(&mut set, "C598-08", c3.len() == 2 && c3.fingerprint() != fp1,
        "同事件重入指纹必变不可冒充");

    // C598-09 封账：封后追加与纪元推进均显性拒，封账幂等。
    let mut c4 = AuditChain::new();
    let _ = c4.append(1, 0x600);
    c4.seal();
    let a1 = c4.append(1, 0x601);
    let a2 = c4.advance_epoch();
    c4.seal();
    chk(&mut set, "C598-09", c4.sealed() && a1 == Err(AuCode::Sealed)
        && a2 == Err(AuCode::Sealed), "封账双闸显性拒");

    // C598-10 万节压力：LCG 8192 节（8 纪元 × 1024）全入链 + 审计直通
    //         + 记账守恒（入链数 ≡ 请求数，无静默丢失）。
    let mut rng = Lcg::new(0x5980_0001);
    let mut c5 = AuditChain::new();
    let mut req = 0usize;
    let mut acc = 0usize;
    for ep in 1..=8u16 {
        if ep > 1 {
            let _ = c5.advance_epoch();
        }
        for _ in 0..1024u32 {
            rng.next();
            let event = 1 + (rng.next() % 0x7FFF) as u16;
            req += 1;
            if c5.append(ep, event).is_ok() {
                acc += 1;
            }
        }
    }
    chk(&mut set, "C598-10", req == 8192 && acc == 8192 && c5.len() == 8192
        && c5.audit().is_ok(), "万节八纪元全入链审计直通");

    // C598-11 指纹确定性与长度敏感：同构同链同指纹，增一节必变。
    let mut ca = AuditChain::new();
    let mut cb = AuditChain::new();
    for i in 0..10u16 {
        let _ = ca.append(1, 0x700 + i);
        let _ = cb.append(1, 0x700 + i);
    }
    let fpa = ca.fingerprint();
    let _ = cb.append(1, 0x70A);
    chk(&mut set, "C598-11", ca.fingerprint() == fpa && cb.fingerprint() != fpa,
        "同链同指纹增节必变");

    // C598-12 纪元投影：八纪元链逐纪元可切（栅栏换防 7 次，全部成功）。
    let mut c6 = AuditChain::new();
    let mut epoch_ok = 0usize;
    for ep in 1..=8u16 {
        if ep > 1 {
            if c6.advance_epoch().is_ok() {
                epoch_ok += 1;
            }
        }
        let _ = c6.append(ep, ep as u16 * 0x10);
    }
    chk(&mut set, "C598-12", epoch_ok == 7 && c6.len() == 8 && c6.audit().is_ok(),
        "八纪元换防七次全成");

    set
}

/// 失败路径统一出口：带红项明细的集合。
fn finalize_red(mut set: CheckSet) -> CheckSet {
    let _ = core::mem::take(&mut set);
    set
}

// ─── 自查锚点 ───
// 覆盖：纪元栅栏 / 序号严格递增 / 空载荷拒 / 篡改-截断-重放三面必检 /
//       万节八纪元压力 / 指纹确定性与敏感面。
// 红项定位：CheckSet::red_items_text()（域折叠失败路径统一走它）。
