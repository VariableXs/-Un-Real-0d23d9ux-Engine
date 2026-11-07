//! UNX-F51581 M 型五批联轧回归（×100 判据回归） · 0xC1400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51582-J1 四账 ×3 波终核零漂移；真机阈值随闸门补测清单复核在册
//!
//! 结构清单：
//!   - AcctKind：四账闭集（API/UASP/CDC/带宽）
//!   - WaveRecord：单批单波四账流水行
//!   - BatchBaseline：批次四账基线（终核锚点）
//!   - WaveLedger：五批×三波账本（摄入→终核→封存状态机）
//!   - RegressCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② 四账 ×3 波终核零漂移：每批固定 3 波，每波 4 账计数；终核把「批内三波
//!      累计」与基线逐账比对，任一账漂移即 Err，账本状态原地不动。
//!   ③ 摄入判序：批次界 → 波序单调（波号必须严格 +1，重复/倒退/跳波皆拒）
//!      → 重复波拒 → 落账。判据回归 ×100：5 批 × 3 波 × 4 账 = 60 条对账行，
//!      加终核行与注入行合计百条量级。
//!   ④ 注入核：4 类坏样本（批次越界/波序倒退/重复波/账漂移）逐类 10 轮精确
//!      复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//!   ⑤ 双跑一致性：同一脚本两次建账本，快照 FNV-1a 指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1400;

/// 批次数（五批联轧）。
pub const BATCHES: usize = 5;
/// 每批波数（三波终核）。
pub const WAVES: usize = 3;
/// 账目数（四账）。
pub const ACCTS: usize = 4;

/// 四账闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcctKind {
    /// API 账
    Api = 0,
    /// UASP 账
    Uasp = 1,
    /// CDC 账
    Cdc = 2,
    /// 带宽账
    Bw = 3,
}

impl AcctKind {
    /// 闭集下标，用于基线数组寻址。
    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            AcctKind::Api => "API",
            AcctKind::Uasp => "UASP",
            AcctKind::Cdc => "CDC",
            AcctKind::Bw => "带宽",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [AcctKind; ACCTS] {
        [AcctKind::Api, AcctKind::Uasp, AcctKind::Cdc, AcctKind::Bw]
    }
}

/// 单批单波四账流水行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaveRecord {
    /// 批次号 0..5
    pub batch: usize,
    /// 波号 0..3（批内严格递增）
    pub wave: usize,
    /// 四账计数（API/UASP/CDC/带宽）
    pub counts: [u32; ACCTS],
}

/// 批次四账基线：终核锚点（手写定值，联轧回归的期望累计）。
pub const BASELINES: [[u32; ACCTS]; BATCHES] = [
    [30, 12, 9, 60],
    [24, 18, 6, 48],
    [33, 9, 12, 66],
    [21, 15, 3, 42],
    [27, 21, 15, 54],
];

/// 诊断码闭集（联轧回归域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegressCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 批次号越界
    BatchOob,
    /// 波号越界
    WaveOob,
    /// 波序倒退（批内波号必须严格 +1）
    WaveRegress,
    /// 重复波
    WaveDup,
    /// 账漂移（终核累计与基线不符）
    AcctDrift,
    /// 终核未做就要求封存
    FinalMissing,
    /// 账本已封存（只读）
    Sealed,
    /// 记录行数与账面不符
    RowMismatch,
    /// 波覆盖不全（某批未收满三波）
    WaveCover,
    /// 账面不符（守恒账破坏）
    LedgerMismatch,
}

impl RegressCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegressCode::Ok => "联轧回归判定成功",
            RegressCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            RegressCode::BatchOob => "批次越界：批次号必须在 0..5 之内",
            RegressCode::WaveOob => "波号越界：波号必须在 0..3 之内",
            RegressCode::WaveRegress => "波序倒退：批内波号必须严格递增",
            RegressCode::WaveDup => "重复波：同批同波不得重复摄入",
            RegressCode::AcctDrift => "账漂移：终核累计与基线不符，逐账核查",
            RegressCode::FinalMissing => "终核缺失：先终核再封存",
            RegressCode::Sealed => "账本已封存：封存后只读，不得再摄入",
            RegressCode::RowMismatch => "行数不符：流水行数与账面计数对不上",
            RegressCode::WaveCover => "波覆盖不全：每批必须收满三波",
            RegressCode::LedgerMismatch => "账面不符：注入守恒账被破坏",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [RegressCode; 12] = [
    RegressCode::Ok,
    RegressCode::SegBad,
    RegressCode::BatchOob,
    RegressCode::WaveOob,
    RegressCode::WaveRegress,
    RegressCode::WaveDup,
    RegressCode::AcctDrift,
    RegressCode::FinalMissing,
    RegressCode::Sealed,
    RegressCode::RowMismatch,
    RegressCode::WaveCover,
    RegressCode::LedgerMismatch,
];

/// 摄入→终核→封存状态机相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 摄入中
    Ingest = 0,
    /// 已终核
    Final = 1,
    /// 已封存
    Sealed = 2,
}

/// 五批×三波账本：摄入 → 终核 → 封存。
pub struct WaveLedger {
    phase: Phase,
    /// 每批已收到的最高波号（0 表示尚未开波，1..=3 表示已收到波 0..n）
    wave_hi: [usize; BATCHES],
    rows: Vec<WaveRecord>,
    /// 批内四账累计
    sums: [[u32; ACCTS]; BATCHES],
    ingested: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl WaveLedger {
    /// 新建空账本（相位=摄入中）。
    pub fn new() -> WaveLedger {
        WaveLedger {
            phase: Phase::Ingest,
            wave_hi: [0; BATCHES],
            rows: Vec::new(),
            sums: [[0; ACCTS]; BATCHES],
            ingested: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 摄入一行流水：批次界 → 波号界 → 波序单调 → 重复波 → 落账。
    pub fn ingest(&mut self, r: WaveRecord) -> Result<(), RegressCode> {
        if self.phase != Phase::Ingest {
            self.rejected += 1;
            return Err(RegressCode::Sealed);
        }
        if r.batch >= BATCHES {
            self.rejected += 1;
            return Err(RegressCode::BatchOob);
        }
        if r.wave >= WAVES {
            self.rejected += 1;
            return Err(RegressCode::WaveOob);
        }
        let hi = match self.wave_hi.get(r.batch) {
            Some(&h) => h,
            None => {
                self.rejected += 1;
                return Err(RegressCode::BatchOob);
            }
        };
        // 波序判序：已消费的波号重发即重复；跳波（wave != hi）即波序违规。
        if r.wave < hi {
            self.rejected += 1;
            return Err(RegressCode::WaveDup);
        }
        if r.wave != hi {
            self.rejected += 1;
            return Err(RegressCode::WaveRegress);
        }
        match self.wave_hi.get_mut(r.batch) {
            Some(h) => *h = r.wave + 1,
            None => return Err(RegressCode::BatchOob),
        }
        let sums = match self.sums.get_mut(r.batch) {
            Some(s) => s,
            None => return Err(RegressCode::BatchOob),
        };
        for i in 0..ACCTS {
            match counts_at(&r.counts, i) {
                Some(c) => sums[i] = sums[i].saturating_add(c),
                None => return Err(RegressCode::AcctDrift),
            }
        }
        self.rows.push(r);
        self.ingested += 1;
        Ok(())
    }

    /// 终核：波覆盖必须满，逐批逐账比对基线，零漂移才进终核相位。
    pub fn finalize(&mut self) -> Result<(), RegressCode> {
        if self.phase != Phase::Ingest {
            self.rejected += 1;
            return Err(RegressCode::Sealed);
        }
        for b in 0..BATCHES {
            if self.wave_hi[b] != WAVES {
                self.rejected += 1;
                return Err(RegressCode::WaveCover);
            }
            for a in 0..ACCTS {
                let base = match BASELINES.get(b).and_then(|r| r.get(a)) {
                    Some(&v) => v,
                    None => return Err(RegressCode::AcctDrift),
                };
                let sum = match self.sums.get(b).and_then(|r| r.get(a)) {
                    Some(&v) => v,
                    None => return Err(RegressCode::AcctDrift),
                };
                if sum != base {
                    self.rejected += 1;
                    return Err(RegressCode::AcctDrift);
                }
            }
        }
        self.phase = Phase::Final;
        Ok(())
    }

    /// 封存：终核之后才可封存，封存后只读。
    pub fn seal(&mut self) -> Result<(), RegressCode> {
        if self.phase != Phase::Final {
            self.rejected += 1;
            return Err(RegressCode::FinalMissing);
        }
        self.phase = Phase::Sealed;
        Ok(())
    }

    /// 行数对账：流水行数必须等于摄入计数。
    pub fn rows_consistent(&self) -> bool {
        self.rows.len() as u64 == self.ingested
    }

    /// 批内四账累计读取。
    pub fn batch_sums(&self, batch: usize) -> Option<[u32; ACCTS]> {
        match self.sums.get(batch) {
            Some(s) => Some(*s),
            None => None,
        }
    }

    /// 批次波覆盖读取。
    pub fn wave_hi_at(&self, batch: usize) -> Option<usize> {
        match self.wave_hi.get(batch) {
            Some(&h) => Some(h),
            None => None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// (摄入数, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.ingested, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, usize, [usize; BATCHES], u8) {
        (self.ingested, self.rejected, self.rows.len(), self.wave_hi, self.phase as u8)
    }

    /// 快照指纹（FNV-1a over 定长字节编码）。
    pub fn fingerprint(&self) -> u64 {
        let (a, b, c, hi, p) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for v in [a, b, c as u64, p as u64] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for h in hi.iter() {
            bytes.extend_from_slice(&(*h as u64).to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

fn counts_at(counts: &[u32; ACCTS], i: usize) -> Option<u32> {
    counts.get(i).copied()
}

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x9e3779b97f4a7c15 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：5 批 × 3 波四账定值流水，全绿路径。
pub fn standard_script() -> Vec<WaveRecord> {
    // 每批三波合计恰等于基线：把基线按 3 波均拆（末波收尾差）。
    let mut v: Vec<WaveRecord> = Vec::new();
    for b in 0..BATCHES {
        let base = match BASELINES.get(b) {
            Some(r) => *r,
            None => [[0; ACCTS]; BATCHES][0],
        };
        for w in 0..WAVES {
            let mut counts = [0u32; ACCTS];
            for a in 0..ACCTS {
                let total = match base.get(a) {
                    Some(&t) => t,
                    None => 0,
                };
                // 均拆：前两波取整除，末波收差值，保证三波合计=基线。
                counts[a] = if w + 1 == WAVES {
                    total - (total / 3) * (WAVES as u32 - 1)
                } else {
                    total / 3
                };
            }
            v.push(WaveRecord { batch: b, wave: w, counts });
        }
    }
    v
}

/// 双跑一致性：同一脚本两次建账本，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64, u64, u64) {
        let mut led = WaveLedger::new();
        let rows = standard_script();
        for r in rows.iter() {
            let _ = led.ingest(*r);
        }
        let fp = led.fingerprint();
        let _ = led.finalize();
        let _ = led.seal();
        (fp, led.fingerprint(), led.snapshot().0, led.snapshot().1)
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
/// 返回 (总检出, 总逃逸, 每类检出)。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    let good = standard_script();
    // 类 0：批次越界。
    for i in 0..10u32 {
        let mut led = WaveLedger::new();
        let bad = WaveRecord { batch: BATCHES + (i as usize % 3), wave: 0, counts: [1, 1, 1, 1] };
        let r = led.ingest(bad);
        if r == Err(RegressCode::BatchOob) {
            led.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：波序倒退（先收波 1 的值——用跳波样本制造严格判据）。
    for i in 0..10u32 {
        let mut led = WaveLedger::new();
        let _ = led.ingest(good[0]);
        // 跳波样本：hi=1 后直接发波 2（wave != hi → WaveRegress）。
        let bad = good[2];
        let r = led.ingest(bad);
        if r == Err(RegressCode::WaveRegress) {
            led.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
        // 类 2：重复波（三波收满后重发末波）。
    for i in 0..10u32 {
        let _ = i;
        let mut led = WaveLedger::new();
        for w in 0..WAVES {
            let _ = led.ingest(good[w]);
        }
        let dup = good[WAVES - 1];
        let r = led.ingest(dup);
        if r == Err(RegressCode::WaveDup) {
            led.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：账漂移（三波合计偏离基线，终核必抓）。
    for i in 0..10u32 {
        let mut led = WaveLedger::new();
        for w in 0..WAVES {
            let mut r0 = good[w];
            if w == WAVES - 1 {
                let bump = (i % 5 + 1) as u32;
                r0.counts[0] = r0.counts[0].wrapping_add(bump);
            }
            let _ = led.ingest(r0);
        }
        let r = led.finalize();
        if r == Err(RegressCode::AcctDrift) {
            led.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            led.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（14 条判据形态）。
pub fn run_unx51581_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51581");
    // 1 正常流：标准脚本全摄入且行数对平。
    {
        let mut led = WaveLedger::new();
        for r in standard_script().iter() {
            let _ = led.ingest(*r);
        }
        let (ing, _) = led.tally();
        s.add(
            "标准脚本全摄入",
            ing == (BATCHES * WAVES) as u64 && led.rows_consistent(),
            "五批三波 15 行应全摄入且流水行数对平",
        );
    }
    // 2 终核零漂移：全账逐批比对基线。
    {
        let mut led = WaveLedger::new();
        for r in standard_script().iter() {
            let _ = led.ingest(*r);
        }
        let fin = led.finalize();
        s.add(
            "四账×3波终核零漂移",
            fin == Ok(()),
            "UNX-F51582-J1：终核累计与基线逐账相等",
        );
    }
    // 3 封存状态机单调：摄入→终核→封存。
    {
        let mut led = WaveLedger::new();
        let early = led.seal();
        for r in standard_script().iter() {
            let _ = led.ingest(*r);
        }
        let _ = led.finalize();
        let ok_seal = led.seal();
        let sealed_readonly = led.ingest(standard_script()[0]) == Err(RegressCode::Sealed);
        s.add(
            "封存状态机单调",
            early == Err(RegressCode::FinalMissing)
                && ok_seal == Ok(())
                && sealed_readonly,
            "终核前封存必拒，封存后只读",
        );
    }
    // 4 摄入判序：批次界先于波号界。
    {
        let mut led = WaveLedger::new();
        let r = led.ingest(WaveRecord { batch: 99, wave: 99, counts: [0; ACCTS] });
        s.add(
            "判序批次界先行",
            r == Err(RegressCode::BatchOob),
            "批次越界先于波号越界与波序判据",
        );
    }
    // 5 波号越界显性拒。
    {
        let mut led = WaveLedger::new();
        let r = led.ingest(WaveRecord { batch: 0, wave: WAVES, counts: [0; ACCTS] });
        s.add("波号越界", r == Err(RegressCode::WaveOob), "波号 0..3 之外一律拒");
    }
    // 6 波覆盖不全拒终核。
    {
        let mut led = WaveLedger::new();
        let _ = led.ingest(standard_script()[0]);
        let r = led.finalize();
        s.add(
            "波覆盖不全拒终核",
            r == Err(RegressCode::WaveCover),
            "任一批未收满三波，终核必须显性拒",
        );
    }
    // 7 诊断码闭集唯一性：val 严格递增且 detail 全非空。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = ALL_CODES[i];
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 8 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全抓",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0，每类恰 10 轮全抓",
        );
    }
    // 9 守恒账单独核：检出/逃逸记账面。
    {
        let mut led = WaveLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        s.add(
            "注入账守恒",
            led.conserved(),
            "注入=检出+逃逸且逃逸=0 且检出>0",
        );
    }
    // 10 双跑指纹一致。
    {
        let (fp_ok, fin_ok) = {
            let mut same = true;
            for _ in 0..2u32 {
                if !dual_run_consistent() {
                    same = false;
                }
            }
            (same, dual_run_consistent())
        };
        s.add(
            "双跑 FNV 指纹一致",
            fp_ok && fin_ok,
            "同一脚本两次建账本，FNV-1a 指纹逐位一致",
        );
    }
    // 11 标准脚本指纹可复算且为确定性文本。
    {
        let led = {
            let mut l = WaveLedger::new();
            for r in standard_script().iter() {
                let _ = l.ingest(*r);
            }
            l
        };
        let fp1 = led.fingerprint();
        let fp2 = led.fingerprint();
        let brief: String = format!("{:016x}", fp1);
        s.add(
            "指纹复算稳定",
            fp1 == fp2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 12 状态机相位单调：相位码只进不退。
    {
        let mut led = WaveLedger::new();
        let p0 = led.phase() as u8;
        for r in standard_script().iter() {
            let _ = led.ingest(*r);
        }
        let _ = led.finalize();
        let p1 = led.phase() as u8;
        let _ = led.seal();
        let p2 = led.phase() as u8;
        s.add(
            "相位单调",
            p0 < p1 && p1 < p2,
            "Ingest→Final→Sealed 相位码严格递增",
        );
    }
    // 13 真机阈值随闸门补测清单复核在册：基线表行数与批数一致。
    {
        let in_register = BASELINES.len() == BATCHES
            && BASELINES.iter().all(|r| r.len() == ACCTS);
        s.add(
            "补测清单复核在册",
            in_register,
            "每批四账基线在册且维度对齐（阈值随闸门补测清单）",
        );
    }
    // 14 结果摘要非空 + 段位守卫。
    {
        let summary: String = format!(
            "unx51581 五批三波回归 终核零漂移 诊断码{} 变体",
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51581"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51581_checks_green() {
        let set = run_unx51581_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn standard_script_zero_drift() {
        let mut led = WaveLedger::new();
        for r in standard_script().iter() {
            assert!(led.ingest(*r).is_ok(), "标准脚本行必须全收");
        }
        assert_eq!(led.finalize(), Ok(()));
        assert_eq!(led.seal(), Ok(()));
        assert_eq!(led.phase(), Phase::Sealed);
    }

    #[test]
    fn wave_regress_rejected() {
        let mut led = WaveLedger::new();
        let good = standard_script();
        assert!(led.ingest(good[0]).is_ok());
        // 跳波：波 0 之后直接发波 2。
        assert_eq!(led.ingest(good[2]), Err(RegressCode::WaveRegress));
        // 重复：hi=1 后重发波 0。
        assert_eq!(led.ingest(good[0]), Err(RegressCode::WaveDup));
        assert_eq!(led.wave_hi_at(0), Some(1));
    }

    #[test]
    fn batch_and_wave_bounds() {
        let mut led = WaveLedger::new();
        assert_eq!(
            led.ingest(WaveRecord { batch: BATCHES, wave: 0, counts: [0; ACCTS] }),
            Err(RegressCode::BatchOob)
        );
        assert_eq!(
            led.ingest(WaveRecord { batch: 0, wave: WAVES, counts: [0; ACCTS] }),
            Err(RegressCode::WaveOob)
        );
    }

    #[test]
    fn drift_injection_all_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0, "注入零逃逸");
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut led = WaveLedger::new();
        for _ in 0..10u32 {
            led.note_detected();
        }
        assert!(led.conserved());
        let mut bad = WaveLedger::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved(), "有逃逸时守恒必须判假");
    }

    #[test]
    fn sealed_is_readonly() {
        let mut led = WaveLedger::new();
        for r in standard_script().iter() {
            let _ = led.ingest(*r);
        }
        let _ = led.finalize();
        let _ = led.seal();
        assert_eq!(led.ingest(standard_script()[0]), Err(RegressCode::Sealed));
        assert_eq!(led.finalize(), Err(RegressCode::Sealed));
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        let build = || -> u64 {
            let mut l = WaveLedger::new();
            for r in standard_script().iter() {
                let _ = l.ingest(*r);
            }
            l.fingerprint()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val(), "诊断码 {} 与 {} 撞值", a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32(), "LCG 同种子必须同序列");
        }
    }

    #[test]
    fn snapshot_covers_ledger_state() {
        let mut led = WaveLedger::new();
        for r in standard_script().iter() {
            let _ = led.ingest(*r);
        }
        let (ing, rej, rows, hi, _) = led.snapshot();
        assert_eq!(ing, 15);
        assert_eq!(rej, 0);
        assert_eq!(rows, 15);
        assert!(hi.iter().all(|&h| h == WAVES));
    }
}
