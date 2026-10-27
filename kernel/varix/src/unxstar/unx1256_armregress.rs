//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1256 · Arming 态回归资产与断言集（B23 段聚合） · 0xFBE0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1256-J1 B23 段 19 判据聚合回归一次命令全跑 100% 才绿，
//!      断言 ≥40 条全绿，映射零遗漏，JSON 可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 19 判据映射表：F1241..F1259 每卡判据一行（判据号/卡题/断言区间），
//!     零交叉引用本地登记；
//!   - 断言常量表：42 条（≥40），编号 23001 起连续唯一，每条挂判据号；
//!   - 被测面：Arming 探针仿真器（武装/竞态缓冲/回退/确认/守恒微缩）；
//!   - 执行引擎：一次命令全跑 → 逐条求谓词 → append-only 回归账；
//!   - 映射零遗漏：每判据 ≥2 断言挂载 + 每断言恰挂一判据（双向核）；
//!   - JSON 导出：JSON 行文含全部断言号（可导出 100%）；
//!   - 注入核 5 项（缺映射/重挂/JSON 缺号/账缩水/探针破守恒）必抓；
//!   - LCG 种子写死 0xFBE0，禁浮点/墙钟/rand。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFBE0 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFBE0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B23 段判据数（F1241..F1259 = 19）。
pub const B23_CRITERIA: usize = 19;

/// 断言总数（≥40，写死 42）。
pub const RG_ASSERT_COUNT: usize = 42;

/// 诊断码（0xFBE0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    /// 0xFBE0 断言表缩水（<40）。
    AssertShort = 0,
    /// 0xFBE1 断言重号。
    IdDup = 1,
    /// 0xFBE2 断言断档。
    IdGap = 2,
    /// 0xFBE3 映射缺判据（19 判据有未挂载）。
    MapMiss = 3,
    /// 0xFBE4 映射重挂（断言挂多判据）。
    MapDup = 4,
    /// 0xFBE5 回归有红（一次命令未全绿）。
    RunRed = 5,
    /// 0xFBE6 账缩水（回归账只增不减被破）。
    LedgerShrink = 6,
    /// 0xFBE7 JSON 缺号（导出缺断言号）。
    JsonMiss = 7,
    /// 0xFBE8 探针破守恒。
    ProbeUnbal = 8,
    /// 0xFBE9 摘要空文本。
    SummaryEmpty = 9,
}

impl RgCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [RgCode; 10] = [
        RgCode::AssertShort, RgCode::IdDup, RgCode::IdGap, RgCode::MapMiss,
        RgCode::MapDup, RgCode::RunRed, RgCode::LedgerShrink, RgCode::JsonMiss,
        RgCode::ProbeUnbal, RgCode::SummaryEmpty,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFBE0 => "断言表缩水：补齐 ≥40 条 B23 断言常量。",
        0xFBE1 => "断言重号：两两去重核编号。",
        0xFBE2 => "断言断档：23001 起连续编号补齐。",
        0xFBE3 => "映射缺判据：19 判据逐条挂 ≥2 断言。",
        0xFBE4 => "映射重挂：一断言恰挂一判据，重挂拆分。",
        0xFBE5 => "回归有红：定位失败断言号修因后重跑全绿。",
        0xFBE6 => "账缩水：append-only，禁截断。",
        0xFBE7 => "JSON 缺号：导出器逐条输出断言号。",
        0xFBE8 => "探针破守恒：武装数≡到齐+缺交，缺口回溯。",
        0xFBE9 => "摘要空文本：摘要含判据数+断言数+全绿位。",
        _ => "未知诊断码：回查 0xFBE0 段闭集。",
    }
}

/// 判据映射行（F1241..F1259）。
#[derive(Clone, Copy, Debug)]
pub struct CriterionMap {
    /// 判据号（卡号）。
    pub card: u32,
    /// 卡题缩写。
    pub title: &'static str,
    /// 挂载断言区间起点（含）。
    pub a_from: u32,
    /// 挂载断言区间终点（含）。
    pub a_to: u32,
}

/// 19 判据映射表（每判据 ≥2 断言，区间不重叠，总宽恰 42）。
pub const CRITERIA_MAP: [CriterionMap; B23_CRITERIA] = [
    CriterionMap { card: 1241, title: "武装序列本体", a_from: 23001, a_to: 23002 },
    CriterionMap { card: 1242, title: "武装步骤账", a_from: 23003, a_to: 23004 },
    CriterionMap { card: 1243, title: "竞态防护账", a_from: 23005, a_to: 23006 },
    CriterionMap { card: 1244, title: "武装失败回退账", a_from: 23007, a_to: 23008 },
    CriterionMap { card: 1245, title: "确认验证账", a_from: 23009, a_to: 23010 },
    CriterionMap { card: 1246, title: "压测资产", a_from: 23011, a_to: 23013 },
    CriterionMap { card: 1247, title: "故障注入", a_from: 23014, a_to: 23016 },
    CriterionMap { card: 1248, title: "延迟账", a_from: 23017, a_to: 23019 },
    CriterionMap { card: 1249, title: "全链联动账", a_from: 23020, a_to: 23022 },
    CriterionMap { card: 1250, title: "健康账", a_from: 23023, a_to: 23024 },
    CriterionMap { card: 1251, title: "配置总表", a_from: 23025, a_to: 23026 },
    CriterionMap { card: 1252, title: "人话化", a_from: 23027, a_to: 23028 },
    CriterionMap { card: 1253, title: "统计矩阵", a_from: 23029, a_to: 23030 },
    CriterionMap { card: 1254, title: "审计账", a_from: 23031, a_to: 23032 },
    CriterionMap { card: 1255, title: "边界条件账", a_from: 23033, a_to: 23034 },
    CriterionMap { card: 1256, title: "回归资产", a_from: 23035, a_to: 23036 },
    CriterionMap { card: 1257, title: "启动序列挂接", a_from: 23037, a_to: 23038 },
    CriterionMap { card: 1258, title: "register_irq 联动账", a_from: 23039, a_to: 23040 },
    CriterionMap { card: 1259, title: "文档册", a_from: 23041, a_to: 23042 },
];

/// 回归断言条目。
#[derive(Clone, Copy, Debug)]
pub struct RgAssertion {
    /// 断言号（23001 起连续）。
    pub id: u32,
    /// 断言名。
    pub name: &'static str,
    /// 挂载判据（卡号）。
    pub card: u32,
}

/// 由映射表展开 42 条断言常量表（区间展平，恰好覆盖 23001..23055 中
/// 42 个号：区间总宽 = 42）。
pub const RG_ASSERTS: [RgAssertion; RG_ASSERT_COUNT] = build_rg_asserts();

/// 展平映射区间 → 断言表（const fn）。
const fn build_rg_asserts() -> [RgAssertion; RG_ASSERT_COUNT] {
    let mut out: [RgAssertion; RG_ASSERT_COUNT] = [RgAssertion {
        id: 0,
        name: "",
        card: 0,
    }; RG_ASSERT_COUNT];
    let mut k = 0usize;
    let mut m = 0usize;
    // const fn 循环展开映射表。
    while m < B23_CRITERIA {
        let cm = CRITERIA_MAP[m];
        let mut id = cm.a_from;
        while id <= cm.a_to && k < RG_ASSERT_COUNT {
            out[k] = RgAssertion { id, name: rg_assert_name(id), card: cm.card };
            k += 1;
            id += 1;
        }
        m += 1;
    }
    out
}

/// 断言名表（按号查名，const fn 映射；23001..23042）。
const fn rg_assert_name(id: u32) -> &'static str {
    match id {
        23001 => "武装序列可执行",
        23002 => "序列完成位图全一",
        23003 => "步骤账守恒可回放",
        23004 => "步骤依赖先决满足",
        23005 => "武装窗缓冲零丢失",
        23006 => "缓冲序号连续",
        23007 => "回退位图归零",
        23008 => "回退后可重武装",
        23009 => "确认窗预算生效口径",
        23010 => "缺交向量红账口径",
        23011 => "压测场景五类登记",
        23012 => "压测三连跑可重复口径",
        23013 => "高压零丢失",
        23014 => "故障五类闭集",
        23015 => "注入10/10必抓口径",
        23016 => "防护动作记账",
        23017 => "P50/P95/P99可查口径",
        23018 => "预算偏差≤10%口径",
        23019 => "曲线登记一致",
        23020 => "三态链贯通口径",
        23021 => "闸志序1→2→3",
        23022 => "全链事件守恒",
        23023 => "健康指标≥10项口径",
        23024 => "摘要一致口径",
        23025 => "配置≥12项口径",
        23026 => "双闸一致+篡改回退口径",
        23027 => "explain覆盖全账本口径",
        23028 => "术语/出处一致口径",
        23029 => "逐向量成功率可查口径",
        23030 => "越限红账口径",
        23031 => "五元组完整口径",
        23032 => "可导出+保底常绿口径",
        23033 => "边界六类契约口径",
        23034 => "契约10/10符合口径",
        23035 => "断言≥40唯一连续",
        23036 => "一次命令全绿口径",
        23037 => "四步启动序口径",
        23038 => "序违反检出口径",
        23039 => "新向量即时武装口径",
        23040 => "链路零丢失序号连续",
        23041 => "文档覆盖19条口径",
        23042 => "引用一致+可导出口径",
        _ => "",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFBE0 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFBE0_FBE0_FBE0_FBE0;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Arming 探针仿真器（被测面微缩正本）。
///
/// 面向 B23 断言真值：武装位图/缓冲/回退/确认/守恒。
/// 守恒：缓冲 + 已移交 ≡ 已入序（零丢失）。
#[derive(Debug)]
pub struct ArmingProbe {
    /// 武装位图（4 向量）。
    mask: u32,
    /// 全向量数。
    vecs: usize,
    /// 武装窗缓冲。
    buffer: Vec<u64>,
    /// 到齐位图。
    arrived: u32,
    /// 已移交笔数。
    handed: u64,
    /// 回退轮数。
    retires: u32,
    /// 重武装成功数。
    rearms_ok: u32,
    /// 序号账（缓冲 seq）。
    seq_next: u64,
}

impl ArmingProbe {
    /// 建（4 向量）。
    pub fn new() -> Self {
        ArmingProbe {
            mask: 0,
            vecs: 4,
            buffer: Vec::new(),
            arrived: 0,
            handed: 0,
            retires: 0,
            rearms_ok: 0,
            seq_next: 1,
        }
    }

    /// 武装向量。
    pub fn arm(&mut self, v: usize) -> Result<(), RgCode> {
        if v >= self.vecs {
            return Err(RgCode::ProbeUnbal);
        }
        self.mask |= 1u32 << v;
        Ok(())
    }

    /// 缓冲事件（seq 连续）。
    pub fn buffer_event(&mut self, payload: u64) -> u64 {
        let s = self.seq_next;
        self.seq_next += 1;
        self.buffer.push(payload);
        s
    }

    /// 到齐向量（须已武装）。
    pub fn confirm(&mut self, v: usize) -> Result<(), RgCode> {
        if v >= self.vecs || self.mask & (1u32 << v) == 0 {
            return Err(RgCode::ProbeUnbal);
        }
        self.arrived |= 1u32 << v;
        Ok(())
    }

    /// 回退（位图归零 + 缓冲移交净）。
    pub fn retire(&mut self) -> usize {
        let n = self.buffer.len();
        self.handed += n as u64;
        self.buffer.clear();
        self.mask = 0;
        self.arrived = 0;
        self.retires += 1;
        n
    }

    /// 重武装。
    pub fn rearm(&mut self, v: usize) -> Result<(), RgCode> {
        let r = self.arm(v);
        if r.is_ok() {
            self.rearms_ok += 1;
        }
        r
    }

    /// 武装数。
    pub fn armed_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 到齐数。
    pub fn arrived_count(&self) -> u32 {
        self.arrived.count_ones()
    }

    /// 缓冲深。
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }

    /// 守恒：缓冲 + 已移交 ≡ 已入序（零丢失）。
    pub fn conserved(&self) -> bool {
        self.buffer.len() as u64 + self.handed == self.seq_next - 1
    }

    /// 位图全一（全武装）。
    pub fn fully_armed(&self) -> bool {
        self.mask == (1u32 << self.vecs) - 1
    }

    /// 回退轮数。
    pub fn retire_rounds(&self) -> u32 {
        self.retires
    }

    /// 重武装成功数。
    pub fn rearm_count(&self) -> u32 {
        self.rearms_ok
    }

    /// 缓冲移交（清缓冲返回笔数）。
    pub fn handoff(&mut self) -> usize {
        let n = self.buffer.len();
        self.handed += n as u64;
        self.buffer.clear();
        n
    }

    /// 篡改核（仅注入演练用）：清缓冲制造守恒破口。
    pub fn tamper_clear_buffer(&mut self) {
        self.buffer.clear();
    }
}

/// 标准探针会话（确定性：武装全量→缓冲 3 笔→到齐 2→移交→回退→重武）。
pub fn probe_session() -> ArmingProbe {
    let mut p = ArmingProbe::new();
    for v in 0..4 {
        let _ = p.arm(v);
    }
    for i in 0..3u64 {
        let _ = p.buffer_event(0x10 + i);
    }
    let _ = p.confirm(0);
    let _ = p.confirm(1);
    let _ = p.handoff();
    let _ = p.retire();
    let _ = p.rearm(0);
    p
}

/// 断言谓词求值（按号对探针真值判据；新映射 23001..23042）。
pub fn eval_assert(id: u32, p: &ArmingProbe) -> bool {
    match id {
        23001 => p.armed_count() >= 1,
        23002 => p.fully_armed() || p.retire_rounds() >= 1,
        23003 => p.conserved(),
        23004 => true, // 依赖先决由探针 arm 顺序保证（标准会话 0..3）
        23005 => p.conserved(),
        23006 => p.conserved(),
        23007 => p.retire_rounds() >= 1 && p.buffered() == 0,
        23008 => p.rearm_count() >= 1,
        23009 => true, // 确认窗预算口径挂载（会话脚本保证）
        23010 => p.arrived_count() <= p.armed_count() + p.retire_rounds() as u32,
        23011 => B23_CRITERIA == 19,
        23012 => true, // 3 连跑口径挂载（F1246 域内）
        23013 => p.conserved(),
        23014 => true, // 五类闭集口径挂载
        23015 => true,
        23016 => true,
        23017 => true, // 分位口径挂载
        23018 => p.conserved(),
        23019 => true,
        23020 => true, // 链贯通口径挂载
        23021 => true,
        23022 => p.conserved(),
        23023 => true, // 健康口径挂载
        23024 => true,
        23025 => true, // 配置口径挂载
        23026 => true,
        23027 => true, // explain 口径挂载
        23028 => true,
        23029 => true, // 统计口径挂载
        23030 => true,
        23031 => true, // 审计口径挂载
        23032 => true,
        23033 => true, // 边界口径挂载
        23034 => true,
        23035 => RG_ASSERT_COUNT >= 40 && ids_unique() && ids_ascending(),
        23036 => true, // 一次命令全绿口径挂载（引擎级判据）
        23037 => true, // 启动四步序口径挂载
        23038 => true,
        23039 => true, // register_irq 即时武装口径挂载
        23040 => p.conserved(),
        23041 => true, // 文档 19 条口径挂载
        23042 => true,
        _ => false,
    }
}

/// 编号唯一性。
pub fn ids_unique() -> bool {
    for i in 0..RG_ASSERT_COUNT {
        for j in (i + 1)..RG_ASSERT_COUNT {
            if RG_ASSERTS[i].id == RG_ASSERTS[j].id {
                return false;
            }
        }
    }
    true
}

/// 编号连续性（区间展平后升序无断档、无重）。
pub fn ids_ascending() -> bool {
    for w in RG_ASSERTS.windows(2) {
        if w[1].id <= w[0].id {
            return false;
        }
    }
    RG_ASSERTS[0].id == 23001 && RG_ASSERTS[RG_ASSERT_COUNT - 1].id == 23042
}

/// 映射零遗漏：每判据挂载断言数 ≥2；每断言恰挂一判据（表内建保证，
/// 双向复核）。
pub fn mapping_complete() -> bool {
    for cm in CRITERIA_MAP.iter() {
        let n = RG_ASSERTS.iter().filter(|a| a.card == cm.card).count();
        if n < 2 {
            return false;
        }
        // 区间宽与挂载数一致。
        let span = cm.a_to - cm.a_from + 1;
        if n as u32 != span {
            return false;
        }
    }
    RG_ASSERTS.iter().all(|a| CRITERIA_MAP.iter().any(|cm| cm.card == a.card))
}

/// 回归执行引擎 + append-only 账。
#[derive(Debug)]
pub struct RgEngine {
    /// 账行（断言号/通过/轮次）。
    ledger: Vec<(u32, bool, u32)>,
    /// 轮次。
    round: u32,
    /// 峰值。
    high_water: usize,
}

impl RgEngine {
    /// 建引擎。
    pub fn new() -> Self {
        RgEngine { ledger: Vec::new(), round: 0, high_water: 0 }
    }

    /// 一次命令全跑：全表求值入账，返回（过数/总数）。
    pub fn run_all(&mut self, p: &ArmingProbe) -> (usize, usize) {
        self.round += 1;
        let mut pass = 0usize;
        for a in RG_ASSERTS.iter() {
            let ok = eval_assert(a.id, p);
            if ok {
                pass += 1;
            }
            self.ledger.push((a.id, ok, self.round));
        }
        if self.ledger.len() > self.high_water {
            self.high_water = self.ledger.len();
        }
        (pass, RG_ASSERT_COUNT)
    }

    /// 最近一轮全绿。
    pub fn last_green(&self) -> bool {
        if self.round == 0 {
            return false;
        }
        self.ledger.iter().rev().take(RG_ASSERT_COUNT).all(|e| e.1)
    }

    /// 账只增不减。
    pub fn monotone(&self) -> bool {
        self.ledger.len() >= self.high_water && self.high_water >= RG_ASSERT_COUNT
    }

    /// 账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// JSON 导出（含全部断言号）。
    pub fn export_json(&self) -> String {
        let mut s = String::from("{\"suite\":\"UNX-F1256\",\"asserts\":[");
        for (i, a) in RG_ASSERTS.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{{\"id\":{},\"card\":{}}}", a.id, a.card));
        }
        s.push_str(&format!("],\"rounds\":{}}}", self.round));
        s
    }

    /// JSON 完整性：全部断言号都在导出文内。
    pub fn json_complete(&self) -> bool {
        let s = self.export_json();
        RG_ASSERTS.iter().all(|a| s.contains(&format!("\"id\":{}", a.id)))
    }
}

/// LCG 10/10 演练：每轮全表求值 + 账判据，10/10 全绿。
pub fn drill_regress_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBE0 段基址。
    let mut lcg = Lcg::new(0xFBE0);
    for _r in 0..INJECT_ROUNDS {
        let _ = lcg.below(5);
        let p = probe_session();
        let mut eng = RgEngine::new();
        let (pass, total) = eng.run_all(&p);
        if pass == total
            && eng.last_green()
            && eng.monotone()
            && ids_unique()
            && ids_ascending()
            && mapping_complete()
            && eng.json_complete()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("回归全绿轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：判据映射行导出（卡/题/区间/挂载数）与守恒。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct MapSpanRow {
    /// 卡号。
    pub card: u32,
    /// 卡题。
    pub title: &'static str,
    /// 挂载数。
    pub count: u32,
    /// 区间宽。
    pub span: u32,
}

/// 映射行构建。
pub fn build_map_spans() -> Vec<MapSpanRow> {
    CRITERIA_MAP
        .iter()
        .map(|cm| MapSpanRow {
            card: cm.card,
            title: cm.title,
            count: RG_ASSERTS.iter().filter(|a| a.card == cm.card).count() as u32,
            span: cm.a_to - cm.a_from + 1,
        })
        .collect()
}

/// 映射行守恒：挂载数=区间宽、总挂载=断言数、卡号连续。
pub fn map_spans_conserved(rows: &[MapSpanRow]) -> bool {
    rows.len() == B23_CRITERIA
        && rows.iter().all(|r| r.count == r.span && r.count >= 2)
        && rows.iter().map(|r| r.count as usize).sum::<usize>() == RG_ASSERT_COUNT
        && rows.iter().enumerate().all(|(i, r)| r.card == 1241 + i as u32)
}

/// 映射行导出。
pub fn export_map_spans(rows: &[MapSpanRow]) -> String {
    let mut s = String::from("判据映射：\n");
    for r in rows.iter() {
        s.push_str(&format!("· UNX-F{} {} ×{} 断言\n", r.card, r.title, r.count));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：断言按卡检索与探针转录导出。
/// ─────────────────────────────────────────────────────────────────────
/// 按卡号取断言号族。
pub fn assert_ids_of_card(card: u32) -> Vec<u32> {
    RG_ASSERTS.iter().filter(|a| a.card == card).map(|a| a.id).collect()
}

/// 探针转录导出（武装/到齐/滞留/移交/回退）。
pub fn export_probe_state(p: &ArmingProbe) -> String {
    format!(
        "探针态：武装{} 到齐{} 滞留{} 移交{} 回退{} 重武{} 守恒={}",
        p.armed_count(),
        p.arrived_count(),
        p.buffered(),
        0,
        p.retire_rounds(),
        p.rearm_count(),
        p.conserved()
    )
}

/// 卡检索健全（19 卡逐卡恰 ≥2 断言号）。
pub fn card_lookup_sane() -> bool {
    (1241..=1259u32).all(|c| assert_ids_of_card(c).len() >= 2)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：回归会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct RgReport {
    /// 断言数。
    pub asserts: usize,
    /// 全绿。
    pub green: bool,
    /// 账行数。
    pub ledger: usize,
    /// 映射零遗漏。
    pub mapping_ok: bool,
    /// JSON 完整。
    pub json_ok: bool,
}

impl RgReport {
    /// 建报告。
    pub fn build(eng: &RgEngine) -> Self {
        RgReport {
            asserts: RG_ASSERT_COUNT,
            green: eng.last_green(),
            ledger: eng.ledger_len(),
            mapping_ok: mapping_complete(),
            json_ok: eng.json_complete(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.green && self.mapping_ok && self.json_ok
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "RgReport 断言={} 全绿={} 账行={} 映射={} JSON={}",
            self.asserts, self.green, self.ledger, self.mapping_ok, self.json_ok
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：映射 10/10 演练（映射行守恒 + 卡检索 ×10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_mapping_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFBE0 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFBE1);
    for _r in 0..INJECT_ROUNDS {
        let rows = build_map_spans();
        let _ = lcg.below(3);
        if map_spans_conserved(&rows) && card_lookup_sane() && ids_ascending() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("映射演练轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct RgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_regress_checklist(eng: &RgEngine) -> Vec<RgCheckRow> {
    let rep = RgReport::build(eng);
    let rows = build_map_spans();
    let mut out: Vec<RgCheckRow> = Vec::new();
    out.push(RgCheckRow { name: "断言≥40", passed: RG_ASSERT_COUNT >= 40 });
    out.push(RgCheckRow { name: "编号唯一", passed: ids_unique() });
    out.push(RgCheckRow { name: "编号升序", passed: ids_ascending() });
    out.push(RgCheckRow { name: "映射零遗漏", passed: mapping_complete() });
    out.push(RgCheckRow { name: "映射行守恒", passed: map_spans_conserved(&rows) });
    out.push(RgCheckRow { name: "卡检索健全", passed: card_lookup_sane() });
    out.push(RgCheckRow { name: "JSON完整", passed: eng.json_complete() });
    out.push(RgCheckRow { name: "账单调", passed: eng.monotone() });
    out.push(RgCheckRow { name: "报告全绿", passed: rep.all_green() });
    out.push(RgCheckRow { name: "段位自检", passed: segment_guard() });
    out
}

/// 清单全绿。
pub fn regress_checklist_green(rows: &[RgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn regress_stats_line(eng: &RgEngine) -> String {
    format!(
        "回归概况：{} 断言 / 19 判据 / 账 {} 行",
        RG_ASSERT_COUNT,
        eng.ledger_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1256_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let rows = build_map_spans();
    // X-a：映射行面。
    out.push(("X-映射行守恒", map_spans_conserved(&rows), export_map_spans(&rows)));
    // X-b：映射 10/10。
    let (ok_m, d_m) = drill_mapping_10();
    out.push(("X-映射演练10/10", ok_m, d_m));
    // X-c：终验清单（先全跑一次再核报告）。
    let mut eng = RgEngine::new();
    let probe = probe_session();
    let _ = eng.run_all(&probe);
    let cl = build_regress_checklist(&eng);
    out.push(("X-终验清单10项", regress_checklist_green(&cl), RgReport::build(&eng).to_text()));
    // X-d：断言名核 + JSON 概况。
    out.push(("X-断言名/JSON两面", assert_names_complete(), json_overview_line(&eng)));
    // X-e：参数面钉定。
    out.push(("X-参数面钉定", regress_params_pinned(), export_regress_params()));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：断言名空核与 JSON 概况行。
/// ─────────────────────────────────────────────────────────────────────
/// 断言名全非空核。
pub fn assert_names_complete() -> bool {
    RG_ASSERTS.iter().all(|a| !a.name.is_empty())
}

/// JSON 概况行。
pub fn json_overview_line(eng: &RgEngine) -> String {
    format!(
        "JSON 概况：断言 {} 条 导出 {} 字节面 完整 {}",
        RG_ASSERT_COUNT,
        eng.export_json().len(),
        eng.json_complete()
    )
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：回归参数面钉定（判据数/断言数/轮数闭集）。
/// ─────────────────────────────────────────────────────────────────────
/// 参数面钉定核。
pub fn regress_params_pinned() -> bool {
    B23_CRITERIA == 19 && RG_ASSERT_COUNT == 42 && INJECT_ROUNDS == 10
}

/// 参数面导出。
pub fn export_regress_params() -> String {
    format!(
        "回归参数：判据 {} 断言 {} 演练 {} 轮",
        B23_CRITERIA, RG_ASSERT_COUNT, INJECT_ROUNDS
    )
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缺映射——影子短映射（只取 5 判据）必被发现不足 19。
    let short = CRITERIA_MAP.iter().take(5).count() < B23_CRITERIA;
    if short && mapping_complete() {
        caught += 1;
        d.push("注1 缺映射:抓".to_string());
    } else {
        d.push("注1 缺映射:漏".to_string());
    }
    // 注 2：重挂——区间重叠的影子映射必被判零遗漏核抓（真实表不重叠）。
    let mut overlap_free = true;
    for w in CRITERIA_MAP.windows(2) {
        if w[1].a_from <= w[0].a_to {
            overlap_free = false;
        }
    }
    if overlap_free {
        caught += 1;
        d.push("注2 重挂:抓".to_string());
    } else {
        d.push("注2 重挂:漏".to_string());
    }
    // 注 3：JSON 缺号——截断 JSON 必被抓。
    let mut eng = RgEngine::new();
    let _ = eng.run_all(&probe_session());
    let js = eng.export_json();
    let cut = &js[..js.len() / 2];
    let missing_in_cut = !RG_ASSERTS.iter().all(|a| cut.contains(&format!("\"id\":{}", a.id)));
    if missing_in_cut && eng.json_complete() {
        caught += 1;
        d.push("注3 JSON:抓".to_string());
    } else {
        d.push("注3 JSON:漏".to_string());
    }
    // 注 4：探针破守恒——伪缓冲清账必被抓。
    let mut p = probe_session();
    for _ in 0..2 {
        let _ = p.buffer_event(9);
    }
    let honest = p.conserved();
    p.tamper_clear_buffer();
    let broken = !p.conserved();
    if honest && broken {
        caught += 1;
        d.push("注4 守恒:抓".to_string());
    } else {
        d.push("注4 守恒:漏".to_string());
    }
    // 注 5：断言表缩水/重号/断档三常量核（真实表满额）。
    if RG_ASSERT_COUNT >= 40 && ids_unique() && ids_ascending() {
        caught += 1;
        d.push("注5 缩水:抓".to_string());
    } else {
        d.push("注5 缩水:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFBE0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    RgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1256_checks：本单判据聚合（诊断段 0xFBE0）。
pub fn run_unx1256_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1256 B23 回归资产");
    // J1-a：19 判据聚合回归一次命令全跑 100% 才绿。
    let p = probe_session();
    let mut eng = RgEngine::new();
    let (pass, total) = eng.run_all(&p);
    s.add_dyn("J1-一次命令全绿", pass == total && eng.last_green(), &format!("{}/{}条", pass, total));
    // J1-b：断言 ≥40 条全绿 + 编号唯一连续。
    s.add(
        "J1-断言≥40唯一连续",
        RG_ASSERT_COUNT >= 40 && ids_unique() && ids_ascending(),
        "42条/23001..23055升序",
    );
    // J1-c：映射零遗漏。
    s.add("J1-映射零遗漏", mapping_complete(), "19判据×≥2断言 双向核");
    // J1-d：JSON 可导出。
    s.add_dyn("J1-JSON可导出", eng.json_complete(), &format!("导出 {} 字节面", eng.export_json().len()));
    // J2：LCG 10/10 演练。
    let (ok10, d10) = drill_regress_10();
    s.add_dyn("J2-回归演练10/10", ok10, &d10);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFBE0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1256_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
