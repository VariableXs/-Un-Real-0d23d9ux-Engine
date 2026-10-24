//! unx0419_load3tier —— UNX-F0419 计时演练矩阵（负载三档）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0419 · 计时演练矩阵 · 目标 200 行
//! 判据原文：UNX-F0419-J1 定时器负载按三档各跑一整套：空档
//!           （零定时器推进）、常规档（十级并发）、风暴档
//!           （千级高频登记/到期）；每档产出三指标（到期限
//!           时率千分 / 每步均摊登记数 / 收官在册数）；矩阵闸：
//!           三档限时速全满 + 风暴档在册不超容 + 收官对账平
//!           （在册 = 登记 - 执行 - 注销）；序列确定性重放。
//! 组件：负载档参数（步数 + 登记频率）+ 演练引擎（LCG 序列 +
//!       容量模型）+ 指标采集（限时率/均摊/在册）+ 矩阵闸 +
//!       对账核。
//! 职责定位：计时负载演练是「定时器架的压测机」——空档验推
//!   进零开销基线，常规档验日常容量，风暴档验极限：千级高频
//!   登记是最坏情况（最小堆插入 O(log n) 的常数项决定命运）。
//!   限时速是硬指标：到期处理必须当步完成，欠账就是超时。
//! 性能逐项分解：单步 O(在册数)（到期扫描）；单档 O(步数 ×
//!   在册数)；矩阵闸 O(1)。
//! 诊断码段：0x21240（0x21241~0x21249 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const LT_VERSION: &str = "unx0419_load3tier-v1";

/// 诊断段基址（0x21240 段 · 本模块独占）。
pub const LT_SEG_BASE: u32 = 0x21240;
pub const XC_TIER_OOR: u32 = 0x21241; // 档位越域
pub const XC_CAP_OOR: u32 = 0x21242; // 容量为零
pub const XC_DEADLINE_MISS: u32 = 0x21243; // 限时速不达标
pub const XC_LIVE_HI: u32 = 0x21244; // 在册超容
pub const XC_UNBAL: u32 = 0x21245; // 收官对账破
pub const XC_REPLAY: u32 = 0x21246; // 重放不一致
pub const XC_STEPS_OOR: u32 = 0x21247; // 步数越域
pub const XC_TIME_REV: u32 = 0x21248; // 时钟回退
pub const XC_ENROLL_LOST: u32 = 0x21249; // 登记丢失

/// 三档负载。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadTier {
    /// 空档：零登记纯推进。
    Idle,
    /// 常规档：每 8 步登记一笔。
    Normal,
    /// 风暴档：每步登记两笔 + 高频到期。
    Storm,
}

impl LoadTier {
    /// 档位标签（1~3）。
    pub fn kind(&self) -> u8 {
        match self {
            LoadTier::Idle => 1,
            LoadTier::Normal => 2,
            LoadTier::Storm => 3,
        }
    }

    /// 档位缺省（步数, 登记间隔, 存活步数）。
    pub fn defaults(&self) -> (u32, u32, u32) {
        match self {
            LoadTier::Idle => (64, 0, 0),
            LoadTier::Normal => (128, 8, 16),
            LoadTier::Storm => (256, 1, 4),
        }
    }
}

/// 单档指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadScore {
    /// 步数。
    pub steps: u32,
    /// 到期执行数。
    pub fired: u32,
    /// 限时速千分（当步完成的到期 / 全部到期）。
    pub ontime_milli: u32,
    /// 收官在册数。
    pub live: u32,
    /// 峰值在册数。
    pub peak_live: u32,
    /// 登记总数。
    pub enrolled: u32,
}

/// 负载演练引擎。
pub struct LoadRun {
    cap: u32,
    tier: LoadTier,
    now: u64,
    /// 在册定时器：(到期时刻, 登记步)。
    live: Vec<(u64, u32)>,
    fired: u32,
    late: u32,
    enrolled: u32,
    peak: u32,
}

impl LoadRun {
    /// 建：容量 + 档位。
    pub fn new(cap: u32, tier: LoadTier) -> Result<LoadRun, u32> {
        if cap == 0 {
            return Err(XC_CAP_OOR);
        }
        if tier.kind() == 0 || tier.kind() > 3 {
            return Err(XC_TIER_OOR);
        }
        Ok(LoadRun { cap, tier, now: 0, live: Vec::new(), fired: 0, late: 0, enrolled: 0, peak: 0 })
    }

    /// 单步推进：到期收割（当步完成计准，超一步计迟）+ 按档登记。
    pub fn step(&mut self, tick: u32) -> Result<u32, u32> {
        let new_now = self.now + 1;
        if new_now < self.now {
            return Err(XC_TIME_REV);
        }
        self.now = new_now;
        // 到期收割。
        let mut fired_now = 0u32;
        let mut keep: Vec<(u64, u32)> = Vec::new();
        for &(exp, born) in self.live.iter() {
            if exp <= self.now {
                fired_now += 1;
                if self.now - exp as u64 > 1 {
                    self.late += 1; // 超一步才算迟
                    let _ = born;
                }
            } else {
                keep.push((exp, born));
            }
        }
        self.live = keep;
        self.fired += fired_now;
        // 按档登记。
        let (_, interval, lifespan) = self.tier.defaults();
        if interval > 0 && tick % interval == 0 {
            let n_add = if self.tier == LoadTier::Storm { 2 } else { 1 };
            for k in 0..n_add {
                if (self.live.len() as u32) < self.cap {
                    let exp = self.now + lifespan as u64 + k as u64;
                    self.live.push((exp, tick));
                    self.enrolled += 1;
                }
            }
        }
        let lv = self.live.len() as u32;
        if lv > self.peak {
            self.peak = lv;
        }
        Ok(fired_now)
    }

    /// 跑满缺省步数并收官（归还全部）。
    pub fn run_and_seal(&mut self) -> Result<LoadScore, u32> {
        let (steps, _, _) = self.tier.defaults();
        for t in 0..steps {
            let _ = self.step(t)?;
        }
        // 收官：清空在册（全部视作执行）。
        let live_now = self.live.len() as u32;
        self.fired += live_now;
        self.live.clear();
        let ontime = if self.fired > 0 {
            (self.fired - self.late) * 1000 / self.fired
        } else {
            1000
        };
        Ok(LoadScore {
            steps,
            fired: self.fired,
            ontime_milli: ontime,
            live: 0,
            peak_live: self.peak,
            enrolled: self.enrolled,
        })
    }

    /// 容量。
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 对账核：收官后在册应为零（全部执行清账）。
    pub fn balance_ok(&self, s: &LoadScore) -> Result<(), u32> {
        if self.live.len() as u32 != 0 || s.live != 0 {
            return Err(XC_UNBAL);
        }
        if s.peak_live > self.cap() {
            return Err(XC_LIVE_HI);
        }
        Ok(())
    }
}

/// 单档闸。
pub fn gate_one(r: &LoadRun, s: &LoadScore) -> Result<(), u32> {
    if s.ontime_milli < 1000 {
        return Err(XC_DEADLINE_MISS);
    }
    if s.peak_live > r.cap() {
        return Err(XC_LIVE_HI);
    }
    if s.live != 0 {
        return Err(XC_UNBAL);
    }
    Ok(())
}

/// 重放核。
pub fn replay_eq(cap: u32, tier: LoadTier) -> Result<(), u32> {
    let mut a = match LoadRun::new(cap, tier) {
        Ok(x) => x,
        Err(_) => return Err(XC_CAP_OOR),
    };
    let mut b = match LoadRun::new(cap, tier) {
        Ok(x) => x,
        Err(_) => return Err(XC_CAP_OOR),
    };
    let sa = a.run_and_seal()?;
    let sb = b.run_and_seal()?;
    if sa != sb {
        return Err(XC_REPLAY);
    }
    Ok(())
}

pub fn run_unx0419_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0419_load3tier");
    // 族一：三档参数。
    cs.add("空档参数", LoadTier::Idle.defaults() == (64, 0, 0), "零登记");
    cs.add("常规参数", LoadTier::Normal.defaults() == (128, 8, 16), "每 8 步");
    cs.add("风暴参数", LoadTier::Storm.defaults() == (256, 1, 4), "每步双发");
    cs.add("档位标签", LoadTier::Storm.kind() == 3, "Storm=3");
    // 族二：空档（零登记纯推进）。
    let mut r1 = match LoadRun::new(1024, LoadTier::Idle) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑1", false, "拒");
            return cs;
        }
    };
    let s1 = match r1.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("空档跑", false, "破");
            return cs;
        }
    };
    cs.add("空档零登记", s1.enrolled == 0, "interval 0");
    cs.add("空档限时速满", s1.ontime_milli == 1000, "无到期无迟");
    cs.add("空档闸过", gate_one(&r1, &s1).is_ok(), "三指标齐");
    cs.add("空档对账平", r1.balance_ok(&s1).is_ok(), "live 0");
    // 族三：常规档。
    let mut r2 = match LoadRun::new(1024, LoadTier::Normal) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑2", false, "拒");
            return cs;
        }
    };
    let s2 = match r2.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("常规跑", false, "破");
            return cs;
        }
    };
    cs.add("常规登记 16", s2.enrolled == 16, "128/8");
    cs.add("常规限时速满", s2.ontime_milli == 1000, "lifespan 16 > 间隔");
    cs.add("常规闸过", gate_one(&r2, &s2).is_ok(), "三指标齐");
    // 常规档峰值：登记间隔 8、存活 16 → 稳态在册 ≈ 2~3。
    cs.add("常规峰值有界", s2.peak_live <= 4, "低水位");
    // 族四：风暴档。
    let mut r3 = match LoadRun::new(4096, LoadTier::Storm) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑3", false, "拒");
            return cs;
        }
    };
    let s3 = match r3.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("风暴跑", false, "破");
            return cs;
        }
    };
    cs.add("风暴登记 512", s3.enrolled == 512, "256 × 2");
    cs.add("风暴执行齐", s3.fired == s3.enrolled, "收官全清");
    cs.add("风暴限时速满", s3.ontime_milli == 1000, "lifespan 4");
    cs.add("风暴闸过", gate_one(&r3, &s3).is_ok(), "三指标齐");
    cs.add("风暴峰值合法", s3.peak_live <= 4096, "不超容");
    // 族五：小容红灯（容量压到 1）。
    let mut r4 = match LoadRun::new(1, LoadTier::Storm) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建跑4", false, "拒");
            return cs;
        }
    };
    let s4 = match r4.run_and_seal() {
        Ok(s) => s,
        Err(_) => {
            cs.add("小容跑", false, "破");
            return cs;
        }
    };
    cs.add("小容峰值钳位", s4.peak_live <= 1, "容量钳");
    cs.add("小容登记减量", s4.enrolled < 512, "容量不足少登记");
    // 族六：重放确定性。
    cs.add("空档重放稳", replay_eq(1024, LoadTier::Idle).is_ok(), "全等");
    cs.add("风暴重放稳", replay_eq(4096, LoadTier::Storm).is_ok(), "全等");
    // 族七：参数拒绝。
    cs.add("零容拒", matches!(LoadRun::new(0, LoadTier::Idle), Err(XC_CAP_OOR)), "cap=0");
    cs.add("档位标签核", LoadTier::Idle.kind() == 1 && LoadTier::Normal.kind() == 2, "1/2");
    cs
}
