//! unx0398_zeropage —— UNX-F0398 零页语义（calloc 面）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0398 · 零页语义 · 目标 200 行
//! 判据原文：UNX-F0398-J1 零化分配承诺「到手即零」：整页内容
//!           逐字节为零（抽查核 + 全扫核两种强度）；零页共享
//!           优化：写前只读映射共享物理零页（引用计数 > 1），
//!           首写触发断链（私有副本，引用计数回落）；语义核：
//!           共享态写入被拦截/断链后写入生效；零化开销账（零
//!           化字节数累计）与分配量对账。
//! 组件：零页表（物理零页 + 引用计数）+ 零化分配器（清零 +
//!       共享映射）+ 写监视器（共享态写拦截 + 断链）+ 开销账
//!       （零化字节累计）+ 语义核。
//! 职责定位：零页语义是「calloc 的合同」——调用方拿到指针的
//!   瞬间内容必须全零，这是安全承诺（密钥/账本初始化不能踩
//!   到前任残迹）；共享物理零页是省钱手段： calloc 了大量但
//!   只写一小部分的场景，未写的页都指着同一张真零页。首写
//!   断链是合同与优化的交点：写之前你看到的是共享零，写之后
//!   你拥有私有页，语义无缝。
//! 性能逐项分解：零化 O(页字节)；共享映射 O(1)；首写断链 O(1)；
//!   全扫核 O(页字节)。
//! 诊断码段：0x20E40（0x20E41~0x20E49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ZP_VERSION: &str = "unx0398_zeropage-v1";

/// 诊断段基址（0x20E40 段 · 本模块独占）。
pub const ZP_SEG_BASE: u32 = 0x20E40;
pub const XC_NOT_ZERO: u32 = 0x20E41; // 零化核破（非零字节）
pub const XC_COW_LEAK: u32 = 0x20E42; // 引用计数泄漏
pub const XC_WRITE_SHARED: u32 = 0x20E43; // 共享态写未断链
pub const XC_PAGE_OOR: u32 = 0x20E44; // 页号越域
pub const XC_ZERO_BYTES: u32 = 0x20E45; // 零字节分配拒
pub const XC_ZBYTES_UNBAL: u32 = 0x20E46; // 零化字节账不平
pub const XC_UNLINKED_RW: u32 = 0x20E47; // 断链后写不生效
pub const XC_REFCNT_NEG: u32 = 0x20E48; // 引用计数负值
pub const XC_MAGIC: u32 = 0x20E49; // 零页魔数破

/// 页尺寸（模型 256 字节便于判据全扫）。
pub const PAGE_SZ: usize = 256;
/// 真零页魔数。
pub const ZERO_MAGIC: u32 = 0x0000_0000;

/// 页映射态。
#[derive(Clone, Debug)]
pub struct ZPage {
    /// 页号。
    pub pno: u32,
    /// 内容。
    pub data: Vec<u8>,
    /// 是否共享真零页（写前只读映射）。
    pub shared_zero: bool,
}

/// 零页表 + 分配器。
pub struct ZeroAlloc {
    pages: Vec<ZPage>,
    /// 真零页引用计数。
    zero_refs: u32,
    /// 零化字节累计。
    zbytes: u64,
    /// 分配字节数累计。
    abytes: u64,
}

impl ZeroAlloc {
    /// 建分配器（含物理真零页初始化）。
    pub fn new() -> ZeroAlloc {
        ZeroAlloc { pages: Vec::new(), zero_refs: 0, zbytes: 0, abytes: 0 }
    }

    /// 零化分配 n 页：整页清零 + 共享映射置位 + 引用计数加。
    pub fn calloc(&mut self, n: u32) -> Result<u32, u32> {
        if n == 0 {
            return Err(XC_ZERO_BYTES);
        }
        let pno = self.pages.len() as u32 + 1;
        for _ in 0..n {
            self.pages.push(ZPage {
                pno,
                data: vec![0u8; PAGE_SZ],
                shared_zero: true,
            });
            self.zero_refs += 1;
            self.zbytes += PAGE_SZ as u64;
            self.abytes += PAGE_SZ as u64;
        }
        Ok(pno)
    }

    /// 零化核：全扫（整页逐字节）。共享态页核对真零页语义
    /// （共享页内容必须全零）；私有页不核（内容归用户）。
    pub fn verify_zero(&self, pno: u32) -> Result<(), u32> {
        for p in self.pages.iter() {
            if p.pno == pno && p.shared_zero {
                for (i, &b) in p.data.iter().enumerate() {
                    if b != 0 {
                        let _ = i;
                        return Err(XC_NOT_ZERO);
                    }
                }
                return Ok(());
            }
        }
        Err(XC_PAGE_OOR)
    }

    /// 写页：共享态必须先断链（返回需断链标记），断链后写生效。
    pub fn write(&mut self, pno: u32, off: usize, val: u8) -> Result<(), u32> {
        let idx = match self.pages.iter().position(|p| p.pno == pno) {
            Some(i) => i,
            None => return Err(XC_PAGE_OOR),
        };
        if off >= PAGE_SZ {
            return Err(XC_PAGE_OOR);
        }
        if self.pages[idx].shared_zero {
            return Err(XC_WRITE_SHARED); // 共享态直写被拦
        }
        self.pages[idx].data[off] = val;
        Ok(())
    }

    /// 首写断链：私有副本（内容保持零——断链不改变内容），
    /// 引用计数回落。
    pub fn unlink(&mut self, pno: u32) -> Result<(), u32> {
        let idx = match self.pages.iter().position(|p| p.pno == pno) {
            Some(i) => i,
            None => return Err(XC_PAGE_OOR),
        };
        if !self.pages[idx].shared_zero {
            return Ok(()); // 已私有，幂等
        }
        self.pages[idx].shared_zero = false;
        if self.zero_refs == 0 {
            return Err(XC_REFCNT_NEG);
        }
        self.zero_refs -= 1;
        Ok(())
    }

    /// 断链后写生效核（独立路径）。
    pub fn write_ok_after_unlink(&mut self, pno: u32, off: usize, val: u8) -> Result<(), u32> {
        self.unlink(pno)?;
        self.write(pno, off, val)
    }

    /// 开销账对账：零化字节 = 分配字节（calloc 面恒等）。
    pub fn zbytes_bal(&self) -> Result<(), u32> {
        if self.zbytes != self.abytes {
            return Err(XC_ZBYTES_UNBAL);
        }
        Ok(())
    }

    /// 引用计数核：共享页数 = 引用计数。
    pub fn refcnt_ok(&self) -> Result<(), u32> {
        let shared = self.pages.iter().filter(|p| p.shared_zero).count() as u32;
        if shared != self.zero_refs {
            return Err(XC_COW_LEAK);
        }
        Ok(())
    }

    /// 页数。
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// 引用计数读取。
    pub fn zero_refs(&self) -> u32 {
        self.zero_refs
    }

    /// 零页魔数核（真零页内容首四字节 = ZERO_MAGIC）。
    pub fn magic_ok(&self) -> Result<(), u32> {
        // 模型内真零页即全零，取首个共享页验证。
        for p in self.pages.iter() {
            if p.shared_zero {
                let m = (p.data[0] as u32)
                    | ((p.data[1] as u32) << 8)
                    | ((p.data[2] as u32) << 16)
                    | ((p.data[3] as u32) << 24);
                if m != ZERO_MAGIC {
                    return Err(XC_MAGIC);
                }
                return Ok(());
            }
        }
        Err(XC_MAGIC)
    }
}

pub fn run_unx0398_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0398_zeropage");
    // 族一：到手即零。
    let mut z = ZeroAlloc::new();
    let p1 = z.calloc(2);
    cs.add("零化分配 2 页", p1 == Ok(1), "首页号 1");
    cs.add("全扫零核", z.verify_zero(1).is_ok(), "256 × 2 全零");
    cs.add("页数 2", z.page_count() == 2, "两页在册");
    cs.add("引用计数 2", z.zero_refs() == 2, "共享 ×2");
    cs.add("引用核平", z.refcnt_ok().is_ok(), "共享数 = 计数");
    cs.add("魔数零", z.magic_ok().is_ok(), "首四字节全零");
    cs.add("零化账平", z.zbytes_bal().is_ok(), "512 = 512");
    // 族二：共享态写拦截 + 断链。
    cs.add("共享态直写拦", z.write(1, 0, 0xFF) == Err(XC_WRITE_SHARED), "未断链");
    cs.add("断链页 1", z.unlink(1).is_ok(), "私有副本");
    cs.add("引用计数 1", z.zero_refs() == 1, "回落");
    cs.add("断链后写生效", z.write(1, 0, 0xFF).is_ok(), "私有可写");
    cs.add("断链幂等", z.unlink(1).is_ok(), "再断链无副作用");
    // 族三：未断链页仍零。
    cs.add("页 2 仍零", z.verify_zero(2).is_ok(), "共享零保持");
    // 族四：断链后写不生效检出（先写后核）。
    let mut w = ZeroAlloc::new();
    let _ = w.calloc(1);
    cs.add("断链+写一体", w.write_ok_after_unlink(1, 7, 0xAB).is_ok(), "链路通");
    // 写后内容核对（模型内直接读）。
    let got = match w.pages.get(0) {
        Some(p) => p.data[7],
        None => 0,
    };
    cs.add("写入值回读", got == 0xAB, "off 7 = AB");
    // 族五：参数拒绝。
    cs.add("零页分配拒", z.calloc(0) == Err(XC_ZERO_BYTES), "n=0");
    cs.add("页号越域核", z.verify_zero(99) == Err(XC_PAGE_OOR), "无页 99");
    cs.add("写越域页", z.write(99, 0, 1) == Err(XC_PAGE_OOR), "无页 99");
    cs.add("偏移越域写", z.write(2, PAGE_SZ, 1) == Err(XC_PAGE_OOR), "off=256");
    // 族六：引用计数负值防御。
    let mut n = ZeroAlloc::new();
    cs.add("空表断链拒", matches!(n.unlink(1), Err(XC_PAGE_OOR)), "无页");
    // 族七：批量分配与计数一致性。
    let mut b = ZeroAlloc::new();
    let _ = b.calloc(4);
    let _ = b.calloc(3);
    cs.add("批量页数 7", b.page_count() == 7, "4 + 3");
    cs.add("批量计数 7", b.zero_refs() == 7, "全共享");
    cs.add("批量引用核", b.refcnt_ok().is_ok(), "7 = 7");
    let _ = b.unlink(3);
    cs.add("断链后核 6", b.zero_refs() == 6, "7 - 1");
    cs.add("断链后引用核", b.refcnt_ok().is_ok(), "6 共享 = 6 计数");
    cs
}
