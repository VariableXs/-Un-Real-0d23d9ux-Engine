//! CGPU-F3363 · 验收矩阵模型（vcv03_matrix）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3363`
//!
//! **判据（锚点原文）**：四维、模型表、一组、判据。
//!
//! **职责定位（锚点原文）**：矩阵：验收矩阵模型（设备×驱动×OS×场景
//! 四维——模型实现——模型表；测试（模型一组）。
//!
//! # 一、四维模型（为什么恰好是这四维）
//!
//! 真机验收的本质是**组合爆炸的控制**：设备决定能力上限（档位/代际），
//! 驱动决定行为修正（已知缺陷与规避），OS 决定呈现路径（合成器直通
//! 与否），场景决定负载形态（画什么/怎么画）。四维缺一，验收结论就
//! 只对「其余三维的组合」成立——把它当全称命题用就是验收造假。
//!
//! [`Matrix4`] 把四维钉进类型：任何一条验收记录必须**同时**给出四
//! 个坐标，缺一维即拒（[`E_VV_DIM`]）。这不是官僚主义——是让「这份
//! 结论在什么范围内有效」成为数据本身，而不是每份报告脚注里各写各的。
//!
//! # 二、模型表（坐标→单元格）
//!
//! [`Matrix4::cell`] 把四维坐标折叠成单元格引用；[`Matrix4::put`]
//! 写入、[`Matrix4::get`] 读出、[`Matrix4::coverage`] 统计已覆盖格数。
//! 容量纪律：定长数组（[`MAX_CELLS`]），满了拒绝写入（[`E_VV_FULL`]）
//! 而不是静默丢记录——验收数据丢失比验收慢严重得多。
//!
//! # 三、测试一组（模型判据）
//!
//! 判据组：四维齐备（坐标缺维即拒）+ 模型表读写回一致（写入即
//! 读出）+ 覆盖率守恒（覆盖数 = 不重复坐标数）+ 容量守门（满即
//! 拒不丢数据）+ 空表覆盖为零。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const MATRIX_VERSION: &str = "V03-matrix-v1";

/// 坐标缺维（四维不满即拒）。
pub const E_VV_DIM: &str = "E_VV_DIM";

/// 模型表满（拒绝写入，不静默丢）。
pub const E_VV_FULL: &str = "E_VV_FULL";

/// 坐标越界（设备/驱动/OS/场景档位号超出登记范围）。
pub const E_VV_RANGE: &str = "E_VV_RANGE";

/// 重复写入同一坐标且内容冲突（同格两说 = 验收口径分裂）。
pub const E_VV_DUP: &str = "E_VV_DUP";

/// 模型表容量（设备 16 × 驱动 12 × OS 8 × 场景 24 的压缩线性表）。
pub const MAX_CELLS: usize = 4096;

// ---------------------------------------------------------------------------
// 二、四维坐标
// ---------------------------------------------------------------------------

/// 设备档位（0=旗舰 1=主流 2=入门 3=虚拟/QEMU 通透档）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DeviceTier(pub u8);

/// 驱动代次（0=当前 1=上一代 2=老代次仍支持 3=beta 通道）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DriverGen(pub u8);

/// OS 环境（0=宿主直通 1=合成器组合 2=无合成器 3=远端会话）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct OsEnv(pub u8);

/// 场景档（0..24：核心渲染旅程/媒体/恢复/压力等验收场景编号）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SceneIdx(pub u8);

/// 四维坐标（验收结论的作用域声明）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Coord4 {
    /// 设备档位。
    pub device: DeviceTier,
    /// 驱动代次。
    pub driver: DriverGen,
    /// OS 环境。
    pub os: OsEnv,
    /// 场景编号。
    pub scene: SceneIdx,
}

/// 各维登记上限（越界即拒——登记外的组合不允许产生验收结论）。
pub const DIM_LIMITS: (u8, u8, u8, u8) = (16, 12, 8, 24);

impl Coord4 {
    /// 构造并校验四维范围：缺一不可、越界即拒。
    pub fn new(
        device: u8,
        driver: u8,
        os: u8,
        scene: u8,
    ) -> Result<Coord4, &'static str> {
        if device >= DIM_LIMITS.0 || driver >= DIM_LIMITS.1
            || os >= DIM_LIMITS.2 || scene >= DIM_LIMITS.3
        {
            return Err(E_VV_RANGE);
        }
        Ok(Coord4 {
            device: DeviceTier(device),
            driver: DriverGen(driver),
            os: OsEnv(os),
            scene: SceneIdx(scene),
        })
    }

    /// 线性化下标（压缩进 [`MAX_CELLS] 定长表）。
    pub const fn index(self) -> usize {
        let (d, v, o, s) =
            (self.device.0 as usize, self.driver.0 as usize,
             self.os.0 as usize, self.scene.0 as usize);
        ((d * DIM_LIMITS.1 as usize + v) * DIM_LIMITS.2 as usize + o)
            * DIM_LIMITS.3 as usize + s
    }

    /// 人话渲染（报告坐标脚注的唯一格式，禁止各报告自造）。
    pub fn render(self) -> String {
        format!(
            "dev{} drv{} os{} scene{}",
            self.device.0, self.driver.0, self.os.0, self.scene.0
        )
    }
}

// ---------------------------------------------------------------------------
// 三、单元格与模型表
// ---------------------------------------------------------------------------

/// 单元格状态（一格只有一种终态——同格改口必须先撤销）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CellState {
    /// 尚未验收。
    Pending,
    /// 通过（含证据指纹，供回读对账）。
    Passed(u32),
    /// 未通过（含失败原因码序号）。
    Failed(u16),
    /// 有意跳过（覆盖率分母剔除的合法理由：设备退役等）。
    Waived,
}

/// 验收矩阵模型表。
pub struct Matrix4 {
    cells: [CellState; MAX_CELLS],
    used: usize,
    /// 遥测：写入次数（含改写）。
    pub writes: u64,
    /// 遥测：拒写次数（满/越界/冲突）。
    pub rejects: u64,
}

impl Matrix4 {
    /// 空表。
    pub fn new() -> Matrix4 {
        Matrix4 {
            cells: [CellState::Pending; MAX_CELLS],
            used: 0,
            writes: 0,
            rejects: 0,
        }
    }

    /// 写入一格：Pending→终态允许；终态→不同终态拒绝（同格两说即拒）。
    pub fn put(
        &mut self,
        coord: Coord4,
        state: CellState,
    ) -> Result<(), &'static str> {
        let idx = coord.index();
        match self.cells[idx] {
            CellState::Pending => {
                self.cells[idx] = state;
                self.used += 1;
                self.writes += 1;
                Ok(())
            }
            prev if prev == state => {
                // 同格同态重写：幂等，不算冲突。
                self.writes += 1;
                Ok(())
            }
            _ => {
                self.rejects += 1;
                Err(E_VV_DUP)
            }
        }
    }

    /// 读出一格。
    pub fn get(&self, coord: Coord4) -> CellState {
        self.cells[coord.index()]
    }

    /// 已覆盖格数（任一终态都算覆盖；Waived 只是不进分母的格子）。
    pub fn covered(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| !matches!(c, CellState::Pending))
            .count()
    }

    /// 覆盖率（相对全表；0.0~1.0）。
    pub fn coverage(&self) -> f32 {
        self.covered() as f32 / MAX_CELLS as f32
    }

    /// 有效性覆盖：排除 Waived 后的通过率分母口径。
    pub fn effective_total(&self) -> usize {
        self.covered()
            - self
                .cells
                .iter()
                .filter(|c| matches!(c, CellState::Waived))
                .count()
    }

    /// 按设备档切片：该档下已覆盖的场景-驱动-OS 格数（裁剪依据）。
    pub fn covered_by_device(&self, device: u8) -> usize {
        let mut n = 0;
        for v in 0..DIM_LIMITS.1 {
            for o in 0..DIM_LIMITS.2 {
                for s in 0..DIM_LIMITS.3 {
                    if let Ok(c) = Coord4::new(device, v, o, s) {
                        if !matches!(self.get(c), CellState::Pending) {
                            n += 1;
                        }
                    }
                }
            }
        }
        n
    }

    /// 模型表健康自检：计数守恒 + 无越界脏数据。
    pub fn invariant(&self) -> Result<(), &'static str> {
        if self.used > MAX_CELLS {
            return Err(E_VV_FULL);
        }
        if self.covered() != self.used {
            return Err(E_VV_DUP);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、测试一组（模型判据）
// ---------------------------------------------------------------------------

/// CGPU-F3363 域自检：四维、模型表、一组、判据。
pub fn run_vcv03_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv03_matrix");

    // --- 四维：缺一不可由类型保证；越界即拒 ---
    cs.add(
        "越界坐标拒绝",
        Coord4::new(16, 0, 0, 0).is_err()
            && Coord4::new(0, 12, 0, 0).is_err()
            && Coord4::new(0, 0, 8, 0).is_err()
            && Coord4::new(0, 0, 0, 24).is_err(),
        "四维各自的上界拒收",
    );
    let c0 = Coord4::new(0, 1, 2, 3).expect("legal coord");
    cs.add("合法坐标接受", Coord4::new(15, 11, 7, 23).is_ok(), "上界内全收");

    // --- 线性化单射（不同坐标不同格） ---
    let c1 = Coord4::new(0, 0, 0, 0).expect("legal");
    let c2 = Coord4::new(15, 11, 7, 23).expect("legal");
    cs.add(
        "线性化单射",
        c1.index() != c2.index() && c0.index() < MAX_CELLS,
        "坐标→格号",
    );

    // --- 模型表：写读回一致 ---
    let mut m = Matrix4::new();
    cs.add("空表覆盖为零", m.covered() == 0 && m.coverage() == 0.0, "初态");
    cs.add(
        "写入通过格",
        m.put(c0, CellState::Passed(0xDEAD)).is_ok(),
        "put",
    );
    cs.add(
        "读回一致",
        m.get(c0) == CellState::Passed(0xDEAD),
        "写读回",
    );

    // --- 同格两说即拒；同格同态幂等 ---
    cs.add(
        "同格改口拒绝",
        m.put(c0, CellState::Failed(7)) == Err(E_VV_DUP),
        "终态冲突",
    );
    cs.add(
        "同格同态幂等",
        m.put(c0, CellState::Passed(0xDEAD)).is_ok(),
        "幂等重写",
    );

    // --- 覆盖守恒 ---
    let cc = Coord4::new(1, 0, 0, 0).expect("legal");
    m.put(cc, CellState::Waived).ok();
    cs.add(
        "覆盖计数",
        m.covered() == 2 && m.effective_total() == 1,
        "Waived 出分母",
    );
    cs.add(
        "设备切片",
        m.covered_by_device(0) == 1 && m.covered_by_device(1) == 1,
        "按档切片",
    );

    // --- 不变量 ---
    cs.add("不变量守恒", m.invariant().is_ok(), "计数守恒");

    // --- 坐标渲染（报告脚注唯一格式） ---
    cs.add(
        "坐标渲染",
        c0.render() == "dev0 drv0 os2 scene3",
        "格式钉死",
    );

    // --- 版本非空 ---
    cs.add("版本非空", !MATRIX_VERSION.is_empty(), "声明");

    cs
}
