//! 深化层四 · F133 第三方图标包规范（2026-09-27 深化批次四 · g 层）。
//!
//! 包安装器状态机（校验→暂存→登记→激活，任一步失败回滚清场）、图标
//! 语义分类表、来源签名链、哈希分桶布局规划、回归采样计划。

use crate::checks::CheckSet;
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// 包安装器状态机：Validate→Staged→Registered→Active；失败→RolledBack
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstallStage {
    Validating,
    Staged,
    Registered,
    Active,
    RolledBack,
}

pub struct PackInstaller {
    pub pack: &'static str,
    pub stage: InstallStage,
}

impl PackInstaller {
    pub fn new(pack: &'static str) -> PackInstaller {
        PackInstaller { pack, stage: InstallStage::Validating }
    }

    pub fn advance(&mut self, to: InstallStage) -> Result<(), &'static str> {
        let legal = match (self.stage, to) {
            (InstallStage::Validating, InstallStage::Staged)
            | (InstallStage::Staged, InstallStage::Registered)
            | (InstallStage::Registered, InstallStage::Active) => true,
            (InstallStage::Validating, InstallStage::RolledBack)
            | (InstallStage::Staged, InstallStage::RolledBack)
            | (InstallStage::Registered, InstallStage::RolledBack) => true,
            _ => false,
        };
        if !legal {
            return Err("非法安装迁移");
        }
        self.stage = to;
        Ok(())
    }

    /// 回滚清场语义：RolledBack 是终态——已激活的包不可回滚（走卸载）。
    pub fn rollback_terminal(&self) -> bool {
        self.stage == InstallStage::RolledBack || self.stage == InstallStage::Active
    }
}

// ---------------------------------------------------------------------------
// 图标语义分类：前缀表驱动（action-/status-/mime-/emblem-）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconCategory {
    Action,
    Status,
    Mime,
    Emblem,
    Unclassified,
}

pub fn classify(name: &str) -> IconCategory {
    if name.starts_with("action-") {
        IconCategory::Action
    } else if name.starts_with("status-") {
        IconCategory::Status
    } else if name.starts_with("mime-") {
        IconCategory::Mime
    } else if name.starts_with("emblem-") {
        IconCategory::Emblem
    } else {
        IconCategory::Unclassified
    }
}

/// 分类覆盖审计：未分类数（规范要求全部入四族）。
pub fn unclassified_count(names: &[&'static str]) -> usize {
    names.iter().filter(|n| classify(n) == IconCategory::Unclassified).count()
}

// ---------------------------------------------------------------------------
// 来源签名链：包 → 签名者指纹 → 日，链式校验（仿 F144 撤销链工艺）
// ---------------------------------------------------------------------------

pub struct SourceChain {
    events: alloc::vec::Vec<(&'static str, u64, u32)>,
    chain: u64,
}

const CHAIN_GENESIS: u64 = 0x1234_5678_9ABC_DEF0;

impl SourceChain {
    pub fn new() -> SourceChain {
        SourceChain { events: alloc::vec::Vec::new(), chain: CHAIN_GENESIS }
    }

    pub fn record(&mut self, pack: &'static str, signer_fp: u64, day: u32) -> u64 {
        self.chain = ebase::fnv1a64(&self.chain.to_be_bytes())
            ^ ebase::fnv1a64(pack.as_bytes())
            ^ signer_fp
            ^ (day as u64);
        self.events.push((pack, signer_fp, day));
        self.chain
    }

    pub fn verify(&self) -> bool {
        let mut chain = CHAIN_GENESIS;
        for (pack, fp, day) in &self.events {
            chain = ebase::fnv1a64(&chain.to_be_bytes())
                ^ ebase::fnv1a64(pack.as_bytes())
                ^ fp
                ^ (*day as u64);
        }
        chain == self.chain
    }

    /// 同包重复签名检出（重签必须走撤销旧签流程）。
    pub fn double_signed(&self, pack: &str) -> bool {
        self.events.iter().filter(|(p, _, _)| *p == pack).count() > 1
    }
}

// ---------------------------------------------------------------------------
// 哈希分桶布局：name → fnv%16 桶 → 稳定路径渲染
// ---------------------------------------------------------------------------

pub fn bucket_of(name: &str) -> u8 {
    (ebase::fnv1a64(name.as_bytes()) % 16) as u8
}

/// 路径渲染：`icons/<桶两位hex>/<name>`（确定性——同名永远同路径）。
pub fn bucket_path(name: &str) -> [u8; 64] {
    let mut out = [0u8; 64];
    let prefix = b"icons/";
    out[..6].copy_from_slice(prefix);
    let b = bucket_of(name);
    let hex = b"0123456789abcdef";
    out[6] = hex[(b >> 4) as usize];
    out[7] = hex[(b & 0xF) as usize];
    out[8] = b'/';
    let n = name.as_bytes();
    let take = n.len().min(64 - 9);
    out[9..9 + take].copy_from_slice(&n[..take]);
    out
}

// ---------------------------------------------------------------------------
// 回归采样计划：按分类确定性抽取 N 个（步长取样，同名单同结果）
// ---------------------------------------------------------------------------

pub fn sample_plan(names: &[&'static str], per_category: usize) -> alloc::vec::Vec<&'static str> {
    let mut by_cat: alloc::vec::Vec<(IconCategory, alloc::vec::Vec<&'static str>)> =
        alloc::vec::Vec::new();
    for n in names {
        let c = classify(n);
        match by_cat.iter_mut().find(|(cc, _)| *cc == c) {
            Some((_, v)) => v.push(n),
            None => by_cat.push((c, alloc::vec![n])),
        }
    }
    let mut out: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for (_, v) in by_cat.iter_mut() {
        if v.len() <= per_category {
            out.extend_from_slice(v);
        } else {
            // 等距步长取样（确定性，无随机）。
            let step = v.len() as f64 / per_category as f64;
            let mut taken: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
            let mut i = 0usize;
            while taken.len() < per_category {
                let idx = (i as f64 * step) as usize;
                if idx < v.len() && !taken.contains(&v[idx]) {
                    taken.push(v[idx]);
                }
                i += 1;
                if i > v.len() * 2 {
                    break;
                }
            }
            out.extend(taken);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F133G_TAG: &str = "stareco-F133-deep4";

pub fn run_f133_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F133G_TAG);

    // 安装状态机
    let mut inst = PackInstaller::new("flat-pk");
    set.add("f133g skip reject", inst.advance(InstallStage::Active).is_err(), "跳级拒绝");
    let _ = inst.advance(InstallStage::Staged);
    let _ = inst.advance(InstallStage::Registered);
    let _ = inst.advance(InstallStage::Active);
    set.add("f133g full path", inst.stage == InstallStage::Active, "四步走通");
    set.add("f133g active no rollback", inst.advance(InstallStage::RolledBack).is_err(), "已激活不可回滚");
    let mut inst2 = PackInstaller::new("bad-pk");
    let _ = inst2.advance(InstallStage::Staged);
    let _ = inst2.advance(InstallStage::RolledBack);
    set.add("f133g rollback terminal", inst2.rollback_terminal(), "回滚终态");

    // 分类
    let names = ["action-open", "status-busy", "mime-pdf", "emblem-lock", "mystery"];
    set.add("f133g classify", classify("mime-pdf") == IconCategory::Mime, "mime 归类");
    set.add("f133g unclassified", unclassified_count(&names) == 1, "未分类点名");

    // 签名链
    let mut sc = SourceChain::new();
    sc.record("pack-a", 0xAA, 1);
    sc.record("pack-b", 0xBB, 2);
    set.add("f133g chain ok", sc.verify(), "链重放一致");
    sc.record("pack-a", 0xCC, 3);
    set.add("f133g double sign", sc.double_signed("pack-a") && !sc.double_signed("pack-b"), "重签检出");

    // 分桶布局
    let p1 = bucket_path("folder");
    let p2 = bucket_path("folder");
    set.add("f133g bucket stable", p1 == p2, "同名同路径");
    set.add("f133g bucket prefix", &p1[..6] == b"icons/" && p1[8] == b'/', "路径形制");
    set.add("f133g bucket range", bucket_of("folder") < 16, "桶号 0-15");

    // 采样计划
    let pool = [
        "action-a", "action-b", "action-c", "action-d",
        "status-x", "status-y",
    ];
    let plan = sample_plan(&pool, 2);
    set.add("f133g sample size", plan.len() == 4, "每族 2 个共 4");
    set.add(
        "f133g sample det",
        sample_plan(&pool, 2) == plan,
        "确定性同结果",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn installer_rollback_paths() {
        for fail_at in [InstallStage::Validating, InstallStage::Staged, InstallStage::Registered] {
            let mut i = PackInstaller::new("p");
            if fail_at != InstallStage::Validating {
                let _ = i.advance(InstallStage::Staged);
            }
            if fail_at == InstallStage::Registered {
                let _ = i.advance(InstallStage::Registered);
            }
            assert!(i.advance(InstallStage::RolledBack).is_ok());
            assert!(i.rollback_terminal());
        }
    }

    #[test]
    fn sample_deterministic_no_dup() {
        let pool = ["action-a", "action-b", "action-c"];
        let s = sample_plan(&pool, 2);
        assert_eq!(s.len(), 2);
        assert_ne!(s[0], s[1]);
    }
}
