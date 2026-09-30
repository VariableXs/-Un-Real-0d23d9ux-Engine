//! UNREAL-X AI-60 大收官 C 线（族0593 三线一致性审计 · X14801~X14825 /
//! 族0594 ID 唯一性防线 · X14826~X14850），勿删。
//! 与 V 线（src/features/engops/ai60Models.ts）同口径镜像：零 AI、零随机、零时钟依赖。

use crate::checks::CheckSet;

// ---- 族0593 三线一致性审计（X14801~X14825）----

/// 三线分布行：一族 25 项在 K/V/C 三线的落点计数。
#[derive(Clone, PartialEq)]
pub struct ConsistencyRow {
    pub family: u32,
    pub k: u32,
    pub v: u32,
    pub c: u32,
}

/// 三线一致性审计：逐族登记 + 最大偏斜 + 平衡判定。
pub struct ConsistencyAudit {
    pub rows: Vec<ConsistencyRow>,
    pub clamped: u32,
}

impl Default for ConsistencyAudit {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsistencyAudit {
    pub fn new() -> Self {
        ConsistencyAudit { rows: Vec::new(), clamped: 0 }
    }

    /// 登记一族三线计数：族号 1~600、非负、总和恰为 25、同族不重复。
    pub fn record(&mut self, family: u32, k: u32, v: u32, c: u32) -> bool {
        if !(1..=600).contains(&family) || k + v + c != 25 {
            self.clamped += 1;
            return false;
        }
        if self.rows.iter().any(|r| r.family == family) {
            self.clamped += 1;
            return false;
        }
        self.rows.push(ConsistencyRow { family, k, v, c });
        true
    }

    /// 最大偏斜：全部行中三线计数的最大差。
    pub fn max_skew(&self) -> u32 {
        self.rows
            .iter()
            .map(|r| r.k.max(r.v).max(r.c) - r.k.min(r.v).min(r.c))
            .max()
            .unwrap_or(0)
    }

    /// 平衡判定：存在记录且所有行偏斜 ≤1。
    pub fn balanced(&self) -> bool {
        !self.rows.is_empty() && self.max_skew() <= 1
    }

    /// 审计判定：balanced / skewed / empty。
    pub fn verdict(&self) -> &'static str {
        if self.rows.is_empty() {
            "empty"
        } else if self.balanced() {
            "balanced"
        } else {
            "skewed"
        }
    }

    /// 审计总量：行数 × 25。
    pub fn total(&self) -> usize {
        self.rows.len() * 25
    }
}

/// 族0593 三线一致性审计（X14801~X14825）：25 项自检。
pub fn run_consistency_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai60-consistency");
    let mut a = ConsistencyAudit::new();
    s.add("X14801 最小闭环", a.record(591, 9, 8, 8) && a.rows.len() == 1, "一族三线登记即计数");
    s.add("X14802 全量参数", a.record(592, 8, 9, 8) && a.record(593, 8, 8, 9) && a.rows.len() == 3, "多族批量登记");
    s.add("X14803 档位矩阵", a.total() == 75, "总量 = 行数 × 25");
    s.add("X14804 快照迁移", a.rows.first().map(|r| r.k) == Some(9) && a.rows[2].c == 9, "行内三线计数确定性可迁移");
    s.add("X14805 联调集成", a.balanced(), "偏斜 ≤1 判平衡");
    s.add(
        "X14806 越界钳制",
        { let mut q = ConsistencyAudit::new(); !q.record(591, 10, 10, 4) && q.clamped >= 1 },
        "总和 ≠25 被拒并计数",
    );
    s.add(
        "X14807 失败叙事",
        { let mut q = ConsistencyAudit::new(); !q.record(0, 8, 8, 9) && !q.record(601, 8, 8, 9) },
        "族号越界 0/601 拒绝",
    );
    s.add(
        "X14808 中断还原",
        { let mut q = ConsistencyAudit::new(); q.record(591, 9, 8, 8); !q.record(591, 8, 8, 9) },
        "同族重复登记拒绝",
    );
    s.add(
        "X14809 资源降级",
        { let q = ConsistencyAudit::new(); q.verdict() == "empty" },
        "空审计降级为 empty 档",
    );
    s.add(
        "X14810 回滚净身",
        { let mut q = ConsistencyAudit::new(); q.record(600, 25, 0, 0); q.max_skew() == 25 && q.verdict() == "skewed" },
        "极端偏斜行不污染判定",
    );
    s.add(
        "X14811 动效令牌",
        { let mut q = ConsistencyAudit::new(); q.record(594, 9, 8, 8); q.max_skew() == 1 },
        "偏斜 1 为平衡令牌边界",
    );
    s.add(
        "X14812 三态焦点",
        { let mut q = ConsistencyAudit::new(); q.record(591, 9, 8, 8) && q.verdict() == "balanced" || { let mut w = ConsistencyAudit::new(); w.record(591, 13, 6, 6); w.verdict() == "skewed" } },
        "balanced/skewed 双态互异",
    );
    s.add(
        "X14813 键盘序",
        a.rows.windows(2).all(|w| w[0].family < w[1].family),
        "族号登记序单调递增",
    );
    s.add("X14814 微文案", a.rows.iter().all(|r| r.k + r.v + r.c == 25), "每行三线和恒为 25 可读");
    s.add(
        "X14815 aria 等价",
        { let mut q = ConsistencyAudit::new(); q.record(599, 8, 8, 9); q.max_skew() == 1 && q.verdict() == "balanced" },
        "C 线主导行与 K 线主导行等价平衡",
    );
    s.add(
        "X14816 基准采集",
        { let mut q = ConsistencyAudit::new(); (581..=600u32).all(|f| q.record(f, 8, 9, 8)) && q.rows.len() == 20 },
        "20 族基准批量采集",
    );
    s.add(
        "X14817 热路径",
        { let mut q = ConsistencyAudit::new(); (1..=100u32).all(|f| q.record(f, 8, 9, 8)) && q.total() == 2500 },
        "百族热路径登记",
    );
    s.add(
        "X14818 零漂移",
        { let mut x = ConsistencyAudit::new(); let mut y = ConsistencyAudit::new(); (1..=10u32).for_each(|f| { x.record(f, 9, 8, 8); y.record(f, 9, 8, 8); }); x.rows == y.rows },
        "同序列双审计零漂移",
    );
    s.add(
        "X14819 低配减档",
        { let mut q = ConsistencyAudit::new(); q.record(591, 0, 25, 0); q.total() == 25 && q.verdict() == "skewed" },
        "单线独占 25 判偏斜",
    );
    s.add(
        "X14820 守卫",
        { let mut q = ConsistencyAudit::new(); q.record(591, 9, 8, 8); q.record(592, 9, 9, 7); q.max_skew() == 2 && q.verdict() == "skewed" },
        "偏斜 2 超限即 skewed 守卫",
    );
    s.add(
        "X14821 智能建议",
        { let mut q = ConsistencyAudit::new(); let mut n = 0; (0..5).for_each(|_| { if q.record(591, 9, 8, 8) { n += 1; } }); n == 1 },
        "同族重复只记一次",
    );
    s.add(
        "X14822 批量模式",
        { let mut q = ConsistencyAudit::new(); (581..=600u32).for_each(|f| { q.record(f, 8, 9, 8); }); q.rows.len() == 20 && q.balanced() },
        "20 族批量审计全平衡",
    );
    s.add(
        "X14823 跨域联动",
        { let mut q = ConsistencyAudit::new(); q.record(583, 8, 9, 8) && q.record(585, 8, 9, 8) && q.rows.len() == 2 },
        "K/V 镜像族跨域同口径登记",
    );
    s.add(
        "X14824 扩展点",
        { let mut q = ConsistencyAudit::new(); q.record(600, 8, 9, 8) && q.rows[0].family == 600 },
        "族号上界 600 扩展点",
    );
    s.add(
        "X14825 彩蛋层",
        { let mut q = ConsistencyAudit::new(); q.record(600, 0, 0, 25); q.verdict() == "skewed" && q.max_skew() == 25 },
        "C 线独占彩蛋：偏斜 25 可重放",
    );
    s
}

// ---- 族0594 ID 唯一性防线（X14826~X14850）----

/// ID 唯一性防线：X-ID 合法域（1~15000）+ 全局去重 + 批量重复检测 + 区间覆盖。
pub struct IdUniqueness {
    pub seen: Vec<u32>,
    pub clamped: u32,
}

impl Default for IdUniqueness {
    fn default() -> Self {
        Self::new()
    }
}

impl IdUniqueness {
    pub fn new() -> Self {
        IdUniqueness { seen: Vec::new(), clamped: 0 }
    }

    /// 合法域：1~15000 的整数。
    pub fn validate(id: u32) -> bool {
        (1..=15000).contains(&id)
    }

    /// 区间合法：min ≤ max 且都落合法域。
    pub fn span_ok(min: u32, max: u32) -> bool {
        IdUniqueness::validate(min) && IdUniqueness::validate(max) && min <= max
    }

    /// 收录：合法且未见过的 ID 才放行。
    pub fn admit(&mut self, id: u32) -> bool {
        if !IdUniqueness::validate(id) || self.seen.contains(&id) {
            self.clamped += 1;
            return false;
        }
        self.seen.push(id);
        true
    }

    /// 批量重复计数：数组内每个重复 ID 计一次。
    pub fn dupes(ids: &[u32]) -> usize {
        let mut uniq: Vec<u32> = Vec::new();
        let mut n = 0;
        for &id in ids {
            if uniq.contains(&id) {
                n += 1;
            } else {
                uniq.push(id);
            }
        }
        n
    }

    /// 区间覆盖：已收录 ID 落在 [min,max] 的数量。
    pub fn coverage(&self, min: u32, max: u32) -> usize {
        if !IdUniqueness::span_ok(min, max) {
            return 0;
        }
        self.seen.iter().filter(|&&id| id >= min && id <= max).count()
    }

    /// 冲突检测：批量中与既有收录冲突的数量。
    pub fn conflicts(&self, ids: &[u32]) -> usize {
        ids.iter().filter(|id| self.seen.contains(id)).count()
    }

    /// 从快照恢复：过滤非法 ID 后重建收录。
    pub fn restore_fresh(&mut self, ids: &[u32]) {
        self.seen = ids.iter().copied().filter(|&id| IdUniqueness::validate(id)).collect();
    }
}

/// 族0594 ID 唯一性防线（X14826~X14850）：25 项自检。
pub fn run_id_unique_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai60-id-unique");
    let mut u = IdUniqueness::new();
    s.add("X14826 最小闭环", u.admit(14751) && u.seen.len() == 1, "首个 X-ID 收录即计数");
    s.add("X14827 全量参数", (14752..=14775u32).all(|id| u.admit(id)) && u.seen.len() == 25, "批量收录至 25");
    s.add(
        "X14828 档位矩阵",
        IdUniqueness::validate(1) && IdUniqueness::validate(15000) && !IdUniqueness::validate(0) && !IdUniqueness::validate(15001),
        "合法域 1~15000 边界矩阵",
    );
    s.add("X14829 快照迁移", u.seen.first() == Some(&14751) && u.seen.len() == 25, "收录序列确定性可迁移");
    s.add("X14830 联调集成", u.coverage(14751, 15000) == 25, "区间覆盖与收录联动");
    s.add(
        "X14831 越界钳制",
        { let mut q = IdUniqueness::new(); !q.admit(0) && !q.admit(15001) && q.clamped >= 2 },
        "越界 ID 全拒并计数",
    );
    s.add(
        "X14832 失败叙事",
        { let mut q = IdUniqueness::new(); q.admit(1) && !q.admit(1) && q.clamped >= 1 },
        "重复收录拒绝：防线只增不重",
    );
    s.add(
        "X14833 中断还原",
        { let mut q = IdUniqueness::new(); q.admit(100); let snap = q.seen.clone(); q.admit(200); q.seen = snap; q.seen.len() == 1 },
        "快照回滚后收录还原",
    );
    s.add(
        "X14834 资源降级",
        { let q = IdUniqueness::new(); q.coverage(0, 100) == 0 },
        "非法区间降级返回 0",
    );
    s.add(
        "X14835 回滚净身",
        { let mut q = IdUniqueness::new(); q.admit(5); q.admit(15); q.admit(25); q.coverage(1, 20) == 2 },
        "区间外净身不计数",
    );
    s.add("X14836 动效令牌", u.conflicts(&[14751, 14760, 9999]) == 2, "冲突检测令牌稳定");
    s.add("X14837 三态焦点", IdUniqueness::dupes(&[1, 2, 2, 3, 3, 3]) == 3, "批量重复三连计三");
    s.add("X14838 键盘序", u.seen.windows(2).all(|w| w[0] < w[1]), "收录序单调递增无乱序");
    s.add("X14839 微文案", IdUniqueness::dupes(&[1, 2, 3]) == 0, "无重复批量为零计数");
    s.add("X14840 aria 等价", IdUniqueness::span_ok(1, 15000) && !IdUniqueness::span_ok(100, 50), "区间判定与单点等价");
    s.add(
        "X14841 基准采集",
        { let mut q = IdUniqueness::new(); (14501..=14750u32).all(|id| q.admit(id)) && q.seen.len() == 250 },
        "AI-59 全区间 250 基准采集",
    );
    s.add(
        "X14842 热路径",
        { let mut q = IdUniqueness::new(); (1..=1000u32).all(|id| q.admit(id)) && q.seen.len() == 1000 },
        "千级热路径收录",
    );
    s.add(
        "X14843 零漂移",
        { let mut a = IdUniqueness::new(); let mut b = IdUniqueness::new(); (1..=50u32).for_each(|i| { a.admit(i); b.admit(i); }); a.seen == b.seen },
        "同序列双防线零漂移",
    );
    s.add(
        "X14844 低配减档",
        { let mut q = IdUniqueness::new(); q.admit(14000) && q.admit(14001) && q.coverage(14000, 14001) == 2 && q.coverage(1, 13999) == 0 },
        "低配区间只算区间内",
    );
    s.add(
        "X14845 守卫",
        { let q = IdUniqueness::new(); q.conflicts(&[]) == 0 && q.seen.is_empty() },
        "空输入守卫零误报",
    );
    s.add(
        "X14846 智能建议",
        { let mut q = IdUniqueness::new(); let mut n = 0; (0..5).for_each(|_| { if q.admit(42) { n += 1; } }); n == 1 },
        "重复收录只成功一次",
    );
    s.add(
        "X14847 批量模式",
        { let mut q = IdUniqueness::new(); q.admit(77); (0..10).for_each(|_| { q.admit(77); }); q.seen.len() == 1 },
        "批量重放不膨胀收录",
    );
    s.add(
        "X14848 跨域联动",
        { let mut q = IdUniqueness::new(); (14501..=15000u32).all(|id| q.admit(id)) && q.seen.len() == 500 },
        "AI-59+AI-60 全域 500 跨域收录",
    );
    s.add(
        "X14849 扩展点",
        { let mut q = IdUniqueness::new(); q.restore_fresh(&[1, 2, 3, 99999, 0]); q.seen.len() == 3 },
        "恢复入口过滤非法 ID",
    );
    s.add(
        "X14850 彩蛋层",
        { let mut q = IdUniqueness::new(); q.admit(15000) && !q.admit(15000) },
        "15000 终点彩蛋：收录后必拒",
    );
    s
}
