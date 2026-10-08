//! VE-F3602 · 域自检（判据逐条对应，见 `ver03_arch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项前缀）：
//! - 三层五段→ `R01-三层五段-*`
//! - 开放格式 P0 → `R01-开放格式-*`
//! - 激励单源 → `R01-激励单源-*`
//! - 沙箱复述 → `R01-沙箱复述-*`
//! - 收敛复述 → `R01-收敛复述-*`
//! - 判据（自证可追溯）→ `R01-判据-*`
//! - 降级矩阵 → `R01-降级-*`
//! - 禁扩面 → `R01-边界-*`
//! - 无障碍替述 → `R01-替述-*`
//!
//! **可证伪纪律**：每组自检都配「注入缺陷 → 必红」的单元测试。报"计数为 0"
//! 不算守住，自检必须能被正当理由打红才算真守住。
//!
//! 零墙钟、零 IO，回归可复现。

use super::ver03_arch::*;
use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 全绿总纲（**唯一正样本构造处**——其余函数只做判定，不各自造"好看的数据"）。
fn ready_arch() -> EcosystemArchitecture {
    EcosystemArchitecture::standard()
}

/// 空总纲（反例用）。
fn empty_arch() -> EcosystemArchitecture {
    EcosystemArchitecture::empty()
}

/// 数指定错误码在问题表里出现的次数（真红项计数，非 CheckSet 的总数）。
fn count_code(issues: &[EcoIssue], code: &str) -> usize {
    issues.iter().filter(|i| i.code == code).count()
}

// ---------------------------------------------------------------------------
// 判据一：三层五段
// ---------------------------------------------------------------------------

fn chk_layers_stages(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 恰三层且三层齐备。
    set.add(
        "R01-三层五段-恰三层",
        EcoLayer::ALL.len() == LAYER_COUNT && a.layers.layers.len() == LAYER_COUNT,
        "锚点原文：创作生态三层（工具层/资产层/分发层）",
    );

    // 2. 层码往返无损（改层码必须同步改 from_code）。
    set.add(
        "R01-三层五段-层码往返",
        a.layers.code_roundtrip(),
        "层码→反查层无损，防改名只改一半",
    );

    // 3. 层级序工具(0)→资产(1)→分发(2)。
    set.add(
        "R01-三层五段-层级序",
        a.layers.order_ok(),
        "锚点原文顺序：工具层/资产层/分发层",
    );

    // 4. 每层有册内真实对端条目（反空壳层）。
    let no_peer = count_code(&a.layers.audit(), E_LAYER_NO_PEER);
    set.add(
        "R01-三层五段-层有对端",
        no_peer == 0,
        if no_peer == 0 { "工具层→R02/R03、资产层→F3603、分发层→F3616 均已挂对端" } else { "存在空壳层" },
    );

    // 5. 工具层对端确为 R02/R03 两批次段（册内 R02=F3621起、R03=F3641 起）。
    let tool = a.layers.get(EcoLayer::Tool);
    set.add(
        "R01-三层五段-工具层对端R02R03",
        tool.has_peer(3621) && tool.has_peer(3641),
        "锚点原文：工具层（编辑器/工坊——R02/R03）",
    );

    // 6. 资产层/分发层对端为册内 F3603/F3616。
    set.add(
        "R01-三层五段-资产分发对端",
        a.layers.get(EcoLayer::Asset).has_peer(3603)
            && a.layers.get(EcoLayer::Distribution).has_peer(3616),
        "锚点原文：资产层（创作资产模型——F3603）/分发层（市场分发——F3616）",
    );

    // 7. 对端条目全在 R 域号段内（承 F3601 跳段裁决）。
    let oob = count_code(&a.layers.audit(), E_PEER_OUT_OF_BAND);
    set.add(
        "R01-三层五段-对端在R域段内",
        oob == 0,
        if oob == 0 {
            "对端条目全落在 F3601-F3800（承VE-F3601 跳段裁决）"
        } else {
            "存在越段对端"
        },
    );

    // 8. 恰五段且段码往返无损。
    set.add(
        "R01-三层五段-恰五段",
        EcoStage::ALL.len() == STAGE_COUNT && a.stages.stages.len() == STAGE_COUNT,
        "锚点原文：架构五段接口（创建→编辑→验证→打包→分发）",
    );
    set.add(
        "R01-三层五段-段码往返",
        a.stages.code_roundtrip(),
        "段码→反查段无损",
    );

    // 9. 段序严格递增（创建→编辑→验证→打包→分发）。
    set.add(
        "R01-三层五段-段序严格",
        a.stages.audit().iter().filter(|i| i.code == E_STAGE_ORDER).count() == 0,
        "锚点原文五段顺序",
    );

    // 10. 段码字面正确（不是随便五个码）。
    let codes_ok = EcoStage::ALL
        .iter()
        .enumerate()
        .all(|(i, s)| s.code() == ["S1-CREATE", "S2-EDIT", "S3-VERIFY", "S4-PACKAGE", "S5-DIST"][i]);
    set.add(
        "R01-三层五段-段码字面",
        codes_ok,
        "五段码必须与锚点名物对应，改名即失配",
    );
}

// ---------------------------------------------------------------------------
// 判据二：签名冻结 v1（锚点：签名冻结 v1）
// ---------------------------------------------------------------------------

fn chk_signature_frozen(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 五段签名全等于冻结值。
    let drift = count_code(&a.stages.check_frozen(), E_STAGE_SIG_DRIFT);
    set.add(
        "R01-签名冻结-五段无漂移",
        drift == 0,
        if drift == 0 { STAGE_SIG_VERSION } else { "签名漂移须走 ADR" },
    );

    // 2. 五段签名两两不同（否则两段同签名=接口实为一段）。
    let mut sigs: Vec<u32> = a.stages.stages.iter().map(|s| s.sig).collect();
    sigs.sort_unstable();
    let before = sigs.len();
    sigs.dedup();
    set.add(
        "R01-签名冻结-签名互异",
        sigs.len() == before,
        "五段签名互不相同",
    );

    // 3. 每段参产齐备。
    let incomplete = count_code(&a.stages.audit(), E_STAGE_NO_PARAM)
        + count_code(&a.stages.audit(), E_STAGE_NO_RESULT);
    set.add(
        "R01-签名冻结-参产齐备",
        incomplete == 0,
        "每段须有入参与产出字面",
    );

    // 4. 每段有取项依据（推导也须写明，不许冒充锚点原句）。
    set.add(
        "R01-签名冻结-段有依据",
        count_code(&a.stages.audit(), E_STAGE_NO_BASIS) == 0,
        "五段各写明取项依据",
    );

    // 5. 产出衔接：上一段产出=下一段入参首项。
    set.add(
        "R01-签名冻结-产出衔接",
        a.stages.chain_continuous(),
        "asset_draft→edit→verified→pkg→receipt 首尾相接",
    );

    // 6. 段指纹互异（防两段内容雷同）。
    let mut fps: Vec<u64> = a.stages.stages.iter().map(|s| s.fingerprint()).collect();
    fps.sort_unstable();
    let fb = fps.len();
    fps.dedup();
    set.add("R01-签名冻结-段指纹互异", fps.len() == fb, "五段契约指纹互不相同");
}

// ---------------------------------------------------------------------------
// 判据三：开放格式 P0
// ---------------------------------------------------------------------------

fn chk_open_format(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 恰十类（F3204 十类）。
    set.add(
        "R01-开放格式-恰十类",
        ResourceClass::ALL.len() == RESOURCE_CLASS_COUNT,
        "锚点 F3204：纹理/网格/材质/音频/字体/动画/样式表/场景图/预制体/脚本数据",
    );

    // 2. 类码往返无损。
    set.add("R01-开放格式-类码往返", a.formats.code_roundtrip(), "类码→反查类无损");

    // 3. 无类锁定。
    let locked = a.formats.locked_count();
    set.add(
        "R01-开放格式-无锁定",
        locked == 0,
        if locked == 0 {
            "十类全部可导出可迁入有开放规范"
        } else {
            "存在锁定类（生态死刑）"
        },
    );

    // 4. 十类逐个有开放规范版本（非空）。
    let no_spec = a.formats.items.iter().filter(|f| f.open_spec.trim().is_empty()).count();
    set.add(
        "R01-开放格式-规范版本齐备",
        no_spec == 0,
        if no_spec == 0 { "十类各有开放规范版本" } else { "存在无规范类" },
    );

    // 5. 锁定检出必须报P0 性质（红线）。
    set.add(
        "R01-开放格式-锁定判红线",
        EcoIssue::new(E_FORMAT_LOCKED, "T", "x").is_redline(),
        "锚点：格式锁定检出→P0（红线实测——生态执法）",
    );

    // 6. 锁定错误码映射到阻断动作 + P0 级。
    let e = EcoError::new(E_FORMAT_LOCKED, "T-TEX", "锁定");
    set.add(
        "R01-开放格式-锁定阻断P0",
        e.action() == DegradeAction::BlockRelease && e.severity() == P0,
        "锁定必须阻断分发且级为 P0",
    );

    // 7. 四种锁法各自都能检出（不可导出 / 不可迁入 / 无规范 / **专有名**）。
    //    专有格式最阴险：名字非空、能通过「非空即开放」的弱检，但照样锁死用户。
    let no_export = OpenFormat {
        exportable: false,
        migratable_in: true,
        open_spec: "s",
        class: ResourceClass::Texture,
    };
    let no_import = OpenFormat {
        exportable: true,
        migratable_in: false,
        open_spec: "s",
        class: ResourceClass::Texture,
    };
    let no_spec = OpenFormat {
        exportable: true,
        migratable_in: true,
        open_spec: "",
        class: ResourceClass::Texture,
    };
    let vendor_spec = OpenFormat {
        exportable: true,
        migratable_in: true,
        open_spec: "vendor-lock/prefab-v9",
        class: ResourceClass::Prefab,
    };
    let lock_kinds = [no_export.locked(), no_import.locked(), no_spec.locked(), vendor_spec.locked()]
        .iter()
        .filter(|x| **x)
        .count();
    set.add(
        "R01-开放格式-四锁法可检出",
        lock_kinds == 4,
        "不可导出/不可迁入/无规范/专有名——任一即锁",
    );

    // 7b. 专有格式必须单独点名（弱检「非空即开放」会放过它）。
    set.add(
        "R01-开放格式-专有名判锁",
        !vendor_spec.spec_is_open() && vendor_spec.lock_reason().contains("专有"),
        "非空的专有名同样是生态死刑——不得只判空",
    );

    // 7c. 开放规范前缀是单一事实源（标准十类全带此前缀）。
    set.add(
        "R01-开放格式-规范前缀统一",
        ready_arch().formats.items.iter().all(|f| f.open_spec.starts_with(OPEN_SPEC_PREFIX)),
        "十类规范名统一以 open-spec/ 开头",
    );

    // 8. 全合规时不误报（反恒真：正样本须零红项）。
    set.add(
        "R01-开放格式-正样本零红",
        a.formats.audit().is_empty(),
        "全合规总表不应产生任何红项",
    );
}

// ---------------------------------------------------------------------------
// 判据四：激励单源（歧义二的落点）
// ---------------------------------------------------------------------------

fn chk_incentive(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 恰三项（署名/分成/评级）。
    set.add(
        "R01-激励单源-恰三项",
        IncentiveKind::ALL.len() == INCENTIVE_COUNT,
        "锚点原文：创作者署名/分成/评级",
    );

    // 2. 三项字面正确且顺序正确。
    let names_ok = IncentiveKind::ALL
        .iter()
        .enumerate()
        .all(|(i, k)| k.code() == ["INC-ATTR", "INC-SHARE", "INC-RATE"][i]);
    set.add("R01-激励单源-三项字面", names_ok, "署名/分成/评级，顺序照锚点");

    // 3. 每项恰一数据对端，且各有册内出处。
    set.add(
        "R01-激励单源-三项有主",
        a.incentives.audit().is_empty(),
        "署名→F3114、评级→F3109、分成→F3306，各有册内出处",
    );

    // 4. 逐项核对数据对端（不只看汇总）。
    set.add(
        "R01-激励单源-署名归F3114",
        a.incentives.get(IncentiveKind::Attribution).owner_code == 3114,
        "锚点直陈：复用 F3114 模式",
    );
    set.add(
        "R01-激励单源-分成归F3306",
        a.incentives.get(IncentiveKind::RevenueShare).owner_code == 3306,
        "转引 F3616：F3306 权限（结算对端）",
    );
    set.add(
        "R01-激励单源-评级归F3109",
        a.incentives.get(IncentiveKind::Rating).owner_code == 3109,
        "转引 F3114：评分进 F3109 评级",
    );

    // 5. 每项有取项依据。
    set.add(
        "R01-激励单源-项有依据",
        count_code(&a.incentives.audit(), E_INCENTIVE_NO_BASIS) == 0,
        "转引也须写明册内出处",
    );

    // 6. 署名不可剥（盗包红线）。
    set.add(
        "R01-激励单源-署名不可剥",
        !IncentiveKind::Attribution.strippable(),
        "锚点 F3114：署名不可剥=盗包红线",
    );

    // 7. 双单源是**两个侧面**且条目不同（歧义二的核心断言）。
    set.add(
        "R01-激励单源-双单源两侧",
        a.incentives.facets_distinct()
            && a.incentives.eco_source == Some(3114)
            && a.incentives.sdk_source == Some(3347),
        "F3114=生态语义侧、F3347=SDK 接入侧，两者条目不同——不是同一能力的两个来源",
    );

    // 8. 侧面名与锚点对得上。
    set.add(
        "R01-激励单源-侧面锚点",
        SourceFacet::Ecosystem.anchor().contains("F3114")
            && SourceFacet::Sdk.anchor().contains("F3347"),
        "两侧各自锚定册内条目",
    );

    // 9. 激励分歧走对拍不阻断（处置方向与 P0 类相反，不共用）。
    let dup = EcoError::new(E_INCENTIVE_DUP, "INC-SHARE", "分歧");
    set.add(
        "R01-激励单源-分歧走对拍",
        dup.action() == DegradeAction::Reconcile && dup.severity() == P1,
        "锚点：激励分歧→对拍（复述）——不阻断，否则误伤正常结算",
    );
}

// ---------------------------------------------------------------------------
// 判据五：沙箱复述
// ---------------------------------------------------------------------------

fn chk_sandbox(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 代码类识别正确（当前仅脚本数据）。
    let code_classes: Vec<ResourceClass> =
        ResourceClass::ALL.iter().copied().filter(|c| c.is_code()).collect();
    set.add(
        "R01-沙箱复述-代码类识别",
        code_classes == [ResourceClass::ScriptData],
        "锚点原文：创作代码类资产（脚本/逻辑）",
    );

    // 2. 代码类已挂沙箱引用（且有出处）。
    set.add(
        "R01-沙箱复述-代码类有沙箱",
        count_code(&a.sandbox.audit(), E_SANDBOX_MISSING) == 0,
        "脚本数据已引用沙箱",
    );

    // 3. 非代码类不强加沙箱。
    set.add(
        "R01-沙箱复述-非代码不强加",
        count_code(&a.sandbox.audit(), E_SANDBOX_UNNEEDED) == 0,
        "纹理/网格等九类不挂沙箱",
    );

    // 4. 只许复述，不许重实现。
    set.add(
        "R01-沙箱复述-只许复述",
        a.sandbox.reference_only && count_code(&a.sandbox.audit(), E_SANDBOX_REIMPLEMENTED) == 0,
        "锚点原文：隔离复述——本项不实现沙箱本体",
    );

    // 5. 绕过检出判P0 + 阻断。
    let bypass = EcoError::new(E_SANDBOX_BYPASS, "T-SCRIPT", "绕过");
    set.add(
        "R01-沙箱复述-绕过判P0",
        bypass.action() == DegradeAction::BlockRelease && bypass.severity() == P0,
        "锚点：沙箱绕过→P0（复述）",
    );

    // 6. 绕过项在问题表里以红线性质出现。
    set.add(
        "R01-沙箱复述-绕过判红线",
        EcoIssue::new(E_SANDBOX_BYPASS, "T-SCRIPT", "绕过").is_redline(),
        "绕过属红线实测类",
    );

    // 7. 正样本沙箱表零红。
    set.add("R01-沙箱复述-正样本零红", a.sandbox.audit().is_empty(), "标准沙箱表应零红项");
}

// ---------------------------------------------------------------------------
// 判据六：收敛复述（只有两段走管线）
// ---------------------------------------------------------------------------

fn chk_convergence(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 收敛点恰两个（打包/分发）。
    set.add(
        "R01-收敛复述-恰两收敛点",
        {
            let via = a.convergence.points.iter().filter(|p| p.via_pipeline).count();
            // 走管线的段数恰等于「对外交付段数」——不靠硬编码 2，靠推导对齐。
            let delivers = EcoStage::ALL.iter().filter(|s| s.delivers_externally()).count();
            via == delivers
                && a.convergence.via_pipeline(EcoStage::Package) == Some(true)
                && a.convergence.via_pipeline(EcoStage::Distribute) == Some(true)
        },
        "走管线的恰为打包与分发两段（容量留一槽余量，多登不等于多走管线）",
    );

    // 2. 打包/分发段判定为对外交付。
    set.add(
        "R01-收敛复述-交付段判定",
        EcoStage::Package.delivers_externally() && EcoStage::Distribute.delivers_externally()
            && !EcoStage::Create.delivers_externally()
            && !EcoStage::Edit.delivers_externally()
            && !EcoStage::Verify.delivers_externally(),
        "创建/编辑/验证为本地态",
    );

    // 3. 本地段误走管线即红（防「五段全走」这种偷懒做法）。
    set.add(
        "R01-收敛复述-本地段不入管线",
        count_code(&a.convergence.audit(), E_PIPELINE_LOCAL_LEAK) == 0,
        "本地段硬塞正式管线会污染正式缓存（与 F3605 预览隔离冲突）",
    );

    // 4. 交付段绕管线即红（判红线性质）。
    set.add(
        "R01-收敛复述-交付段必走管线",
        count_code(&a.convergence.audit(), E_PIPELINE_BYPASS) == 0,
        "转引 F3616：绕管线分发→立案（收敛复述）",
    );

    // 5. 收敛点各有依据。
    set.add(
        "R01-收敛复述-点有依据",
        count_code(&a.convergence.audit(), E_CONVERGENCE_NO_BASIS) == 0,
        "判据「收敛复述」正文无此四字，须写明册内出处",
    );

    // 6. 绕管线判P0 + 阻断。
    let byp = EcoError::new(E_PIPELINE_BYPASS, "S5-DIST", "绕管线");
    set.add(
        "R01-收敛复述-绕管线P0",
        byp.action() == DegradeAction::BlockRelease && byp.severity() == P0,
        "绕管线对外交付=P0",
    );

    // 7. 正样本收敛表零红。
    set.add("R01-收敛复述-正样本零红", a.convergence.audit().is_empty(), "标准收敛表应零红项");
}

// ---------------------------------------------------------------------------
// 降级矩阵与判据自证
// ---------------------------------------------------------------------------

fn chk_degrade_and_criteria(set: &mut CheckSet) {
    let a = ready_arch();

    // 1. 三条锚点错误路径齐备。
    set.add(
        "R01-降级-三条路径齐备",
        count_code(&a.degrade.audit(), E_DEGRADE_MISSING) == 0,
        "格式锁定→P0 / 沙箱绕过→P0 / 激励分歧→对拍",
    );

    // 2. 级与动作匹配锚点。
    set.add(
        "R01-降级-级动匹配",
        count_code(&a.degrade.audit(), E_DEGRADE_MISMATCH) == 0,
        "两条 P0 阻断、一条 P1 对拍",
    );

    // 3. 三条路径指纹互异（防三条同款）。
    let mut fps: Vec<u64> = a.degrade.paths.iter().map(|p| p.fingerprint()).collect();
    fps.sort_unstable();
    let fb = fps.len();
    fps.dedup();
    set.add("R01-降级-路径指纹互异", fps.len() == fb, "三条降级路径处置各不相同");

    // 4. 锚点引文非空（照录原文，不许空写）。
    let quoted = a.degrade.paths.iter().all(|p| p.quote.trim().is_empty() == false);
    set.add("R01-降级-引文照录", quoted, "三条路径各带锚点原文引文");

    // 5. 判据恰六项且逐条有依据。
    set.add(
        "R01-判据-六项齐备",
        Criterion::ALL.len() == CRITERION_COUNT && audit_criteria().is_empty(),
        "锚点判据：三层五段/开放格式P0/激励单源/沙箱复述/收敛复述/判据",
    );

    // 6. 判据标题逐条照录锚点用词。
    let titles_ok = Criterion::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.title == ["三层五段", "开放格式 P0", "激励单源", "沙箱复述", "收敛复述", "判据"][i]);
    set.add("R01-判据-标题照录", titles_ok, "判据标题不得改写");

    // 7. 六项判据各有唯一机检前缀（自证可追溯）。
    let mut probes: Vec<&str> = Criterion::ALL.iter().map(|c| c.probe).collect();
    probes.sort_unstable();
    let pb = probes.len();
    probes.dedup();
    set.add("R01-判据-机检前缀唯一", probes.len() == pb, "每判据一个自检前缀");

    // 8. 判据台账指纹稳定（改判据即变）。
    set.add(
        "R01-判据-台账指纹非零",
        Criterion::ledger_fingerprint() != 0,
        "六项判据拼接指纹",
    );

    // 9. 禁扩面条数与对端登记齐备。
    set.add(
        "R01-边界-禁扩面条数",
        ECO_EXCLUSIONS.len() == EXCLUSION_COUNT,
        "七条：本项只声明架构，不替 F3603/F3616/F3607/F3204/F3114/F3306/Q 管线实现",
    );

    // 10. 越界检出可判。
    let hit = check_no_overreach("要实现 F3204:registry 的类型注册表");
    set.add(
        "R01-边界-越界可检出",
        hit.is_err() && eco_scope_advice("普通架构声明").is_none(),
        "命中禁扩面条目即 E_OVERREACH；正常声明不误报",
    );
}

// ---------------------------------------------------------------------------
// 无障碍替述与总纲自证
// ---------------------------------------------------------------------------

fn chk_narration(set: &mut CheckSet) {
    let a = ready_arch();
    let n = a.narration();

    // 1. 替述逐条产出且不少于六条（三层/五段/开放格式/沙箱/激励/收敛）。
    set.add(
        "R01-替述-条数齐备",
        n.len() >= 6,
        "六项要点逐条口述，不依赖图形",
    );

    // 2. 替述非空且每条有实质长度（防空话）。
    let all_substantive = n.iter().all(|s| s.trim().len() >= 12);
    set.add("R01-替述-非空话", all_substantive, "每条替述须有实质内容");

    // 3. 替述覆盖三层名称。
    let joined = n.join("");
    set.add(
        "R01-替述-覆盖三层",
        joined.contains("工具层") && joined.contains("资产层") && joined.contains("分发层"),
        "三层须在替述中逐一点名",
    );

    // 4. 替述覆盖五段。
    set.add(
        "R01-替述-覆盖五段",
        ["创建", "编辑", "验证", "打包", "分发"]
            .iter()
            .all(|w| joined.contains(w)),
        "五段须在替述中逐一点名",
    );

    // 5. 替述讲清开放格式为何是死刑（不只说要做开放格式）。
    set.add(
        "R01-替述-开放格式讲因果",
        joined.contains("带不走"),
        "替述须讲清「锁定格式=生态死刑」的理由",
    );

    // 6. 替述讲清双单源是两个侧面（歧义二的替代表述）。
    set.add(
        "R01-替述-双单源讲两侧",
        joined.contains("SDK 接入侧") && joined.contains("生态语义侧"),
        "替述须区分 F3114 与 F3347 是两个侧面",
    );

    // 7. 替述讲清只有两段走管线。
    set.add(
        "R01-替述-收敛讲两段",
        joined.contains("只有打包与分发两段"),
        "替述须点明本地段不入正式管线",
    );

    // 8. 总纲指纹非零且随改动变化。
    set.add("R01-替述-总纲指纹", a.fingerprint() != 0, "八张表串接指纹");

    // 9. 段→层映射无空壳（每层至少承载一段）。
    set.add(
        "R01-替述-映射双向无空",
        EcoLayer::ALL.iter().all(|l| a.map.load_of(*l) > 0),
        "三层各至少承载一段，五段各恰一属主",
    );

    // 10. 段→层映射与推导一致。
    set.add(
        "R01-替述-映射合推导",
        a.map.audit().is_empty(),
        "五段属主层与头注 §1.3 推导表一致",
    );

    // 11. 工具层承载三段、资产层一段、分发层一段（载荷分布显性）。
    set.add(
        "R01-替述-载荷分布",
        a.map.load_of(EcoLayer::Tool) == 3
            && a.map.load_of(EcoLayer::Asset) == 1
            && a.map.load_of(EcoLayer::Distribution) == 1,
        "工具3/资产1/分发1——推导的自然结果",
    );

    // 12. 开前置：全绿总纲 preflight 通过。
    set.add(
        "R01-替述-preflight通过",
        ready_arch().preflight().is_ok(),
        "全绿总纲应通过开工前置校验",
    );
}

// ---------------------------------------------------------------------------
// 反例总纲（空总纲必须被拦住——反恒真）
// ---------------------------------------------------------------------------

fn chk_negative(set: &mut CheckSet) {
    let e = empty_arch();
    let issues = e.collect_issues();

    // 1. 空总纲问题数不为零（自检不是恒真）。
    set.add(
        "R01-反例-空总纲被拦",
        !issues.is_empty() && e.preflight().is_err(),
        "空总纲必须 preflight 失败",
    );

    // 2. 空格式表产生锁定红项。
    set.add(
        "R01-反例-空格式全锁",
        e.formats.locked_count() == RESOURCE_CLASS_COUNT,
        "空格式表十类全锁",
    );

    // 3. 空层栈三处空壳检出。
    set.add(
        "R01-反例-空层栈三空壳",
        count_code(&e.layers.audit(), E_LAYER_NO_PEER) == LAYER_COUNT,
        "三层全无对端",
    );

    // 4. 空签名链五段无签名。
    set.add(
        "R01-反例-空签名五段缺",
        count_code(&e.stages.check_frozen(), E_STAGE_NO_SIG) == STAGE_COUNT,
        "五段签名全空",
    );

    // 5. 空激励表两侧单源均缺。
    set.add(
        "R01-反例-空激励缺双源",
        count_code(&e.incentives.audit(), E_SDK_SOURCE_SPLIT)
            + count_code(&e.incentives.audit(), E_INCENTIVE_NO_OWNER)
            >= 2,
        "SDK 侧与生态侧单源均未登记",
    );

    // 6. 空收敛表两交付段绕管线。
    set.add(
        "R01-反例-空收敛全绕",
        count_code(&e.convergence.audit(), E_PIPELINE_BYPASS) == CONVERGENCE_POINT_COUNT,
        "打包与分发均标不走管线",
    );

    // 7. 全堆工具层映射触发空壳层检出。
    set.add(
        "R01-反例-映射堆工具层",
        count_code(&e.map.audit(), E_LAYER_NO_STAGE) >= 1,
        "五段全归工具层→资产层与分发层成空壳",
    );

    // 8. preflight 返回的红线项确为 P0 类（阻断而非对拍）。
    let pf = e.preflight().err().map(|x| x.code);
    let redline_blocked = match pf {
        Some(c) => c == E_FORMAT_LOCKED || c == E_PIPELINE_BYPASS || c == E_LAYER_NO_PEER,
        None => false,
    };
    set.add(
        "R01-反例-preflight取红线",
        redline_blocked,
        "空总纲首个阻断项须来自红线类",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 本域自检全集（聚合入口，由 [`EcosystemArchitecture::run_checks`] 调用）。
pub fn run_ver03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ve-eco");
    chk_layers_stages(&mut set);
    chk_signature_frozen(&mut set);
    chk_open_format(&mut set);
    chk_incentive(&mut set);
    chk_sandbox(&mut set);
    chk_convergence(&mut set);
    chk_degrade_and_criteria(&mut set);
    chk_narration(&mut set);
    chk_negative(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（含证伪测试：注入缺陷后必须以正当理由红）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 正样本：全绿总纲零红项且预检通过。
    #[test]
    fn ver03_standard_arch_is_clean() {
        let a = EcosystemArchitecture::standard();
        let issues = a.collect_issues();
        assert!(issues.is_empty(), "标准总纲不该有红项，实际：{:?}", issues);
        assert!(a.preflight().is_ok());
    }

    /// 自检全集必须全绿（且未截断）。
    #[test]
    fn ver03_checks_all_green() {
        let s = run_ver03_checks();
        let (items, total) = s.red_items();
        assert!(!s.truncated(), "自检项被截断：{}", s.dropped());
        assert!(total >= 60, "自检项偏少（{}），防漏登", total);
        for i in 0..total {
            let c = items[i].expect("项缺失");
            assert!(c.passed, "红项：{} —— {}", c.name, c.detail);
        }
    }

    /// 证伪：**非空的专有名同样是生态死刑**（弱检「非空即开放」会放过它）。
    #[test]
    fn ver03_vendor_spec_is_still_locked() {
        let mut a = EcosystemArchitecture::standard();
        // 预制体换成厂商专有格式：名字非空，但用户带不走。
        a.formats.get_mut(ResourceClass::Prefab).open_spec = "vendor-lock/prefab-v9";
        assert!(
            !a.formats.get(ResourceClass::Prefab).spec_is_open(),
            "专有名不得被当成开放规范"
        );
        let issues = a.formats.audit();
        assert_eq!(
            count_code(&issues, E_FORMAT_LOCKED),
            1,
            "专有格式未被检出为锁定：{:?}",
            issues
        );
        assert!(
            issues[0].detail.contains("专有"),
            "理由须点明是专有格式：{}",
            issues[0].detail
        );
        assert!(a.preflight().is_err(), "专有格式资产不得通过开工预检");
    }

    /// 证伪：改冻结签名必须被抓（且须是正当理由 E_STAGE_SIG_DRIFT）。
    #[test]
    fn ver03_frozen_signature_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        a.stages.get_mut(EcoStage::Verify).sig = 0xDEAD_0001;
        let issues = a.stages.check_frozen();
        assert_eq!(count_code(&issues, E_STAGE_SIG_DRIFT), 1, "签名漂移未被检出");
        assert!(issues[0].detail.contains("冻结"), "理由须指向冻结：{}", issues[0].detail);
    }

    /// 证伪：把一个资源类锁死，必须报P0 红线且总纲预检失败。
    #[test]
    fn ver03_format_lock_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        a.formats.get_mut(ResourceClass::Texture).exportable = false;
        let issues = a.formats.audit();
        assert_eq!(count_code(&issues, E_FORMAT_LOCKED), 1);
        let e = EcoError::new(E_FORMAT_LOCKED, "T-TEX", "锁定");
        assert_eq!(e.severity(), P0);
        assert_eq!(e.action(), DegradeAction::BlockRelease);
        assert!(a.preflight().is_err(), "锁定资产不得通过开工预检");
    }

    /// 证伪：单个类被锁不能被九个合规类淹没（逐个核，不是只看汇总）。
    #[test]
    fn ver03_single_locked_class_is_not_masked() {
        let mut a = EcosystemArchitecture::standard();
        a.formats.get_mut(ResourceClass::Font).migratable_in = false;
        assert_eq!(a.formats.locked_count(), 1, "只有一个类被锁");
        let issues = a.formats.audit();
        assert!(
            issues.iter().any(|i| i.code == E_FORMAT_LOCKED && i.subject == "T-FONT"),
            "被锁的字体类须单独点名，实际：{:?}",
            issues
        );
    }

    /// 证伪：脚本数据沙箱引用被摘掉必须被抓。
    #[test]
    fn ver03_sandbox_missing_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        a.sandbox.get_mut(ResourceClass::ScriptData).cite = "";
        let issues = a.sandbox.audit();
        assert_eq!(count_code(&issues, E_SANDBOX_MISSING), 1, "代码类失去沙箱引用未被检出");
        assert!(a.preflight().is_err());
    }

    /// 证伪：把本项标成「自己实现了沙箱」必须红。
    #[test]
    fn ver03_sandbox_reimplementation_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        a.sandbox.reference_only = false;
        let issues = a.sandbox.audit();
        assert_eq!(
            count_code(&issues, E_SANDBOX_REIMPLEMENTED),
            1,
            "重实现沙箱未被检出"
        );
    }

    /// 证伪：交付段绕管线必须被抓（判红线性质）。
    #[test]
    fn ver03_pipeline_bypass_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        a.convergence.points[1].via_pipeline = false;
        let issues = a.convergence.audit();
        assert_eq!(count_code(&issues, E_PIPELINE_BYPASS), 1, "绕管线未被检出");
        assert!(issues[0].subject.contains("S5-DIST"));
    }

    /// 证伪：本地段误标走正式管线必须被抓（防「五段全走」）。
    #[test]
    fn ver03_local_stage_leak_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        // 手工构造：把「创建」也标成走管线。
        let mut pts = a.convergence.points;
        pts[0].stage = EcoStage::Create;
        pts[0].via_pipeline = true;
        a.convergence.points = [
            pts[0],
            ConvergencePoint {
                stage: EcoStage::Distribute,
                via_pipeline: true,
                basis: "转引：F3616 锚点错误路径写「绕管线分发→立案（收敛复述）」",
            },
        ];
        let issues = a.convergence.audit();
        assert!(
            count_code(&issues, E_PIPELINE_LOCAL_LEAK) >= 1,
            "本地段误走管线未被检出：{:?}",
            issues
        );
    }

    /// 证伪：同一激励能力指向两个对端必须报「单源纪律破」。
    #[test]
    fn ver03_incentive_split_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        // 构造第二处署名声明，指向不同对端。
        let mut clauses = a.incentives.clauses;
        clauses[1] = IncentiveClause {
            kind: IncentiveKind::Attribution,
            owner_code: 9999,
            owner_note: "第二处署名声明",
            basis: "反例构造",
        };
        a.incentives.clauses = clauses;
        let issues = a.incentives.audit();
        assert!(
            count_code(&issues, E_INCENTIVE_DUP) >= 1,
            "激励多主未被检出：{:?}",
            issues
        );
        let e = EcoError::new(E_INCENTIVE_DUP, "INC-ATTR", "多主");
        assert_eq!(e.action(), DegradeAction::Reconcile, "分歧应对拍不阻断");
    }

    /// 证伪：SDK 接入侧单源缺失必须被抓。
    #[test]
    fn ver03_sdk_facet_missing_is_falsifiable() {
        let mut a = EcosystemArchitecture::standard();
        a.incentives.sdk_source = None;
        let issues = a.incentives.audit();
        assert_eq!(count_code(&issues, E_SDK_SOURCE_SPLIT), 1, "SDK 单源缺失未被检出");
    }

    /// 证伪：五段全归工具层必须让资产层/分发层成空壳。
    #[test]
    fn ver03_all_tool_map_is_falsifiable() {
        let issues = StageLayerMap::all_tool().audit();
        assert!(
            count_code(&issues, E_LAYER_NO_STAGE) >= 2,
            "空壳层未被检出：{:?}",
            issues
        );
    }

    /// 证伪：越界声明必须被拦。
    #[test]
    fn ver03_overreach_is_falsifiable() {
        let r = check_no_overreach("顺手把 F3603:model 的七要素也写了");
        assert!(r.is_err(), "越界未被拦");
        match r {
            Err(e) => {
                assert_eq!(e.code, E_OVERREACH);
                assert!(e.detail.contains("只声明架构"));
            }
            Ok(_) => panic!("不该放行"),
        }
        assert!(check_no_overreach("登记三层与五段映射").is_ok());
    }

    /// 空总纲必须被拦住（反恒真：自检不是走过场）。
    #[test]
    fn ver03_empty_arch_is_falsifiable() {
        let e = EcosystemArchitecture::empty();
        assert!(e.preflight().is_err());
        assert!(count_code(&e.layers.audit(), E_LAYER_NO_PEER) == 3);
        assert_eq!(e.formats.locked_count(), 10);
        assert_eq!(count_code(&e.convergence.audit(), E_PIPELINE_BYPASS), 2);
    }

    /// 双单源必须是两个不同侧面（歧义二的固化）。
    #[test]
    fn ver03_dual_source_are_two_facets() {
        let a = EcosystemArchitecture::standard();
        assert!(a.incentives.facets_distinct(), "F3114 与 F3347 条目须不同");
        assert_eq!(a.incentives.eco_source, Some(3114));
        assert_eq!(a.incentives.sdk_source, Some(3347));
        assert_ne!(SourceFacet::Ecosystem.anchor(), SourceFacet::Sdk.anchor());
    }

    /// 判据六项齐备且各有唯一机检前缀。
    #[test]
    fn ver03_criteria_complete_and_unique() {
        assert_eq!(Criterion::ALL.len(), CRITERION_COUNT);
        assert!(audit_criteria().is_empty());
        let mut p: Vec<&str> = Criterion::ALL.iter().map(|c| c.probe).collect();
        p.sort_unstable();
        let n = p.len();
        p.dedup();
        assert_eq!(p.len(), n, "判据机检前缀重复");
    }

    /// 五段×三层映射与头注推导表逐条一致。
    #[test]
    fn ver03_stage_layer_map_matches_derivation() {
        let m = StageLayerMap::standard();
        assert_eq!(m.owner_of(EcoStage::Create), EcoLayer::Tool);
        assert_eq!(m.owner_of(EcoStage::Edit), EcoLayer::Tool);
        assert_eq!(m.owner_of(EcoStage::Verify), EcoLayer::Tool);
        assert_eq!(m.owner_of(EcoStage::Package), EcoLayer::Asset);
        assert_eq!(m.owner_of(EcoStage::Distribute), EcoLayer::Distribution);
        assert!(m.audit().is_empty());
    }

    /// 收敛线只有两段（本地三段不得走管线）。
    #[test]
    fn ver03_only_two_stages_use_pipeline() {
        let c = ConvergenceLedger::standard();
        assert_eq!(c.via_pipeline(EcoStage::Package), Some(true));
        assert_eq!(c.via_pipeline(EcoStage::Distribute), Some(true));
        assert_eq!(c.via_pipeline(EcoStage::Create), None);
        assert_eq!(c.via_pipeline(EcoStage::Edit), None);
        assert_eq!(c.via_pipeline(EcoStage::Verify), None);
        let delivers: Vec<EcoStage> = EcoStage::ALL
            .iter()
            .copied()
            .filter(|s| s.delivers_externally())
            .collect();
        assert_eq!(delivers, vec![EcoStage::Package, EcoStage::Distribute]);
    }

    /// 十类资源齐备且代码类唯一。
    #[test]
    fn ver03_ten_resource_classes() {
        assert_eq!(ResourceClass::ALL.len(), RESOURCE_CLASS_COUNT);
        let codes: Vec<&str> = ResourceClass::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(codes.len(), 10);
        let mut uniq = codes.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 10, "类码须互异");
        let code: Vec<ResourceClass> =
            ResourceClass::ALL.iter().copied().filter(|c| c.is_code()).collect();
        assert_eq!(code, vec![ResourceClass::ScriptData]);
    }

    /// 总纲指纹随实质改动而变（不是恒定值）。
    #[test]
    fn ver03_fingerprint_tracks_change() {
        let a = EcosystemArchitecture::standard();
        let base = a.fingerprint();
        let mut b = EcosystemArchitecture::standard();
        b.formats.get_mut(ResourceClass::Prefab).exportable = false;
        assert_ne!(base, b.fingerprint(), "锁定一个类后指纹必须变");
    }

    /// 读屏替述覆盖两层三层五段六要点。
    #[test]
    fn ver03_narration_is_complete() {
        let n = EcosystemArchitecture::standard().narration();
        assert!(n.len() >= 6);
        let j = n.join("");
        for kw in ["工具层", "资产层", "分发层", "创建", "编辑", "验证", "打包", "分发"] {
            assert!(j.contains(kw), "替述缺关键词：{}", kw);
        }
        assert!(j.contains("带不走"), "替述须讲开放格式的因果");
        assert!(j.contains("只有打包与分发两段"), "替述须讲收敛线");
    }
}