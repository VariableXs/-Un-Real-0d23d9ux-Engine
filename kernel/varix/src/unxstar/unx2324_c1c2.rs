//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2324 · C1/C2 联签件（调度实体绑定核验） · 0x72C40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B37）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F2324-J1 C1/C2 联签核验通过（sched_entity 创建/唤醒/
//!     优先级设置三接口消费面——发证归 C 部入队归 A3 判例对表），fake 对拍
//!     10/10 复跑一致，联签记录落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 三接口消费面：创建（Create）/ 唤醒（Wakeup）/ 优先级设置（PrioSet），
//!     绑定序硬约束——实体必须先有创建联签，唤醒/优先级记录才可落账；
//!   · 判例对表（冻结常量）：三接口 × 发证方（C 部）× 入队方（A3）逐条
//!     对表，签章方与判例不符即 PairMismatch；
//!   · 双签章：C1 = C 部章 / C2 = A3 章（异种子混洗），重算不等即伪造；
//!   · 状态机 Pending→Cosigned→Filed 单调推进，倒退显性拒；
//!   · fake 对拍 10 路（双章错/接口换/实体改/判倒退/未绑定/序重号/
//!     发证方错/入队方错/截尾），核验器 10/10 必抓，复跑两次全同。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72C40，VX3-1103 C1/C2 联签窗口独占）。
pub const SEG_BASE: u32 = 0x72C40;
/// 消费面接口数（冻结 3：创建/唤醒/优先级设置）。
pub const IFACE_N: usize = 3;
/// 联签纪元（冻结 3）。
pub const EPOCH: u64 = 3;
/// 黄金账实体数（冻结 12：每实体 3 接口记录 → 36 单）。
pub const ENTITY_N: usize = 12;
/// fake 对拍路数（判据原文 10/10，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// C 部章种子（冻结 0xC1C1C1C1）。
pub const C1_SEED: u32 = 0xC1C1_C1C1;
/// A3 章种子（冻结 0xA3A3A3A3）。
pub const C2_SEED: u32 = 0xA3A3_A3A3;
/// 发证方标签（判例对表冻结：发证归 C 部）。
pub const ISSUER_C: &str = "C-DEPT";
/// 入队方标签（判例对表冻结：入队归 A3）。
pub const ENQUEUE_A3: &str = "A3-QUEUE";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignCode {
    /// 接口未知：只认创建/唤醒/优先级设置三接口，其余一律拒
    IfaceUnknown,
    /// 签章伪造：C1/C2 任一章重算不等即伪造
    SigForged,
    /// 实体未绑定：唤醒/优先级记录落账前实体必须已有创建联签
    EntityUnbound,
    /// 判定倒退：Pending→Cosigned→Filed 单调，倒退显性拒
    VerdictBack,
    /// 判例不符：签章方/入队方与判例对表不符
    PairMismatch,
    /// 空联签账：空账拒绝落档
    LedgerEmpty,
    /// 复跑漂移：全账重算与落档件逐字节不一致
    ReplayDrift,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl CosignCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CosignCode::IfaceUnknown => "接口未知：只认 Create/Wakeup/PrioSet 三接口",
            CosignCode::SigForged => "签章伪造：C1/C2 章重算不等，逐单定位伪造记录",
            CosignCode::EntityUnbound => "实体未绑定：先创建联签再谈唤醒/优先级，按实体号回查",
            CosignCode::VerdictBack => "判定倒退：状态机单调推进，倒退单即拒",
            CosignCode::PairMismatch => "判例不符：发证归 C 部/入队归 A3，对表不符即拒",
            CosignCode::LedgerEmpty => "空联签账：空账不落档，先建账再核验",
            CosignCode::ReplayDrift => "复跑漂移：全账重算与落档件不一致，逐字节定位差异单",
            CosignCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [CosignCode; 8] = [
        CosignCode::IfaceUnknown,
        CosignCode::SigForged,
        CosignCode::EntityUnbound,
        CosignCode::VerdictBack,
        CosignCode::PairMismatch,
        CosignCode::LedgerEmpty,
        CosignCode::ReplayDrift,
        CosignCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CosignCode::ALL.len() {
        if CosignCode::ALL[i].val() == code {
            return CosignCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72C40 段闭集内"
}

// ── 消费面接口（冻结三接口） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iface {
    /// 创建：sched_entity 挂账 birth 单。
    Create,
    /// 唤醒：入队唤醒单。
    Wakeup,
    /// 优先级设置：权重/ nice 调整单。
    PrioSet,
}

impl Iface {
    /// 接口序值（判定/落账序冻结）。
    pub fn ord(self) -> u8 {
        match self {
            Iface::Create => 0,
            Iface::Wakeup => 1,
            Iface::PrioSet => 2,
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            Iface::Create => "CREATE",
            Iface::Wakeup => "WAKEUP",
            Iface::PrioSet => "PRIOSET",
        }
    }
    /// 按标签还原接口（未知标签 None）。
    pub fn from_tag(t: &str) -> Option<Iface> {
        match t {
            "CREATE" => Some(Iface::Create),
            "WAKEUP" => Some(Iface::Wakeup),
            "PRIOSET" => Some(Iface::PrioSet),
            _ => None,
        }
    }
    /// 全接口表（判例对表用）。
    pub const ALL: [Iface; IFACE_N] = [Iface::Create, Iface::Wakeup, Iface::PrioSet];
}

// ── 判例对表（冻结常量：三接口 × 发证方 × 入队方） ──────────────
/// 判例三元组：接口 / 发证方（C 部）/ 入队方（A3）。
pub const PRECEDENT_TABLE: [(u8, &'static str, &'static str); IFACE_N] = [
    (0, ISSUER_C, ENQUEUE_A3),
    (1, ISSUER_C, ENQUEUE_A3),
    (2, ISSUER_C, ENQUEUE_A3),
];

/// 判例对表核：接口序值命中且两方与判例全等。
pub fn precedent_match(iface: Iface, issuer: &str, enqueuer: &str) -> bool {
    match PRECEDENT_TABLE.iter().find(|(o, _, _)| *o == iface.ord()) {
        Some((_, is, en)) => *is == issuer && *en == enqueuer,
        None => false,
    }
}

// ── 联签状态机（Pending→Cosigned→Filed，单调推进） ──────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CosignState {
    /// 待签：双章未齐。
    Pending,
    /// 已联签：双章重算均等。
    Cosigned,
    /// 已落档：联签记录入终账。
    Filed,
}

impl CosignState {
    /// 单调序值：Pending=0 < Cosigned=1 < Filed=2。
    pub fn rank(self) -> u8 {
        match self {
            CosignState::Pending => 0,
            CosignState::Cosigned => 1,
            CosignState::Filed => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: CosignState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 联签记录：seq 全账单调，entity 绑定键，双章 + 判定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CosignRecord {
    pub seq: u64,
    pub entity: u64,
    pub iface: u8,
    pub issuer: String,
    pub enqueuer: String,
    pub c1_sig: u32,
    pub c2_sig: u32,
    pub state: u8,
}

/// 联签账：纪元 + 记录表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CosignLedger {
    pub epoch: u64,
    pub records: Vec<CosignRecord>,
}

/// FNV-1a 32 位（章值基座，整数实现）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 章值：FNV-1a("E<entity>|I<iface>|S<seq>") 后与章种子混洗。
pub fn seal_of(entity: u64, iface: u8, seq: u64, seed: u32) -> u32 {
    let raw = format!("E{}|I{}|S{}", entity, iface, seq);
    fnv1a(raw.as_bytes()) ^ seed
}

/// C1 章（C 部发证章）。
pub fn c1_seal(entity: u64, iface: u8, seq: u64) -> u32 {
    seal_of(entity, iface, seq, C1_SEED)
}

/// C2 章（A3 入队章）。
pub fn c2_seal(entity: u64, iface: u8, seq: u64) -> u32 {
    seal_of(entity, iface, seq, C2_SEED)
}

// ── 联签簿记员（建账 + 绑定序 + 状态机） ────────────────────────
#[derive(Clone, Debug)]
pub struct Cosigner {
    pub ledger: CosignLedger,
    /// 簿记开销：章值计算次数 + 查重扫描数（诊断用）。
    pub admin_ops: u64,
    pub payload_ops: u64,
}

impl Cosigner {
    pub fn new(epoch: u64) -> Cosigner {
        Cosigner {
            ledger: CosignLedger { epoch, records: Vec::new() },
            admin_ops: 0,
            payload_ops: 0,
        }
    }

    /// 联签落账：七门校验（接口合法/判例对表/绑定序/双章重算/状态单调/seq 单调）。
    pub fn cosign(
        &mut self,
        entity: u64,
        iface: Iface,
        issuer: &str,
        enqueuer: &str,
        c1_sig: u32,
        c2_sig: u32,
    ) -> Result<u64, u32> {
        // 门 1：接口闭集。
        let io = iface.ord() as usize;
        if io >= IFACE_N {
            return Err(CosignCode::IfaceUnknown.val());
        }
        // 门 2：判例对表（发证归 C 部 / 入队归 A3）。
        if !precedent_match(iface, issuer, enqueuer) {
            return Err(CosignCode::PairMismatch.val());
        }
        // 门 3：绑定序——唤醒/优先级记录前实体必须已有创建联签。
        if iface != Iface::Create && !self.entity_bound(entity) {
            return Err(CosignCode::EntityUnbound.val());
        }
        // 门 4：seq 单调（全账连续，从 1 起）。
        let seq = (self.ledger.records.len() + 1) as u64;
        // 门 5/6：双章重算（章值由 entity/iface/seq 派生）。
        let c1 = c1_seal(entity, io as u8, seq);
        let c2 = c2_seal(entity, io as u8, seq);
        self.admin_ops += 2;
        if c1_sig != c1 || c2_sig != c2 {
            return Err(CosignCode::SigForged.val());
        }
        // 门 7：状态机单调——落账即 Cosigned（Pending 起点隐含）。
        let st = CosignState::Cosigned;
        if !CosignState::Pending.can_advance(st) {
            return Err(CosignCode::VerdictBack.val());
        }
        self.ledger.records.push(CosignRecord {
            seq,
            entity,
            iface: io as u8,
            issuer: String::from(issuer),
            enqueuer: String::from(enqueuer),
            c1_sig,
            c2_sig,
            state: st.rank(),
        });
        self.payload_ops += 16;
        Ok(seq)
    }

    /// 实体绑定查账：账内是否存在该实体的创建联签。
    pub fn entity_bound(&self, entity: u64) -> bool {
        self.ledger.records.iter().any(|r| r.entity == entity && r.iface == Iface::Create.ord())
    }

    /// 落档：全账 Cosigned→Filed 单调推进。
    pub fn file_all(&mut self) -> Result<usize, u32> {
        if self.ledger.records.is_empty() {
            return Err(CosignCode::LedgerEmpty.val());
        }
        for r in self.ledger.records.iter_mut() {
            let cur = match r.state {
                0 => CosignState::Pending,
                1 => CosignState::Cosigned,
                _ => CosignState::Filed,
            };
            if !cur.can_advance(CosignState::Filed) {
                return Err(CosignCode::VerdictBack.val());
            }
            r.state = CosignState::Filed.rank();
        }
        Ok(self.ledger.records.len())
    }
}

// ── 两条独立渲染路径（对拍双路） ────────────────────────────────
/// 落档路径渲染：format 一次性成型（字段序冻结）。
pub fn render_record(r: &CosignRecord) -> String {
    format!(
        "{{\"seq\":{},\"ent\":{},\"if\":{},\"is\":{},\"en\":{},\"c1\":{:08X},\"c2\":{:08X},\"st\":{}}}\n",
        r.seq, r.entity, r.iface, esc_json(&r.issuer), esc_json(&r.enqueuer), r.c1_sig, r.c2_sig, r.state
    )
}

/// 直读路径渲染：逐字段拼接（与落档路径结构独立，供对拍基准）。
pub fn direct_read_record(r: &CosignRecord) -> String {
    let mut s = format!("{{\"seq\":{}", r.seq);
    s.push_str(",\"ent\":"); s.push_str(&r.entity.to_string());
    s.push_str(",\"if\":"); s.push_str(&r.iface.to_string());
    s.push_str(",\"is\":"); s.push_str(&esc_json(&r.issuer));
    s.push_str(",\"en\":"); s.push_str(&esc_json(&r.enqueuer));
    s.push_str(",\"c1\":"); s.push_str(&format!("{:08X}", r.c1_sig));
    s.push_str(",\"c2\":"); s.push_str(&format!("{:08X}", r.c2_sig));
    s.push_str(",\"st\":"); s.push_str(&r.state.to_string());
    s.push_str("}\n");
    s
}

/// 全账规范正文（落档路径）。
pub fn canonical_body(l: &CosignLedger) -> String {
    let mut s = String::new();
    for r in l.records.iter() {
        s.push_str(&render_record(r));
    }
    s
}

/// 全账直读正文（对拍基准路径）。
pub fn direct_body(l: &CosignLedger) -> String {
    let mut s = String::new();
    for r in l.records.iter() {
        s.push_str(&direct_read_record(r));
    }
    s
}

// ── JSON 字符串转义（落档件的转义闭集） ─────────────────────────
/// 键串转义：引号/反斜杠/换行/回车/制表 + 0x00..0x1F 控制符走 \u00XX。
pub fn esc_json(s: &str) -> String {
    let mut out = String::new();
    out.push('"');
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x00..=0x1F => out.push_str(&format!("\\u{:04x}", b[i])),
            _ => out.push(b[i] as char),
        }
        i += 1;
    }
    out.push('"');
    out
}

// ── 落档件（联签记录落档的载体） ────────────────────────────────
/// 落档头行：@VXC2|epoch=|count=|sum=XXXXXXXX;
pub fn archive_header(epoch: u64, count: u64, sum: u32) -> String {
    format!("@VXC2|epoch={}|count={}|sum={:08X};\n", epoch, count, sum)
}

/// 落档件 = 头行 + 全账规范正文。
pub fn archive_text(l: &CosignLedger) -> String {
    let body = canonical_body(l);
    let mut out = archive_header(l.epoch, l.records.len() as u64, fnv1a(body.as_bytes()));
    out.push_str(&body);
    out
}

/// 解析后的落档头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchHeader {
    pub epoch: u64,
    pub count: u64,
    pub sum: u32,
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    h.iter().skip(from).position(|&c| c == needle).map(|p| p + from)
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    h.windows(pat.len()).skip(from).position(|w| w == pat).map(|p| p + from)
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
        i += 1;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < 8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
        i += 1;
    }
    Some(v)
}

/// 落档头解析：逐 token 扫描，未知字段即破损。
pub fn parse_header(hdr: &str) -> Option<ArchHeader> {
    let mut epoch: Option<u64> = None;
    let mut count: Option<u64> = None;
    let mut sum: Option<u32> = None;
    for tok in hdr.split('|') {
        if tok.is_empty() {
            continue;
        }
        let eq = tok.find('=')?;
        let (k, v) = (&tok[..eq], &tok[eq + 1..]);
        match k {
            "epoch" => epoch = Some(parse_dec(v)?),
            "count" => count = Some(parse_dec(v)?),
            "sum" => sum = Some(parse_hex8(v)?),
            _ => return None,
        }
    }
    Some(ArchHeader { epoch: epoch?, count: count?, sum: sum? })
}

/// 落档件核验：头行解析 + 纪元/计数对账 + 正文逐字节对拍 + 校验和复核
/// + 绑定序/判定域复核（正文里 st 必须 ≥ Cosigned）。
pub fn verify_archive(l: &CosignLedger, out: &str) -> Result<(), u32> {
    let ob = out.as_bytes();
    let drift = || CosignCode::ReplayDrift.val();
    if out.len() < 7 || !out.starts_with("@VXC2|") {
        return Err(drift());
    }
    let semi = match find_byte(ob, 0, b';') {
        Some(p) => p,
        None => return Err(drift()),
    };
    let h = match parse_header(&out[6..semi]) {
        Some(h) => h,
        None => return Err(drift()),
    };
    if h.epoch != l.epoch || h.count != l.records.len() as u64 {
        return Err(drift());
    }
    // 正文从头行换行之后起算（头行行尾 '\n' 不属于正文）。
    let mut start = semi + 1;
    if start < out.len() && ob[start] == b'\n' {
        start += 1;
    }
    let body = &out[start..];
    // 裸控制符扫描（<0x20 且非换行 → 破损）。
    for c in body.as_bytes().iter() {
        if *c < 0x20 && *c != b'\n' {
            return Err(drift());
        }
    }
    if body != canonical_body(l) || h.sum != fnv1a(body.as_bytes()) {
        return Err(drift());
    }
    // 绑定序域复核：唤醒/优先级单出现前同实体创建单必须已在更早行。
    let mut seen_create: Vec<u64> = Vec::new();
    for r in l.records.iter() {
        if r.iface == Iface::Create.ord() {
            seen_create.push(r.entity);
        } else if !seen_create.iter().any(|&e| e == r.entity) {
            return Err(CosignCode::EntityUnbound.val());
        }
        if r.state < CosignState::Cosigned.rank() {
            return Err(CosignCode::VerdictBack.val());
        }
    }
    Ok(())
}

// ── 黄金建账（每实体三接口依次联签 → 全账落档） ────────────────
/// 黄金联签账：12 实体 × 3 接口 = 36 单，全部 Filed。
pub fn build_golden_ledger(epoch: u64) -> Result<CosignLedger, u32> {
    let mut cs = Cosigner::new(epoch);
    let mut e = 1u64;
    while e <= ENTITY_N as u64 {
        // 创建 → 唤醒 → 优先级设置，绑定序硬约束。
        // 章值按 cosign 内部派生序（落账前 len+1）预计算，三接口逐一联签。
        let s1 = (cs.ledger.records.len() + 1) as u64;
        let _ = cs.cosign(e, Iface::Create, ISSUER_C, ENQUEUE_A3,
            c1_seal(e, Iface::Create.ord(), s1), c2_seal(e, Iface::Create.ord(), s1))?;
        let s2 = (cs.ledger.records.len() + 1) as u64;
        let _ = cs.cosign(e, Iface::Wakeup, ISSUER_C, ENQUEUE_A3,
            c1_seal(e, Iface::Wakeup.ord(), s2), c2_seal(e, Iface::Wakeup.ord(), s2))?;
        let s3 = (cs.ledger.records.len() + 1) as u64;
        let _ = cs.cosign(e, Iface::PrioSet, ISSUER_C, ENQUEUE_A3,
            c1_seal(e, Iface::PrioSet.ord(), s3), c2_seal(e, Iface::PrioSet.ord(), s3))?;
        e += 1;
    }
    let n = cs.file_all()?;
    if n != ENTITY_N * IFACE_N {
        return Err(CosignCode::LedgerEmpty.val());
    }
    Ok(cs.ledger)
}

// ── fake 对拍（10 路逐字伪造） ─────────────────────────────────
/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = find_sub(b, 0, pat).map(|p| p + pat.len()).unwrap_or(usize::MAX);
    for (i, c) in b.iter().enumerate() {
        v.push(if i == tgt { if *c == newc { alt } else { newc } } else { *c });
    }
}

/// 头行行尾下标（头行 '\n' 位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) {
        Some(x) => x,
        None => b.len(),
    }
}

/// 对黄金落档件注入第 kind 路伪造，返回坏件（内容全 ASCII，安全）。
pub fn forge(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    match kind % 10 {
        0 => patch_after(b, b"\"c1\":", b'A', b'B', &mut v), // C1 章改写
        1 => patch_after(b, b"\"c2\":", b'A', b'B', &mut v), // C2 章改写
        2 => {
            // 接口换：正文首个 "if": 值改写（0↔1 边界）。
            patch_after(b, b"\"if\":", b'0', b'1', &mut v);
        }
        3 => patch_after(b, b"\"ent\":", b'1', b'2', &mut v), // 实体号改写
        4 => patch_after(b, b"\"st\":", b'0', b'2', &mut v), // 判定倒退注入
        5 => {
            // 未绑定注入：删正文首个创建单行（唤醒单失绑）。
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        6 => patch_after(b, b"\"seq\":", b'9', b'8', &mut v), // 序重号
        7 => patch_after(b, b"\"is\":\"C-DEPT\"", b'C', b'X', &mut v), // 发证方错
        8 => patch_after(b, b"\"en\":\"A3-QUEUE\"", b'A', b'Y', &mut v), // 入队方错
        _ => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 记录截尾
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的伪造路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit { self.caught += 1; } else { self.miss_kinds.push(kind); }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金落档件注入一路伪造，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let l = build_golden_ledger(EPOCH + r as u64)?;
        let out = archive_text(&l);
        verify_archive(&l, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_archive(&l, &forge(&out, kind)).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 对拍抽验（落档路径 vs 直读路径） ────────────────────────────
/// 对拍：两路径全账正文逐字节相等 + 章值复核（全账逐单重算双章）。
pub const FIDELITY_SAMPLES: usize = 100;

pub fn fidelity_100(l: &CosignLedger) -> Result<u32, u32> {
    if l.records.is_empty() {
        return Err(CosignCode::LedgerEmpty.val());
    }
    if canonical_body(l) != direct_body(l) {
        return Err(CosignCode::ReplayDrift.val());
    }
    for k in 0..FIDELITY_SAMPLES {
        let r = match l.records.get(k % l.records.len()) {
            Some(r) => r,
            None => return Err(CosignCode::ReplayDrift.val()),
        };
        // 章值复核 + 判例对表（发证归 C 部 / 入队归 A3）。
        let iface = match r.iface {
            0 => Iface::Create,
            1 => Iface::Wakeup,
            _ => Iface::PrioSet,
        };
        if r.c1_sig != c1_seal(r.entity, r.iface, r.seq) || r.c2_sig != c2_seal(r.entity, r.iface, r.seq) {
            return Err(CosignCode::SigForged.val());
        }
        if !precedent_match(iface, &r.issuer, &r.enqueuer) {
            return Err(CosignCode::PairMismatch.val());
        }
    }
    Ok(FIDELITY_SAMPLES as u32)
}

// ── 复跑一致（fake 对拍复跑红线） ───────────────────────────────
/// 复跑：全账重算（章值/正文/落档件）两次逐字节相等；返回 (单数, 对拍样本)。
pub fn replay_consistency() -> Result<(usize, usize), u32> {
    let a = build_golden_ledger(EPOCH)?;
    let b = build_golden_ledger(EPOCH)?;
    if archive_text(&a) != archive_text(&b) {
        return Err(CosignCode::ReplayDrift.val());
    }
    let (fa, fb) = (fidelity_100(&a)?, fidelity_100(&b)?);
    if fa != fb {
        return Err(CosignCode::ReplayDrift.val());
    }
    Ok((a.records.len(), fa as usize))
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：建账落档 + 核验 + 对拍 + 复跑 + 五拒探。
/// 返回 (正旗数, 拒旗数, 联签单数)。
pub fn golden_run() -> Result<(u32, u32, usize), u32> {
    let l = build_golden_ledger(EPOCH)?;
    let out = archive_text(&l);
    verify_archive(&l, &out)?;
    let fid = fidelity_100(&l)?;
    let (n, _) = replay_consistency()?;
    // 五拒探：伪造章 / 未绑定 / 判例不符 / 空账落档 / 落档件篡改。
    let mut cs = Cosigner::new(EPOCH);
    let forge_rej = cs.cosign(1, Iface::Create, ISSUER_C, ENQUEUE_A3, 0, 0).is_err();
    let unbound_rej = cs.cosign(7, Iface::Wakeup, ISSUER_C, ENQUEUE_A3,
        c1_seal(7, Iface::Wakeup.ord(), 1), c2_seal(7, Iface::Wakeup.ord(), 1)).is_err();
    let pair_rej = cs.cosign(1, Iface::Create, "X-DEPT", ENQUEUE_A3,
        c1_seal(1, Iface::Create.ord(), 2), c2_seal(1, Iface::Create.ord(), 2)).is_err();
    let empty_rej = Cosigner::new(EPOCH).file_all().is_err();
    let tamper_rej = verify_archive(&l, &forge(&out, 0)).is_err();
    let flags = (fid == FIDELITY_SAMPLES as u32) as u32 + (n == ENTITY_N * IFACE_N) as u32
        + verify_archive(&l, &out).is_ok() as u32 + probe_escape() as u32;
    let rejects = forge_rej as u32 + unbound_rej as u32 + pair_rej as u32
        + empty_rej as u32 + tamper_rej as u32;
    Ok((flags, rejects, n))
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CosignCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CosignCode::ALL.len() {
        let v = CosignCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 落档头往返探针（建头→解析→逐域对账） ────────────────────────
pub fn header_roundtrip() -> bool {
    let sum = fnv1a(b"probe");
    let h = archive_header(7, 36, sum);
    let a = match parse_header(&h[6..h.len() - 2]) {
        Some(p) => p.epoch == 7 && p.count == 36 && p.sum == sum,
        None => false,
    };
    // 破损头三拒：未知字段 / 坏数字 / 非 8 位校验和。
    let bad = parse_header("epoch=7|ghost=1|sum=00000000").is_none()
        && parse_header("epoch=x|sum=00000000").is_none()
        && parse_header("epoch=7|sum=00").is_none();
    a && bad
}

// ── 转义探针 ────────────────────────────────────────────────────
/// 转义闭集探针：引号/反斜杠/换行/制表/控制符逐字验证。
pub fn probe_escape() -> bool {
    let q = esc_json("a\"b\\c\nd\te");
    let q2 = esc_json("\u{01}");
    q.starts_with('"') && q.ends_with('"')
        && q.contains("\\\"") && q.contains("\\\\") && q.contains("\\n") && q.contains("\\t")
        && !q.contains('\n') && !q.contains('\t')
        && q2 == String::from("\"\\u0001\"")
        && esc_json("C-DEPT") == String::from("\"C-DEPT\"")
}

// ── 状态机单调守卫探针 ──────────────────────────────────────────
/// 平移/倒退拒 + 判例对表逐条核对 + 接口标签往返。
pub fn probe_monotonic() -> bool {
    let m1 = !CosignState::Pending.can_advance(CosignState::Pending);
    let m2 = !CosignState::Filed.can_advance(CosignState::Pending);
    let m3 = CosignState::Pending.can_advance(CosignState::Filed);
    let m4 = CosignState::Cosigned.can_advance(CosignState::Filed);
    // 判例对表：三接口全过 + 反例拒。
    let p1 = Iface::ALL.iter().all(|f| precedent_match(*f, ISSUER_C, ENQUEUE_A3));
    let p2 = !precedent_match(Iface::Create, "X-DEPT", ENQUEUE_A3)
        && !precedent_match(Iface::Wakeup, ISSUER_C, "B-QUEUE");
    // 接口标签往返。
    let t1 = Iface::from_tag("CREATE") == Some(Iface::Create)
        && Iface::from_tag("WAKEUP") == Some(Iface::Wakeup)
        && Iface::from_tag("PRIOSET") == Some(Iface::PrioSet)
        && Iface::from_tag("GHOST").is_none();
    m1 && m2 && m3 && m4 && p1 && p2 && t1
}

// ── 绑定序守卫探针 ──────────────────────────────────────────────
/// 先创建后唤醒/优先级；未创建一律拒；同实体多创建单按序号区分允许。
pub fn probe_binding_order() -> bool {
    let mut cs = Cosigner::new(EPOCH);
    // 未绑定唤醒拒。
    let r1 = cs.cosign(5, Iface::Wakeup, ISSUER_C, ENQUEUE_A3,
        c1_seal(5, Iface::Wakeup.ord(), 1), c2_seal(5, Iface::Wakeup.ord(), 1)).is_err();
    // 创建落账 → 绑定后唤醒/优先级可落 → 他实体仍未绑定拒。
    let seal_next = |cs: &mut Cosigner, e: u64, f: Iface, ok: bool| -> bool {
        let n = (cs.ledger.records.len() + 1) as u64;
        cs.cosign(e, f, ISSUER_C, ENQUEUE_A3, c1_seal(e, f.ord(), n), c2_seal(e, f.ord(), n)).is_ok() == ok
    };
    let c1 = seal_next(&mut cs, 5, Iface::Create, true);
    let r2 = seal_next(&mut cs, 5, Iface::Wakeup, true);
    let r3 = seal_next(&mut cs, 5, Iface::PrioSet, true);
    let r4 = seal_next(&mut cs, 9, Iface::PrioSet, false);
    r1 && c1 && r2 && r3 && r4
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let l = build_golden_ledger(EPOCH)?;
    let out = archive_text(&l);
    verify_archive(&l, &out)?;
    let fid = fidelity_100(&l)?;
    Ok(format!(
        "状态=已完成 records={} fidelity={} sum={:08X} ifaces=CREATE+WAKEUP+PRIOSET",
        l.records.len(),
        fid,
        fnv1a(canonical_body(&l).as_bytes())
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正五拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, n)) => (*f == 4 && *r == 5, format!("flags={} rejects={} records={}", f, r, n)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-联签核验全链", gp, gd));
    // J1 fake 对拍 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-fake对拍10/10", ip, id));
    // J1 复跑一致。
    let rp = replay_consistency();
    let (rpp, rpd) = match &rp {
        Ok((n, f)) => (*n == ENTITY_N * IFACE_N && *f == FIDELITY_SAMPLES, format!("records={} fid={}", n, f)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-复跑一致", rpp, rpd));
    // J1 联签记录落档：落档件核验过且计数齐。
    let arc = match build_golden_ledger(EPOCH) {
        Ok(l) => {
            let out = archive_text(&l);
            verify_archive(&l, &out).is_ok() && l.records.len() == ENTITY_N * IFACE_N
        }
        Err(_) => false,
    };
    v.push(("J1-联签记录落档", arc, format!("{} 单全 Filed", ENTITY_N * IFACE_N)));
    // J2 判例对表：三接口全过 + 反例拒。
    v.push(("J2-判例对表", probe_monotonic(), String::from("发证归C部/入队归A3 三接口全过+反例拒")));
    // J2 绑定序守卫。
    v.push(("J2-绑定序守卫", probe_binding_order(), String::from("未绑定拒/绑定后放行/他实体仍拒")));
    // J2 章值独立性：C1≠C2 同参异种。
    let seal_pair = c1_seal(3, 1, 7) != c2_seal(3, 1, 7);
    v.push(("J2-双章异种", seal_pair, String::from("C1 种 0xC1C1C1C1 / C2 种 0xA3A3A3A3")));
    // J1 落档头往返 + 破损头拒。
    v.push(("J1-落档头校验", header_roundtrip(), String::from("建头→解析逐域对账；未知字段/坏数字/短校验和必败")));
    // J1 转义闭集。
    v.push(("J1-转义闭集", probe_escape(), String::from("引号/反斜杠/换行/制表/\\u00XX 全落位")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72C40 窗口")));
    v
}

pub fn run_unx2324_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2324 C1/C2 联签件（调度实体绑定核验）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
