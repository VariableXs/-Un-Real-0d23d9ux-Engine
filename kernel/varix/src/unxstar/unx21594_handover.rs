//! UNX-F21594 · 域交接与深化移交（AI-90 排程）
//!
//! 诊断段 0x91069~0x91070（F2-B40 域收官段 14/100）。
//!
//! 判据原文（施工书 UNX-F21594-J1）：移交清单：深化 B01–B40 排程建议，接口冻结面清单。
//!
//! 设计要点（为什么判决只有两位、成因另立枚举）：
//! 本单码段只给两位（0x91069 可交 / 0x91070 不可交），而「不可交」的成因有八种。
//! 八种都编成码会与邻单码段抢号；只报一个裸 0x91070，运维还得逐条翻清单才知道坏在
//! 哪。故分层：**码回答「这份清单能不能交出去」，成因回答「交给谁去修」**。
//!
//! 排程用**稳定插入排序**而非全序比较：题目只给两级键（冻结依赖优先、未收口缺陷
//! 优先）。若再拿批号当 tie-break，「同输入同序」就成了同义反复，而并列项的真实顺序
//! 本该由**登记顺序**决定（那是人工排定的责任顺序）。稳定排序把这条信息留住，也让
//! 「换输入顺序 → 换并列顺序」成为可观测判据。
//!
//! 冻结面按**批**聚合而非按承接方：冻结的含义是「这批接口在本批收口前不许动」，责任
//! 边界在批。按承接方聚合会把两批接口并成一个集合，冻结范围被悄悄放大。
//!
//! `frozen=true` 而接口集为空是「空承诺」：不该当冻结去抢排程队首（那会挤掉真有依赖
//! 的批），也不该报错（那会让整本台账无法归档）。故归一化——`freeze_committed()`
//! 要求标记与接口集同时成立，空承诺降级为普通条目。交接清单的用途是「能交出去」。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 域内批数（施工书 B01–B40）。
pub const BATCH_SPAN: u16 = 40;
/// 移交清单容量（40 批 × 2 条并行建议 + 溢出样本空间）。
pub const ROSTER_CAP: usize = 96;
/// 单批依赖接口上限（超过说明这批没做接口收口）。
pub const IFACE_CAP: usize = 6;
/// 深化优先级下界（1 最高）。
pub const PRIORITY_MIN: u8 = 1;
/// 深化优先级上界（5 最低）。
pub const PRIORITY_MAX: u8 = 5;
/// FNV-1a 64 偏移基（来源：FNV 规范 1a 变体）。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 质数（同上）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 交接诊断码（本单码段仅两位）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入新变体不能让旧日志失配。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverCode {
    Ok = 0x91069,
    Blocked = 0x91070,
}

impl HandoverCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            HandoverCode::Ok => "移交清单可交：四十批全覆盖且无互斥建议",
            HandoverCode::Blocked => "移交清单不可交：存在覆盖缺口或互斥建议",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            HandoverCode::Ok => "每批至少一条建议，且同批冻结面承接方唯一",
            HandoverCode::Blocked => "缺批、双冻结承接方或条目字段越界",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            HandoverCode::Ok => "归档清单并转入 AI-90 深化排程",
            HandoverCode::Blocked => "按成因逐条补齐或拆单后重新审计",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断成 0x1069 而与低段码撞车）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 不可交成因（八元闭集，定位粒度在此而不在码上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Taint {
    BatchOutOfRange,
    DuplicateSlot,
    RosterFull,
    CoverageGap,
    OwnerMissing,
    PriorityOutOfRange,
    MutexPair,
    IfaceOverflow,
}

impl Taint {
    /// 定位文本：一段话读完就知道「哪儿坏了、为什么算坏」。
    ///
    /// 成因不是诊断码，三要素由 `HandoverCode` 承载；这里合成句子直接进报告，
    /// 免得每处调用各写一份措辞、措辞还互不一致。
    pub const fn what(self) -> &'static str {
        match self {
            Taint::BatchOutOfRange => "批号越出 B01–B40：域外批号无处移交",
            Taint::DuplicateSlot => "批内序号重复：同一槽位被两条建议占用",
            Taint::RosterFull => "清单已满：第 97 条建议无处落账",
            Taint::CoverageGap => "覆盖缺口：该批没有任何移交建议",
            Taint::OwnerMissing => "承接方为空：没人接手就没有移交",
            Taint::PriorityOutOfRange => "深化优先级越界：不在 1..=5",
            Taint::MutexPair => "互斥建议：同批两条冻结建议指向不同承接方",
            Taint::IfaceOverflow => "依赖接口超限：单批超过 6 个接口",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            Taint::BatchOutOfRange => "批号改回 1..=40，域外内容另立交接单",
            Taint::DuplicateSlot => "给并行建议换一个批内序号",
            Taint::RosterFull => "先归并同批同类建议，再补登记",
            Taint::CoverageGap => "为该批补一条建议，或写明无需深化",
            Taint::OwnerMissing => "填上承接方工号后重新登记",
            Taint::PriorityOutOfRange => "按 1..=5 重新评定深化优先级",
            Taint::MutexPair => "冻结面只留一个承接方，另一条改为非冻结",
            Taint::IfaceOverflow => "先做接口收口把依赖压到 6 个以内",
        }
    }
}

/// 交接故障（`Err` 侧：只表达「这条建议进不了清单」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fault {
    /// 诊断码（u32：五位数码段，u16 会截断）。
    pub code: u32,
    pub taint: Taint,
    pub batch: u16,
}

impl Fault {
    const fn new(taint: Taint, batch: u16) -> Fault {
        Fault { code: HandoverCode::Blocked.code(), taint, batch }
    }
}

/// 一条移交建议：批号 / 批内序号 / 深化优先级 / 承接方 / 接口依赖 / 两个标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub batch: u16,
    pub serial: u16,
    pub priority: u8,
    pub owner: &'static str,
    pub ifaces: &'static [&'static str],
    pub frozen: bool,
    pub open_defect: bool,
}

impl Entry {
    /// 构造一条建议（不校验；校验集中在 `Roster::add`）。
    pub const fn new(
        batch: u16,
        serial: u16,
        priority: u8,
        owner: &'static str,
        ifaces: &'static [&'static str],
        frozen: bool,
        open_defect: bool,
    ) -> Entry {
        Entry { batch, serial, priority, owner, ifaces, frozen, open_defect }
    }

    /// 冻结承诺是否成立（标记 + 非空接口集，缺一不算）。
    pub fn freeze_committed(&self) -> bool {
        self.frozen && !self.ifaces.is_empty()
    }

    /// 排程键：冻结依赖优先、未收口缺陷优先，小者靠前。
    ///
    /// 刻意只有两级：把批号塞进来会得到一个平凡全序，稳定排序就再也测不出东西。
    fn rank(&self) -> (u8, u8) {
        (if self.freeze_committed() { 0 } else { 1 }, if self.open_defect { 0 } else { 1 })
    }

    /// 导出行：`b<批号>|s<序号>|p<优先级>|<承接方>|<接口,接口>|<F/O 标记>`。
    pub fn line(&self) -> String {
        let mut out = String::with_capacity(48);
        out.push('b');
        out.push_str(&self.batch.to_string());
        out.push_str("|s");
        out.push_str(&self.serial.to_string());
        out.push_str("|p");
        out.push_str(&self.priority.to_string());
        out.push('|');
        out.push_str(self.owner);
        out.push('|');
        out.push_str(&join_ifaces(self.ifaces));
        out.push('|');
        out.push(if self.frozen { 'F' } else { '-' });
        out.push(if self.open_defect { 'O' } else { '-' });
        out
    }
}

/// 逗号拼接接口集（`join` 要引 alloc 的 Join 特征，手写反而少一处隐藏依赖）。
fn join_ifaces(ifaces: &[&'static str]) -> String {
    let mut out = String::with_capacity(ifaces.len() * 12);
    for (k, name) in ifaces.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str(name);
    }
    out
}

/// 一条成因记录（审计用，带批号便于直接定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaintRec {
    pub taint: Taint,
    pub batch: u16,
}

/// 全量审计结论。
///
/// 覆盖用 **u64 位图**而不是 `Vec<bool>`：40 批是定长闭集，位图让「是否覆盖」退化成
/// 一次位移，`covered()` 只需一次 popcount，不必再遍历一遍。
pub struct Audit {
    coverage: u64,
    taints: Vec<TaintRec>,
}

impl Audit {
    /// 判决码。
    pub fn code(&self) -> HandoverCode {
        if self.taints.is_empty() {
            HandoverCode::Ok
        } else {
            HandoverCode::Blocked
        }
    }
    /// 已覆盖批数。
    pub fn covered(&self) -> u32 {
        self.coverage.count_ones()
    }
    /// 某批是否被覆盖。
    pub fn has_batch(&self, batch: u16) -> bool {
        batch >= 1 && batch <= BATCH_SPAN && self.coverage & (1u64 << (batch - 1)) != 0
    }
    /// 成因清单（按批号升序，同批按成因枚举序）。
    pub fn taints(&self) -> &[TaintRec] {
        &self.taints
    }
    /// 某成因出现次数。
    pub fn taint_count(&self, taint: Taint) -> usize {
        self.taints.iter().filter(|t| t.taint == taint).count()
    }
}

/// 冻结面一行：一批 + 该批冻结的接口集（去重、字典序）。
pub struct SurfaceRow {
    pub batch: u16,
    pub ifaces: Vec<&'static str>,
}

/// 接口冻结面清单（按批聚合）。
pub struct Surface {
    rows: Vec<SurfaceRow>,
}

impl Surface {
    /// 取某批的冻结面。
    pub fn of(&self, batch: u16) -> Option<&SurfaceRow> {
        self.rows.iter().find(|r| r.batch == batch)
    }
    /// 有冻结面的批数。
    pub fn batch_count(&self) -> usize {
        self.rows.len()
    }
    /// 冻结接口总数（跨批累加，含跨批重复）。
    pub fn iface_total(&self) -> usize {
        self.rows.iter().map(|r| r.ifaces.len()).sum()
    }
    /// 可归档文本（每行 `b<批号> <接口,接口>`）。
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(256);
        for row in &self.rows {
            out.push('b');
            out.push_str(&row.batch.to_string());
            out.push(' ');
            out.push_str(&join_ifaces(&row.ifaces));
            out.push('\n');
        }
        out
    }
}

/// 移交清单。
pub struct Roster {
    entries: Vec<Entry>,
}

impl Roster {
    /// 新建空清单。
    pub const fn new() -> Roster {
        Roster { entries: Vec::new() }
    }

    /// 登记一条建议。
    ///
    /// 闸序按「便宜先行」：容量 → 批号 → 字段 → 去重扫描。前三条纯局部判断，
    /// 去重要扫全表（O(n)），放最后以免坏条目白扫一遍。
    pub fn add(&mut self, e: Entry) -> Result<(), Fault> {
        if self.entries.len() >= ROSTER_CAP {
            return Err(Fault::new(Taint::RosterFull, e.batch));
        }
        if e.batch < 1 || e.batch > BATCH_SPAN {
            return Err(Fault::new(Taint::BatchOutOfRange, e.batch));
        }
        if e.owner.is_empty() {
            return Err(Fault::new(Taint::OwnerMissing, e.batch));
        }
        if e.priority < PRIORITY_MIN || e.priority > PRIORITY_MAX {
            return Err(Fault::new(Taint::PriorityOutOfRange, e.batch));
        }
        if e.ifaces.len() > IFACE_CAP {
            return Err(Fault::new(Taint::IfaceOverflow, e.batch));
        }
        for x in &self.entries {
            if x.batch == e.batch && x.serial == e.serial {
                return Err(Fault::new(Taint::DuplicateSlot, e.batch));
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否空清单。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 按登记序取第 i 条（越界返回 None，不 panic）。
    pub fn at(&self, i: usize) -> Option<Entry> {
        self.entries.get(i).copied()
    }

    /// 全量审计：覆盖位图 → 缺口 → 互斥。
    ///
    /// 缺口必须排在互斥之前全部记下：某批既缺建议又「看似有互斥」是不可能的，
    /// 但若先判互斥，调用方会看到「有互斥」而误以为该批已经有建议。
    pub fn audit(&self) -> Audit {
        let mut coverage = 0u64;
        for e in &self.entries {
            if e.batch >= 1 && e.batch <= BATCH_SPAN {
                coverage |= 1u64 << (e.batch - 1);
            }
        }
        let mut taints: Vec<TaintRec> = Vec::new();
        for b in 1..=BATCH_SPAN {
            if coverage & (1u64 << (b - 1)) == 0 {
                taints.push(TaintRec { taint: Taint::CoverageGap, batch: b });
            }
        }
        // 互斥：同批两条已承诺冻结的建议，承接方不同就有一处无主。
        // 每批最多记一次——三条互斥是同一个病，记三次只让报告变长。
        for b in 1..=BATCH_SPAN {
            let mut first: &'static str = "";
            let mut seen = false;
            for e in &self.entries {
                if e.batch == b && e.freeze_committed() {
                    if !seen {
                        first = e.owner;
                        seen = true;
                    } else if e.owner != first {
                        taints.push(TaintRec { taint: Taint::MutexPair, batch: b });
                        break;
                    }
                }
            }
        }
        Audit { coverage, taints }
    }

    /// 排程建议：两级键 + 稳定插入排序。
    ///
    /// 只在**严格更优**时前移（`rank < rank`），相等就让位——这正是稳定性的定义。
    /// 写成 `<=` 会把并列项翻成逆序：两次运行仍「一致」，却已是另一份排程。
    pub fn schedule(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = self.entries.clone();
        for i in 1..out.len() {
            let mut j = i;
            while j > 0 && out[j].rank() < out[j - 1].rank() {
                out.swap(j, j - 1);
                j -= 1;
            }
        }
        out
    }

    /// 接口冻结面清单：逐批聚合已承诺冻结的接口集（去重 + 字典序）。
    ///
    /// 去重要紧：同批两条建议依赖同一接口时，冻结面里出现两次会让下游以为要锁两个
    /// 接口。字典序是为报告可 diff——集合相同而顺序不同，每次归档都产生噪声差异。
    pub fn freeze_surface(&self) -> Surface {
        let mut rows: Vec<SurfaceRow> = Vec::new();
        for b in 1..=BATCH_SPAN {
            let mut set: Vec<&'static str> = Vec::new();
            for e in &self.entries {
                if e.batch == b && e.freeze_committed() {
                    for name in e.ifaces {
                        if !set.contains(name) {
                            set.push(name);
                        }
                    }
                }
            }
            if !set.is_empty() {
                // 已去重，不存在相等元素，故非稳定排序也给出确定序。
                set.sort_unstable();
                rows.push(SurfaceRow { batch: b, ifaces: set });
            }
        }
        Surface { rows }
    }

    /// 清单指纹：按**登记顺序**咬合全部字段。
    ///
    /// 不先排序再算：排程依赖登记顺序做稳定 tie-break，顺序变了排程就变了；
    /// 指纹若对顺序不敏感，就查不出「清单被重排过」这种真问题。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        for e in &self.entries {
            h = fnv_u64(h, e.batch as u64);
            h = fnv_u64(h, e.serial as u64);
            h = fnv_u64(h, e.priority as u64);
            h = fnv_u64(h, if e.frozen { 1 } else { 0 });
            h = fnv_u64(h, if e.open_defect { 1 } else { 0 });
            h = fnv_str(h, e.owner);
            for name in e.ifaces {
                h = fnv_str(h, name);
            }
        }
        h
    }

    /// 导出清单文本：按 (批号, 批内序号) 排序，一行一条。
    ///
    /// 导出与指纹刻意用两种顺序：指纹守「登记顺序」（责任顺序），导出给人看要的是
    /// 「批号顺序」（查阅顺序）。两者都确定，但服务于不同读者，混用会丢信息。
    pub fn export(&self) -> String {
        let mut rows: Vec<Entry> = self.entries.clone();
        rows.sort_unstable_by(|a, b| a.batch.cmp(&b.batch).then(a.serial.cmp(&b.serial)));
        let mut out = String::with_capacity(rows.len() * 48);
        for e in &rows {
            out.push_str(&e.line());
            out.push('\n');
        }
        out
    }
}

/// FNV-1a 64 单步。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= ((v >> (i * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 字符串咬合（尾部补 0xFF 作分隔）。
///
/// 必须补分隔符：否则 ("ab","c") 与 ("a","bc") 产生同一条字节流，指纹撞车。
/// 0xFF 在合法 UTF-8 里永不出现，不可能与真实字节混淆。
fn fnv_str(mut h: u64, s: &str) -> u64 {
    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h ^= 0xFF;
    h.wrapping_mul(FNV_PRIME)
}

/// 承接方表（AI-90 深化分队八个工号）。
pub const OWNERS: [&str; 8] =
    ["ai-90", "ai-91", "ai-92", "ai-93", "ai-94", "ai-95", "ai-96", "ai-97"];
/// 空接口集（边界样本）。
pub const IF0: [&str; 0] = [];
/// 一个接口。
pub const IF1: [&str; 1] = ["core.irq"];
/// 两个接口。
pub const IF2: [&str; 2] = ["mem.pool", "sched.run"];
/// 三个接口（乱序写入，靠 `freeze_surface` 收敛）。
pub const IF3: [&str; 3] = ["vfs.node", "net.q", "time.tick"];
/// 上限 6 个接口（边界内最大合法值）。
pub const IF6: [&str; 6] = ["i1", "i2", "i3", "i4", "i5", "i6"];
/// 上限 + 1（7 个，必须被拒）。
pub const IF7: [&str; 7] = ["i1", "i2", "i3", "i4", "i5", "i6", "i7"];

/// 标准条目：批号唯一决定优先级 / 承接方 / 接口集 / 两个标记。
///
/// 让批号当唯一自变量，夹具才有「同输入同序」可言。冻结面（批号非 4 的倍数）与
/// 缺陷标记（批号 3 的倍数）由两个不同的模数决定，两级排序键才有真实分层。
pub fn standard_entry(batch: u16) -> Entry {
    let idx = (batch as usize).saturating_sub(1);
    let ifaces: &'static [&'static str] = match batch % 4 {
        1 => &IF1[..],
        2 => &IF2[..],
        3 => &IF3[..],
        _ => &IF0[..],
    };
    let priority = ((idx % 5) as u8) + PRIORITY_MIN;
    Entry::new(batch, 1, priority, OWNERS[idx % OWNERS.len()], ifaces, batch % 4 != 0, batch % 3 == 0)
}

/// 标准移交清单（40 批各一条，判据夹具）。
///
/// 返回 Result 而非直接构造：夹具自带非法条目时 `?` 当场暴露，而不是让后续十几条
/// 判据全跑在「夹具其实不成立」的沙地上。
pub fn standard_roster() -> Result<Roster, Fault> {
    let mut r = Roster::new();
    for b in 1..=BATCH_SPAN {
        r.add(standard_entry(b))?;
    }
    Ok(r)
}

/// 判据侧独立排程键（故意不复用 `Entry::rank`）。
///
/// 判据要能证伪实现：两边共用同一个键函数时，键算错了判据跟着错，判据就退化成
/// 「实现自己证自己」。
fn check_rank(e: &Entry) -> (u8, u8) {
    let f = if e.frozen && !e.ifaces.is_empty() { 0u8 } else { 1u8 };
    (f, if e.open_defect { 0u8 } else { 1u8 })
}

/// 取拒绝结果的 (码, 成因)，供反向判据断具体码。
fn rejected(r: Result<(), Fault>) -> Option<(u32, Taint)> {
    r.err().map(|f| (f.code, f.taint))
}

/// 判据入口（UNX-F21594-J1 及其配套十二条）。
///
/// 判据常量独立写死：`40` / `60` / `0x91069` / `0x91070` 在断言里以字面量出现，
/// 避免「实现改了常量，判据跟着改」的自证循环。
pub fn run_unx21594_handover_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21594");
    let standard = standard_roster();
    let audit = standard.as_ref().ok().map(|r| r.audit());
    let sched = standard.as_ref().ok().map(|r| r.schedule());
    let surf = standard.as_ref().ok().map(|r| r.freeze_surface());
    let oor = Some((0x91070u32, Taint::BatchOutOfRange));

    // 1) 夹具可构造：先证夹具，再证逻辑（夹具不成立时后续判据全是空转）。
    cs.add(
        "f21594.fixture_buildable",
        standard.as_ref().map_or(0, |r| r.len()) == 40
            && matches!(&audit, Some(a) if a.covered() == 40 && a.code() == HandoverCode::Ok),
        "标准清单 40 条不全、未全覆盖或自检不干净",
    );

    // 2) 正向 + 码表钉死：两位码值不许漂移（跨模块日志对账键），三要素非空。
    cs.add(
        "f21594.positive_clean",
        matches!(&audit, Some(a) if a.taints().is_empty() && a.code().code() == 0x91069)
            && HandoverCode::Blocked.code() == 0x91070
            && !HandoverCode::Ok.what().is_empty()
            && !HandoverCode::Ok.why().is_empty()
            && !HandoverCode::Ok.advice().is_empty()
            && !HandoverCode::Blocked.what().is_empty()
            && !HandoverCode::Blocked.why().is_empty()
            && !HandoverCode::Blocked.advice().is_empty(),
        "标准清单未判可交，或码不是 0x91069/0x91070",
    );

    // 3) 两级排程：队首十批必须是「冻结 + 未收口」那十批，且全程键不增。
    cs.add(
        "f21594.schedule_two_level",
        sched.as_ref().map_or(false, |s| {
            let head: Vec<u16> = s.iter().take(10).map(|e| e.batch).collect();
            let mono = s.windows(2).all(|w| check_rank(&w[0]) <= check_rank(&w[1]));
            s.len() == 40 && mono && head == vec![3u16, 6, 9, 15, 18, 21, 27, 30, 33, 39]
        }),
        "队首十批不符（应 3,6,9,15,18,21,27,30,33,39），或键序倒挂",
    );

    // 4) 稳定 + 确定性：并列项随登记顺序（不随批号），且同输入两次逐条相同。
    {
        let a = Entry::new(4, 1, 2, "ai-90", &IF0[..], false, false);
        let b = Entry::new(8, 1, 3, "ai-91", &IF0[..], false, false);
        let mut r1 = Roster::new();
        let mut r2 = Roster::new();
        let built = r1.add(a).is_ok() && r1.add(b).is_ok() && r2.add(b).is_ok() && r2.add(a).is_ok();
        let (s1, s2) = (r1.schedule(), r2.schedule());
        let stable = s1.len() == 2
            && s2.len() == 2
            && s1[0].batch == 4
            && s1[1].batch == 8
            && s2[0].batch == 8
            && s2[1].batch == 4;
        let repeat = match (standard_roster(), standard_roster()) {
            (Ok(x), Ok(y)) => x.schedule() == y.schedule() && x.fingerprint() == y.fingerprint(),
            _ => false,
        };
        cs.add(
            "f21594.schedule_stable",
            built && stable && repeat,
            "并列项未保留登记顺序，或同输入两次排程不一致",
        );
    }

    // 5) 冻结面：按批聚合、去重、字典序；接口总数 60（10×1 + 10×2 + 10×3）。
    cs.add(
        "f21594.freeze_surface_grouped",
        surf.as_ref().map_or(false, |s| {
            let row3 = s
                .of(3)
                .map_or(false, |r| r.ifaces.as_slice() == ["net.q", "time.tick", "vfs.node"]);
            s.batch_count() == 30
                && s.iface_total() == 60
                && row3
                && s.of(4).is_none()
                && s.render().contains("b3 net.q,time.tick,vfs.node")
        }),
        "冻结面未按批聚合：批数≠30 / 接口数≠60 / 批 3 未排序去重",
    );

    // 6) 反向：缺第 40 批必判缺口（0x91070 + CoverageGap），位图不含该批。
    {
        let mut r = Roster::new();
        let mut built = true;
        for b in 1..BATCH_SPAN {
            if r.add(standard_entry(b)).is_err() {
                built = false;
                break;
            }
        }
        let a = r.audit();
        let hit = a.taints().iter().any(|t| t.taint == Taint::CoverageGap && t.batch == 40);
        cs.add(
            "f21594.coverage_gap_flagged",
            built
                && a.code().code() == 0x91070
                && a.covered() == 39
                && hit
                && !a.has_batch(40)
                && a.taint_count(Taint::CoverageGap) == 1,
            Taint::CoverageGap.advice(),
        );
    }

    // 7) 反向：同批两条冻结建议承接方不同 → 互斥；非冻结并行不算互斥。
    {
        let mut r = match standard_roster() {
            Ok(x) => x,
            Err(_) => Roster::new(),
        };
        let soft = Entry::new(7, 2, 2, "ai-90", &IF0[..], false, false);
        let add_soft = r.add(soft).is_ok();
        let still_clean = r.audit().code() == HandoverCode::Ok;
        let add_hard = r.add(Entry::new(7, 3, 2, "ai-90", &IF2[..], true, false)).is_ok();
        let a = r.audit();
        let hit = a.taints().iter().any(|t| t.taint == Taint::MutexPair && t.batch == 7);
        cs.add(
            "f21594.mutex_pair_flagged",
            add_soft
                && still_clean
                && add_hard
                && hit
                && a.code().code() == 0x91070
                && a.taint_count(Taint::MutexPair) == 1,
            Taint::MutexPair.what(),
        );
    }

    // 8) 边界：批号 0 / 41 拒，1 / 40 收。
    {
        let mut r = Roster::new();
        let low = rejected(r.add(standard_entry(0)));
        let high = rejected(r.add(standard_entry(41)));
        cs.add(
            "f21594.batch_range",
            low == oor
                && high == oor
                && r.add(standard_entry(1)).is_ok()
                && r.add(standard_entry(40)).is_ok(),
            "批号 0/41 未被拒，或端点 1/40 被误拒",
        );
    }

    // 9) 反向：同批同序号重复登记被拒；换序号即收。
    {
        let mut r = Roster::new();
        let first = r.add(standard_entry(5)).is_ok();
        let dup = rejected(r.add(standard_entry(5)));
        let alt = r.add(Entry::new(5, 2, 3, "ai-90", &IF0[..], false, false)).is_ok();
        cs.add(
            "f21594.duplicate_slot",
            first && dup == Some((0x91070, Taint::DuplicateSlot)) && alt && r.len() == 2,
            Taint::DuplicateSlot.advice(),
        );
    }

    // 10) 边界：96 条恰满可收，第 97 条必拒（上限 / 上限+1）。
    {
        let mut r = Roster::new();
        let mut ok = true;
        for n in 1..=BATCH_SPAN {
            if r.add(standard_entry(n)).is_err() {
                ok = false;
                break;
            }
        }
        for k in 0u16..56 {
            let e = Entry::new((k % 40) + 1, 2 + (k / 40), 1, "ai-90", &IF0[..], false, false);
            if !ok || r.add(e).is_err() {
                ok = false;
                break;
            }
        }
        let over = rejected(r.add(Entry::new(1, 9, 1, "ai-90", &IF0[..], false, false)));
        cs.add(
            "f21594.roster_capacity",
            ok && r.len() == 96 && over == Some((0x91070, Taint::RosterFull)),
            "96 条未收满，或第 97 条未报 RosterFull",
        );
    }

    // 11) 边界：优先级 1/5 收、0/6 拒；空承接方拒；6 接口收、7 接口拒。
    {
        let mut r = Roster::new();
        let p0 = rejected(r.add(Entry::new(1, 1, 0, "ai-90", &IF0[..], false, false)));
        let p6 = rejected(r.add(Entry::new(1, 1, 6, "ai-90", &IF0[..], false, false)));
        let p1 = r.add(Entry::new(1, 1, 1, "ai-90", &IF0[..], false, false)).is_ok();
        let p5 = r.add(Entry::new(1, 2, 5, "ai-90", &IF0[..], false, false)).is_ok();
        let nobody = rejected(r.add(Entry::new(2, 1, 2, "", &IF0[..], false, false)));
        let over = rejected(r.add(Entry::new(3, 1, 2, "ai-90", &IF7[..], false, false)));
        let cap = r.add(Entry::new(4, 1, 2, "ai-90", &IF6[..], false, false)).is_ok();
        cs.add(
            "f21594.field_bounds",
            p0 == Some((0x91070, Taint::PriorityOutOfRange))
                && p6 == Some((0x91070, Taint::PriorityOutOfRange))
                && p1
                && p5
                && nobody == Some((0x91070, Taint::OwnerMissing))
                && over == Some((0x91070, Taint::IfaceOverflow))
                && cap,
            "优先级 0/6、空承接方、7 接口三者有一未被拒，或 1/5/6 被误拒",
        );
    }

    // 12) 边界：空清单 = 40 条覆盖缺口，指纹停在偏移基上。
    {
        let r = Roster::new();
        let a = r.audit();
        cs.add(
            "f21594.empty_roster",
            r.is_empty()
                && r.at(0).is_none()
                && a.covered() == 0
                && a.taint_count(Taint::CoverageGap) == 40
                && a.code().code() == 0x91070
                && r.schedule().is_empty()
                && r.export().is_empty()
                && r.fingerprint() == 0xcbf2_9ce4_8422_2325,
            "空清单未产出 40 条缺口，或指纹未停在偏移基",
        );
    }

    // 13) 指纹对单字段敏感 + 导出首末行格式（末行 F/O 标记必须为 "--"）。
    cs.add(
        "f21594.fingerprint_and_export",
        standard.as_ref().map_or(false, |r| {
            let mut alt = Roster::new();
            let mut built = true;
            for b in 1..=BATCH_SPAN {
                let mut e = standard_entry(b);
                if b == 5 {
                    e.priority = 4;
                }
                if alt.add(e).is_err() {
                    built = false;
                    break;
                }
            }
            let exp = r.export();
            built
                && r.fingerprint() != alt.fingerprint()
                && exp.matches('\n').count() == 40
                && exp.starts_with("b1|s1|p1|ai-90|core.irq|F-")
                && exp.contains("b40|s1|p5|ai-97||--")
        }),
        "指纹对优先级改动不敏感，或导出首末行格式不符",
    );

    cs
}
