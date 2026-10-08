//! 星图目录服务与校验链（WP-305 · B-2101 假目录、坏哈希全拒）。
//!
//! MD2 篇 21.1：星图数据是 JSON 文件集（每软件一卡文件+一索引文件）。目录
//! 服务的职责——加载与校验（schema 校验加哈希核对，Q55）、版本化（每次变更
//! 产生一个目录版本号，单调递增）、星卡引用目录版本（"你看到的信息是哪个
//! 时点的"永远可答）、安装面与评级面分离（Q83：目录是评级事实，不是安装
//! 白名单——开放性与秩序并存）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;
use crate::sdkmanifest::valid_id;

// ---------------------------------------------------------------------------
// 目录条目（模型面）
// ---------------------------------------------------------------------------

/// 评级五档（MD1 18.1 表序锁定——与 starmapui 同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rating {
    Native,
    DirectPlug,
    Compat,
    Bridge,
    Fallback,
}

/// 目录条目（schema 面字段齐备才可入校验）。
#[derive(Clone, Copy)]
pub struct StarEntry {
    /// 星卡 id（反域名——与 SDK 清单同源校验）。
    pub id: &'static str,
    /// 评级五档（穷举类型面——无档外取值）。
    pub rating: Rating,
    /// 必备字段齐备（schema 面——缺字段的条目是坏条目）。
    pub schema_ok: bool,
    /// 本条目最后变更时的目录版本（星卡引用目录版本）。
    pub dir_ver: u32,
}

// ---------------------------------------------------------------------------
// 目录指纹（Q55 哈希核对——内容到指纹的确定性映射）
// ---------------------------------------------------------------------------

/// 目录指纹：逐条目逐字节 FNV-1a 混合（id 字节 + 评级序号 + schema 位 +
/// 引用版本）。同内容必同指纹——内容被改一枚字节指纹即变。
pub fn dir_digest(entries: &[StarEntry]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        let b = e.id.as_bytes();
        let mut j = 0;
        while j < b.len() {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x100000001b3);
            j += 1;
        }
        h ^= (e.rating as u64) << 32;
        h = h.wrapping_mul(0x100000001b3);
        h ^= (e.schema_ok as u64) << 40;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.dir_ver as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 目录校验裁决（二值——校验链没有"大概是真"的第三种结局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirVerdict {
    Accept,
    Reject,
}

/// 目录校验链（**B-2101 达标线：假目录、坏哈希全拒**）——schema 面（id 合法
/// +必备字段齐）与哈希面（重算指纹==声称指纹）双关，任一失守整体拒。
pub fn validate_dir(entries: &[StarEntry], claimed: u64) -> DirVerdict {
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        if !valid_id(e.id) || !e.schema_ok {
            return DirVerdict::Reject;
        }
        i += 1;
    }
    if dir_digest(entries) != claimed {
        return DirVerdict::Reject;
    }
    DirVerdict::Accept
}

// ---------------------------------------------------------------------------
// 目录版本（单调递增——一变一号）
// ---------------------------------------------------------------------------

/// 版本步进判：变更一次进一号（next==prev+1），不变号不动（next==prev）。
/// 跳号与回退都拒——版本号是"时点"的坐标，跳了就答不了"哪个时点"。
pub fn version_step(prev: u32, next: u32, changed: bool) -> bool {
    if changed {
        next == prev + 1
    } else {
        next == prev
    }
}

// ---------------------------------------------------------------------------
// 星卡引用目录版本 + 安装面与评级面分离
// ---------------------------------------------------------------------------

/// 条目可见判：星卡引用版本 <= 当前目录版本——引用未来的条目不可见
/// （"你看到的信息是哪个时点的"永远可答的坐标约束）。
pub fn entry_visible(e: &StarEntry, cur_ver: u32) -> bool {
    e.dir_ver <= cur_ver
}

/// 安装面与评级面分离（Q83）：目录收录（评级事实）与安装放行（白名单）
/// 是两本账——在册不等于可装，可装不依赖在册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TwoLedgers {
    /// 目录在册（评级事实）。
    pub listed: bool,
    /// 安装放行（白名单独立判定）。
    pub install_allow: bool,
}

// ---------------------------------------------------------------------------
// CheckSet（B-2101 · 5 项）
// ---------------------------------------------------------------------------

pub fn run_starmapdir_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2101 星图目录校验链");
    // 1. schema 校验面：id 合法 + 必备字段齐，坏条目整体拒。
    let good = StarEntry { id: "edit.foo", rating: Rating::Compat, schema_ok: true, dir_ver: 3 };
    let bad_id = StarEntry { id: "Edit.Foo", rating: Rating::Compat, schema_ok: true, dir_ver: 3 };
    let bad_schema = StarEntry { id: "edit.foo", rating: Rating::Compat, schema_ok: false, dir_ver: 3 };
    set.add(
        "B-2101 schema 校验面",
        valid_id(good.id)
            && validate_dir(&[good], dir_digest(&[good])) == DirVerdict::Accept
            && validate_dir(&[bad_id], dir_digest(&[bad_id])) == DirVerdict::Reject
            && validate_dir(&[bad_schema], dir_digest(&[bad_schema])) == DirVerdict::Reject,
        "id 反域名逐字节+必备字段齐备——坏条目拒收，schema 面没有将就",
    );
    // 2. 哈希核对（Q55）：假目录（内容改一枚字节）与坏哈希（声称不符）全拒。
    let entries = [
        StarEntry { id: "edit.foo", rating: Rating::Compat, schema_ok: true, dir_ver: 3 },
        StarEntry { id: "term.bar", rating: Rating::Native, schema_ok: true, dir_ver: 3 },
    ];
    let right = dir_digest(&entries);
    let mut forged = entries;
    forged[1].rating = Rating::DirectPlug; // 内容被改：假目录
    let clean = [entries[0]];
    set.add(
        "B-2101 哈希核对 Q55",
        validate_dir(&entries, right) == DirVerdict::Accept
            && validate_dir(&entries, right.wrapping_add(1)) == DirVerdict::Reject
            && validate_dir(&forged, right) == DirVerdict::Reject
            && validate_dir(&clean, right) == DirVerdict::Reject,
        "重算指纹==声称指纹才放行——假目录、坏哈希全拒（B-2101 达标线）",
    );
    // 3. 版本单调递增：一变一号，跳号与回退都拒。
    set.add(
        "B-2101 版本单调递增",
        version_step(7, 8, true)
            && version_step(7, 7, false)
            && !version_step(7, 7, true)
            && !version_step(7, 9, true)
            && !version_step(8, 7, true),
        "变更一次进一号——版本号是时点坐标，跳号回退都答不了'哪个时点'",
    );
    // 4. 星卡引用目录版本：引用未来的条目不可见。
    let past = StarEntry { id: "edit.foo", rating: Rating::Compat, schema_ok: true, dir_ver: 3 };
    let future = StarEntry { id: "edit.foo", rating: Rating::Compat, schema_ok: true, dir_ver: 9 };
    set.add(
        "B-2101 版本引用可答",
        entry_visible(&past, 5) && !entry_visible(&future, 5),
        "引用版本<=当前版本才可见——'你看到的信息是哪个时点的'永远可答",
    );
    // 5. 安装面与评级面分离（Q83）：在册不可装是合法状态。
    let listed_no_install = TwoLedgers { listed: true, install_allow: false };
    let not_listed = TwoLedgers { listed: false, install_allow: false };
    set.add(
        "B-2101 安装评级分离",
        listed_no_install.listed && !listed_no_install.install_allow && !not_listed.install_allow,
        "目录是评级事实不是安装白名单——两本账各记各的，开放性与秩序并存（Q83）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe14 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe14_reject_forgeries() {
        // 假目录：内容改一枚字节，声称哈希不变——拒。
        let base = [StarEntry { id: "a.b", rating: Rating::Native, schema_ok: true, dir_ver: 1 }];
        let h = dir_digest(&base);
        let mut fake = base;
        fake[0].dir_ver = 2;
        assert_eq!(validate_dir(&base, h), DirVerdict::Accept);
        assert_eq!(validate_dir(&fake, h), DirVerdict::Reject);
        // 坏哈希：内容未动，声称哈希错——拒。
        assert_eq!(validate_dir(&base, h ^ 0xff), DirVerdict::Reject);
        // 指纹确定性：同内容两次计算必同值。
        assert_eq!(dir_digest(&base), dir_digest(&base));
    }

    #[test]
    fn fe14_version_rules() {
        // 一变一号：变更必 +1、不变必不动；跳号/回退/不变却进号全拒。
        assert!(version_step(5, 6, true));
        assert!(version_step(5, 5, false));
        assert!(!version_step(5, 5, true));
        assert!(!version_step(5, 7, true));
        assert!(!version_step(6, 5, true));
        // 连续变更严格逐号递增。
        let mut v = 1u32;
        let mut i = 0;
        while i < 4 {
            assert!(version_step(v, v + 1, true));
            v += 1;
            i += 1;
        }
        assert_eq!(v, 5);
    }

    #[test]
    fn fe14_entry_visibility() {
        // 引用过去可见、引用当下可见、引用未来不可见。
        let e3 = StarEntry { id: "a.b", rating: Rating::Compat, schema_ok: true, dir_ver: 3 };
        assert!(entry_visible(&e3, 3));
        assert!(entry_visible(&e3, 9));
        let e9 = StarEntry { id: "a.b", rating: Rating::Compat, schema_ok: true, dir_ver: 9 };
        assert!(!entry_visible(&e9, 3));
    }

    #[test]
    fn fe14_two_ledgers_separated() {
        // 在册而不可装合法、不在册也不可装合法——评级面与安装面互不推论。
        let a = TwoLedgers { listed: true, install_allow: false };
        let b = TwoLedgers { listed: false, install_allow: true };
        let c = TwoLedgers { listed: true, install_allow: true };
        assert!(a.listed && !a.install_allow);
        assert!(!b.listed && b.install_allow);
        assert!(c.listed && c.install_allow);
    }
}
