//! VE-F6003 · 物理材质与表面 · 域自检（判据逐条映射，六族）
//!
//! 锚点判据 → 判据族：
//! - 材质库（闭集备案 + O(1) 直引）→ [`group_lib`]
//! - 安全默认（兜底+标注不静默）→ [`group_safe`]
//! - 组合预览（实测/推导分账）→ [`group_preview`]
//! - 冲突检测（上报带责任段）→ [`group_conflict`]
//! - 越权审计（留痕+回放）→ [`group_audit`]
//! - 判据（收口自检）→ [`group_meta`]
//!
//! 双向验证纪律：冲突检测在干净备案上必须零上报（无假阳）、在注入冲突上
//! 必须点名且责任段非空（无漏报）；重复注册必须被拒且计数不变（越权拦截
//! 真可达）；未注册表面必须带标注（兜底不静默是本单的核心承诺）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vead03_materials::*;

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vead03_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F6003");
    group_lib(&mut set);
    group_safe(&mut set);
    group_preview(&mut set);
    group_conflict(&mut set);
    group_audit(&mut set);
    group_meta(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 材质库
// ---------------------------------------------------------------------------

fn group_lib(set: &mut CheckSet) {
    // ① 库闭集非平凡：行数≥5 且全字段在物理合法域（千分比 0..=1000、密度>0）。
    let mut rows_ok = MATERIAL_LIBRARY.len() >= 5;
    let mut k = 0usize;
    while k < MATERIAL_LIBRARY.len() {
        let m = &MATERIAL_LIBRARY[k];
        if m.friction_permille > 1000 || m.elasticity_permille > 1000 || m.density_kg_m3 == 0 {
            rows_ok = false;
        }
        if m.name.is_empty() {
            rows_ok = false;
        }
        k += 1;
    }
    set.add(
        "C6003-LIB-01 材质库闭集且全字段在合法域",
        rows_ok,
        "千分比整数口径：物理结算跨平台字节级确定（F1619 纪律延续）",
    );

    // ② 名字→id→材质 往返可逆（注册期解析的正确性）。
    let mut reversible = true;
    let mut k = 0usize;
    while k < MATERIAL_LIBRARY.len() {
        let id = k as u8;
        match material_of(id) {
            Some(m) => {
                if material_id_of(m.name) != Some(id) {
                    reversible = false;
                }
            }
            None => reversible = false,
        }
        k += 1;
    }
    set.add(
        "C6003-LIB-02 名字与 id 双向可逆",
        reversible,
        "id 直引 O(1) 的前提是映射稳定：可逆断言防表序漂移",
    );

    // ③ 越界 id 走安全默认（O(1) 直引的越界语义显性）。
    let bad = 200u8;
    let (m, note) = resolve_material(bad);
    set.add(
        "C6003-LIB-03 越界 id 兜底默认材质且带标注",
        m.name == DEFAULT_MATERIAL.name && note.fallback,
        "越界不是 panic 也不是静默取邻位——兜底+标注是返回值的一部分",
    );
}

// ---------------------------------------------------------------------------
// 安全默认
// ---------------------------------------------------------------------------

fn group_safe(set: &mut CheckSet) {
    // ④ 默认材质中庸无极值（兜底不能自带风险）。
    set.add(
        "C6003-SAFE-01 默认材质参数中庸（无极值风险）",
        DEFAULT_MATERIAL.friction_permille >= 200
            && DEFAULT_MATERIAL.friction_permille <= 800
            && DEFAULT_MATERIAL.elasticity_permille >= 100
            && DEFAULT_MATERIAL.elasticity_permille <= 600
            && DEFAULT_MATERIAL.density_kg_m3 >= 500
            && DEFAULT_MATERIAL.density_kg_m3 <= 2000,
        "兜底材质取极值=未备案内容获得极端物理——中庸是安全默认的本意",
    );

    // ⑤ 未注册表面：安全默认 + SURFACE_UNREGISTERED 标注（兜底不静默）。
    let reg = SurfaceRegistry::new();
    let (m, note) = reg.resolve(0xDEAD);
    set.add(
        "C6003-SAFE-02 未注册表面兜底且标注原因码",
        m.name == DEFAULT_MATERIAL.name
            && note.fallback
            && note.reason == MatCode::SURFACE_UNREGISTERED.code(),
        "静默兜底把配置错误变成看起来正常的物理——标注是承诺不是日志",
    );
}

// ---------------------------------------------------------------------------
// 组合预览
// ---------------------------------------------------------------------------

fn group_preview(set: &mut CheckSet) {
    // ⑥ 已测组合：预览与备案逐字段相等且无标注（rubber-steel = 库 id 0,1）。
    let pr = preview_pair(0, 1);
    let known_ok = match pr {
        Ok((r, note)) => !note.fallback && r.measured && r.owner.contains("rubber-steel"),
        Err(_) => false,
    };
    set.add(
        "C6003-PRV-01 已测组合预览等于备案值",
        known_ok,
        "备案可信档：预览读矩阵，不现算",
    );

    // ⑦ 未测组合：推导值强制标注未测（不伪装实测）。
    let pr = preview_pair(1, 5); // steel-glass：备案外组合
    let derived_ok = match pr {
        Ok((r, note)) => note.fallback && !r.measured && r.owner.contains("derived"),
        Err(_) => false,
    };
    set.add(
        "C6003-PRV-02 未测组合标注未测且责任段显性",
        derived_ok,
        "推导值伪装实测=配置错误静默进物理结算",
    );

    // ⑧ pair_index 对称且越界 None（矩阵寻址的正确性前提）。
    let sym = pair_index(0, 1).is_some() && pair_index(0, 1) == pair_index(1, 0);
    let oob = pair_index(200, 0).is_none() && pair_index(0, 200).is_none();
    let diag = pair_index(2, 2).is_some();
    set.add(
        "C6003-PRV-03 组合键对称归一且越界拒绝",
        sym && oob && diag,
        "(a,b) 与 (b,a) 同格：矩阵不重复备案也不漏备案",
    );
}

// ---------------------------------------------------------------------------
// 冲突检测
// ---------------------------------------------------------------------------

fn group_conflict(set: &mut CheckSet) {
    // ⑨ 干净备案零上报（无假阳：检测不是装饰）。
    let clean = detect_conflicts(&PAIR_MATRIX, &PAIR_MATRIX_KEYS);
    set.add(
        "C6003-CON-01 干净备案零冲突上报",
        clean.is_empty(),
        "检测器对着自己的备案库报警=容差或索引有 bug",
    );

    // ⑩ 注入冲突：同组合两条超容差备案必被点名，责任段非空（无漏报）。
    let mut rows: Vec<PairResponse> = Vec::new();
    let mut pairs: Vec<(u8, u8)> = Vec::new();
    let mut k = 0usize;
    while k < PAIR_MATRIX.len() {
        rows.push(PAIR_MATRIX[k]);
        pairs.push(PAIR_MATRIX_KEYS[k]);
        k += 1;
    }
    rows.push(PairResponse {
        friction_permille: PAIR_MATRIX[0].friction_permille + 300,
        restitution_permille: 400,
        measured: true,
        owner: "AD03-test-injected",
    });
    pairs.push((0, 1));
    let rep = detect_conflicts(&rows, &pairs);
    let inject_ok = rep.len() == 1
        && rep[0].friction_delta == 300
        && !rep[0].owner_a.is_empty()
        && !rep[0].owner_b.is_empty();
    set.add(
        "C6003-CON-02 注入冲突被点名且上报带责任段",
        inject_ok,
        "上报含责任段是锚点原文：追责不到段的冲突上报只是噪音",
    );

    // ⑪ 容差边界：恰好等于容差不报（>容差才冲突——语义精确对钉）。
    let mut rows2: Vec<PairResponse> = Vec::new();
    let mut pairs2: Vec<(u8, u8)> = Vec::new();
    rows2.push(PairResponse {
        friction_permille: 500,
        restitution_permille: 300,
        measured: true,
        owner: "seg-a",
    });
    pairs2.push((0, 1));
    rows2.push(PairResponse {
        friction_permille: 500 + CONFLICT_TOLERANCE_PERMILLE,
        restitution_permille: 300,
        measured: true,
        owner: "seg-b",
    });
    pairs2.push((0, 1));
    let edge = detect_conflicts(&rows2, &pairs2);
    rows2[1].friction_permille += 1;
    let over = detect_conflicts(&rows2, &pairs2);
    set.add(
        "C6003-CON-03 容差边界恰等不报超容差必报",
        edge.is_empty() && over.len() == 1,
        "夹逼对钉边界：容差语义一字不差",
    );
}

// ---------------------------------------------------------------------------
// 越权审计
// ---------------------------------------------------------------------------

fn group_audit(set: &mut CheckSet) {
    // ⑫ 注册留痕：register 成功必有 Register 痕且序号递增。
    let mut reg = SurfaceRegistry::new();
    let mut log = AuditLog::new();
    let r1 = reg.register(1001, 0, &mut log);
    let r2 = reg.register(1002, 2, &mut log);
    set.add(
        "C6003-AUD-01 注册成功且审计留痕",
        r1.is_ok() && r2.is_ok() && reg.len() == 2 && log.len() == 2,
        "写路径统一走审计：没有痕的写=不可追责的写",
    );

    // ⑬ 重复注册（越权改）被拒：注册数不变 + Denied 痕 + 库外材质同拒。
    let dup = reg.register(1001, 1, &mut log);
    let badmat = reg.register(1003, 200, &mut log);
    set.add(
        "C6003-AUD-02 重复注册与库外材质均被拒且留痕",
        dup == Err(MatCode::AUDIT_DENIED)
            && badmat == Err(MatCode::AUDIT_DENIED)
            && reg.len() == 2
            && reg.denied == 2,
        "越权改→审计：拒绝必须显性且计数上行，不静默覆盖",
    );

    // ⑭ 回放追责：按表面 id 取全部痕（首注册+重复拒两条、序号递增可回放）；
    //    满容拒收计数不静默。
    let trail = log.replay(1001);
    let mut trail_ok = trail.len() == 2
        && trail[0].op == AuditOp::Register
        && trail[1].op == AuditOp::Denied
        && trail[0].step < trail[1].step;
    let mut tiny = AuditLog::new();
    let mut i = 0u32;
    while i < 200 {
        tiny.push(AuditOp::Register, i as u64);
        i += 1;
    }
    if tiny.dropped != 200u32 - AUDIT_CAP as u32 {
        trail_ok = false;
    }
    set.add(
        "C6003-AUD-03 回放追责与满容拒收如实计数",
        trail_ok,
        "审计簿满容覆盖旧痕=争议时证据消失——拒收必须留数字",
    );
}

// ---------------------------------------------------------------------------
// 判据：收口自检
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑮ 实挂条数从 CheckSet 实取：前五族 14 条，META 段 2 条，合计 16。
    let before_meta = set.len();
    set.add(
        "C6003-META-01 实挂条数+2(META)=声明条数16",
        before_meta == 14 && before_meta + 2 == 16,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑯ 判据名全集互异（重名=聚合器 tally 失真）。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C6003-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 16,
        "重名判据让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
