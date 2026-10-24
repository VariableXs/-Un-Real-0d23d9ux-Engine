//! veb342_cov2d —— VE-F0342 2D 全功能覆盖声明。
//! 锚点：VE-F0342 · 目标 380 行。
//! 判据原文：全功能清单（填充 / 位块传输 / 拉伸过滤 / 混合 /
//!           裁剪 / 路径描边填充 / 文本栅格化 / 格式转换八项）、
//!           精度口径（逐项登记精度口径不模糊）、测试背书
//!           （清单与实现一致性由测试套件背书，缺一阻断）、
//!           先声明后实现、判据。
//! 组件：覆盖清单 + 背书账 + 声明闸。
//! 职责定位：覆盖声明是「菜单」——写了就要端得出来；没背书
//!           的功能是菜单上画饼，声明闸直接阻断上桌。
//! 性能逐项分解：清单 O(功能数)；背书 O(用例数)；查询 O(1)。
//! 诊断码段：0x31E80（0x31E81~0x31E89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CV_VERSION: &str = "veb342_cov2d-v1";

/// 诊断段基址（0x31E80 段 · 本模块独占）。
pub const CV_SEG_BASE: u32 = 0x31E80;
pub const XC_BADF: u32 = 0x31E81; // 功能号非法
pub const XC_NOPREC: u32 = 0x31E82; // 精度口径未登记
pub const XC_NOEND: u32 = 0x31E83; // 无测试背书
pub const XC_DUP: u32 = 0x31E84; // 重复声明
pub const XC_IMPL: u32 = 0x31E85; // 未声明先实现

/// 八项功能号。
pub const F_FILL: u32 = 1; // 填充
pub const F_BLIT: u32 = 2; // 位块传输
pub const F_STRETCH: u32 = 3; // 拉伸过滤
pub const F_BLEND: u32 = 4; // 混合
pub const F_CLIP: u32 = 5; // 裁剪
pub const F_PATH: u32 = 6; // 路径描边填充
pub const F_TEXT: u32 = 7; // 文本栅格化
pub const F_FMT: u32 = 8; // 格式转换
pub const F_MAX: u32 = 8;

/// 精度口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prec {
    /// 位精确。
    Exact,
    /// 容差（单位 1/255）。
    Tol(u32),
}

/// 覆盖条目。
pub struct Cov {
    pub feat: u32,
    /// 语义代号。
    pub sem: u32,
    pub prec: Prec,
    /// 背书用例数（0 = 无背书）。
    pub cases: u32,
}

/// 覆盖声明簿。
pub struct CovBook {
    covs: Vec<Cov>,
    /// 声明过的功能位图。
    claimed: u32,
    /// 先声明后实现的违规登记。
    pub impl_first: u32,
}

impl CovBook {
    /// 空簿。
    pub fn new() -> CovBook {
        CovBook {
            covs: Vec::new(),
            claimed: 0,
            impl_first: 0,
        }
    }

    /// 声明：功能号 1~8、精度口径必填、重复拒。
    pub fn declare(&mut self, feat: u32, sem: u32, prec: Prec) -> Result<(), u32> {
        if feat == 0 || feat > F_MAX {
            return Err(XC_BADF);
        }
        if self.covs.iter().any(|c| c.feat == feat) {
            return Err(XC_DUP);
        }
        self.claimed |= 1 << feat;
        self.covs.push(Cov {
            feat,
            sem,
            prec,
            cases: 0,
        });
        Ok(())
    }

    /// 背书：给已声明功能加测试用例计数。
    pub fn endorse(&mut self, feat: u32, n: u32) -> Result<(), u32> {
        let c = self
            .covs
            .iter_mut()
            .find(|c| c.feat == feat)
            .ok_or(XC_BADF)?;
        c.cases += n;
        Ok(())
    }

    /// 实现登记：未声明先实现 → 违规登记（先声明后实现纪律）。
    pub fn implement(&mut self, feat: u32) -> Result<(), u32> {
        if feat == 0 || feat > F_MAX {
            return Err(XC_BADF);
        }
        if self.claimed & (1 << feat) == 0 {
            self.impl_first += 1;
            return Err(XC_IMPL);
        }
        Ok(())
    }

    /// 声明有效性：有声明、有口径（恒有）、有用例背书。
    pub fn valid(&self, feat: u32) -> Result<(), u32> {
        let c = self.covs.iter().find(|c| c.feat == feat).ok_or(XC_BADF)?;
        if c.cases == 0 {
            return Err(XC_NOEND);
        }
        Ok(())
    }

    /// 全量声明闸：八项全声明且全背书才许出全覆盖声明。
    pub fn full_claim(&self) -> Result<u32, u32> {
        let mut ok = 0u32;
        for f in 1..=F_MAX {
            if self.valid(f).is_ok() {
                ok += 1;
            }
        }
        if ok < F_MAX {
            return Err(XC_NOEND);
        }
        Ok(ok)
    }

    /// 精度口径查询（不模糊：查无即无）。
    pub fn prec_of(&self, feat: u32) -> Option<Prec> {
        self.covs.iter().find(|c| c.feat == feat).map(|c| c.prec)
    }

    /// 清单长。
    pub fn len(&self) -> usize {
        self.covs.len()
    }
}

pub fn run_veb342_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0342");
    // 族零：声明门。
    let mut b = CovBook::new();
    s.add("零号拒", b.declare(0, 1, Prec::Exact) == Err(XC_BADF), "无零功能");
    s.add("越界拒", b.declare(9, 1, Prec::Exact) == Err(XC_BADF), ">8");
    s.add("填充过", b.declare(F_FILL, 0x11, Prec::Exact).is_ok(), "位精确");
    s.add("重复拒", b.declare(F_FILL, 2, Prec::Exact) == Err(XC_DUP), "一功能一声明");
    // 族一：八项声明。
    let _ = b.declare(F_BLIT, 0x12, Prec::Exact);
    let _ = b.declare(F_STRETCH, 0x13, Prec::Tol(1));
    let _ = b.declare(F_BLEND, 0x14, Prec::Tol(2));
    let _ = b.declare(F_CLIP, 0x15, Prec::Exact);
    let _ = b.declare(F_PATH, 0x16, Prec::Tol(1));
    let _ = b.declare(F_TEXT, 0x17, Prec::Exact);
    let _ = b.declare(F_FMT, 0x18, Prec::Exact);
    s.add("清单八", b.len() == 8, "全功能");
    // 族二：精度口径。
    s.add("填充位精", b.prec_of(F_FILL) == Some(Prec::Exact), "不模糊");
    s.add("混合容差二", b.prec_of(F_BLEND) == Some(Prec::Tol(2)), "口径 2/255");
    s.add("拉伸容差一", b.prec_of(F_STRETCH) == Some(Prec::Tol(1)), "口径 1/255");
    s.add("查无口径", b.prec_of(99).is_none(), "查无即无");
    // 族三：背书与阻断。
    s.add("无背书拒", b.valid(F_FILL) == Err(XC_NOEND), "画饼");
    s.add("背书过", b.endorse(F_FILL, 3).is_ok(), "三用例");
    s.add("背书有效", b.valid(F_FILL).is_ok(), "有据");
    s.add("未声明背书拒", b.endorse(9, 1) == Err(XC_BADF), "无此条");
    s.add("全覆盖阻断", b.full_claim() == Err(XC_NOEND), "七项无背书");
    // 其余七项补背书。
    for f in 2..=8u32 {
        let _ = b.endorse(f, 2);
    }
    s.add("全覆盖过", b.full_claim() == Ok(8), "八项全背书");
    // 族四：先声明后实现。
    s.add("已声明实现过", b.implement(F_FILL).is_ok(), "序正");
    s.add("未声明实现拒", b.implement(8 + 1) == Err(XC_BADF), "越界");
    let mut b2 = CovBook::new();
    s.add("未声明先实现拒", b2.implement(F_BLEND) == Err(XC_IMPL), "违规");
    s.add("违规登记一", b2.impl_first == 1, "留痕");
    // 族五：空簿全量阻断。
    s.add("空簿阻断", CovBook::new().full_claim() == Err(XC_NOEND), "零背书");
    s.add("魔数核", CV_SEG_BASE == 0x31E80, "段基址");
    s
}
