//! UNX-F56057 引导段观测零触碰自证件（unxstar 段）
//!
//! [J1] 观测零触碰：引导段各观测站对被观测面只允许 READ/SCAN 两类动作，
//!      任何 WRITE 动作立即拒绝并计入触碰；触碰计数必须为 0 才可签发自证件。
//! [J2] 自证可复核：自证件含站清单、动作分布与折叠指纹；同内容两次签发指纹
//!      必须一致；非零触碰签发一律拒绝（D_TOUCHED）。
//! [J3] 站闭集纪律：站名走五站闭集（firmware/bootloader/kernel/login/desktop 外加
//!      auditor），未知站拒绝登记；注入核 5 例必抓。
//!
//! 域：CoRun·UNX-F2-B40
//!
//! 结构总览：
//!   常量段：SEG_BASE + 7 个诊断码（未知站/未知动作/写入触碰/带触碰签发/证已存/指纹漂移/站清单不齐）
//!   纯函数：fnv32、动作指纹折叠
//!   TouchRec：单条观测动作记录
//!   TouchLedger：触碰账本（record/certify/分布统计）
//!   injection_core：5 例必抓
//!   run_unx56057_checks：≥13 条判据

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x60700;

/// 未知观测站
pub const D_STATION_UNKNOWN: u32 = SEG_BASE + 0x01;
/// 未知动作（不在 READ/SCAN/WRITE 闭集）
pub const D_ACTION_UNKNOWN: u32 = SEG_BASE + 0x02;
/// 写入触碰（WRITE 动作被拒绝记账）
pub const D_WRITE_TOUCH: u32 = SEG_BASE + 0x03;
/// 带触碰签发（touch_count > 0 时 certify 拒绝）
pub const D_TOUCHED: u32 = SEG_BASE + 0x04;
/// 自证件重复签发
pub const D_CERT_DUP: u32 = SEG_BASE + 0x05;
/// 指纹漂移（同内容两次签发指纹不一致）
pub const D_FP_DRIFT: u32 = SEG_BASE + 0x06;
/// 站清单不齐（五站未全部登记即试图签发）
pub const D_STATIONS_MISSING: u32 = SEG_BASE + 0x07;

/// 观测站闭集（六站：五个被观测站 + 审计站）
pub const STATIONS: [&str; 6] =
    ["firmware", "bootloader", "kernel", "login", "desktop", "auditor"];
/// 动作闭集
pub const ACTIONS: [&str; 3] = ["READ", "SCAN", "WRITE"];

fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 单条记录指纹
pub fn rec_fp(r: &TouchRec) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(r.station.as_bytes());
    buf.extend_from_slice(r.action.as_bytes());
    buf.extend_from_slice(&r.seq.to_le_bytes());
    fnv32(&buf)
}

/// 折叠全部记录指纹（顺序敏感）
pub fn fold_fps(fps: &[u32]) -> u32 {
    let mut acc: u32 = 0x56057_00;
    for &f in fps {
        acc = acc.wrapping_mul(16777619) ^ f;
    }
    acc
}

/// 单条观测动作记录
#[derive(Clone)]
pub struct TouchRec {
    pub station: String,
    pub action: String,
    pub seq: u64,
}

/// 触碰账本
pub struct TouchLedger {
    pub book_name: String,
    recs: Vec<TouchRec>,
    seq: u64,
    write_attempts: u64,
    cert_issued: Option<u32>,
}

impl TouchLedger {
    pub fn new(book_name: &str) -> Self {
        TouchLedger {
            book_name: book_name.to_string(),
            recs: Vec::new(),
            seq: 0,
            write_attempts: 0,
            cert_issued: None,
        }
    }

    /// 登记一次观测动作；WRITE 直接拒绝并计触碰，未知站/未知动作拒绝
    pub fn record(&mut self, station: &str, action: &str) -> Result<u64, u32> {
        if !STATIONS.contains(&station) {
            return Err(D_STATION_UNKNOWN);
        }
        if !ACTIONS.contains(&action) {
            return Err(D_ACTION_UNKNOWN);
        }
        if action == "WRITE" {
            self.write_attempts += 1;
            return Err(D_WRITE_TOUCH);
        }
        self.seq += 1;
        let rec = TouchRec {
            station: station.to_string(),
            action: action.to_string(),
            seq: self.seq,
        };
        let fp = rec_fp(&rec);
        self.recs.push(rec);
        Ok(fp as u64)
    }

    pub fn touch_count(&self) -> u64 {
        self.write_attempts
    }

    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }

    /// 五站（不含 auditor）是否全部有 READ/SCAN 记录
    pub fn stations_covered(&self) -> bool {
        STATIONS[..5].iter().all(|st| {
            self.recs
                .iter()
                .any(|r| &r.station == st)
        })
    }

    /// 动作分布 (read, scan)
    pub fn action_split(&self) -> (usize, usize) {
        let read = self.recs.iter().filter(|r| r.action == "READ").count();
        let scan = self.recs.iter().filter(|r| r.action == "SCAN").count();
        (read, scan)
    }

    /// 签发自证件；带触碰、站不齐、重复签发均拒绝
    pub fn certify(&mut self) -> Result<u32, u32> {
        if self.cert_issued.is_some() {
            return Err(D_CERT_DUP);
        }
        if self.write_attempts > 0 {
            return Err(D_TOUCHED);
        }
        if !self.stations_covered() {
            return Err(D_STATIONS_MISSING);
        }
        let fps: Vec<u32> = self.recs.iter().map(rec_fp).collect();
        let fp = fold_fps(&fps);
        self.cert_issued = Some(fp);
        Ok(fp)
    }

    pub fn cert_fingerprint(&self) -> Option<u32> {
        self.cert_issued
    }

    /// 自证件文档行
    pub fn cert_rows(&self) -> Vec<String> {
        self.recs
            .iter()
            .map(|r| {
                format!("TOUCH|{}|{}|{}", r.station, r.action, r.seq)
            })
            .collect()
    }
}

/// 标准夹具：六站各一次 READ/SCAN，零触碰
pub fn fixture_ledger() -> TouchLedger {
    let mut l = TouchLedger::new("ai71.5-zero-touch");
    let plan: [(&str, &str); 6] = [
        ("firmware", "READ"),
        ("bootloader", "SCAN"),
        ("kernel", "READ"),
        ("login", "SCAN"),
        ("desktop", "READ"),
        ("auditor", "SCAN"),
    ];
    for (st, act) in plan {
        l.record(st, act).expect("fixture record");
    }
    l
}

/// 指纹稳定性：同内容两次折叠必须一致
pub fn fp_stability() -> Result<(), u32> {
    let a = {
        let mut l = fixture_ledger();
        l.certify().map_err(|e| e)?
    };
    let b = {
        let mut l = fixture_ledger();
        l.certify().map_err(|e| e)?
    };
    if a != b {
        return Err(D_FP_DRIFT);
    }
    Ok(())
}

/// 触碰探针：一次 WRITE 后 certify 必须拒绝
pub fn touch_probe() -> Result<(), u32> {
    let mut l = fixture_ledger();
    let r = l.record("kernel", "WRITE");
    if r != Err(D_WRITE_TOUCH) {
        return Err(D_WRITE_TOUCH);
    }
    if l.certify() != Err(D_TOUCHED) {
        return Err(D_TOUCHED);
    }
    Ok(())
}

/// 注入核：5 例必抓
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 未知站
    let mut l = TouchLedger::new("inj1");
    if l.record("hypervisor", "READ") == Err(D_STATION_UNKNOWN) {
        caught += 1;
    }

    // 2) 未知动作
    let mut l2 = TouchLedger::new("inj2");
    if l2.record("kernel", "PUNCH") == Err(D_ACTION_UNKNOWN) {
        caught += 1;
    }

    // 3) 写入触碰
    if touch_probe().is_ok() {
        // touch_probe 返回 Ok 即代表未按预期拒绝，此处不计数
    } else {
        caught += 1;
    }

    // 4) 站不齐签发
    let mut l4 = TouchLedger::new("inj4");
    let _ = l4.record("firmware", "READ");
    if l4.certify() == Err(D_STATIONS_MISSING) {
        caught += 1;
    }

    // 5) 重复签发
    let mut l5 = fixture_ledger();
    let _ = l5.certify().expect("first cert");
    if l5.certify() == Err(D_CERT_DUP) {
        caught += 1;
    }

    (caught, total)
}

/// 判据集
pub fn run_unx56057_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56057");

    // J1 观测零触碰
    let mut l = fixture_ledger();
    s.add("56057-J1-零触碰", l.touch_count() == 0, "触碰计数非零");
    s.add("56057-J1-记录数", l.rec_count() == 6, "夹具记录数不为 6");
    let (read, scan) = l.action_split();
    s.add("56057-J1-动作分布", read == 3 && scan == 3, "READ/SCAN 分布异常");
    s.add("56057-J1-站清单覆盖", l.stations_covered(), "五站未全部覆盖");

    // J2 自证签发
    let fp = l.certify().expect("certify");
    s.add("56057-J2-签发成功", l.cert_fingerprint() == Some(fp), "签发后指纹回读不一致");
    s.add("56057-J2-重复签发拒绝", l.certify() == Err(D_CERT_DUP), "重复签发未被拒绝");
    s.add("56057-J2-证行数", l.cert_rows().len() == 6, "自证文档行数不为 6");
    s.add("56057-J2-行格式锚", l.cert_rows()[0].starts_with("TOUCH|firmware|READ|1"), "首行格式锚不符");

    // J2 带触碰签发拒绝
    s.add("56057-J2-带触碰拒绝", touch_probe().is_ok(), "带触碰签发未被拒绝");

    // J3 站闭集与指纹
    let mut u = TouchLedger::new("u");
    s.add("56057-J3-站闭集", u.record("smm", "READ") == Err(D_STATION_UNKNOWN), "未知站未被拒绝");
    s.add("56057-J3-动作闭集", u.record("kernel", "WRITE") == Err(D_WRITE_TOUCH), "WRITE 未被识别为触碰");
    s.add("56057-J3-指纹稳定", fp_stability().is_ok(), "同内容两次签发指纹漂移");
    s.add("56057-J3-空账签发拒", {
        let mut e = TouchLedger::new("empty");
        e.certify() == Err(D_STATIONS_MISSING)
    }, "空账签发未被拒绝");

    // 注入核
    let (caught, total) = injection_core();
    s.add_dyn(
        "56057-J1-注入必抓",
        caught == total,
        &format!("注入捕获 {}/{}", caught, total),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56057_all_checks_green() {
        let s = run_unx56057_checks();
        assert!(
            s.all_passed(),
            "UNX-F56057 有红项: {}",
            s.red_items_text()
        );
    }
}
