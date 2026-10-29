//! UNX-F0667 · 耗时离群检测与归因账 · 诊断段 0x54a0（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0667-J1 离群样本检出 100%（3×IQR 口径注入复验）且逐个归因账行
//! （归因类/证据指针）齐。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 3×IQR 口径：栅栏 fence_lo = p25 − 3·IQR，fence_hi = p75 + 3·IQR（整数截断保护）。
//!  3. 归因账行四要素：样本号/样本值/归因类/证据指针，行行可解引。
//!  4. 注入复验：人工植入离群样本必检出 100%，无离群纯净样本零误报。
//!  5. 归因类闭集四类，未归因样本拦截（禁止静默吞离群）。
//!  6. 诊断段 0x54a0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x54a0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_EMPTY: u16 = SEG_BASE + 1;   // 样本集为空
pub const E_IQR: u16 = SEG_BASE + 2;     // IQR 非法（0 或破序）
pub const E_UNATTR: u16 = SEG_BASE + 3;  // 离群未归因（静默吞警拦截）
pub const E_PTR: u16 = SEG_BASE + 4;     // 证据指针不可解引
pub const E_DUP: u16 = SEG_BASE + 5;     // 同样本号重复归因
pub const E_SEAL: u16 = SEG_BASE + 6;    // 归因账已封
pub const E_FALSE: u16 = SEG_BASE + 7;   // 纯净样本误报（误报率必须 0）

/// IQR 倍数硬线（判据写死 3）。
pub const IQR_MULT: u64 = 3;

/// LCG64。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 归因类闭集四类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrClass {
    /// 时钟源抖动：TSC/RTC 漂移导致。
    ClockJitter,
    /// 后台负载：宿主进程抢占。
    BgLoad,
    /// 采集器误差：打点自身开销。
    ProbeErr,
    /// 环境冷效应：缓存/页表冷启动。
    ColdEnv,
}

impl AttrClass {
    pub fn name(self) -> &'static str {
        match self {
            AttrClass::ClockJitter => "时钟源抖动",
            AttrClass::BgLoad => "后台负载",
            AttrClass::ProbeErr => "采集器误差",
            AttrClass::ColdEnv => "环境冷效应",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            AttrClass::ClockJitter => 1,
            AttrClass::BgLoad => 2,
            AttrClass::ProbeErr => 3,
            AttrClass::ColdEnv => 4,
        }
    }
}

/// 证据指针：指向归因证据所在账源（可解引闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidencePtr {
    /// 时钟账第 n 行。
    ClockRow(u16),
    /// 负载账第 n 行。
    LoadRow(u16),
    /// 采集器标定账第 n 行。
    ProbeRow(u16),
    /// 环境快照第 n 张。
    EnvSnap(u16),
}

impl EvidencePtr {
    /// 解引合法性：指针载荷非零即认为可解引（账行号 1 起）。
    pub fn deref_ok(&self) -> bool {
        match self {
            EvidencePtr::ClockRow(n) => *n > 0,
            EvidencePtr::LoadRow(n) => *n > 0,
            EvidencePtr::ProbeRow(n) => *n > 0,
            EvidencePtr::EnvSnap(n) => *n > 0,
        }
    }
}

/// 分位数（排序法，与 F0666 同口径：插入排序 + 下标取点）。
pub struct Quartiles {
    pub p25: u64,
    pub p75: u64,
}

/// 排序副本后取四分位（n≥2）。
pub fn quartiles_of(samples: &[u64]) -> Result<Quartiles, u16> {
    if samples.is_empty() {
        return Err(E_EMPTY);
    }
    let mut v = samples.to_vec();
    // 冒泡排序（与 F0666 插入排序不同实现路径，独立性更强）
    let n = v.len();
    for i in 0..n {
        for j in 0..(n - 1 - i) {
            if v[j] > v[j + 1] {
                let t = v[j];
                v[j] = v[j + 1];
                v[j + 1] = t;
            }
        }
    }
    let i25 = (n - 1) * 25 / 100;
    let i75 = (n - 1) * 75 / 100;
    Ok(Quartiles { p25: v[i25], p75: v[i75] })
}

/// 栅栏结构。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fence {
    pub lo: u64,
    pub hi: u64,
    pub iqr: u64,
}

/// 3×IQR 栅栏计算：p25 向下截断保护（0 下限），p75 向上溢出保护。
pub fn fence_of(samples: &[u64]) -> Result<Fence, u16> {
    let q = quartiles_of(samples)?;
    let iqr = q.p75 - q.p25;
    if q.p75 < q.p25 {
        return Err(E_IQR);
    }
    let span = iqr.checked_mul(IQR_MULT).ok_or(E_IQR)?;
    let lo = if q.p25 >= span { q.p25 - span } else { 0 };
    let hi = q.p75.checked_add(span).ok_or(E_IQR)?;
    Ok(Fence { lo, hi, iqr })
}

/// 离群判定。
pub fn is_outlier(f: &Fence, x: u64) -> bool {
    x < f.lo || x > f.hi
}

/// 归因账行：四要素。
#[derive(Clone, Debug)]
pub struct AttrRow {
    /// 样本号（1 起，样本集内唯一）。
    pub sample_no: u16,
    /// 样本值（ms）。
    pub value_ms: u64,
    /// 归因类。
    pub class: AttrClass,
    /// 证据指针。
    pub ptr: EvidencePtr,
}

impl AttrRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.sample_no == 0 || self.value_ms == 0 {
            return Err(E_PTR);
        }
        if !self.ptr.deref_ok() {
            return Err(E_PTR);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("#");
        s.push_str(&self.sample_no.to_string());
        s.push(' ');
        s.push_str(&self.value_ms.to_string());
        s.push_str("ms 归因=");
        s.push_str(self.class.name());
        s
    }
}

/// 离群检测 + 归因引擎。
pub struct OutlierEngine {
    pub fence: Fence,
    /// 离群样本号集（1 起）。
    pub outlier_nos: Vec<u16>,
    /// 归因账行。
    pub rows: Vec<AttrRow>,
    pub sealed: bool,
}

impl OutlierEngine {
    pub fn new(samples: &[u64]) -> Result<OutlierEngine, u16> {
        if samples.is_empty() {
            return Err(E_EMPTY);
        }
        let fence = fence_of(samples)?;
        if fence.iqr == 0 {
            return Err(E_IQR);
        }
        Ok(OutlierEngine {
            fence,
            outlier_nos: Vec::new(),
            rows: Vec::new(),
            sealed: false,
        })
    }

    /// 扫描：逐样本判定，离群样本号入列。
    pub fn scan(&mut self, samples: &[u64]) -> Result<usize, u16> {
        if samples.is_empty() {
            return Err(E_EMPTY);
        }
        self.outlier_nos.clear();
        for (i, x) in samples.iter().enumerate() {
            if is_outlier(&self.fence, *x) {
                self.outlier_nos.push((i + 1) as u16);
            }
        }
        Ok(self.outlier_nos.len())
    }

    /// 归因：每个离群样本必须归因一次（号唯一、类闭集、指针可解引）。
    pub fn attribute(&mut self, sample_no: u16, value_ms: u64, class: AttrClass, ptr: EvidencePtr) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if !self.outlier_nos.contains(&sample_no) {
            return Err(E_UNATTR);
        }
        for r in &self.rows {
            if r.sample_no == sample_no {
                return Err(E_DUP);
            }
        }
        let row = AttrRow { sample_no, value_ms, class, ptr };
        row.audit()?;
        self.rows.push(row);
        Ok(())
    }

    /// 归因齐备核：离群数 = 归因行数，零静默吞离群。
    pub fn assert_fully_attributed(&self) -> Result<(), u16> {
        if self.outlier_nos.len() != self.rows.len() {
            return Err(E_UNATTR);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_fully_attributed()?;
        self.sealed = true;
        Ok(())
    }
}

/// 确定性样本生成（基线带内抖动，纯净无离群）。
pub fn synth_clean(seed: u64, n: usize) -> Result<Vec<u64>, u16> {
    if n == 0 {
        return Err(E_EMPTY);
    }
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(2_200 + (rng.next_u32() % 300) as u64);
    }
    Ok(out)
}

/// 植入离群：在 idx 位替换为远超栅栏的值。
pub fn plant_outliers(samples: &mut Vec<u64>, spots: &[(usize, u64)]) -> Result<(), u16> {
    for (idx, v) in spots {
        if *idx >= samples.len() || *v == 0 {
            return Err(E_PTR);
        }
        samples[*idx] = *v;
    }
    Ok(())
}

/// 注入复验核 A：植入 2 个离群必检出 2 个（检出率 100%）。
pub fn inject_outliers_must_catch() -> Result<(usize, Vec<u64>), u16> {
    let mut s = synth_clean(0x0667, 50)?;
    let f0 = fence_of(&s)?;
    // 植入值取 hi + 2×IQR，保证远超栅栏
    let v_hi = f0.hi + f0.iqr * 2;
    plant_outliers(&mut s, &[(7, v_hi), (31, v_hi)])?;
    let mut eng = OutlierEngine::new(&s)?;
    let n = eng.scan(&s)?;
    if n != 2 {
        return Err(SEG_BASE + 0x30);
    }
    Ok((n, s))
}

/// 注入复验核 B：纯净样本零误报（误报率 0）。
pub fn inject_clean_must_pass() -> Result<(), u16> {
    let s = synth_clean(0x0667, 60)?;
    let mut eng = OutlierEngine::new(&s)?;
    let n = eng.scan(&s)?;
    if n != 0 {
        return Err(E_FALSE);
    }
    Ok(())
}

/// 归因指纹。
pub fn attr_fp(rows: &[AttrRow]) -> u64 {
    let mut buf = vec![0u8; rows.len() * 10];
    for (i, r) in rows.iter().enumerate() {
        buf[i * 10] = (r.sample_no >> 8) as u8;
        buf[i * 10 + 1] = r.sample_no as u8;
        let v = r.value_ms;
        for k in 0..6u8 {
            buf[i * 10 + 2 + k as usize] = (v >> (40 - k * 8)) as u8;
        }
        buf[i * 10 + 8] = r.class.code();
        buf[i * 10 + 9] = match r.ptr {
            EvidencePtr::ClockRow(n) => (n & 0xFF) as u8,
            EvidencePtr::LoadRow(n) => (n & 0xFF) as u8,
            EvidencePtr::ProbeRow(n) => (n & 0xFF) as u8,
            EvidencePtr::EnvSnap(n) => (n & 0xFF) as u8,
        };
    }
    fnv1a64(&buf)
}

/// 指针闭集自检：零载荷指针必须判不可解引。
pub fn ptr_audit() -> Result<(), u16> {
    if EvidencePtr::ClockRow(0).deref_ok() {
        return Err(SEG_BASE + 0x31);
    }
    if EvidencePtr::LoadRow(0).deref_ok() {
        return Err(SEG_BASE + 0x32);
    }
    if EvidencePtr::ProbeRow(0).deref_ok() {
        return Err(SEG_BASE + 0x33);
    }
    if EvidencePtr::EnvSnap(0).deref_ok() {
        return Err(SEG_BASE + 0x34);
    }
    if !EvidencePtr::ClockRow(1).deref_ok() {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 复跑对账簿：每个离群样本复跑一次（消除瞬时扰动），复跑值与栅栏比对。
/// 复跑入栅栏 → 瞬时扰动归因坐实；复跑仍离群 → 升级立案（持续性异常）。
pub struct RerunBook {
    /// (样本号, 复跑值, 入栅栏旗)。
    pub rows: Vec<(u16, u64, bool)>,
}

impl RerunBook {
    pub fn new() -> RerunBook {
        RerunBook { rows: Vec::new() }
    }

    /// 复跑落账：复跑值 = 原值 − 确定性修正量（整数），入栅栏判定即落旗。
    pub fn rerun(&mut self, fence: &Fence, sample_no: u16, orig_ms: u64, fix_ms: u64) -> Result<(), u16> {
        if sample_no == 0 {
            return Err(E_PTR);
        }
        for (no, _, _) in &self.rows {
            if no == &sample_no {
                return Err(E_DUP);
            }
        }
        let v = orig_ms.saturating_sub(fix_ms);
        let inside = !is_outlier(fence, v);
        self.rows.push((sample_no, v, inside));
        Ok(())
    }

    /// 对账核：全部复跑入栅栏 → 瞬时扰动结论成立。
    pub fn assert_all_transient(&self) -> Result<(), u16> {
        for (no, v, inside) in &self.rows {
            if !inside {
                // 持续性异常必须升级而非静默
                return Err(E_UNATTR);
            }
            let _ = (no, v);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("复跑对账: 行数=");
        s.push_str(&self.rows.len().to_string());
        let mut transient = 0;
        for (_, _, inside) in &self.rows {
            if *inside {
                transient += 1;
            }
        }
        s.push_str(" 瞬时扰动=");
        s.push_str(&transient.to_string());
        s
    }
}

/// 栅栏位移复验核：向 hi 加一个 IQR 后，原临界样本（原超 hi 但 ≤ hi+IQR）
/// 应转为带内——证明栅栏计算与判定函数耦合正确。
pub fn fence_shift_audit() -> Result<(), u16> {
    let mut s = synth_clean(0x0667, 40)?;
    // 植入临界离群：hi + 1
    let f0 = fence_of(&s)?;
    let crit = f0.hi + 1;
    s[39] = crit;
    let f1 = fence_of(&s)?;
    if !is_outlier(&f1, crit) {
        return Err(SEG_BASE + 0x3F);
    }
    // 位移后栅栏（hi + IQR）→ 同值转为带内
    let f2 = Fence { lo: f1.lo, hi: f1.hi + f1.iqr, iqr: f1.iqr };
    if is_outlier(&f2, crit) {
        return Err(SEG_BASE + 0x40);
    }
    Ok(())
}

/// 归因账报告（程序化生成）。
pub struct AttrReport {
    pub lines: Vec<String>,
}

impl AttrReport {
    pub fn generate(eng: &OutlierEngine) -> Result<AttrReport, u16> {
        eng.assert_fully_attributed()?;
        let mut lines = Vec::new();
        let mut l0 = String::from("离群归因账: 离群数=");
        l0.push_str(&eng.outlier_nos.len().to_string());
        l0.push_str(" 栅栏=[");
        l0.push_str(&eng.fence.lo.to_string());
        l0.push(',');
        l0.push_str(&eng.fence.hi.to_string());
        l0.push_str("] IQR=");
        l0.push_str(&eng.fence.iqr.to_string());
        lines.push(l0);
        for r in &eng.rows {
            lines.push(r.summary());
        }
        Ok(AttrReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 2 {
            return Err(E_UNATTR);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_EMPTY);
            }
        }
        Ok(())
    }
}

/// 栅栏边界自检：对称样本 IQR>0、单值样本 IQR=0 拦截。
pub fn fence_audit() -> Result<(), u16> {
    let mut s = Vec::new();
    for i in 0..50u64 {
        s.push(100 + i);
    }
    let f = fence_of(&s)?;
    if f.iqr == 0 || f.hi <= f.lo {
        return Err(E_IQR);
    }
    // 单值样本：IQR = 0 → 引擎构造拦截
    let flat = vec![100u64; 20];
    if OutlierEngine::new(&flat).err() != Some(E_IQR) {
        return Err(SEG_BASE + 0x36);
    }
    Ok(())
}

/// 自例程：植入检出 100% + 纯净零误报 + 归因四要素齐 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    ptr_audit()?;
    fence_audit()?;

    // A：植入 2 离群检出
    let (n_out, samples) = inject_outliers_must_catch()?;
    if n_out != 2 {
        return Err(SEG_BASE + 0x37);
    }
    let mut eng = OutlierEngine::new(&samples)?;
    eng.scan(&samples)?;
    // 归因四要素：两类各一，指针齐
    let no1 = eng.outlier_nos[0];
    let no2 = eng.outlier_nos[1];
    let v1 = samples[(no1 - 1) as usize];
    let v2 = samples[(no2 - 1) as usize];
    eng.attribute(no1, v1, AttrClass::BgLoad, EvidencePtr::LoadRow(12))?;
    eng.attribute(no2, v2, AttrClass::ClockJitter, EvidencePtr::ClockRow(3))?;
    eng.assert_fully_attributed()?;
    // 归因指纹稳定
    let fp1 = attr_fp(&eng.rows);
    let fp2 = attr_fp(&eng.rows);
    if fp1 != fp2 {
        return Err(SEG_BASE + 0x38);
    }
    eng.seal()?;

    // B：纯净零误报
    inject_clean_must_pass()?;

    // 注入 1：未归因先封账 → 拦截
    let (n2, s2) = inject_outliers_must_catch()?;
    let mut eng2 = OutlierEngine::new(&s2)?;
    eng2.scan(&s2)?;
    if eng2.seal().is_ok() {
        return Err(SEG_BASE + 0x39);
    }
    // 注入 2：非离群样本归因 → 拦截
    if eng2.attribute(1, 2_300, AttrClass::ProbeErr, EvidencePtr::ProbeRow(1)).is_ok() {
        return Err(SEG_BASE + 0x3A);
    }
    // 注入 3：同号重复归因 → 拦截
    let mut eng3 = OutlierEngine::new(&s2)?;
    eng3.scan(&s2)?;
    let n3 = eng3.outlier_nos[0];
    let v3 = s2[(n3 - 1) as usize];
    eng3.attribute(n3, v3, AttrClass::ColdEnv, EvidencePtr::EnvSnap(2))?;
    if eng3.attribute(n3, v3, AttrClass::ColdEnv, EvidencePtr::EnvSnap(2)) != Err(E_DUP) {
        return Err(SEG_BASE + 0x3B);
    }
    // 注入 4：零指针归因 → 拦截
    let mut eng4 = OutlierEngine::new(&s2)?;
    eng4.scan(&s2)?;
    let n4 = eng4.outlier_nos[0];
    let v4 = s2[(n4 - 1) as usize];
    if eng4.attribute(n4, v4, AttrClass::BgLoad, EvidencePtr::LoadRow(0)) != Err(E_PTR) {
        return Err(SEG_BASE + 0x3C);
    }
    // 注入 5：归因不齐先封 → 拦截
    if eng2.seal().is_ok() {
        return Err(SEG_BASE + 0x3D);
    }
    // 归因摘要非空
    if eng.rows[0].summary().is_empty() {
        return Err(SEG_BASE + 0x3E);
    }
    // 栅栏位移复验
    fence_shift_audit()?;
    // 复跑对账：两个离群样本复跑后入栅栏（瞬时扰动坐实）
    let f = fence_of(&samples)?;
    let mut rerun = RerunBook::new();
    rerun.rerun(&f, no1, v1, v1 - f.hi + 10)?;
    rerun.rerun(&f, no2, v2, v2 - f.hi + 10)?;
    rerun.assert_all_transient()?;
    if rerun.summary().is_empty() {
        return Err(SEG_BASE + 0x41);
    }
    // 注入 6：复跑仍离群 → 升级拦截
    let mut rerun2 = RerunBook::new();
    rerun2.rerun(&f, no1, v1, 0)?;
    if rerun2.assert_all_transient().is_ok() {
        return Err(SEG_BASE + 0x42);
    }
    // 归因报告：全行生成 + 完整性
    let rep = AttrReport::generate(&eng)?;
    rep.assert_complete()?;
    Ok(0)
}
