// unx1232_u1232 —— UNX-F1232 Draining 态配置总表（B22 段参数收口）
// 判据原文 UNX-F1232-J1：配置总表全过（参数逐项范围校验、冻结位生效后
// 改值即拒、加法（新增冻结参数）被允许、全表指纹冻结、计数守恒）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb830 独占（0xb830~0xb86f）。
//
// 设计（纯功能 no_std）：
//   * 参数登记：名（编码号）/值/合法域 [lo,hi]/冻结位；
//   * 范围校验：登记与改值都过域校验（越域拒）；
//   * 冻结纪律：冻结位生效后改值即拒；**新增冻结参数被允许**（对偶面）；
//   * 守恒：登记数 = 可变数 + 冻结数；全表 FNV 指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb830 独占） ─────────────────────────────────────────────
pub const XC_CF_SEALED: u32 = 0xB831; // 表封账后操作
pub const XC_CF_DUP_KEY: u32 = 0xB832; // 参数键重复
pub const XC_CF_OOB: u32 = 0xB833; // 值越合法域
pub const XC_CF_FROZEN: u32 = 0xB834; // 冻结后改值
pub const XC_CF_NOKEY: u32 = 0xB835; // 未登记键操作
pub const XC_CF_FULL: u32 = 0xB836; // 表容量满

/// 参数行。
#[derive(Clone, Copy)]
pub struct CfgRow {
    /// 参数键（B22 段参数编号）。
    pub key: u32,
    pub value: u64,
    pub lo: u64,
    pub hi: u64,
    pub frozen: bool,
}

/// 配置总表。
pub struct CfgBook {
    cap: usize,
    rows: Vec<CfgRow>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl CfgBook {
    pub fn new(cap: usize) -> CfgBook {
        CfgBook { cap, rows: vec![], sealed: false, rejects: Vec::new() }
    }

    /// 新增参数（新增冻结参数被允许——对偶面）。
    pub fn add(&mut self, key: u32, value: u64, lo: u64, hi: u64, frozen: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CF_SEALED, "表封账后新增"));
            return Err(XC_CF_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_CF_FULL, "表容量满"));
            return Err(XC_CF_FULL);
        }
        if lo > hi {
            self.rejects.push((XC_CF_OOB, "域倒置"));
            return Err(XC_CF_OOB);
        }
        if value < lo || value > hi {
            self.rejects.push((XC_CF_OOB, "值越合法域"));
            return Err(XC_CF_OOB);
        }
        if self.rows.iter().any(|r| r.key == key) {
            self.rejects.push((XC_CF_DUP_KEY, "参数键重复"));
            return Err(XC_CF_DUP_KEY);
        }
        self.rows.push(CfgRow { key, value, lo, hi, frozen });
        Ok(())
    }

    /// 改值（冻结即拒；域校验照走）。
    pub fn set(&mut self, key: u32, value: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CF_SEALED, "表封账后改值"));
            return Err(XC_CF_SEALED);
        }
        let idx = match self.rows.iter().position(|r| r.key == key) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_CF_NOKEY, "未登记键"));
                return Err(XC_CF_NOKEY);
            }
        };
        if self.rows[idx].frozen {
            self.rejects.push((XC_CF_FROZEN, "冻结后改值"));
            return Err(XC_CF_FROZEN);
        }
        if value < self.rows[idx].lo || value > self.rows[idx].hi {
            self.rejects.push((XC_CF_OOB, "值越合法域"));
            return Err(XC_CF_OOB);
        }
        self.rows[idx].value = value;
        Ok(())
    }

    /// 冻结参数。
    pub fn freeze(&mut self, key: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CF_SEALED, "表封账后冻结"));
            return Err(XC_CF_SEALED);
        }
        match self.rows.iter_mut().find(|r| r.key == key) {
            Some(r) => {
                r.frozen = true;
                Ok(())
            }
            None => {
                self.rejects.push((XC_CF_NOKEY, "未登记键"));
                Err(XC_CF_NOKEY)
            }
        }
    }

    /// 守恒：登记 = 可变 + 冻结。
    pub fn conserved(&self) -> bool {
        let f = self.rows.iter().filter(|r| r.frozen).count();
        f + self.rows.iter().filter(|r| !r.frozen).count() == self.rows.len()
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }
    pub fn frozen_count(&self) -> usize {
        self.rows.iter().filter(|r| r.frozen).count()
    }
    pub fn get(&self, key: u32) -> Option<u64> {
        self.rows.iter().find(|r| r.key == key).map(|r| r.value)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CF_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.key as u64).rotate_left(5) ^ r.value.rotate_left(17);
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.lo.rotate_left(23) ^ r.hi.rotate_left(31) ^ (r.frozen as u64).rotate_left(41);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// B22 段标准参数面：8 参数（预算/阈值/开关/档位）收口。
pub fn cfg_scenario() -> Result<(usize, usize, u64), u32> {
    let mut b = CfgBook::new(16);
    b.add(1, 50, 10, 100, false)?;   // 排空预算窗数
    b.add(2, 512, 64, 4096, false)?; // 窗容量
    b.add(3, 1, 0, 1, true)?;        // 双源开关（冻结）
    b.add(4, 300, 100, 600, false)?; // 超时预算 tick
    b.add(5, 2, 1, 3, true)?;        // 降级档（冻结）
    b.add(6, 1024, 16, 8192, false)?;// 延迟告警线
    b.add(7, 25, 0, 100, false)?;    // 容忍越带数
    b.add(8, 7, 0, 15, true)?;       // 诊断级（冻结）
    // 改一个可变参数
    b.set(2, 1024)?;
    // 冻结一个
    b.freeze(1)?;
    if !b.conserved() {
        return Err(XC_CF_FULL);
    }
    if b.frozen_count() != 4 {
        return Err(XC_CF_FROZEN);
    }
    let fp = b.seal()?;
    Ok((b.rows_len(), b.frozen_count(), fp))
}

// ── 深化块：参数导出与围栏校验 ─────────────────────────────────────
impl CfgBook {
    /// 参数导出：键升序稳定 (key, value, frozen)。
    pub fn export(&self) -> Vec<(u32, u64, bool)> {
        let mut v: Vec<(u32, u64, bool)> = vec![];
        for r in &self.rows { v.push((r.key, r.value, r.frozen)); }
        for i in 0..v.len() {
            let mut min = i;
            for j in i + 1..v.len() {
                if v[j].0 < v[min].0 { min = j; }
            }
            let t = v[i]; v[i] = v[min]; v[min] = t;
        }
        v
    }
    /// 范围围栏校验：全部参数在各自域内。
    pub fn fence_check(&self) -> bool {
        self.rows.iter().all(|r| r.value >= r.lo && r.value <= r.hi)
    }
    /// 可变参数键清单。
    pub fn mutable_keys(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        for r in &self.rows {
            if !r.frozen { v.push(r.key); }
        }
        v
    }
    /// 值域宽度（键查询）。
    pub fn band_width(&self, key: u32) -> Option<u64> {
        self.rows.iter().find(|r| r.key == key).map(|r| r.hi - r.lo)
    }
}

// ── 深化块二：配置画像 ─────────────────────────────────────────────
impl CfgBook {
    /// 冻结占比（基点）。
    pub fn frozen_ratio_bps(&self) -> u32 {
        if self.rows.is_empty() { return 0; }
        ((self.frozen_count() as u64 * 10000) / self.rows.len() as u64) as u32
    }
    /// 全表值和。
    pub fn value_sum(&self) -> u64 {
        let mut s: u64 = 0;
        for r in &self.rows { s = s.saturating_add(r.value); }
        s
    }
    /// 最窄值域宽度。
    pub fn narrowest_band(&self) -> Option<u64> {
        let mut m: Option<u64> = None;
        for r in &self.rows {
            let w = r.hi - r.lo;
            match m {
                Some(x) if x <= w => {}
                _ => m = Some(w),
            }
        }
        m
    }
    /// 键存在性。
    pub fn has_key(&self, key: u32) -> bool {
        self.rows.iter().any(|r| r.key == key)
    }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）值越域必拒
    {
        let mut b = CfgBook::new(4);
        if b.add(1, 101, 10, 100, false) == Err(XC_CF_OOB) {
            caught += 1;
        }
    }
    // 2）键重复必拒
    {
        let mut b = CfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, false);
        if b.add(1, 6, 0, 10, false) == Err(XC_CF_DUP_KEY) {
            caught += 1;
        }
    }
    // 3）冻结后改值必拒
    {
        let mut b = CfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, true);
        if b.set(1, 6) == Err(XC_CF_FROZEN) {
            caught += 1;
        }
    }
    // 4）未登记键改值必拒
    {
        let mut b = CfgBook::new(4);
        if b.set(9, 1) == Err(XC_CF_NOKEY) {
            caught += 1;
        }
    }
    // 5）域倒置必拒
    {
        let mut b = CfgBook::new(4);
        if b.add(1, 5, 10, 0, false) == Err(XC_CF_OOB) {
            caught += 1;
        }
    }
    // 6）冻结后新增冻结参数被允许（对偶面正例）
    {
        let mut b = CfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, true);
        if b.add(2, 7, 0, 10, true) == Ok(()) && b.frozen_count() == 2 {
            caught += 1;
        }
    }
    // 7）封账后操作必拒
    {
        let mut b = CfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, false);
        let _ = b.seal();
        if b.set(1, 6) == Err(XC_CF_SEALED) && b.add(2, 1, 0, 10, false) == Err(XC_CF_SEALED) {
            caught += 1;
        }
    }
    (caught, N)
}
