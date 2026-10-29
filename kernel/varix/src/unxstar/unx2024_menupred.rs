// unx2024_menupred —— UNX-F2024 菜单式预测策略（典型间隔预测）
// 判据原文 UNX-F2024-J1：菜单式预测断言全过（历史间隔统计/典型值提取/扰动裁断三步），
// 预测精度账在册，预测漂移注入 10/10 检出。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9CA0 独占（0x9CA0~0x9CBF）。
//
// 设计（纯功能 no_std，Linux menu governor 语义对齐）：
//   * 第一步·历史间隔统计：环形窗（8 槽）登记最近空闲间隔 us；
//   * 第二步·典型值提取：对窗口取中位数（排序后取中位，偶数取下中位——保守）；
//   * 第三步·扰动裁断：离群值（> 4×中位 或 < 中位/4）裁出后再取中位；
//   * 预测输出：裁断后典型值，且受「最大预测上限」钳制；
//   * 精度账：逐次落行（seq/预测/实测/误差千分比），读时验签；
//   * 漂移注入：10 笔间隔突变注入 10/10 检出（误差超 50% 阈值即漂移）；
//   * 零 unwrap/expect/panic 面。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9CA0 独占） ─────────────────────────────────────────────
pub const XC_MP_WIN_EMPTY: u32 = 0x9CA1; // 空窗预测（无历史）
pub const XC_MP_DRIFT: u32 = 0x9CA2; // 预测漂移（误差超阈）
pub const XC_MP_LEDGER: u32 = 0x9CA3; // 精度账签名漂移
pub const XC_MP_SEALED: u32 = 0x9CA4; // 封账后登记
pub const XC_MP_ZERO: u32 = 0x9CA5; // 零间隔登记（无效样本）
pub const XC_MP_CAP: u32 = 0x9CA6; // 预测超上限钳制（告警）

pub const WINDOW: usize = 8;
pub const OUTLIER_HI_X4: u64 = 4; // 上离群：> 4×中位
pub const OUTLIER_LO_DIV: u64 = 4; // 下离群：< 中位/4
pub const DRIFT_PERMILLE: u64 = 500; // 漂移阈值：误差 > 50%

/// 一条精度账行。
#[derive(Clone, Copy)]
pub struct AccuracyRow {
    pub seq: u64,
    pub predicted_us: u32,
    pub measured_us: u32,
    pub err_permille: u64,
    pub sig: u64,
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn row_sig(seq: u64, predicted_us: u32, measured_us: u32, err_permille: u64) -> u64 {
    let mut buf = [0u8; 20];
    buf[0..8].copy_from_slice(&seq.to_le_bytes());
    buf[8..12].copy_from_slice(&predicted_us.to_le_bytes());
    buf[12..16].copy_from_slice(&measured_us.to_le_bytes());
    buf[16..20].copy_from_slice(&(err_permille as u32).to_le_bytes());
    fnv64(&buf)
}

/// 菜单式预测器。
pub struct MenuPredictor {
    window: [u32; WINDOW],
    win_len: usize,
    win_pos: usize,
    cap_us: u32,
    ledger: Vec<AccuracyRow>,
    seq: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl MenuPredictor {
    /// 构造：预测上限钳制 cap_us（必须 > 0）。
    pub fn new(cap_us: u32) -> Result<MenuPredictor, u32> {
        if cap_us == 0 {
            return Err(XC_MP_ZERO);
        }
        Ok(MenuPredictor {
            window: [0u32; WINDOW],
            win_len: 0,
            win_pos: 0,
            cap_us,
            ledger: Vec::new(),
            seq: 0,
            sealed: false,
            rejects: Vec::new(),
        })
    }

    /// 第一步：登记历史间隔（环形窗；零间隔拒）。
    pub fn observe(&mut self, interval_us: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_MP_SEALED, "封账后登记"));
            return Err(XC_MP_SEALED);
        }
        if interval_us == 0 {
            self.rejects.push((XC_MP_ZERO, "零间隔"));
            return Err(XC_MP_ZERO);
        }
        self.window[self.win_pos] = interval_us;
        self.win_pos = (self.win_pos + 1) % WINDOW;
        if self.win_len < WINDOW {
            self.win_len += 1;
        }
        Ok(())
    }

    /// 第二步+第三步：典型值提取（中位）→ 扰动裁断（剔离群再中位）→ 上限钳制。
    /// 返回 (预测值, 是否触发钳制)。
    pub fn predict(&self) -> Result<(u32, bool), u32> {
        if self.win_len == 0 {
            return Err(XC_MP_WIN_EMPTY);
        }
        // 窗口有效样本拷出
        let mut samples: Vec<u32> = Vec::new();
        for i in 0..self.win_len {
            samples.push(self.window[i]);
        }
        // 插入排序（8 元素小样本，稳定且无递归）
        for i in 1..samples.len() {
            let mut j = i;
            while j > 0 && samples[j] < samples[j - 1] {
                samples.swap(j, j - 1);
                j -= 1;
            }
        }
        // 典型值：下中位（偶数取 samples[n/2 - 1]，奇数取 samples[n/2]）
        let mid = if samples.len() % 2 == 0 {
            samples[samples.len() / 2 - 1]
        } else {
            samples[samples.len() / 2]
        } as u64;
        // 扰动裁断：剔离群后重新取下中位
        let mut kept: Vec<u32> = Vec::new();
        for &v in samples.iter() {
            let vv = v as u64;
            let outlier = vv > mid.wrapping_mul(OUTLIER_HI_X4) || (mid > 0 && vv * OUTLIER_LO_DIV < mid);
            if !outlier {
                kept.push(v);
            }
        }
        // 裁断后至少保 1 样本（全离群时退回原中位——防饿死）
        let typical: u64 = if kept.is_empty() {
            mid
        } else {
            // kept 已按序（保持原遍历序不保序？——samples 已排序，遍历保序，kept 有序）
            let n = kept.len();
            if n % 2 == 0 {
                kept[n / 2 - 1] as u64
            } else {
                kept[n / 2] as u64
            }
        };
        // 上限钳制
        if typical > self.cap_us as u64 {
            return Ok((self.cap_us, true));
        }
        Ok((typical as u32, false))
    }

    /// 精度账：登记一次「预测 vs 实测」，误差千分比入账；超阈值判漂移。
    pub fn account(&mut self, measured_us: u32) -> Result<AccuracyRow, u32> {
        if self.sealed {
            self.rejects.push((XC_MP_SEALED, "封账后登记精度"));
            return Err(XC_MP_SEALED);
        }
        if measured_us == 0 {
            self.rejects.push((XC_MP_ZERO, "零实测"));
            return Err(XC_MP_ZERO);
        }
        let predicted_us = match self.predict() {
            Ok((p, _)) => p,
            Err(e) => return Err(e),
        };
        let p = predicted_us as u64;
        let m = measured_us as u64;
        let err = if p >= m { p - m } else { m - p };
        let err_permille = err.wrapping_mul(1000) / m;
        self.seq += 1;
        let row = AccuracyRow {
            seq: self.seq,
            predicted_us,
            measured_us,
            err_permille,
            sig: row_sig(self.seq, predicted_us, measured_us, err_permille),
        };
        self.ledger.push(row);
        if err_permille > DRIFT_PERMILLE {
            self.rejects.push((XC_MP_DRIFT, "预测漂移超阈"));
            return Ok(row); // 漂移入账但返回 Ok——漂移由 reject/查询面暴露
        }
        Ok(row)
    }

    /// 漂移注入核 10/10：稳态主导窗口下逐笔注入间隔尖峰，逐笔断言检出。
    /// 每笔注入后用 7 笔稳态洗窗，保证下一笔注入时窗口仍稳态主导
    /// （否则尖峰填满窗口后预测跟着抬升，后几笔会漏抓）。
    pub fn drift_inject(&mut self) -> (u32, u32) {
        const N: u32 = 10;
        let mut caught = 0u32;
        for k in 0..10u32 {
            let spike = 5000 + k * 500;
            let _ = self.observe(spike);
            // 实测 = 尖峰；预测被裁断压在稳态 → 误差 ≥ 800‰ > 500‰ 阈 → 漂移
            let pre_rejects = self.rejects.len();
            let _ = self.account(spike);
            if self.rejects.len() > pre_rejects {
                caught += 1;
            }
            // 洗窗：7 笔稳态把尖峰挤出环形窗（窗容量 8）
            for j in 0..7u32 {
                let _ = self.observe(1000 + ((k * 7 + j) % 8) as u32);
            }
        }
        (N, caught)
    }

    /// 精度账逐次可查。
    pub fn row_at(&self, seq: u64) -> Option<&AccuracyRow> {
        self.ledger.iter().find(|r| r.seq == seq)
    }

    pub fn row_count(&self) -> usize {
        self.ledger.len()
    }

    /// 漂移行计数（误差超阈的账行）。
    pub fn drift_rows(&self) -> usize {
        self.ledger.iter().filter(|r| r.err_permille > DRIFT_PERMILLE).count()
    }

    /// 读时验签。
    pub fn ledger_verified(&self) -> bool {
        self.ledger
            .iter()
            .all(|r| r.sig == row_sig(r.seq, r.predicted_us, r.measured_us, r.err_permille))
    }

    /// seq 致密。
    pub fn seq_dense(&self) -> bool {
        for (i, r) in self.ledger.iter().enumerate() {
            if r.seq != (i as u64) + 1 {
                return false;
            }
        }
        true
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_MP_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x9ca0_9ca0_9ca0_9ca0;
        for r in &self.ledger {
            h ^= r.sig;
            h = h.wrapping_mul(0x100000001b3);
        }
        h.wrapping_add((self.win_len as u64) << 32).wrapping_add(self.seq)
    }
}

// ── 标准夹具 ─────────────────────────────────────────────────────────
/// 标准预测器：稳态 8 笔 1000±小扰动。
fn standard_predictor() -> MenuPredictor {
    match MenuPredictor::new(1_000_000) {
        Ok(mut p) => {
            let base = [980u32, 1020, 1000, 990, 1010, 1005, 995, 1000];
            for v in base.iter() {
                let _ = p.observe(*v);
            }
            p
        }
        Err(_) => match MenuPredictor::new(1) {
            Ok(p) => p,
            Err(_) => MenuPredictor {
                window: [0u32; WINDOW],
                win_len: 0,
                win_pos: 0,
                cap_us: 1,
                ledger: Vec::new(),
                seq: 0,
                sealed: false,
                rejects: Vec::new(),
            },
        },
    }
}

// ── 判据入口（UNX-F2024-J1） ─────────────────────────────────────────
pub fn run_unx2024_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2024 菜单式预测策略");
    // C24-01 稳态典型值：8 笔 ~1000us → 预测 ≈ 1000（±60 容差）
    {
        let p = standard_predictor();
        let ok = match p.predict() {
            Ok((v, capped)) => !capped && v >= 940 && v <= 1060,
            Err(_) => false,
        };
        s.add("C24-01-稳态典型值", ok, if ok { "" } else { "典型值漂移" });
    }
    // C24-02 扰动裁断：注入 1 笔 100000us 大离群 → 预测仍 ≈1000（裁断吸收）
    {
        let mut p = standard_predictor();
        let _ = p.observe(100_000);
        let ok = match p.predict() {
            Ok((v, capped)) => !capped && v >= 900 && v <= 1200,
            Err(_) => false,
        };
        s.add("C24-02-离群裁断", ok, if ok { "" } else { "离群穿透" });
    }
    // C24-03 全离群防饿死：窗内全大值 → 至少给出非零预测
    {
        let mut p = match MenuPredictor::new(1_000_000) {
            Ok(v) => v,
            Err(_) => return s,
        };
        for k in 0..8u32 {
            let _ = p.observe(50_000 + k * 100);
        }
        let ok = match p.predict() {
            Ok((v, _)) => v > 0,
            Err(_) => false,
        };
        s.add("C24-03-防饿死", ok, if ok { "" } else { "全离群归零" });
    }
    // C24-04 上限钳制：cap=2000，全窗 5000 → 预测=2000 且 capped=true
    {
        let mut p = match MenuPredictor::new(2_000) {
            Ok(v) => v,
            Err(_) => return s,
        };
        for _ in 0..8u32 {
            let _ = p.observe(5_000);
        }
        let ok = p.predict() == Ok((2_000, true));
        s.add("C24-04-上限钳制", ok, if ok { "" } else { "钳制失守" });
    }
    // C24-05 空窗预测拒 + 零间隔拒
    {
        let p = match MenuPredictor::new(1_000_000) {
            Ok(v) => v,
            Err(_) => return s,
        };
        let e1 = p.predict();
        let mut p2 = standard_predictor();
        let e2 = p2.observe(0);
        let ok = e1 == Err(XC_MP_WIN_EMPTY) && e2 == Err(XC_MP_ZERO);
        s.add("C24-05-空窗与零拒", ok, if ok { "" } else { "无效输入放行" });
    }
    // C24-06 精度账在册：稳态预测 vs 稳态实测 → 误差 < 10% 且账行齐全
    {
        let mut p = standard_predictor();
        let r = p.account(1_000);
        let ok = match r {
            Ok(row) => row.err_permille <= 100 && p.row_count() == 1,
            Err(_) => false,
        };
        s.add("C24-06-精度账在册", ok, if ok { "" } else { "精度账失守" });
    }
    // C24-07 预测漂移注入 10/10
    {
        let mut p = standard_predictor();
        let (n, c) = p.drift_inject();
        let ok = n == 10 && c == 10 && p.drift_rows() >= 10;
        s.add("C24-07-漂移注入10/10", ok, if ok { "" } else { "漂移漏抓" });
    }
    // C24-08 seq 致密 + 读时验签
    {
        let mut p = standard_predictor();
        let _ = p.account(1_000);
        let _ = p.account(1_010);
        let ok = p.seq_dense() && p.ledger_verified();
        s.add("C24-08-致密验签", ok, if ok { "" } else { "账完整性破" });
    }
    // C24-09 封账后登记必拒
    {
        let mut p = standard_predictor();
        let _ = p.seal();
        let o = p.observe(1_000);
        let a = p.account(1_000);
        let ok = o == Err(XC_MP_SEALED) && a.err() == Some(XC_MP_SEALED) && p.reject_count() >= 2;
        s.add("C24-09-封账必拒", ok, if ok { "" } else { "封账失守" });
    }
    // C24-10 指纹一致 + 封账稳定
    {
        let f = || standard_predictor().fingerprint();
        let ok0 = f() == f();
        let mut p = standard_predictor();
        let pre = p.fingerprint();
        let sealed = p.seal();
        s.add(
            "C24-10-指纹封账",
            ok0 && sealed.is_ok() && p.fingerprint() == pre && p.seal().is_err(),
            if ok0 { "" } else { "指纹漂移" },
        );
    }
    s
}
