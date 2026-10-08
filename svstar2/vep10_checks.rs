//! VE-F3010 自检 · 共享元素转场（VE-P 域）
//!
//! **锚点判据逐条对应**（`#VE-F3010`「pair 声明、四维插值、内容不拉伸、
//! 飞行体协议、语义不飞、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | pair 声明 | `P10-配对-*`（声明入口双闸 + 配对表 O(log n) 有序二分 + 重复源拒） |
//! | 四维插值 | `P10-插值-*`（端点恒等/中点独立重算/单调/进度钳制/病态双向） |
//! | 内容不拉伸 | `P10-内容-*`（内容尺寸多帧恒等 + 居中独立重算 + 溢出不崩） |
//! | 飞行体协议 | `P10-飞行-*`（强制提升 + 快照复用账 + 丢失重建 + 海报降级） |
//! | 语义不飞 | `P10-语义-*`（隐藏恒真 + 语义归属投影 + reduce 直达显性） |
//! | 判据 | `P10-判据-*`（版本/错误码/退化矩阵/条数对账） |
//!
//! **判据设计硬规矩**（承 P 域先例）：期望值判据侧独立重算；不变量两头都测
//! （违规被拒 + 合规放行）；阈值/常量钉死具体数值；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::TokenTable;
use crate::svstar2::vep10_shared_element as se;

// ---------------------------------------------------------------------------
// 组一：pair 声明与配对表
// ---------------------------------------------------------------------------

fn chk_pair(s: &mut CheckSet) {
    // P10-配对-01：合法声明放行（CropCenter）。
    let ok = se::PairDecl::new(1, 2, se::ContentMap::CropCenter).is_ok();
    s.add("P10-配对-01", ok, "合法配对声明放行（crop-center）");

    // P10-配对-02：源与目标同 id 拒（自配对无过渡语义）。
    let ok = se::PairDecl::new(7, 7, se::ContentMap::CropCenter).is_err();
    s.add("P10-配对-02", ok, "自配对拒绝（入口闸一）");

    // P10-配对-03：Stretch 映射拒（内容不拉伸红线的声明入口）。
    let ok = se::PairDecl::new(1, 2, se::ContentMap::Stretch).is_err();
    s.add("P10-配对-03", ok, "Stretch 映射拒绝（入口闸二）");

    // P10-配对-04：映射闭集 wire 互异。
    let ok = se::ContentMap::CropCenter.wire() != se::ContentMap::Stretch.wire()
        && !se::ContentMap::CropCenter.wire().is_empty();
    s.add("P10-配对-04", ok, "映射短码互异且非空（线上可区分）");

    // P10-配对-05：配对表乱序登记仍保序（二分插入）。
    let mut t = se::PairTable::new();
    let b1 = t
        .declare(se::PairDecl::new(30, 40, se::ContentMap::CropCenter).unwrap_or(se::PairDecl {
            source: 30,
            target: 40,
            map: se::ContentMap::CropCenter,
        }))
        .is_ok();
    let b2 = t
        .declare(se::PairDecl::new(10, 20, se::ContentMap::CropCenter).unwrap_or(se::PairDecl {
            source: 10,
            target: 20,
            map: se::ContentMap::CropCenter,
        }))
        .is_ok();
    let b3 = t
        .declare(se::PairDecl::new(20, 50, se::ContentMap::CropCenter).unwrap_or(se::PairDecl {
            source: 20,
            target: 50,
            map: se::ContentMap::CropCenter,
        }))
        .is_ok();
    s.add(
        "P10-配对-05",
        b1 && b2 && b3 && t.is_sorted() && t.len() == 3,
        "乱序登记保序（源 id 严格递增）",
    );

    // P10-配对-06：find 命中且映射正确。
    let hit = t.find(20).map_or(false, |d| d.target == 50 && d.map == se::ContentMap::CropCenter);
    s.add("P10-配对-06", hit, "按源二分查配对命中");

    // P10-配对-07：find 未命中 None。
    s.add("P10-配对-07", t.find(99).is_none(), "未登记源查无配对");

    // P10-配对-08：重复源声明拒（一源多目标无合法语义）。
    let dup = t
        .declare(se::PairDecl::new(10, 60, se::ContentMap::CropCenter).unwrap_or(se::PairDecl {
            source: 10,
            target: 60,
            map: se::ContentMap::CropCenter,
        }))
        .is_err();
    s.add("P10-配对-08", dup && t.len() == 3, "重复源拒绝且表未被污染");
}

// ---------------------------------------------------------------------------
// 组二：四维插值（零浮点千分比定点）
// ---------------------------------------------------------------------------

fn chk_midframe(s: &mut CheckSet) {
    let src = se::Geo { x: 10, y: 20, w: 100, h: 50, radius: 8, crop: 4 };
    let dst = se::Geo { x: 110, y: 320, w: 400, h: 300, radius: 24, crop: 0 };

    // P10-插值-01：p=0 恒等于源（sanitize 后）。
    let g0 = se::midframe(&src, &dst, 0);
    s.add("P10-插值-01", g0 == src.sanitize(), "进度 0 = 源几何（端点恒等）");

    // P10-插值-02：p=1000 恒等于目标。
    let g1 = se::midframe(&src, &dst, 1000);
    s.add("P10-插值-02", g1 == dst.sanitize(), "进度 1000 = 目标几何（端点恒等）");

    // P10-插值-03：p=500 位置两维独立重算（判据侧公式复算）。
    let gm = se::midframe(&src, &dst, 500);
    let exp_x = (10 + 110) / 2;
    let exp_y = (20 + 320) / 2;
    s.add("P10-插值-03", gm.x == exp_x && gm.y == exp_y, "位置 x/y 中点独立对账");

    // P10-插值-04：尺寸两维独立重算。
    let exp_w = (100 + 400) / 2;
    let exp_h = (50 + 300) / 2;
    s.add("P10-插值-04", gm.w == exp_w && gm.h == exp_h, "尺寸 w/h 中点独立对账");

    // P10-插值-05：圆角/裁剪两维独立重算（四维全覆盖）。
    let exp_r = (8 + 24) / 2;
    s.add("P10-插值-05", gm.radius == exp_r && gm.crop == 2, "圆角/裁剪中点独立对账（四维齐）");

    // P10-插值-06：单调性（正向差维度：p=250 值 < p=750 值，逐维）。
    let ga = se::midframe(&src, &dst, 250);
    let gb = se::midframe(&src, &dst, 750);
    let mono = ga.x < gb.x && ga.y < gb.y && ga.w < gb.w && ga.h < gb.h && ga.radius < gb.radius;
    s.add("P10-插值-06", mono, "正向差维度插值单调递增");

    // P10-插值-07：进度超界钳制（p=-5≡0、p=2000≡1000——双向钳）。
    let g_lo = se::midframe(&src, &dst, -5);
    let g_hi = se::midframe(&src, &dst, 2000);
    s.add("P10-插值-07", g_lo == g0 && g_hi == g1, "进度超界钳回 [0,1000]（双向）");

    // P10-插值-08：负尺寸钳 0（病态输入不进插值）。
    let bad = se::Geo { x: 0, y: 0, w: -50, h: 30, radius: 0, crop: 0 };
    let sb = bad.sanitize();
    s.add("P10-插值-08", sb.w == 0 && sb.h == 30, "负尺寸钳 0（负值不外泄）");

    // P10-插值-09：圆角/裁剪超界钳 min(w,h)/2。
    let over = se::Geo { x: 0, y: 0, w: 40, h: 20, radius: 99, crop: 99 };
    let so = over.sanitize();
    s.add(
        "P10-插值-09",
        so.radius == 10 && so.crop == 10,
        "圆角/裁剪超界钳半边（超界收缩）",
    );

    // P10-插值-10：病态检出双向（零面积真 / 正常几何假）。
    let zero = se::Geo { x: 0, y: 0, w: -1, h: -1, radius: 0, crop: 0 };
    let ok = se::needs_early_takeover(&zero, &dst) && !se::needs_early_takeover(&src, &dst);
    s.add("P10-插值-10", ok, "病态检出双向（零面积真+正常假）");
}

// ---------------------------------------------------------------------------
// 组三：内容不拉伸
// ---------------------------------------------------------------------------

fn chk_content(s: &mut CheckSet) {
    // P10-内容-01：内容尺寸多帧恒等（容器变形不改内容尺寸——红线主断言）。
    let src = se::Geo { x: 0, y: 0, w: 100, h: 80, radius: 0, crop: 0 };
    let dst = se::Geo { x: 200, y: 400, w: 600, h: 400, radius: 12, crop: 0 };
    let mut ok = true;
    let mut p = 0;
    while p <= 1000 {
        let g = se::midframe(&src, &dst, p);
        let (_cx, _cy, cw, ch) = se::content_layout(&g, 240, 160);
        ok = ok && cw == 240 && ch == 160;
        p += 250;
    }
    s.add("P10-内容-01", ok, "内容宽高五帧采样恒等于声明值（不拉伸）");

    // P10-内容-02：居中公式独立重算（x = geo.x + (w - cw)/2）。
    let g = se::midframe(&src, &dst, 500);
    let (cx, cy, _cw, _ch) = se::content_layout(&g, 240, 160);
    let exp_x = g.x + (g.w - 240) / 2;
    let exp_y = g.y + (g.h - 160) / 2;
    s.add("P10-内容-02", cx == exp_x && cy == exp_y, "居中偏移判据侧复算一致");

    // P10-内容-03：容器小于内容时溢出居中不崩（偏移为负、尺寸仍恒等）。
    let tiny = se::Geo { x: 0, y: 0, w: 10, h: 10, radius: 0, crop: 0 };
    let (tx, _ty, tw, th) = se::content_layout(&tiny, 240, 160);
    let exp = 0 + (10 - 240) / 2;
    s.add("P10-内容-03", tx == exp && tw == 240 && th == 160, "溢出居中（负偏移+尺寸恒等）");

    // P10-内容-04：裁剪内缩不超过容器半边（sanitize 收口）。
    let g2 = se::Geo { x: 0, y: 0, w: 40, h: 20, radius: 5, crop: 500 }.sanitize();
    s.add("P10-内容-04", g2.crop <= g2.w.min(g2.h) / 2, "裁剪内缩受容器半边约束");

    // P10-内容-05：千分比 lerp 端点安全（0/1000 恒等两端，中途线性）。
    let l0 = se::lerp_i32(10, -10, 0) == 10;
    let l1 = se::lerp_i32(10, -10, 1000) == -10;
    let lm = se::lerp_i32(10, -10, 500) == 0;
    s.add("P10-内容-05", l0 && l1 && lm, "定点 lerp 端点恒等+中点对账");
}

// ---------------------------------------------------------------------------
// 组四：飞行体协议
// ---------------------------------------------------------------------------

fn chk_flight(s: &mut CheckSet) {
    let mut fm = se::FlightManager::new();

    // P10-飞行-01：起飞创建飞行体，强制提升声明钉位。
    let b1 = fm.launch(1, se::ContentType::Image).is_ok();
    let got = fm.get(1).map_or(false, |b| {
        b.boost == se::BOOST_FORCE_DECLARED && b.alive && b.layer.starts_with("flight-layer-")
    });
    s.add("P10-飞行-01", b1 && got, "起飞即独立图层+强制提升声明");

    // P10-飞行-02：F2850 契约位非空（前向声明钉死）。
    s.add(
        "P10-飞行-02",
        !se::BOOST_FORCE_DECLARED.is_empty() && se::BOOST_FORCE_DECLARED.starts_with("F2850"),
        "F2850 提升契约位钉死（落库前以常量钉位）",
    );

    // P10-飞行-03：重复起飞拒（双体竞争显性）。
    let dup = fm.launch(1, se::ContentType::Image).is_err();
    s.add("P10-飞行-03", dup && fm.alive_len() == 1, "同源重复起飞拒绝");

    // P10-飞行-04：快照复用计数增长（O(1) 纹理复用的账）。
    let r1 = fm.snapshot_reuse(1);
    let cnt = fm.get(1).map_or(0, |b| b.reuse_count);
    s.add("P10-飞行-04", r1 && cnt == 2, "快照复用计数 +1（纹理复用可观测）");

    // P10-飞行-05：丢失重建（逐出→新体存活→诊断计数）。
    let e1 = fm.note_ejected(1).is_ok();
    let rebuilt = fm.get(1).map_or(false, |b| b.alive);
    s.add(
        "P10-飞行-05",
        e1 && rebuilt && fm.lost_rebuilds == 1,
        "图层逐出→重建+诊断计数（降级矩阵第三路）",
    );

    // P10-飞行-06：丢失报告账实相符（无存活体时拒）。
    let bad = fm.note_ejected(99).is_err();
    s.add("P10-飞行-06", bad, "无飞行体的丢失报告拒绝（不虚构）");

    // P10-飞行-07：接管销毁幂等（竞态收敛）。
    fm.take_over(1);
    let after = fm.get(1).is_none();
    fm.take_over(1);
    s.add("P10-飞行-07", after && fm.alive_len() == 0, "接管销毁飞行体且重复幂等");

    // P10-飞行-08：快照模式选择（视频降级、其余纹理复用）。
    let ok = se::ContentType::Video.snapshot_mode() == se::SnapshotMode::PosterFrame
        && se::ContentType::Image.snapshot_mode() == se::SnapshotMode::Texture
        && se::ContentType::Vector.snapshot_mode() == se::SnapshotMode::Texture
        && se::ContentType::Text.snapshot_mode() == se::SnapshotMode::Texture;
    s.add("P10-飞行-08", ok, "快照模式闭集：视频=海报帧，其余=纹理复用");

    // P10-飞行-09：海报帧降级显性入账（不静默）。
    let v1 = fm.launch(2, se::ContentType::Video).is_ok();
    s.add(
        "P10-飞行-09",
        v1 && fm.poster_degrades == 1,
        "视频起飞入海报帧降级账（显性声明）",
    );
}

// ---------------------------------------------------------------------------
// 组五：语义不飞 + reduce 直达
// ---------------------------------------------------------------------------

fn chk_semantics_reduce(s: &mut CheckSet) {
    // P10-语义-01：飞行期读屏语义=目标语义（隐藏位同步断言）。
    let (label, hidden) = se::semantics_during_flight("详情大图：山脉照片");
    s.add(
        "P10-语义-01",
        label == "详情大图：山脉照片" && hidden,
        "飞行期读屏焦点=目标语义",
    );

    // P10-语义-02：飞行体对读屏恒隐藏（多输入采样恒真）。
    let mut ok = true;
    for t in ["a", "较长的一段目标语义描述", ""] {
        let (_l, h) = se::semantics_during_flight(t);
        ok = ok && h;
    }
    s.add("P10-语义-02", ok, "飞行体读屏隐藏恒真（语义不跟着飞红线）");

    // P10-语义-03：语义归属投影（配对期=源侧、飞行/接管=目标侧）。
    let decl = se::PairDecl::new(1, 2, se::ContentMap::CropCenter).unwrap_or(se::PairDecl {
        source: 1,
        target: 2,
        map: se::ContentMap::CropCenter,
    });
    let mut r = se::PairRecord::begin(decl, 0);
    let owner_paired = r.semantics_owner();
    let _ = r.launch(100);
    let owner_flying = r.semantics_owner();
    let _ = r.take_over(200);
    let owner_taken = r.semantics_owner();
    s.add(
        "P10-语义-03",
        owner_paired == se::PairState::Paired
            && owner_flying == se::PairState::TakenOver
            && owner_taken == se::PairState::TakenOver,
        "语义归属：飞行开始即随目标（不跟飞）",
    );

    // P10-语义-04：reduce 计划双零（零飞行帧+直达）。
    s.add("P10-语义-04", se::reduce_plan() == (0, true), "reduce 计划 (0,true) 钉死");

    // P10-语义-05：reduce 直达 Paired→TakenOver 且退化显性。
    let mut r2 = se::PairRecord::begin(decl, 0);
    let ok = r2.reduce_take_over(50).is_ok()
        && r2.state == se::PairState::TakenOver
        && r2.early_takeover
        && r2.degraded.is_some();
    s.add("P10-语义-05", ok, "reduce 直达接管且显性记退化（不静默省略）");

    // P10-语义-06：reduce 直达只覆盖 Paired（飞行中拒）。
    let mut r3 = se::PairRecord::begin(decl, 0);
    let _ = r3.launch(10);
    s.add("P10-语义-06", r3.reduce_take_over(20).is_err(), "飞行中 reduce 直达拒绝");
}

// ---------------------------------------------------------------------------
// 组六：生命周期状态机 + 判据元
// ---------------------------------------------------------------------------

fn chk_lifecycle_meta(s: &mut CheckSet) {
    let decl = se::PairDecl::new(3, 4, se::ContentMap::CropCenter).unwrap_or(se::PairDecl {
        source: 3,
        target: 4,
        map: se::ContentMap::CropCenter,
    });

    // P10-生命-01：Paired→Flying→TakenOver 全链合法，taken_ms 回填。
    let mut r = se::PairRecord::begin(decl, 0);
    let ok = r.launch(10).is_ok()
        && r.state == se::PairState::Flying
        && r.take_over(300).is_ok()
        && r.state == se::PairState::TakenOver
        && r.taken_ms == Some(300);
    s.add("P10-生命-01", ok, "三态全链合法且接管时刻回填");

    // P10-生命-02：Paired 直接接管拒（跳步）。
    let mut r2 = se::PairRecord::begin(decl, 0);
    s.add("P10-生命-02", r2.take_over(0).is_err(), "跳过飞行直接接管拒绝");

    // P10-生命-03：Flying 重复起飞拒 / TakenOver 再起飞拒（非法迁移闭包）。
    let mut r3 = se::PairRecord::begin(decl, 0);
    let _ = r3.launch(0);
    let _ = r3.take_over(10);
    let ok = r3.launch(20).is_err() && r3.take_over(30).is_err();
    s.add("P10-生命-03", ok, "接管后起飞/再接管均拒（非法迁移闭包）");

    // P10-生命-04：退化显性（Degraded 态 + 非空说明）。
    let mut r4 = se::PairRecord::begin(decl, 0);
    r4.degrade(se::Degradation::IndependentEntryExit);
    let ok = r4.state == se::PairState::Degraded
        && r4.degraded.map_or(false, |d| !d.note().is_empty());
    s.add("P10-生命-04", ok, "配对失败显性退化（不静默取消红线）");

    // P10-生命-05：飞行时长令牌单源（转查 dur-page）。
    let table = TokenTable::from_lang();
    let dur = se::flight_duration_ms(&table);
    s.add(
        "P10-生命-05",
        se::FLIGHT_DURATION_TOKEN == "dur-page" && dur.is_ok(),
        "飞行时长转查令牌表单源（不私设数值）",
    );

    // P10-生命-06：读屏替述含退化说明。
    let mut r5 = se::PairRecord::begin(decl, 0);
    r5.degrade(se::Degradation::EarlyTakeOver);
    let line = se::screen_line(&r5);
    let ok = line.contains("3→4") && line.contains("提前接管");
    s.add("P10-生命-06", ok, "读屏单行含配对与退化说明");

    // P10-判据-01：协议版本前缀。
    s.add(
        "P10-判据-01",
        se::SHARED_PROTOCOL_VERSION.starts_with("P10"),
        "协议版本 P10-*（跨版本对账锚）",
    );

    // P10-判据-02：错误码非空互异。
    s.add(
        "P10-判据-02",
        !se::E_PAIR_FAIL.is_empty()
            && !se::E_PAIR_GEO.is_empty()
            && !se::E_FLIGHT_LOST.is_empty()
            && !se::E_PAIR_PHASE.is_empty()
            && se::E_PAIR_FAIL != se::E_PAIR_GEO
            && se::E_FLIGHT_LOST != se::E_PAIR_PHASE
            && se::E_PAIR_GEO != se::E_PAIR_PHASE,
        "错误码非空互异（外部可观测分支）",
    );

    // P10-判据-03：退化矩阵四路 note 非空且 code 有归属。
    let all = [
        se::Degradation::IndependentEntryExit,
        se::Degradation::EarlyTakeOver,
        se::Degradation::FlightRebuild,
        se::Degradation::PosterSnapshot,
    ];
    let mut ok = true;
    for d in all.iter() {
        ok = ok && !d.note().is_empty() && !d.code().is_empty();
    }
    s.add("P10-判据-03", ok, "四路降级矩阵全显性（note+code 齐备）");

    // P10-判据-04：进度上限常量钉死 1000。
    s.add("P10-判据-04", se::PROGRESS_MAX == 1000, "千分比进度上限钉死（零浮点纪律）");

    // P10-判据-05：判据条数对账（本条前已有 48 条，本条为第 49 条）。
    s.add("P10-判据-05", s.len() == 48, "判据条数对账（声明 49）");
}

// ---------------------------------------------------------------------------
// 聚合（单集 46 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// F3010 域自检（聚合入口，注册表用）。
pub fn run_vep10_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F3010");
    chk_pair(&mut s);
    chk_midframe(&mut s);
    chk_content(&mut s);
    chk_flight(&mut s);
    chk_semantics_reduce(&mut s);
    chk_lifecycle_meta(&mut s);
    s
}
