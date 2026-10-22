//! UNX-F21582 · ID 终值闭合（F20801–F21600 连续唯一）
//!
//! 诊断段 0x91009~0x91010（F2-B40 域收官段 2/100）。
//!
//! 判据原文（施工书 UNX-F21582-J1）：800 条连续唯一脚本实核，零空洞零重号。
//!
//! 设计要点（为什么不是「排个序再比一遍」）：
//! 排序会同时抹掉两条最要紧的信息——**登记顺序**与**首个越界位置**。台账是逐条
//! 登记出来的，乱序本身就是故障（两段脚本拼接错位最常见的形态是后段整体前移），
//! 排完序再看就永远发现不了。故本模块有两条互不复用代码的路径：写路径
//! `Registry::register` 追加时与上一条严格咬合；读路径 `audit` 只读整批扫描。
//! 两条路径须对同一序列给出同一指纹，闭环才成立——这是「闭合」的实质含义。
//!
//! 唯一性用 800 位位图而非相邻比较：相邻比较只能抓连续重号，真实故障多是「两段
//! 脚本拼在一起、接缝处整段重复」，重号之间隔着几十条，相邻比较一个也抓不到。
//! 位图 100 字节，定长栈上放得下，不为此付一次分配。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 域段首号（施工书 F20801）。
pub const FIRST_ID: u32 = 20_801;
/// 域段末号（施工书 F21600）。
pub const LAST_ID: u32 = 21_600;
/// 域段宽度（末号减首号加一，800 条）。
pub const ID_SPAN: u32 = 800;
/// 台账容量：与域段宽度同值——域外编号进不来，容量与宽度必然相等。
pub const LEDGER_CAP: usize = 800;
/// 位图字节数（800 位 / 8）。
const BITMAP_BYTES: usize = 100;
/// 每批条数（施工书 F2 域每批 20 条，用于把编号折算成批号）。
const PER_BATCH: u32 = 20;

/// 闭合诊断码闭集（0x91009~0x91010 八码）。
///
/// 显式写判别式而不用隐式递增：码值是跨模块日志的对账键，日后插入一个变体就会
/// 让后续码整体漂移，历史日志再也归不了因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureCode {
    Ok = 0x91009,
    OutOfRange = 0x9100A,
    OutOfOrder = 0x9100B,
    Duplicate = 0x9100C,
    Gap = 0x9100D,
    LedgerFull = 0x9100E,
    Incomplete = 0x9100F,
    AlreadyClosed = 0x91010,
}

impl ClosureCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            ClosureCode::Ok => "编号台账闭合成功",
            ClosureCode::OutOfRange => "编号落在 F20801–F21600 之外",
            ClosureCode::OutOfOrder => "登记顺序倒退",
            ClosureCode::Duplicate => "同一编号被登记两次",
            ClosureCode::Gap => "编号序列存在空洞",
            ClosureCode::LedgerFull => "台账已满仍被登记",
            ClosureCode::Incomplete => "台账不成闭合",
            ClosureCode::AlreadyClosed => "台账已闭合仍被写入",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            ClosureCode::Ok => "八百条首末在位、逐条咬合、无洞无重",
            ClosureCode::OutOfRange => "编号由批号与批内序算出时越过了域边界",
            ClosureCode::OutOfOrder => "后段脚本被插到了前段之前",
            ClosureCode::Duplicate => "两段脚本的覆盖区间重叠",
            ClosureCode::Gap => "编号生成器跳过了某一段",
            ClosureCode::LedgerFull => "域段条目数已达容量上界",
            ClosureCode::Incomplete => "条数不足或首末编号不在域段两端",
            ClosureCode::AlreadyClosed => "闭合后仍调用登记路径",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ClosureCode::Ok => "转入判据终钉核",
            ClosureCode::OutOfRange => "回查编号生成式的域段基数",
            ClosureCode::OutOfOrder => "按编号升序重排登记批次",
            ClosureCode::Duplicate => "裁掉重叠段后重新登记",
            ClosureCode::Gap => "按返回的洞首号与宽度补齐缺条",
            ClosureCode::LedgerFull => "先确认域宽是否被改小",
            ClosureCode::Incomplete => "补齐缺条或核对首末编号",
            ClosureCode::AlreadyClosed => "改走只读复核接口",
        }
    }
    /// 数值形式（日志/判据对账用）。
    ///
    /// 返回 u32：五位数码段（0x91009）超出 u16 值域，`as u16` 会静默截断成
    /// 0x1009，与低段码撞车——这类截断在日志里表现为「另一个模块的码」。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 闭合故障（带定位：序号 + 编号 + 区间宽度）。
///
/// 三个定位字段缺一不可：`at_seq` 说「扫到第几条出的问题」，`at_id` 说「是哪一
/// 号」，`span` 说「跨了几号」。只给编号时，重号与空洞无法区分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClosureFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 涉事编号（空洞时为洞首号，空台账为 0）。
    pub at_id: u32,
    /// 涉事登记序（1-based，0 表示无具体条）。
    pub at_seq: u16,
    /// 区间宽度（空洞补几条 / 缺口几条）。
    pub span: u32,
}

impl ClosureFault {
    /// 建故障（宽度默认 0，需要时用 `with_span` 补）。
    pub const fn new(code: ClosureCode, at_id: u32, at_seq: u16) -> ClosureFault {
        ClosureFault { code: code.code(), at_id, at_seq, span: 0 }
    }
    /// 补区间宽度。
    pub const fn with_span(mut self, span: u32) -> ClosureFault {
        self.span = span;
        self
    }
    /// 码还原成枚举。
    ///
    /// 闭集外的值不 panic：保守落到 `AlreadyClosed`（「未知即可疑」），同时
    /// `is_known` 让调用方能区分「真已闭合」与「码本身坏了」。
    pub const fn as_code(self) -> ClosureCode {
        match self.code {
            0x91009 => ClosureCode::Ok,
            0x9100A => ClosureCode::OutOfRange,
            0x9100B => ClosureCode::OutOfOrder,
            0x9100C => ClosureCode::Duplicate,
            0x9100D => ClosureCode::Gap,
            0x9100E => ClosureCode::LedgerFull,
            0x9100F => ClosureCode::Incomplete,
            _ => ClosureCode::AlreadyClosed,
        }
    }
    /// 码是否落在闭集内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91009 && self.code <= 0x91010
    }
    /// 一行可读诊断（运维直接抄进工单）。
    pub fn describe(self) -> String {
        let mut s = String::with_capacity(96);
        s.push_str(self.as_code().what());
        s.push_str(" id=");
        s.push_str(&self.at_id.to_string());
        s.push_str(" seq=");
        s.push_str(&self.at_seq.to_string());
        s.push_str(" span=");
        s.push_str(&self.span.to_string());
        s
    }
}

/// 台账单条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdRecord {
    /// 编号。
    pub id: u32,
    /// 登记序（1-based，必须等于下标加一）。
    pub seq: u16,
}

impl IdRecord {
    /// 编号折算批号（1-based）。
    ///
    /// 越界编号返回 0 而不绕回：批号 0 在报表里一眼是坏值，绕回则会伪装成合法
    /// 批号（20800 会算出第 40 批）。
    pub const fn batch(&self) -> u16 {
        if self.id < FIRST_ID {
            return 0;
        }
        ((self.id - FIRST_ID) / PER_BATCH + 1) as u16
    }
}

/// 审计摘要。
///
/// `unique` 与 `count` 分开留档：闭合时两者相等，只要有一个重号 `unique` 就小于
/// `count`——这条差值比任何布尔量都能说明「重了几条」。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AuditDigest {
    /// 条数。
    pub count: u16,
    /// 首号。
    pub first: u32,
    /// 末号。
    pub last: u32,
    /// 覆盖跨度（末号减首号加一）。
    pub span: u32,
    /// 去重后条数。
    pub unique: u16,
    /// 批序指纹（FNV-1a 64，按编号与序号逐条咬合）。
    pub hash: u64,
}

/// 闭合回执（`hash` 非零是弱但必要的一条：FNV 偏移基非零，算出 0 说明台账为空
/// 或指纹函数被换掉了，故回执判据里一并核它）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClosureReceipt {
    /// 闭合条数。
    pub count: u16,
    /// 首号。
    pub first: u32,
    /// 末号。
    pub last: u32,
    /// 批序指纹。
    pub hash: u64,
}

/// 编号登记台账（写路径）。
///
/// 只做追加：既有条不可改、不可删。允许就地改写会把「重号」变成静默覆盖，而这
/// 正是本模块要抓的一类故障。要修只能新建台账重登。
pub struct Registry {
    entries: Vec<IdRecord>,
    closed: bool,
    /// 成功闭合次数（幂等观测点：闭合后恒为 1）。
    close_count: u16,
    /// 被拒操作次数（诊断面：拒写不该无声无息）。
    rejects: u16,
}

impl Registry {
    /// 新建空台账。
    pub const fn new() -> Registry {
        Registry { entries: Vec::new(), closed: false, close_count: 0, rejects: 0 }
    }

    /// 下一条应得的登记序。
    fn seq_next(&self) -> u16 {
        (self.entries.len() as u16).saturating_add(1)
    }

    /// 登记一条编号。
    ///
    /// 四道闸的次序是刻意排的：闭合态最便宜且语义上压倒一切（闭合后即只读）；
    /// 容量先于码界——满账时再谈「这条在不在域内」已无意义，若把码界放前面，
    /// 满账永远报越界，运维会去改编号而不是查容量；码界先于咬合——域外编号
    /// 不必与上一条比较。咬合三分（重号 / 倒退 / 跳号）必须给三个不同的码，
    /// 因为修法完全不同：删重复段、重排批次、补缺条。
    pub fn register(&mut self, id: u32) -> Result<u16, ClosureFault> {
        let seq = self.seq_next();
        if self.closed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ClosureFault::new(ClosureCode::AlreadyClosed, id, seq));
        }
        if self.entries.len() >= LEDGER_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ClosureFault::new(ClosureCode::LedgerFull, id, seq));
        }
        if id < FIRST_ID || id > LAST_ID {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ClosureFault::new(ClosureCode::OutOfRange, id, seq));
        }
        if let Some(prev) = self.entries.last() {
            if id == prev.id {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ClosureFault::new(ClosureCode::Duplicate, id, seq));
            }
            if id < prev.id {
                self.rejects = self.rejects.saturating_add(1);
                return Err(ClosureFault::new(ClosureCode::OutOfOrder, id, seq));
            }
            if id > prev.id.saturating_add(1) {
                let span = id - prev.id - 1;
                self.rejects = self.rejects.saturating_add(1);
                return Err(ClosureFault::new(ClosureCode::Gap, prev.id + 1, seq).with_span(span));
            }
        }
        self.entries.push(IdRecord { id, seq });
        Ok(seq)
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空台账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 成功闭合次数。
    pub const fn close_count(&self) -> u16 {
        self.close_count
    }

    /// 被拒操作次数。
    pub const fn rejects(&self) -> u16 {
        self.rejects
    }

    /// 首号（空台账返回 0）。
    pub fn first_id(&self) -> u32 {
        match self.entries.first() {
            Some(r) => r.id,
            None => 0,
        }
    }

    /// 末号（空台账返回 0）。
    pub fn last_id(&self) -> u32 {
        match self.entries.last() {
            Some(r) => r.id,
            None => 0,
        }
    }

    /// 按登记序取一条（1-based；0 与越界都返回 None——序从 1 起算，0 不是漏号）。
    pub fn record(&self, seq: u16) -> Option<IdRecord> {
        if seq == 0 {
            return None;
        }
        self.entries.get((seq - 1) as usize).copied()
    }

    /// 编号序列副本（供审计路径复用，避免两条路径共享内部布局）。
    pub fn ids(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            out.push(self.entries[i].id);
            i += 1;
        }
        out
    }

    /// 批序指纹：逐条喂 (编号, 序号)。
    ///
    /// 逐字段喂而不是喂结构体内存：内存含 padding，未初始化字节会让指纹在不同
    /// 编译版本间漂移，跨版本复核就永远对不上。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.entries.len() {
            let r = self.entries[i];
            h = fnv_u64(h, r.id as u64);
            h = fnv_u64(h, r.seq as u64);
            i += 1;
        }
        h
    }

    /// 闭合台账。
    ///
    /// 只核条数、首号、末号三件：中间是否连续由写路径保证（每条与上一条咬合），
    /// 闭合时重扫属重复劳动；但**指纹**要在这里算，因为它是跨路径对账的凭据，
    /// 必须与审计路径对同一序列算同一种指纹。
    pub fn close(&mut self) -> Result<ClosureReceipt, ClosureFault> {
        if self.closed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ClosureFault::new(ClosureCode::AlreadyClosed, 0, 0));
        }
        if self.entries.len() != LEDGER_CAP {
            self.rejects = self.rejects.saturating_add(1);
            let short = (LEDGER_CAP - self.entries.len()) as u32;
            return Err(
                ClosureFault::new(ClosureCode::Incomplete, 0, self.seq_next()).with_span(short)
            );
        }
        if self.first_id() != FIRST_ID || self.last_id() != LAST_ID {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ClosureFault::new(ClosureCode::Incomplete, self.last_id(), 0).with_span(1));
        }
        self.closed = true;
        self.close_count = self.close_count.saturating_add(1);
        Ok(ClosureReceipt {
            count: LEDGER_CAP as u16,
            first: self.first_id(),
            last: self.last_id(),
            hash: self.fingerprint(),
        })
    }
}

/// FNV-1a 64 偏移基（FNV 规范 1a 变体）。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 单步（质数 0x100000001b3 来自 FNV 规范）。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 整批审计（读路径，与写路径零共享代码）。
///
/// 扫描次序即故障优先级：码界 → 重号 → 乱序 → 空洞 → 条数。前一步结论是后一步的
/// 前提——不先证「全在域内」，位图下标就越界；不先证「无重号」，重复项既像倒退又
/// 像零宽洞，会把乱序与空洞判成一团；不先证「单调」，空洞区间的定义本身不成立。
pub fn audit(raw: &[u32]) -> Result<AuditDigest, ClosureFault> {
    if raw.is_empty() {
        return Err(ClosureFault::new(ClosureCode::Incomplete, 0, 0).with_span(ID_SPAN));
    }
    let mut i = 0usize;
    while i < raw.len() {
        if raw[i] < FIRST_ID || raw[i] > LAST_ID {
            return Err(ClosureFault::new(
                ClosureCode::OutOfRange,
                raw[i],
                (i as u16).saturating_add(1),
            ));
        }
        i += 1;
    }
    // 位图查重：100 字节定长，栈上放得下，不为这一步付一次分配。
    let mut bits = [0u8; BITMAP_BYTES];
    i = 0;
    while i < raw.len() {
        let off = (raw[i] - FIRST_ID) as usize;
        let byte = off / 8;
        let mask = 1u8 << (off % 8);
        if bits[byte] & mask != 0 {
            return Err(ClosureFault::new(
                ClosureCode::Duplicate,
                raw[i],
                (i as u16).saturating_add(1),
            ));
        }
        bits[byte] |= mask;
        i += 1;
    }
    i = 1;
    while i < raw.len() {
        if raw[i] < raw[i - 1] {
            return Err(ClosureFault::new(
                ClosureCode::OutOfOrder,
                raw[i],
                (i as u16).saturating_add(1),
            ));
        }
        i += 1;
    }
    i = 1;
    while i < raw.len() {
        let prev = raw[i - 1];
        let cur = raw[i];
        if cur > prev.saturating_add(1) {
            return Err(ClosureFault::new(
                ClosureCode::Gap,
                prev + 1,
                (i as u16).saturating_add(1),
            )
            .with_span(cur - prev - 1));
        }
        i += 1;
    }
    if raw.len() != LEDGER_CAP {
        let short = (LEDGER_CAP - raw.len()) as u32;
        return Err(ClosureFault::new(
            ClosureCode::Incomplete,
            raw[raw.len() - 1],
            raw.len() as u16,
        )
        .with_span(short));
    }
    let mut h = FNV_BASIS;
    i = 0;
    while i < raw.len() {
        h = fnv_u64(h, raw[i] as u64);
        h = fnv_u64(h, (i as u16).saturating_add(1) as u64);
        i += 1;
    }
    Ok(AuditDigest {
        count: raw.len() as u16,
        first: raw[0],
        last: raw[raw.len() - 1],
        span: raw[raw.len() - 1].saturating_sub(raw[0]).saturating_add(1),
        unique: raw.len() as u16,
        hash: h,
    })
}

/// 构造标准台账（判据夹具，800 条 F20801..F21600）。
///
/// 返回 Result 而非直接给值：历史上见过夹具自带矛盾，整册判据被一处强制取值挡在
/// 门外，从未真跑过。夹具能否构造本身就是第一条判据。
pub fn standard_registry() -> Result<Registry, ClosureFault> {
    let mut reg = Registry::new();
    let mut id = FIRST_ID;
    while id <= LAST_ID {
        reg.register(id)?;
        id += 1;
    }
    Ok(reg)
}

/// 构造标准编号序列。
pub fn standard_ids() -> Result<Vec<u32>, ClosureFault> {
    let reg = standard_registry()?;
    Ok(reg.ids())
}

/// 构造「整体平移」台账：从 `start` 起连续 `count` 条。
///
/// 平移台账在写路径完全合法（域内、连续、不重），但首末不在域段两端，故闭合必拒
/// 而审计报条数不足——正好用来证明指纹对编号敏感。
pub fn shifted_registry(start: u32, count: u32) -> Result<Registry, ClosureFault> {
    let mut reg = Registry::new();
    let mut k = 0u32;
    while k < count {
        reg.register(start.saturating_add(k))?;
        k += 1;
    }
    Ok(reg)
}

/// 取故障码（成功记 0）：把「结果 → 码」压成一次比较，判据只剩断言本身。
fn code_of<T>(r: Result<T, ClosureFault>) -> u32 {
    match r {
        Ok(_) => 0,
        Err(f) => f.code,
    }
}

/// 判据入口（UNX-F21582-J1 及其配套）。
///
/// 断言里的 800 / 20801 / 21600 一律写字面量，不复用被测常量：常量一旦被改，
/// 判据跟着改就成了自己证自己。
pub fn run_unx21582_idclosure_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21582");
    let fixture = standard_registry();
    let ids = standard_ids();

    // 1) 夹具可构造：先证夹具，再证被测逻辑。
    cs.add("f21582.fixture_buildable", fixture.is_ok() && ids.is_ok(),
        "标准 800 条台账或序列构造失败");

    // 2) 写路径容量与首末；3) 批号折算（第 21 条属第 2 批，每批 20 条）。
    match &fixture {
        Ok(reg) => {
            cs.add("f21582.ledger_800",
                reg.len() == 800 && reg.first_id() == 20_801 && reg.last_id() == 21_600,
                "台账条数或首末编号不符（应为 800 / 20801 / 21600）");
            cs.add("f21582.batch_math",
                reg.record(21).map(|r| r.batch()) == Some(2)
                    && reg.record(1).map(|r| r.batch()) == Some(1),
                "第 21 条未落在第 2 批，批号折算错");
        }
        Err(_) => {
            cs.add("f21582.ledger_800", false, "夹具不可用");
            cs.add("f21582.batch_math", false, "夹具不可用");
        }
    }

    // 读路径只算一次，后续三条判据共用同一份摘要。
    let digest = match &ids {
        Ok(v) => audit(v),
        Err(_) => Err(ClosureFault::new(ClosureCode::Incomplete, 0, 0)),
    };

    // 4) 读路径密排：800 条、跨 800、去重 800、首末在位。
    cs.add("f21582.audit_dense",
        match &digest {
            Ok(d) => d.count == 800 && d.span == 800 && d.unique == 800
                && d.first == 20_801 && d.last == 21_600,
            Err(_) => false,
        },
        "审计摘要不满足满宽密排");

    // 5) 双路指纹一致（写路径 vs 读路径，同一序列同一指纹）。
    cs.add("f21582.dual_path_hash",
        match (&fixture, &digest) {
            (Ok(reg), Ok(d)) => reg.fingerprint() == d.hash && d.hash != 0,
            _ => false,
        },
        "写路径指纹与读路径指纹不一致");

    // 6) 闭合回执。
    match standard_registry() {
        Ok(mut reg) => {
            let good = match reg.close() {
                Ok(r) => r.count == 800 && r.first == 20_801 && r.last == 21_600
                    && r.hash != 0 && reg.close_count() == 1,
                Err(_) => false,
            };
            cs.add("f21582.close_receipt", good, "回执不自洽或首末不符");
        }
        Err(_) => cs.add("f21582.close_receipt", false, "夹具不可用"),
    }

    // 7) 空洞必拒：登记跳号（写路径）+ 首个洞区间（读路径）。
    {
        let mut gap_ok = false;
        let mut detail = String::new();
        if let Ok(mut reg) = shifted_registry(20_801, 5) {
            if let Err(f) = reg.register(20_807) {
                gap_ok = f.code == ClosureCode::Gap.code()
                    && f.at_id == 20_806 && f.span == 1 && f.at_seq == 6;
                detail = f.describe();
            }
        }
        let audit_gap = match audit(&[20_801u32, 20_802, 20_805]) {
            Err(f) => f.code == ClosureCode::Gap.code() && f.at_id == 20_803 && f.span == 2,
            Ok(_) => false,
        };
        if detail.is_empty() {
            cs.add("f21582.gap_rejected", gap_ok && audit_gap,
                "跳号未被拒或洞区间错（写路径应报洞首 20806/宽 1，审计应报 20803/宽 2）");
        } else {
            cs.add_dyn("f21582.gap_rejected", gap_ok && audit_gap, &detail);
        }
    }

    // 8) 重号必拒：相邻重号由写路径抓，跨距重号只能由位图抓。
    {
        let adj = match shifted_registry(20_801, 1) {
            Ok(mut reg) => code_of(reg.register(20_801)) == ClosureCode::Duplicate.code(),
            Err(_) => false,
        };
        let spanning = match audit(&[20_801u32, 20_802, 20_803, 20_801]) {
            Err(f) => f.code == ClosureCode::Duplicate.code() && f.at_id == 20_801
                && f.at_seq == 4,
            Ok(_) => false,
        };
        cs.add("f21582.duplicate_rejected", adj && spanning,
            "相邻或跨距重号未被拒（跨距重号须由位图在 seq=4 处命中）");
    }

    // 9) 乱序必拒（写路径与读路径各一例）。
    {
        let ordered = match shifted_registry(20_805, 1) {
            Ok(mut reg) => code_of(reg.register(20_803)) == ClosureCode::OutOfOrder.code(),
            Err(_) => false,
        };
        let audit_order = code_of(audit(&[20_805u32, 20_803, 20_804]))
            == ClosureCode::OutOfOrder.code();
        cs.add("f21582.out_of_order_rejected", ordered && audit_order,
            "登记倒退未被拒或码不符");
    }

    // 10) 域外必拒：下界外与上界外各一例。
    {
        let low = code_of(Registry::new().register(20_800)) == ClosureCode::OutOfRange.code();
        let high = code_of(Registry::new().register(21_601)) == ClosureCode::OutOfRange.code();
        cs.add("f21582.out_of_range_rejected", low && high,
            "20800 或 21601 未被拒（越界码不符）");
    }

    // 11) 上限边界：799→800 可登记；满账后再进必须报容量码而非越界码。
    {
        let under = match shifted_registry(20_801, 799) {
            Ok(mut reg) => reg.len() == 799 && reg.register(21_600).is_ok() && reg.len() == 800,
            Err(_) => false,
        };
        let full = match standard_registry() {
            Ok(mut reg) => {
                let before = reg.rejects();
                code_of(reg.register(21_601)) == ClosureCode::LedgerFull.code()
                    && reg.len() == 800 && reg.rejects() == before.saturating_add(1)
            }
            Err(_) => false,
        };
        cs.add("f21582.capacity_boundary", under && full,
            "799→800 未通过，或满账后未报 LedgerFull（报越界即归因错位）");
    }

    // 12) 空台账 / 单条台账边界；13) 短账闭合必拒（799 条，末号差一条）。
    {
        let blank = Registry::new();
        let empty = blank.is_empty() && match audit(&[]) {
            Err(f) => f.code == ClosureCode::Incomplete.code() && f.at_seq == 0 && f.span == 800,
            Ok(_) => false,
        };
        let single = match audit(&[20_801u32]) {
            Err(f) => f.code == ClosureCode::Incomplete.code() && f.at_seq == 1 && f.span == 799,
            Ok(_) => false,
        };
        let empty_close = match Registry::new().close() {
            Err(f) => f.code == ClosureCode::Incomplete.code() && f.span == 800,
            Ok(_) => false,
        };
        cs.add("f21582.empty_and_single", empty && single && empty_close,
            "空台账/单条台账未被判 Incomplete（或缺失条数不符）");
        let short = match shifted_registry(20_801, 799) {
            Ok(mut reg) => match reg.close() {
                Err(f) => f.code == ClosureCode::Incomplete.code() && f.span == 1,
                Ok(_) => false,
            },
            Err(_) => false,
        };
        cs.add("f21582.short_close_rejected", short, "799 条闭合未被拒或缺口数不为 1");
    }

    // 14) 幂等：审计两遍逐位相同；二次闭合被拒且计数只加一次。
    {
        let stable = match (standard_ids(), standard_ids()) {
            (Ok(a), Ok(b)) => match (audit(&a), audit(&b)) {
                (Ok(x), Ok(y)) => x == y && x.hash == y.hash,
                _ => false,
            },
            _ => false,
        };
        let lock = match standard_registry() {
            Ok(mut reg) => {
                let first = reg.close().is_ok();
                let before = reg.rejects();
                let second = reg.close();
                first && second.err().map(|f| f.code) == Some(ClosureCode::AlreadyClosed.code())
                    && reg.close_count() == 1
                    && reg.rejects() == before.saturating_add(1)
                    && reg.register(20_801).is_err()
            }
            Err(_) => false,
        };
        cs.add("f21582.idempotent", stable && lock,
            "两次审计结果不同，或二次闭合未拒/计数不符/闭合后仍可登记");
    }

    // 15) 指纹对编号敏感：整体平移一条，指纹必变（闭合逻辑本身抓不到它）。
    {
        let fp = |r: Result<Registry, ClosureFault>| match r { Ok(x) => x.fingerprint(), Err(_) => 0 };
        let a = fp(standard_registry());
        let b = fp(shifted_registry(20_802, 799));
        let c = fp(shifted_registry(20_802, 799));
        cs.add("f21582.fingerprint_sensitive", a != 0 && a != b && b == c,
            "平移后指纹未变，或同一输入两次指纹不同（非确定性）");
    }

    // 16) 八码闭集：码落段内、两两互异、三要素齐全、未知码不 panic。
    {
        const ALL: [ClosureCode; 8] = [
            ClosureCode::Ok, ClosureCode::OutOfRange, ClosureCode::OutOfOrder,
            ClosureCode::Duplicate, ClosureCode::Gap, ClosureCode::LedgerFull,
            ClosureCode::Incomplete, ClosureCode::AlreadyClosed,
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < ALL.len() {
            let c = ALL[i];
            let n = c.code();
            if n < 0x91009 || n > 0x91010 {
                ok = false;
            }
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                ok = false;
            }
            let mut j = i + 1;
            while j < ALL.len() {
                if ALL[j].code() == n {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        let bogus = ClosureFault { code: 0x910FF, at_id: 0, at_seq: 0, span: 0 };
        ok = ok && !bogus.is_known() && !bogus.describe().is_empty();
        cs.add("f21582.code_closed_set", ok,
            "诊断码越段、互复、三要素缺失，或未知码未被识别");
    }

    cs
}
