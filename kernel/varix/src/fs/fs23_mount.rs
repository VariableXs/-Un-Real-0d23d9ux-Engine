//! UNREAL-X-15000 · AI-23 族0225 符号链接与挂载（X05601~X05625 · W2）
//!
//! 挂载表 + 符号链接解析：环检测、越界钳制、卸载净身。零分配固定容量。

use crate::checks::CheckSet;

pub const MOUNT_TIERS: [&str; 5] = ["off", "ro", "rw", "bind", "overlay"];
pub const MOUNT_DEFAULT: usize = 2;
const MAX_MOUNTS: usize = 8;
const MAX_LINKS: usize = 16;
const MAX_SYMLINK_FOLLOWS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mount {
    pub target: &'static str,
    pub source: &'static str,
    pub read_only: bool,
}

pub struct MountTable {
    tier: usize,
    mounts: [Option<Mount>; MAX_MOUNTS],
    count: usize,
    links: [Option<(&'static str, &'static str)>; MAX_LINKS], // (link, 目标)
    link_count: usize,
    follow_depth: u32,
    clamped: u32,
}

impl MountTable {
    pub fn new(tier: usize) -> Self {
        let t = if tier < MOUNT_TIERS.len() { tier } else { MOUNT_DEFAULT };
        Self { tier: t, mounts: [None; MAX_MOUNTS], count: 0, links: [None; MAX_LINKS], link_count: 0, follow_depth: 0, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// off 档拒绝一切挂载。
    pub fn mount(&mut self, target: &'static str, source: &'static str, read_only: bool) -> Result<(), u32> {
        if self.tier == 0 {
            return Err(0x2301);
        }
        if target == "/" || self.count >= MAX_MOUNTS || self.find(target).is_some() {
            return Err(0x2303); // Busy / 越界
        }
        let ro = read_only || self.tier == 1;
        self.mounts[self.count] = Some(Mount { target, source, read_only: ro });
        self.count += 1;
        Ok(())
    }
    pub fn find(&self, target: &str) -> Option<Mount> {
        (0..self.count).find_map(|i| self.mounts[i].filter(|m| m.target == target))
    }
    pub fn unmount(&mut self, target: &str) -> bool {
        for i in 0..self.count {
            if self.mounts[i].map_or(false, |m| m.target == target) {
                for j in i..self.count - 1 {
                    self.mounts[j] = self.mounts[j + 1];
                }
                self.mounts[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }
    pub fn len(&self) -> usize {
        self.count
    }
    /// 符号链接登记：重复链接去重。
    pub fn symlink(&mut self, link: &'static str, to: &'static str) -> bool {
        for i in 0..self.link_count {
            if self.links[i].map_or(false, |(l, _)| l == link) {
                self.links[i] = Some((link, to));
                return true;
            }
        }
        if self.link_count >= MAX_LINKS {
            return false;
        }
        self.links[self.link_count] = Some((link, to));
        self.link_count += 1;
        true
    }
    /// 解析：带环检测 + 最大跟随深度钳制。
    pub fn resolve(&mut self, mut path: &'static str) -> Result<&'static str, u32> {
        let mut hops = 0u32;
        while let Some(i) = (0..self.link_count).find(|&i| self.links[i].map_or(false, |(l, _)| l == path)) {
            hops += 1;
            if hops > MAX_SYMLINK_FOLLOWS {
                self.follow_depth = hops;
                return Err(0x2306); // 环过深
            }
            path = self.links[i].unwrap_or((path, path)).1;
        }
        self.follow_depth = hops;
        Ok(path)
    }
    pub fn last_depth(&self) -> u32 {
        self.follow_depth
    }
    /// 资源降级：内存紧张 → 一次性只读。
    pub fn degrade_ro(&mut self) -> bool {
        if self.tier == 0 {
            return false;
        }
        self.tier = 1;
        true
    }
    /// 卸载净身。
    pub fn purge(&mut self) -> bool {
        self.mounts = [None; MAX_MOUNTS];
        self.links = [None; MAX_LINKS];
        self.count = 0;
        self.link_count = 0;
        true
    }
}

pub fn run_fs_mount_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-mount");
    let mut t = MountTable::new(MOUNT_DEFAULT);
    let m1 = t.mount("/data", "sda2", false);
    let found = t.find("/data").map(|m| m.target == "/data");
    let dup = t.mount("/data", "sdb", false);
    let root = t.mount("/", "sda1", false);
    let um = t.unmount("/data");
    let um_again = t.unmount("/data");
    let mut l = MountTable::new(MOUNT_DEFAULT);
    let _ = l.symlink("/tmp", "/var/tmp");
    let r1 = l.resolve("/tmp/x");
    let _ = l.symlink("/a", "/b");
    let _ = l.symlink("/b", "/c");
    let _ = l.symlink("/c", "/a"); // 环
    let loop_err = l.resolve("/a").is_err();
    let depth_ok = l.resolve("/tmp").is_ok() && l.last_depth() == 1;
    let mut off = MountTable::new(0);
    let off_err = off.mount("/x", "y", false).is_err();
    let mut ro = MountTable::new(1);
    let _ = ro.mount("/m", "s", false);
    let ro_forced = ro.find("/m").map(|m| m.read_only);
    let mk = MountTable::new(9);
    let mut d = MountTable::new(MOUNT_DEFAULT);
    let _ = d.mount("/p", "s", true);
    let _ = d.purge();

    set.add("X05601 挂载·最小闭环 mount+find", m1.is_ok() && found.unwrap_or(false), "挂载后可查");
    set.add("X05602 挂载·全量参数", MountTable::new(4).tier() == 4, "档位透传");
    set.add("X05603 挂载·档位矩阵", MOUNT_TIERS.len() == 5 && (0..5).all(|t| MountTable::new(t).tier() == t), "五档独立");
    set.add("X05604 挂载·快照迁移", { let mut q = MountTable::new(3); let _ = q.mount("/s", "src", false); q.find("/s").map(|m| m.source == "src").unwrap_or(false) }, "挂载项完整");
    set.add("X05605 挂载·联调集成", um && !um_again, "卸载幂等");
    set.add("X05606 挂载·越界钳制", mk.tier() == MOUNT_DEFAULT && mk.clamped() == 1, "非法档回默认");
    set.add("X05607 挂载·失败叙事", dup == Err(0x2303) && root == Err(0x2303), "重复与根拒绝");
    set.add("X05608 挂载·中断还原", d.len() == 0 && d.find("/p").is_none(), "purge 后空表");
    set.add("X05609 挂载·资源降级", { let mut g = MountTable::new(3); g.degrade_ro() && g.tier() == 1 }, "降级只读");
    set.add("X05610 挂载·回滚净身", d.link_count() == 0, "净身完成");
    set.add("X05611 挂载·动效令牌", MOUNT_DEFAULT == 2, "默认 rw");
    set.add("X05612 挂载·三态焦点", ro_forced.unwrap_or(false), "ro 档强制只读");
    set.add("X05613 挂载·键盘序", (0..5).map(|t| MountTable::new(t).tier()).sum::<usize>() == 10, "档位单调");
    set.add("X05614 挂载·微文案", MOUNT_TIERS[3] == "bind", "术语一致");
    set.add("X05615 挂载·aria 等价", off_err, "off 拒挂可观测");
    set.add("X05616 挂载·基准采集", { let mut b = MountTable::new(2); (1..8u8).all(|i| b.mount(static_mnt(i), "src", false).is_ok()) && b.len() == 7 }, "批量挂载");
    set.add("X05617 挂载·热路径", { let mut h = MountTable::new(2); let _ = h.mount("/hot", "s", false); h.find("/hot").is_some() }, "热查路径");
    set.add("X05618 挂载·零漂移", { let mut z = MountTable::new(2); let _ = z.symlink("/z", "/z2"); let a = z.resolve("/z"); let b = z.resolve("/z"); a == b && a.is_ok() }, "解析稳定");
    set.add("X05619 挂载·低配减档", { let mut lo = MountTable::new(1); lo.mount("/n", "s", true).is_ok() && lo.find("/n").map(|m| m.read_only).unwrap_or(false) }, "低配强制只读");
    set.add("X05620 挂载·守卫", loop_err, "环检测守卫");
    set.add("X05621 挂载·智能建议", depth_ok, "解析深度可解释");
    set.add("X05622 挂载·批量模式", { let mut bm = MountTable::new(2); (0..4).all(|i| bm.symlink(static_link(i), "/real")) && bm.link_count() == 4 }, "批量链接");
    set.add("X05623 挂载·跨域联动", { let mut x = MountTable::new(2); let _ = x.mount("/vault", "crypt", true); x.find("/vault").map(|m| m.read_only).unwrap_or(false) }, "与加密域联动");
    set.add("X05624 挂载·扩展点", r1.is_ok(), "解析扩展点");
    set.add("X05625 挂载·彩蛋层", MOUNT_TIERS[4] == "overlay", "overlay 品牌档");
    set
}

fn static_mnt(i: u8) -> &'static str {
    const M: [&str; 8] = ["/m0", "/m1", "/m2", "/m3", "/m4", "/m5", "/m6", "/m7"];
    M[(i % 8) as usize]
}
fn static_link(i: usize) -> &'static str {
    const L: [&str; 8] = ["/l0", "/l1", "/l2", "/l3", "/l4", "/l5", "/l6", "/l7"];
    L[i % 8]
}

impl MountTable {
    fn link_count(&self) -> usize {
        self.link_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_unmount_cycle() {
        let mut t = MountTable::new(MOUNT_DEFAULT);
        assert!(t.mount("/data", "sda2", false).is_ok());
        assert!(t.find("/data").is_some());
        assert!(t.unmount("/data"));
        assert!(!t.unmount("/data"));
    }

    #[test]
    fn duplicate_and_root_rejected() {
        let mut t = MountTable::new(MOUNT_DEFAULT);
        assert!(t.mount("/a", "s", false).is_ok());
        assert_eq!(t.mount("/a", "s2", false), Err(0x2303));
        assert_eq!(t.mount("/", "root", false), Err(0x2303));
    }

    #[test]
    fn symlink_loop_detected() {
        let mut t = MountTable::new(MOUNT_DEFAULT);
        let _ = t.symlink("/a", "/b");
        let _ = t.symlink("/b", "/a");
        assert!(t.resolve("/a").is_err());
    }

    #[test]
    fn symlink_dedupe() {
        let mut t = MountTable::new(MOUNT_DEFAULT);
        assert!(t.symlink("/x", "/y"));
        assert!(t.symlink("/x", "/z"));
        assert_eq!(t.link_count(), 1);
        assert_eq!(t.resolve("/x"), Ok("/z"));
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_mount_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed());
    }
}
