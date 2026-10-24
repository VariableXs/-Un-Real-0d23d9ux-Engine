//! veb316_mobvdf —— VE-F0316 移动组版本差异表。
//! 锚点（CoRun Unxreal 主册）：VE-F0316 · 移动组版本差异表 · 目标 320 行
//! 判据原文：VE-F0316-J1 移动版本差异账：三版本轴（Android
//!           API 级 / HAL 版 / 驱动月版）；差异条目（功能 +
//!           三轴门槛）；逐条判定（三轴全过 → 有，任一不过
//!           → 无）；最紧轴核（判定取三轴中最紧的：任一短
//!           板即无）；改名映射（跨门槛行为名变）；三态计数；
//!           重门槛拒（同功能同轴重复登记拒）。
//! 组件：三轴版 + 差异表 + 最紧轴 + 改名映射。
//! 职责定位：移动版本差异是「三把锁的门」——三轴是三把锁：
//!           少开一把门都不开（最紧轴核）；改名映射是换锁后
//!           的新钥匙：老钥匙捅不开新锁；重门槛拒是一锁一
//!           钥：一把锁配两把钥匙迟早出事。
//! 性能逐项分解：判定 O(条数)。
//! 诊断码段：0x31800（0x31801~0x31809 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MV_VERSION: &str = "veb316_mobvdf-v1";

/// 诊断段基址（0x31800 段 · 本模块独占）。
pub const MV_SEG_BASE: u32 = 0x31800;
pub const XC_MAGIC: u32 = 0x31801; // 账魔数破
pub const XC_DUP: u32 = 0x31802; // 重门槛

/// 账魔数。
const MAGIC: u32 = 0x3180_1B3C;

/// 三轴版本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axes {
    pub api: u32,
    pub hal: u32,
    /// 驱动月版（yymm 压缩）。
    pub drv: u32,
}

impl Axes {
    /// 三轴全 ≥ 门。
    pub fn ge(&self, gate: &Axes) -> bool {
        self.api >= gate.api && self.hal >= gate.hal && self.drv >= gate.drv
    }

    /// 任一轴 ≥ 门（用于最紧轴核：找出满足的最小门槛轴）。
    pub fn any_ge(&self, gate: &Axes) -> bool {
        self.api >= gate.api || self.hal >= gate.hal || self.drv >= gate.drv
    }
}

/// 行为标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Have,
    None,
    Renamed,
}

/// 条目：三轴门槛。
pub struct MRow {
    pub feat: u32,
    pub gate: Axes,
    pub mark: Mark,
    pub new_feat: u32,
}

/// 差异表。
pub struct MobDiff {
    rows: Vec<MRow>,
    pub cur: Axes,
    magic: u32,
}

/// 判定。
#[derive(Debug, PartialEq, Eq)]
pub enum MVerdict {
    Have(u32),
    None,
    Renamed(u32),
}

impl MobDiff {
    /// 建。
    pub fn new(cur: Axes) -> MobDiff {
        MobDiff { rows: Vec::new(), cur, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记（同功能重门槛拒）。
    pub fn add(&mut self, feat: u32, gate: Axes, mark: Mark, new_feat: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for r in self.rows.iter() {
            if r.feat == feat {
                return Err(XC_DUP);
            }
        }
        self.rows.push(MRow { feat, gate, mark, new_feat });
        Ok(())
    }

    /// 判定（三轴全过 → 有/改名；任一短板 → 无）。
    pub fn verdict(&self, feat: u32) -> Result<MVerdict, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for r in self.rows.iter() {
            if r.feat == feat {
                return Ok(if self.cur.ge(&r.gate) {
                    match r.mark {
                        Mark::Have => MVerdict::Have(r.feat),
                        Mark::None => MVerdict::None,
                        Mark::Renamed => MVerdict::Renamed(r.new_feat),
                    }
                } else {
                    MVerdict::None
                });
            }
        }
        Ok(MVerdict::None)
    }

    /// 三态计数。
    pub fn census(&self) -> (u32, u32, u32) {
        let (mut h, mut n, mut rn) = (0u32, 0u32, 0u32);
        for r in self.rows.iter() {
            if self.cur.ge(&r.gate) {
                match r.mark {
                    Mark::Have => h += 1,
                    Mark::None => n += 1,
                    Mark::Renamed => rn += 1,
                }
            } else {
                n += 1;
            }
        }
        (h, n, rn)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

pub fn run_veb316_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0316");
    // 族零：三轴比较。
    let cur = Axes { api: 34, hal: 5, drv: 24050 };
    let g1 = Axes { api: 33, hal: 5, drv: 24010 };
    let g2 = Axes { api: 34, hal: 6, drv: 24010 };
    s.add("三轴全过", cur.ge(&g1), "全 ≥");
    s.add("单轴短即拒", cur.ge(&g2) == false, "HAL 5<6");
    s.add("任一轴过", cur.any_ge(&g2), "api/drv 过");
    // 族一：登记与重门槛。
    let mut t = MobDiff::new(cur);
    s.add("登 A", t.add(1, g1, Mark::Have, 0).is_ok(), "功能 1");
    s.add("重登拒", t.add(1, g1, Mark::Have, 0) == Err(XC_DUP), "重门槛");
    let g_high = Axes { api: 35, hal: 5, drv: 24010 };
    s.add("登 B 高槛", t.add(2, g_high, Mark::None, 0).is_ok(), "未达槛");
    s.add("登 C 改名", t.add(3, Axes { api: 30, hal: 4, drv: 24000 }, Mark::Renamed, 33).is_ok(), "跨槛名变");
    s.add("表 3 条", t.len() == 3, "三行");
    // 族二：判定（最紧轴核）。
    s.add("A 有", t.verdict(1) == Ok(MVerdict::Have(1)), "三轴全过");
    s.add("B 无（API 短）", t.verdict(2) == Ok(MVerdict::None), "api 34<35");
    s.add("C 新名", t.verdict(3) == Ok(MVerdict::Renamed(33)), "改名映射");
    s.add("表外无", t.verdict(9) == Ok(MVerdict::None), "未登记");
    // 轴序核：只升 drv 不够（其他轴也须过）。
    let drv_only = MobDiff::new(Axes { api: 33, hal: 5, drv: 24099 });
    s.add("B 仍无", drv_only.verdict(2) == Ok(MVerdict::None), "api 仍短");
    // 只升 api 到 35 → B 有。
    let api_up = MobDiff::new(Axes { api: 35, hal: 5, drv: 24010 });
    s.add("升 api B 有", api_up.verdict(2) == Ok(MVerdict::Have(2)), "补短板");
    // 族三：三态计数。
    let (h, n, r) = t.census();
    s.add("计有 1", h == 1, "A");
    s.add("计无 1", n == 1, "B");
    s.add("计改名 1", r == 1, "C");
    // 升版计数翻转。
    let t2 = MobDiff::new(Axes { api: 36, hal: 7, drv: 24100 });
    // t2 空表——重建同条目。
    let mut t3 = MobDiff::new(Axes { api: 36, hal: 7, drv: 24100 });
    let _ = t3.add(1, g1, Mark::Have, 0);
    let _ = t3.add(2, g_high, Mark::None, 0);
    let _ = t3.add(3, Axes { api: 30, hal: 4, drv: 24000 }, Mark::Renamed, 33);
    let (h3, n3, r3) = t3.census();
    s.add("新版有 2", h3 == 2, "A+B");
    s.add("新版无 0", n3 == 0, "全过");
    s.add("新版改名 1", r3 == 1, "C");
    let _ = t2;
    // 族四：恰槛含等。
    let eq = MobDiff::new(Axes { api: 35, hal: 6, drv: 24099 });
    let mut te = MobDiff::new(Axes { api: 35, hal: 6, drv: 24099 });
    let _ = te.add(4, g2, Mark::Have, 0);
    s.add("恰槛有", te.verdict(4) == Ok(MVerdict::Have(4)), "三轴含等");
    let _ = eq;
    s.add("魔数核", MV_SEG_BASE == 0x31800, "段基址");
    s
}
