//! CGPU-F3367 · 验收环境管理（vcv07_env）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3367`
//!
//! **判据（锚点原文）**：真机登记、两组、判据。
//!
//! **职责定位（锚点原文）**：环境：验收环境管理（真机登记/环境
//! 快照——环境实现；测试（登记/快照两组）。
//!
//! # 一、真机登记（验收结论的物理锚点）
//!
//! 每台参与验收的真机必须先登记：机型代号、设备档位（vcv03 的
//! [`DeviceTier] 坐标）、驱动代次、OS 环境。未登记的机器跑出来的
//! 数据**没有归属**——不知道结论对哪格矩阵生效（[`E_VV_UNKNOWN`]）。
//! 登记表容量纪律：定长（[`MAX_MACHINES`]），满即拒，不挤掉已
//! 登记的机器——挤掉一台等于作废它名下全部历史数据。
//!
//! # 二、环境快照（可复现的最低要求）
//!
//! 快照 = 某次验收开跑前的环境指纹：机器 ID + 驱动版本号 + 环境旗
//! 标（合成器/直通/远端）。**先拍快照再开跑**是流程门（vcv04 预备
//! 门的数据来源）；跑完回读快照不一致 = 环境中途漂移，该轮数据
//! 全部作废（[`E_VV_DRIFT`]）——环境漂移下继续用数据等于验收造假。
//!
//! # 三、两组测试（登记/快照）
//!
//! 登记组：登记-查询-容量守门；快照组：拍-回读-漂移检测。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const ENV_VERSION: &str = "V07-env-v1";

/// 未登记机器。
pub const E_VV_UNKNOWN: &str = "E_VV_UNKNOWN";

/// 登记表满。
pub const E_VV_FULL: &str = "E_VV_FULL";

/// 重复登记（同机型代号两台算两台，代号必须唯一）。
pub const E_VV_DUP: &str = "E_VV_DUP";

/// 环境漂移（跑前跑后快照不一致）。
pub const E_VV_DRIFT: &str = "E_VV_DRIFT";

/// 登记表容量。
pub const MAX_MACHINES: usize = 64;

/// 快照账容量。
pub const MAX_SNAPSHOTS: usize = 256;

// ---------------------------------------------------------------------------
// 二、真机登记
// ---------------------------------------------------------------------------

/// 一台登记的真机。
#[derive(Clone, Debug)]
pub struct Machine {
    /// 机型代号（唯一键）。
    pub codename: String,
    /// 设备档位（vcv03 坐标·设备维）。
    pub tier: u8,
    /// 驱动代次（vcv03 坐标·驱动维）。
    pub driver: u8,
    /// OS 环境（vcv03 坐标·OS 维）。
    pub os: u8,
}

/// 真机登记表。
pub struct Registry {
    machines: Vec<Machine>,
    /// 遥测：拒登记次数。
    pub rejects: u64,
}

impl Registry {
    /// 空表。
    pub fn new() -> Registry {
        Registry {
            machines: Vec::new(),
            rejects: 0,
        }
    }

    /// 登记一台真机：容量守门 + 代号唯一。
    pub fn enroll(
        &mut self,
        codename: &str,
        tier: u8,
        driver: u8,
        os: u8,
    ) -> Result<usize, &'static str> {
        if self.machines.len() >= MAX_MACHINES {
            self.rejects += 1;
            return Err(E_VV_FULL);
        }
        if self.machines.iter().any(|m| m.codename == codename) {
            self.rejects += 1;
            return Err(E_VV_DUP);
        }
        self.machines.push(Machine {
            codename: String::from(codename),
            tier,
            driver,
            os,
        });
        Ok(self.machines.len() - 1)
    }

    /// 按代号查机器。
    pub fn find(&self, codename: &str) -> Option<&Machine> {
        self.machines.iter().find(|m| m.codename == codename)
    }

    /// 是否已登记。
    pub fn known(&self, codename: &str) -> bool {
        self.find(codename).is_some()
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.machines.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.machines.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、环境快照
// ---------------------------------------------------------------------------

/// 环境指纹（跑前拍、跑后对）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 机器代号。
    pub machine: String,
    /// 驱动版本号（驱动更新即漂移）。
    pub driver_build: u32,
    /// 环境旗标（bit0 合成器 / bit1 直通 / bit2 远端）。
    pub flags: u8,
}

/// 快照账。
pub struct SnapBook {
    snaps: Vec<Snapshot>,
    /// 遥测：漂移检出次数。
    pub drifts: u64,
}

impl SnapBook {
    /// 空账。
    pub fn new() -> SnapBook {
        SnapBook {
            snaps: Vec::new(),
            drifts: 0,
        }
    }

    /// 拍快照（先拍再跑的流程门数据源）。
    pub fn take(&mut self, s: Snapshot) -> Result<usize, &'static str> {
        if self.snaps.len() >= MAX_SNAPSHOTS {
            return Err(E_VV_FULL);
        }
        self.snaps.push(s);
        Ok(self.snaps.len() - 1)
    }

    /// 跑后回读：与拍下的一致则通过，漂移即作废信号。
    pub fn verify(&mut self, idx: usize, s: &Snapshot) -> Result<(), &'static str> {
        let taken = self.snaps.get(idx).ok_or(E_VV_UNKNOWN)?;
        if taken == s {
            Ok(())
        } else {
            self.drifts += 1;
            Err(E_VV_DRIFT)
        }
    }

    /// 快照数。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }
}

// ---------------------------------------------------------------------------
// 四、两组测试（登记/快照）
// ---------------------------------------------------------------------------

/// CGPU-F3367 域自检：真机登记、两组、判据。
pub fn run_vcv07_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv07_env");

    // --- 组一：登记 ---
    let mut reg = Registry::new();
    cs.add("空表", reg.is_empty() && reg.len() == 0, "初态");
    cs.add(
        "登记 A",
        reg.enroll("AURA-01", 0, 0, 0).is_ok(),
        "旗舰档",
    );
    cs.add(
        "登记 B",
        reg.enroll("MIDO-02", 1, 1, 1).is_ok(),
        "主流档",
    );
    cs.add(
        "代号唯一",
        reg.enroll("AURA-01", 2, 0, 0) == Err(E_VV_DUP),
        "重复拒",
    );
    cs.add(
        "查询命中",
        reg.find("MIDO-02").map(|m| m.tier) == Some(1),
        "find",
    );
    cs.add(
        "未登记可识别",
        !reg.known("GHOST-99"),
        "E_VV_UNKNOWN 前置",
    );
    cs.add(
        "登记数",
        reg.len() == 2 && reg.rejects == 1,
        "含 1 拒",
    );

    // --- 组二：快照 ---
    let mut book = SnapBook::new();
    let s1 = Snapshot {
        machine: String::from("AURA-01"),
        driver_build: 4112,
        flags: 0b001,
    };
    let idx = book.take(s1.clone());
    cs.add("拍快照", idx.is_ok(), "先拍再跑");
    let i = idx.expect("taken");
    cs.add("快照入账", book.len() == 1, "1 条");

    // 回读一致 → 通过。
    let s_same = Snapshot {
        machine: String::from("AURA-01"),
        driver_build: 4112,
        flags: 0b001,
    };
    cs.add(
        "回读一致",
        book.verify(i, &s_same).is_ok(),
        "环境未漂移",
    );

    // 回读漂移（驱动升了版本）→ 作废信号。
    let s_drift = Snapshot {
        machine: String::from("AURA-01"),
        driver_build: 4113,
        flags: 0b001,
    };
    cs.add(
        "漂移检出",
        book.verify(i, &s_drift) == Err(E_VV_DRIFT),
        "驱动变了",
    );
    cs.add("漂移计数", book.drifts == 1, "遥测");

    // 未知下标。
    cs.add(
        "未知快照拒",
        book.verify(99, &s_same) == Err(E_VV_UNKNOWN),
        "E_VV_UNKNOWN",
    );

    // --- 版本声明 ---
    cs.add("版本非空", !ENV_VERSION.is_empty(), "声明");

    cs
}
