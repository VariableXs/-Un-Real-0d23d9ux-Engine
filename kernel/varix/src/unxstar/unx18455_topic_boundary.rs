//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18455 · 声明面防重声明（与 D2/D3 划界） · 0x7ED80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18455-J1 20 条主题聚类对 D2/D3 可见判据对表，嫌疑清
//!       单=空断言
//! 本模块结构清单：
//!   1. 冻结常量层：主题聚类数 20 / 面闭集 3（D1 声明/D2 装载/D3
//!      错误路径）/ 可见位闭集 / 判据轮数（推导注释写死）；
//!   2. 诊断码闭集 TbCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一（对表）：TOPIC_TABLE——20 行冻结主题账（题号/名/
//!      辖面/D2 可见位/D3 可见位），与 D2（F18445 装载侧）、D3
//!      （F18450 错误路径账）的辖区判据一一成对；
//!   4. 数据结构二（嫌疑账）：SuspectLedger——追加只读（类别/题号/
//!      指认码），干净语料断言为空；
//!   5. 防重判定器：题号重复 / 主题名重复 / 辖面污染（非 D1 声明
//!      混入声明面）三类扫描，命中即入嫌疑账；
//!   6. 划界审计：声明面主题逐行对 D2/D3 可见位与冻结对表复算，
//!      位漂移即嫌疑；
//!   7. 注入核 4 类×10 轮：题号重复 / 名重复 / 辖面污染 / 可见位
//!      篡改，逐类断言嫌疑账非空且类别指认正确；
//!   8. 黄金全链 golden_run：干净语料 → 嫌疑清单空 → 注入全抓 →
//!      双跑一致；
//!   9. run_unx18455_checks：15 条判据（断言性质）。
//! 为什么防重声明要划界：D2 装载面与 D3 错误路径面各判各的账，
//! 声明面若重复声明同一主题或越界替 D2/D3 发言，可见判据对表就
//! 会出现双主——本卡把「谁在哪个面说话」冻结成对表，嫌疑清单
//! 空即划界干净。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7ED80，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7ED80;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 主题聚类数（冻结 20：J1 口径）。
pub const TOPICS: usize = 20;
/// 面闭集：D1=声明面（本卡辖区）、D2=装载面（F18445）、D3=错误路径面
/// （F18450）。
pub const FACE_D1: u32 = 1;
pub const FACE_D2: u32 = 2;
pub const FACE_D3: u32 = 3;
/// 可见位闭集：0=不可见 1=可见。
pub const VIS_OFF: u8 = 0;
pub const VIS_ON: u8 = 1;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TbCode {
    /// 题号重复（防重主判）
    TopicDup,
    /// 主题名重复（聚类键唯一）
    NameDup,
    /// 辖面污染（非 D1 声明混入声明面）
    FaceContaminate,
    /// 可见位越域（非 {0,1}）
    VisBitIllegal,
    /// 可见位漂移（与冻结对表不符）
    VisBitDrift,
    /// 主题欠账（不足 20 条）
    TopicShort,
    /// 嫌疑账类别误指（检出的类别与注入类别不符）
    SuspectMislabel,
    /// 嫌疑账守恒破（嫌疑行数与检出事件数不等）
    SuspectLedgerBroken,
    /// 对表行数不符
    TableSizeMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl TbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            TbCode::TopicDup => "题号重复：聚类键唯一红线",
            TbCode::NameDup => "主题名重复：聚类名唯一红线",
            TbCode::FaceContaminate => "辖面污染：声明面只许 D1 声明发言",
            TbCode::VisBitIllegal => "可见位越域：非 {0,1}",
            TbCode::VisBitDrift => "可见位漂移：与冻结对表不符",
            TbCode::TopicShort => "主题欠账：聚类不足 20 条",
            TbCode::SuspectMislabel => "嫌疑账类别误指",
            TbCode::SuspectLedgerBroken => "嫌疑账守恒破：行数与检出事件不等",
            TbCode::TableSizeMismatch => "对表行数不符",
            TbCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [TbCode; 10] = [
        TbCode::TopicDup,
        TbCode::NameDup,
        TbCode::FaceContaminate,
        TbCode::VisBitIllegal,
        TbCode::VisBitDrift,
        TbCode::TopicShort,
        TbCode::SuspectMislabel,
        TbCode::SuspectLedgerBroken,
        TbCode::TableSizeMismatch,
        TbCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TbCode::ALL.len() {
        if TbCode::ALL[i].val() == code {
            return TbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7ED80 段闭集内"
}

// ── 数据结构一：主题行（对表） ──────────────────────────────────
/// 主题聚类行：题号 / 名 / 辖面 / D2 可见位 / D3 可见位。
/// 辖面冻结 = D1（这 20 条聚类全部是声明面发言）；D2/D3 可见位是
/// 对下游两面的「可见判据对表」——D2 装载面读 d2 位、D3 错误路径
/// 面读 d3 位，声明面自身不再重复发这两面的判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopicRow {
    pub topic_id: u32,
    pub name: &'static str,
    pub face: u32,
    pub d2_visible: u8,
    pub d3_visible: u8,
}

/// 20 条主题聚类冻结对表。
/// 推导：主题按声明面族分 5 组（世代/GUID/级别/兼容值/观测），每组
/// 4 条；d2 位 = ((题号-1)/4)%2（组内交替），d3 位 = (题号%3 != 0)
/// ——两组位独立推导，可逐行复算。
pub const TOPIC_TABLE: [TopicRow; TOPICS] = [
    TopicRow { topic_id: 1, name: "gen_verdict",      face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 2, name: "gen_anchor",       face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 3, name: "gen_era_max",      face: FACE_D1, d2_visible: 0, d3_visible: 0 },
    TopicRow { topic_id: 4, name: "gen_baseline",     face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 5, name: "guid_set",         face: FACE_D1, d2_visible: 1, d3_visible: 1 },
    TopicRow { topic_id: 6, name: "guid_fake_rej",    face: FACE_D1, d2_visible: 1, d3_visible: 0 },
    TopicRow { topic_id: 7, name: "guid_multi_max",   face: FACE_D1, d2_visible: 1, d3_visible: 1 },
    TopicRow { topic_id: 8, name: "guid_era_link",    face: FACE_D1, d2_visible: 1, d3_visible: 1 },
    TopicRow { topic_id: 9, name: "level_parse",      face: FACE_D1, d2_visible: 0, d3_visible: 0 },
    TopicRow { topic_id: 10, name: "level_four_tier", face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 11, name: "level_virt_link", face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 12, name: "level_undecl",    face: FACE_D1, d2_visible: 0, d3_visible: 0 },
    TopicRow { topic_id: 13, name: "value_fallback",  face: FACE_D1, d2_visible: 1, d3_visible: 1 },
    TopicRow { topic_id: 14, name: "value_matrix",    face: FACE_D1, d2_visible: 1, d3_visible: 1 },
    TopicRow { topic_id: 15, name: "value_default",   face: FACE_D1, d2_visible: 1, d3_visible: 0 },
    TopicRow { topic_id: 16, name: "value_hotpath",   face: FACE_D1, d2_visible: 1, d3_visible: 1 },
    TopicRow { topic_id: 17, name: "obs_export",      face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 18, name: "obs_profile",     face: FACE_D1, d2_visible: 0, d3_visible: 0 },
    TopicRow { topic_id: 19, name: "obs_hook_stub",   face: FACE_D1, d2_visible: 0, d3_visible: 1 },
    TopicRow { topic_id: 20, name: "obs_perf_ledger", face: FACE_D1, d2_visible: 0, d3_visible: 1 },
];

/// 对表推导复算：d2 位 = ((题号-1)/4) % 2；d3 位 = (题号 % 3 != 0)。
pub fn expect_vis(topic_id: u32) -> (u8, u8) {
    let d2 = (((topic_id - 1) / 4) % 2) as u8;
    let d3 = if topic_id % 3 != 0 { VIS_ON } else { VIS_OFF };
    (d2, d3)
}

// ── 数据结构二：嫌疑账 ──────────────────────────────────────────
/// 嫌疑账行：类别码 + 题号 + 注记序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuspectRow {
    pub kind: u32,
    pub topic_id: u32,
    pub event_seq: u32,
}

/// 嫌疑账（追加只读）。
pub struct SuspectLedger {
    pub rows: Vec<SuspectRow>,
    pub events: u32,
    pub seq_next: u32,
}

impl SuspectLedger {
    pub fn new() -> SuspectLedger {
        SuspectLedger { rows: Vec::new(), events: 0, seq_next: 1 }
    }

    /// 记一条嫌疑（检出事件）。
    pub fn record(&mut self, kind: u32, topic_id: u32) {
        self.rows.push(SuspectRow { kind, topic_id, event_seq: self.seq_next });
        self.seq_next = self.seq_next.saturating_add(1);
        self.events = self.events.saturating_add(1);
    }

    /// 嫌疑清单为空断言（干净语料的 J1 验收口径）。
    pub fn is_empty(&self) -> bool {
        self.rows.len() == 0 && self.events == 0
    }

    /// 守恒：行数与事件数相等，序号连续。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.rows.len() as u32 != self.events {
            return Err(TbCode::SuspectLedgerBroken.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(x) => x,
                None => return Err(TbCode::SuspectLedgerBroken.val()),
            };
            if r.event_seq != (i as u32) + 1 {
                return Err(TbCode::SuspectLedgerBroken.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 指认核对：账里存在指定类别且无其他类别（注入类别唯一指认）。
    pub fn only_kind(&self, kind: u32) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) if r.kind != kind => return false,
                Some(_) => {}
                None => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 防重判定器 ──────────────────────────────────────────────────
/// 三类防重扫描 + 对表划界审计。入参：工作副本对表（冻结本体
/// 不写回）。返回嫌疑账。
pub fn scan(table: &[TopicRow; TOPICS], led: &mut SuspectLedger) {
    // 扫 0：题号重复（成对比较）。
    let mut i = 0;
    while i < table.len() {
        let mut j = i + 1;
        while j < table.len() {
            let a = match table.get(i) {
                Some(x) => *x,
                None => break,
            };
            let b = match table.get(j) {
                Some(x) => *x,
                None => break,
            };
            if a.topic_id == b.topic_id {
                led.record(TbCode::TopicDup.val(), a.topic_id);
            }
            if a.name == b.name {
                led.record(TbCode::NameDup.val(), a.topic_id);
            }
            j += 1;
        }
        i += 1;
    }
    // 扫 1：辖面污染（声明面只许 D1）。
    let mut k = 0;
    while k < table.len() {
        let r = match table.get(k) {
            Some(x) => *x,
            None => break,
        };
        if r.face != FACE_D1 {
            led.record(TbCode::FaceContaminate.val(), r.topic_id);
        }
        // 扫 2：可见位越域。
        if (r.d2_visible != VIS_OFF && r.d2_visible != VIS_ON)
            || (r.d3_visible != VIS_OFF && r.d3_visible != VIS_ON)
        {
            led.record(TbCode::VisBitIllegal.val(), r.topic_id);
        }
        // 扫 3：对表划界审计（可见位与冻结推导不符）。
        let (ed2, ed3) = expect_vis(r.topic_id);
        if r.d2_visible != ed2 || r.d3_visible != ed3 {
            led.record(TbCode::VisBitDrift.val(), r.topic_id);
        }
        k += 1;
    }
}

/// 便捷入口：冻结本体扫描（干净路径）。
pub fn scan_frozen() -> SuspectLedger {
    let mut led = SuspectLedger::new();
    scan(&TOPIC_TABLE, &mut led);
    led
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽 0 题号重复 / 槽 1 名重复 / 槽 2 辖面污染 /
/// 槽 3 可见位篡改。全部在工作副本上注入，冻结对表本体不写回。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：题号重复（行 2 改成行 1 的题号）。
        let mut t0 = TOPIC_TABLE;
        if let (Some(a), Some(b)) = (t0.get(1).copied(), t0.get(0).copied()) {
            t0[1] = TopicRow { topic_id: b.topic_id, name: a.name, face: a.face,
                d2_visible: a.d2_visible, d3_visible: a.d3_visible };
        }
        let mut led0 = SuspectLedger::new();
        scan(&t0, &mut led0);
        let hit0 = !led0.is_empty()
            && led0.only_kind(TbCode::TopicDup.val())
            && led0.conserve().is_ok();
        led.record(0, hit0);
        // 槽 1：名重复（行 3 名改成行 2 的名）。
        let mut t1 = TOPIC_TABLE;
        if let Some(a) = t1.get(1).copied() {
            if let Some(row) = t1.get_mut(2) {
                let mut nrow = *row;
                nrow.name = a.name;
                t1[2] = nrow;
            }
        }
        let mut led1 = SuspectLedger::new();
        scan(&t1, &mut led1);
        let hit1 = !led1.is_empty() && led1.only_kind(TbCode::NameDup.val());
        led.record(1, hit1);
        // 槽 2：辖面污染（行 4 辖面改成 D2）。
        let mut t2 = TOPIC_TABLE;
        if let Some(row) = t2.get_mut(3) {
            let mut nrow = *row;
            nrow.face = FACE_D2;
            *row = nrow;
        }
        let mut led2 = SuspectLedger::new();
        scan(&t2, &mut led2);
        let hit2 = !led2.is_empty() && led2.only_kind(TbCode::FaceContaminate.val());
        led.record(2, hit2);
        // 槽 3：可见位篡改（行 5 的 d2 位翻面 → 对表漂移指认）。
        let mut t3 = TOPIC_TABLE;
        if let Some(row) = t3.get_mut(4) {
            let mut nrow = *row;
            nrow.d2_visible = if nrow.d2_visible == VIS_ON { VIS_OFF } else { VIS_ON };
            *row = nrow;
        }
        let mut led3 = SuspectLedger::new();
        scan(&t3, &mut led3);
        let hit3 = !led3.is_empty() && led3.only_kind(TbCode::VisBitDrift.val());
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：对表 20 行齐 → 干净扫描嫌疑空 → 注入全抓 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    if TOPIC_TABLE.len() != TOPICS {
        return Err(TbCode::TableSizeMismatch.val());
    }
    let led = scan_frozen();
    if !led.is_empty() {
        return Err(TbCode::SuspectLedgerBroken.val());
    }
    led.conserve()?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(TbCode::NotReproducible.val());
    }
    Ok((led.events, inj.caught[0] + inj.caught[1] + inj.caught[2] + inj.caught[3]))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TbCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：冻结对表干净扫描零嫌疑。
pub fn probe_seq_a() -> Result<(), u32> {
    let led = scan_frozen();
    if led.is_empty() {
        Ok(())
    } else {
        Err(led.rows.get(0).map(|r| r.kind).unwrap_or(TbCode::TopicDup.val()))
    }
}

/// 负探针 1：对表推导复算（20 行逐行 expect_vis 复算零漂移）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut i = 0;
    while i < TOPIC_TABLE.len() {
        let r = match TOPIC_TABLE.get(i) {
            Some(x) => *x,
            None => return Err(TbCode::TableSizeMismatch.val()),
        };
        let (ed2, ed3) = expect_vis(r.topic_id);
        if r.d2_visible != ed2 || r.d3_visible != ed3 {
            return Err(TbCode::VisBitDrift.val());
        }
        i += 1;
    }
    Ok(())
}

/// 负探针 2：面闭集守卫（对表中不允许出现 D3 辖面发言）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut i = 0;
    while i < TOPIC_TABLE.len() {
        let r = match TOPIC_TABLE.get(i) {
            Some(x) => *x,
            None => return Err(TbCode::TableSizeMismatch.val()),
        };
        if r.face == FACE_D3 {
            return Err(TbCode::FaceContaminate.val());
        }
        i += 1;
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TbCode::ALL.len() {
        let v = TbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18455_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18455 声明面防重声明");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "嫌疑{:?} 注入检出{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0)
        ),
    );
    // ② 嫌疑清单=空断言（J1 后半句）。
    let led = scan_frozen();
    s.add("嫌疑清单空断言", led.is_empty(), "干净语料零嫌疑零事件");
    // ③ 20 条主题聚类齐（J1 前半句）。
    s.add("主题聚类20条齐", TOPIC_TABLE.len() == TOPICS,
        "5 组×4 条，题号 1..20 连续");
    // ④ 题号唯一 + 连续复算。
    let mut ids_ok = true;
    let mut ti = 0;
    while ti < TOPICS {
        let r = TOPIC_TABLE[ti];
        if r.topic_id != (ti + 1) as u32 {
            ids_ok = false;
        }
        ti += 1;
    }
    s.add("题号连续唯一", ids_ok, "1..20 逐行复算");
    // ⑤ D2/D3 可见判据对表复算（划界审计）。
    s.add("D2D3对表零漂移", probe_seq_b().is_ok(), "20 行可见位与冻结推导逐行相等");
    // ⑥ 辖面闭集：全部 D1 发言。
    s.add("辖面全D1", probe_seq_c().is_ok(), "声明面无 D2/D3 越界发言");
    // ⑦ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑧ 漏检为零。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入=检出+漏检，漏检=0");
    // ⑨ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "干净零嫌疑/对表复算/辖面守卫");
    // ⑩ 嫌疑账守恒语义（注入路径的账面核对）。
    let mut linj = SuspectLedger::new();
    let mut tdup = TOPIC_TABLE;
    if let (Some(a), Some(b)) = (tdup.get(1).copied(), tdup.get(0).copied()) {
        tdup[1] = TopicRow { topic_id: b.topic_id, name: a.name, face: a.face,
            d2_visible: a.d2_visible, d3_visible: a.d3_visible };
    }
    scan(&tdup, &mut linj);
    s.add("嫌疑账守恒", linj.conserve().is_ok() && !linj.is_empty(),
        "注入后嫌疑行数=事件数且序号连续");
    // ⑪ 冻结对表本体不被注入写回。
    s.add("对表本体不写回", TOPIC_TABLE[0].topic_id == 1 && TOPIC_TABLE[1].topic_id == 2,
        "注入全部走工作副本");
    // ⑫ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑬ 可见位闭集复算：对表全部位在 {0,1}。
    let mut vis_ok = true;
    let mut vi = 0;
    while vi < TOPICS {
        let r = TOPIC_TABLE[vi];
        if (r.d2_visible != VIS_OFF && r.d2_visible != VIS_ON)
            || (r.d3_visible != VIS_OFF && r.d3_visible != VIS_ON)
        {
            vis_ok = false;
        }
        vi += 1;
    }
    s.add("可见位闭集", vis_ok, "40 个位全在 {0,1}");
    // ⑭ 文案闭集。
    let mut closed = true;
    let mut ci = 0;
    while ci < TbCode::ALL.len() {
        if TbCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    // ⑮ 段位守卫。
    s.add("段位守卫 0x7ED80", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_corpus_empty_suspects() {
        let led = scan_frozen();
        assert!(led.is_empty(), "干净语料嫌疑清单必须为空");
        assert!(led.conserve().is_ok());
        assert_eq!(led.events, 0);
    }

    #[test]
    fn twenty_topics_ids_unique_consecutive() {
        assert_eq!(TOPIC_TABLE.len(), 20);
        let mut i = 0;
        while i < TOPICS {
            let r = TOPIC_TABLE[i];
            assert_eq!(r.topic_id, (i + 1) as u32, "题号连续");
            assert_eq!(r.face, FACE_D1, "辖面全 D1");
            assert!(r.d2_visible == 0 || r.d2_visible == 1);
            assert!(r.d3_visible == 0 || r.d3_visible == 1);
            i += 1;
        }
    }

    #[test]
    fn vis_expect_derivation_recap() {
        // 推导复算抽检：题 1 → (0,1)；题 3 → (0,0)；题 5 → (1,1)；
        // 题 6 → (1,0)；题 9 → (0,0)；题 20 → (0,1)。
        assert_eq!(expect_vis(1), (0, 1));
        assert_eq!(expect_vis(3), (0, 0));
        assert_eq!(expect_vis(5), (1, 1));
        assert_eq!(expect_vis(9), (0, 0));
        assert_eq!(expect_vis(20), (0, 1));
    }

    #[test]
    fn topic_dup_injection_caught_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let mut t = TOPIC_TABLE;
            let a = t[0];
            let b = t[1];
            t[1] = TopicRow { topic_id: a.topic_id, name: b.name, face: b.face,
                d2_visible: b.d2_visible, d3_visible: b.d3_visible };
            let mut led = SuspectLedger::new();
            scan(&t, &mut led);
            assert!(!led.is_empty(), "第 {} 轮题号重复必检", r);
            assert!(led.only_kind(TbCode::TopicDup.val()));
            r += 1;
        }
    }

    #[test]
    fn face_contaminate_caught() {
        let mut t = TOPIC_TABLE;
        let row = t[3];
        t[3] = TopicRow { topic_id: row.topic_id, name: row.name, face: FACE_D3,
            d2_visible: row.d2_visible, d3_visible: row.d3_visible };
        let mut led = SuspectLedger::new();
        scan(&t, &mut led);
        assert!(led.only_kind(TbCode::FaceContaminate.val()));
        assert!(led.rows.get(0).map(|x| x.topic_id).unwrap_or(0) == row.topic_id);
    }

    #[test]
    fn vis_drift_caught() {
        let mut t = TOPIC_TABLE;
        let row = t[7];
        let flipped = if row.d2_visible == VIS_ON { VIS_OFF } else { VIS_ON };
        t[7] = TopicRow { topic_id: row.topic_id, name: row.name, face: row.face,
            d2_visible: flipped, d3_visible: row.d3_visible };
        let mut led = SuspectLedger::new();
        scan(&t, &mut led);
        assert!(led.only_kind(TbCode::VisBitDrift.val()));
    }

    #[test]
    fn frozen_table_not_mutated_by_injection() {
        let _ = inject_rounds(10);
        assert_eq!(TOPIC_TABLE[0].topic_id, 1);
        assert_eq!(TOPIC_TABLE[1].topic_id, 2);
        assert_eq!(TOPIC_TABLE[1].name, "gen_anchor");
        assert_eq!(TOPIC_TABLE[3].face, FACE_D1);
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(), "四类注入 10/10 全抓");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn suspect_ledger_conservation_semantics() {
        let mut led = SuspectLedger::new();
        led.record(TbCode::NameDup.val(), 5);
        led.record(TbCode::NameDup.val(), 7);
        assert_eq!(led.rows.len() as u32, led.events);
        assert!(led.conserve().is_ok());
        assert!(led.only_kind(TbCode::NameDup.val()));
        assert!(!led.only_kind(TbCode::TopicDup.val()));
    }

    #[test]
    fn probes_and_double_run() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18455_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
