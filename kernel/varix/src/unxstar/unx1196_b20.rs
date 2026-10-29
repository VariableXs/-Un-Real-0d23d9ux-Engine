// unx1196_b20 —— UNX-F1196 亲和面配置总表（B20 段参数收口）
// 判据原文 UNX-F1196-J1：配置总表全过（参数逐项范围校验、冻结位生效后
// 改值即拒、新增冻结参数被允许（对偶面）、全表指纹冻结、计数守恒）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8f30 独占（0x8f30~0x8f6f）。
//
// 设计（纯功能 no_std）：
//   * 参数行：键/值/域 [lo,hi]/冻结位；登记与改值双域校验；
//   * 冻结纪律：冻结后改值拒；新增冻结参数允许（对偶）；
//   * 守恒：登记 = 可变 + 冻结；键升序稳定导出；
//   * FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8f30 独占） ─────────────────────────────────────────────
pub const XC_AC_SEALED: u32 = 0x8F31; // 封账后操作
pub const XC_AC_DUP_KEY: u32 = 0x8F32; // 键重复
pub const XC_AC_OOB: u32 = 0x8F33; // 值越域
pub const XC_AC_FROZEN: u32 = 0x8F34; // 冻结后改值
pub const XC_AC_NOKEY: u32 = 0x8F35; // 未登记键
pub const XC_AC_FULL: u32 = 0x8F36; // 容量满

/// 参数行。
#[derive(Clone, Copy)]
pub struct AffCfgRow {
    pub key: u32,
    pub value: u64,
    pub lo: u64,
    pub hi: u64,
    pub frozen: bool,
}

/// 亲和面配置总表。
pub struct AffCfgBook {
    cap: usize,
    rows: Vec<AffCfgRow>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl AffCfgBook {
    pub fn new(cap: usize) -> AffCfgBook {
        AffCfgBook { cap, rows: vec![], sealed: false, rejects: vec![] }
    }

    /// 新增（冻结参数允许）。
    pub fn add(&mut self, key: u32, value: u64, lo: u64, hi: u64, frozen: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AC_SEALED, "封账后新增"));
            return Err(XC_AC_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_AC_FULL, "容量满"));
            return Err(XC_AC_FULL);
        }
        if lo > hi {
            self.rejects.push((XC_AC_OOB, "域倒置"));
            return Err(XC_AC_OOB);
        }
        if value < lo || value > hi {
            self.rejects.push((XC_AC_OOB, "值越域"));
            return Err(XC_AC_OOB);
        }
        if self.rows.iter().any(|r| r.key == key) {
            self.rejects.push((XC_AC_DUP_KEY, "键重复"));
            return Err(XC_AC_DUP_KEY);
        }
        self.rows.push(AffCfgRow { key, value, lo, hi, frozen });
        Ok(())
    }

    /// 改值（冻结即拒）。
    pub fn set(&mut self, key: u32, value: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AC_SEALED, "封账后改值"));
            return Err(XC_AC_SEALED);
        }
        let idx = match self.rows.iter().position(|r| r.key == key) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_AC_NOKEY, "未登记键"));
                return Err(XC_AC_NOKEY);
            }
        };
        if self.rows[idx].frozen {
            self.rejects.push((XC_AC_FROZEN, "冻结后改值"));
            return Err(XC_AC_FROZEN);
        }
        if value < self.rows[idx].lo || value > self.rows[idx].hi {
            self.rejects.push((XC_AC_OOB, "值越域"));
            return Err(XC_AC_OOB);
        }
        self.rows[idx].value = value;
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self, key: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AC_SEALED, "封账后冻结"));
            return Err(XC_AC_SEALED);
        }
        match self.rows.iter_mut().find(|r| r.key == key) {
            Some(r) => {
                r.frozen = true;
                Ok(())
            }
            None => {
                self.rejects.push((XC_AC_NOKEY, "未登记键"));
                Err(XC_AC_NOKEY)
            }
        }
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.frozen_count() + self.rows.iter().filter(|r| !r.frozen).count() == self.rows.len()
    }

    /// 键升序稳定导出。
    pub fn export(&self) -> Vec<(u32, u64, bool)> {
        let mut v: Vec<(u32, u64, bool)> = vec![];
        for r in &self.rows {
            v.push((r.key, r.value, r.frozen));
        }
        for i in 0..v.len() {
            let mut min = i;
            for j in i + 1..v.len() {
                if v[j].0 < v[min].0 { min = j; }
            }
            let t = v[i]; v[i] = v[min]; v[min] = t;
        }
        v
    }

    /// 围栏校验：全部在域内。
    pub fn fence_check(&self) -> bool {
        self.rows.iter().all(|r| r.value >= r.lo && r.value <= r.hi)
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }
    pub fn frozen_count(&self) -> usize {
        self.rows.iter().filter(|r| r.frozen).count()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_AC_SEALED);
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

/// B20 段标准参数面：8 参数收口。
pub fn aff_cfg_scenario() -> Result<(usize, usize, u64), u32> {
    let mut b = AffCfgBook::new(16);
    b.add(1, 4, 1, 8, true)?;      // 在线核数（冻结）
    b.add(2, 10, 1, 100, false)?;  // 每轮配额
    b.add(3, 6400, 100, 12800, true)?; // IPI 预算 ns（冻结）
    b.add(4, 300, 0, 1000, false)?;    // 扰动带（千分比）
    b.add(5, 50, 0, 200, false)?;      // 深 C 预算 µs
    b.add(6, 2, 1, 3, true)?;          // 回退策略档（冻结）
    b.add(7, 1000, 0, 10000, false)?;  // 健警线 bps
    b.add(8, 7, 0, 15, true)?;         // 诊断级（冻结）
    b.set(2, 20)?;
    b.freeze(4)?;
    if !b.conserved() || !b.fence_check() {
        return Err(XC_AC_FULL);
    }
    let fp = b.seal()?;
    Ok((b.rows_len(), b.frozen_count(), fp))
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）值越域必拒
    {
        let mut b = AffCfgBook::new(4);
        if b.add(1, 101, 10, 100, false) == Err(XC_AC_OOB) {
            caught += 1;
        }
    }
    // 2）键重复必拒
    {
        let mut b = AffCfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, false);
        if b.add(1, 6, 0, 10, false) == Err(XC_AC_DUP_KEY) {
            caught += 1;
        }
    }
    // 3）冻结后改值必拒
    {
        let mut b = AffCfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, true);
        if b.set(1, 6) == Err(XC_AC_FROZEN) {
            caught += 1;
        }
    }
    // 4）未登记键必拒
    {
        let mut b = AffCfgBook::new(4);
        if b.set(9, 1) == Err(XC_AC_NOKEY) {
            caught += 1;
        }
    }
    // 5）冻结新增允许（对偶正例）
    {
        let mut b = AffCfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, true);
        if b.add(2, 7, 0, 10, true) == Ok(()) {
            caught += 1;
        }
    }
    // 6）封账后操作必拒
    {
        let mut b = AffCfgBook::new(4);
        let _ = b.add(1, 5, 0, 10, false);
        let _ = b.seal();
        if b.set(1, 6) == Err(XC_AC_SEALED) {
            caught += 1;
        }
    }
    // 7）导出稳定序
    {
        let mut b = AffCfgBook::new(8);
        let _ = b.add(5, 1, 0, 9, false);
        let _ = b.add(2, 2, 0, 9, false);
        let e = b.export();
        if e[0].0 == 2 && e[1].0 == 5 {
            caught += 1;
        }
    }
    (caught, N)
}
