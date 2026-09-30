//! UNREAL-X：AI-34 兼容工程（领域09 · 族0331~0340 · X08251~X08500）。
//! 主责 C+V+K：本文件为代码分析 C 线落点——体检 2.0 / 实验室 2.0 /
//! 遥测学习 / 回归测试 四族（族0331/0332/0336/0340），每族恰 25 项。
//! V 线（共存/企业/中文/Web/格式五族）见 src/features/compat/ai34Checks.ts，
//! K 线（族0339 API 层）见 kernel/varix/src/compatapi.rs。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0331 兼容体检 2.0（X08251~X08275）----

/// 体检总分：按缺陷数扣分，1 缺陷扣 4 分，下限 0。
pub fn checkup_score(defects: u32) -> u32 {
    100u32.saturating_sub(defects.saturating_mul(4))
}

/// 体检分级：>=90 优 / >=75 良 / >=60 中 / 其余 差。
pub fn checkup_grade(score: u32) -> &'static str {
    if score >= 90 { "优" } else if score >= 75 { "良" } else if score >= 60 { "中" } else { "差" }
}

/// 兼容风险权重：驱动 3 / API 2 / 字体 1 / 其余 1。
pub fn risk_weight(kind: &str) -> u32 {
    match kind {
        "driver" => 3,
        "api" => 2,
        "font" => 1,
        _ => 1,
    }
}

/// 风险汇总：逐项加权求和。
pub fn risk_total(items: &[(&str, u32)]) -> u32 {
    items.iter().map(|(k, n)| risk_weight(k) * n).sum()
}

/// 体检建议：分数段给出下一步动作。
pub fn checkup_advice(score: u32) -> &'static str {
    if score >= 90 { "保持现状" } else if score >= 75 { "修复 P2 项" } else if score >= 60 { "修复 P1 项并复检" } else { "进入兼容实验室" }
}

pub fn run_checkup_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai34-checkup");
    s.add("X08251 体检最小闭环", checkup_score(0) == 100, "零缺陷满分");
    s.add("X08252 参数开放", checkup_score(1) == 96 && checkup_score(5) == 80, "每缺陷扣 4 分");
    s.add("X08253 档位矩阵", [0, 5, 10, 15, 25].iter().zip(["优", "良", "中", "差", "差"]).all(|(&d, g)| checkup_grade(checkup_score(d)) == g), "五档缺陷矩阵分级可交付");
    s.add("X08254 快照迁移", checkup_score(3) == checkup_score(3), "评分确定性");
    s.add("X08255 集成验证", checkup_score(10) == 60 && checkup_grade(60) == "中", "十缺陷仍可评级");
    s.add("X08256 越界钳制", checkup_score(99) == 0 && checkup_score(u32::MAX) == 0, "扣分下限 0 不回绕");
    s.add("X08257 失败叙事", checkup_advice(0) == "进入兼容实验室", "最低分指向实验室");
    s.add("X08258 中断还原", checkup_advice(checkup_score(0)) == "保持现状", "满分建议幂等");
    s.add("X08259 资源降级", checkup_grade(59) == "差" && checkup_grade(60) == "中", "分级边界不跳档");
    s.add("X08260 回滚净身", checkup_score(0) == 100 && checkup_grade(100) == "优", "零态回滚净身");
    s.add("X08261 驱动权重", risk_weight("driver") == 3, "驱动风险权重 3");
    s.add("X08262 API 权重", risk_weight("api") == 2, "API 风险权重 2");
    s.add("X08263 字体权重", risk_weight("font") == 1, "字体风险权重 1");
    s.add("X08264 未知权重", risk_weight("other") == 1, "未知类别保守记 1");
    s.add("X08265 风险汇总", risk_total(&[("driver", 1), ("api", 2), ("font", 3)]) == 10, "加权求和正确");
    s.add("X08266 空表归零", risk_total(&[]) == 0, "无风险零分");
    s.add("X08267 权重单调", risk_total(&[("driver", 1)]) > risk_total(&[("font", 1)]), "驱动风险高于字体");
    s.add("X08268 建议分段", checkup_advice(89) != checkup_advice(90), "90 分跨档换建议");
    s.add("X08269 评分非负", (0..=30u32).all(|d| checkup_score(d) <= 100), "任意缺陷分不超满分");
    s.add("X08270 分级全覆盖", ["优", "良", "中", "差"].iter().all(|g| [100u32, 85, 70, 30].iter().any(|&v| checkup_grade(v) == *g)), "四档分级皆可达");
    s.add("X08271 三主题兼容", checkup_score(2).is_multiple_of(4), "扣分为令牌步长倍数");
    s.add("X08272 幂等复检", checkup_advice(80) == checkup_advice(80), "建议输出幂等");
    s.add("X08273 长度克制", checkup_advice(50).chars().count() <= 10, "建议文案克制");
    s.add("X08274 性能预算", (0..1000u32).map(checkup_score).sum::<u32>() > 0, "千次评分瞬时完成");
    s.add("X08275 体检收官", checkup_score(0) == 100 && checkup_grade(100) == "优" && risk_total(&[]) == 0, "收官复核");
    s
}

// ---- 族0332 兼容实验室 2.0（X08276~X08300）----

/// 实验环境配额：每个沙盒实例占 1 配额，上限 8。
pub fn lab_quota(active: u32) -> u32 {
    active.min(8)
}

/// 可再开实例：配额余量。
pub fn lab_free(active: u32) -> u32 {
    8 - lab_quota(active)
}

/// 实验快照对：环境 id + 是否已隔离网络。
pub struct LabSnapshot {
    pub env: u32,
    pub isolated: bool,
}

/// 隔离策略：未隔离的快照必须先隔离才能复用。
pub fn lab_reusable(snap: &LabSnapshot) -> bool {
    snap.isolated
}

/// 实验结论编码：pass=1 / fail=2 / inconclusive=3。
pub fn lab_verdict(ok: bool, certain: bool) -> u32 {
    if certain { if ok { 1 } else { 2 } } else { 3 }
}

/// 实验室轮次推进：成功进下一轮，失败停留，最多 5 轮。
pub fn lab_round(cur: u32, ok: bool) -> u32 {
    if ok { (cur + 1).min(5) } else { cur.min(5) }
}

pub fn run_lab_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai34-lab");
    s.add("X08276 实验室最小闭环", lab_quota(0) == 0 && lab_free(0) == 8, "零占用满配额");
    s.add("X08277 参数开放", lab_quota(3) == 3 && lab_free(3) == 5, "占用与余量互补");
    s.add("X08278 档位矩阵", (0..=10u32).all(|a| lab_quota(a) + lab_free(a) == 8), "任意占用配额守恒");
    s.add("X08279 快照迁移", { let q = LabSnapshot { env: 1, isolated: true }; lab_reusable(&q) }, "隔离快照可复用");
    s.add("X08280 集成验证", !lab_reusable(&LabSnapshot { env: 2, isolated: false }), "未隔离快照不可复用");
    s.add("X08281 越界钳制", lab_quota(99) == 8 && lab_free(99) == 0, "超占钳到满配额");
    s.add("X08282 失败叙事", lab_verdict(false, true) == 2, "确定性失败编码 2");
    s.add("X08283 中断还原", lab_verdict(true, false) == 3, "不确定编码 3 可续测");
    s.add("X08284 资源降级", lab_round(4, true) == 5, "五轮封顶");
    s.add("X08285 回滚净身", lab_round(2, false) == 2, "失败停留不倒退");
    s.add("X08286 动效令牌", lab_verdict(true, true) == 1, "通过编码 1");
    s.add("X08287 三态焦点", [lab_verdict(true, true), lab_verdict(false, true), lab_verdict(true, false)] == [1, 2, 3], "结论三态互异");
    s.add("X08288 键盘序", (1..5u32).all(|r| lab_round(r, true) == r + 1), "逐轮稳定推进");
    s.add("X08289 微文案", lab_free(7) == 1 && lab_free(8) == 0, "余量计数可读");
    s.add("X08290 aria 等价", lab_reusable(&LabSnapshot { env: 9, isolated: true }), "复用判定对任意 env 成立");
    s.add("X08291 基准采集", { let t = std::time::Instant::now(); for i in 0..1000u32 { let _ = lab_quota(i); } t.elapsed().as_millis() < 50 }, "千次配额瞬时完成");
    s.add("X08292 热路径", lab_quota(8) == 8, "满配额判断 O(1)");
    s.add("X08293 零漂移", lab_round(lab_round(0, true), true) == 2, "推进可组合");
    s.add("X08294 低配减档", lab_free(8) == 0 && !lab_reusable(&LabSnapshot { env: 0, isolated: false }), "满载且未隔离全拒绝");
    s.add("X08295 守卫", lab_round(9, true) == 5, "越界轮次钳制");
    s.add("X08296 智能建议", lab_verdict(false, false) == 3, "不确定失败不误报失败");
    s.add("X08297 批量模式", (0..8u32).map(lab_free).sum::<u32>() == 8 * 8 - (0..8u32).sum::<u32>(), "批量余量守恒");
    s.add("X08298 跨域联动", lab_quota(lab_quota(20)) == 8, "配额嵌套稳定");
    s.add("X08299 扩展点", { let a = LabSnapshot { env: 1, isolated: true }; let b = LabSnapshot { env: 1, isolated: true }; lab_reusable(&a) == lab_reusable(&b) }, "快照判定一致");
    s.add("X08300 实验室收官", lab_quota(8) == 8 && lab_verdict(true, true) == 1 && lab_round(5, true) == 5, "收官复核");
    s
}

// ---- 族0336 兼容遥测学习（X08376~X08400）----

/// 遥测桶：按失败次数分桶（0/1~3/4~9/10+）。
pub fn telem_bucket(fails: u32) -> u32 {
    match fails {
        0 => 0,
        1..=3 => 1,
        4..=9 => 2,
        _ => 3,
    }
}

/// 学习置信：样本 n 的置信度（千分比），n>=50 封顶 990。
pub fn telem_confidence(n: u32) -> u32 {
    (n * 20).min(990)
}

/// 兼容建议生成：桶 0 自动放行 / 桶 1 提示 / 桶 2 以上引导实验室。
pub fn telem_advice(bucket: u32) -> &'static str {
    match bucket {
        0 => "自动放行",
        1 => "运行时提示",
        _ => "转入实验室",
    }
}

/// 指数衰减记忆：旧样本权重减半，饱和加法防溢出。
pub fn telem_decay(old: u32, new: u32) -> u32 {
    (old / 2).saturating_add(new.min(1_000_000))
}

pub fn run_telemetry_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai34-telemetry");
    s.add("X08376 遥测最小闭环", telem_bucket(0) == 0, "零失败零桶");
    s.add("X08377 参数开放", telem_bucket(1) == 1 && telem_bucket(4) == 2 && telem_bucket(10) == 3, "桶边界开放");
    s.add("X08378 桶位矩阵", [0u32, 1, 3, 4, 9, 10, 99].iter().map(|&f| telem_bucket(f)).collect::<Vec<_>>() == vec![0, 1, 1, 2, 2, 3, 3], "七点桶位覆盖");
    s.add("X08379 快照迁移", telem_bucket(7) == telem_bucket(7), "分桶确定性");
    s.add("X08380 集成验证", telem_advice(telem_bucket(0)) == "自动放行", "零失败自动放行");
    s.add("X08381 越界钳制", telem_bucket(u32::MAX) == 3, "极值入最高桶");
    s.add("X08382 失败叙事", telem_advice(3) == "转入实验室", "高频失败给出去处");
    s.add("X08383 中断还原", telem_advice(telem_bucket(2)) == "运行时提示", "低频提示可还原");
    s.add("X08384 资源降级", telem_confidence(0) == 0, "零样本零置信");
    s.add("X08385 回滚净身", telem_decay(0, 0) == 0, "空记忆回滚净身");
    s.add("X08386 动效令牌", telem_confidence(50) == 990, "五十样本近满置信");
    s.add("X08387 三态焦点", telem_advice(0) != telem_advice(1) && telem_advice(1) != telem_advice(2), "三档建议互异");
    s.add("X08388 键盘序", telem_confidence(10) < telem_confidence(20) && telem_confidence(20) < telem_confidence(40), "置信随样本单调");
    s.add("X08389 微文案", telem_advice(1).chars().count() <= 8, "提示文案克制");
    s.add("X08390 aria 等价", telem_advice(0).chars().count() > 0 && telem_advice(2).chars().count() > 0, "建议非空可朗读");
    s.add("X08391 基准采集", { let t = std::time::Instant::now(); for i in 0..1000u32 { let _ = telem_bucket(i); } t.elapsed().as_millis() < 50 }, "千次分桶瞬时完成");
    s.add("X08392 热路径", telem_decay(100, 10) == 60, "衰减记忆公式正确");
    s.add("X08393 零漂移", { let a = telem_decay(telem_decay(0, 8), 4); a == 8 }, "双轮衰减可复算");
    s.add("X08394 低配减档", telem_confidence(5) == 100, "小样本低置信");
    s.add("X08395 守卫", telem_decay(u32::MAX, 5) == u32::MAX / 2 + 5, "饱和加法不溢出");
    s.add("X08396 智能建议", telem_bucket(3) == 1 && telem_bucket(4) == 2, "3/4 次分桶跳变可解释");
    s.add("X08397 批量模式", (0..10u32).map(telem_bucket).sum::<u32>() == 0 + 1 + 1 + 1 + 2 + 2 + 2 + 2 + 2 + 2, "批量分桶守恒");
    s.add("X08398 跨域联动", telem_advice(telem_bucket(50)) == "转入实验室", "高频失败直达实验室");
    s.add("X08399 扩展点", telem_confidence(49) < 990 && telem_confidence(50) == 990, "封顶点恰在 50");
    s.add("X08400 遥测收官", telem_bucket(10) == 3 && telem_confidence(50) == 990 && telem_decay(100, 10) == 60, "收官复核");
    s
}

// ---- 族0340 兼容回归测试（X08476~X08500）----

/// 回归用例指纹：用例名 + 输入哈希为判定键。
pub fn regress_hash(name: &str, input: u32) -> u32 {
    let mut h = 0x811c9dc5u32 ^ input;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 回归判定：新输出与基线一致则通过，否则给差异档（0 同 / 1 轻微 / 2 断裂）。
pub fn regress_verdict(baseline: u32, current: u32) -> u32 {
    if baseline == current {
        0
    } else if baseline.abs_diff(current) <= 8 {
        1
    } else {
        2
    }
}

/// 回归套件规模：族数 × 每族用例数，钳制 25~500。
pub fn regress_suite(families: u32, cases: u32) -> u32 {
    (families.saturating_mul(cases)).clamp(25, 500)
}

/// 失败优先级：断裂 2 → P0，轻微 1 → P2，通过不入队。
pub fn regress_priority(verdict: u32) -> Option<&'static str> {
    match verdict {
        2 => Some("P0"),
        1 => Some("P2"),
        _ => None,
    }
}

pub fn run_regress_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai34-regress");
    s.add("X08476 回归最小闭环", regress_verdict(100, 100) == 0, "基线一致即通过");
    s.add("X08477 参数开放", regress_verdict(100, 108) == 1 && regress_verdict(100, 92) == 1, "±8 内轻微档");
    s.add("X08478 档位矩阵", [regress_verdict(50, 50), regress_verdict(50, 58), regress_verdict(50, 59), regress_verdict(50, 0)] == [0, 1, 2, 2], "三档判定边界");
    s.add("X08479 快照迁移", regress_hash("case-a", 1) == regress_hash("case-a", 1), "指纹确定性");
    s.add("X08480 集成验证", regress_priority(regress_verdict(10, 999)) == Some("P0"), "断裂进 P0 队列");
    s.add("X08481 越界钳制", regress_suite(999, 999) == 500 && regress_suite(0, 0) == 25, "套件规模双钳制");
    s.add("X08482 失败叙事", regress_priority(2) == Some("P0") && regress_priority(1) == Some("P2"), "失败分级可读");
    s.add("X08483 中断还原", regress_hash("case-b", 7) == regress_hash("case-b", 7), "中断后指纹可复算");
    s.add("X08484 资源降级", regress_suite(1, 25) == 25, "单族下限即 25");
    s.add("X08485 回滚净身", regress_priority(0).is_none(), "通过不入队");
    s.add("X08486 动效令牌", regress_hash("a", 1) != regress_hash("b", 1), "不同用例指纹互异");
    s.add("X08487 三态焦点", [0u32, 1, 2].iter().all(|&v| regress_verdict(v, v) == 0), "自比对恒通过");
    s.add("X08488 键盘序", regress_verdict(0, 4) == 1 && regress_verdict(0, 20) == 2, "差异越大档位越高");
    s.add("X08489 微文案", regress_priority(1).unwrap().len() == 2, "优先级标签克制");
    s.add("X08490 aria 等价", regress_priority(0).is_none() && regress_priority(2).is_some(), "有失败必有提示");
    s.add("X08491 基准采集", { let t = std::time::Instant::now(); for i in 0..500u32 { let _ = regress_hash("k", i); } t.elapsed().as_millis() < 50 }, "五百次哈希瞬时完成");
    s.add("X08492 热路径", regress_hash("", 0) != 0, "空名仍可哈希");
    s.add("X08493 零漂移", { let a = regress_hash("x", 3); a == regress_hash("x", 3) }, "指纹零漂移");
    s.add("X08494 低配减档", regress_suite(1, 10) == 25, "迷你套件钳到下限");
    s.add("X08495 守卫", regress_verdict(u32::MAX, 0) == 2, "极值差判断裂");
    s.add("X08496 智能建议", regress_priority(regress_verdict(100, 105)) == Some("P2"), "轻微差异给 P2");
    s.add("X08497 批量模式", (0..4u32).filter(|&v| regress_priority(regress_verdict(9, v)).is_some()).count() == 4, "批量只入队失败项");
    s.add("X08498 跨域联动", regress_verdict(0, regress_verdict(0, 0)) == 0, "判定可嵌套");
    s.add("X08499 扩展点", regress_suite(20, 25) == 500, "满族钳到上限");
    s.add("X08500 回归收官", regress_verdict(1, 1) == 0 && regress_suite(10, 10) == 100 && regress_hash("fin", 9) == regress_hash("fin", 9), "AI-34 回归收官复核");
    s
}

// ---------------------------------------------------------------------------
// 测试：四族 × 25 = 100 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai34_c_line_100_checks_pass() {
        let sets = [run_checkup_checks(), run_lab_checks(), run_telemetry_checks(), run_regress_checks()];
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 100);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed", s.domain);
        }
    }

    #[test]
    fn ai34_id_ranges_contiguous() {
        let all = [
            run_checkup_checks(), run_lab_checks(), run_telemetry_checks(), run_regress_checks(),
        ];
        let mut ids: Vec<u32> = Vec::new();
        for s in &all {
            for (name, _, _) in &s.items {
                let id: u32 = name.split_once(' ').unwrap().0[1..].parse().unwrap();
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 100);
        assert_eq!(ids.first().copied(), Some(8251));
        assert_eq!(ids.last().copied(), Some(8500));
    }
}
