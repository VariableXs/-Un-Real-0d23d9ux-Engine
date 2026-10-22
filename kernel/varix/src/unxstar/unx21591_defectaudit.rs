//! UNX-F21591 · 域缺陷账本清点（产线双轨纪律）
//!
//! 诊断段 0x91051~0x91058（F2-B40 域收官段 11/100）。
//!
//! 判据原文（施工书 UNX-F21591-J1）：缺陷账 60 条清点：🔴0/🟡0/🟢 待冲刺清单移交。
//!
//! 设计要点（为什么「关闭」必须拆成两条轨道）：
//! 缺陷账最容易骗人的不是漏记，而是把「我说修了」当成「修好了」。只留一个
//! `closed` 布尔，「声明关闭无证据」与「有证据无人签关闭」都会被压成 `false`，
//! 而两者处置方向相反：前者补证据，后者补签字。故分存 `declared_closed`
//! （批次报告自报）与 `evidenced_closed`（证据台账查得），不一致即立案，
//! 立案清单带缺陷号，可直接派单。
//!
//! 清点顺序（总数 → 登记面 → 证据面 → 双轨 → 红 → 黄 → 移交）也是刻意的：
//! 总数不对时任何比例都不可信，故先卡总数；双轨必须早于红黄计数，因为
//! 「自报关闭」的红缺陷会被算成已关闭，红数假性归零——那是最危险的错读数。

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 缺陷账总条数（施工书 J1 的口径：D-001~D-060）。
pub const DEFECT_TOTAL: u16 = 60;
/// 归属批数（B01~B08）。
pub const BATCH_COUNT: u8 = 8;
/// 缺陷号下界。
pub const ID_MIN: u16 = 1;
/// 缺陷号上界（与总条数同源，但独立成常量：号段与条数是两套口径）。
pub const ID_MAX: u16 = 60;

/// 清点诊断码闭集（0x91051~0x91058 八码）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入变体不得让历史
/// 日志整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditCode {
    /// 0x91051 清点通过。
    AuditOk = 0x91051,
    /// 0x91052 条数与 60 不符（缺条/超条/超容）。
    CountMismatch = 0x91052,
    /// 0x91053 红色缺陷未清零。
    RedRemaining = 0x91053,
    /// 0x91054 黄色缺陷未清零。
    YellowRemaining = 0x91054,
    /// 0x91055 双轨不一致。
    DualTrackDiverge = 0x91055,
    /// 0x91056 自报关闭却无证据号。
    EvidenceMissing = 0x91056,
    /// 0x91057 登记面非法条目（越号/重号/越批）。
    BadDefect = 0x91057,
    /// 0x91058 已移交后二次移交被拒。
    AlreadyHandedOver = 0x91058,
}

impl AuditCode {
    /// 诊断三要素之一：现象。
    pub const fn what(self) -> &'static str {
        match self {
            AuditCode::AuditOk => "缺陷账清点通过",
            AuditCode::CountMismatch => "缺陷账条数与清点口径不符",
            AuditCode::RedRemaining => "仍有红色缺陷未关闭",
            AuditCode::YellowRemaining => "仍有黄色缺陷未关闭",
            AuditCode::DualTrackDiverge => "声明关闭与有证据关闭不一致",
            AuditCode::EvidenceMissing => "声明关闭但无证据号",
            AuditCode::BadDefect => "登记条目本身非法",
            AuditCode::AlreadyHandedOver => "缺陷账已移交仍被写入",
        }
    }
    /// 诊断三要素之二：原因。
    pub const fn why(self) -> &'static str {
        match self {
            AuditCode::AuditOk => "六十条齐、双轨合、红黄皆零",
            AuditCode::CountMismatch => "账本缺条、超条，或容量已满仍登记",
            AuditCode::RedRemaining => "阻断级缺陷被重新打开或从未修复",
            AuditCode::YellowRemaining => "严重级缺陷被重新打开或从未修复",
            AuditCode::DualTrackDiverge => "批次报告的自报与证据台账的实查是两套记录",
            AuditCode::EvidenceMissing => "关闭动作先于证据归档，或证据号被清空",
            AuditCode::BadDefect => "编号越号段、同号二次登记，或批号超出 B01~B08",
            AuditCode::AlreadyHandedOver => "移交后仍走登记路径而非复核路径",
        }
    }
    /// 诊断三要素之三：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            AuditCode::AuditOk => "导出待冲刺清单并移交",
            AuditCode::CountMismatch => "按 D-001~D-060 补登或裁剪超容条目",
            AuditCode::RedRemaining => "先修红项再谈移交，红项不允许带债",
            AuditCode::YellowRemaining => "补修黄项或经评审显式降级并留痕",
            AuditCode::DualTrackDiverge => "按立案清单逐条对齐自报与证据",
            AuditCode::EvidenceMissing => "补归档证据或撤回关闭声明",
            AuditCode::BadDefect => "核对缺陷号与归属批后重登",
            AuditCode::AlreadyHandedOver => "改走复核接口，不要重写账本",
        }
    }
    /// 数值形式（u32：五位数码段，u16 会截断成低段码）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 缺陷严重度三档。自带序（`rank`）：清点靠它做稳定排序与指纹编码，故不用
/// 裸字符串——拼错一个字母就会变成一档新严重度，且不会有任何判据发现。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 阻断级。
    Red,
    /// 严重级。
    Yellow,
    /// 一般级（可进冲刺清单）。
    Green,
}

impl Severity {
    /// 排序与指纹用的数值档位（与严重程度同向）。
    pub const fn rank(self) -> u8 {
        match self { Severity::Red => 0, Severity::Yellow => 1, Severity::Green => 2 }
    }
}

/// 单条缺陷。两轨分开存是立论基础（见文件头）；`evidence_id` 为 0 表示证据缺位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefectEntry {
    /// 缺陷号 D-001~D-060。
    pub id: u16,
    /// 严重度。
    pub severity: Severity,
    /// 归属批 B01~B08。
    pub batch: u8,
    /// 批次报告自报关闭。
    pub declared_closed: bool,
    /// 证据台账查得关闭。
    pub evidenced_closed: bool,
    /// 证据号（0 = 无证据）。
    pub evidence_id: u16,
}

impl DefectEntry {
    /// 构造（不在构造期校验：校验统一在 `admit` 做，避免两处口径分叉）。
    pub const fn new(
        id: u16,
        severity: Severity,
        batch: u8,
        declared_closed: bool,
        evidenced_closed: bool,
        evidence_id: u16,
    ) -> DefectEntry {
        DefectEntry { id, severity, batch, declared_closed, evidenced_closed, evidence_id }
    }

    /// 结构自洽（只看本条）。「有证据关闭但证据号为 0」在这里判死：放进来
    /// 只会让后续统计替它背锅。
    pub const fn is_well_formed(&self) -> bool {
        self.id >= ID_MIN
            && self.id <= ID_MAX
            && self.batch >= 1
            && self.batch <= BATCH_COUNT
            && !(self.evidenced_closed && self.evidence_id == 0)
    }

    /// 两条轨道是否同向。
    pub const fn tracks_agree(&self) -> bool {
        self.declared_closed == self.evidenced_closed
    }

    /// 是否仍未关闭（两轨都未关才算真未关）。
    pub const fn is_open(&self) -> bool {
        !self.declared_closed && !self.evidenced_closed
    }

    /// 排序键：先批号后缺陷号。用元组而非「批号 × 1000」这类隐式进制。
    pub const fn sprint_key(&self) -> (u8, u16) {
        (self.batch, self.id)
    }
}

/// 三档计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SeverityCounts {
    /// 红。
    pub red: u16,
    /// 黄。
    pub yellow: u16,
    /// 绿。
    pub green: u16,
}

impl SeverityCounts {
    /// 合计。
    pub const fn total(&self) -> u16 {
        self.red.saturating_add(self.yellow).saturating_add(self.green)
    }
    /// 报告片段（档名与数值成对，便于直接 diff）。
    pub fn line(&self) -> String {
        let mut out = String::with_capacity(48);
        out.push_str("red=");
        out.push_str(&self.red.to_string());
        out.push_str(" yellow=");
        out.push_str(&self.yellow.to_string());
        out.push_str(" green=");
        out.push_str(&self.green.to_string());
        out
    }
}

/// 清点故障（带码与定位缺陷号，0 = 与具体缺陷无关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditFault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 定位缺陷号。
    pub at: u16,
}

impl AuditFault {
    const fn new(code: AuditCode, at: u16) -> AuditFault {
        AuditFault { code: code.code(), at }
    }
    /// 把码还原成枚举；闭集外落到「可疑即拒」的保守落点。
    pub const fn as_code(self) -> AuditCode {
        match self.code {
            0x91051 => AuditCode::AuditOk,
            0x91052 => AuditCode::CountMismatch,
            0x91053 => AuditCode::RedRemaining,
            0x91054 => AuditCode::YellowRemaining,
            0x91055 => AuditCode::DualTrackDiverge,
            0x91056 => AuditCode::EvidenceMissing,
            0x91057 => AuditCode::BadDefect,
            _ => AuditCode::AlreadyHandedOver,
        }
    }
    /// 码是否在闭集内（未知码要能被识别，不能冒充已知码）。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91051 && self.code <= 0x91058
    }
}

/// 清点回执（移交凭据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AuditReceipt {
    /// 账本条数。
    pub total: u16,
    /// 红项未关闭数。
    pub red_open: u16,
    /// 黄项未关闭数。
    pub yellow_open: u16,
    /// 绿项未关闭数（进冲刺清单）。
    pub green_open: u16,
    /// 立案数。
    pub cases: u16,
    /// 归属批数。
    pub batches: u8,
    /// 账本指纹。
    pub fingerprint: u64,
}

/// 移交回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HandoverReceipt {
    /// 红项未关闭数。
    pub red_open: u16,
    /// 黄项未关闭数。
    pub yellow_open: u16,
    /// 移交的待冲刺条数。
    pub sprint_len: u16,
    /// 移交时的账本指纹快照。
    pub fingerprint: u64,
}

/// 缺陷账本。
pub struct DefectLedger {
    defects: Vec<DefectEntry>,
    handed_over: bool,
    /// 被拒写入次数（误登不该无声无息）。
    rejected: u16,
}

impl DefectLedger {
    /// 新建空账。
    pub const fn new() -> DefectLedger {
        DefectLedger { defects: Vec::new(), handed_over: false, rejected: 0 }
    }

    /// 登记一条缺陷。闸门「便宜先行」：移交态 → 容量 → 条目自洽 → 重号；
    /// 重号要扫全表最贵，故放最后，但四条缺一不可——审计期再发现重号时
    /// 已经无法判断哪一条才是真的。
    pub fn admit(&mut self, entry: DefectEntry) -> Result<(), AuditFault> {
        match self.try_admit(entry) {
            Ok(()) => {
                self.defects.push(entry);
                Ok(())
            }
            Err(f) => {
                self.rejected = self.rejected.saturating_add(1);
                Err(f)
            }
        }
    }

    /// 只读判定（把改计数从做判断里剥出来，探测不会抬高被拒计数）。
    fn try_admit(&self, entry: DefectEntry) -> Result<(), AuditFault> {
        if self.handed_over {
            return Err(AuditFault::new(AuditCode::AlreadyHandedOver, entry.id));
        }
        if self.defects.len() >= DEFECT_TOTAL as usize {
            return Err(AuditFault::new(AuditCode::CountMismatch, entry.id));
        }
        if !entry.is_well_formed() {
            return Err(AuditFault::new(AuditCode::BadDefect, entry.id));
        }
        if self.defects.iter().any(|d| d.id == entry.id) {
            return Err(AuditFault::new(AuditCode::BadDefect, entry.id));
        }
        Ok(())
    }

    /// 已登条数。
    pub fn len(&self) -> usize {
        self.defects.len()
    }

    /// 是否已移交。
    pub const fn is_handed_over(&self) -> bool {
        self.handed_over
    }

    /// 被拒写入次数。
    pub const fn rejected(&self) -> u16 {
        self.rejected
    }

    /// 三档计数（含已关闭项：这是账本存量，不是待办量）。
    pub fn counts(&self) -> SeverityCounts {
        let mut c = SeverityCounts::default();
        for d in self.defects.iter() {
            match d.severity {
                Severity::Red => c.red = c.red.saturating_add(1),
                Severity::Yellow => c.yellow = c.yellow.saturating_add(1),
                Severity::Green => c.green = c.green.saturating_add(1),
            }
        }
        c
    }

    /// 指定档位的未关闭数。
    ///
    /// 「未关闭」按**最保守**口径：两轨任一为真即不算未关。计数不替「自报
    /// 关闭」背书——那笔账交给双轨复核去判。
    pub fn open_of(&self, sev: Severity) -> u16 {
        self.defects.iter().filter(|d| d.severity == sev && d.is_open()).count() as u16
    }

    /// 按批分组计数（批号升序，只列出现过的批）。
    ///
    /// 按批号扫一遍而非哈希表：批号是 1..=8 的小闭集，扫描顺序天然确定，
    /// 导出结果可直接 diff，不给哈希顺序留不确定性。
    pub fn batch_histogram(&self) -> Vec<(u8, u16)> {
        let mut out = Vec::new();
        let mut batch: u8 = 1;
        while batch <= BATCH_COUNT {
            let n = self.defects.iter().filter(|d| d.batch == batch).count() as u16;
            if n > 0 {
                out.push((batch, n));
            }
            batch = batch.saturating_add(1);
        }
        out
    }

    /// 立案清单：双轨不一致的缺陷号（升序）。只报号不报原因：原因由 `audit`
    /// 的首因码给出，而清单要能直接贴进派单系统，多一列原因反而要求二次解析。
    pub fn cases(&self) -> Vec<u16> {
        self.defects.iter().filter(|d| !d.tracks_agree()).map(|d| d.id).collect()
    }

    /// 待冲刺清单：绿档且未关闭，按「批号升序、同批按缺陷号」。
    ///
    /// 用稳定排序（`sort_by_key`）而非快排：移交清单要求同键条目保持登记序，
    /// 否则同一批里的两条绿项可能在两次导出中互换，diff 就没法看了。
    pub fn sprint_list(&self) -> Vec<u16> {
        let mut out: Vec<u16> =
            self.defects.iter().filter(|d| d.severity == Severity::Green && d.is_open()).map(|d| d.id).collect();
        out.sort_by_key(|id| self.key_of(*id));
        out
    }

    /// 取排序键（号不在账上时给哨兵键排最后，不 panic）。
    fn key_of(&self, id: u16) -> (u8, u16) {
        match self.defects.iter().find(|d| d.id == id) {
            Some(d) => d.sprint_key(),
            None => (u8::MAX, u16::MAX),
        }
    }

    /// 借出可写条目（改账的唯一入口，被拒计数在此统一累加）。
    fn entry_mut(&mut self, id: u16) -> Result<&mut DefectEntry, AuditFault> {
        match self.defects.iter_mut().find(|d| d.id == id) {
            Some(d) => Ok(d),
            None => {
                self.rejected = self.rejected.saturating_add(1);
                Err(AuditFault::new(AuditCode::BadDefect, id))
            }
        }
    }

    /// 账本指纹：逐条把 (id, 档位, 批号, 自报, 实查, 证据号) 喂进 FNV-1a。
    ///
    /// 逐字段喂而非喂结构体内存：内存含 padding，未初始化字节会让指纹在编译器
    /// 版本间漂移，跨版本复核就永远对不上。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for d in self.defects.iter() {
            h = fnv_u64(h, d.id as u64);
            h = fnv_u64(h, d.severity.rank() as u64);
            h = fnv_u64(h, d.batch as u64);
            h = fnv_u64(h, d.declared_closed as u64);
            h = fnv_u64(h, d.evidenced_closed as u64);
            h = fnv_u64(h, d.evidence_id as u64);
        }
        h
    }

    /// 全账清点。只报第一因：条数错则后续统计不可信；双轨先于红黄，因为
    /// 自报关闭会把红数压低（见文件头）。
    pub fn audit(&self) -> Result<AuditReceipt, AuditFault> {
        if self.defects.len() != DEFECT_TOTAL as usize {
            return Err(AuditFault::new(AuditCode::CountMismatch, self.defects.len() as u16));
        }
        // 重号与越界复核：`admit` 已拦过，但账本可能被别处构造后整体塞入，
        // 审计期必须能独立发现（不能信任登记路径的自述）。
        for (i, d) in self.defects.iter().enumerate() {
            if d.id < ID_MIN || d.id > ID_MAX || d.batch < 1 || d.batch > BATCH_COUNT {
                return Err(AuditFault::new(AuditCode::BadDefect, d.id));
            }
            if self.defects[i + 1..].iter().any(|o| o.id == d.id) {
                return Err(AuditFault::new(AuditCode::BadDefect, d.id));
            }
        }
        // 证据面：自报关闭必须有证据号。这一条必须先于双轨，否则「无证据关闭」
        // 会被双轨判成普通分叉，运维拿不到「去补证据」这个具体动作。
        for d in self.defects.iter() {
            if d.declared_closed && d.evidence_id == 0 {
                return Err(AuditFault::new(AuditCode::EvidenceMissing, d.id));
            }
        }
        // 双轨：两条轨道分属两套记录，不一致即立案。
        for d in self.defects.iter() {
            if !d.tracks_agree() {
                return Err(AuditFault::new(AuditCode::DualTrackDiverge, d.id));
            }
        }
        let (red_open, yellow_open) = (self.open_of(Severity::Red), self.open_of(Severity::Yellow));
        if red_open > 0 {
            return Err(AuditFault::new(AuditCode::RedRemaining, 0));
        }
        if yellow_open > 0 {
            return Err(AuditFault::new(AuditCode::YellowRemaining, 0));
        }
        Ok(AuditReceipt {
            total: self.defects.len() as u16,
            red_open, yellow_open,
            green_open: self.open_of(Severity::Green),
            cases: self.cases().len() as u16,
            batches: BATCH_COUNT,
            fingerprint: self.fingerprint(),
        })
    }

    /// 移交（清点通过才放行，且只放行一次）。
    pub fn hand_over(&mut self) -> Result<HandoverReceipt, AuditFault> {
        if self.handed_over {
            self.rejected = self.rejected.saturating_add(1);
            return Err(AuditFault::new(AuditCode::AlreadyHandedOver, 0));
        }
        let r = self.audit()?;
        self.handed_over = true;
        Ok(HandoverReceipt {
            red_open: r.red_open, yellow_open: r.yellow_open,
            sprint_len: r.green_open, fingerprint: r.fingerprint,
        })
    }

    /// 撤回关闭声明（缺陷复发：自报摘掉，证据保留）。
    ///
    /// 保留证据号是刻意的：复发不等于「当时那份证据是假的」，账本要同时记住
    /// 「曾经修好过」与「现在又坏了」，否则历史被抹掉。
    pub fn withdraw_declaration(&mut self, id: u16) -> Result<(), AuditFault> {
        self.entry_mut(id)?.declared_closed = false;
        Ok(())
    }

    /// 重新打开（红黄复核用：两轨一起摘掉）。
    pub fn reopen(&mut self, id: u16) -> Result<(), AuditFault> {
        let e = self.entry_mut(id)?;
        e.declared_closed = false;
        e.evidenced_closed = false;
        Ok(())
    }

    /// 抹掉证据号（模拟证据台账被清理而批次报告没同步）。
    pub fn strip_evidence(&mut self, id: u16) -> Result<(), AuditFault> {
        self.entry_mut(id)?.evidence_id = 0;
        Ok(())
    }

    /// 生成可归档的清点报告（字段顺序固定，便于 diff）。
    pub fn report(&self) -> String {
        let mut out = String::with_capacity(192);
        out.push_str("defects total=");
        out.push_str(&self.defects.len().to_string());
        out.push(' ');
        out.push_str(&self.counts().line());
        out.push_str("red_open=");
        out.push_str(&self.open_of(Severity::Red).to_string());
        out.push_str(" yellow_open=");
        out.push_str(&self.open_of(Severity::Yellow).to_string());
        out.push_str(" green_open=");
        out.push_str(&self.open_of(Severity::Green).to_string());
        out.push_str(" cases=");
        out.push_str(&self.cases().len().to_string());
        out.push_str(" handed=");
        out.push_str(if self.handed_over { "1" } else { "0" });
        out.push_str(" fp=");
        out.push_str(&hex64(self.fingerprint()));
        out
    }
}

/// FNV-1a 64 单步（质数与偏移基写死，来源：FNV 规范 1a 变体）。
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

/// 64 位十六进制（no_std 手写，避免格式化器的分配）。
pub fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 构造标准 60 条缺陷账（判据夹具）。
///
/// 档位分布 10/20/30、关闭线切在 D-040，是为了让「红黄全关、绿留 20 条待冲刺」
/// 这个移交态自然出现；归属批用 `(id × 7) % 8 + 1` 轮转而非顺序切块，是为了
/// 让待冲刺清单的排序有真实的乱序输入——顺序切块的样本会让「排」与「不排」
/// 输出一致，那条判据就永远绿着却什么也没测。
pub fn standard_ledger() -> Result<DefectLedger, AuditFault> {
    let mut ledger = DefectLedger::new();
    let mut id: u16 = ID_MIN;
    while id <= ID_MAX {
        let severity = match id {
            0..=10 => Severity::Red,
            11..=30 => Severity::Yellow,
            _ => Severity::Green,
        };
        let closed = id <= 40;
        let batch = (((id as u32).saturating_mul(7) % 8) as u8) + 1;
        ledger.admit(DefectEntry::new(id, severity, batch, closed, closed, if closed { id } else { 0 }))?;
        id += 1;
    }
    Ok(ledger)
}

/// 夹具失败时统一返回 false（夹具本身由第一条判据单独证明）。
fn on_standard<F: FnOnce(&mut DefectLedger) -> bool>(f: F) -> bool {
    match standard_ledger() {
        Ok(mut l) => f(&mut l),
        Err(_) => false,
    }
}

/// 取清点结果的诊断码（通过即 None）。
fn fault_of(r: Result<AuditReceipt, AuditFault>) -> Option<u32> {
    r.err().map(|f| f.code)
}

/// 判据入口（UNX-F21591-J1 及配套十五条）。
///
/// 判据里的 60 / 20 / 8 一律写死字面量，不复用被测常量：常量一旦被改，
/// 判据跟着改就成了自证循环。
pub fn run_unx21591_defectaudit_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21591");
    let standard = standard_ledger();

    // 1) 夹具可构造：先证夹具，再证被测逻辑。
    cs.add("f21591.fixture_buildable", standard.is_ok(), "标准 60 条缺陷账构造失败");

    // 2) 总量与三档分布（正向）。
    let split = standard.as_ref().map(|l| {
        let c = l.counts();
        l.len() == 60 && c.red == 10 && c.yellow == 20 && c.green == 30 && c.total() == 60
    });
    cs.add("f21591.total_split", split.unwrap_or(false), "条数或三档分布不符（应 60 = 10/20/30）");

    // 3) 按批分组：8 批全出现、首末批计数与轮转公式一致、合计守恒。
    let hist_ok = standard.as_ref().map(|l| {
        let hist = l.batch_histogram();
        let sum: u16 = hist.iter().map(|x| x.1).sum();
        hist.len() == 8
            && sum == 60
            && hist.first() == Some(&(1u8, 7u16))
            && hist.last() == Some(&(8u8, 8u16))
    });
    cs.add(
        "f21591.batch_histogram",
        hist_ok.unwrap_or(false),
        "按批分组不完整或计数不符（B01 应 7、B08 应 8、合计 60）",
    );

    // 4) 双轨对齐：40 条双轨一致关闭、零立案、绿档留 20（正向）。
    let aligned = standard.as_ref().map(|l| {
        let closed = l.counts().total().saturating_sub(l.open_of(Severity::Green));
        closed == 40 && l.cases().is_empty() && l.open_of(Severity::Green) == 20
    });
    cs.add("f21591.dual_track_aligned", aligned.unwrap_or(false), "标准账双轨不一致或关闭数不为 40");

    // 5) 待冲刺清单：条数与批序（排序不是空转）。
    let sprint_ok = standard.as_ref().map(|l| {
        let s = l.sprint_list();
        s.len() == 20
            && s.first() == Some(&48u16)
            && s.get(1) == Some(&56u16)
            && s.get(10) == Some(&60u16)
            && s.last() == Some(&57u16)
    });
    cs.add(
        "f21591.sprint_order",
        sprint_ok.unwrap_or(false),
        "清单条数或批序不符（应 20 条、首 48/56、第 11 条 60、末 57）",
    );

    // 6) 移交放行：红黄双零、回执指纹等于账本快照且非零（正向）。
    let handover_ok = match standard_ledger() {
        Ok(mut l) => {
            let before = l.fingerprint();
            let r = l.hand_over();
            let receipt_ok = match r {
                Ok(r) => {
                    r.red_open == 0 && r.yellow_open == 0 && r.sprint_len == 20
                        && r.fingerprint == before && r.fingerprint != 0
                }
                Err(_) => false,
            };
            receipt_ok && l.is_handed_over()
        }
        Err(_) => false,
    };
    cs.add("f21591.handover_ok", handover_ok, "移交回执与账本快照不符，或标准账被拒移交");

    // 7) 条数边界：0 与 59 拒、60 放行、第 61 条登记被拒（上限 + 1）。
    let boundary = {
        let empty = fault_of(DefectLedger::new().audit());
        let mut short = DefectLedger::new();
        for id in 1u16..60 {
            let _ = short.admit(DefectEntry::new(id, Severity::Green, 1, false, false, 0));
        }
        let short_code = fault_of(short.audit());
        let mut full = match standard_ledger() {
            Ok(l) => l,
            Err(_) => DefectLedger::new(),
        };
        let full_audit = full.audit().is_ok();
        let overflow = full.admit(DefectEntry::new(61, Severity::Green, 1, false, false, 0)).err();
        empty == Some(0x91052)
            && short_code == Some(0x91052)
            && full_audit
            && overflow.map(|f| f.code) == Some(0x91052)
            && full.len() == 60
    };
    cs.add("f21591.count_boundary", boundary, "0/59 条未被拒、恰 60 条未放行，或第 61 条被写入账本");

    // 8) 红项未清零必拒（反向，断言具体码）。
    let red_blocks = on_standard(|l| {
        let _ = l.reopen(3);
        fault_of(l.audit()) == Some(AuditCode::RedRemaining.code())
    });
    cs.add("f21591.red_blocks", red_blocks, "红项打开后未被拒或码不符");

    // 9) 黄项未清零必拒（反向）。
    let yellow_blocks = on_standard(|l| {
        let _ = l.reopen(15);
        fault_of(l.audit()) == Some(AuditCode::YellowRemaining.code())
    });
    cs.add("f21591.yellow_blocks", yellow_blocks, "黄项打开后未被拒或码不符");

    // 10) 自报关闭却无证据号（反向，且必须先于双轨报出）。
    let evidence_missing = on_standard(|l| {
        let _ = l.strip_evidence(7);
        fault_of(l.audit()) == Some(AuditCode::EvidenceMissing.code())
    });
    cs.add("f21591.evidence_missing", evidence_missing, "无证据关闭未被拒或码不符（不应报成双轨分叉）");

    // 11) 双轨分叉立案（反向）：撤回声明但证据仍在。
    let dual_case = on_standard(|l| {
        let _ = l.withdraw_declaration(25);
        let cases = l.cases();
        let fault = l.audit().err();
        cases.len() == 1
            && cases.first() == Some(&25u16)
            && fault.map(|f| f.code) == Some(AuditCode::DualTrackDiverge.code())
            && fault.map(|f| f.at) == Some(25)
    });
    cs.add("f21591.dual_track_case", dual_case, "双轨分叉未立案，或立案号/码不符");

    // 12) 重号必拒（反向）。
    let dup_ok = {
        let mut l = DefectLedger::new();
        let first = l.admit(DefectEntry::new(1, Severity::Green, 1, false, false, 0));
        let dup = l.admit(DefectEntry::new(1, Severity::Red, 3, false, false, 0)).err().map(|f| f.code);
        first.is_ok() && dup == Some(AuditCode::BadDefect.code()) && l.len() == 1
    };
    cs.add("f21591.duplicate_rejected", dup_ok, "重号未被拒或账本被写入两次");

    // 13) 编号越界必拒（反向，边界 0 与 61）。
    let range_ok = {
        let mut l = DefectLedger::new();
        let low = l.admit(DefectEntry::new(0, Severity::Green, 1, false, false, 0)).err().map(|f| f.code);
        let high = l.admit(DefectEntry::new(61, Severity::Green, 1, false, false, 0)).err().map(|f| f.code);
        low == Some(AuditCode::BadDefect.code()) && high == Some(AuditCode::BadDefect.code()) && l.len() == 0
    };
    cs.add("f21591.id_range_rejected", range_ok, "号段 0 / 61 未被拒或码不符");

    // 14) 移交幂等：二次移交被拒，被拒计数只加一次。
    let once = on_standard(|l| {
        let _ = l.hand_over();
        let before = l.rejected();
        l.hand_over().err().map(|f| f.code) == Some(AuditCode::AlreadyHandedOver.code())
            && l.rejected() == before.saturating_add(1)
    });
    cs.add("f21591.handover_once", once, "二次移交未被拒或被拒计数不符");

    // 15) 指纹确定性 + 对篡改敏感 + 报告可归档。
    let fp_a = standard_ledger().map(|l| l.fingerprint()).unwrap_or(0);
    let fp_b = standard_ledger().map(|l| l.fingerprint()).unwrap_or(1);
    let fp_tamped = standard_ledger()
        .map(|mut l| {
            let _ = l.reopen(3);
            l.fingerprint()
        })
        .unwrap_or(2);
    let report_ok = standard_ledger()
        .map(|l| {
            let r = l.report();
            r.contains("total=60")
                && r.contains("red=10")
                && r.contains("green=30")
                && r.contains("green_open=20")
                && r.contains("cases=0")
                && r.contains("handed=0")
        })
        .unwrap_or(false);
    cs.add(
        "f21591.fingerprint_stable",
        fp_a != 0 && fp_a == fp_b && fp_a != fp_tamped && report_ok,
        "指纹不稳定、对篡改不敏感，或报告字段缺失",
    );

    // 16) 诊断码闭集：八码连续、三要素非空、未知码不得冒充已知码。
    let closed_set = {
        let codes = [
            AuditCode::AuditOk, AuditCode::CountMismatch, AuditCode::RedRemaining,
            AuditCode::YellowRemaining, AuditCode::DualTrackDiverge, AuditCode::EvidenceMissing,
            AuditCode::BadDefect, AuditCode::AlreadyHandedOver,
        ];
        let mut ok = true;
        for (i, c) in codes.iter().enumerate() {
            if c.what().is_empty() || c.why().is_empty() || c.advice().is_empty() {
                ok = false;
            }
            if c.code() != 0x91051u32 + i as u32 {
                ok = false;
            }
        }
        let unknown = AuditFault { code: 0x91099, at: 0 };
        ok && !unknown.is_known() && unknown.as_code() == AuditCode::AlreadyHandedOver
    };
    cs.add("f21591.code_closed_set", closed_set, "码值不连续、三要素为空，或未知码被当成已知码");

    cs
}
