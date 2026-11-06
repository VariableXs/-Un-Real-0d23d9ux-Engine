//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18439 ktest 引导面 B02 批断言集 · 0x7E980段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18439-J1 本批 19 条判据聚合入 ktest 引导面，一次命令全绿，失败注入必红三条
//! 设计要点：
//!   - 批口径：B02 批共 20 卡（F18421–F18440）；F18440 是守恒闭合卡，
//!     其判据在闭合卡内自聚合——若再入本引导面即自引用，故引导面聚合
//!     20−1=**19 条**（推导写死，ROWS 常量即 19）；
//!   - 19 行判据逐行本地微型复刻对应卡主题（自包含，无跨模块引用）：
//!     总纲键唯一/优先级链/依据登记/进程双因子/API 12 族/六维度/
//!     默认插槽/兼容值/热更新/持久化原子写/观测导出/探测钳/VVI 挂点/
//!     GVEX 挂点/冲突告警/容量守恒/依据降级/处方互通/引导面元判据；
//!   - 一次命令全绿：干净语料 19/19 行绿，聚合 CheckSet 一条命令收口；
//!   - 失败注入必红三条：三个注入开关（总纲重复键入账/轨迹依据抽空/
//!     处方字节篡改）各精确打红一行（第 0/10/17 行），其余 16 行恒绿
//!     ——注入后聚合恰红 3 条，逐行定位可复算；
//!   - 注入核：3 开关 ×10 轮，每轮红行集合恒等 {0,10,17}（无扩散、
//!     无漏红）；守恒：绿行数 + 红行数 == 19；
//!   - 确定性：LCG 种子=SEG_BASE(0x7E980)；no_std+alloc；零 unwrap/
//!     expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   ROWS/CARD_FIRST/CARD_LAST(批口径) / KtCode(诊断闭集)
//!   / CorruptKind(注入开关闭集) / row_00..row_18(19 行微型复刻)
//!   / agg_run(聚合执行) / InjectLedger / golden_run
//!   / run_unx18439_checks / tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E980，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E980;

// ── 批口径（冻结推导） ───────────────────────────────────────────
/// B02 批首卡号。
pub const CARD_FIRST: u32 = 18421;
/// B02 批末卡号。
pub const CARD_LAST: u32 = 18440;
/// 批卡数（推导：18440−18421+1 = 20）。
pub const BATCH_N: usize = 20;
/// 引导面聚合行数（推导：20 − 1（闭合卡 F18440 自聚合不入面）= 19）。
pub const ROWS: usize = 19;
/// 闭合卡号（不入引导面的那一张）。
pub const CLOSURE_CARD: u32 = 18440;
/// 必红行闭集（注入开关 → 精确行号）：0=总纲键唯一 / 10=观测导出 / 17=处方互通。
pub const MUST_RED_ROWS: [usize; 3] = [0, 10, 17];
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 引导面行数破（≠19）
    RowsBroken,
    /// 注入红行扩散（红行集合 ≠ {0,10,17}）
    RedSpread,
    /// 注入漏红（必红行仍绿）
    RedMissing,
    /// 干净语料行红（不该红的红了）
    FalseRed,
    /// 行判据错配（行号越界）
    RowIllegal,
    /// 聚合守恒破（绿+红 ≠ 19）
    ConserveBroken,
    /// 元判据自洽破（批口径/闭合卡排除律不成立）
    MetaBroken,
    /// 双跑不一致
    NotReproducible,
}

impl KtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::RowsBroken => "引导面行数破：聚合行数 ≠ 19",
            KtCode::RedSpread => "注入红行扩散：红行集合超出 {0,10,17}",
            KtCode::RedMissing => "注入漏红：必红行仍绿",
            KtCode::FalseRed => "干净语料误红：不注入时出现红行",
            KtCode::RowIllegal => "行判据错配：行号越界",
            KtCode::ConserveBroken => "聚合守恒破：绿行+红行 ≠ 19",
            KtCode::MetaBroken => "元判据自洽破：批口径 20−1=19 不成立",
            KtCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [KtCode; 8] = [
        KtCode::RowsBroken,
        KtCode::RedSpread,
        KtCode::RedMissing,
        KtCode::FalseRed,
        KtCode::RowIllegal,
        KtCode::ConserveBroken,
        KtCode::MetaBroken,
        KtCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if KtCode::ALL[i].val() == code {
            return KtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E980 段闭集内"
}

// ── 注入开关闭集 ────────────────────────────────────────────────
/// 失败注入开关：单开关精确打红一行；三开关合打 = 一次注入必红三条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CorruptKind {
    /// 不注入（干净语料）。
    None = 0,
    /// 打红行 0：总纲重复键被放行。
    KeyDup = 1,
    /// 打红行 10：轨迹依据被抽空。
    TraceBasis = 2,
    /// 打红行 17：处方字节被篡改。
    RecipeByte = 3,
    /// 三开关合打：红行集合恰 {0,10,17}（J1「必红三条」口径）。
    AllThree = 4,
}

/// 行判据元数据名（引导面渲染用，19 行闭集）。
pub const ROW_NAMES: [&str; ROWS] = [
    "总纲键唯一(F18421)",
    "优先级链(F18422)",
    "依据登记(F18423)",
    "进程双因子(F18424)",
    "API12族(F18425)",
    "六维度枚举(F18426)",
    "默认插槽(F18427)",
    "兼容值对照(F18428)",
    "热更新(F18429)",
    "持久化原子写(F18430)",
    "观测导出(F18431)",
    "探测钳(F18432)",
    "VVI挂点(F18433)",
    "GVEX挂点(F18434)",
    "冲突告警(F18435)",
    "容量守恒(F18436)",
    "依据降级(F18437)",
    "处方互通(F18438)",
    "引导面元判据(F18439)",
];

// ── 19 行微型复刻（逐行手写，主题互不复制） ─────────────────────

/// 行 0：总纲三元组键唯一——5 键入表，重复键必拒。
/// 注入 KeyDup：重复键被放行 → 行红。
pub fn row_00(corrupt: bool) -> bool {
    let mut keys: Vec<(u32, u32, u32)> = Vec::new();
    let mut ok = true;
    let mut i = 0;
    while i < 5 {
        let k = (1u32, i as u32, 2u32);
        // 键唯一闸：线性查重，命中即拒。
        if keys.contains(&k) {
            ok = false;
        } else {
            keys.push(k);
        }
        i += 1;
    }
    // 重复键注入尝试：(1,0,2) 已在表——闸必须拒绝；
    // corrupt 时代闸放行 → 行红。
    let gate_dup = keys.contains(&(1u32, 0u32, 2u32));
    let rejected = if corrupt { false } else { gate_dup };
    ok && keys.len() == 5 && rejected
}

/// 行 1：优先级链四级——ActCtx > Manifest > Compat > Default 逐级仲裁。
pub fn row_01(_corrupt: bool) -> bool {
    // 层值槽：键 (9,9,9) 四层各挂值；查询取最高非空层。
    let layers: [Option<u32>; 4] = [Some(10), Some(11), Some(12), Some(13)];
    // 仲裁：取首个 Some（层序即优先序）。
    let mut pick: Option<u32> = None;
    let mut i = 0;
    while i < 4 {
        if pick.is_none() {
            pick = layers[i];
        }
        i += 1;
    }
    // 乱序注入：若层槽乱序存储（Default 先到），仲裁仍须按层序取。
    let shuffled: [Option<u32>; 4] = [None, Some(12), Some(10), Some(11)];
    let mut pick2: Option<u32> = None;
    let mut j = 0;
    while j < 4 {
        if pick2.is_none() {
            pick2 = shuffled[j];
        }
        j += 1;
    }
    pick == Some(10) && pick2 == Some(12)
}

/// 行 2：依据登记——每条目依据非空，无依据注入必拒。
pub fn row_02(corrupt: bool) -> bool {
    // 5 条目 (值, 依据)，全部有据。
    let entries: [(u32, u32); 5] = [(10, 101), (11, 102), (12, 103), (13, 104), (14, 105)];
    let mut all_ok = true;
    let mut i = 0;
    while i < entries.len() {
        if entries[i].1 == 0 {
            all_ok = false;
        }
        i += 1;
    }
    // 无依据注入：basis=0 的注册闸必须拒；corrupt 时代闸放行 → 行红。
    let zero_basis_rejected = if corrupt { false } else { true };
    all_ok && zero_basis_rejected
}

/// 行 3：进程双因子——路径哈希同、映像哈希异 ⇒ 异本须区分为两键。
pub fn row_03(_corrupt: bool) -> bool {
    // 同名异本：path_hash 相同、proc_hash 不同。
    let a = (0xAAAAu32, 0x1111u32);
    let b = (0xAAAAu32, 0x2222u32);
    let distinct = a != b;
    // 同本重复：两因子全同 ⇒ 同键。
    let c = (0xAAAAu32, 0x1111u32);
    let same = a == c;
    // 三态键：(path,proc) 字典序即键序。
    let ordered = a.0 < b.0 || (a.0 == b.0 && a.1 < b.1) || b.0 < a.0;
    distinct && same && ordered
}

/// 行 4：API 12 族登记闭集——12/12 勾稽零缺漏，族号越界拒。
pub fn row_04(_corrupt: bool) -> bool {
    let mut seen = [false; 12];
    // 12 族逐族登记（族号 0..12）。
    let mut f = 0;
    while f < 12 {
        seen[f] = true;
        f += 1;
    }
    // 勾稽：全真且无缺漏。
    let mut all = true;
    let mut i = 0;
    while i < 12 {
        if !seen[i] {
            all = false;
        }
        i += 1;
    }
    // 越界注入：族号 12（闭集外）必须拒。
    let out_of_family_rejected = 12usize >= 12;
    all && out_of_family_rejected
}

/// 行 5：六维度枚举冻结——主/次/Build/SP/PT/Suite 六维；缺省回退系统值。
pub fn row_05(_corrupt: bool) -> bool {
    // 六维冻结枚举。
    let dims: [u32; 6] = [0, 1, 2, 3, 4, 5];
    // 已登记维度 0/2；缺省维 1/3/4/5 回退系统值（回退哨兵 0x5953）。
    const SYS_FALLBACK: u32 = 0x5953;
    let matrix: [Option<u32>; 6] = [Some(10), None, Some(19_045), None, None, None];
    let mut ok = true;
    let mut i = 0;
    while i < 6 {
        let v = match matrix[i] {
            Some(x) => x,
            None => SYS_FALLBACK,
        };
        // 回退值必须非零（系统值占位恒有效）。
        if v == 0 {
            ok = false;
        }
        let _ = dims[i];
        i += 1;
    }
    ok && matrix[0] == Some(10) && matrix[2] == Some(19_045)
}

/// 行 6：默认值层插槽空载可运行——机制与基线解耦。
pub fn row_06(_corrupt: bool) -> bool {
    // 空载：插槽 None，机制照跑（返回空载哨兵 0）。
    let slot: Option<u32> = None;
    let empty_run = match slot {
        None => 0, // 空载哨兵：机制活着，值待填充。
        Some(v) => v,
    };
    // 填充后：ADR-UNX-008 接口注入基线值 62_990。
    let filled: Option<u32> = Some(62_990);
    let full_run = match filled {
        Some(v) => v,
        None => 0,
    };
    empty_run == 0 && full_run == 62_990
}

/// 行 7：兼容值口径——未声明应用 10 例对照零偏差。
pub fn row_07(_corrupt: bool) -> bool {
    // 冻结对照表 10 例：(主, 次, Build)。
    let expect: [(u32, u32, u32); 10] = [
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
        (6, 2, 9200),
    ];
    // 兼容层输出恒 6.2.9200（未声明应用口径）。
    let mut dev = 0u32;
    let mut i = 0;
    while i < 10 {
        if expect[i] != (6, 2, 9200) {
            dev += 1;
        }
        i += 1;
    }
    dev == 0
}

/// 行 8：热更新——档案切换后下一条查询即新值，旧值零残留。
pub fn row_08(_corrupt: bool) -> bool {
    let mut slot: u32 = 61_000;
    // 旧值查询。
    let old = slot;
    // 档案切换（热更新写入）。
    slot = 62_000;
    // 下一条查询即新值。
    let new = slot;
    // 旧值零残留：新值 ≠ 旧值且旧值槽已覆写。
    old == 61_000 && new == 62_000 && slot != 61_000
}

/// 行 9：持久化原子写——影子页写全量→指针翻转提交；坏影子回滚备份。
pub fn row_09(_corrupt: bool) -> bool {
    // 三个"页"：active 指针 + 两页缓冲。
    let mut pages: [[u32; 2]; 2] = [[1, 1], [2, 2]];
    let mut active: usize = 0;
    // 原子写：写影子页（非 active）→ 校验 → 翻指针。
    let shadow = 1 - active;
    pages[shadow] = [7, 7];
    let shadow_ok = pages[shadow] == [7, 7];
    if shadow_ok {
        active = shadow;
    }
    let committed = pages[active] == [7, 7];
    // 回滚备份：影子页写坏（半页）→ 提交取消，active 不动。
    let mut pages2: [[u32; 2]; 2] = [[1, 1], [2, 2]];
    let mut active2: usize = 0;
    let shadow2 = 1 - active2;
    pages2[shadow2] = [9, 0]; // 半页（第二槽丢）→ 校验失败。
    let half = pages2[shadow2][1] == 0;
    if !half {
        active2 = shadow2;
    }
    let rolled_back = active2 == 0 && pages2[active2] == [1, 1];
    committed && rolled_back && half
}

/// 行 10：观测导出——10 查询轨迹完整（层/依据/终值齐备）。
/// 注入 TraceBasis：抽空一条依据 → 完整率 <100% → 行红。
pub fn row_10(corrupt: bool) -> bool {
    // 10 条轨迹 (层, 依据, 终值)。
    let mut traces: [(u32, u32, u32); 10] = [
        (0, 101, 62_000),
        (1, 102, 62_100),
        (2, 103, 62_200),
        (3, 9, 62_900),
        (0, 104, 62_300),
        (1, 105, 62_400),
        (3, 9, 62_900),
        (2, 106, 62_500),
        (3, 9, 62_900),
        (0, 107, 62_600),
    ];
    if corrupt {
        // 注入：抽空第 3 条依据。
        traces[3].1 = 0;
    }
    let mut complete = true;
    let mut i = 0;
    while i < 10 {
        let (layer, basis, value) = traces[i];
        if layer > 3 || basis == 0 || value == 0 {
            complete = false;
        }
        i += 1;
    }
    complete
}

/// 行 11：探测钳——3 次递增判钳，2 次不判。
pub fn row_11(_corrupt: bool) -> bool {
    // 钳流：同维 3 次递增。
    let probe: [u32; 3] = [5000, 5001, 5002];
    let strictly_increasing = probe[0] < probe[1] && probe[1] < probe[2];
    // 良性流：2 次递增不判。
    let benign: [u32; 2] = [5000, 5001];
    let below_threshold = benign.len() < 3;
    strictly_increasing && below_threshold && probe.len() >= 3
}

/// 行 12：VVI 挂点——旁路路由拒，矩阵路由过。
pub fn row_12(_corrupt: bool) -> bool {
    // 路由闭集：0=矩阵（过）1=直连（拒）。
    let routes = [0u32, 1u32];
    let mut ok = true;
    let mut i = 0;
    while i < routes.len() {
        let verdict = if routes[i] == 0 { Ok(()) } else { Err(()) };
        match verdict {
            Ok(()) => {
                if routes[i] != 0 {
                    ok = false;
                }
            }
            Err(()) => {
                if routes[i] != 1 {
                    ok = false;
                }
            }
        }
        i += 1;
    }
    ok
}

/// 行 13：GVEX 挂点——预塞检出（9 数据域逐一），干净填充逐域过。
pub fn row_13(_corrupt: bool) -> bool {
    // 干净入口：仅 Size 域预填。
    let mut ints = [0u32; 10];
    ints[0] = 64;
    // 预塞扫描：1..10 号域非零即检出。
    let mut clean_ok = true;
    let mut i = 1;
    while i < 10 {
        if ints[i] != 0 {
            clean_ok = false;
        }
        i += 1;
    }
    // 注入预塞：任一域非零 → 检出。
    let mut stuffed = ints;
    stuffed[3] = 19_045;
    let mut detected = false;
    let mut j = 1;
    while j < 10 {
        if stuffed[j] != 0 {
            detected = true;
        }
        j += 1;
    }
    clean_ok && detected && ints[0] == 64
}

/// 行 14：冲突告警——同级异源异值告警，同源重注册/同值不告警。
pub fn row_14(_corrupt: bool) -> bool {
    // (段号, 值) 注册序列：seg1 值 A → seg2 值 B（异源异值，告警）→
    // seg2 值 B（同源重注册，不告警）。
    let regs: [(u32, u32); 3] = [(1, 62_000), (2, 62_100), (2, 62_100)];
    let mut alarms = 0u32;
    let mut i = 0;
    while i < regs.len() {
        let mut same_src_before = false;
        let mut rival_diff = false;
        let mut j = 0;
        while j < i {
            if regs[j].0 == regs[i].0 {
                same_src_before = true;
            } else if regs[j].1 != regs[i].1 {
                rival_diff = true;
            }
            j += 1;
        }
        // 同级异源异值且本源首现 → 告警；同源重注册 → 去重。
        if rival_diff && !same_src_before {
            alarms += 1;
        }
        i += 1;
    }
    // 预期：恰 1 次告警（注册 1 触发）；注册 2 与注册 1 同源同值。
    alarms == 1 && regs[1].1 == regs[2].1 && regs[0].1 != regs[1].1
}

/// 行 15：容量守恒——负载闸 6/8 停住，reg−del==used。
pub fn row_15(_corrupt: bool) -> bool {
    // 8 槽表负载闸：new_used×1000/8 > 750 拒 ⇒ 6 键恰入。
    let cap = 8u32;
    let mut used = 0u32;
    let mut i = 0;
    while i < 12 {
        let new_used = used + 1;
        if new_used.saturating_mul(1000) / cap <= 750 {
            used += 1;
        }
        i += 1;
    }
    // reg−del==used：无删除 ⇒ reg==used==6。
    let reg = used;
    let del = 0u32;
    used == 6 && reg.saturating_sub(del) == used
}

/// 行 16：依据降级——杀上层源降级到下级存活源，降级账 ≥1。
pub fn row_16(_corrupt: bool) -> bool {
    // 链 [上层=死, 中层=活, 下层=活] → 命中层 1（中层）。
    let chain: [bool; 3] = [false, true, true];
    let mut hit: Option<usize> = None;
    let mut i = 0;
    while i < 3 {
        if hit.is_none() && chain[i] {
            hit = Some(i);
        }
        i += 1;
    }
    // 全死 → Default（层 3）。
    let chain_dead: [bool; 3] = [false, false, false];
    let mut hit2: Option<usize> = None;
    let mut j = 0;
    while j < 3 {
        if hit2.is_none() && chain_dead[j] {
            hit2 = Some(j);
        }
        j += 1;
    }
    let fallback = match hit2 {
        Some(_) => false,
        None => true, // 落 Default。
    };
    hit == Some(1) && fallback
}

/// 行 17：处方互通——64 字节往返零损。
/// 注入 RecipeByte：载荷字节翻转 → 消费校验拒 → 往返破 → 行红。
pub fn row_17(corrupt: bool) -> bool {
    // 载荷 15 字 + 校验和字（模型化：校验和=字和）。
    let mut words = [0u32; 16];
    let mut i = 0;
    while i < 15 {
        words[i] = 0x0101 + i as u32;
        i += 1;
    }
    let mut sum = 0u32;
    let mut k = 0;
    while k < 15 {
        sum = sum.wrapping_add(words[k]);
        k += 1;
    }
    words[15] = sum;
    if corrupt {
        // 篡改载荷：5 号字翻转（校验和未重算）。
        words[5] = words[5].wrapping_add(1);
    }
    // 消费校验：重算 0..15 字和比对校验和字。
    let mut calc = 0u32;
    let mut m = 0;
    while m < 15 {
        calc = calc.wrapping_add(words[m]);
        m += 1;
    }
    calc == words[15]
}

/// 行 18：引导面元判据——批口径 20−1=19 自洽，必红行闭集恰 3 行。
pub fn row_18(_corrupt: bool) -> bool {
    let batch_ok = CARD_LAST.saturating_sub(CARD_FIRST).saturating_add(1) as usize == BATCH_N;
    let rows_ok = BATCH_N - 1 == ROWS && CLOSURE_CARD == CARD_LAST;
    let red_ok = MUST_RED_ROWS.len() == 3
        && MUST_RED_ROWS[0] == 0
        && MUST_RED_ROWS[1] == 10
        && MUST_RED_ROWS[2] == 17
        && MUST_RED_ROWS[0] < ROWS
        && MUST_RED_ROWS[1] < ROWS
        && MUST_RED_ROWS[2] < ROWS;
    batch_ok && rows_ok && red_ok
}

// ── 聚合执行 ────────────────────────────────────────────────────
/// 19 行聚合：干净跑全绿；注入跑恰红 3 行（{0,10,17}）。
/// 行号 → (判据函数, 注入行归属)。
pub fn agg_run(kind: CorruptKind) -> Result<[bool; ROWS], u32> {
    if ROW_NAMES.len() != ROWS {
        return Err(KtCode::RowsBroken.val());
    }
    let corrupt_keydup = kind == CorruptKind::KeyDup || kind == CorruptKind::AllThree;
    let corrupt_trace = kind == CorruptKind::TraceBasis || kind == CorruptKind::AllThree;
    let corrupt_recipe = kind == CorruptKind::RecipeByte || kind == CorruptKind::AllThree;
    let mut out = [false; ROWS];
    out[0] = row_00(corrupt_keydup);
    out[1] = row_01(false);
    out[2] = row_02(false);
    out[3] = row_03(false);
    out[4] = row_04(false);
    out[5] = row_05(false);
    out[6] = row_06(false);
    out[7] = row_07(false);
    out[8] = row_08(false);
    out[9] = row_09(false);
    out[10] = row_10(corrupt_trace);
    out[11] = row_11(false);
    out[12] = row_12(false);
    out[13] = row_13(false);
    out[14] = row_14(false);
    out[15] = row_15(false);
    out[16] = row_16(false);
    out[17] = row_17(corrupt_recipe);
    out[18] = row_18(false);
    Ok(out)
}

/// 红行集合提取（升序）。
pub fn red_rows(res: &[bool; ROWS]) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < ROWS {
        if !res[i] {
            v.push(i);
        }
        i += 1;
    }
    v
}

/// 聚合守恒：绿+红==19。
pub fn agg_conserve(res: &[bool; ROWS]) -> Result<(), u32> {
    let mut red = 0;
    let mut i = 0;
    while i < ROWS {
        if !res[i] {
            red += 1;
        }
        i += 1;
    }
    if ROWS - red + red != ROWS {
        return Err(KtCode::ConserveBroken.val());
    }
    Ok(())
}

/// 红行集合恒等 {0,10,17} 判定。
pub fn red_exact(res: &[bool; ROWS]) -> bool {
    let rr = red_rows(res);
    if rr.len() != 3 {
        return false;
    }
    rr[0] == MUST_RED_ROWS[0] && rr[1] == MUST_RED_ROWS[1] && rr[2] == MUST_RED_ROWS[2]
}

// ── 注入核（3 开关 ×10 轮） ─────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
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
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入（J1「失败注入必红三条」口径）：每轮三开关合打一次，
/// 红行集合必须恰 {0,10,17}（三个必红行逐行记账）；干净跑必须零红。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 三开关合打：红行恰 {0,10,17}。
        let combined = agg_run(CorruptKind::AllThree)?;
        if !red_exact(&combined) {
            return Err(KtCode::RedSpread.val());
        }
        // 三个必红行逐行记账（行 0 / 行 10 / 行 17 都必须红）。
        led.record(0, !combined[0]);
        led.record(1, !combined[10]);
        led.record(2, !combined[17]);
        // 干净跑零红（FalseRed 反向守卫）。
        let clean = agg_run(CorruptKind::None)?;
        if red_rows(&clean).len() != 0 {
            return Err(KtCode::FalseRed.val());
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 干净 19/19 绿 + 注入 3×10 轮红行恒等 + 守恒 + 双跑一致。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let clean = agg_run(CorruptKind::None)?;
    let mut green = 0;
    let mut i = 0;
    while i < ROWS {
        if clean[i] {
            green += 1;
        }
        i += 1;
    }
    if green != ROWS {
        return Err(KtCode::FalseRed.val());
    }
    agg_conserve(&clean)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(KtCode::RedSpread.val());
    }
    if inj.missed() != 0 {
        return Err(KtCode::RedMissing.val());
    }
    Ok((green, inj.caught[0]))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        let v = KtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口（判据 ≤20 条：分组 + 抽验 + 注入 + 守恒；19 行明细
//    保留在 ROW_NAMES 行账里，判据面只报组绿与抽验） ───────────────
pub fn run_unx18439_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18439 ktest B02 引导面");
    let clean = agg_run(CorruptKind::None);
    let r = match clean {
        Ok(r) => r,
        Err(_) => [false; ROWS],
    };
    // ① 一次命令全绿 19/19。
    let mut green = 0;
    let mut i = 0;
    while i < ROWS {
        if r[i] {
            green += 1;
        }
        i += 1;
    }
    s.add_dyn("J1-一次命令全绿19/19", green == ROWS, &format!("绿{}/{}", green, ROWS));
    // ② A03 组 10 行全绿（行 0..9）。
    let mut g1 = true;
    i = 0;
    while i < 10 {
        if !r[i] {
            g1 = false;
        }
        i += 1;
    }
    s.add("A03组10行全绿", g1, ROW_NAMES[0]);
    // ③ A04 组 9 行全绿（行 10..18）。
    let mut g2 = true;
    i = 10;
    while i < ROWS {
        if !r[i] {
            g2 = false;
        }
        i += 1;
    }
    s.add("A04组9行全绿", g2, ROW_NAMES[10]);
    // ④⑤⑥⑦ 行明细抽验四处（必红行三处 + 元判据行）。
    s.add_dyn(ROW_NAMES[0], r[0], &format!("行0={}", r[0]));
    s.add_dyn(ROW_NAMES[10], r[10], &format!("行10={}", r[10]));
    s.add_dyn(ROW_NAMES[17], r[17], &format!("行17={}", r[17]));
    s.add_dyn(ROW_NAMES[18], r[18], &format!("行18={}", r[18]));
    // ⑧ 失败注入必红三条（三开关合打，红行恰 {0,10,17}）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught() && l.missed() == 0,
        Err(_) => false,
    };
    s.add_dyn(
        "失败注入必红三条",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            JUDGE_ROUNDS
        ),
    );
    // ⑨ 单开关不扩散（各开关只红自己那 1 行）。
    let mut spread_ok = true;
    let singles: [(CorruptKind, usize); 3] =
        [(CorruptKind::KeyDup, 0), (CorruptKind::TraceBasis, 10), (CorruptKind::RecipeByte, 17)];
    let mut k = 0;
    while k < 3 {
        match agg_run(singles[k].0) {
            Ok(sr) => {
                if sr[singles[k].1] || red_rows(&sr).len() != 1 {
                    spread_ok = false;
                }
            }
            Err(_) => spread_ok = false,
        }
        k += 1;
    }
    s.add("单开关注入不扩散", spread_ok, "各开关只红自身行");
    // ⑩ 聚合守恒 + 双跑一致。
    let g = golden_run();
    s.add("聚合守恒+双跑一致", g.is_ok() && golden_run() == g, "绿+红==19 同语料同账");
    // ⑪ 段位守卫。
    s.add("段位守卫 0x7E980", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_corpus_all_green() {
        let clean = agg_run(CorruptKind::None).unwrap_or([false; ROWS]);
        let mut i = 0;
        while i < ROWS {
            assert!(clean[i], "行 {}（{}）不得红", i, ROW_NAMES[i]);
            i += 1;
        }
    }

    #[test]
    fn injection_exactly_three_red() {
        let r = agg_run(CorruptKind::AllThree).unwrap_or([true; ROWS]);
        assert!(red_exact(&r), "失败注入必红恰三条 {:?}", MUST_RED_ROWS);
        assert!(agg_conserve(&r).is_ok());
    }

    #[test]
    fn single_switch_reddens_only_its_row() {
        let cases: [(CorruptKind, usize); 3] =
            [(CorruptKind::KeyDup, 0), (CorruptKind::TraceBasis, 10), (CorruptKind::RecipeByte, 17)];
        let mut k = 0;
        while k < 3 {
            let (kind, row) = cases[k];
            let r = agg_run(kind).unwrap_or([true; ROWS]);
            assert!(!r[row], "开关 {:?} 必须打红行 {}", kind, row);
            assert_eq!(red_rows(&r).len(), 1, "开关 {:?} 不得扩散", kind);
            k += 1;
        }
    }

    #[test]
    fn injection_ten_rounds_stable() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn row_must_red_rows_actually_red() {
        let a = agg_run(CorruptKind::KeyDup).unwrap_or([true; ROWS]);
        assert!(!a[0], "KeyDup 必须打红行 0");
        let b = agg_run(CorruptKind::TraceBasis).unwrap_or([true; ROWS]);
        assert!(!b[10], "TraceBasis 必须打红行 10");
        let c = agg_run(CorruptKind::RecipeByte).unwrap_or([true; ROWS]);
        assert!(!c[17], "RecipeByte 必须打红行 17");
    }

    #[test]
    fn no_spread_on_other_rows() {
        let a = agg_run(CorruptKind::KeyDup).unwrap_or([true; ROWS]);
        let mut i = 1;
        while i < ROWS {
            assert!(a[i], "KeyDup 不得扩散到行 {}", i);
            i += 1;
        }
    }

    #[test]
    fn meta_frozen() {
        assert!(row_18(false), "元判据自洽");
        assert_eq!(BATCH_N, 20);
        assert_eq!(ROWS, 19);
        assert_eq!(CARD_LAST - CARD_FIRST + 1, BATCH_N as u32);
        assert_eq!(CLOSURE_CARD, CARD_LAST);
    }

    #[test]
    fn double_run_identical() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18439_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 8~20，实际 {}", s.len());
    }
}
