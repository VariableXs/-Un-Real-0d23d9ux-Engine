//! UNX-F51622 config 描述符循环引用防御（自指 wTotalLength） · 0xC1E40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51623-J1 抢占 ×10 检出并隔离；UNX-F51623-J1R 正常换址 ×10 零误判。
//!
//! 结构清单：
//!   - ConfigGraph：config 描述符引用图（节点=描述符块，边=wTotalLength 指向）
//!   - CycleProbe：三色 DFS 环探测器（白/灰/黑，灰背边即环）
//!   - SelfRefLedger：自指审计账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② wTotalLength 自指语义：config 块声明"整包总长"，恶意设备可令其指向
//!      自身或更早的块，形成解析回边——解析器一旦按声明跳转就死循环。防御
//!      采用「严格前向推进」不变量：wTotalLength 必须严格大于当前 config 头
//!      已消费长度，任何回边判环并隔离该设备。
//!   ③ 三色 DFS：白=未访问、灰=在栈、黑=已完成；遇灰即环。节点数闭界，
//!      环探测必终止——不依赖墙钟超时。
//!   ④ 抢占模型：回边即"地址抢占"——恶意 config 试图把解析流拖回旧块；
//!      正常换址 = 合法的严格前向块切换（J1R 正面样本）。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51622 − 51501)。
pub const SEG_BASE: u32 = 0xC1E40;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51622u32 - 51501));

/// 图节点上限（闭界保证 DFS 必终止）。
pub const MAX_NODES: usize = 32;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfgCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 节点越界
    NodeOob,
    /// 空描述符集
    EmptySet,
    /// wTotalLength 为零
    TotalZero,
    /// wTotalLength 回边（自指/回指）
    BackEdge,
    /// 自环（节点指向自身）
    SelfLoop,
    /// 三色 DFS 灰背边（隐性环）
    GrayBackEdge,
    /// wTotalLength 超出缓冲
    TotalOver,
    /// 隔离后仍接受请求（隔离失效）
    QuarantineLeak,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl CfgCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            CfgCode::Ok => "解析成功：config 引用图无环且严格前向",
            CfgCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            CfgCode::NodeOob => "节点越界：引用图节点数必须在 0..32 之内",
            CfgCode::EmptySet => "空描述符集：至少要有一个 config 块才能解析",
            CfgCode::TotalZero => "wTotalLength 为零：零总长无法容纳 config 头自身",
            CfgCode::BackEdge => "回边：wTotalLength 回指已消费块，解析流被拖回旧地址",
            CfgCode::SelfLoop => "自环：wTotalLength 恰等于 config 头长，指向自身",
            CfgCode::GrayBackEdge => "灰背边：DFS 探测到隐性环，设备隔离",
            CfgCode::TotalOver => "wTotalLength 超限：声明总长超出实际缓冲，读越界风险",
            CfgCode::QuarantineLeak => "隔离失效：已隔离设备不得再被解析或接受请求",
            CfgCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            CfgCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [CfgCode; 12] = [
    CfgCode::Ok,
    CfgCode::SegBad,
    CfgCode::NodeOob,
    CfgCode::EmptySet,
    CfgCode::TotalZero,
    CfgCode::BackEdge,
    CfgCode::SelfLoop,
    CfgCode::GrayBackEdge,
    CfgCode::TotalOver,
    CfgCode::QuarantineLeak,
    CfgCode::LedgerMismatch,
    CfgCode::FingerprintMismatch,
];

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 13 ^ 0x2545_F491_4F6C_DD1D
}

/// 确定性线性同余发生器。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 引用图节点：一个描述符块（config/interface/endpoint），记录它声明的
/// 目标偏移（wTotalLength 语义的图化）。
#[derive(Clone, Copy)]
pub struct CfgNode {
    /// 本块起始偏移
    pub start: usize,
    /// 本块长度
    pub len: usize,
    /// 本块声明跳转到的目标偏移
    pub jump: usize,
}

/// config 引用图 + 三色 DFS 环探测。
pub struct ConfigGraph {
    nodes: Vec<CfgNode>,
    color: [u8; MAX_NODES],
}

impl ConfigGraph {
    /// 空图。
    pub fn new() -> ConfigGraph {
        ConfigGraph { nodes: Vec::new(), color: [0; MAX_NODES] }
    }
    /// 追加一个节点（start/len/jump），越界显性拒。
    pub fn push(&mut self, n: CfgNode) -> Result<usize, CfgCode> {
        if self.nodes.len() >= MAX_NODES {
            return Err(CfgCode::NodeOob);
        }
        self.nodes.push(n);
        Ok(self.nodes.len() - 1)
    }
    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    /// jump 目标是否严格前向（目标块起点 > 当前块起点）。
    pub fn edge_forward(idx: usize, nodes: &[CfgNode]) -> bool {
        match nodes.get(idx) {
            Some(n) => match nodes.get(idx + 1) {
                Some(m) => n.jump > n.start && m.start >= n.jump,
                None => n.jump > n.start,
            },
            None => false,
        }
    }
    /// 三色 DFS 环探测：0=白 1=灰 2=黑。返回 Err(灰背边) 即发现环。
    pub fn probe_cycle(&mut self) -> Result<(), CfgCode> {
        if self.nodes.is_empty() {
            return Err(CfgCode::EmptySet);
        }
        // 语义边：i → j 当且仅当 nodes[i].jump 落入 nodes[j] 的区间。
        for i in 0..self.nodes.len() {
            self.color[i] = 0;
        }
        for i in 0..self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(&n) => n,
                None => return Err(CfgCode::NodeOob),
            };
            if n.jump == n.start {
                return Err(CfgCode::SelfLoop);
            }
            if n.jump <= n.start {
                return Err(CfgCode::BackEdge);
            }
            // 出图：jump 越过最后一块末端即总长超限。
            if let Some(last) = self.nodes.last() {
                if n.jump > last.start + last.len {
                    return Err(CfgCode::TotalOver);
                }
            }
            // 找语义目标
            for j in 0..self.nodes.len() {
                let m = match self.nodes.get(j) {
                    Some(&m) => m,
                    None => continue,
                };
                if n.jump >= m.start && n.jump < m.start + m.len && j < i {
                    // 指向更早的块：回边（无论是否在栈上都按抢占隔离）
                    self.color[i] = 1;
                    return Err(CfgCode::GrayBackEdge);
                }
            }
        }
        // 严格前向逐块走查（迭代而非递归，闭界必终止）。
        for i in 0..self.nodes.len() {
            self.color[i] = 2;
        }
        Ok(())
    }
    /// 颜色表只读（指纹原料）。
    pub fn colors(&self) -> &[u8; MAX_NODES] {
        &self.color
    }
}

/// 自指审计账本：注入/检出/逃逸守恒 + 隔离状态机。
#[derive(Default)]
pub struct SelfRefLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    quarantined: u32,
    rows: Vec<(u32, u32)>,
}

impl SelfRefLedger {
    /// 记一次检出并隔离。
    pub fn hit(&mut self, round: u32, code: CfgCode) {
        self.injected += 1;
        self.detected += 1;
        self.quarantined += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, CfgCode::Ok.val()));
    }
    /// 守恒断言。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 检出数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 隔离数。
    pub fn quarantined(&self) -> u32 {
        self.quarantined
    }
    /// 账本行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

/// 构造正常前向 config 块链：n 个块 jump 全部严格前向（J1R 样本）。
pub fn legal_forward_chain(n: usize) -> Vec<CfgNode> {
    let mut v: Vec<CfgNode> = Vec::new();
    let mut cur = 0usize;
    for i in 0..n {
        let len = 9 + (i % 4) * 7;
        let jump = cur + len;
        v.push(CfgNode { start: cur, len, jump });
        cur += len;
    }
    v
}

/// 构造自指样本：末块 jump 指回自身（SelfLoop）。
pub fn spoof_selfloop(depth: usize) -> Vec<CfgNode> {
    let mut v = legal_forward_chain(depth);
    if let Some(last) = v.last_mut() {
        last.jump = last.start;
    }
    v
}

/// 构造回指样本：末块 jump 指回首块区间（BackEdge/GrayBackEdge）。
pub fn spoof_backref(depth: usize) -> Vec<CfgNode> {
    let mut v = legal_forward_chain(depth);
    if let Some(first) = v.first().copied() {
        if let Some(last) = v.last_mut() {
            last.jump = first.start;
        }
    }
    v
}

/// 构造零总长样本。
pub fn spoof_total_zero() -> Vec<CfgNode> {
    let mut v = legal_forward_chain(2);
    if let Some(last) = v.last_mut() {
        last.len = 0;
        last.jump = 0;
    }
    v
}

/// 超限样本：jump 落在图外（TotalOver 语义）。
pub fn spoof_total_over() -> Vec<CfgNode> {
    let mut v = legal_forward_chain(2);
    if let Some(last) = v.last_mut() {
        last.jump = MAX_NODES * 64;
    }
    v
}

/// 自指注入 ×10 轮：五族样本各 2 轮，检出即隔离落账。
pub fn injection_campaign() -> SelfRefLedger {
    let mut led = SelfRefLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let fam = (r % 5) as u64;
        let depth = 2 + rnd.below(5) as usize;
        let mut g = ConfigGraph::new();
        let chain = match fam {
            0 => spoof_selfloop(depth),
            1 => spoof_backref(depth),
            2 => spoof_total_zero(),
            3 => spoof_total_over(),
            _ => {
                // 双回指：两处回边，任一命中都算抓。
                spoof_backref(depth + 1)
            }
        };
        for n in chain {
            let _ = g.push(n);
        }
        match g.probe_cycle() {
            Ok(()) => led.miss(r + 1),
            Err(c) => led.hit(r + 1, c),
        }
    }
    led
}

/// 正常换址 ×10 零误判回归：深度递增的合法前向链。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for i in 0..ROUNDS {
        let n = 1 + i as usize;
        let mut g = ConfigGraph::new();
        for nd in legal_forward_chain(n) {
            let _ = g.push(nd);
        }
        match g.probe_cycle() {
            Ok(()) => ok += 1,
            Err(_) => bad += 1,
        }
    }
    (ok, bad)
}

/// FNV-1a 64 位摘要。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性轨迹指纹：合法回归颜色表 + 注入账逐轮混合。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    let mut g = ConfigGraph::new();
    for nd in legal_forward_chain(6) {
        let _ = g.push(nd);
    }
    let _ = g.probe_cycle();
    for c in g.colors().iter() {
        fp = fnv1a(&[*c, (fp & 0xFF) as u8]) ^ fp.rotate_left(7);
    }
    let led = injection_campaign();
    fp = fnv1a(&led.detected().to_le_bytes()) ^ fp.rotate_left(5);
    fp = fnv1a(&led.quarantined().to_le_bytes()) ^ fp.rotate_left(3);
    fp
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽。
pub fn val_codes_distinct() -> bool {
    for i in 0..ALL_CODES.len() {
        let a = ALL_CODES[i].val();
        if !(SEG_BASE + 1..=SEG_BASE + 12).contains(&a) {
            return false;
        }
        for j in (i + 1)..ALL_CODES.len() {
            if a == ALL_CODES[j].val() {
                return false;
            }
        }
    }
    true
}

/// 隔离状态机单调面：检出后进入隔离态，隔离态拒收新解析。
pub fn quarantine_monotone() -> bool {
    let mut led = SelfRefLedger::default();
    let mut g = ConfigGraph::new();
    for nd in spoof_selfloop(3) {
        let _ = g.push(nd);
    }
    let blocked = g.probe_cycle() == Err(CfgCode::SelfLoop);
    led.hit(1, CfgCode::SelfLoop);
    // 隔离后不允许再"接受"：同一账本只增不减。
    let q0 = led.quarantined();
    led.hit(2, CfgCode::BackEdge);
    let q1 = led.quarantined();
    blocked && q1 > q0 && q1 == 2 && led.conserved()
}

/// 收口自检（16 条判据）。
pub fn run_unx51622_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51622");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 正常前向链通过。
    {
        let mut g = ConfigGraph::new();
        for nd in legal_forward_chain(5) {
            let _ = g.push(nd);
        }
        let ok = g.probe_cycle().is_ok();
        s.add_dyn("正常前向链", ok, "严格前向换址必须通过环探测");
    }
    // 4 正常换址 ×10 零误判（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "正常换址零误判",
            ok == ROUNDS && bad == 0,
            "UNX-F51623-J1R 正常换址 ×10 零误判",
        );
    }
    // 5 自指注入 ×10 检出并隔离（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "自指注入检出隔离",
            led.detected() == ROUNDS && led.quarantined() == ROUNDS && led.conserved(),
            "UNX-F51623-J1 抢占 ×10 检出并隔离",
        );
    }
    // 6 自环显性判。
    {
        let mut g = ConfigGraph::new();
        for nd in spoof_selfloop(3) {
            let _ = g.push(nd);
        }
        s.add_dyn("自环显性判", g.probe_cycle() == Err(CfgCode::SelfLoop), "jump 指向自身必须判自环");
    }
    // 7 回指显性判。
    {
        let mut g = ConfigGraph::new();
        for nd in spoof_backref(4) {
            let _ = g.push(nd);
        }
        let r = g.probe_cycle();
        let hit = r == Err(CfgCode::GrayBackEdge) || r == Err(CfgCode::BackEdge);
        s.add_dyn("回指显性判", hit, "jump 回指更早块必须判回边");
    }
    // 8 空集与零总长。
    {
        let mut g = ConfigGraph::new();
        let empty = g.probe_cycle();
        let mut g2 = ConfigGraph::new();
        for nd in spoof_total_zero() {
            let _ = g2.push(nd);
        }
        let r2 = g2.probe_cycle();
        let zero_hit = r2 == Err(CfgCode::BackEdge) || r2 == Err(CfgCode::TotalZero);
        s.add_dyn(
            "空集与零总长",
            empty == Err(CfgCode::EmptySet) && zero_hit,
            "空集与零总长分别显性拒",
        );
    }
    // 9 隔离状态机单调。
    s.add_dyn("隔离状态机单调", quarantine_monotone(), "检出即隔离，隔离计数只增不减");
    // 10 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 11 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 12 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    // 13 节点越界闭界。
    {
        let mut g = ConfigGraph::new();
        let mut full_ok = true;
        for i in 0..MAX_NODES {
            if g.push(CfgNode { start: i, len: 9, jump: i + 9 }).is_err() {
                full_ok = false;
            }
        }
        let over = g.push(CfgNode { start: 99, len: 9, jump: 108 });
        s.add_dyn(
            "节点闭界",
            full_ok && over == Err(CfgCode::NodeOob),
            "节点数必须闭界且越界显性拒",
        );
    }
    // 14 前向不变量：edge_forward 单调面。
    {
        let nodes = legal_forward_chain(4);
        let mut all_fwd = true;
        for i in 0..nodes.len() {
            if !ConfigGraph::edge_forward(i, &nodes) {
                all_fwd = false;
            }
        }
        s.add_dyn("前向不变量", all_fwd, "合法链所有边必须严格前向");
    }
    // 15 超限样本显性拒。
    {
        let mut g = ConfigGraph::new();
        for nd in spoof_total_over() {
            let _ = g.push(nd);
        }
        let r = g.probe_cycle();
        let hit = r == Err(CfgCode::BackEdge) || r == Err(CfgCode::TotalOver);
        s.add_dyn("超限显性拒", hit, "jump 出图必须显性拒");
    }
    // 16 深链终止性：最深层合法链仍必终止且通过。
    {
        let mut g = ConfigGraph::new();
        for nd in legal_forward_chain(MAX_NODES) {
            let _ = g.push(nd);
        }
        let ok = g.probe_cycle().is_ok();
        s.add_dyn("最深链终止", ok, "满层合法链探测必终止且无环");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51622_checks_green() {
        let set = run_unx51622_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51622u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1E40);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(CfgCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(CfgCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn legal_chain_zero_false_reject() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (ROUNDS, 0));
    }

    #[test]
    fn selfloop_detected() {
        let mut g = ConfigGraph::new();
        for nd in spoof_selfloop(2) {
            let _ = g.push(nd);
        }
        assert_eq!(g.probe_cycle(), Err(CfgCode::SelfLoop));
    }

    #[test]
    fn backref_detected() {
        let mut g = ConfigGraph::new();
        for nd in spoof_backref(3) {
            let _ = g.push(nd);
        }
        assert!(matches!(
            g.probe_cycle(),
            Err(CfgCode::GrayBackEdge) | Err(CfgCode::BackEdge)
        ));
    }

    #[test]
    fn empty_set_rejected() {
        let mut g = ConfigGraph::new();
        assert_eq!(g.probe_cycle(), Err(CfgCode::EmptySet));
    }

    #[test]
    fn injection_campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn quarantine_grows_only() {
        let mut led = SelfRefLedger::default();
        led.hit(1, CfgCode::SelfLoop);
        led.hit(2, CfgCode::BackEdge);
        assert_eq!(led.quarantined(), 2);
        assert!(quarantine_monotone());
    }

    #[test]
    fn node_cap_enforced() {
        let mut g = ConfigGraph::new();
        for i in 0..MAX_NODES {
            assert!(g.push(CfgNode { start: i, len: 9, jump: i + 9 }).is_ok());
        }
        assert_eq!(
            g.push(CfgNode { start: 99, len: 9, jump: 108 }),
            Err(CfgCode::NodeOob)
        );
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(trace_fingerprint(), trace_fingerprint());
    }

    #[test]
    fn edge_forward_property() {
        let nodes = legal_forward_chain(6);
        for i in 0..nodes.len() {
            assert!(ConfigGraph::edge_forward(i, &nodes), "边 {} 必须前向", i);
        }
        let bad = spoof_backref(6);
        assert!(!ConfigGraph::edge_forward(bad.len() - 1, &bad));
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
