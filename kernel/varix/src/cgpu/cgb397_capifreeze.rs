//! CGPU-F0397 · C 域 API 总冻结 v1.0（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 总冻结：C 域 API 全集合并总册（C01-C05 冻结签名——账户/申请/仲裁/
//!   交易/配方/预测六族）；十年承诺（v1.0 向后兼容——十四章接口纪律域级化）；
//!   废弃流程（两版过渡+公告）；与 B 域接口交叉引用核对。
//! - 判据：总册、十年承诺、废弃流程、交叉核对、判据。
//!
//! 设计纪律：
//! - 六族签名收敛为单一冻结总册：每条签名带「冻结版本 + 家族 + 稳定等级」，
//!   任何签名变更走版本化而非原地改（向后兼容的形式化）。
//! - 十年承诺 = 机器可检查的兼容契约：v1.0 后允许「只增不改不删」，
//!   校验器对新册做增/改/删三分类，改与删一律拒绝。
//! - 废弃流程两版过渡：Announced →（下一版）→ Deprecated →（再下一版）→
//!   可移除；公告文本随签名入册，不许静默移除。
//! - 与 B 域交叉引用：本域暴露给 B 域的每一个名字必须在 B 域侧有对端
//!   引用声明；对端缺失即交叉核对红。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C00 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C00，批三起点，全仓独占）。
pub const SEG_BASE: u16 = 0x0C00;

pub const E_FREEZE_DUP: u16 = SEG_BASE + 1; // 总册内签名重名
pub const E_FREEZE_MUTATE: u16 = SEG_BASE + 2; // 冻结面被改/删（违反十年承诺）
pub const E_DEPREC_SHORT: u16 = SEG_BASE + 3; // 废弃过渡不足两版
pub const E_XREF_MISSING: u16 = SEG_BASE + 4; // B 域对端引用缺失
pub const E_XREF_STALE: u16 = SEG_BASE + 5; // B 域对端指向已不存在的名字
pub const E_FREEZE_ORDER: u16 = SEG_BASE + 6; // 废弃状态机非法跃迁

/// 冻结版本号（v1.0 起，单调递增的小版本）。
pub const FREEZE_VERSION: u32 = 10; // v1.0 编码为 10（v1.N → 10+N）
/// 十年承诺适用起始版本：自此版起只增不改不删。
pub const TENURE_SINCE: u32 = 10;

/// 签名稳定等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stability {
    /// v1.0 冻结：十年承诺保护。
    Frozen,
    /// 已公告废弃：仍有完整功能，新代码禁用。
    Announced,
    /// 二版废弃：功能保留但进入移除倒计时。
    Deprecated,
}

/// 废弃状态机合法跃迁：Frozen→Announced→Deprecated；不可逆、不可跳级。
fn deprec_transition_legal(from: Stability, to: Stability) -> bool {
    matches!(
        (from, to),
        (Stability::Frozen, Stability::Announced)
            | (Stability::Announced, Stability::Deprecated)
    )
}

/// 一条冻结签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenSig {
    /// 全域唯一名（家族前缀.名，如 acct.open / arb.submit）。
    pub name: String,
    /// 所属族：0 账户 / 1 申请 / 2 仲裁 / 3 交易 / 4 配方 / 5 预测。
    pub family: u8,
    /// 冻结时的版本。
    pub frozen_at: u32,
    pub stability: Stability,
    /// 公告文本（废弃时必填；冻结时为空）。
    pub notice: String,
    /// 签名指纹（名字+族的确定性摘要——交叉核对的对账键）。
    pub fingerprint: u64,
}

impl FrozenSig {
    pub fn new(name: &str, family: u8, version: u32) -> Self {
        FrozenSig {
            fingerprint: fp64(name.as_bytes(), family),
            name: name.to_string(),
            family,
            frozen_at: version,
            stability: Stability::Frozen,
            notice: String::new(),
        }
    }

    /// 签名是否在十年承诺保护期内（v1.0 冻结且未走完废弃流程）。
    pub fn tenure_protected(&self) -> bool {
        self.frozen_at >= TENURE_SINCE && self.stability != Stability::Deprecated
    }
}

/// 确定性指纹：FNV-1a 64 位，混入族号（同族同名才可能同指纹）。
pub fn fp64(bytes: &[u8], family: u8) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325 ^ (family as u64) << 8;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

/// C 域 API 总冻结册。
#[derive(Clone, Debug)]
pub struct FreezeRegistry {
    pub version: u32,
    pub sigs: Vec<FrozenSig>,
    /// B 域交叉引用表：本域名 → B 域对端声明名。
    pub b_refs: Vec<(String, String)>,
    /// 废弃公告链（按公告版本归档，审计用）。
    pub announcements: Vec<(u32, String)>,
}

impl FreezeRegistry {
    pub fn new(version: u32) -> Self {
        FreezeRegistry {
            version,
            sigs: Vec::new(),
            b_refs: Vec::new(),
            announcements: Vec::new(),
        }
    }

    /// 冻结一条新签名（只增——十年承诺下唯一合法的变更类别）。
    pub fn freeze(&mut self, name: &str, family: u8) -> Result<(), u16> {
        if self.sigs.iter().any(|s| s.name == name) {
            return Err(E_FREEZE_DUP);
        }
        self.sigs.push(FrozenSig::new(name, family, self.version));
        Ok(())
    }

    /// 六族冻结齐全性：每族至少一条签名（总册完整性下限）。
    pub fn six_families_covered(&self) -> bool {
        (0..6).all(|f| self.sigs.iter().any(|s| s.family as usize == f))
    }

    /// 十年承诺校验：对「旧册 → 新册」做增/改/删三分类。
    /// 只允许增；签名改（指纹变）与删一律 E_FREEZE_MUTATE。
    pub fn tenure_check(old: &FreezeRegistry, new: &FreezeRegistry) -> Result<(), u16> {
        if new.version < old.version {
            return Err(E_FREEZE_MUTATE);
        }
        for o in &old.sigs {
            match new.sigs.iter().find(|n| n.name == o.name) {
                None => return Err(E_FREEZE_MUTATE), // 删
                Some(n) => {
                    if n.fingerprint != o.fingerprint || n.family != o.family {
                        return Err(E_FREEZE_MUTATE); // 改
                    }
                    // 稳定等级只允许沿废弃状态机正向走。
                    if n.stability != o.stability
                        && !deprec_transition_legal(o.stability, n.stability)
                    {
                        return Err(E_FREEZE_ORDER);
                    }
                }
            }
        }
        Ok(())
    }

    /// 废弃公告：签名进入 Announced，公告文本入链。
    /// 两版过渡承诺：Deprecated 只能在公告版本 +2 之后被置位。
    pub fn announce_deprecation(&mut self, name: &str, notice: &str) -> Result<(), u16> {
        let ver = self.version;
        let sig = self
            .sigs
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or(E_XREF_MISSING)?;
        if !deprec_transition_legal(sig.stability, Stability::Announced) {
            return Err(E_FREEZE_ORDER);
        }
        if notice.is_empty() {
            return Err(E_DEPREC_SHORT);
        }
        sig.stability = Stability::Announced;
        sig.notice = notice.to_string();
        self.announcements.push((ver, name.to_string()));
        Ok(())
    }

    /// 二版废弃：公告后满两版才允许 Deprecated。
    pub fn advance_deprecation(&mut self, name: &str) -> Result<(), u16> {
        let announced_ver = self
            .announcements
            .iter()
            .find(|(_, n)| n == name)
            .map(|(v, _)| *v)
            .ok_or(E_DEPREC_SHORT)?;
        if self.version < announced_ver + 2 {
            return Err(E_DEPREC_SHORT); // 过渡期未满两版
        }
        let sig = self
            .sigs
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or(E_XREF_MISSING)?;
        if !deprec_transition_legal(sig.stability, Stability::Deprecated) {
            return Err(E_FREEZE_ORDER);
        }
        sig.stability = Stability::Deprecated;
        Ok(())
    }

    /// 登记 B 域交叉引用（本域名 → B 域对端名）。
    pub fn bind_b_ref(&mut self, local: &str, b_side: &str) -> Result<(), u16> {
        if !self.sigs.iter().any(|s| s.name == local) {
            return Err(E_XREF_STALE);
        }
        self.b_refs.push((local.to_string(), b_side.to_string()));
        Ok(())
    }

    /// 交叉核对：① 每条 B 引用的本域名必须仍在册（不悬空）；
    /// ② 对端名非空；③ 被引用签名若已 Deprecated，对端必须有迁移公告。
    pub fn cross_check(&self) -> Result<(), u16> {
        for (local, b_side) in &self.b_refs {
            let sig = self
                .sigs
                .iter()
                .find(|s| &s.name == local)
                .ok_or(E_XREF_STALE)?;
            if b_side.is_empty() {
                return Err(E_XREF_MISSING);
            }
            if sig.stability == Stability::Deprecated {
                let migrated = self
                    .announcements
                    .iter()
                    .any(|(_, n)| n == local);
                if !migrated {
                    return Err(E_XREF_MISSING);
                }
            }
        }
        // 反向：在册签名若声明过 B 引用但对端名与登记表不一致即悬空。
        for s in &self.sigs {
            if !s.notice.is_empty()
                && !self.b_refs.iter().any(|(l, _)| l == &s.name)
                && s.stability == Stability::Deprecated
            {
                return Err(E_XREF_STALE);
            }
        }
        Ok(())
    }

    /// 总册快照：六族计数 + 冻结/公告/二版废弃三档计数（收口报告素材）。
    pub fn snapshot(&self) -> ([usize; 6], [usize; 3]) {
        let mut by_family = [0usize; 6];
        let mut by_stab = [0usize; 3];
        for s in &self.sigs {
            if (s.family as usize) < 6 {
                by_family[s.family as usize] += 1;
            }
            by_stab[match s.stability {
                Stability::Frozen => 0,
                Stability::Announced => 1,
                Stability::Deprecated => 2,
            }] += 1;
        }
        (by_family, by_stab)
    }
}

/// 十年承诺文本（域级化——十四章接口纪律在 C 域的落点）。
pub const TENURE_PLEDGE: &str = "v1.0 冻结面十年向后兼容：只增不改不删；\
废弃走两版过渡+公告；对 B 域交叉引用永不悬空。";

/// 冻结面语义版本推进：小版本只允许加签名/推进废弃状态机。
pub fn bump_version(reg: &mut FreezeRegistry) -> u32 {
    reg.version += 1;
    reg.version
}

// ==========================================================================
// 判据测试（a01 起按判据关键词逐条落名）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn seed_registry() -> FreezeRegistry {
        let mut r = FreezeRegistry::new(FREEZE_VERSION);
        // 六族各冻结两条：账户/申请/仲裁/交易/配方/预测。
        let names = [
            ("acct.open", 0u8),
            ("acct.close", 0),
            ("req.submit", 1),
            ("req.cancel", 1),
            ("arb.arbitrate", 2),
            ("arb.rebalance", 2),
            ("txn.grant", 3),
            ("txn.rollback", 3),
            ("recipe.apply", 4),
            ("recipe.validate", 4),
            ("pred.forecast", 5),
            ("pred.reserve", 5),
        ];
        for (n, f) in names {
            r.freeze(n, f).ok();
        }
        r
    }

    #[test]
    fn a01_总册_六族合并与重名拒绝() {
        let r = seed_registry();
        // 六族齐全。
        assert!(r.six_families_covered());
        let (fam, stab) = r.snapshot();
        assert_eq!(fam, [2, 2, 2, 2, 2, 2]);
        assert_eq!(stab, [12, 0, 0]);
        // 重名冻结拒绝。
        let mut r2 = seed_registry();
        assert_eq!(r2.freeze("acct.open", 0), Err(E_FREEZE_DUP));
        // 同名不同族也不允许（名字全域唯一）。
        assert_eq!(r2.freeze("acct.open", 3), Err(E_FREEZE_DUP));
    }

    #[test]
    fn a02_十年承诺_只增不改不删() {
        let old = seed_registry();
        // 合法：只增。
        let mut ok = old.clone();
        bump_version(&mut ok);
        ok.freeze("acct.query", 0).ok();
        assert_eq!(FreezeRegistry::tenure_check(&old, &ok), Ok(()));
        // 非法：删。
        let mut del = ok.clone();
        del.sigs.retain(|s| s.name != "txn.grant");
        assert_eq!(
            FreezeRegistry::tenure_check(&old, &del),
            Err(E_FREEZE_MUTATE)
        );
        // 非法：改指纹（同名单方面换族/换指纹）。
        let mut mutate = ok.clone();
        if let Some(s) = mutate.sigs.iter_mut().find(|s| s.name == "txn.grant") {
            s.fingerprint ^= 0xFF;
        }
        assert_eq!(
            FreezeRegistry::tenure_check(&old, &mutate),
            Err(E_FREEZE_MUTATE)
        );
        // 版本回退视为变更（v1.0 之下即违约）。
        let mut back = ok.clone();
        back.version = old.version.saturating_sub(1);
        assert_eq!(
            FreezeRegistry::tenure_check(&old, &back),
            Err(E_FREEZE_MUTATE)
        );
        // 保护期语义：v1.0 冻结且未二版废弃 ⇒ 受保护。
        assert!(old.sigs.iter().all(|s| s.tenure_protected()));
        assert_eq!(TENURE_SINCE, FREEZE_VERSION);
    }

    #[test]
    fn a03_废弃流程_两版过渡加公告() {
        let mut r = seed_registry();
        // 公告（公告文本必填）。
        assert_eq!(r.announce_deprecation("acct.close", "改用 acct.open2"), Ok(()));
        assert_eq!(
            r.announce_deprecation("acct.open", ""),
            Err(E_DEPREC_SHORT)
        );
        // 未满两版不允许进二版废弃。
        let mut r1 = r.clone();
        bump_version(&mut r1); // v1.1：只过了一版
        assert_eq!(r1.advance_deprecation("acct.close"), Err(E_DEPREC_SHORT));
        // 满两版放行。
        bump_version(&mut r1); // v1.2
        assert_eq!(r1.advance_deprecation("acct.close"), Ok(()));
        // 状态机：Deprecated 不可再跃迁，公告不可重放。
        let mut r2 = r1.clone();
        assert_eq!(
            r2.announce_deprecation("acct.close", "again"),
            Err(E_FREEZE_ORDER)
        );
        // 未公告直接跳 Deprecated：tenure_check 的状态机拒绝。
        let mut skip = r.clone();
        if let Some(s) = skip.sigs.iter_mut().find(|s| s.name == "acct.open") {
            s.stability = Stability::Deprecated;
        }
        assert_eq!(
            FreezeRegistry::tenure_check(&r, &skip),
            Err(E_FREEZE_ORDER)
        );
        // 公告链留痕。
        assert!(r.announcements.iter().any(|(v, n)| *v == FREEZE_VERSION && n == "acct.close"));
    }

    #[test]
    fn a04_交叉核对_对端存在不悬空() {
        let mut r = seed_registry();
        // 正常绑定 + 核对绿。
        assert_eq!(r.bind_b_ref("arb.arbitrate", "b08.bus.arb"), Ok(()));
        assert_eq!(r.cross_check(), Ok(()));
        // 本域名不存在 ⇒ 引用悬空。
        assert_eq!(
            r.bind_b_ref("ghost.sig", "b08.ghost"),
            Err(E_XREF_STALE)
        );
        // 对端名空 ⇒ 缺失。
        let mut r2 = seed_registry();
        r2.bind_b_ref("txn.grant", "").ok();
        assert_eq!(r2.cross_check(), Err(E_XREF_MISSING));
        // 已二版废弃且公告在册 ⇒ 迁移公告成立 ⇒ 核对绿。
        let mut r3 = seed_registry();
        r3.bind_b_ref("acct.close", "b05.acct.close").ok();
        r3.announce_deprecation("acct.close", "migrate to acct.open2").ok();
        bump_version(&mut r3);
        bump_version(&mut r3);
        r3.advance_deprecation("acct.close").ok();
        assert_eq!(r3.cross_check(), Ok(()));
        // 反例：Deprecated 但公告链为空 ⇒ 对端缺失红。
        let mut r4 = seed_registry();
        r4.bind_b_ref("acct.close", "b05.acct.close").ok();
        if let Some(s) = r4.sigs.iter_mut().find(|s| s.name == "acct.close") {
            s.stability = Stability::Deprecated;
        }
        assert_eq!(r4.cross_check(), Err(E_XREF_MISSING));
    }

    #[test]
    fn a05_指纹确定性与快照可复算() {
        // 同名同族同指纹；族不同指纹不同。
        assert_eq!(fp64(b"txn.grant", 3), fp64(b"txn.grant", 3));
        assert_ne!(fp64(b"txn.grant", 3), fp64(b"txn.grant", 4));
        // FrozenSig::new 与独立 fp64 一致（对账键同源）。
        let s = FrozenSig::new("pred.forecast", 5, FREEZE_VERSION);
        assert_eq!(s.fingerprint, fp64(b"pred.forecast", 5));
        // 十年承诺文本非空（收口报告引用）。
        assert!(TENURE_PLEDGE.contains("十年"));
    }
}
