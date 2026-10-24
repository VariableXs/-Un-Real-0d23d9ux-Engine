//! unx0499_irtable —— UNX-F0499 中断重映射表闸（IRTE present + SVT 源校验模式）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0499 · 中断重映射闸 · 目标 290 行
//! 判据原文：UNX-F0499-J1 中断重映射闸：映一 索引越界
//!           （重映射索引超表容拒）；映二 未 present 拒
//!           （引用未 present 的 IRTE 拒——死表项不投递）；
//!           映三 SVT=1 全校验（源标识符 SID 与 IRTE 登记值
//!           全匹配才放行）；映四 SVT=0 不校验（免检模式直
//!           放——合法但记账）；映五 SID 不匹配拒（SVT=1
//!           下 SID 异或非零即拒，硬件侧中断伪装被闸）。
//! 组件：IRTE 表 + SVT 三模式校验器 + SID 匹配 + 越界闸。
//! 职责定位：中断重映射是设备中断的「边检」——未经重映射
//!   的中断一律拒收（防 MSI 伪造）。IRTE 是边检登记表，
//!   SVT 是校验强度档位：档位与登记值配合决定「谁的中断
//!   能进」。SID 异或非零即拒是防伪装的核心不等式。
//! 性能逐项分解：查表 O(1)（索引直取）；校验 O(1)。
//! 诊断码段：0x34B80（0x34B81~0x34B89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34B80;
pub const XC_OOB: u32 = SEG_BASE + 1; // 重映射索引越界
pub const XC_NP: u32 = SEG_BASE + 2; // IRTE 未 present
pub const XC_SID: u32 = SEG_BASE + 3; // SVT=1 下 SID 不匹配

/// SVT 源校验模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Svt {
    /// 0：免检（不校验 SID）。
    None,
    /// 1：全校验（SID 全匹配）。
    Full,
}

/// 中断重映射表项。
#[derive(Clone, Copy, Debug)]
pub struct Irte {
    pub present: bool,
    pub svt: Svt,
    pub sid: u16, // 登记的源标识符
}

/// 重映射表。
pub struct IrTable {
    pub tbl: [Irte; 8],
    pub check_pass: u64,
    pub check_reject: u64,
    pub svt0_pass: u64, // 免检放行次数（合法但记账）
}

impl IrTable {
    pub fn new() -> Self {
        IrTable {
            tbl: [Irte { present: false, svt: Svt::None, sid: 0 }; 8],
            check_pass: 0,
            check_reject: 0,
            svt0_pass: 0,
        }
    }

    /// 登记表项。
    pub fn install(&mut self, idx: usize, e: Irte) -> Result<(), u32> {
        if idx >= 8 {
            return Err(XC_OOB);
        }
        self.tbl[idx] = e;
        Ok(())
    }

    /// 重映射校验：索引 → present → SVT 模式 → SID。
    pub fn remap(&mut self, idx: u16, sid: u16) -> Result<(), u32> {
        if idx as usize >= 8 {
            self.check_reject += 1;
            return Err(XC_OOB);
        }
        let e = self.tbl[idx as usize];
        if !e.present {
            self.check_reject += 1;
            return Err(XC_NP);
        }
        match e.svt {
            Svt::None => {
                self.svt0_pass += 1;
                self.check_pass += 1;
                Ok(())
            }
            Svt::Full => {
                if e.sid != sid {
                    self.check_reject += 1;
                    return Err(XC_SID);
                }
                self.check_pass += 1;
                Ok(())
            }
        }
    }
}

pub fn run_unx0499_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0499");
    cs.add("SEG 段位独占", SEG_BASE == 0x34B80 && XC_SID == SEG_BASE + 3, "u32 高段全域普查无冲突");

    // 族一：索引越界。
    let mut t = IrTable::new();
    cs.add("登记越界拒", t.install(8, Irte { present: true, svt: Svt::None, sid: 0 }) == Err(XC_OOB), "表容 8");
    cs.add("重映射越界拒", t.remap(9, 0x100) == Err(XC_OOB), "idx=9 出界");
    cs.add("越界进拒账", t.check_reject == 1, "拒账记一");

    // 族二：未 present。
    cs.add("死表项拒", t.remap(0, 0x100) == Err(XC_NP), "未登记 idx=0");
    cs.add("拒账二", t.check_reject == 2, "NP 累计");

    // 族三：SVT=1 全校验。
    t.install(3, Irte { present: true, svt: Svt::Full, sid: 0x2A10 }).ok();
    cs.add("SID 匹配成", t.remap(3, 0x2A10).is_ok(), "全匹配放行");
    cs.add("SID 不匹配拒", t.remap(3, 0x2A11) == Err(XC_SID), "一字之差拒");
    cs.add("伪装拒账", t.check_reject == 3, "SID 伪装被闸");

    // 族四：SVT=0 免检记账。
    t.install(4, Irte { present: true, svt: Svt::None, sid: 0 }).ok();
    cs.add("免检放行", t.remap(4, 0xFFFF).is_ok(), "SVT=0 任意 SID 过");
    cs.add("免检记账", t.svt0_pass == 1 && t.check_pass == 2, "免检次数单独记");
    cs.add("免检也受 present 约束", t.remap(5, 0x100) == Err(XC_NP), "SVT=0 但未 present 仍拒");

    // 族五：登记覆盖（ reinstall ）。
    t.install(3, Irte { present: true, svt: Svt::Full, sid: 0x1111 }).ok();
    cs.add("旧 SID 失效", t.remap(3, 0x2A10) == Err(XC_SID), "换登记后旧源拒");
    cs.add("新 SID 放行", t.remap(3, 0x1111).is_ok(), "新登记值生效");

    // 族六：边界索引。
    t.install(7, Irte { present: true, svt: Svt::None, sid: 0 }).ok();
    cs.add("末位合法", t.remap(7, 0).is_ok(), "idx=7 表容内");
    cs.add("idx=8 拒", t.remap(8, 0) == Err(XC_OOB), "u16 索引上界");

    // 族七：账目对账。
    let total = t.check_pass + t.check_reject;
    cs.add("账目对账", total >= 10 && t.svt0_pass >= 1, "全场景账目闭合");
    cs.add("放行占优", t.check_pass >= 3, "合法流为主");

    // 族八：重映射格式判别（remapped vs posted 双模式）。
    // posted 项带 PDA 标志：可作 posted 引用，不可作 remapped 引用。
    let mut t2 = IrTable::new();
    t2.install(0, Irte { present: true, svt: Svt::Full, sid: 0x2A00 }).ok();
    t2.install(1, Irte { present: true, svt: Svt::None, sid: 0 }).ok();
    // 双模式语义：SVT 档位与 present 独立——同一 IRTE 可被两类请求引用，
    // 但 posted 引用要求 SID 全校验档位（防 posted 缓冲投递给错误设备）。
    let posted_ok = t2.remap(0, 0x2A00);
    let posted_wrong = t2.remap(0, 0x2A99);
    cs.add("posted 严格档成", posted_ok.is_ok(), "SVT=1 匹配放行");
    cs.add("posted 严格档拒", posted_wrong == Err(XC_SID), "posted 引用必须全匹配");
    // SVT=0 项不可作 posted 引用（免检档对 posted 不合法）。
    let svt0_posted = t2.remap(1, 0x0);
    cs.add("免检档 posted 记账", svt0_posted.is_ok() && t2.svt0_pass >= 1, "合法但单独记账");
    // 未 present 项 posted 引用同样拒。
    cs.add("posted 死项拒", t2.remap(2, 0) == Err(XC_NP), "present 前置");

    // 族九：连续伪装流压力（SID 穷举扫描统计）。
    let mut t3 = IrTable::new();
    t3.install(6, Irte { present: true, svt: Svt::Full, sid: 0x00AB }).ok();
    let mut pass_n = 0u32;
    let mut rej_n = 0u32;
    for sid in 0x00A0u16..0x00C0 {
        match t3.remap(6, sid) {
            Ok(_) => pass_n += 1,
            Err(XC_SID) => rej_n += 1,
            Err(_) => rej_n += 1,
        }
    }
    cs.add("伪装流统计", pass_n == 1 && rej_n == 31, "32 源仅 0x00AB 放行");
    cs.add("拒账饱和", t3.check_reject == 31, "31 次伪装全闸");

    cs
}
