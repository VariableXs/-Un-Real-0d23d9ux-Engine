//! F024 深化批次四 · 证书链构建引擎（compatstar2/deep3 · G-A-24）。
//!
//! 主层 tlsstore.rs 覆盖证书库/链验证/SNI/OCSP，批次三深化覆盖 EKU/SAN/
//! 名称约束；本批补齐主册【功能定义】「全语义对齐」的构建/账本/容错面：
//! 链构建（叶证书按发行者字段向上查表，定长 16 池、深度上限 8，缺发行者/
//! 环检出显性 Err）、逐级时间有效窗校验（notBefore/notAfter 边界含）、链长
//! 账与截断原因码（缺根/过深/过期三分类计数）、信任判定矩阵（根在信任库?
//! 全链时间有效? 吊销桩状态? → 三位输入 8 格输出表）。
//!
//! 判据对账：主册 G-A-24（自签/过期/域名错三负样本全拒）+ MS/rustls 链
//! 构建语义（RFC 5280 路径构造：叶向上逐级按 issuer 查找，自签根终止）。
//! 时间轴为 Unix 秒；样例证书窗口取真实日历时刻。与主层 tlsstore.rs、
//! deep/f024d（密钥用法三位子面）、deep2/f024e.rs（扩展与名称约束）语义面
//! 互补不重叠。零堆纪律：定长数组 + &'static str，错误显性化。

use crate::checks::CheckSet;

/// 证书池容量。
pub const MAX_CERTS: usize = 16;
/// 链深度上限（RFC 5280 路径长度域内口径）。
pub const MAX_CHAIN_DEPTH: usize = 8;

/// Unix 秒样例时刻（真实日历：2020-01-01 / 2025-01-01 / 2026-01-01 /
/// 2030-01-01 的 00:00:00Z）。
pub const T_2020: u64 = 1577836800;
pub const T_2025: u64 = 1735689600;
pub const T_2026: u64 = 1767225600;
pub const T_2030: u64 = 1893456000;

/// 一张证书（域内投影：主题/发行者/有效窗/CA 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cert {
    pub subject: &'static str,
    pub issuer: &'static str,
    pub not_before: u64,
    pub not_after: u64,
    pub is_ca: bool,
}

pub struct CertPool {
    certs: [Option<Cert>; MAX_CERTS],
    pub count: usize,
}

/// 链构建结果：自叶至根的池下标序列（定长 8）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainBuild {
    pub idx: [usize; MAX_CHAIN_DEPTH],
    pub len: usize,
}

/// 逐级时间审计账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainTimeAudit {
    pub levels: usize,
    pub expired_levels: u32,
    pub all_valid: bool,
}

impl CertPool {
    pub const fn new() -> Self {
        CertPool { certs: [None; MAX_CERTS], count: 0 }
    }

    /// 入池；池满显性 Err。返回池下标。
    pub fn install(&mut self, c: Cert) -> Result<usize, &'static str> {
        if self.count >= MAX_CERTS {
            return Err("cert-pool-full");
        }
        self.certs[self.count] = Some(c);
        self.count += 1;
        Ok(self.count - 1)
    }

    fn find_by_subject(&self, subject: &str) -> Option<usize> {
        (0..self.count).find(|&i| self.certs[i].map(|c| c.subject == subject).unwrap_or(false))
    }

    /// 链构建：叶起按 issuer 向上查表；自签根终止。缺发行者/环/过深均显性
    /// Err（零静默——RFC 5280 路径构造失败必须带原因上抛）。
    pub fn build_chain(&self, leaf: usize) -> Result<ChainBuild, &'static str> {
        if leaf >= self.count {
            return Err("unknown-leaf");
        }
        let mut visited = [false; MAX_CERTS];
        let mut idx = [0usize; MAX_CHAIN_DEPTH];
        let mut n = 0usize;
        let mut cur = leaf;
        loop {
            if visited[cur] {
                return Err("loop-detected");
            }
            visited[cur] = true;
            idx[n] = cur;
            n += 1;
            let c = self.certs[cur].unwrap_or(Cert {
                subject: "?", issuer: "?", not_before: 0, not_after: 0, is_ca: false,
            });
            if c.issuer == c.subject {
                break; // 自签根——链终
            }
            if n == MAX_CHAIN_DEPTH {
                // 已达深度上限：发行者存在即过深，不存在即缺根（如实区分）。
                return match self.find_by_subject(c.issuer) {
                    Some(_) => Err("too-deep"),
                    None => Err("issuer-missing"),
                };
            }
            match self.find_by_subject(c.issuer) {
                Some(next) => cur = next,
                None => return Err("issuer-missing"),
            }
        }
        Ok(ChainBuild { idx, len: n })
    }
}

/// 逐级时间有效窗校验（notBefore/notAfter 边界含——MS CertVerifyTimeValidity
/// 语义：恰在两端点均视为有效）。
pub fn time_window_valid(c: &Cert, now: u64) -> bool {
    c.not_before <= now && now <= c.not_after
}

/// 对已构建链逐级审计时间窗。
pub fn audit_chain_time(pool: &CertPool, chain: &ChainBuild, now: u64) -> ChainTimeAudit {
    let mut expired = 0u32;
    for i in 0..chain.len {
        if let Some(c) = pool.certs[chain.idx[i]] {
            if !time_window_valid(&c, now) {
                expired += 1;
            }
        }
    }
    ChainTimeAudit { levels: chain.len, expired_levels: expired, all_valid: expired == 0 }
}

/// 链长账与截断原因码账（缺根/过深/过期三分类计数——构建面诊断数据源）。
pub struct ChainBuilder {
    pub pool: CertPool,
    pub built: u32,
    pub missing_root: u32,
    pub too_deep: u32,
    pub expired: u32,
}

impl ChainBuilder {
    pub const fn new() -> Self {
        ChainBuilder { pool: CertPool::new(), built: 0, missing_root: 0, too_deep: 0, expired: 0 }
    }

    pub fn install(&mut self, c: Cert) -> Result<usize, &'static str> {
        self.pool.install(c)
    }

    /// 一次构建尝试：链构建失败按缺根/过深计账，时间审计失败按过期计账，
    /// 均显性 Err（计数与返回值账面自洽）。
    pub fn attempt(&mut self, leaf: usize, now: u64) -> Result<ChainBuild, &'static str> {
        let chain = match self.pool.build_chain(leaf) {
            Ok(c) => c,
            Err("issuer-missing") => {
                self.missing_root += 1;
                return Err("issuer-missing");
            }
            Err("too-deep") => {
                self.too_deep += 1;
                return Err("too-deep");
            }
            Err(e) => return Err(e),
        };
        if !audit_chain_time(&self.pool, &chain, now).all_valid {
            self.expired += 1;
            return Err("chain-time-invalid");
        }
        self.built += 1;
        Ok(chain)
    }
}

// ---------------------------------------------------------------------------
// 信任判定矩阵（三位输入 8 格输出表）
// ---------------------------------------------------------------------------

/// 信任判定输出（8 格全枚举，见 trust_decision 逐格注释）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustVerdict {
    Trusted,
    UntrustedRoot,
    TimeInvalid,
    RevokedStub,
}

/// 三位输入（根在信任库? 全链时间有效? 吊销桩命中?）→ 8 格判定表。
/// 逐格语义（OCSP 软失败策略的判定出口，主层 tlsstore.rs 橙灯上游）：
/// 1. (T,T,F) → Trusted：三关全过。
/// 2. (T,T,T) → RevokedStub：吊销桩命中优先于一切。
/// 3. (T,F,*) → TimeInvalid：时间窗失守（过期/未生效）。
/// 4. (F,*,F) → UntrustedRoot：根不在信任库（自签/未知根——负样本口径）。
/// 5. (F,*,T) → RevokedStub：吊销信息仍优先出示。
pub fn trust_decision(root_in_store: bool, time_valid: bool, revoked: bool) -> TrustVerdict {
    match (root_in_store, time_valid, revoked) {
        (true, true, false) => TrustVerdict::Trusted,
        (_, _, true) => TrustVerdict::RevokedStub,
        (false, _, _) => TrustVerdict::UntrustedRoot,
        (true, false, _) => TrustVerdict::TimeInvalid,
    }
}

/// 域自检（深化批次四）。
pub fn run_f024f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F024-certchain-d4");
    // 1) 三级链构建：svc ← TLS CA ← Root（自签根终止，len=3）。
    let mut b = ChainBuilder::new();
    let root = b.install(Cert { subject: "VARIX Root CA", issuer: "VARIX Root CA", not_before: T_2020, not_after: T_2030, is_ca: true }).expect("池未满");
    let inter = b.install(Cert { subject: "VARIX TLS CA", issuer: "VARIX Root CA", not_before: T_2020, not_after: T_2030, is_ca: true }).expect("池未满");
    let leaf = b.install(Cert { subject: "svc.varix.local", issuer: "VARIX TLS CA", not_before: T_2025, not_after: T_2026, is_ca: false }).expect("池未满");
    let chain = b.attempt(leaf, T_2025 + 86400).expect("时间窗内必成");
    cs.add("chain_build_three_level", chain.len == 3 && chain.idx[0] == leaf
        && chain.idx[1] == inter && chain.idx[2] == root, "");
    // 2) 缺发行者显性 Err 并计账（链走不到自签根 = 缺根分类）。
    let _ = b.install(Cert { subject: "orphan", issuer: "Nobody CA", not_before: T_2020, not_after: T_2030, is_ca: false });
    let orphan = b.pool.count - 1;
    cs.add("issuer_missing_err", b.attempt(orphan, T_2025) == Err("issuer-missing")
        && b.missing_root == 1, "");
    // 3) 环检出显性 Err（A↔B 互签）。
    let _ = b.install(Cert { subject: "loopA", issuer: "loopB", not_before: T_2020, not_after: T_2030, is_ca: true });
    let la = b.pool.count - 1;
    let _ = b.install(Cert { subject: "loopB", issuer: "loopA", not_before: T_2020, not_after: T_2030, is_ca: true });
    let lb = b.pool.count - 1;
    cs.add("loop_detected_err", b.pool.build_chain(la) == Err("loop-detected")
        && b.pool.build_chain(lb) == Err("loop-detected"), "");
    // 4) 深度上限 8：九级链如实拒绝为 too-deep 并计账。
    let mut deep = ChainBuilder::new();
    const DEEP: [&str; 9] = ["deep-leaf", "deep-ca1", "deep-ca2", "deep-ca3", "deep-ca4", "deep-ca5", "deep-ca6", "deep-ca7", "deep-root"];
    for i in 0..9usize {
        let issuer = if i == 8 { DEEP[8] } else { DEEP[i + 1] };
        let _ = deep.install(Cert { subject: DEEP[i], issuer, not_before: T_2020, not_after: T_2030, is_ca: true });
    }
    cs.add("too_deep_err", deep.attempt(0, T_2025) == Err("too-deep") && deep.too_deep == 1, "");
    // 5) 时间窗边界含：恰在 notBefore/notAfter 均有效（MS 语义），窗外无效。
    let c = Cert { subject: "edge", issuer: "edge", not_before: T_2025, not_after: T_2026, is_ca: false };
    cs.add("time_boundary_inclusive", time_window_valid(&c, T_2025)
        && time_window_valid(&c, T_2026) && time_window_valid(&c, T_2026 - 1)
        && !time_window_valid(&c, T_2030), "");
    // 6) 过期计账：叶窗 2025-2026，now=2030 → 链时间无效并计账。
    let mut b2 = ChainBuilder::new();
    let _ = b2.install(Cert { subject: "r", issuer: "r", not_before: T_2020, not_after: T_2030, is_ca: true });
    let l2 = b2.install(Cert { subject: "l", issuer: "r", not_before: T_2025, not_after: T_2026, is_ca: false }).expect("池未满");
    cs.add("expired_counted", b2.attempt(l2, T_2030) == Err("chain-time-invalid")
        && b2.expired == 1 && b2.built == 0, "");
    // 7) 时间审计账：过期级数如实记录（叶一级过期 → expired_levels=1）。
    let ch = b2.pool.build_chain(l2).expect("链可构建");
    let aud = audit_chain_time(&b2.pool, &ch, T_2030);
    cs.add("time_audit_levels", aud.levels == 2 && aud.expired_levels == 1
        && !aud.all_valid, "");
    // 8) 信任矩阵 8 格扫掠：Trusted 1 格、RevokedStub 4 格、UntrustedRoot
    //    2 格、TimeInvalid 1 格——账面与逐格注释一致。
    let mut out = [0u32; 4];
    for bits in 0u8..8 {
        let v = trust_decision(bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
        out[match v {
            TrustVerdict::Trusted => 0,
            TrustVerdict::TimeInvalid => 1,
            TrustVerdict::UntrustedRoot => 2,
            TrustVerdict::RevokedStub => 3,
        }] += 1;
    }
    cs.add("trust_matrix_8_cells", out == [1, 1, 2, 4], "");
    // 9) 池下标越界显性 Err。
    cs.add("unknown_leaf_err", b2.pool.build_chain(99) == Err("unknown-leaf"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_signed_single_level_chain() {
        let mut b = ChainBuilder::new();
        let _ = b.install(Cert {
            subject: "self", issuer: "self", not_before: T_2020, not_after: T_2030, is_ca: true,
        });
        let ch = b.attempt(0, T_2025).expect("自签单级必成");
        assert_eq!(ch.len, 1, "自签即根，链长 1");
        assert_eq!(b.built, 1);
    }

    #[test]
    fn leaf_before_window_is_expired_class() {
        let mut b = ChainBuilder::new();
        let _ = b.install(Cert { subject: "r", issuer: "r", not_before: T_2020, not_after: T_2030, is_ca: true });
        let l = b.install(Cert { subject: "l", issuer: "r", not_before: T_2025, not_after: T_2026, is_ca: false }).expect("池未满");
        // now=2020 早于叶 notBefore → 同归「过期/未生效」分类，账面如实。
        assert_eq!(b.attempt(l, T_2020), Err("chain-time-invalid"));
        assert_eq!(b.expired, 1);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f024f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
