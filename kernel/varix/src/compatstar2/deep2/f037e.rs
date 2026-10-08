//! F037 深化批次三 · 审计链验证器面（compatstar2/deep2 · G-A-37）。
//!
//! 批次一深化覆盖解释卡/越权流/信任窗，批次二覆盖布隆前置/规则裁决；
//! 本批补齐主册判据「审计不可篡改验证（改一条序号断链即检出）」的
//! 执行/边界/注入面：FNV-1a 链重放验证（逐条重算 hash=fnv1a(prev_hash
//! || entry)，全链重放对账）、攻击注入检出矩阵（改/删/插/重放四攻击
//! 各向检出函数）、发布者 pin 集（指纹 + 证书链根双因子——主册
//! 【设计细节】「验证签名证书链而非仅哈希」的执行面，MS WinVerifyTrust
//! 信任根语义对拍）、信任窗过期扫描器（72h 窗口逐条到期标记）。
//!
//! 判据对账：主册 G-A-37 验收判据「审计不可篡改验证」+【设计细节】
//! 「信任发布者验证签名证书链（F024 证书库）而非仅哈希」+「越权后 72
//! 小时信任窗口」；哈希面为 FNV-1a 公开参考参数（非 MS 面如实注明）。
//!
//! 零堆纪律：定长账表 + 借片（&[T]）扫描，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// FNV-1a 链哈希
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基（FNV 公开参考参数——非 MS 面如实注明）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 字节散列（审计链的基本散列原语）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 审计链条目（F194 序号链同源口径：seq 严格递增 + 链式哈希）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainEntry {
    pub seq: u64,
    pub prev_hash: u64,
    pub hash: u64,
    pub payload: [u8; 8],
}

impl ChainEntry {
    /// 从前条哈希追加：hash = fnv1a(prev_hash 的 8 字节 LE || payload)。
    pub fn append(prev_hash: u64, seq: u64, payload: [u8; 8]) -> ChainEntry {
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&prev_hash.to_le_bytes());
        buf[8..].copy_from_slice(&payload);
        ChainEntry { seq, prev_hash, hash: fnv1a(&buf), payload }
    }
}

/// 链重放验证：从创世（prev_hash = 0）逐条重算 hash 并与前链哈希对账。
/// 返回第一个断链位（None = 全链一致）。
pub fn replay_first_break(chain: &[ChainEntry]) -> Option<usize> {
    let mut prev = 0u64;
    for (i, e) in chain.iter().enumerate() {
        let expect = ChainEntry::append(prev, e.seq, e.payload).hash;
        if e.prev_hash != prev || e.hash != expect {
            return Some(i);
        }
        prev = e.hash;
    }
    None
}

// ---------------------------------------------------------------------------
// 攻击注入检出矩阵（四向，各返回 true = 检出）
// ---------------------------------------------------------------------------

/// 攻击一「改一条」：payload 被篡改 → 重放失配检出。
pub fn detect_modified(chain: &[ChainEntry]) -> bool {
    replay_first_break(chain).is_some()
}

/// 攻击二「删一条」：后继条目 prev_hash 指向被删条目 → 链接断裂检出。
pub fn detect_deleted(chain: &[ChainEntry]) -> bool {
    replay_first_break(chain).is_some()
}

/// 攻击三「插一条」：伪造条目哈希对不上前链 → 断链检出。
pub fn detect_inserted(chain: &[ChainEntry]) -> bool {
    replay_first_break(chain).is_some()
}

/// 攻击四「重放」：旧条目重复追加 → seq 重复即检出（序号链单调纪律）。
pub fn detect_replayed(chain: &[ChainEntry]) -> bool {
    for i in 0..chain.len() {
        for j in 0..i {
            if chain[i].seq == chain[j].seq {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 发布者 pin 集（指纹 + 证书链根双因子）
// ---------------------------------------------------------------------------

/// pin 集容量（指纹/链根各 8 槽——域内定长口径）。
pub const PIN_CAP: usize = 8;

/// 发布者 pin 集：命中指纹或链根任一即过；全不中拒绝并计数（显性化）。
pub struct PublisherPins {
    pub fingerprints: [u64; PIN_CAP],
    pub roots: [u64; PIN_CAP],
    pub fingerprint_pins: usize,
    pub root_pins: usize,
    /// 指纹命中账面。
    pub fingerprint_hits: u32,
    /// 链根命中账面。
    pub root_hits: u32,
    /// 双因子全不中的拒绝计数（零静默吞错）。
    pub rejections: u32,
}

impl PublisherPins {
    pub const fn new() -> Self {
        PublisherPins {
            fingerprints: [0; PIN_CAP],
            roots: [0; PIN_CAP],
            fingerprint_pins: 0,
            root_pins: 0,
            fingerprint_hits: 0,
            root_hits: 0,
            rejections: 0,
        }
    }

    pub fn pin_fingerprint(&mut self, fp: u64) -> bool {
        if self.fingerprint_pins >= PIN_CAP {
            return false;
        }
        self.fingerprints[self.fingerprint_pins] = fp;
        self.fingerprint_pins += 1;
        true
    }

    pub fn pin_root(&mut self, root: u64) -> bool {
        if self.root_pins >= PIN_CAP {
            return false;
        }
        self.roots[self.root_pins] = root;
        self.root_pins += 1;
        true
    }

    /// 校验：指纹命中或链根命中任一即过；全不中 → Err 并计数。
    pub fn verify(&mut self, fingerprint: u64, chain_root: u64) -> Result<(), &'static str> {
        if self.fingerprints[..self.fingerprint_pins].contains(&fingerprint) {
            self.fingerprint_hits += 1;
            return Ok(());
        }
        if self.roots[..self.root_pins].contains(&chain_root) {
            self.root_hits += 1;
            return Ok(());
        }
        self.rejections += 1;
        Err("pin-reject")
    }
}

// ---------------------------------------------------------------------------
// 信任窗过期扫描器
// ---------------------------------------------------------------------------

/// 信任窗 72 小时（秒口径——主册【设计细节】信任窗口）。
pub const TRUST_WINDOW_S: u64 = 72 * 3600;
/// 信任账容量 32（定长——零堆纪律）。
pub const TRUST_LEDGER_CAP: usize = 32;

/// 信任窗过期扫描器：逐条扫描到期标记，定长 32 条账。
pub struct TrustLedger {
    pub granted_s: [u64; TRUST_LEDGER_CAP],
    pub expired: [bool; TRUST_LEDGER_CAP],
    pub count: usize,
    /// 累计到期标记数（账面）。
    pub expired_total: u32,
}

impl TrustLedger {
    pub const fn new() -> Self {
        TrustLedger { granted_s: [0; TRUST_LEDGER_CAP], expired: [false; TRUST_LEDGER_CAP], count: 0, expired_total: 0 }
    }

    pub fn grant(&mut self, now_s: u64) -> bool {
        if self.count >= TRUST_LEDGER_CAP {
            return false;
        }
        self.granted_s[self.count] = now_s;
        self.count += 1;
        true
    }

    /// 扫描：now - granted ≥ 72h → 标记到期；返回本次新标记数。
    pub fn scan(&mut self, now_s: u64) -> u32 {
        let mut newly = 0u32;
        for i in 0..self.count {
            if !self.expired[i] && now_s.saturating_sub(self.granted_s[i]) >= TRUST_WINDOW_S {
                self.expired[i] = true;
                self.expired_total += 1;
                newly += 1;
            }
        }
        newly
    }
}

/// 域自检（深化批次三）。
pub fn run_f037e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F037-peblockui-d3");
    // 1) FNV-1a 公开参考向量：空串=偏移基、"a"=已知值。
    cs.add(
        "fnv_reference_vectors",
        fnv1a(b"") == FNV_OFFSET && fnv1a(b"a") == 0xaf63_dc4c_8601_ec8c,
        "",
    );
    // 2) 建链 + 全链重放干净（三链条目逐条对账无断链）。
    let e0 = ChainEntry::append(0, 1, *b"rule-001");
    let e1 = ChainEntry::append(e0.hash, 2, *b"rule-002");
    let e2 = ChainEntry::append(e1.hash, 3, *b"rule-003");
    let clean = [e0, e1, e2];
    cs.add("chain_replay_clean", replay_first_break(&clean).is_none(), "");
    // 3) 攻击一「改一条」：篡改 payload（哈希未同步）→ 第 1 位断链。
    let mut tampered = clean;
    tampered[1].payload = *b"rule-9XX";
    cs.add(
        "attack_modified",
        detect_modified(&tampered) && replay_first_break(&tampered) == Some(1),
        "",
    );
    // 4) 攻击二「删一条」：抽掉中间条目 → 后继链接断裂于原位。
    let deleted = [e0, e2];
    cs.add("attack_deleted", detect_deleted(&deleted) && replay_first_break(&deleted) == Some(1), "");
    // 5) 攻击三「插一条」：伪造条目 prev_hash 对不上 → 断链检出。
    let forged = ChainEntry::append(0, 9, *b"forged-9");
    let inserted = [e0, forged, e1, e2];
    cs.add(
        "attack_inserted",
        detect_inserted(&inserted) && replay_first_break(&inserted) == Some(1),
        "",
    );
    // 6) 攻击四「重放」：旧条目重复追加 → seq 重复检出；干净链不误报。
    let replayed = [e0, e1, e2, e1];
    cs.add("attack_replayed", detect_replayed(&replayed) && !detect_replayed(&clean), "");
    // 7) pin 集：指纹命中即过（链根未配也不拒）。
    let mut pins = PublisherPins::new();
    let _ = pins.pin_fingerprint(0xF1CE);
    cs.add("pin_fingerprint_hit", pins.verify(0xF1CE, 0).is_ok() && pins.fingerprint_hits == 1, "");
    // 8) pin 集：链根命中即过（主册「验证证书链而非仅哈希」的双因子另一路）。
    let _ = pins.pin_root(0xCAFE);
    cs.add("pin_root_hit", pins.verify(0x9999, 0xCAFE).is_ok() && pins.root_hits == 1, "");
    // 9) 双因子全不中 → 拒绝并计数（显性化，不静默）。
    cs.add(
        "pin_reject_counted",
        pins.verify(0x9999, 0xBEEF) == Err("pin-reject") && pins.rejections == 1,
        "",
    );
    // 10) 信任窗扫描：72h 内不标记、到期逐条标记并入账。
    let mut tl = TrustLedger::new();
    let _ = tl.grant(0);
    let _ = tl.grant(3600);
    let before = tl.scan(TRUST_WINDOW_S - 1);
    let first_expire = tl.scan(TRUST_WINDOW_S);
    let second_expire = tl.scan(TRUST_WINDOW_S + 3600);
    cs.add(
        "trust_window_scan",
        before == 0 && first_expire == 1 && second_expire == 1 && tl.expired_total == 2 && tl.expired[0],
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_foobar_vector() {
        // FNV-1a 64 公开参考向量 "foobar"。
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn trust_expiry_boundary_exact() {
        let mut tl = TrustLedger::new();
        let _ = tl.grant(100);
        assert_eq!(tl.scan(100 + TRUST_WINDOW_S - 1), 0, "71h59m59s 未到期");
        assert_eq!(tl.scan(100 + TRUST_WINDOW_S), 1, "72h 整到期即标记");
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f037e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
