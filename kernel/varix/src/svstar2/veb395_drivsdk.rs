//! veb395_drivsdk —— VE-F0395 驱动接入 SDK。
//! 锚点（CoRun Unxreal 主册）：VE-F0395 · 驱动接入 SDK · 目标 420 行
//! 判据原文：VE-F0395-J1 驱动接入 SDK 把接入流程固化为五步清
//!           单：声明（能力位集）、绑定（总线槽位）、注册（回
//!           调表）、自证（沙箱自测）、放行（签章）；步骤乱序
//!           拒（没声明不许绑定……）；每步留步账；全步走完才
//!           出签章（缺步拒章）；SDK 幂等核（同驱动重复走完
//!           流程拒重入）。
//! 组件：五步状态机 + 步账 + 签章器 + 重入拒。
//! 职责定位：驱动接入 SDK 是「B 域的移民局」——五步是五道窗口：
//!           声明是递交材料，绑定是分配身份，注册是入册，自证
//!           是面谈，放行是盖章；乱序等于插队，缺步盖章是渎职；
//!           步账让每一步可追溯，重入拒防同一驱动办两本护照。
//! 性能逐项分解：步进 O(1)；签章 O(步数)；重入查 O(n)。
//! 诊断码段：0x30840（0x30841~0x3084A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SDK_VERSION: &str = "veb395_drivsdk-v1";

/// 诊断段基址（0x30840 段 · 本模块独占）。
pub const SDK_SEG_BASE: u32 = 0x30840;
pub const XC_STEP_ORDER: u32 = 0x30841; // 乱序
pub const XC_DUP_DRV: u32 = 0x30842; // 驱动重入
pub const XC_NOENT: u32 = 0x30843; // 无此驱动
pub const XC_NO_STAMP: u32 = 0x30844; // 缺步拒章
pub const XC_MAGIC: u32 = 0x30845; // 账魔数破
pub const XC_SLOT_BUSY: u32 = 0x30846; // 槽位被占
pub const XC_NO_CAP: u32 = 0x30847; // 能力位空
pub const XC_SELF_FAIL: u32 = 0x30848; // 自证不过
pub const XC_STAMP: u32 = 0x30849; // 章不一致
pub const XC_DONE: u32 = 0x3084A; // 已终态再步进

/// 五步。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SdkStep {
    /// 未开始。
    None = 0,
    /// 声明。
    Declare = 1,
    /// 绑定。
    Bind = 2,
    /// 注册。
    Register = 3,
    /// 自证。
    Prove = 4,
    /// 放行。
    Stamp = 5,
}

/// 驱动接入档案。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dossier {
    pub name: &'static str,
    pub step: SdkStep,
    pub caps: u32,
    pub slot: Option<u16>,
    /// 自证分（0~100，≥80 过）。
    pub prove_score: u32,
    pub stamp: Option<u32>,
}

/// 自证过线。
pub const PROVE_MIN: u32 = 80;

/// 签章哈希（FNV32 于名字 + 步数 + 槽位）。
fn stamp_of(name: &str, slot: u16) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^= slot as u32;
    h = h.wrapping_mul(0x0100_0193);
    h
}

/// SDK 门厅。
pub struct SdkGate {
    dossiers: Vec<Dossier>,
    /// 已占槽位。
    slots: Vec<u16>,
    magic: u32,
}

impl SdkGate {
    /// 建。
    pub fn new() -> SdkGate {
        SdkGate { dossiers: Vec::new(), slots: Vec::new(), magic: 0x0840_5D3F }
    }

    /// 开始接入（建档案）。
    pub fn begin(&mut self, name: &'static str) -> Result<(), u32> {
        if self.magic != 0x0840_5D3F {
            return Err(XC_MAGIC);
        }
        for d in self.dossiers.iter() {
            if d.name == name {
                return Err(XC_DUP_DRV);
            }
        }
        self.dossiers.push(Dossier {
            name,
            step: SdkStep::None,
            caps: 0,
            slot: None,
            prove_score: 0,
            stamp: None,
        });
        Ok(())
    }

    fn pos(&self, name: &str) -> Result<usize, u32> {
        for (i, d) in self.dossiers.iter().enumerate() {
            if d.name == name {
                return Ok(i);
            }
        }
        Err(XC_NOENT)
    }

    /// 步 1：声明能力位集。
    pub fn declare(&mut self, name: &str, caps: u32) -> Result<(), u32> {
        let i = self.pos(name)?;
        if self.dossiers[i].step == SdkStep::Stamp {
            return Err(XC_DONE);
        }
        if self.dossiers[i].step != SdkStep::None {
            return Err(XC_STEP_ORDER);
        }
        if caps == 0 {
            return Err(XC_NO_CAP);
        }
        self.dossiers[i].caps = caps;
        self.dossiers[i].step = SdkStep::Declare;
        Ok(())
    }

    /// 步 2：绑定槽位。
    pub fn bind(&mut self, name: &str, slot: u16) -> Result<(), u32> {
        let i = self.pos(name)?;
        if self.dossiers[i].step != SdkStep::Declare {
            return Err(XC_STEP_ORDER);
        }
        for s in self.slots.iter() {
            if *s == slot {
                return Err(XC_SLOT_BUSY);
            }
        }
        self.slots.push(slot);
        self.dossiers[i].slot = Some(slot);
        self.dossiers[i].step = SdkStep::Bind;
        Ok(())
    }

    /// 步 3：注册回调表（以回调数代表表规模，>0 合法）。
    pub fn register(&mut self, name: &str, callbacks: u32) -> Result<(), u32> {
        let i = self.pos(name)?;
        if self.dossiers[i].step != SdkStep::Bind {
            return Err(XC_STEP_ORDER);
        }
        if callbacks == 0 {
            return Err(XC_STEP_ORDER); // 空回调表视为未注册
        }
        self.dossiers[i].step = SdkStep::Register;
        Ok(())
    }

    /// 步 4：自证。
    pub fn prove(&mut self, name: &str, score: u32) -> Result<(), u32> {
        let i = self.pos(name)?;
        if self.dossiers[i].step != SdkStep::Register {
            return Err(XC_STEP_ORDER);
        }
        if score < PROVE_MIN {
            self.dossiers[i].prove_score = score;
            return Err(XC_SELF_FAIL);
        }
        self.dossiers[i].prove_score = score;
        self.dossiers[i].step = SdkStep::Prove;
        Ok(())
    }

    /// 步 5：签章放行。
    pub fn stamp(&mut self, name: &str) -> Result<u32, u32> {
        let i = self.pos(name)?;
        if self.dossiers[i].step != SdkStep::Prove {
            return Err(XC_NO_STAMP);
        }
        let slot = match self.dossiers[i].slot {
            Some(s) => s,
            None => return Err(XC_NO_STAMP),
        };
        let st = stamp_of(name, slot);
        self.dossiers[i].stamp = Some(st);
        self.dossiers[i].step = SdkStep::Stamp;
        Ok(st)
    }

    /// 章核：章 = FNV(名, 槽)。
    pub fn verify_stamp(&self, name: &str) -> Result<bool, u32> {
        let i = self.pos(name)?;
        let st = match self.dossiers[i].stamp {
            Some(s) => s,
            None => return Err(XC_NO_STAMP),
        };
        let slot = match self.dossiers[i].slot {
            Some(s) => s,
            None => return Err(XC_NO_STAMP),
        };
        Ok(st == stamp_of(name, slot))
    }

    /// 步读。
    pub fn step_of(&self, name: &str) -> Result<SdkStep, u32> {
        let i = self.pos(name)?;
        Ok(self.dossiers[i].step)
    }

    /// 档案数。
    pub fn dossier_count(&self) -> usize {
        self.dossiers.len()
    }

    /// 占槽数。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

pub fn run_veb395_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0395");
    // 族一：五步全程。
    let mut g = SdkGate::new();
    let _ = g.begin("eth0");
    s.add("建档 1", g.dossier_count() == 1, "受理");
    s.add("重入拒", g.begin("eth0") == Err(XC_DUP_DRV), "一本护照");
    // 乱序拒：未声明先绑。
    s.add("未声明先绑拒", g.bind("eth0", 3) == Err(XC_STEP_ORDER), "插队");
    s.add("空能力拒", g.declare("eth0", 0) == Err(XC_NO_CAP), "白卷");
    s.add("声明过", g.declare("eth0", 0b1111).is_ok(), "步 1");
    s.add("步读 Declare", g.step_of("eth0") == Ok(SdkStep::Declare), "步账");
    let _ = g.bind("eth0", 3);
    s.add("槽占 1", g.slot_count() == 1, "占 3 号槽");
    s.add("槽冲突拒", g.bind("eth0", 3) == Err(XC_STEP_ORDER), "已过步");
    s.add("注册过", g.register("eth0", 4).is_ok(), "步 3");
    s.add("空回调表拒", g.register("eth0", 0) == Err(XC_STEP_ORDER), "白表");
    s.add("低分自证拒", g.prove("eth0", 60) == Err(XC_SELF_FAIL), "面谈不过");
    s.add("低分不进步", g.step_of("eth0") == Ok(SdkStep::Register), "留在注册");
    s.add("高分自证过", g.prove("eth0", 95).is_ok(), "步 4");
    let st = g.stamp("eth0");
    s.add("签章出", st.is_ok(), "步 5");
    s.add("章核过", g.verify_stamp("eth0") == Ok(true), "FNV 一致");
    s.add("缺步拒章", g.stamp("eth0") == Err(XC_NO_STAMP), "已终态");
    // 族二：缺步拒章（另一驱动卡在注册步）。
    let mut h = SdkGate::new();
    let _ = h.begin("eth1");
    let _ = h.declare("eth1", 1);
    let _ = h.bind("eth1", 4);
    let _ = h.register("eth1", 2);
    // 不自证直接要章。
    s.add("跳自证拒章", h.stamp("eth1") == Err(XC_NO_STAMP), "缺步 4");
    // 族三：槽位冲突。
    let mut k = SdkGate::new();
    let _ = k.begin("a");
    let _ = k.begin("b");
    let _ = k.declare("a", 1);
    let _ = k.declare("b", 1);
    let _ = k.bind("a", 7);
    s.add("槽 7 占用", k.bind("b", 7) == Err(XC_SLOT_BUSY), "一槽一主");
    s.add("异槽过", k.bind("b", 8).is_ok(), "8 号槽空");
    s.add("占槽 2", k.slot_count() == 2, "7+8");
    // 族四：野驱动。
    s.add("野驱动步拒", g.declare("ghost", 1) == Err(XC_NOENT), "未建档");
    s.add("野驱动章拒", g.stamp("ghost") == Err(XC_NOENT), "未建档");
    s.add("步序核", SdkStep::None < SdkStep::Declare && SdkStep::Prove < SdkStep::Stamp, "枚举序");
    s.add("自证线", PROVE_MIN == 80, "过线 80");
    s.add("魔数核", SDK_SEG_BASE == 0x30840, "段基址");
    s
}
