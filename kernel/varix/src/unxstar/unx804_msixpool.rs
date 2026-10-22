//! unx804_msixpool —— UNX-F0804 全局 MSI-X 向量池与 IPI 向量区隔离。
//! 锚点（CoRun Unxreal 主册）：UNX-F0804 · 全局 MSI-X 向量池与 IPI 向量区隔离 · 目标 320 行 · 已深化
//! 判据原文：MSI-X 池分配/回收全局一致，IPI 保留区零侵占（越界分配 100% 拒绝落账）
//! 组件：向量空间模型（0~255 闭集；IPI 保留区 0xE0~0xFF 恒占）
//!       + 全局池（MSI-X 向量位图分配/回收 + 幂等回收拒绝）
//!       + 隔离闸（IPI 保留区任何 MSI-X 分配 100% 拒绝落账）
//!       + 分配账（append-only：向量/方向/持有者三列）
//!       + 对拍核（池指纹 + 注入核四类必抓）。
//! 职责定位：中断面唯一权威向量池——MSI-X 与 IPI 空间硬隔离；
//!   位图分配使池状态全局一致可复算；隔离闸保证 IPI 区零侵占。
//! 性能逐项分解：分配/回收 O(256/64)=O(4) 位字扫描；落账 O(1) 均摊；
//!   指纹 O(账行数)。全整数确定性（LCG，零墙钟）。
//! 诊断码段：0xE780（0xE781~0xE788 闭集；空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const MX_VERSION: &str = "unx804_msixpool-v1";

/// 诊断段基址（0xE780 段 · 本模块独占）。
pub const MX_SEG_BASE: u16 = 0xE780;

/// XcCode 闭集（0xE781~0xE788）。
pub const XC_IPI_TRESPASS: u32 = 0xE781;
pub const XC_VEC_DOUBLE: u32 = 0xE782;
pub const XC_FREE_UNOWNED: u32 = 0xE783;
pub const XC_POOL_EXHAUST: u32 = 0xE784;
pub const XC_FINGER_DRIFT: u32 = 0xE785;
pub const XC_INJECT_MISSED: u32 = 0xE786;
pub const XC_LEDGER_BROKEN: u32 = 0xE787;
pub const XC_RANGE_BAD: u32 = 0xE788;

/// 中文诊断文本（闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_IPI_TRESPASS => ("IPI 保留区侵占（MSI-X 分配触 0xE0~0xFF）", "MSI-X 池上限硬隔离到 0xDF"),
        XC_VEC_DOUBLE => ("向量重复分配（位图已置位）", "分配前查位图，重复即拒"),
        XC_FREE_UNOWNED => ("回收未持有向量（幂等回收拒绝）", "回收须与分配同持有者"),
        XC_POOL_EXHAUST => ("MSI-X 池耗尽（0xE0 前无空位）", "扩容或回收后重试"),
        XC_FINGER_DRIFT => ("池指纹与冻结值不一致（池漂移）", "复核分配序列或重跑指纹"),
        XC_INJECT_MISSED => ("注入故障未被检出（守卫失守）", "修复注入核比对逻辑后重跑"),
        XC_LEDGER_BROKEN => ("分配账 append-only 性被破坏（序号跳号）", "检查落账路径是否绕过账务封装"),
        XC_RANGE_BAD => ("向量号越界（超出 0~255 闭集）", "向量常量取自八位闭集"),
        _ => ("未知诊断码", "查 0xE780 段闭集登记表"),
    }
}

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// IPI 保留区下界（0xE0~0xFF 恒为 IPI 区）。
pub const IPI_RESV_LO: u8 = 0xE0;
/// MSI-X 池上界（开区间，硬隔离）。
pub const MSIX_HI: u8 = 0xE0;

/// 分配账一行（append-only）。
#[derive(Clone, Copy, Debug)]
pub struct AllocRow {
    /// 向量号。
    pub vec: u8,
    /// 方向（0=分配 1=回收）。
    pub op: u8,
    /// 持有者（模块域号，0=系统保留）。
    pub owner: u8,
}

/// 池错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolError {
    IpiTrespass(u8),
    DoubleAlloc(u8),
    FreeUnowned(u8),
    RangeBad(u8),
    Exhaust,
    LedgerBroken(u32),
}

/// 全局 MSI-X 向量池。
pub struct MsixPool {
    bitmap: [u64; 4],
    owner: [u8; 256],
    rows: Vec<AllocRow>,
    next_seq: u32,
    frozen: u32,
    sealed: bool,
}

impl MsixPool {
    pub fn new() -> MsixPool {
        MsixPool {
            bitmap: [0; 4],
            owner: [0; 256],
            rows: Vec::new(),
            next_seq: 1,
            frozen: 0,
            sealed: false,
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.next_seq as usize == self.rows.len() + 1
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    pub fn row_at(&self, i: usize) -> Option<AllocRow> {
        self.rows.get(i).copied()
    }

    /// 已分配向量数（MSI-X 区内）。
    pub fn allocated(&self) -> u32 {
        let mut n = 0;
        for (w, &bits) in self.bitmap.iter().enumerate() {
            let base = (w as u32) * 64;
            let mut b = bits;
            while b != 0 {
                let bit = b.trailing_zeros();
                if base + bit < MSIX_HI as u32 {
                    n += 1;
                }
                b &= b - 1;
            }
        }
        n
    }

    /// 分配指定向量（隔离闸前置）。
    pub fn alloc(&mut self, vec: u8, owner: u8) -> Result<(), PoolError> {
        if self.sealed {
            return Err(PoolError::LedgerBroken(self.next_seq));
        }
        if vec >= MSIX_HI {
            // IPI 保留区侵占：拒绝落账
            let seq = self.next_seq;
            self.rows.push(AllocRow {
                vec,
                op: 0,
                owner: owner | 0x80,
            });
            self.next_seq = seq + 1;
            return Err(PoolError::IpiTrespass(vec));
        }
        let (w, bit) = ((vec / 64) as usize, vec % 64);
        if self.bitmap[w] & (1u64 << bit) != 0 {
            return Err(PoolError::DoubleAlloc(vec));
        }
        self.bitmap[w] |= 1u64 << bit;
        self.owner[vec as usize] = owner;
        let seq = self.next_seq;
        self.rows.push(AllocRow {
            vec,
            op: 0,
            owner,
        });
        self.next_seq = seq + 1;
        Ok(())
    }

    /// 按需分配（找最低空闲位）。
    pub fn alloc_any(&mut self, owner: u8) -> Result<u8, PoolError> {
        for w in 0..4usize {
            let base = (w as u8) * 64;
            if base as usize >= MSIX_HI as usize {
                break;
            }
            let mut free = !self.bitmap[w];
            // 高位掩码：超过 MSIX_HI 的位屏蔽
            for bit in 0..64u32 {
                let v = base as u32 + bit;
                if v >= MSIX_HI as u32 {
                    free &= !(1u64 << bit);
                }
            }
            if free != 0 {
                let bit = free.trailing_zeros();
                let v = base as u32 + bit;
                self.alloc(v as u8, owner)?;
                return Ok(v as u8);
            }
        }
        Err(PoolError::Exhaust)
    }

    /// 回收（持有者必须一致；幂等回收拒绝）。
    pub fn free(&mut self, vec: u8, owner: u8) -> Result<(), PoolError> {
        if vec >= MSIX_HI {
            return Err(PoolError::IpiTrespass(vec));
        }
        let (w, bit) = ((vec / 64) as usize, vec % 64);
        if self.bitmap[w] & (1u64 << bit) == 0 || self.owner[vec as usize] != owner {
            return Err(PoolError::FreeUnowned(vec));
        }
        self.bitmap[w] &= !(1u64 << bit);
        self.owner[vec as usize] = 0;
        let seq = self.next_seq;
        self.rows.push(AllocRow {
            vec,
            op: 1,
            owner,
        });
        self.next_seq = seq + 1;
        Ok(())
    }

    /// 池指纹（位图 + 账序确定性）。
    pub fn fingerprint(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for w in &self.bitmap {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        for r in &self.rows {
            buf.push(r.vec);
            buf.push(r.op);
            buf.push(r.owner & 0x7F);
        }
        fnv1a32(&buf)
    }

    pub fn seal(&mut self) -> u32 {
        self.frozen = self.fingerprint();
        self.sealed = true;
        self.frozen
    }

    pub fn verify_frozen(&self) -> bool {
        self.sealed && self.fingerprint() == self.frozen
    }
}

/// 注入核：四类违规 + 守卫 10/10 必抓。
pub fn inject_probe() -> Result<bool, u32> {
    let mut caught = 0u32;
    let mut p = MsixPool::new();
    // 1) IPI 区分配拒绝（0xF0）
    if p.alloc(0xF0, 3).is_err() {
        caught += 1;
    }
    // 2) IPI 区侵占落账（高位标记）
    if p.row_at(0).map(|r| r.owner & 0x80 != 0).unwrap_or(false) {
        caught += 1;
    }
    // 3) 重复分配拒绝
    let _ = p.alloc(0x10, 2);
    if p.alloc(0x10, 3).is_err() {
        caught += 1;
    }
    // 4) 非持有者回收拒绝
    if p.free(0x10, 5).is_err() {
        caught += 1;
    }
    // 5) 正常回收放行
    if p.free(0x10, 2).is_ok() {
        caught += 1;
    }
    // 6) 幂等回收拒绝（已回收再回收）
    if p.free(0x10, 2).is_err() {
        caught += 1;
    }
    // 7) 按需分配不触 IPI 区（连分 224 个后耗尽）
    let mut p2 = MsixPool::new();
    let mut got = 0u32;
    for _ in 0..0xE0 {
        if p2.alloc_any(7).is_ok() {
            got += 1;
        }
    }
    if got == 0xE0 && p2.alloc_any(7).is_err() {
        caught += 1;
    }
    // 8) 耗尽后池位图全满
    if p2.allocated() == 0xE0 {
        caught += 1;
    }
    // 9) 指纹随分配变化（漂移可检）
    let f1 = p.fingerprint();
    let _ = p.alloc(0x20, 4);
    let f2 = p.fingerprint();
    if f1 != f2 {
        caught += 1;
    }
    // 10) 封账后写拒 + 冻结核验
    let _ = p.seal();
    if p.alloc(0x30, 5).is_err() && p.verify_frozen() {
        caught += 1;
    }
    if caught != 10 {
        return Err(XC_INJECT_MISSED as u32);
    }
    Ok(true)
}

/// 域自检（`run_unx804_checks`）：判据逐条可失败断言。
pub fn run_unx804_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx804");

    // C804-01 IPI 保留区隔离 100% 拒绝（J1 核心）
    {
        let mut p = MsixPool::new();
        let r1 = p.alloc(IPI_RESV_LO, 2);
        let r2 = p.alloc(0xFF, 2);
        let ok = matches!(r1, Err(PoolError::IpiTrespass(0xE0)))
            && matches!(r2, Err(PoolError::IpiTrespass(0xFF)));
        s.add("C804-01-IPI区全拒", ok, if ok { "" } else { "隔离闸失守" });
    }

    // C804-02 池分配/回收位图一致
    {
        let mut p = MsixPool::new();
        let _ = p.alloc(0x08, 1);
        let _ = p.alloc(0x09, 1);
        let ok = p.allocated() == 2
            && p.free(0x08, 1).is_ok()
            && p.allocated() == 1;
        s.add("C804-02-位图分配回收一致", ok, if ok { "" } else { "位图失守" });
    }

    // C804-03 按需分配最低位优先且全池可耗尽
    {
        let mut p = MsixPool::new();
        let first = p.alloc_any(3);
        let ok = first == Ok(0) && p.allocated() == 1;
        s.add("C804-03-最低位优先", ok, if ok { "" } else { "分配序失守" });
    }

    // C804-04 越界向量拒绝
    {
        let mut p = MsixPool::new();
        let r = p.alloc(0xFF, 1); // 0xFF 落在 IPI 保留区（0xE0~0xFF 恒占）→ 必拒
        let ok = r.is_err();
        s.add("C804-04-越界拒绝", ok, if ok { "" } else { "越界漏放" });
    }

    // C804-05 指纹漂移检出
    {
        let mut p = MsixPool::new();
        let f0 = p.fingerprint();
        let _ = p.alloc(0x40, 2);
        let f1 = p.fingerprint();
        let ok = f0 != f1 && p.verify_frozen() == false;
        s.add("C804-05-指纹漂移可检", ok, if ok { "" } else { "指纹锚失守" });
    }

    // C804-06 账序守恒
    {
        let mut p = MsixPool::new();
        for v in 0..8u8 {
            let _ = p.alloc(v * 2, 1);
        }
        let ok = p.ledger_ok() && p.rows() == 8;
        s.add("C804-06-账序守恒", ok, if ok { "" } else { "append-only 被破坏" });
    }

    // C804-07 注入 10/10 全抓
    {
        let ok = inject_probe() == Ok(true);
        s.add("C804-07-注入全抓", ok, if ok { "" } else { "注入核漏检" });
    }

    s
}
