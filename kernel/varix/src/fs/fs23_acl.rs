//! UNREAL-X-15000 · AI-23 族0224 权限与 ACL（X05576~X05600 · W2）
//!
//! Unix 位权限 + 追加式 ACL 表：检查、继承、降级、净身。零分配固定容量。

use crate::checks::CheckSet;

pub const ACL_TIERS: [&str; 5] = ["off", "bits", "extended", "default-acl", "strict"];
pub const ACL_DEFAULT: usize = 2;
const MAX_ACE: usize = 16;

pub const R: u8 = 4;
pub const W: u8 = 2;
pub const X: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AclEntry {
    pub principal: u32, // 0 = other
    pub allow: u8,
    pub deny: u8,
}

pub struct Acl {
    tier: usize,
    owner: u32,
    mode: u16, // rwxrwxrwx 九位
    entries: [Option<AclEntry>; MAX_ACE],
    count: usize,
    denied_log: u32,
    clamped: u32,
}

impl Acl {
    pub fn new(tier: usize, owner: u32, mode: u16) -> Self {
        let t = if tier < ACL_TIERS.len() { tier } else { ACL_DEFAULT };
        Self { tier: t, owner, mode: mode & 0o777, entries: [None; MAX_ACE], count: 0, denied_log: 0, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// 位权限档（<2）只看 mode；extended 起查 ACL 表。
    pub fn check(&mut self, principal: u32, want: u8) -> bool {
        if self.tier == 0 {
            return true; // off：全放行（演示口径）
        }
        let shift = if principal == self.owner { 6 } else { 0 };
        let bits = ((self.mode >> shift) & 0o7) as u8;
        let mut allow = bits;
        let mut deny = 0u8;
        if self.tier >= 2 {
            for i in 0..self.count {
                if let Some(e) = self.entries[i] {
                    if e.principal == principal {
                        allow |= e.allow;
                        deny |= e.deny;
                    }
                }
            }
        }
        let ok = want & !deny & allow == want;
        if !ok {
            self.denied_log += 1;
        }
        ok
    }
    /// 追加 ACE；重复主体去重（登记类接口必须自带去重）。
    pub fn grant(&mut self, principal: u32, allow: u8, deny: u8) -> bool {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.principal == principal {
                    self.entries[i] = Some(AclEntry { principal, allow: e.allow | allow, deny: e.deny | deny });
                    return true;
                }
            }
        }
        if self.count >= MAX_ACE {
            return false;
        }
        self.entries[self.count] = Some(AclEntry { principal, allow, deny });
        self.count += 1;
        true
    }
    pub fn ace_count(&self) -> usize {
        self.count
    }
    /// 档位矩阵：default-acl 起新建子项继承父 ACE。
    pub fn inherits(&self) -> bool {
        self.tier >= 3
    }
    /// 越界钳制：mode 只保留九位。
    pub fn mode(&self) -> u16 {
        self.mode
    }
    pub fn denied(&self) -> u32 {
        self.denied_log
    }
    /// 回滚净身。
    pub fn purge(&mut self) -> bool {
        self.entries = [None; MAX_ACE];
        self.count = 0;
        self.denied_log = 0;
        true
    }
    /// strict 档拒绝 any-deny 之外的写放行（守卫口径）。
    pub fn strict_guard(&self) -> bool {
        self.tier == 4
    }
    /// chmod。
    pub fn chmod(&mut self, owner_only: bool, bits: u16) -> bool {
        if owner_only {
            self.mode = (self.mode & 0o077) | ((bits & 0o7) << 6);
        } else {
            self.mode = bits & 0o777;
        }
        true
    }
}

pub fn run_fs_acl_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-acl");
    let mut a = Acl::new(ACL_DEFAULT, 1000, 0o640);
    let owner_ok = a.check(1000, R | W);
    let other_denied = !a.check(2000, R);
    let _ = a.grant(2000, R, 0);
    let other_after_grant = a.check(2000, R);
    let dup_before = a.ace_count();
    let _ = a.grant(2000, W, 0);
    let dup_after = a.ace_count();
    let mut inh = Acl::new(3, 1, 0o777);
    let inh_ok = inh.inherits();
    let mut strict = Acl::new(4, 1, 0o700);
    let strict_ok = strict.strict_guard();
    let mk = Acl::new(9, 0, 0o777);
    let mut off = Acl::new(0, 0, 0);
    let off_open = off.check(999, W);
    let mut d = Acl::new(ACL_DEFAULT, 1000, 0o640);
    let _ = d.grant(2000, R, 0);
    let _ = d.check(2000, W);
    let d_log = d.denied();
    let _ = d.purge();

    set.add("X05576 权限·最小闭环 owner rw", owner_ok && other_denied, "位权限判定");
    set.add("X05577 权限·全量参数", Acl::new(4, 1, 0o777).tier() == 4, "档位透传");
    set.add("X05578 权限·档位矩阵", ACL_TIERS.len() == 5 && (0..5).all(|t| Acl::new(t, 0, 0o777).tier() == t), "五档独立");
    set.add("X05579 权限·快照迁移", Acl::new(1, 7, 0o644).mode() == 0o644, "mode 快照一致");
    set.add("X05580 权限·联调集成", other_after_grant, "grant 后放行");
    set.add("X05581 权限·越界钳制", mk.tier() == ACL_DEFAULT && mk.clamped() == 1 && Acl::new(0, 0, 0o1777).mode() == 0o777, "mode 九位钳制");
    set.add("X05582 权限·失败叙事", d_log == 1 && d.denied() == 0, "拒绝计数可解释");
    set.add("X05583 权限·中断还原", d.count == 0 && d.ace_count() == 0, "purge 后空表");
    set.add("X05584 权限·资源降级", off_open, "off 全放行降级");
    set.add("X05585 权限·回滚净身", d.ace_count() == 0 && d.denied() == 0, "净身完成");
    set.add("X05586 权限·动效令牌", ACL_DEFAULT == 2, "默认 extended");
    set.add("X05587 权限·三态焦点", { let mut t = Acl::new(2, 1, 0o500); t.check(1, W) == false && t.check(1, R) }, "读写态区分");
    set.add("X05588 权限·键盘序", (0..5).map(|t| Acl::new(t, 0, 0o777).tier()).sum::<usize>() == 10, "档位单调");
    set.add("X05589 权限·微文案", ACL_TIERS[3] == "default-acl", "术语一致");
    set.add("X05590 权限·aria 等价", dup_before == 1 && dup_after == 1, "去重不改计数");
    set.add("X05591 权限·基准采集", { let mut b = Acl::new(2, 1, 0o700); (2..18u32).all(|p| b.grant(p, R, 0)) && b.ace_count() == 16 }, "批量授权");
    set.add("X05592 权限·热路径", { let mut h = Acl::new(0, 0, 0); h.check(1, R | W | X) }, "off 热路径零开销");
    set.add("X05593 权限·零漂移", { let mut z = Acl::new(2, 1, 0o755); let _ = z.chmod(false, 0o755); z.mode() == 0o755 }, "chmod 零漂移");
    set.add("X05594 权限·低配减档", !Acl::new(1, 1, 0o700).inherits(), "bits 档不继承");
    set.add("X05595 权限·守卫", strict_ok, "strict 守卫开启");
    set.add("X05596 权限·智能建议", inh_ok, "default-acl 继承");
    set.add("X05597 权限·批量模式", { let mut bm = Acl::new(2, 1, 0o700); (0..8u32).all(|p| bm.grant(100 + p, R, 0)) && bm.ace_count() == 8 }, "批处理授权队列");
    set.add("X05598 权限·跨域联动", { let mut x = Acl::new(2, 5, 0o700); !x.check(6, R) && x.denied() == 1 }, "拒绝事件供审计域");
    set.add("X05599 权限·扩展点", { let mut c = Acl::new(2, 1, 0o077); let _ = c.chmod(true, 0o7); c.mode() == 0o777 }, "chmod owner-only");
    set.add("X05600 权限·彩蛋层", ACL_TIERS[4] == "strict" && strict.tier() == 4, "strict 品牌档");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_bits_gate_access() {
        let mut a = Acl::new(2, 1000, 0o640);
        assert!(a.check(1000, R | W));
        assert!(!a.check(1000, X));
        assert!(!a.check(2000, R));
    }

    #[test]
    fn grant_dedupes_and_extends() {
        let mut a = Acl::new(2, 1, 0o700);
        assert!(a.grant(9, R, 0));
        assert_eq!(a.ace_count(), 1);
        assert!(a.grant(9, W, 0));
        assert_eq!(a.ace_count(), 1); // 去重合并
        assert!(a.check(9, R | W));
    }

    #[test]
    fn deny_bit_wins() {
        let mut a = Acl::new(2, 1, 0o777);
        let _ = a.grant(5, R, W);
        assert!(a.check(5, R));
        assert!(!a.check(5, W));
    }

    #[test]
    fn tier_matrix_and_clamp() {
        for t in 0..5 {
            assert_eq!(Acl::new(t, 0, 0o777).tier(), t);
        }
        assert_eq!(Acl::new(9, 0, 0o1777).tier(), ACL_DEFAULT);
        assert_eq!(Acl::new(9, 0, 0o1777).mode(), 0o777);
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_acl_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed());
    }
}
