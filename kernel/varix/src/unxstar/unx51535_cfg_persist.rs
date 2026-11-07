//! UNX-F51535 B17 配置持久化（QoS/预算策略落盘与恢复） · 0xC08C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51536-J1 遥测接口对账 ×10（J5 消费锚定行零改写）
//!
//! 设计要点：
//!   ① QoS/预算策略四元组 (档位, 信用, 突发, 延迟上限) 定界校验后落盘，
//!      版本号单调递增，回退/前跳显性拒。
//!   ② 冻结导出布局：8 字节头 (魔数 + 条数) + 每条 20 字节记录 (4 字段
//!      + 校验和)，导出字节账 = 8 + n×20，账面与实写字节数逐字节对平。
//!   ③ 恢复判序：魔数 → 长度 → 校验和 → 字段界；任何一层损坏都拒恢复，
//!      损坏注入（字节翻转/魔数/截断）×10 全抓，逃逸恒 0。
//!   ④ J5 消费锚定行：每次提交落一条 (版本, 指纹, 字节总量) 锚定行，
//!      只增不改；×10 轮遥测接口对账前后锚定行逐字段零改写。
//!   ⑤ 账面单调：字节账与锚定行只增不减，回退注入显性拒并留痕。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：0xC0880 + 0x40 × (51535 − 51534)。
pub const SEG_BASE: u32 = 0xC08C0;

/// 档位上限（含）。
pub const TIER_MAX: u32 = 4;
/// 信用上限（含）。
pub const CREDITS_MAX: u32 = 4096;
/// 突发上限（含）。
pub const BURST_MAX: u32 = 1024;
/// 延迟上限（含），单位为抽象节拍。
pub const LAT_MAX: u32 = 100_000;
/// 策略库容量。
pub const MAX_POLICIES: usize = 16;
/// 导出头部字节数。
pub const HEADER_BYTES: usize = 8;
/// 单条记录字节数（4 字段 ×4 + 校验和 ×4）。
pub const RECORD_BYTES: usize = 20;
/// 导出魔数。
pub const EXPORT_MAGIC: u32 = 0xC08C_0B17;

/// QoS/预算策略四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QosPolicy {
    pub tier: u32,
    pub credits: u32,
    pub burst: u32,
    pub latency_cap: u32,
}

/// 诊断码闭集（配置持久化域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfgCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 档位为负
    TierNegative,
    /// 零档拒（档位必须 ≥1）
    TierZero,
    /// 档位越界
    TierOutOfRange,
    /// 信用越界
    CreditsOutOfRange,
    /// 零突发拒
    BurstZero,
    /// 突发越界
    BurstOutOfRange,
    /// 延迟上限越界
    LatencyOutOfRange,
    /// 策略库容量耗尽
    StoreFull,
    /// 版本回退或前跳
    VersionRegress,
    /// 导出魔数损坏
    MagicBad,
    /// 导出长度不符（含截断）
    LenBad,
    /// 记录校验和不符
    RecordCorrupt,
    /// 记录字段越界（校验和完好）
    FieldOutOfRange,
    /// 账面回退拒
    AccountRegress,
    /// 锚定行改写拒（零改写红线）
    AnchorLocked,
    /// 注入账守恒失败
    LedgerMismatch,
}

impl CfgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CfgCode::Ok => "成功",
            CfgCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值范围",
            CfgCode::TierNegative => "档位为负：档位必须 ≥1",
            CfgCode::TierZero => "零档拒：档位必须 ≥1",
            CfgCode::TierOutOfRange => "档位越界：档位不得超过上限",
            CfgCode::CreditsOutOfRange => "信用越界：核对抗议预算上限",
            CfgCode::BurstZero => "零突发拒：突发必须 ≥1",
            CfgCode::BurstOutOfRange => "突发越界：突发不得超过上限",
            CfgCode::LatencyOutOfRange => "延迟上限越界：不得超过节拍上限",
            CfgCode::StoreFull => "策略库已满：先封版或扩容",
            CfgCode::VersionRegress => "版本回退：版本号只进不退",
            CfgCode::MagicBad => "魔数损坏：导出头被改写，拒恢复",
            CfgCode::LenBad => "长度不符：载荷被截断或虚增，拒恢复",
            CfgCode::RecordCorrupt => "校验和不符：记录被翻转，拒恢复",
            CfgCode::FieldOutOfRange => "字段越界：字段值超出冻结界",
            CfgCode::AccountRegress => "账面回退：字节账只增不减",
            CfgCode::AnchorLocked => "锚定行零改写：J5 消费锚定行只增不改",
            CfgCode::LedgerMismatch => "注入账不平：注入=检出+逃逸必须守恒",
        }
    }
}

/// J5 消费锚定行：提交即落锚，只增不改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnchorRow {
    pub ver: u32,
    pub fp: u64,
    pub bytes_total: usize,
}

/// 策略指纹（64 位折叠）。
pub fn policy_fp(p: &QosPolicy) -> u64 {
    let mut fp = p.tier as u64;
    fp = fp.rotate_left(9) ^ p.credits as u64;
    fp = fp.rotate_left(9) ^ p.burst as u64;
    fp = fp.rotate_left(9) ^ p.latency_cap as u64;
    fp.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// 记录校验和（冻结算法）。
pub fn record_checksum(p: &QosPolicy) -> u32 {
    let v = p.tier
        ^ p.credits.rotate_left(8)
        ^ p.burst.rotate_left(16)
        ^ p.latency_cap.rotate_left(24);
    v ^ 0xB17C_08C0
}

/// 字段界校验。
fn fields_in_bounds(p: &QosPolicy) -> bool {
    p.tier >= 1
        && p.tier <= TIER_MAX
        && p.credits <= CREDITS_MAX
        && p.burst >= 1
        && p.burst <= BURST_MAX
        && p.latency_cap <= LAT_MAX
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push((v >> 24) as u8);
}

fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    let b0 = *buf.get(off)?;
    let b1 = *buf.get(off.checked_add(1)?)?;
    let b2 = *buf.get(off.checked_add(2)?)?;
    let b3 = *buf.get(off.checked_add(3)?)?;
    Some(u32::from_le_bytes([b0, b1, b2, b3]))
}

/// 单条记录序列化（20 字节）。
fn push_record(out: &mut Vec<u8>, p: &QosPolicy) {
    push_u32(out, p.tier);
    push_u32(out, p.credits);
    push_u32(out, p.burst);
    push_u32(out, p.latency_cap);
    push_u32(out, record_checksum(p));
}

/// 裸记录构造（注入面：字段可越界、校验和按冻结算法补齐）。
pub fn make_raw_record(tier: u32, credits: u32, burst: u32, lat: u32) -> Vec<u8> {
    let p = QosPolicy {
        tier,
        credits,
        burst,
        latency_cap: lat,
    };
    let mut rec: Vec<u8> = Vec::new();
    push_record(&mut rec, &p);
    rec
}

/// 策略库：版本化提交 + 导出/恢复 + J5 锚定行账。
pub struct PolicyStore {
    policies: Vec<QosPolicy>,
    version: u32,
    anchors: Vec<AnchorRow>,
    byte_account: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PolicyStore {
    pub fn new() -> PolicyStore {
        PolicyStore {
            policies: Vec::new(),
            version: 0,
            anchors: Vec::new(),
            byte_account: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 提交一条策略（判序：档位负/零 → 档位界 → 信用 → 突发 → 延迟 → 容量）。
    pub fn commit(&mut self, tier: i64, credits: u32, burst: u32, latency_cap: u32) -> Result<u32, CfgCode> {
        if tier < 0 {
            return Err(CfgCode::TierNegative);
        }
        let tier = tier as u32;
        if tier == 0 {
            return Err(CfgCode::TierZero);
        }
        if tier > TIER_MAX {
            return Err(CfgCode::TierOutOfRange);
        }
        if credits > CREDITS_MAX {
            return Err(CfgCode::CreditsOutOfRange);
        }
        if burst == 0 {
            return Err(CfgCode::BurstZero);
        }
        if burst > BURST_MAX {
            return Err(CfgCode::BurstOutOfRange);
        }
        if latency_cap > LAT_MAX {
            return Err(CfgCode::LatencyOutOfRange);
        }
        if self.policies.len() >= MAX_POLICIES {
            return Err(CfgCode::StoreFull);
        }
        let p = QosPolicy {
            tier,
            credits,
            burst,
            latency_cap,
        };
        self.policies.push(p);
        self.version = match self.version.checked_add(1) {
            Some(v) => v,
            None => return Err(CfgCode::VersionRegress),
        };
        let bytes_total = match self.policies.len().checked_mul(RECORD_BYTES) {
            Some(m) => match HEADER_BYTES.checked_add(m) {
                Some(n) => n,
                None => usize::MAX,
            },
            None => usize::MAX,
        };
        self.anchors.push(AnchorRow {
            ver: self.version,
            fp: policy_fp(&p),
            bytes_total,
        });
        Ok(self.version)
    }

    /// 导出：冻结布局写缓冲，字节账与实写逐字节对平。
    pub fn export(&mut self, out: &mut Vec<u8>) -> Result<usize, CfgCode> {
        let before = out.len();
        push_u32(out, EXPORT_MAGIC);
        push_u32(out, self.policies.len() as u32);
        for i in 0..self.policies.len() {
            let p = match self.policies.get(i) {
                Some(p) => *p,
                None => return Err(CfgCode::LenBad),
            };
            push_record(out, &p);
        }
        let written = out.len() - before;
        let expected = match self.policies.len().checked_mul(RECORD_BYTES) {
            Some(m) => match HEADER_BYTES.checked_add(m) {
                Some(n) => n,
                None => return Err(CfgCode::LenBad),
            },
            None => return Err(CfgCode::LenBad),
        };
        if written != expected {
            return Err(CfgCode::LenBad);
        }
        self.byte_account = match self.byte_account.checked_add(written) {
            Some(n) => n,
            None => return Err(CfgCode::AccountRegress),
        };
        Ok(written)
    }

    /// 恢复（判序：魔数 → 长度 → 校验和 → 字段界），纯校验不改库。
    pub fn import(buf: &[u8]) -> Result<usize, CfgCode> {
        let magic = match read_u32(buf, 0) {
            Some(v) => v,
            None => return Err(CfgCode::LenBad),
        };
        if magic != EXPORT_MAGIC {
            return Err(CfgCode::MagicBad);
        }
        let count = match read_u32(buf, 4) {
            Some(v) => v,
            None => return Err(CfgCode::LenBad),
        };
        let expected = match (count as usize).checked_mul(RECORD_BYTES) {
            Some(m) => match HEADER_BYTES.checked_add(m) {
                Some(n) => n,
                None => return Err(CfgCode::LenBad),
            },
            None => return Err(CfgCode::LenBad),
        };
        if buf.len() != expected {
            return Err(CfgCode::LenBad);
        }
        for i in 0..count as usize {
            let base = match i.checked_mul(RECORD_BYTES) {
                Some(m) => match HEADER_BYTES.checked_add(m) {
                    Some(b) => b,
                    None => return Err(CfgCode::LenBad),
                },
                None => return Err(CfgCode::LenBad),
            };
            let tier = match read_u32(buf, base) {
                Some(v) => v,
                None => return Err(CfgCode::LenBad),
            };
            let credits = match base.checked_add(4).and_then(|o| read_u32(buf, o)) {
                Some(v) => v,
                None => return Err(CfgCode::LenBad),
            };
            let burst = match base.checked_add(8).and_then(|o| read_u32(buf, o)) {
                Some(v) => v,
                None => return Err(CfgCode::LenBad),
            };
            let lat = match base.checked_add(12).and_then(|o| read_u32(buf, o)) {
                Some(v) => v,
                None => return Err(CfgCode::LenBad),
            };
            let stored_ck = match base.checked_add(16).and_then(|o| read_u32(buf, o)) {
                Some(v) => v,
                None => return Err(CfgCode::LenBad),
            };
            let p = QosPolicy {
                tier,
                credits,
                burst,
                latency_cap: lat,
            };
            if record_checksum(&p) != stored_ck {
                return Err(CfgCode::RecordCorrupt);
            }
            if !fields_in_bounds(&p) {
                return Err(CfgCode::FieldOutOfRange);
            }
        }
        Ok(count as usize)
    }

    /// 锚定行改写防线：任何改写尝试一律拒（零改写红线）。
    pub fn attempt_anchor_rewrite(&mut self, idx: usize, forged_fp: u64) -> Result<(), CfgCode> {
        if self.anchors.get(idx).is_none() {
            return Err(CfgCode::AnchorLocked);
        }
        let _ = forged_fp;
        Err(CfgCode::AnchorLocked)
    }

    /// 版本回退/前跳防线。
    pub fn try_set_version(&mut self, v: u32) -> Result<u32, CfgCode> {
        if v != self.version {
            return Err(CfgCode::VersionRegress);
        }
        Ok(self.version)
    }

    /// 账面回退防线。
    pub fn try_rewind_account(&mut self, target: usize) -> Result<(), CfgCode> {
        if target < self.byte_account {
            return Err(CfgCode::AccountRegress);
        }
        Ok(())
    }

    /// 锚定行快照（对账面：×10 轮前后零改写）。
    pub fn anchor_snapshot(&self) -> Vec<AnchorRow> {
        self.anchors.clone()
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    pub fn byte_account(&self) -> usize {
        self.byte_account
    }

    pub fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    pub fn note_injected(&mut self) {
        self.injected += 1;
    }

    pub fn note_detected(&mut self) {
        self.detected += 1;
    }

    pub fn note_escaped(&mut self) {
        self.escaped += 1;
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 损坏注入器 ①：载荷内单字节翻转。
pub fn inject_flip(buf: &mut Vec<u8>, byte_idx: usize) -> bool {
    match buf.get_mut(byte_idx) {
        Some(b) => {
            *b ^= 0x01;
            true
        }
        None => false,
    }
}

/// 损坏注入器 ②：魔数置零。
pub fn inject_magic(buf: &mut Vec<u8>) -> bool {
    let mut hit = true;
    for i in 0..4usize {
        match buf.get_mut(i) {
            Some(b) => *b = 0,
            None => hit = false,
        }
    }
    hit
}

/// 损坏注入器 ③：载荷截断。
pub fn inject_truncate(buf: &mut Vec<u8>, keep: usize) -> bool {
    if keep < buf.len() {
        buf.truncate(keep);
        true
    } else {
        false
    }
}

/// 便捷构造：提交 n 条合法策略的库。
pub fn store_with(n: usize) -> PolicyStore {
    let mut st = PolicyStore::new();
    for i in 0..n {
        let tier = 1 + (i as u32) % TIER_MAX;
        let credits = 256 + (i as u32) * 64;
        let burst = 64 + (i as u32) * 8;
        let lat = 1000 + (i as u32) * 100;
        let _ = st.commit(tier as i64, credits, burst, lat);
    }
    st
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一提交序列两次导出逐字节一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<u8>, Vec<AnchorRow>) {
        let mut st = store_with(3);
        let mut buf: Vec<u8> = Vec::new();
        let _ = st.export(&mut buf);
        (buf, st.anchor_snapshot())
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51535_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51535");
    // 1 正常流：三次提交版本单调，导出→恢复闭环。
    {
        let mut st = store_with(3);
        let mut buf: Vec<u8> = Vec::new();
        let exported = st.export(&mut buf);
        let imported = PolicyStore::import(&buf);
        s.add_dyn(
            "正常提交与导出恢复闭环",
            st.version() == 3
                && st.anchor_count() == 3
                && exported == Ok(HEADER_BYTES + 3 * RECORD_BYTES)
                && imported == Ok(3),
            "版本 1..3 单调，导出 68 字节，恢复 3 条",
        );
    }
    // 2 边界-零：零信用合法；零突发拒；空库导出仅 8 字节头。
    {
        let mut st = PolicyStore::new();
        let r0 = st.commit(1i64, 0, 32, 100);
        let mut st2 = PolicyStore::new();
        let rz = st2.commit(1i64, 0, 0, 100);
        let mut buf: Vec<u8> = Vec::new();
        let empty_export = st2.export(&mut buf);
        s.add_dyn(
            "边界零",
            r0.is_ok() && rz == Err(CfgCode::BurstZero) && empty_export == Ok(HEADER_BYTES) && buf.len() == HEADER_BYTES,
            "零信用放行、零突发拒、空库导出仅头",
        );
    }
    // 3 边界-负：负档位显性拒。
    {
        let mut st = PolicyStore::new();
        let r = st.commit(-1i64, 0, 32, 100);
        s.add_dyn("边界负值拒", r == Err(CfgCode::TierNegative), "负档位必须显性拒");
    }
    // 4 边界-上限：四字段全部取上限放行。
    {
        let mut st = PolicyStore::new();
        let r = st.commit(TIER_MAX as i64, CREDITS_MAX, BURST_MAX, LAT_MAX);
        s.add_dyn("边界上限放行", r.is_ok(), "上限四元组合法提交");
    }
    // 5 边界-上限+1：四字段分别越界逐一拒。
    {
        let mut st = PolicyStore::new();
        let r1 = st.commit((TIER_MAX + 1) as i64, 0, 32, 100);
        let r2 = st.commit(1i64, CREDITS_MAX + 1, 32, 100);
        let r3 = st.commit(1i64, 0, BURST_MAX + 1, 100);
        let r4 = st.commit(1i64, 0, 32, LAT_MAX + 1);
        s.add_dyn(
            "边界上限+1 拒",
            r1 == Err(CfgCode::TierOutOfRange)
                && r2 == Err(CfgCode::CreditsOutOfRange)
                && r3 == Err(CfgCode::BurstOutOfRange)
                && r4 == Err(CfgCode::LatencyOutOfRange),
            "四字段越界必须逐一拒绝",
        );
    }
    // 6 注入必抓 ①：字节翻转 ×10 校验和全抓。
    {
        let mut st = store_with(10);
        let mut caught = 0u32;
        for k in 0..10u32 {
            let mut buf: Vec<u8> = Vec::new();
            let _ = st.export(&mut buf);
            let byte_idx = HEADER_BYTES + (k as usize) * RECORD_BYTES + 5;
            let _ = inject_flip(&mut buf, byte_idx);
            st.note_injected();
            match PolicyStore::import(&buf) {
                Err(CfgCode::RecordCorrupt) => {
                    caught += 1;
                    st.note_detected();
                }
                _ => st.note_escaped(),
            }
        }
        s.add_dyn("翻转注入 10/10", caught == 10, "字节翻转必须被校验和全抓");
    }
    // 7 注入必抓 ②：魔数损坏 ×10 全抓。
    {
        let mut st = store_with(2);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut buf: Vec<u8> = Vec::new();
            let _ = st.export(&mut buf);
            let _ = inject_magic(&mut buf);
            st.note_injected();
            match PolicyStore::import(&buf) {
                Err(CfgCode::MagicBad) => {
                    caught += 1;
                    st.note_detected();
                }
                _ => st.note_escaped(),
            }
        }
        s.add_dyn("魔数注入 10/10", caught == 10, "魔数损坏必须全抓");
    }
    // 8 注入必抓 ③：截断注入 ×10 全抓。
    {
        let mut st = store_with(2);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut buf: Vec<u8> = Vec::new();
            let _ = st.export(&mut buf);
            let n = buf.len();
            let _ = inject_truncate(&mut buf, n - 1);
            st.note_injected();
            match PolicyStore::import(&buf) {
                Err(CfgCode::LenBad) => {
                    caught += 1;
                    st.note_detected();
                }
                _ => st.note_escaped(),
            }
        }
        s.add_dyn("截断注入 10/10", caught == 10, "载荷截断必须被长度判据全抓");
    }
    // 9 判定次序：魔数先于长度（坏魔数+截断同传报魔数）；校验和先于字段界。
    {
        let mut st9 = store_with(1);
        let mut buf: Vec<u8> = Vec::new();
        let _ = st9.export(&mut buf);
        let _ = inject_magic(&mut buf);
        let n = buf.len();
            let _ = inject_truncate(&mut buf, n - 1);
        let r1 = PolicyStore::import(&buf);
        let mut rec = make_raw_record(1, CREDITS_MAX + 1, 32, 100);
        // 追加破坏校验和：末字节翻转。
        let last = rec.len() - 1;
        let _ = inject_flip(&mut rec, last);
        let mut buf2: Vec<u8> = Vec::new();
        push_u32(&mut buf2, EXPORT_MAGIC);
        push_u32(&mut buf2, 1);
        for b in rec.iter() {
            buf2.push(*b);
        }
        let r2 = PolicyStore::import(&buf2);
        s.add_dyn(
            "判序魔数先于长度、校验和先于字段",
            r1 == Err(CfgCode::MagicBad) && r2 == Err(CfgCode::RecordCorrupt),
            "坏魔数+截断报魔数、坏校验报损坏（先于字段界）",
        );
    }
    // 10 状态机拒止：版本回退与空跳全部显性拒。
    {
        let mut st = store_with(3);
        let r1 = st.try_set_version(2);
        let r2 = st.try_set_version(9);
        s.add_dyn(
            "版本单调拒止",
            r1 == Err(CfgCode::VersionRegress) && r2 == Err(CfgCode::VersionRegress),
            "版本只进不退、不可虚跳",
        );
    }
    // 11 容量耗尽：满库第 17 条拒。
    {
        let mut st = PolicyStore::new();
        let mut last = Ok(0u32);
        for i in 0..MAX_POLICIES {
            last = st.commit(1 + (i as i64) % TIER_MAX as i64, 64, 32, 100);
        }
        let overflow = st.commit(1i64, 64, 32, 100);
        s.add_dyn(
            "容量耗尽拒",
            last.is_ok() && overflow == Err(CfgCode::StoreFull),
            "满库后再提交必须拒",
        );
    }
    // 12 导出字节账：账面 = 实写 = 8 + n×20。
    {
        let mut st = store_with(5);
        let mut buf: Vec<u8> = Vec::new();
        let written = match st.export(&mut buf) {
            Ok(n) => n,
            Err(_) => 0,
        };
        let expected = HEADER_BYTES + 5 * RECORD_BYTES;
        s.add_dyn(
            "导出字节账对平",
            written == expected && st.byte_account() == expected && buf.len() == expected,
            "账面、实写、缓冲长度三方一致",
        );
    }
    // 13 遥测接口对账 ×10：锚定行零改写。
    {
        let mut st = store_with(3);
        let snap_before = st.anchor_snapshot();
        let mut stable = true;
        for _ in 0..10u32 {
            let mut buf: Vec<u8> = Vec::new();
            if st.export(&mut buf).is_err() {
                stable = false;
            }
            if PolicyStore::import(&buf).is_err() {
                stable = false;
            }
            if st.anchor_snapshot() != snap_before {
                stable = false;
            }
        }
        s.add_dyn(
            "对账 ×10 锚定行零改写",
            stable && st.anchor_count() == 3,
            "十轮导出恢复对账，J5 锚定行逐字段不变",
        );
    }
    // 14 锚定行改写注入 ×10 全拒 + 账面回退注入 ×10 全拒。
    {
        let mut st = store_with(3);
        let mut buf14: Vec<u8> = Vec::new();
        let _ = st.export(&mut buf14);
        let mut caught = 0u32;
        for k in 0..10u32 {
            st.note_injected();
            if st.attempt_anchor_rewrite(0, 0x1234 + k as u64) == Err(CfgCode::AnchorLocked) {
                caught += 1;
                st.note_detected();
            } else {
                st.note_escaped();
            }
        }
        let mut rewound = 0u32;
        for k in 0..10u32 {
            st.note_injected();
            if st.try_rewind_account((k + 1) as usize) == Err(CfgCode::AccountRegress) {
                rewound += 1;
                st.note_detected();
            } else {
                st.note_escaped();
            }
        }
        s.add_dyn(
            "锚定改写与账面回退全拒",
            caught == 10 && rewound == 10 && st.conserved(),
            "零改写红线与账面单调全部守住",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑逐字节一致", dual_run_consistent(), "同一提交序列两次导出全等");
    // 16 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 17 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [CfgCode; 18] = [
    CfgCode::Ok,
    CfgCode::SegBad,
    CfgCode::TierNegative,
    CfgCode::TierZero,
    CfgCode::TierOutOfRange,
    CfgCode::CreditsOutOfRange,
    CfgCode::BurstZero,
    CfgCode::BurstOutOfRange,
    CfgCode::LatencyOutOfRange,
    CfgCode::StoreFull,
    CfgCode::VersionRegress,
    CfgCode::MagicBad,
    CfgCode::LenBad,
    CfgCode::RecordCorrupt,
    CfgCode::FieldOutOfRange,
    CfgCode::AccountRegress,
    CfgCode::AnchorLocked,
    CfgCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_versions_monotone_and_roundtrip() {
        let mut st = store_with(3);
        assert_eq!(st.version(), 3);
        let mut buf: Vec<u8> = Vec::new();
        assert!(st.export(&mut buf).is_ok());
        assert_eq!(PolicyStore::import(&buf), Ok(3));
    }

    #[test]
    fn zero_and_negative_bounds() {
        let mut st = PolicyStore::new();
        assert!(st.commit(1i64, 0, 32, 100).is_ok());
        assert_eq!(st.commit(0i64, 0, 32, 100), Err(CfgCode::TierZero));
        assert_eq!(st.commit(-3i64, 0, 32, 100), Err(CfgCode::TierNegative));
        assert_eq!(st.commit(1i64, 0, 0, 100), Err(CfgCode::BurstZero));
    }

    #[test]
    fn upper_bounds_accepted_and_exceeded_rejected() {
        let mut st = PolicyStore::new();
        assert!(st.commit(TIER_MAX as i64, CREDITS_MAX, BURST_MAX, LAT_MAX).is_ok());
        assert_eq!(
            st.commit(1i64, CREDITS_MAX + 1, 32, 100),
            Err(CfgCode::CreditsOutOfRange)
        );
        assert_eq!(
            st.commit(1i64, 0, BURST_MAX + 1, 100),
            Err(CfgCode::BurstOutOfRange)
        );
    }

    #[test]
    fn flip_injection_caught_ten_of_ten() {
        let mut st = store_with(10);
        let mut caught = 0u32;
        for k in 0..10u32 {
            let mut buf: Vec<u8> = Vec::new();
            let _ = st.export(&mut buf);
            let _ = inject_flip(&mut buf, HEADER_BYTES + (k as usize) * RECORD_BYTES + 5);
            st.note_injected();
            if PolicyStore::import(&buf) == Err(CfgCode::RecordCorrupt) {
                caught += 1;
                st.note_detected();
            } else {
                st.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert!(st.conserved());
    }

    #[test]
    fn truncate_and_magic_caught() {
        let mut st = store_with(2);
        let mut buf: Vec<u8> = Vec::new();
        let _ = st.export(&mut buf);
        let n = buf.len();
        let _ = inject_truncate(&mut buf, n - 1);
        assert_eq!(PolicyStore::import(&buf), Err(CfgCode::LenBad));
        let mut buf2: Vec<u8> = Vec::new();
        let _ = st.export(&mut buf2);
        let _ = inject_magic(&mut buf2);
        assert_eq!(PolicyStore::import(&buf2), Err(CfgCode::MagicBad));
    }

    #[test]
    fn judge_order_magic_before_len_checksum_before_field() {
        let mut st = store_with(1);
        let mut buf: Vec<u8> = Vec::new();
        let _ = st.export(&mut buf);
        let _ = inject_magic(&mut buf);
        let n = buf.len();
            let _ = inject_truncate(&mut buf, n - 1);
        assert_eq!(PolicyStore::import(&buf), Err(CfgCode::MagicBad));
        let mut rec = make_raw_record(1, CREDITS_MAX + 1, 32, 100);
        let nrec = rec.len();
        let last = nrec - 1;
        let _ = inject_flip(&mut rec, last);
        let mut buf2: Vec<u8> = Vec::new();
        push_u32(&mut buf2, EXPORT_MAGIC);
        push_u32(&mut buf2, 1);
        for b in rec.iter() {
            buf2.push(*b);
        }
        assert_eq!(PolicyStore::import(&buf2), Err(CfgCode::RecordCorrupt));
    }

    #[test]
    fn version_regress_rejected() {
        let mut st = store_with(2);
        assert_eq!(st.try_set_version(1), Err(CfgCode::VersionRegress));
        assert_eq!(st.try_set_version(5), Err(CfgCode::VersionRegress));
        assert_eq!(st.try_set_version(2), Ok(2));
    }

    #[test]
    fn anchor_rows_immutable_across_ten_rounds() {
        let mut st = store_with(3);
        let before = st.anchor_snapshot();
        for _ in 0..10u32 {
            let mut buf: Vec<u8> = Vec::new();
            assert!(st.export(&mut buf).is_ok());
            assert_eq!(PolicyStore::import(&buf), Ok(3));
        }
        assert_eq!(st.anchor_snapshot(), before);
        assert_eq!(st.attempt_anchor_rewrite(0, 1), Err(CfgCode::AnchorLocked));
    }

    #[test]
    fn store_capacity_exhausted() {
        let mut st = PolicyStore::new();
        for i in 0..MAX_POLICIES {
            assert!(st.commit(1 + (i as i64) % TIER_MAX as i64, 64, 32, 100).is_ok());
        }
        assert_eq!(st.commit(1i64, 64, 32, 100), Err(CfgCode::StoreFull));
    }

    #[test]
    fn byte_account_matches_actual() {
        let mut st = store_with(4);
        let mut buf: Vec<u8> = Vec::new();
        let written = match st.export(&mut buf) {
            Ok(n) => n,
            Err(_) => 0,
        };
        assert_eq!(written, HEADER_BYTES + 4 * RECORD_BYTES);
        assert_eq!(st.byte_account(), written);
        assert_eq!(buf.len(), written);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
