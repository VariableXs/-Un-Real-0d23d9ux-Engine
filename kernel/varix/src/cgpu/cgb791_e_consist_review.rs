//! CGPU-F0791 · E 域一致性审查（目标 340 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0791，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域一致性审查——规格、实现、文档三类工件按主题键
//!   交叉对齐：同一主题键在三类工件中的版本摘要必须一致，
//!   不一致即记不一致项；审查账本定容守恒，结论闭集。
//!
//! 设计纪律：
//! - 工件类别闭集三类：Spec（规格）/ Impl（实现）/ Doc（文档）。
//! - 一致性口径：同一主题键下三类工件齐备且摘要两两相等 → 一致；
//!   缺任一类 → 记"缺口"；齐备但不等 → 记"漂移"；无第三种口径。
//! - 版本摘要：FNV-1a 64 位整数摘要（版本串 → u64），确定性。
//! - 审查账本定容：AUDIT_CAP=120 条，超容逐出最旧并计数，
//!   守恒恒等式：total_checked == held + evicted。
//! - 结论闭集三态：Aligned（全一致）/ Gapped（有缺口无漂移）/
//!   Drifted（有漂移）；漂移优先于缺口定级。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6E20 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6E20，批内 CGPU-F0791 独占）。
pub const SEG_BASE: u16 = 0x6E20;

/// 工件编号重复登记。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 工件库已满（防御位）。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 版本串为空。
pub const E_VER: u16 = SEG_BASE + 3;
/// 审查库为空。
pub const E_EMPTY: u16 = SEG_BASE + 4;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 5;
/// 未知主题键。
pub const E_NOKEY: u16 = SEG_BASE + 6;

/// 审查账本容量。
pub const AUDIT_CAP: usize = 120;

/// FNV-1a 64 位摘要。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 工件类别（闭集三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    Spec,
    Impl,
    Doc,
}

impl ArtifactKind {
    /// 类别序号（0..2，用于槽位数组）。
    pub fn slot(self) -> usize {
        match self {
            ArtifactKind::Spec => 0,
            ArtifactKind::Impl => 1,
            ArtifactKind::Doc => 2,
        }
    }
}

/// 工件登记项。
#[derive(Clone, Debug)]
pub struct Artifact {
    /// 主题键（如 "blend_semantics"）。
    pub key: String,
    /// 类别。
    pub kind: ArtifactKind,
    /// 版本摘要。
    pub digest: u64,
}

impl Artifact {
    pub fn new(key: &str, kind: ArtifactKind, version: &str) -> Result<Self, u16> {
        if key.is_empty() || version.is_empty() {
            return Err(E_VER);
        }
        Ok(Artifact {
            key: key.to_string(),
            kind,
            digest: fnv1a64(version.as_bytes()),
        })
    }
}

/// 工件库。
#[derive(Clone, Debug, Default)]
pub struct ArtifactStore {
    items: Vec<Artifact>,
}

impl ArtifactStore {
    pub fn new() -> Self {
        ArtifactStore { items: Vec::new() }
    }
    /// 登记：(key, kind) 组合唯一。
    pub fn put(&mut self, a: Artifact) -> Result<(), u16> {
        if self.items.iter().any(|o| o.key == a.key && o.kind == a.kind) {
            return Err(E_DUP);
        }
        self.items.push(a);
        Ok(())
    }
    /// 取某键某类的摘要。
    pub fn digest_of(&self, key: &str, kind: ArtifactKind) -> Result<Option<u64>, u16> {
        if key.is_empty() {
            return Err(E_NOKEY);
        }
        Ok(self
            .items
            .iter()
            .find(|o| o.key == key && o.kind == kind)
            .map(|o| o.digest))
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// 单主题审查结论（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyVerdict {
    /// 齐备且摘要两两相等。
    Aligned,
    /// 缺任一类工件。
    Gapped,
    /// 齐备但摘要漂移。
    Drifted,
}

impl KeyVerdict {
    pub fn ord(self) -> u8 {
        match self {
            KeyVerdict::Aligned => 0,
            KeyVerdict::Gapped => 1,
            KeyVerdict::Drifted => 2,
        }
    }
}

/// 审查记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRec {
    /// 主题键哈希（脱敏记录）。
    pub key_hash: u64,
    /// 结论。
    pub verdict: KeyVerdict,
}

/// 审查账本（环形定容）。
#[derive(Clone, Debug)]
pub struct ConsistLedger {
    ring: Vec<AuditRec>,
    evicted: u64,
    total: u64,
}

impl ConsistLedger {
    pub fn new() -> Self {
        ConsistLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, r: AuditRec) {
        if self.ring.len() >= AUDIT_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(r);
        self.total += 1;
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
}

/// 审查总表结论（闭集三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverallVerdict {
    Aligned,
    Gapped,
    Drifted,
}

/// 审查单个主题键。
pub fn audit_key(store: &ArtifactStore, key: &str) -> Result<KeyVerdict, u16> {
    let d0 = store.digest_of(key, ArtifactKind::Spec)?;
    let d1 = store.digest_of(key, ArtifactKind::Impl)?;
    let d2 = store.digest_of(key, ArtifactKind::Doc)?;
    let slots = [d0, d1, d2];
    let missing = slots.iter().filter(|s| s.is_none()).count();
    if missing == 3 {
        return Err(E_NOKEY);
    }
    if missing > 0 {
        return Ok(KeyVerdict::Gapped);
    }
    let a = slots[0].ok_or(E_NOKEY)?;
    let b = slots[1].ok_or(E_NOKEY)?;
    let c = slots[2].ok_or(E_NOKEY)?;
    if a == b && b == c {
        Ok(KeyVerdict::Aligned)
    } else {
        Ok(KeyVerdict::Drifted)
    }
}

/// 全库审查：对库内出现过的全部主题键逐个审并落账；
/// 返回总表结论（漂移优先）。
pub fn audit_all(store: &ArtifactStore, led: &mut ConsistLedger) -> Result<OverallVerdict, u16> {
    if store.len() == 0 {
        return Err(E_EMPTY);
    }
    // 收集去重键序（按首次出现序）。
    let mut keys: Vec<String> = Vec::new();
    for a in &store.items {
        if !keys.iter().any(|k| k == &a.key) {
            keys.push(a.key.clone());
        }
    }
    let mut has_drift = false;
    let mut has_gap = false;
    for k in &keys {
        let v = audit_key(store, k)?;
        led.push(AuditRec {
            key_hash: fnv1a64(k.as_bytes()),
            verdict: v,
        });
        match v {
            KeyVerdict::Drifted => has_drift = true,
            KeyVerdict::Gapped => has_gap = true,
            KeyVerdict::Aligned => {}
        }
    }
    Ok(if has_drift {
        OverallVerdict::Drifted
    } else if has_gap {
        OverallVerdict::Gapped
    } else {
        OverallVerdict::Aligned
    })
}

/// 自例程：主路径（全对齐）+ 缺口路径 + 漂移路径 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut s = ArtifactStore::new();
    // 主路径 1：键 "blend" 三类齐备且同版本 → Aligned。
    s.put(Artifact::new("blend", ArtifactKind::Spec, "v1")?)?;
    s.put(Artifact::new("blend", ArtifactKind::Impl, "v1")?)?;
    s.put(Artifact::new("blend", ArtifactKind::Doc, "v1")?)?;
    // 主路径 2：键 "raster" 缺 Doc → Gapped。
    s.put(Artifact::new("raster", ArtifactKind::Spec, "v2")?)?;
    s.put(Artifact::new("raster", ArtifactKind::Impl, "v2")?)?;
    // 主路径 3：键 "sync" 齐备但 Impl 漂移 → Drifted。
    s.put(Artifact::new("sync", ArtifactKind::Spec, "v3")?)?;
    s.put(Artifact::new("sync", ArtifactKind::Impl, "v9")?)?;
    s.put(Artifact::new("sync", ArtifactKind::Doc, "v3")?)?;
    // (key,kind) 重复登记拒绝。
    if s.put(Artifact::new("blend", ArtifactKind::Spec, "vX")?) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 单键审查。
    if audit_key(&s, "blend")? != KeyVerdict::Aligned {
        return Err(E_EMPTY);
    }
    if audit_key(&s, "raster")? != KeyVerdict::Gapped {
        return Err(E_EMPTY);
    }
    if audit_key(&s, "sync")? != KeyVerdict::Drifted {
        return Err(E_EMPTY);
    }
    // 全库审查 → Drifted（漂移优先）。
    let mut led = ConsistLedger::new();
    let ov = audit_all(&s, &mut led)?;
    if ov != OverallVerdict::Drifted {
        return Err(E_EMPTY);
    }
    led.audit()?;
    if led.held() != 3 {
        return Err(E_EMPTY);
    }
    // 去掉漂移键后 → Gapped。
    let mut s2 = ArtifactStore::new();
    s2.put(Artifact::new("blend", ArtifactKind::Spec, "v1")?)?;
    s2.put(Artifact::new("blend", ArtifactKind::Impl, "v1")?)?;
    s2.put(Artifact::new("blend", ArtifactKind::Doc, "v1")?)?;
    s2.put(Artifact::new("raster", ArtifactKind::Spec, "v2")?)?;
    s2.put(Artifact::new("raster", ArtifactKind::Impl, "v2")?)?;
    let mut led2 = ConsistLedger::new();
    if audit_all(&s2, &mut led2)? != OverallVerdict::Gapped {
        return Err(E_EMPTY);
    }
    // 全对齐库 → Aligned。
    let mut s3 = ArtifactStore::new();
    for k in ["a", "b"].iter() {
        s3.put(Artifact::new(k, ArtifactKind::Spec, "v1")?)?;
        s3.put(Artifact::new(k, ArtifactKind::Impl, "v1")?)?;
        s3.put(Artifact::new(k, ArtifactKind::Doc, "v1")?)?;
    }
    let mut led3 = ConsistLedger::new();
    if audit_all(&s3, &mut led3)? != OverallVerdict::Aligned {
        return Err(E_EMPTY);
    }
    // 错误路径 1：空版本串 / 空键。
    if Artifact::new("k", ArtifactKind::Spec, "") != Err(E_VER)
        || Artifact::new("", ArtifactKind::Spec, "v") != Err(E_VER)
    {
        return Err(E_VER);
    }
    // 错误路径 2：完全未知键（三类全缺）。
    if audit_key(&s, "no_such") != Err(E_NOKEY) {
        return Err(E_NOKEY);
    }
    // 错误路径 3：空库全审。
    let mut led4 = ConsistLedger::new();
    if audit_all(&ArtifactStore::new(), &mut led4) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 摘要确定性：同版本串同摘要，异版本串异摘要。
    let d1 = fnv1a64(b"v1");
    let d2 = fnv1a64(b"v1");
    let d3 = fnv1a64(b"v2");
    if d1 != d2 || d1 == d3 {
        return Err(E_VER);
    }
    // 定容守恒：灌 130 条进 120 容账本，恒等式仍成立。
    let mut led5 = ConsistLedger::new();
    for i in 0..130u64 {
        led5.push(AuditRec {
            key_hash: i + 1,
            verdict: KeyVerdict::Aligned,
        });
    }
    led5.audit()?;
    if led5.held() != AUDIT_CAP || led5.evicted != 10 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
