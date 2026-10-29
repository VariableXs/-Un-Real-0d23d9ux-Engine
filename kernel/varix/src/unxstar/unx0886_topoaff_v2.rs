//! unx0886_topoaff_v2 —— UNX-F0886 · 拓扑感知亲和性约束（核池与超线程面）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0886 · 拓扑感知亲和性约束。
//! 判据原文摘录（卡面验收行）：拓扑感知亲和性约束（核池与超线程面）
//!           ——亲和掩码逐核池校验、超线程兄弟面互斥约束全收口、
//!           违约束亲和全拦截。
//! 诊断段 0x79C0（本单独占，0x79C1~0x79CF 闭集）。
//!
//! 设计纪律：
//! - 拓扑建模：核池（物理核簇）× 超线程面（每物理核 2 逻辑核：偶/奇）。
//!   逻辑核号 → (池号, 物理核号, 面位) 三元组。
//! - 亲和约束三类：
//!   ① 池约束：亲和掩码不得跨池（跨池即红）；
//!   ② 兄弟互斥：同物理核双逻辑面不得同掩码（SMT 面互斥纪律）；
//!   ③ 池容量：池内并发亲和数不越池容量（CAP_PER_POOL）。
//! - 掩码模型：u16 位图（16 逻辑核位面），位 i = 逻辑核 i。
//! - 守恒恒等式：Σ池亲和数 = 总亲和数；掩码位和 = 亲和核数。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const TA_VERSION: &str = "unx0886_topoaff_v2-v1";

/// 诊断段基址（0x79C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x79C0;

/// XcCode 闭集（0x79C1~0x79CF）。
pub const XC_CROSS_POOL: u16 = SEG_BASE + 1; // 亲和跨池
pub const XC_SMT_CLASH: u16 = SEG_BASE + 2; // 超线程兄弟面互斥破
pub const XC_POOL_CAP: u16 = SEG_BASE + 3; // 池容量越界
pub const XC_MASK_EMPTY: u16 = SEG_BASE + 4; // 空掩码
pub const XC_MASK_RANGE: u16 = SEG_BASE + 5; // 掩码位越拓扑面（>逻辑核数）
pub const XC_CONSERVE: u16 = SEG_BASE + 6; // 守恒破
pub const XC_TOPO_RANGE: u16 = SEG_BASE + 7; // 拓扑参数非法
pub const XC_DUP: u16 = SEG_BASE + 8; // 亲和键重复

/// 池数上限。
pub const POOL_MAX: usize = 4;
/// 每池物理核上限。
pub const PHYS_PER_POOL: usize = 2;
/// 超线程面宽（每物理核逻辑核数）。
pub const SMT_WIDTH: usize = 2;
/// 逻辑核总数 = 池 × 物理核 × 面。
pub const LCPU_MAX: usize = POOL_MAX * PHYS_PER_POOL * SMT_WIDTH;
/// 每池亲和容量。
pub const CAP_PER_POOL: usize = 4;

/// 亲和请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AffReq {
    /// 亲和键（向量或任务号）。
    pub key: u32,
    /// 亲和掩码（逻辑核位图）。
    pub mask: u16,
}

/// 亲和落账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AffRow {
    /// 亲和键。
    pub key: u32,
    /// 掩码。
    pub mask: u16,
    /// 归属池（掩码所在池）。
    pub pool: usize,
    /// 掩码位和。
    pub bits: u32,
}

/// 拓扑感知亲和裁决器。
pub struct TopoAff {
    rows: Vec<AffRow>,
    /// 拦截账：(键, 码, 序)。
    intercepts: Vec<(u32, u16, u64)>,
    seq: u64,
    overflow: usize,
    /// 拓扑参数：池数（≤POOL_MAX）。
    pools: usize,
}

impl TopoAff {
    /// 建拓扑：pools 个池 × PHYS_PER_POOL 物理 × SMT_WIDTH 逻辑。
    pub fn new(pools: usize) -> Result<TopoAff, u16> {
        if pools == 0 || pools > POOL_MAX {
            return Err(XC_TOPO_RANGE);
        }
        Ok(TopoAff {
            rows: Vec::new(),
            intercepts: Vec::new(),
            seq: 0,
            overflow: 0,
            pools,
        })
    }

    /// 逻辑核号 → 池号（核号 = 池*PHYS*SMT + 物*SMT + 面）。
    pub fn pool_of(&self, lcpu: usize) -> Result<usize, u16> {
        let total = self.pools * PHYS_PER_POOL * SMT_WIDTH;
        if lcpu >= total {
            return Err(XC_TOPO_RANGE);
        }
        Ok(lcpu / (PHYS_PER_POOL * SMT_WIDTH))
    }

    /// 逻辑核号 → 物理核全局号（跨池统一编号）。
    pub fn phys_of(&self, lcpu: usize) -> Result<usize, u16> {
        let total = self.pools * PHYS_PER_POOL * SMT_WIDTH;
        if lcpu >= total {
            return Err(XC_TOPO_RANGE);
        }
        Ok(lcpu / SMT_WIDTH)
    }

    /// 掩码合法性：非空 + 位不越拓扑面。
    fn mask_ok(&self, mask: u16) -> Result<u32, u16> {
        if mask == 0 {
            return Err(XC_MASK_EMPTY);
        }
        let total = self.pools * PHYS_PER_POOL * SMT_WIDTH;
        if total < 16 && (mask >> total) != 0 {
            return Err(XC_MASK_RANGE);
        }
        Ok(mask.count_ones())
    }

    /// 掩码跨池判：掩码内逻辑核须同池。
    fn mask_pool(&self, mask: u16) -> Result<usize, u16> {
        let mut pool: Option<usize> = None;
        for i in 0..16usize {
            if mask & (1u16 << i) != 0 {
                let p = self.pool_of(i)?;
                match pool {
                    None => pool = Some(p),
                    Some(q) if q != p => return Err(XC_CROSS_POOL),
                    _ => {}
                }
            }
        }
        match pool {
            Some(p) => Ok(p),
            None => Err(XC_MASK_EMPTY),
        }
    }

    /// 落亲和：三类约束全过才入账。
    pub fn apply(&mut self, key: u32, mask: u16) -> Result<AffRow, u16> {
        self.seq += 1;
        // 0. 键重复拦截
        if self.rows.iter().any(|r| r.key == key) {
            self.reject(key, XC_DUP);
            return Err(XC_DUP);
        }
        // 1. 掩码合法性
        let bits = match self.mask_ok(mask) {
            Ok(b) => b,
            Err(e) => {
                self.reject(key, e);
                return Err(e);
            }
        };
        // 2. 池约束（不跨池）
        let pool = match self.mask_pool(mask) {
            Ok(p) => p,
            Err(e) => {
                self.reject(key, e);
                return Err(e);
            }
        };
        // 3. 兄弟互斥：掩码若含同物理核双面（连续 SMT_WIDTH 位对齐对）
        //    即互斥破——检查掩码内任意两核是否同物理核
        let lc: Vec<usize> = (0..16usize)
            .filter(|i| mask & (1u16 << i) != 0)
            .collect();
        for a in 0..lc.len() {
            for b in (a + 1)..lc.len() {
                if self.phys_of(lc[a])? == self.phys_of(lc[b])? {
                    self.reject(key, XC_SMT_CLASH);
                    return Err(XC_SMT_CLASH);
                }
            }
        }
        // 4. 池容量
        let in_pool = self.rows.iter().filter(|r| r.pool == pool).count();
        if in_pool >= CAP_PER_POOL {
            self.reject(key, XC_POOL_CAP);
            return Err(XC_POOL_CAP);
        }
        let row = AffRow { key, mask, pool, bits };
        self.rows.push(row);
        Ok(row)
    }

    fn reject(&mut self, key: u32, code: u16) {
        if self.intercepts.len() < 64 {
            self.intercepts.push((key, code, self.seq));
        } else {
            self.overflow += 1;
        }
    }

    /// 亲和行读取。
    pub fn row(&self, i: usize) -> Result<AffRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_CONSERVE);
        }
        Ok(self.rows[i])
    }

    /// 拦截行读取。
    pub fn intercept(&self, i: usize) -> Result<(u32, u16, u64), u16> {
        if i >= self.intercepts.len() {
            return Err(XC_CONSERVE);
        }
        Ok(self.intercepts[i])
    }

    /// 守恒：Σ池亲和数 = 总亲和数；位和 = Σ行 bits。
    pub fn conserve(&self) -> bool {
        let bits_sum: u32 = self.rows.iter().map(|r| r.bits).sum();
        let mask_bits: u32 = self.rows.iter().map(|r| r.mask.count_ones()).sum();
        let mut per_pool = [0usize; POOL_MAX];
        for r in self.rows.iter() {
            per_pool[r.pool] += 1;
        }
        per_pool.iter().sum::<usize>() == self.rows.len() && bits_sum == mask_bits
    }

    /// 拦截守恒。
    pub fn intercept_conserve(&self) -> bool {
        self.intercepts.len() + self.overflow == self.seq as usize - self.rows.len() as usize
    }

    /// 池亲和分布。
    pub fn pool_dist(&self) -> [usize; POOL_MAX] {
        let mut d = [0usize; POOL_MAX];
        for r in self.rows.iter() {
            d[r.pool] += 1;
        }
        d
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.rows.len()
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 拓扑：2 池 × 2 物理 × 2 面 = 8 逻辑核
    // 核位面：池0=[0,1,2,3]（物0=[0,1] 物1=[2,3]），池1=[4,5,6,7]
    let mut t = TopoAff::new(2)?;
    // 1. 单核亲和（池0）
    let r1 = t.apply(1, 0b0000_0001)?;
    if r1.pool != 0 || r1.bits != 1 {
        return Err(XC_CONSERVE);
    }
    // 2. 双核同池不同物（核 0 与核 2）
    let r2 = t.apply(2, 0b0000_0101)?;
    if r2.pool != 0 || r2.bits != 2 {
        return Err(XC_CONSERVE);
    }
    // 3. 池1 单核
    let r3 = t.apply(3, 0b0001_0000)?;
    if r3.pool != 1 {
        return Err(XC_CONSERVE);
    }
    // 4. 错误路径①：跨池掩码（核 0 与核 4）
    if t.apply(4, 0b0001_0001).is_ok() {
        return Err(XC_CROSS_POOL);
    }
    // 5. 错误路径②：兄弟互斥（核 0 与核 1 同物双面）
    if t.apply(5, 0b0000_0011).is_ok() {
        return Err(XC_SMT_CLASH);
    }
    // 6. 错误路径③：空掩码
    if t.apply(6, 0).is_ok() {
        return Err(XC_MASK_EMPTY);
    }
    // 7. 错误路径④：键重复
    if t.apply(1, 0b0000_0100).is_ok() {
        return Err(XC_DUP);
    }
    // 8. 错误路径⑤：拓扑参数非法
    if TopoAff::new(0).is_err() && TopoAff::new(POOL_MAX + 1).is_err() {
        // 双拒 ✓
    } else {
        return Err(XC_TOPO_RANGE);
    }
    // 9. 池容量：池0 已 2 行，再补 2 行后第 5 行拒
    t.apply(7, 0b0000_0100)?; // 核2（物1 面0）
    t.apply(8, 0b0000_1000)?; // 核3（物1 面1）
    if t.apply(9, 0b0000_0001).is_ok() {
        return Err(XC_POOL_CAP);
    }
    // 10. 守恒核：池分布 [4,1]，行数 5
    let d = t.pool_dist();
    if d[0] != 4 || d[1] != 1 || t.count() != 5 {
        return Err(XC_CONSERVE);
    }
    if !t.conserve() || !t.intercept_conserve() {
        return Err(XC_CONSERVE);
    }
    // 11. 拦截账抽查：跨池 / 兄弟 / 空掩码 / 重复 / 容量 5 条在账
    if t.intercept(0)?.1 != XC_CROSS_POOL
        || t.intercept(1)?.1 != XC_SMT_CLASH
        || t.intercept(2)?.1 != XC_MASK_EMPTY
        || t.intercept(3)?.1 != XC_DUP
        || t.intercept(4)?.1 != XC_POOL_CAP
    {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
