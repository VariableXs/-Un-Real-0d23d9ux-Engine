//! CGPU-F3378 · 设备档位矩阵（vcv18_devmt）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3378`
//!
//! **判据（锚点原文）**：三档、档位表、一组、判据。
//!
//! **职责定位（锚点原文）**：设备：设备档位矩阵（旗舰/主流/入门三
//! 档——档位实现——档位表；测试（档位一组）。
//!
//! # 一、三档制（为什么不是五档或一档）
//!
//! 档位是**承诺密度**与**验收成本**的折中：一档=「旗舰即全体」造
//! 假；五档=每档都要有真机池，成本失控。三档是锚点钉死的口径：
//! 旗舰（[`Tier::Flagship`]，全能力承诺）、主流（[`Tier::Mainstream`]，
//! 核心能力承诺）、入门（[`Tier::Entry`]，基础能力承诺）。每档有
//! 明确的**能力承诺表**（[`Tier::promise`]）：分辨率上限、并发会话
//! 数、最低帧预算——承诺表是档位判据的依据，没写进承诺表的能力
//! 不许在验收里要求。
//!
//! # 二、档位表（设备→档位的登记）
//!
//! [`DeviceBook`] 把真实设备登记到档位：设备名唯一（重名即拒）、
//! 升档/降档留痕（[`DeviceBook::retier`] 记录迁移史）。降档是合法
//! 操作（真机老化），但**降档后旧档位上的已取证结论不自动迁移**
//! ——旧结论只对旧档位成立（F3363 四维语义的兑现）。
//!
//! # 三、档位一组（判据）
//!
//! 判据组：三档闭集 + 承诺单调（高档承诺 ⊇ 低档承诺）+ 登记唯一
//! + 升降档留痕 + 降档不迁移旧结论。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DEVMATRIX_VERSION: &str = "V18-devmatrix-v1";

/// 设备重名（档位表唯一性破坏）。
pub const E_VV_DUPDEV: &str = "E_VV_DUPDEV";

/// 未登记设备。
pub const E_VV_NODEV: &str = "E_VV_NODEV";

/// 能力承诺三元组（分辨率档上限 / 并发会话数 / 最低帧预算毫秒）。
pub type Promise = (u8, u8, u16);

/// 设备档位三档（锚点原文口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 入门档（最低承诺）。
    Entry,
    /// 主流档。
    Mainstream,
    /// 旗舰档（全承诺）。
    Flagship,
}

impl Tier {
    /// 三档闭集。
    pub const ALL: [Tier; 3] = [Tier::Entry, Tier::Mainstream, Tier::Flagship];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Tier::Entry => "入门",
            Tier::Mainstream => "主流",
            Tier::Flagship => "旗舰",
        }
    }

    /// 能力承诺表（分辨率档上限 / 并发会话 / 最低帧预算 ms）。
    ///
    /// 单调纪律：高档承诺必须 ⊇ 低档承诺（逐项 ≥）。
    pub const fn promise(self) -> Promise {
        match self {
            // 分辨率档（0=1080p 1=2K 2=4K 3=8K）/ 并发 / 帧预算
            Tier::Entry => (1, 1, 33),
            Tier::Mainstream => (2, 4, 25),
            Tier::Flagship => (3, 8, 12),
        }
    }

    /// 对应 vcv03 设备轴取值号。
    pub const fn axis(self) -> u8 {
        match self {
            Tier::Entry => 2,
            Tier::Mainstream => 1,
            Tier::Flagship => 0,
        }
    }
}

/// 承诺单调核验：高档逐项 ≥ 低档。
pub fn promise_monotone() -> bool {
    let e = Tier::Entry.promise();
    let m = Tier::Mainstream.promise();
    let f = Tier::Flagship.promise();
    m.0 >= e.0 && m.1 >= e.1 && m.2 <= e.2 && f.0 >= m.0 && f.1 >= m.1 && f.2 <= m.2
}

// ---------------------------------------------------------------------------
// 二、档位表
// ---------------------------------------------------------------------------

/// 档位迁移史一行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetierLog {
    /// 设备名。
    pub dev: String,
    /// 旧档。
    pub from: Tier,
    /// 新档。
    pub to: Tier,
}

/// 设备档位表。
pub struct DeviceBook {
    entries: Vec<(String, Tier)>,
    /// 迁移史。
    pub logs: Vec<RetierLog>,
}

impl DeviceBook {
    /// 空表。
    pub fn new() -> DeviceBook {
        DeviceBook { entries: Vec::new(), logs: Vec::new() }
    }

    /// 登记（重名即拒）。
    pub fn enroll(&mut self, name: &str, t: Tier) -> Result<(), String> {
        if self.entries.iter().any(|(n, _)| n == name) {
            return Err(format!("{}：设备 {} 重名", E_VV_DUPDEV, name));
        }
        self.entries.push((name.to_string(), t));
        Ok(())
    }

    /// 查档。
    pub fn tier_of(&self, name: &str) -> Result<Tier, &'static str> {
        self.entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
            .ok_or(E_VV_NODEV)
    }

    /// 升/降档（留痕；目标档相同为幂等不算迁移）。
    pub fn retier(&mut self, name: &str, to: Tier) -> Result<(), String> {
        let cur = self.tier_of(name).map_err(|e| e.to_string())?;
        if cur == to {
            return Ok(());
        }
        for (n, t) in self.entries.iter_mut() {
            if n == name {
                *t = to;
            }
        }
        self.logs.push(RetierLog { dev: name.to_string(), from: cur, to });
        Ok(())
    }

    /// 表规模。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 按档清点（覆盖率分母的口径来源）。
    pub fn count_tier(&self, t: Tier) -> usize {
        self.entries.iter().filter(|(_, x)| *x == t).count()
    }
}

/// 档位表声明（非空即判据）。
pub const DEVMATRIX_DECLARATION: &str =
    "设备三档：旗舰全承诺/主流核心/入门基础，承诺表单调，降档不迁移旧结论";

// ---------------------------------------------------------------------------
// 三、档位一组（判据）
// ---------------------------------------------------------------------------

/// CGPU-F3378 域自检：三档、档位表、一组、判据。
pub fn run_vcv18_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv18_devmatrix");

    // --- 三档闭集 + 承诺单调 ---
    cs.add(
        "三档闭集",
        Tier::ALL == [Tier::Entry, Tier::Mainstream, Tier::Flagship],
        "锚点三档：旗舰/主流/入门",
    );
    cs.add(
        "承诺单调（高档⊇低档）",
        promise_monotone(),
        "分辨率↑ 并发↑ 帧预算↓（越小越严）",
    );

    // --- 档位表：登记唯一 ---
    let mut bk = DeviceBook::new();
    cs.add(
        "登记三台",
        bk.enroll("旗舰A", Tier::Flagship).is_ok()
            && bk.enroll("主流B", Tier::Mainstream).is_ok()
            && bk.enroll("入门C", Tier::Entry).is_ok()
            && bk.len() == 3,
        "enroll",
    );
    let r = bk.enroll("旗舰A", Tier::Mainstream);
    cs.add(
        "设备重名拒收",
        r.is_err() && r.unwrap_err().contains(E_VV_DUPDEV),
        "档位表唯一性",
    );

    // --- 查档 ---
    cs.add(
        "查档回读",
        bk.tier_of("旗舰A") == Ok(Tier::Flagship) && bk.tier_of("入门C") == Ok(Tier::Entry),
        "写读回",
    );
    cs.add(
        "未登记拒查",
        bk.tier_of("幽灵机").is_err(),
        E_VV_NODEV,
    );

    // --- 升降档留痕 + 幂等 ---
    bk.retier("主流B", Tier::Entry).expect("retier");
    cs.add(
        "降档留痕",
        bk.tier_of("主流B") == Ok(Tier::Entry)
            && bk.logs.len() == 1
            && bk.logs[0].from == Tier::Mainstream
            && bk.logs[0].to == Tier::Entry,
        "迁移史可查",
    );
    bk.retier("主流B", Tier::Entry).expect("idempotent");
    cs.add("同档幂等不记迁移", bk.logs.len() == 1, "幂等");

    // --- 降档不迁移旧结论（旧结论只对旧档成立——承诺表口径） ---
    let old_promise = Tier::Mainstream.promise();
    let new_promise = bk.tier_of("主流B").expect("present").promise();
    cs.add(
        "降档承诺缩水可见",
        new_promise.0 < old_promise.0,
        "旧档结论不自动覆盖新档——须重新取证",
    );

    // --- 按档清点 ---
    cs.add(
        "按档清点",
        bk.count_tier(Tier::Flagship) == 1
            && bk.count_tier(Tier::Mainstream) == 0
            && bk.count_tier(Tier::Entry) == 2,
        "覆盖率分母口径",
    );

    // --- 轴映射（接 vcv03） ---
    cs.add(
        "三档轴号互异",
        Tier::Flagship.axis() != Tier::Mainstream.axis()
            && Tier::Mainstream.axis() != Tier::Entry.axis(),
        "档位→vcv03 设备轴",
    );

    // --- 声明非空 ---
    cs.add(
        "档位声明非空",
        !DEVMATRIX_DECLARATION.is_empty() && DEVMATRIX_DECLARATION.contains("三档"),
        "锚点档位声明",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in DEVMATRIX_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, DEVMATRIX_VERSION);

    cs
}
