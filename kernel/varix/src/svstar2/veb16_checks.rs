//! VE-F0216 域自检（判据逐条映射锚点）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0216`
//!
//! 锚点判据原文：「支持范围、语义版本化、测试为准、公告机制、判据」——
//! 五条各成一组判据。
//!
//! 判据设计纪律：
//! 1. **不得用表内元素验表内函数**——查清单/统计这类必须用表外真实形态。
//! 2. **阈值不得同时充当预期值**——规格常量另用**锚点字面量**断言一次。
//! 3. **断言两侧在测试点上不得同值**——测试点要选「正确与错误实现
//!    结果不同」的那一点（如「恰好等于下限」而非「远超下限」）。
//! 4. **同源驱动恒真**——若被测两处由同一 bool 驱动，须绕过聚合层
//!    直接断言被测字段，或手工构造矛盾态。
//! 5. **声明常量须与实算对账**——`CLAUSE_CAPACITY` 等对外规模声明
//!    若无人监督，改成任意值全部判据仍绿（VE-F0215 亲历）。
//! 6. **判据区零 panic 面**——不写 `unwrap()`/`expect()`/`[0]`；
//!    取值一律 `match` 单次绑定（`None` 走显式兜底分支）。

use crate::checks::CheckSet;
use crate::svstar2::veb16_declaration::*;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格锚点字面量（**不引用实现常量**，否则改常量=改预期，自证）
// ---------------------------------------------------------------------------

/// 锚点「QEMU 6.0 以上」——字面量，不写 `MIN_QEMU.major`。
const ANCHOR_MIN_MAJOR: u32 = 6;
const ANCHOR_MIN_MINOR: u32 = 0;
/// 锚点「2D 与 virgl 与 Venus 三通路」——三通路名册条数。
const ANCHOR_PATHWAYS: usize = 3;
/// 锚点「`VIRTIO_F_VERSION_1` 必需」——特性位定值（virtio 规范）。
const ANCHOR_FEATURE_V1: u32 = 32;
/// 锚点「声明 QEMU 版本对照行」——版本比对 O(1) 的夹逼对三件。
const ANCHOR_QEMU_BELOW: (u32, u32, u32) = (5, 2, 9);
const ANCHOR_QEMU_EXACT_MIN: (u32, u32, u32) = (6, 0, 0);
const ANCHOR_QEMU_ABOVE: (u32, u32, u32) = (6, 0, 1);

// ---------------------------------------------------------------------------
// 一、支持范围（锚点「支持范围」）
// ---------------------------------------------------------------------------

fn c216_scope() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 最低版本对齐锚点字面量（三元组逐项比，不用 MIN_QEMU 自身）
    v.push((
        "C216-范围-最低QEMU版本对齐锚点",
        MIN_QEMU.major == ANCHOR_MIN_MAJOR
            && MIN_QEMU.minor == ANCHOR_MIN_MINOR
            && MIN_QEMU.patch == 0,
    ));

    // 夹逼对：低于下限 / 恰在下限 / 高于下限 —— 版本比对是 O(1) 且边界位置正确
    let below = Version::new(ANCHOR_QEMU_BELOW.0, ANCHOR_QEMU_BELOW.1, ANCHOR_QEMU_BELOW.2);
    let exact = Version::new(
        ANCHOR_QEMU_EXACT_MIN.0,
        ANCHOR_QEMU_EXACT_MIN.1,
        ANCHOR_QEMU_EXACT_MIN.2,
    );
    let above = Version::new(ANCHOR_QEMU_ABOVE.0, ANCHOR_QEMU_ABOVE.1, ANCHOR_QEMU_ABOVE.2);
    v.push((
        "C216-范围-版本夹逼对边界正确",
        !below.at_least_min(MIN_QEMU) && exact.at_least_min(MIN_QEMU)
            && above.at_least_min(MIN_QEMU),
    ));

    // 版本比对是整数序关系而非字符串序：6.10.0 > 6.9.0（字符串比会判反）
    let v610 = Version::new(6, 10, 0);
    let v69 = Version::new(6, 9, 0);
    v.push((
        "C216-范围-版本按整数序而非字典序",
        v610.at_least_min(v69) && v69 < v610,
    ));

    // 必需特性位对齐锚点定值
    v.push((
        "C216-范围-必需特性位对齐锚点",
        VIRTIO_F_VERSION_1 == ANCHOR_FEATURE_V1,
    ));

    // 特性位判定：缺 V1 即范围外（用「恰缺 V1」这一真实测试点）
    let d = declaration();
    let ok_req = Request {
        pathway: Pathway::TwoD,
        qemu: exact,
        features: VIRTIO_F_VERSION_1,
    };
    let no_v1 = Request {
        pathway: Pathway::TwoD,
        qemu: exact,
        features: VIRTIO_F_VERSION_1 ^ ANCHOR_FEATURE_V1,
    };
    let old_qemu = Request {
        pathway: Pathway::TwoD,
        qemu: below,
        features: VIRTIO_F_VERSION_1,
    };
    v.push((
        "C216-范围-特征齐全方准入",
        d.admits(&ok_req) && !d.admits(&no_v1) && !d.admits(&old_qemu),
    ));

    // 三通路全部在支持范围名册内（逐个真跑 admits，不靠数数）
    let mut all_paths_ok = true;
    let mut i = 0;
    while i < Pathway::ALL.len() {
        let r = Request {
            pathway: Pathway::ALL[i],
            qemu: exact,
            features: VIRTIO_F_VERSION_1,
        };
        if !d.admits(&r) {
            all_paths_ok = false;
        }
        i += 1;
    }
    v.push(("C216-范围-三通路逐一准入", all_paths_ok));

    // 宣告声明覆盖三通路（声明面与准入面双向对账）
    v.push((
        "C216-范围-宣告覆盖三通路",
        Pathway::ALL.len() == ANCHOR_PATHWAYS && pathways_declared() == ANCHOR_PATHWAYS,
    ));

    // 通路线编码自洽（枚举判别值不得充当 wire 码）
    v.push(("C216-范围-通路线编码自洽", wire_codes_self_consistent()));

    // wire 编码互异（两条通路同码 = 线上无法区分）
    let mut wire_distinct = true;
    let mut a = 0;
    while a < Pathway::ALL.len() {
        let mut b = a + 1;
        while b < Pathway::ALL.len() {
            if Pathway::ALL[a].wire() == Pathway::ALL[b].wire() {
                wire_distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    v.push(("C216-范围-通路线编码互异", wire_distinct));

    // 反向：未知 wire 码不得静默解析成某条通路（范围外不许模糊）
    v.push((
        "C216-范围-未知线码不解析为通路",
        Pathway::from_wire(0).is_none() && Pathway::from_wire(4).is_none(),
    ));

    v
}

// ---------------------------------------------------------------------------
// 二、语义版本化（锚点「语义版本化」）
// ---------------------------------------------------------------------------

fn c216_semver() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    v.push((
        "C216-版本-主版本号变更为破坏性",
        SemVer::new(2, 0, 0).is_breaking_from(SemVer::new(1, 9, 9))
            && !SemVer::new(1, 9, 9).is_breaking_from(SemVer::new(1, 0, 0)),
    ));

    // 升主版本必破坏；**降主版本**不得判破坏（语义版本化主版本单调）
    v.push((
        "C216-版本-主版本降级不判破坏",
        !SemVer::new(1, 0, 0).is_breaking_from(SemVer::new(2, 0, 0)),
    ));

    // 次版本/修订号变更不破坏（这正是 semver 的核心承诺）
    v.push((
        "C216-版本-次版本修订号不破坏",
        !SemVer::new(1, 1, 0).is_breaking_from(SemVer::new(1, 0, 0))
            && !SemVer::new(1, 0, 1).is_breaking_from(SemVer::new(1, 0, 0)),
    ));

    // 语义版本序关系（次版本递增为升序）
    v.push((
        "C216-版本-版本序单调",
        SemVer::new(1, 0, 0) < SemVer::new(1, 0, 1)
            && SemVer::new(1, 0, 1) < SemVer::new(1, 1, 0)
            && SemVer::new(1, 1, 0) < SemVer::new(2, 0, 0),
    ));

    // wire 编码单调且互异（跨版本比对不得撞码）
    let semvers = [
        SemVer::new(1, 0, 0),
        SemVer::new(1, 0, 1),
        SemVer::new(1, 1, 0),
        SemVer::new(2, 0, 0),
    ];
    let mut wire_distinct = true;
    let mut i = 0;
    while i < semvers.len() {
        let mut j = i + 1;
        while j < semvers.len() {
            if semvers[i].wire() == semvers[j].wire() {
                wire_distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push(("C216-版本-线码互异", wire_distinct));

    // 单调性：编码随版本升序（不是「互异就算」，是保序）
    v.push((
        "C216-版本-线码保序",
        semvers[0].wire() < semvers[1].wire()
            && semvers[1].wire() < semvers[2].wire()
            && semvers[2].wire() < semvers[3].wire(),
    ));

    // 宣告携带语义版本
    v.push((
        "C216-版本-宣告携带语义版本",
        declaration().semver == CURRENT_SEMVER,
    ));

    // 文本形式三段（仅输出用，但须渲染正确）
    v.push((
        "C216-版本-文本三段渲染正确",
        SemVer::new(2, 3, 4).text() == "2.3.4" && Version::new(6, 1, 2).text() == "6.1.2",
    ));

    v
}

// ---------------------------------------------------------------------------
// 三、测试为准（锚点「测试为准」）
// ---------------------------------------------------------------------------

fn c216_test_authority() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 默认证据缺失 → 阻断（**不默认假定成立**）
    let fresh = declaration();
    v.push((
        "C216-测试-新宣告默认阻断",
        fresh.verdict() == Verdict::Blocked && !fresh.blocking_gaps().is_empty(),
    ));

    // 全绿 → Declared
    let mut ok = declaration();
    let mut i = 0;
    while i < ok.len() {
        ok.set_evidence(CLAUSES[i].key, Evidence::Verified);
        i += 1;
    }
    v.push((
        "C216-测试-证据齐备判已宣告",
        ok.verdict() == Verdict::Declared && ok.blocking_gaps().is_empty(),
    ));

    // **阻断优先级高于修正**：既有失败项又有缺证据项 → 仍阻断
    // （这正是「阻断不许被修正盖过」的判据点）
    let mut mixed = declaration();
    if mixed.len() > 1 {
        mixed.set_evidence(CLAUSES[0].key, Evidence::Failed);
    }
    v.push((
        "C216-测试-缺证据优先于失败",
        mixed.verdict() == Verdict::Blocked,
    ));

    // 仅失败项、无缺证据 → 须以测试结果为准修正
    let mut only_fail = declaration();
    let mut j = 0;
    while j < only_fail.len() {
        only_fail.set_evidence(CLAUSES[j].key, Evidence::Verified);
        j += 1;
    }
    if only_fail.len() > 0 {
        only_fail.set_evidence(CLAUSES[0].key, Evidence::Failed);
    }
    v.push((
        "C216-测试-失败项判以测试为准修正",
        only_fail.verdict() == Verdict::Corrected,
    ));

    // 失败项必进阻断清单（缺证据清单只含缺证据，失败项不算「缺」）
    v.push((
        "C216-测试-失败项计入缺口清单",
        only_fail.blocking_gaps().len() == 1,
    ));

    // 证据按 key 定位，不按下标：打乱写入顺序不影响最终态
    let mut bykey = declaration();
    let mut k = bykey.len();
    while k > 0 {
        k -= 1;
        bykey.set_evidence(CLAUSES[k].key, Evidence::Verified);
    }
    v.push((
        "C216-测试-证据按名写入与序无关",
        bykey.verdict() == Verdict::Declared,
    ));

    // 未登记键写入不得凭空造出证据
    let mut unknown = declaration();
    unknown.set_evidence("no-such-key", Evidence::Verified);
    v.push((
        "C216-测试-未登记键不生证据",
        unknown.verdict() == Verdict::Blocked,
    ));

    // 查不存在的键 → Missing（不是 panic）
    v.push((
        "C216-测试-查不存在键返缺证",
        ok.evidence_of("no-such-key") == Evidence::Missing,
    ));

    v
}

// ---------------------------------------------------------------------------
// 四、公告机制（锚点「公告机制」）
// ---------------------------------------------------------------------------

fn c216_notice() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 破坏性变更：提前一版公告（1.0 公告 → 2.0 落地）为合规
    let mut good = ChangeLog::new();
    let recorded = good.record(Change {
        landed_in: SemVer::new(2, 0, 0),
        key: "drop-legacy-queue",
        breaking: true,
        announced_in: SemVer::new(1, 5, 0),
    });
    v.push(("C216-公告-变更入册", recorded && good.len() == 1));
    v.push((
        "C216-公告-破坏性提前一版合规",
        good.missing_notices().is_empty() && good.release_gate() == Verdict::Declared,
    ));

    // **同版公告+落地** = 追溯，锚点要「提前一版」→ 违规
    let mut same_ver = ChangeLog::new();
    same_ver.record(Change {
        landed_in: SemVer::new(2, 0, 0),
        key: "retroactive",
        breaking: true,
        announced_in: SemVer::new(2, 0, 0),
    });
    v.push((
        "C216-公告-同版公告落地判违规",
        same_ver.missing_notices().len() == 1,
    ));

    // 落地版本**早于**公告版本（逻辑倒置）→ 违规
    let mut inverted = ChangeLog::new();
    inverted.record(Change {
        landed_in: SemVer::new(1, 0, 0),
        key: "inverted",
        breaking: true,
        announced_in: SemVer::new(1, 5, 0),
    });
    v.push((
        "C216-公告-公告晚于落地判违规",
        inverted.missing_notices().len() == 1,
    ));

    // 公告缺失 → 阻断发布（锚点「公告缺失 → 阻断发布」）
    v.push((
        "C216-公告-缺失阻断发布",
        same_ver.release_gate() == Verdict::Blocked
            && Verdict::Blocked.blocks_release(),
    ));

    // 非破坏性变更不需要预告（否则每次修订都要公告，公告成噪声）
    let mut patch = ChangeLog::new();
    patch.record(Change {
        landed_in: SemVer::new(1, 0, 1),
        key: "typo",
        breaking: false,
        announced_in: SemVer::new(1, 0, 1),
    });
    v.push((
        "C216-公告-非破坏性免预告",
        patch.missing_notices().is_empty() && patch.release_gate() == Verdict::Declared,
    ));

    // 预告在册但尚未落地 → pending_notices 列出
    v.push((
        "C216-公告-待落地预告可列出",
        good.pending_notices().len() == 1 && good.get(0).map(|c| c.key) == Some("drop-legacy-queue"),
    ));

    // 合规预告**在册**（不论是否已落地）：1.0 公告 → 2.0 落地，走过流程
    let mut landed = ChangeLog::new();
    landed.record(Change {
        landed_in: SemVer::new(2, 0, 0),
        key: "done",
        breaking: true,
        announced_in: SemVer::new(1, 0, 0),
    });
    let announced_list = landed.announced_breaking();
    let announced_key = match announced_list.get(0) {
        Some(k) => *k,
        None => "",
    };
    v.push((
        "C216-公告-合规预告列入公告账",
        announced_list.len() == 1 && announced_key == "done",
    ));

    // 容量满拒绝而非静默丢弃（溢出不许无痕）
    let mut full = ChangeLog::new();
    let mut n = 0;
    let mut all_ok = true;
    while n < CHANGE_CAPACITY {
        if !full.record(Change {
            landed_in: SemVer::new(1, 0, 0),
            key: "x",
            breaking: false,
            announced_in: SemVer::new(1, 0, 0),
        }) {
            all_ok = false;
        }
        n += 1;
    }
    let overflow_rejected = !full.record(Change {
        landed_in: SemVer::new(1, 0, 0),
        key: "y",
        breaking: false,
        announced_in: SemVer::new(1, 0, 0),
    });
    v.push((
        "C216-公告-容量满拒收不丢弃",
        all_ok && overflow_rejected && full.len() == CHANGE_CAPACITY,
    ));

    // 取越界下标返 None（零 panic 面）
    v.push((
        "C216-公告-越界取项返空",
        full.get(full.len() + 1).is_none() && full.get(0).is_some(),
    ));

    v
}

// ---------------------------------------------------------------------------
// 五、明确拒绝 + 裁决（锚点「范围外请求 → 明确拒绝不模糊承诺」）
// ---------------------------------------------------------------------------

fn c216_refusal() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    let d = declaration();

    // 三个越界项各自给出**具体原因**（不是笼统 false）
    let bad_qemu = Request {
        pathway: Pathway::TwoD,
        qemu: Version::new(5, 2, 9),
        features: VIRTIO_F_VERSION_1,
    };
    let bad_feat = Request {
        pathway: Pathway::TwoD,
        qemu: Version::new(6, 0, 0),
        features: 0,
    };

    let r1 = d.admit(&bad_qemu);
    let r2 = d.admit(&bad_feat);
    // Venus 本身在声明通路内，故「通路不在名册」这一分支无法由公开
    // `Request` 触发（三通路都在册）——改验多因子组合：声明了 Venus
    // 的通路但设备 QEMU 版本过低，仍须明确拒在 `min-qemu` 上。
    let bad_combo = Request {
        pathway: Pathway::Venus,
        qemu: Version::new(5, 0, 0),
        features: VIRTIO_F_VERSION_1,
    };
    let r3 = d.admit(&bad_combo);

    // 三项**逐一**都要拿到非空的具体原因。
    // 初值必须为 `true`：本循环只负责「发现空原因就翻红」，
    // 若初值写成 `false` 则循环只会把它按在 false 上——判据恒红，
    // 属于自检分号陷阱（与 veb15 的 `let ok = ...;` 同类）。
    let mut reasons_specific = true;
    let mut i = 0;
    while i < 3 {
        let r = match i {
            0 => r1.err(),
            1 => r2.err(),
            _ => r3.err(),
        };
        match r {
            Some(refusal) => {
                if refusal.key.is_empty() || refusal.reason.is_empty() {
                    reasons_specific = false;
                }
            }
            None => reasons_specific = false,
        }
        i += 1;
    }
    v.push(("C216-拒绝-范围外逐项给具体原因", reasons_specific));

    // 拒绝键正确：版本不足 → min-qemu；缺特性 → virtio-f-version-1
    let key_qemu = match r1 {
        Ok(()) => "",
        Err(r) => r.key,
    };
    let key_feat = match r2 {
        Ok(()) => "",
        Err(r) => r.key,
    };
    v.push((
        "C216-拒绝-拒绝键定位到具体项",
        key_qemu == "min-qemu" && key_feat == "virtio-f-version-1",
    ));

    // **拒绝不等于失败**：被正确拒绝的请求不使宣告判坏
    v.push((
        "C216-拒绝-正确拒绝不判坏宣告",
        Verdict::Refused.is_refusal() && !Verdict::Refused.blocks_release(),
    ));

    // 准入与拒绝两面必须一致（不許 admits 说行、admit 说不行）
    let mut consistent = true;
    let mut j = 0;
    while j < 5 {
        let r = Request {
            pathway: Pathway::ALL[j % Pathway::ALL.len()],
            qemu: if j == 4 { Version::new(5, 0, 0) } else { Version::new(6, 0, 0) },
            features: if j == 3 { 0 } else { VIRTIO_F_VERSION_1 },
        };
        if d.admits(&r) != d.admit(&r).is_ok() {
            consistent = false;
        }
        j += 1;
    }
    v.push(("C216-拒绝-准入与拒绝两面一致", consistent));

    // 拒绝计数：范围外请求被拒数正确统计，且拒绝不影响发布裁决
    let reqs = [
        Request { pathway: Pathway::TwoD, qemu: Version::new(6, 0, 0), features: VIRTIO_F_VERSION_1 },
        bad_qemu,
        bad_feat,
    ];
    v.push((
        "C216-拒绝-范围外计数正确",
        count_refusals(&d, &reqs) == 2,
    ));

    // 全绿宣告 + 齐备公告 → 放行
    let mut ok = declaration();
    let mut k = 0;
    while k < ok.len() {
        ok.set_evidence(CLAUSES[k].key, Evidence::Verified);
        k += 1;
    }
    let mut log = ChangeLog::new();
    log.record(Change {
        landed_in: SemVer::new(2, 0, 0),
        key: "planned",
        breaking: true,
        announced_in: SemVer::new(1, 9, 0),
    });
    let r_ok = rule(&ok, &log);
    v.push((
        "C216-裁决-证据公告齐备放行",
        r_ok.verdict == Verdict::Declared && r_ok.blockers == 0,
    ));

    // 证据缺失 → 阻断，阻断项数 = 缺口数 + 缺公告数（两面合算）
    let fresh = declaration();
    let mut bad_log = ChangeLog::new();
    bad_log.record(Change {
        landed_in: SemVer::new(3, 0, 0),
        key: "unannounced",
        breaking: true,
        announced_in: SemVer::new(3, 0, 0),
    });
    let r_bad = rule(&fresh, &bad_log);
    v.push((
        "C216-裁决-两面缺项合算阻断",
        r_bad.verdict == Verdict::Blocked
            && r_bad.blockers == fresh.blocking_gaps().len() + bad_log.missing_notices().len()
            && r_bad.blockers > 1,
    ));

    v
}

// ---------------------------------------------------------------------------
// 六、结构自洽（对外规模声明与实算对账 + 条目唯一性）
// ---------------------------------------------------------------------------

fn c216_structure() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 声明容量须容得下实际条目（否则证据写入被静默截断——证据丢失
    // 会表现为「莫名阻断」，极难查）
    v.push((
        "C216-结构-声明容量容得下条目",
        CLAUSES.len() <= CLAUSE_CAPACITY,
    ));
    // 实算：把全部条目证据写满后无缺口（容量不足时此处会红）
    let mut full = declaration();
    let mut i = 0;
    while i < CLAUSES.len() {
        full.set_evidence(CLAUSES[i].key, Evidence::Verified);
        i += 1;
    }
    v.push((
        "C216-结构-全条写入后无缺口",
        full.blocking_gaps().is_empty() && full.verdict() == Verdict::Declared,
    ));

    // 条目键唯一（重名会让按名定位写错条目）
    let mut names_ok = true;
    let mut a = 0;
    while a < CLAUSES.len() {
        let mut b = a + 1;
        while b < CLAUSES.len() {
            if CLAUSES[a].key == CLAUSES[b].key {
                names_ok = false;
            }
            b += 1;
        }
        a += 1;
    }
    v.push(("C216-结构-条目键唯一", names_ok));

    // 两轴齐备：既有支持范围条目也有语义承诺条目
    let mut has_support = false;
    let mut has_semantic = false;
    let mut c = 0;
    while c < CLAUSES.len() {
        match CLAUSES[c].kind {
            ClauseKind::Support => has_support = true,
            ClauseKind::Semantic => has_semantic = true,
        }
        c += 1;
    }
    v.push(("C216-结构-两轴条目齐备", has_support && has_semantic));

    // 锚点「宣告文档含无障碍相关承诺项」——必须真有一条无障碍承诺
    let mut has_a11y = false;
    let mut d = 0;
    while d < CLAUSES.len() {
        if CLAUSES[d].kind == ClauseKind::Semantic && CLAUSES[d].pathways.is_empty() {
            has_a11y = true;
        }
        d += 1;
    }
    v.push(("C216-结构-含无障碍承诺项", has_a11y));

    // 条目数与支持/语义两段之和一致（三处登记须同源）
    v.push((
        "C216-结构-条目数与两段之和一致",
        CLAUSES.len() == SUPPORT_CLAUSES.len() + SEMANTIC_CLAUSES.len(),
    ));

    // 每条必需特性位非零时必须等于 V1（宣告不得凭空要求别的特性）
    let mut feat_ok = true;
    let mut e = 0;
    while e < CLAUSES.len() {
        let f = CLAUSES[e].required_feature;
        if f != 0 && f != VIRTIO_F_VERSION_1 {
            feat_ok = false;
        }
        e += 1;
    }
    v.push(("C216-结构-必需特性仅声明V1", feat_ok));

    v
}

/// 跑完 VE-F0216 全部判据。
pub fn run_veb16_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb16");
    for group in [
        c216_scope,
        c216_semver,
        c216_test_authority,
        c216_notice,
        c216_refusal,
        c216_structure,
    ] {
        for (name, passed) in group() {
            set.add(name, passed, "");
        }
    }
    set
}
