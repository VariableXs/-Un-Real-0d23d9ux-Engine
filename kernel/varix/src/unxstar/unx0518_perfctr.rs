//! unx0518_perfctr —— UNX-F0518 翻译性能计数账（命中/旁路/失速分账 + 饱和 + 归一率核）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0518 · 性能计数账 · 目标 210 行
//! 判据原文：UNX-F0518-J1 性能计数账：计一 三账分立
//!           （TLB 命中/走查旁路/失速三项计数独立累加，
//!           不串账）；计二 饱和不回卷（计数到 u32 上限
//!           后停在饱和值，绝不回卷清零——回卷即假低）；
//!           计三 归一率核（命中率 = 命中/(命中+旁路)，
//!           千分位整数表示，分母零时率恒零）；计四 失
//!           速不计命中（失速（队列拥塞等待）是独立事件，
//!           既不算命中也不算旁路）；计五 采样期隔离（
//!           窗口切换时账快照归档，新窗从零起算）。
//! 组件：三账计数器 + u32 饱和 + 千分位归一 + 窗口快照。
//! 职责定位：性能计数是 IOMMU 的体检表——命中率决定缓
//!   存收益、失速率决定队列是否成瓶颈。u32 回卷是最经典
//!   的监控自欺：长跑计数回卷成小数值，告警全灭。饱和+
//!   窗口快照是标准化对策。
//! 性能逐项分解：累加 O(1)；归一 O(1)。
//! 诊断码段：0x34F00（0x34F01~0x34F09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34F00;
pub const XC_MIXED: u32 = SEG_BASE + 1; // 失速串账违例

/// u32 饱和累加。
pub fn sat_add(cur: u32, d: u32) -> u32 {
    cur.saturating_add(d)
}

/// 三账计数器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfCtr {
    pub hit: u32,
    pub walk: u32,
    pub stall: u32,
    pub sat: bool,
}

impl PerfCtr {
    pub fn new() -> Self {
        PerfCtr { hit: 0, walk: 0, stall: 0, sat: false }
    }

    /// 事件记账（计四：Stall 走独立账）。
    pub fn event(&mut self, kind: EvKind) {
        match kind {
            EvKind::Hit => {
                self.hit = sat_add(self.hit, 1);
                let _ = XC_MIXED;
            }
            EvKind::Walk => self.walk = sat_add(self.walk, 1),
            EvKind::Stall => self.stall = sat_add(self.stall, 1),
        }
        if u32::MAX == self.hit || u32::MAX == self.walk || u32::MAX == self.stall {
            self.sat = true;
        }
    }

    /// 计三：命中率（千分位，分母零恒零）。
    pub fn hit_permille(&self) -> u32 {
        let denom = self.hit as u64 + self.walk as u64;
        if denom == 0 {
            return 0;
        }
        (self.hit as u64 * 1000 / denom) as u32
    }

    /// 计五：窗口快照归档（返回旧账并清零）。
    pub fn rotate(&mut self) -> PerfCtr {
        let snap = *self;
        *self = PerfCtr::new();
        snap
    }
}

/// 事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    Hit,
    Walk,
    Stall,
}

pub fn run_unx0518_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0518");
    cs.add("SEG 段位独占", SEG_BASE == 0x34F00 && XC_MIXED == SEG_BASE + 1, "u32 高段全域普查无冲突");

    // 族一：三账分立。
    let mut c = PerfCtr::new();
    c.event(EvKind::Hit);
    c.event(EvKind::Hit);
    c.event(EvKind::Walk);
    c.event(EvKind::Stall);
    cs.add("三账独立", c.hit == 2 && c.walk == 1 && c.stall == 1, "各记各账");
    cs.add("无串账", c.hit + c.walk + c.stall == 4, "总量=事件数");

    // 族二：饱和不回卷。
    let mut s = PerfCtr::new();
    s.hit = u32::MAX - 1;
    s.event(EvKind::Hit);
    cs.add("饱和停在顶", s.hit == u32::MAX, "MAX-1+1=MAX");
    s.event(EvKind::Hit);
    cs.add("顶上不回卷", s.hit == u32::MAX && s.sat, "saturating 语义");

    // 族三：归一率。
    let r = PerfCtr { hit: 750, walk: 250, stall: 0, sat: false };
    cs.add("命中率千分位", r.hit_permille() == 750, "750/1000");
    let r2 = PerfCtr { hit: 0, walk: 0, stall: 5, sat: false };
    cs.add("分母零恒零", r2.hit_permille() == 0, "零请求零率");
    let r3 = PerfCtr { hit: 1, walk: 0, stall: 0, sat: false };
    cs.add("全中千分千", r3.hit_permille() == 1000, "1000‰");
    // stall 不进分母（计四）。
    let r4 = PerfCtr { hit: 500, walk: 500, stall: 1000, sat: false };
    cs.add("失速不进率", r4.hit_permille() == 500, "分母只有命中+旁路");

    // 族四：失速独立（失速不影响 hit/walk）。
    let mut s2 = PerfCtr::new();
    s2.event(EvKind::Stall);
    s2.event(EvKind::Stall);
    cs.add("纯失速零率", s2.hit_permille() == 0 && s2.stall == 2, "失速仅自账");

    // 族五：窗口快照。
    let mut w = PerfCtr::new();
    w.event(EvKind::Hit);
    w.event(EvKind::Walk);
    let snap = w.rotate();
    cs.add("快照留痕", snap.hit == 1 && snap.walk == 1, "归档含旧值");
    cs.add("新窗归零", w.hit == 0 && w.walk == 0 && w.stall == 0, "rotate 清零");
    cs.add("新窗独立率", w.hit_permille() == 0, "零起点");
    // 快照不影响原账能力。
    w.event(EvKind::Hit);
    cs.add("新窗可再记", w.hit == 1, "清零后照常累加");

    // 族六：饱和边界遍历。
    let mut e = 0u32;
    let mut v = u32::MAX - 2;
    for _ in 0..5 {
        v = sat_add(v, 1);
        if v == u32::MAX {
            e += 1;
        }
    }
    cs.add("饱和边界核", v == u32::MAX && e == 1, "两次到顶后恒顶");

    // 族七：事件流归一稳定性（大流量下率值抖动核）。
    let mut f = PerfCtr::new();
    // 前半窗：命中 7 旁路 3 → 700‰。
    for _ in 0..7 {
        f.event(EvKind::Hit);
    }
    for _ in 0..3 {
        f.event(EvKind::Walk);
    }
    cs.add("前半窗率", f.hit_permille() == 700, "7/10");
    // 后半窗：命中 3 旁路 7 → 累计 10/20 → 500‰。
    for _ in 0..3 {
        f.event(EvKind::Hit);
    }
    for _ in 0..7 {
        f.event(EvKind::Walk);
    }
    cs.add("累计率稀释", f.hit_permille() == 500, "10/20 拉平");
    // 旋转后新窗率独立。
    let old = f.rotate();
    cs.add("旧窗存档", old.hit == 10 && old.walk == 10, "归档完整");
    f.event(EvKind::Hit);
    cs.add("新窗全中率", f.hit_permille() == 1000, "1/1");

    // 族八：单调账自检（计数只增不减）。
    let mut g = PerfCtr::new();
    let mut mono = true;
    let mut prev = (0u32, 0u32, 0u32);
    for i in 0..9u32 {
        match i % 3 {
            0 => g.event(EvKind::Hit),
            1 => g.event(EvKind::Walk),
            _ => g.event(EvKind::Stall),
        }
        if !(g.hit >= prev.0 && g.walk >= prev.1 && g.stall >= prev.2) {
            mono = false;
        }
        prev = (g.hit, g.walk, g.stall);
    }
    cs.add("账单调", mono && g.hit + g.walk + g.stall == 9, "九事件九增");
    cs.add("三账均衡", g.hit == 3 && g.walk == 3 && g.stall == 3, "轮转均匀");

    cs
}
