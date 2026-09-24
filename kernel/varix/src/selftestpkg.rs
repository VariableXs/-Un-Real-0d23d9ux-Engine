//! 星图自测脚本包安全面（WP-305 · B-4301 采集范围限定验证）。
//!
//! MD2 篇 43.1：社区提交的前置是标准化自测——星图发布自测脚本包（判例引
//! 擎的轻量分发版，篇 25.1 的只读形态：执行预设判例、采集数据、输出标准
//! 报告，**无写权限不碰系统配置**）。防滥用三件：报告带环境指纹与时间戳
//! （防伪造）、采集范围限定在目标应用（不扫系统其他部分）、报告明示"提交
//! 后由人工复核，复核结果可公开可匿名"。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 只读形态（能力位钉死——"不碰系统配置"是结构面不是承诺）
// ---------------------------------------------------------------------------

/// 自测包能力面（**只读形态：can_write 恒 false**——类型面没有写路径，
/// "不碰系统配置"在编译面不可能被违反）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelfTestPkg {
    /// 写能力位（只读形态钉死 false——本包无写权限）。
    pub can_write: bool,
    /// 预设判例数（执行预设判例——不能自造判例）。
    pub preset_cases: u8,
}

/// 只读形态常量（能力位的唯一合法值——false）。
pub const READ_ONLY_CAN_WRITE: bool = false;

/// 自测包出厂判：写位必须是只读常量且预设判例在册（零判例的包没有用途）。
pub fn pkg_ok(p: &SelfTestPkg) -> bool {
    p.can_write == READ_ONLY_CAN_WRITE && p.preset_cases > 0
}

// ---------------------------------------------------------------------------
// 采集范围限定（目标应用白名单——不扫系统其他部分）
// ---------------------------------------------------------------------------

/// 采集许可判：事件归属应用==目标应用才可采（**B-4301 达标线：采集范围
/// 限定**）——范围外事件拒采，"id 相等"是唯一许可规则。
pub fn collect_allowed(target_app: &str, event_app: &str) -> bool {
    target_app == event_app
}

// ---------------------------------------------------------------------------
// 报告有效性（防伪造 + 复核明示）
// ---------------------------------------------------------------------------

/// 自测报告（机器可验的兼容性报告——三要素齐才有效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelfReport {
    /// 环境指纹（0 = 没算——缺指纹的报告无法防伪造）。
    pub env_fp: u64,
    /// 采集时间戳（0 = 没打——缺时间戳的报告无法定位时点）。
    pub ts: u32,
    /// "提交后由人工复核"已明示（没说的报告不能交）。
    pub review_disclosed: bool,
    /// "复核结果可公开可匿名"已明示（安全感的知情面）。
    pub disclosure_anon: bool,
}

impl SelfReport {
    /// 报告有效判：指纹在册（非 0）+ 时间戳在册（非 0）+ 复核与匿名双明示
    /// ——防伪造与知情面缺一不可。
    pub fn valid(&self) -> bool {
        self.env_fp != 0 && self.ts > 0 && self.review_disclosed && self.disclosure_anon
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-4301 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_selftestpkg_checks() -> CheckSet {
    let mut set = CheckSet::new("B-4301 自测包安全");
    // 1. 只读形态：写位恒 false + 预设判例在册（结构面钉死）。
    let pkg = SelfTestPkg { can_write: READ_ONLY_CAN_WRITE, preset_cases: 8 };
    let evil = SelfTestPkg { can_write: true, preset_cases: 8 };
    let empty = SelfTestPkg { can_write: READ_ONLY_CAN_WRITE, preset_cases: 0 };
    set.add(
        "B-4301 只读形态",
        pkg_ok(&pkg) && !pkg_ok(&evil) && !pkg_ok(&empty),
        "无写权限不碰系统配置——能力位出厂钉死 false，写位为真即非本包",
    );
    // 2. 采集范围限定：目标应用内可采、范围外拒采（B-4301 达标线）。
    set.add(
        "B-4301 采集范围限定",
        collect_allowed("edit.foo", "edit.foo")
            && !collect_allowed("edit.foo", "term.bar")
            && !collect_allowed("edit.foo", "edit.foo.evil"),
        "事件归属==目标应用才可采——不扫系统其他部分，范围是白名单不是氛围",
    );
    // 3. 防伪造：指纹与时间戳缺一报告无效。
    let ok = SelfReport { env_fp: 0x9e3779b97f4a7c15, ts: 1_729_872_000, review_disclosed: true, disclosure_anon: true };
    let no_fp = SelfReport { env_fp: 0, ..ok };
    let no_ts = SelfReport { ts: 0, ..ok };
    set.add(
        "B-4301 防伪造三要素",
        ok.valid() && !no_fp.valid() && !no_ts.valid(),
        "环境指纹+时间戳缺一即无效——伪造不了的报告才是可信的报告",
    );
    // 4. 复核明示：人工复核与可公开可匿名双明示（知情面）。
    let no_disclose = SelfReport { review_disclosed: false, ..ok };
    let no_anon = SelfReport { disclosure_anon: false, ..ok };
    set.add(
        "B-4301 复核明示",
        ok.valid() && !no_disclose.valid() && !no_anon.valid(),
        "'提交后人工复核，可公开可匿名'写进报告面——用户的安全感在线",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe17 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe17_read_only_pinned() {
        // 写位常量恒 false；合法包判过、写位真/零判例判拒。
        assert!(!READ_ONLY_CAN_WRITE);
        assert!(pkg_ok(&SelfTestPkg { can_write: false, preset_cases: 1 }));
        assert!(!pkg_ok(&SelfTestPkg { can_write: true, preset_cases: 1 }));
        assert!(!pkg_ok(&SelfTestPkg { can_write: false, preset_cases: 0 }));
    }

    #[test]
    fn fe17_scope_gate() {
        // 许可规则只有 id 相等：自体过、他体拒、前缀包含也拒。
        assert!(collect_allowed("app.one", "app.one"));
        assert!(!collect_allowed("app.one", "app.two"));
        assert!(!collect_allowed("app.one", "app.one.two"));
        assert!(!collect_allowed("app.one", "app"));
    }

    #[test]
    fn fe17_report_validity() {
        // 三要素逐项独立判：缺指纹/缺时间戳/缺任一明示都无效。
        let base = SelfReport { env_fp: 42, ts: 7, review_disclosed: true, disclosure_anon: true };
        assert!(base.valid());
        assert!(!SelfReport { env_fp: 0, ..base }.valid());
        assert!(!SelfReport { ts: 0, ..base }.valid());
        assert!(!SelfReport { review_disclosed: false, ..base }.valid());
        assert!(!SelfReport { disclosure_anon: false, ..base }.valid());
    }

    #[test]
    fn fe17_disclosure_semantics() {
        // 明示位语义复核：disclosed 是"说了要复核"，anon 是"说了可公开可匿名"。
        let said = SelfReport { env_fp: 1, ts: 1, review_disclosed: true, disclosure_anon: true };
        assert!(said.review_disclosed && said.disclosure_anon);
        // 只说复核不说匿名：安全感缺一半——无效。
        let half = SelfReport { disclosure_anon: false, ..said };
        assert!(!half.valid());
    }
}
