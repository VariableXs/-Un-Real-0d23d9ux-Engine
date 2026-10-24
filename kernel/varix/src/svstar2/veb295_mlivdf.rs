//! veb295_mlivdf —— VE-F0295 Mali 驱动版本差异表。
//! 锚点（CoRun Unxreal 主册）：VE-F0295 · Mali 驱动版本差异表 · 目标 320 行
//! 判据原文：VE-F0295-J1 版本差异账：版本序（r 价 + 修订号
//!           二元组比较）；差异条目（功能名 + 门槛版本 + 行
//!           为标记 有/无/改名）；逐条判定（当前版本 ≥ 门槛
//!           → 有，否则 无）；改名映射（跨门槛版本行为名变
//!           → 提供新名映射）；差异表冻结（版本选定后表只
//!           读）；全景计数（有/无/改名三类计数核）；门槛序
//!           核（同功能门槛重复登记拒）。
//! 组件：版本序 + 差异表 + 改名映射 + 三态计数。
//! 职责定位：版本差异表是「Mali 驱动的分水岭地图」——门槛
//!           版本是界碑：界碑这边一个名字那边另一个名字；
//!           改名映射是过河的词典：不 provided 词典的改名就
//!           是断桥；重复门槛拒是地图的诚实：一条河只立一
//!           块界碑。
//! 性能逐项分解：比较 O(1)；判定 O(条数)。
//! 诊断码段：0x312C0（0x312C1~0x312C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VD_VERSION: &str = "veb295_mlivdf-v1";

/// 诊断段基址（0x312C0 段 · 本模块独占）。
pub const VD_SEG_BASE: u32 = 0x312C0;
pub const XC_MAGIC: u32 = 0x312C1; // 账魔数破
pub const XC_FROZEN: u32 = 0x312C2; // 表已冻结
pub const XC_DUP: u32 = 0x312C3; // 功能重复

/// 账魔数。
const MAGIC: u32 = 0x312C_1B07;

/// 驱动版本（r 价 + 修订）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrvVer {
    pub rel: u32,
    pub rev: u32,
}

impl DrvVer {
    /// 比较：先 rel 后 rev。
    pub fn ge(&self, o: &DrvVer) -> bool {
        self.rel > o.rel || (self.rel == o.rel && self.rev >= o.rev)
    }
}

/// 行为标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// 有（新版行为名）。
    Have,
    /// 无。
    None,
    /// 改名（旧名 → 新名）。
    Renamed,
}

/// 差异条目。
pub struct DiffRow {
    /// 功能名。
    pub feat: u32, // 模型化为功能 ID
    pub gate: DrvVer,
    pub mark: Mark,
    /// 改名后的新功能 ID。
    pub new_feat: u32,
}

/// 差异表。
pub struct DiffTab {
    rows: Vec<DiffRow>,
    /// 选定版本。
    pub cur: DrvVer,
    pub frozen: bool,
    magic: u32,
}

/// 逐条判定结果。
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Have(u32),      // 功能 ID（改名条给新名）
    None,
    Renamed(u32),
}

impl DiffTab {
    /// 建（选定版本即冻结基）。
    pub fn new(cur: DrvVer) -> DiffTab {
        DiffTab { rows: Vec::new(), cur, frozen: false, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记条目（冻结后拒、功能重复拒）。
    pub fn add(&mut self, feat: u32, gate: DrvVer, mark: Mark, new_feat: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        for r in self.rows.iter() {
            if r.feat == feat {
                return Err(XC_DUP);
            }
        }
        self.rows.push(DiffRow { feat, gate, mark, new_feat });
        Ok(())
    }

    /// 冻结：版本选定后表只读。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.frozen = true;
        Ok(())
    }

    /// 逐条判定。
    pub fn verdict(&self, feat: u32) -> Result<Verdict, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for r in self.rows.iter() {
            if r.feat == feat {
                return Ok(if self.cur.ge(&r.gate) {
                    match r.mark {
                        Mark::Have => Verdict::Have(r.feat),
                        Mark::None => Verdict::None,
                        Mark::Renamed => Verdict::Renamed(r.new_feat),
                    }
                } else {
                    Verdict::None
                });
            }
        }
        // 未登记 → 无。
        Ok(Verdict::None)
    }

    /// 三类全景计数。
    pub fn census(&self) -> (u32, u32, u32) {
        let (mut have, mut none, mut ren) = (0u32, 0u32, 0u32);
        for r in self.rows.iter() {
            if self.cur.ge(&r.gate) {
                match r.mark {
                    Mark::Have => have += 1,
                    Mark::None => none += 1,
                    Mark::Renamed => ren += 1,
                }
            } else {
                none += 1;
            }
        }
        (have, none, ren)
    }
}

pub fn run_veb295_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0295");
    // 族零：版本序。
    let v1 = DrvVer { rel: 30, rev: 5 };
    let v2 = DrvVer { rel: 30, rev: 10 };
    let v3 = DrvVer { rel: 31, rev: 0 };
    s.add("同 rel 比 rev", v2.ge(&v1), "10≥5");
    s.add("rev 逆不过", v1.ge(&v2) == false, "5<10");
    s.add("rel 优先", v3.ge(&v2), "31>30");
    s.add("同版等", v1.ge(&v1), "自比");
    // 族一：登记与重复。
    let mut t = DiffTab::new(v2);
    let g20 = DrvVer { rel: 20, rev: 0 };
    let g35 = DrvVer { rel: 35, rev: 0 };
    s.add("登 A 有", t.add(1, g20, Mark::Have, 0).is_ok(), "门槛 20");
    s.add("重 A 拒", t.add(1, g20, Mark::Have, 0) == Err(XC_DUP), "门槛重复");
    s.add("登 B 无", t.add(2, g35, Mark::None, 0).is_ok(), "门槛 35");
    s.add("登 C 改名", t.add(3, g20, Mark::Renamed, 33).is_ok(), "20 后更名");
    // 族二：逐条判定（当前 v2 = r30.10）。
    s.add("A 有", t.verdict(1) == Ok(Verdict::Have(1)), "30≥20");
    s.add("B 无", t.verdict(2) == Ok(Verdict::None), "30<35");
    s.add("C 新名", t.verdict(3) == Ok(Verdict::Renamed(33)), "过河词典");
    s.add("未登记无", t.verdict(9) == Ok(Verdict::None), "表外无");
    // 门槛前版本 → 无（改名条不过河也是无）。
    let mut t_old = DiffTab::new(DrvVer { rel: 10, rev: 9 });
    let _ = t_old.add(3, g20, Mark::Renamed, 33);
    s.add("河前无名", t_old.verdict(3) == Ok(Verdict::None), "未过门槛");
    // 族三：全景计数（t: A 有、B 无、C 改名）。
    let (h, n, r) = t.census();
    s.add("计有 1", h == 1, "A");
    s.add("计无 1", n == 1, "B");
    s.add("计改名 1", r == 1, "C");
    // 换版本计数翻转：v3 过 35 门槛。
    let mut t2 = DiffTab::new(v3);
    let _ = t2.add(1, g20, Mark::Have, 0);
    let _ = t2.add(2, g35, Mark::Have, 0);
    let _ = t2.add(3, g20, Mark::Renamed, 33);
    let (h2, n2, r2) = t2.census();
    s.add("新版有 2", h2 == 2, "A+B");
    s.add("新版无 0", n2 == 0, "全过");
    s.add("新版改名 1", r2 == 1, "C 仍名变");
    // 族四：冻结。
    s.add("冻结", t.freeze().is_ok(), "版本定");
    s.add("冻后登拒", t.add(4, g20, Mark::Have, 0) == Err(XC_FROZEN), "表只读");
    s.add("冻后再冻", t.freeze().is_ok(), "幂等");
    s.add("冻后判定照常", t.verdict(1) == Ok(Verdict::Have(1)), "只读可用");
    // 族五：rev 边界。
    let eq1 = DrvVer { rel: 20, rev: 0 };
    let mut tb = DiffTab::new(eq1);
    let _ = tb.add(5, g20, Mark::Have, 0);
    s.add("恰门槛有", tb.verdict(5) == Ok(Verdict::Have(5)), "≥ 含等");
    s.add("魔数核", VD_SEG_BASE == 0x312C0, "段基址");
    s
}
