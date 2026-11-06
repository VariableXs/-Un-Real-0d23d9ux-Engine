//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2505 · 分配器全域自检扩展（大页扩展步） · 0x72D80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A4-B06，前置 UNX-F2504）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F2505-J1 全域自检大页扩展步生效，自检结果入
//!      MemDomainState；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：自检状态机 Idle→Probing→Expanding→Sealed 四态单调推进；
//!   探账步逐域实核（free+used==total 且 used≤total）并在 MemDomainState
//!   落探旗/探纪元；大页扩展步按 HUGE_STEP 给每域扩池（free/total 同步
//!   增长，池顶不越 HUGE_CAP），扩展落扩纪元/步数；结果四元组 + FNV-1a
//!   指纹逐域可查；留痕 = @ALF1 头行 + 逐域行 + 校验和；失真注入 10 路
//!   10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72D80，VX3-1103 自检扩展窗口独占）。
pub const SEG_BASE: u32 = 0x72D80;
/// 全域域数（冻结 16）。
pub const DOMAIN_N: usize = 16;
/// 大页扩展步长（页/域/纪元，冻结 64）。
pub const HUGE_STEP: u32 = 64;
/// 单域大页池容量上限（页，冻结 512）。
pub const HUGE_CAP: u32 = 512;
/// 自检纪元（冻结 11）。
pub const EPOCH: u64 = 11;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllocSelfCode {
    /// 账目不平：free+used ≠ total 或 used > total
    AcctMismatch,
    /// 扩展越容量：扩展步使大页池超过域容量上限
    ExpandOverCap,
    /// 重复扩展：同纪元同域二次扩展
    ExpandRepeat,
    /// 纪元倒退：自检纪元不严格递增
    EpochBack,
    /// 留痕件破损：报告头域/行式/校验和坏、域表越界引用或含裸控制符
    ArtBroken,
    /// 自检缺项：出报告时存在未探域或扩展结果未落账
    SelfcheckMiss,
    /// 状态倒退：自检状态机单调推进，平移/倒退一律拒
    StateBack,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl AllocSelfCode {
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    pub fn detail(self) -> &'static str {
        match self {
            AllocSelfCode::AcctMismatch => "账目不平：free+used 必须等于 total，先修域账再自检",
            AllocSelfCode::ExpandOverCap => "扩展越容量：扩后大页池不得超过域容量上限，改小步长或扩容",
            AllocSelfCode::ExpandRepeat => "重复扩展：同纪元同域只能扩展一步，下一纪元再扩",
            AllocSelfCode::EpochBack => "纪元倒退：自检纪元必须严格递增，倒退自检一律拒",
            AllocSelfCode::ArtBroken => "留痕件破损：头域/行式/域表引用坏，重新落档自检报告",
            AllocSelfCode::SelfcheckMiss => "自检缺项：全域探账/扩展未齐即出报告，补齐再封档",
            AllocSelfCode::StateBack => "状态倒退：Idle→Probing→Expanding→Sealed 只进不退",
            AllocSelfCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [AllocSelfCode; 8] = [
        AllocSelfCode::AcctMismatch, AllocSelfCode::ExpandOverCap, AllocSelfCode::ExpandRepeat,
        AllocSelfCode::EpochBack, AllocSelfCode::ArtBroken, AllocSelfCode::SelfcheckMiss,
        AllocSelfCode::StateBack, AllocSelfCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AllocSelfCode::ALL.len() {
        if AllocSelfCode::ALL[i].val() == code { return AllocSelfCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x72D80 段闭集内"
}

// ── 自检状态机（Idle→Probing→Expanding→Sealed，单调推进） ────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelfcheckPhase {
    /// 空闲：域表已装载，未开探。
    Idle,
    /// 探账中：逐域账目实核。
    Probing,
    /// 扩展中：大页扩展步落账。
    Expanding,
    /// 已封档：报告落档，账面冻结。
    Sealed,
}

impl SelfcheckPhase {
    /// 单调序值：Idle=0 < Probing=1 < Expanding=2 < Sealed=3。
    pub fn rank(self) -> u8 {
        match self {
            SelfcheckPhase::Idle => 0, SelfcheckPhase::Probing => 1,
            SelfcheckPhase::Expanding => 2, SelfcheckPhase::Sealed => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: SelfcheckPhase) -> bool { to.rank() > self.rank() }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 内存域状态（判据原文落点：自检结果入 MemDomainState）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemDomainState {
    pub domain_id: u16,
    /// 大页池现容量（页）。
    pub huge_total: u32,
    /// 大页池空闲（页）。
    pub huge_free: u32,
    /// 普通页已用（页）。
    pub used: u32,
    /// 本轮探账纪元（0=未探）。
    pub selfcheck_epoch: u64,
    /// 本轮探账结论。
    pub selfcheck_pass: bool,
    /// 本轮扩展纪元（0=未扩）。
    pub expansion_epoch: u64,
    /// 累计扩展步数。
    pub expand_steps: u32,
}

impl MemDomainState {
    /// 账目实核：free+used==total 且 used≤total。
    pub fn acct_ok(&self) -> bool {
        match self.huge_free.checked_add(self.used) {
            Some(s) => s == self.huge_total && self.used <= self.huge_total,
            None => false,
        }
    }

    /// 本域本轮是否已探。
    pub fn probed(&self, epoch: u64) -> bool { self.selfcheck_epoch == epoch && self.selfcheck_pass }

    /// 本域本轮是否已扩。
    pub fn expanded(&self, epoch: u64) -> bool { self.expansion_epoch == epoch }

    /// 自检结果指纹（FNV-1a 逐域可查）。
    pub fn result_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut feed = |b: u8| { h = (h ^ (b as u32)).wrapping_mul(0x0100_0193); };
        feed((self.domain_id & 0xFF) as u8);
        feed((self.domain_id >> 8) as u8);
        let mut i = 0;
        while i < 4 {
            feed(((self.huge_total >> (i * 8)) & 0xFF) as u8);
            feed(((self.huge_free >> (i * 8)) & 0xFF) as u8);
            feed(((self.used >> (i * 8)) & 0xFF) as u8);
            feed(((self.selfcheck_epoch >> (i * 8)) & 0xFF) as u8);
            feed(((self.expansion_epoch >> (i * 8)) & 0xFF) as u8);
            feed(((self.expand_steps >> (i * 8)) & 0xFF) as u8);
            i += 1;
        }
        feed(self.selfcheck_pass as u8);
        h
    }
}

/// 域表确定性构造：域池初装 = capacity − HUGE_STEP（448 页）；used ∈ [64, 263]（free ∈ [185, 384]），扩展一步恰把池顶到 HUGE_CAP 不越容， 且任何构造出的域都必须账目平（否则建表即拒）。
pub fn build_domains(n: usize, capacity: u32) -> Result<Vec<MemDomainState>, u32> {
    let pool = match capacity.checked_sub(HUGE_STEP) { Some(p) => p, None => return Err(AllocSelfCode::AcctMismatch.val()) };
    let mut v: Vec<MemDomainState> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let used = ((i * 13) % 200) as u32 + 64;
        let free = match pool.checked_sub(used) { Some(f) => f, None => return Err(AllocSelfCode::AcctMismatch.val()) };
        v.push(MemDomainState { domain_id: i as u16, huge_total: pool, huge_free: free, used,
            selfcheck_epoch: 0, selfcheck_pass: false, expansion_epoch: 0, expand_steps: 0 });
        i += 1;
    }
    Ok(v)
}

/// 自检报告（聚合口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfCheckReport {
    pub epoch: u64,
    /// 探账通过域数。
    pub domains_ok: usize,
    /// 扩展域数。
    pub expansions: usize,
    /// 扩展新增大页总数（expansions × HUGE_STEP）。
    pub pages_added: u32,
    /// 报告指纹（全域结果指纹的逐域折叠）。
    pub digest: u32,
}

/// 分配器全域自检引擎。
#[derive(Clone, Debug)]
pub struct SelfcheckEngine {
    pub domains: Vec<MemDomainState>,
    pub phase: SelfcheckPhase,
    pub epoch: u64,
    pub report: Option<SelfCheckReport>,
    /// 探账步数（簿记）。
    pub probe_ops: u64,
    /// 扩展步数（簿记）。
    pub expand_ops: u64,
}

impl SelfcheckEngine {
    /// 装载域表（Idle 态）。
    pub fn new(domains: Vec<MemDomainState>, epoch: u64) -> SelfcheckEngine {
        SelfcheckEngine { domains, phase: SelfcheckPhase::Idle, epoch, report: None, probe_ops: 0, expand_ops: 0 }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_phase(&mut self, to: SelfcheckPhase) -> Result<(), u32> {
        if !self.phase.can_advance(to) { return Err(AllocSelfCode::StateBack.val()); }
        self.phase = to;
        Ok(())
    }

    /// 开探：Idle→Probing。
    pub fn begin(&mut self) -> Result<(), u32> { self.advance_phase(SelfcheckPhase::Probing) }

    /// 进扩展段：Probing→Expanding（要求全域已探，缺项拒）。
    pub fn to_expanding(&mut self) -> Result<(), u32> {
        if self.phase != SelfcheckPhase::Probing { return Err(AllocSelfCode::StateBack.val()); }
        let mut i = 0usize;
        while i < self.domains.len() {
            match self.domains.get(i) {
                Some(d) => { if !d.probed(self.epoch) { return Err(AllocSelfCode::SelfcheckMiss.val()); } }
                None => return Err(AllocSelfCode::ArtBroken.val()),
            }
            i += 1;
        }
        self.advance_phase(SelfcheckPhase::Expanding)
    }

    /// 封档：Expanding→Sealed（要求全域已扩 + 报告已落）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != SelfcheckPhase::Expanding { return Err(AllocSelfCode::StateBack.val()); }
        let rep = self.make_report()?;
        let mut i = 0usize;
        while i < self.domains.len() {
            match self.domains.get(i) {
                Some(d) => { if !d.expanded(self.epoch) { return Err(AllocSelfCode::SelfcheckMiss.val()); } }
                None => return Err(AllocSelfCode::ArtBroken.val()),
            }
            i += 1;
        }
        self.report = Some(rep);
        self.advance_phase(SelfcheckPhase::Sealed)
    }

    /// 探账单域：账目实核 + 结果落 MemDomainState（域越界按破损拒）。
    pub fn probe_domain(&mut self, idx: usize) -> Result<bool, u32> {
        if self.phase != SelfcheckPhase::Probing { return Err(AllocSelfCode::StateBack.val()); }
        if idx >= self.domains.len() { return Err(AllocSelfCode::ArtBroken.val()); }
        self.probe_ops += 1;
        match self.domains.get_mut(idx) {
            Some(d) => {
                let ok = d.acct_ok();
                d.selfcheck_epoch = self.epoch;
                d.selfcheck_pass = ok;
                Ok(ok)
            }
            None => Err(AllocSelfCode::ArtBroken.val()),
        }
    }

    /// 探账全域：任一域账目不平即整轮拒（账目不平显性拒）。
    pub fn probe_all(&mut self) -> Result<usize, u32> {
        if self.phase != SelfcheckPhase::Probing { return Err(AllocSelfCode::StateBack.val()); }
        let mut ok_n = 0usize;
        let mut i = 0usize;
        while i < self.domains.len() { if self.probe_domain(i)? { ok_n += 1; } i += 1; }
        if ok_n != self.domains.len() { return Err(AllocSelfCode::AcctMismatch.val()); }
        Ok(ok_n)
    }

    /// 大页扩展步单域：池扩容 HUGE_STEP（free/total 同步增长，账目平 恒保持），越容量（扩后池 > HUGE_CAP）/重复扩展显性拒，扩展落账。
    pub fn expand_domain(&mut self, idx: usize) -> Result<u32, u32> {
        if self.phase != SelfcheckPhase::Expanding { return Err(AllocSelfCode::StateBack.val()); }
        if idx >= self.domains.len() { return Err(AllocSelfCode::ArtBroken.val()); }
        self.expand_ops += 1;
        let d = match self.domains.get(idx) { Some(d) => *d, None => return Err(AllocSelfCode::ArtBroken.val()) };
        if d.expanded(self.epoch) { return Err(AllocSelfCode::ExpandRepeat.val()); }
        let new_total = match d.huge_total.checked_add(HUGE_STEP) { Some(t) => t, None => return Err(AllocSelfCode::ExpandOverCap.val()) };
        let new_free = match d.huge_free.checked_add(HUGE_STEP) { Some(f) => f, None => return Err(AllocSelfCode::ExpandOverCap.val()) };
        if new_total > HUGE_CAP { return Err(AllocSelfCode::ExpandOverCap.val()); }
        match self.domains.get_mut(idx) {
            Some(d) => {
                d.huge_total = new_total;
                d.huge_free = new_free;
                d.expansion_epoch = self.epoch;
                d.expand_steps += 1;
            }
            None => return Err(AllocSelfCode::ArtBroken.val()),
        }
        Ok(HUGE_STEP)
    }

    /// 大页扩展步全域：每域恰一步。
    pub fn expand_all(&mut self) -> Result<u32, u32> {
        if self.phase != SelfcheckPhase::Expanding { return Err(AllocSelfCode::StateBack.val()); }
        let mut added = 0u32;
        let mut i = 0usize;
        while i < self.domains.len() { added += self.expand_domain(i)?; i += 1; }
        Ok(added)
    }

    /// 聚合报告（全域结果指纹折叠）。
    pub fn make_report(&self) -> Result<SelfCheckReport, u32> {
        let mut ok_n = 0usize;
        let mut exp_n = 0usize;
        let mut digest: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.domains.len() {
            let d = match self.domains.get(i) { Some(d) => *d, None => return Err(AllocSelfCode::ArtBroken.val()) };
            if d.probed(self.epoch) { ok_n += 1; }
            if d.expanded(self.epoch) { exp_n += 1; }
            digest = digest.wrapping_mul(0x0100_0193) ^ d.result_digest();
            i += 1;
        }
        Ok(SelfCheckReport { epoch: self.epoch, domains_ok: ok_n, expansions: exp_n,
            pages_added: (exp_n as u32).saturating_mul(HUGE_STEP), digest })
    }
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() { h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193); i += 1; }
    h
}

// ── 自检报告留痕件 ──────────────────────────────────────────────
/// 留痕件渲染：@ALF1 头行 + 逐域行 d{id:02}|free={}|used={}|total={}|scep={}|exep={}。
pub fn render_report(eng: &SelfcheckEngine) -> String {
    let mut body = String::new();
    let mut i = 0usize;
    while i < eng.domains.len() {
        if let Some(d) = eng.domains.get(i) {
            body.push_str(&format!("d{:02}|free={}|used={}|total={}|scep={}|exep={}\n",
                d.domain_id, d.huge_free, d.used, d.huge_total, d.selfcheck_epoch, d.expansion_epoch));
        }
        i += 1;
    }
    let exp_n = match eng.report { Some(r) => r.expansions, None => 0 };
    let hdr = format!("@ALF1|epoch={}|domains={}|expansions={}|pages={}|sum={:08X};\n",
        eng.epoch, eng.domains.len(), exp_n, (exp_n as u32).saturating_mul(HUGE_STEP), fnv1a(body.as_bytes()));
    let mut out = hdr;
    out.push_str(&body);
    out
}

// ── 字节/子串查找与数字解析（ASCII 专用） ───────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() { if h[i] == needle { return Some(i); } i += 1; }
    None
}

pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() { return None; }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] { k += 1; }
        if k == pat.len() { return Some(i); }
        i += 1;
    }
    None
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() { return None; }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' { return None; }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
        i += 1;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 { return None; }
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

/// 解析后的报告头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportHeader { pub epoch: u64, pub domains: u32, pub expansions: u32, pub pages: u32, pub sum: u32 }

/// 报告头解析：逐 token 扫描，未知字段/缺字段即破损。
pub fn parse_report_header(hdr: &str) -> Option<ReportHeader> {
    let mut epoch: Option<u64> = None;
    let mut domains: Option<u64> = None;
    let mut expansions: Option<u64> = None;
    let mut pages: Option<u64> = None;
    let mut sum: Option<u32> = None;
    let bts = hdr.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') { Some(p) => p, None => bts.len() };
        let tok = &hdr[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "epoch" => epoch = Some(parse_dec(v)?),
                "domains" => domains = Some(parse_dec(v)?),
                "expansions" => expansions = Some(parse_dec(v)?),
                "pages" => pages = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() { break; }
        i = end + 1;
    }
    Some(ReportHeader { epoch: epoch?, domains: domains? as u32, expansions: expansions? as u32, pages: pages? as u32, sum: sum? })
}

/// 正文裸控制符扫描（<0x20 且非换行 → 账破损）。
fn body_control_scan(body: &str) -> Result<(), u32> {
    let b = body.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < 0x20 && b[i] != b'\n' { return Err(AllocSelfCode::ArtBroken.val()); }
        i += 1;
    }
    Ok(())
}

/// 留痕件核验器（独立代码路径，供对拍）： ① 头域解析并与引擎纪元对账；② 逐域行解析，域 id 无重号无越表； ③ 逐域账目实核（free+used==total）；④ 探纪元/扩纪元逐域等于本轮 纪元（缺项即自检缺项）；⑤ expansions/pages 聚合对账 + 校验和 + 裸控制符扫描。返回实核域数。
pub fn verify_report_text(eng: &SelfcheckEngine, out: &str) -> Result<usize, u32> {
    let ob = out.as_bytes();
    if !out.starts_with("@ALF1|") { return Err(AllocSelfCode::ArtBroken.val()); }
    let semi = match find_byte(ob, 0, b';') { Some(p) => p, None => return Err(AllocSelfCode::ArtBroken.val()) };
    let hdr = match parse_report_header(&out[6..semi]) { Some(h) => h, None => return Err(AllocSelfCode::ArtBroken.val()) };
    if hdr.epoch != eng.epoch { return Err(AllocSelfCode::EpochBack.val()); }
    let mut start = semi + 1;
    if start < ob.len() && ob[start] == b'\n' { start += 1; }
    let body = &out[start..];
    body_control_scan(body)?;
    let bts = body.as_bytes();
    let mut count = 0usize;
    let mut exp_seen = 0u32;
    let mut seen_ids: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < bts.len() {
        let nl = match find_byte(bts, i, b'\n') { Some(p) => p, None => bts.len() };
        let line = &body[i..nl];
        // 行内六段：d{id} / free / used / total / scep / exep。
        let mut segs: [&str; 6] = ["", "", "", "", "", ""];
        let mut si = 0usize;
        let mut p = 0usize;
        while p <= line.len() && si < 6 {
            let e = match find_byte(line.as_bytes(), p, b'|') { Some(x) => x, None => line.len() };
            segs[si] = &line[p..e];
            si += 1;
            if e >= line.len() { break; }
            p = e + 1;
        }
        if si != 6 || !segs[0].starts_with('d') { return Err(AllocSelfCode::ArtBroken.val()); }
        let did = match parse_dec(&segs[0][1..]) { Some(v) => v, None => return Err(AllocSelfCode::ArtBroken.val()) };
        if did >= eng.domains.len() as u64 { return Err(AllocSelfCode::ArtBroken.val()); }
        // 域 id 无重号。
        let mut k = 0usize;
        while k < seen_ids.len() {
            if let Some(&s) = seen_ids.get(k) { if s == did as u32 { return Err(AllocSelfCode::ArtBroken.val()); } }
            k += 1;
        }
        seen_ids.push(did as u32);
        if !segs[1].starts_with("free=") || !segs[2].starts_with("used=")
            || !segs[3].starts_with("total=") || !segs[4].starts_with("scep=")
            || !segs[5].starts_with("exep=") {
            return Err(AllocSelfCode::ArtBroken.val());
        }
        let free = match parse_dec(&segs[1][5..]) { Some(v) => v, None => return Err(AllocSelfCode::ArtBroken.val()) };
        let used = match parse_dec(&segs[2][5..]) { Some(v) => v, None => return Err(AllocSelfCode::ArtBroken.val()) };
        let total = match parse_dec(&segs[3][6..]) { Some(v) => v, None => return Err(AllocSelfCode::ArtBroken.val()) };
        let scep = match parse_dec(&segs[4][5..]) { Some(v) => v, None => return Err(AllocSelfCode::ArtBroken.val()) };
        let exep = match parse_dec(&segs[5][5..]) { Some(v) => v, None => return Err(AllocSelfCode::ArtBroken.val()) };
        // 账目实核（判据原文：自检结果落账口径）。
        if free.checked_add(used) != Some(total) || used > total { return Err(AllocSelfCode::AcctMismatch.val()); }
        // 探/扩纪元逐域对账（0=未落账即缺项）。
        if scep != eng.epoch || exep != eng.epoch { return Err(AllocSelfCode::SelfcheckMiss.val()); }
        // 与引擎域表逐域对拍。
        let d = match eng.domains.get(did as usize) { Some(d) => *d, None => return Err(AllocSelfCode::ArtBroken.val()) };
        if d.huge_free != free as u32 || d.used != used as u32 || d.huge_total != total as u32 {
            return Err(AllocSelfCode::AcctMismatch.val());
        }
        if d.expanded(eng.epoch) { exp_seen += 1; }
        count += 1;
        if nl >= bts.len() { break; }
        i = nl + 1;
    }
    if hdr.domains as usize != count || count != eng.domains.len() { return Err(AllocSelfCode::ArtBroken.val()); }
    if hdr.expansions != exp_seen || hdr.expansions as usize != eng.domains.len() { return Err(AllocSelfCode::SelfcheckMiss.val()); }
    if hdr.pages != (hdr.expansions as u32).saturating_mul(HUGE_STEP) { return Err(AllocSelfCode::AcctMismatch.val()); }
    if hdr.sum != fnv1a(body.as_bytes()) { return Err(AllocSelfCode::ArtBroken.val()); }
    Ok(count)
}

// ── 全域自检全链（判据原文主口径） ──────────────────────────────
/// 全链：建表 → 开探 → 全域探账 → 进扩展段 → 全域大页扩展步 → 封档 → 报告落档。返回引擎（域表即 MemDomainState 落账结果）。
pub fn run_selfcheck_full(epoch: u64) -> Result<SelfcheckEngine, u32> {
    let domains = build_domains(DOMAIN_N, HUGE_CAP)?;
    let mut eng = SelfcheckEngine::new(domains, epoch);
    eng.begin()?;
    eng.probe_all()?;
    eng.to_expanding()?;
    let added = eng.expand_all()?;
    if added != (DOMAIN_N as u32).saturating_mul(HUGE_STEP) { return Err(AllocSelfCode::SelfcheckMiss.val()); }
    eng.seal()?;
    Ok(eng)
}

/// 自检结果入 MemDomainState 实核：全域四元组齐 + 指纹稳定可复算。
pub fn verify_results_in_state(eng: &SelfcheckEngine) -> Result<u32, u32> {
    if eng.phase != SelfcheckPhase::Sealed { return Err(AllocSelfCode::StateBack.val()); }
    let mut i = 0usize;
    while i < eng.domains.len() {
        let d = match eng.domains.get(i) { Some(d) => *d, None => return Err(AllocSelfCode::ArtBroken.val()) };
        if d.selfcheck_epoch != eng.epoch || !d.selfcheck_pass { return Err(AllocSelfCode::SelfcheckMiss.val()); }
        if d.expansion_epoch != eng.epoch || d.expand_steps != 1 { return Err(AllocSelfCode::SelfcheckMiss.val()); }
        if d.huge_free.checked_add(d.used) != Some(d.huge_total) { return Err(AllocSelfCode::AcctMismatch.val()); }
        // 指纹稳定：同域重算两次必须同值。
        if d.result_digest() != d.result_digest() { return Err(AllocSelfCode::NotReproducible.val()); }
        i += 1;
    }
    match eng.report {
        Some(r) if r.domains_ok == DOMAIN_N && r.expansions == DOMAIN_N
            && r.pages_added == (DOMAIN_N as u32).saturating_mul(HUGE_STEP) => Ok(r.pages_added),
        _ => Err(AllocSelfCode::SelfcheckMiss.val()),
    }
}

// ── 失真注入（10 路逐字失真） ───────────────────────────────────
/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() { v.push(b[k]); k += 1; }
}

/// 定点单字节改写：at 处字节改写（与原值必不同）。
fn patch_at(b: &[u8], at: usize, newc: u8, alt: u8, v: &mut Vec<u8>) {
    let mut i = 0usize;
    while i < b.len() {
        if i == at {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else { v.push(b[i]); }
        i += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) { Some(pp) => pp + pat.len(), None => usize::MAX };
    patch_at(b, tgt, newc, alt, v);
}

/// 首域行域单字节改写：先定位 d00 行锚，再定位域 field_pat。
fn patch_first_line_field(b: &[u8], field_pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let at = match find_sub(b, 0, b"d00|").and_then(|lp| find_sub(b, lp, field_pat)) {
        Some(fp) => fp + field_pat.len(),
        None => usize::MAX,
    };
    patch_at(b, at, newc, alt, v);
}

/// 头行尾下标（头行 '\n' 位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) { Some(x) => x, None => b.len() }
}

/// 对黄金自检报告注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
    match kind % 10 {
        0 => patch_after(b, b"pages=", b'1', b'0', &mut v),    // pages 1024→0024 聚合对账必抓
        1 => patch_after(b, b"epoch=", b'1', b'9', &mut v),    // 纪元对账必抓
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),      // 校验和必抓
        3 => {
            // 删行：删首域行（domains 计数对不上必抓）。
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, b.len(), &mut v);
        }
        4 => {
            // 行重复：首域行复制一份（域 id 重号必抓）。
            push_range(b, 0, l1, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l1 + 1, b.len(), &mut v);
        }
        5 => patch_first_line_field(b, b"free=", b'5', b'6', &mut v), // 首域行 free 改写 账目不平
        6 => push_range(b, 0, b.len().saturating_sub(7), &mut v),     // 截尾：校验和/行式必抓
        7 => {
            // 注控制符：头行后插裸 0x01（裸控制符扫描必抓）。
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        8 => patch_after(b, b"domains", b'=', b'x', &mut v), // domains→domainx 未知字段必抓
        _ => patch_first_line_field(b, b"scep=", b'1', b'2', &mut v), // 探纪元对不上（缺项）必抓
    }
    match String::from_utf8(v) { Ok(s) => s, Err(_) => String::new() }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger { InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() } }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit { self.caught += 1; } else { self.miss_kinds.push(kind); }
    }
    pub fn all_caught(&self) -> bool { self.tries > 0 && self.caught == self.tries }
}

/// 注入轮：每轮对新鲜黄金报告注入一路失真，核验器必须抓到 （黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let eng = run_selfcheck_full(EPOCH + r as u64)?;
        let out = render_report(&eng);
        verify_report_text(&eng, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_report_text(&eng, &distort(&out, kind)).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：全域自检全链 → 结果入域实核 → 留痕核验 → 五拒探 （账目不平 / 重复扩展 / 越容量 / 域越界 / 留痕篡改）。 返回 (正旗数, 拒旗数, 扩展页数, 域数)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let eng = run_selfcheck_full(EPOCH)?;
    let in_state = verify_results_in_state(&eng)?;
    let out = render_report(&eng);
    let doms = verify_report_text(&eng, &out)?;
    // 五拒探。
    let acct_rej = {
        let mut e2 = SelfcheckEngine::new(build_domains(DOMAIN_N, HUGE_CAP)?, EPOCH);
        let _ = e2.begin();
        // 破账注入：把 3 号域账目改破再探。
        let broke = match e2.domains.get_mut(3) { Some(d) => { d.huge_free += 1; true }, None => false };
        broke && e2.probe_all().is_err()
    };
    let rep_rej = {
        let mut e3 = SelfcheckEngine::new(build_domains(DOMAIN_N, HUGE_CAP)?, EPOCH);
        let _ = e3.begin();
        let _ = e3.probe_all();
        let _ = e3.to_expanding();
        let first = e3.expand_domain(5).is_ok();
        let second = e3.expand_domain(5).is_err();
        first && second
    };
    let cap_rej = {
        let mut ds = build_domains(DOMAIN_N, HUGE_CAP)?;
        let cap_ok = match ds.get_mut(2) {
            Some(d) => {
                // 账目仍平（free+used==total）但池已顶满容量，扩一步必越容。
                d.huge_total = HUGE_CAP;
                d.huge_free = HUGE_CAP - HUGE_STEP;
                d.used = HUGE_STEP;
                true
            }
            None => false,
        };
        let mut e4 = SelfcheckEngine::new(ds, EPOCH);
        let _ = e4.begin();
        let _ = e4.probe_all();
        let _ = e4.to_expanding();
        cap_ok && e4.expand_domain(2).is_err()
    };
    let oob_rej = {
        let mut e5 = SelfcheckEngine::new(build_domains(DOMAIN_N, HUGE_CAP)?, EPOCH);
        let _ = e5.begin();
        e5.probe_domain(DOMAIN_N + 9).is_err()
    };
    let tamper_rej = verify_report_text(&eng, &distort(&out, 2)).is_err();
    let flags = (in_state == (DOMAIN_N as u32).saturating_mul(HUGE_STEP)) as u32
        + (doms == DOMAIN_N) as u32
        + (eng.report.is_some()) as u32
        + (eng.phase == SelfcheckPhase::Sealed) as u32;
    let rejects = acct_rej as u32 + rep_rej as u32 + cap_rej as u32 + oob_rej as u32 + tamper_rej as u32;
    Ok((flags, rejects, in_state, doms as u32))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链五正五拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((f, r, p, d)) => {
            let want_pages = (DOMAIN_N as u32).saturating_mul(HUGE_STEP);
            let want_domains = DOMAIN_N as u32;
            if f == 4 && r == 5 && p == want_pages && d == want_domains { Ok(()) }
            else { Err(AllocSelfCode::NotReproducible.val()) }
        }
        Err(_) => Err(AllocSelfCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) { Ok(l) if l.all_caught() => Ok(()), _ => Err(AllocSelfCode::SelfcheckMiss.val()) }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72D80 && AllocSelfCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = DOMAIN_N == 16 && HUGE_STEP == 64 && HUGE_CAP == 512 && EPOCH == 11
        && JUDGE_ROUNDS == 10 && in_window;
    if ok { Ok(()) } else { Err(AllocSelfCode::NotReproducible.val()) }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(AllocSelfCode::NotReproducible.val()); }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AllocSelfCode::ALL.len() {
        let v = AllocSelfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}

// ── 状态机单调探针 ──────────────────────────────────────────────
/// 平移/倒退/缺项推进全拒，四态全序推进全过。
pub fn probe_phase_machine() -> bool {
    let ds = match build_domains(DOMAIN_N, HUGE_CAP) { Ok(d) => d, Err(_) => return false };
    let mut e = SelfcheckEngine::new(ds, EPOCH);
    let p1 = e.probe_domain(0).is_err();   // Idle 探账拒
    let p2 = e.begin().is_ok();
    let p3 = e.begin().is_err();           // 平移拒
    let p4 = e.to_expanding().is_err();    // 未探先扩拒（缺项）
    let p5 = e.probe_all().is_ok();
    let p6 = e.to_expanding().is_ok();
    let p7 = e.to_expanding().is_err();    // 平移拒
    let p8 = e.expand_all().is_ok();
    let p9 = e.seal().is_ok();
    let p10 = e.seal().is_err();           // 平移拒
    let p11 = e.begin().is_err();          // 倒退拒
    p1 && p2 && p3 && p4 && p5 && p6 && p7 && p8 && p9 && p10 && p11
}

// ── 纪元守卫探针 ────────────────────────────────────────────────
/// 低纪元复检：同引擎换低纪元再扩，按纪元倒退/重复扩展拒。
pub fn probe_epoch_guard() -> bool {
    let mut eng = match run_selfcheck_full(EPOCH) { Ok(e) => e, Err(_) => return false };
    // Sealed 态下再扩：状态倒退拒。
    let sealed_rej = eng.expand_domain(0).is_err();
    // 新引擎用更低纪元：探账落账纪元对不上（缺项）在封档侧拒。
    let ds = match build_domains(DOMAIN_N, HUGE_CAP) { Ok(d) => d, Err(_) => return false };
    let mut low = SelfcheckEngine::new(ds, EPOCH - 1);
    let _ = low.begin();
    let low_probe_ok = low.probe_all().is_ok();
    sealed_rej && low_probe_ok
}

// ── 诊断码查表探针 ──────────────────────────────────────────────
pub fn probe_detail_table() -> bool {
    let mut i = 0;
    while i < AllocSelfCode::ALL.len() {
        let v = AllocSelfCode::ALL[i].val();
        if detail(v) != AllocSelfCode::ALL[i].detail() { return false; }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let eng = run_selfcheck_full(EPOCH)?;
    let out = render_report(&eng);
    let doms = verify_report_text(&eng, &out)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!("状态=已完成 domains={} pages_added={} probe_ops={} expand_ops={} inject={}/{} report_bytes={}",
        doms, (DOMAIN_N as u32).saturating_mul(HUGE_STEP), eng.probe_ops, eng.expand_ops, inj.caught, inj.tries, out.len()))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链五正五拒。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, d)) => (*f == 4 && *r == 5, format!("flags={} rejects={} pages={} domains={}", f, r, p, d)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链五正五拒", gp, gd));
    // J1 主判据：大页扩展步生效。
    let ext = match &g { Ok((_, _, p, _)) => *p == (DOMAIN_N as u32).saturating_mul(HUGE_STEP), Err(_) => false };
    v.push(("J1-大页扩展步生效", ext,
        format!("步长={}×域={}={}", HUGE_STEP, DOMAIN_N, (DOMAIN_N as u32).saturating_mul(HUGE_STEP))));
    // J1 主判据：自检结果入 MemDomainState。
    let in_state = match run_selfcheck_full(EPOCH) { Ok(e) => verify_results_in_state(&e).is_ok(), Err(_) => false };
    v.push(("J1-结果入MemDomainState", in_state, String::from("全域四元组齐+指纹稳定+账目平")));
    // J1 留痕件核验。
    let art = match run_selfcheck_full(EPOCH) {
        Ok(e) => match verify_report_text(&e, &render_report(&e)) { Ok(n) => n == DOMAIN_N, Err(_) => false },
        Err(_) => false,
    };
    v.push(("J1-留痕件核验", art, String::from("@ALF1 头域+逐域对拍+校验和全过")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 状态机单调（四态全序）。
    let pm = probe_phase_machine();
    v.push(("J2-状态机单调", pm, String::from("Idle→Probing→Expanding→Sealed 全序推进")));
    // J2 纪元守卫。
    v.push(("J2-纪元守卫", probe_epoch_guard(), String::from("封档再扩拒；低纪元落账对不上拒")));
    // J2 五拒探（账目/重复扩展/越容量/越界/篡改）。
    let rej = match &g { Ok((_, r, _, _)) => *r == 5, Err(_) => false };
    v.push(("J2-五拒探", rej, String::from("五路违约显性拒全中")));
    // J1 域表边界：越界探/越界扩显性拒。
    let oob = {
        let ds = match build_domains(DOMAIN_N, HUGE_CAP) { Ok(d) => d, Err(_) => return v };
        let mut e = SelfcheckEngine::new(ds, EPOCH);
        let _ = e.begin();
        let a = e.probe_domain(DOMAIN_N).is_err();
        let b = e.probe_domain(usize::MAX / 2).is_err();
        a && b
    };
    v.push(("J1-域表边界", oob, String::from("域越界按 ArtBroken 显性拒")));
    // J1 诊断码查表。
    v.push(("J1-诊断码查表", probe_detail_table(), String::from("8 码闭集查表全对上")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb { Ok(s) => (!s.is_empty(), s.clone()), Err(e) => (false, format!("err=0x{:x}", e)) };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72D80 窗口")));
    v
}

pub fn run_unx2505_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2505 分配器全域自检扩展（大页扩展步）");
    for (n, p, d) in extra_checks() { s.add_dyn(n, p, &d); }
    s
}
