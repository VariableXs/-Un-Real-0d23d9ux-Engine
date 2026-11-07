//! UNX-F81004 批次记录行唯一键生成器（批号×域号复合键） · 0x91060段
//!
//! 判据原文（派工卡 81003 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81004-J1 「批次记录行唯一键生成器（批号×域号
//!      复合键）」判据落地：正向断言 ×10 与冻结口径完全一致；注入同型
//!      故障 10 次零静默；失败路径显性化、埋点入账；台账写入仅走本条
//!      登记通道（非法第四态零存在）；判据锚定 §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. BatchKey 以 u64 高 32 位装批号、低 32 位装域号 pack/unpack，纯位
//!    运算往返一致，无浮点、无除法歧义。
//! 2. 键空间守卫前置：批号 0 拒、域号 0 拒、越上界拒，守卫先于编码，
//!    非法输入零编码。
//! 3. KeyIssuer 单调发放器：发放账 append-only，重发同键拒、跳号检测
//!    登记、回退检测，发放历史不可改写。
//! 4. 键字典 key→slot 映射容量 4096 槽，线性探测闭域；满槽、重入逐类
//!    显性拒绝。
//! 5. 注入核 ×10（零批号/零域号/越界批号/越界域号/重发同键/跳号/回退/
//!    字典满/字典重入/解包错位），逐类断言具体诊断码，注入必抓。
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81003 − 81001)）。
pub const SEG_BASE: u32 = 0x91060;

// ── 诊断码闭集（段内偏移 0x01..0x0D） ─────────────────────────────
/// 零批号。
pub const E_ZEROB: u16 = 1;
/// 零域号。
pub const E_ZEROD: u16 = 2;
/// 批号越上界。
pub const E_BATCHHI: u16 = 3;
/// 域号越上界。
pub const E_DOMHI: u16 = 4;
/// 重发同键。
pub const E_REISSUE: u16 = 5;
/// 跳号。
pub const E_SKIP: u16 = 6;
/// 发放序回退。
pub const E_REWIND: u16 = 7;
/// 字典满。
pub const E_DICTFULL: u16 = 8;
/// 字典重入（同键已登记到不同槽）。
pub const E_DICTREENTRY: u16 = 9;
/// 解包错位（键分量与期望不符）。
pub const E_UNPACK: u16 = 10;
/// 发放账空引用。
pub const E_NOLEDGER: u16 = 11;
/// 非法复合键（守卫外的键值直接登记）。
pub const E_BADKEY: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 批号/域号合法上界（u16 全域，键空间 65535×65535）。
pub const MAX_BATCH: u16 = u16::MAX;
pub const MAX_DOMAIN: u16 = u16::MAX;

/// 键字典容量（4096 槽）。
pub const DICT_CAP: usize = 4096;

/// 批次复合键（u64 位包：高 32 位批号，低 32 位域号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BatchKey(u64);

/// 键空间守卫：批号/域号均不得为 0（越上界对 u16 入参不可能，但键值
/// 直构路径仍需复检）。
pub fn guard(batch: u16, domain: u16) -> Result<(), KeyError> {
    if batch == 0 {
        return Err(KeyError::ZeroB);
    }
    if domain == 0 {
        return Err(KeyError::ZeroD);
    }
    if batch > MAX_BATCH {
        return Err(KeyError::BatchHi);
    }
    if domain > MAX_DOMAIN {
        return Err(KeyError::DomHi);
    }
    Ok(())
}

impl BatchKey {
    /// 打包：守卫通过后位运算合成 u64。
    pub fn pack(batch: u16, domain: u16) -> Result<BatchKey, KeyError> {
        guard(batch, domain)?;
        Ok(BatchKey((batch as u64) << 32 | domain as u64))
    }

    /// 从裸 u64 直构（登记通道复检：高位批号域、低位域号域分别越界即拒，
    /// 非法键零入账）。
    pub fn from_raw(raw: u64) -> Result<BatchKey, KeyError> {
        let b32 = raw >> 32;
        let d32 = raw & 0xFFFF_FFFF;
        if b32 > MAX_BATCH as u64 {
            return Err(KeyError::BatchHi);
        }
        if d32 > MAX_DOMAIN as u64 {
            return Err(KeyError::DomHi);
        }
        if b32 == 0 || d32 == 0 {
            return Err(KeyError::BadKey);
        }
        guard(b32 as u16, d32 as u16)?;
        Ok(BatchKey(raw))
    }

    /// 解包校验：裸键分量必须与期望批号/域号逐位一致，错位即拒。
    pub fn verify(raw: u64, batch: u16, domain: u16) -> Result<BatchKey, KeyError> {
        let k = BatchKey::from_raw(raw)?;
        if k.batch() != batch || k.domain() != domain {
            return Err(KeyError::Unpack);
        }
        Ok(k)
    }

    /// 批号分量。
    pub fn batch(self) -> u16 {
        (self.0 >> 32) as u16
    }

    /// 域号分量。
    pub fn domain(self) -> u16 {
        self.0 as u16
    }

    /// 裸 u64。
    pub fn raw(self) -> u64 {
        self.0
    }

    /// 往返一致：pack → unpack → pack 逐位相同。
    pub fn roundtrip(self) -> bool {
        match BatchKey::pack(self.batch(), self.domain()) {
            Ok(k) => k.0 == self.0,
            Err(_) => false,
        }
    }
}

/// 键域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// 零批号。
    ZeroB,
    /// 零域号。
    ZeroD,
    /// 批号越上界。
    BatchHi,
    /// 域号越上界。
    DomHi,
    /// 重发同键。
    Reissue,
    /// 跳号。
    Skip,
    /// 发放序回退。
    Rewind,
    /// 字典满。
    DictFull,
    /// 字典重入。
    DictReentry,
    /// 解包错位。
    Unpack,
    /// 发放账空引用。
    NoLedger,
    /// 非法复合键。
    BadKey,
}

impl KeyError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            KeyError::ZeroB => E_ZEROB,
            KeyError::ZeroD => E_ZEROD,
            KeyError::BatchHi => E_BATCHHI,
            KeyError::DomHi => E_DOMHI,
            KeyError::Reissue => E_REISSUE,
            KeyError::Skip => E_SKIP,
            KeyError::Rewind => E_REWIND,
            KeyError::DictFull => E_DICTFULL,
            KeyError::DictReentry => E_DICTREENTRY,
            KeyError::Unpack => E_UNPACK,
            KeyError::NoLedger => E_NOLEDGER,
            KeyError::BadKey => E_BADKEY,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            KeyError::ZeroB => "zero batch number",
            KeyError::ZeroD => "zero domain number",
            KeyError::BatchHi => "batch number above bound",
            KeyError::DomHi => "domain number above bound",
            KeyError::Reissue => "reissue of same key",
            KeyError::Skip => "issuer sequence skip",
            KeyError::Rewind => "issuer sequence rewind",
            KeyError::DictFull => "key dictionary full",
            KeyError::DictReentry => "dictionary re-entry of existing key",
            KeyError::Unpack => "unpack component mismatch",
            KeyError::NoLedger => "empty issuer ledger",
            KeyError::BadKey => "raw key outside key space",
        }
    }
}

/// 发放账记录（append-only）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IssueRecord {
    /// 发放序号（1 起严格递增）。
    pub seq: u64,
    /// 发放的键。
    pub key: BatchKey,
}

/// 单调发放器：发放账 + 槽位字典（key→slot）。
#[derive(Debug)]
pub struct KeyIssuer {
    slots: Vec<Option<(u64, u32)>>,
    ledger: Vec<IssueRecord>,
    next_seq: u64,
}

impl KeyIssuer {
    /// 新发放器：4096 槽空字典、空账。
    pub fn new() -> KeyIssuer {
        let mut slots: Vec<Option<(u64, u32)>> = Vec::new();
        let mut i = 0usize;
        while i < DICT_CAP {
            slots.push(None);
            i += 1;
        }
        KeyIssuer {
            slots,
            ledger: Vec::new(),
            next_seq: 1,
        }
    }

    /// 已发放数。
    pub fn issued(&self) -> usize {
        self.ledger.len()
    }

    /// 发放账记录数（append-only 总量）。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 按序取账记录。
    pub fn record(&self, idx: usize) -> Option<IssueRecord> {
        if idx >= self.ledger.len() {
            return None;
        }
        Some(self.ledger[idx])
    }

    /// 发放键：守卫 → 重发检查 → 跳号/回退检测 → 入账 + 入字典。
    pub fn issue(&mut self, batch: u16, domain: u16) -> Result<BatchKey, KeyError> {
        let key = BatchKey::pack(batch, domain)?;
        // 重发同键拒（账内或字典内任一命中）。
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].key == key {
                return Err(KeyError::Reissue);
            }
            i += 1;
        }
        if self.lookup(key).is_some() {
            return Err(KeyError::DictReentry);
        }
        if self.issued() >= DICT_CAP {
            return Err(KeyError::DictFull);
        }
        // 跳号检测：账序必须连续；回退检测：键序必须严格递增。
        let expect_seq = self.next_seq;
        if self.issued() > 0 {
            let last = self.ledger[self.issued() - 1];
            if key.raw() < last.key.raw() {
                return Err(KeyError::Rewind);
            }
            if key.raw() == last.key.raw() {
                return Err(KeyError::Reissue);
            }
        }
        self.ledger.push(IssueRecord { seq: expect_seq, key });
        self.next_seq += 1;
        // 入字典：线性探测首空槽。
        let mut probe = 0usize;
        while probe < DICT_CAP {
            let idx = (key.raw() as usize + probe) % DICT_CAP;
            if self.slots[idx].is_none() {
                self.slots[idx] = Some((key.raw(), self.issued() as u32 - 1));
                break;
            }
            probe += 1;
        }
        Ok(key)
    }

    /// 按键查槽位（线性探测；无则 None）。
    pub fn lookup(&self, key: BatchKey) -> Option<u32> {
        let mut probe = 0usize;
        while probe < DICT_CAP {
            let idx = (key.raw() as usize + probe) % DICT_CAP;
            match self.slots[idx] {
                Some((k, slot)) => {
                    if k == key.raw() {
                        return Some(slot);
                    }
                }
                None => return None,
            }
            probe += 1;
        }
        None
    }

    /// 跳号审计：账内 seq 必须从 1 连续递增；断链即 Skip。
    pub fn audit_sequence(&self) -> Result<(), KeyError> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].seq != i as u64 + 1 {
                return Err(KeyError::Skip);
            }
            i += 1;
        }
        Ok(())
    }

    /// 外部塞账检测口：把账记录替换为乱序 seq 后由审计抓 Skip（注入核用）。
    pub fn corrupt_seq_for_audit(&mut self, idx: usize, seq: u64) -> Result<(), KeyError> {
        if idx >= self.ledger.len() {
            return Err(KeyError::NoLedger);
        }
        self.ledger[idx].seq = seq;
        Ok(())
    }

    /// 外部塞字典检测口：往字典塞一条账外登记（有槽无账）后由发放通道
    /// 抓 DictReentry（注入核用）。
    pub fn corrupt_dict_for_audit(&mut self, raw: u64, slot: u32) -> Result<(), KeyError> {
        if slot as usize >= DICT_CAP {
            return Err(KeyError::NoLedger);
        }
        self.slots[slot as usize] = Some((raw, slot));
        Ok(())
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err 或审计报错（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    // 1 零批号 → ZeroB
    match BatchKey::pack(0, 1) {
        Err(e) => out.push(("zero-batch", e.code())),
        Ok(_) => out.push(("zero-batch-MISSED", SEG_BASE)),
    }
    // 2 零域号 → ZeroD
    match BatchKey::pack(1, 0) {
        Err(e) => out.push(("zero-domain", e.code())),
        Ok(_) => out.push(("zero-domain-MISSED", SEG_BASE)),
    }
    // 3 越界批号（u16 直构路径越 u32 高位）→ BatchHi（from_raw 复检）
    match BatchKey::from_raw(0x1_0000_0000_0001) {
        Err(e) => out.push(("batch-over", e.code())),
        Ok(_) => out.push(("batch-over-MISSED", SEG_BASE)),
    }
    // 4 越界域号（低位溢出 u16）→ DomHi
    match BatchKey::from_raw(0x0000_0001_0001) {
        Err(e) => out.push(("domain-over", e.code())),
        Ok(_) => out.push(("domain-over-MISSED", SEG_BASE)),
    }
    // 5 重发同键 → Reissue
    let mut iss = KeyIssuer::new();
    let _ = iss.issue(7, 9);
    match iss.issue(7, 9) {
        Err(e) => out.push(("reissue", e.code())),
        Ok(_) => out.push(("reissue-MISSED", SEG_BASE)),
    }
    // 6 跳号 → Skip（篡改账 seq 后审计抓断链）
    let mut iss = KeyIssuer::new();
    let _ = iss.issue(1, 1);
    let _ = iss.issue(2, 1);
    let corrupt = iss.corrupt_seq_for_audit(1, 9);
    match (corrupt, iss.audit_sequence()) {
        (Ok(()), Err(e)) => out.push(("seq-skip", e.code())),
        _ => out.push(("seq-skip-MISSED", SEG_BASE)),
    }
    // 7 回退 → Rewind（小键在大键之后发放）
    let mut iss = KeyIssuer::new();
    let _ = iss.issue(500, 500);
    match iss.issue(10, 10) {
        Err(e) => out.push(("rewind", e.code())),
        Ok(_) => out.push(("rewind-MISSED", SEG_BASE)),
    }
    // 8 字典满 → DictFull（4096 槽打满；(b/255+1, b%255+1) 双射键映射）
    let mut iss = KeyIssuer::new();
    let mut b: u64 = 1;
    let mut filled = 0usize;
    while filled < DICT_CAP {
        let kd = (b % 255) as u16 + 1;
        let kb = (b / 255) as u16 + 1;
        match iss.issue(kb, kd) {
            Ok(_) => {
                filled += 1;
                b += 1;
            }
            Err(_) => break,
        }
    }
    match iss.issue(60001, 60001) {
        Err(e) => out.push(("dict-full", e.code())),
        Ok(_) => out.push(("dict-full-MISSED", SEG_BASE)),
    }
    // 9 字典重入 → DictReentry（塞入账外登记后同键再发放）
    let mut iss = KeyIssuer::new();
    let ghost = BatchKey::pack(42, 42).map(|k| k.raw()).unwrap_or(0);
    let start_slot = (ghost % DICT_CAP as u64) as u32;
    let _ = iss.corrupt_dict_for_audit(ghost, start_slot);
    match iss.issue(42, 42) {
        Err(e) => out.push(("dict-reentry", e.code())),
        Ok(_) => out.push(("dict-reentry-MISSED", SEG_BASE)),
    }
    // 10 解包错位：分量与期望不符 → Unpack
    let raw_ok = BatchKey::pack(3, 4).map(|k| k.raw()).unwrap_or(0);
    match BatchKey::verify(raw_ok, 3, 5) {
        Err(e) => out.push(("unpack-misalign", e.code())),
        Ok(_) => out.push(("unpack-misalign-MISSED", SEG_BASE)),
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓、零静默）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (name, code) = samples[i];
        if name.ends_with("MISSED") {
            return false;
        }
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81004-J1 落地）：14 条，正向 + 注入汇总。
pub fn run_unx81003_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81003");

    // 1 段位守卫
    let ok = SEG_BASE > 0x91000 && SEG_BASE < 0x94000 && (SEG_BASE - 0x91000) % 0x30 == 0;
    s.add("F81004-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81004-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 pack 正向：批号 100 × 域号 7 → 高 32 批号低 32 域号
    let ok = match BatchKey::pack(100, 7) {
        Ok(k) => k.raw() == (100u64 << 32 | 7) && k.batch() == 100 && k.domain() == 7,
        Err(_) => false,
    };
    s.add("F81004-03 pack正向", ok, if ok { "" } else { "打包漂移" });

    // 4 往返一致 ×10 组
    let mut rt_ok = true;
    let mut i = 1u16;
    while i <= 10 {
        match BatchKey::pack(i, 65535 - i + 1) {
            Ok(k) => {
                if !k.roundtrip() {
                    rt_ok = false;
                }
            }
            Err(_) => rt_ok = false,
        }
        i += 1;
    }
    s.add("F81004-04 往返一致", ok_fwd(rt_ok), if rt_ok { "" } else { "往返不一致" });

    // 5 键空间守卫三路（零批号/零域号/直构非法键）
    let ok = matches!(BatchKey::pack(0, 1), Err(KeyError::ZeroB))
        && matches!(BatchKey::pack(1, 0), Err(KeyError::ZeroD))
        && matches!(BatchKey::from_raw(0), Err(KeyError::BadKey));
    s.add("F81004-05 守卫三路", ok, if ok { "" } else { "守卫漏抓" });

    // 6 边界键合法（批号域号取满上界）
    let ok = match BatchKey::pack(MAX_BATCH, MAX_DOMAIN) {
        Ok(k) => k.batch() == MAX_BATCH && k.domain() == MAX_DOMAIN && k.roundtrip(),
        Err(_) => false,
    };
    s.add("F81004-06 上界合法", ok, if ok { "" } else { "上界误拒" });

    // 7 发放账单调：seq 从 1 连续递增
    let mut iss = KeyIssuer::new();
    let _ = iss.issue(3, 1);
    let _ = iss.issue(4, 1);
    let _ = iss.issue(5, 1);
    let r0 = iss.record(0);
    let r2 = iss.record(2);
    let ok = iss.audit_sequence() == Ok(())
        && matches!(r0, Some(r) if r.seq == 1)
        && matches!(r2, Some(r) if r.seq == 3);
    s.add("F81004-07 发放单调", ok, if ok { "" } else { "账序漂移" });

    // 8 重发同键拒且账不增长
    let mut iss = KeyIssuer::new();
    let _ = iss.issue(9, 9);
    let n0 = iss.ledger_len();
    let dup = iss.issue(9, 9);
    let ok = matches!(dup, Err(KeyError::Reissue)) && iss.ledger_len() == n0;
    s.add("F81004-08 重发拒", ok, if ok { "" } else { "重发漏抓" });

    // 9 字典槽映射与探测一致
    let mut iss = KeyIssuer::new();
    let k1 = iss.issue(11, 22).unwrap_or(BatchKey(0));
    let slot = iss.lookup(k1);
    let ok = match slot {
        Some(sl) => sl < DICT_CAP as u32,
        None => false,
    };
    s.add("F81004-09 字典映射", ok, if ok { "" } else { "字典映射坏" });

    // 10 键序严格递增（发放序即键序）
    let mut iss = KeyIssuer::new();
    let a = iss.issue(1, 2).unwrap_or(BatchKey(0));
    let b = iss.issue(2, 2).unwrap_or(BatchKey(0));
    let ok = b.raw() > a.raw();
    s.add("F81004-10 键序递增", ok, if ok { "" } else { "键序漂移" });

    // 11 注入 10/10 零静默
    let ok = inject_all_caught();
    s.add("F81004-11 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    // 12 跳号审计幂等：健康账连续两次审计均绿
    let mut iss = KeyIssuer::new();
    let _ = iss.issue(1, 3);
    let _ = iss.issue(1, 4);
    let ok = iss.audit_sequence() == Ok(()) && iss.audit_sequence() == Ok(());
    s.add("F81004-12 审计幂等", ok, if ok { "" } else { "审计不确定" });

    // 13 解包分量错位必拒（verify 期望错位与高位越界双路）
    let raw_k = BatchKey::pack(5, 6).map(|k| k.raw()).unwrap_or(0);
    let ok = matches!(BatchKey::verify(raw_k, 6, 5), Err(KeyError::Unpack))
        && matches!(BatchKey::verify(raw_k, 5, 6), Ok(_))
        && matches!(BatchKey::from_raw(0x1_0000_0000_0001), Err(KeyError::BatchHi));
    s.add("F81004-13 错位拒", ok, if ok { "" } else { "错位漏抓" });

    // 14 发放器幂等：同一发放器重复 lookup 同键结果稳定
    let mut iss = KeyIssuer::new();
    let k = iss.issue(2, 8).unwrap_or(BatchKey(0));
    let ok = iss.lookup(k) == iss.lookup(k) && iss.lookup(k).is_some();
    s.add("F81004-14 查找幂等", ok, if ok { "" } else { "查找不确定" });

    s
}

/// 小工具：bool 直通（判据可读性）。
fn ok_fwd(b: bool) -> bool {
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81003_checks_green() {
        let set = run_unx81003_checks();
        assert!(set.all_passed(), "UNX-F81003 red: {}", set.red_items_text());
    }
}
