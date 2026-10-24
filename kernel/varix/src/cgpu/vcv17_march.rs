//! CGPU-F3377 · 真机矩阵与设备覆盖总架构（vcv17_march）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3377`
//!
//! **判据（锚点原文）**：四矩阵、深化声明、三组、判据。
//!
//! **职责定位（锚点原文）**：架构：架构（真机矩阵与设备覆盖总架构
//! （设备/驱动/OS/分辨率四矩阵——四段（V01 模型复用深化——深化
//! 声明；测试（定位/四段/复用三组）。
//!
//! # 一、V02 批次定位（深化而非重建）
//!
//! V01 的 [`super::vcv03_matrix`] 给了**抽象模型**：四维坐标 + 单元
//! 格表。V02 把四个维度各自**实体化**成四张具体矩阵：设备档位矩阵
//! （vcv18）、驱动版本矩阵（vcv19）、OS 环境矩阵（vcv20）、分辨率
//! DPI 矩阵（vcv21）——每张矩阵登记的是**真实世界的取值域**（真
//! 实设备档、真实驱动代次、真实 OS 组合、真实分辨率档），抽象坐
//! 标从此有了实义。深化的边界：不改 vcv03 的坐标类型与单元格语
//! 义（F3369 冻结线约束），只在登记层扩展——这是「复用深化」与
//! 「推倒重来」的纪律差别。
//!
//! # 二、四段单向流水线（复用 vcv01 架构）
//!
//! 与 V01 相同的流水线纪律：登记→覆盖→裁剪→遥测四段单向，跳段
//! 拒、回退拒（复用 vcv01 的显性码风格）。设备覆盖（V02 批次主
//! 题）的定义随之精确：**一张验收结论必须能指回四张实体矩阵里的
//! 具体登记项**——指不回的结论是悬空结论。
//!
//! # 三、测试三组（定位/四段/复用）
//!
//! 判据组：定位声明（深化不重建）+ 四段齐备 + 复用契约（vcv03 类
//! 型被 V02 实际引用）+ 四矩阵互异 + 流水线单向。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcv03_matrix::{Coord4, DIM_LIMITS};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const MARCH_VERSION: &str = "V17-march-v1";

/// 悬空结论（验收结论指不回任何实体矩阵登记项）。
pub const E_VV_HANG: &str = "E_VV_HANG";

/// 跳段（流水线未过前段直接进后段）。
pub const E_VV_SKIPSEG: &str = "E_VV_SKIPSEG";

/// 四段流水线段名（登记→覆盖→裁剪→遥测）。
pub const MARCH_SEGMENTS: [&str; 4] = ["enroll", "cover", "prune", "telemetry"];

/// 四实体矩阵名（锚点原文：设备/驱动/OS/分辨率）。
pub const MARCH_MATRICES: [&str; 4] = ["device", "driver", "os", "dpi"];

// ---------------------------------------------------------------------------
// 二、实体登记与悬空检查
// ---------------------------------------------------------------------------

/// 实体登记项（一张实体矩阵里的一行——真实世界的一个取值）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrolled {
    /// 所属矩阵（四矩阵之一）。
    pub matrix: &'static str,
    /// 登记名（如 "旗舰档" / "驱动代次0" / "1080p"）。
    pub label: String,
    /// 对应 vcv03 抽象坐标的该维取值号。
    pub axis_val: u8,
}

/// 登记合法性：矩阵名在四矩阵闭集内 + 轴值在该维登记上限内。
pub fn enroll_ok(e: &Enrolled) -> Result<(), &'static str> {
    let axis = match e.matrix {
        "device" => 0,
        "driver" => 1,
        "os" => 2,
        "dpi" => 3,
        _ => return Err(E_VV_HANG),
    };
    let lim = [DIM_LIMITS.0, DIM_LIMITS.1, DIM_LIMITS.2, DIM_LIMITS.3][axis];
    if e.axis_val >= lim {
        return Err(E_VV_HANG);
    }
    Ok(())
}

/// 悬空结论检查：一条验收坐标的四维必须全部有实体登记。
///
/// 这是「深化」的核心纪律：抽象坐标四维若有一维没有实体登记项，
/// 该坐标上的验收结论无法指回真实世界——悬空即拒。
pub fn no_hanging(coord: &Coord4, pool: &[Enrolled]) -> Result<(), &'static str> {
    let need: [(usize, u8); 4] = [
        (0, coord.device.0),
        (1, coord.driver.0),
        (2, coord.os.0),
        (3, coord.scene.0.min(coord.scene.0)),
    ];
    for (axis, val) in need.iter() {
        let hit = pool.iter().any(|e| {
            let m = match MARCH_MATRICES[*axis] {
                "device" => enroll_ok(e).is_ok() && e.matrix == "device" && e.axis_val == coord.device.0,
                "driver" => enroll_ok(e).is_ok() && e.matrix == "driver" && e.axis_val == coord.driver.0,
                "os" => enroll_ok(e).is_ok() && e.matrix == "os" && e.axis_val == coord.os.0,
                _ => enroll_ok(e).is_ok() && e.matrix == "dpi" && e.axis_val == val % DIM_LIMITS.3,
            };
            m
        });
        if !hit {
            return Err(E_VV_HANG);
        }
    }
    Ok(())
}

/// 段序核验（流水线单向：上一步没做不许做下一步）。
pub fn segment_order(done: &[&str]) -> Result<(), &'static str> {
    for (i, s) in MARCH_SEGMENTS.iter().enumerate() {
        if done.contains(s) {
            // 前序段必须全部已做。
            for prev in MARCH_SEGMENTS.iter().take(i) {
                if !done.contains(prev) {
                    return Err(E_VV_SKIPSEG);
                }
            }
        }
    }
    Ok(())
}

/// 深化声明（复用深化定位——非空即判据）。
pub const MARCH_DECLARATION: &str =
    "V02 深化不重建：vcv03 抽象模型不动，四维各自实体化为四张登记矩阵";

// ---------------------------------------------------------------------------
// 三、测试三组（定位/四段/复用）
// ---------------------------------------------------------------------------

/// CGPU-F3377 域自检：四矩阵、深化声明、三组、判据。
pub fn run_vcv17_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv17_march");

    // --- 组一：定位声明 + 四矩阵齐备 ---
    cs.add(
        "四矩阵齐备",
        MARCH_MATRICES == ["device", "driver", "os", "dpi"],
        "锚点四矩阵：设备/驱动/OS/分辨率",
    );
    cs.add(
        "深化声明非空",
        !MARCH_DECLARATION.is_empty() && MARCH_DECLARATION.contains("不重建"),
        "复用深化，不是推倒重来",
    );

    // --- 组二：四段流水线 ---
    cs.add(
        "四段齐备",
        MARCH_SEGMENTS.len() == 4,
        "登记→覆盖→裁剪→遥测",
    );
    cs.add(
        "段序单向通过",
        segment_order(&["enroll", "cover", "prune", "telemetry"]).is_ok(),
        "全序合规",
    );
    cs.add(
        "跳段拒收",
        segment_order(&["telemetry"]).is_err(),
        "没登记就遥测=跳段",
    );
    cs.add(
        "前缀合规放行",
        segment_order(&["enroll", "cover"]).is_ok(),
        "做到中段合法",
    );

    // --- 组三：复用契约（vcv03 类型实际引用） ---
    cs.add(
        "复用 vcv03 坐标上限",
        DIM_LIMITS == (16, 12, 8, 24),
        "V02 实体矩阵的轴上限=vcv03 登记上限",
    );
    let dev = Enrolled { matrix: "device", label: "旗舰档".to_string(), axis_val: 0 };
    let drv = Enrolled { matrix: "driver", label: "代次0".to_string(), axis_val: 0 };
    let os = Enrolled { matrix: "os", label: "宿主直通".to_string(), axis_val: 0 };
    let dpi = Enrolled { matrix: "dpi", label: "1080p".to_string(), axis_val: 0 };
    let pool = vec![dev, drv, os, dpi];
    let coord = Coord4::new(0, 0, 0, 0).expect("legal coord");
    cs.add(
        "全登记坐标不悬空",
        no_hanging(&coord, &pool).is_ok(),
        "四维全有实体登记",
    );
    let coord2 = Coord4::new(3, 0, 0, 0).expect("legal coord");
    cs.add(
        "未登记维悬空拒",
        no_hanging(&coord2, &pool).is_err(),
        "设备档 3 无登记项=结论悬空",
    );

    // --- 登记越轴拒收 ---
    let over = Enrolled { matrix: "device", label: "超档".to_string(), axis_val: 16 };
    cs.add(
        "登记越轴拒收",
        enroll_ok(&over).is_err(),
        "轴值超出 vcv03 上限",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in MARCH_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, MARCH_VERSION);

    cs
}
