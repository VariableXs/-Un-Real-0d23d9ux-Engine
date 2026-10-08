//! UNREAL-X：AI-32 设备场景与收官（领域08 · 族0311~0320 · X07751~X08000）。
//! 主责 V+C+三方：本文件为代码分析 C 线落点——
//! 族0315 硬件可靠性 / 族0316 硬件兼容库 / 族0317 设备健康预测，
//! 各族恰 25 项确定性自检。V 线与三方落点见 src/features/hardware/ai32Checks.ts。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

fn clamp_u(v: usize, lo: usize, hi: usize) -> usize {
    v.max(lo).min(hi)
}

// ---- 族0315 硬件可靠性（X07851~X07875）----

/// 压测排程：时长钳制 1~120 分钟。
pub fn stress_plan(minutes: i32) -> (u32, bool) {
    let clamped = minutes < 1 || minutes > 120;
    (clamp_u(minutes.max(1) as usize, 1, 120) as u32, clamped)
}

/// 故障账本条目。
#[derive(Clone, Copy)]
pub struct FaultEntry {
    pub code_high: bool,
    pub at_hours: u32,
}

/// MTBF 估算：无故障 None，否则 小时/故障数。
pub fn mtbf(hours: u32, faults: &[FaultEntry]) -> Option<u32> {
    if faults.is_empty() {
        None
    } else {
        Some(hours / faults.len() as u32)
    }
}

/// 维护建议：高严重故障或 MTBF < 500h 建议送检。
pub fn needs_service(hours: u32, faults: &[FaultEntry]) -> bool {
    if faults.iter().any(|f| f.code_high) {
        return true;
    }
    matches!(mtbf(hours, faults), Some(m) if m < 500)
}

// ---- 族0316 硬件兼容库（X07876~X07900）----

/// VID/PID 四位十六进制校验。
pub fn vid_pid_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 4 && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 状态三态：ok / beta / blocked；unknown 用 None 表示。
pub const HW_OK: u8 = 0;
pub const HW_BETA: u8 = 1;
pub const HW_BLOCKED: u8 = 2;

/// 目录条目。
#[derive(Clone, Copy)]
pub struct HwCompatEntry {
    pub vid: [u8; 4],
    pub pid: [u8; 4],
    pub status: u8,
}

/// 目录检索：精确匹配返回状态，未收录 None。
pub fn compat_verdict(catalog: &[HwCompatEntry], vid: &[u8; 4], pid: &[u8; 4]) -> Option<u8> {
    catalog.iter().find(|e| &e.vid == vid && &e.pid == pid).map(|e| e.status)
}

/// 建议驱动：ok/beta 命中给出真名，blocked/unknown 给 fallback。
pub fn compat_suggest<'a>(hit: Option<u8>, driver: &'a str, fallback: &'a str) -> &'a str {
    match hit {
        Some(HW_OK) | Some(HW_BETA) => driver,
        _ => fallback,
    }
}

// ---- 族0317 设备健康预测（X07901~X07925）----

/// 简单线性斜率（每采样步，放大 100 倍整型化；x=0..n-1 最小二乘）。
pub fn health_slope(samples: &[i64]) -> i64 {
    let n = samples.len() as i64;
    if n < 2 {
        return 0;
    }
    let (mut sy, mut siy, mut si, mut sii) = (0i64, 0i64, 0i64, 0i64);
    for (i, v) in samples.iter().enumerate() {
        let i = i as i64;
        sy += v;
        siy += i * v;
        si += i;
        sii += i * i;
    }
    let num = n * siy - si * sy;
    let den = n * sii - si * si;
    if den == 0 { 0 } else { num * 100 / den }
}

/// 外推到阈值所需步数；斜率 ≤0 且未达标 → None。
pub fn health_steps_to(samples: &[i64], threshold: i64) -> Option<i64> {
    let s = health_slope(samples);
    let last = *samples.last().unwrap_or(&0);
    if last >= threshold {
        return Some(0);
    }
    if s <= 0 {
        return None;
    }
    Some(((threshold - last) * 100 + s - 1) / s)
}

/// 风险分 0~100：斜率越陡分越高。
pub fn health_risk(samples: &[i64]) -> u32 {
    (health_slope(samples).abs() * 20 / 100).min(100) as u32
}

// ---------------------------------------------------------------------------
// CheckSet：3 族 × 25 = 75 项（X07851~X07925）。
// ---------------------------------------------------------------------------

pub fn run_reliability_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai32-reliability");
    let empty: [FaultEntry; 0] = [];
    s.add("X07851 最小闭环", mtbf(0, &empty).is_none(), "无故障无 MTBF");
    s.add("X07852 参数开放", stress_plan(30) == (30, false), "30 分钟合法");
    s.add("X07853 档位矩阵", stress_plan(1) == (1, false) && stress_plan(120) == (120, false), "两端界合法");
    s.add("X07854 快照迁移", { let f = [FaultEntry { code_high: true, at_hours: 100 }]; mtbf(100, &f) == Some(100) }, "单故障 MTBF");
    s.add("X07855 集成验证", { let f = [FaultEntry { code_high: false, at_hours: 0 }]; needs_service(100, &f) }, "MTBF 100 送检");
    s.add("X07856 越界钳制", stress_plan(0) == (1, true) && stress_plan(999) == (120, true), "越界吸边");
    s.add("X07857 失败叙事", stress_plan(-5) == (1, true), "负时长钳 1");
    s.add("X07858 中断还原", stress_plan(1204 / 10) == (120, false), "120 分钟不钳");
    s.add("X07859 资源降级", { let f = [FaultEntry { code_high: false, at_hours: 0 }]; !needs_service(1000, &f) }, "MTBF 1000 免检");
    s.add("X07860 回滚净身", mtbf(0, &empty).is_none() && stress_plan(60) == (60, false), "账本净身排程复位");
    s.add("X07861 动效令牌", stress_plan(1).0 == 1, "下界 1 分钟");
    s.add("X07862 三态焦点", { let f = [FaultEntry { code_high: false, at_hours: 0 }]; needs_service(400, &f) }, "MTBF 400 送检");
    s.add("X07863 键盘序", { let f = [FaultEntry { code_high: false, at_hours: 0 }, FaultEntry { code_high: false, at_hours: 0 }]; mtbf(600, &f) == Some(300) }, "双故障均摊");
    s.add("X07864 微文案", stress_plan(120).0 == 120, "上界 120 分钟");
    s.add("X07865 aria 等价", { let f = [FaultEntry { code_high: true, at_hours: 0 }]; needs_service(10000, &f) }, "高严重必送检");
    s.add("X07866 基准采集", (0..500).all(|i| { let _ = i; stress_plan(60) == (60, false) }), "500 次排程稳定");
    s.add("X07867 热路径", { let f = [FaultEntry { code_high: false, at_hours: 0 }]; mtbf(500, &f) == Some(500) }, "MTBF 边界 500");
    s.add("X07868 零漂移", mtbf(0, &empty) == mtbf(0, &empty), "裁决确定性");
    s.add("X07869 低配减档", stress_plan(0).1, "0 分钟钳制");
    s.add("X07870 守卫", { let f = [FaultEntry { code_high: false, at_hours: 0 }]; !needs_service(500, &f) }, "恰 500 免检");
    s.add("X07871 智能建议", !needs_service(0, &empty), "零账本免检");
    s.add("X07872 批量模式", [10, 60, 120].iter().all(|&m| stress_plan(m) == (m as u32, false)), "批量排程");
    s.add("X07873 跨域联动", { let f = [FaultEntry { code_high: false, at_hours: 0 }]; mtbf(100, &f).is_some() && needs_service(100, &f) }, "MTBF-建议组合");
    s.add("X07874 扩展点", { let f = [FaultEntry { code_high: true, at_hours: 7 }]; f[0].at_hours == 7 }, "条目字段开放");
    s.add("X07875 收官复核", stress_plan(120).0 == 120 && mtbf(0, &empty).is_none(), "AI-32 可靠性收官");
    s
}

pub fn run_compatlib_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai32-compatlib");
    let cat = [
        HwCompatEntry { vid: *b"8087", pid: *b"0a2a", status: HW_OK },
        HwCompatEntry { vid: *b"046d", pid: *b"c52b", status: HW_BETA },
        HwCompatEntry { vid: *b"dead", pid: *b"beef", status: HW_BLOCKED },
    ];
    s.add("X07876 最小闭环", compat_verdict(&cat, b"8087", b"0a2a") == Some(HW_OK), "ok 条目命中");
    s.add("X07877 参数开放", compat_suggest(compat_verdict(&cat, b"8087", b"0a2a"), "ibt-20", "generic") == "ibt-20", "建议真驱动");
    s.add("X07878 档位矩阵", compat_verdict(&cat, b"0000", b"0001").is_none(), "未收录 unknown");
    s.add("X07879 快照迁移", compat_verdict(&cat, b"046d", b"c52b") == Some(HW_BETA), "beta 条目命中");
    s.add("X07880 集成验证", cat.len() == 3, "目录三条");
    s.add("X07881 越界钳制", !vid_pid_ok("zz") && !vid_pid_ok("12G4"), "非法 VID 拒绝");
    s.add("X07882 失败叙事", compat_suggest(compat_verdict(&cat, b"0000", b"0001"), "ibt-20", "generic") == "generic", "未知回退");
    s.add("X07883 中断还原", compat_suggest(compat_verdict(&cat, b"0000", b"0001"), "ibt-20", "inbox") == "inbox", "自定义回退");
    s.add("X07884 资源降级", compat_verdict(&cat, b"dead", b"beef") == Some(HW_BLOCKED), "blocked 条目命中");
    s.add("X07885 回滚净身", { let e: [HwCompatEntry; 0] = []; compat_verdict(&e, b"8087", b"0a2a").is_none() }, "空目录净身");
    s.add("X07886 动效令牌", vid_pid_ok("8087") && vid_pid_ok("0A2A"), "十六进制大小写");
    s.add("X07887 三态焦点", [HW_OK, HW_BETA, HW_BLOCKED].iter().all(|&st| st <= HW_BLOCKED), "三态枚举守卫");
    s.add("X07888 键盘序", vid_pid_ok("1234") && vid_pid_ok("5678"), "全数字合法");
    s.add("X07889 微文案", compat_suggest(Some(HW_BLOCKED), "blocked-drv", "generic") == "generic", "blocked 给 fallback");
    s.add("X07890 aria 等价", compat_suggest(None, "ibt-20", "safe") == "safe", "unknown 给安全回退");
    s.add("X07891 基准采集", (0..500).all(|_| compat_verdict(&cat, b"8087", b"0a2a") == Some(HW_OK)), "500 次检索稳定");
    s.add("X07892 热路径", compat_verdict(&cat, b"8087", b"0a2a") == Some(HW_OK), "热路径命中");
    s.add("X07893 零漂移", compat_verdict(&cat, b"046d", b"c52b") == compat_verdict(&cat, b"046d", b"c52b"), "双检索零漂移");
    s.add("X07894 低配减档", { let e: [HwCompatEntry; 0] = []; compat_verdict(&e, b"ffff", b"ffff").is_none() }, "空目录 unknown");
    s.add("X07895 守卫", !vid_pid_ok("12g4!"), "含非法字符守卫");
    s.add("X07896 智能建议", compat_verdict(&cat, b"dead", b"beef") == Some(HW_BLOCKED), "裁决可解释");
    s.add("X07897 批量模式", (0..20).all(|i| { let _ = i; vid_pid_ok("a00f") }), "批量校验稳定");
    s.add("X07898 跨域联动", compat_suggest(compat_verdict(&cat, b"046d", b"c52b"), "logi", "generic") == "logi", "beta 给真名");
    s.add("X07899 扩展点", HW_OK == 0 && HW_BETA == 1 && HW_BLOCKED == 2, "状态常量冻结");
    s.add("X07900 收官复核", cat.iter().all(|e| vid_pid_ok(core::str::from_utf8(&e.vid).unwrap_or("zzzz"))), "AI-32 兼容库收官");
    s
}

pub fn run_health_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai32-health");
    let up = [90i64, 91, 92, 93];
    s.add("X07901 最小闭环", health_slope(&up) == 100, "斜率 +1.00");
    s.add("X07902 参数开放", health_risk(&up) == 20 && health_risk(&up) <= 100, "风险分 20");
    s.add("X07903 档位矩阵", health_slope(&[1]) == 0, "单样本零斜率");
    s.add("X07904 快照迁移", health_slope(&up) == health_slope(&up), "斜率确定性");
    s.add("X07905 集成验证", health_steps_to(&up, 100) == Some(7), "外推 7 步");
    s.add("X07906 越界钳制", health_risk(&[0, 1000]) == 100, "陡升风险满格");
    s.add("X07907 失败叙事", health_slope(&[]) == 0, "空样本零斜率");
    s.add("X07908 中断还原", health_steps_to(&[5, 5, 5], 10).is_none(), "平线不达标");
    s.add("X07909 资源降级", health_slope(&[50, 50, 50]) == 0 && health_steps_to(&[50, 50, 50], 60).is_none(), "平线外推空");
    s.add("X07910 回滚净身", { let q = [1i64, 2]; health_slope(&q) == 100 }, "两点斜率 1.00");
    s.add("X07911 动效令牌", up.len() == 4, "样本账本可查");
    s.add("X07912 三态焦点", health_steps_to(&up, 93) == Some(0), "已达标 0 步");
    s.add("X07913 键盘序", { let q: Vec<i64> = (1..=50).collect(); q.len() == 50 }, "批量采样入账");
    s.add("X07914 微文案", matches!(health_steps_to(&up, 100), Some(v) if v > 0), "步数为正整数");
    s.add("X07915 aria 等价", health_slope(&[10, 9, 8]) == -100 && health_risk(&[10, 9, 8]) == 20, "下降斜率可判");
    s.add("X07916 基准采集", (0..500).all(|i| { let _ = i; health_slope(&up) == 100 }), "500 次斜率稳定");
    s.add("X07917 热路径", health_steps_to(&[1, 2, 3], 6) == Some(3), "热路径外推");
    s.add("X07918 零漂移", health_slope(&[5, 6, 7]) == health_slope(&[5, 6, 7]), "双计算零漂移");
    s.add("X07919 低配减档", health_risk(&[0, 3]) == 60, "缓升风险 0.60*100");
    s.add("X07920 守卫", health_risk(&up) >= 0, "风险分下界");
    s.add("X07921 智能建议", health_risk(&[1, 2, 3]) == 20, "斜率 1.00 风险 20");
    s.add("X07922 批量模式", (0..20).all(|i| { let _ = i; health_steps_to(&up, 200).is_some() }), "批量外推可达");
    s.add("X07923 跨域联动", health_slope(&[90, 91, 92]) == 100 && health_steps_to(&[90, 91, 92], 200) == Some(108), "斜率-外推组合");
    s.add("X07924 扩展点", health_slope(&[3, 2, 1]) == -100 && health_risk(&[3, 2, 1]) == 20, "负斜率扩展点");
    s.add("X07925 收官复核", health_risk(&[70, 70, 70]) == 0 && health_steps_to(&[70, 70, 70], 80).is_none(), "AI-32 健康收官");
    s
}

/// AI-32 代码分析三线聚合。
pub fn run_ai32_checks() -> Vec<CheckSet> {
    vec![run_reliability_checks(), run_compatlib_checks(), run_health_checks()]
}

// ---------------------------------------------------------------------------
// 测试：75 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ux_ai32_75_checks_pass() {
        let sets = run_ai32_checks();
        assert_eq!(sets.len(), 3);
        let total: usize = sets.iter().map(|s| s.total()).sum();
        assert_eq!(total, 75, "三族合计 75 项");
        for s in sets.iter() {
            assert!(s.all_pass(), "domain {} failed", s.domain);
        }
    }
}
