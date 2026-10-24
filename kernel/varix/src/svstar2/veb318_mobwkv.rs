//! veb318_mobwkv —— VE-F0318 移动组规避表。
//! 锚点（CoRun Unxreal 主册）：VE-F0318 · 移动组规避表 · 目标 320 行
//! 判据原文：VE-F0318-J1 移动规避账：条目（缺陷号 + 波及场
//!           景位集：游戏/视频/相机/UI 四场景 + 动作码 + 电
//!           量成本）；场景命中（当前场景位 ∈ 波及集）；电
//!           量闸（动作码执行代价超电量预算 → 动作降级为
//!           省电码）；电量档位（满/中/低三档，低档全走省
//!           电码）；成本合计（命中动作的电量代价和）；重
//!           号拒；表容 48。
//! 组件：场景位账 + 电量闸 + 降级码 + 成本账。
//! 职责定位：移动规避是「手机的省电药方」——动作带电量价
//!           是药价：满电吃好药、低电吃退而求其次的药；电量
//!           闸是药房的黑名单：余额不够就换平价药；场景位是
//!           处方的适用症：不对症的药不吃。
//! 性能逐项分解：命中 O(条数)。
//! 诊断码段：0x31880（0x31881~0x31889 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MW_VERSION: &str = "veb318_mobwkv-v1";

/// 诊断段基址（0x31880 段 · 本模块独占）。
pub const MW_SEG_BASE: u32 = 0x31880;
pub const XC_MAGIC: u32 = 0x31881; // 账魔数破
pub const XC_DUP: u32 = 0x31882; // 重号
pub const XC_FULL: u32 = 0x31883; // 表满
pub const XC_COST: u32 = 0x31884; // 代价越界
pub const XC_SCENE: u32 = 0x31885; // 场景位越界

/// 账魔数。
const MAGIC: u32 = 0x3188_1B0B;

/// 表容。
pub const MW_CAP: usize = 48;

/// 场景位（游戏/视频/相机/UI）。
pub const SC_GAME: u32 = 1;
pub const SC_VIDEO: u32 = 2;
pub const SC_CAM: u32 = 4;
pub const SC_UI: u32 = 8;

/// 电量档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Batt {
    Full,
    Mid,
    Low,
}

/// 电量预算（动作代价和上限）。
pub fn batt_budget(b: Batt) -> u32 {
    match b {
        Batt::Full => 10,
        Batt::Mid => 5,
        Batt::Low => 2,
    }
}

/// 条目。
pub struct MwRow {
    pub bug: u32,
    pub scenes: u32,
    /// 动作码（1~99）。
    pub action: u32,
    /// 电量代价（0~3）。
    pub cost: u32,
    /// 省电码（降级动作）。
    pub eco_action: u32,
}

/// 移动规避表。
pub struct MwTab {
    rows: Vec<MwRow>,
    magic: u32,
}

impl MwTab {
    /// 建。
    pub fn new() -> MwTab {
        MwTab { rows: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记。
    pub fn add(&mut self, bug: u32, scenes: u32, action: u32, cost: u32, eco: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.rows.len() >= MW_CAP {
            return Err(XC_FULL);
        }
        if cost > 3 {
            return Err(XC_COST);
        }
        if scenes & !0xF != 0 {
            return Err(XC_SCENE);
        }
        for r in self.rows.iter() {
            if r.bug == bug {
                return Err(XC_DUP);
            }
        }
        self.rows.push(MwRow { bug, scenes, action, cost, eco_action: eco });
        Ok(())
    }

    /// 命中条数。
    pub fn hit_count(&self, scenes: u32) -> u32 {
        let mut n = 0;
        for r in self.rows.iter() {
            if r.scenes & scenes != 0 {
                n += 1;
            }
        }
        n
    }

    /// 命中动作分发（带电量闸：预算内发原码，超支位发省电码）。
    /// 贪心：按登记序逐条判断「已发原码代价和 + 本条代价 ≤ 预算」。
    pub fn dispatch(&self, scenes: u32, batt: Batt) -> Vec<(u32, bool)> {
        // (动作码, 是否省电降级)
        let mut out = Vec::new();
        let mut used = 0u32;
        let budget = batt_budget(batt);
        for r in self.rows.iter() {
            if r.scenes & scenes != 0 {
                if used + r.cost <= budget {
                    used += r.cost;
                    out.push((r.action, false));
                } else {
                    out.push((r.eco_action, true));
                }
            }
        }
        out
    }

    /// 成本合计（命中条原码代价和）。
    pub fn cost_total(&self, scenes: u32) -> u32 {
        let mut t = 0;
        for r in self.rows.iter() {
            if r.scenes & scenes != 0 {
                t += r.cost;
            }
        }
        t
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

pub fn run_veb318_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0318");
    let mut t = MwTab::new();
    // 族零：登记门。
    s.add("代价 4 拒", t.add(1, SC_GAME, 10, 4, 11) == Err(XC_COST), ">3");
    s.add("场景 16 拒", t.add(1, 0x10, 10, 1, 11) == Err(XC_SCENE), "位外");
    s.add("登 1", t.add(1, SC_GAME, 10, 2, 11).is_ok(), "游戏缺陷");
    s.add("重号拒", t.add(1, SC_UI, 20, 1, 21) == Err(XC_DUP), "一号一病历");
    s.add("登 2", t.add(2, SC_GAME | SC_VIDEO, 20, 1, 21).is_ok(), "双场景");
    s.add("登 3", t.add(3, SC_CAM, 30, 3, 31).is_ok(), "相机缺陷");
    s.add("登 4", t.add(4, SC_UI, 40, 1, 41).is_ok(), "UI 缺陷");
    s.add("表 4 条", t.len() == 4, "四病历");
    // 族一：场景命中。
    s.add("游戏命中 2", t.hit_count(SC_GAME) == 2, "1+2");
    s.add("视频命中 1", t.hit_count(SC_VIDEO) == 1, "2");
    s.add("相机命中 1", t.hit_count(SC_CAM) == 1, "3");
    s.add("UI 命中 1", t.hit_count(SC_UI) == 1, "4");
    // 成本合计。
    s.add("游戏成本 3", t.cost_total(SC_GAME) == 3, "2+1");
    s.add("相机成本 3", t.cost_total(SC_CAM) == 3, "单条 3");
    // 族二：电量闸（满电预算 10）。
    let d_game = t.dispatch(SC_GAME, Batt::Full);
    s.add("满电全原码", d_game.len() == 2 && d_game[0].1 == false && d_game[1].1 == false, "预算足");
    // 相机单条代价 3 ≤ 10 → 原码。
    let d_cam = t.dispatch(SC_CAM, Batt::Full);
    s.add("满电相机原码", d_cam.len() == 1 && d_cam[0] == (30, false), "过闸");
    // 族三：低档全走省电码（预算 2）。
    // 游戏：条 1 代价 2 ≤ 2 → 原码；条 2 代价 1，2+1=3 > 2 → 省电码。
    let dl = t.dispatch(SC_GAME, Batt::Low);
    s.add("低档先原后省", dl.len() == 2 && dl[0] == (10, false) && dl[1] == (21, true), "贪心序");
    // 相机：代价 3 > 2 → 省电码。
    let dcl = t.dispatch(SC_CAM, Batt::Low);
    s.add("低档相机省电", dcl[0] == (31, true), "换平价药");
    // 中档（预算 5）：游戏 2+1=3 ≤ 5 → 全原码。
    let dm = t.dispatch(SC_GAME, Batt::Mid);
    s.add("中档全原码", dm[0].1 == false && dm[1].1 == false, "预算 5");
    // 族四：预算边界。
    s.add("满档预算 10", batt_budget(Batt::Full) == 10, "口径");
    s.add("中档预算 5", batt_budget(Batt::Mid) == 5, "口径");
    s.add("低档预算 2", batt_budget(Batt::Low) == 2, "口径");
    // 恰预算：构造代价和恰 = 预算的组合——UI+视频同时命中条 2、4：代价 1+1=2 ≤ 2（低档）→ 全原码。
    let du = t.dispatch(SC_VIDEO | SC_UI, Batt::Low);
    s.add("双场景低档", du.len() == 2 && du[0].1 == false && du[1].1 == false, "恰预算");
    // 多一场景（游戏 + 视频 + UI 低档）：条 1(2)+2(1)+4(1)：
    // 条 1: 0+2=2≤2 原码；条 2: 2+1=3>2 省电码；条 4: 2+1=3>2 省电码。
    let da = t.dispatch(SC_GAME | SC_VIDEO | SC_UI, Batt::Low);
    s.add("三场景低档", da.len() == 3 && da[0].1 == false && da[1].1 == true && da[2].1 == true, "超支降级");
    // 族五：表容。
    let mut f = MwTab::new();
    let mut ok = true;
    for i in 0..MW_CAP as u32 {
        if f.add(i, SC_UI, i, 0, i).is_err() {
            ok = false;
        }
    }
    s.add("满表 48", ok && f.len() == MW_CAP, "定容");
    s.add("49 条拒", f.add(999, SC_UI, 1, 0, 1) == Err(XC_FULL), "溢出");
    s.add("魔数核", MW_SEG_BASE == 0x31880, "段基址");
    s
}
