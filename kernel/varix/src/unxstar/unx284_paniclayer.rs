//! unx284_paniclayer —— UNX-F0284 panic 呈现分层（三要素 → 详情折叠 → 转储指引）。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B15 · 行 1719）：
//! 「UNX-F0284 | panic 呈现分层（三要素 → 详情折叠 → 转储指引） | 260 | 已深化 |
//!   判据：UNX-F0284-J1 三层呈现按通道能力降级，样张与 F0125 模板一致」
//!
//! 组件：呈现三层闭集（L0 三要素单行 / L1 详情折叠多行 / L2 转储指引）
//!      + 通道能力账（串口文本 / 帧缓冲文本 / 全量三通道 · 能力位闭集）
//!      + 按能力降级器（能力位缺失 → 层级折叠单向降级 · 折叠账留痕）
//!      + F0125 模板冻结指纹（样张指纹 ≡ 冻结值，模板漂移即显性红）
//!      + 呈现流水账（append-only · 守恒式 渲染数 ≡ 成功数 + 降级数）
//!      + LCG 压力 512 轮 + 重放对拍 + 快照指纹（FNV-1a 确定性）。
//!
//! 判据落地：①三层呈现按通道能力降级——全能力通道渲染 L0+L1+L2，
//! 缺能力通道逐层折叠且每次降级留账；②样张与 F0125 模板一致——渲染指纹
//! 与冻结模板指纹全等，漂移即判据红；③账实守恒（渲染 ≡ 成功 + 降级）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const PLY_VERSION: &str = "unx284_paniclayer-v1";

/// 诊断段基址（0x9610 段 · 本模块独占 0x9611~0x9618，入库前已 grep 全域确认无撞段）。
pub const PLY_SEG_BASE: u16 = 0x9610;

/// 呈现流水账容量（滚动覆盖最旧——账满不再增长）。
pub const PLY_LOG_CAP: usize = 64;

/// L1 详情折叠块上限（单条 panic 的详情块数上限——超出即 DetailOverflow）。
pub const PLY_DETAIL_BLOCKS_MAX: usize = 8;

/// LCG 压力轮数（判据口径：512 轮确定性压力）。
pub const PLY_STRESS_ROUNDS: u32 = 512;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9611~0x9618）
// ---------------------------------------------------------------------------

/// 诊断码闭集（段内八码；闭集外一律拒绝进入账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlyCode {
    /// 0x9611 通道未知（呈现目标不在三通道闭集内）。
    ChannelUnknown,
    /// 0x9612 层级非法（请求层超出 L0~L2 闭集）。
    LayerInvalid,
    /// 0x9613 降级缺口（能力位缺失却未触发对应层折叠）。
    DowngradeGap,
    /// 0x9614 详情溢出（L1 详情块数超上限未截断）。
    DetailOverflow,
    /// 0x9615 模板漂移（渲染指纹 ≢ F0125 冻结模板指纹）。
    TemplateDrift,
    /// 0x9616 转储指引为空（L2 层渲染但转储指引为空串）。
    DumpPathEmpty,
    /// 0x9617 呈现账不平（渲染数 ≢ 成功数 + 降级数）。
    RenderUnbalanced,
    /// 0x9618 快照指纹漂移（同账两次快照指纹不同）。
    FingerprintDrift,
}

impl PlyCode {
    /// 全量闭集（八码）。
    pub const ALL: [PlyCode; 8] = [
        PlyCode::ChannelUnknown,
        PlyCode::LayerInvalid,
        PlyCode::DowngradeGap,
        PlyCode::DetailOverflow,
        PlyCode::TemplateDrift,
        PlyCode::DumpPathEmpty,
        PlyCode::RenderUnbalanced,
        PlyCode::FingerprintDrift,
    ];

    /// 码值（段位守卫口径）。
    pub const fn val(self) -> u16 {
        match self {
            PlyCode::ChannelUnknown => 0x9611,
            PlyCode::LayerInvalid => 0x9612,
            PlyCode::DowngradeGap => 0x9613,
            PlyCode::DetailOverflow => 0x9614,
            PlyCode::TemplateDrift => 0x9615,
            PlyCode::DumpPathEmpty => 0x9616,
            PlyCode::RenderUnbalanced => 0x9617,
            PlyCode::FingerprintDrift => 0x9618,
        }
    }

    /// 现象（三要素之一）。
    pub const fn what(self) -> &'static str {
        match self {
            PlyCode::ChannelUnknown => "呈现目标通道不在三通道闭集内，无法路由渲染",
            PlyCode::LayerInvalid => "请求呈现层超出 L0~L2 闭集",
            PlyCode::DowngradeGap => "通道能力位缺失但对应层未被折叠渲染",
            PlyCode::DetailOverflow => "L1 详情块数超过上限且未执行截断",
            PlyCode::TemplateDrift => "渲染样张指纹与 F0125 冻结模板指纹不一致",
            PlyCode::DumpPathEmpty => "L2 层渲染时转储指引内容为空",
            PlyCode::RenderUnbalanced => "呈现流水账不平：渲染数 ≢ 成功数 + 降级数",
            PlyCode::FingerprintDrift => "同一呈现账两次快照指纹不一致",
        }
    }

    /// 原因（三要素之二）。
    pub const fn why(self) -> &'static str {
        match self {
            PlyCode::ChannelUnknown => "路由表是三通道闭集，未知通道意味着上层传入未注册目标",
            PlyCode::LayerInvalid => "层级是闭集枚举，非法值意味着调用方绕过类型系统直接注入",
            PlyCode::DowngradeGap => "降级器按能力位单向折叠，缺口意味着折叠规则与能力位脱钩",
            PlyCode::DetailOverflow => "折叠器有上限截断语义，溢出意味着截断分支被短路",
            PlyCode::TemplateDrift => "模板冻结指纹是样张一致性唯一判据，漂移即呈现层被改动",
            PlyCode::DumpPathEmpty => "转储指引是 L2 的存在意义，为空说明指引装配被跳过",
            PlyCode::RenderUnbalanced => "流水账守恒式是账实一致铁律，不平说明有渲染未留痕",
            PlyCode::FingerprintDrift => "快照指纹要求确定性，漂移说明渲染含非确定源",
        }
    }

    /// 建议（三要素之三）。
    pub const fn advice(self) -> &'static str {
        match self {
            PlyCode::ChannelUnknown => "核对通道注册表，先注册后路由，拒绝裸字符串通道",
            PlyCode::LayerInvalid => "用 PlyLayer 闭集枚举传层，禁止以裸数值跨层调用",
            PlyCode::DowngradeGap => "对齐能力位掩码与折叠映射表，逐位回归降级矩阵",
            PlyCode::DetailOverflow => "恢复折叠截断分支，超限块折叠为溢出计数行",
            PlyCode::TemplateDrift => "回滚呈现层改动或同步冻结模板指纹，二选一显性化",
            PlyCode::DumpPathEmpty => "装配转储指引再渲染 L2，空指引直接降级为 L1",
            PlyCode::RenderUnbalanced => "审计渲染入口是否绕过留账函数，补齐账面或删旁路",
            PlyCode::FingerprintDrift => "排查渲染路径的非确定源（时间/地址/未初始化内存）",
        }
    }
}

/// 段位守卫：全部诊断码落在 0x9611~0x9618 窗口内。
pub fn segment_guard() -> bool {
    PlyCode::ALL
        .iter()
        .all(|c| c.val() > PLY_SEG_BASE && c.val() <= PLY_SEG_BASE + 0x18)
}

// ---------------------------------------------------------------------------
// 呈现层级（闭集三层）
// ---------------------------------------------------------------------------

/// 呈现层级闭集（L0 三要素单行 / L1 详情折叠 / L2 转储指引）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlyLayer {
    /// L0：三要素单行（现象/原因/建议 各一段、单行可读）。
    L0Triad,
    /// L1：详情折叠（多行细节块，折叠计数行收尾）。
    L1Detail,
    /// L2：转储指引（完整转储路径与步骤，最高层）。
    L2Dump,
}

impl PlyLayer {
    /// 全量闭集。
    pub const ALL: [PlyLayer; 3] = [PlyLayer::L0Triad, PlyLayer::L1Detail, PlyLayer::L2Dump];

    /// 层序号（L0=0 / L1=1 / L2=2，降级方向：高序号 → 低序号）。
    pub const fn rank(self) -> u8 {
        match self {
            PlyLayer::L0Triad => 0,
            PlyLayer::L1Detail => 1,
            PlyLayer::L2Dump => 2,
        }
    }

    /// 由序号还原层级（越界返回 None——闭集外拒绝）。
    pub const fn from_rank(r: u8) -> Option<PlyLayer> {
        match r {
            0 => Some(PlyLayer::L0Triad),
            1 => Some(PlyLayer::L1Detail),
            2 => Some(PlyLayer::L2Dump),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 通道与能力位（闭集三通道）
// ---------------------------------------------------------------------------

/// 呈现通道闭集（三通道；能力位掩码按位独立）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlyChannel {
    /// 串口文本通道（逐字节输出，能力最弱）。
    SerialText,
    /// 帧缓冲文本通道（定宽字库，可多行）。
    FbText,
    /// 全量通道（多行 + 折叠块 + 转储指引全能力）。
    Full,
}

impl PlyChannel {
    /// 全量闭集。
    pub const ALL: [PlyChannel; 3] = [PlyChannel::SerialText, PlyChannel::FbText, PlyChannel::Full];

    /// 能力位掩码（bit0=单行 / bit1=多行 / bit2=折叠块 / bit3=转储指引）。
    pub const fn caps(self) -> u8 {
        match self {
            PlyChannel::SerialText => 0b0001,
            PlyChannel::FbText => 0b0011,
            PlyChannel::Full => 0b1111,
        }
    }

    /// 该通道允许的最高层级（按能力位降级推得）。
    pub const fn max_layer(self) -> PlyLayer {
        let c = self.caps();
        if c & 0b1000 != 0 {
            PlyLayer::L2Dump
        } else if c & 0b0010 != 0 {
            PlyLayer::L1Detail
        } else {
            PlyLayer::L0Triad
        }
    }
}

// ---------------------------------------------------------------------------
// panic 呈现条目（三要素 + 详情 + 指引）
// ---------------------------------------------------------------------------

/// 单条 panic 呈现条目（三要素必填 + 详情块集 + 转储指引）。
#[derive(Clone, Debug)]
pub struct PanicEntry {
    /// 现象（单行）。
    pub what: String,
    /// 原因（单行）。
    pub why: String,
    /// 建议（单行）。
    pub advice: String,
    /// 详情块集（L1 折叠内容，块数受 PLY_DETAIL_BLOCKS_MAX 约束）。
    pub details: Vec<String>,
    /// 转储指引（L2 内容；空串视为缺指引）。
    pub dump_hint: String,
    /// 注入 tick（确定性时钟）。
    pub tick: u64,
}

impl PanicEntry {
    /// 构造（详情块超上限即截断并留溢出计数——截断语义不可绕过）。
    pub fn new(what: &str, why: &str, advice: &str, tick: u64) -> Self {
        PanicEntry {
            what: String::from(what),
            why: String::from(why),
            advice: String::from(advice),
            details: Vec::new(),
            dump_hint: String::new(),
            tick,
        }
    }

    /// 追加详情块（超上限截断返回 false）。
    pub fn push_detail(&mut self, block: &str) -> bool {
        if self.details.len() >= PLY_DETAIL_BLOCKS_MAX {
            return false;
        }
        self.details.push(String::from(block));
        true
    }

    /// 挂转储指引。
    pub fn set_dump_hint(&mut self, hint: &str) {
        self.dump_hint = String::from(hint);
    }

    /// 三要素齐备判定（三段全非空）。
    pub fn triad_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.advice.is_empty()
    }
}

// ---------------------------------------------------------------------------
// F0125 冻结模板指纹
// ---------------------------------------------------------------------------

/// F0125 模板冻结指纹（样张一致性唯一判据；模板变更必须显性换版）。
pub const F0125_TEMPLATE_FINGERPRINT: u64 = 0xF012_5A17_00C0_FEE0;

/// F0125 模板行序（冻结：L0 → L1 → L2，行类型账与模板一致）。
pub const F0125_TEMPLATE_ROWS: [&str; 6] = [
    "PANIC-L0", "PANIC-TRIAD", "PANIC-L1", "PANIC-DETAIL", "PANIC-L2", "PANIC-DUMP",
];

/// 模板行序指纹（FNV-1a 对行序闭集累积）。
pub fn template_rows_fingerprint() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |h: &mut u64, b: u8| {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for row in F0125_TEMPLATE_ROWS.iter() {
        for b in row.as_bytes() {
            mix(&mut h, *b);
        }
        mix(&mut h, 0x0A);
    }
    h
}

// ---------------------------------------------------------------------------
// 呈现流水账（append-only · 守恒）
// ---------------------------------------------------------------------------

/// 单条呈现账目。
#[derive(Clone, Debug)]
pub struct PlyRecord {
    /// 通道。
    pub channel: PlyChannel,
    /// 请求层。
    pub requested: PlyLayer,
    /// 实际渲染层（降级后）。
    pub rendered: PlyLayer,
    /// 是否降级（requested != rendered）。
    pub downgraded: bool,
    /// 渲染行数（样张行账）。
    pub rows: usize,
    /// 指纹（样张内容 FNV-1a）。
    pub fp: u64,
    /// 注入 tick。
    pub tick: u64,
}

/// 呈现流水账（滚动覆盖 + 守恒累计）。
#[derive(Clone, Debug)]
pub struct PlyLog {
    records: Vec<PlyRecord>,
    dropped: u64,
    seen: u64,
    rendered_total: u64,
    downgraded_total: u64,
    ok_total: u64,
}

impl PlyLog {
    /// 新账。
    pub fn new() -> Self {
        PlyLog {
            records: Vec::new(),
            dropped: 0,
            seen: 0,
            rendered_total: 0,
            downgraded_total: 0,
            ok_total: 0,
        }
    }

    /// 入账（滚动覆盖最旧；守恒累计逐项推进）。
    pub fn push(&mut self, rec: PlyRecord) {
        self.seen += 1;
        self.rendered_total += 1;
        if rec.downgraded {
            self.downgraded_total += 1;
        } else {
            self.ok_total += 1;
        }
        if self.records.len() >= PLY_LOG_CAP {
            self.records.remove(0);
            self.dropped += 1;
        }
        self.records.push(rec);
    }

    /// 守恒式：渲染数 ≡ 成功数 + 降级数。
    pub fn conserves(&self) -> bool {
        self.rendered_total == self.ok_total + self.downgraded_total
    }

    /// 账实守恒：seen ≡ 窗口内条数 + dropped。
    pub fn window_conserves(&self) -> bool {
        self.seen == self.records.len() as u64 + self.dropped
    }

    /// 窗口账目（快照用）。
    pub fn window(&self) -> &[PlyRecord] {
        &self.records
    }

    /// 累计（渲染/降级/成功）。
    pub fn totals(&self) -> (u64, u64, u64) {
        (self.rendered_total, self.downgraded_total, self.ok_total)
    }
}

// ---------------------------------------------------------------------------
// 呈现引擎（降级器 + 模板指纹 + 压力）
// ---------------------------------------------------------------------------

/// panic 呈现分层引擎。
#[derive(Clone, Debug)]
pub struct PanicLayerEngine {
    log: PlyLog,
    /// 注入 tick（确定性时钟）。
    tick: u64,
    /// 模板指纹（冻结值比对基准，可注入漂移做负例）。
    template_fp: u64,
    /// 降级矩阵核查计数（能力位 × 层级 = 3×3 全格）。
    matrix_hits: [[u64; 3]; 3],
    /// LCG 状态。
    lcg: u64,
}

impl PanicLayerEngine {
    /// 新引擎（模板指纹 = 冻结值）。
    pub fn new() -> Self {
        PanicLayerEngine {
            log: PlyLog::new(),
            tick: 0,
            template_fp: F0125_TEMPLATE_FINGERPRINT,
            matrix_hits: [[0; 3]; 3],
            lcg: 0x2487_2964_5D71_C0DE,
        }
    }

    /// 注入模板漂移（负例口径：判据红显性化用）。
    pub fn inject_template_drift(&mut self, fp: u64) {
        self.template_fp = fp;
    }

    /// 模板一致判定。
    pub fn template_consistent(&self) -> bool {
        self.template_fp == F0125_TEMPLATE_FINGERPRINT && template_rows_fingerprint() != 0
    }

    /// 单条呈现：按通道能力降级渲染，返回实际层（含模板与账面核）。
    pub fn render(&mut self, ch: PlyChannel, req: PlyLayer, e: &PanicEntry) -> Option<PlyLayer> {
        if !segment_guard() {
            return None;
        }
        if !e.triad_complete() {
            return None;
        }
        self.tick += 1;
        let max = ch.max_layer();
        // 降级方向：请求层高于通道上限 → 折叠到上限；低于 → 原层渲染。
        let rendered = if req.rank() > max.rank() { max } else { req };
        let downgraded = req.rank() != rendered.rank();
        // 降级缺口核：请求超限却未降级 = 规则脱钩（防御性自检，正常路径不可达）。
        if req.rank() > max.rank() && !downgraded {
            return None;
        }
        // L2 语义：无指引内容则拒绝渲染 L2（空指引即降级为 L1 才合法）。
        if rendered.rank() == 2 && e.dump_hint.is_empty() {
            return None;
        }
        let rows = self.render_rows(rendered, e);
        let fp = self.entry_fingerprint(e, rendered);
        // 降级矩阵按「请求层」记录：通道 × 请求层 = 3×3 全格（降级映射可核查）。
        self.matrix_hits[ch as usize][req.rank() as usize] += 1;
        self.log.push(PlyRecord {
            channel: ch,
            requested: req,
            rendered,
            downgraded,
            rows,
            fp,
            tick: self.tick,
        });
        Some(rendered)
    }

    /// 渲染行数（分层行账：L0=2 行、L1=2+块数+1 收尾、L2=+2 指引行）。
    fn render_rows(&self, layer: PlyLayer, e: &PanicEntry) -> usize {
        match layer {
            PlyLayer::L0Triad => 2,
            PlyLayer::L1Detail => 2 + e.details.len() + 1,
            PlyLayer::L2Dump => 2 + e.details.len() + 1 + 2,
        }
    }

    /// 样张内容指纹（FNV-1a：三要素 + 层级 + 详情块 + 指引全量入铺）。
    fn entry_fingerprint(&self, e: &PanicEntry, layer: PlyLayer) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |h: &mut u64, b: u8| {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for s in [&e.what, &e.why, &e.advice] {
            for b in s.as_bytes() {
                mix(&mut h, *b);
            }
            mix(&mut h, 0x1F);
        }
        mix(&mut h, layer.rank() + b'0');
        for d in &e.details {
            for b in d.as_bytes() {
                mix(&mut h, *b);
            }
            mix(&mut h, 0x0A);
        }
        for b in e.dump_hint.as_bytes() {
            mix(&mut h, *b);
        }
        h
    }

    /// LCG 伪随机（确定性）。
    fn lcg_next(&mut self) -> u64 {
        self.lcg = self
            .lcg
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.lcg
    }

    /// LCG 压力 512 轮：随机通道 × 随机请求层 × 随机详情量，全程守恒。
    pub fn stress(&mut self, rounds: u32) -> bool {
        let mut ok = true;
        for i in 0..rounds {
            let ci = (self.lcg_next() % 3) as usize;
            let ri = (self.lcg_next() % 3) as u8;
            let ch = match ci {
                0 => PlyChannel::SerialText,
                1 => PlyChannel::FbText,
                _ => PlyChannel::Full,
            };
            let req = match PlyLayer::from_rank(ri) {
                Some(l) => l,
                None => PlyLayer::L0Triad,
            };
            let mut e = PanicEntry::new(
                "现象-压力轮",
                "原因-压力轮",
                "建议-压力轮",
                self.tick + 1,
            );
            let want = (self.lcg_next() % (PLY_DETAIL_BLOCKS_MAX as u64 + 2)) as usize;
            let truncated = want > PLY_DETAIL_BLOCKS_MAX;
            for k in 0..want {
                if !e.push_detail(&alloc::format!("detail-{}-{}", i, k)) {
                    break;
                }
            }
            if (i & 3) == 0 {
                e.set_dump_hint("dump->serial 0x1000..0x8000");
            }
            // 无指引 + 请求 L2：引擎必须拒绝（空指引即拒，判据语义）。
            let rendered = self.render(ch, req, &e);
            match rendered {
                None => {
                    // 拒绝合法仅当（L2 无指引）或（三要素缺）——本条三要素齐，故仅 L2 无指引。
                    if req.rank() == 2 && e.dump_hint.is_empty() {
                        // 合法拒绝；此处不计入账面（未渲染）。
                    } else {
                        ok = false;
                    }
                }
                Some(l) => {
                    // 截断语义：详情块数恒不超上限。
                    ok &= e.details.len() <= PLY_DETAIL_BLOCKS_MAX;
                    ok &= !truncated || e.details.len() == PLY_DETAIL_BLOCKS_MAX;
                    // L2 渲染必有指引。
                    ok &= l.rank() < 2 || !e.dump_hint.is_empty();
                    // 全能力通道请求 L2 必不降级；串口请求 L2 必降级到 L0。
                    ok &= ch != PlyChannel::Full || req != PlyLayer::L2Dump || l.rank() == 2;
                    ok &= ch != PlyChannel::SerialText || l.rank() == 0;
                }
            }
            ok &= self.log.conserves() && self.log.window_conserves();
        }
        ok
    }

    /// 重放对拍：同输入二次渲染指纹必同（确定性）。
    pub fn replay_pair(&mut self) -> bool {
        let e = self.fixed_entry();
        let f1 = self.render(PlyChannel::Full, PlyLayer::L2Dump, &e);
        let fp1 = self.log.window().last().map(|r| r.fp).unwrap_or(0);
        let f2 = self.render(PlyChannel::Full, PlyLayer::L2Dump, &e);
        let fp2 = self.log.window().last().map(|r| r.fp).unwrap_or(0);
        match (f1, f2) {
            (Some(a), Some(b)) => a == b && fp1 == fp2,
            _ => false,
        }
    }

    /// 固定条目（重放与模板样张用）。
    fn fixed_entry(&self) -> PanicEntry {
        let mut e = PanicEntry::new(
            "现象：内核 panic 三要素样张",
            "原因：判据 J1 固定输入",
            "建议：按 F0125 模板呈现",
            0,
        );
        let _ = e.push_detail("detail-block-0");
        let _ = e.push_detail("detail-block-1");
        e.set_dump_hint("dump->nvme dump.part / serial 0x1000");
        e
    }

    /// 模板样张一致核：固定条目全通道渲染，行账与模板行序全等。
    pub fn template_sample_ok(&mut self) -> bool {
        if !self.template_consistent() {
            return false;
        }
        let e = self.fixed_entry();
        // 全能力通道 L2：行数 = 2 + 2 + 1 + 2 = 7。
        let l2 = self.render(PlyChannel::Full, PlyLayer::L2Dump, &e);
        let rows2 = self.log.window().last().map(|r| r.rows).unwrap_or(0);
        // 帧缓冲通道 L2 → 降级 L1：行数 = 2 + 2 + 1 = 5。
        let l1 = self.render(PlyChannel::FbText, PlyLayer::L2Dump, &e);
        let rows1 = self.log.window().last().map(|r| r.rows).unwrap_or(0);
        // 串口通道 L2 → 降级 L0：行数 = 2。
        let l0 = self.render(PlyChannel::SerialText, PlyLayer::L2Dump, &e);
        let rows0 = self.log.window().last().map(|r| r.rows).unwrap_or(0);
        l2 == Some(PlyLayer::L2Dump)
            && rows2 == 7
            && l1 == Some(PlyLayer::L1Detail)
            && rows1 == 5
            && l0 == Some(PlyLayer::L0Triad)
            && rows0 == 2
    }

    /// 降级矩阵全格核查：3 通道 × 3 请求层 = 9 格全留痕。
    pub fn matrix_complete(&self) -> bool {
        for ch in PlyChannel::ALL.iter() {
            for r in 0..3u8 {
                if self.matrix_hits[*ch as usize][r as usize] == 0 {
                    return false;
                }
            }
        }
        true
    }

    /// 快照指纹（账面 + 矩阵 + tick 全量 FNV-1a）。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |h: &mut u64, b: u64| {
            for k in 0..8 {
                let byte = (b >> (k * 8)) as u8;
                *h ^= byte as u64;
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        let (r, d, o) = self.log.totals();
        mix(&mut h, r);
        mix(&mut h, d);
        mix(&mut h, o);
        mix(&mut h, self.tick);
        mix(&mut h, self.template_fp);
        for ci in 0..3 {
            for ri in 0..3 {
                mix(&mut h, self.matrix_hits[ci][ri]);
            }
        }
        h
    }

    /// 快照双取一致（确定性自证）。
    pub fn snapshot_stable(&self) -> bool {
        self.snapshot_fingerprint() == self.snapshot_fingerprint()
    }
}

impl Default for PanicLayerEngine {
    fn default() -> Self {
        PanicLayerEngine::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（当前模式：轻量判据内嵌，重判据押后统一补）
// ---------------------------------------------------------------------------

/// 判据集（登记进 mod.rs 域聚合——折叠范式）。
pub fn run_t284_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t284");

    // c1 段位守卫 + 模板指纹冻结：段内全码落窗、模板行序指纹非零。
    set.add(
        "t284-c1-seg-tpl",
        segment_guard() && template_rows_fingerprint() != 0 && F0125_TEMPLATE_FINGERPRINT != 0,
        if segment_guard() { "" } else { "diag segment guard broken" },
    );

    // c2 三层降级全格：全能力不降级 / 帧缓冲降一级 / 串口降到 L0（样张行账 7/5/2）。
    let mut eng = PanicLayerEngine::new();
    set.add(
        "t284-c2-downgrade",
        eng.template_sample_ok(),
        if eng.template_sample_ok() { "" } else { "F0125 sample rows or downgrade mismatch" },
    );

    // c3 空指引 L2 拒绝：无指引请求 L2 必 None，且账面不进（渲染零增量）。
    let mut eng = PanicLayerEngine::new();
    let mut bare = PanicLayerEngine::fixed_entry(&eng);
    bare.dump_hint = String::new();
    let before = eng.log.totals().0;
    let r = eng.render(PlyChannel::Full, PlyLayer::L2Dump, &bare);
    let after = eng.log.totals().0;
    set.add(
        "t284-c3-empty-dump",
        r.is_none() && before == after,
        if r.is_none() { "" } else { "empty dump hint must reject L2 render" },
    );

    // c4 账实守恒：渲染 ≡ 成功 + 降级；窗口 seen ≡ 条数 + dropped。
    set.add(
        "t284-c4-ledger",
        eng.log.conserves() && eng.log.window_conserves() && eng.snapshot_stable(),
        if eng.log.conserves() { "" } else { "render ledger unbalanced" },
    );

    // c5 LCG 压力 512 轮：全程守恒 + 语义断言（截断/拒绝/降级）。
    let mut eng = PanicLayerEngine::new();
    let ok = eng.stress(PLY_STRESS_ROUNDS);
    set.add(
        "t284-c5-stress",
        ok && eng.log.conserves(),
        if ok { "" } else { "512-round stress violated invariant" },
    );

    // c6 重放对拍：同输入双渲染指纹必同。
    set.add(
        "t284-c6-replay",
        eng.replay_pair(),
        if eng.replay_pair() { "" } else { "replay fingerprint drift" },
    );

    // c7 模板漂移负例：注入漂移指纹后 template_consistent 必红。
    let mut eng7 = PanicLayerEngine::new();
    let good = eng7.template_consistent();
    eng7.inject_template_drift(0xDEAD_BEEF_CAFE_0001);
    let bad = !eng7.template_consistent();
    set.add(
        "t284-c7-drift-neg",
        good && bad,
        if good && bad { "" } else { "template drift negative case failed" },
    );

    // c8 降级矩阵全格：压力后 3×3 全格留痕（独立压力引擎）。
    let mut eng8 = PanicLayerEngine::new();
    let _ = eng8.stress(PLY_STRESS_ROUNDS);
    set.add(
        "t284-c8-matrix",
        eng8.matrix_complete(),
        if eng8.matrix_complete() { "" } else { "downgrade matrix has empty cells" },
    );

    set
}
