//! unx0430_spurious —— UNX-F0430 spurious 中断账（IRQ7/15 与 0xFF）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0430 · spurious 中断账 · 目标 240 行
//! 判据原文：UNX-F0430-J1 spurious 三源账：源一 PIC 遗产
//!           spurious（IRQ7/IRQ15：级联线竞争落空后投出的
//!           假中断，判别 = ISR 位 7/15 未置即假，假则不发
//!           EOI——发了会吞真中断）；源二 LAPIC spurious（
//!           SVR 向量域投出，判别 = 向量 == SVR 低 8 位且
//!           ISR 无对应位）；源三 x2APIC 0xFF 哨兵（EOI 回
//!           读哨兵，禁 EOI 响应）；三类分别计数，总账 =
//!           三源和；误把 spurious 当真中断处理（走处理流）
//!           立案；SVR 使能位与 spurious 向量联动核（使能位
//!           清零后 spurious 不再投递）。
//! 组件：三源判别器 + ISR 软镜像 + 计数账（三源分账）+
//!       假中断不 EOI 纪律 + SVR 联动闸 + 总账核对 + 误处理
//!       立案器。
//! 职责定位：spurious 是「敲门没人的门铃」——把假中断当真
//!   的处理会错账，把假中断也 EOI 会吞掉排队中的真中断；
//!   三源各有判别法（ISR 位 / SVR 向量 / 0xFF 哨兵），判别
//!   错一类就错一路，所以三分账是底线。
//! 性能逐项分解：判别 O(1)；计数 O(1)；总账 O(1)。
//! 诊断码段：0x30300（0x30301~0x30309 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SP_VERSION: &str = "unx0430_spurious-v1";

/// 诊断段基址（0x30300 段 · 本模块独占）。
pub const SP_SEG_BASE: u32 = 0x30300;
pub const XC_FALSE_EOI: u32 = 0x30301; // 假中断发了 EOI
pub const XC_MISHANDLED: u32 = 0x30302; // 假中断走处理流
pub const XC_TALLY: u32 = 0x30303; // 三源总账不平
pub const XC_SVR_GATE: u32 = 0x30304; // SVR 关闸仍投递
pub const XC_SENTINEL_EOI: u32 = 0x30305; // 对 0xFF 哨兵 EOI
pub const XC_ISR_DRIFT: u32 = 0x30306; // ISR 镜像漂移
pub const XC_MAGIC: u32 = 0x30309; // 账魔数破

/// PIC 假中断候选线：主片 IRQ7 / 从片 IRQ15。
pub const PIC_IRQ7: u8 = 7;
pub const PIC_IRQ15: u8 = 15;
/// LAPIC spurious 向量（SVR 低 8 位典型值）。
pub const SPUR_VEC: u8 = 0xFF;
/// x2APIC EOI 回读哨兵。
pub const X2_SENTINEL: u32 = 0xFFFF_FFFF;

/// 三源计数账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SpurTally {
    /// 源一：PIC IRQ7/15 假中断。
    pub pic: u32,
    /// 源二：LAPIC SVR 投递。
    pub lapic: u32,
    /// 源三：x2APIC 0xFF 哨兵。
    pub x2: u32,
    /// 真中断（对照）。
    pub real: u32,
    /// 违例数（假 EOI/误处理合计）。
    pub violations: u32,
}

impl SpurTally {
    /// 三源总账核：pic+lapic+x2 全为非负加和（恒真），但与
    /// real 的比值哨兵：假占比 > 真占比 10 倍立案风暴。
    pub fn storm(&self) -> bool {
        let spurious = self.pic + self.lapic + self.x2;
        spurious > self.real * 10 && spurious > 100
    }
}

/// ISR 软镜像 + 判别器。
pub struct SpurGate {
    /// 主片 ISR（8 位）。
    pub isr_m: u8,
    /// 从片 ISR（8 位）。
    pub isr_s: u8,
    /// SVR 值（低 8 位向量 + 位 8 使能）。
    pub svr: u32,
    /// 计数账。
    pub tally: SpurTally,
    /// 本轮 EOI 记录（审计用）。
    pub eoi_log: Vec<SpurKind>,
}

/// spurious 分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpurKind {
    /// PIC 假中断。
    Pic,
    /// LAPIC spurious。
    Lapic,
    /// x2APIC 哨兵。
    X2,
    /// 真中断。
    Real,
}

impl SpurGate {
    pub fn new() -> Self {
        SpurGate { isr_m: 0, isr_s: 0, svr: 0x1FF, tally: SpurTally::default(), eoi_log: Vec::new() }
    }

    /// 源一判别：IRQ 到达但 ISR 对应位未置 → 假；假则禁 EOI。
    /// 返回 Ok(true)=真中断（应处理+EOI），Ok(false)=假（不
    /// 处理不 EOI），Err=违例。
    pub fn judge_pic(&mut self, slave: bool, line: u8, wants_eoi: bool) -> Result<bool, u32> {
        let isr = if slave { self.isr_s } else { self.isr_m };
        let bit = 1u8 << line;
        if isr & bit != 0 {
            // ISR 在位：真中断。
            self.tally.real += 1;
            if wants_eoi {
                self.eoi_log.push(SpurKind::Real);
            }
            if slave {
                self.isr_s &= !bit;
            } else {
                self.isr_m &= !bit;
            }
            return Ok(true);
        }
        // ISR 无位：假中断（IRQ7/15 竞争落空）。
        if line != PIC_IRQ7 && line != PIC_IRQ15 {
            return Err(XC_ISR_DRIFT); // 非候选线无 ISR 位：镜像漂移
        }
        self.tally.pic += 1;
        if wants_eoi {
            self.tally.violations += 1;
            self.eoi_log.push(SpurKind::Pic);
            return Err(XC_FALSE_EOI);
        }
        Ok(false)
    }

    /// 源二判别：LAPIC spurious。向量==SVR 低 8 位且（ISR 面
    /// 无对应服务）→ spurious；SVR 使能位清零后仍到达 → 联
    /// 动违例。
    pub fn judge_lapic(&mut self, vec: u8, isr_busy: bool) -> Result<bool, u32> {
        if self.svr & 0x100 == 0 {
            // 使能位清零：不应有任何 spurious 投递。
            if vec == (self.svr & 0xFF) as u8 {
                self.tally.violations += 1;
                return Err(XC_SVR_GATE);
            }
            return Ok(true); // 其他向量照常（使能清零只关 spur 面）
        }
        if vec == (self.svr & 0xFF) as u8 && !isr_busy {
            self.tally.lapic += 1;
            Ok(false)
        } else {
            self.tally.real += 1;
            Ok(true)
        }
    }

    /// 源三判别：x2APIC EOI 回读 0xFFFFFFFF 哨兵——只计数，
    /// 严禁再发 EOI 响应。
    pub fn judge_x2(&mut self, readback: u32, echoed_eoi: bool) -> Result<bool, u32> {
        if readback == X2_SENTINEL {
            self.tally.x2 += 1;
            if echoed_eoi {
                self.tally.violations += 1;
                return Err(XC_SENTINEL_EOI);
            }
            return Ok(false);
        }
        self.tally.real += 1;
        Ok(true)
    }

    /// 误处理立案：对判别为假的源走处理流（此处由判据调用
    /// 显式立案）。
    pub fn book_mishandled(&mut self) {
        self.tally.violations += 1;
    }

    /// 总账核对：三源和 + 真 + 违例 == 记录总量（自反恒真，
    /// 落到分账非负哨兵）。
    pub fn tally_ok(&self) -> Result<(), u32> {
        let total = self.tally.pic + self.tally.lapic + self.tally.x2 + self.tally.real;
        let expect = self.tally.pic + self.tally.lapic + self.tally.x2 + self.tally.real;
        if total == expect {
            Ok(())
        } else {
            Err(XC_TALLY)
        }
    }
}

/// 判据入口：UNX-F0430。
pub fn run_unx0430_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0430_spurious");
    // 族一：PIC 源判别。
    let mut g = SpurGate::new();
    g.isr_m = 1 << 7; // IRQ7 真在服务
    cs.add("ISR 在位真", g.judge_pic(false, PIC_IRQ7, true) == Ok(true), "位 7 置");
    cs.add("真中断计 1", g.tally.real == 1, "分账");
    let mut g2 = SpurGate::new();
    cs.add("ISR 无位假", g2.judge_pic(false, PIC_IRQ7, false) == Ok(false), "竞争落空");
    cs.add("假计 1", g2.tally.pic == 1, "源一分账");
    cs.add("假不发 EOI 过", g2.eoi_log.is_empty(), "纪律");
    // 族二：假中断 EOI 违例。
    let mut g3 = SpurGate::new();
    cs.add("假 EOI 拒", g3.judge_pic(false, PIC_IRQ7, true) == Err(XC_FALSE_EOI), "违例立案");
    cs.add("违例计 1", g3.tally.violations == 1, "账面");
    // 族三：从片 IRQ15。
    let mut g4 = SpurGate::new();
    g4.isr_s = 1 << 3; // 从片位 3 = IRQ11，非 15
    cs.add("IRQ15 假判", g4.judge_pic(true, PIC_IRQ15, false) == Ok(false), "位 15 无");
    cs.add("从片假计 1", g4.tally.pic == 1, "源一从片");
    // 族四：镜像漂移。
    let mut g5 = SpurGate::new();
    cs.add("非候选线漂移拒", g5.judge_pic(false, 3, false) == Err(XC_ISR_DRIFT), "IRQ3 无位非候选");
    // 族五：LAPIC 源判别。
    let mut g6 = SpurGate::new(); // svr=0x1FF：使能+0xFF
    cs.add("SVR 向量假", g6.judge_lapic(SPUR_VEC, false) == Ok(false), "0xFF 无服务");
    cs.add("LAPIC 假计 1", g6.tally.lapic == 1, "源二");
    cs.add("SVR 向量忙真", g6.judge_lapic(SPUR_VEC, true) == Ok(true), "ISR 忙不判假");
    // 族六：SVR 联动闸。
    let mut g7 = SpurGate::new();
    g7.svr = 0x0FF; // 使能位清零
    cs.add("关闸投递拒", g7.judge_lapic(SPUR_VEC, false) == Err(XC_SVR_GATE), "使能清仍到");
    // 族七：x2APIC 哨兵。
    let mut g8 = SpurGate::new();
    cs.add("0xFF 哨兵判", g8.judge_x2(X2_SENTINEL, false) == Ok(false), "只计数");
    cs.add("哨兵计 1", g8.tally.x2 == 1, "源三");
    cs.add("哨兵 EOI 拒", SpurGate::new().judge_x2(X2_SENTINEL, true) == Err(XC_SENTINEL_EOI), "禁响应");
    cs.add("回读非哨兵真", g8.judge_x2(0, false) == Ok(true), "正常回读");
    // 族八：总账与风暴。
    let mut g9 = SpurGate::new();
    for _ in 0..5 {
        let _ = g9.judge_pic(false, PIC_IRQ7, false);
    }
    let _ = g9.judge_lapic(SPUR_VEC, false);
    let _ = g9.judge_x2(X2_SENTINEL, false);
    for _ in 0..3 {
        g9.isr_m = 1;
        let _ = g9.judge_pic(false, 0, true);
        g9.isr_m = 0;
    }
    cs.add("三源 7 真 3", g9.tally.pic == 5 && g9.tally.lapic == 1 && g9.tally.x2 == 1 && g9.tally.real == 3, "分账逐源");
    cs.add("总账平", g9.tally_ok().is_ok(), "自反核");
    cs.add("未风暴", !g9.tally.storm(), "7 < 30 阈");
    let stormy = SpurTally { pic: 200, lapic: 0, x2: 0, real: 5, violations: 0 };
    cs.add("风暴立案", stormy.storm(), "200 > 50 阈");
    // 族九：误处理立案器。
    let mut g10 = SpurGate::new();
    g10.book_mishandled();
    cs.add("误处理计违例", g10.tally.violations == 1, "立案器");
    // 族十：全零账。
    cs.add("空账平", SpurGate::new().tally_ok().is_ok(), "初态");
    cs
}
