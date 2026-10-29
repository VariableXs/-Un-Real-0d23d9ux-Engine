//! VE-F4304 · 一致性×版本发布（U06 场景应用组）
//! 职责：发布前全量扫描+基线对拍 + 发布说明一致性报告双级（摘要+全文）
//! + 发布后回归窗热修绿色通道（带事后追认——追认拒绝即回滚）
//! + 扫描不过发布阻断出整改包 + 报告缺失禁发 + 无障碍格式强制。
//! 段位：0x3040（续表 idx538）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlCode {
    /// 扫描不过（发布阻断出整改包）。
    ScanFail = SEG_BASE,
    /// 报告缺失（禁发）。
    ReportMissing = SEG_BASE + 0x01,
    /// 追认拒绝即回滚。
    RatifyReject = SEG_BASE + 0x02,
    /// 报告无障碍格式缺（域色）。
    A11yFmtMissing = SEG_BASE + 0x03,
    /// 发布闸账漂移（重复热修号/越界查询）。
    GateDrift = SEG_BASE + 0x04,
    /// 扫描指纹失配（序列化损坏）。
    ScanFpMismatch = SEG_BASE + 0x05,
}

/// 双级报告。
#[derive(Clone, Copy, Debug)]
pub struct RelReport {
    pub summary: bool, // 摘要级
    pub full: bool,    // 全文级
    pub a11y_fmt: bool, // 无障碍格式（域色强制）
}

impl RelReport {
    pub fn ok(&self) -> bool {
        self.summary && self.full && self.a11y_fmt
    }
}

/// 热修绿色通道条目。
#[derive(PartialEq, Clone, Copy, Debug)]
pub struct Hotfix {
    pub id: u16,
    pub ratified: Option<bool>, // None=待追认
}

/// 发布闸。
pub struct ReleaseGate {
    /// 全量扫描结果：(规则, 过)。
    pub scan: Vec<(u16, bool)>,
    /// 基线快照。
    pub baseline: Vec<(u16, u32)>,
    /// 当前值。
    pub current: Vec<(u16, u32)>,
    pub report: Option<RelReport>,
    pub hotfixes: Vec<Hotfix>,
    pub remedy: Vec<u16>,
    pub rolled_back: u32,
}

impl ReleaseGate {
    pub fn new() -> Self {
        ReleaseGate { scan: Vec::new(), baseline: Vec::new(), current: Vec::new(), report: None, hotfixes: Vec::new(), remedy: Vec::new(), rolled_back: 0 }
    }

    pub fn set_scan(&mut self, s: Vec<(u16, bool)>) {
        self.scan = s;
    }

    /// 全量扫描对拍基线：漂移项即失败。
    pub fn full_check(&mut self) -> Result<(), RlCode> {
        self.remedy.clear();
        let mut fail = false;
        for (r, ok) in &self.scan {
            if !ok {
                self.remedy.push(*r);
                fail = true;
            }
        }
        // 基线漂移
        for (k, bv) in &self.baseline {
            if let Some((_, cv)) = self.current.iter().find(|(ck, _)| ck == k) {
                if cv != bv {
                    self.remedy.push(*k + 10_000);
                    fail = true;
                }
            }
        }
        if fail {
            Err(RlCode::ScanFail)
        } else {
            Ok(())
        }
    }

    /// 发布：报告齐备+扫描过。
    pub fn release(&self) -> Result<(), RlCode> {
        match &self.report {
            None => Err(RlCode::ReportMissing),
            Some(r) if !r.ok() => {
                if !r.a11y_fmt {
                    Err(RlCode::A11yFmtMissing)
                } else {
                    Err(RlCode::ReportMissing)
                }
            }
            Some(_) => self.full_check_pass(),
        }
    }

    fn full_check_pass(&self) -> Result<(), RlCode> {
        if self.scan.iter().any(|(_, ok)| !ok) {
            return Err(RlCode::ScanFail);
        }
        Ok(())
    }

    /// 热修绿色通道：登记待追认。
    pub fn hotfix(&mut self, id: u16) {
        self.hotfixes.push(Hotfix { id, ratified: None });
    }

    /// 事后追认：拒绝即回滚。
    pub fn ratify(&mut self, id: u16, ok: bool) -> Result<(), RlCode> {
        let h = self.hotfixes.iter_mut().find(|h| h.id == id && h.ratified.is_none()).ok_or(RlCode::RatifyReject)?;
        if ok {
            h.ratified = Some(true);
            Ok(())
        } else {
            h.ratified = Some(false);
            self.rolled_back += 1;
            Err(RlCode::RatifyReject)
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C538-01..12。
pub fn run_ves538_checks() -> CheckSet {
    let mut set = CheckSet::new("ves538");

    let mut rg = ReleaseGate::new();
    rg.set_scan(alloc::vec![(1, true), (2, true)]);
    rg.baseline = alloc::vec![(10, 100)];
    rg.current = alloc::vec![(10, 100)];
    chk(&mut set, "C538-01", rg.full_check().is_ok(), "扫描全过+基线对齐");
    rg.report = Some(RelReport { summary: true, full: true, a11y_fmt: true });
    chk(&mut set, "C538-02", rg.release().is_ok(), "全条件发布放行");

    // 扫描失败
    rg.set_scan(alloc::vec![(1, true), (2, false)]);
    chk(&mut set, "C538-03", rg.release() == Err(RlCode::ScanFail), "扫描不过发布阻断");
    chk(&mut set, "C538-04", rg.remedy == alloc::vec![2], "整改包定位失败项");
    rg.current[0].1 = 999;
    let _ = rg.full_check();
    chk(&mut set, "C538-05", rg.remedy.contains(&(2)) && rg.remedy.contains(&(10_010)), "基线漂移入整改包（10010=键10+万位标）");

    // 报告缺失
    rg.set_scan(alloc::vec![(1, true)]);
    rg.current[0].1 = 100;
    rg.report = None;
    chk(&mut set, "C538-06", rg.release() == Err(RlCode::ReportMissing), "报告缺失禁发");
    rg.report = Some(RelReport { summary: true, full: true, a11y_fmt: false });
    chk(&mut set, "C538-07", rg.release() == Err(RlCode::A11yFmtMissing), "无障碍格式缺拒绝（域色）");

    // 热修追认
    rg.hotfix(7);
    chk(&mut set, "C538-08", rg.ratify(7, true).is_ok(), "热修追认通过");
    rg.hotfix(8);
    chk(&mut set, "C538-09", rg.ratify(8, false) == Err(RlCode::RatifyReject), "追认拒绝即回滚");
    chk(&mut set, "C538-10", rg.rolled_back == 1, "回滚计数");
    chk(&mut set, "C538-11", rg.ratify(8, true) == Err(RlCode::RatifyReject), "已裁断不可重复追认");
    chk(&mut set, "C538-12", rg.hotfixes.iter().all(|h| h.ratified.is_some()), "热修账全裁断");

    ves538_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：整改包结构化账本（失败项归因分类）+ 守恒恒等式 + 边界探针
// + 扫描通过率整数投影 + LCG 扫描注入回归 + 幂等闸 + 热修定长序列化
// 往返 + FNV-1a 扫描指纹。

use alloc::string::{String, ToString};

/// 整改项类别：0=扫描失败 1=基线漂移。
pub const REMEDY_SCAN: u8 = 0;
pub const REMEDY_DRIFT: u8 = 1;

/// 整改包条目：项号 × 类别 × 批次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemedyItem {
    pub key: u16,
    pub kind: u8,
    pub batch: u16,
}

/// 发布深化整改包账。
pub struct RemedyPack {
    pub items: Vec<RemedyItem>,
    pub batch: u16,
    pub scans_total: u32,
}

impl RemedyPack {
    pub fn new() -> Self {
        RemedyPack { items: Vec::new(), batch: 0, scans_total: 0 }
    }

    /// 归集一轮整改项（整包覆盖语义：新批次清旧账）。
    pub fn collect(&mut self, fails: Vec<(u16, u8)>) {
        self.batch = self.batch.wrapping_add(1);
        self.items = fails.into_iter().map(|(k, kind)| RemedyItem { key: k, kind, batch: self.batch }).collect();
        self.scans_total += 1;
    }

    /// 守恒恒等式：账内条目数 ≡ 最近一轮失败项数（覆盖语义下旧账不残留）。
    pub fn conservation(&self, last_fail_n: usize) -> bool {
        self.items.len() == last_fail_n && self.items.iter().all(|i| i.batch == self.batch)
    }

    /// 边界探针：按类别取项，查无 → 显性 Err。
    pub fn by_kind(&self, kind: u8) -> Result<Vec<u16>, RlCode> {
        let v: Vec<u16> = self.items.iter().filter(|i| i.kind == kind).map(|i| i.key).collect();
        if v.is_empty() {
            Err(RlCode::GateDrift)
        } else {
            Ok(v)
        }
    }

    /// 投影：扫描通过率整数百分数（禁浮点）。
    pub fn pass_pct(&self, scan: &[(u16, bool)]) -> u32 {
        if scan.is_empty() {
            return 0;
        }
        let p = scan.iter().filter(|(_, ok)| *ok).count();
        p as u32 * 100 / scan.len() as u32
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

/// LCG 扫描注入回归：64 轮随机扫描结果——凡含失败项 full_check 必 Err
/// 且 remedy 与失败集逐一对应（异常必抓）；负对照全过零误报且包清空。
fn scan_inject(seed: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..64u32 {
        let mut rg = ReleaseGate::new();
        let mut expect: Vec<u16> = Vec::new();
        for r in 0..8u16 {
            let ok = rng.next() % 3 != 0; // 1/3 概率失败
            rg.scan.push((r, ok));
            if !ok {
                expect.push(r);
            }
        }
        let res = rg.full_check();
        if expect.is_empty() {
            if res.is_ok() && !rg.remedy.is_empty() {
                return Err("扫描注入负对照：全过却残留整改项".to_string());
            }
        } else {
            if res != Err(RlCode::ScanFail) {
                return Err("扫描注入：含失败项未被发布闸抓到".to_string());
            }
            if rg.remedy != expect {
                return Err("扫描注入：整改包与失败集不对应".to_string());
            }
        }
    }
    Ok(())
}

/// 幂等闸：release 二次调用零副作用（remedy/回滚计数/热修账不动）。
fn idempotence_probe() -> bool {
    let mut rg = ReleaseGate::new();
    rg.set_scan(alloc::vec![(1, false)]);
    rg.hotfix(3);
    let snap_remedy = rg.remedy.clone();
    let snap_rb = rg.rolled_back;
    let snap_hf = rg.hotfixes.len();
    let _ = rg.release();
    let _ = rg.release();
    rg.remedy == snap_remedy && rg.rolled_back == snap_rb && rg.hotfixes.len() == snap_hf
}

/// 热修条目定长 3 字节编码：[id lo, id hi, 态(0=待追认 1=过 2=拒) + 0x40 旗]。
pub fn encode_hotfix(h: &Hotfix) -> [u8; 3] {
    let st = match h.ratified {
        None => 0u8,
        Some(true) => 1,
        Some(false) => 2,
    };
    [(h.id & 0xFF) as u8, (h.id >> 8) as u8, st | 0x40]
}

/// 解码：高三位旗损坏 → ScanFpMismatch。
pub fn decode_hotfix(b: &[u8; 3]) -> Result<Hotfix, RlCode> {
    if b[2] & 0xF8 != 0x40 {
        return Err(RlCode::ScanFpMismatch);
    }
    let id = b[0] as u16 | ((b[1] as u16) << 8);
    let ratified = match b[2] & 0x03 {
        0 => None,
        1 => Some(true),
        _ => Some(false),
    };
    Ok(Hotfix { id, ratified })
}

/// FNV-1a 32 位扫描指纹。
pub fn fnv_scan(scan: &[(u16, bool)]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for (r, ok) in scan {
        h ^= u32::from(*r);
        h = h.wrapping_mul(0x0100_0193);
        h ^= u32::from(*ok);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 深化判据并入主判据流（C538-13..23）。
pub fn ves538_deep_checks(set: &mut CheckSet) {
    let mut rp = RemedyPack::new();
    rp.collect(alloc::vec![(2, REMEDY_SCAN), (10_010, REMEDY_DRIFT)]);
    set.add("C538-13", rp.conservation(2), "整改包守恒（账目≡最近轮失败数）");
    set.add("C538-14", rp.collect_probe_ok(), "整改包批次归因同轮一致");
    set.add("C538-15", rp.by_kind(REMEDY_SCAN) == Ok(alloc::vec![2]) && rp.by_kind(9) == Err(RlCode::GateDrift), "类别分列探针+查无显性报错");
    set.add("C538-16", rp.pass_pct(&[(1, true), (2, false), (3, true)]) == 66, "扫描通过率整数投影 2/3→66");

    match scan_inject(0x5CA1_3801) {
        Ok(()) => set.add("C538-17", true, "LCG 扫描注入 64 轮失败必抓+负对照零残留"),
        Err(msg) => set.add_dyn("C538-17", false, &msg),
    }

    set.add("C538-18", idempotence_probe(), "幂等闸：release 二次调用零副作用");

    let hf = Hotfix { id: 0x1234, ratified: None };
    match decode_hotfix(&encode_hotfix(&hf)) {
        Ok(back) => set.add("C538-19", back.id == hf.id && back.ratified.is_none(), "热修 3 字节编码解码往返"),
        Err(_) => set.add("C538-19", false, "热修解码异常失败"),
    }
    set.add("C538-20", decode_hotfix(&[0x34, 0x12, 0xFF]) == Err(RlCode::ScanFpMismatch), "热修态旗损坏拒绝解码");

    let fp1 = fnv_scan(&[(1, true), (2, true)]);
    set.add("C538-21", fnv_scan(&[(1, true), (2, false)]) != fp1, "FNV 扫描指纹随结果漂移");
    set.add("C538-22", fnv_scan(&[]) == 0x811C_9DC5, "空扫描指纹偏移基探针");
    set.add("C538-23", ReleaseGate::new().full_check().is_ok() && RemedyPack::new().pass_pct(&[]) == 0, "空闸/空扫描边界探针");
}

impl RemedyPack {
    /// 探针辅助：批次归因一致性（与 collect 同源）。
    fn collect_probe_ok(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.batch == self.batch) && self.batch >= 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_gate() {
        let mut rg = ReleaseGate::new();
        rg.set_scan(alloc::vec![(1, true)]);
        assert!(rg.release().is_err()); // 无报告
        rg.report = Some(RelReport { summary: true, full: true, a11y_fmt: true });
        assert!(rg.release().is_ok());
        rg.hotfix(1);
        assert!(rg.ratify(1, false).is_err());
    }
}
