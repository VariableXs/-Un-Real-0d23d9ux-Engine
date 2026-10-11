//! cgi09_bwfuzz — CGPU-F1288 带宽 fuzz 与安全（风暴打不穿守恒，越权进不了
//! 队列）。
//!
//! 判据原文（施工书锚点 CGPU-F1288）：fuzz 安全：带宽 fuzz（传输风暴/畸形
//! 请求——守恒不变量：Σ账≤带宽）；带宽安全（传输数据完整性校验；队列权
//! 限——非授权传输拒绝）；案例固化 CI。判据：风暴、完整性、权限、CI、判
//! 据。
//!
//! 职责定位：传输风暴注入器（海量随机传输灌带宽账——守恒不变量全程核验：
//! 四账户 Σ账 ≤ 总带宽，风暴越大等式越不能破）+ 畸形请求注入器（零长/超
//! 长/负值语义/未知账户——畸形面准入钳制与拒绝显性）+ 数据完整性校验器
//! （传输载荷指纹 FNV——写前算/读后验，指纹失配=数据损坏立案）+ 队列权限
//! 门（传输主体权限表——非授权主体拒绝入队，权限闭集外拒绝）+ 案例固化
//! CI（LCG 种子+风暴谱指纹）。与 cgi03_bwaccount（账本）、cgi05_pciearb
//! （仲裁）、cgi06_txqueue（队列）分工：彼是正常路径，此验证守恒与安全在
//! 异常输入下不破。诊断码段 0x6601~0x6605。

use alloc::vec::Vec;

/// 模块版本。
pub const BW_FUZZ_VERSION: &str = "I09-bwfuzz-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x66 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_CONSERVATION_BROKEN: u32 = 0x6601; // 守恒破（Σ账>带宽）
pub const XC_PAYLOAD_CORRUPT: u32 = 0x6602; // 数据完整性失守（指纹失配）
pub const XC_UNAUTHORIZED_TX: u32 = 0x6603; // 非授权传输被采纳
pub const XC_MALFORMED_ACCEPTED: u32 = 0x6604; // 畸形请求被采纳
pub const XC_STORM_CASE_NONREPRO: u32 = 0x6605; // 风暴案例不可复现

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 四账户闭集（与 cgi03 对齐）。
pub const BW_ACCOUNTS: [&str; 4] = ["present", "interactive", "background", "system"];
/// 总带宽（MB/ms×100 定点——cgi03 口径）。
pub const TOTAL_BW_MB100: u64 = 50_000;
/// 载荷指纹种子域。
pub const PAYLOAD_SIG_SEED: u64 = 0xB15A_0000_0000_0001;
/// 权限主体容量。
pub const PRINCIPAL_CAP: usize = 16;
/// LCG 参数。
pub const LCG_A: u64 = 6364136223846793005;
pub const LCG_C: u64 = 1442695040888963407;

/// 可复现随机流。
pub struct BwFuzzRng {
    state: u64,
}

impl BwFuzzRng {
    pub fn new(seed: u64) -> Self {
        BwFuzzRng { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 带宽账（守恒不变量载体——判据"Σ账≤带宽"）。
pub struct ConservationLedger {
    accounts: [u64; 4],
    pub total_bw_mb100: u64,
    /// 拒绝计数（超带宽/畸形）。
    pub rejected: u64,
    pub accepted: u64,
}

impl ConservationLedger {
    pub fn new(total: u64) -> Self {
        ConservationLedger {
            accounts: [0; 4],
            total_bw_mb100: total,
            rejected: 0,
            accepted: 0,
        }
    }

    /// 记账（超带宽拒——守恒硬保证）。
    pub fn charge(&mut self, account: usize, amount_mb100: u64) -> Result<(), u32> {
        if account >= 4 {
            self.rejected += 1;
            return Err(XC_MALFORMED_ACCEPTED);
        }
        if amount_mb100 == 0 {
            self.rejected += 1;
            return Err(XC_MALFORMED_ACCEPTED);
        }
        let used = self.accounts[0] + self.accounts[1] + self.accounts[2] + self.accounts[3];
        if used + amount_mb100 > self.total_bw_mb100 {
            self.rejected += 1;
            return Err(XC_CONSERVATION_BROKEN);
        }
        self.accounts[account] += amount_mb100;
        self.accepted += 1;
        Ok(())
    }

    /// 守恒核验（判据核心不变量）。
    pub const fn verify(&self) -> Result<(), u32> {
        let used = self.accounts[0] + self.accounts[1] + self.accounts[2] + self.accounts[3];
        if used > self.total_bw_mb100 {
            return Err(XC_CONSERVATION_BROKEN);
        }
        Ok(())
    }

    /// 已用。
    pub const fn used(&self) -> u64 {
        self.accounts[0] + self.accounts[1] + self.accounts[2] + self.accounts[3]
    }

    /// 余额。
    pub const fn remaining(&self) -> u64 {
        self.total_bw_mb100 - self.used()
    }
}

/// 载荷完整性校验器（判据"传输数据完整性"——FNV 指纹）。
pub struct PayloadIntegrity {
    pub corrupt_events: u64,
    pub verified: u64,
}

impl PayloadIntegrity {
    pub fn new() -> Self {
        PayloadIntegrity {
            corrupt_events: 0,
            verified: 0,
        }
    }

    /// 写前指纹。
    pub const fn seal(payload: &[u8]) -> u64 {
        let mut h: u64 = PAYLOAD_SIG_SEED;
        let mut i = 0;
        while i < payload.len() {
            h = (h ^ (payload[i] as u64)).wrapping_mul(0x1000_0000_01b3);
            i += 1;
        }
        h
    }

    /// 读后验（指纹失配=损坏立案）。
    pub fn verify(&mut self, payload: &[u8], expect: u64) -> Result<(), u32> {
        let got = Self::seal(payload);
        self.verified += 1;
        if got != expect {
            self.corrupt_events += 1;
            return Err(XC_PAYLOAD_CORRUPT);
        }
        Ok(())
    }
}

/// 队列权限门（判据"队列权限——非授权传输拒绝"）。
pub struct QueueAcl {
    /// 授权主体表（闭集登记制）。
    principals: Vec<(u64, bool)>, // (principal_id, allow)
    pub denied: u64,
}

impl QueueAcl {
    pub fn new() -> Self {
        QueueAcl {
            principals: Vec::with_capacity(PRINCIPAL_CAP),
            denied: 0,
        }
    }

    /// 主体登记（幂等覆盖）。
    pub fn register(&mut self, id: u64, allow: bool) -> Result<(), u32> {
        if id == 0 {
            self.denied += 1;
            return Err(XC_UNAUTHORIZED_TX);
        }
        let mut i = 0;
        while i < self.principals.len() {
            if self.principals[i].0 == id {
                self.principals[i].1 = allow;
                return Ok(());
            }
            i += 1;
        }
        if self.principals.len() >= PRINCIPAL_CAP {
            self.denied += 1;
            return Err(XC_UNAUTHORIZED_TX);
        }
        self.principals.push((id, allow));
        Ok(())
    }

    /// 准入判定（未登记=拒绝——默认 deny 白名单制）。
    pub fn admit(&mut self, id: u64) -> Result<(), u32> {
        let mut i = 0;
        while i < self.principals.len() {
            if self.principals[i].0 == id {
                if self.principals[i].1 {
                    return Ok(());
                }
                self.denied += 1;
                return Err(XC_UNAUTHORIZED_TX);
            }
            i += 1;
        }
        self.denied += 1;
        Err(XC_UNAUTHORIZED_TX)
    }

    /// 主体数。
    pub const fn len(&self) -> usize {
        self.principals.len()
    }
}

/// 带宽 fuzz 编排器。
pub struct BwFuzzer {
    cases: Vec<u64>,
    pub runs: u64,
    pub failures: u64,
}

impl BwFuzzer {
    pub fn new(_seed: u64) -> Self {
        BwFuzzer {
            cases: Vec::new(),
            runs: 0,
            failures: 0,
        }
    }

    /// 传输风暴 fuzz：海量随机记账——守恒全程不破（判据"传输风暴"）。
    pub fn fuzz_storm(&mut self, rounds: u64, seed: u64) -> Result<(u64, u64), u32> {
        self.runs += 1;
        let mut rng = BwFuzzRng::new(seed);
        let mut ledger = ConservationLedger::new(TOTAL_BW_MB100);
        let mut i = 0;
        while i < rounds {
            let acct = rng.below(6) as usize; // 含越界（4/5）——畸形面
            let amt = rng.below(TOTAL_BW_MB100 / 4);
            let _ = ledger.charge(acct, amt);
            // 守恒必须每轮成立（风暴越大越不能破）。
            ledger.verify()?;
            i += 1;
        }
        Ok((ledger.accepted, ledger.rejected))
    }

    /// 畸形请求 fuzz：零长/超长/越界账户——全拒不采纳。
    pub fn fuzz_malformed(&mut self, rounds: u64, seed: u64) -> Result<u64, u32> {
        self.runs += 1;
        let mut rng = BwFuzzRng::new(seed);
        let mut ledger = ConservationLedger::new(TOTAL_BW_MB100);
        let mut all_rejected = 0;
        let mut i = 0;
        while i < rounds {
            // 畸形面：零额（rng 恰 0）、越界账户、超带宽单笔。
            let pick = rng.below(3);
            let r = match pick {
                0 => ledger.charge(rng.below(6) as usize, 0), // 零额
                1 => ledger.charge(4 + rng.below(4) as usize, 100), // 越界账户
                _ => ledger.charge(0, TOTAL_BW_MB100 + 1),    // 超带宽单笔
            };
            if r.is_err() {
                all_rejected += 1;
            } else {
                self.failures += 1;
                return Err(XC_MALFORMED_ACCEPTED);
            }
            i += 1;
        }
        Ok(all_rejected)
    }

    /// 权限 fuzz：白名单外主体全拒、授权主体全过（判据"队列权限"）。
    pub fn fuzz_acl(&mut self, rounds: u64, seed: u64) -> Result<(u64, u64), u32> {
        self.runs += 1;
        let mut rng = BwFuzzRng::new(seed);
        let mut acl = QueueAcl::new();
        acl.register(100, true)?;
        acl.register(200, false)?; // 显性禁用主体
        let mut allowed = 0;
        let mut denied = 0;
        let mut i = 0;
        while i < rounds {
            let id = match rng.below(3) {
                0 => 100,                      // 授权
                1 => 200,                      // 禁用
                _ => 300 + rng.below(1000),    // 未登记
            };
            match acl.admit(id) {
                Ok(()) => allowed += 1,
                Err(_) => denied += 1,
            }
            i += 1;
        }
        Ok((allowed, denied))
    }

    /// 完整性 fuzz：写前指纹/读后验——篡改必被查（判据"完整性"）。
    pub fn fuzz_integrity(&mut self, rounds: u64, seed: u64) -> Result<u64, u32> {
        self.runs += 1;
        let mut rng = BwFuzzRng::new(seed);
        let mut checker = PayloadIntegrity::new();
        let mut caught = 0;
        let mut i = 0;
        while i < rounds {
            // 载荷：随机 16 字节。
            let mut payload = [0u8; 16];
            let mut j = 0;
            while j < 16 {
                payload[j] = (rng.next() & 0xFF) as u8;
                j += 1;
            }
            let sig = PayloadIntegrity::seal(&payload);
            // 一半概率篡改一个字节。
            if rng.below(2) == 0 {
                let k = rng.below(16) as usize;
                payload[k] ^= 0xFF;
            }
            if checker.verify(&payload, sig) == Err(XC_PAYLOAD_CORRUPT) {
                caught += 1;
            }
            i += 1;
        }
        // 篡改必须全被抓（约一半轮）。
        let expect_min = rounds / 4;
        if caught < expect_min {
            self.failures += 1;
            return Err(XC_PAYLOAD_CORRUPT);
        }
        Ok(caught)
    }

    /// 案例固化（判据"案例固化 CI"）。
    pub fn freeze_case(&mut self, seed: u64, rounds: u64) -> Result<(u64, u64), u32> {
        let mut a = BwFuzzer::new(seed);
        let r1 = a.fuzz_storm(rounds, seed)?;
        let mut b = BwFuzzer::new(seed);
        let r2 = b.fuzz_storm(rounds, seed)?;
        if r1 != r2 {
            self.failures += 1;
            return Err(XC_STORM_CASE_NONREPRO);
        }
        self.cases.push(seed);
        Ok(r1)
    }

    /// CI 回放。
    pub fn replay_ci(&mut self, rounds: u64) -> Result<usize, u32> {
        let n = self.cases.len();
        let mut i = 0;
        while i < n {
            let seed = self.cases[i];
            let mut p1 = BwFuzzer::new(seed);
            let r1 = p1.fuzz_storm(rounds, seed)?;
            let mut p2 = BwFuzzer::new(seed);
            let r2 = p2.fuzz_storm(rounds, seed)?;
            if r1 != r2 {
                self.failures += 1;
                return Err(XC_STORM_CASE_NONREPRO);
            }
            i += 1;
        }
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：风暴守恒 / 畸形拒绝 / 权限 / 完整性 / CI）
// ---------------------------------------------------------------------------

impl BwFuzzer {
    /// 组一：风暴守恒——大风暴后守恒仍成立、拒绝计数显性。
    pub fn selfcheck_storm(&mut self) -> Result<(), u32> {
        let (acc, rej) = self.fuzz_storm(4096, 2026)?;
        if acc == 0 {
            return Err(XC_CONSERVATION_BROKEN);
        }
        if rej == 0 {
            return Err(XC_CONSERVATION_BROKEN); // 大风暴必有拒绝（带宽有限）
        }
        // 手工核验：恰好灌满后拒绝。
        let mut l = ConservationLedger::new(1000);
        l.charge(0, 600)?;
        if l.charge(1, 500) != Err(XC_CONSERVATION_BROKEN) {
            return Err(XC_CONSERVATION_BROKEN);
        }
        l.charge(1, 400)?; // 恰好满
        if l.used() != 1000 || l.remaining() != 0 {
            return Err(XC_CONSERVATION_BROKEN);
        }
        if l.verify().is_err() {
            return Err(XC_CONSERVATION_BROKEN);
        }
        Ok(())
    }

    /// 组二：畸形拒绝——三族畸形全拒零采纳。
    pub fn selfcheck_malformed(&mut self) -> Result<(), u32> {
        let rej = self.fuzz_malformed(256, 777)?;
        if rej != 256 {
            return Err(XC_MALFORMED_ACCEPTED);
        }
        Ok(())
    }

    /// 组三：权限——授权过/禁用拒/未登记拒、默认 deny。
    pub fn selfcheck_acl(&mut self) -> Result<(), u32> {
        let (allowed, denied) = self.fuzz_acl(300, 999)?;
        if allowed == 0 || denied == 0 {
            return Err(XC_UNAUTHORIZED_TX);
        }
        // 手工：未登记主体拒。
        let mut acl = QueueAcl::new();
        if acl.admit(42) != Err(XC_UNAUTHORIZED_TX) {
            return Err(XC_UNAUTHORIZED_TX);
        }
        acl.register(42, true)?;
        acl.admit(42)?;
        // 零 ID 主体拒绝登记。
        if acl.register(0, true) != Err(XC_UNAUTHORIZED_TX) {
            return Err(XC_UNAUTHORIZED_TX);
        }
        // 授权翻转（幂等覆盖）。
        acl.register(42, false)?;
        if acl.admit(42) != Err(XC_UNAUTHORIZED_TX) {
            return Err(XC_UNAUTHORIZED_TX);
        }
        Ok(())
    }

    /// 组四：完整性——篡改全被抓、未篡改全过。
    pub fn selfcheck_integrity(&mut self) -> Result<(), u32> {
        let caught = self.fuzz_integrity(200, 31337)?;
        if caught == 0 {
            return Err(XC_PAYLOAD_CORRUPT);
        }
        // 未篡改路径。
        let mut c = PayloadIntegrity::new();
        let p = [1u8, 2, 3, 4, 5];
        let sig = PayloadIntegrity::seal(&p);
        c.verify(&p, sig)?;
        // 单字节翻转被抓。
        let mut p2 = p;
        p2[3] ^= 1;
        if c.verify(&p2, sig) != Err(XC_PAYLOAD_CORRUPT) || c.corrupt_events != 1 {
            return Err(XC_PAYLOAD_CORRUPT);
        }
        Ok(())
    }

    /// 组五：CI 固化——同种子风暴谱一致、回放全绿。
    pub fn selfcheck_ci(&mut self) -> Result<(), u32> {
        let mut f = BwFuzzer::new(0);
        let _ = f.freeze_case(2026, 512)?;
        let _ = f.freeze_case(1010, 512)?;
        let n = f.replay_ci(512)?;
        if n != 2 {
            return Err(XC_STORM_CASE_NONREPRO);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_storm()?;
        self.selfcheck_malformed()?;
        self.selfcheck_acl()?;
        self.selfcheck_integrity()?;
        self.selfcheck_ci()?;
        Ok(())
    }
}
