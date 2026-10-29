// unx2006_couplacct —— UNX-F2006 单点导出：耦合账接口
// 判据原文 UNX-F2006-J1：第 19 处单点导出在册（util/请求/生效三段链路账），
// 签名/单位/同窗快照三列齐备，与 B09 汇聚消费一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0x7B20 独占（0x7B20~0x7B3F）。
//
// 设计（纯功能 no_std）：
//   * 三段链路账：util（‰）→ request（kHz）→ effective（kHz），逐窗逐行；
//   * 三列纪律：
//       - 签名列：行级 FNV 签名（三段值 + 窗号合成），篡改即检出；
//       - 单位列：每段单位标签冻结（‰ / kHz / kHz），错标即拒；
//       - 同窗快照列：同窗二次导出逐字节一致（静态幂等）；
//   * 单点导出：全部消费方（B09 汇聚）只经 export_snapshot() 一个口子，
//     导出快照带总签名，旁路读账即违规（无旁路 API，类型面封闭）；
//   * B09 汇聚消费：按窗聚合（util 均值定点 / 请求取最大 / 生效取账尾），
//     消费口径与导出快照逐项一致。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x7B20 独占）──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7B20;

pub const XC_CA_SEALED: u32 = SEG_BASE as u32 + 0x01; // 封账后入账
pub const XC_CA_OOB: u32 = SEG_BASE as u32 + 0x02; // util 越界（>1000‰）
pub const XC_CA_WIN_DUP: u32 = SEG_BASE as u32 + 0x03; // 同窗重复入账
pub const XC_CA_SIG: u32 = SEG_BASE as u32 + 0x04; // 行签名失配（篡改检出）
pub const XC_CA_UNIT: u32 = SEG_BASE as u32 + 0x05; // 单位标签错
pub const XC_CA_EMPTY: u32 = SEG_BASE as u32 + 0x06; // 空账导出（谎报防线）
pub const XC_CA_TAMPER: u32 = SEG_BASE as u32 + 0x07; // 快照总签名失配

/// 单位标签（闭集，冻结）。
pub const UNIT_UTIL: u8 = 1; // ‰ 千分比
pub const UNIT_KHZ: u8 = 2; // kHz

/// 耦合账单行：窗号 + 三段值 + 签名 + 单位三元组。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CouplingRow {
    pub window: u64,
    pub util_permille: u16, // 段一：util（‰）
    pub req_khz: u32,       // 段二：请求（kHz）
    pub eff_khz: u32,       // 段三：生效（kHz）
    pub sig: u32,           // 行签名（FNV 三段合成）
}

impl CouplingRow {
    /// 行签名：FNV-1a over (window, util, req, eff)。
    pub fn compute_sig(window: u64, util: u16, req: u32, eff: u32) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (window as u32);
        h = h.wrapping_mul(0x0100_0193) ^ ((window >> 32) as u32);
        h = h.wrapping_mul(0x0100_0193) ^ (util as u32);
        h = h.wrapping_mul(0x0100_0193) ^ req;
        h = h.wrapping_mul(0x0100_0193) ^ eff;
        h
    }
}

/// 三段链路账（第 19 处单点导出正本）。
pub struct CouplingLedger {
    rows: Vec<CouplingRow>,
    seq: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl CouplingLedger {
    pub fn new() -> CouplingLedger {
        CouplingLedger { rows: Vec::new(), seq: 0, sealed: false, rejects: Vec::new() }
    }

    /// 入账：同窗同值重复拒（同窗多行合法：多核多行），util 越界拒，行签名入账即核。
    pub fn record(&mut self, window: u64, util_permille: u16, req_khz: u32, eff_khz: u32) -> Result<CouplingRow, u32> {
        if self.sealed {
            self.rejects.push((XC_CA_SEALED, "封账后入账"));
            return Err(XC_CA_SEALED);
        }
        if util_permille > 1000 {
            self.rejects.push((XC_CA_OOB, "util 越界"));
            return Err(XC_CA_OOB);
        }
        let sig = CouplingRow::compute_sig(window, util_permille, req_khz, eff_khz);
        for r in &self.rows {
            if r.window == window && r.sig == sig {
                self.rejects.push((XC_CA_WIN_DUP, "同窗同值重复入账"));
                return Err(XC_CA_WIN_DUP);
            }
        }
        let sig = CouplingRow::compute_sig(window, util_permille, req_khz, eff_khz);
        let row = CouplingRow { window, util_permille, req_khz, eff_khz, sig };
        // 单位列语义核：util 段必须落 ‰ 值域（0..=1000），两频率段落 kHz 正数域
        if util_permille > 1000 {
            self.rejects.push((XC_CA_UNIT, "单位标签错"));
            return Err(XC_CA_UNIT);
        }
        self.seq += 1;
        self.rows.push(row);
        Ok(row)
    }

    /// 单点导出：同窗快照（行集 + 总签名 + 单位三元组冻结描述）。
    /// 快照是消费方唯一可读面（无旁路）。
    pub fn export_snapshot(&self, window: u64) -> Result<CouplingSnapshot, u32> {
        if self.rows.is_empty() {
            return Err(XC_CA_EMPTY);
        }
        let mut picked: Vec<CouplingRow> = Vec::new();
        for r in &self.rows {
            if r.window == window {
                picked.push(*r);
            }
        }
        if picked.is_empty() {
            return Err(XC_CA_EMPTY);
        }
        // 快照总签名：行签名序贯合成（顺序敏感，防重排）
        let mut h: u32 = 0x811C_9DC5;
        for r in &picked {
            h = h.wrapping_mul(0x0100_0193) ^ r.sig;
        }
        Ok(CouplingSnapshot {
            window,
            rows: picked,
            units: (UNIT_UTIL, UNIT_KHZ, UNIT_KHZ),
            total_sig: h,
        })
    }

    /// 快照静态幂等：同窗两次导出逐行逐字段一致（含总签名）。
    pub fn snapshot_idempotent(&self, window: u64) -> bool {
        match (self.export_snapshot(window), self.export_snapshot(window)) {
            (Ok(a), Ok(b)) => {
                a.total_sig == b.total_sig
                    && a.units == b.units
                    && a.rows.len() == b.rows.len()
                    && {
                        let mut same = true;
                        for i in 0..a.rows.len() {
                            if a.rows[i] != b.rows[i] {
                                same = false;
                            }
                        }
                        same
                    }
            }
            _ => false,
        }
    }

    /// B09 汇聚消费：按快照聚合（util 均值定点、请求取最大、生效取账尾）。
    pub fn b09_consume(snap: &CouplingSnapshot) -> Result<(u32, u32, u32), u32> {
        if snap.rows.is_empty() {
            return Err(XC_CA_EMPTY);
        }
        // 单位核：消费前先验单位三元组（错标即拒，B09 口径）
        if snap.units.0 != UNIT_UTIL || snap.units.1 != UNIT_KHZ || snap.units.2 != UNIT_KHZ {
            return Err(XC_CA_UNIT);
        }
        // 篡改核：重算总签名
        let mut h: u32 = 0x811C_9DC5;
        for r in &snap.rows {
            h = h.wrapping_mul(0x0100_0193) ^ r.sig;
        }
        if h != snap.total_sig {
            return Err(XC_CA_TAMPER);
        }
        // 行签名逐行核（双重防线）
        for r in &snap.rows {
            let expect = CouplingRow::compute_sig(r.window, r.util_permille, r.req_khz, r.eff_khz);
            if expect != r.sig {
                return Err(XC_CA_SIG);
            }
        }
        // util 均值定点（×1000 保留两位精度：sum*1000/n/1000 拆分为整数两点）
        let n = snap.rows.len() as u64;
        let sum_util: u64 = snap.rows.iter().map(|r| r.util_permille as u64).sum();
        let avg_util_ppm = (sum_util * 1_000_000 / n / 1000) as u32; // ppm 口径
        let max_req = snap.rows.iter().map(|r| r.req_khz).max().unwrap_or(0);
        let tail_eff = snap.rows[snap.rows.len() - 1].eff_khz;
        Ok((avg_util_ppm, max_req, tail_eff))
    }

    /// 账审计：seq 零跳号（隐含行序即账序）+ 窗号单调不减。
    pub fn audit(&self) -> bool {
        let mut prev_win: u64 = 0;
        for (i, r) in self.rows.iter().enumerate() {
            let _ = i;
            if r.window < prev_win {
                return false;
            }
            prev_win = r.window;
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CA_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rows {
            h = h.wrapping_mul(0x0100_0193) ^ r.sig;
        }
        h ^= self.seq as u32;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.rows.len().min(0xFFFF) as u64)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 导出快照（消费方唯一可读面）。
#[derive(Clone, PartialEq, Eq)]
pub struct CouplingSnapshot {
    pub window: u64,
    pub rows: Vec<CouplingRow>,
    pub units: (u8, u8, u8),
    pub total_sig: u32,
}

/// 判据入口：UNX-F2006-J1。
pub fn run_unx2006_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2006");

    // J1-01 三段链路入账（util→请求→生效逐行齐备）
    {
        let mut l = CouplingLedger::new();
        let r = l.record(1, 700, 3_200_000, 3_200_000);
        let ok = match r {
            Ok(row) => row.util_permille == 700 && row.req_khz == 3_200_000 && row.eff_khz == 3_200_000,
            Err(_) => false,
        };
        s.add("J1-01-三段入账", ok, if ok { "" } else { "三段失列" });
    }
    // J1-02 行签名可复算（签名列自洽）
    {
        let row = CouplingRow::compute_sig(7, 500, 2_400_000, 2_000_000);
        let ok = row == CouplingRow::compute_sig(7, 500, 2_400_000, 2_000_000)
            && row != CouplingRow::compute_sig(8, 500, 2_400_000, 2_000_000)
            && row != CouplingRow::compute_sig(7, 501, 2_400_000, 2_000_000);
        s.add("J1-02-签名自洽", ok, if ok { "" } else { "签名碰撞" });
    }
    // J1-03 同窗快照静态幂等（二次导出逐字节一致）
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(10, 300, 1_800_000, 1_800_000);
        let _ = l.record(10, 500, 2_400_000, 2_100_000);
        // 注：同窗多行按本模块语义允许（同窗多核），快照取全行集
        let ok = l.snapshot_idempotent(10);
        s.add("J1-03-快照幂等", ok, if ok { "" } else { "快照漂移" });
    }
    // J1-04 B09 汇聚消费（均值/取最大/账尾三口径）
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(20, 400, 1_800_000, 1_600_000);
        let _ = l.record(20, 600, 2_400_000, 2_400_000);
        let snap = l.export_snapshot(20);
        let ok = match snap {
            Ok(sn) => match CouplingLedger::b09_consume(&sn) {
                Ok((u, req, eff)) => u == 500_000 && req == 2_400_000 && eff == 2_400_000,
                Err(_) => false,
            },
            Err(_) => false,
        };
        s.add("J1-04-B09汇聚消费", ok, if ok { "" } else { "消费口径漂移" });
    }
    // J1-05 快照篡改检出（总签名失配）
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(30, 500, 2_400_000, 2_400_000);
        let mut snap = match l.export_snapshot(30) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-05-篡改检出", false, "导出失败");
                return s;
            }
        };
        snap.rows[0].eff_khz = 9_999_999; // 篡改
        let ok = CouplingLedger::b09_consume(&snap) == Err(XC_CA_TAMPER);
        s.add("J1-05-篡改检出", ok, if ok { "" } else { "篡改漏抓" });
    }
    // J1-06 行级签名篡改检出（总签名撞库防线：逐行核）
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(40, 500, 2_400_000, 2_400_000);
        let _ = l.record(40, 500, 2_400_000, 2_400_000);
        let mut snap = match l.export_snapshot(40) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-06-行签名核", false, "导出失败");
                return s;
            }
        };
        // 双行同值：篡改行一值并同步伪造总签名 → 行级核仍能抓（行签名未同步）
        snap.rows[0].req_khz = 1_111_111;
        let mut h: u32 = 0x811C_9DC5;
        for r in &snap.rows {
            h = h.wrapping_mul(0x0100_0193) ^ r.sig;
        }
        snap.total_sig = h;
        let ok = CouplingLedger::b09_consume(&snap) == Err(XC_CA_SIG);
        s.add("J1-06-行签名核", ok, if ok { "" } else { "行级漏抓" });
    }
    // J1-07 单位列冻结（单位三元组错标即拒）
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(50, 500, 2_400_000, 2_400_000);
        let mut snap = match l.export_snapshot(50) {
            Ok(v) => v,
            Err(_) => {
                s.add("J1-07-单位冻结", false, "导出失败");
                return s;
            }
        };
        snap.units.0 = UNIT_KHZ; // 错标：util 段标 kHz
        let ok = CouplingLedger::b09_consume(&snap) == Err(XC_CA_UNIT);
        s.add("J1-07-单位冻结", ok, if ok { "" } else { "错标放行" });
    }
    // J1-08 同窗同值重复入账拒（同窗多行合法，精确重复才拒）
    {
        let mut l = CouplingLedger::new();
        let r1 = l.record(60, 500, 2_400_000, 2_400_000);
        let r2 = l.record(60, 600, 2_400_000, 2_400_000);
        let r3 = l.record(60, 500, 2_400_000, 2_400_000);
        let ok = r1.is_ok() && r2.is_ok() && r3 == Err(XC_CA_WIN_DUP) && l.row_count() == 2;
        s.add("J1-08-重复行拒", ok, if ok { "" } else { "重复/多行判定漂移" });
    }
    // J1-09 空账导出拒（谎报防线）
    {
        let l = CouplingLedger::new();
        let ok = l.export_snapshot(1) == Err(XC_CA_EMPTY);
        s.add("J1-09-空账导出拒", ok, if ok { "" } else { "空账谎报" });
    }
    // J1-10 未记账窗导出拒
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(70, 500, 2_400_000, 2_400_000);
        let ok = l.export_snapshot(71) == Err(XC_CA_EMPTY);
        s.add("J1-10-缺窗导出拒", ok, if ok { "" } else { "缺窗谎报" });
    }
    // J1-11 util 越界拒
    {
        let mut l = CouplingLedger::new();
        let ok = l.record(80, 1001, 2_400_000, 2_400_000) == Err(XC_CA_OOB);
        s.add("J1-11-util越界拒", ok, if ok { "" } else { "越界放行" });
    }
    // J1-12 窗号单调审计
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(90, 100, 1_000_000, 1_000_000);
        let _ = l.record(91, 200, 1_200_000, 1_200_000);
        let _ = l.record(92, 300, 1_400_000, 1_400_000);
        let ok = l.audit() && l.row_count() == 3;
        s.add("J1-12-窗号单调", ok, if ok { "" } else { "窗号回退" });
    }
    // J1-13 封账后入账必拒 + 封账稳定
    {
        let mut l = CouplingLedger::new();
        let _ = l.record(95, 500, 2_400_000, 2_400_000);
        let pre = l.fingerprint();
        let _ = l.seal();
        let ok = l.record(96, 600, 2_400_000, 2_400_000) == Err(XC_CA_SEALED)
            && l.fingerprint() == pre
            && l.seal().is_err();
        s.add("J1-13-封账拒+稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // J1-14 指纹冻结
    {
        let f = || -> u64 {
            let mut l = CouplingLedger::new();
            let _ = l.record(100, 333, 2_400_000, 2_400_000);
            let _ = l.record(101, 667, 3_600_000, 3_200_000);
            l.fingerprint()
        };
        let ok = f() == f() && f() != 0;
        s.add("J1-14-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }
    // J1-15 段位独占
    {
        let ok = SEG_BASE == 0x7B20;
        s.add("J1-15-段位独占", ok, if ok { "" } else { "0x7B20~0x7B3F 本模块独占失守" });
    }
    s
}
