//! CGPU-F0713 · 滚动 API 冻结 v1（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0713，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动 API 冻结 v1——对滚动子系统对外的 API 面做
//!   登记、指纹、冻结与演进比对：冻结项不得删改，新增须报备。
//!
//! 设计纪律：
//! - API 面闭集：条目类别枚举（函数/结构体/枚举/常量/诊断码），
//!   稳定层级三档（Frozen/Stable/Experimental）。
//! - 指纹：FNV-1a 64 对（名称+类别+签名串）哈希，冻结比对只认指纹。
//! - 演进闭集：差异类别枚举（新增/删除/签名变更/层级降级），
//!   冻结门槛：Frozen 层级出现删除或签名变更即拒绝冻结通过。
//! - 清单守恒：登记数 == 新增数 + 保留数（跨版本对账恒等式）。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x4760 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4760，批内 CGPU-F0713 独占）。
pub const SEG_BASE: u16 = 0x4760;

/// API 条目重名。
pub const E_DUP_NAME: u16 = SEG_BASE + 1;
/// 条目名非法（空名/超长）。
pub const E_BAD_NAME: u16 = SEG_BASE + 2;
/// 冻结门槛被击穿（Frozen 项被删改）。
pub const E_FROZEN_BROKEN: u16 = SEG_BASE + 3;
/// 版本号非法（主版本回退）。
pub const E_VERSION: u16 = SEG_BASE + 4;
/// 对账不平（旧条目数 ≠ 新增 + 保留）。
pub const E_LEDGER: u16 = SEG_BASE + 5;
/// 清单为空（冻结一个空面无意义）。
pub const E_EMPTY: u16 = SEG_BASE + 6;

/// 冻结版主版本号（v1）。
pub const FROZEN_MAJOR: u32 = 1;
/// 名称长度上限。
pub const NAME_MAX: usize = 48;

/// FNV-1a 64 指纹。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// API 条目类别（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiKind {
    /// 函数。
    Fn,
    /// 结构体。
    Struct,
    /// 枚举。
    Enum,
    /// 常量。
    Const,
    /// 诊断错误码。
    ErrCode,
}

/// 稳定层级（三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 冻结：不可删改。
    Frozen,
    /// 稳定：可加不可删。
    Stable,
    /// 实验性：可任意演进。
    Experimental,
}

/// API 条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiEntry {
    /// 名称（如 "scroll_con_tick"）。
    pub name: String,
    /// 类别。
    pub kind: ApiKind,
    /// 签名串（如 "fn(&mut ScrollCon) -> Result<u32,u16>"）。
    pub sig: String,
    /// 稳定层级。
    pub tier: Tier,
}

impl ApiEntry {
    /// 条目指纹：FNV-1a(名称|类别序|签名)。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.name.as_bytes());
        bytes.push(self.kind_ord());
        bytes.extend_from_slice(self.sig.as_bytes());
        fnv1a64(&bytes)
    }
    /// 类别序号（指纹与导出用）。
    pub fn kind_ord(&self) -> u8 {
        match self.kind {
            ApiKind::Fn => 0,
            ApiKind::Struct => 1,
            ApiKind::Enum => 2,
            ApiKind::Const => 3,
            ApiKind::ErrCode => 4,
        }
    }
}

/// API 清单（一个版本的完整面）。
#[derive(Clone, Debug)]
pub struct ApiManifest {
    /// 版本号（major*1000+minor）。
    pub ver: u32,
    /// 条目表（按登记序）。
    entries: Vec<ApiEntry>,
}

impl ApiManifest {
    /// 空清单。
    pub fn new(ver: u32) -> Result<Self, u16> {
        if ver / 1000 < FROZEN_MAJOR {
            return Err(E_VERSION);
        }
        Ok(Self {
            ver,
            entries: Vec::new(),
        })
    }

    /// 登记条目：名合法 + 不重名。
    pub fn add(&mut self, e: ApiEntry) -> Result<(), u16> {
        if e.name.is_empty() || e.name.len() > NAME_MAX {
            return Err(E_BAD_NAME);
        }
        if self.entries.iter().any(|x| x.name == e.name) {
            return Err(E_DUP_NAME);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空面判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全面指纹：按名排序后逐条指纹折叠（版本无关的面前缀）。
    pub fn surface_hash(&self) -> u64 {
        let mut fps: Vec<(String, u64)> = self
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.fingerprint()))
            .collect();
        // 插入排序按名（n 小，确定性）。
        for i in 1..fps.len() {
            let mut j = i;
            while j > 0 && fps[j - 1].0 < fps[j].0 {
                fps.swap(j - 1, j);
                j -= 1;
            }
        }
        let mut h: u64 = 0x9e37_79b9_7f4a_7c15;
        for (_, f) in &fps {
            h ^= *f;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 按名查条目。
    pub fn find(&self, name: &str) -> Option<&ApiEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 只读条目视图。
    pub fn view(&self) -> &[ApiEntry] {
        &self.entries
    }
}

/// 差异类别（演进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delta {
    /// 新增条目。
    Added,
    /// 删除条目。
    Removed,
    /// 签名或类别变更（指纹变了名字没变）。
    Changed,
    /// 层级降级（Frozen→其它）。
    Downgraded,
}

/// 单条差异记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRec {
    /// 条目名。
    pub name: String,
    /// 差异类别。
    pub d: Delta,
    /// 旧层级（Removed/Changed 时有效，否则 None）。
    pub old_tier: Option<Tier>,
}

/// 跨版本对账恒等式（显式）：
///   old.len() == added + kept
///   new.len() == old.len() + added - removed
pub fn diff(old: &ApiManifest, new: &ApiManifest) -> Result<Vec<DiffRec>, u16> {
    if old.is_empty() || new.is_empty() {
        return Err(E_EMPTY);
    }
    let mut out: Vec<DiffRec> = Vec::new();
    // 删除与变更：遍历旧面。
    for e in old.view() {
        match new.find(&e.name) {
            None => out.push(DiffRec {
                name: e.name.clone(),
                d: Delta::Removed,
                old_tier: Some(e.tier),
            }),
            Some(n) => {
                if n.fingerprint() != e.fingerprint() {
                    out.push(DiffRec {
                        name: e.name.clone(),
                        d: Delta::Changed,
                        old_tier: Some(e.tier),
                    });
                }
                if e.tier == Tier::Frozen && n.tier != Tier::Frozen {
                    out.push(DiffRec {
                        name: e.name.clone(),
                        d: Delta::Downgraded,
                        old_tier: Some(e.tier),
                    });
                }
            }
        }
    }
    // 新增：遍历新面。
    for e in new.view() {
        if old.find(&e.name).is_none() {
            out.push(DiffRec {
                name: e.name.clone(),
                d: Delta::Added,
                old_tier: None,
            });
        }
    }
    Ok(out)
}

/// 冻结门槛：Frozen 项不得 Removed/Changed/Downgraded。
pub fn freeze_gate(old: &ApiManifest, new: &ApiManifest) -> Result<u64, u16> {
    let ds = diff(old, new)?;
    for d in &ds {
        if d.old_tier == Some(Tier::Frozen)
            && matches!(
                d.d,
                Delta::Removed | Delta::Changed | Delta::Downgraded
            )
        {
            return Err(E_FROZEN_BROKEN);
        }
    }
    // 对账恒等式。
    let added = ds.iter().filter(|d| d.d == Delta::Added).count();
    let removed = ds.iter().filter(|d| d.d == Delta::Removed).count();
    let kept = old.len() - removed;
    if old.len() != added + kept {
        return Err(E_LEDGER);
    }
    if new.len() != old.len() + added - removed {
        return Err(E_LEDGER);
    }
    Ok(new.surface_hash())
}

/// 差异汇总行（报告用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeltaStat {
    /// 新增数。
    pub added: u32,
    /// 删除数。
    pub removed: u32,
    /// 变更数。
    pub changed: u32,
    /// 降级数。
    pub downgraded: u32,
}

/// 差异统计。
pub fn delta_stat(ds: &[DiffRec]) -> DeltaStat {
    DeltaStat {
        added: ds.iter().filter(|d| d.d == Delta::Added).count() as u32,
        removed: ds.iter().filter(|d| d.d == Delta::Removed).count() as u32,
        changed: ds.iter().filter(|d| d.d == Delta::Changed).count() as u32,
        downgraded: ds.iter().filter(|d| d.d == Delta::Downgraded).count() as u32,
    }
}

/// 便捷构造条目。
pub fn ent(name: &str, kind: ApiKind, sig: &str, tier: Tier) -> ApiEntry {
    ApiEntry {
        name: String::from(name),
        kind,
        sig: String::from(sig),
        tier,
    }
}

/// 自例程：v0 实验面 → v1 冻结面 → v1.1 合法演进 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 错误路径 0：主版本回退（0.100 < 1）。
    if ApiManifest::new(0_100).err() != Some(E_VERSION) {
        return Err(E_VERSION);
    }
    // v0：三个实验条目。
    let mut v0 = ApiManifest::new(FROZEN_MAJOR * 1000)?;
    v0.add(ent("scroll_arm", ApiKind::Fn, "fn(&mut ScrollCon,ScrollCmd)->Result<(),u16>", Tier::Experimental))?;
    v0.add(ent("ScrollCon", ApiKind::Struct, "struct ScrollCon{slots:[Option<SurfSlot>;16]}", Tier::Experimental))?;
    v0.add(ent("SCROLL_MAX_SURF", ApiKind::Const, "const u16 = 16", Tier::Experimental))?;
    // 错误路径 1：重名。
    if v0.add(ent("scroll_arm", ApiKind::Fn, "fn()", Tier::Experimental)) != Err(E_DUP_NAME) {
        return Err(E_DUP_NAME);
    }
    // 错误路径 2：空名。
    if v0.add(ent("", ApiKind::Fn, "fn()", Tier::Experimental)) != Err(E_BAD_NAME) {
        return Err(E_BAD_NAME);
    }
    // v1：同三项升级为 Frozen。
    let mut v1 = ApiManifest::new(FROZEN_MAJOR * 1000 + 1)?;
    v1.add(ent("scroll_arm", ApiKind::Fn, "fn(&mut ScrollCon,ScrollCmd)->Result<(),u16>", Tier::Frozen))?;
    v1.add(ent("ScrollCon", ApiKind::Struct, "struct ScrollCon{slots:[Option<SurfSlot>;16]}", Tier::Frozen))?;
    v1.add(ent("SCROLL_MAX_SURF", ApiKind::Const, "const u16 = 16", Tier::Frozen))?;
    let h_v1 = freeze_gate(&v0, &v1)?;
    // v1.1：新增一个 Stable 函数，其余不动 → 应通过。
    let mut v11 = ApiManifest::new(FROZEN_MAJOR * 1000 + 1)?;
    v11.add(ent("scroll_arm", ApiKind::Fn, "fn(&mut ScrollCon,ScrollCmd)->Result<(),u16>", Tier::Frozen))?;
    v11.add(ent("ScrollCon", ApiKind::Struct, "struct ScrollCon{slots:[Option<SurfSlot>;16]}", Tier::Frozen))?;
    v11.add(ent("SCROLL_MAX_SURF", ApiKind::Const, "const u16 = 16", Tier::Frozen))?;
    v11.add(ent("scroll_telem_pct", ApiKind::Fn, "fn(&ScrollTelemetry,u32)->Result<u32,u16>", Tier::Stable))?;
    let h_v11 = freeze_gate(&v1, &v11)?;
    if h_v11 == h_v1 {
        return Err(E_LEDGER); // 面变了指纹必须变
    }
    let ds = delta_stat(&diff(&v1, &v11)?);
    if ds.added != 1 || ds.removed != 0 || ds.changed != 0 {
        return Err(E_LEDGER);
    }
    // 错误路径 3：改 Frozen 签名 → 门槛击穿。
    let mut v12 = ApiManifest::new(FROZEN_MAJOR * 1000 + 2)?;
    v12.add(ent("scroll_arm", ApiKind::Fn, "fn(&mut ScrollCon,ScrollCmd,u32)->Result<(),u16>", Tier::Frozen))?;
    v12.add(ent("ScrollCon", ApiKind::Struct, "struct ScrollCon{slots:[Option<SurfSlot>;16]}", Tier::Frozen))?;
    v12.add(ent("SCROLL_MAX_SURF", ApiKind::Const, "const u16 = 16", Tier::Frozen))?;
    if freeze_gate(&v1, &v12) != Err(E_FROZEN_BROKEN) {
        return Err(E_FROZEN_BROKEN);
    }
    // 错误路径 4：删 Frozen 条目。
    let mut v13 = ApiManifest::new(FROZEN_MAJOR * 1000 + 3)?;
    v13.add(ent("scroll_arm", ApiKind::Fn, "fn(&mut ScrollCon,ScrollCmd)->Result<(),u16>", Tier::Frozen))?;
    v13.add(ent("ScrollCon", ApiKind::Struct, "struct ScrollCon{slots:[Option<SurfSlot>;16]}", Tier::Frozen))?;
    if freeze_gate(&v1, &v13) != Err(E_FROZEN_BROKEN) {
        return Err(E_FROZEN_BROKEN);
    }
    // 错误路径 5：空面冻结。
    let empty = ApiManifest::new(FROZEN_MAJOR * 1000)?;
    if freeze_gate(&v1, &empty) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    Ok(0)
}
