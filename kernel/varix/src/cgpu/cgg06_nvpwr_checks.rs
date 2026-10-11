//! cgg06_nvpwr_checks — CGPU-F0966 NVIDIA 功耗画像·判据集（20 条）。
//!
//! 判据覆盖锚点五要素：双线（1~4）/ 约束画像（5~6）/ 曲线（7~12）/
//! 公开（13~15）/ 纪律与判据面（16~20）。全部走隔离探针与真仓两路验证。

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use crate::cgpu::cgg06_nvpwr::*;

/// 判据集入口（cgpu/mod.rs 折叠聚合器挂载点）。
pub fn run_cgg06_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0966");

    // 1 双线·挂载与查询：同芯片 TDP/TGP 两线各自挂载、各自查询命中。
    let ok1 = {
        let mut p = DualLineProfile::new("AD106");
        let d = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            fx_points(&[(100, 55_00), (220, 100_00)]),
            DataSource::VendorSpec,
        );
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (200, 100_00)]),
            DataSource::Measured,
        );
        match (d, l) {
            (Ok(dc), Ok(lc)) => {
                let m1 = p.mount(dc);
                let m2 = p.mount(lc);
                // 锚点直取（无插值算术风险）：台式 220W 锚点=100%、
                // 笔记本 60W 首锚=40%、笔记本 250W 超末锚钳制=100%。
                let q1 = p.query(PowerClass::DesktopTdp, 220);
                let q2 = p.query(PowerClass::LaptopTgp, 60);
                let q3 = p.query(PowerClass::LaptopTgp, 250);
                m1.is_ok()
                    && m2.is_ok()
                    && match (q1, q2, q3) {
                        (Ok((pd, _, _)), Ok((pl, _, _)), Ok((pc, _, _))) => {
                            pd == 100_00 && pl == 40_00 && pc == 100_00
                        }
                        _ => false,
                    }
            }
            _ => false,
        }
    };
    s.add("dual_mount_query", ok1, "dual line mount+query");

    // 2 双线·错线拒收：曲线芯片名与档案不一致即拒（XC_PWR_CLASS_MISMATCH）。
    let ok2 = {
        let mut p = DualLineProfile::new("AD107");
        let wc = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            120,
            fx_points(&[(50, 30_00), (120, 90_00)]),
            DataSource::VendorSpec,
        );
        match wc {
            Ok(c) => p.mount(c) == Err(XC_PWR_CLASS_MISMATCH),
            Err(_) => false,
        }
    };
    s.add("dual_wrong_line_reject", ok2, "wrong chip rejected");

    // 3 双线·墙序违反：TGP 标称高于台式 TDP 拒；目录 TGP 越上限拒；
    //   反序修复（TGP ≤ TDP）放行。
    let ok3 = {
        // A：台式 100W + 笔记本 140W → 140 > 100 违墙序。
        let mut p = DualLineProfile::new("AD106");
        let d = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            100,
            fx_points(&[(50, 40_00), (100, 70_00)]),
            DataSource::VendorSpec,
        );
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        let a = match (d, l) {
            (Ok(dc), Ok(lc)) => {
                let r1 = p.mount(dc);
                let r2 = p.mount(lc);
                r1.is_ok() && r2 == Err(XC_PWR_WALL_ORDER)
            }
            _ => false,
        };
        // B：目录越界——AD106 TGP 上限 140，挂 175 即拒。
        let lb = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            175,
            fx_points(&[(80, 50_00), (175, 95_00)]),
            DataSource::VendorSpec,
        );
        let b = match lb {
            Ok(c) => p.mount(c) == Err(XC_PWR_WALL_ORDER),
            Err(_) => false,
        };
        // C：修复——台式 220W + 笔记本 140W（≤TDP 且落目录）双挂放行。
        let d2 = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            fx_points(&[(100, 55_00), (220, 100_00)]),
            DataSource::VendorSpec,
        );
        let l2 = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        let c = match (d2, l2) {
            (Ok(dc), Ok(lc)) => p.mount(dc).is_ok() && p.mount(lc).is_ok(),
            _ => false,
        };
        a && b && c
    };
    s.add("dual_wall_order_reject", ok3, "wall order+catalog gate");

    // 4 目录闭集：八芯片全查 + 纯笔记本语义 + 目录外 None + TGP 归档六档
    //   恒等 + 闭集外 None + tgp_in_range 边界。
    let ok4 = {
        let mut acc = CHIP_CATALOG.len() == 8;
        let mut i = 0;
        while i < CHIP_CATALOG.len() {
            match catalog_find(CHIP_CATALOG[i].code) {
                Some(sp) => acc = acc && sp.code == CHIP_CATALOG[i].code,
                None => acc = false,
            }
            i += 1;
        }
        let gn = catalog_find("GN22");
        acc = acc
            && match gn {
                Some(sp) => sp.laptop_only() && sp.desktop_tdp == 0,
                None => false,
            };
        acc = acc && catalog_find("ZZ99").is_none();
        let mut ti = 0;
        while ti < TgpTier::ALL.len() {
            let t = TgpTier::ALL[ti];
            acc = acc
                && match TgpTier::tier_of_watts(t.watts()) {
                    Some(g) => g.watts() == t.watts(),
                    None => false,
                };
            ti += 1;
        }
        acc = acc && TgpTier::tier_of_watts(10).is_none() && TgpTier::tier_of_watts(500).is_none();
        // AD106 目录 TGP 35..=140：边界含端点。
        let ad = catalog_find("AD106");
        acc = acc
            && match ad {
                Some(sp) => sp.tgp_in_range(35) && sp.tgp_in_range(140) && !sp.tgp_in_range(34) && !sp.tgp_in_range(141),
                None => false,
            };
        acc
    };
    s.add("catalog_closed", ok4, "8-chip catalog+tier closed set");

    // 5 约束画像·墙损失量化：140W 墙下 85% 损失 15%；插值中点介于两锚间。
    let ok5 = {
        let mut p = DualLineProfile::new("AD106");
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (90, 62_00), (140, 85_00), (200, 100_00)]),
            DataSource::Measured,
        );
        match l {
            Ok(lc) => {
                let m = p.mount(lc);
                let c = p.laptop_constraint();
                let mid = p.query(PowerClass::LaptopTgp, 115);
                m.is_ok()
                    && match (c, mid) {
                        (Ok((w, pf, loss)), Ok((mp, _, _))) => {
                            w == 140 && pf == 85_00 && loss == 15_00 && mp > 62_00 && mp < 85_00
                        }
                        _ => false,
                    }
            }
            Err(_) => false,
        }
    };
    s.add("constraint_wall_loss", ok5, "wall loss quantified");

    // 6 约束画像·三区判定：UnderWall/AtWall/DeepClamp 三态互异且可复现。
    let ok6 = {
        let mut p = DualLineProfile::new("AD106");
        // TGP 线：墙 140W 损失 15% → DeepClamp。
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00), (200, 100_00)]),
            DataSource::Measured,
        );
        // TDP 线：墙 220W 饱和 → AtWall（损失 0）。
        let d = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            fx_points(&[(100, 55_00), (220, 100_00)]),
            DataSource::VendorSpec,
        );
        match (l, d) {
            (Ok(lc), Ok(dc)) => {
                let m1 = p.mount(lc);
                let m2 = p.mount(dc);
                let z1 = p.zone_at(PowerClass::LaptopTgp, 100); // 预算 < 墙 → UnderWall
                let z2 = p.zone_at(PowerClass::DesktopTdp, 220); // 饱和墙 → AtWall
                let z3 = p.zone_at(PowerClass::LaptopTgp, 140); // 损失 15% → DeepClamp
                let r1 = p.constraint_report(PowerClass::LaptopTgp, 140);
                m1.is_ok()
                    && m2.is_ok()
                    && z1 == Ok(WallZone::UnderWall)
                    && z2 == Ok(WallZone::AtWall)
                    && z3 == Ok(WallZone::DeepClamp)
                    && match r1 {
                        Ok(rep) => {
                            rep.watts == 140
                                && rep.perf_pct == 85_00
                                && rep.loss_pct == 15_00
                                && rep.zone == WallZone::DeepClamp
                                && rep.eff_ppm == 85_00 * 100 / 140
                                && !rep.degrade_signal // 15% < 20% 阈值
                        }
                        Err(_) => false,
                    }
            }
            _ => false,
        }
    };
    s.add("constraint_zones", ok6, "3-zone verdict+full report");

    // 7 曲线·未排序拒收：瓦特降序即 XC_PWR_CURVE_UNSORTED。
    let ok7 = {
        let bad = fx_points(&[(100, 55_00), (80, 40_00)]);
        PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, bad, DataSource::VendorSpec)
            == Err(XC_PWR_CURVE_UNSORTED)
    };
    s.add("curve_unsorted_reject", ok7, "unsorted watts rejected");

    // 8 曲线·物理约束三关：性能倒退拒 + 空锚点集拒 + 持平平台段放行。
    let ok8 = {
        let bad = fx_points(&[(100, 80_00), (150, 75_00)]);
        let r1 = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 220, bad, DataSource::VendorSpec);
        let r2 = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            Vec::new(),
            DataSource::VendorSpec,
        );
        let flat = fx_points(&[(100, 80_00), (150, 80_00), (200, 90_00)]);
        let r3 = PowerCurve::build(PowerClass::DesktopTdp, "AD106", 200, flat, DataSource::VendorSpec);
        r1 == Err(XC_PWR_CURVE_REGRESS) && r2 == Err(XC_PWR_NO_DATA) && r3.is_ok()
    };
    s.add("curve_regress_reject", ok8, "regress+empty+flat rules");

    // 9 曲线·端点钳制：低于首锚取首值、高于末锚取末值、锚点处还原。
    let ok9 = {
        let c = matrix_curve();
        c.perf_at(50) == 40_00
            && c.perf_at(500) == 95_00
            && c.perf_at(RULE_BASE_W) == 40_00
            && c.perf_at(210) == 95_00
    };
    s.add("curve_clamp", ok9, "endpoint clamped");

    // 10 曲线·双写对拍（全域双写纪律第 5 单）：CURVE_MATRIX 表 ↔ curve_rule
    //     规则推导（索引公式）↔ perf_at（扫描插值）三向 31 采样点全等。
    let ok10 = {
        let c = matrix_curve();
        let mut acc = true;
        let mut w = RULE_BASE_W;
        let top = CURVE_MATRIX[CURVE_MATRIX.len() - 1].0;
        while w <= top {
            let a = c.perf_at(w);
            let b = curve_rule(w);
            if a != b {
                acc = false;
                break;
            }
            w += 5;
        }
        // 矩阵锚点逐格还原（表即正本）。
        let mut i = 0;
        while i < CURVE_MATRIX.len() {
            if curve_rule(CURVE_MATRIX[i].0) != CURVE_MATRIX[i].1 {
                acc = false;
                break;
            }
            i += 1;
        }
        acc
    };
    s.add("curve_doublewrite", ok10, "matrix vs rule vs scan 31 pts");

    // 11 效率度量：峰值效率锚点定位（62_00@90 → 6888 ppm）+ 递减段起点
    //     180W + 平局取低瓦特。
    let ok11 = {
        let c = matrix_curve();
        let (w, e) = c.peak_efficiency();
        let d = c.diminishing_start();
        let tie = fx_points(&[(80, 50_00), (160, 100_00)]);
        let c2 = PowerCurve::build(PowerClass::DesktopTdp, "AD104", 160, tie, DataSource::VendorSpec);
        let w2 = match c2 {
            Ok(cc) => cc.peak_efficiency().0,
            Err(_) => 0,
        };
        w == 90 && e == 62_00 * 100 / 90 && d == 180 && w2 == 80
    };
    s.add("efficiency_metrics", ok11, "peak eff+diminishing+tie-low");

    // 12 逆向反查：端点/段内取整/封顶不可达/超满性能拒/零目标五例。
    let ok12 = {
        let c = matrix_curve();
        let w0 = c.watts_for_perf(0);
        let w1 = c.watts_for_perf(40_00);
        let w2 = c.watts_for_perf(80_00);
        let r1 = c.watts_for_perf(96_00);
        let r2 = c.watts_for_perf(PERF_FULL);
        let r3 = c.watts_for_perf(PERF_FULL + 1);
        w0 == Ok(60)
            && w1 == Ok(60)
            && w2 == Ok(135)
            && r1 == Err(XC_PWR_PERF_UNREACHABLE)
            && r2 == Err(XC_PWR_PERF_UNREACHABLE)
            && r3 == Err(XC_PWR_PERF_UNREACHABLE)
    };
    s.add("inverse_query", ok12, "watts_for_perf 5 cases");

    // 13 公开·白名单：七字段全出现 + 双墙数字如实 + 白名单外字段名零出现。
    let ok13 = {
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let d = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            fx_points(&[(100, 55_00), (220, 100_00)]),
            DataSource::VendorSpec,
        );
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        match (d, l) {
            (Ok(dc), Ok(lc)) => {
                let m1 = p.mount(dc);
                let m2 = p.mount(lc);
                let r = reg.register(p);
                let json = reg.export_profile("AD106");
                let mut acc = m1.is_ok() && m2.is_ok() && r.is_ok();
                if let Ok(j) = json {
                    let mut fi = 0;
                    while fi < PROFILE_FIELDS.len() {
                        acc = acc && contains_str(&j, PROFILE_FIELDS[fi]);
                        fi += 1;
                    }
                    acc = acc
                        && contains_str(&j, "\"tdp_watts\":220")
                        && contains_str(&j, "\"tgp_watts\":140")
                        && !contains_str(&j, "\"owner")
                        && !contains_str(&j, "\"user_");
                } else {
                    acc = false;
                }
                acc
            }
            _ => false,
        }
    };
    s.add("public_whitelist", ok13, "7-field whitelist export");

    // 14 公开·密封：指纹入账非零 + 重验一致 + 未密封显性无账 + 篡改检出。
    let ok14 = {
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        match l {
            Ok(lc) => {
                let m = p.mount(lc);
                let r = reg.register(p);
                let seal = reg.seal_profile("AD106");
                let mut acc = m.is_ok() && r.is_ok();
                if let Ok((_, s0)) = seal {
                    acc = acc && s0 != 0;
                    // 重验一致。
                    acc = acc && reg.verify_seal("AD106").is_ok();
                    // 未密封芯片显性无账（在册但未 seal）。
                    let mut p2 = DualLineProfile::new("AD104");
                    let d = PowerCurve::build(
                        PowerClass::DesktopTdp,
                        "AD104",
                        220,
                        fx_points(&[(100, 55_00), (220, 100_00)]),
                        DataSource::VendorSpec,
                    );
                    if let Ok(dc) = d {
                        let _ = p2.mount(dc);
                        let _ = reg.register(p2);
                        acc = acc && reg.verify_seal("AD104") == Err(XC_PWR_NO_DATA);
                    }
                    // 篡改检出：账面指纹翻转 → 失配。
                    let mut i = 0;
                    while i < reg.seal_map_len() {
                        if reg.seal_chip_at(i) == "AD106" {
                            reg.seal_set_at(i, s0 ^ 1);
                            break;
                        }
                        i += 1;
                    }
                    acc = acc && reg.verify_seal("AD106") == Err(XC_PWR_SEAL_MISMATCH);
                } else {
                    acc = false;
                }
                acc
            }
            Err(_) => false,
        }
    };
    s.add("public_seal", ok14, "FNV seal+tamper detect");

    // 15 诚实空态：未知芯片导出显性 Unknown + 计数如实 + 缺档位查询空态码。
    let ok15 = {
        let mut reg = NvPowerRegistry::new();
        let j = reg.export_profile("GB203");
        let mut acc = match j {
            Ok(js) => contains_str(&js, "Unknown"),
            Err(_) => false,
        } && reg.unknown_fallbacks == 1;
        let mut p = DualLineProfile::new("AD106");
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        if let Ok(lc) = l {
            let _ = p.mount(lc);
            acc = acc && p.query(PowerClass::DesktopTdp, 220) == Err(XC_PWR_NO_DATA);
        } else {
            acc = false;
        }
        acc
    };
    s.add("honest_unknown", ok15, "explicit unknown, no interpolation");

    // 16 两权分立：Runtime 调 register/mount/seal 三口全拒留痕，export 放行。
    let ok16 = {
        let mut reg = NvPowerRegistry::new();
        reg.set_role(RegistryRole::Runtime);
        // ① register 拒。
        let r1 = reg.register(DualLineProfile::new("AD106"));
        // ② mount_path 拒（先备一条合法曲线）。
        let cv = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        let r2 = match cv {
            Ok(c) => reg.mount_path("AD106", c),
            Err(_) => Err(XC_PWR_NO_DATA),
        };
        // ③ seal 拒。
        let (denied, code) = reg.seal_profile_for_test(&PowerCurve {
            class: PowerClass::LaptopTgp,
            chip: String::from("AD106"),
            nominal_watts: 140,
            points: Vec::new(),
            source: DataSource::Measured,
        });
        // ④ export 放行（公开纪律——Runtime 消费面合法）。
        let ex = reg.export_profile("AD106");
        r1 == Err(XC_PWR_ROLE_DENY)
            && r2 == Err(XC_PWR_ROLE_DENY)
            && denied
            && code == XC_PWR_ROLE_DENY
            && reg.role_denies == 3
            && ex.is_ok()
    };
    s.add("role_deny", ok16, "runtime gate 3 denies+export ok");

    // 17 审计链：seq 严格递增 + 满容拒记不覆盖（1 register + 264 export →
    //     容量 256 满，dropped = 264 - 255 = 9）。
    let ok17 = {
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        if let Ok(lc) = l {
            let _ = p.mount(lc);
        }
        let _ = reg.register(p);
        let n = AUDIT_CAP + 8; // 264 次 export
        let mut k = 0;
        while k < n {
            let _ = reg.export_profile("AD106");
            k += 1;
        }
        // 满容：账面 256 条，dropped 9。
        let mut acc = reg.audit_len() == AUDIT_CAP && reg.audit_dropped == (n - (AUDIT_CAP - 1)) as u32;
        // seq 严格递增。
        let mut i = 1;
        while i < reg.audit_len() {
            match (reg.audit_at(i - 1), reg.audit_at(i)) {
                (Some(a), Some(b)) => acc = acc && b.seq > a.seq,
                _ => acc = false,
            }
            i += 1;
        }
        // 旧痕不覆盖：首条必为 Registered。
        acc = acc
            && match reg.audit_at(0) {
                Some(a) => a.op == AuditOp::Registered,
                None => false,
            };
        acc
    };
    s.add("audit_chain", ok17, "seq strict+cap 256 drop 9");

    // 18 J 域联动：schema 四字段闭集 + F0854 对位三条非空 + 遥测三例
    //     （UnderWall/AtWall/DeepClamp）+ 空态显性拒 + 零瓦拒。
    let ok18 = {
        let mut acc = TELEMETRY_FIELDS.len() == 4 && F0854_ALIGNMENT.len() == 3;
        let mut ai = 0;
        while ai < F0854_ALIGNMENT.len() {
            acc = acc && F0854_ALIGNMENT[ai].0.len() > 0 && F0854_ALIGNMENT[ai].1.len() > 0;
            ai += 1;
        }
        acc = acc && F0854_ALIGNMENT[0].0 == "tbp_whole_board";
        let mut reg = NvPowerRegistry::new();
        let mut p = DualLineProfile::new("AD106");
        let l = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00), (200, 100_00)]),
            DataSource::Measured,
        );
        let d = PowerCurve::build(
            PowerClass::DesktopTdp,
            "AD106",
            220,
            fx_points(&[(100, 55_00), (220, 100_00)]),
            DataSource::VendorSpec,
        );
        match (l, d) {
            (Ok(lc), Ok(dc)) => {
                let _ = p.mount(lc);
                let _ = p.mount(dc);
                let _ = reg.register(p);
                let t1 = reg.telemetry_record("AD106", PowerClass::LaptopTgp, 100);
                let t2 = reg.telemetry_record("AD106", PowerClass::DesktopTdp, 220);
                let t3 = reg.telemetry_record("AD106", PowerClass::LaptopTgp, 140);
                acc = acc
                    && match t1 {
                        Ok(t) => t.zone == WallZone::UnderWall && t.zone_code() == 0,
                        Err(_) => false,
                    }
                    && match t2 {
                        Ok(t) => t.zone == WallZone::AtWall && t.zone_code() == 1,
                        Err(_) => false,
                    }
                    && match t3 {
                        Ok(t) => {
                            t.zone == WallZone::DeepClamp
                                && t.zone_code() == 2
                                && t.perf_pct == 85_00
                                && t.eff_ppm == 85_00 * 100 / 140
                        }
                        Err(_) => false,
                    };
                acc = acc
                    && reg.telemetry_record("ZZ99", PowerClass::LaptopTgp, 100) == Err(XC_PWR_NO_DATA)
                    && reg.telemetry_record("AD106", PowerClass::LaptopTgp, 0) == Err(XC_PWR_WALL_ZERO);
            }
            _ => acc = false,
        }
        acc
    };
    s.add("telemetry_alignment", ok18, "J-domain schema 3 zones");

    // 19 确定性红线：同输入双跑（曲线构造×2 + 规则×2 + 导出×2）逐位一致。
    let ok19 = {
        let c1 = matrix_curve();
        let c2 = matrix_curve();
        let mut acc = true;
        let mut w = 40;
        while w <= 220 {
            acc = acc && c1.perf_at(w) == c2.perf_at(w) && curve_rule(w) == curve_rule(w);
            if !acc {
                break;
            }
            w += 10;
        }
        acc = acc
            && c1.peak_efficiency() == c2.peak_efficiency()
            && c1.diminishing_start() == c2.diminishing_start()
            && c1.wall_loss_pct() == c2.wall_loss_pct();
        let mut reg1 = NvPowerRegistry::new();
        let mut reg2 = NvPowerRegistry::new();
        let mut p1 = DualLineProfile::new("AD106");
        let mut p2 = DualLineProfile::new("AD106");
        let l1 = PowerCurve::build(
            PowerClass::LaptopTgp,
            "AD106",
            140,
            fx_points(&[(60, 40_00), (140, 85_00)]),
            DataSource::Measured,
        );
        if let Ok(lc) = l1 {
            let l2 = PowerCurve::build(
                PowerClass::LaptopTgp,
                "AD106",
                140,
                fx_points(&[(60, 40_00), (140, 85_00)]),
                DataSource::Measured,
            );
            if let Ok(lc2) = l2 {
                let _ = p1.mount(lc);
                let _ = p2.mount(lc2);
                let _ = reg1.register(p1);
                let _ = reg2.register(p2);
                let j1 = reg1.export_profile("AD106");
                let j2 = reg2.export_profile("AD106");
                acc = acc && j1 == j2;
            } else {
                acc = false;
            }
        } else {
            acc = false;
        }
        acc
    };
    s.add("determinism", ok19, "bit-identical double run");

    // 20 聚合·模块十三组自检全绿（骨架五组+深化八组总闸）。
    let ok20 = {
        let mut reg = NvPowerRegistry::new();
        reg.selfcheck_all().is_ok()
    };
    s.add("selfcheck_all_agg", ok20, "13 selfcheck groups green");

    s
}
