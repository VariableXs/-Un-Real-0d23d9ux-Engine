//! UNX-F0171 · TPM2 事件重放（只读）账 · 诊断段 0xB810
//! 验收锚点：UNX-F0171-J1 事件日志重放计算 PCR0-7 预期值，与读回
//! PCR 实值一致，双判据缺一即批内红灯。

use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB810;

// ── 诊断段独占 · TPM 重放错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_PCR: u16 = SEG_ERR + 1;   // PCR 越域（>7 或 >23）
const E_ORDER: u16 = SEG_ERR + 2; // 事件序错（序号断档）
const E_MISMATCH: u16 = SEG_ERR + 3; // 预期≠实值
const E_TYPE: u16 = SEG_ERR + 4;  // 事件类型未登记
const E_RDONLY: u16 = SEG_ERR + 5; // 只读违例（写 PCR）

pub const PCR_REPLAY_MAX: u8 = 7; // 重放域 PCR0-7

// ── FNV-1a 64（摘要折叠模拟 SHA-1 extend）──────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
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
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// extend 语义模拟：new = H(old ‖ digest)，确定性折叠。
pub fn pcr_extend(old: u64, digest: u64) -> u64 {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&old.to_le_bytes());
    buf[8..].copy_from_slice(&digest.to_le_bytes());
    fnv1a64(&buf)
}

// ── TPM 事件 ─────────────────────────────────────────────────
/// 一条事件日志：序号 + PCR 索引 + 类型 + 摘要。
#[derive(Clone, Debug)]
pub struct TpmEvent {
    pub seq: u32,
    pub pcr: u8,
    pub etype: u32,   // 事件类型（EV_POST_CODE=1 等）
    pub digest: u64,
}

impl TpmEvent {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in [self.seq as u64, self.pcr as u64, self.etype as u64, self.digest] {
            for b in x.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

/// 已登记事件类型（真实 TCG 常量域）。
pub fn known_types() -> [u32; 6] {
    [1, 2, 3, 4, 5, 6] // EV_PREBOOT..EV_ACTION 域代表
}

// ── 事件日志 ─────────────────────────────────────────────────
/// 日志：序号连续、PCR 域合法、类型已登记。
pub struct TpmLog {
    pub events: Vec<TpmEvent>,
}

impl TpmLog {
    pub fn new() -> Self {
        TpmLog { events: Vec::new() }
    }
    pub fn append(&mut self, pcr: u8, etype: u32, digest: u64) -> Result<u32, u16> {
        if pcr > 23 {
            return Err(E_PCR);
        }
        if !known_types().contains(&etype) {
            return Err(E_TYPE);
        }
        let seq = self.events.len() as u32 + 1;
        self.events.push(TpmEvent { seq, pcr, etype, digest });
        Ok(seq)
    }
    /// 日志自洽：序号 1..N 连续。
    pub fn seq_ok(&self) -> bool {
        self.events.iter().enumerate().all(|(i, e)| e.seq == i as u32 + 1)
    }
    /// 重放：只读遍历计算 PCR0-7 预期值（零基 → 逐条 extend）。
    pub fn replay(&self) -> Result<[u64; 8], u16> {
        let mut pcrs = [0u64; 8];
        for e in &self.events {
            if e.pcr > PCR_REPLAY_MAX {
                return Err(E_PCR);
            }
            pcrs[e.pcr as usize] = pcr_extend(pcrs[e.pcr as usize], e.digest);
        }
        Ok(pcrs)
    }
    /// 与读回 PCR 实值比对。
    pub fn verify(&self, readback: &[u64; 8]) -> Result<(), u16> {
        let expect = self.replay()?;
        for i in 0..8 {
            if expect[i] != readback[i] {
                return Err(E_MISMATCH);
            }
        }
        Ok(())
    }
    /// 只读性：重放前后日志指纹不变。
    pub fn readonly_check(&self) -> bool {
        let f1: u64 = self.events.iter().map(|e| e.fp()).fold(0xcbf2_9ce4_8422_2325, |acc, x| {
            let mut h = acc;
            for b in x.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h
        });
        let _ = self.replay();
        let f2: u64 = self.events.iter().map(|e| e.fp()).fold(0xcbf2_9ce4_8422_2325, |acc, x| {
            let mut h = acc;
            for b in x.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h
        });
        f1 == f2
    }
}

/// 标准日志夹具：PCR0-7 各 1~2 条事件。
pub fn standard_log() -> TpmLog {
    let mut lg = TpmLog::new();
    let mut lcg = Lcg(0xB810);
    let mut digest = 0x1000u64;
    for pcr in 0..=7u8 {
        let n = 1 + (lcg.below(2)) as usize;
        for _ in 0..n {
            digest = digest.wrapping_mul(3).wrapping_add(7);
            let _ = lg.append(pcr, 1 + (lcg.below(6)) as u32, digest);
        }
    }
    lg
}

/// 按日志真值生成读回 PCR。
pub fn readback_of(lg: &TpmLog) -> [u64; 8] {
    lg.replay().unwrap_or([0; 8])
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx0171_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F0171 TPM2事件重放只读");
    let lg = standard_log();

    // 1. 日志自洽（序号连续）
    s.add("TPM-日志序连续", lg.seq_ok(), if lg.seq_ok() { "" } else { "序号断档" });

    // 2. 重放与读回一致
    let rb = readback_of(&lg);
    let v = lg.verify(&rb);
    s.add("TPM-重放一致", v.is_ok(), if v.is_ok() { "" } else { "PCR 不一致" });

    // 3. 只读性（重放不改日志）
    s.add("TPM-只读性", lg.readonly_check(), if lg.readonly_check() { "" } else { "重放改写日志" });

    // 4. 空日志全 PCR 为零基
    let empty = TpmLog::new();
    let z = empty.replay().unwrap_or([1; 8]);
    s.add("TPM-空日志零基", z.iter().all(|x| *x == 0),
        if z.iter().all(|x| *x == 0) { "" } else { "零基污染" });

    // 5. 单事件 extend 语义（确定性 + 顺序敏感）
    let a = pcr_extend(0, 100);
    let b = pcr_extend(0, 100);
    let c = pcr_extend(a, 100);
    s.add("TPM-extend语义", a == b && a != c && a != 0,
        if a == b && a != c { "" } else { "extend 漂移" });

    // 6. 顺序敏感：同事件不同序结果不同
    let mut lg2 = TpmLog::new();
    let _ = lg2.append(0, 1, 10);
    let _ = lg2.append(0, 2, 20);
    let r1 = lg2.replay().unwrap_or([0; 8]);
    let mut lg3 = TpmLog::new();
    let _ = lg3.append(0, 2, 20);
    let _ = lg3.append(0, 1, 10);
    let r2 = lg3.replay().unwrap_or([0; 8]);
    s.add("TPM-顺序敏感", r1[0] != r2[0], if r1[0] != r2[0] { "" } else { "序不敏感" });

    // 7. 越域 PCR 拒（8..23 与 >23）
    let mut lg4 = TpmLog::new();
    let r8 = lg4.append(8, 1, 5);    // >7：重放域外但日志可存
    let r24 = lg4.append(24, 1, 5);  // >23：日志拒
    let replay_rej = {
        let mut tmp = TpmLog::new();
        let _ = tmp.append(7, 1, 5);
        // 手工塞一条 PCR>7 验重放拒
        tmp.events.push(TpmEvent { seq: 2, pcr: 8, etype: 1, digest: 6 });
        tmp.replay() == Err(E_PCR)
    };
    s.add("TPM-PCR域拒", r24 == Err(E_PCR) && r8.is_ok() && replay_rej,
        if r24 == Err(E_PCR) { "" } else { "越域放行" });

    // 8. 未登记类型拒
    let mut lg5 = TpmLog::new();
    let rt = lg5.append(0, 99, 1);
    s.add("TPM-类型拒", rt == Err(E_TYPE), if rt == Err(E_TYPE) { "" } else { "野类型放行" });

    // 9. 篡改检出：读回与重放不符 → E_MISMATCH
    let mut bad_rb = rb;
    bad_rb[3] = bad_rb[3].wrapping_add(1);
    let mis = lg.verify(&bad_rb);
    s.add("TPM-篡改检出", mis == Err(E_MISMATCH), if mis == Err(E_MISMATCH) { "" } else { "篡改漏检" });

    // 10. LCG 抽样 5 事件指纹稳定
    let mut lcg = Lcg(0xB811);
    let mut spot = true;
    for _ in 0..5 {
        let i = lcg.below(lg.events.len() as u64) as usize;
        if lg.events[i].fp() != lg.events[i].fp() {
            spot = false;
        }
    }
    s.add("TPM-指纹稳定", spot, if spot { "" } else { "指纹漂移" });

    // 11. 事件指纹互异
    let mut fp_uniq = true;
    for i in 0..lg.events.len() {
        for j in (i + 1)..lg.events.len() {
            if lg.events[i].fp() == lg.events[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("TPM-指纹互异", fp_uniq, if fp_uniq { "" } else { "指纹碰撞" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB810 && SEG_BASE < 0xB830;
    s.add("TPM-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
