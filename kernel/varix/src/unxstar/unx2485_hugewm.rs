//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2485 · 大页池水位与告警 · 0x72D40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A4-B05，前置 UNX-F2484）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F2485-J1 池低水告警 10/10 次触发零误报（滞回在档）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：池低水 = 已用页 used 上穿高水位线 hi，解除 = 下穿 lo，带内
//!   [lo, hi) 冻结前态（滞回）；监视器生命周期 Idle→Armed→Sealed 单调
//!   推进；告警电平 Normal/Active 按滞回规则合法翻转，翻转逐笔落账
//!（seq/纪元/带值在档）；留痕 = @HWM1 头行 + 逐迁移行 + FNV-1a 校验
//!   和；失真注入 10 路 10/10 必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72D40，VX3-1103 大页水位窗口独占）。
pub const SEG_BASE: u32 = 0x72D40;
/// 大页池总量（页，冻结 4096）。
pub const POOL_TOTAL: u32 = 4096;
/// 高水位线（已用页，上穿即池低水告警）。
pub const HI_WATER: u32 = 3480;
/// 低水位线（已用页，下穿即解除告警）。
pub const LO_WATER: u32 = 3264;
/// 滞回带宽（hi - lo，冻结 216）。
pub const BAND: u32 = HI_WATER - LO_WATER;
/// 账本纪元（冻结 9）。
pub const EPOCH: u64 = 9;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HugeWmCode {
    /// 水位越顶：喂水的 used 超过池总量
    WaterAboveTotal,
    /// 时序倒退：喂水纪元不严格递增
    EpochBack,
    /// 滞回抖动：带内翻转或账面连续同向迁移
    HysteresisFlip,
    /// 误报：激活/解除与水位越限事实不符
    FalseAlarm,
    /// 告警账破损：留痕件头域/行式/校验和坏或含裸控制符
    LogBroken,
    /// 封档守卫：封档后继续喂水一律拒
    SealGuard,
    /// 配置破损：lo≥hi 或水位线越池
    ConfigBroken,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl HugeWmCode {
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    pub fn detail(self) -> &'static str {
        match self {
            HugeWmCode::WaterAboveTotal => "水位越顶：used 不得超过池总量，先核对池账再喂水",
            HugeWmCode::EpochBack => "时序倒退：喂水纪元必须严格递增，倒退喂水一律拒",
            HugeWmCode::HysteresisFlip => "滞回抖动：带内 [lo, hi) 冻结前态，带内翻转即违约",
            HugeWmCode::FalseAlarm => "误报：激活须 used≥hi、解除须 used≤lo，与事实不符按误报记账",
            HugeWmCode::LogBroken => "告警账破损：头域/行式/校验和坏，重新落档告警账",
            HugeWmCode::SealGuard => "封档守卫：监视器已封档，继续观测请开新监视器",
            HugeWmCode::ConfigBroken => "配置破损：须 0≤lo<hi≤池总量，重配水位线",
            HugeWmCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [HugeWmCode; 8] = [
        HugeWmCode::WaterAboveTotal, HugeWmCode::EpochBack, HugeWmCode::HysteresisFlip,
        HugeWmCode::FalseAlarm, HugeWmCode::LogBroken, HugeWmCode::SealGuard,
        HugeWmCode::ConfigBroken, HugeWmCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HugeWmCode::ALL.len() {
        if HugeWmCode::ALL[i].val() == code { return HugeWmCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x72D40 段闭集内"
}

// ── 告警电平（滞回双态） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmLevel {
    /// 正常：池低水未成立。
    Normal,
    /// 告警激活：池低水成立，已上穿高水位线。
    Active,
}

// ── 监视器生命周期（Idle→Armed→Sealed，单调推进） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonitorState {
    /// 空闲：未布防。
    Idle,
    /// 布防：接受喂水、按滞回规则记账。
    Armed,
    /// 已封档：喂水一律拒。
    Sealed,
}

impl MonitorState {
    /// 单调序值：Idle=0 < Armed=1 < Sealed=2。
    pub fn rank(self) -> u8 {
        match self { MonitorState::Idle => 0, MonitorState::Armed => 1, MonitorState::Sealed => 2 }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: MonitorState) -> bool { to.rank() > self.rank() }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 告警迁移记录：电平翻转一笔一账（seq 单调、纪元单调、带值在档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmTransition {
    pub seq: u32,
    pub epoch: u64,
    /// 迁移时刻的已用页数（激活须 ≥hi，解除须 ≤lo）。
    pub used: u32,
    /// true=Normal→Active（激活），false=Active→Normal（解除）。
    pub activation: bool,
    /// 迁移时刻在档的滞回带宽（应恒等于 BAND）。
    pub band: u32,
}

/// 大页池水位监视器。
#[derive(Clone, Debug)]
pub struct WaterMonitor {
    pub total: u32,
    pub hi: u32,
    pub lo: u32,
    /// 上一喂水纪元（须严格递增）。
    pub last_epoch: u64,
    /// 上一喂水的已用页数。
    pub used_last: u32,
    /// 当前告警电平。
    pub alarm: AlarmLevel,
    /// 告警迁移账。
    pub transitions: Vec<AlarmTransition>,
    /// 生命周期状态。
    pub state: MonitorState,
    /// 累计喂水步数。
    pub feeds: u32,
}

impl WaterMonitor {
    /// 构造：配置守卫（0≤lo<hi≤total），违者 ConfigBroken。
    pub fn new(total: u32, hi: u32, lo: u32) -> Result<WaterMonitor, u32> {
        if lo >= hi || hi > total { return Err(HugeWmCode::ConfigBroken.val()); }
        Ok(WaterMonitor { total, hi, lo, last_epoch: 0, used_last: 0, alarm: AlarmLevel::Normal,
            transitions: Vec::new(), state: MonitorState::Idle, feeds: 0 })
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: MonitorState) -> Result<(), u32> {
        if !self.state.can_advance(to) { return Err(HugeWmCode::SealGuard.val()); }
        self.state = to;
        Ok(())
    }

    /// 布防：Idle→Armed。
    pub fn arm(&mut self) -> Result<(), u32> { self.advance_state(MonitorState::Armed) }

    /// 封档：Armed→Sealed。
    pub fn seal(&mut self) -> Result<(), u32> { self.advance_state(MonitorState::Sealed) }

    /// 下一合法纪元（喂水方按此取号）。
    pub fn next_epoch(&self) -> u64 { self.last_epoch + 1 }

    /// 滞回带宽在档值。
    pub fn band(&self) -> u32 { self.hi - self.lo }

    /// 喂水一步：水位越顶/时序倒退/未布防/封档显性拒；
    /// 电平按滞回规则迁移，翻转落账（激活/解除事实随账在档）。
    pub fn feed(&mut self, used: u32, epoch: u64) -> Result<AlarmLevel, u32> {
        if self.state != MonitorState::Armed { return Err(HugeWmCode::SealGuard.val()); }
        if used > self.total { return Err(HugeWmCode::WaterAboveTotal.val()); }
        if epoch <= self.last_epoch { return Err(HugeWmCode::EpochBack.val()); }
        let prev = self.alarm;
        // 滞回规则：Normal 上穿 hi 才激活；Active 下穿 lo 才解除；带内冻结前态。
        let next = match prev {
            AlarmLevel::Normal => if used >= self.hi { AlarmLevel::Active } else { AlarmLevel::Normal },
            AlarmLevel::Active => if used <= self.lo { AlarmLevel::Normal } else { AlarmLevel::Active },
        };
        self.last_epoch = epoch;
        self.used_last = used;
        self.feeds += 1;
        if next != prev {
            let seq = match self.transitions.last() { Some(t) => t.seq + 1, None => 1 };
            self.transitions.push(AlarmTransition { seq, epoch, used, activation: next == AlarmLevel::Active, band: self.band() });
        }
        self.alarm = next;
        Ok(next)
    }

    /// 告警账实核：seq 连续、激活/解除事实与越限口径一致、
    /// 激活与解除交替、带值逐条在档且等于现配带宽。
    pub fn verify_transitions(&self) -> Result<u32, u32> {
        let n = self.transitions.len() as u32;
        let mut i = 0usize;
        while i < self.transitions.len() {
            let t = match self.transitions.get(i) { Some(t) => *t, None => return Err(HugeWmCode::LogBroken.val()) };
            if t.seq != (i + 1) as u32 { return Err(HugeWmCode::LogBroken.val()); }
            if t.band != self.band() { return Err(HugeWmCode::ConfigBroken.val()); }
            if t.activation {
                if t.used < self.hi { return Err(HugeWmCode::FalseAlarm.val()); }
                if i % 2 != 0 { return Err(HugeWmCode::HysteresisFlip.val()); }
            } else {
                if t.used > self.lo { return Err(HugeWmCode::FalseAlarm.val()); }
                if i % 2 != 1 { return Err(HugeWmCode::HysteresisFlip.val()); }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 带内冻结探：当前电平下喂带内水位，电平必须不翻转、不落账。
    pub fn probe_band_freeze(&mut self, used_in_band: u32) -> Result<bool, u32> {
        let before = self.alarm;
        let t_before = self.transitions.len();
        let after = self.feed(used_in_band, self.next_epoch())?;
        Ok(after == before && self.transitions.len() == t_before)
    }

    /// 统计：激活次数。
    pub fn activations(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.transitions.len() {
            if let Some(t) = self.transitions.get(i) { if t.activation { c += 1; } }
            i += 1;
        }
        c
    }

    /// 统计：解除次数。
    pub fn clears(&self) -> u32 { self.transitions.len() as u32 - self.activations() }
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() { h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193); i += 1; }
    h
}

// ── 告警账留痕件 ────────────────────────────────────────────────
/// 留痕件渲染：@HWM1 头行 + 逐迁移行 e{seq}|ep={epoch}|used={used}|{A|C}。
pub fn render_log(m: &WaterMonitor) -> String {
    let mut body = String::new();
    let mut i = 0usize;
    while i < m.transitions.len() {
        if let Some(t) = m.transitions.get(i) {
            body.push_str(&format!("e{}|ep={}|used={}|{}\n", t.seq, t.epoch, t.used,
                if t.activation { "A" } else { "C" }));
        }
        i += 1;
    }
    let hdr = format!("@HWM1|total={}|hi={}|lo={}|band={}|events={}|sum={:08X};\n",
        m.total, m.hi, m.lo, m.band(), m.transitions.len(), fnv1a(body.as_bytes()));
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

/// 解析后的留痕头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogHeader { pub total: u32, pub hi: u32, pub lo: u32, pub band: u32, pub events: u32, pub sum: u32 }

/// 留痕头解析：逐 token 扫描，未知字段/缺字段即破损。
pub fn parse_log_header(hdr: &str) -> Option<LogHeader> {
    let mut total: Option<u64> = None;
    let mut hi: Option<u64> = None;
    let mut lo: Option<u64> = None;
    let mut band: Option<u64> = None;
    let mut events: Option<u64> = None;
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
                "total" => total = Some(parse_dec(v)?),
                "hi" => hi = Some(parse_dec(v)?),
                "lo" => lo = Some(parse_dec(v)?),
                "band" => band = Some(parse_dec(v)?),
                "events" => events = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() { break; }
        i = end + 1;
    }
    Some(LogHeader { total: total? as u32, hi: hi? as u32, lo: lo? as u32, band: band? as u32, events: events? as u32, sum: sum? })
}

/// 正文裸控制符扫描（<0x20 且非换行 → 账破损）。
fn body_control_scan(body: &str) -> Result<(), u32> {
    let b = body.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < 0x20 && b[i] != b'\n' { return Err(HugeWmCode::LogBroken.val()); }
        i += 1;
    }
    Ok(())
}

/// 留痕件核验器（独立代码路径，供对拍）：
///   ① 头域解析并与监视器现配逐域对账（total/hi/lo/band）；
///   ② 逐行解析 e{seq}|ep|used|旗，seq 连续、纪元严格递增、used∈[0,total]；
///   ③ 激活须 used≥hi、解除须 used≤lo（否则误报）；
///   ④ 激活/解除必须 A/C 交替（否则滞回抖动）；
///   ⑤ events 计数对账 + FNV-1a 校验和 + 裸控制符扫描。返回实核迁移笔数。
pub fn verify_log_text(m: &WaterMonitor, out: &str) -> Result<u32, u32> {
    let ob = out.as_bytes();
    if !out.starts_with("@HWM1|") { return Err(HugeWmCode::LogBroken.val()); }
    let semi = match find_byte(ob, 0, b';') { Some(p) => p, None => return Err(HugeWmCode::LogBroken.val()) };
    let hdr = match parse_log_header(&out[6..semi]) { Some(h) => h, None => return Err(HugeWmCode::LogBroken.val()) };
    if hdr.total != m.total || hdr.hi != m.hi || hdr.lo != m.lo { return Err(HugeWmCode::LogBroken.val()); }
    if hdr.band != m.hi - m.lo { return Err(HugeWmCode::ConfigBroken.val()); }
    let mut start = semi + 1;
    if start < ob.len() && ob[start] == b'\n' { start += 1; }
    let body = &out[start..];
    body_control_scan(body)?;
    let bts = body.as_bytes();
    let mut count = 0u32;
    let mut prev_ep: u64 = 0;
    let mut i = 0usize;
    while i < bts.len() {
        let nl = match find_byte(bts, i, b'\n') { Some(p) => p, None => bts.len() };
        let line = &body[i..nl];
        // 行内四段：e{seq} / ep={} / used={} / 旗。
        let mut segs: [&str; 4] = ["", "", "", ""];
        let mut si = 0usize;
        let mut p = 0usize;
        while p <= line.len() && si < 4 {
            let e = match find_byte(line.as_bytes(), p, b'|') { Some(x) => x, None => line.len() };
            segs[si] = &line[p..e];
            si += 1;
            if e >= line.len() { break; }
            p = e + 1;
        }
        if si != 4 || !segs[0].starts_with('e') { return Err(HugeWmCode::LogBroken.val()); }
        let seq = match parse_dec(&segs[0][1..]) { Some(v) => v, None => return Err(HugeWmCode::LogBroken.val()) };
        if seq != (count + 1) as u64 { return Err(HugeWmCode::LogBroken.val()); }
        if !segs[1].starts_with("ep=") { return Err(HugeWmCode::LogBroken.val()); }
        let ep = match parse_dec(&segs[1][3..]) { Some(v) => v, None => return Err(HugeWmCode::EpochBack.val()) };
        if ep <= prev_ep { return Err(HugeWmCode::EpochBack.val()); }
        prev_ep = ep;
        if !segs[2].starts_with("used=") { return Err(HugeWmCode::LogBroken.val()); }
        let used64 = match parse_dec(&segs[2][5..]) { Some(v) => v, None => return Err(HugeWmCode::WaterAboveTotal.val()) };
        if used64 > m.total as u64 { return Err(HugeWmCode::WaterAboveTotal.val()); }
        let used = used64 as u32;
        let act = match segs[3] { "A" => true, "C" => false, _ => return Err(HugeWmCode::LogBroken.val()) };
        if act {
            if used < m.hi { return Err(HugeWmCode::FalseAlarm.val()); }
            if count % 2 != 0 { return Err(HugeWmCode::HysteresisFlip.val()); }
        } else {
            if used > m.lo { return Err(HugeWmCode::FalseAlarm.val()); }
            if count % 2 != 1 { return Err(HugeWmCode::HysteresisFlip.val()); }
        }
        count += 1;
        if nl >= bts.len() { break; }
        i = nl + 1;
    }
    if hdr.events != count { return Err(HugeWmCode::LogBroken.val()); }
    if hdr.sum != fnv1a(body.as_bytes()) { return Err(HugeWmCode::LogBroken.val()); }
    Ok(count)
}

// ── 池低水场景机（判据口径：N 轮每轮触发一次） ──────────────────
/// 低水场景机：每轮六步——逼近（hi-1 不触发）→ 上穿（激活）→ 高位持
/// 续 → 降入带内（冻结）→ 下穿 lo（解除）→ 低位持续。
/// 每轮恰好 1 激活 + 1 解除，零误报零抖动。
pub fn low_water_scenario(rounds: u32) -> Result<WaterMonitor, u32> {
    let mut m = WaterMonitor::new(POOL_TOTAL, HI_WATER, LO_WATER)?;
    m.arm()?;
    let mut r = 0u32;
    while r < rounds {
        m.feed(HI_WATER - 1, m.next_epoch())?;                       // 逼近线：不触发
        m.feed(HI_WATER, m.next_epoch())?;                           // 上穿高水位线：激活
        m.feed((HI_WATER + 100).min(POOL_TOTAL), m.next_epoch())?;   // 高位持续：不翻转
        m.feed(HI_WATER - 1, m.next_epoch())?;                       // 降入带内：冻结前态
        m.feed(LO_WATER, m.next_epoch())?;                           // 下穿低水位线：解除
        m.feed(LO_WATER - 10, m.next_epoch())?;                      // 低位持续：不翻转
        r += 1;
    }
    m.seal()?;
    Ok(m)
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

/// 行域单字节改写：先定位行锚 line_pat，再从行锚起定位域 field_pat。
fn patch_line_field(b: &[u8], line_pat: &[u8], field_pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let at = match find_sub(b, 0, line_pat).and_then(|lp| find_sub(b, lp, field_pat)) {
        Some(fp) => fp + field_pat.len(),
        None => usize::MAX,
    };
    patch_at(b, at, newc, alt, v);
}

/// 头域单字节改写：定位 pat，把其后首个字节改写。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) { Some(pp) => pp + pat.len(), None => usize::MAX };
    patch_at(b, tgt, newc, alt, v);
}

/// 头行尾下标（头行 '\n' 位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) { Some(x) => x, None => b.len() }
}

/// 对黄金告警账注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
    match kind % 10 {
        0 => patch_line_field(b, b"e1|", b"used=", b'3', b'2', &mut v), // 激活行 used 3480→2480 误报
        1 => patch_line_field(b, b"e1|", b"ep=", b'1', b'2', &mut v),   // 激活行 ep 1→2 时序重号
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),               // 校验和必抓
        3 => {
            // 删行：删正文首行（events 计数对不上必抓）。
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, b.len(), &mut v);
        }
        4 => {
            // 行重复：首行复制一份（seq 重号必抓）。
            push_range(b, 0, l1, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l1 + 1, b.len(), &mut v);
        }
        5 => {
            // 换序：前两行对调（纪元倒退必抓）。
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        6 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾：校验和必抓
        7 => {
            // 注控制符：头行后插裸 0x01（裸控制符扫描必抓）。
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        8 => patch_after(b, b"band", b'=', b'x', &mut v), // band→bandx 未知字段必抓
        _ => {
            // 加伪造行：尾部追加 used=100 的激活行（误报必抓）。
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"e99|ep=99|used=100|A\n";
            let mut k = 0usize;
            while k < tail.len() { v.push(tail[k]); k += 1; }
        }
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

/// 注入轮：每轮对新鲜黄金告警账注入一路失真，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let m = low_water_scenario(JUDGE_ROUNDS)?;
        let out = render_log(&m);
        verify_log_text(&m, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_log_text(&m, &distort(&out, kind)).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：低水场景 10 轮 → 账实核 → 留痕核验 → 带内冻结探 → 四拒探
///（越顶 / 纪元倒退 / 封档 / 留痕篡改）。
/// 返回 (正旗数, 拒旗数, 激活数, 迁移笔数)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let m = low_water_scenario(JUDGE_ROUNDS)?;
    let n_trans = m.verify_transitions()?;
    let acts = m.activations();
    let out = render_log(&m);
    let ev = verify_log_text(&m, &out)?;
    // 带内冻结双探：Normal 态与 Active 态各喂一带内值，均不得翻转。
    let mut probe = WaterMonitor::new(POOL_TOTAL, HI_WATER, LO_WATER)?;
    probe.arm()?;
    let f1 = probe.probe_band_freeze(HI_WATER - 1)?;
    let _ = probe.feed(HI_WATER, probe.next_epoch())?;
    let f2 = probe.probe_band_freeze(LO_WATER + 1)?;
    let band_ok = f1 && f2 && probe.transitions.len() == 1;
    // 四拒探。
    let mut rej = WaterMonitor::new(POOL_TOTAL, HI_WATER, LO_WATER)?;
    rej.arm()?;
    let over_rej = rej.feed(POOL_TOTAL + 1, rej.next_epoch()).is_err();
    let _ = rej.feed(10, rej.next_epoch());
    let ep_rej = rej.feed(10, rej.last_epoch).is_err();
    let _ = rej.seal();
    let seal_rej = rej.feed(10, rej.next_epoch()).is_err();
    let tamper_rej = verify_log_text(&m, &distort(&out, 2)).is_err();
    let flags = (acts == JUDGE_ROUNDS) as u32
        + (n_trans == JUDGE_ROUNDS.saturating_mul(2) && ev == n_trans) as u32
        + band_ok as u32
        + (m.clears() == JUDGE_ROUNDS) as u32;
    let rejects = over_rej as u32 + ep_rej as u32 + seal_rej as u32 + tamper_rej as u32;
    Ok((flags, rejects, acts, n_trans))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((f, r, a, t)) => {
            let want_trans = JUDGE_ROUNDS.saturating_mul(2);
            if f == 4 && r == 4 && a == JUDGE_ROUNDS && t == want_trans { Ok(()) }
            else { Err(HugeWmCode::NotReproducible.val()) }
        }
        Err(_) => Err(HugeWmCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) { Ok(l) if l.all_caught() => Ok(()), _ => Err(HugeWmCode::FalseAlarm.val()) }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72D40 && HugeWmCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = POOL_TOTAL == 4096 && HI_WATER == 3480 && LO_WATER == 3264 && BAND == 216
        && EPOCH == 9 && JUDGE_ROUNDS == 10 && in_window;
    if ok { Ok(()) } else { Err(HugeWmCode::NotReproducible.val()) }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(HugeWmCode::NotReproducible.val()); }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HugeWmCode::ALL.len() {
        let v = HugeWmCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}

// ── 配置守卫探针 ────────────────────────────────────────────────
/// lo≥hi / hi 越池 / 合法配置三路实核。
pub fn probe_config() -> bool {
    let bad1 = WaterMonitor::new(POOL_TOTAL, 3000, 3000).is_err();
    let bad2 = WaterMonitor::new(POOL_TOTAL, POOL_TOTAL + 1, 100).is_err();
    let bad3 = WaterMonitor::new(POOL_TOTAL, 100, 200).is_err();
    let ok1 = WaterMonitor::new(POOL_TOTAL, HI_WATER, LO_WATER).is_ok();
    bad1 && bad2 && bad3 && ok1
}

// ── 诊断码查表探针 ──────────────────────────────────────────────
pub fn probe_detail_table() -> bool {
    let mut i = 0;
    while i < HugeWmCode::ALL.len() {
        let v = HugeWmCode::ALL[i].val();
        if detail(v) != HugeWmCode::ALL[i].detail() { return false; }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let m = low_water_scenario(JUDGE_ROUNDS)?;
    let out = render_log(&m);
    let ev = verify_log_text(&m, &out)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!("状态=已完成 activations={} clears={} events={} inject={}/{} log_bytes={}",
        m.activations(), m.clears(), ev, inj.caught, inj.tries, out.len()))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链四正四拒。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, a, t)) => (*f == 4 && *r == 4, format!("flags={} rejects={} acts={} trans={}", f, r, a, t)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 主判据：池低水告警 10/10 次触发。
    let acts = match &g { Ok((_, _, a, _)) => *a, Err(_) => 0 };
    v.push(("J1-低水告警10/10触发", acts == JUDGE_ROUNDS, format!("activations={}/{}", acts, JUDGE_ROUNDS)));
    // J1 主判据：零误报（账实核 + 留痕核验双口径）。
    let zero = match low_water_scenario(JUDGE_ROUNDS) {
        Ok(m) => m.verify_transitions().is_ok() && m.activations() == m.clears(),
        Err(_) => false,
    };
    v.push(("J1-零误报双口径", zero, String::from("激活须 used≥hi / 解除须 used≤lo，A/C 交替")));
    // J1 主判据：滞回在档。
    let band_doc = match low_water_scenario(JUDGE_ROUNDS) {
        Ok(m) => {
            let mut all_band = true;
            let mut i = 0usize;
            while i < m.transitions.len() {
                if let Some(t) = m.transitions.get(i) { if t.band != BAND { all_band = false; } }
                i += 1;
            }
            all_band && m.band() == BAND
        }
        Err(_) => false,
    };
    v.push(("J1-滞回在档", band_doc, format!("band={} 逐迁移记录在档", BAND)));
    // J1 留痕件核验。
    let art = match low_water_scenario(JUDGE_ROUNDS) {
        Ok(m) => match verify_log_text(&m, &render_log(&m)) { Ok(n) => n == JUDGE_ROUNDS.saturating_mul(2), Err(_) => false },
        Err(_) => false,
    };
    v.push(("J1-留痕件核验", art, String::from("@HWM1 头域+逐行对账+校验和全过")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 带内冻结（滞回核心语义）。
    let freeze = {
        let mut probe = WaterMonitor::new(POOL_TOTAL, HI_WATER, LO_WATER);
        match probe {
            Ok(ref mut p) => {
                let _ = p.arm();
                let f1 = p.probe_band_freeze(HI_WATER - 1).unwrap_or(false);
                let _ = p.feed(HI_WATER, p.next_epoch());
                let f2 = p.probe_band_freeze(LO_WATER + 1).unwrap_or(false);
                f1 && f2 && p.transitions.len() == 1
            }
            Err(_) => false,
        }
    };
    v.push(("J2-带内冻结", freeze, String::from("带内 [lo,hi) 喂水电平不翻转不落账")));
    // J2 配置守卫。
    v.push(("J2-配置守卫", probe_config(), String::from("lo≥hi / hi 越池拒；合法配置过")));
    // J2 状态机单调：平移/倒退/未布防拒。
    let mut m2 = match WaterMonitor::new(POOL_TOTAL, HI_WATER, LO_WATER) { Ok(m) => m, Err(_) => return v };
    let s1 = m2.feed(10, m2.next_epoch()).is_err();
    let s2 = m2.arm().is_ok();
    let s3 = m2.arm().is_err();
    let s4 = m2.seal().is_ok();
    let s5 = m2.seal().is_err();
    let s6 = m2.arm().is_err();
    v.push(("J2-状态机单调", s1 && s2 && s3 && s4 && s5 && s6,
        format!("未布防拒={} 布防={} 平移拒={} 封档={} 平移拒={} 倒退拒={}", s1, s2, s3, s4, s5, s6)));
    // J2 拒探三路（越顶/倒退/封档）。
    let rej = match &g { Ok((_, r, _, _)) => *r >= 3, Err(_) => false };
    v.push(("J2-拒探三路", rej, String::from("越顶/纪元倒退/封档喂水显性拒")));
    // J1 诊断码查表。
    v.push(("J1-诊断码查表", probe_detail_table(), String::from("8 码闭集查表全对上")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb { Ok(s) => (!s.is_empty(), s.clone()), Err(e) => (false, format!("err=0x{:x}", e)) };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72D40 窗口")));
    v
}

pub fn run_unx2485_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2485 大页池水位与告警");
    for (n, p, d) in extra_checks() { s.add_dyn(n, p, &d); }
    s
}
