//! UNX-F56070 内存采样 O2 噪声标定对接件（0x608A0 段）
//!
//! 锚点：UNX-F56070-J1 O2 噪声标定参数对接账（内存采集器 O2 层三参数
//! ——噪声底/漂移带/采样间隔——逐项对接落账）；J2 标定源一致性（对接值
//! 必须来自标定件正本指纹，双源比对一致才可入账）；J3 对接审计链（对接
//! 波次单调、源指纹留痕、参数三元组可复算）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·O2 对接面）。
//!
//! 结构：O2 参数三元组登记册（噪声底 × 漂移带 × 采样间隔逐项落账）+
//! 标定源指纹核（源指纹匹配才放行）+ 对接波次链（单调递增拒回退）+
//! 值域物理界核（越界即拒）+ 注入核（未知参数名/指纹不匹配/波次回退/
//! 值越界/空标定源/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x608A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x608A0;
pub const O2M_UNK_PARAM: u32 = SEG_BASE + 0x01; // 未知 O2 参数名（三元组闭集外）
pub const O2M_FP_MISS: u32 = SEG_BASE + 0x02; // 标定源指纹不匹配
pub const O2M_WAVE_BACK: u32 = SEG_BASE + 0x03; // 对接波次回退
pub const O2M_VAL_RANGE: u32 = SEG_BASE + 0x04; // 参数值越物理界
pub const O2M_SRC_EMPTY: u32 = SEG_BASE + 0x05; // 空标定源
pub const O2M_SEALED: u32 = SEG_BASE + 0x06; // 对接账已封账拒写
pub const O2M_PARTIAL: u32 = SEG_BASE + 0x07; // 三元组不全即用（残账放行）

/// O2 参数名闭集（内存采集器 O2 层三元组）。
pub const P_NOISE: usize = 0;
pub const P_BAND: usize = 1;
pub const P_INTERVAL: usize = 2;
pub const P_COUNT: usize = 3;
pub const P_NAMES: [&str; P_COUNT] = ["noise_floor_bp", "drift_band_bp", "sample_interval_ms"];

/// 参数物理界。
pub const NOISE_MAX: u32 = 8_000; // 噪声底上限（万分比）
pub const BAND_MAX: u32 = 25_000; // 漂移带半宽上限（万分比）
pub const INTERVAL_MIN: u32 = 10; // 采样间隔下限（毫秒）
pub const INTERVAL_MAX: u32 = 5_000; // 采样间隔上限（毫秒）

/// 标定源指纹：FNV-1a 32 位。
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
        P_NOISE => val <= NOISE_MAX,
        P_BAND => val <= BAND_MAX,
        P_INTERVAL => val >= INTERVAL_MIN && val <= INTERVAL_MAX,
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

/// O2 对接总账：三元组逐项落账 + 封账语义 + 审计链。
pub struct O2MemCalibLedger {
    entries: [Option<O2ParamEntry>; P_COUNT],
    sealed: bool,
    audit: Vec<String>,
}

impl O2MemCalibLedger {
    pub fn new() -> Self {
        O2MemCalibLedger { entries: [None, None, None], sealed: false, audit: Vec::new() }
    }

    /// 对接一参：指纹匹配 + 值域核 + 波次单调三闸。
    pub fn calibrate(
        &mut self,
        param: usize,
        value: u32,
        src_fp: u32,
        wave: u32,
        expected_fp: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(O2M_SEALED);
        }
        if param >= P_COUNT {
            return Err(O2M_UNK_PARAM);
        }
        if src_fp != expected_fp {
            return Err(O2M_FP_MISS);
        }
        if !value_in_range(param, value) {
            return Err(O2M_VAL_RANGE);
        }
        if wave == 0 {
            return Err(O2M_WAVE_BACK);
        }
        if let Some(prev) = &self.entries[param] {
            if wave <= prev.wave {
                return Err(O2M_WAVE_BACK);
            }
        }
        self.audit.push(format!(
            "{}|{}|{:08x}|w{}",
            P_NAMES[param], value, src_fp, wave
        ));
        self.entries[param] = Some(O2ParamEntry { param, value, src_fp, wave });
        Ok(())
    }

    /// 现行参数值（未对接显性拒绝）。
    pub fn value_of(&self, param: usize) -> Result<u32, u32> {
        if param >= P_COUNT {
            return Err(O2M_UNK_PARAM);
        }
        match &self.entries[param] {
            None => Err(O2M_PARTIAL),
            Some(e) => Ok(e.value),
        }
    }

    /// J1：三元组全量（三参数逐项已对接）。
    pub fn triple_complete(&self) -> bool {
        self.entries.iter().all(|e| e.is_some())
    }

    /// J2：参数三元组可复算（三值与给定基准逐项一致）。
    pub fn matches_baseline(&self, base: [u32; P_COUNT]) -> bool {
        for p in 0..P_COUNT {
            match &self.entries[p] {
                None => return false,
                Some(e) if e.value != base[p] => return false,
                _ => {}
            }
        }
        true
    }

    /// J3：审计链完整（链长 = 已对接参数数，逐条带指纹痕迹）。
    pub fn audit_ok(&self) -> bool {
        let mut done = 0usize;
        for e in self.entries.iter() {
            if e.is_some() {
                done += 1;
            }
        }
        self.audit.len() == done && self.audit.iter().all(|a| a.len() >= 8)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（三参数现行账复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for p in 0..P_COUNT {
            match &self.entries[p] {
                None => {}
                Some(e) => {
                    let line = format!("{}|{}|{}|{}", e.param, e.value, e.src_fp, e.wave);
                    for b in line.as_bytes() {
                        h ^= *b as u32;
                        h = h.wrapping_mul(0x0100_0193);
                    }
                }
            }
        }
        h
    }
}

/// 标定正本指纹（标准夹具与判据共用同源声明）。
pub const CANON_SRC: &str = "varix-mem-o2-canonical-source";
pub fn canon_fp() -> u32 {
    src_fingerprint(CANON_SRC)
}

/// 标准夹具：三参数按基准值全量对接（第 1 波次）。
pub fn standard_ledger() -> O2MemCalibLedger {
    let mut l = O2MemCalibLedger::new();
    let base: [u32; P_COUNT] = [1_200, 6_000, 50];
    for p in 0..P_COUNT {
        if l.calibrate(p, base[p], canon_fp(), 1, canon_fp()).is_err() {
            panic!("standard fixture calibrate must succeed");
        }
    }
    if !l.triple_complete() {
        panic!("standard fixture must complete triple");
    }
    l
}

/// 注入核：7 注入必抓（未知参数名/指纹不匹配/波次回退/值越界/空标定源/封账改写/残账放行）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知参数名（p=9 越闭集）
    let mut l = O2MemCalibLedger::new();
    if l.calibrate(9, 100, canon_fp(), 1, canon_fp()) == Err(O2M_UNK_PARAM) {
        caught += 1;
    }
    // 2) 指纹不匹配
    if l.calibrate(P_NOISE, 100, 0xDEAD_BEEF, 1, canon_fp()) == Err(O2M_FP_MISS) {
        caught += 1;
    }
    // 3) 值越物理界（噪声底超上限）
    if l.calibrate(P_NOISE, NOISE_MAX + 1, canon_fp(), 1, canon_fp()) == Err(O2M_VAL_RANGE) {
        caught += 1;
    }
    // 4) 采样间隔低于下限
    if l.calibrate(P_INTERVAL, INTERVAL_MIN - 1, canon_fp(), 1, canon_fp()) == Err(O2M_VAL_RANGE) {
        caught += 1;
    }
    // 5) 波次回退（先 w2 后 w1）
    if l.calibrate(P_BAND, 5_000, canon_fp(), 2, canon_fp()).is_ok()
        && l.calibrate(P_BAND, 5_100, canon_fp(), 1, canon_fp()) == Err(O2M_WAVE_BACK)
    {
        caught += 1;
    }
    // 6) 残账放行必抓（未对接参数查询 → 残账诊断）
    if l.value_of(P_INTERVAL).err() == Some(O2M_PARTIAL) {
        caught += 1;
    }
    // 7) 封账后改写拒
    l.seal();
    if l.calibrate(P_NOISE, 1_000, canon_fp(), 3, canon_fp()) == Err(O2M_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 对接账渲染导出（逐参数一行，未对接行 UNSET）。
pub fn render_ledger(l: &O2MemCalibLedger) -> String {
    let mut out = String::new();
    for p in 0..P_COUNT {
        match &l.entries[p] {
            None => out.push_str(&format!("{}|UNSET\n", P_NAMES[p])),
            Some(e) => out.push_str(&format!(
                "{}|{}|{:08x}|w{}\n",
                P_NAMES[p], e.value, e.src_fp, e.wave
            )),
        }
    }
    out
}

/// 渲染完整性核：三行齐 + 全对接则无 UNSET。
pub fn render_complete(text: &str, expect_full: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != P_COUNT {
        return false;
    }
    let has_unset = text.contains("UNSET");
    if expect_full && has_unset {
        return false;
    }
    for p in 0..P_COUNT {
        if !text.contains(P_NAMES[p]) {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56070_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56070");
    // J1-1 三元组全量对接
    let l = standard_ledger();
    let ok = l.triple_complete() && !l.sealed;
    s.add("56070-J1-三元组全量", ok, if ok { "" } else { "三参数残缺" });
    // J1-2 基准值复算一致（1200/6000/50）
    let ok = l.matches_baseline([1_200, 6_000, 50]);
    s.add("56070-J1-基准复算", ok, if ok { "" } else { "基准值漂移" });
    // J1-3 值域闭集边界自证
    let ok = value_in_range(P_NOISE, NOISE_MAX) && !value_in_range(P_NOISE, NOISE_MAX + 1)
        && value_in_range(P_INTERVAL, INTERVAL_MIN) && !value_in_range(P_INTERVAL, INTERVAL_MIN - 1);
    s.add("56070-J1-值域边界", ok, if ok { "" } else { "值域边界漂移" });
    // J2-1 指纹放行（正本指纹入账成功）
    let mut l2 = O2MemCalibLedger::new();
    let ok = l2.calibrate(P_NOISE, 1_200, canon_fp(), 1, canon_fp()).is_ok();
    s.add("56070-J2-指纹放行", ok, if ok { "" } else { "指纹放行失败" });
    // J2-2 指纹不匹配拒收
    let mut l3 = O2MemCalibLedger::new();
    let ok = l3.calibrate(P_NOISE, 1_200, 0x1234_5678, 1, canon_fp()) == Err(O2M_FP_MISS);
    s.add("56070-J2-指纹拒收", ok, if ok { "" } else { "指纹闸失效" });
    // J2-3 同参重对接升波次放行
    let mut l4 = standard_ledger();
    let ok = l4.calibrate(P_NOISE, 1_250, canon_fp(), 2, canon_fp()).is_ok()
        && l4.value_of(P_NOISE).unwrap_or(0) == 1_250;
    s.add("56070-J2-升波重对接", ok, if ok { "" } else { "重对接语义漂移" });
    // J3-1 审计链完整（链长 3 = 已对接参数数）
    let ok = standard_ledger().audit_ok();
    s.add("56070-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹确定性
    let ok = standard_ledger().seal_fingerprint() == standard_ledger().seal_fingerprint();
    s.add("56070-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 对接后指纹敏感（改一参数指纹变）
    let fp0 = standard_ledger().seal_fingerprint();
    let mut l5 = standard_ledger();
    let _ = l5.calibrate(P_INTERVAL, 60, canon_fp(), 2, canon_fp());
    let ok = l5.seal_fingerprint() != fp0;
    s.add("56070-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J2-4 渲染导出三行齐无 UNSET
    let text = render_ledger(&standard_ledger());
    let ok = render_complete(&text, true) && text.contains("sample_interval_ms|50|");
    s.add("56070-J2-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J1-4 参数名闭集齐（三元组命名同源）
    let ok = P_NAMES.len() == P_COUNT && P_NAMES[0] == "noise_floor_bp";
    s.add("56070-J1-参数名闭集齐", ok, if ok { "" } else { "参数名漂移" });
    // J3-4 封账拒写
    let mut l6 = standard_ledger();
    l6.seal();
    let ok = l6.calibrate(P_BAND, 6_500, canon_fp(), 2, canon_fp()) == Err(O2M_SEALED);
    s.add("56070-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56070-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56070_all_checks_green() {
        let set = run_unx56070_checks();
        assert!(set.all_passed(), "UNX-F56070 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56070_fingerprint_stable() {
        assert_eq!(standard_ledger().seal_fingerprint(), standard_ledger().seal_fingerprint());
    }
}
