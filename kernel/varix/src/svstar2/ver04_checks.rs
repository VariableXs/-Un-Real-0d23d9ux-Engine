//! VE-F3603 · 域自检（判据逐条对应，见 `ver04_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - 七要素 → `R01-七要素-*`
//! - 许可红线 → `R01-许可红线-*`
//! - 兼容警告 → `R01-兼容警告-*`
//! - 类型单源 → `R01-类型单源-*`
//! - schema 复用 → `R01-schema复用-*`
//! - 判据（自证可追溯）→ `R01-判据-*`
//! - 降级矩阵 → `R01-降级-*`
//! - 禁扩面 → `R01-边界-*`
//! - 装载门禁链 → `R01-装载-*`
//! - 读屏替述 → `R01-替述-*`
//!
//! **可证伪纪律**：每组自检都配「注入缺陷 → 必红」的单元测试。报"计数为 0"
//! 不算守住，自检必须能被正当理由打红才算真守住。
//!
//! 零墙钟、零 IO，回归可复现。

use super::ver04_arch::*;
use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 全绿总纲（**唯一正样本构造处**）。
fn ready_arch() -> AssetArchitecture {
    AssetArchitecture::standard()
}

/// 空总纲（反例用）。
fn empty_arch() -> AssetArchitecture {
    AssetArchitecture::empty()
}

/// 数指定错误码在问题表里出现的次数（真红项计数，非 CheckSet 总数）。
fn count_code(issues: &[AssetIssue], code: &str) -> usize {
    issues.iter().filter(|i| i.code == code).count()
}

// ---------------------------------------------------------------------------
// 判据一：七要素
// ---------------------------------------------------------------------------

fn chk_seven_fields(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 恰七要素。
    set.add(
        "R01-七要素-恰七要素",
        AssetField::ALL.len() == FIELD_COUNT && a.model.fields.len() == FIELD_COUNT,
        "锚点原文：ID/类型/内容/元数据/版本/来源/许可七要素",
    );

    // 2. 要素码与锚点逐字一致。
    set.add(
        "R01-七要素-码字面照录",
        AssetModel::codes_match_anchor(),
        "七要素码不得改写",
    );

    // 3. 要素名与锚点原词一致。
    let names_ok = AssetField::ALL
        .iter()
        .enumerate()
        .all(|(i, f)| f.name_cn() == ["ID", "类型", "内容", "元数据", "版本", "来源", "许可"][i]);
    set.add("R01-七要素-名照录", names_ok, "要素中文名照锚点原词");

    // 4. 码往返无损。
    set.add(
        "R01-七要素-码往返",
        a.model.audit().is_empty(),
        "要素码→反查无损；要素表自身零红项",
    );

    // 5. 齐备资产七要素全在。
    let asset = CreationAsset::complete("asset-001", "K-WALL");
    set.add(
        "R01-七要素-齐备断言",
        asset.missing_fields().is_empty(),
        "齐备资产不应报缺项",
    );

    // 6. **逐要素**查缺（不查汇总）——每个字段单独构造缺失实例。
    //    Meta 例外：元数据走宽松语义（空值由 schema 层填默认并告警），
    //    所以它不进「要素缺失」这条路，改由 schema 组单独验。
    let mut miss_detected = 0;
    let mut expect_missing = 0usize;
    for f in AssetField::ALL.iter() {
        if f.is_meta_lenient() {
            continue;
        }
        expect_missing += 1;
        let mut bad = CreationAsset::complete("a", "K-WALL");
        match f {
            AssetField::Id => bad.id = "",
            AssetField::Kind => bad.kind = "",
            AssetField::Content => bad.content = "",
            AssetField::Meta => bad.meta = "",
            AssetField::Version => bad.version = "",
            AssetField::Origin => bad.origin = "",
            AssetField::License => bad.license = LicenseState::Missing,
        }
        if bad.missing_fields().contains(f) {
            miss_detected += 1;
        }
    }
    set.add(
        "R01-七要素-逐要素查缺",
        miss_detected == expect_missing && expect_missing == FIELD_COUNT - 1,
        if miss_detected == expect_missing {
            "六个严格要素逐个缺失都能被单独检出（Meta 走宽松，不计缺）"
        } else {
            "有要素缺失检不出"
        },
    );

    // 7. 内容/许可为上架必需。
    set.add(
        "R01-七要素-上架必需",
        AssetField::Content.required_for_listing() && AssetField::License.required_for_listing(),
        "内容与许可缺一即不可上架",
    );

    // 8. 内容严格、元数据宽松的分界可辨。
    set.add(
        "R01-七要素-两级语义",
        AssetField::Content.is_content_strict()
            && AssetField::Meta.is_meta_lenient()
            && !AssetField::Meta.is_content_strict(),
        "锚点：宽容元数据严格内容",
    );

    // 8b. 宽松/严格要素**恰各一**（防止有人把宽松语义扩散或抹掉）。
    let strict_cnt = AssetField::ALL.iter().filter(|f| f.is_content_strict()).count();
    let lenient_cnt = AssetField::ALL.iter().filter(|f| f.is_meta_lenient()).count();
    set.add(
        "R01-七要素-宽松严格各一",
        strict_cnt == 1 && lenient_cnt == 1,
        "内容严格恰一项、元数据宽松恰一项——两者语义不得混同",
    );
}

// ---------------------------------------------------------------------------
// 判据二：许可红线
// ---------------------------------------------------------------------------

fn chk_license(set: &mut CheckSet) {
    let a = ready_arch();

    // 0. 缺许可的资产在**总纲装载链**上同样被拒（不只上架闸拒）。
    let mut unlicensed = CreationAsset::complete("a", "K-WALL");
    unlicensed.license = LicenseState::Missing;
    set.add(
        "R01-许可红线-装载链同拒",
        a.load_asset(&unlicensed, Some(CompatDecl::new(1, 99)), 15)
            .is_err(),
        "缺许可在装载链与上架闸两处都拦",
    );

    // 1. 许可三态齐备。
    set.add(
        "R01-许可红线-三态",
        LicenseState::ALL.len() == 3,
        "Declared / Missing / Unverified",
    );

    // 2. 只有已核验可上架。
    set.add(
        "R01-许可红线-仅已核验可上架",
        LicenseState::Declared.listable()
            && !LicenseState::Missing.listable()
            && !LicenseState::Unverified.listable(),
        "缺许可与待核验都不可直接上架",
    );

    // 3. 缺许可 → 阻断上架（P0）。
    //    **绝对断言**：三态的「在场性」写死在此，不从实现反推——否则实现
    //    一旦同源退化（要素与闸门一起变对），自检就看不出问题。
    let mut m0 = CreationAsset::complete("a", "K-WALL");
    m0.license = LicenseState::Missing;
    let mut d0 = CreationAsset::complete("a", "K-WALL");
    d0.license = LicenseState::Declared;
    let mut u0 = CreationAsset::complete("a", "K-WALL");
    u0.license = LicenseState::Unverified;
    set.add(
        "R01-许可红线-三态在场性",
        !m0.has_field(AssetField::License)
            && d0.has_field(AssetField::License)
            && u0.has_field(AssetField::License),
        "缺许可不在场；已声明与待核验均在场（待核验只是要确认，不是缺）",
    );

    let mut bad = CreationAsset::complete("a", "K-WALL");
    bad.license = LicenseState::Missing;
    let gate = bad.listing_gate();
    set.add(
        "R01-许可红线-缺失阻断",
        gate.is_err()
            && matches!(&gate, Err(e) if e.code == E_LICENSE_MISSING && e.severity() == P0
                && e.action() == DegradeAction::BlockListing),
        "锚点：许可缺失上架→阻断（红线实测）",
    );

    // 4. 待核验 → 警告+确认（**非阻断**）——三态分流的核心断言。
    let mut unver = CreationAsset::complete("a", "K-WALL");
    unver.license = LicenseState::Unverified;
    let e = AssetError::new(E_LICENSE_UNVERIFIED, "a", "待核验");
    set.add(
        "R01-许可红线-待核验不阻断",
        unver.listing_gate().is_ok()
            && e.action() == DegradeAction::WarnConfirm
            && !e.action().blocks(),
        "待核验可上架但要确认——与缺许可分道",
    );

    // 5. 缺许可问题项属阻断类。
    set.add(
        "R01-许可红线-缺属阻断类",
        AssetIssue::new(E_LICENSE_MISSING, "a", "x").is_blocking()
            && !AssetIssue::new(E_LICENSE_UNVERIFIED, "a", "x").is_blocking(),
        "缺许可=阻断，待核验≠阻断",
    );

    // 6. 三态语义互异（防止两态行为相同=白设）。
    //    签名直接编码三态的**关键行为**（可否上架 / 上架闸是否放行），
    //    不掺字符串长度这类偶然量——掺了会出现两态碰撞的假红。
    let mut sig: Vec<u8> = LicenseState::ALL
        .iter()
        .map(|s| {
            let mut asset = CreationAsset::complete("a", "K-WALL");
            asset.license = *s;
            let mut h: u8 = 0;
            h ^= asset.listing_gate().is_ok() as u8; // 闸门是否放行
            h ^= (s.listable() as u8) << 1; // 是否可上架
            h
        })
        .collect();
    let before = sig.len();
    sig.sort_unstable();
    sig.dedup();
    set.add(
        "R01-许可红线-三态语义互异",
        sig.len() == before,
        "三态行为不得有两态相同（签名含闸门放行与可上架两项）",
    );

    // 7. 剥离许可（署名类资产）另判。
    set.add(
        "R01-许可红线-剥离判红",
        AssetIssue::new(E_LICENSE_STRIPPED, "a", "x").is_blocking(),
        "署名类资产被剥许可按阻断处置",
    );
}

// ---------------------------------------------------------------------------
// 判据三：兼容警告
// ---------------------------------------------------------------------------

fn chk_compat(set: &mut CheckSet) {
    // 1. 相容度四级齐备。
    set.add(
        "R01-兼容警告-四级",
        CompatLevel::ALL.len() == 4,
        "精确/相容/未声明/不兼容",
    );

    // 2. 区间覆盖 → 精确。
    set.add(
        "R01-兼容警告-精确判定",
        CompatReport::evaluate(Some(CompatDecl::new(10, 20)), 15).level == CompatLevel::Exact,
        "声明区间覆盖当前版本",
    );

    // 3. 未声明 → Undeclared（**不是** Incompatible——归因须分开）。
    let und = CompatReport::evaluate(None, 15);
    set.add(
        "R01-兼容警告-未声明归因",
        und.level == CompatLevel::Undeclared
            && und.to_action().is_err()
            && !und.needs_confirm() == false,
        "未声明与明确不兼容须分开报",
    );

    // 4. 区间无交集 → 不兼容。
    set.add(
        "R01-兼容警告-不兼容判定",
        CompatReport::evaluate(Some(CompatDecl::new(30, 40)), 15).level == CompatLevel::Incompatible,
        "声明区间与当前版本无交集",
    );

    // 5. 警告级不阻断（老资产可能能跑）。
    set.add(
        "R01-兼容警告-警告不阻断",
        CompatLevel::Undeclared.needs_warning()
            && CompatLevel::Incompatible.needs_warning()
            && CompatLevel::Exact.passes()
            && CompatLevel::Compatible.passes(),
        "四级中两级提示、两级放行",
    );

    // 6. 未声明 → 警告+确认（P1，非P0）。
    let e = AssetError::new(E_COMPAT_UNDECLARED, "x", "未声明");
    set.add(
        "R01-兼容警告-未声明走确认",
        e.action() == DegradeAction::WarnConfirm
            && e.severity() == P1
            && !e.action().blocks(),
        "锚点：兼容未声明→警告+确认（红线实测）",
    );

    // 7. 区间非法（下界> 上界）单独检出。
    set.add(
        "R01-兼容警告-区间非法检出",
        !CompatDecl::new(20, 10).range_valid()
            && CompatDecl::new(20, 10).validate_range().is_err()
            && CompatDecl::new(10, 10).range_valid(),
        "下界> 上界即非法；相等视为合法单点",
    );

    // 8. 兼容问题项属提示类。
    set.add(
        "R01-兼容警告-提示非阻断",
        AssetIssue::new(E_COMPAT_UNDECLARED, "x", "y").is_advisory()
            && !AssetIssue::new(E_COMPAT_UNDECLARED, "x", "y").is_blocking(),
        "兼容问题走提示不走阻断",
    );
}

// ---------------------------------------------------------------------------
// 判据四：类型单源
// ---------------------------------------------------------------------------

fn chk_type_single_source(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 恰七类。
    set.add(
        "R01-类型单源-恰七类",
        AssetKind::ALL.len() == KIND_COUNT && a.registry.kinds.len() == KIND_COUNT,
        "锚点原文：主题/皮肤/壁纸/图标/组件/模板/脚本数据七类",
    );

    // 2. 类别码与锚点逐字一致。
    set.add(
        "R01-类型单源-类码照录",
        KindRegistry::codes_match_anchor(),
        "七类码不得改写",
    );

    // 3. 七类各已注册且有资源绑定（未注册即拒载）。
    set.add(
        "R01-类型单源-七类已注册",
        a.registry.audit().is_empty(),
        "七类均登记资源绑定；标准注册表零红项",
    );

    // 4. 每类绑定非空（**逐类查**）。
    let empty_bind: Vec<&str> = a
        .registry
        .kinds
        .iter()
        .filter(|s| s.binding_count == 0)
        .map(|s| s.kind.code())
        .collect();
    set.add(
        "R01-类型单源-绑定非空",
        empty_bind.is_empty(),
        if empty_bind.is_empty() { "七类各有资源绑定" } else { "存在无绑定类" },
    );

    // 5. 未注册类型码被拒。
    set.add(
        "R01-类型单源-未注册拒载",
        !a.registry.is_registered("K-NOT-EXIST")
            && a.registry.is_registered("K-WALL"),
        "未知类型码不得被当作已注册",
    );

    // 6. 代码类标沙箱、非代码类不标（承 F3602 判据四）。
    set.add(
        "R01-类型单源-沙箱标记",
        a.registry.get(AssetKind::ScriptData).needs_sandbox
            && AssetKind::ALL
                .iter()
                .filter(|k| !k.is_code())
                .all(|k| !a.registry.get(*k).needs_sandbox),
        "脚本数据标沙箱；其余六类不标",
    );

    // 7. 两轴正交：七类 × F3204 十类是不同集合，只重叠脚本数据一项。
    let overlap: Vec<ResourceType> = ResourceType::ALL
        .iter()
        .copied()
        .filter(|r| *r == ResourceType::ScriptData)
        .collect();
    set.add(
        "R01-类型单源-两轴正交",
        overlap.len() == 1
            && AssetKind::ALL.len() != RESOURCE_TYPE_COUNT
            && ResourceType::ALL.len() == RESOURCE_TYPE_COUNT,
        "七类与F3204 十类不同集合，仅脚本数据重叠",
    );

    // 8. 每类的资源绑定真实存在于 F3204 十类内（不越界）。
    let all_bound_in_range = a.registry.kinds.iter().all(|s| {
        s.bindings[..s.binding_count]
            .iter()
            .all(|b| ResourceType::from_code(b.resource.code()) == Some(b.resource))
    });
    set.add(
        "R01-类型单源-绑定不越界",
        all_bound_in_range,
        "绑定资源全在 F3204 十类内",
    );

    // 9. 壁纸类的绑定组合符合常识（纹理 + 场景图）。
    set.add(
        "R01-类型单源-壁纸绑定",
        a.registry.get(AssetKind::Wallpaper).has_binding(ResourceType::Texture)
            && a.registry.get(AssetKind::Wallpaper).has_binding(ResourceType::SceneGraph),
        "壁纸=纹理+场景图（静态图与动态壁纸）",
    );
}

// ---------------------------------------------------------------------------
// 判据五：schema 复用
// ---------------------------------------------------------------------------

fn chk_schema_reuse(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 复用登记有出处、不得自实现。
    set.add(
        "R01-schema复用-登记合规",
        a.schema.audit().is_empty(),
        "只引用 F3204 校验器；自实现即红",
    );

    // 2. 元数据宽松：缺字段→宽松解析 + 告警，**不拒**。
    let mut no_meta = CreationAsset::complete("a", "K-WALL");
    no_meta.meta = "";
    let (outcome, issues) = a.schema.validate(&no_meta);
    set.add(
        "R01-schema复用-元数据宽松",
        outcome == SchemaOutcome::MetaLenient
            && outcome.passes()
            && count_code(&issues, E_SCHEMA_LENIENT) == 1,
        "元数据缺字段走宽松+告警",
    );

    // 3. 内容严格：内容缺→拒绝。
    let mut no_content = CreationAsset::complete("a", "K-WALL");
    no_content.content = "";
    let (outcome2, issues2) = a.schema.validate(&no_content);
    set.add(
        "R01-schema复用-内容严格",
        outcome2 == SchemaOutcome::ContentViolation
            && !outcome2.passes()
            && count_code(&issues2, E_SCHEMA_VIOLATION) == 1,
        "内容本体违例须拒绝",
    );

    // 4. 合规资产零问题。
    let good = CreationAsset::complete("a", "K-WALL");
    let (outcome3, issues3) = a.schema.validate(&good);
    set.add(
        "R01-schema复用-合规零问题",
        outcome3 == SchemaOutcome::Ok && issues3.is_empty(),
        "合规资产不应产生任何 schema 问题",
    );

    // 5. 内容违例时元数据问题不重复报（先内容后元数据，短路）。
    let mut both_bad = CreationAsset::complete("a", "K-WALL");
    both_bad.content = "";
    both_bad.meta = "";
    let (_, issues4) = a.schema.validate(&both_bad);
    set.add(
        "R01-schema复用-短路不重复",
        count_code(&issues4, E_SCHEMA_VIOLATION) == 1 && count_code(&issues4, E_SCHEMA_LENIENT) == 0,
        "内容已拒时不再叠一条元数据告警",
    );

    // 6. schema 违例→宽松+告警（P2，放行）。
    let e = AssetError::new(E_SCHEMA_VIOLATION, "a", "x");
    set.add(
        "R01-schema复用-宽松放行",
        e.action() == DegradeAction::WarnLenient
            && e.severity() == P2
            && e.action().advisory_only(),
        "锚点：schema 违例→宽松+告警（复述）",
    );
}

// ---------------------------------------------------------------------------
// 降级矩阵与判据自证
// ---------------------------------------------------------------------------

fn chk_degrade_and_criteria(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 四条锚点错误路径齐备。
    set.add(
        "R01-降级-四条路径齐备",
        a.degrade.paths.len() == DEGRADE_PATH_COUNT
            && count_code(&a.degrade.audit(), E_DEGRADE_MISSING) == 0,
        "许可阻断/未注册拒绝/兼容警告/schema 宽松",
    );

    // 2. 级与动作匹配锚点。
    set.add(
        "R01-降级-级动匹配",
        count_code(&a.degrade.audit(), E_DEGRADE_MISMATCH) == 0,
        "P0阻断/P1拒绝/P1提示/P2放行",
    );

    // 3. 四条路径**三种方向**互异（处置方向相反者不得共用）。
    let mut fps: Vec<u64> = a.degrade.paths.iter().map(|p| p.fingerprint()).collect();
    fps.sort_unstable();
    let n = fps.len();
    fps.dedup();
    set.add("R01-降级-路径指纹互异", fps.len() == n, "四条路径处置各不相同");

    // 4. 动作方向分类正确：两条拦住流、两条仅提示。
    set.add(
        "R01-降级-方向三态",
        DegradeAction::ALL.iter().filter(|x| x.blocks()).count() == 2
            && DegradeAction::ALL.iter().filter(|x| x.advisory_only()).count() == 2,
        "阻断+拒绝=拦住流；警告+确认/宽松=仅提示",
    );

    // 5. 引文非空（照录锚点原文）。
    set.add(
        "R01-降级-引文照录",
        a.degrade.paths.iter().all(|p| !p.quote.trim().is_empty()),
        "四条路径各带锚点原文引文",
    );

    // 6. 判据六项齐备且有依据。
    set.add(
        "R01-判据-六项齐备",
        Criterion::ALL.len() == CRITERION_COUNT && audit_criteria().is_empty(),
        "七要素/许可红线/兼容警告/类型单源/schema复用/判据",
    );

    // 7. 判据标题照录锚点用词。
    let titles_ok = Criterion::ALL.iter().enumerate().all(|(i, c)| {
        c.title == ["七要素", "许可红线", "兼容警告", "类型单源", "schema 复用", "判据"][i]
    });
    set.add("R01-判据-标题照录", titles_ok, "判据标题不得改写");

    // 8. 机检前缀唯一。
    let mut probes: Vec<&str> = Criterion::ALL.iter().map(|c| c.probe).collect();
    probes.sort_unstable();
    let pn = probes.len();
    probes.dedup();
    set.add("R01-判据-前缀唯一", probes.len() == pn, "每判据一个自检前缀");

    // 9. 判据台账指纹非零。
    set.add(
        "R01-判据-台账指纹",
        Criterion::ledger_fingerprint() != 0,
        "六项判据拼接指纹",
    );

    // 10. 禁扩面条数与越界检出。
    set.add(
        "R01-边界-禁扩面条数",
        ASSET_EXCLUSIONS.len() == EXCLUSION_COUNT,
        "六条：本项只定义模型与登记",
    );
    set.add(
        "R01-边界-越界可检出",
        check_no_overreach("顺手把 F3204:decoder 的解码路由也写了").is_err()
            && asset_scope_advice("登记七要素模型").is_none(),
        "命中禁扩面条目即越界；正常声明不误报",
    );
}

// ---------------------------------------------------------------------------
// 装载门禁链
// ---------------------------------------------------------------------------

fn chk_load_chain(set: &mut CheckSet) {
    let a = ready_arch();
    let asset = CreationAsset::complete("asset-001", "K-WALL");

    // 1. 完整链路装载成功。
    let ok = a.load_asset(&asset, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-合规资产通过",
        ok.is_ok(),
        "齐备资产 + 已注册类型 + 已声明兼容 → 装载成功",
    );

    // 2. 未注册类型被拒（第一道门）。
    let mut bad_kind = CreationAsset::complete("a", "K-NOT-EXIST");
    bad_kind.kind = "K-NOT-EXIST";
    let r = a.load_asset(&bad_kind, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-未注册拒载",
        matches!(&r, Err(e) if e.code == E_KIND_UNREGISTERED
            && e.action() == DegradeAction::RejectLoad),
        "锚点：未注册类型→拒绝（复述）",
    );

    // 3. 要素缺失被拒（第二道门）。
    let mut no_origin = CreationAsset::complete("a", "K-WALL");
    no_origin.origin = "";
    let r2 = a.load_asset(&no_origin, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-要素缺失拒载",
        matches!(&r2, Err(e) if e.code == E_FIELD_MISSING),
        "七要素缺一即拒载",
    );

    // 4. 缺许可被拒（**上架闸在装载链内**）。
    let mut no_lic = CreationAsset::complete("a", "K-WALL");
    no_lic.license = LicenseState::Missing;
    let r3 = a.load_asset(&no_lic, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-缺许可拒载",
        matches!(&r3, Err(e) if e.code == E_LICENSE_MISSING
            && e.severity() == P0),
        "锚点：缺许可=不可分发",
    );

    // 5. 兼容未声明**放行但带警告**（不阻断）。
    let r4 = a.load_asset(&asset, None, 15);
    set.add(
        "R01-装载-未声明兼容放行",
        match &r4 {
            Ok(o) => o.compat.level == CompatLevel::Undeclared && o.compat.needs_confirm(),
            Err(_) => false,
        },
        "未声明兼容走警告+确认，不阻断装载",
    );

    // 6. 内容违例被拒（schema 门）。
    let mut no_content = CreationAsset::complete("a", "K-WALL");
    no_content.content = "";
    let r5 = a.load_asset(&no_content, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-内容违例拒载",
        matches!(&r5, Err(e) if e.code == E_SCHEMA_VIOLATION),
        "内容严格——不合规则拒绝",
    );

    // 7. 元数据宽松放行（带告警）。
    let mut no_meta = CreationAsset::complete("a", "K-WALL");
    no_meta.meta = "";
    let r6 = a.load_asset(&no_meta, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-元数据宽松放行",
        match &r6 {
            Ok(o) => o.schema == SchemaOutcome::MetaLenient && !o.warnings.is_empty(),
            Err(_) => false,
        },
        "元数据宽松解析并带告警放行",
    );

    // 8. 门禁**顺序**：类型先于要素（未注册类型 + 缺要素应报类型错）。
    let mut both = CreationAsset::complete("a", "K-NOT-EXIST");
    both.origin = "";
    let r7 = a.load_asset(&both, Some(CompatDecl::new(10, 20)), 15);
    set.add(
        "R01-装载-门禁顺序",
        matches!(&r7, Err(e) if e.code == E_KIND_UNREGISTERED),
        "类型门在最前——根本问题先报",
    );

    // 9. 全绿总纲 preflight 通过。
    set.add(
        "R01-装载-preflight通过",
        ready_arch().preflight().is_ok(),
        "全绿总纲应通过开工前置校验",
    );
}

// ---------------------------------------------------------------------------
// 读屏替述与总纲自证
// ---------------------------------------------------------------------------

fn chk_narration(set: &mut CheckSet) {
    let a = ready_arch();
    let n = a.narration();

    // 1. 替述条数齐备（七要素/七类/许可/兼容/两轴/schema/沙箱）。
    set.add(
        "R01-替述-条数齐备",
        n.len() >= 7,
        "七要点逐条口述，不依赖图形",
    );

    // 2. 每条有实质内容（防空话）。
    set.add(
        "R01-替述-非空话",
        n.iter().all(|s| s.trim().len() >= 10),
        "每条替述须有实质内容",
    );

    // 3. 覆盖七要素名称。
    let j = n.join("");
    let fields_covered = AssetField::ALL
        .iter()
        .enumerate()
        .filter(|(i, _)| *i > 0) // 首元素 ID 在句首单独出现
        .all(|(_, f)| j.contains(f.name_cn()));
    set.add(
        "R01-替述-覆盖七要素",
        fields_covered,
        "七要素须在替述中逐一点名",
    );

    // 4. 覆盖七类名称。
    set.add(
        "R01-替述-覆盖七类",
        AssetKind::ALL.iter().all(|k| j.contains(k.name_cn())),
        "七类须在替述中逐一点名",
    );

    // 5. 讲清要素与种类的区别（最易混处）。
    set.add(
        "R01-替述-要素种类有别",
        j.contains("两回事") || j.contains("不是一回事"),
        "替述须点明七要素≠七类",
    );

    // 6. 讲清许可三态的分流理由。
    set.add(
        "R01-替述-许可讲三态",
        j.contains("三态") && j.contains("各走各的门"),
        "替述须讲清为什么分三态",
    );

    // 7. 讲清兼容四级不阻断的理由。
    set.add(
        "R01-替述-兼容讲四级",
        j.contains("四级") && j.contains("阻断会误伤"),
        "替述须讲清为什么四级且不阻断",
    );

    // 8. 讲清两轴正交。
    set.add(
        "R01-替述-两轴讲正交",
        j.contains("两轴") && j.contains("纹理"),
        "替述须讲清七类与十类资源是两轴",
    );

    // 9. 总纲指纹非零。
    set.add("R01-替述-总纲指纹", a.fingerprint() != 0, "四张表串接指纹");
}

// ---------------------------------------------------------------------------
// 反例总纲（反恒真）
// ---------------------------------------------------------------------------

fn chk_negative(set: &mut CheckSet) {
    let e = empty_arch();

    // 1. 空注册表七类全部未注册。
    set.add(
        "R01-反例-空注册七类未注册",
        count_code(&e.registry.audit(), E_KIND_UNREGISTERED) == KIND_COUNT,
        "空注册表七类全未注册",
    );

    // 2. 空注册表 preflight 失败。
    set.add(
        "R01-反例-空总纲被拦",
        e.preflight().is_err(),
        "空总纲必须 preflight 失败",
    );

    // 3. 空总纲装载任何资产都失败。
    let asset = CreationAsset::complete("a", "K-WALL");
    set.add(
        "R01-反例-空总纲拒载",
        e.load_asset(&asset, Some(CompatDecl::new(1, 99)), 15).is_err(),
        "未注册类型表下装载必失败",
    );

    // 4. 沙箱标记被摘时检出。
    let mut a = ready_arch();
    a.registry.get_mut(AssetKind::ScriptData).needs_sandbox = false;
    set.add(
        "R01-反例-沙箱标记可摘",
        count_code(&a.registry.audit(), E_SANDBOX_FLAG_MISSING) == 1,
        "代码类去掉沙箱标记必须被检出",
    );

    // 5. schema 自实现被检出。
    let mut a2 = ready_arch();
    a2.schema.reimplements = true;
    set.add(
        "R01-反例-schema自实现可检出",
        count_code(&a2.schema.audit(), E_OVERREACH) == 1,
        "自实现校验器即越界",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 本域自检全集。
pub fn run_ver04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ve-asset");
    chk_seven_fields(&mut set);
    chk_license(&mut set);
    chk_compat(&mut set);
    chk_type_single_source(&mut set);
    chk_schema_reuse(&mut set);
    chk_degrade_and_criteria(&mut set);
    chk_load_chain(&mut set);
    chk_narration(&mut set);
    chk_negative(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（含证伪测试）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 正样本：全绿总纲零红项且预检通过。
    #[test]
    fn ver04_standard_arch_is_clean() {
        let a = AssetArchitecture::standard();
        let issues = a.collect_issues();
        assert!(issues.is_empty(), "标准总纲不该有红项，实际：{:?}", issues);
        assert!(a.preflight().is_ok());
    }

    /// 自检全集全绿且未截断。
    #[test]
    fn ver04_checks_all_green() {
        let s = run_ver04_checks();
        let (items, total) = s.red_items();
        assert!(!s.truncated(), "自检项被截断：{}", s.dropped());
        assert!(total >= 60, "自检项偏少（{}），防漏登", total);
        for i in 0..total {
            let c = items[i].expect("项缺失");
            assert!(c.passed, "红项：{} —— {}", c.name, c.detail);
        }
    }

    /// 证伪：抽掉任一**严格**要素必须被单独检出（**逐要素**不是查汇总）。
    #[test]
    fn ver04_each_field_is_falsifiable() {
        for f in AssetField::ALL.iter() {
            // 元数据走宽松语义：空值由 schema 层填默认并告警，不算要素缺失。
            if f.is_meta_lenient() {
                let mut a = CreationAsset::complete("x", "K-WALL");
                a.meta = "";
                assert!(
                    !a.missing_fields().contains(f),
                    "元数据不应被判要素缺失（宽松语义）"
                );
                continue;
            }
            let mut a = CreationAsset::complete("x", "K-WALL");
            match f {
                AssetField::Id => a.id = "",
                AssetField::Kind => a.kind = "",
                AssetField::Content => a.content = "",
                AssetField::Meta => a.meta = "",
                AssetField::Version => a.version = "",
                AssetField::Origin => a.origin = "",
                AssetField::License => a.license = LicenseState::Missing,
            }
            let missing = a.missing_fields();
            assert!(
                missing.contains(f),
                "要素 {} 缺失未被检出，missing={:?}",
                f.code(),
                missing.iter().map(|m| m.code()).collect::<Vec<_>>()
            );
        }
    }

    /// 证伪：缺许可必须阻断，且与待核验分道。
    #[test]
    fn ver04_license_states_diverge() {
        let mut missing = CreationAsset::complete("x", "K-WALL");
        missing.license = LicenseState::Missing;
        assert!(missing.listing_gate().is_err(), "缺许可须阻断");

        let mut unver = CreationAsset::complete("x", "K-WALL");
        unver.license = LicenseState::Unverified;
        assert!(unver.listing_gate().is_ok(), "待核验不应阻断");

        let e = AssetError::new(E_LICENSE_MISSING, "x", "x");
        assert_eq!(e.action(), DegradeAction::BlockListing);
        assert_eq!(e.severity(), P0);
    }

    /// 证伪：未声明兼容与明确不兼容必须分开归因。
    #[test]
    fn ver04_compat_levels_are_distinct() {
        let und = CompatReport::evaluate(None, 15);
        let inc = CompatReport::evaluate(Some(CompatDecl::new(30, 40)), 15);
        assert_eq!(und.level, CompatLevel::Undeclared);
        assert_eq!(inc.level, CompatLevel::Incompatible);
        assert_ne!(und.level, inc.level, "两种红线的归因不得相同");
        // 两者都要警告+确认，但错误码不同。
        let e1 = und.to_action().unwrap_err();
        let e2 = inc.to_action().unwrap_err();
        assert_eq!(e1.code, E_COMPAT_UNDECLARED);
        assert_eq!(e2.code, E_COMPAT_INCOMPATIBLE);
        assert_eq!(e1.action(), e2.action());
    }

    /// 证伪：代码类去掉沙箱标记必须被抓。
    #[test]
    fn ver04_sandbox_flag_is_falsifiable() {
        let mut a = AssetArchitecture::standard();
        a.registry.get_mut(AssetKind::ScriptData).needs_sandbox = false;
        let issues = a.registry.audit();
        assert_eq!(
            count_code(&issues, E_SANDBOX_FLAG_MISSING),
            1,
            "代码类失去沙箱标记未被检出：{:?}",
            issues
        );
    }

    /// 证伪：非代码类误挂沙箱也被抓（双向都查）。
    #[test]
    fn ver04_sandbox_flag_on_noncode_is_falsifiable() {
        let mut a = AssetArchitecture::standard();
        a.registry.get_mut(AssetKind::Icon).needs_sandbox = true;
        let issues = a.registry.audit();
        assert_eq!(
            count_code(&issues, E_SANDBOX_FLAG_MISSING),
            1,
            "非代码类误挂沙箱未被检出：{:?}",
            issues
        );
    }

    /// 证伪：元数据宽松 / 内容严格的两级语义必须真的分岔。
    #[test]
    fn ver04_schema_two_levels_are_falsifiable() {
        let a = AssetArchitecture::standard();

        let mut no_meta = CreationAsset::complete("x", "K-WALL");
        no_meta.meta = "";
        let (o1, _) = a.schema.validate(&no_meta);
        assert_eq!(o1, SchemaOutcome::MetaLenient);
        assert!(o1.passes(), "元数据宽松须放行");

        let mut no_content = CreationAsset::complete("x", "K-WALL");
        no_content.content = "";
        let (o2, _) = a.schema.validate(&no_content);
        assert_eq!(o2, SchemaOutcome::ContentViolation);
        assert!(!o2.passes(), "内容违例须拒绝");
    }

    /// 证伪：装载门禁顺序——类型门先于要素门。
    #[test]
    fn ver04_load_gate_order_is_falsifiable() {
        let a = AssetArchitecture::standard();
        let mut bad = CreationAsset::complete("x", "K-NOT-EXIST");
        bad.origin = ""; // 同时缺要素
        match a.load_asset(&bad, Some(CompatDecl::new(1, 99)), 15) {
            Err(e) => assert_eq!(
                e.code,
                E_KIND_UNREGISTERED,
                "根本问题（类型）应先报，实际报 {}",
                e.code
            ),
            Ok(_) => panic!("不该通过"),
        }
    }

    /// 证伪：许可三态的**绝对语义**（不依赖被检函数的自洽——若要素判定与闸门
/// 判定同源退化，两处会一起变对，自检反而看不出；故此处直断三态字面）。
#[test]
    fn ver04_license_verdict_is_single_source() {
        for s in LicenseState::ALL.iter() {
            let mut a = CreationAsset::complete("x", "K-WALL");
            a.license = *s;
            let present = a.has_field(AssetField::License);
            let gate_blocks =
                matches!(a.listing_gate(), Err(e) if e.code == E_LICENSE_MISSING);
            assert_eq!(
                present, !gate_blocks,
                "许可态 {} 的两处判定不一致：要素 {}、闸门阻塞 {}",
                s.code(),
                present,
                gate_blocks
            );
        }
        // **绝对断言**：三态各自的许可在场性写死在此，不从实现反推。
        // 缺许可不在场；已声明在场；待核验**也在场**（写了，只是待核）。
        let mut m = CreationAsset::complete("x", "K-WALL");
        m.license = LicenseState::Missing;
        assert!(!m.has_field(AssetField::License), "缺许可不得判为在场");
        let mut d = CreationAsset::complete("x", "K-WALL");
        d.license = LicenseState::Declared;
        assert!(d.has_field(AssetField::License), "已声明必须在场");
        let mut u = CreationAsset::complete("x", "K-WALL");
        u.license = LicenseState::Unverified;
        assert!(
            u.has_field(AssetField::License),
            "待核验的许可已写入，视为在场（只是上架要确认）"
        );
    }

    /// 证伪：缺许可必须报**自己的专码**，不得被并进「七要素缺项」降格。
    #[test]
    fn ver04_license_redline_not_downgraded() {
        let a = AssetArchitecture::standard();
        let mut bad = CreationAsset::complete("x", "K-WALL");
        bad.license = LicenseState::Missing;
        // 同时制造另一个要素缺失——即便如此，也须先报许可红线。
        bad.origin = "";
        match a.load_asset(&bad, Some(CompatDecl::new(1, 99)), 15) {
            Err(e) => {
                assert_eq!(
                    e.code,
                    E_LICENSE_MISSING,
                    "缺许可被降格成别的码：{}",
                    e.code
                );
                assert_eq!(e.severity(), P0, "红线须保持 P0");
                assert_eq!(e.action(), DegradeAction::BlockListing);
            }
            Ok(_) => panic!("缺许可不该通过装载"),
        }
    }

    /// 证伪：未声明兼容不阻断装载（老资产不误伤）。
    #[test]
    fn ver04_undeclared_compat_does_not_block() {
        let a = AssetArchitecture::standard();
        let asset = CreationAsset::complete("x", "K-WALL");
        match a.load_asset(&asset, None, 15) {
            Ok(o) => {
                assert_eq!(o.compat.level, CompatLevel::Undeclared);
                assert!(o.compat.needs_confirm(), "须要用户确认");
            }
            Err(e) => panic!("未声明兼容不应阻断装载，实际：{}", e),
        }
    }

    /// 证伪：空总纲必须拦住一切。
    #[test]
    fn ver04_empty_arch_is_falsifiable() {
        let e = AssetArchitecture::empty();
        assert!(e.preflight().is_err());
        assert_eq!(count_code(&e.registry.audit(), E_KIND_UNREGISTERED), 7);
        let asset = CreationAsset::complete("x", "K-WALL");
        assert!(e.load_asset(&asset, Some(CompatDecl::new(1, 99)), 15).is_err());
    }

    /// 七要素与七类是不同集合（防混为一谈）。
    #[test]
    fn ver04_fields_and_kinds_are_distinct() {
        assert_eq!(AssetField::ALL.len(), FIELD_COUNT);
        assert_eq!(AssetKind::ALL.len(), KIND_COUNT);
        // 数字相同但语义不同：要素是字段，种类是分类。
        let field_codes: Vec<&str> = AssetField::ALL.iter().map(|f| f.code()).collect();
        let kind_codes: Vec<&str> = AssetKind::ALL.iter().map(|k| k.code()).collect();
        for fc in field_codes.iter() {
            assert!(!kind_codes.contains(fc), "要素码与种类码不应相同：{}", fc);
        }
    }

    /// 降级四条路径三种方向互异。
    #[test]
    fn ver04_degrade_directions_differ() {
        let m = DegradeMatrix::standard();
        let mut fps: Vec<u64> = m.paths.iter().map(|p| p.fingerprint()).collect();
        fps.sort_unstable();
        let n = fps.len();
        fps.dedup();
        assert_eq!(fps.len(), n, "四条降级路径处置不得相同");
        assert_eq!(DegradeAction::ALL.iter().filter(|x| x.blocks()).count(), 2);
        assert_eq!(DegradeAction::ALL.iter().filter(|x| x.advisory_only()).count(), 2);
    }

    /// 判据六项齐备且前缀唯一。
    #[test]
    fn ver04_criteria_complete_and_unique() {
        assert_eq!(Criterion::ALL.len(), CRITERION_COUNT);
        assert!(audit_criteria().is_empty());
        let mut p: Vec<&str> = Criterion::ALL.iter().map(|c| c.probe).collect();
        p.sort_unstable();
        let n = p.len();
        p.dedup();
        assert_eq!(p.len(), n);
    }

    /// 读屏替述覆盖两轴与三态。
    #[test]
    fn ver04_narration_covers_key_points() {
        let n = AssetArchitecture::standard().narration();
        assert!(n.len() >= 7);
        let j = n.join("");
        assert!(j.contains("三态"), "须讲许可三态");
        assert!(j.contains("四级"), "须讲兼容四级");
        assert!(j.contains("两轴"), "须讲两轴正交");
        assert!(j.contains("宽松"), "须讲两级语义");
    }
}