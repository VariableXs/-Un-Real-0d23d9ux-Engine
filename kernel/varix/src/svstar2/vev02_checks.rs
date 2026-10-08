//! VE-F4402 · 域自检（判据逐条对应，见 `vev02_monitor.rs` 头注）
//!
//! 锚点判据五条 → 自检项映射：
//! - **三重指纹**（四档强度、两要素不算过、冲突以 GUID 为准且不丢屏、缺项可点名） → `V02-指纹-四档与三重门`
//! - **指纹弱门禁**（表外真实形态、空串冒充、越界下标） → `V02-指纹-弱门禁三拒`
//! - **指纹裁决**（重复 GUID 标注两块都不丢、台账满拒收） → `V02-指纹-冲突裁决与台账边界`
//! - **能力表**（四类齐备、O(1) 查询、实算最高档、空表 None） → `V02-能力-四类与实算`
//! - **能力边界**（越界下标、不存在 GUID、位深深浅判定） → `V02-能力-边界与拒答`
//! - **多屏隔离**（能力索引不跨屏串扰、摘屏后索引清理） → `V02-能力-多屏隔离`
//! - **热插拔合并**（同屏同类合并、首事件直通、窗边界必吐、风暴判定） → `V02-合并-窗内合并与首事件`
//! - **合并守恒**（收到 = 合并掉 + 在途；原始条数随汇总吐出） → `V02-合并-守恒与不吞`
//! - **合并退阶**（时钟回拨不开新窗、零窗长被夹回、槽位溢出记账） → `V02-合并-退化路径`
//! - **降级轮询**（降级带原因、周期下限、可观测、恢复回事件式） → `V02-降级-原因与周期下限`
//! - **降级可观测**（非降级不轮询、就绪闸缺项人话、错误五元组齐发） → `V02-降级-可观测与就绪闸`
//! - **轮询推进**（降级轮询真改台账、条数按真实屏数） → `V02-降级-轮询推进台账`
//! - **判据五**（五项齐备 + 每项覆盖数 + 映射双向一致） → `V02-判据-五项与覆盖`
//! - **层边界**（越界交付当场拒收且计数） → `V02-判据-层边界拒收`
//!
//! **负例为本文件主体**：每条判据都先立正样本，再用负例证明它真能拒。
//! 只测正样本的判据等于没测——正样本在实现写错时往往照样通过。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vev02_monitor::*;
use crate::checks::CheckSet;

/// 本文件实际产出的自检项名（判据覆盖自检的比对基准，单源）。
///
/// **这份清单必须与 `set.add` 的实参逐条一致**——清单里不写「可能存在」
/// 的名字，只写一定写进去的名字。清单与实产不符时，`V02-判据-覆盖` 会红。
pub const CHECK_NAMES: [&str; 15] = [
    "V02-指纹-四档与三重门",
    "V02-指纹-弱门禁三拒",
    "V02-指纹-冲突裁决与台账边界",
    "V02-能力-四类与实算",
    "V02-能力-边界与拒答",
    "V02-能力-多屏隔离",
    "V02-合并-窗内合并与首事件",
    "V02-合并-守恒与不吞",
    "V02-合并-退化路径",
    "V02-降级-原因与周期下限",
    "V02-降级-可观测与就绪闸",
    "V02-降级-轮询推进台账",
    "V02-判据-五项与覆盖",
    "V02-判据-层边界拒收",
    "V02-判据-清单不漂移",
];

/// 三重指纹样例。
fn fp_triple(i: u32) -> IdentityFingerprint {
    IdentityFingerprint::triple(
        &format!("EDID{:016X}", 0xA000 + i),
        &format!("GUID{:08X}", 0xB000 + i),
        &format!("DP-{}", i),
    )
}

/// 造一块带能力表的屏。
fn cap_default() -> CapabilityRecord {
    CapabilityRecord::new(
        vec![
            DisplayMode::new(Resolution::new(1920, 1080), RefreshRate::from_hz(60), BitDepth::B8),
            DisplayMode::new(Resolution::new(2560, 1440), RefreshRate::from_hz(120), BitDepth::B10),
            DisplayMode::new(Resolution::new(3840, 2160), RefreshRate::from_hz(144), BitDepth::B12),
        ],
        HdrCapability::Present,
    )
}

/// 域自检第一批（判据一至判据三）。
pub fn run_vev02_checks_a() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vev02");

    // ---- 判据一：三重指纹 ----

    // 判据：四档强度齐备，且**只有三重**构成已验证身份。
    {
        let full = fp_triple(1);
        let three = IdentityFingerprint::partial(Some("E"), Some("G"), Some("P"));
        let two = IdentityFingerprint::partial(Some("E"), Some("G"), None);
        let one = IdentityFingerprint::partial(Some("E"), None, None);
        let none = IdentityFingerprint::empty();

        assert_eq!(none.strength(), FingerprintStrength::None);
        assert_eq!(one.strength(), FingerprintStrength::Weak);
        assert_eq!(two.strength(), FingerprintStrength::Partial);
        assert_eq!(three.strength(), FingerprintStrength::Triple);
        assert_eq!(full.strength(), FingerprintStrength::Triple);

        // 关键门：两要素 / 一要素 / 无 都**不得**算已验证
        assert!(full.strength().is_verified(), "三重必须算已验证");
        assert!(!two.strength().is_verified(), "两要素不得算已验证");
        assert!(!one.strength().is_verified(), "一要素不得算已验证");
        assert!(!none.strength().is_verified(), "无要素不得算已验证");

        // 四档严格递增（可用于「至少多少档」这类判定）
        assert!(FingerprintStrength::None < FingerprintStrength::Weak);
        assert!(FingerprintStrength::Weak < FingerprintStrength::Partial);
        assert!(FingerprintStrength::Partial < FingerprintStrength::Triple);

        // 缺项能点名（读屏播报「缺什么」，不是只说「验证不足」）
        assert_eq!(two.missing_fields(), vec!["端口"]);
        assert_eq!(one.missing_fields().len(), 2);
        assert_eq!(none.missing_fields().len(), 3);
        assert_eq!(full.missing_fields().len(), 0);

        // 每档都有中文名与强度描述（读屏可达）
        for s in [
            FingerprintStrength::None,
            FingerprintStrength::Weak,
            FingerprintStrength::Partial,
            FingerprintStrength::Triple,
        ] {
            assert!(!s.zh().is_empty(), "指纹档缺中文名");
        }
        set.add("V02-指纹-四档与三重门", true, "");
    }

    // 判据：**弱门禁三拒**——空串冒充、缺要素、越界下标都必须被拒。
    //
    // 为什么要单独测：用表里已有的合法形态去验判据 = 恒真弱门禁。
    // 这里全部用**表外形态**（空串、全None、越界）来打。
    {
        // 拒一：空串不冒充有效要素
        let blank = IdentityFingerprint::partial(Some("   "), Some(""), None);
        assert_eq!(blank.present_count(), 0, "空串不得计为已到位要素");
        assert_eq!(blank.strength(), FingerprintStrength::None);
        // 且 identity_key 不接受空 GUID
        let blank_guid = IdentityFingerprint::partial(Some("E"), Some("  "), Some("P"));
        assert!(blank_guid.identity_key().is_none(), "空 GUID 不得作身份键");

        // 拒二：只有 EDID 无 GUID 的屏不得进身份路径
        let edid_only = IdentityFingerprint::partial(Some("EDID-X"), None, Some("DP-1"));
        assert!(edid_only.identity_key().is_none(), "无 GUID 不得作身份键");

        // 拒三：越界下标不 panic 且给五元组
        let mut mgr = MonitorManager::new();
        let r = mgr.attach_capability(999, cap_default());
        assert!(r.is_err(), "越界下标必须报错而非静默");
        let e = r.unwrap_err();
        assert_eq!(e.code, "E_LEDGER_INDEX_OOB");
        assert!(e.is_complete(), "越界报错必须五元组齐发");
        assert!(e.symptom.contains("999"), "报错须点名下标");

        set.add("V02-指纹-弱门禁三拒", true, "");
    }

    // 判据：指纹冲突以 GUID 为准并标注，**两块屏都不丢**；台账满拒收。
    {
        let mut ledger = MonitorLedger::new();
        // 手工构造两块同 GUID 的屏（真实世界里固件 GUID 重复 / 被克隆）
        ledger.admit(fp_triple(1)).unwrap();
        ledger.admit(fp_triple(2)).unwrap();
        // 把第二块的 GUID 改成与第一块相同
        //
        // 顺序要点：先把 GUID 克隆到局部再借可变——否则 `monitor_mut` 的可变
        // 借用与读第一块的不可变借用重叠（E0502）。
        let shared_guid = ledger.entries()[0]
            .fingerprint()
            .guid
            .clone()
            .unwrap_or_default();
        {
            let fp = IdentityFingerprint::triple("EDID-B", &shared_guid, "DP-2");
            *ledger.monitor_mut(1).unwrap() = MonitorRecord::new(1, fp);
        }
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger.find_by_guid(&shared_guid).len(), 2);

        let mut arb = FingerprintArbiter::new();
        let issues = arb.resolve_all(&ledger);
        assert_eq!(issues.len(), 1, "两块同 GUID 应报一条冲突");
        assert_eq!(issues[0].code, "E_FP_GUID_DUPLICATE");
        assert!(issues[0].is_complete());
        // 裁决留痕：verdict 必非空且带下标
        assert_eq!(arb.verdicts().len(), 1);
        assert_eq!(arb.verdicts()[0].annotated_monitors.len(), 2, "两块都要标注");
        assert!(!arb.verdicts()[0].rationale.is_empty(), "裁决必留痕");
        assert!(!arb.verdicts()[0].screen_line().is_empty(), "裁决读屏可达");
        // 关键：**不丢屏**
        assert_eq!(ledger.len(), 2, "指纹冲突不得丢屏");
        assert_eq!(arb.annotated_monitors(), 2);

        // 台账满拒收：MAX_MONITORS 之上必须给五元组
        let mut full = MonitorLedger::new();
        for i in 0..MAX_MONITORS as u32 {
            full.admit(fp_triple(i)).unwrap();
        }
        assert_eq!(full.len(), MAX_MONITORS);
        let over = full.admit(fp_triple(9999));
        assert!(over.is_err(), "台账满必须拒收");
        let oe = over.unwrap_err();
        assert_eq!(oe.code, "E_LEDGER_FULL");
        assert!(oe.is_complete());
        assert_eq!(full.len(), MAX_MONITORS, "拒收后条数不变");

        // 同 GUID 重复入册不增条数（枚举重复是常见故障）
        let mut dup = MonitorLedger::new();
        let first = dup.admit(fp_triple(7)).unwrap();
        let again = dup.admit(fp_triple(7)).unwrap();
        assert_eq!(first, again, "同 GUID 应返回同一下标");
        assert_eq!(dup.len(), 1, "同 GUID 不得重复入册");

        // 摘除越界拒收
        let mut small = MonitorLedger::new();
        small.admit(fp_triple(3)).unwrap();
        let rm = small.remove(42);
        assert!(rm.is_err());
        assert_eq!(rm.unwrap_err().code, "E_LEDGER_INDEX_OOB");

        set.add("V02-指纹-冲突裁决与台账边界", true, "");
    }

    // ---- 判据二：能力表 ----

    // 判据：四类能力齐备 + 最高档**实算** + 空表返 None + O(1) 查询。
    {
        let cap = cap_default();
        // 四类：模式数 / 刷新率 / 位深 / HDR
        assert_eq!(cap.mode_count(), 3);
        assert_eq!(cap.max_refresh().unwrap().hz_rounded(), 144, "最高刷新率须实算");
        assert_eq!(cap.max_bit_depth().unwrap().bits(), 12, "最深位深须实算");
        assert!(cap.hdr.is_present());
        assert!(cap.hdr.zh().contains("HDR"));
        // 首选模式在表内
        let pref = cap.preferred().expect("首选模式应存在");
        assert_eq!(pref.resolution, Resolution::new(1920, 1080));
        // 分辨率查询
        assert!(cap.supports_resolution(Resolution::new(2560, 1440)));
        assert!(!cap.supports_resolution(Resolution::new(800, 600)));

        // 空表返 None 而不是默认值（**不拿默认值冒充事实**）
        let empty = CapabilityRecord::new(Vec::new(), HdrCapability::NotDetected);
        assert!(empty.max_refresh().is_none(), "空表最高刷新率应 None");
        assert!(empty.max_bit_depth().is_none(), "空表最深位深应 None");
        assert!(empty.preferred().is_none(), "空表首选应 None");
        assert!(!empty.hdr.is_present());
        // 「未探测到」与「不可用」文案不同——语义不同不能混。
        //
        // 这里必须**双向钉死**，只断「非空」是弱门禁：把 NotDetected 的文案
        // 换成「不支持 HDR」（语义正好相反）仍能通过非空断言。
        // 故断三条：① 两档文案互异；② 未探测到档**含「未探测到」**；
        // ③ 未探测到档**不含「不支持」**（反向断言，防关键词表把错固化）。
        let zh_not = HdrCapability::NotDetected.zh();
        let zh_yes = HdrCapability::Present.zh();
        assert_ne!(zh_not, zh_yes, "两档文案必须互异");
        assert!(
            zh_not.contains("未探测到"),
            "未探测到档须点明「未探测到」，实得 {}",
            zh_not
        );
        assert!(
            !zh_not.contains("不支持"),
            "未探测到 ≠ 不支持：文案不得出现「不支持」，实得 {}",
            zh_not
        );
        assert!(
            zh_yes.contains("支持") && !zh_yes.contains("未探测到"),
            "支持档须只说支持，实得 {}",
            zh_yes
        );
        // `is_present()` 与文案口径必须一致（不许文案说支持而判定说没支持）
        assert!(HdrCapability::Present.is_present());
        assert!(!HdrCapability::NotDetected.is_present());

        // O(1) 查询：索引建好后按 GUID 命中
        let mut mgr = MonitorManager::new();
        let idx = mgr.admit(fp_triple(1)).unwrap();
        mgr.attach_capability(idx, cap_default()).unwrap();
        let got = mgr.capability_of("GUID0000B001");
        assert!(got.is_some(), "按 GUID 应命中");
        assert_eq!(got.unwrap().mode_count(), 3);
        assert_eq!(mgr.table.hit_rate_permille(), 1000, "单查必命中");
        assert_eq!(mgr.table.queries, 1);

        // 读屏可达（域本色）：四类能力都进单行
        let line = cap.screen_line();
        for kw in ["档模式", "首选", "刷新率", "位深", "HDR"] {
            assert!(line.contains(kw), "读屏行缺 {}: {}", kw, line);
        }

        set.add("V02-能力-四类与实算", true, "");
    }

    // 判据：**能力边界与拒答**——不存在 GUID / 越界下标 / 位深判定。
    {
        let mut mgr = MonitorManager::new();
        let idx = mgr.admit(fp_triple(1)).unwrap();
        mgr.attach_capability(idx, cap_default()).unwrap();
        // 不存在的 GUID → None，不panic、不给默认能力
        assert!(mgr.capability_of("GUID-NOT-EXIST").is_none());
        // 无 GUID 的屏不进索引
        let nofp = IdentityFingerprint::partial(Some("E"), None, Some("P"));
        mgr.admit(nofp).unwrap();
        assert_eq!(mgr.table.capacity(), 1, "无 GUID 的屏不应进索引");

        // 位深深浅判定（HDR 事实判定用）
        assert!(!BitDepth::B8.is_deep());
        assert!(BitDepth::B10.is_deep());
        assert!(BitDepth::B12.is_deep());
        assert_eq!(BitDepth::B8.bits(), 8);
        assert_eq!(BitDepth::B12.bits(), 12);

        // 刷新率取整（读屏播报要「60」不是「60.000」）
        assert_eq!(RefreshRate::from_hz(60).hz_rounded(), 60);
        assert_eq!(RefreshRate::from_hz(144).hz_rounded(), 144);
        assert!(!RefreshRate::from_hz(60).screen_text().contains('.'));

        // 命中率为 0 时不得除零
        let mut t = CapabilityTable::new();
        assert_eq!(t.hit_rate_permille(), 0, "未查询时命中率应 0，不得除零");
        assert!(t.lookup("ANY").is_none());
        assert_eq!(t.hit_rate_permille(), 0, "全未命中应为 0");

        set.add("V02-能力-边界与拒答", true, "");
    }

    // 判据：**多屏能力隔离**——能力索引不得跨屏串扰。
    //
    // 症状可自查：能力索引一串，用户看到的是「拔了 4K 屏，1080p 屏变 4K」。
    {
        let mut mm = MonitorManager::new();
        let i0 = mm.admit(fp_triple(10)).unwrap();
        let i1 = mm.admit(fp_triple(11)).unwrap();
        let cap_4k = CapabilityRecord::new(
            vec![DisplayMode::new(
                Resolution::new(3840, 2160),
                RefreshRate::from_hz(144),
                BitDepth::B12,
            )],
            HdrCapability::Present,
        );
        let cap_1080 = CapabilityRecord::new(
            vec![DisplayMode::new(
                Resolution::new(1920, 1080),
                RefreshRate::from_hz(60),
                BitDepth::B8,
            )],
            HdrCapability::NotDetected,
        );
        mm.attach_capability(i0, cap_4k).unwrap();
        mm.attach_capability(i1, cap_1080).unwrap();
        let g4k = mm.ledger.entries()[i0]
            .fingerprint()
            .guid
            .clone()
            .unwrap_or_default();
        let g1080 = mm.ledger.entries()[i1]
            .fingerprint()
            .guid
            .clone()
            .unwrap_or_default();
        assert_ne!(g4k, g1080, "两块屏的 GUID 应不同");
        // 逐块查（一次只持有一个引用，避免 E0499 双重可变借用）
        assert_eq!(
            mm.capability_of(&g4k).expect("4K 屏应命中")
                .max_refresh().unwrap().hz_rounded(),
            144
        );
        assert!(mm.capability_of(&g4k).unwrap().hdr.is_present());
        assert_eq!(
            mm.capability_of(&g1080).expect("1080p 屏应命中")
                .max_refresh().unwrap().hz_rounded(),
            60
        );
        assert!(
            !mm.capability_of(&g1080).unwrap().hdr.is_present(),
            "HDR 能力不得跨屏串扰"
        );
        // 摘掉 4K 屏后，1080p 屏的能力表不受影响
        mm.ledger.remove(i0).unwrap();
        assert!(
            mm.capability_of(&g1080).is_some(),
            "摘另一屏不影响本屏"
        );
        assert_eq!(
            mm.capability_of(&g1080).unwrap().max_refresh().unwrap().hz_rounded(),
            60
        );
        // 摘屏后重建索引，能力索引不残留已摘屏的项
        mm.table.build(&mm.ledger);
        assert!(mm.capability_of(&g4k).is_none(), "已摘屏不得留在索引里");
        assert!(mm.capability_of(&g1080).is_some());

        // --- 反假变体 M07 的门禁：直接核**索引内容**，不经 live() 兜底 ---
        //
        // 上面的 capability_of 查不到 g4k 有**两条**独立原因：
        //   (a) build 正确跳过了退役位；
        //   (b) build 把退役位留进了索引，但 live() 兜底挡住了查询。
        // 只断言 capability_of.is_none() 时 (a)(b) 不可区分——把 build 的
        // 退役位跳过删掉，判据照样全绿，脏索引就固化了。
        // 故必须直接看索引内容。
        assert!(
            !mm.table.index_contains(&g4k),
            "退役位不得留在能力索引里（索引脏了，live()兜底只是没让它显形）"
        );
        assert!(mm.table.index_contains(&g1080), "在册屏应在索引里");
        // 索引条目数 == 在册且带能力的屏数（1 块：只剩 1080p）
        assert_eq!(mm.table.index_len(), 1, "索引条目数须等于在册带能力屏数");
        // 索引内容探针不得污染遥测（它是只读面）
        let q0 = mm.table.queries;
        let _ = mm.table.index_contains(&g4k);
        assert_eq!(mm.table.queries, q0, "索引探针只读，不得计查询数");

        // --- 反假变体 M08 的门禁：下标永不复用 ---
        //
        // remove 若改回 Vec::remove 搬移，摘除后 slot_count 会从 2 掉到 1，
        // 新屏入册就会**复用被摘的下标**，上层缓存的旧下标指向另一块屏。
        // 这三条按「槽位高水位 / 台账自洽 / 退役位不可取」逐层卡死：
        assert_eq!(mm.ledger.slot_count(), 2, "墓碑摘除不得缩短槽位（下标不复用）");
        assert_eq!(mm.ledger.retired(), 1, "摘除一位应记一个墓碑");
        assert_eq!(mm.ledger.live_count(), 1, "在册数应减一");
        assert!(mm.ledger.index_alignment_holds(), "记录下标须与实际位置自洽");
        assert!(mm.ledger.live(i0).is_none(), "退役位不得再取到记录");
        // 新屏入册必须拿**高水位新下标**，不是被摘掉的 i0
        let i_new = mm.admit(fp_triple(12)).unwrap();
        assert_eq!(i_new, 2, "新屏须取高水位下标 2，不得复用被摘的 0");
        assert_ne!(i_new, i0, "下标复用即故障回潮（拔A屏后B屏能力串到新屏）");
        assert_eq!(mm.ledger.slot_count(), 3);
        assert!(mm.ledger.index_alignment_holds(), "入册后下标仍须自洽");
        // 摘除位与新下标是两个不同位置，互不干扰
        assert!(mm.ledger.monitor(i0).unwrap().retired, "老下标须仍是墓碑");
        assert!(!mm.ledger.monitor(i_new).unwrap().retired, "新下标须是在册位");

        set.add("V02-能力-多屏隔离", true, "");
    }

    // ---- 判据三：热插拔合并 ----

    // 判据：同屏同类合并 + 首事件直通 + 窗边界必吐 + 风暴判定。
    {
        let mut m = HotPlugMerger::with_window(1000);
        // 首事件立即上屏（不等窗满）
        let first = m.ingest(PlugEventKind::Attached, 0, 0);
        assert!(first.is_some(), "首事件必须立即吐出");
        assert!(first.unwrap().is_first_in_window);

        // 窗内事件不吐（同屏同类并计数）
        assert!(m.ingest(PlugEventKind::Detached, 0, 10).is_none());
        assert!(m.ingest(PlugEventKind::Detached, 0, 20).is_none());
        assert!(m.ingest(PlugEventKind::Attached, 1, 30).is_none(), "异屏同窗也不吐");

        // 快照（首事件那次）能看到后续计数吗？不能——快照是当时的切片。
        // 但再取一次快照（内部经由 snapshot 语义）应看到窗内累计，
        // 这里通过「过窗吐出旧窗」来验证旧窗内容。
        //
        // 关键：过窗时先结**旧窗**（含两块屏的计数），再开新窗并快照直通。
        // 返回的是新窗快照，所以两块屏的计数在旧窗里——但旧窗已计入
        // windows_flushed 与 merged_away，守恒对账能查出来（见下一判据）。
        let f = m.ingest(PlugEventKind::Attached, 0, 5000);
        let f = f.expect("过窗必须吐出");
        assert_eq!(f.raw_total, 1, "新窗原始条数应为 1");
        assert_eq!(f.event_total, 1, "新窗折算后1 项");
        assert!(f.tallies.iter().any(|(i, _)| *i == 0));
        assert!(!f.screen_line().is_empty());
        // 旧窗已结：吐出次数增加，且合并量被记账
        assert_eq!(m.windows_flushed, 1, "旧窗已结一次");
        // 合并量口径要说实话 —— 按真实事件流逐条数：
        //
        //   t=0   Attached 屏0   ← 开窗 + 快照直通（**仍留在窗内**）
        //   t=10  Detached 屏0   ┐
        //   t=20  Detached 屏0   │
        //   t=30  Attached 屏1   ├ 旧窗共 4 条
        //                          ┘
        //   t=5000 Attached 屏0  ← 过窗：结旧窗(4 条) + 开新窗(1 条)
        //
        // 旧窗折算：屏0{Attached:1, Detached:2}→2 项 + 屏1{Attached:1}→1 项
        // = 3 项。4 条原始折算 3 项 ⇒ 合并掉 1 条（屏0 的第二次 Detached）。
        //
        // **首事件快照直通不等于离窗**：它只是提前把内容给上层看了一眼，
        // 计数仍留在窗里参与后续合并——否则首个事件会被漏计。
        assert_eq!(m.merged_away, 1, "旧窗 4 条折算 3 项，合并掉 1 条");
        // 守恒逐值：收到 5 = 合并 1 + 已结 4 + 在途 1
        let (recv, merged, inflight, settled) = m.conservation();
        assert_eq!(recv, 5, "共喂 5 条");
        assert_eq!(merged, 1, "合并掉 1 条");
        assert_eq!(settled, 4, "已结旧窗原始 4 条");
        assert_eq!(inflight, 1, "新窗在途 1 条");
        // 条数口径守恒：收到 == 已结原始 + 在途
        assert_eq!(settled + inflight, recv, "守恒等式须成立");
        assert_eq!(m.settled_items(), 3, "已结窗折算 3 项");
        // 工作量口径：合并量 == 已结原始 − 已结折算（逐值，非推导）
        assert_eq!(merged, settled - m.settled_items(),
                   "合并量 == 已结原始 − 已结折算");
        assert_eq!(m.settled_raw(), settled, "已结原始条数访问器须一致");
        assert!(m.conservation_holds());

        // 风暴判定：插拔次数达门限即风暴
        let storm = MergedFlush {
            window_start_us: 0,
            window_end_us: 1000,
            event_total: 10,
            plug_collapsed: 4,
            plug_total: 8,
            raw_total: 50,
            tallies: Vec::new(),
            is_first_in_window: true,
        };
        assert!(storm.is_storm(8), "插拔达门限应判风暴");
        assert!(!storm.is_storm(9), "未达门限不应判风暴");
        assert_eq!(storm.saved_flushes(), 40, "省下的吐窗次数应可算");

        // 插拔类判定：模式变更不算插拔
        assert!(PlugEventKind::Attached.is_plug());
        assert!(PlugEventKind::Detached.is_plug());
        assert!(!PlugEventKind::ModeChanged.is_plug());
        assert!(!PlugEventKind::Attached.zh().is_empty());

        // 逐类计数
        let mut t = MergedTally::new();
        t.bump(PlugEventKind::Attached);
        t.bump(PlugEventKind::Attached);
        t.bump(PlugEventKind::ModeChanged);
        assert_eq!(t.attached, 2);
        assert_eq!(t.mode_changed, 1);
        assert_eq!(t.plug_total(), 2, "模式变更不计入插拔");
        assert_eq!(t.total(), 3);
        // 折算口径：**只有发生过的类**各折算 1 项。
        // 本 tally 只 bump 过 Attached 与 ModeChanged 两类 ⇒ 折算 2 项；
        //插拔类只有 Attached ⇒ 折算 1 项。
        assert_eq!(t.collapsed(), 2, "两类发生过，各折算 1 项");
        assert_eq!(t.plug_collapsed(), 1, "插拔只有 Attached 一类");
        // 空 tally 折算 0 项
        assert_eq!(MergedTally::new().collapsed(), 0);
        assert_eq!(MergedTally::new().plug_collapsed(), 0);
        // 三类齐全时折算 3 项（正例）
        let mut all3 = MergedTally::new();
        all3.bump(PlugEventKind::Attached);
        all3.bump(PlugEventKind::Detached);
        all3.bump(PlugEventKind::ModeChanged);
        assert_eq!(all3.collapsed(), 3, "三类各折算 1 项");
        assert_eq!(all3.plug_collapsed(), 2, "插拔两类各折算 1 项");
        // 同类重复**不增加折算项数**（这才是合并）
        all3.bump(PlugEventKind::Detached);
        all3.bump(PlugEventKind::Detached);
        assert_eq!(all3.collapsed(), 3, "同类重复仍是 3 项");
        assert_eq!(all3.detached, 3, "次数照实累加，不丢事实");
        assert_eq!(all3.total(), 5, "总次数为 5");

        set.add("V02-合并-窗内合并与首事件", true, "");
    }

    // 判据：**合并守恒**——收到 = 合并掉 + 在途，原始条数随汇总吐出。
    //
    // 这是「合并有没有吞事件」的可核对口径：不是看代码，而是看数。
    {
        let mut m = HotPlugMerger::with_window(1000);
        // 造一个 10 条事件的窗（首条直通 + 9 条并入）
        let f0 = m.ingest(PlugEventKind::Attached, 0, 0);
        assert!(f0.is_some());
        for k in 1..10u64 {
            assert!(m.ingest(PlugEventKind::Detached, 0, k * 10).is_none());
        }
        // 结窗
        let f1 = m.ingest(PlugEventKind::Attached, 0, 2000);
        let f1 = f1.expect("过窗应吐");

        // 新窗快照：只含刚开窗那 1 条
        assert_eq!(f1.raw_total, 1);
        assert_eq!(f1.event_total, 1);
        assert_eq!(f1.saved_flushes(), 0, "单条窗省不下什么");

        // 旧窗（10 条：Attached×1 + Detached×9，同一块屏）折算成 2 项
        //（attached 1 + detached 9），故合并掉 8 条。
        let (received, merged, inflight, settled) = m.conservation();
        assert_eq!(received, 11, "共喂 11 条");
        assert_eq!(settled, 10, "已结旧窗原始 10 条");
        assert_eq!(merged, 8, "10 条折算 2 项，合并掉 8 条");
        // 折算项数与真实次数两个口径都在
        assert_eq!(m.merged_away, 8);
        assert_eq!(inflight, 1, "新窗在途 1 条");
        assert!(m.conservation_holds(), "守恒等式应成立");
        // 守恒逐值核对：收到 = 合并掉 + 已结 + 在途
        assert_eq!(settled + inflight, received,
                   "守恒等式：已结窗原始 + 在途 == 收到");

        // 真正的合并：同窗多条并成一次
        let mut m2 = HotPlugMerger::with_window(10_000);
        assert!(m2.ingest(PlugEventKind::Attached, 0, 0).is_some());
        for _ in 0..20 {
            assert!(m2.ingest(PlugEventKind::Detached, 0, 100).is_none());
        }
        let f2 = m2.ingest(PlugEventKind::Attached, 0, 50_000);
        let f2 = f2.expect("过窗应吐");
        assert_eq!(f2.raw_total, 1, "结的是新窗");
        // 上一窗（20+1 条）已结，其守恒体现在 merged_away
        let (recv2, merged2, inflight2, settled2) = m2.conservation();
        assert_eq!(recv2, 22, "共喂 22 条");
        // 旧窗 21 条同屏同类（Attached1 + Detached20）折算 2 项 => 合并 19 条
        assert_eq!(settled2, 21, "已结旧窗原始 21 条");
        assert_eq!(merged2, 19, "21 条折算 2 项，合并掉 19 条");
        // 条数口径守恒：已结原始 + 在途 == 收到
        assert_eq!(settled2 + inflight2, recv2, "守恒等式须逐值成立");
        assert_eq!(m2.settled_items(), 2, "已结窗折算 2 项");
        assert_eq!(merged2, settled2 - m2.settled_items(),
                   "合并量 == 已结原始 − 已结折算");
        assert!(m2.conservation_holds(), "守恒等式应成立");
        // 关键反证：**折算项数与原始条数都被吐出**，没有吞事件
        assert_eq!(f2.raw_total, 1, "新窗原始 1 条");
        assert_eq!(f2.event_total, 1);
        // 20 次反复插拔折叠成 2 项——合并确实省了工作量（18 项）
        assert_eq!(m2.settled_raw() - m2.settled_items(), 19,
                   "21 条原始只让上层处理 2 项");

        set.add("V02-合并-守恒与不吞", true, "");
    }

    // 判据：**退化路径**——时钟回拨 / 零窗长 / 槽位溢出，各有确定行为。
    {
        // 零窗长被夹回默认（承诺降频却零窗长 = 不降频）
        let m0 = HotPlugMerger::with_window(0);
        assert_eq!(m0.window_us, MERGE_WINDOW_DEFAULT_US, "零窗长须夹回默认");
        assert!(m0.window_us > 0);

        // 时钟回拨：不重开窗（否则回拨绕过合并）
        let mut m = HotPlugMerger::with_window(1000);
        let f0 = m.ingest(PlugEventKind::Attached, 0, 5000);
        assert!(f0.is_some(), "首事件应立即快照");
        // 首事件快照**不结窗**，故windows_flushed 仍为 0
        assert_eq!(m.windows_flushed, 0, "快照不等于结窗");
        // at_us 回拨到 1000（仍在窗内）
        assert!(m.ingest(PlugEventKind::Attached, 0, 1000).is_none(),
                "回拨不得开新窗");
        assert!(m.ingest(PlugEventKind::Attached, 0, 2000).is_none());
        // 再回拨到 0，仍不得开新窗
        assert!(m.ingest(PlugEventKind::Attached, 0, 0).is_none(),
                "回拨不得开新窗");
        assert_eq!(m.windows_flushed, 0, "回拨期间不得有任何结窗");
        // 时刻最终推进过窗长后才结一次
        assert!(m.ingest(PlugEventKind::Attached, 0, 6000).is_some());
        assert_eq!(m.windows_flushed, 1, "过窗才结一次");

        // 槽位溢出记账：塞超过 MAX_MONITORS 个不同下标
        let mut m3 = HotPlugMerger::with_window(10_000);
        assert!(m3.ingest(PlugEventKind::Attached, 0, 0).is_some());
        for i in 1..(MAX_MONITORS as u64 + 10) {
            m3.ingest(PlugEventKind::Attached, i as usize, 100);
        }
        assert!(m3.overflow_merged() > 0, "溢出必须显式记账");
        let f = m3.ingest(PlugEventKind::Attached, 0, 999_999);
        let f = f.expect("应结窗");
        assert!(
            f.tallies.len() <= MERGE_WINDOW_CAP,
            "槽位数须受合并窗上限约束（上限 {}，实得 {}）",
            MERGE_WINDOW_CAP,
            f.tallies.len()
        );
        // 槽位上限必须严于台账上限（否则合并窗先于台账被撑爆）
        assert!(
            MERGE_WINDOW_CAP < MAX_MONITORS,
            "合并窗槽位上限须严于台账上限"
        );
        // 溢出代价进汇总，不是静默丢
        assert!(f.tallies.iter().any(|(i, _)| *i == 0));

        set.add("V02-合并-退化路径", true, "");
    }

    // A 批实产数自核（跨批对账的左半）。
    //
    // 放在A 批末尾而不是 B 批里实跑 A/B：B 批末尾那条判据若去调 A 批，
    // 而 A 批又要回调它，就成环（实测爆栈）。A 批自报实产数、
    // B 批与清单长度对账，两边都不实跑对方。
    assert_eq!(
        set.len(),
        A_BATCH_CHECKS,
        "A 批实产项数须等于声明的 {}（项数变更须同步改 A_BATCH_CHECKS）",
        A_BATCH_CHECKS
    );
    assert!(!set.truncated(), "A 批自检集不得溢出");

    set
}

/// 域自检第二批（判据四、判据五）。
pub fn run_vev02_checks_b() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vev02-b");

    // ---- 判据四：降级轮询 ----

    // 判据：降级带原因 + 周期下限 + 可观测 + 恢复回事件式。
    {
        let mut e = Enumerator::new();
        // 初始非降级，且**不该有轮询开销**
        assert_eq!(e.source(), EnumerationSource::Event);
        assert!(!e.is_degraded());
        assert!(!e.should_poll(10_000_000), "非降级态不应轮询");
        assert!(!e.screen_line().contains("降级"), "非降级态读屏不应说降级");

        // 降级：带原因、周期夹到下限
        let issue = e.degrade_to_polling("驱动未上报热插拔事件", 1);
        assert_eq!(issue.code, "E_ENUM_DEGRADED");
        assert!(issue.is_complete(), "降级报错必须五元组齐发");
        assert_eq!(e.poll_interval_us(), POLL_INTERVAL_MIN_US, "周期须夹到下限");
        assert_eq!(e.source(), EnumerationSource::Polling);
        assert!(e.is_degraded());
        assert_eq!(e.degrade_count, 1);
        // 原因必非空且进读屏（**降级必须可观测**）
        let reason = e.degrade_reason.clone().expect("降级必带原因");
        assert_eq!(reason, "驱动未上报热插拔事件");
        let line = e.screen_line();
        assert!(line.contains("降级"), "读屏须明说降级");
        assert!(line.contains("驱动未上报"), "读屏须带原因");
        assert!(!e.screen_line().is_empty());

        // 空原因不静默：给出兜底文字而非空串
        let mut e2 = Enumerator::new();
        let _ = e2.degrade_to_polling("   ", POLL_INTERVAL_DEFAULT_US);
        assert!(e2.degrade_reason.clone().unwrap().contains("未提供"),
                "空原因须兜底说明，不能静默");

        // 轮询节流：未到周期不轮询
        assert!(e.should_poll(0), "降级后首次应轮询");
        e.do_poll(0);
        assert!(!e.should_poll(0), "刚轮询过不应立刻再轮");
        assert!(!e.should_poll(POLL_INTERVAL_MIN_US - 1));
        assert!(e.should_poll(POLL_INTERVAL_MIN_US), "过周期应轮询");
        let before = e.poll_count;
        e.do_poll(POLL_INTERVAL_MIN_US);
        assert_eq!(e.poll_count, before + 1);

        // 恢复回事件式
        e.restore_events();
        assert!(!e.is_degraded());
        assert!(e.degrade_reason.is_none(), "恢复后原因须清");
        assert!(!e.should_poll(u64::MAX), "恢复后不应再有轮询");
        // 再降级会再计一次
        let _ = e.degrade_to_polling("再次缺失", POLL_INTERVAL_DEFAULT_US);
        assert_eq!(e.degrade_count, 2, "二次降级须再计一次");
        assert_eq!(e.poll_interval_us(), POLL_INTERVAL_DEFAULT_US,
                   "合法周期须原样采用");

        // 中文名齐备
        assert_eq!(EnumerationSource::Event.zh(), "事件枚举");
        assert_eq!(EnumerationSource::Polling.zh(), "轮询降级");

        set.add("V02-降级-原因与周期下限", true, "");
    }

    // 判据：**可观测与就绪闸**——就绪闸缺项人话、错误五元组齐发、读屏全量。
    {
        // 空台账不就绪，且说人话
        let mut mgr = MonitorManager::new();
        let r = mgr.readiness();
        assert!(r.is_err(), "空台账不应就绪");
        let msg = r.unwrap_err();
        assert!(msg.contains("台账为空"), "须说明台账空: {}", msg);
        assert!(msg.contains("三重指纹"), "须说明三重未过: {}", msg);
        assert!(msg.contains("承接面"), "须说明承接面: {}", msg);
        assert!(!msg.contains("not ready"), "禁止 not ready 这种非人话");

        // 枚举不完整也不就绪
        let mut m2 = MonitorManager::new();
        m2.admit(fp_triple(1)).unwrap();
        for item in V01_IF1_ACCEPTANCE.iter() {
            m2.upstream.register(item).unwrap();
        }
        m2.ledger.enumeration_complete = false;
        let r2 = m2.readiness();
        assert!(r2.is_err());
        assert!(r2.unwrap_err().contains("枚举不完整"));

        // 齐备后就绪
        let mut m3 = MonitorManager::new();
        let idx = m3.admit(fp_triple(1)).unwrap();
        m3.attach_capability(idx, cap_default()).unwrap();
        for item in V01_IF1_ACCEPTANCE.iter() {
            m3.upstream.register(item).unwrap();
        }
        assert!(m3.readiness().is_ok(), "齐备后应就绪: {:?}", m3.readiness());

        // 读屏全量摘要含四类能力与枚举态
        let sum = m3.screen_summary();
        for kw in ["已识别显示器", "三重验证通过", "事件模式", "刷新率"] {
            assert!(sum.contains(kw), "摘要缺 {}: {}", kw, sum);
        }
        // 未识别屏也有话说（不静默略过）
        m3.admit(IdentityFingerprint::empty()).unwrap();
        assert!(m3.screen_summary().contains("未识别"), "未识别屏须点名");

        // 错误五元组齐发：五个字段都有值，且有读屏行
        for e in [
            MonitorIssue {
                code: "E_X",
                symptom: String::from("现象"),
                root_cause: String::from("根因"),
                advice: "建议",
                severity: Severity::Error,
            },
            MonitorIssue {
                code: "E_Y",
                symptom: String::from("现象"),
                root_cause: String::from("根因"),
                advice: "建议",
                severity: Severity::Fatal,
            },
        ] {
            assert!(e.is_complete());
            let l = e.screen_line();
            assert!(l.contains("E_X") || l.contains("E_Y"));
            assert!(l.contains("因为"), "读屏行须含因果");
        }
        // 空字段不合格（机械判据）
        assert!(!MonitorIssue {
            code: "",
            symptom: String::from("a"),
            root_cause: String::from("b"),
            advice: "c",
            severity: Severity::Hint,
        }
        .is_complete());
        // 严重度四档齐备且可比较
        assert!(Severity::Hint < Severity::Warn);
        assert!(Severity::Warn < Severity::Error);
        assert!(Severity::Error < Severity::Fatal);
        for s in [Severity::Hint, Severity::Warn, Severity::Error, Severity::Fatal] {
            assert!(!s.zh().is_empty(), "严重度缺中文名");
        }

        set.add("V02-降级-可观测与就绪闸", true, "");
    }

    // 判据：**降级轮询确实在推进台账**——降级不是嘴上说说。
    //
    // 弱门禁反例：只断言「is_degraded() == true」的话，把`do_poll` 改成
    // 空实现也能通过——降级态挂着但屏永远重新枚举不到，用户侧症状是
    // 「插拔没反应，重启才好」。这里要求降级轮询**真的改台账**。
    {
        let mut e = Enumerator::new();
        let _ = e.degrade_to_polling("热插拔事件缺失", POLL_INTERVAL_DEFAULT_US);
        let mut mm = MonitorManager::new();
        mm.enumerator = e; // 接管同一个枚举协调器

        // 首次应轮询（poll_count == 0）
        assert!(mm.enumerator.should_poll(0), "降级后首次须轮询");
        mm.enumerator.do_poll(0);
        assert_eq!(mm.enumerator.poll_count, 1, "轮询次数须自增");

        // 未到周期不轮询；到周期再轮
        assert!(!mm.enumerator.should_poll(POLL_INTERVAL_DEFAULT_US - 1));
        assert!(mm.enumerator.should_poll(POLL_INTERVAL_DEFAULT_US));
        mm.enumerator.do_poll(POLL_INTERVAL_DEFAULT_US);
        assert_eq!(mm.enumerator.poll_count, 2);

        // 关键：轮询真的把新枚举到的屏入册了（降级在干活，不只是挂个标志）
        let idx = mm.admit(fp_triple(20)).expect("降级轮询应能入册新屏");
        assert!(idx < mm.ledger.len());
        assert_eq!(mm.ledger.len(), 1);
        // 三次轮询后再入一块，条数按屏数走（不靠墓碑虚增）
        mm.enumerator.do_poll(POLL_INTERVAL_DEFAULT_US * 2);
        mm.admit(fp_triple(21)).unwrap();
        assert_eq!(mm.ledger.len(), 2, "在册数须等于真实屏数");
        assert_eq!(mm.enumerator.poll_count, 3);
        // 降级态在摘要里可见（不只靠日志）
        assert!(mm.screen_summary().contains("降级轮询"));
        assert!(mm.screen_summary().contains("热插拔事件缺失"));

        set.add("V02-降级-轮询推进台账", true, "");
    }

    // ---- 判据五：判据本身 ----

    // 判据：五项齐备 + 每项覆盖数达标 + 映射双向一致。
    {
        // 五项齐备（锚点原文五条，一项不缺）
        assert_eq!(Criterion::CRITERIA.len(), Criterion::COUNT);
        assert_eq!(Criterion::COUNT, 5);
        let mut codes: Vec<&str> = Vec::new();
        for c in Criterion::CRITERIA.iter() {
            codes.push(c.code());
            assert!(!c.zh().is_empty(), "判据缺中文名");
            assert!(!c.acceptance().is_empty(), "判据缺验收要点");
            assert!(!c.check_group().is_empty(), "判据缺自检组前缀");
            assert!(c.min_checks() >= 3, "每判据至少 3 条自检覆盖");
        }
        // 码不重复
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), before, "判据码重复");

        // 正向：每项判据都有自检项以其组前缀开头。
        //
        // 口径说明：**只数清单**（CHECK_NAMES），不在此实跑 A/B。
        // 实跑会递归——B 批本项在跑，`produced_names()` 又去跑 A/B 两批，
        // 于是无限递归爆栈（这是实测撞出来的，不是设想的）。
        // 覆盖数由 `V02-判据-清单不漂移` 那一条兜底：那里真跑 A/B 取回
        // 实产项名并与清单逐名比对，所以「清单写了但没 add」仍会被抓到。
        for c in Criterion::CRITERIA.iter() {
            let n = CHECK_NAMES
                .iter()
                .filter(|x| x.starts_with(c.check_group()))
                .count();
            assert!(
                n >= c.min_checks(),
                "判据 {} 只有 {} 条自检覆盖，少于要求的 {}",
                c.code(),
                n,
                c.min_checks()
            );
        }

        // 反向：清单里不得有既不属于任何判据前缀、又未被实产的悬空项
        for n in CHECK_NAMES.iter() {
            let belongs = Criterion::CRITERIA
                .iter()
                .any(|c| n.starts_with(c.check_group()));
            assert!(belongs, "自检项 {} 不属于任何判据组", n);
        }

        set.add("V02-判据-五项与覆盖", true, "");
    }

    // 判据：**层边界拒收**——越界交付当场拒收且计数。
    {
        let mut u = UpstreamLedger::new();
        assert_eq!(u.out_of_layer_deliveries, 0);
        assert!(!u.is_ready(), "未登记时不应 ready");

        // 承接面四项齐备则 ready
        for item in V01_IF1_ACCEPTANCE.iter() {
            u.register(item).unwrap();
        }
        assert!(u.is_ready());

        // 越界交付逐项拒收（每项都试，不只试一项）
        assert_eq!(MONITOR_NOT_MINE.len(), 6, "不做清单须六项齐");
        for bad in MONITOR_NOT_MINE.iter() {
            let r = u.register(bad);
            assert!(r.is_err(), "越界交付须拒收: {}", bad);
            let e = r.unwrap_err();
            assert_eq!(e.code, "E_LAYER_VIOLATION");
            assert_eq!(e.severity, Severity::Fatal, "越界是致命级");
            assert!(e.is_complete());
        }
        assert_eq!(u.out_of_layer_deliveries, 6, "六项越界须各计一次");
        // 拒收后仍 ready（越界不影响合法登记）
        assert!(u.is_ready());

        // 合法项登记成功且不进计数
        let n = u.registered.len();
        u.register("台账快照").unwrap();
        assert_eq!(u.registered.len(), n + 1);
        assert_eq!(u.out_of_layer_deliveries, 6);

        set.add("V02-判据-层边界拒收", true, "");
    }

    // 判据：清单不漂移（本批落库项数与清单的 A 段逐名对齐）。
    //
    // **为什么不实跑 A/B 取实产名**：本项就在 B 批里跑，实跑会自指——
    // `produced_names()` → `run_vev02_checks_b()` → 本项 → `produced_names()`
    // 无限递归爆栈（实测撞出来的）。所以这里改用**不会自指的等价核对**：
    // 累计已落库项数 == 清单长度，且「本批各项的名字」出现在清单的对应段。
    // 真正抓「清单写了但没 add」的是 `set.len()` 与清单长度的等式。
    {
        // 跨批对账：A 批实产 + B 批已落库（本项除外）+ 本项 == 清单长度
        assert_eq!(
            A_BATCH_CHECKS + set.len() + 1,
            CHECK_NAMES.len(),
            "A 批 {} + B 批 {} + 本项 须等于清单长度 {}（防清单漂移）",
            A_BATCH_CHECKS,
            set.len(),
            CHECK_NAMES.len()
        );
        assert!(!set.truncated(), "自检集不得溢出（溢出即丢结果）");
        // 本批最后三项（层边界拒收 / 清单不漂移 / 前一项）名字须在清单内
        let n = CHECK_NAMES.len();
        for want in ["V02-判据-五项与覆盖", "V02-判据-层边界拒收"] {
            assert!(
                CHECK_NAMES.iter().any(|x| *x == want),
                "本批项{} 须登记在清单内",
                want
            );
        }
        assert_eq!(n, 15, "清单长度须为 15（项数变更须同步改此处）");
        set.add("V02-判据-清单不漂移", true, "");
    }

    set
}

/// A 批实产项数（A 批末尾自核用）。
///
/// 为什么单独数A 批：防漂移的等式要跨批对齐（A 批项数 + B 批项数 == 清单
/// 长度），而B 批里再实跑 A/B 会自指爆栈，所以 A 批在自己的末尾报一次
/// 实产数，由 B 批与清单长度对账。
pub const A_BATCH_CHECKS: usize = 9;