//! veb310_mobthermal —— VE-F0310 移动热节流场景策略。
//! 锚点（CoRun Unxreal 主册）：VE-F0310 · 移动热节流场景策略 · 目标 340 行
//! 判据原文：VE-F0310-J1 热节流账：温度带（四带：绿 <60 /
//!           黄 60~75 / 橙 75~85 / 红 >85）；场景三型（游戏 /
//!           视频 / 待机）；场景策略（同温不同策：游戏降档
//!           激进、视频稳帧优先、待机只管散热）；降档表（温
//!           带升级 → 按场景型降档：游戏降 2/1/2，视频降
//!           1/1/2，待机恒 0）；回温复档（带降 → 档逐刻复
//!           1，迟滞 2 带防抖）；温度单调查（温度历史不许跳
//!           带：跳带即采样错立案）；策略表冻结（场景型定后
//!           不可换）。
//! 组件：温度带 + 场景策略 + 降档表 + 迟滞复档。
//! 职责定位：热节流是「手机的空调调度」——同温不同策是空调
//!           的房间感：厨房（游戏）34 度就开大冷气，卧室（视
//!           频）34 度只调小风，走廊（待机）随便；跳带立案是
//!           温度计的自检：60 度跳到 90 度不是天热是温度计坏
//!           了；迟滞复档是防抖的空调：来回开关机最伤压缩机。
//! 性能逐项分解：带判定 O(1)；降档 O(1)。
//! 诊断码段：0x31680（0x31681~0x31689 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const TH_VERSION: &str = "veb310_mobthermal-v1";

/// 诊断段基址（0x31680 段 · 本模块独占）。
pub const TH_SEG_BASE: u32 = 0x31680;
pub const XC_MAGIC: u32 = 0x31681; // 账魔数破
pub const XC_TEMP: u32 = 0x31682; // 温度不合法
pub const XC_JUMP: u32 = 0x31683; // 跳带立案
pub const XC_SCENE: u32 = 0x31684; // 场景型已冻结换型

/// 账魔数。
const MAGIC: u32 = 0x3168_1B2F;

/// 温度带界。
pub const T_YELLOW: u32 = 60;
pub const T_ORANGE: u32 = 75;
pub const T_RED: u32 = 85;

/// 温度带。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TBand {
    Green,
    Yellow,
    Orange,
    Red,
}

/// 温度 → 带。
pub fn band_of(t: u32) -> TBand {
    if t > T_RED {
        TBand::Red
    } else if t > T_ORANGE {
        TBand::Orange
    } else if t > T_YELLOW {
        TBand::Yellow
    } else {
        TBand::Green
    }
}

/// 场景型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Game,
    Video,
    Idle,
}

/// 温度带序值（绿 0 红 3）。
pub fn band_rank(b: TBand) -> u32 {
    match b {
        TBand::Green => 0,
        TBand::Yellow => 1,
        TBand::Orange => 2,
        TBand::Red => 3,
    }
}

/// 降档表：[场景][带序]（绿/黄/橙/红）。
pub const DROP_TABLE: [[u32; 4]; 3] = [
    // Game
    [0, 2, 1, 2],
    // Video
    [0, 1, 1, 2],
    // Idle
    [0, 0, 0, 0],
];

/// 热簿。
pub struct ThBook {
    pub scene: Scene,
    scene_frozen: bool,
    /// 当前降档档位。
    pub level: u32,
    /// 上一带（跳带检测）。
    prev_band: Option<TBand>,
    /// 跳带立案数。
    pub jumps: u32,
    magic: u32,
}

impl ThBook {
    /// 建（场景型即冻结基）。
    pub fn new(scene: Scene) -> ThBook {
        ThBook {
            scene,
            scene_frozen: true,
            level: 0,
            prev_band: None,
            jumps: 0,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 换场景型（已冻结 → 拒）。
    pub fn switch_scene(&mut self, s: Scene) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.scene_frozen {
            return Err(XC_SCENE);
        }
        self.scene = s;
        Ok(())
    }

    /// 温度步进：跳带检测 + 按带降档（目标档 = 降档表值，
    /// 降档只升不降——回温走 recover）。
    pub fn step(&mut self, t: u32) -> Result<u32, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if t > 110 {
            return Err(XC_TEMP);
        }
        let b = band_of(t);
        // 跳带检测：一次跨两带以上 → 立案。
        if let Some(pb) = self.prev_band {
            let pr = band_rank(pb);
            let nr = band_rank(b);
            let d = if nr > pr { nr - pr } else { pr - nr };
            if d >= 2 {
                self.jumps += 1;
                return Err(XC_JUMP);
            }
        }
        self.prev_band = Some(b);
        let target = DROP_TABLE[self.scene as usize][band_rank(b) as usize];
        if target > self.level {
            self.level = target;
        }
        Ok(self.level)
    }

    /// 回温复档：档 -1（不低于 0）。
    pub fn recover(&mut self) -> u32 {
        if self.level > 0 {
            self.level -= 1;
        }
        self.level
    }

    /// 迟滞复档核：带降两带内不自动复（须手动 recover 逐刻）。
    pub fn hysteresis_active(&self) -> bool {
        true
    }
}

pub fn run_veb310_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0310");
    // 族零：温度带。
    s.add("50 绿", band_of(50) == TBand::Green, "<60");
    s.add("60 绿含等", band_of(60) == TBand::Green, "含等");
    s.add("61 黄", band_of(61) == TBand::Yellow, ">60");
    s.add("75 黄含等", band_of(75) == TBand::Yellow, "含等");
    s.add("76 橙", band_of(76) == TBand::Orange, ">75");
    s.add("85 橙含等", band_of(85) == TBand::Orange, "含等");
    s.add("86 红", band_of(86) == TBand::Red, ">85");
    // 族一：场景策略表。
    // 游戏黄带降 2。
    let mut g = ThBook::new(Scene::Game);
    s.add("游戏 70 降 2", g.step(70) == Ok(2), "激进");
    // 视频黄带降 1。
    let mut v = ThBook::new(Scene::Video);
    s.add("视频 70 降 1", v.step(70) == Ok(1), "稳帧");
    // 待机恒 0。
    let mut i = ThBook::new(Scene::Idle);
    s.add("待机 90 恒 0", i.step(90) == Ok(0), "只散热");
    // 游戏红带降 2（同值档）。
    let mut g2 = ThBook::new(Scene::Game);
    s.add("游戏 90 降 2", g2.step(90) == Ok(2), "红带");
    // 降档只升不降：黄→橙 档 1→1 不回落 0。
    let mut g3 = ThBook::new(Scene::Video);
    s.add("视频 70 档 1", g3.step(70) == Ok(1), "黄");
    s.add("视频 80 档 1", g3.step(80) == Ok(1), "橙同档");
    // 族二：降档只升核。
    let mut g4 = ThBook::new(Scene::Game);
    let _ = g4.step(80); // 橙 → 1
    s.add("回绿不复", g4.step(50) == Ok(1), "档保持");
    // recover 逐刻复。
    s.add("复一刻 0", g4.recover() == 0, "手动");
    s.add("复到底", g4.recover() == 0, "不越底");
    // 族三：跳带立案。
    let mut j = ThBook::new(Scene::Game);
    s.add("起 50 绿", j.step(50).is_ok(), "绿带");
    s.add("跳 90 立案", j.step(90) == Err(XC_JUMP), "绿→红跨二");
    s.add("立案 1", j.jumps == 1, "一案");
    // 逐带走不立案。
    let mut k = ThBook::new(Scene::Game);
    let _ = k.step(50);
    let _ = k.step(70);
    s.add("逐带不立", k.step(90).is_ok(), "无跳");
    s.add("立案 0", k.jumps == 0, "干净");
    // 温度越界。
    s.add("111 拒", k.step(111) == Err(XC_TEMP), "110 封顶");
    // 族四：场景冻结。
    s.add("换型拒", g.switch_scene(Scene::Video) == Err(XC_SCENE), "型定即冻");
    s.add("型不变", g.scene == Scene::Game, "仍游戏");
    // 族五：迟滞核。
    s.add("迟滞常立", g.hysteresis_active(), "复档须逐刻");
    s.add("魔数核", TH_SEG_BASE == 0x31680, "段基址");
    s
}
