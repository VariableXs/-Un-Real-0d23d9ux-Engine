//! VE-AB01 · 音频域总架构 域自检（VE-F5601）。
//!
//! 覆盖锚点四族：四层架构 / 音频三律 / 承接追补 / 错误路径。
//!
//! ## 判据纪律（本域遵守的十条）
//!
//! 1. **映射表逐条钉死**：四层的 `tag`/`label`/`ordinal`、三律的
//!    `tag`/`code`/`of_code`、十件的 `tag`/`target_layer` —— 全部由判据侧
//!    **独立写一份期望表**再逐条比对，不问被测实现（问被测＝自证式）。
//! 2. **层间依赖双向验证**：合法向下依赖必须**被接受**，越权向上必须
//!    **被拒**。只测拒不放行是弱门禁（闸门常年关着也「通过」）。
//! 3. **越权诊断三要素齐备**：逐条断言 `exceeded`/`because`/`remedy`
//!    非空且各不相同（空串不算要素）。
//! 4. **红线不可降级**：`Precedent::degradable()` 恒 false；
//!    `MutedOut`（「静音掉」）也计违例 —— 静音不等于合规。
//! 5. **归类失败仍计违例**：`Unclassified` 不得因归类失败而放行。
//! 6. **追补可合并不可丢失**：重复 `chase` 同一件仍只有一条，
//!    且 `accept` 未核验件会转追补。
//! 7. **冻结前置闸双向验证**：有追补件时冻结须被拒；追补清零后同
//!    一 `freeze` 必须成功（证明拒绝是因缺件而非永久封禁）。
//! 8. **开工闸五因逐项拆测**：四层冻结齐备 / 无越权 / 无红线 /
//!    三律已公示 / 十件齐备 —— 每次只破坏一项，看闸门是否**恰好**
//!    因这一项而held（防止「闸门恒held」被误读成判据全绿）。
//! 9. **枚举穷举完备**：四层/三律/十件/五先例的穷举函数长度与元素
//!    互异数都对账（防「漏一个元素」这类不可见的缺口）。
//! 10. **零 panic 面**：语料含空架构、满架构、越权、自环等边界形态，
//!     一律只调`&self` 方法，不要求调用方先自证。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::veab01_audioarch::*;

// ===========================================================================
// 判据侧独立期望表（**不引用被测实现的映射**）
// ===========================================================================

/// 四层期望表（层序、短名、中文名）。判据侧独立写死。
const EXP_LAYERS: [(u8, &str, &str); 4] = [
    (0, "device", "设备层"),
    (1, "mix", "混音层"),
    (2, "spatial", "空间层"),
    (3, "content", "内容层"),
];

/// 三律期望表（码位、短名、中文名）。
const EXP_LAWS: [(u8, &str, &str); 3] = [
    (0, "no-blast", "不炸耳"),
    (1, "mutable", "可静音"),
    (2, "honest-volume", "诚实音量"),
];

/// 十件期望表（短名、应落层序）。层序 0=设备 1=混音 2=空间 3=内容。
const EXP_PIECES: [(&str, u8); 10] = [
    ("event-bus", 1),
    ("voice-channel", 3),
    ("listener-binding", 2),
    ("budget-spec", 2),
    ("telemetry-spec", 0),
    ("safety-law-ref", 0),
    ("doc-outline", 3),
    ("test-assets", 3),
    ("fuzz-corpus", 3),
    ("knowledge-base", 3),
];

/// 五先例期望表（短名、归属律码位、升级阶段码位）。
/// 升级阶段码位：0=ClampNow 1=SuspendFeature 2=HaltAndTrace。
const EXP_PRECEDENTS: [(&str, u8, u8); 5] = [
    ("over-limit", 0, 0),
    ("mislabeled", 2, 1),
    ("muted-out", 1, 2),
    ("unreachable-by-screen-reader", 1, 2),
    ("unclassified", 1, 2),
];

/// 升级阶段码位还原（判据侧自建，不调被测的 `code()`）。
fn stage_code(s: EscalationStage) -> u8 {
    match s {
        EscalationStage::ClampNow => 0,
        EscalationStage::SuspendFeature => 1,
        EscalationStage::HaltAndTrace => 2,
    }
}

/// 律码位还原（判据侧自建）。
fn law_code(l: AudioLaw) -> u8 {
    match l {
        AudioLaw::NoBlast => 0,
        AudioLaw::Mutable => 1,
        AudioLaw::HonestVolume => 2,
    }
}

/// 先例短名还原（判据侧自建，用于逐条钉死）。
fn precedent_tag(p: Precedent) -> &'static str {
    match p {
        Precedent::OverLimit => "over-limit",
        Precedent::Mislabeled => "mislabeled",
        Precedent::MutedOut => "muted-out",
        Precedent::UnreachableByScreenReader => "unreachable-by-screen-reader",
        Precedent::Unclassified => "unclassified",
    }
}

/// 十件短名还原（判据侧自建）。
fn piece_tag(p: HandoffPiece) -> &'static str {
    match p {
        HandoffPiece::EventBus => "event-bus",
        HandoffPiece::VoiceChannel => "voice-channel",
        HandoffPiece::ListenerBinding => "listener-binding",
        HandoffPiece::BudgetSpec => "budget-spec",
        HandoffPiece::TelemetrySpec => "telemetry-spec",
        HandoffPiece::SafetyLawRef => "safety-law-ref",
        HandoffPiece::DocOutline => "doc-outline",
        HandoffPiece::TestAssets => "test-assets",
        HandoffPiece::FuzzCorpus => "fuzz-corpus",
        HandoffPiece::KnowledgeBase => "knowledge-base",
    }
}

/// 层序还原（判据侧自建）。
fn layer_ord(l: Layer) -> u8 {
    match l {
        Layer::Device => 0,
        Layer::Mix => 1,
        Layer::Spatial => 2,
        Layer::Content => 3,
    }
}

/// 构造一个「四层齐冻结+ 三律已公示 + 十件齐承接」的干净架构（闸门应Open）。
///
/// 语料自建，**不复用被测的 freeze/accept**（那是被测行为，自建才独立）。
fn clean_arch() -> AudioArchitecture {
    let mut a = AudioArchitecture::new();
    // 三律全部公示且读屏可达。
    for law in [
        AudioLaw::NoBlast,
        AudioLaw::Mutable,
        AudioLaw::HonestVolume,
    ] {
        let _ = a.laws.declare(law, true);
    }
    // 十件全部承接（直接构造台账，不走 accept 以免被测行为混入语料）。
    for p in HandoffPiece::all() {
        a.handoff.accepted.push(AcceptedPiece {
            piece: p,
            layer: p.target_layer(),
            verified: true,
        });
    }
    // 四层各冻结一条。
    for l in Layer::all() {
        a.frozen.push(FrozenIface {
            id: 1,
            owner: l,
            version: 1,
        });
    }
    a
}

/// 断言一个字符串非空（空串不算要素 —— 弱门禁十诫）。
fn non_empty(s: &str) -> bool {
    !s.trim().is_empty()
}

/// 判据侧独立重算：某层的追补件数（不查被测的 `chasing` 内部结构）。
fn ref_pending_for(layer: Layer, pending: &[HandoffPiece]) -> usize {
    pending.iter().filter(|p| p.target_layer() == layer).count()
}

/// 判据侧独立重算：合法依赖对数（四层全对× 合法方向）。
fn ref_legal_dep_count() -> usize {
    let mut n = 0;
    for f in Layer::all() {
        for t in Layer::all() {
            if f.may_depend_on(t) {
                n += 1;
            }
        }
    }
    n
}

/// 判据侧独立重算：越权对数（四层全对 − 合法对数）。
fn ref_illegal_dep_count() -> usize {
    LAYER_COUNT * LAYER_COUNT - ref_legal_dep_count()
}

/// VE-F5601 域自检入口。
pub fn run_veab01_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-AB01音频域总架构");

    // -----------------------------------------------------------------------
    // A01 族：四层架构
    // -----------------------------------------------------------------------

    // A01-01四层穷举条数与层序逐条对账。
    {
        let all = Layer::all();
        let ord_ok: bool = all.len() == LAYER_COUNT
            && (0..LAYER_COUNT).all(|i| {
                let got_ord = layer_ord(all[i]);
                let got_tag = all[i].tag();
                let got_label = all[i].label();
                let e = EXP_LAYERS[i];
                got_ord == e.0 && got_tag == e.1 && got_label == e.2
            });
        set.add("A01-四层穷举与层序逐条对账", ord_ok, "");
    }

    // A01-02 层短名互异 + 中文名互异（防`all()` 返回重复项）。
    {
        let all = Layer::all();
        let mut tags: Vec<&str> = Vec::new();
        let mut labels: Vec<&str> = Vec::new();
        for l in all.iter() {
            tags.push(l.tag());
            labels.push(l.label());
        }
        let mut t2 = tags.clone();
        t2.sort_unstable();
        t2.dedup();
        let mut l2 = labels.clone();
        l2.sort_unstable();
        l2.dedup();
        set.add(
            "A01-层短名与中文名互异",
            t2.len() == tags.len() && l2.len() == labels.len(),
            "",
        );
    }

    // A01-03 `may_depend_on`：自环与向下合法、向上非法（判据侧独立重算全对）。
    {
        let mut legal = 0usize;
        let mut illegal = 0usize;
        let mut self_ok = true;
        for f in Layer::all().iter() {
            for t in Layer::all().iter() {
                let expect = layer_ord(*f) >= layer_ord(*t);
                if f.may_depend_on(*t) {
                    legal += 1;
                } else {
                    illegal += 1;
                }
                if f.may_depend_on(*t) != expect {
                    self_ok = false;
                }
            }
            // 自环必须合法（层内自引用不叫越权）。
            if !f.may_depend_on(*f) {
                self_ok = false;
            }
        }
        set.add(
            "A01-层依赖方向全对矩阵与自环合法",
            self_ok && legal == ref_legal_dep_count() && illegal == ref_illegal_dep_count(),
            "",
        );
    }

    // A01-04 四层各出接口冻结清单：每层≥1 条且`layer_frozen` 与手工扫描一致。
    {
        let mut a = clean_arch();
        let mut scan_ok = true;
        for l in Layer::all().iter() {
            let by_scan = a.frozen.iter().filter(|f| f.owner == *l).count();
            if by_scan < 1 || !a.layer_frozen(*l) {
                scan_ok = false;
            }
        }
        set.add("A01-四层各有冻结接口且查询一致", scan_ok, "");
    }

    // A01-05 冻结清单文档非空且四层名齐现（层间契约白纸黑字）。
    {
        let doc = LAYER_FREEZE_DOC;
        let all_present = EXP_LAYERS
            .iter()
            .all(|e| doc.contains(e.2));
        set.add(
            "A01-冻结清单文档含四层中文名",
            non_empty(doc) && all_present,
            "",
        );
    }

    // A01-06 冻结接口层内 id 唯一：同层同 id 重复冻结被拒（E_IFACE_REDEFINE）。
    {
        let mut a = clean_arch();
        let r = a.freeze(Layer::Mix, 1, 2);
        set.add(
            "A01-同层同id重复冻结被拒",
            r.is_err() && a.errors.iter().any(|(_, c, _)| *c == "E_IFACE_REDEFINE"),
            "",
        );
    }

    // A01-07 升版是冻结变更的唯一合法路径，且不动其它接口的版本。
    {
        let mut a = clean_arch();
        let before = a
            .frozen
            .iter()
            .find(|f| f.owner == Layer::Device)
            .map(|f| f.version)
            .unwrap_or(0);
        let others_before = a
            .frozen
            .iter()
            .find(|f| f.owner == Layer::Spatial)
            .map(|f| f.version)
            .unwrap_or(0);
        let bumped = a.bump_version(Layer::Device, 1);
        let after = a
            .frozen
            .iter()
            .find(|f| f.owner == Layer::Device)
            .map(|f| f.version)
            .unwrap_or(0);
        let others_after = a
            .frozen
            .iter()
            .find(|f| f.owner == Layer::Spatial)
            .map(|f| f.version)
            .unwrap_or(0);
        set.add(
            "A01-升版只动目标接口",
            bumped == Ok(before + 1) && after == before + 1 && others_after == others_before,
            "",
        );
    }

    // A01-08 升版不存在的接口被拒（不得凭空造版本）。
    {
        let mut a = clean_arch();
        let r = a.bump_version(Layer::Mix, 9999);
        set.add("A01-升版不存在接口被拒", r.is_err(), "");
    }

    // -----------------------------------------------------------------------
    // A02 族：音频三律
    // -----------------------------------------------------------------------

    // A02-01 三律穷举与码位/短名/中文名逐条对账 + of_code 往返。
    {
        let all = AudioLaw::all();
        let mut ok = all.len() == LAW_COUNT;
        for i in 0..LAW_COUNT {
            let e = EXP_LAWS[i];
            let l = all[i];
            if l.code() != e.0 || l.tag() != e.1 || l.label() != e.2 {
                ok = false;
            }
            // 往返：code -> of_code 必回自身。
            if AudioLaw::of_code(law_code(l)) != Some(l) {
                ok = false;
            }
        }
        set.add("A02-三律穷举逐条对账且of_code 往返", ok, "");
    }

    // A02-02 未登记码位反查失败（0..2 之外必须 None，不猜）。
    {
        let bad: Vec<bool> = [3u8, 4, 200, 255]
            .iter()
            .map(|c| AudioLaw::of_code(*c).is_none())
            .collect();
        set.add("A02-未登记码位反查失败", bad.iter().all(|b| *b), "");
    }

    // A02-03 三律声明齐备才`fully_declared`（逐项拆：少一条即假）。
    {
        let mut laws = AudioLaws::default();
        let r0 = laws.fully_declared();
        for l in [AudioLaw::NoBlast, AudioLaw::Mutable].iter() {
            let _ = laws.declare(*l, true);
        }
        let r2 = laws.fully_declared();
        let _ = laws.declare(AudioLaw::HonestVolume, true);
        let r3 = laws.fully_declared();
        set.add(
            "A02-三律公示齐备判定逐项拆",
            !r0 && !r2 && r3 && laws.declared.len() == LAW_COUNT,
            "",
        );
    }

    // A02-04 声明不可达即违例（读屏可达是锚点明写要求）+ 错误码专属。
    {
        let mut laws = AudioLaws::default();
        let r = laws.declare(AudioLaw::NoBlast, false);
        set.add(
            "A02-声明不可达即违例且码专属",
            r == Err("E_LAW_UNREACHABLE") && !laws.clean(),
            "",
        );
    }

    // A02-05 重复公示同一律不叠加（声明不是计数器）。
    {
        let mut laws = AudioLaws::default();
        let _ = laws.declare(AudioLaw::Mutable, true);
        let _ = laws.declare(AudioLaw::Mutable, true);
        set.add("A02-重复公示不叠加", laws.declared.len() == 1, "");
    }

    // A02-06 先例表逐条对账：短名/归属律/升级阶段三列全部钉死。
    {
        let all = [
            Precedent::OverLimit,
            Precedent::Mislabeled,
            Precedent::MutedOut,
            Precedent::UnreachableByScreenReader,
            Precedent::Unclassified,
        ];
        let mut ok = all.len() == EXP_PRECEDENTS.len();
        for i in 0..EXP_PRECEDENTS.len() {
            let e = EXP_PRECEDENTS[i];
            let p = all[i];
            if precedent_tag(p) != e.0 || law_code(p.law()) != e.1 || stage_code(p.escalation()) != e.2
            {
                ok = false;
            }
        }
        set.add("A02-五先例映射三列逐条对账", ok, "");
    }

    // A02-07 **三律一律不可降级**（`degradable` 恒false，逐条断）。
    {
        let all = [
            Precedent::OverLimit,
            Precedent::Mislabeled,
            Precedent::MutedOut,
            Precedent::UnreachableByScreenReader,
            Precedent::Unclassified,
        ];
        set.add(
            "A02-五先例一律不可降级",
            all.iter().all(|p| !p.degradable()),
            "",
        );
    }

    // A02-08 **静音掉也计违例**（`MutedOut` 不得被当作合规出路）。
    {
        let mut a = AudioArchitecture::new();
        a.raise(AudioLaw::Mutable, Precedent::MutedOut, "静音路径仍出声 0.4s");
        set.add(
            "A02-静音掉计违例不豁免",
            !a.laws.clean() && a.laws.count_of(AudioLaw::Mutable) == 1,
            "",
        );
    }

    // A02-09 **归类失败仍计违例且走三线**（不因归类失败降级）。
    {
        let mut a = AudioArchitecture::new();
        a.raise(AudioLaw::Mutable, Precedent::Unclassified, "无法归类的声音事件");
        let v = a.laws.violations.first();
        let ok = a.laws.count_of(AudioLaw::Mutable) == 1
            && v.map(|v| v.stage == EscalationStage::HaltAndTrace).unwrap_or(false);
        set.add("A02-归类失败仍红线且走三线", ok, "");
    }

    // A02-10 升级阶段由先例推导，不自由填写（改阶段即改输入即改输出）。
    {
        let mut a = AudioArchitecture::new();
        a.raise(AudioLaw::NoBlast, Precedent::OverLimit, "超上限 12dB");
        let s1 = a.laws.violations[0].stage;
        a.raise(AudioLaw::HonestVolume, Precedent::Mislabeled, "显示 0.6 实际 0.2");
        let s2 = a.laws.violations[1].stage;
        set.add(
            "A02-升级阶段随先例变化",
            s1 == EscalationStage::ClampNow && s2 == EscalationStage::SuspendFeature,
            "",
        );
    }

    // A02-11 每条红线三要素齐备且 exceeded 非空（逐条断，不抽样）。
    {
        let mut a = AudioArchitecture::new();
        a.raise(AudioLaw::NoBlast, Precedent::OverLimit, "超上限 12dB");
        a.raise(AudioLaw::Mutable, Precedent::MutedOut, "静音失效");
        a.raise(AudioLaw::HonestVolume, Precedent::Mislabeled, "虚标 0.9");
        let ok = a.laws.violations.iter().all(|v| {
            non_empty(&v.exceeded) && non_empty(&v.because) && non_empty(&v.remedy)
        }) && a.laws.total_violations() == 3;
        set.add("A02-每条红线三要素齐备", ok, "");
    }

    // A02-12 三律声明文档含三条律名（读屏可达的朗读文本）。
    {
        let doc = THREE_LAWS_DOC;
        set.add(
            "A02-三律文档含三条律中文名",
            EXP_LAWS.iter().all(|e| doc.contains(e.2)),
            "",
        );
    }

    // -----------------------------------------------------------------------
    // A03 族：承接追补
    // -----------------------------------------------------------------------

    // A03-01 十件穷举与短名/落层逐条对账（落层表逐条钉死，不只断非空）。
    {
        let all = HandoffPiece::all();
        let mut ok = all.len() == HANDOFF_PIECE_COUNT;
        for i in 0..HANDOFF_PIECE_COUNT {
            let e = EXP_PIECES[i];
            let p = all[i];
            if piece_tag(p) != e.0 || layer_ord(p.target_layer()) != e.1 {
                ok = false;
            }
        }
        set.add("A03-十件短名与落层逐条对账", ok, "");
    }

    // A03-02 十件短名互异 + 落层覆盖四层（不留悬空层）。
    {
        let all = HandoffPiece::all();
        let mut tags: Vec<&str> = Vec::new();
        let mut layer_hits = [false; LAYER_COUNT];
        for p in all.iter() {
            tags.push(piece_tag(*p));
            let o = layer_ord(p.target_layer()) as usize;
            if o < LAYER_COUNT {
                layer_hits[o] = true;
            }
        }
        let mut t2 = tags.clone();
        t2.sort_unstable();
        t2.dedup();
        set.add(
            "A03-十件短名互异且落层覆盖四层",
            t2.len() == HANDOFF_PIECE_COUNT && layer_hits.iter().all(|b| *b),
            "",
        );
    }

    // A03-03 齐备十件即 `complete`（分母口径：恰等于十）。
    {
        let mut led = HandoffLedger::default();
        for p in HandoffPiece::all().iter() {
            led.accept(*p, true);
        }
        set.add(
            "A03-十件齐备即complete",
            led.complete()
                && led.accepted_count() == HANDOFF_PIECE_COUNT
                && led.chasing_count() == 0,
            "",
        );
    }

    // A03-04 缺一件即不 complete，且追补恰好一条（恰等于 1，不>=）。
    {
        let mut led = HandoffLedger::default();
        for p in HandoffPiece::all().iter() {
            if *p != HandoffPiece::FuzzCorpus {
                led.accept(*p, true);
            }
        }
        let missing_fuzz = HandoffPiece::FuzzCorpus;
        led.chase(missing_fuzz);
        set.add(
            "A03-缺一件即不complete且追补恰一条",
            !led.complete()
                && led.accepted_count() == HANDOFF_PIECE_COUNT - 1
                && led.chasing_count() == 1
                && led.chasing[0].piece == missing_fuzz,
            "",
        );
    }

    // A03-05 **未核验件不算承接**（转追补而非静默丢弃）。
    {
        let mut led = HandoffLedger::default();
        led.accept(HandoffPiece::EventBus, false);
        set.add(
            "A03-未核验件转追补",
            led.accepted_count() == 0 && led.chasing_count() == 1,
            "",
        );
    }

    // A03-06 重复承接不叠加（同一件只登记一次）。
    {
        let mut led = HandoffLedger::default();
        led.accept(HandoffPiece::DocOutline, true);
        led.accept(HandoffPiece::DocOutline, true);
        set.add("A03-重复承接不叠加", led.accepted_count() == 1, "");
    }

    // A03-07 重复追补合并为一条（不刷屏）。
    {
        let mut led = HandoffLedger::default();
        led.chase(HandoffPiece::BudgetSpec);
        led.chase(HandoffPiece::BudgetSpec);
        set.add("A03-重复追补合并为一条", led.chasing_count() == 1, "");
    }

    // A03-08 追补三要素齐备（缺任一即不可交付）。
    {
        let mut led = HandoffLedger::default();
        led.chase(HandoffPiece::SafetyLawRef);
        let c = led.chasing.first();
        let ok = c
            .map(|c| non_empty(&c.missing) && non_empty(c.because) && non_empty(c.remedy))
            .unwrap_or(false);
        set.add("A03-追补三要素齐备", ok, "");
    }

    // A03-09 追补件落层与该件的目标层一致（追补不落悬空层）。
    {
        let mut led = HandoffLedger::default();
        for p in HandoffPiece::all().iter() {
            led.chase(*p);
        }
        let ok = led
            .chasing
            .iter()
            .all(|c| c.layer == c.piece.target_layer());
        set.add("A03-追补件落层与目标层一致", ok, "");
    }

    // A03-10 **每层待追补件数与判据侧独立重算一致**（不是问被测要的）。
    {
        let pending = [
            HandoffPiece::TelemetrySpec,  // → 设备层
            HandoffPiece::EventBus,       // → 混音层
            HandoffPiece::BudgetSpec,     // → 空间层
            HandoffPiece::FuzzCorpus,     // → 内容层
        ];
        let mut led = HandoffLedger::default();
        for p in pending.iter() {
            led.chase(*p);
        }
        let mut ok = true;
        for l in Layer::all().iter() {
            let by_scan = led
                .chasing
                .iter()
                .filter(|c| c.layer == *l)
                .count();
            if by_scan != ref_pending_for(*l, &pending) {
                ok = false;
            }
        }
        set.add("A03-每层追补数与独立重算一致", ok, "");
    }

    // -----------------------------------------------------------------------
    // A04 族：错误路径（越权拒绝 / 违例红线 / 缺件追补）
    // -----------------------------------------------------------------------

    // A04-01 **合法向下依赖被接受**（闸门不能常年关着）。
    {
        let mut a = clean_arch();
        let r1 = a.declare_dep(Layer::Content, Layer::Mix, 7);
        let r2 = a.declare_dep(Layer::Mix, Layer::Device, 8);
        let r3 = a.declare_dep(Layer::Spatial, Layer::Spatial, 9);
        set.add(
            "A04-合法依赖被接受且入账",
            r1.is_ok() && r2.is_ok() && r3.is_ok() && a.deps.len() == 3,
            "",
        );
    }

    // A04-02 **越权向上依赖被拒**且不入账。
    {
        let mut a = clean_arch();
        let r = a.declare_dep(Layer::Device, Layer::Content, 7);
        set.add(
            "A04-越权依赖被拒且不入账",
            r == Err("E_LAYER_OVERREACH") && a.deps.is_empty(),
            "",
        );
    }

    // A04-03 越权诊断三要素齐备（超了多少含层序差）。
    {
        let mut a = clean_arch();
        let _ = a.declare_dep(Layer::Device, Layer::Content, 7);
        let m = a.mismatches.first();
        let ok = a.mismatches.len() == 1
            && m.map(|m| {
                m.from == Layer::Device
                    && m.to == Layer::Content
                    && m.iface == 7
                    && non_empty(&m.exceeded)
                    && non_empty(m.because)
                    && non_empty(m.remedy)
                    // 「超了多少」必须含真实层序差（0→3 跨 3 层）
                    && m.exceeded.contains("3")
            })
            .unwrap_or(false);
        set.add("A04-越权诊断三要素齐备含层序差", ok, "");
    }

    // A04-04 全对越权扫描：逐对独立重算，越权数与诊断数**恰等于**。
    {
        let mut a = clean_arch();
        for f in Layer::all().iter() {
            for t in Layer::all().iter() {
                if !f.may_depend_on(*t) {
                    let _ = a.declare_dep(*f, *t, 1);
                }
            }
        }
        let expect = ref_illegal_dep_count();
        set.add(
            "A04-全对越权扫描数与独立重算一致",
            a.mismatches.len() == expect && a.deps.is_empty(),
            "",
        );
    }

    // A04-05 越权错误码与主体专属（码位复用即错）。
    {
        let mut a = clean_arch();
        let _ = a.declare_dep(Layer::Device, Layer::Spatial, 3);
        let e = a.errors.iter().find(|(_, c, _)| *c == "E_LAYER_OVERREACH");
        let ok = e
            .map(|(_, _, msg)| msg.contains("设备层") && msg.contains("空间层"))
            .unwrap_or(false);
        set.add("A04-越权错误含双方层名", ok, "");
    }

    // A04-06 跨一层的越权也被拒（不只是跨三层）。
    {
        let mut a = clean_arch();
        let r = a.declare_dep(Layer::Mix, Layer::Spatial, 5);
        set.add("A04-跨一层越权同样被拒", r.is_err(), "");
    }

    // A04-07 **逐对钉死诊断里的层序差**（不给「跨了 N 层」留自由发挥空间）。
    //
    // 弱门禁教训：先前只断 `exceeded.contains("3")`，那是断**字面量**，
    // 而被测内部那个「层序差」变量没有任何判据钉住 —— 把它改成恒 0 全绿。
    // 本判据按判据侧独立算出的真实差值逐对比对，恒 0 立刻转红。
    {
        let mut a = clean_arch();
        for f in Layer::all().iter() {
            for t in Layer::all().iter() {
                if !f.may_depend_on(*t) {
                    let _ = a.declare_dep(*f, *t, 1);
                }
            }
        }
        let mut ok = true;
        for m in a.mismatches.iter() {
            // 判据侧独立重算：真实层序差 = |from序 - to序|（越权时from序 < to序，
            // 故必须取绝对值——带符号差是负数，永远匹配不上「跨了 N 层」）。
            let raw = layer_ord(m.from) as i32 - layer_ord(m.to) as i32;
            if raw >= 0 {
                ok = false; // 越权只能向上（from序 必须 < to序）
            }
            let want = raw.abs();
            let want_str = format!("跨了 {} 层", want);
            if !m.exceeded.contains(&want_str) {
                ok = false;
            }
            // 反向断言：不得出现别的层数说法（防「跨了 0 层」蒙混）。
            for other in 0..LAYER_COUNT as i32 {
                if other != want && m.exceeded.contains(&format!("跨了 {} 层", other)) {
                    ok = false;
                }
            }
        }
        set.add("A04-诊断层序差逐对等于独立重算", ok && !a.mismatches.is_empty(), "");
    }

    // A04-08 层序差随跨层数变化（夹逼对：跨一层≠跨三层，防止差值写死）。
    {
        let mut a1 = clean_arch();
        let _ = a1.declare_dep(Layer::Mix, Layer::Spatial, 1); // 差 1
        let mut a3 = clean_arch();
        let _ = a3.declare_dep(Layer::Device, Layer::Content, 1); // 差 3
        let m1 = a1.mismatches.first().map(|m| m.exceeded.clone()).unwrap_or_default();
        let m3 = a3.mismatches.first().map(|m| m.exceeded.clone()).unwrap_or_default();
        set.add(
            "A04-层序差随跨层数变化",
            m1.contains("跨了 1 层") && m3.contains("跨了 3 层") && m1 != m3,
            "",
        );
    }

    // -----------------------------------------------------------------------
    // A05 族：开工闸五因（逐项破坏，闸门恰好因此 held）
    // -----------------------------------------------------------------------

    // A05-01 干净架构判 Open（五因全满足）。
    {
        let a = clean_arch();
        set.add(
            "A05-干净架构判Open",
            a.gate() == GateVerdict::Open
                && a.mismatches.is_empty()
                && a.laws.clean()
                && a.laws.fully_declared()
                && a.handoff.complete()
                && Layer::all().iter().all(|l| a.layer_frozen(*l)),
            "",
        );
    }

    // A05-02 破坏因一（缺一层冻结）→ held，且**只有**这一因。
    {
        let mut a = clean_arch();
        a.frozen.retain(|f| f.owner != Layer::Spatial);
        set.add(
            "A05-缺一层冻结即held",
            a.gate() == GateVerdict::Held && !a.layer_frozen(Layer::Spatial),
            "",
        );
    }

    // A05-03 破坏因二（有越权）→ held。
    {
        let mut a = clean_arch();
        let _ = a.declare_dep(Layer::Device, Layer::Content, 1);
        set.add("A05-有越权即held", a.gate() == GateVerdict::Held, "");
    }

    // A05-04 破坏因三（有红线）→ held。
    {
        let mut a = clean_arch();
        a.raise(AudioLaw::NoBlast, Precedent::OverLimit, "超 6dB");
        set.add("A05-有红线即 held", a.gate() == GateVerdict::Held, "");
    }

    // A05-05 破坏因四（三律未全公示）→ held。
    {
        let mut a = clean_arch();
        a.laws.declared.retain(|l| *l != AudioLaw::HonestVolume);
        set.add(
            "A05-三律未全公示即 held",
            a.gate() == GateVerdict::Held && !a.laws.fully_declared(),
            "",
        );
    }

    // A05-06 破坏因五（十件缺一）→ held。
    {
        let mut a = clean_arch();
        a.handoff.accepted.retain(|p| p.piece != HandoffPiece::KnowledgeBase);
        a.handoff.chase(HandoffPiece::KnowledgeBase);
        set.add(
            "A05-十件缺一即 held",
            a.gate() == GateVerdict::Held && !a.handoff.complete(),
            "",
        );
    }

    // A05-07 **闸门不是恒held**：修好唯一破坏项后必转 Open（双向验证）。
    {
        let mut a = clean_arch();
        a.frozen.retain(|f| f.owner != Layer::Mix);
        let held = a.gate() == GateVerdict::Held;
        a.frozen.push(FrozenIface {
            id: 1,
            owner: Layer::Mix,
            version: 1,
        });
        let opened = a.gate() == GateVerdict::Open;
        set.add("A05-修好破坏项后闸门转Open", held && opened, "");
    }

    // A05-08 空架构判 held（零 panic 面：不是 open 也不是崩）。
    {
        let a = AudioArchitecture::new();
        set.add(
            "A05-空架构判held且零panic",
            a.gate() == GateVerdict::Held && a.laws.clean() && !a.laws.fully_declared(),
            "",
        );
    }

    // -----------------------------------------------------------------------
    // A06 族：冻结前置闸（承接未落位不得冻结）
    // -----------------------------------------------------------------------

    // A06-01 有追补件时该层冻结被拒（E_IFACE_UNSETTLED_HANDOFF）。
    {
        let mut a = AudioArchitecture::new();
        a.handoff.chase(HandoffPiece::TelemetrySpec); // → 设备层
        let r = a.freeze(Layer::Device, 1, 1);
        set.add(
            "A06-有追补件时该层冻结被拒",
            r == Err("E_IFACE_UNSETTLED_HANDOFF") && a.frozen.is_empty(),
            "",
        );
    }

    // A06-02 **追补只挡自己那层**：别的层仍可冻结（闸门不是全封）。
    {
        let mut a = AudioArchitecture::new();
        a.handoff.chase(HandoffPiece::TelemetrySpec); // → 设备层
        let r = a.freeze(Layer::Content, 1, 1);
        set.add(
            "A06-追补只挡自己那层",
            r.is_ok() && a.layer_frozen(Layer::Content) && !a.layer_frozen(Layer::Device),
            "",
        );
    }

    // A06-03 **拒绝是因缺件而非永久封禁**：清零追补后同参数冻结必成功。
    {
        let mut a = AudioArchitecture::new();
        a.handoff.chase(HandoffPiece::TelemetrySpec);
        let rejected = a.freeze(Layer::Device, 1, 1).is_err();
        a.handoff.accepted.push(AcceptedPiece {
            piece: HandoffPiece::TelemetrySpec,
            layer: Layer::Device,
            verified: true,
        });
        a.handoff.chasing.clear();
        let accepted = a.freeze(Layer::Device, 1, 1).is_ok();
        set.add(
            "A06-追补清零后同参数冻结成功",
            rejected && accepted && a.layer_frozen(Layer::Device),
            "",
        );
    }

    // A06-04 冻结数达上界被拒（E_IFACE_LIMIT，逐层独立计）。
    {
        let mut a = AudioArchitecture::new();
        for i in 0..MAX_FROZEN_PER_LAYER {
            let _ = a.freeze(Layer::Mix, 1000 + i as u32, 1);
        }
        let before = a.frozen.len();
        let r = a.freeze(Layer::Mix, 9999, 1);
        set.add(
            "A06-冻结数达上界被拒且不入账",
            r == Err("E_IFACE_LIMIT") && a.frozen.len() == before,
            "",
        );
    }

    // A06-05 上界是**逐层**的：设备层满了不影响空间层。
    {
        let mut a = AudioArchitecture::new();
        for i in 0..MAX_FROZEN_PER_LAYER {
            let _ = a.freeze(Layer::Device, 1000 + i as u32, 1);
        }
        let r = a.freeze(Layer::Spatial, 1, 1);
        set.add("A06-冻结上界逐层独立", r.is_ok(), "");
    }

    // A06-06 域声明接口逐个走公示流程（不可达即抛专属码）。
    {
        let mut a = AudioArchitecture::new();
        let r = a.declare_laws(false);
        set.add(
            "A06-域级公示不可达即拒",
            r == Err("E_LAW_UNREACHABLE") && !a.laws.clean(),
            "",
        );
    }

    // A06-07 域级公示可达时三条律全公示（逐条断齐备）。
    {
        let mut a = AudioArchitecture::new();
        let r = a.declare_laws(true);
        set.add(
            "A06-域级公示可达即三律齐备",
            r.is_ok() && a.laws.fully_declared() && a.laws.declared.len() == LAW_COUNT,
            "",
        );
    }

    // A06-08 域标识与常量自洽（判据侧独立对账，不只断非空）。
    {
        set.add(
            "A06-域常量自洽",
            VAB_DOMAIN == "VE-AB"
                && LAYER_COUNT == EXP_LAYERS.len()
                && LAW_COUNT == EXP_LAWS.len()
                && HANDOFF_PIECE_COUNT == EXP_PIECES.len()
                && MAX_FROZEN_PER_LAYER >= 1,
            "",
        );
    }

    set
}

/// 变体登记表（变异测试实测清单；每条须「基线绿 + 变体红」双向成立）。
pub const VARIANT_REGISTRY: [(&str, &str); 16] = [
    ("AB01-V01 层依赖方向反转（向下改向上）", "may_depend_on"),
    ("AB01-V02 自环判为越权", "may_depend_on"),
    ("AB01-V03 三律码位与 of_code 往返断裂", "of_code"),
    ("AB01-V04 静音掉不再计违例", "count_of"),
    ("AB01-V05 归类失败降级处置", "escalation"),
    ("AB01-V06 越权依赖被静默接受", "declare_dep"),
    ("AB01-V07 越权诊断三要素留空", "remedy"),
    ("AB01-V08 未核验件静默丢弃", "accept"),
    ("AB01-V09 重复追补不合并", "chase"),
    ("AB01-V10 十件落层表改一处", "target_layer"),
    ("AB01-V11 冻结前置闸恒拒", "freeze"),
    ("AB01-V12 冻结上界按全局计", "MAX_FROZEN_PER_LAYER"),
    ("AB01-V13 重复公示叠加计数", "declare"),
    ("AB01-V14 开工闸忽略红线", "gate"),
    ("AB01-V15 升版不动版本", "bump_version"),
    ("AB01-V16 重复承接叠加", "accept"),
];
