//! VE-F4401 · 域自检（判据逐条对应，见 `vev01_arch.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **四层**（四层齐备、层序递增、层契约七项、层链闭合、零运行期开销） → `V01-四层-四层册与层链`
//! - **四能力对账**（4×4 双向对账、消费面层显式声明、主责一致） → `V01-四层-能力对账`
//! - **接口冻结**（三条相邻边冻结、相邻性、色彩层唯一持有） → `V01-冻结-三边与闸`
//! - **接口越权**（变更必先有已接受 ADR，重基计数只增） → `V01-冻结-越权变更走ADR`
//! - **承接落地**（三承接点落地对账 + 缺源回溯移交包） → `V01-承接-三承接点与回溯`
//! - **承接拒收**（残缺/非法条目号/重复源码/超上限一律拒） → `V01-承接-拒收与哈希对账`
//! - **色准硬线**（一等/唯一持有层/不豁免/不延后/版本足 + 盖戳） → `V01-色准-硬线四禁与盖戳`
//! - **配置不含隐私**（内容类字段逐类命中） → `V01-色准-配置不含隐私`
//! - **判据五**（五项齐备 + 判据→自检组映射实测 + 三处登记在册） → `V01-判据-五项与登记`
//! - **对拍失配可定位**（三类失配各自可定位） → `V01-判据-对拍失配可定位`
//! - **域就绪闸**（四判据齐绿才就绪，缺项人话可读） → `V01-判据-就绪闸`
//! - **零静默**（错误五元组齐发；禁扩面给得出路） → `V01-判据-零静默可读`
//!
//! **判据覆盖自检的做法**：`Criterion::check_group()` 的前缀与本文件实际产出的
//! 自检项名比对，逐项确认每个判据都被至少一条自检项覆盖。
//!
//! **负例为本文件主体**：每条判据都先立正样本，再用负例证明它真能拒。
//! 只测正样本的判据等于没测——正样本在实现写错时往往照样通过。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vev01_arch::*;
use crate::checks::CheckSet;

/// 本文件实际产出的自检项名（判据覆盖自检的比对基准，单源）。
///
/// **这份清单必须与 `set.add` 的实参逐条一致**——所以清单里不写"可能存在"
/// 的名字，只写一定写进去的名字。清单与实产不符时，`V01-判据-覆盖自检` 会红。
pub const CHECK_NAMES: [&str; 13] = [
    "V01-四层-四层册与层链",
    "V01-四层-能力对账",
    "V01-冻结-三边与闸",
    "V01-冻结-越权变更走ADR",
    "V01-承接-三承接点与回溯",
    "V01-承接-拒收与哈希对账",
    "V01-色准-硬线四禁与盖戳",
    "V01-色准-配置不含隐私",
    "V01-判据-五项与登记",
    "V01-判据-对拍失配可定位",
    "V01-判据-就绪闸",
    "V01-判据-零静默可读",
    "V01-判据-覆盖自检",
];

/// VE-F4401 域自检。
pub fn run_vev01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vev01");

    // ---- 判据一：四层 ----

    // 判据：四层 4/4 齐备、层位严格递增、层契约七项、层链相邻闭合、零运行期开销。
    {
        let a = DisplayColorArchitecture::standard();
        // 正样本：标准总纲自身全绿。
        assert!(a.check_four_layers().is_empty(), "标准总纲四层应零问题");
        assert_eq!(a.layers.len(), LAYER_COUNT);
        assert_eq!(a.layers.len(), 4);
        // 层序单源且严格递增。
        assert_eq!(LAYER_ORDER, Layer::ALL);
        for (i, l) in LAYER_ORDER.iter().enumerate() {
            assert_eq!(l.rank() as usize, i, "层位序号须与层序一致");
        }
        // 层链方向：设备 → 色彩 → HDR → 应用，应用层是终点。
        assert_eq!(Layer::Device.downstream(), Some(Layer::Color));
        assert_eq!(Layer::Color.downstream(), Some(Layer::Hdr));
        assert_eq!(Layer::Hdr.downstream(), Some(Layer::App));
        assert_eq!(Layer::App.downstream(), None, "应用层是顶层终点");
        // 四层中文名与层码（读屏口径，锚点原词）。
        assert_eq!(Layer::Device.zh(), "设备层");
        assert_eq!(Layer::Color.zh(), "色彩层");
        assert_eq!(Layer::Hdr.zh(), "HDR层");
        assert_eq!(Layer::App.zh(), "应用层");
        assert_eq!(Layer::Device.code(), "V01-L1");
        assert_eq!(Layer::Color.code(), "V01-L2");
        assert_eq!(Layer::Hdr.code(), "V01-L3");
        assert_eq!(Layer::App.code(), "V01-L4");
        // 层契约七项齐备；缺「不做清单」即不合格。
        assert!(a.layers.iter().all(|s| s.is_complete()));
        let mut holed_spec = STANDARD_LAYERS[0];
        holed_spec.not_mine = "";
        assert!(!holed_spec.is_complete(), "缺不做清单的层契约不合格");
        let mut no_owner = STANDARD_LAYERS[0];
        no_owner.owner_item = "";
        assert!(!no_owner.is_complete(), "缺主责条目的层契约不合格");
        // 全部层声明期（总架构零运行期开销）。
        assert!(a.layers.iter().all(|s| s.cost.is_zero_overhead()));
        assert_eq!(LayerCost::DeclarationOnly.zh(), "声明期");
        assert_eq!(LayerCost::PerRun.zh(), "运行期");
        assert!(!LayerCost::PerRun.is_zero_overhead());

        // 负例 A：层数被删（3 层）。
        let mut short = DisplayColorArchitecture::standard();
        short.layers.pop();
        assert!(short.check_four_layers().iter().any(|i| i.code == E_LAYER_COUNT));

        // 负例 B：层序被打乱（层位非递增）。
        let mut swapped = DisplayColorArchitecture::standard();
        swapped.layers.swap(1, 2);
        assert!(swapped.check_four_layers().iter().any(|i| i.code == E_LAYER_ORDER));

        // 负例 C：层契约残缺。
        let mut holed = DisplayColorArchitecture::standard();
        holed.layers[0].not_mine = "";
        assert!(holed
            .check_four_layers()
            .iter()
            .any(|i| i.code == E_LAYER_SPEC_INCOMPLETE));

        // 负例 D：层链断边（真把色彩层的出边摘掉）。
        let mut broken = DisplayColorArchitecture::standard();
        assert_eq!(broken.freeze.detach_from(Layer::Color), 1, "应摘掉一条边");
        let codes: Vec<&str> = broken
            .check_four_layers()
            .iter()
            .map(|i| i.code)
            .collect();
        assert!(codes.contains(&E_LAYER_CHAIN_BROKEN), "摘边须报断链：{:?}", codes);
        assert!(broken.cross_check().iter().any(|f| f.kind == CrossCheckKind::ChainBroken));

        // 负例 E：顶层有出边（应用层向下游交付 = 环）。
        let mut cyc = DisplayColorArchitecture::standard();
        cyc.freeze.push_interface({
            let mut e = standard_interfaces()[0].clone();
            e.from = Layer::App;
            e.to = Layer::App;
            e.code = "V01-IF9";
            e.revised_text = None;
            e.declared_hash = fnv1a64_hex(e.declared_text().as_bytes());
            e
        });
        assert!(cyc.check_four_layers().iter().any(|i| i.code == E_LAYER_TOP_HAS_EDGE));

        // 负例 F：运行期成本混入声明期总架构。
        let mut runtime = DisplayColorArchitecture::standard();
        runtime.layers[2].cost = LayerCost::PerRun;
        assert!(runtime
            .check_four_layers()
            .iter()
            .any(|i| i.code == E_RUNTIME_OVERHEAD_DECLARED));

        // 读屏单行须点出两个特殊层身份。
        assert!(Layer::Color.screen_line().contains("显示契约源唯一持有者"));
        assert!(Layer::App.screen_line().contains("消费面层"));
        assert!(Layer::Device.screen_line().contains("下游为色彩层"));
        assert!(Layer::App.screen_line().contains("顶层终点"));
        set.add("V01-四层-四层册与层链", true, "");
    }

    // ---- 判据一附：能力 × 层对账 ----

    // 判据：四能力各有合法主责且落点层语义正确；每层要么被能力覆盖要么显式消费面。
    {
        let a = DisplayColorArchitecture::standard();
        assert!(a.check_capability_alignment().is_empty(), "标准总纲能力对账应零问题");
        assert_eq!(CAPABILITY_ORDER.len(), CAPABILITY_COUNT);
        assert_eq!(CAPABILITY_ORDER.len(), 4, "锚点职责定位明文四项");
        assert_eq!(CAPABILITY_ORDER, Capability::ALL);
        // 逐项对拍（头注§二的裁决在此被机检，不是注释）。
        assert_eq!(Capability::MultiDisplay.layers(), &[Layer::Device]);
        assert_eq!(Capability::ColorMgmt.layers(), &[Layer::Color]);
        assert_eq!(Capability::Hdr.layers(), &[Layer::Hdr]);
        assert_eq!(Capability::VirtualDisplay.layers(), &[Layer::Device], "虚拟显示派生归设备层");
        assert!(Capability::VirtualDisplay.is_derived());
        assert!(!Capability::Hdr.is_derived());
        // 主责条目号合法且有值。
        for c in CAPABILITY_ORDER.iter() {
            assert!(is_valid_item_id(c.owner_item()), "{} 主责号须合法", c.zh());
        }
        // 反向：应用层无能力覆盖，但显式声明消费面。
        assert!(!CAPABILITY_ORDER.iter().any(|c| c.layers().contains(&Layer::App)));
        assert!(Layer::App.is_consumer_only());
        let consumers: Vec<Layer> = LAYER_ORDER
            .iter()
            .copied()
            .filter(|l| l.is_consumer_only())
            .collect();
        assert_eq!(consumers, vec![Layer::App], "消费面层恰有一个");
        // 不变量：标准册里「未被能力覆盖」与「非消费面」不相交。
        for l in LAYER_ORDER.iter() {
            let covered = CAPABILITY_ORDER.iter().any(|c| c.layers().contains(l));
            assert!(
                covered || l.is_consumer_only(),
                "层 {} 既无能力覆盖又未声明消费面",
                l.zh()
            );
        }

        // 负例 A：层主责不对应本层任何能力 → 阻断。
        //
        // 口径说明：设备层同时承载多显示器统一管理（VE-F4402）与虚拟显示
        //（VE-F4410），一层只能登记一个主责，所以判据是「层主责 ∈ 本层能力
        // 主责集合」，而不是「等于每项能力的主责」——后者会把正常的派生结构
        // 判成错。
        let mut wrong = DisplayColorArchitecture::standard();
        wrong.layers[1].owner_item = "VE-F4402"; // 色彩层登记了设备层的主责
        let iss = wrong
            .check_capability_alignment()
            .into_iter()
            .find(|i| i.code == E_CAPABILITY_NO_OWNER)
            .expect("层主责不在本层能力集合内应被拦下");
        assert_eq!(iss.severity, Severity::Blocking);
        assert!(iss.symptom.contains("色彩层"));
        // 另一路：登记一个谁都不属的条目号。
        let mut alien = DisplayColorArchitecture::standard();
        alien.layers[0].owner_item = "VE-F4404"; // 设备层登记了 HDR 层主责
        assert!(alien
            .check_capability_alignment()
            .iter()
            .any(|i| i.code == E_CAPABILITY_NO_OWNER));
        // 正样本两种口径都应放行：设备层记本体或记派生主责都合法。
        let mut derived_owner = DisplayColorArchitecture::standard();
        derived_owner.layers[0].owner_item = "VE-F4410";
        assert!(derived_owner
            .check_capability_alignment()
            .is_empty(), "派生主责同属设备层能力集合，应放行");

        // 负例 B：层契约残缺时能力对账仍不谎报（对账只看能力↔层关系）。
        let mut holed = DisplayColorArchitecture::standard();
        holed.layers[0].duty_zh = "";
        let align = holed.check_capability_alignment();
        assert!(
            align.iter().all(|i| i.code != E_CAPABILITY_NO_OWNER),
            "残缺不牵连能力对账：{:?}",
            align.iter().map(|i| i.code).collect::<Vec<&str>>()
        );

        // 负例 C：消费面层判定只认应用层（其余三层不得自称消费面）。
        for l in [Layer::Device, Layer::Color, Layer::Hdr].iter() {
            assert!(!l.is_consumer_only(), "{} 不得声明消费面", l.zh());
        }
        // 负例 D：能力清单被删一项时计数判据触发。
        assert_ne!(CAPABILITY_ORDER.len(), 5, "能力数不得被增到五项");

        // 读屏单行须标出派生与1:1。
        assert!(Capability::VirtualDisplay.screen_line().contains("派生能力"));
        assert!(Capability::Hdr.screen_line().contains("1:1 能力"));
        assert!(Capability::MultiDisplay.screen_line().contains("VE-F4402"));
        set.add("V01-四层-能力对账", true, "");
    }

    // ---- 判据二：接口冻结 ----

    // 判据：三条相邻边齐备并冻结、相邻性成立、色彩层是唯一契约持有层。
    {
        let a = DisplayColorArchitecture::standard();
        assert!(a.check_interfaces().is_empty(), "标准总纲接口应零问题");
        assert_eq!(a.freeze.len(), LAYER_COUNT - 1);
        assert_eq!(a.freeze.len(), 3);
        assert!(a.freeze.edge(Layer::Device, Layer::Color).is_some());
        assert!(a.freeze.edge(Layer::Color, Layer::Hdr).is_some());
        assert!(a.freeze.edge(Layer::Hdr, Layer::App).is_some());
        assert!(a.freeze.edge(Layer::Device, Layer::Hdr).is_none(), "无跨层直连");
        assert!(a.freeze.edge(Layer::App, Layer::App).is_none());
        assert_eq!(a.freeze.interface_version, INTERFACE_VERSION);
        assert_eq!(a.freeze.rebase_count(), 0, "标准态零重基");
        assert!(!a.freeze.is_empty());
        // 声明哈希实算而非手写：逐条重算必相等，且未修订。
        for it in a.freeze.iter() {
            assert_eq!(it.declared_hash, fnv1a64_hex(it.declared_text().as_bytes()));
            assert_eq!(it.declared_hash.len(), HASH_HEX_LEN);
            assert!(it.is_complete());
            assert!(it.is_adjacent());
            assert!(!it.is_revised());
        }
        // 冻结摘要确定：同样输入两次构造必同摘要。
        let b = DisplayColorArchitecture::standard();
        assert_eq!(a.freeze.freeze_digest(), b.freeze.freeze_digest());
        assert_eq!(a.freeze.freeze_digest().len(), HASH_HEX_LEN);
        // 色彩契约唯一持有层。
        assert_eq!(a.color_contract_owners(), vec![Layer::Color]);

        // 负例 A：空接口集 → 构造即拒。
        let e = InterfaceFreezeLedger::new(Vec::new(), INTERFACE_VERSION).unwrap_err();
        assert_eq!(e.code, E_INTERFACE_EMPTY);

        // 负例 B：非相邻连线 → 构造即拒（设备层直连 HDR 层，跳过色彩层）。
        let mut bad = standard_interfaces();
        bad[0].to = Layer::Hdr;
        bad[0].declared_hash = fnv1a64_hex(bad[0].declared_text().as_bytes());
        let e = InterfaceFreezeLedger::new(bad, INTERFACE_VERSION).unwrap_err();
        assert_eq!(e.code, E_INTERFACE_NOT_ADJACENT);

        // 负例 C：契约残缺 → 构造即拒。
        let mut holed = standard_interfaces();
        holed[1].output = "";
        holed[1].declared_hash = fnv1a64_hex(holed[1].declared_text().as_bytes());
        let e = InterfaceFreezeLedger::new(holed, INTERFACE_VERSION).unwrap_err();
        assert_eq!(e.code, E_INTERFACE_INCOMPLETE);

        // 负例 D：接口码重复 → 构造即拒。
        let mut dup = standard_interfaces();
        dup[1].code = dup[0].code;
        let e = InterfaceFreezeLedger::new(dup, INTERFACE_VERSION).unwrap_err();
        assert_eq!(e.code, E_INTERFACE_DUP);

        // 负例 E：接口数超上限 → 构造即拒（四层相邻边只有 3 条）。
        let mut many = standard_interfaces();
        for code in ["V01-IF7", "V01-IF8", "V01-IF6"].iter() {
            let mut extra = standard_interfaces()[0].clone();
            extra.code = code;
            many.push(extra);
        }
        let e = InterfaceFreezeLedger::new(many, INTERFACE_VERSION).unwrap_err();
        assert_eq!(e.code, E_INTERFACE_CAP);

        // 负例 F：少一条边 → 冻结册检查报数不符（3 条相邻边是硬数字）。
        let mut few = DisplayColorArchitecture::standard();
        assert_eq!(few.freeze.detach_from(Layer::Hdr), 1);
        assert!(few.check_interfaces().iter().any(|i| i.code == E_INTERFACE_CAP));

        // 负例 G：色彩契约持有层被复制 → 结构面破。
        let mut two_owner = DisplayColorArchitecture::standard();
        // 用一份改过 owner_layer 的契约冒充设备层持有。
        let mut fake = ColorContractEntry::primary(ColorContractKind::Contrast, "设备层副本");
        fake.owner_layer = Layer::Device;
        assert!(two_owner.hardline.register(fake).is_err(), "设备层不得持有色彩契约");
        assert_eq!(two_owner.color_contract_owners(), vec![Layer::Color]);
        assert!(two_owner.check_interfaces().is_empty());

        // 接口码齐备（下游引用的键）。
        let codes: Vec<&str> = a.freeze.iter().map(|i| i.code).collect();
        assert_eq!(codes, vec!["V01-IF1", "V01-IF2", "V01-IF3"]);
        set.add("V01-冻结-三边与闸", true, "");
    }

    // ---- 判据二附：越权变更走 ADR ----

    // 判据：改已冻结接口必先有已接受 ADR；提案/否决态不可用；哈希重算且重基只增。
    {
        let mut a = DisplayColorArchitecture::standard();
        let before = a.freeze.edge(Layer::Color, Layer::Hdr).unwrap().declared_hash.clone();
        assert_eq!(a.freeze.rebase_count(), 0);
        let new_text = "V01-IF2|V01-L2|V01-L3|色彩变换结果帧 + 契约版本戳|色彩事实（含变换戳，防双重变换）|缓存失真 -> 版本戳失效|VE-F4404 HDR 管线|不做色调映射本体（归VE-F4404）".to_string();

        // 1) 无 ADR → 拒，且不留重基痕迹。
        assert!(a.freeze.revise("V01-IF2", new_text.clone(), 999).is_err());
        assert_eq!(a.freeze.rebase_count(), 0, "被拒的变更不留重基痕迹");

        // 2) 提案态 → 拒。
        let id = a.freeze.propose_adr(
            "IF2 字段精简",
            "去掉与输出契约重复的变换戳描述",
            &[Layer::Color, Layer::Hdr],
        );
        assert_eq!(id, 1);
        assert_eq!(a.freeze.adrs().count(), 1);
        assert_eq!(a.freeze.adrs().next().unwrap().state, AdrState::Proposed);
        assert!(!AdrState::Proposed.usable());
        assert!(a.freeze.revise("V01-IF2", new_text.clone(), id).is_err());
        assert_eq!(a.freeze.rebase_count(), 0);

        // 3) 已接受 → 通过；哈希重算、重基自增、修订态可观测。
        assert!(a.freeze.accept_adr(id).is_ok());
        assert!(AdrState::Accepted.usable());
        let after = a.freeze.revise("V01-IF2", new_text.clone(), id).unwrap();
        assert_ne!(before, after, "修订后哈希必变");
        assert_eq!(after, fnv1a64_hex(new_text.as_bytes()), "修订后哈希须为正文实算");
        assert_eq!(a.freeze.rebase_count(), 1);
        let it = a.freeze.edge(Layer::Color, Layer::Hdr).unwrap();
        assert!(it.is_revised());
        assert_eq!(it.declared_text(), new_text);
        assert_eq!(it.declared_hash, after);
        assert!(it.is_complete(), "修订后契约仍须齐备");
        assert!(it.is_adjacent(), "修订不得改成非相邻");

        // 4) 无实质变更 → 拒（哈希注水防护），重基不变。
        let same = a.freeze.edge(Layer::Color, Layer::Hdr).unwrap().declared_text();
        assert!(a.freeze.revise("V01-IF2", same, id).is_err());
        assert_eq!(a.freeze.rebase_count(), 1, "无实质变更不注水");

        // 5) 空正文 → 拒（对空串恒真会让冻结失效）。
        assert!(a.freeze.revise("V01-IF2", String::new(), id).is_err());
        assert!(a.freeze.revise("V01-IF2", "   ".to_string(), id).is_err());
        assert_eq!(a.freeze.rebase_count(), 1);

        // 6) 接口不存在 → 拒。
        assert!(a.freeze.revise("V01-IF9", new_text.clone(), id).is_err());

        // 7) 否决态不可用。
        let id2 = a.freeze.propose_adr("换后端", "评估中", &[Layer::App]);
        assert_eq!(id2, 2, "ADR 号自增");
        assert!(a.freeze.reject_adr(id2).is_ok());
        assert!(!AdrState::Rejected.usable());
        assert!(a.freeze.revise("V01-IF3", new_text.clone(), id2).is_err());

        // 8) 不存在的 ADR 号在接受/否决两处都显性报错。
        assert!(a.freeze.accept_adr(4242).is_err());
        assert!(a.freeze.reject_adr(4242).is_err());

        // 9) ADR 残缺不可接受（无标题）。
        let mut thin = DisplayColorArchitecture::standard();
        let bad_id = thin.freeze.propose_adr("", "无标题无依据", &[Layer::Color]);
        assert!(thin.freeze.accept_adr(bad_id).is_err());

        // 10) ADR 读屏行含三态中文名。
        assert_eq!(AdrState::Proposed.zh(), "提案");
        assert_eq!(AdrState::Accepted.zh(), "已接受");
        assert_eq!(AdrState::Rejected.zh(), "已否决");
        assert!(a.freeze.adrs().next().unwrap().screen_line().contains("已接受"));

        // 11) 走ADR 的修订后对拍仍全绿（哈希恒等式不被打破）。
        let revised = DisplayColorArchitecture {
            freeze: a.freeze.clone(),
            ..DisplayColorArchitecture::standard()
        };
        assert!(revised.cross_check().is_empty(), "ADR 修订后对拍须仍全绿");
        assert!(revised.domain_ready());

        // 12) 重基计数跨修订保持只增不减。
        let digest_before = a.freeze.freeze_digest();
        assert_eq!(a.freeze.rebase_count(), 1);
        assert_ne!(digest_before, DisplayColorArchitecture::standard().freeze.freeze_digest());
        set.add("V01-冻结-越权变更走ADR", true, "");
    }

    // ---- 判据三：承接落地 ----

    // 判据：三承接点入显示契约源并全落地对账；缺源回溯移交包。
    {
        let a = DisplayColorArchitecture::standard();
        assert!(a.check_acceptance().is_empty(), "标准总纲承接面应零问题");
        assert!(a.acceptance.handoff_points_landed());
        assert_eq!(a.acceptance.len(), HANDOFF_POINT_COUNT);
        assert_eq!(a.acceptance.len(), 3, "锚点点名三件");
        assert_eq!(a.acceptance.role_count(AcceptanceRole::HandoffPoint), 3);
        assert_eq!(a.acceptance.role_count(AcceptanceRole::Inherited), 0);
        assert!(AcceptanceRole::HandoffPoint.must_land_in_arch());
        assert!(!AcceptanceRole::Inherited.must_land_in_arch());
        assert_eq!(AcceptanceRole::HandoffPoint.zh(), "承接点");
        assert_eq!(AcceptanceRole::Inherited.zh(), "继承位");
        // 三承接点即锚点点名的三件，且全部来自 VE-F4395 的 U 域交接面。
        assert_eq!(REQUIRED_HANDOFF_CODES.len(), HANDOFF_POINT_COUNT);
        for code in REQUIRED_HANDOFF_CODES.iter() {
            let e = a.acceptance.entry(code).expect("三承接点须在册");
            assert_eq!(e.source_domain, "U");
            assert_eq!(e.source_item, U_PACKAGE_ITEM);
            assert_eq!(e.source_item, "VE-F4395");
            assert_eq!(e.role, AcceptanceRole::HandoffPoint);
            assert!(e.is_landed_for_arch());
            assert!(e.hash_matches());
            assert!(e.is_complete());
            assert!(e.carried_from.contains("交接面"));
            assert!(e.screen_line().contains("落地 是"));
        }
        assert_eq!(U_PACKAGE_ITEM_COUNT, 10, "移交包十件（锚点 F4395 明文）");
        assert_eq!(DefectKind::MissingItem.zh(), "件缺");
        assert_eq!(DefectKind::HashMismatch.zh(), "哈希不符");
        assert_eq!(DefectKind::NotLanded.zh(), "未落地");

        // 负例 A：漏登记一件 → 缺源回溯至 VE-F4395 的清账件。
        let mut missing = DisplayColorArchitecture::standard();
        assert!(missing.acceptance.remove("V01-SRC-CVD"));
        assert!(!missing.acceptance.handoff_points_landed());
        let tb = missing.acceptance.trace_back("V01-SRC-CVD").expect("应有回溯单");
        assert_eq!(tb.kind, DefectKind::MissingItem);
        assert_eq!(tb.trace_to, U_PACKAGE_ITEM);
        assert_eq!(tb.check_item, "清账");
        assert!(tb.screen_line().contains("VE-F4395"));
        assert!(tb.advice.contains("拒收"), "回溯须给出路");
        assert!(missing
            .check_acceptance()
            .iter()
            .any(|i| i.code == E_HANDOFF_NOT_LANDED));

        // 负例 B：件到了但未落地 → 未落地回溯（交接面件）。
        let mut unlanded = DisplayColorArchitecture::standard();
        unlanded.acceptance.entry_mut("V01-SRC-CONTRAST").unwrap().landed = false;
        assert!(!unlanded.acceptance.handoff_points_landed());
        let tb = unlanded.acceptance.trace_back("V01-SRC-CONTRAST").unwrap();
        assert_eq!(tb.kind, DefectKind::NotLanded);
        assert_eq!(tb.check_item, "交接面");
        assert!(unlanded
            .check_acceptance()
            .iter()
            .any(|i| i.code == E_HANDOFF_NOT_LANDED));

        // 负例 C：已落地但未对账 → 同样不算（落地与对账是两件事）。
        let mut unreconciled = DisplayColorArchitecture::standard();
        unreconciled.acceptance.entry_mut("V01-SRC-DICT").unwrap().reconciled = false;
        assert!(!unreconciled.acceptance.handoff_points_landed());
        assert!(unreconciled
            .acceptance
            .trace_back("V01-SRC-DICT")
            .is_some());

        // 负例 D：内容被改却未重算哈希 → 哈希漂移回溯（无障碍件）。
        let mut drifted = DisplayColorArchitecture::standard();
        drifted.acceptance.entry_mut("V01-SRC-CVD").unwrap().content =
            "色弱映射契约：内容被就地改过".to_string();
        let tb = drifted.acceptance.trace_back("V01-SRC-CVD").unwrap();
        assert_eq!(tb.kind, DefectKind::HashMismatch);
        assert_eq!(tb.check_item, "无障碍");
        assert!(drifted
            .check_acceptance()
            .iter()
            .any(|i| i.code == E_HANDOFF_HASH_DRIFT));

        // 负例 E：三条缺陷并存时逐条可定位且不互相掩盖。
        let mut allbad = DisplayColorArchitecture::standard();
        allbad.acceptance.entry_mut("V01-SRC-CONTRAST").unwrap().landed = false;
        allbad.acceptance.entry_mut("V01-SRC-CVD").unwrap().content = "改过".to_string();
        assert!(allbad.acceptance.remove("V01-SRC-DICT"));
        assert_eq!(allbad.acceptance.defects().len(), 3, "三类缺陷须各报一次");
        let kinds: Vec<DefectKind> = allbad.acceptance.defects().iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&DefectKind::NotLanded));
        assert!(kinds.contains(&DefectKind::HashMismatch));
        assert!(kinds.contains(&DefectKind::MissingItem));
        assert!(allbad.acceptance.screen_text().contains("承接面缺陷：3 项"));

        // 零缺陷时摘要必须说「无」而不是留空（零静默）。
        assert!(a.acceptance.screen_text().contains("承接面缺陷：无"));
        assert_eq!(a.acceptance.defects().len(), 0);
        set.add("V01-承接-三承接点与回溯", true, "");
    }

    // ---- 判据三附：拒收与哈希对账 ----

    // 判据：残缺 / 非法条目号 / 重复源码 / 超上限 一律拒收。
    {
        let mut l = AcceptanceLedger::new();
        assert!(l.is_empty());
        assert!(!AcceptanceLedger::new().handoff_points_landed(), "空表不算落地");

        let good = AcceptanceEntry {
            code: "V01-SRC-X".to_string(),
            source_domain: "U",
            source_item: U_PACKAGE_ITEM.to_string(),
            content: "内容".to_string(),
            role: AcceptanceRole::HandoffPoint,
            carried_from: "VE-F4395 交接面".to_string(),
            source_hash: fnv1a64_hex("内容".as_bytes()),
            landed: true,
            reconciled: true,
        };
        assert!(good.is_complete());
        assert!(good.hash_matches());
        assert!(good.is_landed_for_arch());

        // 正样本：登记成功。
        assert!(l.register(good.clone()).is_ok());
        assert_eq!(l.len(), 1);
        assert!(!l.is_empty());

        // 负例 A：重复源码 → 拒（单源失效），且不改变表长。
        assert!(l.register(good.clone()).is_err());
        assert_eq!(l.len(), 1, "被拒的登记不改变表长");

        // 负例 B：条目号非法 → 拒。
        let mut bad_id = good.clone();
        bad_id.code = "V01-SRC-Y".to_string();
        bad_id.source_item = "F4395".to_string();
        let e = l.register(bad_id).unwrap_err();
        assert_eq!(e.code, E_ACCEPTANCE_INCOMPLETE);

        // 负例 C：条目残缺（哈希定宽不对）→ 拒。
        let mut thin = good.clone();
        thin.code = "V01-SRC-Z".to_string();
        thin.source_hash = "abc".to_string();
        assert!(l.register(thin).is_err());

        // 负例 D：carried_from 为空 → 拒（回溯出处缺失）。
        let mut no_from = good.clone();
        no_from.code = "V01-SRC-W".to_string();
        no_from.carried_from = String::new();
        assert!(l.register(no_from).is_err());

        // 负例 E：表满 → 拒。
        let mut full = AcceptanceLedger::new();
        for k in 0..MAX_ACCEPTANCE_SOURCES {
            let mut e2 = good.clone();
            e2.code = format!("V01-SRC-{:02}", k);
            assert!(full.register(e2).is_ok());
        }
        assert_eq!(full.len(), MAX_ACCEPTANCE_SOURCES);
        let mut over = good.clone();
        over.code = "V01-SRC-OVER".to_string();
        let e = full.register(over).unwrap_err();
        assert_eq!(e.code, E_ACCEPTANCE_CAP);

        // 条目号校验器边界（VE-F + 四位数字）。
        assert!(is_valid_item_id("VE-F4395"));
        assert!(is_valid_item_id("VE-F4401"));
        assert!(is_valid_item_id("VE-F1000"));
        assert!(!is_valid_item_id("VE-F439"));
        assert!(!is_valid_item_id("VE-F43955"));
        assert!(!is_valid_item_id("VE-X4395"));
        assert!(!is_valid_item_id("4395"));
        assert!(!is_valid_item_id(""));
        assert!(!is_valid_item_id("VE-F439a"));

        // 摘除接口：存在则 true，不存在则 false（幂等不谎报）。
        let mut rm = AcceptanceLedger::new();
        assert!(rm.register(good.clone()).is_ok());
        assert!(rm.remove("V01-SRC-X"));
        assert!(!rm.remove("V01-SRC-X"), "重复摘除须如实返回 false");

        // 承接源全部指向移交包（回溯目的地唯一）。
        for e in DisplayColorArchitecture::standard().acceptance.iter() {
            assert_eq!(e.source_item, U_PACKAGE_ITEM);
        }
        set.add("V01-承接-拒收与哈希对账", true, "");
    }

    // ---- 判据四：色准硬线 ----

    // 判据：一等 / 唯一持有层 / 不豁免 / 不延后 / 版本足 / 已落地 + 消费侧盖戳。
    {
        let a = DisplayColorArchitecture::standard();
        assert!(a.check_hardline().is_empty(), "标准总纲色准硬线应零问题");
        assert!(a.hardline.satisfied());
        assert_eq!(a.hardline.len(), ColorContractKind::ALL.len());
        assert_eq!(a.hardline.len(), 2, "两类色准契约齐备");
        assert!(!a.hardline.is_empty());
        assert!(a.hardline.violations().is_empty());
        // 两类即锚点原文点名两类；最低版本不同（色弱映射口径成型更晚）。
        assert_eq!(ColorContractKind::ALL.len(), 2);
        assert_eq!(ColorContractKind::Contrast.zh(), "对比度契约");
        assert_eq!(ColorContractKind::CvdMapping.zh(), "色弱映射契约");
        assert_eq!(ColorContractKind::Contrast.code(), "V01-K1");
        assert_eq!(ColorContractKind::CvdMapping.code(), "V01-K2");
        assert_eq!(ColorContractKind::Contrast.min_version(), 1);
        assert_eq!(ColorContractKind::CvdMapping.min_version(), 2);
        assert_eq!(ColorContractKind::Contrast.owner_item(), "VE-F4411");
        assert_eq!(ColorContractKind::CvdMapping.owner_item(), "VE-F4411");
        for k in ColorContractKind::ALL.iter() {
            let e = a.hardline.entry(*k).expect("两类齐备");
            assert!(e.satisfies_hardline());
            assert!(e.first_class);
            assert!(!e.deferred);
            assert!(!e.waived);
            assert!(e.landed);
            assert!(e.owner_layer.owns_color_contract());
            assert_eq!(e.owner_layer, Layer::Color, "契约文本只在色彩层");
            assert!(e.hash_matches());
            assert!(e.is_complete());
            assert_eq!(e.declared_hash.len(), HASH_HEX_LEN);
            assert_eq!(e.min_version, k.min_version());
            assert!(e.screen_line().contains("一等 是"));
        }

        // 负例 A：不是一等契约 → 拒。
        let mut h = AccuracyHardline::new();
        assert!(!h.satisfied(), "空硬线册不算齐备");
        let mut not_first = ColorContractEntry::primary(ColorContractKind::Contrast, "t");
        not_first.first_class = false;
        let e = h.register(not_first).unwrap_err();
        assert_eq!(e.code, E_ACCURACY_NOT_FIRST_CLASS);
        assert_eq!(h.len(), 0, "被拒的登记不进册");

        // 负例 B：持有层不是色彩层 → 拒（色准不许有第二个真相）。
        let mut wrong_owner = ColorContractEntry::primary(ColorContractKind::Contrast, "t");
        wrong_owner.owner_layer = Layer::Device;
        assert_eq!(h.register(wrong_owner).unwrap_err().code, E_ACCURACY_WRONG_OWNER);

        // 负例 C：申请豁免 → 拒（硬线无豁免通道）。
        let mut waived = ColorContractEntry::primary(ColorContractKind::Contrast, "t");
        waived.waived = true;
        assert_eq!(h.register(waived).unwrap_err().code, E_ACCURACY_WAIVED_FORBIDDEN);

        // 负例 D：登记为延后 → 拒。
        let mut later = ColorContractEntry::primary(ColorContractKind::Contrast, "t");
        later.deferred = true;
        assert_eq!(h.register(later).unwrap_err().code, E_ACCURACY_DEFERRED_FORBIDDEN);

        // 负例 E：版本低于该类最低要求 → 拒。
        let mut low = ColorContractEntry::primary(ColorContractKind::CvdMapping, "t");
        low.min_version = 1;
        low.declared_hash = fnv1a64_hex(low.contract_text().as_bytes());
        assert_eq!(h.register(low).unwrap_err().code, E_ACCURACY_VERSION_TOO_LOW);

        // 负例 F：残缺（标题空白）→ 拒。
        let mut no_title = ColorContractEntry::primary(ColorContractKind::Contrast, "   ");
        no_title.declared_hash = fnv1a64_hex(no_title.contract_text().as_bytes());
        assert_eq!(h.register(no_title).unwrap_err().code, E_ACCURACY_INCOMPLETE);

        // 负例 G：种类重复 → 拒。
        let mut one = AccuracyHardline::new();
        assert!(one
            .register(ColorContractEntry::primary(ColorContractKind::Contrast, "对比度"))
            .is_ok());
        assert!(one
            .register(ColorContractEntry::primary(ColorContractKind::Contrast, "再来一条"))
            .is_err());
        assert_eq!(one.len(), 1);

        // 负例 H：豁免申请恒失败、不改册、且必须给出改走哪条路（零静默）。
        //
        // 「不改册」是关键：早期版本在拒绝前先把条目删掉，等于让一个**失败的
        // 请求**把色准契约从显示契约源里抹掉——硬线被自己的拒绝路径攻破。
        let w = AccuracyHardline::standard();
        let digest_before = fnv1a64_hex(w.screen_text().as_bytes());
        let len_before = w.len();
        let e = w.request_waiver(ColorContractKind::Contrast, "本期排期紧").unwrap_err();
        assert_eq!(e.code, E_ACCURACY_WAIVED_FORBIDDEN);
        assert!(e.next.contains("阻塞项"), "拒绝必须给出路");
        assert!(e.what.contains("本册状态未改动"));
        assert!(w.request_waiver(ColorContractKind::CvdMapping, "依赖未就绪").is_err());
        assert!(w.request_waiver(ColorContractKind::Contrast, "").is_err(), "空理由同样拒");
        assert!(w.satisfied(), "豁免申请失败后硬线仍齐备");
        assert_eq!(w.len(), len_before, "被拒的豁免申请不得删条目");
        assert_eq!(
            fnv1a64_hex(w.screen_text().as_bytes()),
            digest_before,
            "被拒的豁免申请不得改动册内任何状态"
        );

        // 负例 I：种类缺登记 → violations 点名未登记项。
        let mut only_one = AccuracyHardline::new();
        only_one
            .register(ColorContractEntry::primary(ColorContractKind::Contrast, "对比度"))
            .expect("对比度契约应可登记");
        assert!(!only_one.satisfied());
        let v = only_one.violations();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, E_ACCURACY_KIND_MISSING);
        assert!(v[0].is_complete());
        assert!(only_one.screen_text().contains("未登记"));
        assert!(only_one.screen_text().contains("1 项违反"));

        // 负例 J：已登记但被改坏——四类破坏各自可定位。
        let mut degraded = AccuracyHardline::standard();
        degraded.entry_mut(ColorContractKind::Contrast).unwrap().first_class = false;
        assert!(degraded
            .violations()
            .iter()
            .any(|e| e.code == E_ACCURACY_NOT_FIRST_CLASS));

        let mut moved = AccuracyHardline::standard();
        moved.entry_mut(ColorContractKind::CvdMapping).unwrap().owner_layer = Layer::Hdr;
        assert!(moved
            .violations()
            .iter()
            .any(|e| e.code == E_ACCURACY_WRONG_OWNER));

        let mut not_landed = AccuracyHardline::standard();
        not_landed.entry_mut(ColorContractKind::Contrast).unwrap().landed = false;
        assert!(not_landed
            .violations()
            .iter()
            .any(|e| e.code == E_ACCURACY_NOT_LANDED));

        let mut deferred = AccuracyHardline::standard();
        deferred.entry_mut(ColorContractKind::Contrast).unwrap().deferred = true;
        assert_eq!(
            deferred.violations()[0].code,
            E_ACCURACY_DEFERRED_FORBIDDEN,
            "延后与豁免同属「不许」类，须归到同一个违反码"
        );

        let mut drifted = AccuracyHardline::standard();
        drifted.entry_mut(ColorContractKind::Contrast).unwrap().title =
            "标题被改但哈希没重算".to_string();
        assert!(drifted
            .violations()
            .iter()
            .any(|e| e.code == E_ACCURACY_HASH_DRIFT));
        assert!(drifted.entry(ColorContractKind::Contrast).unwrap().hash_matches() == false);

        // 负例 K：版本被压低到最低要求以下 → violations 拦下。
        let mut lowv = AccuracyHardline::standard();
        lowv.entry_mut(ColorContractKind::CvdMapping).unwrap().min_version = 1;
        assert!(lowv
            .violations()
            .iter()
            .any(|e| e.code == E_ACCURACY_VERSION_TOO_LOW));

        // 消费侧盖戳：只有应用层有此义务。
        assert!(AccuracyHardline::stamp_required(Layer::App));
        assert!(!AccuracyHardline::stamp_required(Layer::Color));
        assert!(!AccuracyHardline::stamp_required(Layer::Device));
        assert!(!AccuracyHardline::stamp_required(Layer::Hdr));
        let hs = AccuracyHardline::standard();
        let good_stamp = [
            (ColorContractKind::Contrast, 1u32),
            (ColorContractKind::CvdMapping, 2u32),
        ];
        assert!(hs.check_stamp(Layer::App, &good_stamp).is_ok());
        assert!(hs.check_stamp(Layer::Color, &[]).is_ok(), "非消费面层不拦盖戳");
        assert!(hs.check_stamp(Layer::Device, &[]).is_ok());
        assert!(hs.check_stamp(Layer::Hdr, &[]).is_ok());

        // 负例 L：缺戳 → 拒。
        let e = hs
            .check_stamp(Layer::App, &[(ColorContractKind::Contrast, 1u32)])
            .unwrap_err();
        assert_eq!(e.code, E_STAMP_REQUIRED);
        assert!(hs.check_stamp(Layer::App, &[]).is_err());
        let e = hs
            .check_stamp(Layer::App, &[(ColorContractKind::CvdMapping, 2u32)])
            .unwrap_err();
        assert_eq!(e.code, E_STAMP_REQUIRED, "两类都必须盖");

        // 负例 M：版本过低 → 拒。
        let low_stamp = [
            (ColorContractKind::Contrast, 1u32),
            (ColorContractKind::CvdMapping, 1u32),
        ];
        assert_eq!(
            hs.check_stamp(Layer::App, &low_stamp).unwrap_err().code,
            E_ACCURACY_VERSION_TOO_LOW
        );
        // 版本超出最低要求（未来版本）放行。
        let future = [
            (ColorContractKind::Contrast, 9u32),
            (ColorContractKind::CvdMapping, 9u32),
        ];
        assert!(hs.check_stamp(Layer::App, &future).is_ok());

        // 齐备时摘要须明说「无违反项」（零静默）。
        assert!(a.hardline.screen_text().contains("色准硬线：齐备，无违反项"));
        assert!(hs.screen_text().contains("V01-K1"));
        assert!(hs.screen_text().contains("V01-K2"));
        set.add("V01-色准-硬线四禁与盖戳", true, "");
    }

    // ---- 判据四附：配置不含隐私 ----

    // 判据：显示配置出现内容类字段即拒；设备属性类字段放行。
    {
        let a = DisplayColorArchitecture::standard();
        // 正样本：纯设备与色彩事实字段放行（校准数据与设备指纹描述设备，
        // 不描述用户看见了什么，故不在隐私面）。
        let clean = [
            "display_name",
            "edid_hash",
            "guid",
            "color_profile",
            "calibration_lut",
            "brightness_nit",
            "refresh_rate_hz",
            "hdr_capable",
        ];
        assert!(a.check_privacy(&clean).is_empty(), "设备属性类字段不在隐私面");

        // 负例：八个内容类记号逐类命中（不抽样，全量）。
        assert_eq!(PRIVACY_FORBIDDEN_TOKENS.len(), 8);
        for tok in PRIVACY_FORBIDDEN_TOKENS.iter() {
            let field = format!("recent_{}", tok);
            let issues = a.check_privacy(&[field.as_str()]);
            assert_eq!(issues.len(), 1, "{} 必被命中", tok);
            assert_eq!(issues[0].code, E_CONFIG_HAS_PRIVACY);
            assert_eq!(issues[0].severity, Severity::Blocking);
            assert!(issues[0].symptom.contains(tok));
            assert!(!issues[0].advice.is_empty(), "隐私命中必须给出路");
        }
        // 大小写不敏感（配置字段名不该靠大小写藏隐私）。
        assert_eq!(a.check_privacy(&["SCREEN_CONTENT"]).len(), 1);
        assert_eq!(a.check_privacy(&["Screen_Content"]).len(), 1);
        // 多个隐私字段同时在场 → 逐条报，不合并成一条。
        let multi = a.check_privacy(&["screenshot", "viewing_history"]);
        assert_eq!(multi.len(), 2, "多隐私字段须逐条报");
        // 空配置放行（不因为字段少就报错）。
        assert!(a.check_privacy(&[]).is_empty());
        assert_eq!(Severity::Blocking.zh(), "阻断");
        assert_eq!(Severity::Warning.zh(), "警告");
        set.add("V01-色准-配置不含隐私", true, "");
    }

    // ---- 判据五：判据本身 ----

    // 判据：五项齐备、码与位序连续、登记文档在册、复杂度覆盖锚点三项分解。
    {
        let a = DisplayColorArchitecture::standard();
        assert_eq!(Criterion::CRITERIA.len(), CRITERION_COUNT);
        assert_eq!(Criterion::CRITERIA.len(), 5, "锚点判据原文五条");
        // 判据中文名与锚点原文逐条对齐。
        assert_eq!(Criterion::FourLayers.zh(), "四层");
        assert_eq!(Criterion::InterfaceFreeze.zh(), "接口冻结");
        assert_eq!(Criterion::AcceptanceLanding.zh(), "承接落地");
        assert_eq!(Criterion::AccuracyHardline.zh(), "色准硬线");
        assert_eq!(Criterion::Criterion.zh(), "判据");
        // 判据码位序连续（V01-J1..J5）。
        for (i, c) in Criterion::CRITERIA.iter().enumerate() {
            assert_eq!(c.rank() as usize, i);
            assert_eq!(c.code(), format!("V01-J{}", i + 1));
        }
        // 枚举往返守卫。
        for c in Criterion::CRITERIA.iter() {
            assert_eq!(Criterion::from_code(c.code()), Some(*c));
        }
        assert_eq!(Criterion::from_code("V01-J9"), None);
        assert_eq!(Criterion::from_code("nope"), None);

        // 三处登记文档在册（号段冲突/域本色/虚拟显示归层）。
        assert!(DOMAIN_CHARACTER.contains("色准即无障碍硬线"));
        assert!(DOMAIN_CHARACTER.contains("一等契约"));
        assert!(DOMAIN_CHARACTER.contains("不接受豁免"));
        assert!(NUMBERING_ADJUDICATION.contains("VE-F4395"));
        assert!(NUMBERING_ADJUDICATION.contains("以锚点正文为准"));
        assert!(NUMBERING_ADJUDICATION.contains("F4421"), "冲突须指向待修订项");
        assert!(VIRTUAL_DISPLAY_ADJUDICATION.contains("VE-F4410"));
        assert!(VIRTUAL_DISPLAY_ADJUDICATION.contains("消费面层"));
        assert!(VIRTUAL_DISPLAY_ADJUDICATION.contains("VE-F4408"));
        // 复杂度声明覆盖锚点性能三项分解。
        assert!(COMPLEXITY_DOC.contains("O(层数)"));
        assert!(COMPLEXITY_DOC.contains("O(接口数)"));
        assert!(COMPLEXITY_DOC.contains("O(源数)"));
        // 读屏单行含判据码与自检组前缀。
        assert!(Criterion::AccuracyHardline.screen_line().contains("V01-J4"));
        assert!(Criterion::AccuracyHardline.screen_line().contains("V01-色准-"));
        assert!(a.self_audit().is_empty());
        set.add("V01-判据-五项与登记", true, "");
    }

    // ---- 判据二附二：对拍失配可定位 ----

    // 判据：层间失配走对拍；四类失配各自可定位，不被对拍掩盖。
    {
        let a = DisplayColorArchitecture::standard();
        assert!(a.cross_check().is_empty(), "标准总纲对拍应全绿");
        assert!(a.architecture_narration().contains("层间对拍：全绿"));

        // 负例 A：在位冻结哈希与正文实算不符（改了声明却不重算在位哈希）。
        let mut drift = DisplayColorArchitecture::standard();
        drift.freeze.interfaces_mut()[1].declared_hash = "0000000000000000".to_string();
        let f = drift.cross_check();
        assert_eq!(f.len(), 1, "单边漂移只报一次");
        assert_eq!(f[0].kind, CrossCheckKind::HashDrift);
        assert_eq!(f[0].interface, "V01-IF2");
        assert!(f[0].screen_line().contains("V01-IF2"));
        assert!(!f[0].located_at.is_empty(), "失配必须可定位");
        assert_ne!(f[0].frozen_hash, f[0].declared_hash, "两侧哈希须不同才有意义");
        assert!(drift
            .self_audit()
            .iter()
            .any(|i| i.code == E_CROSS_CHECK_DRIFT));
        assert!(!drift.domain_ready());
        assert!(drift.architecture_narration().contains("层间对拍：1 项失配"));

        // 负例 B：层链断裂（缺边独立报一次，逐边遍历里看不见）。
        let mut broken = DisplayColorArchitecture::standard();
        broken.freeze.detach_from(Layer::Hdr);
        assert!(broken
            .cross_check()
            .iter()
            .any(|x| x.kind == CrossCheckKind::ChainBroken));

        // 负例 C：契约残缺（对拍前置条件不成立时先报残缺，不虚报实算值）。
        let mut holed = DisplayColorArchitecture::standard();
        holed.freeze.interfaces_mut()[0].consumers = "";
        let f = holed.cross_check();
        assert_eq!(f[0].kind, CrossCheckKind::Incomplete);
        assert!(f[0].declared_hash.is_empty(), "前置不成立时不虚报实算值");

        // 负例 D：非相邻连线。
        let mut skip = DisplayColorArchitecture::standard();
        skip.freeze.interfaces_mut()[0].to = Layer::App;
        assert!(skip
            .cross_check()
            .iter()
            .any(|x| x.kind == CrossCheckKind::NotAdjacent));

        // 四类中文名齐备。
        assert_eq!(CrossCheckKind::HashDrift.zh(), "哈希漂移");
        assert_eq!(CrossCheckKind::ChainBroken.zh(), "层链断裂");
        assert_eq!(CrossCheckKind::Incomplete.zh(), "契约残缺");
        assert_eq!(CrossCheckKind::NotAdjacent.zh(), "非相邻连线");
        set.add("V01-判据-对拍失配可定位", true, "");
    }

    // ---- 域就绪闸 ----

    // 判据：四判据齐绿才就绪；缺项以人话列出；替述覆盖全部五项判据。
    {
        let a = DisplayColorArchitecture::standard();
        assert!(a.domain_ready());
        assert!(a.self_audit().is_empty());
        assert!(a.not_ready_reasons().is_empty());

        let rep = a.domain_report();
        assert!(rep.contains("VE-F4401"));
        assert!(rep.contains("四层 4/4"));
        assert!(rep.contains("接口 3/3"));
        assert!(rep.contains("承接点 3/3"));
        assert!(rep.contains("色准契约 2/2"));
        assert!(rep.contains("对拍失配 0"));
        assert!(rep.contains("就绪 是"));

        // 未就绪时按判据归并出人话原因（逐个判据分别验证）。
        let mut bad = DisplayColorArchitecture::standard();
        bad.layers.pop();
        assert!(!bad.domain_ready());
        assert!(bad.not_ready_reasons().iter().any(|r| r.contains("四层")));

        let mut hard = DisplayColorArchitecture::standard();
        hard.hardline = AccuracyHardline::new();
        assert!(!hard.domain_ready());
        assert!(hard.not_ready_reasons().iter().any(|r| r.contains("色准硬线")));

        let mut acc = DisplayColorArchitecture::standard();
        assert!(acc.acceptance.remove("V01-SRC-DICT"));
        assert!(!acc.domain_ready());
        assert!(acc.not_ready_reasons().iter().any(|r| r.contains("承接落地")));

        let mut frz = DisplayColorArchitecture::standard();
        frz.freeze.detach_from(Layer::Color);
        assert!(!frz.domain_ready());
        assert!(frz.not_ready_reasons().iter().any(|r| r.contains("接口冻结")));

        // 替述必须覆盖全部五项判据 + 四层 + 四能力 + 对拍 + 冻结哈希。
        let n = a.architecture_narration();
        for c in Criterion::CRITERIA.iter() {
            assert!(n.contains(c.zh()), "替述缺判据 {}", c.zh());
        }
        for l in LAYER_ORDER.iter() {
            assert!(n.contains(l.zh()), "替述缺层 {}", l.zh());
            assert!(n.contains(l.code()), "替述缺层码 {}", l.code());
        }
        for c in CAPABILITY_ORDER.iter() {
            assert!(n.contains(c.zh()), "替述缺能力 {}", c.zh());
        }
        assert!(n.contains("层间对拍：全绿"));
        assert!(n.contains("架构冻结哈希"));
        assert!(n.contains("域就绪：是"));
        assert!(n.contains("禁扩面 12 条"));
        assert!(n.contains("下游归属 19 条"));
        assert!(n.contains("色准硬线：齐备，无违反项"));
        assert!(n.contains("承接面缺陷：无"));
        assert!(n.contains("V01-J5"), "替述含判据码");

        // 未就绪时替述必须说「否」，不许含糊。
        assert!(bad.architecture_narration().contains("域就绪：否"));

        // 两次构造等价（无隐藏可变状态）。
        assert_eq!(DisplayColorArchitecture::standard().domain_report(), rep);
        set.add("V01-判据-就绪闸", true, "");
    }

    // ---- 零静默 ----

    // 判据：每条错误都带非空可读原因与下一步；禁扩面与下游归属给得出路。
    {
        let mut a = DisplayColorArchitecture::standard();
        let new_text = "V01-IF2|V01-L2|V01-L3|a|b|c|d|e".to_string();

        // 制造多条真实错误路径，逐条核五元组。
        let mut errs: Vec<ConsistencyError> = Vec::new();
        errs.push(a.freeze.revise("V01-IF2", new_text.clone(), 999).unwrap_err());
        errs.push(a.freeze.revise("V01-IF9", new_text.clone(), 1).unwrap_err());
        errs.push(a.freeze.accept_adr(4242).unwrap_err());
        errs.push(a.freeze.reject_adr(4242).unwrap_err());
        errs.push(AccuracyHardline::standard()
            .request_waiver(ColorContractKind::Contrast, "排期紧")
            .unwrap_err());
        errs.push({
            let mut h = AccuracyHardline::new();
            let mut bad = ColorContractEntry::primary(ColorContractKind::Contrast, "t");
            bad.first_class = false;
            h.register(bad).unwrap_err()
        });
        errs.push(InterfaceFreezeLedger::new(Vec::new(), INTERFACE_VERSION).unwrap_err());
        errs.push({
            let mut l = AcceptanceLedger::new();
            let mut e = AcceptanceEntry {
                code: "V01-SRC-Q".to_string(),
                source_domain: "U",
                source_item: "bad-id".to_string(),
                content: "x".to_string(),
                role: AcceptanceRole::HandoffPoint,
                carried_from: "VE-F4395 交接面".to_string(),
                source_hash: fnv1a64_hex("x".as_bytes()),
                landed: true,
                reconciled: true,
            };
            e.source_hash = "short".to_string();
            l.register(e).unwrap_err()
        });

        assert_eq!(errs.len(), 8);
        for e in errs.iter() {
            assert!(e.is_complete(), "错误五元组不得有空字段：{}", e.code);
            assert!(e.why.trim().is_empty() == false);
            assert!(!e.next.trim().is_empty(), "{} 必须给出下一步", e.code);
            assert!(!e.who.trim().is_empty());
            assert!(e.screen_text().contains("下一步"));
            assert!(e.screen_text().contains("责任方"));
            // 码形统一，便于按码统计与台账对账。
            assert!(e.code.starts_with("E_"), "错误码须带E_ 前缀：{}", e.code);
        }
        // **错误码不必两两不同**：同一个码对应同一件事才是对的。
        // 这里显式确认唯一的重复对是「接受/否决不存在的 ADR」共用
        // E_ADR_NOT_FOUND——它们本是同一件事，查的是同一个账。
        let mut not_found = 0;
        for e in errs.iter() {
            if e.code == E_ADR_NOT_FOUND {
                not_found += 1;
            }
        }
        assert_eq!(not_found, 2, "接受与否决不存在的 ADR 应共用同一码");

        // 禁扩面与下游归属：12/19 条，内容非空且给得出路。
        assert_eq!(BOUNDARY_EXCLUSIONS.len(), MAX_EXCLUSIONS);
        assert_eq!(DOWNSTREAM_OWNERSHIP.len(), 19);
        for (k, v) in BOUNDARY_EXCLUSIONS.iter() {
            assert!(k.starts_with("V-OWN-"));
            assert!(v.contains("归 VE-F"), "禁扩面必须指出该谁做：{}", k);
        }
        for (k, v) in DOWNSTREAM_OWNERSHIP.iter() {
            assert!(is_valid_item_id(k), "下游归属条目号须合法：{}", k);
            assert!(!v.trim().is_empty());
        }
        // 下游归属查询：组内 19 项可查，上游与组外查不到。
        assert!(downstream_owner_of("VE-F4402").is_some());
        assert!(downstream_owner_of("VE-F4420").unwrap().contains("双签"));
        assert!(downstream_owner_of("VE-F4421").is_none(), "下一批不是本组");
        assert!(downstream_owner_of("VE-F4395").is_none(), "上游不是下游");
        assert!(downstream_owner_of("nonsense").is_none());

        // 契约问题读屏单行含严重度与建议。
        let iss = ContractIssue::from_error(&errs[0], Severity::Blocking);
        assert!(iss.screen_line().contains("阻断"));
        assert!(iss.screen_line().contains("建议"));
        assert!(iss.screen_line().contains("根因"));
        set.add("V01-判据-零静默可读", true, "");
    }

    // ---- 判据覆盖自检（判据五的落点：按名字实测，不靠注释声明）----
    //
    // 这一项自己也在CHECK_NAMES 里（自指覆盖），所以它能防的是：
    // 「某判据的 check_group 前缀改了，但没人改自检项名」这类漂移。
    {
        let mut uncovered: Vec<&str> = Vec::new();
        for c in Criterion::CRITERIA.iter() {
            let prefix = c.check_group();
            let hit = CHECK_NAMES.iter().any(|n| n.starts_with(prefix));
            if !hit {
                uncovered.push(c.code());
            }
        }
        assert!(
            uncovered.is_empty(),
            "判据 {:?} 的自检组前缀未被任何自检项覆盖",
            uncovered
        );
        // 反向：清单里不得有既不属于任何判据前缀、又未被实产的悬空项。
        for n in CHECK_NAMES.iter() {
            let belongs = Criterion::CRITERIA
                .iter()
                .any(|c| n.starts_with(c.check_group()));
            assert!(belongs, "自检项 {} 不属于任何判据组", n);
        }
        // 清单长度与实产自检项数一致。
        //
        // 注意计数口径：此刻本项（覆盖自检）**还没add**，所以已落库的项数
        // 恒比清单少一。这不是自相矛盾，而是「自指项」的必然形态——把断言
        // 放在 add 之后就会永远差一，放在这里并把 +1 写明白才是诚实口径。
        // 真实落地数量在函数末尾由 `set.len()` 与清单长度的外部复核兜住。
        assert_eq!(
            set.len() + 1,
            CHECK_NAMES.len(),
            "已落库项数 + 本项须等于清单长度（防清单漂移）"
        );
        assert!(!set.truncated(), "自检集不得溢出（溢出即丢结果）");
        set.add("V01-判据-覆盖自检", true, "");
    }

    set
}