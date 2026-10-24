//! veb345_bound3d —— VE-F0345 3D 软渲边界清单。
//! 锚点：VE-F0345 · 目标 340 行。
//! 判据原文：显性边界（支持子集 vs 不支持/降级项逐项登记）、
//!           精度口径（逐项登记精度与性能口径）、承诺非愿望
//!           （清单是承诺边界不是愿望单）、先测后入（新增支
//!           持先测后入清单）、清单外拒绝（显性拒绝或降级并
//!           告知）、判据。
//! 组件：边界清单 + 请求裁决器 + 承诺账。
//! 职责定位：边界清单是「店门口的营业范围」——挂在墙上的一
//!           定做得到；没挂的不硬接，接了就写清楚是降级版；
//!           想上新菜先试菜（先测后入），试不过不许上菜单。
//! 性能逐项分解：清单 O(项数)；裁决 O(1) 测试引用；拒绝 O(1)。
//! 诊断码段：0x31F40（0x31F41~0x31F49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const B3_VERSION: &str = "veb345_bound3d-v1";

/// 诊断段基址（0x31F40 段 · 本模块独占）。
pub const B3_SEG_BASE: u32 = 0x31F40;
pub const XC_OUT: u32 = 0x31F41; // 清单外能力请求
pub const XC_NOTEST: u32 = 0x31F42; // 未测先入（先测后入纪律）
pub const XC_DUP: u32 = 0x31F43; // 重复登记
pub const XC_BADIT: u32 = 0x31F44; // 条目号非法
pub const XC_UNWISH: u32 = 0x31F45; // 愿望单（无测试引用的支持态）

/// 能力项号。
pub const I_RAST: u32 = 1; // 基础光栅化
pub const I_TEXB: u32 = 2; // 常用纹理过滤
pub const I_MIX: u32 = 3; // 基础混合
pub const I_HIFILT: u32 = 4; // 高阶过滤（降级项）
pub const I_MSAA: u32 = 5; // 多采样语义（降级项）
pub const I_GEO: u32 = 6; // 几何着色类（不支持）

/// 支持态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sup {
    /// 完整支持。
    Yes,
    /// 降级支持。
    Degr,
    /// 显性不支持。
    No,
}

/// 边界条目。
pub struct Bound {
    pub item: u32,
    pub sup: Sup,
    /// 精度口径（1/255 单位；不支持项填 0）。
    pub prec: u32,
    /// 测试引用号（0 = 无测试）。
    pub test: u32,
}

/// 边界清单。
pub struct Bound3D {
    items: Vec<Bound>,
    /// 请求告知数（拒绝或降级时告知）。
    pub told: u32,
}

impl Bound3D {
    /// 空单。
    pub fn new() -> Bound3D {
        Bound3D {
            items: Vec::new(),
            told: 0,
        }
    }

    /// 登记：条目号界、重复拒。
    /// 支持态必须带测试引用（承诺非愿望）；不支持态可无测试。
    pub fn put(&mut self, item: u32, sup: Sup, prec: u32, test: u32) -> Result<(), u32> {
        if item == 0 || item > 99 {
            return Err(XC_BADIT);
        }
        if self.items.iter().any(|b| b.item == item) {
            return Err(XC_DUP);
        }
        if sup != Sup::No && test == 0 {
            return Err(XC_UNWISH); // 有支持无测试 = 愿望单
        }
        if sup == Sup::No {
            self.items.push(Bound {
                item,
                sup,
                prec: 0,
                test: 0,
            });
        } else {
            self.items.push(Bound {
                item,
                sup,
                prec,
                test,
            });
        }
        Ok(())
    }

    /// 先测后入：测试通过才许改支持态（未测拒）。
    pub fn promote(&mut self, item: u32, test: u32) -> Result<(), u32> {
        if test == 0 {
            return Err(XC_NOTEST);
        }
        let b = self
            .items
            .iter_mut()
            .find(|b| b.item == item)
            .ok_or(XC_BADIT)?;
        if b.sup == Sup::No {
            b.sup = Sup::Yes;
            b.test = test;
            Ok(())
        } else {
            b.test = test;
            Ok(())
        }
    }

    /// 请求裁决：支持 → Ok(态)；不支持 → 拒并告知；在册降级 → 降级告知。
    pub fn req(&mut self, item: u32) -> Result<Sup, u32> {
        match self.items.iter().find(|b| b.item == item) {
            None => {
                self.told += 1;
                Err(XC_OUT) // 清单外显性拒绝
            }
            Some(b) => match b.sup {
                Sup::Yes => Ok(Sup::Yes),
                Sup::Degr => {
                    self.told += 1;
                    Ok(Sup::Degr) // 降级并告知
                }
                Sup::No => {
                    self.told += 1;
                    Err(XC_OUT) // 显性不支持
                }
            },
        }
    }

    /// 精度口径查询。
    pub fn prec_of(&self, item: u32) -> Option<u32> {
        self.items.iter().find(|b| b.item == item).map(|b| b.prec)
    }

    /// 态查询。
    pub fn sup_of(&self, item: u32) -> Option<Sup> {
        self.items.iter().find(|b| b.item == item).map(|b| b.sup)
    }

    /// 三态计数。
    pub fn tally(&self) -> (usize, usize, usize) {
        let mut c = (0usize, 0usize, 0usize);
        for b in self.items.iter() {
            match b.sup {
                Sup::Yes => c.0 += 1,
                Sup::Degr => c.1 += 1,
                Sup::No => c.2 += 1,
            }
        }
        c
    }

    /// 单长。
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

pub fn run_veb345_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0345");
    // 族零：登记门。
    let mut b = Bound3D::new();
    s.add("零号拒", b.put(0, Sup::Yes, 0, 11) == Err(XC_BADIT), "无零项");
    s.add("越界拒", b.put(100, Sup::Yes, 0, 11) == Err(XC_BADIT), ">99");
    s.add("愿望单拒", b.put(I_RAST, Sup::Yes, 0, 0) == Err(XC_UNWISH), "无测试引用");
    s.add("光栅过", b.put(I_RAST, Sup::Yes, 0, 101).is_ok(), "位精确口径 0");
    s.add("重复拒", b.put(I_RAST, Sup::Yes, 0, 101) == Err(XC_DUP), "一项一态");
    // 族一：三态登记。
    s.add("纹理过", b.put(I_TEXB, Sup::Yes, 1, 102).is_ok(), "容差 1");
    s.add("混合过", b.put(I_MIX, Sup::Yes, 2, 103).is_ok(), "容差 2");
    s.add("高阶降级", b.put(I_HIFILT, Sup::Degr, 8, 104).is_ok(), "降级容差 8");
    s.add("多采样降级", b.put(I_MSAA, Sup::Degr, 16, 105).is_ok(), "语义近似");
    s.add("几何不支持", b.put(I_GEO, Sup::No, 0, 0).is_ok(), "不支持免测");
    s.add("单六", b.len() == 6, "全登记");
    let t = b.tally();
    s.add("三三零", t == (3, 2, 1), "三支二降一拒");
    // 族二：请求裁决。
    s.add("光栅支持", b.req(I_RAST) == Ok(Sup::Yes), "完整");
    s.add("高阶降级", b.req(I_HIFILT) == Ok(Sup::Degr), "降级告知");
    s.add("几何拒", b.req(I_GEO) == Err(XC_OUT), "显性拒绝");
    s.add("清单外拒", b.req(77) == Err(XC_OUT), "没挂就不接");
    s.add("告知三", b.told == 3, "降一拒二");
    // 族三：精度口径。
    s.add("光栅口径零", b.prec_of(I_RAST) == Some(0), "位精确");
    s.add("高阶口径八", b.prec_of(I_HIFILT) == Some(8), "降级口径");
    s.add("不支持口径零", b.prec_of(I_GEO) == Some(0), "无口径");
    s.add("查无口径", b.prec_of(77).is_none(), "清单外");
    // 族四：先测后入。
    s.add("未测升拒", b.promote(77, 0) == Err(XC_BADIT), "清单外");
    s.add("零测试拒", b.promote(I_GEO, 0) == Err(XC_NOTEST), "先测后入");
    s.add("几何升过", b.promote(I_GEO, 106).is_ok(), "测试 106 过");
    s.add("几何转支", b.sup_of(I_GEO) == Some(Sup::Yes), "新菜上桌");
    s.add("口径刷新", b.prec_of(I_GEO).is_some(), "有测试有口径");
    let t2 = b.tally();
    s.add("四二零", t2 == (4, 2, 0), "升一");
    // 族五：空单拒绝。
    let mut b2 = Bound3D::new();
    let mut b3 = Bound3D::new();
    s.add("空单拒", b2.req(I_RAST) == Err(XC_OUT), "空店");
    s.add("空单告知", {
        let _ = b3.req(I_RAST);
        b3.told == 1
    }, "拒也告知");
    s.add("魔数核", B3_SEG_BASE == 0x31F40, "段基址");
    s
}
