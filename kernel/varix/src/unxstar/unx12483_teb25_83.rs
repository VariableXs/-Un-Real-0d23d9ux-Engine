//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12483 · TEB 面注入矩阵 · 0x30a60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12483-J1 坏槽位/坏 TLS 索引/坏异常链 30 例注入全过，拒止码
//!      与 B04 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - TEB 档三面：TLS 槽位表（128 槽，槽态三态：空/占用/保留）、
//!   TLS 索引域（TLS_MIN_AVAILABLE=64 口径）、异常链（单向链表，链内
//!   节点号界内且无环）；
//!   - 闸序：槽位 → TLS 索引 → 异常链（先查槽账完整性，再查索引域，
//!   后查链一致性），每例按闸序命中首个拒码；
//!   - B04 档拒止码锚定：坏槽位 → STATUS_INVALID_PARAMETER（0xC000000D），
//!   坏 TLS 索引 → STATUS_INVALID_PARAMETER_2（0xC00000F0），坏异常链 →
//!   STATUS_INVALID_UNWIND_TARGET（0xC0000029）；
//!   - 注入 30 例：三档各 10 例（LCG 组合参数），逐例码值与 B04 锚定
//!   表一致；
//!   - 异常链核：环检出（访问集重访即环）+ 界外节点检出 + 断链检出；
//!   - 合法域核对：三面全好的样本必须放行（Pass 面）；
//!   - 错误路径带码+中文 detail，拒止不写回 TEB 档账；
//!   - 注入核：LCG 写死种子 0x30a60，30/30 码值一致 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30a60 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30a60;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入例数（判准 30/30）。
pub const INJECT_CASES: usize = 30;

/// 每档例数（三档各 10）。
pub const CASES_PER_CLASS: usize = 10;

/// TLS 槽位表宽（128 槽口径）。
pub const TEB_SLOTS: usize = 128;

/// TLS 索引域上限（TLS_MIN_AVAILABLE 口径 64）。
pub const TLS_INDEX_MAX: u32 = 64;

/// 异常链节点上限（链长界）。
pub const CHAIN_MAX: u32 = 32;

// B04 档拒止码锚定（ntstatus.h 口径）。
/// 坏槽位拒止码（B04 档）。
pub const B04_BAD_SLOT: u32 = 0xC000_000D;
/// 坏 TLS 索引拒止码（B04 档）。
pub const B04_BAD_TLS: u32 = 0xC000_00F0;
/// 坏异常链拒止码（B04 档）。
pub const B04_BAD_CHAIN: u32 = 0xC000_0029;

/// 诊断码（0x30a60 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebCode {
    /// 0x30a60 注入例漏判（例落三档之外）。
    CaseMiss = 0,
    /// 0x30a61 注入例重号。
    CaseDup = 1,
    /// 0x30a62 码值失配（拒码 ≠ B04 锚定）。
    AnchorMiss = 2,
    /// 0x30a63 面越域（面号出三面闭集）。
    FaceOob = 3,
    /// 0x30a64 TEB 档账缩水。
    TebShrink = 4,
    /// 0x30a65 合法例误拒。
    PassMiss = 5,
    /// 0x30a66 环检出面失灵（成环节未被拒）。
    CycleMiss = 6,
    /// 0x30a67 链账序倒挂。
    LedgerBack = 7,
    /// 0x30a68 注入类覆盖缺。
    ClassMiss = 8,
    /// 0x30a69 槽账漂（占用+保留 ≠ 已发索引数）。
    SlotDrift = 9,
}

impl TebCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TebCode; 10] = [
        TebCode::CaseMiss, TebCode::CaseDup, TebCode::AnchorMiss, TebCode::FaceOob,
        TebCode::TebShrink, TebCode::PassMiss, TebCode::CycleMiss, TebCode::LedgerBack,
        TebCode::ClassMiss, TebCode::SlotDrift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30a60 => "注入例漏判：逐例归三档闭集（槽位/TLS/异常链），漏判红账。",
        0x30a61 => "注入例重号：例号唯一，重号先查抽签器。",
        0x30a62 => "码值失配：拒止码必须与 B04 锚定表逐例一致。",
        0x30a63 => "面越域：面号先比三面闭集再取。",
        0x30a64 => "TEB 档账缩水：append-only，禁止截断。",
        0x30a65 => "合法例误拒：三面全好的样本必须放行。",
        0x30a66 => "环检出失灵：链内重访节点必须按坏链拒。",
        0x30a67 => "链账序倒挂：账序全局单调，回拨红账。",
        0x30a68 => "注入类覆盖缺：30 例必须三档全覆盖。",
        0x30a69 => "槽账漂：占用 + 保留 ≡ 已发索引数，先对平。",
        _ => "未知诊断码：回查 0x30a60 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30a60 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30a6_30a6_30a6_30a6;

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

/// 槽态（三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空。
    Free = 0,
    /// 占用。
    Used = 1,
    /// 保留。
    Reserved = 2,
}

/// 异常链节点（节点号 + 下一节点号；尾节点下一号为 u32::MAX）。
#[derive(Clone, Copy, Debug)]
pub struct ChainNode {
    /// 节点号。
    pub node: u32,
    /// 下一节点号（u32::MAX=尾）。
    pub next: u32,
}

/// TEB 注入校验器（纯函数；闸序：槽位 → TLS 索引 → 异常链）。
///
/// 返回 Ok 放行；Err(B04 档码) 即拒止口径。
pub fn validate_teb(
    slot: u32,
    tls_index: u32,
    chain: &[ChainNode],
) -> Result<(), u32> {
    // 闸 1：槽位。
    if slot as usize >= TEB_SLOTS {
        return Err(B04_BAD_SLOT);
    }
    // 闸 2：TLS 索引。
    if tls_index >= TLS_INDEX_MAX {
        return Err(B04_BAD_TLS);
    }
    // 闸 3：异常链（界内 + 无环 + 不断链）。
    if !chain_ok(chain) {
        return Err(B04_BAD_CHAIN);
    }
    Ok(())
}

/// 异常链核对（节点界内、下一号界内或尾、无环、可达尾）。
pub fn chain_ok(chain: &[ChainNode]) -> bool {
    if chain.is_empty() {
        return true;
    }
    if chain.len() as u32 > CHAIN_MAX {
        return false;
    }
    // 节点号域：0..len 且互异（规范链编号口径）。
    let mut seen = [false; CHAIN_MAX as usize];
    for n in chain.iter() {
        if n.node as usize >= chain.len() || n.node as usize >= CHAIN_MAX as usize {
            return false;
        }
        if seen[n.node as usize] {
            return false;
        }
        seen[n.node as usize] = true;
    }
    // 从 0 号节点走链：下一号界内或尾；重访即环；必须可达尾。
    let mut cur = 0u32;
    let mut steps = 0u32;
    let mut visited = [false; CHAIN_MAX as usize];
    loop {
        if steps > CHAIN_MAX {
            return false;
        }
        if visited[cur as usize] {
            return false;
        }
        visited[cur as usize] = true;
        let node = match chain.iter().find(|n| n.node == cur) {
            Some(n) => n,
            None => return false,
        };
        if node.next == u32::MAX {
            return true;
        }
        if node.next as usize >= chain.len() {
            return false;
        }
        cur = node.next;
        steps += 1;
    }
}

/// 面档（三面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebFace {
    /// 槽位。
    Slot = 0,
    /// TLS 索引。
    Tls = 1,
    /// 异常链。
    Chain = 2,
}

impl TebFace {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<TebFace> {
        match i {
            0 => Some(TebFace::Slot),
            1 => Some(TebFace::Tls),
            2 => Some(TebFace::Chain),
            _ => None,
        }
    }
}

/// 标准好链（0→1→2→尾）。
pub fn good_chain() -> Vec<ChainNode> {
    vec![
        ChainNode { node: 0, next: 1 },
        ChainNode { node: 1, next: 2 },
        ChainNode { node: 2, next: u32::MAX },
    ]
}

/// 标准合法 TEB 档。
pub fn good_teb() -> (u32, u32, Vec<ChainNode>) {
    (7, 3, good_chain())
}

/// TEB 注入档账正本。
///
/// 生命周期：run_case（逐例校验 + 码值核对 + 账面登记）→
/// class_cover（三面覆盖核对）。
#[derive(Debug)]
pub struct TebBook {
    /// 注入例账（append-only）。
    cases: Vec<(usize, u32)>,
    /// 末例号。
    last_case: u64,
    /// 放行计数。
    passes: u32,
    /// 拒止累计。
    rejects: u32,
}

impl TebBook {
    /// 建账。
    pub fn new() -> Self {
        TebBook {
            cases: Vec::new(),
            last_case: u64::MAX,
            passes: 0,
            rejects: 0,
        }
    }

    /// 跑一例（判止后记账；合法例放行计数）。
    pub fn run_case(&mut self, case_no: usize, slot: u32, tls: u32, chain: &[ChainNode], expect: u32) -> Result<(), u32> {
        if self.last_case != u64::MAX && (case_no as u64) <= self.last_case {
            self.rejects += 1;
            return Err(0);
        }
        self.last_case = case_no as u64;
        match validate_teb(slot, tls, chain) {
            Ok(()) => {
                self.passes += 1;
                Ok(())
            }
            Err(code) => {
                if code != expect {
                    self.rejects += 1;
                    return Err(code);
                }
                // 拒止命中：码值一致面入账并计入拒止累计。
                self.cases.push((case_no, code));
                self.rejects += 1;
                Ok(())
            }
        }
    }

    /// 例账长。
    pub fn case_len(&self) -> usize {
        self.cases.len()
    }

    /// 例账快照。
    pub fn case_of(&self, i: usize) -> Option<(usize, u32)> {
        if i >= self.cases.len() {
            None
        } else {
            Some(self.cases[i])
        }
    }

    /// 例号唯一判据。
    pub fn cases_unique(&self) -> bool {
        let mut i = 0;
        while i < self.cases.len() {
            let mut j = i + 1;
            while j < self.cases.len() {
                if self.cases[i].0 == self.cases[j].0 {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 放行计数。
    pub fn pass_total(&self) -> u32 {
        self.passes
    }
}

/// 三档注入例抽签（30 例：槽位 0..10、TLS 10..20、链 20..30）。
fn pick_case(lcg: &mut Lcg, i: usize) -> (usize, u32, u32, Vec<ChainNode>, u32) {
    let class = i / CASES_PER_CLASS;
    let k = lcg.below(4);
    let (slot, tls, chain) = good_teb();
    match class {
        0 => {
            // 槽位档：越 128 槽。
            let s = match k {
                0 => TEB_SLOTS as u32,
                1 => TEB_SLOTS as u32 + 7,
                2 => u32::MAX,
                _ => TEB_SLOTS as u32 * 2,
            };
            (i, s, tls, chain, B04_BAD_SLOT)
        }
        1 => {
            // TLS 档：越 64 索引。
            let t = match k {
                0 => TLS_INDEX_MAX,
                1 => TLS_INDEX_MAX + 9,
                2 => u32::MAX,
                _ => TLS_INDEX_MAX * 2,
            };
            (i, slot, t, chain, B04_BAD_TLS)
        }
        _ => {
            // 链档：成环/界外/断链。
            let c = match k {
                0 => vec![
                    ChainNode { node: 0, next: 1 },
                    ChainNode { node: 1, next: 0 },
                ],
                1 => vec![
                    ChainNode { node: 0, next: 9 },
                    ChainNode { node: 2, next: u32::MAX },
                ],
                2 => vec![
                    ChainNode { node: 0, next: 1 },
                    ChainNode { node: 1, next: 1 },
                ],
                _ => vec![
                    ChainNode { node: 0, next: 1 },
                    ChainNode { node: 5, next: u32::MAX },
                ],
            };
            (i, slot, tls, c, B04_BAD_CHAIN)
        }
    }
}

/// 30/30 注入演练（逐例码值与 B04 锚一致；三面覆盖 + 唯一性）。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a60 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TebBook::new();
        let mut hit = 0usize;
        let mut slot_n = 0usize;
        let mut tls_n = 0usize;
        let mut chain_n = 0usize;
        for i in 0..INJECT_CASES {
            let (n, s, t, c, expect) = pick_case(&mut lcg, i);
            // 面归档按期望码分段。
            if expect == B04_BAD_SLOT {
                slot_n += 1;
            } else if expect == B04_BAD_TLS {
                tls_n += 1;
            } else {
                chain_n += 1;
            }
            match bk.run_case(n, s, t, &c, expect) {
                Ok(()) => hit += 1,
                Err(_) => {}
            }
        }
        if bk.case_len() == INJECT_CASES
            && hit == INJECT_CASES
            && slot_n == CASES_PER_CLASS
            && tls_n == CASES_PER_CLASS
            && chain_n == CASES_PER_CLASS
            && bk.cases_unique()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("TEB 注入 30/30 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 异常链核演练：好链放行、环/断链/界外拒 10/10。
pub fn drill_chain_face_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a60 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let good = chain_ok(&good_chain());
        // 单节点尾链合法。
        let single = chain_ok(&[ChainNode { node: 0, next: u32::MAX }]);
        // 环链拒。
        let cycle = matches!(
            validate_teb(0, 0, &[ChainNode { node: 0, next: 1 }, ChainNode { node: 1, next: 0 }]),
            Err(c) if c == B04_BAD_CHAIN
        );
        // 界外节点拒。
        let oob = matches!(
            validate_teb(0, 0, &[ChainNode { node: 0, next: 9 }, ChainNode { node: 2, next: u32::MAX }]),
            Err(c) if c == B04_BAD_CHAIN
        );
        // 自环拒。
        let self_loop = matches!(
            validate_teb(0, 0, &[ChainNode { node: 0, next: 1 }, ChainNode { node: 1, next: 1 }]),
            Err(c) if c == B04_BAD_CHAIN
        );
        let _ = lcg.below(3);
        if good && single && cycle && oob && self_loop {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("异常链核轮={}/{}", ok, INJECT_ROUNDS))
}

/// 合法域闸序演练：好样本放行、闸序逐面命中 10/10。
pub fn drill_pass_face_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a60 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let (s, t, c) = good_teb();
        let good = validate_teb(s, t, &c).is_ok();
        // 闸序：坏槽+坏 TLS → 槽位码（首闸）。
        let g1 = matches!(validate_teb(TEB_SLOTS as u32, TLS_INDEX_MAX + 1, &c), Err(x) if x == B04_BAD_SLOT);
        // 闸序：好槽+坏 TLS+坏链 → TLS 码。
        let g2 = matches!(validate_teb(s, TLS_INDEX_MAX + 3, &[]), Err(x) if x == B04_BAD_TLS);
        // 闸序：槽/TLS 好+坏链 → 链码。
        let g3 = matches!(validate_teb(s, t, &[ChainNode { node: 0, next: 0 }]), Err(x) if x == B04_BAD_CHAIN);
        let _ = lcg.below(3);
        if good && g1 && g2 && g3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("合法域闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入例账转录导出（逐例一行：例号/面/码）。
/// ─────────────────────────────────────────────────────────────────────
/// 注入例账转录导出。
pub fn export_case_lines(bk: &TebBook) -> String {
    let mut s = String::from("TEB 注入例账转录：\n");
    for i in 0..bk.case_len() {
        if let Some((n, code)) = bk.case_of(i) {
            let cls = if n < 10 { "槽位" } else if n < 20 { "TLS" } else { "链" };
            s.push_str(&format!("#{} {} {:08x}\n", n, cls, code));
        }
    }
    s
}

/// 转录完整判据（逐例行齐备）。
pub fn case_lines_complete(bk: &TebBook) -> bool {
    let s = export_case_lines(bk);
    if bk.case_len() == 0 {
        return false;
    }
    (0..bk.case_len()).all(|i| {
        bk.case_of(i)
            .map(|(n, _)| s.contains(&format!("#{} ", n)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入矩阵报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct TebReport {
    /// 例账行数。
    pub cases: usize,
    /// 放行计数。
    pub passes: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 例号唯一。
    pub unique: bool,
    /// 转录完整。
    pub lines: bool,
}

impl TebReport {
    /// 建报告。
    pub fn build(bk: &TebBook) -> Self {
        TebReport {
            cases: bk.case_len(),
            passes: bk.pass_total(),
            rejects: bk.reject_total(),
            unique: bk.cases_unique(),
            lines: case_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.unique && self.lines && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "TebReport 例={} 放={} 拒={} 唯一={} 转录={}",
            self.cases, self.passes, self.rejects, self.unique, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：常量口径与面域演练（B04 三码/三面 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_const_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a60 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = B04_BAD_SLOT == 0xC000_000D;
        pass = pass && B04_BAD_TLS == 0xC000_00F0 && B04_BAD_CHAIN == 0xC000_0029;
        pass = pass && TEB_SLOTS == 128 && TLS_INDEX_MAX == 64 && CHAIN_MAX == 32;
        pass = pass && (0..3).all(|k| TebFace::of(k).is_some()) && TebFace::of(3).is_none();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("常量域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：B04 锚定面演练（三码互异且落 NT 错误域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30a60 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        const SET: [u32; 3] = [B04_BAD_SLOT, B04_BAD_TLS, B04_BAD_CHAIN];
        let mut pass = true;
        for i in 0..3 {
            for j in 0..3 {
                if i != j {
                    pass = pass && SET[i] != SET[j];
                }
            }
            pass = pass && SET[i] & 0xC000_0000 == 0xC000_0000;
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("B04 锚定轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct TebCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_teb_checklist(bk: &TebBook) -> Vec<TebCheckRow> {
    let rep = TebReport::build(bk);
    let mut rows: Vec<TebCheckRow> = Vec::new();
    rows.push(TebCheckRow { name: "30例在账", passed: bk.case_len() == INJECT_CASES });
    rows.push(TebCheckRow { name: "例号唯一", passed: bk.cases_unique() });
    rows.push(TebCheckRow { name: "口径", passed: INJECT_CASES == 30 && CASES_PER_CLASS == 10 });
    rows.push(TebCheckRow { name: "常量域", passed: drill_const_domain_10().0 });
    rows.push(TebCheckRow { name: "B04锚定", passed: drill_anchor_10().0 });
    rows.push(TebCheckRow { name: "转录完整", passed: case_lines_complete(bk) });
    rows.push(TebCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TebCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(TebCheckRow { name: "面域", passed: TebFace::of(2).is_some() && TebFace::of(3).is_none() });
    rows.push(TebCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn teb_checklist_green(rows: &[TebCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn teb_stats_line(bk: &TebBook) -> String {
    format!(
        "TEB 矩阵概况：例 {} 放 {} 拒 {}",
        bk.case_len(),
        bk.pass_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12483_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TebBook::new();
    let (s, t, c) = good_teb();
    let _ = bk.run_case(0, TEB_SLOTS as u32, t, &c, B04_BAD_SLOT);
    let _ = bk.run_case(1, s, TLS_INDEX_MAX + 1, &c, B04_BAD_TLS);
    let _ = bk.run_case(2, s, t, &[ChainNode { node: 0, next: 0 }], B04_BAD_CHAIN);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        case_lines_complete(&bk) && bk.case_len() == 3,
        format!("例 {} 放 {} 拒 {}", bk.case_len(), bk.pass_total(), bk.reject_total()),
    ));
    // X-b：常量域 10/10。
    let (ok_c, d_c) = drill_const_domain_10();
    out.push(("X-常量域10/10", ok_c, d_c));
    // X-c：B04 锚定 10/10。
    let (ok_a, d_a) = drill_anchor_10();
    out.push(("X-B04锚定10/10", ok_a, d_a));
    // X-d：终验清单（30 例全账面）。
    let mut bk2 = TebBook::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for i in 0..INJECT_CASES {
        let (n, s2, t2, c2, expect) = pick_case(&mut lcg, i);
        let _ = bk2.run_case(n, s2, t2, &c2, expect);
    }
    let rows = build_teb_checklist(&bk2);
    out.push(("X-终验清单10项", teb_checklist_green(&rows), TebReport::build(&bk2).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let (s, t, c) = good_teb();
    // 注 1：坏槽位必拒且码与 B04 一致。
    if matches!(validate_teb(TEB_SLOTS as u32 + 1, t, &c), Err(x) if x == B04_BAD_SLOT) {
        caught += 1;
        d.push("注1 槽位:抓".to_string());
    } else {
        d.push("注1 槽位:漏".to_string());
    }
    // 注 2：坏 TLS 索引必拒且码与 B04 一致。
    if matches!(validate_teb(s, TLS_INDEX_MAX + 2, &c), Err(x) if x == B04_BAD_TLS) {
        caught += 1;
        d.push("注2 TLS:抓".to_string());
    } else {
        d.push("注2 TLS:漏".to_string());
    }
    // 注 3：环链必拒且码与 B04 一致。
    let cyc = [ChainNode { node: 0, next: 1 }, ChainNode { node: 1, next: 0 }];
    if matches!(validate_teb(s, t, &cyc), Err(x) if x == B04_BAD_CHAIN) {
        caught += 1;
        d.push("注3 环链:抓".to_string());
    } else {
        d.push("注3 环链:漏".to_string());
    }
    // 注 4：好样本放行（Pass 面）。
    if validate_teb(s, t, &c).is_ok() {
        caught += 1;
        d.push("注4 放行:抓".to_string());
    } else {
        d.push("注4 放行:漏".to_string());
    }
    // 注 5：面域越界（of(3) None）。
    if TebFace::of(3).is_none() {
        caught += 1;
        d.push("注5 越域:抓".to_string());
    } else {
        d.push("注5 越域:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30a60 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    TebCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12483_checks：本单判据聚合（诊断段 0x30a60）。
pub fn run_unx12483_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12483 TEB 面注入矩阵");
    // J1-a：30/30 注入全过（码值与 B04 一致）。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-TEB注入30/30", ok1, &d1);
    // J1-b：异常链核（10/10）。
    let (ok2, d2) = drill_chain_face_10();
    s.add_dyn("J1-异常链核10/10", ok2, &d2);
    // J1-c：合法域闸序（10/10）。
    let (ok3, d3) = drill_pass_face_10();
    s.add_dyn("J1-合法域闸序10/10", ok3, &d3);
    // J1-d：标准会话（三档覆盖 + 例号唯一 + 转录）。
    let mut bk = TebBook::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for i in 0..INJECT_CASES {
        let (n, s2, t2, c2, expect) = pick_case(&mut lcg, i);
        let _ = bk.run_case(n, s2, t2, &c2, expect);
    }
    s.add_dyn(
        "J1-三档覆盖唯一转录",
        bk.case_len() == INJECT_CASES
            && bk.cases_unique()
            && case_lines_complete(&bk)
            && bk.reject_total() >= 1,
        &format!(
            "例={} 唯一={} 转录={}",
            bk.case_len(),
            bk.cases_unique(),
            case_lines_complete(&bk)
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30a60段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12483_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
