//! UNX-F56059 启动计时器 O2 标定对接件（unxstar 段）
//!
//! [J1] 标定对接：启动计时器的每段原始 tick 必须经 O2 标定因子（频率档位 MHz，
//!      FNV 校验的标定记录）换算为 ns，换算结果与外部实测 ns 的漂移须 ≤ 500 万分比。
//! [J2] 因子纪律：频率档位走闭集（3600/3300/4000 MHz）；非法档位拒绝；
//!      标定记录指纹同内容稳定、异内容必异。
//! [J3] O2 预链 admit：五段全过漂移闸且标定因子锁定后才可签发 O2 预链就绪指纹；
//!      注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（档位未知/tick 为零/外部为零/漂移超限/段缺失/指纹漂移/预链未就绪）
//!   纯函数：fnv32、档位校验、万分比漂移、tick→ns 换算
//!   CalibRow：单段对接条目，new() 守闸
//!   O2CalibBook：对接账本（put/lock_factor/admit_o2/drift_rows）
//!   injection_core：5 例必抓
//!   run_unx56059_checks：≥12 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60740;

/// 标定档位未知（不在三档闭集）
pub const D_FACTOR_UNKNOWN: u32 = SEG_BASE + 0x01;
/// 原始 tick 为零
pub const D_TICK_ZERO: u32 = SEG_BASE + 0x02;
/// 外部实测为零
pub const D_EXT_ZERO: u32 = SEG_BASE + 0x03;
/// 漂移超限（>500pbp）
pub const D_DRIFT_OVER: u32 = SEG_BASE + 0x04;
/// 段缺失（段号越界或重复落账）
pub const D_SEG_MISSING: u32 = SEG_BASE + 0x05;
/// 指纹漂移（同内容两次折叠不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// O2 预链未就绪（因子未锁定或漂移闸未全过或段缺失）
pub const D_NOT_READY: u32 = SEG_BASE + 0x07;

/// 频率档位闭集（MHz）
pub const FACTOR_STEPS: [u64; 3] = [3600, 3300, 4000];
/// 漂移闸（万分比）
pub const DRIFT_GATE: u64 = 500;

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 档位校验
pub fn factor_ok(factor: u64) -> bool {
    FACTOR_STEPS.contains(&factor)
}

/// 万分比漂移
pub fn drift_pbp(a: u64, b: u64) -> u64 {
    let d = if a > b { a - b } else { b - a };
    let base = if a > b { a } else { b };
    if base == 0 {
        0
    } else {
        (d * 10_000) / base
    }
}

/// tick→ns 换算：ticks_million 为真实 tick 数（以百万计），
/// ns = ticks_million * 1e6 / (factor_mhz * 1e6) * 1e9 = ticks_million * 1e9 / factor_mhz。
pub fn ticks_to_ns(ticks_million: u64, factor_mhz: u64) -> u64 {
    ticks_million * 1_000_000_000 / factor_mhz
}

/// 标定记录指纹：因子 + 段号 + tick
pub fn calib_fp(seg_id: u32, ticks_million: u64, factor_mhz: u64) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&factor_mhz.to_le_bytes());
    buf.extend_from_slice(&seg_id.to_le_bytes());
    buf.extend_from_slice(&ticks_million.to_le_bytes());
    fnv32(&buf)
}

/// 单段对接条目
#[derive(Clone)]
pub struct CalibRow {
    pub seg_id: u32,
    pub ticks_million: u64,
    pub factor_mhz: u64,
    pub external_ns: u64,
}

impl CalibRow {
    /// 守闸：档位闭集、tick/外部非零
    pub fn new(seg_id: u32, ticks_million: u64, factor_mhz: u64, external_ns: u64) -> Result<Self, u32> {
        if !factor_ok(factor_mhz) {
            return Err(D_FACTOR_UNKNOWN);
        }
        if ticks_million == 0 {
            return Err(D_TICK_ZERO);
        }
        if external_ns == 0 {
            return Err(D_EXT_ZERO);
        }
        Ok(CalibRow { seg_id, ticks_million, factor_mhz, external_ns })
    }

    /// 换算 ns
    pub fn converted_ns(&self) -> u64 {
        ticks_to_ns(self.ticks_million, self.factor_mhz)
    }

    /// 换算 vs 外部漂移（万分比）
    pub fn drift(&self) -> u64 {
        drift_pbp(self.converted_ns(), self.external_ns)
    }

    pub fn fp(&self) -> u32 {
        calib_fp(self.seg_id, self.ticks_million, self.factor_mhz)
    }
}

/// O2 标定对接账本
pub struct O2CalibBook {
    pub book_name: String,
    pub factor_mhz: u64,
    rows: Vec<CalibRow>,
    factor_locked: bool,
}

impl O2CalibBook {
    pub fn new(book_name: &str, factor_mhz: u64) -> Result<Self, u32> {
        if !factor_ok(factor_mhz) {
            return Err(D_FACTOR_UNKNOWN);
        }
        Ok(O2CalibBook {
            book_name: book_name.to_string(),
            factor_mhz,
            rows: Vec::new(),
            factor_locked: false,
        })
    }

    /// 落账一条；段号闭集 + 重复段拒绝
    pub fn put(&mut self, r: CalibRow) -> Result<u32, u32> {
        if r.seg_id < 1 || r.seg_id > 5 {
            return Err(D_SEG_MISSING);
        }
        if self.rows.iter().any(|x| x.seg_id == r.seg_id) {
            return Err(D_SEG_MISSING);
        }
        let fp = r.fp();
        self.rows.push(r);
        Ok(fp)
    }

    pub fn lock_factor(&mut self) {
        self.factor_locked = true;
    }

    pub fn is_locked(&self) -> bool {
        self.factor_locked
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 五段齐备
    pub fn all_present(&self) -> bool {
        (1..=5).all(|id| self.rows.iter().any(|r| r.seg_id == id))
    }

    /// 漂移闸：全段过闸
    pub fn drift_gate_ok(&self) -> bool {
        self.rows.iter().all(|r| r.drift() <= DRIFT_GATE)
    }

    /// O2 预链就绪指纹：五段齐 + 因子锁定 + 漂移闸全过
    pub fn admit_o2(&self) -> Result<u32, u32> {
        if !self.all_present() || !self.factor_locked || !self.drift_gate_ok() {
            return Err(D_NOT_READY);
        }
        let mut acc: u32 = 0x56059_00;
        for r in &self.rows {
            acc = acc.wrapping_mul(16777619) ^ r.fp();
        }
        Ok(acc)
    }

    /// 漂移行（机器可读）
    pub fn drift_rows(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|r| {
                format!(
                    "O2CAL|{}|{}|{}|{}",
                    r.seg_id, r.converted_ns(), r.external_ns, r.drift()
                )
            })
            .collect()
    }
}

/// 标准夹具：3600MHz 档，段时长 10/20/50/100/250ms，外部实测 = 换算值（漂移 0）。
/// tick 百万数 = 时长 ms * 3.6（3600MHz 下每 ms 3.6e6 tick → 每百万 tick 组 = 3.6/ms 组）：
/// 10ms→36、20ms→72、50ms→180、100ms→360、250ms→900，均为整数。
pub fn fixture_book() -> O2CalibBook {
    let mut b = O2CalibBook::new("ai71.5-o2cal", 3600).expect("book");
    let plan: [(u32, u64, u64); 5] = [
        (1, 36, 10_000_000),
        (2, 72, 20_000_000),
        (3, 180, 50_000_000),
        (4, 360, 100_000_000),
        (5, 900, 250_000_000),
    ];
    for (id, tm, ns) in plan {
        let r = CalibRow::new(id, tm, 3600, ns).expect("row");
        b.put(r).expect("put");
    }
    b
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 档位未知
    if CalibRow::new(1, 36, 3500, 10_000_000).err() == Some(D_FACTOR_UNKNOWN) {
        caught += 1;
    }

    // 2) tick 为零
    if CalibRow::new(1, 0, 3600, 1).err() == Some(D_TICK_ZERO) {
        caught += 1;
    }

    // 3) 重复段落账
    let mut b = fixture_book();
    let dup = CalibRow::new(3, 180, 3600, 50_000_000).expect("dup");
    if b.put(dup) == Err(D_SEG_MISSING) {
        caught += 1;
    }

    // 4) 漂移超限：换算 50ms vs 外部 100ms → 5000pbp
    let bad = CalibRow::new(6, 180, 3600, 100_000_000).expect("bad");
    if bad.drift() == 5000 && bad.drift() > DRIFT_GATE {
        caught += 1;
    }

    // 5) 因子未锁定即 admit
    let c = fixture_book();
    if c.admit_o2() == Err(D_NOT_READY) {
        caught += 1;
    }

    (caught, total)
}

/// 指纹稳定性：同内容两次折叠一致，异内容必异
pub fn fp_stability() -> Result<(), u32> {
    let a = calib_fp(1, 36, 3600);
    let b = calib_fp(1, 36, 3600);
    if a != b {
        return Err(D_FP_DRIFT);
    }
    if a == calib_fp(2, 36, 3600) {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 判据集
pub fn run_unx56059_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56059");

    // J1 标定对接
    let mut b = fixture_book();
    s.add("56059-J1-五段落账", b.len() == 5, "对接账本段数不为 5");
    s.add("56059-J1-换算一致", b.rows[0].converted_ns() == 10_000_000, "段 1 换算 ns 不为 10e6");
    s.add("56059-J1-漂移零", b.rows.iter().all(|r| r.drift() == 0), "夹具存在非零漂移");
    s.add("56059-J1-漂移行数", b.drift_rows().len() == 5, "漂移行数不为 5");

    // J2 因子纪律
    s.add("56059-J2-档位闭集", factor_ok(4000) && !factor_ok(3500), "档位闭集判断异常");
    s.add("56059-J2-非法档位建账", O2CalibBook::new("x", 3800).err() == Some(D_FACTOR_UNKNOWN), "非法档位未被拒绝");
    s.add("56059-J2-指纹稳定", fp_stability().is_ok(), "标定指纹漂移");
    s.add("56059-J2-漂移闸全过", b.drift_gate_ok(), "漂移闸未全过");

    // J3 O2 预链 admit
    s.add("56059-J3-未锁定拒admit", b.admit_o2() == Err(D_NOT_READY), "因子未锁定 admit 未被拒绝");
    b.lock_factor();
    let fp1 = b.admit_o2().expect("admit");
    s.add("56059-J3-锁定后admit成功", b.is_locked(), "锁定标志未置位");
    let mut c = fixture_book();
    c.lock_factor();
    s.add("56059-J3-admit指纹一致", c.admit_o2() == Ok(fp1), "同内容两次 admit 指纹不一致");
    s.add("56059-J3-错档换算超闸", {
        let bad = CalibRow::new(5, 900, 4000, 250_000_000).expect("bad");
        // 换算 = 900e9/4000 = 225e6；外部 250e6 → 漂移 25e6/25e7 = 1000pbp > 500
        bad.drift() == 1000 && bad.drift() > DRIFT_GATE
    }, "错档换算应超漂移闸");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56059-J1-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56059_all_checks_green() {
        let s = run_unx56059_checks();
        assert!(
            s.all_passed(),
            "UNX-F56059 有红项: {}",
            s.red_items_text()
        );
    }
}
