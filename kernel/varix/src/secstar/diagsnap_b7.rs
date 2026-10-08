//! F174 诊断快照 · 批次七深化（v7）——快照版本协商、增量基线切换、
//! 敏感面二次扫描、快照传输分片账。零堆、no_std。

use crate::checks::CheckSet;

/// 支持的快照格式版本。
pub const SUPPORTED_VERSIONS: [u8; 3] = [1, 2, 3];
/// 协商帧长（6B）。
pub const NEGOTIATE_FRAME_LEN: usize = 6;
/// 增量基线槽容量。
pub const BASELINE_CAP: usize = 4;
/// 敏感模式表（二次扫描的匹配串）。
pub const SENSITIVE_PATTERNS: [&[u8]; 3] = [b"secret", b"token", b"password"];
/// 分片大小（字节）。
pub const FRAGMENT_SIZE: usize = 256;
/// 分片账容量。
pub const FRAG_ACCOUNT_CAP: usize = 16;

/// 版本协商：双方版本集交集取最高（兼容面最大公约）。
/// 无交集 → None（诚实拒绝不硬降级到未知格式）。
pub fn negotiate_version(theirs: &[u8]) -> Option<u8> {
    let mut best: Option<u8> = None;
    for &t in theirs {
        if SUPPORTED_VERSIONS.contains(&t) {
            best = Some(match best {
                Some(b) if b >= t => b,
                _ => t,
            });
        }
    }
    best
}

/// 协商帧（6B）：[0..2) "SN" · [2..3) 选定版本 · [3..4) 对端版本数 ·
/// [4..6) 校验和（前 4B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_negotiate(version: u8, peer_count: u8, out: &mut [u8; NEGOTIATE_FRAME_LEN]) -> bool {
    if !SUPPORTED_VERSIONS.contains(&version) {
        return false;
    }
    out[0] = b'S';
    out[1] = b'N';
    out[2] = version;
    out[3] = peer_count;
    let c = fnv16(&out[..4]);
    out[4] = (c & 0xFF) as u8;
    out[5] = (c >> 8) as u8;
    true
}

pub fn decode_negotiate(frame: &[u8; NEGOTIATE_FRAME_LEN]) -> Option<(u8, u8)> {
    if frame[0] != b'S' || frame[1] != b'N' {
        return None;
    }
    let want = (frame[5] as u16) << 8 | frame[4] as u16;
    if fnv16(&frame[..4]) != want {
        return None;
    }
    Some((frame[2], frame[3]))
}

/// 增量基线管理：快照 id → 基线指纹，新快照与基线差分（只传增量）。
/// 同 id 再登记 = 基线切换（旧基线作废——切换是显式动作不是意外）。
#[derive(Clone, Copy)]
pub struct BaselineStore {
    ids: [Option<u32>; BASELINE_CAP],
    fingerprints: [u32; BASELINE_CAP],
    pub switches: u32,
}

impl BaselineStore {
    pub const fn new() -> BaselineStore {
        BaselineStore { ids: [None; BASELINE_CAP], fingerprints: [0; BASELINE_CAP], switches: 0 }
    }

    fn fnv32(&self, id: u32, snap_fingerprint: u32) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in id.to_le_bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        h.wrapping_add(snap_fingerprint)
    }

    /// 登记基线：新 id 占槽（满驱逐最旧 = index 0）；旧 id → 切换。
    pub fn register(&mut self, id: u32, snap_fingerprint: u32) -> bool {
        let fp = self.fnv32(id, snap_fingerprint);
        for i in 0..BASELINE_CAP {
            if self.ids[i] == Some(id) {
                self.fingerprints[i] = fp;
                self.switches += 1;
                return true;
            }
        }
        let slot = match (0..BASELINE_CAP).find(|&i| self.ids[i].is_none()) {
            Some(i) => i,
            None => {
                for j in 1..BASELINE_CAP {
                    self.ids[j - 1] = self.ids[j];
                    self.fingerprints[j - 1] = self.fingerprints[j];
                }
                BASELINE_CAP - 1
            }
        };
        self.ids[slot] = Some(id);
        self.fingerprints[slot] = fp;
        true
    }

    /// 基线在位判定（差分的前提——没基线就必须全量）。
    pub fn has_baseline(&self, id: u32) -> bool {
        self.ids.iter().take(BASELINE_CAP).any(|&x| x == Some(id))
    }

    /// 基线指纹查询。
    pub fn fingerprint_of(&self, id: u32) -> Option<u32> {
        for i in 0..BASELINE_CAP {
            if self.ids[i] == Some(id) {
                return Some(self.fingerprints[i]);
            }
        }
        None
    }
}

/// 敏感面二次扫描：主脱敏漏网的敏感串计数（双保险——脱敏后必扫）。
/// 返回 (命中总数, 各模式命中 [3])。
pub fn rescan_sensitive(buf: &[u8]) -> (u32, [u32; 3]) {
    let mut total = 0u32;
    let mut per = [0u32; 3];
    for (k, pat) in SENSITIVE_PATTERNS.iter().enumerate() {
        let mut i = 0;
        while i + pat.len() <= buf.len() {
            if &buf[i..i + pat.len()] == *pat {
                per[k] += 1;
                total += 1;
                i += pat.len();
            } else {
                i += 1;
            }
        }
    }
    (total, per)
}

/// 分片账：快照按 FRAGMENT_SIZE 切片，账记每片到达状态（断点续传面）。
#[derive(Clone, Copy)]
pub struct FragmentAccount {
    arrived: [bool; FRAG_ACCOUNT_CAP],
    pub total_frags: usize,
    pub received: usize,
}

impl FragmentAccount {
    /// 建账：payload 字节数 → 片数（ceil）。
    pub fn new(payload_len: usize) -> FragmentAccount {
        let frags = payload_len.div_ceil(FRAGMENT_SIZE).min(FRAG_ACCOUNT_CAP);
        FragmentAccount { arrived: [false; FRAG_ACCOUNT_CAP], total_frags: frags, received: 0 }
    }

    /// 片到达（乱序到达——按位记账；重复片不重复计）。
    pub fn on_fragment(&mut self, idx: usize) -> bool {
        if idx >= self.total_frags || self.arrived[idx] {
            return false;
        }
        self.arrived[idx] = true;
        self.received += 1;
        true
    }

    /// 完整判定。
    pub fn complete(&self) -> bool {
        self.total_frags > 0 && self.received == self.total_frags
    }

    /// 缺口表：第一个未到片号（断点续传的续点）。
    pub fn first_gap(&self) -> Option<usize> {
        (0..self.total_frags).find(|&i| !self.arrived[i])
    }
}

#[inline(never)]
pub fn run_diagsnap_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F174-b7");

    // 1) 版本协商：对端 {1,3} → 取最高 3；{1,9} → 1；{} → None。
    cs.add(
        "negotiate_highest_common",
        negotiate_version(&[1, 3]) == Some(3) && negotiate_version(&[1, 9]) == Some(1) && negotiate_version(&[]).is_none() && negotiate_version(&[9]).is_none(),
        "",
    );

    // 2) 协商帧 round-trip：合法版本入帧、非法版本拒。
    let mut f = [0u8; NEGOTIATE_FRAME_LEN];
    let ok = encode_negotiate(3, 2, &mut f);
    let bad = !encode_negotiate(9, 2, &mut f);
    cs.add(
        "negotiate_frame_roundtrip",
        ok && decode_negotiate(&f) == Some((3, 2)) && bad && f[0] == b'S',
        "",
    );

    // 3) 协商帧撕裂拒：6 字节逐一翻转全拦。
    let mut tear_ok = true;
    for i in 0..NEGOTIATE_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x71;
        if decode_negotiate(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("negotiate_frame_tear_proof", tear_ok, "");

    // 4) 基线登记：三 id 占三槽、has_baseline 逐个可查。
    let mut b = BaselineStore::new();
    b.register(11, 0xAAAA);
    b.register(22, 0xBBBB);
    b.register(33, 0xCCCC);
    cs.add(
        "baseline_register",
        b.has_baseline(11) && b.has_baseline(22) && b.has_baseline(33) && !b.has_baseline(44),
        "",
    );

    // 5) 基线指纹确定性：同 id 同指纹重放一致（FNV 混入快照指纹——可复核）。
    let fp1 = b.fingerprint_of(22);
    let mut b2 = BaselineStore::new();
    b2.register(22, 0xBBBB);
    cs.add("baseline_fingerprint_stable", fp1.is_some() && b2.fingerprint_of(22) == fp1, "");

    // 6) 基线切换：同 id 再登记 → switches=1、指纹更新。
    b.register(22, 0xDDDD);
    cs.add(
        "baseline_switch_accounted",
        b.switches == 1 && b.fingerprint_of(22) != fp1,
        "",
    );

    // 7) 基线满容驱逐最旧：第 4 个入满、第 5 个入账 → 11 出账（最旧让位）。
    b.register(44, 0xEEEE);
    cs.add("baseline_full_no_evict", b.has_baseline(11) && b.has_baseline(44), "");
    b.register(55, 0x9999);
    cs.add("baseline_evicts_oldest", !b.has_baseline(11) && b.has_baseline(55), "");

    // 8) 二次扫描：三类敏感串各命中一次、总 3（双保险的量化）。
    let mut buf = [0u8; 64];
    buf[..6].copy_from_slice(b"secret");
    buf[10..15].copy_from_slice(b"token");
    buf[20..28].copy_from_slice(b"password");
    let (total, per) = rescan_sensitive(&buf);
    cs.add(
        "rescan_three_patterns",
        total == 3 && per[0] == 1 && per[1] == 1 && per[2] == 1,
        "",
    );

    // 9) 二次扫描零命中：干净缓冲 0（误报面必须为零——扫描不冤枉）。
    let clean = [b'a'; 64];
    let (t2, p2) = rescan_sensitive(&clean);
    cs.add("rescan_clean_zero", t2 == 0 && p2.iter().all(|&x| x == 0), "");

    // 10) 分片账：600B → 3 片（256×2+88）、乱序到达 2,0,1 → 完整。
    let mut fa = FragmentAccount::new(600);
    cs.add("frag_account_size", fa.total_frags == 3, "");
    assert!(fa.on_fragment(2));
    assert!(fa.on_fragment(0));
    cs.add("frag_gap_tracking", fa.first_gap() == Some(1) && !fa.complete(), "");
    assert!(fa.on_fragment(1));
    cs.add("frag_complete", fa.complete() && fa.first_gap().is_none(), "");

    // 11) 分片守门：越界片拒、重复片拒（账面不失真）。
    let mut fb = FragmentAccount::new(300); // 2 片
    let dup = fb.on_fragment(0) && !fb.on_fragment(0);
    let oob = !fb.on_fragment(2);
    cs.add("frag_guards", dup && oob && fb.received == 1, "");

    // 12) 常量自洽：版本 3 档、基线 4 槽、片 256B。
    cs.add(
        "b7_constants",
        SUPPORTED_VERSIONS.len() == 3 && BASELINE_CAP == 4 && FRAGMENT_SIZE == 256,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_prefers_mutual_max() {
        // 双方都支持的集合里取最大：本方 {1,2,3} × 对端 {2} → 2（不是单方最大 3）。
        assert_eq!(negotiate_version(&[2]), Some(2));
        assert_eq!(negotiate_version(&[3, 2, 1]), Some(3));
    }

    #[test]
    fn rescan_overlapping_patterns() {
        // 重叠敏感串：token 内含 token——逐词界推进不重复计（扫描口径锁定）。
        let buf = *b"token";
        let (total, per) = rescan_sensitive(&buf);
        assert_eq!(total, 1);
        assert_eq!(per[1], 1);
    }

    #[test]
    fn frag_account_exact_multiple() {
        // 恰好多片：512B → 恰 2 片（ceil 语义不虚增片数）。
        let fa = FragmentAccount::new(512);
        assert_eq!(fa.total_frags, 2);
        let fz = FragmentAccount::new(0);
        assert_eq!(fz.total_frags, 0);
        assert!(!fz.complete(), "零载荷无片可收——不宣告完整");
    }
}
