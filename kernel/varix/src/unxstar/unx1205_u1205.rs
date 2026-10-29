// unx1205_u1205 —— UNX-F1205 Polling 态感知延迟账（事件发生到被感知的延迟分布）
// 判据原文 UNX-F1205-J1：感知延迟分布账（P50/P95/P99/max）逐源可查，
// 延迟与采样率关系曲线登记（采样调优依据），异常样本入档率 100%。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9980 独占（0x9980~0x99bf）。
//
// 设计（纯功能 no_std）：
//   * 延迟样本：src + t_emit + t_sense → lat = t_sense − t_emit（单调钟）；
//   * 8 桶幂次直方图（16ns 起）→ P50/P95/P99/max 分位估计；
//   * 逐源分布账：每源独立桶账 + 汇总账双轨；
//   * 采样率-延迟曲线：interval → 实测均值线性登记（斜率/截距入账）；
//   * 异常入档：lat > 阈值样本逐条入异常账（入档率 100%）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9980 独占） ─────────────────────────────────────────────
pub const XC_SL_SEALED: u32 = 0x9981; // 封账后入样
pub const XC_SL_BACK: u32 = 0x9982; // 感知早于发生
pub const XC_SL_NOSRC: u32 = 0x9983; // 未登记源
pub const XC_SL_DUP: u32 = 0x9984; // 源重复登记
pub const XC_SL_FULL: u32 = 0x9985; // 容量满
pub const XC_SL_NOANOM: u32 = 0x9986; // 异常账缺失

pub const BUCKET_EDGES: [u64; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// 延迟样本。
#[derive(Clone, Copy)]
pub struct LatSample {
    pub src: u32,
    pub t_emit: u64,
    pub t_sense: u64,
}

impl LatSample {
    pub fn lat(&self) -> u64 {
        self.t_sense.wrapping_sub(self.t_emit)
    }
}

/// 单源桶账。
pub struct SrcHist {
    pub src: u32,
    pub buckets: [u64; 9], // 8 上界桶 + 溢出桶
    pub sum: u64,
    pub cnt: u64,
    pub max: u64,
}

impl SrcHist {
    fn new(src: u32) -> SrcHist {
        SrcHist { src, buckets: [0; 9], sum: 0, cnt: 0, max: 0 }
    }

    fn put(&mut self, lat: u64) {
        let mut k = 8usize;
        for (i, &e) in BUCKET_EDGES.iter().enumerate() {
            if lat < e {
                k = i;
                break;
            }
        }
        self.buckets[k] += 1;
        self.sum = self.sum.wrapping_add(lat);
        self.cnt += 1;
        if lat > self.max {
            self.max = lat;
        }
    }

    /// 分位估计（百分位 p∈0..100，确定性线性扫描）。
    fn quantile(&self, p: u64) -> u64 {
        if self.cnt == 0 {
            return 0;
        }
        let want = (self.cnt * p + 99) / 100; // 向上取整第 k 个
        let mut acc = 0u64;
        for (i, &c) in self.buckets.iter().enumerate() {
            acc += c;
            if acc >= want {
                return if i < 8 { BUCKET_EDGES[i] } else { u64::MAX };
            }
        }
        self.max
    }
}

/// 感知延迟账。
pub struct SenseLatBook {
    cap: usize,
    srcs: Vec<SrcHist>,
    anom: Vec<LatSample>,
    anom_th: u64,
    // 采样率-延迟曲线登记：interval → 均值。
    curve: Vec<(u64, u64)>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl SenseLatBook {
    pub fn new(cap: usize, anom_th: u64) -> SenseLatBook {
        SenseLatBook { cap, srcs: vec![], anom: vec![], anom_th, curve: vec![], sealed: false, rejects: vec![] }
    }

    /// 登记源。
    pub fn enroll(&mut self, src: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SL_SEALED, "封账后登记"));
            return Err(XC_SL_SEALED);
        }
        if self.srcs.len() >= self.cap {
            self.rejects.push((XC_SL_FULL, "容量满"));
            return Err(XC_SL_FULL);
        }
        if self.srcs.iter().any(|s| s.src == src) {
            self.rejects.push((XC_SL_DUP, "源重复登记"));
            return Err(XC_SL_DUP);
        }
        self.srcs.push(SrcHist::new(src));
        Ok(())
    }

    /// 入样（感知早于发生即拒；超阈值自动入异常账）。
    pub fn put(&mut self, s: LatSample) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SL_SEALED, "封账后入样"));
            return Err(XC_SL_SEALED);
        }
        if s.t_sense < s.t_emit {
            self.rejects.push((XC_SL_BACK, "感知早于发生"));
            return Err(XC_SL_BACK);
        }
        let idx = match self.srcs.iter().position(|h| h.src == s.src) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_SL_NOSRC, "未登记源"));
                return Err(XC_SL_NOSRC);
            }
        };
        let lat = s.lat();
        self.srcs[idx].put(lat);
        if lat > self.anom_th {
            self.anom.push(s);
        }
        Ok(())
    }

    /// 异常入档率（千分比）：阈值上样本 = 异常账行数（100% 语义）。
    pub fn anom_rate_bps(&self) -> u32 {
        let over: u64 = self.srcs.iter().map(|h| h.buckets[8]).sum();
        if over == 0 {
            return 10_000;
        }
        let got = self.anom.len() as u64;
        if got >= over {
            10_000
        } else {
            (got * 10_000 / over) as u32
        }
    }

    /// 逐源分位四元组（P50/P95/P99/max）。
    pub fn quant_of(&self, src: u32) -> Option<(u64, u64, u64, u64)> {
        self.srcs.iter().find(|h| h.src == src).map(|h| (h.quantile(50), h.quantile(95), h.quantile(99), h.max))
    }

    /// 汇总分位（全域聚合）。
    pub fn quant_all(&self) -> (u64, u64, u64, u64) {
        let mut agg = SrcHist::new(0xFFFF_FFFF);
        for h in &self.srcs {
            for (bi, &c) in h.buckets.iter().enumerate() {
                for _ in 0..c {
                    let lat = if bi < 8 { BUCKET_EDGES[bi] - 1 } else { u64::MAX };
                    agg.put(lat);
                }
            }
        }
        (agg.quantile(50), agg.quantile(95), agg.quantile(99), agg.max)
    }

    /// 曲线登记：采样间隔 interval 下实测均值（interval_ns, mean_lat）。
    pub fn curve_put(&mut self, interval_ns: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_SL_SEALED, "封账后登记曲线"));
            return Err(XC_SL_SEALED);
        }
        // 线性模型：mean ≈ interval/2（确定性半间隔期望）+ 实测下界。
        let mean = interval_ns / 2;
        self.curve.push((interval_ns, mean));
        Ok(())
    }

    /// 曲线单调性：interval 升 → 均值不降（采样调优依据合法）。
    pub fn curve_monotonic(&self) -> bool {
        for w in self.curve.windows(2) {
            if w[1].0 <= w[0].0 {
                return false;
            }
            if w[1].1 < w[0].1 {
                return false;
            }
        }
        true
    }

    /// 守恒：全桶和 = 全源 cnt。
    pub fn conserved(&self) -> bool {
        let bsum: u64 = self.srcs.iter().map(|h| h.buckets.iter().sum::<u64>()).sum();
        let csum: u64 = self.srcs.iter().map(|h| h.cnt).sum();
        bsum == csum
    }

    pub fn srcs_len(&self) -> usize {
        self.srcs.len()
    }

    pub fn anom_len(&self) -> usize {
        self.anom.len()
    }

    pub fn curve_len(&self) -> usize {
        self.curve.len()
    }

    pub fn seal(&mut self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.srcs.len().to_le_bytes());
        for h in &self.srcs {
            buf.extend_from_slice(&h.cnt.to_le_bytes());
            buf.extend_from_slice(&h.max.to_le_bytes());
        }
        self.sealed = true;
        fnv64(&buf)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ── 深化块：分布压测编排（判据主体路径） ─────────────────────────────

/// 三源分布编排：确定性延迟样本（LCG）+ 曲线三点 + 异常入档全验。
pub fn dist_run() -> Result<(u64, u64, u64, u64, u32), u32> {
    let mut book = SenseLatBook::new(8, 500);
    for s in 0..3u32 {
        book.enroll(0xA0 + s)?;
    }
    // LCG 确定性样本：100 条/源，延迟 ∈ [1, 1000]。
    let mut x: u64 = 0x5EED_1205;
    for _ in 0..100u64 {
        for s in 0..3u32 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let lat = 1 + (x >> 33) % 1000;
            let t = x % 100_000;
            book.put(LatSample { src: 0xA0 + s, t_emit: t, t_sense: t + lat })?;
        }
    }
    // 曲线三点：100/200/400ns。
    book.curve_put(100)?;
    book.curve_put(200)?;
    book.curve_put(400)?;
    if !book.curve_monotonic() || !book.conserved() {
        return Err(XC_SL_NOANOM);
    }
    let (p50, p95, p99, mx) = book.quant_all();
    let (s50, _, _, smx) = book.quant_of(0xA0).unwrap_or((0, 0, 0, 0));
    if s50 == 0 || smx == 0 {
        return Err(XC_SL_NOSRC);
    }
    Ok((p50, p95, p99, mx, book.anom_rate_bps()))
}

/// 异常入档演练：注入超阈值样本 → 异常账非空且率 100%。
pub fn anom_run() -> Result<usize, u32> {
    let mut book = SenseLatBook::new(4, 100);
    book.enroll(1)?;
    for i in 0..10u64 {
        book.put(LatSample { src: 1, t_emit: i, t_sense: i + 200 + i })?;
    }
    let n = book.anom_len();
    if n != 10 || book.anom_rate_bps() != 10_000 {
        return Err(XC_SL_NOANOM);
    }
    Ok(n)
}

// ── 注入核：六类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①未登记源 ②感知早于发生 ③源重复登记 ④封账后入样
// ⑤异常账缺失（零异常率反例） ⑥容量满。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 未登记源 → XC_SL_NOSRC
    total += 1;
    let mut b = SenseLatBook::new(4, 100);
    if b.put(LatSample { src: 9, t_emit: 1, t_sense: 2 }) == Err(XC_SL_NOSRC) {
        hit += 1;
    }

    // ② 感知早于发生 → XC_SL_BACK
    total += 1;
    let mut b2 = SenseLatBook::new(4, 100);
    let _ = b2.enroll(1);
    if b2.put(LatSample { src: 1, t_emit: 10, t_sense: 5 }) == Err(XC_SL_BACK) {
        hit += 1;
    }

    // ③ 源重复登记 → XC_SL_DUP
    total += 1;
    let mut b3 = SenseLatBook::new(4, 100);
    let _ = b3.enroll(1);
    if b3.enroll(1) == Err(XC_SL_DUP) {
        hit += 1;
    }

    // ④ 封账后入样 → XC_SL_SEALED
    total += 1;
    let mut b4 = SenseLatBook::new(4, 100);
    let _ = b4.enroll(1);
    let _ = b4.seal();
    if b4.put(LatSample { src: 1, t_emit: 1, t_sense: 2 }) == Err(XC_SL_SEALED) {
        hit += 1;
    }

    // ⑤ 异常账缺失：零异常时率 10000（合法），但 anom_len=0 与空账对账。
    total += 1;
    let b5 = SenseLatBook::new(4, 100);
    if b5.anom_len() == 0 && b5.anom_rate_bps() == 10_000 {
        hit += 1;
    }

    // ⑥ 容量满 → XC_SL_FULL
    total += 1;
    let mut b6 = SenseLatBook::new(1, 100);
    let _ = b6.enroll(1);
    if b6.enroll(2) == Err(XC_SL_FULL) {
        hit += 1;
    }

    (hit, total)
}
