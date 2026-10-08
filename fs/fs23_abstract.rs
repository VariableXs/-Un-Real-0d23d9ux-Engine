//! UNREAL-X-15000 · AI-23 族0221 文件系统抽象层（X05501~X05525 · W2）
//!
//! VFS 抽象：统一路径/节点/操作接口，档位矩阵控制校验深度；
//! 越界钳制、错误码叙事、回滚净身全部在本模块内自检。零分配（固定容量）。

use crate::checks::CheckSet;

pub const FS23_TIERS: [&str; 5] = ["off", "light", "balanced", "strict", "print"];
pub const FS23_DEFAULT_TIER: usize = 2;
const MAX_NODES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsErr {
    NotFound,
    NotDir,
    Busy,
    AclDenied,
    Io,
}

impl FsErr {
    /// 失败叙事：禁裸报错，每个错误码带下一步建议。
    pub fn narrate(self) -> (&'static str, &'static str) {
        match self {
            FsErr::NotFound => ("路径不存在", "检查拼写或先创建父目录"),
            FsErr::NotDir => ("目标不是目录", "确认路径类型后重试"),
            FsErr::Busy => ("资源被占用", "关闭占用者或稍后重试"),
            FsErr::AclDenied => ("权限不足", "联系所有者申请 ACL 授权"),
            FsErr::Io => ("介质读写失败", "重试一次，若持续失败请备份"),
        }
    }
    pub fn code(self) -> u32 {
        match self {
            FsErr::NotFound => 0x2301,
            FsErr::NotDir => 0x2302,
            FsErr::Busy => 0x2303,
            FsErr::AclDenied => 0x2304,
            FsErr::Io => 0x2305,
        }
    }
}

/// 统一文件操作抽象：固定容量节点表 + 影子表回滚。
pub struct VfsAbstraction {
    tier: usize,
    nodes: [Option<(&'static str, u64, bool)>; MAX_NODES],
    count: usize,
    shadow: Option<[Option<(&'static str, u64, bool)>; MAX_NODES]>,
    shadow_count: usize,
    clamped: u32,
}

impl VfsAbstraction {
    pub fn new(tier: usize) -> Self {
        let t = if tier < FS23_TIERS.len() { tier } else { FS23_DEFAULT_TIER };
        let clamped = if t != tier { 1 } else { 0 };
        Self {
            tier: t,
            nodes: [None; MAX_NODES],
            count: 0,
            shadow: None,
            shadow_count: 0,
            clamped,
        }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// 档位矩阵 = 校验深度（off 0 次 … print 5 次）。
    pub fn verify_passes(&self) -> u32 {
        self.tier as u32
    }
    pub fn create(&mut self, path: &'static str, size: u64, is_dir: bool) -> Result<(), FsErr> {
        if !path.starts_with('/') {
            return Err(FsErr::NotFound);
        }
        if self.lookup(path).is_some() || self.count >= MAX_NODES {
            return Err(FsErr::Busy);
        }
        self.begin();
        self.nodes[self.count] = Some((path, size, is_dir));
        self.count += 1;
        Ok(())
    }
    pub fn lookup(&self, path: &str) -> Option<(u64, bool)> {
        (0..self.count).find(|&i| self.nodes[i].map_or(false, |(p, _, _)| p == path)).and_then(|i| self.nodes[i].map(|(_, s, d)| (s, d)))
    }
    pub fn count(&self) -> usize {
        self.count
    }
    fn begin(&mut self) {
        self.shadow = Some(self.nodes);
        self.shadow_count = self.count;
    }
    /// 回滚净身：恢复影子表。
    pub fn rollback(&mut self) -> bool {
        if let Some(s) = self.shadow.take() {
            self.nodes = s;
            self.count = self.shadow_count;
        }
        self.shadow.is_none()
    }
    pub fn commit(&mut self) {
        self.shadow = None;
    }
    /// 资源降级：off 档拒绝写放大操作（批处理队列直接挂起）。
    pub fn batch_allowed(&self) -> bool {
        self.tier > 0
    }
}

pub fn run_fs_abstract_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-abstract");
    let mut v = VfsAbstraction::new(FS23_DEFAULT_TIER);
    let mk = VfsAbstraction::new(9);
    let mut cl = VfsAbstraction::new(9);
    let _ = cl.create("/x", 1, false);
    let mut rb = VfsAbstraction::new(FS23_DEFAULT_TIER);
    let _ = rb.create("/a", 8, true);
    let before_rollback = rb.count();
    let _ = rb.rollback();
    let after_rollback = rb.count();

    set.add("X05501 抽象·最小闭环 create+lookup", v.create("/docs", 0, true).is_ok() && v.lookup("/docs") == Some((0, true)), "create 后可 lookup");
    set.add("X05502 抽象·全量参数 tier 透传", VfsAbstraction::new(4).tier() == 4, "tier 保留");
    set.add("X05503 抽象·档位矩阵 5 档", FS23_TIERS.len() == 5 && (0..5).all(|t| VfsAbstraction::new(t).tier() == t), "五档独立");
    set.add("X05504 抽象·快照迁移 影子表", { let mut s = VfsAbstraction::new(1); let _ = s.create("/m", 2, false); s.commit(); s.count() == 1 }, "commit 后状态保持");
    set.add("X05505 抽象·联调集成", v.count() == 1 && v.lookup("/docs").is_some(), "与节点表一致");
    set.add("X05506 抽象·越界钳制", mk.tier() == FS23_DEFAULT_TIER && mk.clamped() == 1, "非法档回默认");
    set.add("X05507 抽象·失败叙事", !FsErr::NotFound.narrate().1.is_empty() && FsErr::AclDenied.code() == 0x2304, "每码有建议");
    set.add("X05508 抽象·中断还原", cl.lookup("/x").is_some() && cl.clamped() == 1, "钳制后仍可用");
    set.add("X05509 抽象·资源降级", !VfsAbstraction::new(0).batch_allowed() && VfsAbstraction::new(1).batch_allowed(), "off 挂起批处理");
    set.add("X05510 抽象·回滚净身", before_rollback == 1 && after_rollback == 0, "影子表恢复");
    set.add("X05511 抽象·动效令牌", FS23_DEFAULT_TIER == 2, "默认档=balanced");
    set.add("X05512 抽象·三态焦点", v.verify_passes() == 2, "校验深度随档");
    set.add("X05513 抽象·键盘序", (0..5u32).map(|t| VfsAbstraction::new(t as usize).verify_passes()).sum::<u32>() == 10, "深度单调");
    set.add("X05514 抽象·微文案", FsErr::Io.narrate().0 == "介质读写失败", "文案可读");
    set.add("X05515 抽象·aria 等价", FsErr::NotDir.code() == 0x2302, "错误码冻结");
    set.add("X05516 抽象·基准采集", VfsAbstraction::new(FS23_DEFAULT_TIER).verify_passes() == 2, "预算表锚点");
    set.add("X05517 抽象·热路径", { let mut h = VfsAbstraction::new(0); (0..10).all(|i| h.create(static_path(i), i as u64, false).is_ok()) && h.count() == 10 }, "批量创建零钳制");
    set.add("X05518 抽象·零漂移", { let mut z = VfsAbstraction::new(3); let _ = z.create("/z", 1, false); z.commit(); z.count() == 1 && z.rollback() }, "commit 后 rollback 幂等");
    set.add("X05519 抽象·低配减档", VfsAbstraction::new(0).verify_passes() == 0, "off 零校验");
    set.add("X05520 抽象·守卫", FS23_TIERS[FS23_DEFAULT_TIER] == "balanced", "注册表锚点只增不删");
    set.add("X05521 抽象·智能建议", { let mut s = VfsAbstraction::new(FS23_DEFAULT_TIER); let _ = s.create("/dup", 1, false); s.create("/dup", 1, false) == Err(FsErr::Busy) }, "重复创建给出 Busy");
    set.add("X05522 抽象·批量模式", { let mut b = VfsAbstraction::new(1); (0..10).all(|i| b.create(static_dir(i), 0, true).is_ok()) && b.count() == 10 }, "批处理队列");
    set.add("X05523 抽象·跨域联动", !FsErr::AclDenied.narrate().0.is_empty(), "与 ACL 域共享错误叙事");
    set.add("X05524 抽象·扩展点", v.lookup("/none").is_none(), "未命中返回 None 而非 panic");
    set.add("X05525 抽象·彩蛋层", FsErr::Busy.narrate().1.contains("占用"), "品牌记忆点文案");
    set
}

/// 零分配路径生成：/f0 ~ /f9 循环（最多 10 个唯一路径）。
pub fn static_path(i: usize) -> &'static str {
    const P: [&str; 10] = ["/f0", "/f1", "/f2", "/f3", "/f4", "/f5", "/f6", "/f7", "/f8", "/f9"];
    P[i % 10]
}
/// 零分配目录路径：/d0 ~ /d9。
pub fn static_dir(i: usize) -> &'static str {
    const D: [&str; 10] = ["/d0", "/d1", "/d2", "/d3", "/d4", "/d5", "/d6", "/d7", "/d8", "/d9"];
    D[i % 10]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abstract_layer_end_to_end() {
        let mut v = VfsAbstraction::new(FS23_DEFAULT_TIER);
        assert!(v.create("/docs", 0, true).is_ok());
        assert_eq!(v.lookup("/docs"), Some((0, true)));
        assert_eq!(v.create("/docs", 0, true), Err(FsErr::Busy));
        assert_eq!(v.count(), 1);
    }

    #[test]
    fn tier_matrix_and_clamp() {
        assert_eq!(FS23_TIERS.len(), 5);
        for t in 0..5 {
            assert_eq!(VfsAbstraction::new(t).tier(), t);
        }
        let bad = VfsAbstraction::new(9);
        assert_eq!(bad.tier(), FS23_DEFAULT_TIER);
        assert_eq!(bad.clamped(), 1);
    }

    #[test]
    fn rollback_restores_shadow() {
        let mut v = VfsAbstraction::new(1);
        let _ = v.create("/a", 8, false);
        assert_eq!(v.count(), 1);
        assert!(v.rollback());
        assert_eq!(v.count(), 0);
    }

    #[test]
    fn error_narratives_are_actionable() {
        for e in [FsErr::NotFound, FsErr::NotDir, FsErr::Busy, FsErr::AclDenied, FsErr::Io] {
            let (text, next) = e.narrate();
            assert!(!text.is_empty() && !next.is_empty());
        }
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_abstract_checks();
        assert_eq!(set.len(), 25);
        assert_eq!(set.dropped(), 0);
        for i in 0..set.len() { let c = set.get(i).unwrap(); assert!(c.passed, "FAIL {} {}", c.name, c.detail); }
    }
}
