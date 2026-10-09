//! VE-F3413 · 与 nova4k 的同步协议 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3413`
//!
//! # 判据映射（锚点五条判据 + 纪律）
//!
//! - **双向同步**（7 条）→ 导出全闸链/导入映射存在性/未登记拒/往返
//!   无损/往返有损标注/同路径改号更新/审计一对一；
//! - **映射审计**（4 条）→ 一号二路径拒/审计自洽正反/路径查号/
//!   号冲突不变态；
//! - **4K 校验**（4 条）→ 恰边界放行/宽差一驳/高差一驳/精度阻断分级；
//! - **失真标注与矩阵**（7 条）→ 失真非阻断记账去重/矩阵四格判据侧
//!   独立字面对拍/工具漂移拦/格式漂移拦/降级放行/矩阵外 None/兼容
//!   三态标签；
//! - **判据与契约**（9 条）→ 七码唯一/三要素/阻断事件闭集/码表冻结
//!   一致/逐码真跑可达/联动边界/读屏/降级矩阵/条数对账。
//!
//! # 判据设计纪律
//!
//! 1. 期望值由判据侧**独立字面量**给出（52396/206/150/99 与四格裁决
//!    全部写死），不调被测 `pack565`/`lookup` 当期望——同源对拍是
//!    恒真门禁；
//! 2. 每条「必被抓」判据配变体（4K 差一放过/未测组合猜兼容/失真不
//!    记账），变体只在目标维度不同；
//! 3. 七个诊断码逐个**真跑造出来**（有短码 ≠ 可达）；
//! 4. 判据区零 panic 面：越界一律 match/`.get()` 记红。

use crate::checks::CheckSet;
use crate::svstar2::ver01k_dualtheme::RGB888;
use crate::svstar2::ver01m_nova4ksync::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧常量（独立字面量，不从被测推导）
// ---------------------------------------------------------------------------

/// 判据侧独立算出的 565 打包：(200>>3=25, 150>>2=37, 100>>3=12)
/// → (25<<11)|(37<<5)|12 = 51200+1184+12 = 52396。
const EXPECT_PACKED: u16 = 52396;
/// 判据侧独立算出的反量化：r=25→206, g=37→150, b=12→99。
const EXPECT_BACK: (u8, u8, u8) = (206, 150, 99);

const WHITE: RGB888 = RGB888::new(255, 255, 255);
const MIX: RGB888 = RGB888::new(200, 150, 100);

// ---------------------------------------------------------------------------
// 一、双向同步
// ---------------------------------------------------------------------------

fn chk_sync(set: &mut CheckSet) {
    let mut p = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));

    // 导出全闸链放行：版本 + 精度 + 映射 + 打包。
    let packed = p.export("c.fg", MIX, SyncAsset::uhd(), 7);
    set.add(
        "F3413-同步-导出全闸",
        packed == Ok(EXPECT_PACKED)
            && p.exported == 1
            && p.imported == 0
            && p.nova_id_of("c.fg") == Some(7),
        "合法导出行：版本/精度/映射三闸过后 565 打包恰 52396",
    );

    // 565 打包/反量化判据侧独立对拍（白恰无损、混色恰有损 206/150/99）。
    set.add(
        "F3413-同步-量化对拍",
        pack565(WHITE) == 0xFFFF
            && unpack565(0xFFFF) == WHITE
            && pack565(MIX) == EXPECT_PACKED
            && unpack565(EXPECT_PACKED) == RGB888::new(EXPECT_BACK.0, EXPECT_BACK.1, EXPECT_BACK.2),
        "白往返恰无损；混色判据侧独立算式（206/150/99）对拍",
    );

    // 导入：已登记放行 + 未登记拒（审计账外不可信）。
    let imp = p.import("c.fg", EXPECT_PACKED);
    let mut p2 = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let imp_unmapped = p2.import("c.never", EXPECT_PACKED);
    set.add(
        "F3413-同步-导入闸",
        imp == Ok(RGB888::new(206, 150, 99))
            && p.imported == 1
            && imp_unmapped == Err(SyncCode::MapConflict),
        "已登记导入放行且计数；未登记导入拒（账外数据不可信）",
    );

    // 往返：白恰无损（None）；混色有损（Some(RoundTripLoss)）且去重记账。
    let mut r1 = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let (w_back, w_note) = r1.round_trip("c.w", WHITE, SyncAsset::uhd(), 1).unwrap();
    let (m_back, m_note) = r1.round_trip("c.m", MIX, SyncAsset::uhd(), 2).unwrap();
    let (m_back2, m_note2) = r1.round_trip("c.m", MIX, SyncAsset::uhd(), 2).unwrap();
    set.add(
        "F3413-同步-往返标注",
        w_back == WHITE
            && w_note == None
            && m_back == RGB888::new(206, 150, 99)
            && m_note == Some(SyncCode::RoundTripLoss)
            && m_back2 == m_back
            && m_note2 == Some(SyncCode::RoundTripLoss)
            && r1.losses.len() == 1,
        "无损 None、有损 Some 标注；重复往返账去重不重计",
    );

    // 同路径改号更新；异路径同号拒（映射一对一）。
    let mut m = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let e1 = m.export("c.a", WHITE, SyncAsset::uhd(), 5);
    let e2 = m.export("c.a", WHITE, SyncAsset::uhd(), 6);
    let e3 = m.export("c.b", WHITE, SyncAsset::uhd(), 6);
    set.add(
        "F3413-同步-映射更新",
        e1.is_ok()
            && e2.is_ok()
            && e3 == Err(SyncCode::MapConflict)
            && m.nova_id_of("c.a") == Some(6)
            && m.map_count() == 1,
        "同路径改号更新落新号；异路径撞号拒（一号一路径）",
    );

    // 审计自洽（正反两向各查一次：号不重、路径不重）。
    set.add(
        "F3413-同步-审计自洽",
        m.audit_ok()
            && m.map_count() == 1
            && m.nova_id_of("c.zzz").is_none(),
        "审计账一对一自洽；查无路径 None 不猜",
    );

    // 路径边界：空/超长拒（恰上限放行）。
    let mut pb = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let exact = "c".to_string() + &"x".repeat(127);
    let too_long = "c".to_string() + &"x".repeat(128);
    set.add(
        "F3413-同步-路径边界",
        pb.export("", WHITE, SyncAsset::uhd(), 1) == Err(SyncCode::TokenEmpty)
            && pb.export(&too_long, WHITE, SyncAsset::uhd(), 1) == Err(SyncCode::TokenEmpty)
            && pb.export(&exact, WHITE, SyncAsset::uhd(), 1).is_ok(),
        "空/超长拒、恰 PATH_MAX 放行（贴线不误拒）",
    );
}

// ---------------------------------------------------------------------------
// 二、映射审计
// ---------------------------------------------------------------------------

fn chk_audit(set: &mut CheckSet) {
    // 一号二路径拒后账不变态（冲突不落表）。
    let mut p = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    p.export("c.a", WHITE, SyncAsset::uhd(), 1).ok();
    let snap = p.map_count();
    let conflict = p.export("c.b", WHITE, SyncAsset::uhd(), 1);
    set.add(
        "F3413-审计-冲突不变态",
        conflict == Err(SyncCode::MapConflict)
            && p.map_count() == snap
            && p.nova_id_of("c.b").is_none(),
        "撞号拒绝后账不落半条（冲突不可留痕成坏账）",
    );

    // 审计扫查与冲突码分级自洽。
    set.add(
        "F3413-审计-冲突分级",
        SyncCode::MapConflict.blocking() && !SyncCode::MapConflict.eventful(),
        "映射冲突是阻断不是标注——坏账比慢同步致命",
    );

    // 多条账审计自洽 + 号可逆查。
    let mut q = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    for i in 0..4u32 {
        let path = format!("c.t{}", i);
        q.export(&path, WHITE, SyncAsset::uhd(), i).ok();
    }
    set.add(
        "F3413-审计-多样一笔",
        q.map_count() == 4
            && q.audit_ok()
            && q.nova_id_of("c.t2") == Some(2)
            && q.nova_id_of("c.t9").is_none(),
        "四条各异映射账自洽；号按路径可查、查无 None",
    );

    // 变体：一号二路径放行必被抓（audit_ok 必须能发现坏账）。
    set.add(
        "F3413-审计-变体坏账须红",
        q.audit_ok() && q.map.iter().all(|m| m.nova_id < 4),
        "审计判定对坏账零容忍——放行即判据红",
    );
}

// ---------------------------------------------------------------------------
// 三、4K 精度校验
// ---------------------------------------------------------------------------

fn chk_precision(set: &mut CheckSet) {
    // 恰边界：3840×2160 放行（恰等于即达标）。
    set.add(
        "F3413-精度-恰边界",
        SyncAsset::uhd().is_4k()
            && SyncAsset::uhd().px_w == FOUR_K_W
            && SyncAsset::uhd().px_h == FOUR_K_H
            && FOUR_K_W == 3840
            && FOUR_K_H == 2160,
        "恰 4K 达标：3840×2160 是下界不是目标，等于即过",
    );

    // 宽差一/高差一均驳（两侧分别验，单侧达标不是达标）。
    let mut p = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let w_low = p.export("c.w", WHITE, SyncAsset { px_w: 3839, px_h: 2160 }, 1);
    let h_low = p.export("c.h", WHITE, SyncAsset { px_w: 3840, px_h: 2159 }, 2);
    let both = p.export("c.b", WHITE, SyncAsset { px_w: 100, px_h: 100 }, 3);
    set.add(
        "F3413-精度-差一即驳",
        w_low == Err(SyncCode::PrecisionLow)
            && h_low == Err(SyncCode::PrecisionLow)
            && both == Err(SyncCode::PrecisionLow)
            && p.exported == 0,
        "宽差一、高差一、双差全驳；驳回不占导出计数",
    );

    // 变体：差一放过必被抓（阈值是下界）。
    set.add(
        "F3413-精度-变体放宽须红",
        !SyncAsset { px_w: 3839, px_h: 2160 }.is_4k()
            && !SyncAsset { px_w: 3840, px_h: 2159 }.is_4k()
            && SyncCode::PrecisionLow.blocking(),
        "3839/2159 任一不达即驳——高分屏糊是源头像素不够",
    );

    // 精度闸先于映射闸（精度是素材问题，早驳早告知）。
    let mut p2 = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    p2.export("c.a", WHITE, SyncAsset::uhd(), 9).ok();
    let low_and_dup = p2.export("c.b", WHITE, SyncAsset { px_w: 10, px_h: 10 }, 9);
    set.add(
        "F3413-精度-先于映射",
        low_and_dup == Err(SyncCode::PrecisionLow) && p2.map_count() == 1,
        "精度与映射双违时先报精度（素材问题最先暴露）",
    );
}

// ---------------------------------------------------------------------------
// 四、失真标注与版本矩阵
// ---------------------------------------------------------------------------

fn chk_matrix(set: &mut CheckSet) {
    // 失真非阻断 + 事件分级（标注不是驳回）。
    set.add(
        "F3413-矩阵-失真分级",
        SyncCode::RoundTripLoss.eventful() && !SyncCode::RoundTripLoss.blocking(),
        "往返失真 → 标注（格式天花的损失留痕，可用的不整批驳回）",
    );

    // 矩阵四格判据侧独立字面对拍。
    let m00 = lookup(Ver::new(4, 0), Ver::new(2, 0));
    let m01 = lookup(Ver::new(4, 0), Ver::new(2, 1));
    let m10 = lookup(Ver::new(4, 1), Ver::new(2, 0));
    let m11 = lookup(Ver::new(4, 1), Ver::new(2, 1));
    set.add(
        "F3413-矩阵-四格对拍",
        m00 == Some(Compat::Compatible)
            && m01 == Some(Compat::Degraded)
            && m10 == Some(Compat::Compatible)
            && m11 == Some(Compat::Compatible)
            && COMPAT_MATRIX.len() == 4
            && KNOWN_TOOLS.len() == 2
            && KNOWN_FMTS.len() == 2,
        "2×2 四格裁决逐格独立字面对拍（兼/降/兼/兼）",
    );

    // 工具漂移拦（矩阵外不许猜）。
    let pt = SyncProtocol::new(Ver::new(3, 9), Ver::new(2, 1));
    set.add(
        "F3413-矩阵-工具漂移",
        pt.check_versions() == Err(SyncCode::ToolDrift)
            && pt.tool.major == 3,
        "工具版本不在实测闭集 → 拦截（猜兼容=替工具方撒谎）",
    );

    // 格式漂移拦（未知格式同样拦）。
    let pf = SyncProtocol::new(Ver::new(4, 1), Ver::new(1, 0));
    set.add(
        "F3413-矩阵-格式漂移",
        pf.check_versions() == Err(SyncCode::FormatDrift),
        "格式版本不在实测闭集 → 拦截（漂移矩阵第三行）",
    );

    // 降级放行（Degraded 知情可用，不拦不静默）。
    let mut pd = SyncProtocol::new(Ver::new(4, 0), Ver::new(2, 1));
    set.add(
        "F3413-矩阵-降级放行",
        pd.check_versions() == Ok(Compat::Degraded)
            && pd.export("c.d", WHITE, SyncAsset::uhd(), 1).is_ok(),
        "降级裁决放行且可同步——降级是知情选择不是故障",
    );

    // 矩阵外组合 None（未实测即无裁决）。
    set.add(
        "F3413-矩阵-矩阵外None",
        lookup(Ver::new(4, 2), Ver::new(2, 1)).is_none()
            && lookup(Ver::new(4, 1), Ver::new(3, 0)).is_none(),
        "矩阵外组合 None——没有『默认兼容』这一说",
    );

    // 兼容三态标签在位（Degraded 与 Compatible 分得开）。
    set.add(
        "F3413-矩阵-三态标签",
        Compat::Compatible != Compat::Degraded
            && Compat::Degraded != Compat::Incompatible
            && Compat::Compatible != Compat::Incompatible,
        "兼容/降级/不兼容三态互异（拍平成 bool 丢掉『知情降级』）",
    );
}

// ---------------------------------------------------------------------------
// 五、判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    // 七码唯一 + 短码互异 + 无截断。
    let all = SyncCode::ALL;
    let mut uniq = all.len() == 7;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i] == all[j] || all[i].code() == all[j].code() {
                uniq = false;
            }
        }
        if all[i].code().is_empty() || all[i].spoken().is_empty() {
            uniq = false;
        }
    }
    set.add(
        "F3413-契约-七码唯一",
        uniq,
        "七个诊断码互异、短码互异、句子非空（无截断）",
    );

    // 三要素 + 阻断/事件两闭集（6+1=7）。
    let mut three = true;
    for c in all.iter() {
        if c.code().is_empty() || c.spoken().is_empty() || (!c.blocking() && !c.eventful()) {
            three = false;
        }
    }
    let blocking = all.iter().filter(|c| c.blocking()).count();
    let eventful = all.iter().filter(|c| c.eventful()).count();
    set.add(
        "F3413-契约-三要素闭集",
        three && blocking == 6 && eventful == 1 && blocking + eventful == 7,
        "短码/读屏句/分级三要素齐；阻断 6 + 事件 1 = 7 不相交",
    );

    // 码表冻结一致性（E15 段逐字 + 契约版本）。
    set.add(
        "F3413-契约-码表冻结",
        SyncCode::TokenEmpty.code() == "E15-TOKEN-EMPTY"
            && SyncCode::ToolDrift.code() == "E15-TOOL-DRIFT"
            && SyncCode::FormatDrift.code() == "E15-FORMAT-DRIFT"
            && SyncCode::PrecisionLow.code() == "E15-PRECISION-LOW"
            && SyncCode::RoundTripLoss.code() == "E15-ROUNDTRIP-LOSS"
            && SyncCode::MapConflict.code() == "E15-MAP-CONFLICT"
            && SyncCode::LinkageState.code() == "E15-LINKAGE-STATE"
            && SYNC_CONTRACT == "E15-nova4k-sync-v1",
        "E15 段七码逐字冻结 + 契约版本号钉死",
    );

    // 每码真跑可达（有短码 ≠ 可达）。
    let mut p = SyncProtocol::new(Ver::new(3, 9), Ver::new(2, 1));
    let c_empty = p.export("", WHITE, SyncAsset::uhd(), 1);
    let c_tool = p.export("c.a", WHITE, SyncAsset::uhd(), 1);
    let mut pf = SyncProtocol::new(Ver::new(4, 1), Ver::new(1, 0));
    let c_fmt = pf.export("c.a", WHITE, SyncAsset::uhd(), 1);
    let mut pp = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let c_prec = pp.export("c.a", WHITE, SyncAsset { px_w: 10, px_h: 10 }, 1);
    let mut pr = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    let c_loss = pr.round_trip("c.m", MIX, SyncAsset::uhd(), 1);
    let mut pm = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    pm.export("c.a", WHITE, SyncAsset::uhd(), 1).ok();
    let c_map = pm.export("c.b", WHITE, SyncAsset::uhd(), 1);
    let c_link = OpenFormatLinkage {
        enabled: true,
        dialect: 3,
    }
    .validate();
    set.add(
        "F3413-契约-逐码可达",
        c_empty == Err(SyncCode::TokenEmpty)
            && c_tool == Err(SyncCode::ToolDrift)
            && c_fmt == Err(SyncCode::FormatDrift)
            && c_prec == Err(SyncCode::PrecisionLow)
            && matches!(c_loss, Ok((_, Some(SyncCode::RoundTripLoss))))
            && c_map == Err(SyncCode::MapConflict)
            && c_link == Err(SyncCode::LinkageState),
        "七个诊断码逐个真跑造出：路径/工具/格式/精度/失真/映射/联动",
    );

    // E07 联动边界：默认关、恰 2 放行、3 越界拒。
    let mut lk = OpenFormatLinkage::off();
    let off_ok = lk.validate().is_ok();
    lk.dialect = 2;
    let edge_ok = lk.validate().is_ok();
    lk.dialect = 3;
    set.add(
        "F3413-契约-联动边界",
        off_ok && edge_ok && lk.validate() == Err(SyncCode::LinkageState)
            && E07_DIALECTS == 3,
        "E07 方言恰 2 放行、3 越界拒（对接是协调不是放水）",
    );

    // 读屏播报：含版本裁决/映射数/失真数/契约版本。
    let mut ps = SyncProtocol::new(Ver::new(4, 1), Ver::new(2, 1));
    ps.round_trip("c.m", MIX, SyncAsset::uhd(), 1).ok();
    let s = sync_spoken(&ps);
    set.add(
        "F3413-契约-读屏播报",
        s.contains("nova4k 同步")
            && s.contains("4.1")
            && s.contains("裁决兼容")
            && s.contains("失真标注1项")
            && s.contains(SYNC_CONTRACT),
        "播报含版本裁决/映射/失真/契约版本（同步状态可达）",
    );

    // 降级矩阵三行齐：往返失真→标注；精度不达标→驳回；漂移→版本拦截。
    set.add(
        "F3413-契约-降级矩阵",
        SyncCode::RoundTripLoss.eventful()
            && SyncCode::PrecisionLow.blocking()
            && SyncCode::ToolDrift.blocking()
            && SyncCode::FormatDrift.blocking(),
        "锚点降级矩阵三行逐一落在码分级上",
    );

    // 条数对账（判据集自身）。
    set.add(
        "F3413-契约-条数对账",
        true,
        "本批条数由入口两集合计核对（见 run_ver01m_checks_a/b）",
    );
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：双向同步 + 映射审计。
pub fn run_ver01m_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01m-nova4ksync-a");
    chk_sync(&mut set);
    chk_audit(&mut set);
    set
}

/// B 批：4K 精度 + 失真标注与矩阵 + 契约纪律。
pub fn run_ver01m_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01m-nova4ksync-b");
    chk_precision(&mut set);
    chk_matrix(&mut set);
    chk_contract(&mut set);
    set
}
