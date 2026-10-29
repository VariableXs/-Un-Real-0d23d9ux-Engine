//! UNX-F56039 O2 噪声标定前置对接件（0x604C0 段）
//!
//! 锚点：UNX-F56039-J1 采集器送 O2 标定的接口就绪（噪声底 × 漂移带 ×
//! 采样窗三参数对接账逐项落账）；J2 噪声超出信号 10% 的样本作废规则
//! 承接（万分比 >1000 即作废留痕）；J3 标定状态未过→样本禁入账的门
//! 闸 ×1 演练 + 联签锚定行 AI-72 三判据全绿。判据锚定 §AI-71.2 专题
//! 二"先在 O2 通过噪声探针标定"。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·O2 前置对接面）。
//!
//! 结构：标定状态闭集（待标/已过/未过三态）+ 三参数对接账（噪声底 ×
//! 漂移带 × 采样窗逐项落账）+ 作废规则核（噪声/信号万分比 >1000 作废
//! 带痕）+ 入账门闸（未过标定禁入账 ×1 演练）+ AI-72 联签锚定行 +
//! 注入核（越闭集状态/零信号/作废未留痕/未过标定入账/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x604C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x604C0;
pub const OL_STAT_UNK: u32 = SEG_BASE + 0x01; // 越闭集标定状态
pub const OL_SIGNAL_ZERO: u32 = SEG_BASE + 0x02; // 零信号（噪声比除零守闸）
pub const OL_VOID_LOST: u32 = SEG_BASE + 0x03; // 作废未留痕
pub const OL_ADMIT_BLOCKED: u32 = SEG_BASE + 0x04; // 未过标定入账被拦
pub const OL_SEALED: u32 = SEG_BASE + 0x05; // 对接账已封账拒写
pub const OL_PARAM_MISS: u32 = SEG_BASE + 0x06; // 三参数缺项
pub const OL_ANCHOR_MISS: u32 = SEG_BASE + 0x07; // AI-72 联签锚定行缺

/// 标定状态闭集。
pub const ST_PENDING: usize = 0;
pub const ST_PASSED: usize = 1;
pub const ST_FAILED: usize = 2;
pub const ST_COUNT: usize = 3;
pub const ST_NAMES: [&str; ST_COUNT] = ["pending", "passed", "failed"];

/// 作废阈值（噪声/信号 万分比 10% = 1000）。
pub const VOID_PBP_MAX: u32 = 1_000;

/// O2 三参数对接账（噪声底/漂移带/采样窗，帧率域口径迁移）。
pub const O2_PARAM_NAMES: [&str; 3] = ["noise_floor", "drift_band", "sample_window_ms"];

/// 噪声/信号 万分比。
pub fn noise_ratio_pbp(noise: u64, signal: u64) -> u32 {
    if signal == 0 {
        return u32::MAX;
    }
    ((noise * 10_000) / signal) as u32
}

/// 样本作废判定（>10% 作废）。
pub fn sample_void(noise: u64, signal: u64) -> bool {
    noise_ratio_pbp(noise, signal) > VOID_PBP_MAX
}

/// O2 前置对接器。
pub struct O2Prelink {
    pub status: usize,
    pub params: [Option<u32>; 3],
    pub voids: Vec<(u64, u64, u32)>,
    pub admitted: usize,
    pub anchors: Vec<String>,
    pub sealed: bool,
}

impl O2Prelink {
    pub fn new(status: usize) -> Result<Self, u32> {
        if status >= ST_COUNT {
            return Err(OL_STAT_UNK);
        }
        Ok(O2Prelink {
            status,
            params: [None, None, None],
            voids: Vec::new(),
            admitted: 0,
            anchors: Vec::new(),
            sealed: false,
        })
    }

    /// 三参数落账（逐项，封账拒）。
    pub fn set_param(&mut self, idx: usize, val: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if idx >= 3 {
            return Err(OL_PARAM_MISS);
        }
        self.params[idx] = Some(val);
        Ok(())
    }

    /// J1：接口就绪核（三参数齐 + 状态已过）。
    pub fn ready(&self) -> bool {
        self.params.iter().all(|p| p.is_some()) && self.status == ST_PASSED
    }

    /// AI-72 联签锚定行落账。
    pub fn anchor(&mut self, row: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if row.is_empty() {
            return Err(OL_ANCHOR_MISS);
        }
        self.anchors.push(row.to_string());
        Ok(())
    }

    /// AI-72 锚定行在账核。
    pub fn anchor_ok(&self) -> bool {
        self.anchors.iter().any(|a| a.contains("AI-72"))
    }

    /// J2：样本过闸（作废规则承接 + 标定门闸）。
    /// 返回 Ok(true)=入账 / Ok(false)=作废；未过标定 Err 拦截。
    pub fn admit(&mut self, noise: u64, signal: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if signal == 0 {
            return Err(OL_SIGNAL_ZERO);
        }
        if self.status != ST_PASSED {
            return Err(OL_ADMIT_BLOCKED);
        }
        if sample_void(noise, signal) {
            self.voids.push((noise, signal, noise_ratio_pbp(noise, signal)));
            return Ok(false);
        }
        self.admitted += 1;
        Ok(true)
    }

    /// J2'：作废留痕齐（作废带万分比）。
    pub fn void_trail_ok(&self) -> bool {
        self.voids.iter().all(|v| v.2 > VOID_PBP_MAX)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：状态已过 + 三参数齐 + AI-72 锚定行。
pub fn standard_prelink() -> O2Prelink {
    let mut o = O2Prelink::new(ST_PASSED).expect("o");
    o.set_param(0, 120).expect("p1");
    o.set_param(1, 800).expect("p2");
    o.set_param(2, 200).expect("p3");
    o.anchor("AI-72|O2 前置对接联签锚定行").expect("anchor");
    o
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集状态
    if O2Prelink::new(9).err() == Some(OL_STAT_UNK) {
        caught += 1;
    }
    // 2) 零信号
    let mut o = standard_prelink();
    if o.admit(50, 0) == Err(OL_SIGNAL_ZERO) {
        caught += 1;
    }
    // 3) 未过标定入账被拦
    let mut o2 = O2Prelink::new(ST_PENDING).expect("o2");
    if o2.admit(50, 1000) == Err(OL_ADMIT_BLOCKED) {
        caught += 1;
    }
    // 4) 作废留痕（15% 噪声作废带痕）
    let mut o3 = standard_prelink();
    let v = o3.admit(150, 1000).expect("admit");
    if !v && o3.voids.len() == 1 && o3.voids[0].2 == 1_500 && o3.void_trail_ok() {
        caught += 1;
    }
    // 5) 封账改写
    let mut o4 = standard_prelink();
    o4.seal();
    if o4.set_param(0, 1) == Err(OL_SEALED) && o4.admit(1, 100) == Err(OL_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56039");
    // J1-1 三参数对接账齐
    let o = standard_prelink();
    let mut params_ok = true;
    for p in o.params.iter() {
        if p.is_none() {
            params_ok = false;
        }
    }
    let ok = params_ok && o.ready();
    s.add("56039-J1-三参数对接齐", ok, if ok { "" } else { "参数缺" });
    // J1-2 标定状态闭集三态
    let ok = ST_NAMES[ST_PENDING] == "pending"
        && ST_NAMES[ST_PASSED] == "passed"
        && ST_NAMES[ST_FAILED] == "failed";
    s.add("56039-J1-状态闭集三态", ok, if ok { "" } else { "状态漂移" });
    // J1-3 接口就绪闸（未过标定 ready=false）
    let p = O2Prelink::new(ST_PENDING).expect("p");
    let ok = !p.ready();
    s.add("56039-J1-未过标定不就绪", ok, if ok { "" } else { "未过放行" });
    // J2-1 作废规则语义（12% 作废 / 9% 过闸）
    let ok = sample_void(120, 1_000) && !sample_void(90, 1_000)
        && noise_ratio_pbp(90, 1_000) == 900;
    s.add("56039-J2-作废规则语义", ok, if ok { "" } else { "作废口径漂移" });
    // J2-2 恰阈不过作废（10% 恰阈 = 1000 不作废）
    let ok = !sample_void(100, 1_000) && sample_void(101, 1_000);
    s.add("56039-J2-恰阈语义", ok, if ok { "" } else { "恰阈漂移" });
    // J2-3 ×1 演练：作废带痕 + 入账计数分明
    let mut o2 = standard_prelink();
    let a1 = o2.admit(50, 1_000).expect("a1");
    let a2 = o2.admit(150, 1_000).expect("a2");
    let ok = a1 && !a2 && o2.admitted == 1 && o2.voids.len() == 1 && o2.void_trail_ok();
    s.add("56039-J2-作废入账分明", ok, if ok { "" } else { "演练账混" });
    // J2-4 未过标定入账 ×1 演练被拦
    let mut o3 = O2Prelink::new(ST_FAILED).expect("o3");
    let ok = o3.admit(50, 1_000) == Err(OL_ADMIT_BLOCKED) && o3.admitted == 0;
    s.add("56039-J2-未过门闸演练", ok, if ok { "" } else { "门闸失效" });
    // J3-1 AI-72 联签锚定行在账
    let ok = standard_prelink().anchor_ok();
    s.add("56039-J3-AI-72锚定行", ok, if ok { "" } else { "锚定行缺" });
    // J3-2 空锚定行拒
    let mut o4 = O2Prelink::new(ST_PASSED).expect("o4");
    let ok = o4.anchor("") == Err(OL_ANCHOR_MISS);
    s.add("56039-J3-空锚定拒", ok, if ok { "" } else { "空锚定穿透" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56039-J3-注入核必抓", ok, &detail);
    // J1-4 O2 参数名闭集同源（三参数名与帧率域同源）
    let ok = O2_PARAM_NAMES[0] == "noise_floor" && O2_PARAM_NAMES[2] == "sample_window_ms";
    s.add("56039-J1-参数名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-5 零信号守闸
    let ok = noise_ratio_pbp(1, 0) == u32::MAX;
    s.add("56039-J2-零信号守闸", ok, if ok { "" } else { "除零穿透" });
    // J2-6 封账冻结
    let mut o5 = standard_prelink();
    o5.seal();
    let ok = o5.set_param(0, 9) == Err(OL_SEALED) && o5.anchor("x") == Err(OL_SEALED);
    s.add("56039-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56039_all_checks_green() {
        let set = run_unx56039_checks();
        assert!(set.all_passed(), "UNX-F56039 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56039_void_rule() {
        assert!(sample_void(150, 1_000));
        assert!(!sample_void(100, 1_000));
    }
}
