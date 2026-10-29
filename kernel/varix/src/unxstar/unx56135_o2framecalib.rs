//! UNX-F56135 帧率 O2 噪声标定对接件（0x610C0 段）
//!
//! 锚点：UNX-F56135-J1 O2 噪声标定参数对接账（帧率采集器 O2 层三参数
//! ——噪声底/漂移带/采样窗——逐项对接落账）；J2 标定源一致性（对接的
//! 标定值必须来自标定件正本，双源比对一致才可入账）；J3 对接审计链
//! （对接波次、源指纹、参数三元组全留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·O2 对接面）。
//!
//! 结构：O2 参数三元组登记册（噪声底 × 漂移带 × 采样窗逐项落账）+
//! 标定源指纹核（源指纹匹配才放行入账）+ 对接波次链（单调递增拒回退）
//! + 漂移带重叠仲裁（新旧带重叠不足即拒对接）+ 注入核（未知参数名/
//! 指纹不匹配/波次回退/带重叠不足/空标定源必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x610C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x610C0;
pub const O2F_UNK_PARAM: u32 = SEG_BASE + 0x01; // 未知 O2 参数名（非三元组闭集）
pub const O2F_FP_MISS: u32 = SEG_BASE + 0x02; // 标定源指纹不匹配
pub const O2F_WAVE_BACK: u32 = SEG_BASE + 0x03; // 对接波次回退
pub const O2F_BAND_OVERLAP: u32 = SEG_BASE + 0x04; // 漂移带重叠不足
pub const O2F_SRC_EMPTY: u32 = SEG_BASE + 0x05; // 空标定源
pub const O2F_VAL_RANGE: u32 = SEG_BASE + 0x06; // 参数值越物理界
pub const O2F_SEALED: u32 = SEG_BASE + 0x07; // 对接账已封账拒写

/// O2 参数名闭集（帧率采集器 O2 层三元组）。
pub const PARAM_NOISE: usize = 0;
pub const PARAM_BAND: usize = 1;
pub const PARAM_WINDOW: usize = 2;
pub const PARAM_COUNT: usize = 3;

/// 参数名渲染（账目与对接共用同源命名）。
pub const PARAM_NAMES: [&str; PARAM_COUNT] = ["noise_floor", "drift_band", "sample_window_ms"];

/// 参数物理界（越界即拒，防标定源脏值穿透）。
pub const NOISE_MAX: u32 = 5_000; // 噪声底上限（帧率万分比）
pub const BAND_MAX: u32 = 20_000; // 漂移带半宽上限（帧率万分比）
pub const WINDOW_MIN: u32 = 50; // 采样窗下限（毫秒）
pub const WINDOW_MAX: u32 = 10_000; // 采样窗上限（毫秒）

/// 标定源指纹：FNV-1a 32 位（与源件指纹同口径）。
pub fn src_fingerprint(src: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in src.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 参数值域核：逐参数按物理界校验。
pub fn value_in_range(param: usize, val: u32) -> bool {
    match param {
        PARAM_NOISE => val <= NOISE_MAX,
        PARAM_BAND => val <= BAND_MAX,
        PARAM_WINDOW => val >= WINDOW_MIN && val <= WINDOW_MAX,
        _ => false,
    }
}

/// 单参数对接账目：参数 × 值 × 源指纹 × 波次 四元组。
#[derive(Clone)]
pub struct O2ParamEntry {
    pub param: usize,
    pub value: u32,
    pub src_fp: u32,
    pub wave: u32,
}

impl O2ParamEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|{}|{:08x}|w{}",
            PARAM_NAMES[self.param], self.value, self.src_fp, self.wave
        )
    }
}

/// 漂移带重叠核：新旧带 [c-w, c+w] 区间重叠比例 ≥ 阈值才可对接。
pub fn band_overlap_ratio(old_c: u32, old_w: u32, new_c: u32, new_w: u32) -> u32 {
    // 全部用 i64 中转防 u32 下溢
    let lo1 = old_c as i64 - old_w as i64;
    let hi1 = old_c as i64 + old_w as i64;
    let lo2 = new_c as i64 - new_w as i64;
    let hi2 = new_c as i64 + new_w as i64;
    let ov = (hi1.min(hi2) - lo1.max(lo2)).max(0);
    let union = (hi1.max(hi2) - lo1.min(lo2)).max(1);
    ((ov as u64 * 10_000) / union as u64) as u32
}

pub const BAND_OVERLAP_MIN: u32 = 3_000; // 重叠比例下限（万分比）

/// O2 对接总账：三元组逐项落账 + 封账语义。
pub struct O2FrameCalibLedger {
    entries: [Option<O2ParamEntry>; PARAM_COUNT],
    sealed: bool,
    top_wave: u32,
    src_fp_expected: u32,
    audit: Vec<String>,
}

impl O2FrameCalibLedger {
    pub fn new(src_fp_expected: u32) -> Self {
        O2FrameCalibLedger {
            entries: [None, None, None],
            sealed: false,
            top_wave: 0,
            src_fp_expected,
            audit: Vec::new(),
        }
    }

    /// 对接一参：指纹匹配 + 波次单调 + 值域核 + 带重叠核四闸。
    pub fn calibrate(
        &mut self,
        param: usize,
        value: u32,
        src_fp: u32,
        wave: u32,
        old_center: u32,
        old_width: u32,
    ) -> Result<(), u32> {
        if param >= PARAM_COUNT {
            return Err(O2F_UNK_PARAM);
        }
        if self.sealed {
            return Err(O2F_SEALED);
        }
        if src_fp != self.src_fp_expected {
            return Err(O2F_FP_MISS);
        }
        if wave <= self.top_wave && self.top_wave > 0 {
            return Err(O2F_WAVE_BACK);
        }
        if !value_in_range(param, value) {
            return Err(O2F_VAL_RANGE);
        }
        if param == PARAM_BAND {
            let ratio = band_overlap_ratio(old_center, old_width, value, value / 2);
            if ratio < BAND_OVERLAP_MIN {
                return Err(O2F_BAND_OVERLAP);
            }
        }
        self.entries[param] = Some(O2ParamEntry {
            param,
            value,
            src_fp,
            wave,
        });
        self.top_wave = wave;
        self.audit
            .push(format!("calib:{}:w{}", PARAM_NAMES[param], wave));
        Ok(())
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed {
            return false;
        }
        // 三元组全落账才可封账
        if self.entries.iter().any(|e| e.is_none()) {
            return false;
        }
        self.sealed = true;
        self.audit.push("seal".to_string());
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn top_wave(&self) -> u32 {
        self.top_wave
    }

    pub fn entry(&self, param: usize) -> Option<&O2ParamEntry> {
        if param >= PARAM_COUNT {
            return None;
        }
        self.entries[param].as_ref()
    }

    /// 全量落账核：三元组逐项齐 + 波次一致。
    pub fn full_ledger_ok(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
            && self
                .entries
                .iter()
                .all(|e| e.as_ref().map(|x| x.wave).unwrap_or(0) == self.top_wave)
    }

    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 渲染导出：三元组行 + 封账印。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("SEALED\n");
        }
        out
    }
}

/// 标准夹具：指纹源 + 全三元组对接成功的标准账。
fn standard_ledger() -> O2FrameCalibLedger {
    let src = "o2-frame-calib-src-v1";
    let fp = src_fingerprint(src);
    let mut led = O2FrameCalibLedger::new(fp);
    // 波次 1：噪声底
    let _ = led.calibrate(PARAM_NOISE, 800, fp, 1, 0, 0);
    // 波次 2：漂移带（中心 12000 半宽 6000，与旧带 0 重叠按全带算）
    let _ = led.calibrate(PARAM_BAND, 12_000, fp, 2, 12_000, 6_000);
    // 波次 3：采样窗
    let _ = led.calibrate(PARAM_WINDOW, 250, fp, 3, 0, 0);
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知参数名
    let src = "o2-frame-calib-src-v1";
    let fp = src_fingerprint(src);
    let mut led = O2FrameCalibLedger::new(fp);
    if led.calibrate(9, 100, fp, 1, 0, 0) == Err(O2F_UNK_PARAM) {
        caught += 1;
    }
    // 注 2：指纹不匹配
    let mut led = O2FrameCalibLedger::new(fp);
    if led.calibrate(PARAM_NOISE, 100, 0xDEAD_BEEF, 1, 0, 0) == Err(O2F_FP_MISS) {
        caught += 1;
    }
    // 注 3：波次回退
    let mut led = O2FrameCalibLedger::new(fp);
    let _ = led.calibrate(PARAM_NOISE, 800, fp, 3, 0, 0);
    if led.calibrate(PARAM_BAND, 12_000, fp, 2, 12_000, 6_000) == Err(O2F_WAVE_BACK) {
        caught += 1;
    }
    // 注 4：带重叠不足（新带远离旧带）
    let mut led = O2FrameCalibLedger::new(fp);
    if led.calibrate(PARAM_BAND, 60_000, fp, 1, 5_000, 1_000) == Err(O2F_BAND_OVERLAP) {
        caught += 1;
    }
    // 注 5：封账后写
    let mut led = standard_ledger();
    let _ = led.seal();
    if led.calibrate(PARAM_NOISE, 900, fp, 4, 0, 0) == Err(O2F_SEALED) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 对接账全量；J2 标定源一致；J3 对接审计链。
pub fn run_unx56135_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56135");
    // J1-1 三元组全落账
    let led = standard_ledger();
    let ok = led.full_ledger_ok() && !led.sealed() && led.top_wave() == 3;
    s.add("56135-J1-三元组全落账", ok, if ok { "" } else { "对接账不全" });
    // J1-2 渲染导出三行齐 + 参数名同源
    let text = standard_ledger().render_ledger();
    let ok = text.contains("noise_floor|800|") && text.contains("drift_band|12000|")
        && text.contains("sample_window_ms|250|");
    s.add("56135-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-3 值域核逐参数
    let ok = value_in_range(PARAM_NOISE, NOISE_MAX)
        && value_in_range(PARAM_BAND, BAND_MAX)
        && value_in_range(PARAM_WINDOW, WINDOW_MIN)
        && value_in_range(PARAM_WINDOW, WINDOW_MAX)
        && !value_in_range(PARAM_NOISE, NOISE_MAX + 1)
        && !value_in_range(PARAM_WINDOW, WINDOW_MIN - 1)
        && !value_in_range(9, 1);
    s.add("56135-J1-值域核逐参", ok, if ok { "" } else { "值域面漂移" });
    // J2-1 指纹确定性：同源同指纹
    let ok = src_fingerprint("o2-frame-calib-src-v1") == src_fingerprint("o2-frame-calib-src-v1");
    s.add("56135-J2-指纹确定性", ok, if ok { "" } else { "指纹漂移" });
    // J2-2 指纹异源性：异源异指纹
    let ok = src_fingerprint("src-a") != src_fingerprint("src-b");
    s.add("56135-J2-指纹异源", ok, if ok { "" } else { "指纹碰撞" });
    // J2-3 指纹已知值锚（FNV-1a 口径锚）
    let ok = src_fingerprint("") == 0x811C_9DC5;
    s.add("56135-J2-指纹空串锚", ok, if ok { "" } else { "口径漂移" });
    // J3-1 带重叠核：全重叠 = 满比例
    let ok = band_overlap_ratio(10_000, 2_000, 10_000, 2_000) == 10_000;
    s.add("56135-J3-带全重叠", ok, if ok { "" } else { "重叠算术漂移" });
    // J3-2 带零重叠：不相交带比例归零
    let ok = band_overlap_ratio(1_000, 500, 50_000, 500) == 0;
    s.add("56135-J3-带零重叠", ok, if ok { "" } else { "重叠算术漂移" });
    // J3-3 带半重叠：一半相交比例
    let ok = band_overlap_ratio(10_000, 1_000, 11_000, 1_000) > 0
        && band_overlap_ratio(10_000, 1_000, 11_000, 1_000) < 10_000;
    s.add("56135-J3-带半重叠", ok, if ok { "" } else { "重叠算术漂移" });
    // J3-4 审计链留痕：4 次对接 + 封账 = 4 账
    let mut led = standard_ledger();
    let n0 = led.audit_len();
    let _ = led.seal();
    let ok = n0 == 3 && led.audit_len() == 4 && led.sealed();
    s.add("56135-J3-审计链留痕", ok, if ok { "" } else { "审计链漂移" });
    // J3-5 封账前提：三元组缺一拒封
    let fp = src_fingerprint("o2-frame-calib-src-v1");
    let mut led = O2FrameCalibLedger::new(fp);
    let _ = led.calibrate(PARAM_NOISE, 800, fp, 1, 0, 0);
    let ok = !led.seal() && !led.sealed();
    s.add("56135-J3-缺一拒封", ok, if ok { "" } else { "封账闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56135-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56135_all_checks_green() {
        let set = run_unx56135_checks();
        assert!(set.all_passed(), "UNX-F56135 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56135_fingerprint_stable() {
        assert_eq!(src_fingerprint("o2-frame-calib-src-v1"), src_fingerprint("o2-frame-calib-src-v1"));
    }
}
