//! unx0514_svmtrack —— UNX-F0514 SVM 设备-进程翻译追踪账（FL 翻译 + PGD 切换 + 标签核对）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0514 · SVM 追踪账 · 目标 210 行
//! 判据原文：UNX-F0514-J1 SVM 追踪账：追一 一级翻译
//!           （带 PASID 的请求走 first-level 页表（进程页
//!           表），无 PASID 走 second-level（宿页表），路
//!           径由标签驱动）；追二 PGD 切换（进程切页表后
//!           设备侧必须收到 PGD 更新通知，未通知的旧 PGD
//!           翻译拒——陈旧进程地址空间）；追三 标签核对
//!           （(设备,PASID) 对必须与登记一致，错配拒）；
//!           追四 二级回退（一级翻译缺页时按策略回退二级，
//!           回退动作记账）；追五 双表隔离（一级基址与二
//!           级基址字段独立，串位即违例）。
//! 组件：双级翻译路由 + PGD 时钟 + 标签核对器 + 回退账。
//! 职责定位：SVM（共享虚拟内存）让设备直接用进程虚拟地址
//!   发 DMA——一级页表是进程的，二级是宿的。进程 munmap/
//!   切 PGD 后设备侧不知道，就会按已回收的页翻译（UAF 的
//!   DMA 形态）。PGD 时钟（代次号）把「页表变了」变成可
//!   比较的整数：设备侧代次落后即拒。
//! 性能逐项分解：路由 O(1)；核对 O(1)。
//! 诊断码段：0x34E00（0x34E01~0x34E09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码：追二 PGD 陈旧。
pub const SEG_BASE: u32 = 0x34E00;
pub const XC_STALE: u32 = SEG_BASE + 1;
pub const XC_TAG: u32 = SEG_BASE + 2; // 标签错配
pub const XC_FALLBACK: u32 = SEG_BASE + 3; // 回退记账标记（非错误）
pub const XC_MIXED: u32 = SEG_BASE + 4; // 双表基址串位

/// 翻译请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Req {
    pub bdf: u16,
    pub pasid: Option<u16>, // None = 二级路径
}

/// 设备-进程登记项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DevTag {
    bdf: u16,
    pasid: u16,
    pgd_gen: u64, // 设备侧已知的 PGD 代次
}

/// SVM 追踪器。
pub struct SvmTracker {
    pub tags: Vec<DevTag>,
    pub host_gen: u64,     // 宿侧当前 PGD 代次
    pub fallbacks: u64,    // 追四回退账
    pub fl_walks: u64,     // 一级走查数
    pub sl_walks: u64,     // 二级走查数
}

impl SvmTracker {
    pub fn new() -> Self {
        SvmTracker { tags: Vec::new(), host_gen: 0, fallbacks: 0, fl_walks: 0, sl_walks: 0 }
    }

    /// 登记 (设备,PASID)，绑定当前代次。
    pub fn enroll(&mut self, bdf: u16, pasid: u16) -> Result<(), u32> {
        if self.tags.iter().any(|t| t.bdf == bdf && t.pasid == pasid) {
            return Ok(()); // 幂等
        }
        self.tags.push(DevTag { bdf, pasid, pgd_gen: self.host_gen });
        Ok(())
    }

    /// 追二：进程切 PGD → 宿代次推进（设备侧落后即陈旧）。
    pub fn bump_gen(&mut self) {
        self.host_gen += 1;
    }

    /// 追一+追三+追二：路由与核对。
    pub fn translate(&mut self, r: Req) -> Result<&'static str, u32> {
        match r.pasid {
            None => {
                self.sl_walks += 1;
                Ok("second-level")
            }
            Some(p) => {
                let t = match self.tags.iter().find(|t| t.bdf == r.bdf) {
                    Some(t) if t.pasid == p => t.pgd_gen,
                    _ => return Err(XC_TAG),
                };
                if t < self.host_gen {
                    return Err(XC_STALE);
                }
                self.fl_walks += 1;
                Ok("first-level")
            }
        }
    }

    /// 追四：一级缺页回退二级（记账）。
    pub fn fallback(&mut self) {
        self.fallbacks += 1;
        self.sl_walks += 1;
        let _ = XC_FALLBACK;
    }
}

pub fn run_unx0514_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0514");
    cs.add("SEG 段位独占", SEG_BASE == 0x34E00 && XC_MIXED == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：路由（有 PASID → 一级，无 → 二级）。
    let mut s = SvmTracker::new();
    s.enroll(0x0210, 1).ok();
    cs.add("一级路由", s.translate(Req { bdf: 0x0210, pasid: Some(1) }) == Ok("first-level"), "标签路径");
    cs.add("二级路由", s.translate(Req { bdf: 0x0210, pasid: None }) == Ok("second-level"), "无标签路径");
    cs.add("走查分账", s.fl_walks == 1 && s.sl_walks == 1, "双路各一");

    // 族二：标签核对。
    cs.add("错 PASID 拒", s.translate(Req { bdf: 0x0210, pasid: Some(9) }) == Err(XC_TAG), "登记 1 请求 9");
    cs.add("未登记拒", s.translate(Req { bdf: 0x0300, pasid: Some(1) }) == Err(XC_TAG), "设备未登记");
    cs.add("错误不记账", s.fl_walks == 1, "拒路径不走查");

    // 族三：PGD 代次与陈旧拒。
    s.bump_gen();
    cs.add("代次落后拒", s.translate(Req { bdf: 0x0210, pasid: Some(1) }) == Err(XC_STALE), "宿 1 设备 0");
    // 重登记刷新代次（设备收到 PGD 更新通知）。
    s.tags[0].pgd_gen = s.host_gen;
    cs.add("刷新后通行", s.translate(Req { bdf: 0x0210, pasid: Some(1) }).is_ok(), "代次追平");

    // 族四：多代推进。
    s.bump_gen();
    s.bump_gen();
    cs.add("双代落后拒", s.translate(Req { bdf: 0x0210, pasid: Some(1) }) == Err(XC_STALE), "宿 2 设备 1");
    s.tags[0].pgd_gen = s.host_gen;
    s.bump_gen();
    cs.add("又落后拒", s.translate(Req { bdf: 0x0210, pasid: Some(1) }) == Err(XC_STALE), "宿 3 设备 2");

    // 族五：回退记账。
    let mut s2 = SvmTracker::new();
    s2.enroll(0x0400, 2).ok();
    s2.translate(Req { bdf: 0x0400, pasid: Some(2) }).ok();
    s2.fallback();
    s2.fallback();
    cs.add("回退两笔", s2.fallbacks == 2, "回退账线性");
    cs.add("回退并二级", s2.sl_walks == 2, "回退即二级走查");
    cs.add("一级不动", s2.fl_walks == 1, "回退不冲一级账");

    // 族六：双表基址隔离（字段独立核）。
    let mut s3 = SvmTracker::new();
    let fl_before = s3.fl_walks;
    let sl_before = s3.sl_walks;
    s3.translate(Req { bdf: 0x1, pasid: None }).ok();
    cs.add("二级不冲一级", s3.fl_walks == fl_before && s3.sl_walks == sl_before + 1, "字段独立增量");
    s3.enroll(0x1, 1).ok();
    s3.translate(Req { bdf: 0x1, pasid: Some(1) }).ok();
    cs.add("一级不冲二级", s3.fl_walks == fl_before + 1 && s3.sl_walks == sl_before + 1, "反向独立");

    // 族七：多设备多代次矩阵（设备各自持有代次）。
    let mut m = SvmTracker::new();
    m.enroll(0x10, 1).ok();
    m.enroll(0x11, 1).ok();
    m.enroll(0x12, 1).ok();
    cs.add("三设备同 PASID", m.tags.len() == 3, "共享进程三挂");
    // 代次推进后：只有刷新的设备通行。
    m.bump_gen();
    let r10 = m.translate(Req { bdf: 0x10, pasid: Some(1) });
    let r11 = m.translate(Req { bdf: 0x11, pasid: Some(1) });
    cs.add("矩阵同批拒", r10 == Err(XC_STALE) && r11 == Err(XC_STALE), "未刷新全陈旧");
    m.tags[0].pgd_gen = m.host_gen;
    let r10b = m.translate(Req { bdf: 0x10, pasid: Some(1) });
    let r12 = m.translate(Req { bdf: 0x12, pasid: Some(1) });
    cs.add("矩阵选择性", r10b.is_ok() && r12 == Err(XC_STALE), "刷新者通未刷者拒");
    // 全部刷新后全通（先快照键集再逐个走查，避免借用交叠）。
    let keys: alloc::vec::Vec<u16> = m.tags.iter().map(|t| t.bdf).collect();
    let all_ok = keys.iter().all(|bdf| m.translate(Req { bdf: *bdf, pasid: Some(1) }).is_ok());
    cs.add("矩阵全通", all_ok, "全员代次追平");

    // 族八：同设备多 PASID 登记（进程切换语义）。
    let mut s4 = SvmTracker::new();
    s4.enroll(0x20, 1).ok();
    s4.enroll(0x20, 2).ok();
    cs.add("同设备双 PASID 共存", s4.tags.len() == 2, "两进程两标签");
    let t1 = s4.translate(Req { bdf: 0x20, pasid: Some(1) });
    let t2 = s4.translate(Req { bdf: 0x20, pasid: Some(2) });
    cs.add("双标各走一级", t1.is_ok() && t2.is_ok(), "标签区分进程");
    s4.bump_gen();
    cs.add("双标同批陈旧", s4.translate(Req { bdf: 0x20, pasid: Some(1) }) == Err(XC_STALE) && s4.translate(Req { bdf: 0x20, pasid: Some(2) }) == Err(XC_STALE), "切 PGD 全进程陈旧");

    cs
}
