//! 前缀模板与生成器（WP-302 · B-1002 生成产物与模板版本可追溯）。
//!
//! MD2 篇 10.2："每个应用一个独立片场"制度化——模板五要素（Windows
//! 版本伪装/字体注入表/代码页映射/默认环境/DPI 绑定声明）+ 按模板实例化
//! （/home/wine/〈应用名〉）+ **实例化产物带模板版本号**（可追溯达标线）。
//! 模板升级后旧前缀标记"可升级"，升级即重建（23.2 重建优于手术）；
//! 应用自改（安装器改注册表，正常业务记星卡）与模板级篡改（绕过生成器
//! 手工造前缀，会话服务拒绝）分清界限。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 模板五要素（篇 10.2 冻结面）
// ---------------------------------------------------------------------------

/// 模板版本号（单调递增——升级即递增，产物可追溯的锚）。
pub type TemplateVer = u32;

/// 初始模板版本（1 起，0 留给"未实例化"哨兵）。
pub const TEMPLATE_VER_FIRST: TemplateVer = 1;

/// 前缀模板五要素（MD2 篇 10.2：版本伪装/字体注入/代码页/默认环境/DPI）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrefixTemplate {
    /// Windows 版本伪装值（如 win10）。
    pub win_ver: u32,
    /// 字体注入表项数（宋体雅黑 → 开源映射，MD1 23.5）。
    pub font_entries: u32,
    /// 代码页映射（中文文件名判例 SC-053 的关键）。
    pub codepage: u32,
    /// 默认环境项数（PATH/LANG 等统一下发面）。
    pub env_entries: u32,
    /// DPI 绑定声明（缩放因子换算在桥内完成）。
    pub dpi_bound: bool,
}

impl PrefixTemplate {
    /// 五要素齐：字段全非零（DPI 绑定必声明）。
    pub fn complete(&self) -> bool {
        self.win_ver > 0
            && self.font_entries > 0
            && self.codepage > 0
            && self.env_entries > 0
            && self.dpi_bound
    }
}

/// 内置模板 v1（篇 10.2 缺省面：win10 伪装 + 中文字体映射 + 65001 + DPI 绑定）。
pub const TEMPLATE_V1: PrefixTemplate = PrefixTemplate {
    win_ver: 10,
    font_entries: 4,
    codepage: 65001,
    env_entries: 6,
    dpi_bound: true,
};

// ---------------------------------------------------------------------------
// 实例化与可追溯（B-1002 达标线：产物与模板版本可追溯）
// ---------------------------------------------------------------------------

/// 前缀实例化产物（/home/wine/〈应用名〉的模型面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrefixInstance {
    pub app_id: u32,
    /// 实例化所用模板版本——**产物可追溯的锚**。
    pub templ_ver: TemplateVer,
    pub from_template: bool,
}

/// 按模板实例化：产物带模板版本号（可追溯）。
pub fn instantiate(app_id: u32, t: &PrefixTemplate, ver: TemplateVer) -> Option<PrefixInstance> {
    if app_id == 0 || !t.complete() || ver == 0 {
        return None;
    }
    Some(PrefixInstance { app_id, templ_ver: ver, from_template: true })
}

/// 升级判定：产物版本旧于当前模板版本 → 标记"可升级"。
pub fn upgrade_available(inst: &PrefixInstance, current_ver: TemplateVer) -> bool {
    inst.templ_ver < current_ver
}

/// 升级即重建（23.2：重建优于手术）——返回新模板版本的新实例，不是原地改。
pub fn upgrade_rebuild(inst: &PrefixInstance, t: &PrefixTemplate, current_ver: TemplateVer) -> Option<PrefixInstance> {
    if !upgrade_available(inst, current_ver) {
        return None; // 已是最新——无升级可做
    }
    instantiate(inst.app_id, t, current_ver)
}

// ---------------------------------------------------------------------------
// 来源自治（自改记星卡 / 模板级篡改拒绝——篇 10.2 分界）
// ---------------------------------------------------------------------------

/// 前缀来源（穷举三态：模板实例化/应用自改/模板级篡改）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provenance {
    /// 生成器按模板实例化（正路）。
    Templated,
    /// 应用自带安装器改注册表等（正常业务——记录星卡）。
    AppModified,
    /// 绕过生成器手工造前缀（模板级篡改——会话服务拒绝）。
    Forged,
}

/// 来源裁决：篡改一律拒；自改放行但要求记星卡；模板产物直通。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Allow,
    AllowWithStarCard,
    Reject,
}

pub fn provenance_verdict(p: Provenance) -> Verdict {
    match p {
        Provenance::Templated => Verdict::Allow,
        Provenance::AppModified => Verdict::AllowWithStarCard,
        Provenance::Forged => Verdict::Reject,
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-1002 · 6 项）
// ---------------------------------------------------------------------------

pub fn run_winepfx_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1002 前缀模板一致性");
    // 1. 模板五要素齐：缺一不完整。
    set.add(
        "B-1002 模板五要素齐",
        TEMPLATE_V1.complete()
            && !PrefixTemplate { win_ver: 10, font_entries: 0, codepage: 65001, env_entries: 6, dpi_bound: true }.complete(),
        "版本伪装/字体注入/代码页/默认环境/DPI 绑定——缺一不完整",
    );
    // 2. 实例化带版本号：产物可追溯（B-1002 达标线）。
    let inst = instantiate(42, &TEMPLATE_V1, TEMPLATE_VER_FIRST);
    set.add(
        "B-1002 实例化带版本",
        inst.is_some() && inst.unwrap().templ_ver == TEMPLATE_VER_FIRST && inst.unwrap().from_template,
        "生成产物与模板版本可追溯——每个前缀知道自己从哪个模板来",
    );
    // 3. 非法实例化拒：零组号/不完整模板/零版本一律 None。
    let half = PrefixTemplate { win_ver: 10, font_entries: 4, codepage: 0, env_entries: 6, dpi_bound: true };
    set.add(
        "B-1002 非法实例化拒",
        instantiate(0, &TEMPLATE_V1, 1).is_none()
            && instantiate(42, &half, 1).is_none()
            && instantiate(42, &TEMPLATE_V1, 0).is_none(),
        "无主前缀/残缺模板/零版本——生成器入口全清洗",
    );
    // 4. 升级标记与重建优于手术：旧版本标可升级，升级产生新实例非原地改。
    let v2 = TEMPLATE_VER_FIRST + 1;
    let old = instantiate(42, &TEMPLATE_V1, TEMPLATE_VER_FIRST).unwrap();
    let rebuilt = upgrade_rebuild(&old, &TEMPLATE_V1, v2);
    set.add(
        "B-1002 升级即重建",
        upgrade_available(&old, v2) && rebuilt.is_some() && rebuilt.unwrap().templ_ver == v2,
        "模板升级后旧前缀标记可升级——升级即重建不是原地改（23.2）",
    );
    // 5. 最新版本无升级可做（幂等——不重复重建）。
    let fresh = instantiate(42, &TEMPLATE_V1, v2).unwrap();
    set.add(
        "B-1002 最新版不重建",
        !upgrade_available(&fresh, v2) && upgrade_rebuild(&fresh, &TEMPLATE_V1, v2).is_none(),
        "已是当前版本的前缀不动——重建只对落后的做",
    );
    // 6. 来源自治：自改记星卡、篡改拒绝（篇 10.2 分界线）。
    set.add(
        "B-1002 自改与篡改分界",
        provenance_verdict(Provenance::Templated) == Verdict::Allow
            && provenance_verdict(Provenance::AppModified) == Verdict::AllowWithStarCard
            && provenance_verdict(Provenance::Forged) == Verdict::Reject,
        "安装器自改正常业务记星卡；绕过生成器手工造前缀会话服务拒绝",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe02 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe02_template_complete_and_instantiate() {
        assert!(TEMPLATE_V1.complete());
        assert_eq!(TEMPLATE_V1.codepage, 65001);
        let inst = instantiate(7, &TEMPLATE_V1, TEMPLATE_VER_FIRST).unwrap();
        assert_eq!(inst.app_id, 7);
        assert_eq!(inst.templ_ver, 1);
        assert!(inst.from_template);
    }

    #[test]
    fn fe02_traceability_version_chain() {
        // 版本链：v1 实例 → 模板升 v2 → 可升级 → 重建后带 v2。
        let i1 = instantiate(9, &TEMPLATE_V1, 1).unwrap();
        assert!(upgrade_available(&i1, 2));
        let i2 = upgrade_rebuild(&i1, &TEMPLATE_V1, 2).unwrap();
        assert_eq!(i2.templ_ver, 2);
        assert!(!upgrade_available(&i2, 2));
        // 降级声明（产物版本比"当前"还新）不算可升级。
        let i3 = instantiate(9, &TEMPLATE_V1, 3).unwrap();
        assert!(!upgrade_available(&i3, 2));
    }

    #[test]
    fn fe02_rebuild_not_inplace() {
        // 重建产生新实例且版本前进——旧实例字段不被改动（重建优于手术）。
        let i1 = instantiate(5, &TEMPLATE_V1, 1).unwrap();
        let snapshot = i1;
        let _i2 = upgrade_rebuild(&i1, &TEMPLATE_V1, 4).unwrap();
        assert_eq!(i1, snapshot); // 旧实例原样（可追溯的历史不被篡改）
    }

    #[test]
    fn fe02_provenance_exhaustive_verdict() {
        // 三态穷举裁决：Allow/AllowWithStarCard/Reject 各有人口。
        let all = [Provenance::Templated, Provenance::AppModified, Provenance::Forged];
        let mut allow = 0;
        let mut star = 0;
        let mut reject = 0;
        for p in all {
            match provenance_verdict(p) {
                Verdict::Allow => allow += 1,
                Verdict::AllowWithStarCard => star += 1,
                Verdict::Reject => reject += 1,
            }
        }
        assert_eq!((allow, star, reject), (1, 1, 1));
        // 篡改零放行——会话服务拒绝是硬门。
        assert_eq!(provenance_verdict(Provenance::Forged), Verdict::Reject);
    }
}
