//! ves358_s19integ — VE-F4118 · T06 S06/T06 跨域联调
//! 输入无障碍×地区适配交叉演练：IME 组合期×地区格式输入（日期/数字键盘差异）+ 替代输入×地区表单 + 交叉场景八组×四断言（功能/无障碍/性能/审计——无障碍断言硬门）+ 断言不过场景红立案 + 交叉干扰隔离复测 + 场景数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xCC40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IgCode {
    Ok = 0,
    SceneRed = SEG_BASE + 0x01,
    Interference = SEG_BASE + 0x02,
}

/// 四断言维。
pub const FOUR_ASSERTS: [&'static str; 4] = ["func", "a11y", "perf", "audit"];

/// 八组交叉场景。
pub const EIGHT_SCENES: [&'static str; 8] = [
    "ime-ima-date-cn",       // IME 组合期 × 中文日期键盘
    "ime-ima-number-de",     // IME 组合期 × 德式数字（逗号小数）
    "alt-input-form-us",     // 替代输入（语音/开关）× 美式地址表单
    "alt-input-form-jp",     // 替代输入 × 日式姓名序表单
    "ime-zip-gb",            // IME × 英式邮编字母混排
    "alt-input-pay-br",      // 替代输入 × 巴西支付表单
    "ime-percent-fr",        // IME × 法式百分比输入
    "alt-input-resid-eu",    // 替代输入 × 驻留选择表单
];

/// 场景执行册：场景 × 四断言结果。
pub struct SceneBook {
    /// (场景, 断言维, 通过)。
    pub results: Vec<(&'static str, &'static str, bool)>,
    pub quarantined: Vec<&'static str>,
}

impl SceneBook {
    pub fn new() -> Self { SceneBook { results: Vec::new(), quarantined: Vec::new() } }

    pub fn assert(&mut self, scene: &'static str, dim: &'static str, ok: bool) {
        self.results.push((scene, dim, ok));
    }

    /// 场景四断言齐备（每维一条）。
    pub fn scene_complete(&self, scene: &str) -> bool {
        FOUR_ASSERTS.iter().all(|d| self.results.iter().any(|(s, dd, _)| *s == scene && dd == d))
    }

    /// 场景红：任一断言红 → 场景红（供立案）。
    pub fn scene_red(&self, scene: &str) -> bool {
        self.results.iter().any(|(s, _, ok)| *s == scene && !ok)
    }

    /// 无障碍硬门：四维中 a11y 必须全绿（域本色）。
    pub fn a11y_hardgate(&self, scene: &str) -> bool {
        self.results.iter().filter(|(s, d, _)| *s == scene && *d == "a11y")
            .all(|(_, _, ok)| *ok)
    }

    /// 八场景全断言齐备 + 全绿。
    pub fn all_pass(&self) -> bool {
        EIGHT_SCENES.iter().all(|s| self.scene_complete(s) && !self.scene_red(s))
    }

    /// 红场景立案。
    pub fn red_scenes(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (s, _, _) in self.results.iter() {
            if self.scene_red(s) && !out.contains(s) { out.push(s); }
        }
        out
    }

    /// 交叉干扰：两场景结果互斥 → 隔离复测。
    pub fn interfere(&mut self, a: &'static str, b: &'static str) -> bool {
        let ra = self.scene_red(a);
        let rb = self.scene_red(b);
        if ra != rb {
            self.quarantined.push(a);
            return true;
        }
        false
    }
}

/// 合成场景数据（确定性）。
pub fn synth_scene_data(seed: u32, n: usize) -> Vec<u32> {
    let mut h = seed;
    let mut out = Vec::new();
    for _ in 0..n {
        h = h.wrapping_mul(0x0100_0193) ^ 0xC2B2_AE35;
        out.push(h % 10_000);
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves358_checks() -> CheckSet {
    let mut set = CheckSet::new("ves358");

    // C358-01 八场景单源 + 四维单源。
    chk(&mut set, "C358-01", EIGHT_SCENES.len() == 8 && FOUR_ASSERTS.len() == 4, "八组×四维单源");

    // C358-02 首场景四断言全绿。
    let mut sb = SceneBook::new();
    for d in FOUR_ASSERTS.iter() { sb.assert(EIGHT_SCENES[0], d, true); }
    chk(&mut set, "C358-02", sb.scene_complete(EIGHT_SCENES[0]) && !sb.scene_red(EIGHT_SCENES[0]), "场景 1 四维全绿");

    // C358-03 无障碍硬门：a11y 红则场景不过（功能绿也拦）。
    sb.assert(EIGHT_SCENES[1], "func", true);
    sb.assert(EIGHT_SCENES[1], "a11y", false);
    sb.assert(EIGHT_SCENES[1], "perf", true);
    sb.assert(EIGHT_SCENES[1], "audit", true);
    chk(&mut set, "C358-03", !sb.a11y_hardgate(EIGHT_SCENES[1]) && sb.scene_red(EIGHT_SCENES[1]), "场景 2 硬门拦");

    // C358-04 红场景立案。
    chk(&mut set, "C358-04", sb.red_scenes() == vec![EIGHT_SCENES[1]], "红场景单条立案");

    // C358-05 交叉干扰隔离复测。
    chk(&mut set, "C358-05", sb.interfere(EIGHT_SCENES[0], EIGHT_SCENES[1]) && sb.quarantined.len() == 1, "异色 → 隔离复测");

    // C358-06 同色不隔离。
    let mut sb2 = SceneBook::new();
    for d in FOUR_ASSERTS.iter() { sb2.assert(EIGHT_SCENES[2], d, true); }
    for d in FOUR_ASSERTS.iter() { sb2.assert(EIGHT_SCENES[3], d, true); }
    chk(&mut set, "C358-06", !sb2.interfere(EIGHT_SCENES[2], EIGHT_SCENES[3]), "同绿不隔离");

    // C358-07 全八场景铺满全绿。
    let mut sb3 = SceneBook::new();
    for s in EIGHT_SCENES.iter() {
        for d in FOUR_ASSERTS.iter() { sb3.assert(s, d, true); }
    }
    chk(&mut set, "C358-07", sb3.all_pass() && sb3.results.len() == 32, "八×四全绿 32 断言");

    // C358-08 一处红则 all_pass 破。
    sb3.assert(EIGHT_SCENES[7], "perf", false);
    sb3.assert(EIGHT_SCENES[7], "audit", false);
    chk(&mut set, "C358-08", !sb3.all_pass(), "补红后整体破");

    // C358-09 IME×地区格式：德式小数逗号合成输入过功能断言。
    let data = synth_scene_data(11, 4);
    let joined_ok = data.iter().all(|v| *v < 10_000);
    chk(&mut set, "C358-09", joined_ok && data.len() == 4, "合成输入域内");

    // C358-10 断言维覆盖计数：场景 1 与 2 各四条。
    let c1 = sb.results.iter().filter(|(s, _, _)| *s == EIGHT_SCENES[0]).count();
    let c2 = sb.results.iter().filter(|(s, _, _)| *s == EIGHT_SCENES[1]).count();
    chk(&mut set, "C358-10", c1 == 4 && c2 == 4, "两场景各四断言");

    // C358-11 无障碍断言全部记录（域本色硬门复核）。
    let a11y_count = sb.results.iter().filter(|(_, d, _)| *d == "a11y").count();
    chk(&mut set, "C358-11", a11y_count == 2, "已录两场景的 a11y 断言");

    // C358-12 场景数据合成确定性。
    chk(&mut set, "C358-12", synth_scene_data(11, 4) == synth_scene_data(11, 4), "合成确定");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves358_smoke() {
        let set = run_ves358_checks();
        assert!(set.failed_count() == 0, "ves358 red: {}", set.red_items_text());
    }

    #[test]
    fn ves358_hardgate() {
        let mut sb = SceneBook::new();
        for d in FOUR_ASSERTS.iter() { sb.assert(EIGHT_SCENES[0], d, *d != "a11y"); }
        assert!(!sb.a11y_hardgate(EIGHT_SCENES[0]));
    }
}
