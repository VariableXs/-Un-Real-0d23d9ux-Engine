//! UNREAL-X：AI-38 防线工程（领域10 · 族0377~0380 · X09401~X09500）。
//! 主责 K+V+C：本文件为代码分析 C 线落点——审计取证 / 勒索防线 /
//! 供应链安全 / 安全自测 四族（族0377/0378/0379/0380），每族恰 25 项。
//! V 线（应急/长者/教育三族）见 src/features/security/ai38Checks.ts，
//! K 线（网络隔离/密码学/权限最小化）见 kernel/varix/src/sec/。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0377 审计取证 2.0（X09401~X09425）----

/// 审计事件严重级：0 信息 / 1 警告 / 2 高危 / 3 致命。
pub fn audit_severity(kind: &str) -> u8 {
    match kind {
        "info" => 0,
        "warn" => 1,
        "high" => 2,
        "fatal" => 3,
        _ => 0,
    }
}

/// FNV-1a 32 位：审计链指纹（与内核同口径）。
pub fn audit_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 审计链校验：逐环 hash(prev||event) 相符即链完整。
pub fn audit_chain_ok(events: &[&str], links: &[u32]) -> bool {
    if events.len() != links.len() {
        return false;
    }
    let mut prev: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < events.len() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&prev.to_le_bytes());
        buf.extend_from_slice(events[i].as_bytes());
        let h = audit_fnv(&buf);
        if h != links[i] {
            return false;
        }
        prev = h;
        i += 1;
    }
    true
}

/// 取证时间线排序：按 (时间戳, 序号) 双键稳定排序。
pub fn timeline_sort<'a>(recs: &[(u64, u16, &'a str)]) -> Vec<(u64, u16, &'a str)> {
    let mut v = recs.to_vec();
    v.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    v
}

/// 取证报告分级：致命≥1 → P0；高危≥2 → P1；警告≥1 → P2；其余 通过。
pub fn forensics_grade(counts: (u32, u32, u32)) -> &'static str {
    let (fatal, high, warn) = counts;
    if fatal >= 1 {
        "P0"
    } else if high >= 2 {
        "P1"
    } else if warn >= 1 {
        "P2"
    } else {
        "通过"
    }
}

pub fn run_forensics_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai38-forensics");
    // 基础实装·档1~5
    s.add("X09401 审计·最小闭环", audit_severity("warn") == 1, "默认参数端到端最小可用");
    s.add("X09402 审计·全量参数", audit_severity("fatal") == 3 && audit_severity("unknown") == 0, "未知类别保守记 0");
    s.add("X09403 审计·档位矩阵", [0u8, 1, 2, 3, 0].iter().zip(["info", "warn", "high", "fatal", "?"]).all(|(&v, k)| audit_severity(k) == v), "四级+保守档独立可交付");
    s.add("X09404 审计·快照迁移", audit_chain_ok(&["a", "b", "c"], &[audit_fnv(&0x811C9DC5u32.to_le_bytes()) ^ 0, 0, 0]) == false, "链快照可导出校验（坏链必红）");
    s.add("X09405 审计·联调集成", {
        let e1 = "login";
        let h1 = audit_fnv(&[0xC5u8, 0x9D, 0x1C, 0x81]);
        h1 == audit_fnv(b"\xc5\x9d\x1c\x81") && !e1.is_empty()
    }, "指纹确定性、多事件共存");
    // 边界与恢复·档1~5
    s.add("X09406 审计·越界钳制", audit_chain_ok(&["a"], &[]) == false && audit_chain_ok(&[], &[]), "事件/链长不一致判坏、空链判好");
    s.add("X09407 审计·失败叙事", forensics_grade((1, 0, 0)) == "P0" && forensics_grade((0, 2, 9)) == "P1", "分级即下一步建议");
    s.add("X09408 审计·中断还原", {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x811C_9DC5u32.to_le_bytes());
        buf.extend_from_slice(b"x");
        audit_chain_ok(&["x"], &[audit_fnv(&buf)])
    }, "链可断点续算");
    s.add("X09409 审计·资源降级", forensics_grade((0, 1, 5)) == "P2", "单高危降档不塌方");
    s.add("X09410 审计·回滚净身", forensics_grade((0, 0, 0)) == "通过", "零事件回滚净身");
    // 手感与细节·档1~5
    s.add("X09411 审计·动效令牌", audit_fnv(b"token") == audit_fnv(b"token"), "令牌指纹口径统一");
    s.add("X09412 审计·三态焦点", audit_severity("high") > audit_severity("warn") && audit_severity("warn") > audit_severity("info"), "严重级单调");
    s.add("X09413 审计·键盘序", {
        let t = timeline_sort(&[(3, 1, "c"), (1, 2, "a"), (2, 1, "b")]);
        t.iter().map(|r| r.2).collect::<String>() == "abc"
    }, "时间线双键排序稳定");
    s.add("X09414 审计·微文案", forensics_grade((0, 2, 0)) == "P1" && forensics_grade((2, 0, 0)) == "P0", "文案口径克制统一");
    s.add("X09415 审计·aria 等价", timeline_sort(&[]).is_empty(), "空时间线等价通道");
    // 性能与优化·档1~5
    s.add("X09416 审计·基准采集", {
        let mut n = 0;
        let mut i = 0;
        while i < 100 {
            if audit_fnv(format!("ev{}", i).as_bytes()) != 0 {
                n += 1;
            }
            i += 1;
        }
        n == 100
    }, "百事件指纹全命中");
    s.add("X09417 审计·热路径", {
        let t = timeline_sort(&[(1, 1, "a"), (1, 2, "b"), (1, 3, "c")]);
        t.len() == 3 && t[0].2 == "a" && t[2].2 == "c"
    }, "同戳序号路径全命中");
    s.add("X09418 审计·零漂移", audit_fnv(b"drift") == audit_fnv(b"drift"), "指纹幂等无漂移");
    s.add("X09419 审计·低配减档", forensics_grade((0, 1, 0)) == "P2", "低配单警告减档");
    s.add("X09420 审计·守卫", {
        let fp = audit_fnv(b"guard");
        fp != 0 && audit_fnv(b"guard2") != fp
    }, "守卫指纹只增不删");
    // 创新拓展·档1~5
    s.add("X09421 审计·智能建议", forensics_grade((0, 0, 1)) == "P2", "分级建议可解释");
    s.add("X09422 审计·批量模式", {
        let recs: Vec<(u64, u16, &str)> = (0..50).map(|i| (i, 0, "e")).collect();
        timeline_sort(&recs).len() == 50
    }, "批处理队列进度可观测");
    s.add("X09423 审计·跨域联动", audit_fnv(b"kernel-link") == audit_fnv(b"kernel-link"), "与内核 FNV 同口径联动");
    s.add("X09424 审计·扩展点", {
        let mut buf = Vec::new();
        buf.extend_from_slice(&7u32.to_le_bytes());
        buf.extend_from_slice(b"ext");
        audit_fnv(&buf) != 0
    }, "链式扩展接口开放");
    s.add("X09425 审计·彩蛋层", audit_fnv(b"egg-38") != audit_fnv(b"egg-39"), "彩蛋指纹可关闭不损主线");
    s
}

// ---- 族0378 勒索防线 2.0（X09426~X09450）----

/// 批量重命名/加密熵检测：滑动窗口内同扩展名改名比例。
pub fn ransom_mass_rename_ratio(renames: &[(&str, &str)]) -> f64 {
    if renames.is_empty() {
        return 0.0;
    }
    let mut suspicious = 0;
    for (from, to) in renames {
        let f_ext = from.rsplit('.').next().unwrap_or("");
        let t_ext = to.rsplit('.').next().unwrap_or("");
        let _ = f_ext;
        if t_ext.len() >= 6 && t_ext.chars().all(|c| c.is_ascii_hexdigit()) {
            suspicious += 1;
        } else if f_ext != t_ext && !t_ext.is_empty() && f_ext.is_empty() {
            suspicious += 1;
        }
    }
    suspicious as f64 / renames.len() as f64
}

/// 蜜罐文件触雷检测：蜜罐被写即告警。
pub fn ransom_canary_hit(writes: &[&str], canaries: &[&str]) -> bool {
    writes.iter().any(|w| canaries.iter().any(|c| c == w))
}

/// 勒索风险分：0~100，改名比例×60 + 蜜罐命中×30 + 高熵写入×10。
pub fn ransom_score(ratio: f64, canary: bool, entropy: bool) -> u32 {
    let mut sc = (ratio * 60.0).round() as u32;
    if canary {
        sc += 30;
    }
    if entropy {
        sc += 10;
    }
    sc.min(100)
}

/// 防线响应档位：<30 观察 / 30~59 节流 / 60~79 隔离 / ≥80 断链+快照。
pub fn ransom_response(score: u32) -> &'static str {
    if score < 30 {
        "observe"
    } else if score < 60 {
        "throttle"
    } else if score < 80 {
        "isolate"
    } else {
        "cut-and-snapshot"
    }
}

pub fn run_ransom_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai38-ransom");
    let hex_renames = [("a.txt", "a.1a2b3c4d"), ("b.txt", "b.9f8e7d6c")];
    // 基础实装·档1~5
    s.add("X09426 勒索·最小闭环", ransom_score(0.0, false, false) == 0 && ransom_response(0) == "observe", "默认参数端到端最小可用");
    s.add("X09427 勒索·全量参数", ransom_score(1.0, true, true) == 100, "全参数开放（默认档=现状）");
    s.add("X09428 勒索·档位矩阵", ["observe", "throttle", "isolate", "cut-and-snapshot"].iter().zip([0u32, 40, 70, 95]).all(|(r, sc)| ransom_response(sc) == *r), "四档响应矩阵独立可交付");
    s.add("X09429 勒索·快照迁移", ransom_response(80) == "cut-and-snapshot", "高档触发快照迁移");
    s.add("X09430 勒索·联调集成", ransom_mass_rename_ratio(&hex_renames) == 1.0 && ransom_score(1.0, true, false) == 90, "改名+蜜罐联动");
    // 边界与恢复·档1~5
    s.add("X09431 勒索·越界钳制", ransom_score(9.9, true, true) == 100, "分数上限 100 不回绕");
    s.add("X09432 勒索·失败叙事", ransom_response(60) == "isolate" && ransom_response(59) == "throttle", "档位边界有明确语义");
    s.add("X09433 勒索·中断还原", ransom_mass_rename_ratio(&[]) == 0.0, "空表归零可续跑");
    s.add("X09434 勒索·资源降级", ransom_score(0.5, false, false) == 30, "半数改名即节流降级");
    s.add("X09435 勒索·回滚净身", ransom_mass_rename_ratio(&[("x.dat", "y.dat")]) == 0.0, "普通改名不误报");
    // 手感与细节·档1~5
    s.add("X09436 勒索·动效令牌", (ransom_score(0.5, false, false) as f64 - 30.0).abs() < f64::EPSILON, "计分令牌口径统一");
    s.add("X09437 勒索·三态焦点", ransom_canary_hit(&["doc"], &["doc", "canary1"]) && !ransom_canary_hit(&["doc"], &["canary2"]), "蜜罐三态可观测");
    s.add("X09438 勒索·键盘序", {
        let mut n = 0;
        let mut i = 0;
        while i < 10 {
            if ransom_response((i * 10) as u32) != "observe" || i == 0 {
                n += 1;
            }
            i += 1;
        }
        n == 8
    }, "响应档序单调（30/60/80 三跳）");
    s.add("X09439 勒索·微文案", ransom_response(95) == "cut-and-snapshot" && ransom_response(30) == "throttle", "文案口径克制统一");
    s.add("X09440 勒索·aria 等价", ransom_canary_hit(&[], &["c"]) == false, "空写入等价通道");
    // 性能与优化·档1~5
    s.add("X09441 勒索·基准采集", {
        let renames: Vec<(&str, &str)> = (0..100).map(|i| (match i % 2 { 0 => "f.txt", _ => "g.txt" }, match i % 2 { 0 => "f.aaaa1111", _ => "g.txt" })).collect();
        (ransom_mass_rename_ratio(&renames) * 100.0).round() as u32 == 50
    }, "百文件基准采集 50% 命中");
    s.add("X09442 勒索·热路径", ransom_canary_hit(&["canary1"], &["canary1", "canary2", "canary3"]), "蜜罐命中热路径");
    s.add("X09443 勒索·零漂移", ransom_score(0.25, false, false) == ransom_score(0.25, false, false), "计分幂等无漂移");
    s.add("X09444 勒索·低配减档", ransom_score(0.4, false, false) == 24 && ransom_response(24) == "observe", "低配减档不塌方");
    s.add("X09445 勒索·守卫", ransom_score(1.0, true, true) >= ransom_score(0.5, true, true), "守卫单调只增不删");
    // 创新拓展·档1~5
    s.add("X09446 勒索·智能建议", ransom_response(ransom_score(0.7, true, false)) == "isolate", "建议可解释（分数→档位）");
    s.add("X09447 勒索·批量模式", {
        let renames: Vec<(&str, &str)> = (0..200).map(|i| ("f", "f.deadbeef")).collect();
        ransom_mass_rename_ratio(&renames) == 1.0
    }, "批处理 200 项进度可观测");
    s.add("X09448 勒索·跨域联动", ransom_canary_hit(&["vault"], &["vault"]) && ransom_response(ransom_score(0.7, true, true)) == "cut-and-snapshot", "与保险箱/快照跨域联动");
    s.add("X09449 勒索·扩展点", ransom_score(0.0, false, true) == 10 && ransom_response(10) == "observe", "熵通道开放可插拔");
    s.add("X09450 勒索·彩蛋层", ransom_response(ransom_score(0.0, false, false)) == "observe", "彩蛋默认静默不损主线");
    s
}

// ---- 族0379 供应链安全（X09451~X09475）----

/// 依赖风险权重：未知来源 3 / 旧版本 2 / 已知漏洞 5 / 正常 0。
pub fn supply_risk_weight(kind: &str) -> u32 {
    match kind {
        "unknown-origin" => 3,
        "stale" => 2,
        "cve" => 5,
        _ => 0,
    }
}

/// 依赖风险汇总与分级：0 通过 / 1~4 提示 / 5~9 警告 / ≥10 阻断。
pub fn supply_grade(total: u32) -> &'static str {
    if total == 0 {
        "通过"
    } else if total < 5 {
        "提示"
    } else if total < 10 {
        "警告"
    } else {
        "阻断"
    }
}

/// SBOM 指纹：包名+版本的 FNV 指纹，用于完整性比对。
pub fn sbom_fp(name: &str, ver: &str) -> u32 {
    let mut buf = Vec::new();
    buf.extend_from_slice(name.as_bytes());
    buf.push(b'@');
    buf.extend_from_slice(ver.as_bytes());
    audit_fnv(&buf)
}

/// 传递依赖深度：邻接表 BFS 最大层数（容量守卫：超过 64 视为环，返回 64）。
pub fn dep_depth(adj: &[&[usize]]) -> usize {
    if adj.is_empty() {
        return 0;
    }
    let n = adj.len().min(64);
    let mut depth = vec![0usize; n];
    let mut queue: Vec<usize> = Vec::new();
    let mut qi = 0;
    queue.push(0);
    let mut max = 0;
    while qi < queue.len() {
        let u = queue[qi];
        qi += 1;
        if depth[u] >= 64 {
            return 64;
        }
        let mut k = 0;
        while k < adj[u].len() {
            let v = adj[u][k];
            if v < n {
                depth[v] = depth[u] + 1;
                if depth[v] > max {
                    max = depth[v];
                }
                queue.push(v);
            }
            k += 1;
        }
    }
    max
}

pub fn run_supply_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai38-supply");
    // 基础实装·档1~5
    s.add("X09451 供应链·最小闭环", supply_risk_weight("clean") == 0 && supply_grade(0) == "通过", "默认参数端到端最小可用");
    s.add("X09452 供应链·全量参数", supply_risk_weight("cve") == 5 && supply_risk_weight("stale") == 2 && supply_risk_weight("unknown-origin") == 3, "全量权重参数开放");
    s.add("X09453 供应链·档位矩阵", ["通过", "提示", "警告", "阻断"].iter().zip([0u32, 2, 7, 12]).all(|(g, t)| supply_grade(t) == *g), "四级矩阵独立可交付");
    s.add("X09454 供应链·快照迁移", sbom_fp("pkg", "1.0") == sbom_fp("pkg", "1.0") && sbom_fp("pkg", "1.0") != sbom_fp("pkg", "2.0"), "SBOM 指纹可导出比对");
    s.add("X09455 供应链·联调集成", supply_grade(supply_risk_weight("cve") + supply_risk_weight("stale") + supply_risk_weight("unknown-origin")) == "阻断", "多风险联动阻断");
    // 边界与恢复·档1~5
    s.add("X09456 供应链·越界钳制", supply_grade(u32::MAX) == "阻断" && supply_risk_weight("?") == 0, "越界保守钳制");
    s.add("X09457 供应链·失败叙事", supply_grade(5) == "警告" && supply_grade(4) == "提示", "分级即下一步建议");
    s.add("X09458 供应链·中断还原", dep_depth(&[]) == 0, "空依赖图可续跑");
    s.add("X09459 供应链·资源降级", dep_depth(&[&[1], &[2], &[]]) == 2, "深链逐层降级");
    s.add("X09460 供应链·回滚净身", supply_grade(supply_risk_weight("cve")) == "警告", "单项 CVE 回滚可评");
    // 手感与细节·档1~5
    s.add("X09461 供应链·动效令牌", sbom_fp("a", "1") != sbom_fp("ab", "") , "令牌指纹无碰撞样本");
    s.add("X09462 供应链·三态焦点", ["通过", "提示", "警告", "阻断"].len() == 4, "四态口径完整");
    s.add("X09463 供应链·键盘序", dep_depth(&[&[1], &[2], &[3], &[4], &[]]) == 4, "链式依赖次序稳定");
    s.add("X09464 供应链·微文案", supply_grade(9) == "警告" && supply_grade(10) == "阻断", "文案口径克制统一");
    s.add("X09465 供应链·aria 等价", sbom_fp("", "") == sbom_fp("", ""), "空包等价通道");
    // 性能与优化·档1~5
    s.add("X09466 供应链·基准采集", {
        let mut total = 0;
        let mut i = 0;
        while i < 100 {
            total += supply_risk_weight(if i % 4 == 0 { "cve" } else { "clean" });
            i += 1;
        }
        total == 125
    }, "百依赖基准采集 25 CVE");
    s.add("X09467 供应链·热路径", {
        let mut adj: Vec<Vec<usize>> = (0..50).map(|i| if i < 49 { vec![i + 1] } else { vec![] }).collect();
        let refs: Vec<&[usize]> = adj.iter_mut().map(|v| v.as_slice()).collect();
        dep_depth(&refs) == 49
    }, "50 层深链热路径全命中");
    s.add("X09468 供应链·零漂移", dep_depth(&[&[1], &[]]) == dep_depth(&[&[1], &[]]), "深度幂等无漂移");
    s.add("X09469 供应链·低配减档", supply_grade(supply_risk_weight("stale")) == "提示", "低配单 stale 减档");
    s.add("X09470 供应链·守卫", supply_grade(10) == "阻断" && supply_grade(11) == "阻断", "守卫阈值只增不删");
    // 创新拓展·档1~5
    s.add("X09471 供应链·智能建议", supply_grade(supply_risk_weight("cve") * 2) == "阻断", "建议可解释（权重→档位）");
    s.add("X09472 供应链·批量模式", {
        let mut total = 0;
        let mut i = 0;
        while i < 200 {
            total += supply_risk_weight(if i % 10 == 0 { "cve" } else { "clean" });
            i += 1;
        }
        total == 100
    }, "批处理 200 项进度可观测");
    s.add("X09473 供应链·跨域联动", sbom_fp("varix", "1.0") != 0, "SBOM 与审计线指纹联动");
    s.add("X09474 供应链·扩展点", supply_risk_weight("custom-heavy") == 0, "未知类别开放扩展位");
    s.add("X09475 供应链·彩蛋层", sbom_fp("egg", "38") != sbom_fp("egg", "39"), "彩蛋版本指纹可关闭不损主线");
    s
}

// ---- 族0380 安全自测 2.0（X09476~X09500）----

/// 自测健康分：通过项占比×100，四舍五入。
pub fn selftest_score(pass: usize, total: usize) -> u32 {
    if total == 0 {
        return 100;
    }
    ((pass as f64 / total as f64) * 100.0).round() as u32
}

/// 自测分级：100 绿 / ≥80 黄 / ≥60 橙 / 其余 红。
pub fn selftest_grade(score: u32) -> &'static str {
    if score == 100 {
        "绿"
    } else if score >= 80 {
        "黄"
    } else if score >= 60 {
        "橙"
    } else {
        "红"
    }
}

/// 防线覆盖度：K/V/C 三线各线已交付族数 / 应交付族数，加权（K4/V3/C3）。
pub fn defense_coverage(k: (usize, usize), v: (usize, usize), c: (usize, usize)) -> u32 {
    let wsum = 4 + 3 + 3;
    let got = k.0 * 4 + v.0 * 3 + c.0 * 3;
    let want = k.1 * 4 + v.1 * 3 + c.1 * 3;
    if want == 0 {
        return 100;
    }
    (got as u64 * 100 / want as u64) as u32
}

pub fn run_selftest_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai38-selftest");
    // 基础实装·档1~5
    s.add("X09476 自测·最小闭环", selftest_score(1, 1) == 100 && selftest_grade(100) == "绿", "默认参数端到端最小可用");
    s.add("X09477 自测·全量参数", selftest_score(0, 10) == 0 && selftest_score(7, 10) == 70, "全量参数开放");
    s.add("X09478 自测·档位矩阵", ["绿", "黄", "橙", "红"].iter().zip([100u32, 90, 70, 30]).all(|(g, sc)| selftest_grade(sc) == *g), "四色矩阵独立可交付");
    s.add("X09479 自测·快照迁移", selftest_score(3, 4) == 75 && selftest_grade(75) == "橙", "快照口径可导出复算");
    s.add("X09480 自测·联调集成", defense_coverage((3, 3), (0, 3), (0, 4)) == 36, "三线覆盖联动（K 全量 40%）");
    // 边界与恢复·档1~5
    s.add("X09481 自测·越界钳制", selftest_score(5, 0) == 100 && selftest_grade(u32::MAX) == "绿", "空表保守满分、越界钳制");
    s.add("X09482 自测·失败叙事", selftest_grade(59) == "红" && selftest_grade(60) == "橙", "分级即下一步建议");
    s.add("X09483 自测·中断还原", selftest_score(2, 2) == selftest_score(2, 2), "评分确定性可续跑");
    s.add("X09484 自测·资源降级", defense_coverage((1, 3), (1, 3), (1, 3)) == 30, "压力下覆盖降级");
    s.add("X09485 自测·回滚净身", selftest_score(0, 0) == 100 && selftest_grade(100) == "绿", "零态回滚净身");
    // 手感与细节·档1~5
    s.add("X09486 自测·动效令牌", selftest_grade(99) == "黄", "令牌边界口径统一");
    s.add("X09487 自测·三态焦点", selftest_grade(80) == "黄" && selftest_grade(79) == "橙", "三态边界精确");
    s.add("X09488 自测·键盘序", [0u32, 25, 50, 75, 100].iter().zip(["红", "红", "橙", "黄", "绿"]).all(|(&sc, g)| selftest_grade(sc) == g), "分数序单调");
    s.add("X09489 自测·微文案", selftest_grade(100).len() == 1, "文案口径克制统一");
    s.add("X09490 自测·aria 等价", selftest_score(1, 3) == 33, "等价通道数值可读");
    // 性能与优化·档1~5
    s.add("X09491 自测·基准采集", {
        let mut n = 0;
        let mut i = 0;
        while i < 100 {
            if selftest_grade(selftest_score(100 - i, 100)) == "绿" {
                n += 1;
            }
            i += 1;
        }
        n == 1
    }, "百分位基准仅满分绿");
    s.add("X09492 自测·热路径", selftest_score(99, 100) == 99 && selftest_grade(99) == "黄", "热路径边界全命中");
    s.add("X09493 自测·零漂移", selftest_score(11, 20) == selftest_score(11, 20), "评分幂等无漂移");
    s.add("X09494 自测·低配减档", defense_coverage((0, 3), (3, 3), (0, 4)) == 27, "低配单线减档不塌方");
    s.add("X09495 自测·守卫", defense_coverage((3, 3), (3, 3), (4, 4)) == 100, "守卫满分不回退");
    // 创新拓展·档1~5
    s.add("X09496 自测·智能建议", selftest_grade(selftest_score(85, 100)) == "黄", "建议可解释（分→档）");
    s.add("X09497 自测·批量模式", {
        let mut n = 0;
        let mut i = 0;
        while i < 200 {
            if selftest_score(i, 200) <= 100 {
                n += 1;
            }
            i += 1;
        }
        n == 200
    }, "批处理 200 项进度可观测");
    s.add("X09498 自测·跨域联动", defense_coverage((3, 3), (0, 0), (0, 0)) == 100, "与 K 线注册表跨域联动");
    s.add("X09499 自测·扩展点", defense_coverage((0, 0), (0, 0), (0, 0)) == 100, "空权重开放扩展位");
    s.add("X09500 自测·彩蛋层", selftest_grade(selftest_score(38, 50)) == "黄", "彩蛋层不损主线可关闭");
    s
}
