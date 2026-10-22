//! UNX-F21597 · 收官验收（20 维度自评表）
//!
//! 诊断段 0x91081~0x91088（F2-B40 域收官段 17/100）。
//!
//! 判据原文（施工书 UNX-F21597-J1）：20 维度自评表落档，任一低标项显式说明（十章/十四章逐条）。
//!
//! 设计要点（为什么自评表不是「打勾算总分就归档」）：
//! 自评表真正会失效的地方不是「总分算错」，而是「低标项被沉默地算进总分」。所以本模型
//! 把「自评低于门槛」当一等公民：先识别低标，再要求低标自带说明，说明为空的低标项直接
//! 拒收整张表——总分再高也不落档。于是「total=94」永远和「low=1 unexplained=0」一起出现，
//! 归档文本没法只保留好看的那一半。
//!
//! 检查顺序固定为「形状 → 逐维合法性 → 权重合计 → 低标说明 → 总分」：19 维时若先报
//! 「某维自评越界」是误导，运维会去改那一维而不是补维度；权重合计必须排在逐维合法性之后，
//! 因为满分字段本身非法时算出的合计没有意义；总分放最后，因为它是唯一一个「能靠别处多给分
//! 掩盖」的量——把它放前面，低标沉默就会被高分遮住。
//!
//! 满分向量写死 20×5：等权是最难自欺的加权方式（任何一维满分被改动，合计立刻不是 100）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 自评维度数（施工书十章/十四章逐条汇总，恰 20 条）。
pub const DIM_COUNT: usize = 20;
/// 权重合计（百分制，整数加权；不用浮点是因为浮点求和在换编译器时会漂）。
pub const WEIGHT_TOTAL: u16 = 100;
/// 落档合格线（总分低于此值不落档）。
pub const PASS_LINE: u16 = 85;
/// 单维满分上限（超过即视为满分字段写错，而不是「新加了一维」）。
pub const DIM_FULL_MAX: u8 = 20;

/// 自评诊断码闭集（0x91081~0x91088 八码）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入新变体会整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelfScoreCode {
    /// 0x91081 自评表通过（可落档）。
    SheetPass = 0x91081,
    /// 0x91082 维度数不等于 20。
    DimCountOff = 0x91082,
    /// 0x91083 某一维字段非法（满分 0 / 自评越界 / 门槛非法 / 重名）。
    DimMalformed = 0x91083,
    /// 0x91084 低标项没有说明。
    LowNoteMissing = 0x91084,
    /// 0x91085 满分合计不等于 100。
    WeightOff = 0x91085,
    /// 0x91086 总分低于合格线。
    TotalShort = 0x91086,
    /// 0x91087 已落档仍再次落档。
    AlreadyFiled = 0x91087,
    /// 0x91088 空表。
    EmptyTable = 0x91088,
}

impl SelfScoreCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            SelfScoreCode::SheetPass => "自评表通过，可落档",
            SelfScoreCode::DimCountOff => "维度数与 20 不符",
            SelfScoreCode::DimMalformed => "某一维字段非法",
            SelfScoreCode::LowNoteMissing => "低标项没有说明",
            SelfScoreCode::WeightOff => "满分合计不等于 100",
            SelfScoreCode::TotalShort => "总分低于合格线",
            SelfScoreCode::AlreadyFiled => "自评表已落档仍再次落档",
            SelfScoreCode::EmptyTable => "自评表为空",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            SelfScoreCode::SheetPass => "二十维齐、权重合、低标均有说明、总分达线",
            SelfScoreCode::DimCountOff => "十章/十四章条目被漏抄或多抄",
            SelfScoreCode::DimMalformed => "满分/自评/门槛三者不自洽，或维度名重复",
            SelfScoreCode::LowNoteMissing => "自评低于门槛却被当成正常项计入总分",
            SelfScoreCode::WeightOff => "某一维满分被改动而其余未同步",
            SelfScoreCode::TotalShort => "自评过于保守，或低标拖累了合计",
            SelfScoreCode::AlreadyFiled => "落档是终态操作，重复执行会覆盖归档文本",
            SelfScoreCode::EmptyTable => "尚未装入任何维度",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SelfScoreCode::SheetPass => "导出归档文本并交由验收方比对",
            SelfScoreCode::DimCountOff => "按十章/十四章逐条补齐或删除多余维度",
            SelfScoreCode::DimMalformed => "核对 0 < 门槛 <= 满分、自评 <= 满分，并去重维度名",
            SelfScoreCode::LowNoteMissing => "为低标项补写差距与整改计划后再提交",
            SelfScoreCode::WeightOff => "重算满分向量，使其合计恰为 100",
            SelfScoreCode::TotalShort => "补齐欠项或走「未达线」复核，不要篡改自评",
            SelfScoreCode::AlreadyFiled => "读取既有归档，改走追加说明而非重写",
            SelfScoreCode::EmptyTable => "先装入 20 维再评估",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会静默截断成 0x1081）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 单个自评维度。
///
/// `note` 用 `String` 而非 `&'static str`：说明是要被落档的正文，由提交方逐条填写，
/// 不是编译期常量。低标判据只看 `score < floor`，与说明无关——说明只决定「能不能落档」，
/// 不决定「是不是低标」，两个判断必须分开，否则补上说明就能把低标洗白。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dimension {
    /// 维度名（唯一，非空）。
    pub name: &'static str,
    /// 满分（权重）。
    pub full: u8,
    /// 自评分。
    pub score: u8,
    /// 门槛（低于它即为低标项）。
    pub floor: u8,
    /// 差距说明（低标项必填）。
    pub note: String,
}

impl Dimension {
    /// 构造一维。
    pub fn new(name: &'static str, full: u8, score: u8, floor: u8, note: &str) -> Dimension {
        Dimension { name, full, score, floor, note: note.to_string() }
    }

    /// 是否低标（严格小于门槛；等于门槛算达标）。
    pub fn is_low(&self) -> bool {
        self.score < self.floor
    }

    /// 是否已写说明（纯空白视为未写：`" "` 不构成说明）。
    pub fn noted(&self) -> bool {
        !self.note.trim().is_empty()
    }

    /// 单维自洽性：名非空、满分在 (0, DIM_FULL_MAX]、自评 <= 满分、门槛在 (0, 满分]。
    ///
    /// 门槛不允许为 0：门槛 0 等于「任何自评都达标」，是一个把低标判据整条废掉的
    /// 退化输入，必须在合法性层面拦下，而不是让它在后面「恰好不触发」。
    pub fn well_formed(&self) -> bool {
        !self.name.is_empty()
            && self.full > 0
            && self.full <= DIM_FULL_MAX
            && self.score <= self.full
            && self.floor > 0
            && self.floor <= self.full
    }
}

/// 评估回执（落档凭据的数值面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditReport {
    /// 维度数。
    pub dims: u16,
    /// 总分。
    pub total: u16,
    /// 满分合计。
    pub weight: u16,
    /// 低标项数。
    pub low: u16,
    /// 未说明的低标项数（通过时必须为 0）。
    pub unexplained: u16,
    /// 评分表指纹（FNV-1a 64，逐维咬合）。
    pub fingerprint: u64,
}

impl AuditReport {
    /// 是否达到落档条件（四条同时成立，缺一不可）。
    pub fn is_pass(&self) -> bool {
        self.dims as usize == DIM_COUNT
            && self.weight == WEIGHT_TOTAL
            && self.unexplained == 0
            && self.total >= PASS_LINE
    }

    /// 低标摘要（固定字段序，直接进归档文本首行）。
    pub fn low_brief(&self) -> String {
        let mut out = String::with_capacity(32);
        out.push_str("low=");
        out.push_str(&self.low.to_string());
        out.push_str(" unexplained=");
        out.push_str(&self.unexplained.to_string());
        out
    }
}

/// 自评故障（码 + 定位维度，缺一不可）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelfScoreFault {
    /// 诊断码（u32：五位数码段）。
    pub code: u32,
    /// 定位维度（1-based；0 表示与具体维度无关）。
    pub dim: u8,
    /// 定位维度名（无则为空串）。
    pub name: &'static str,
}

impl SelfScoreFault {
    const fn new(code: SelfScoreCode, dim: u8, name: &'static str) -> SelfScoreFault {
        SelfScoreFault { code: code.code(), dim, name }
    }

    /// 把码还原成枚举，便于取三要素文本。
    ///
    /// 闭集外不 panic：保守落回 `EmptyTable`（「未知即可疑」）。判据里另有码值断言，
    /// 不会因为这条回退而把未知码当成合格。
    pub const fn as_code(self) -> SelfScoreCode {
        match self.code {
            0x91081 => SelfScoreCode::SheetPass,
            0x91082 => SelfScoreCode::DimCountOff,
            0x91083 => SelfScoreCode::DimMalformed,
            0x91084 => SelfScoreCode::LowNoteMissing,
            0x91085 => SelfScoreCode::WeightOff,
            0x91086 => SelfScoreCode::TotalShort,
            0x91087 => SelfScoreCode::AlreadyFiled,
            _ => SelfScoreCode::EmptyTable,
        }
    }

    /// 三要素拼一行（运维面板直读，不经格式化器）。
    pub fn text(&self) -> String {
        let c = self.as_code();
        let mut out = String::with_capacity(128);
        out.push_str(c.what());
        out.push_str(" | ");
        out.push_str(c.why());
        out.push_str(" | ");
        out.push_str(c.advice());
        out
    }
}

/// 自评表。
///
/// 落档状态与内容同住一个结构：分开存就会出现「文本已归档而 `filed` 仍是 false」的
/// 中间态，重复落档判据也就无从成立。
pub struct ScoreSheet {
    dims: Vec<Dimension>,
    filed: bool,
    /// 成功落档次数（幂等观测点：成功一次后恒为 1）。
    filed_count: u16,
    /// 被拒落档次数（重复落档也要留痕，不静默）。
    rejected_files: u16,
}

impl ScoreSheet {
    /// 空表。
    pub const fn new() -> ScoreSheet {
        ScoreSheet { dims: Vec::new(), filed: false, filed_count: 0, rejected_files: 0 }
    }

    /// 由维度向量直接建表（夹具用；不做校验，校验在 `evaluate`）。
    pub fn from_dims(dims: Vec<Dimension>) -> ScoreSheet {
        ScoreSheet { dims, filed: false, filed_count: 0, rejected_files: 0 }
    }

    /// 追加一维。
    ///
    /// 刻意不在这里做「维度数上限」检查：上限是 `evaluate` 的事，`push` 只负责装。
    /// 若两处都判，构造 21 维反例就得绕过 `push`，反例本身也就不可信了。
    pub fn push(&mut self, d: Dimension) {
        self.dims.push(d);
    }

    /// 维度数。
    pub fn len(&self) -> usize {
        self.dims.len()
    }

    /// 只读视图。
    pub fn dims(&self) -> &[Dimension] {
        &self.dims
    }

    /// 可写视图（判据构造反例用：改一维的满分/自评/门槛/名字）。
    pub fn dims_mut(&mut self) -> &mut [Dimension] {
        &mut self.dims
    }

    /// 总分（整数求和；饱和加法避免溢出绕过合格线）。
    pub fn total(&self) -> u16 {
        let mut sum = 0u16;
        let mut i = 0usize;
        while i < self.dims.len() {
            sum = sum.saturating_add(self.dims[i].score as u16);
            i += 1;
        }
        sum
    }

    /// 满分合计（权重合计）。
    pub fn weight(&self) -> u16 {
        let mut sum = 0u16;
        let mut i = 0usize;
        while i < self.dims.len() {
            sum = sum.saturating_add(self.dims[i].full as u16);
            i += 1;
        }
        sum
    }

    /// 低标项数（不看说明）。
    pub fn low_count(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.dims.len() {
            if self.dims[i].is_low() {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 低标且无说明的项数（这一项必须是 0 才谈得上落档）。
    pub fn unexplained_count(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.dims.len() {
            let d = &self.dims[i];
            if d.is_low() && !d.noted() {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 评分表指纹：逐维把 (名字节, 满分, 自评, 门槛, 说明字节) 喂进 FNV-1a 64。
    ///
    /// 逐字段喂而不是喂结构体内存：内存含 padding，未初始化字节会让指纹在换编译器
    /// 版本时漂移，跨版本复核就永远对不上。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < self.dims.len() {
            let d = &self.dims[i];
            h = fnv_bytes(h, d.name.as_bytes());
            h = fnv_u64(h, d.full as u64);
            h = fnv_u64(h, d.score as u64);
            h = fnv_u64(h, d.floor as u64);
            h = fnv_bytes(h, d.note.as_bytes());
            i += 1;
        }
        h
    }

    /// 评估（检查顺序见模块头；只报第一因）。
    pub fn evaluate(&self) -> Result<AuditReport, SelfScoreFault> {
        if self.dims.is_empty() {
            return Err(SelfScoreFault::new(SelfScoreCode::EmptyTable, 0, ""));
        }
        if self.dims.len() != DIM_COUNT {
            let n = self.dims.len() as u8;
            return Err(SelfScoreFault::new(SelfScoreCode::DimCountOff, n, ""));
        }
        // 重名检测用 O(n²) 双循环：n 恒为 20，建索引的分配成本比省下的比较更大。
        let mut i = 0usize;
        while i < self.dims.len() {
            let d = &self.dims[i];
            let at = (i as u8).saturating_add(1);
            if !d.well_formed() {
                return Err(SelfScoreFault::new(SelfScoreCode::DimMalformed, at, d.name));
            }
            let mut j = 0usize;
            while j < i {
                if self.dims[j].name == d.name {
                    return Err(SelfScoreFault::new(SelfScoreCode::DimMalformed, at, d.name));
                }
                j += 1;
            }
            i += 1;
        }
        let weight = self.weight();
        if weight != WEIGHT_TOTAL {
            return Err(SelfScoreFault::new(SelfScoreCode::WeightOff, 0, ""));
        }
        let mut i = 0usize;
        while i < self.dims.len() {
            let d = &self.dims[i];
            if d.is_low() && !d.noted() {
                let at = (i as u8).saturating_add(1);
                return Err(SelfScoreFault::new(SelfScoreCode::LowNoteMissing, at, d.name));
            }
            i += 1;
        }
        let total = self.total();
        if total < PASS_LINE {
            return Err(SelfScoreFault::new(SelfScoreCode::TotalShort, 0, ""));
        }
        Ok(AuditReport {
            dims: self.dims.len() as u16,
            total,
            weight,
            low: self.low_count(),
            unexplained: self.unexplained_count(),
            fingerprint: self.fingerprint(),
        })
    }

    /// 归档文本（字段序固定，便于逐行 diff）。
    ///
    /// 首行是「一句话结论」，其后每维一行：验收方先看首行决定要不要往下读，逐维行
    /// 只用于定位。字段序一旦固定，任何字段挪位都会在 diff 里显形。
    pub fn archive(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str("selfscore v1 dims=");
        out.push_str(&self.dims.len().to_string());
        out.push_str(" total=");
        out.push_str(&self.total().to_string());
        out.push_str(" weight=");
        out.push_str(&self.weight().to_string());
        out.push_str(" low=");
        out.push_str(&self.low_count().to_string());
        out.push_str(" unexplained=");
        out.push_str(&self.unexplained_count().to_string());
        out.push_str(" fp=");
        out.push_str(&hex64(self.fingerprint()));
        out.push('\n');
        let mut i = 0usize;
        while i < self.dims.len() {
            let d = &self.dims[i];
            out.push_str("d");
            if i + 1 < 10 {
                out.push('0');
            }
            out.push_str(&(i + 1).to_string());
            out.push_str(" name=");
            out.push_str(d.name);
            out.push_str(" full=");
            out.push_str(&d.full.to_string());
            out.push_str(" score=");
            out.push_str(&d.score.to_string());
            out.push_str(" floor=");
            out.push_str(&d.floor.to_string());
            out.push_str(" note=");
            out.push_str(if d.note.is_empty() { "-" } else { d.note.as_str() });
            out.push('\n');
            i += 1;
        }
        out
    }

    /// 落档（终态操作）：先评估，评估不过一律不落。
    ///
    /// 落档与评估分两步而不是「评估时顺手落档」：验收方需要能对同一张表反复评估
    /// （改一维再评），只有确认后才落档；合并则会让每次试探性评估都改状态。
    pub fn file(&mut self) -> Result<u16, SelfScoreFault> {
        if self.filed {
            self.rejected_files = self.rejected_files.saturating_add(1);
            return Err(SelfScoreFault::new(SelfScoreCode::AlreadyFiled, 0, ""));
        }
        let report = self.evaluate()?;
        self.filed = true;
        self.filed_count = self.filed_count.saturating_add(1);
        Ok(report.total)
    }

    /// 是否已落档。
    pub const fn is_filed(&self) -> bool {
        self.filed
    }

    /// 成功落档次数。
    pub const fn filed_count(&self) -> u16 {
        self.filed_count
    }

    /// 被拒落档次数。
    pub const fn rejected_files(&self) -> u16 {
        self.rejected_files
    }
}

/// FNV-1a 64 单步（质数与偏移基来源：FNV 规范 1a 变体）。
fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// FNV-1a 64 字节串单步（维度名与说明是变长文本，必须按字节喂）。
fn fnv_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免格式化器的分配）。
fn hex64(v: u64) -> String {
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

/// 20 维名表（十章/十四章逐条，顺序即归档顺序）。
const DIM_NAMES: [&str; DIM_COUNT] = [
    "内核态实现",
    "no_std 纯度",
    "判据覆盖",
    "诊断三要素",
    "幂等确定性",
    "边界钉死",
    "双路对拍",
    "账链指纹",
    "零警告编译",
    "文件行数",
    "注释占比",
    "夹具先行",
    "反向覆盖",
    "码段合规",
    "u32 码型",
    "域归属",
    "留痕可归档",
    "重复落档幂等",
    "低标说明",
    "终条对齐",
];

/// 标准自评分数向量（合计 94：四个 4 分项各欠 1，一个 3 分项欠 2）。
const BASE_SCORES: [u8; DIM_COUNT] = [
    5, 5, 4, 5, 5, 4, 5, 5, 5, 4, 5, 5, 5, 5, 5, 4, 3, 5, 5, 5,
];

/// 标准门槛向量（统一 4 分：等于门槛算达标，只有 3 分那一维触发低标）。
const BASE_FLOORS: [u8; DIM_COUNT] = [4; DIM_COUNT];

/// 标准体量：每维满分 5（20 × 5 = 100）。
const BASE_FULL: u8 = 5;

/// 低标说明的标准文本（十章/十四章复核口径）。
const LOW_NOTE: &str = "第十批补测未覆盖，差距 1 分，已列入十四章节复核";

/// 构造标准 20 维自评表（判据夹具）。
///
/// 返回 `Result` 而不是直出：夹具自身必须能通过评估，否则整册判据会被一个不合格的
/// 夹具挡在门外，看着全绿其实一条都没跑过。
pub fn standard_sheet() -> Result<ScoreSheet, SelfScoreFault> {
    let mut sheet = ScoreSheet::new();
    let mut i = 0usize;
    while i < DIM_COUNT {
        let note = if BASE_SCORES[i] < BASE_FLOORS[i] { LOW_NOTE } else { "" };
        sheet.push(Dimension::new(DIM_NAMES[i], BASE_FULL, BASE_SCORES[i], BASE_FLOORS[i], note));
        i += 1;
    }
    // 夹具自证：评估不过就在构造期暴露，而不是留给下游判据去猜。
    sheet.evaluate()?;
    Ok(sheet)
}

/// 构造任意维数的表（`n < 20` 取前 n 维；`n > 20` 追加「尾部补维」）。
///
/// 补维若共用既有名会被重名闸先拦下，测不到「维数不对」这条，故补维自带唯一名字。
pub fn sheet_with_count(n: usize) -> ScoreSheet {
    let mut dims: Vec<Dimension> = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        if i < DIM_COUNT {
            let d = Dimension::new(
                DIM_NAMES[i],
                BASE_FULL,
                BASE_SCORES[i],
                BASE_FLOORS[i],
                "补测覆盖",
            );
            dims.push(d);
        } else {
            dims.push(Dimension::new("尾部补维", BASE_FULL, 5, 4, ""));
        }
        i += 1;
    }
    ScoreSheet::from_dims(dims)
}

/// 构造总分恰为 `target` 的表（所有维度先填说明，再从末维往前削分）。
///
/// 先填说明是刻意的：不填说明时「削分」会先触发低标闸，测不到总分红线。
/// 削分从末维开始，保证同一 target 永远得到同一张表（判据可重放）。
pub fn sheet_at_total(target: u16) -> Result<ScoreSheet, SelfScoreFault> {
    let mut sheet = standard_sheet()?;
    let mut i = 0usize;
    while i < sheet.len() {
        sheet.dims_mut()[i].note = "补测覆盖，低标已说明".to_string();
        i += 1;
    }
    let mut total = sheet.total();
    let mut cursor = sheet.len();
    while total > target && cursor > 0 {
        cursor -= 1;
        while total > target && sheet.dims_mut()[cursor].score > 0 {
            sheet.dims_mut()[cursor].score -= 1;
            total -= 1;
        }
    }
    if total != target {
        return Err(SelfScoreFault::new(SelfScoreCode::TotalShort, 0, ""));
    }
    Ok(sheet)
}

/// 判据入口（UNX-F21597-J1 及其配套十四条）。
///
/// 判据常量独立写死（94 / 100 / 20 / 85 / 84），不复用被测代码的常量名：
/// 若断言写成 `total == WEIGHT_TOTAL`，把常量改成 101 时判据会跟着变，等于自证。
pub fn run_unx21597_selfscore_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21597");
    let standard = standard_sheet();

    // 1) 夹具可构造（先证夹具，再证被测逻辑）。
    cs.add("f21597.fixture_buildable", standard.is_ok(), "标准 20 维自评表构造失败");

    // 2) 标准表通过，且四个关键数值同时命中（少一个都可能是「碰巧过线」）。
    if let Ok(sheet) = &standard {
        let verdict = match sheet.evaluate() {
            Ok(r) => {
                r.dims == 20
                    && r.total == 94
                    && r.weight == 100
                    && r.low == 1
                    && r.unexplained == 0
                    && r.is_pass()
                    && r.low_brief() == "low=1 unexplained=0"
            }
            Err(_) => false,
        };
        cs.add("f21597.standard_accepted", verdict, "标准表被拒或关键数值不符");
    } else {
        cs.add("f21597.standard_accepted", false, "夹具不可用");
    }

    // 3) 边界：19 维（少一维）。
    let code = sheet_with_count(19).evaluate().err().map(|f| f.code);
    cs.add(
        "f21597.dim_count_19",
        code == Some(SelfScoreCode::DimCountOff.code()),
        "19 维未被拒或码不符",
    );

    // 4) 边界：21 维（多一维）。
    let code = sheet_with_count(21).evaluate().err().map(|f| f.code);
    cs.add(
        "f21597.dim_count_21",
        code == Some(SelfScoreCode::DimCountOff.code()),
        "21 维未被拒或码不符",
    );

    // 5) 边界：0 维（空表）。
    let code = sheet_with_count(0).evaluate().err().map(|f| f.code);
    cs.add(
        "f21597.empty_table",
        code == Some(SelfScoreCode::EmptyTable.code()),
        "空表未被拒或码不符",
    );

    // 6) 反向：自评越过满分。
    let mut sheet = sheet_with_count(DIM_COUNT);
    sheet.dims_mut()[0].score = 6;
    let mut ok = false;
    if let Err(f) = sheet.evaluate() {
        ok = f.code == SelfScoreCode::DimMalformed.code() && f.dim == 1;
    }
    cs.add("f21597.score_over_full", ok, "自评 6 > 满分 5 未被拒或定位错");

    // 7) 反向：门槛为 0（把低标判据整条废掉的退化输入）。
    let mut sheet = sheet_with_count(DIM_COUNT);
    sheet.dims_mut()[1].floor = 0;
    let mut ok = false;
    if let Err(f) = sheet.evaluate() {
        ok = f.code == SelfScoreCode::DimMalformed.code();
    }
    cs.add("f21597.floor_zero", ok, "门槛 0 未被拒或码不符");

    // 8) 反向：维度重名（重名会让「20 维」变成「19 个不同维度 + 1 个重复」）。
    let mut sheet = sheet_with_count(DIM_COUNT);
    let dup = sheet.dims()[0].name;
    sheet.dims_mut()[1].name = dup;
    let mut ok = false;
    if let Err(f) = sheet.evaluate() {
        ok = f.code == SelfScoreCode::DimMalformed.code() && f.name == dup;
    }
    cs.add("f21597.dup_name", ok, "重名维度未被拒或定位错");

    // 9) 反向：满分合计 101（单维满分被改而其余未同步）。
    let mut sheet = sheet_with_count(DIM_COUNT);
    sheet.dims_mut()[3].full = 6;
    let mut ok = false;
    if let Err(f) = sheet.evaluate() {
        ok = f.code == SelfScoreCode::WeightOff.code();
    }
    cs.add("f21597.weight_off", ok, "满分 101 未被拒或码不符");

    // 10) 反向：低标项无说明（本单最核心的一条——低标不许沉默）。
    let mut ok = false;
    if let Ok(mut sheet) = standard_sheet() {
        let mut i = 0usize;
        let mut cleared = false;
        while i < sheet.len() {
            if sheet.dims()[i].is_low() {
                sheet.dims_mut()[i].note = String::new();
                cleared = true;
            }
            i += 1;
        }
        if cleared {
            if let Err(f) = sheet.evaluate() {
                ok = f.code == SelfScoreCode::LowNoteMissing.code()
                    && f.dim == 17
                    && f.text().contains("低标项没有说明");
            }
        }
    }
    cs.add(
        "f21597.low_note_missing",
        ok,
        "低标说明被清空后仍被接纳，或码/定位/三要素文本不符",
    );

    // 11) 正向对照：同样的低标项，说明非空即放行（说明只影响落档，不洗白低标）。
    let mut ok = false;
    if let Ok(sheet) = standard_sheet() {
        if let Ok(r) = sheet.evaluate() {
            ok = r.low == 1 && r.unexplained == 0 && r.total == 94;
        }
    }
    cs.add(
        "f21597.low_note_present_ok",
        ok,
        "低标项有说明却被拒，或低标计数被说明抹掉",
    );

    // 12) 边界：总分恰为合格线 85（必须放行）。
    let at_line = match sheet_at_total(85) {
        Ok(sheet) => match sheet.evaluate() {
            Ok(r) => r.total == 85,
            Err(_) => false,
        },
        Err(_) => false,
    };
    cs.add("f21597.total_at_line", at_line, "总分 85 未放行");

    // 13) 边界：总分 84（必须拒，且码为短账）。
    let mut ok = false;
    if let Ok(sheet) = sheet_at_total(84) {
        if let Err(f) = sheet.evaluate() {
            ok = f.code == SelfScoreCode::TotalShort.code();
        }
    }
    cs.add("f21597.total_below_line", ok, "总分 84 未被拒或码不符");

    // 14) 幂等：重复落档被拒、落档计数只加一次、归档文本逐字节相同。
    let mut ok = false;
    if let Ok(mut sheet) = standard_sheet() {
        let first = sheet.file();
        let filed_before = sheet.filed_count();
        let second = sheet.file();
        ok = first == Ok(94u16)
            && second.err().map(|f| f.code) == Some(SelfScoreCode::AlreadyFiled.code())
            && filed_before == 1
            && sheet.filed_count() == 1
            && sheet.is_filed()
            && sheet.rejected_files() == 1
            && sheet.archive() == sheet.archive();
    }
    cs.add(
        "f21597.file_idempotent",
        ok,
        "重复落档未被拒，或落档计数/归档文本不稳定",
    );

    // 15) 归档文本：首行字段序固定、逐维 20 行、说明与留空标记都在。
    let mut ok = false;
    if let Ok(sheet) = standard_sheet() {
        let t = sheet.archive();
        ok = t.starts_with("selfscore v1 dims=20 total=94 weight=100 low=1 unexplained=0 ")
            && t.contains(" fp=")
            && t.lines().count() == 21
            && t.contains("d17 name=留痕可归档")
            && t.contains("note=-")
            && t.contains("note=第十批补测未覆盖");
    }
    cs.add("f21597.archive_fields", ok, "归档文本缺字段、行数不为 21 或说明未落档");

    cs
}
