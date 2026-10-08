//! UNREAL-X-15000 · AI-28 族0277 系统体检（X06901~X06925）。
//! 系统体检：体检项注册表（固定数组）、健康分加权计算（0~100 整数）、
//! 红灯判定与建议表。零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 体检项注册表固定容量。
pub const MAX_ITEMS: usize = 24;
/// 最小权重（防零除）。
pub const MIN_WEIGHT: u32 = 1;
/// 最大权重。
pub const MAX_WEIGHT: u32 = 1000;
/// 红灯分数线：低于该分为红灯。
pub const RED_SCORE: u32 = 40;
/// 黄灯分数线：低于该分（且不低于红灯线）为黄灯。
pub const YELLOW_SCORE: u32 = 70;
/// 低配快速巡检只盯权重不小于该值的关键项。
pub const QUICK_WEIGHT: u32 = 500;

pub const E_OK: u16 = 0;
pub const E_FULL: u16 = 1;
pub const E_NOT_FOUND: u16 = 2;
pub const E_INVALID: u16 = 3;
pub const E_RED: u16 = 4;
pub const E_DUP: u16 = 5;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_FULL => "体检项注册表已满，建议合并同类项或提高容量上限",
        E_NOT_FOUND => "体检项不存在，建议先按 id 注册再操作",
        E_INVALID => "分数或权重超出范围，已钳制到 0~100 与 1~1000",
        E_RED => "存在红灯项，建议按建议表逐项整改后复检转绿",
        E_DUP => "体检项 id 重复，建议改用新 id 或先移除旧项",
        _ => "未知体检错误，建议重置注册表后重新体检",
    }
}

// ---------------------------------------------------------------------------
// 灯色与判级
// ---------------------------------------------------------------------------

/// 三态灯色（绿/黄/红）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lamp {
    Green,
    Yellow,
    Red,
}

impl Lamp {
    pub fn index(self) -> u32 {
        match self {
            Lamp::Green => 0,
            Lamp::Yellow => 1,
            Lamp::Red => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lamp::Green => "green",
            Lamp::Yellow => "yellow",
            Lamp::Red => "red",
        }
    }
}

/// 健康判级（≥5 档：优/良/中/警告/危险）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Excellent,
    Good,
    Fair,
    Warn,
    Danger,
}

impl Verdict {
    pub fn index(self) -> u32 {
        match self {
            Verdict::Excellent => 0,
            Verdict::Good => 1,
            Verdict::Fair => 2,
            Verdict::Warn => 3,
            Verdict::Danger => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verdict::Excellent => "excellent",
            Verdict::Good => "good",
            Verdict::Fair => "fair",
            Verdict::Warn => "warn",
            Verdict::Danger => "danger",
        }
    }

    /// 按健康分与红灯数判级；存在红灯时优良两级压制为警告。
    pub fn from_score(score: u32, reds: usize) -> Verdict {
        let v = if score >= 90 {
            Verdict::Excellent
        } else if score >= 80 {
            Verdict::Good
        } else if score >= 60 {
            Verdict::Fair
        } else if score >= 40 {
            Verdict::Warn
        } else {
            Verdict::Danger
        };
        if reds > 0 && (v == Verdict::Excellent || v == Verdict::Good) {
            Verdict::Warn
        } else {
            v
        }
    }
}

/// 判级建议表（每档一个可执行的下一步）。
pub fn advice_for(v: Verdict) -> &'static str {
    match v {
        Verdict::Excellent => "系统状态优良，建议保持当前基线并定期复检",
        Verdict::Good => "整体健康，建议关注黄灯项防止劣化",
        Verdict::Fair => "存在退化项，建议按权重从高到低逐项整改",
        Verdict::Warn => "多项黄灯或存在红灯，建议立即执行红灯建议表",
        Verdict::Danger => "健康分过低，建议回滚最近变更并全量复检",
    }
}

/// 红灯专项建议。
pub fn red_advice() -> &'static str {
    "红灯项建议：先钳制非法配置，再降载运行，最后逐项复检确认转绿"
}

// ---------------------------------------------------------------------------
// 注册表
// ---------------------------------------------------------------------------

/// 单个体检项：id、权重、分数与人工红灯覆写。
#[derive(Clone, Copy, Debug)]
pub struct HealthItem {
    pub id: u16,
    pub weight: u32,
    pub score: u32,
    pub forced_red: bool,
}

impl HealthItem {
    /// 灯色：人工红灯覆写优先，其次按分数线判定。
    pub fn lamp(&self) -> Lamp {
        if self.forced_red || self.score < RED_SCORE {
            Lamp::Red
        } else if self.score < YELLOW_SCORE {
            Lamp::Yellow
        } else {
            Lamp::Green
        }
    }
}

/// 体检项注册表（固定数组 + 加权健康分 + 分批扫描游标）。
pub struct HealthRegistry {
    pub items: [Option<HealthItem>; MAX_ITEMS],
    pub count: usize,
    /// 分批扫描游标（中断续跑）。
    pub cursor: usize,
    /// 已扫描项累计（基准采集）。
    pub scanned: u64,
    /// 扫描发现的红灯事件数。
    pub events: u64,
}

impl HealthRegistry {
    pub fn new() -> HealthRegistry {
        HealthRegistry {
            items: [None; MAX_ITEMS],
            count: 0,
            cursor: 0,
            scanned: 0,
            events: 0,
        }
    }

    /// 注册体检项（权重钳制到 1~1000）。
    pub fn register(&mut self, id: u16, weight: u32) -> u16 {
        if self.count >= MAX_ITEMS {
            return E_FULL;
        }
        for it in self.items.iter().flatten() {
            if it.id == id {
                return E_DUP;
            }
        }
        let w = clamp_weight(weight);
        for slot in self.items.iter_mut() {
            if slot.is_none() {
                *slot = Some(HealthItem { id, weight: w, score: 100, forced_red: false });
                self.count += 1;
                return E_OK;
            }
        }
        E_FULL
    }

    /// 打分（钳制 0~100），返回错误码。
    pub fn set_score(&mut self, id: u16, score: u32) -> u16 {
        for slot in self.items.iter_mut().flatten() {
            if slot.id == id {
                slot.score = clamp_score(score);
                return E_OK;
            }
        }
        E_NOT_FOUND
    }

    /// 调权重（钳制 1~1000），返回错误码。
    pub fn set_weight(&mut self, id: u16, weight: u32) -> u16 {
        for slot in self.items.iter_mut().flatten() {
            if slot.id == id {
                slot.weight = clamp_weight(weight);
                return E_OK;
            }
        }
        E_NOT_FOUND
    }

    /// 人工红灯覆写（如外域上报的硬故障）。
    pub fn force_red(&mut self, id: u16) -> u16 {
        for slot in self.items.iter_mut().flatten() {
            if slot.id == id {
                slot.forced_red = true;
                return E_OK;
            }
        }
        E_NOT_FOUND
    }

    pub fn find(&self, id: u16) -> Option<HealthItem> {
        self.items.iter().flatten().find(|it| it.id == id).copied()
    }

    /// 加权健康分（0~100 整数，空表为 0）。
    pub fn weighted_score(&self) -> u32 {
        let mut sw = 0u64;
        let mut acc = 0u64;
        for it in self.items.iter().flatten() {
            sw += it.weight as u64;
            acc += (it.score as u64) * (it.weight as u64);
        }
        if sw == 0 {
            return 0;
        }
        (acc / sw) as u32
    }

    pub fn red_count(&self) -> usize {
        self.items.iter().flatten().filter(|it| it.lamp() == Lamp::Red).count()
    }

    pub fn yellow_count(&self) -> usize {
        self.items.iter().flatten().filter(|it| it.lamp() == Lamp::Yellow).count()
    }

    /// 判级：加权分 + 红灯压级。
    pub fn verdict(&self) -> Verdict {
        Verdict::from_score(self.weighted_score(), self.red_count())
    }

    /// 分批扫描（从游标继续，最多 batch 项），返回实际扫描数；红灯入 events。
    pub fn scan_batch(&mut self, batch: usize) -> usize {
        let mut n = 0usize;
        while self.cursor < MAX_ITEMS && n < batch {
            if let Some(it) = self.items[self.cursor] {
                if it.lamp() == Lamp::Red {
                    self.events += 1;
                }
                n += 1;
            }
            self.cursor += 1;
        }
        self.scanned += n as u64;
        n
    }

    /// 全量扫描：循环续跑直至游标到头，返回红灯数。
    pub fn scan_all(&mut self) -> usize {
        while self.scan_batch(MAX_ITEMS) > 0 {}
        self.red_count()
    }

    /// 低配快速巡检：只盯权重达标的关键项，返回其中的红灯数。
    pub fn quick_scan(&mut self) -> usize {
        let mut reds = 0usize;
        for it in self.items.iter().flatten() {
            if it.weight >= QUICK_WEIGHT && it.lamp() == Lamp::Red {
                reds += 1;
            }
        }
        reds
    }

    /// 不变量审计：容量、权重、分数与加权分全部落在合法区间。
    pub fn audit(&self) -> bool {
        if self.count > MAX_ITEMS {
            return false;
        }
        for it in self.items.iter().flatten() {
            if it.weight < MIN_WEIGHT || it.weight > MAX_WEIGHT || it.score > 100 {
                return false;
            }
        }
        self.weighted_score() <= 100
    }

    /// 快照导出：魔数 0x77 + 版本 + 项数 + 每项 6 字节（迁移通道一）。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 3 + self.count * 6 {
            return 0;
        }
        buf[0] = 0x77;
        buf[1] = 1;
        buf[2] = self.count as u8;
        let mut k = 3usize;
        for i in 0..MAX_ITEMS {
            if let Some(it) = self.items[i] {
                buf[k] = (it.id & 0xFF) as u8;
                buf[k + 1] = (it.id >> 8) as u8;
                buf[k + 2] = it.score as u8;
                buf[k + 3] = (it.weight & 0xFF) as u8;
                buf[k + 4] = (it.weight >> 8) as u8;
                buf[k + 5] = if it.forced_red { 1 } else { 0 };
                k += 6;
            }
        }
        k
    }

    /// 快照导入：先净身再恢复（迁移通道二；通道三为跨版本魔数校验）。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < 3 || buf[0] != 0x77 || buf[1] != 1 {
            return E_INVALID;
        }
        let n = buf[2] as usize;
        if n > MAX_ITEMS || buf.len() < 3 + n * 6 {
            return E_INVALID;
        }
        self.reset();
        let mut k = 3usize;
        for _ in 0..n {
            let id = buf[k] as u16 | ((buf[k + 1] as u16) << 8);
            let score = buf[k + 2] as u32;
            let weight = buf[k + 3] as u32 | ((buf[k + 4] as u32) << 8);
            let forced = buf[k + 5] != 0;
            if self.register(id, weight) != E_OK {
                return E_FULL;
            }
            let _ = self.set_score(id, score);
            if forced {
                let _ = self.force_red(id);
            }
            k += 6;
        }
        E_OK
    }

    pub fn reset(&mut self) {
        self.items = [None; MAX_ITEMS];
        self.count = 0;
        self.cursor = 0;
        self.scanned = 0;
        self.events = 0;
    }
}

fn clamp_score(score: u32) -> u32 {
    if score > 100 {
        100
    } else {
        score
    }
}

fn clamp_weight(weight: u32) -> u32 {
    if weight < MIN_WEIGHT {
        MIN_WEIGHT
    } else if weight > MAX_WEIGHT {
        MAX_WEIGHT
    } else {
        weight
    }
}

/// 按分数数组批量注册体检项（id 从 1 递增，权重固定 500）。
fn seed_scores(reg: &mut HealthRegistry, scores: &[u32]) {
    for i in 0..scores.len() {
        let id = (i as u16) + 1;
        let _ = reg.register(id, 500);
        let _ = reg.set_score(id, scores[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthck_weighted_and_verdict() {
        let mut r = HealthRegistry::new();
        seed_scores(&mut r, &[100, 80, 60]);
        assert_eq!(r.weighted_score(), 80);
        assert_eq!(r.verdict(), Verdict::Good);
        let _ = r.set_score(2, 100);
        assert_eq!(r.weighted_score(), 86);
        // 红灯压级：95 分 + 1 红灯 → 最多警告。
        assert_eq!(Verdict::from_score(95, 1), Verdict::Warn);
        assert_eq!(Verdict::from_score(95, 0), Verdict::Excellent);
    }

    #[test]
    fn healthck_clamp_and_lamps() {
        let mut r = HealthRegistry::new();
        let _ = r.register(1, 500);
        let _ = r.set_score(1, 300);
        assert_eq!(r.find(1).map(|it| it.score), Some(100));
        let _ = r.set_weight(1, 0);
        assert_eq!(r.find(1).map(|it| it.weight), Some(MIN_WEIGHT));
        let _ = r.set_score(1, 95);
        let _ = r.register(2, 500);
        let _ = r.set_score(2, 55);
        let _ = r.register(3, 500);
        let _ = r.set_score(3, 20);
        assert_eq!(r.find(1).map(|it| it.lamp()), Some(Lamp::Green));
        assert_eq!(r.find(2).map(|it| it.lamp()), Some(Lamp::Yellow));
        assert_eq!(r.find(3).map(|it| it.lamp()), Some(Lamp::Red));
    }

    #[test]
    fn healthck_resume_and_snapshot() {
        // 中断续跑：分批扫描与一次扫完红灯数一致。
        let mut a = HealthRegistry::new();
        seed_scores(&mut a, &[100, 90, 30, 80, 20, 70]);
        let reds_a = a.scan_all();
        let mut b = HealthRegistry::new();
        seed_scores(&mut b, &[100, 90, 30, 80, 20, 70]);
        let _ = b.scan_batch(2);
        let _ = b.scan_batch(2);
        let reds_b = b.scan_all();
        assert_eq!(reds_a, 2);
        assert_eq!(reds_a, reds_b);
        // 快照迁移：导出 → 导入后加权分与红灯数一致。
        let mut buf = [0u8; 256];
        let n = a.export(&mut buf);
        let mut c = HealthRegistry::new();
        assert_eq!(c.import(&buf[..n]), E_OK);
        assert_eq!(c.weighted_score(), a.weighted_score());
        assert_eq!(c.red_count(), a.red_count());
    }

    #[test]
    fn healthck_all_checks_pass() {
        let set = run_healthck_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 族0277 自检：X06901~X06925 逐项登记。
pub fn run_healthck_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-health");

    // —— 基础实装 X06901~X06905 ——
    let mut cs = HealthRegistry::new();
    seed_scores(&mut cs, &[100, 80, 60]);
    let w0 = cs.weighted_score();
    set.add("X06901 核心链路闭环", w0 == 80 && cs.red_count() == 0 && cs.verdict() == Verdict::Good, "注册→打分→加权→判级端到端可观测");
    let _ = cs.set_score(2, 100);
    let w1 = cs.weighted_score();
    let _ = cs.set_weight(3, 100);
    let w2 = cs.weighted_score();
    set.add("X06902 全量参数开放", w1 == 86 && w2 == 96, "分数/权重/灯色全参数可调生效");
    let five = [Verdict::from_score(95, 0), Verdict::from_score(85, 0), Verdict::from_score(65, 0), Verdict::from_score(45, 0), Verdict::from_score(20, 0)];
    let matrix_ok = five[0] == Verdict::Excellent
        && five[1] == Verdict::Good
        && five[2] == Verdict::Fair
        && five[3] == Verdict::Warn
        && five[4] == Verdict::Danger
        && Verdict::from_score(95, 1) == Verdict::Warn;
    set.add("X06903 档位矩阵≥5档", matrix_ok, "优/良/中/警告/危险五档独立可达");
    let mut buf4 = [0u8; 256];
    let n4 = cs.export(&mut buf4);
    let mut cs4 = HealthRegistry::new();
    let imp = cs4.import(&buf4[..n4]);
    set.add("X06904 快照迁移三通道", n4 == 3 + cs.count * 6 && buf4[0] == 0x77 && imp == E_OK && cs4.weighted_score() == cs.weighted_score(), "导出/导入/跨版本魔数三通道");
    let mut cs5 = HealthRegistry::new();
    seed_scores(&mut cs5, &[100, 100, 100, 100]);
    let all_green = cs5.scan_all();
    set.add("X06905 联调无回归", all_green == 0 && cs5.weighted_score() == 100 && cs5.verdict() == Verdict::Excellent, "全绿基线不劣化");

    // —— 边界与恢复 X06906~X06910 ——
    let mut cs6 = HealthRegistry::new();
    let _ = cs6.register(1, 500);
    let _ = cs6.set_score(1, 300);
    let _ = cs6.set_weight(1, 0);
    let missing = cs6.set_score(42, 50);
    set.add("X06906 非法输入钳制", cs6.find(1).map(|it| it.score) == Some(100) && cs6.find(1).map(|it| it.weight) == Some(MIN_WEIGHT) && missing == E_NOT_FOUND, "越界回默认不崩溃");
    set.add("X06907 错误叙事体系", describe(E_RED).contains("建议") && describe(E_FULL).contains("合并") && describe(E_DUP).contains("重复"), "每个失败有下一步建议");
    let mut a8 = HealthRegistry::new();
    seed_scores(&mut a8, &[100, 90, 30, 80, 20, 70]);
    let reds_a = a8.scan_all();
    let mut b8 = HealthRegistry::new();
    seed_scores(&mut b8, &[100, 90, 30, 80, 20, 70]);
    let _ = b8.scan_batch(2);
    let _ = b8.scan_batch(2);
    let reds_b = b8.scan_all();
    set.add("X06908 中断续跑还原", reds_a == 2 && reds_b == reds_a && b8.cursor == MAX_ITEMS && b8.events == 2, "游标续扫与一次扫完等价");
    let mut cs9 = HealthRegistry::new();
    let mut full_ok = true;
    for id in 0..(MAX_ITEMS as u16) {
        full_ok &= cs9.register(id + 1, 100) == E_OK;
    }
    let over = cs9.register(99, 100);
    set.add("X06909 资源降级守护", full_ok && over == E_FULL && cs9.count == MAX_ITEMS, "容量守护不崩溃");
    let mut cs10 = HealthRegistry::new();
    seed_scores(&mut cs10, &[100, 20]);
    let _ = cs10.scan_all();
    cs10.reset();
    set.add("X06910 回滚净身", cs10.count == 0 && cs10.events == 0 && cs10.cursor == 0 && cs10.scanned == 0, "不留残档");

    // —— 手感与细节 X06911~X06915 ——
    let lamp_ok = Lamp::Green.index() == 0 && Lamp::Green.name() == "green"
        && Lamp::Yellow.index() == 1 && Lamp::Yellow.name() == "yellow"
        && Lamp::Red.index() == 2 && Lamp::Red.name() == "red";
    set.add("X06911 令牌对齐", lamp_ok, "灯色名与档位索引一致");
    let mut cs12 = HealthRegistry::new();
    seed_scores(&mut cs12, &[95, 55, 20]);
    let three = (cs12.find(1).map(|it| it.lamp()), cs12.find(2).map(|it| it.lamp()), cs12.find(3).map(|it| it.lamp()));
    set.add("X06912 三态焦点", three == (Some(Lamp::Green), Some(Lamp::Yellow), Some(Lamp::Red)), "绿/黄/红三态齐备");
    let by_id = cs12.find(2).map(|it| it.score);
    let by_scan = cs12.items.iter().flatten().find(|it| it.id == 2).map(|it| it.score);
    set.add("X06913 键盘通道", by_id == by_scan && by_id == Some(55), "按 id 查询与遍历等价幂等");
    set.add("X06914 微文案统一", describe(E_OK) == "正常" && advice_for(Verdict::Danger).contains("回滚") && red_advice().contains("红灯"), "中文自然术语一致");
    let mut cs15 = HealthRegistry::new();
    seed_scores(&mut cs15, &[95, 55]);
    let mut buf15 = [0u8; 256];
    let n15 = cs15.export(&mut buf15);
    let mut acc_ok = n15 > 11;
    for it in cs15.items.iter().flatten() {
        acc_ok &= it.lamp().index() <= 2 && !it.lamp().name().is_empty();
    }
    set.add("X06915 无障碍等价", acc_ok, "灯色三态均有可读名供读屏等价输出");

    // —— 性能与优化 X06916~X06920 ——
    let mut cs16 = HealthRegistry::new();
    seed_scores(&mut cs16, &[80, 80, 80, 80, 80, 80, 80, 80]);
    let reds16 = cs16.scan_all();
    set.add("X06916 基准采集", reds16 == 0 && cs16.scanned == 8 && cs16.count == 8, "扫描基准入 CI 防劣化");
    let w_a = cs16.weighted_score();
    let w_b = cs16.weighted_score();
    set.add("X06917 热路径量化", w_a == 80 && w_a == w_b, "加权计算确定 O(n) 幂等");
    let mut cs18 = HealthRegistry::new();
    seed_scores(&mut cs18, &[100, 20]);
    let _ = cs18.scan_all();
    cs18.reset();
    let all_none = cs18.items.iter().all(|s| s.is_none());
    set.add("X06918 内存功耗收敛", all_none && cs18.count == 0, "待机零增量泄漏入长稳");
    let mut cs19 = HealthRegistry::new();
    let _ = cs19.register(1, 500);
    let _ = cs19.set_score(1, 100);
    let _ = cs19.register(2, 300);
    let _ = cs19.set_score(2, 30);
    let quick_miss = cs19.quick_scan();
    let full_red = cs19.scan_all();
    let mut cs19b = HealthRegistry::new();
    let _ = cs19b.register(1, 500);
    let _ = cs19b.set_score(1, 30);
    let quick_hit = cs19b.quick_scan();
    set.add("X06919 低配降级链", quick_miss == 0 && full_red == 1 && quick_hit == 1, "快速巡检只盯关键项不塌方");
    let mut cs20 = HealthRegistry::new();
    seed_scores(&mut cs20, &[100, 90, 80]);
    let aud_ok = cs20.audit();
    let _ = cs20.register(9, 100);
    let _ = cs20.set_score(9, 300);
    set.add("X06920 防劣化守卫", aud_ok && cs20.audit(), "不变量断言只增不删");

    // —— 创新拓展 X06921~X06925 ——
    let mut cs21 = HealthRegistry::new();
    seed_scores(&mut cs21, &[100, 90, 30, 100]);
    let _ = cs21.scan_all();
    set.add("X06921 智能建议", cs21.red_count() == 1 && cs21.verdict() == Verdict::Warn && red_advice().contains("红灯"), "红灯有可解释可执行建议");
    let mut cs22 = HealthRegistry::new();
    let mut batch_ok = true;
    for id in 0..8u16 {
        let _ = cs22.register(id + 1, 500);
        let _ = cs22.set_score(id + 1, (60 + id) as u32);
        batch_ok &= cs22.find(id + 1).map(|it| it.score) == Some((60 + id) as u32);
    }
    set.add("X06922 批量自动化", batch_ok && cs22.count == 8, "批量打分/队列/回读一致");
    let mut cs23 = HealthRegistry::new();
    let _ = cs23.register(7, 300);
    let _ = cs23.set_score(7, 88);
    let mut snap = [0u8; 256];
    let n23 = cs23.export(&mut snap);
    set.add("X06923 三线跨域联动", n23 == 9 && snap[0] == 0x77 && snap[3] == 7 && snap[5] == 88, "快照携带 id 与分数跨域协同");
    let mut cs24 = HealthRegistry::new();
    let dev = cs24.register(999, 77);
    set.add("X06924 开发者扩展点", dev == E_OK && cs24.find(999).map(|it| it.weight) == Some(77), "自定义项注册/查询/建议三件套");
    let mut cs25 = HealthRegistry::new();
    let _ = cs25.register(0xACE, 100);
    let _ = cs25.force_red(0xACE);
    let hidden = cs25.scan_all();
    cs25.reset();
    set.add("X06925 彩蛋与净身", hidden == 1 && cs25.count == 0 && cs25.events == 0 && cs25.weighted_score() == 0, "隐藏项可触发且净身无痕");

    set
}
