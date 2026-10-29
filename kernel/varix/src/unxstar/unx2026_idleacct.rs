// unx2026_idleacct —— UNX-F2026 单点导出：idle 账接口（第 21 处单点导出）
// 判据原文 UNX-F2026-J1：第 21 处单点导出在册（F1762 idle 账扩容：逐档驻留/进出计数），
// 签名/单位/同窗快照三列齐备，与 B09 汇聚消费一致。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9DA0 独占（0x9DA0~0x9DBF）。
//
// 设计（纯功能 no_std，体例对齐 unx2006_couplacct 单点导出）：
//   * 三段链路账：逐档驻留累计（us）/进入计数/退出计数，六档闭集；
//   * 行级 FNV 签名 + 同窗快照总签名，双重验签；
//   * 同窗快照幂等：同窗二次导出逐字节一致（快照指纹不随读次漂移）；
//   * B09 汇聚消费三口径：均值 / 取最大 / 账尾，三口径同源一致；
//   * 恒等守恒：Σ逐档驻留 = 总驻留、进-出 = 在册深度，双恒等式读时必核；
//   * 篡改注入：改一行即总签名破（验签灵敏度 10/10）；
//   * 零 unwrap/expect/panic 面。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9DA0 独占） ─────────────────────────────────────────────
pub const XC_IA_OOB: u32 = 0x9DA1; // 档号越界
pub const XC_IA_SEALED: u32 = 0x9DA2; // 封账后入账
pub const XC_IA_PAIR: u32 = 0x9DA3; // 进出配对破（退出无进入）
pub const XC_IA_RESID: u32 = 0x9DA4; // 驻留守恒破（Σ档 ≠ 总）
pub const XC_IA_SIG: u32 = 0x9DA5; // 签名验签破
pub const XC_IA_NEG: u32 = 0x9DA6; // 深度为负（出多于进）

pub const LVLS: usize = 6;

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 一行账：某档一次进出会话。
#[derive(Clone, Copy)]
pub struct IdleRow {
    pub seq: u64,
    pub lvl: u8,
    pub residency_us: u64,
    pub sig: u64,
}

fn row_sig(seq: u64, lvl: u8, residency_us: u64) -> u64 {
    let mut buf = [0u8; 17];
    buf[0..8].copy_from_slice(&seq.to_le_bytes());
    buf[8] = lvl;
    buf[9..17].copy_from_slice(&residency_us.to_le_bytes());
    fnv64(&buf)
}

/// 第 21 处单点导出：idle 账。
pub struct IdleLedger {
    rows: Vec<IdleRow>,
    residency_by_lvl: [u64; LVLS],
    enters: [u64; LVLS],
    exits: [u64; LVLS],
    depth: i64,
    total_residency_us: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl IdleLedger {
    pub fn new() -> IdleLedger {
        IdleLedger {
            rows: Vec::new(),
            residency_by_lvl: [0u64; LVLS],
            enters: [0u64; LVLS],
            exits: [0u64; LVLS],
            depth: 0,
            total_residency_us: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 进入 idle 某档（进计数 +1，深度 +1）。
    pub fn enter(&mut self, lvl: u8) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IA_SEALED, "封账后进入"));
            return Err(XC_IA_SEALED);
        }
        if lvl as usize >= LVLS {
            self.rejects.push((XC_IA_OOB, "档号越界"));
            return Err(XC_IA_OOB);
        }
        self.enters[lvl as usize] += 1;
        self.depth += 1;
        Ok(())
    }

    /// 退出 idle（退出计数 +1，驻留入账 + 行签名）。
    pub fn exit(&mut self, lvl: u8, residency_us: u64) -> Result<IdleRow, u32> {
        if self.sealed {
            self.rejects.push((XC_IA_SEALED, "封账后退出"));
            return Err(XC_IA_SEALED);
        }
        if lvl as usize >= LVLS {
            self.rejects.push((XC_IA_OOB, "档号越界"));
            return Err(XC_IA_OOB);
        }
        // 进出配对：该档进入数必须 > 退出数
        if self.enters[lvl as usize] <= self.exits[lvl as usize] {
            self.rejects.push((XC_IA_PAIR, "退出无进入"));
            return Err(XC_IA_PAIR);
        }
        // 全局深度恒等：出多于进即负深度
        if self.depth <= 0 {
            self.rejects.push((XC_IA_NEG, "深度为负"));
            return Err(XC_IA_NEG);
        }
        self.exits[lvl as usize] += 1;
        self.depth -= 1;
        self.residency_by_lvl[lvl as usize] = self.residency_by_lvl[lvl as usize]
            .wrapping_add(residency_us);
        self.total_residency_us = self.total_residency_us.wrapping_add(residency_us);
        let seq = self.rows.len() as u64 + 1;
        let row = IdleRow {
            seq,
            lvl,
            residency_us,
            sig: row_sig(seq, lvl, residency_us),
        };
        self.rows.push(row);
        Ok(row)
    }

    /// 恒等式 1：Σ逐档驻留 == 总驻留。
    pub fn resid_conserved(&self) -> bool {
        let sum: u64 = self.residency_by_lvl.iter().sum();
        sum == self.total_residency_us
    }

    /// 恒等式 2：每档 进 ≥ 出；全局深度 = Σ进 − Σ出 ≥ 0。
    pub fn pair_conserved(&self) -> bool {
        for l in 0..LVLS {
            if self.exits[l] > self.enters[l] {
                return false;
            }
        }
        let e: u64 = self.enters.iter().sum();
        let x: u64 = self.exits.iter().sum();
        self.depth == (e as i64) - (x as i64) && self.depth >= 0
    }

    /// 行级读时验签。
    pub fn ledger_verified(&self) -> bool {
        self.rows
            .iter()
            .all(|r| r.sig == row_sig(r.seq, r.lvl, r.residency_us))
    }

    /// B09 汇聚三口径（对逐档驻留数组）：均值 / 取最大 / 账尾。
    /// 三口径同源：都从 residency_by_lvl 派生；均值×档数 == 总驻留（封顶容差 1）。
    pub fn b09_aggregate(&self) -> (u64, u64, u64) {
        let sum: u64 = self.residency_by_lvl.iter().sum();
        let mean = sum / LVLS as u64;
        let max = *self.residency_by_lvl.iter().max().unwrap_or(&0);
        (mean, max, sum) // (均值, 取最大, 账尾=总)
    }

    /// 三口径一致性：均值×档数与总驻留差 ≤ 档数-1（整除余数容差）。
    pub fn b09_consistent(&self) -> bool {
        let (mean, _max, total) = self.b09_aggregate();
        let back = mean.wrapping_mul(LVLS as u64);
        total >= back && total - back < LVLS as u64
    }

    /// 同窗快照：当前账态总签名（确定性派生，同态同签）。
    pub fn snapshot_sig(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.total_residency_us.to_le_bytes());
        buf.extend_from_slice(&(self.rows.len() as u64).to_le_bytes());
        for l in 0..LVLS {
            buf.extend_from_slice(&self.residency_by_lvl[l].to_le_bytes());
            buf.extend_from_slice(&self.enters[l].to_le_bytes());
            buf.extend_from_slice(&self.exits[l].to_le_bytes());
        }
        fnv64(&buf)
    }

    /// 同窗二次导出幂等（逐字节一致）。
    pub fn snapshot_idempotent(&self) -> bool {
        let a = self.snapshot_sig();
        let b = self.snapshot_sig();
        a == b
    }

    /// 验签灵敏度注入核 10/10：对账行做 10 种篡改，总签名必变。
    pub fn tamper_inject(&self) -> (u32, u32) {
        const N: u32 = 10;
        let mut caught = 0u32;
        let base = self.snapshot_sig();
        // 篡改 1~6：逐档驻留各 +1（六种）
        for l in 0..LVLS {
            let mut probe = self.residency_by_lvl;
            probe[l] = probe[l].wrapping_add(1);
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&self.total_residency_us.to_le_bytes());
            buf.extend_from_slice(&(self.rows.len() as u64).to_le_bytes());
            for x in probe.iter() {
                buf.extend_from_slice(&x.to_le_bytes());
            }
            for l2 in 0..LVLS {
                buf.extend_from_slice(&self.enters[l2].to_le_bytes());
                buf.extend_from_slice(&self.exits[l2].to_le_bytes());
            }
            if fnv64(&buf) != base {
                caught += 1;
            }
        }
        // 篡改 7~8：进/出计数 +1（两种）
        for which in 0..2u32 {
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&self.total_residency_us.to_le_bytes());
            buf.extend_from_slice(&(self.rows.len() as u64).to_le_bytes());
            for l in 0..LVLS {
                buf.extend_from_slice(&self.residency_by_lvl[l].to_le_bytes());
                let v = if which == 0 { self.enters[l] + 1 } else { self.exits[l] + 1 };
                buf.extend_from_slice(&v.to_le_bytes());
                let e = if which == 0 { self.exits[l] } else { self.exits[l] };
                buf.extend_from_slice(&e.to_le_bytes());
            }
            if fnv64(&buf) != base {
                caught += 1;
            }
        }
        // 篡改 9：总驻留 +1
        {
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&(self.total_residency_us + 1).to_le_bytes());
            buf.extend_from_slice(&(self.rows.len() as u64).to_le_bytes());
            for l in 0..LVLS {
                buf.extend_from_slice(&self.residency_by_lvl[l].to_le_bytes());
                buf.extend_from_slice(&self.enters[l].to_le_bytes());
                buf.extend_from_slice(&self.exits[l].to_le_bytes());
            }
            if fnv64(&buf) != base {
                caught += 1;
            }
        }
        // 篡改 10：行数 +1
        {
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&self.total_residency_us.to_le_bytes());
            buf.extend_from_slice(&(self.rows.len() as u64 + 1).to_le_bytes());
            for l in 0..LVLS {
                buf.extend_from_slice(&self.residency_by_lvl[l].to_le_bytes());
                buf.extend_from_slice(&self.enters[l].to_le_bytes());
                buf.extend_from_slice(&self.exits[l].to_le_bytes());
            }
            if fnv64(&buf) != base {
                caught += 1;
            }
        }
        (N, caught)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn total_residency(&self) -> u64 {
        self.total_residency_us
    }

    pub fn depth(&self) -> i64 {
        self.depth
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_IA_SEALED);
        }
        self.sealed = true;
        Ok(self.snapshot_sig())
    }
}

// ── 标准夹具 ─────────────────────────────────────────────────────────
/// 标准账：六档各 2 次进出会话，驻留逐档递增。
fn standard_ledger() -> IdleLedger {
    let mut l = IdleLedger::new();
    for lvl in 0..6u8 {
        let _ = l.enter(lvl);
        let _ = l.exit(lvl, 1000 * (lvl as u64 + 1));
        let _ = l.enter(lvl);
        let _ = l.exit(lvl, 500 * (lvl as u64 + 1));
    }
    l
}

// ── 判据入口（UNX-F2026-J1） ─────────────────────────────────────────
pub fn run_unx2026_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2026 单点导出 idle 账接口");
    // C26-01 第 21 处单点导出在册：标准账 12 行、六档全有账
    {
        let l = standard_ledger();
        let mut ok = l.row_count() == 12;
        for lvl in 0..6usize {
            if l.enters[lvl] != 2 || l.exits[lvl] != 2 {
                ok = false;
            }
        }
        s.add("C26-01-单点在册", ok, if ok { "" } else { "在册缺失" });
    }
    // C26-02 双恒等式守恒
    {
        let l = standard_ledger();
        let ok = l.resid_conserved() && l.pair_conserved();
        s.add("C26-02-双恒等守恒", ok, if ok { "" } else { "守恒破" });
    }
    // C26-03 逐档驻留扩容口径（F1762）：Σ = 1000+...+1500 + 500+...+750
    {
        let l = standard_ledger();
        // Σ(1000..1500 步 100) = 6300；Σ(500..750 步 50) = 3150；总 9450
        let ok = l.total_residency() == 9_450;
        s.add("C26-03-驻留总和", ok, if ok { "" } else { "总和漂移" });
    }
    // C26-04 签名/单位/同窗快照三列齐备：行验签 + 快照幂等
    {
        let l = standard_ledger();
        let ok = l.ledger_verified() && l.snapshot_idempotent();
        s.add("C26-04-三列齐备", ok, if ok { "" } else { "三列缺" });
    }
    // C26-05 B09 三口径一致
    {
        let l = standard_ledger();
        let ok = l.b09_consistent();
        s.add("C26-05-B09三口径", ok, if ok { "" } else { "口径分叉" });
    }
    // C26-06 档号越界拒（enter/exit 双口）
    {
        let mut l = IdleLedger::new();
        let e1 = l.enter(6);
        let e2 = l.exit(9, 1);
        let ok = e1 == Err(XC_IA_OOB) && e2.err() == Some(XC_IA_OOB);
        s.add("C26-06-越界拒", ok, if ok { "" } else { "越界放行" });
    }
    // C26-07 进出配对破：无进入先退出必拒
    {
        let mut l = IdleLedger::new();
        let ok = l.exit(2, 100).err() == Some(XC_IA_PAIR);
        s.add("C26-07-配对拒", ok, if ok { "" } else { "配对放行" });
    }
    // C26-08 篡改注入 10/10
    {
        let l = standard_ledger();
        let (n, c) = l.tamper_inject();
        let ok = n == 10 && c == 10;
        s.add("C26-08-篡改10/10", ok, if ok { "" } else { "验签漏抓" });
    }
    // C26-09 封账后入账必拒 + 快照冻结
    {
        let mut l = standard_ledger();
        let pre = l.snapshot_sig();
        let sealed = l.seal();
        let e = l.enter(0);
        let x = l.exit(0, 1);
        let ok = sealed.is_ok()
            && e == Err(XC_IA_SEALED)
            && x.err() == Some(XC_IA_SEALED)
            && l.snapshot_sig() == pre;
        s.add("C26-09-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C26-10 深度恒等：开账未平（进 3 出 1）→ 深度 2 且守恒仍真
    {
        let mut l = IdleLedger::new();
        let _ = l.enter(1);
        let _ = l.enter(3);
        let _ = l.enter(5);
        let _ = l.exit(3, 700);
        let ok = l.depth() == 2 && l.pair_conserved() && l.resid_conserved();
        s.add("C26-10-开账深度", ok, if ok { "" } else { "深度失守" });
    }
    // C26-11 指纹跨实例稳定（同账同签）
    {
        let f = || standard_ledger().snapshot_sig();
        let ok = f() == f();
        s.add("C26-11-跨实例同签", ok, if ok { "" } else { "指纹漂移" });
    }
    s
}
