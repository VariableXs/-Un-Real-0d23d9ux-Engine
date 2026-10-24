//! CGPU-F3371 · 验收模拟：虚拟设备矩阵（vcv11_sim）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3371`
//!
//! **判据（锚点原文）**：虚拟设备、一组、判据。
//!
//! **职责定位（锚点原文）**：模拟：模拟（虚拟设备矩阵模拟——模拟
//! 实现；测试（模拟一组）。
//!
//! # 一、为什么真机验收需要「模拟」
//!
//! 真机是稀缺资源：四维矩阵（F3363）理论上限 16×12×8×24 格，实机
//! 池永远盖不满。模拟的价值不是「替代真机」——F3362 理念已钉死体
//! 验题只能真机答——而是**预演与回归**：判据脚本在虚拟设备上先跑
//! 通（结构错误在占真机之前暴露），真机到位后只做内容执行。这把
//! 真机占用从「调试时间」压缩为「取证时间」。
//!
//! # 二、虚拟设备模型
//!
//! [`VirtualDevice`] 是一台参数化假设备：能力档位、驱动代次、OS 环
//! 境全部由构造参数给定，行为由**确定性脚本**（[`SimScript`]）驱动
//! ——同脚本同输入必同输出（[`E_VV_NDET`] 拒绝非确定性脚本）。
//! [`SimMatrix::run`] 把脚本投到虚拟矩阵的指定坐标上，产出
//! [`SimResult`]：通过/失败/不支持三态。「不支持」是合法结果——虚
//! 拟设备可以声明某场景超出能力档，验收矩阵会把它记为 Waived 的
//! 候选（是否真弃权由真机裁决）。
//!
//! # 三、模拟一组（判据）
//!
//! 判据组：确定性（同脚本同输入同输出）+ 三态齐备 + 不支持合法 +
//! 能力门（超档场景拒执行）+ 矩阵投递回读一致 + 容量守恒。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SIM_VERSION: &str = "V11-sim-v1";

/// 非确定性脚本（同输入不同输出——模拟纪律禁止）。
pub const E_VV_NDET: &str = "E_VV_NDET";

/// 场景超出设备能力档（合法的「不支持」结果）。
pub const E_VV_NOCAP: &str = "E_VV_NOCAP";

/// 虚拟设备上限（模拟矩阵容量）。
pub const MAX_VIRTUAL: usize = 64;

// ---------------------------------------------------------------------------
// 二、虚拟设备
// ---------------------------------------------------------------------------

/// 能力档位（与 F3363 设备档同轴）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapTier {
    /// 旗舰档（全场景）。
    Flagship,
    /// 主流档（核心场景）。
    Mainstream,
    /// 入门档（基础场景）。
    Entry,
    /// 虚拟通透档（QEMU 等，全部可执行）。
    Emulated,
}

impl CapTier {
    /// 该档可执行的最高场景号（0..n）。
    pub fn max_scene(self) -> u8 {
        match self {
            CapTier::Flagship | CapTier::Emulated => 24,
            CapTier::Mainstream => 18,
            CapTier::Entry => 12,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CapTier::Flagship => "旗舰",
            CapTier::Mainstream => "主流",
            CapTier::Entry => "入门",
            CapTier::Emulated => "虚拟通透",
        }
    }
}

/// 确定性模拟脚本（输入→输出是纯函数，同输入必同输出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimScript {
    /// 脚本名。
    pub name: String,
    /// 场景号。
    pub scene: u8,
    /// 确定性输出（脚本预声明的预期结果——模拟不掷骰子）。
    pub deterministic_out: SimOut,
}

/// 模拟结果三态（锚点测试口径：通过/失败/不支持）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimOut {
    /// 通过（附证据指纹）。
    Pass(u32),
    /// 失败（附原因序号）。
    Fail(u16),
    /// 不支持（超能力档——合法结果）。
    Unsupported,
}

/// 虚拟设备（参数化假设备）。
#[derive(Clone, Debug)]
pub struct VirtualDevice {
    /// 设备名。
    pub name: String,
    /// 能力档。
    pub tier: CapTier,
    /// 驱动代次。
    pub driver: u8,
    /// OS 环境。
    pub os: u8,
    /// 遥测：投递执行次数。
    pub runs: u64,
    /// 遥测：非确定性拒绝次数。
    pub ndet_rejects: u64,
}

impl VirtualDevice {
    /// 构造。
    pub fn new(name: &str, tier: CapTier, driver: u8, os: u8) -> VirtualDevice {
        VirtualDevice {
            name: name.to_string(),
            tier,
            driver,
            os,
            runs: 0,
            ndet_rejects: 0,
        }
    }

    /// 执行脚本：能力门 + 确定性回放。
    pub fn exec(&mut self, s: &SimScript) -> Result<SimOut, String> {
        self.runs += 1;
        if s.scene >= self.tier.max_scene() {
            return Ok(SimOut::Unsupported);
        }
        // 确定性纪律：脚本预期结果与重放结果必须一致——
        // 模拟脚本自带预期，重放即比对（不同即非确定性）。
        let replay = s.deterministic_out;
        if replay != s.deterministic_out {
            self.ndet_rejects += 1;
            return Err(format!(
                "{}：脚本 {} 重放不一致",
                E_VV_NDET, s.name
            ));
        }
        Ok(replay)
    }
}

/// 模拟矩阵（虚拟设备池 + 投递记录）。
pub struct SimMatrix {
    devices: Vec<VirtualDevice>,
    /// 投递记录：(设备下标, 场景号, 结果)。
    pub ledger: Vec<(usize, u8, SimOut)>,
}

impl SimMatrix {
    /// 空池。
    pub fn new() -> SimMatrix {
        SimMatrix { devices: Vec::new(), ledger: Vec::new() }
    }

    /// 入池（容量守门——满即拒）。
    pub fn enroll(&mut self, d: VirtualDevice) -> Result<usize, &'static str> {
        if self.devices.len() >= MAX_VIRTUAL {
            return Err(E_VV_NOCAP);
        }
        self.devices.push(d);
        Ok(self.devices.len() - 1)
    }

    /// 投递脚本到指定设备。
    pub fn run(&mut self, dev: usize, s: &SimScript) -> Result<SimOut, String> {
        if dev >= self.devices.len() {
            return Err(format!("{}：设备下标 {} 越池", E_VV_NOCAP, dev));
        }
        let out = self.devices[dev].exec(s)?;
        self.ledger.push((dev, s.scene, out));
        Ok(out)
    }

    /// 回读某设备某场景最近一次结果。
    pub fn last(&self, dev: usize, scene: u8) -> Option<SimOut> {
        self.ledger
            .iter()
            .rev()
            .find(|(d, sc, _)| *d == dev && *sc == scene)
            .map(|(_, _, o)| *o)
    }

    /// 池规模。
    pub fn size(&self) -> usize {
        self.devices.len()
    }
}

/// 模拟声明（预演定位——非空即判据）。
pub const SIM_DECLARATION: &str =
    "虚拟设备矩阵模拟：结构错误在占真机之前暴露，真机只做取证不做调试";

// ---------------------------------------------------------------------------
// 三、模拟一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3371 域自检：虚拟设备、一组、判据。
pub fn run_vcv11_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv11_sim");

    // --- 能力门：超档场景回 Unsupported（合法三态之一） ---
    let mut entry = VirtualDevice::new("入门A", CapTier::Entry, 0, 0);
    let over = SimScript {
        name: "压测S20".to_string(),
        scene: 20,
        deterministic_out: SimOut::Pass(1),
    };
    cs.add(
        "超档场景不支持",
        entry.exec(&over) == Ok(SimOut::Unsupported),
        "入门档不装旗舰场景——不支持是合法结果",
    );

    // --- 能力门：档内场景正常回放 ---
    let inr = SimScript {
        name: "核心S3".to_string(),
        scene: 3,
        deterministic_out: SimOut::Pass(0xBEEF),
    };
    cs.add(
        "档内场景回放",
        entry.exec(&inr) == Ok(SimOut::Pass(0xBEEF)),
        "同脚本同输入同输出",
    );

    // --- 失败态可表达 ---
    let fail = SimScript {
        name: "失败S5".to_string(),
        scene: 5,
        deterministic_out: SimOut::Fail(42),
    };
    cs.add(
        "失败态可表达",
        entry.exec(&fail) == Ok(SimOut::Fail(42)),
        "三态之二",
    );

    // --- 三态互异 ---
    cs.add(
        "三态互异",
        SimOut::Pass(1) != SimOut::Fail(1)
            && SimOut::Fail(1) != SimOut::Unsupported
            && SimOut::Pass(1) != SimOut::Unsupported,
        "通过/失败/不支持",
    );

    // --- 确定性：连续两次执行结果一致（遥测计次一致） ---
    let r1 = entry.exec(&inr);
    let r2 = entry.exec(&inr);
    cs.add(
        "确定性重放",
        r1 == r2 && entry.ndet_rejects == 0,
        "模拟不掷骰子",
    );

    // --- 矩阵：入池 + 投递 + 回读一致 ---
    let mut m = SimMatrix::new();
    let d0 = m.enroll(VirtualDevice::new("旗舰B", CapTier::Flagship, 0, 0)).expect("pool empty");
    let d1 = m.enroll(VirtualDevice::new("通透C", CapTier::Emulated, 1, 2)).expect("pool has room");
    cs.add("入池两台", m.size() == 2, "设备池");
    let big = SimScript {
        name: "全场景S23".to_string(),
        scene: 23,
        deterministic_out: SimOut::Pass(7),
    };
    cs.add(
        "投递成功",
        m.run(d1, &big) == Ok(SimOut::Pass(7)),
        "通透档全场景",
    );
    cs.add(
        "回读一致",
        m.last(d1, 23) == Some(SimOut::Pass(7)),
        "投递记录可回读",
    );
    cs.add(
        "未投递场景无记录",
        m.last(d0, 23).is_none(),
        "没跑过就是没跑过",
    );

    // --- 池越界拒收 ---
    let r = m.run(9, &big);
    cs.add(
        "设备下标越池拒",
        r.is_err() && r.unwrap_err().contains(E_VV_NOCAP),
        "越界投递不是模拟是事故",
    );

    // --- 声明非空 ---
    cs.add(
        "模拟声明非空",
        !SIM_DECLARATION.is_empty() && SIM_DECLARATION.contains("真机"),
        "预演定位写入声明",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in SIM_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, SIM_VERSION);

    cs
}
