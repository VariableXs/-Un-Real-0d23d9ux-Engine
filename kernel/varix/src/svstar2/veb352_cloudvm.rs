//! veb352_cloudvm —— VE-F0352 软渲唯一性场景（云/VM）。
//! 锚点：VE-F0352 · 目标 340 行。
//! 判据原文：正当路径语义（软渲自动启用不按降级告警，通知文
//!           案中性）、规格分档（按 vCPU 档位给性能预期）、
//!           误判区分（GPU 在但不可用 → 走降级语义并区分记
//!           录）、重评估（实例规格变更联动）、判据。
//! 组件：环境探测账 + 分档预期表 + 启用事件。
//! 职责定位：云/VM 软渲是「常驻司机」不是「备胎」——没有
//!           GPU 直通的车就该软渲开，正常出车不打警示灯；
//!           备胎（GPU 在但坏了）才走降级语义分开记账。
//! 性能逐项分解：探测一次性；分档 O(1)；重评估 O(1)。
//! 诊断码段：0x32100（0x32101~0x32109 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 模块版本。
pub const CV_VERSION: &str = "veb352_cloudvm-v1";

/// 诊断段基址（0x32100 段 · 本模块独占）。
pub const CV_SEG_BASE: u32 = 0x32100;
pub const XC_BADVCPU: u32 = 0x32101; // vCPU 档非法
pub const XC_STILLSF: u32 = 0x32102; // 软渲仍不可用（三要素失败）
pub const XC_MISJUDGE: u32 = 0x32103; // GPU 在但不可用（降级语义）
pub const XC_NOENV: u32 = 0x32104; // 未探测先启用

/// vCPU 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 小档（1~2 vCPU）。
    S,
    /// 中档（3~7）。
    M,
    /// 大档（8+）。
    L,
}

/// 分档。
pub fn tier_of(vcpu: u32) -> Result<Tier, u32> {
    if vcpu == 0 {
        return Err(XC_BADVCPU);
    }
    Ok(if vcpu <= 2 {
        Tier::S
    } else if vcpu <= 7 {
        Tier::M
    } else {
        Tier::L
    })
}

/// 性能预期分（相对基准百分比）。
pub fn expect_of(t: Tier) -> u32 {
    match t {
        Tier::S => 30,
        Tier::M => 60,
        Tier::L => 100,
    }
}

/// 启用事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum En {
    /// 正当路径（GPU 缺席，中性文案）。
    Legit,
    /// 降级语义（GPU 在但不可用）。
    Fallback,
    /// 失败（软渲也不可用，三要素）。
    Fail,
}

/// 启用记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rec {
    pub ev: En,
    /// vCPU 档。
    pub t: Tier,
    /// 通知文案代号（0=中性 1=降级告警 2=失败三要素）。
    pub tone: u32,
}

/// 云环境账。
pub struct CloudEnv {
    probed: bool,
    pub gpu_present: bool,
    pub gpu_usable: bool,
    recs: Vec<Rec>,
    /// 规格变更重评估数。
    pub re_evals: u32,
}

impl CloudEnv {
    /// 探测（一次性；重复幂等）。
    pub fn probe(&mut self, gpu_present: bool, gpu_usable: bool) {
        self.probed = true;
        self.gpu_present = gpu_present;
        self.gpu_usable = gpu_usable;
    }

    /// 已探测。
    pub fn ready(&self) -> bool {
        self.probed
    }

    /// 启用裁决：未探测拒；GPU 缺席 → 正当路径（中性文案 0）；
    /// GPU 在但不可用 → 降级语义（告警 1，区分记录）；
    /// 软渲也不可用 → 失败（三要素 2）。
    pub fn enable(&mut self, vcpu: u32, soft_ok: bool) -> Result<Rec, u32> {
        if !self.probed {
            return Err(XC_NOENV);
        }
        let t = tier_of(vcpu)?;
        let (ev, tone) = if !self.gpu_present {
            (En::Legit, 0u32)
        } else if !self.gpu_usable {
            (En::Fallback, 1)
        } else {
            (En::Fallback, 1)
        };
        if !soft_ok {
            return Err(XC_STILLSF);
        }
        let r = Rec { ev, t, tone };
        self.recs.push(r.clone());
        Ok(r)
    }

    /// 规格变更重评估：重探测 + 记数。
    pub fn re_eval(&mut self, gpu_present: bool, gpu_usable: bool) {
        self.re_evals += 1;
        self.probe(gpu_present, gpu_usable);
    }

    /// 正当路径计数。
    pub fn legit_count(&self) -> u32 {
        self.recs.iter().filter(|r| r.ev == En::Legit).count() as u32
    }

    /// 降级语义计数（区分于正当路径）。
    pub fn fb_count(&self) -> u32 {
        self.recs.iter().filter(|r| r.ev == En::Fallback).count() as u32
    }

    /// 中性文案核：正当路径通知 tone 恒 0（不含失败色彩）。
    pub fn tone_ok(&self) -> bool {
        self.recs
            .iter()
            .all(|r| r.ev != En::Legit || r.tone == 0)
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }
}

pub fn run_veb352_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0352");
    // 族零：分档。
    s.add("零 vCPU 拒", tier_of(0) == Err(XC_BADVCPU), "无核");
    s.add("1 核 S", tier_of(1) == Ok(Tier::S), "小档");
    s.add("2 核 S", tier_of(2) == Ok(Tier::S), "小档界");
    s.add("3 核 M", tier_of(3) == Ok(Tier::M), "中档");
    s.add("7 核 M", tier_of(7) == Ok(Tier::M), "中档界");
    s.add("8 核 L", tier_of(8) == Ok(Tier::L), "大档");
    s.add("预期 30", expect_of(Tier::S) == 30, "小档预期");
    s.add("预期 100", expect_of(Tier::L) == 100, "大档满预期");
    // 族一：探测门。
    let mut e = CloudEnv {
        probed: false,
        gpu_present: false,
        gpu_usable: false,
        recs: Vec::new(),
        re_evals: 0,
    };
    s.add("未探测拒启用", e.enable(4, true) == Err(XC_NOENV), "先探测");
    e.probe(false, false);
    s.add("已探测", e.ready(), "一次性");
    e.probe(false, false);
    s.add("幂等探测", e.ready(), "重复无害");
    // 族二：正当路径。
    s.add("GPU 缺席正当", match e.enable(4, true) { Ok(r) => r.ev == En::Legit && r.tone == 0, Err(_) => false }, "中性");
    s.add("记一笔", e.len() == 1, "在账");
    s.add("文案中性", e.tone_ok(), "tone 0");
    s.add("正当计数一", e.legit_count() == 1, "Legit");
    // 族三：误判区分。
    e.re_eval(true, false);
    s.add("重评估一", e.re_evals == 1, "规格变更");
    s.add("GPU 在不可用", match e.enable(4, true) { Ok(r) => r.ev == En::Fallback && r.tone == 1, Err(_) => false }, "降级语义");
    s.add("区分记录", e.fb_count() == 1, "Fallback 独立账");
    s.add("正当仍一", e.legit_count() == 1, "不混账");
    // 族四：软渲也不可用。
    e.re_eval(false, false);
    s.add("软渲仍不可用拒", e.enable(4, false) == Err(XC_STILLSF), "三要素");
    s.add("失败不入账", e.len() == 2, "只两笔");
    // 族五：重评估再入正当。
    e.re_eval(false, false);
    let _ = e.enable(8, true);
    s.add("大档入账", e.len() == 3, "第三笔");
    s.add("大档正当", e.legit_count() == 2, "Legit 二");
    s.add("重评估二", e.re_evals == 2, "两次变更");
    s.add("魔数核", CV_SEG_BASE == 0x32100, "段基址");
    s
}
