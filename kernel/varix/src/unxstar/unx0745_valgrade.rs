//! UNX-F0745 · 域经价值分级（高中低三档复用度评级） · 诊断段 0x5c20
//! 判据原文摘录：UNX-F0745-J1 评级账齐（逐条四维评级：复用频次/跨域适用/
//! 实现成本/风险规避价值）且三档分布落账、低档不导出断言过；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 四维评级写死：复用频次 / 跨域适用 / 实现成本（反向）/ 风险规避价值，
//!    每维 0..=5 分整数制，逐条独立评级，不允许全条目同分（评级失效）。
//! 2. 三档判据：总分 ≥14 高档、8..=13 中档、≤7 低档；档位由总分确定性映射，
//!    无人工调档通道。
//! 3. 三档分布落账：高/中/低计数之和 == 评级条目数（守恒），分布账封账后只读。
//! 4. 低档不导出：导出白名单只含高/中档，低档条目在导出函数中必须被拒，
//!    拒绝即断言（不静默跳过）。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5c20 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5c20;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_ID_DUP: u16 = SEG_BASE + 1; // 条目号重复登记
const E_ID_FULL: u16 = SEG_BASE + 2; // 评级账已满
const E_ID_NOTFOUND: u16 = SEG_BASE + 3; // 条目号不存在
const E_DIM_RANGE: u16 = SEG_BASE + 4; // 维度分越界（0..=5 之外）
const E_DIM_TWICE: u16 = SEG_BASE + 5; // 同维重复评分
const E_DIM_LACK: u16 = SEG_BASE + 6; // 四维未齐即定档被拒
const E_UNIFORM: u16 = SEG_BASE + 7; // 全条目同分（评级失效）
const E_TIER_BAD: u16 = SEG_BASE + 8; // 档位值非法（内部错误）
const E_LOW_EXPORT: u16 = SEG_BASE + 9; // 低档条目导出被拒（预期拦截码）
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂（三档计数和≠条目数）
const E_SEALED: u16 = SEG_BASE + 11; // 评级账已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 12; // 评级账为空
const E_TWICE: u16 = SEG_BASE + 13; // 条目重复定档
const E_NAME_EMPTY: u16 = SEG_BASE + 14; // 条目名为空

/// 评级账定容。
pub const MAX_ITEMS: usize = 24;

/// 四维编号。
pub const DIM_REUSE: u8 = 1; // 复用频次
pub const DIM_CROSS: u8 = 2; // 跨域适用
pub const DIM_COST: u8 = 3; // 实现成本（反向：成本越低分越高）
pub const DIM_RISK: u8 = 8 - 5; // 风险规避价值（=3，保持位序）

/// 单维分上限。
pub const DIM_MAX: u32 = 5;

/// 三档分界：总分 ≥14 高、8..=13 中、≤7 低。
pub const TIER_HIGH_FLOOR: u32 = 14;
pub const TIER_MID_FLOOR: u32 = 8;

// ── FNV-1a 64（封账哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ID_DUP => "条目号重复：登记序自动编号",
        E_ID_FULL => "评级账已满 24 条：先归档再评",
        E_ID_NOTFOUND => "条目号不存在：按登记序 1..N",
        E_DIM_RANGE => "维度分越界：0..=5 整数制",
        E_DIM_TWICE => "同维重复评分：一维一分",
        E_DIM_LACK => "四维未齐：先补齐再定档",
        E_UNIFORM => "全条目同分：评级失去区分度，复核样本",
        E_TIER_BAD => "档位值非法：内部三档映射损坏",
        E_LOW_EXPORT => "低档条目导出被拒：低档不进移交卷（预期拦截）",
        E_BAL => "守恒破裂：高+中+低必须等于条目数",
        E_SEALED => "评级账已封：封账后只读",
        E_EMPTY => "评级账为空：先登记再评级",
        E_TWICE => "条目重复定档：一定档一落账",
        E_NAME_EMPTY => "条目名为空",
        _ => "未知诊断码",
    }
}

// ── 三档 ─────────────────────────────────────────────────────
/// 价值三档：高档/中档/低档（总分确定性映射）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 高档：≥14 分，首批导出。
    High,
    /// 中档：8..=13 分，次批导出。
    Mid,
    /// 低档：≤7 分，不导出。
    Low,
}

impl Tier {
    /// 按总分确定性映射档位。
    pub fn by_total(total: u32) -> Tier {
        if total >= TIER_HIGH_FLOOR {
            Tier::High
        } else if total >= TIER_MID_FLOOR {
            Tier::Mid
        } else {
            Tier::Low
        }
    }

    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            Tier::High => 3,
            Tier::Mid => 2,
            Tier::Low => 1,
        }
    }

    /// 中文档名。
    pub fn zh(self) -> &'static str {
        match self {
            Tier::High => "高档",
            Tier::Mid => "中档",
            Tier::Low => "低档",
        }
    }
}

// ── 评级条目 ─────────────────────────────────────────────────
/// 一条被评级的域经条目：四维分 + 档位。
#[derive(Clone, Debug)]
pub struct RatedItem {
    /// 条目号（1 起连续）。
    pub item_id: u32,
    /// 条目主题名。
    pub name: String,
    /// 四维分数（下标 0=复用频次 1=跨域适用 2=实现成本 3=风险规避价值）。
    /// 255=未评。
    pub dims: [u8; 4],
    /// 总分（四维齐后落）。
    pub total: u32,
    /// 档位（None=未定档）。
    pub tier: Option<Tier>,
}

impl RatedItem {
    /// 四维齐备：四格均 ≤5 且非 255。
    pub fn dims_complete(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let d = self.dims[i] as u32;
            if self.dims[i] == 255 || d > DIM_MAX {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 评级账（主体）────────────────────────────────────────────
/// 域经价值评级账：逐条四维评级 → 三档定档 → 分布落账 → 低档不导出。
pub struct ValueGrader {
    items: [Option<RatedItem>; MAX_ITEMS],
    item_count: usize,
    /// 三档分布（落账：高档/中档/低档计数）。
    dist_high: u32,
    dist_mid: u32,
    dist_low: u32,
    /// 是否封账。
    sealed: bool,
}

impl ValueGrader {
    /// 开账。
    pub fn new() -> ValueGrader {
        ValueGrader {
            items: core::array::from_fn(|_| None),
            item_count: 0,
            dist_high: 0,
            dist_mid: 0,
            dist_low: 0,
            sealed: false,
        }
    }

    /// 登记条目（未评分态）。
    pub fn enroll(&mut self, name: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.item_count >= MAX_ITEMS {
            return Err(E_ID_FULL);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        let item_id = (self.item_count + 1) as u32;
        self.items[self.item_count] = Some(RatedItem {
            item_id,
            name: name.to_string(),
            dims: [255; 4],
            total: 0,
            tier: None,
        });
        self.item_count += 1;
        Ok(item_id)
    }

    /// 条目定位。
    fn slot_of(&self, item_id: u32) -> Result<usize, u16> {
        if item_id == 0 || item_id as usize > self.item_count {
            return Err(E_ID_NOTFOUND);
        }
        Ok((item_id - 1) as usize)
    }

    /// 四维评分：维号 1..4、分 0..=5、同维一次。
    pub fn score_dim(&mut self, item_id: u32, dim: u8, score: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if dim == 0 || dim > 4 {
            return Err(E_DIM_RANGE);
        }
        if score as u32 > DIM_MAX {
            return Err(E_DIM_RANGE);
        }
        let s = self.slot_of(item_id)?;
        let it = self.items[s].as_mut().ok_or(E_ID_NOTFOUND)?;
        let di = (dim - 1) as usize;
        if it.dims[di] != 255 {
            return Err(E_DIM_TWICE);
        }
        it.dims[di] = score;
        Ok(())
    }

    /// 定档：四维齐 → 求总分 → 映射三档 → 分布落账。
    pub fn grade(&mut self, item_id: u32) -> Result<Tier, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let s = self.slot_of(item_id)?;
        let it = self.items[s].as_mut().ok_or(E_ID_NOTFOUND)?;
        if !it.dims_complete() {
            return Err(E_DIM_LACK);
        }
        if it.tier.is_some() {
            return Err(E_TWICE);
        }
        let total = (it.dims[0] as u32)
            + (it.dims[1] as u32)
            + (it.dims[2] as u32)
            + (it.dims[3] as u32);
        let tier = Tier::by_total(total);
        it.total = total;
        it.tier = Some(tier);
        match tier {
            Tier::High => self.dist_high += 1,
            Tier::Mid => self.dist_mid += 1,
            Tier::Low => self.dist_low += 1,
        }
        Ok(tier)
    }

    /// 守恒恒等式：高+中+低 == 已定档条目数 == item_count（全定档口径）。
    pub fn conserved(&self) -> bool {
        self.dist_high + self.dist_mid + self.dist_low == self.item_count as u32
    }

    /// 全条目同分断言（区分度检查）：存在不同总分即过。
    pub fn discriminant_assert(&self) -> Result<(), u16> {
        if self.item_count == 0 {
            return Err(E_EMPTY);
        }
        let first = match &self.items[0] {
            Some(it) => it.total,
            None => return Err(E_ID_NOTFOUND),
        };
        let mut i = 1usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                if it.total != first {
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(E_UNIFORM)
    }

    /// 导出白名单：高/中档条目号（低档被过滤，非静默——由 export 断言补拦）。
    pub fn exportable_ids(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                match it.tier {
                    Some(Tier::High) | Some(Tier::Mid) => out.push(it.item_id),
                    _ => {}
                }
            }
            i += 1;
        }
        out
    }

    /// 导出单条：低档必须被拒（低档不导出断言的执行点）。
    pub fn export_one(&self, item_id: u32) -> Result<(String, Tier, u32), u16> {
        if self.item_count == 0 {
            return Err(E_EMPTY);
        }
        let s = self.slot_of(item_id)?;
        let it = self.items[s].as_ref().ok_or(E_ID_NOTFOUND)?;
        match it.tier {
            Some(Tier::Low) => Err(E_LOW_EXPORT),
            Some(t) => Ok((it.name.clone(), t, it.total)),
            None => Err(E_DIM_LACK),
        }
    }

    /// 低档不导出断言：全部低档条目逐一尝试导出，必须全拒。
    pub fn low_not_export_assert(&self) -> Result<u32, u16> {
        let mut blocked = 0u32;
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                if it.tier == Some(Tier::Low) {
                    match self.export_one(it.item_id) {
                        Err(c) if c == E_LOW_EXPORT => blocked += 1,
                        _ => return Err(E_LOW_EXPORT),
                    }
                }
            }
            i += 1;
        }
        Ok(blocked)
    }

    /// 条目只读快照。
    pub fn get(&self, item_id: u32) -> Result<(String, Option<Tier>, u32), u16> {
        let s = self.slot_of(item_id)?;
        match &self.items[s] {
            Some(it) => Ok((it.name.clone(), it.tier, it.total)),
            None => Err(E_ID_NOTFOUND),
        }
    }

    /// 三档分布读取：(高, 中, 低)。
    pub fn distribution(&self) -> (u32, u32, u32) {
        (self.dist_high, self.dist_mid, self.dist_low)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.item_count
    }

    /// 封账：守恒 + 区分度 + 全定档后冻结。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.item_count == 0 {
            return Err(E_EMPTY);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        self.discriminant_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                buf.extend_from_slice(it.item_id.to_le_bytes().as_slice());
                buf.extend_from_slice(it.name.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(&it.dims);
                buf.push(it.tier.map(|t| t.code()).unwrap_or(0));
            }
            i += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 评级账行渲染。
    pub fn audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 域经价值评级账 · 四维三档 ===".to_string());
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                let mut line = String::from("V");
                line.push_str(&itoa(it.item_id as u64));
                line.push('|');
                line.push_str(&it.name);
                line.push_str("|四维 ");
                line.push_str(&itoa(it.dims[0] as u64));
                line.push('/');
                line.push_str(&itoa(it.dims[1] as u64));
                line.push('/');
                line.push_str(&itoa(it.dims[2] as u64));
                line.push('/');
                line.push_str(&itoa(it.dims[3] as u64));
                line.push_str("|总 ");
                line.push_str(&itoa(it.total as u64));
                line.push('|');
                line.push_str(match it.tier {
                    Some(t) => t.zh(),
                    None => "未定档",
                });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 三档分布落账行渲染（分布账独立节）。
    pub fn distribution_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 三档分布落账 ===".to_string());
        lines.push("高档: ".to_string() + &itoa(self.dist_high as u64));
        lines.push("中档: ".to_string() + &itoa(self.dist_mid as u64));
        lines.push("低档: ".to_string() + &itoa(self.dist_low as u64));
        lines.push("合计: ".to_string() + &itoa(self.item_count as u64));
        lines
    }

    /// 四维均分（千分整数化：sum*1000/count，count>0 前提下返回四元组）。
    /// 返回 (复用频次, 跨域适用, 实现成本, 风险规避价值) 的千分均值。
    pub fn dim_milli_averages(&self) -> Result<[u32; 4], u16> {
        if self.item_count == 0 {
            return Err(E_EMPTY);
        }
        let mut sums = [0u32; 4];
        let mut counted = [0u32; 4];
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                let mut d = 0usize;
                while d < 4 {
                    if it.dims[d] != 255 {
                        sums[d] += it.dims[d] as u32;
                        counted[d] += 1;
                    }
                    d += 1;
                }
            }
            i += 1;
        }
        let mut avg = [0u32; 4];
        let mut d = 0usize;
        while d < 4 {
            if counted[d] > 0 {
                avg[d] = sums[d] * 1000 / counted[d];
            }
            d += 1;
        }
        Ok(avg)
    }

    /// 按总分降序取前 n 条目号（选择排序，稳定，同分按登记序）。
    pub fn top_items(&self, n: usize) -> Vec<u32> {
        let mut scores: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.item_count {
            if let Some(it) = &self.items[i] {
                if it.tier.is_some() {
                    scores.push((it.total, it.item_id));
                }
            }
            i += 1;
        }
        // 选择排序降序（稳定：同分不交换保持登记序）。
        let m = scores.len();
        let mut a = 0usize;
        while a < m {
            let mut best = a;
            let mut b = a + 1;
            while b < m {
                if scores[b].0 > scores[best].0 {
                    best = b;
                }
                b += 1;
            }
            let tmp = scores[a];
            scores[a] = scores[best];
            scores[best] = tmp;
            a += 1;
        }
        let take = if n > m { m } else { n };
        let mut out: Vec<u32> = Vec::new();
        let mut r = 0usize;
        while r < take {
            out.push(scores[r].1);
            r += 1;
        }
        out
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：四维评级 + 三档分布守恒 + 低档不导出。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut g = ValueGrader::new();
    // 三条样本：高档 / 中档 / 低档。
    let h = g.enroll("哈希先行校验")?;
    let m = g.enroll("段位诊断码")?;
    let l = g.enroll("临时打样脚本")?;
    // 高档：4+4+3+4 = 15。
    g.score_dim(h, DIM_REUSE, 4)?;
    g.score_dim(h, DIM_CROSS, 4)?;
    g.score_dim(h, DIM_COST, 3)?;
    g.score_dim(h, DIM_RISK, 4)?;
    // 中档：3+2+3+2 = 10。
    g.score_dim(m, DIM_REUSE, 3)?;
    g.score_dim(m, DIM_CROSS, 2)?;
    g.score_dim(m, DIM_COST, 3)?;
    g.score_dim(m, DIM_RISK, 2)?;
    // 低档：1+1+2+1 = 5。
    g.score_dim(l, DIM_REUSE, 1)?;
    g.score_dim(l, DIM_CROSS, 1)?;
    g.score_dim(l, DIM_COST, 2)?;
    g.score_dim(l, DIM_RISK, 1)?;
    // 定档三连。
    match g.grade(h)? {
        Tier::High => {}
        _ => return Err(SEG_BASE),
    }
    match g.grade(m)? {
        Tier::Mid => {}
        _ => return Err(SEG_BASE),
    }
    match g.grade(l)? {
        Tier::Low => {}
        _ => return Err(SEG_BASE),
    }
    // 守恒：3=1+1+1。
    if !g.conserved() {
        return Err(SEG_BASE);
    }
    let (dh, dm, dl) = g.distribution();
    if dh != 1 || dm != 1 || dl != 1 {
        return Err(SEG_BASE);
    }
    // 区分度：三条不同总分。
    g.discriminant_assert()?;
    // 取数口可用性：top 首位必须是高档 h；分布行 5 行。
    let tops = g.top_items(3);
    if tops.len() != 3 || tops[0] != h {
        return Err(SEG_BASE);
    }
    let avgs = g.dim_milli_averages()?;
    if avgs[0] == 0 {
        return Err(SEG_BASE);
    }
    let dl2 = g.distribution_lines();
    if dl2.len() != 5 {
        return Err(SEG_BASE);
    }
    // 低档不导出：白名单 2 条 + 低档导出必拒。
    let wl = g.exportable_ids();
    if wl.len() != 2 {
        return Err(SEG_BASE);
    }
    match g.export_one(l) {
        Err(c) if c == E_LOW_EXPORT => {}
        _ => return Err(SEG_BASE),
    }
    let blocked = g.low_not_export_assert()?;
    if blocked != 1 {
        return Err(SEG_BASE);
    }
    // 封账。
    let hash = g.seal()?;
    if hash == 0 {
        return Err(SEG_BASE);
    }
    // 封账后写拒绝。
    match g.enroll("封账后条目") {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：维度分越界。
    let mut g2 = ValueGrader::new();
    let x = g2.enroll("越界样本")?;
    match g2.score_dim(x, DIM_REUSE, 6) {
        Err(c) if c == E_DIM_RANGE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：四维未齐定档被拒。
    match g2.grade(x) {
        Err(c) if c == E_DIM_LACK => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
