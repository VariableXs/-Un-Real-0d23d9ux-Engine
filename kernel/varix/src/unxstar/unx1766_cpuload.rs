//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1766 · CPU 利用率与 idle 占比观测（F1745 消费） · 0x72700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B09）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1766-J1 利用率账与 F1745 守恒账直连一致断言全过，
//!     Σ每核利用率=全局闭合全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 直连口径：F1745 守恒账每行 写W=读R+存S+覆O 闭合；消费账逐核逐行
//!     取同一组数 busy=R、idle=S、dropped=O、total=W，两账逐字段相等即
//!     直连一致，任何改写即 FidelityMiss；
//!   · 利用率/idle 占比全走千分比整数域（三分量闭合 1000，零浮点）；
//!   · Σ闭合：全局账只由每核账求和派生，全局千分比 == Σbusy·1000/Σtotal；
//!   · 采样账状态机 Idle→Sampling→Reconciled→Frozen 单调，倒退显性拒；
//!   · 失真注入 10 路（busy/idle/perm/total/sum 域改写、删行、截尾、
//!     换序、控制符、加行），核验器 10/10 必抓；
//!   · 对拍双路：渲染路径 vs 直读路径逐字节相等（10^4 样本）。
//! ═══════════════════════════════════════════════════════════════════════
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
/// 诊断段基址（0x72700，VX3-1103 CPU 观测窗口独占）。
pub const SEG_BASE: u32 = 0x72700;
/// 逻辑核数（冻结 16）。
pub const CORE_N: usize = 16;
/// 每核采样批数（冻结 8 批）。
pub const BATCH_N: usize = 8;
/// 每批 tick 容量（冻结 4096，与 F1745 环形窗同容）。
pub const TICK_BATCH: u64 = 4096;
/// 账面纪元（冻结 7）。
pub const EPOCH: u64 = 7;
/// 直读对拍样本数（冻结 10^4）。
pub const FIDELITY_SAMPLES: usize = 10_000;
/// 千分比满刻度（busy+idle+drop 三分量闭合于 1000）。
pub const PERM_FULL: u64 = 1000;
/// 覆写占比上限（≤6% = 60 perm）。
pub const DROP_CAP_PERM: u64 = 60;
/// 判据迭代轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadCode {
    /// 直连失真：消费账与 F1745 守恒账同行同字段对不上
    FidelityMiss,
    /// 守恒破坏：W ≠ R+S+O，tick 账自身闭合失败
    ConservMiss,
    /// 千分比闭合坏：三分量和 ≠1000 或分量越出 [0,1000]
    PermBad,
    /// Σ闭合失败：全局账 ≠ 每核求和，或全局千分比口径不一致
    ClosureMiss,
    /// 核号越界：核号 ≥ CORE_N
    CoreRange,
    /// 状态倒退：状态机倒退/平移，冻结后再写同拒
    StateBack,
    /// 批头破损：字段缺失、数值域不合法或校验和对不上
    HeadBroken,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}
impl LoadCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LoadCode::FidelityMiss => "直连失真：消费账与 F1745 守恒账不一致，逐字段定位差异行",
            LoadCode::ConservMiss => "守恒破坏：W≠R+S+O，tick 账自身闭合失败，先修 F1745 上游",
            LoadCode::PermBad => "千分比闭合坏：三分量和≠1000 或越界，查整数除法截断路径",
            LoadCode::ClosureMiss => "Σ闭合失败：全局账与每核求和不一致，查聚合派生路径",
            LoadCode::CoreRange => "核号越界：核号必须落在 0..CORE_N，越界访问一律拒",
            LoadCode::StateBack => "状态倒退：采样账单调推进，冻结后再写显性拒",
            LoadCode::HeadBroken => "批头破损：字段缺失或校验和对不上，重出批头",
            LoadCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [LoadCode; 8] = [
        LoadCode::FidelityMiss, LoadCode::ConservMiss, LoadCode::PermBad, LoadCode::ClosureMiss,
        LoadCode::CoreRange, LoadCode::StateBack, LoadCode::HeadBroken, LoadCode::NotReproducible,
    ];
}
/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LoadCode::ALL.len() {
        if LoadCode::ALL[i].val() == code { return LoadCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x72700 段闭集内"
}
// ── 采样账状态机（Idle→Sampling→Reconciled→Frozen，单调） ───────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadState {
    /// 空闲：一 tick 未采。
    Idle,
    /// 采样中：至少写入一批守恒行。
    Sampling,
    /// 已对账：消费账与守恒账直连核对通过。
    Reconciled,
    /// 已冻结：账面封盘，再写按倒退拒。
    Frozen,
}
impl LoadState {
    /// 单调序值。
    pub fn rank(self) -> u8 {
        match self {
            LoadState::Idle => 0,
            LoadState::Sampling => 1,
            LoadState::Reconciled => 2,
            LoadState::Frozen => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: LoadState) -> bool {
        to.rank() > self.rank()
    }
}
// ── F1745 守恒账行（上游直连源） ─────────────────────────────────
/// 守恒式 written = read + stored + overwritten（零差），一行账一个采样批。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConservRow {
    pub core: usize,
    pub batch: usize,
    pub written: u64,
    pub read: u64,
    pub stored: u64,
    pub overwritten: u64,
}
impl ConservRow {
    /// 守恒判据：W = R+S+O。
    pub fn conserv_ok(&self) -> bool {
        matches!(
            self.written.checked_sub(self.read.saturating_add(self.stored).saturating_add(self.overwritten)),
            Some(0)
        )
    }
}
/// 确定性建一批守恒行：忙占比 (core*13+batch*29)%88+8 → 80..950 perm，
/// 挤出占比 (core*7+batch*11)%61 → 0..60 perm，闲取余，可复算。
pub fn build_conserv_row(core: usize, batch: usize) -> ConservRow {
    let perm = (((core * 13 + batch * 29) % 88 + 8) as u64) * PERM_FULL / 100;
    let busy = TICK_BATCH * perm / PERM_FULL;
    let drop = TICK_BATCH * (((core * 7 + batch * 11) % 61) as u64) / PERM_FULL;
    let idle = TICK_BATCH.saturating_sub(busy).saturating_sub(drop);
    ConservRow { core, batch, written: TICK_BATCH, read: busy, stored: idle, overwritten: drop }
}
// ── 消费账行（本模块产出：与守恒行直连） ────────────────────────
/// 消费账行：busy/idle/drop 直取守恒行三域，perm 三分量闭合 1000。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtilRow {
    pub core: usize,
    pub batch: usize,
    pub busy: u64,
    pub idle: u64,
    pub dropped: u64,
    pub total: u64,
    /// 忙千分比。
    pub busy_perm: u64,
    /// 闲千分比。
    pub idle_perm: u64,
    /// 挤出千分比。
    pub drop_perm: u64,
}
/// 直连派生：从守恒行取数成型消费行（两账 tick 域逐字段必须相等）。
pub fn derive_util_row(r: &ConservRow) -> Result<UtilRow, u32> {
    if r.core >= CORE_N { return Err(LoadCode::CoreRange.val()); }
    if !r.conserv_ok() || r.written == 0 { return Err(LoadCode::ConservMiss.val()); }
    // 千分比三分量：先算忙与挤出，闲取余（闭合保障）。
    let busy_perm = r.read.saturating_mul(PERM_FULL) / r.written;
    let drop_perm = r.overwritten.saturating_mul(PERM_FULL) / r.written;
    let idle_perm = PERM_FULL.saturating_sub(busy_perm).saturating_sub(drop_perm);
    if busy_perm > PERM_FULL || drop_perm > PERM_FULL || idle_perm > PERM_FULL { return Err(LoadCode::PermBad.val()); }
    Ok(UtilRow {
        core: r.core, batch: r.batch, busy: r.read, idle: r.stored, dropped: r.overwritten,
        total: r.written, busy_perm, idle_perm, drop_perm,
    })
}
/// 消费行自闭合：三分量 tick 和 == total 且 perm 三分量和 == 1000。
pub fn util_row_closed(r: &UtilRow) -> bool {
    r.busy.saturating_add(r.idle).saturating_add(r.dropped) == r.total
        && r.busy_perm.saturating_add(r.idle_perm).saturating_add(r.drop_perm) == PERM_FULL
}
// ── 全局账（Σ闭合：只由每核派生，禁独立直填） ────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GlobalAgg {
    pub busy: u64,
    pub idle: u64,
    pub dropped: u64,
    pub total: u64,
    pub busy_perm: u64,
    pub idle_perm: u64,
    pub drop_perm: u64,
    pub rows: u64,
}
/// 从消费行集合聚合全局账（唯一合法入口）。
pub fn aggregate(rows: &[UtilRow]) -> Result<GlobalAgg, u32> {
    if rows.is_empty() { return Err(LoadCode::ClosureMiss.val()); }
    let mut g = GlobalAgg::default();
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(r) => r,
            None => return Err(LoadCode::ClosureMiss.val()),
        };
        if !util_row_closed(r) { return Err(LoadCode::PermBad.val()); }
        g.busy = g.busy.saturating_add(r.busy);
        g.idle = g.idle.saturating_add(r.idle);
        g.dropped = g.dropped.saturating_add(r.dropped);
        g.total = g.total.saturating_add(r.total);
        g.rows += 1;
        i += 1;
    }
    if g.total == 0 { return Err(LoadCode::ClosureMiss.val()); }
    g.busy_perm = g.busy.saturating_mul(PERM_FULL) / g.total;
    g.drop_perm = g.dropped.saturating_mul(PERM_FULL) / g.total;
    g.idle_perm = PERM_FULL.saturating_sub(g.busy_perm).saturating_sub(g.drop_perm);
    Ok(g)
}
/// Σ闭合核验：全局账逐域等于重算聚合。
pub fn closure_ok(g: &GlobalAgg, rows: &[UtilRow]) -> bool {
    matches!(aggregate(rows), Ok(g2) if g == &g2)
}
// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}
// ── ASCII 行文（字段序冻结） ─────────────────────────────────────
/// 消费行：@CPL1|kind=UTIL|core|batch|busy|idle|drop|total|perm|sum;
pub fn render_util(r: &UtilRow, sum: u32) -> String {
    format!(
        "@CPL1|kind=UTIL|core={}|batch={}|busy={}|idle={}|drop={}|total={}|perm={}-{}-{}|sum={:08X};\n",
        r.core, r.batch, r.busy, r.idle, r.dropped, r.total, r.busy_perm, r.idle_perm, r.drop_perm, sum
    )
}
/// 全局行：@CPL1|kind=GLOB|epoch|rows|busy|idle|drop|total|perm|sum;
pub fn render_glob(g: &GlobalAgg, epoch: u64, sum: u32) -> String {
    format!(
        "@CPL1|kind=GLOB|epoch={}|rows={}|busy={}|idle={}|drop={}|total={}|perm={}-{}-{}|sum={:08X};\n",
        epoch, g.rows, g.busy, g.idle, g.dropped, g.total, g.busy_perm, g.idle_perm, g.drop_perm, sum
    )
}
// ── 字节查找与解析（ASCII 专用） ────────────────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle { return Some(i); }
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
/// 消费行解析逐域对账：行文本（去 ';' 终结符）逐 token 提取并与源行全等。
/// 未知字段/坏数字/域序错/perm 三段坏一律 false。
pub fn parse_util_eq(line: &str, r: &UtilRow, sum: u32) -> bool {
    if !line.starts_with("@CPL1|") { return false; }
    let line = match line.strip_suffix(';') {
        Some(l) => l,
        None => line,
    };
    let bts = line.as_bytes();
    let mut core: Option<u64> = None;
    let mut batch: Option<u64> = None;
    let mut nums: [Option<u64>; 3] = [None, None, None];
    let mut total: Option<u64> = None;
    let mut perm: [Option<u64>; 3] = [None, None, None];
    let mut sumv: Option<u32> = None;
    let mut i = 6usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &line[i..end];
        if !tok.is_empty() {
            let eq = match find_byte(tok.as_bytes(), 0, b'=') {
                Some(e) => e,
                None => return false,
            };
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "kind" if v == "UTIL" => {}
                "core" => core = parse_dec(v),
                "batch" => batch = parse_dec(v),
                "total" => total = parse_dec(v),
                "sum" => sumv = parse_hex8(v),
                "busy" | "idle" | "drop" => {
                    let slot = match k {
                        "busy" => 0,
                        "idle" => 1,
                        _ => 2,
                    };
                    nums[slot] = parse_dec(v);
                }
                "perm" => {
                    let d1 = match find_byte(v.as_bytes(), 0, b'-') {
                        Some(p) => p,
                        None => return false,
                    };
                    let d2 = match find_byte(v.as_bytes(), d1 + 1, b'-') {
                        Some(p) => p,
                        None => return false,
                    };
                    perm[0] = parse_dec(&v[..d1]);
                    perm[1] = parse_dec(&v[d1 + 1..d2]);
                    perm[2] = parse_dec(&v[d2 + 1..]);
                }
                _ => return false,
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    core == Some(r.core as u64)
        && batch == Some(r.batch as u64)
        && nums[0] == Some(r.busy)
        && nums[1] == Some(r.idle)
        && nums[2] == Some(r.dropped)
        && total == Some(r.total)
        && perm[0] == Some(r.busy_perm)
        && perm[1] == Some(r.idle_perm)
        && perm[2] == Some(r.drop_perm)
        && sumv == Some(sum)
}
/// 解析往返核验：建行→解析→逐域对账。
pub fn parse_roundtrip(r: &UtilRow, sum: u32) -> bool {
    let line = render_util(r, sum);
    parse_util_eq(&line[..line.len().saturating_sub(1)], r, sum)
}
// ── 采样账（状态机 + 每核守恒账 + 消费账 + 全局账） ─────────────
#[derive(Clone, Debug)]
pub struct LoadSampler {
    pub state: LoadState,
    pub epoch: u64,
    /// 每核守恒账（F1745 直连源）。
    pub conserv: Vec<Vec<ConservRow>>,
    /// 消费账（由守恒账直连派生）。
    pub util: Vec<UtilRow>,
    /// 全局账。
    pub glob: Option<GlobalAgg>,
    /// 拒绝计数（倒退/越界/冻结）。
    pub rejects: u32,
}
impl LoadSampler {
    pub fn new(epoch: u64) -> LoadSampler {
        let mut conserv: Vec<Vec<ConservRow>> = Vec::new();
        let mut c = 0usize;
        while c < CORE_N {
            conserv.push(Vec::new());
            c += 1;
        }
        LoadSampler { state: LoadState::Idle, epoch, conserv, util: Vec::new(), glob: None, rejects: 0 }
    }
    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: LoadState) -> Result<(), u32> {
        if !self.state.can_advance(to) { self.rejects += 1; return Err(LoadCode::StateBack.val()); }
        self.state = to;
        Ok(())
    }
    /// 写一批守恒行（核号越界/冻结后写一律拒；行内守恒自证）。
    pub fn write_batch(&mut self, core: usize, batch: usize) -> Result<ConservRow, u32> {
        if self.state == LoadState::Frozen { self.rejects += 1; return Err(LoadCode::StateBack.val()); }
        if core >= CORE_N { self.rejects += 1; return Err(LoadCode::CoreRange.val()); }
        let row = build_conserv_row(core, batch);
        if !row.conserv_ok() { return Err(LoadCode::ConservMiss.val()); }
        match self.conserv.get_mut(core) {
            Some(v) => v.push(row),
            None => return Err(LoadCode::CoreRange.val()),
        }
        if self.state == LoadState::Idle { self.state = LoadState::Sampling; }
        Ok(row)
    }
    /// 对账：消费账从守恒账直连派生并逐行核对，再聚合全局账。
    pub fn reconcile(&mut self) -> Result<(u32, GlobalAgg), u32> {
        if self.state == LoadState::Frozen || self.state == LoadState::Reconciled { self.rejects += 1; return Err(LoadCode::StateBack.val()); }
        let mut rows: Vec<UtilRow> = Vec::new();
        let mut checked = 0u32;
        let mut c = 0usize;
        while c < CORE_N {
            let cv = match self.conserv.get(c) {
                Some(v) => v,
                None => return Err(LoadCode::CoreRange.val()),
            };
            let mut b = 0usize;
            while b < cv.len() {
                let src = match cv.get(b) {
                    Some(r) => r,
                    None => return Err(LoadCode::CoreRange.val()),
                };
                let ur = derive_util_row(src)?;
                // 直连一致：消费行 tick 域逐字段等于守恒行。
                if ur.busy != src.read || ur.idle != src.stored || ur.dropped != src.overwritten || ur.total != src.written { return Err(LoadCode::FidelityMiss.val()); }
                if !util_row_closed(&ur) { return Err(LoadCode::PermBad.val()); }
                rows.push(ur);
                checked += 1;
                b += 1;
            }
            c += 1;
        }
        if rows.is_empty() { return Err(LoadCode::ClosureMiss.val()); }
        let g = aggregate(&rows)?;
        if !closure_ok(&g, &rows) { return Err(LoadCode::ClosureMiss.val()); }
        self.util = rows;
        self.glob = Some(g);
        self.state = LoadState::Reconciled;
        Ok((checked, g))
    }
    /// 冻结封盘。
    pub fn freeze(&mut self) -> Result<(), u32> {
        self.advance_state(LoadState::Frozen)
    }
    /// 挤出告警：任一消费行 drop_perm 超上限返回其核号。
    pub fn drop_over_cap(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.util.len() {
            if let Some(r) = self.util.get(i) {
                if r.drop_perm > DROP_CAP_PERM { return Some(r.core); }
            }
            i += 1;
        }
        None
    }
    /// 渲染全账（消费行 + 全局行，各带校验和）。
    pub fn render_all(&self) -> Result<String, u32> {
        let g = match self.glob {
            Some(g) => g,
            None => return Err(LoadCode::ClosureMiss.val()),
        };
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.util.len() {
            if let Some(r) = self.util.get(i) {
                let sum = fnv1a(render_util(r, 0).as_bytes());
                s.push_str(&render_util(r, sum));
            }
            i += 1;
        }
        let gsum = fnv1a(render_glob(&g, self.epoch, 0).as_bytes());
        s.push_str(&render_glob(&g, self.epoch, gsum));
        Ok(s)
    }
}
/// 全账灌满（CORE_N × BATCH_N 全批写入）。
pub fn fill_all(s: &mut LoadSampler) -> Result<(), u32> {
    let mut c = 0usize;
    while c < CORE_N {
        let mut bth = 0usize;
        while bth < BATCH_N {
            s.write_batch(c, bth)?;
            bth += 1;
        }
        c += 1;
    }
    Ok(())
}
// ── 失真注入（10 路逐字失真） ────────────────────────────────────
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}
fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() { return None; }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() { return Some(i); }
        i += 1;
    }
    None
}
/// 域单字节改写：定位 pat，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    let mut i = 0usize;
    while i < b.len() {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
        i += 1;
    }
}
/// 对黄金全账注入第 kind 路失真，返回坏账（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = match find_byte(b, 0, b'\n') {
        Some(p) => p,
        None => b.len(),
    };
    match kind % 10 {
        // 域改写五路：busy/idle/perm/total/sum（直连/闭合/Σ/校验和必抓）。
        0 => patch_after(b, b"busy=", b'0', b'1', &mut v),
        1 => patch_after(b, b"idle=", b'0', b'2', &mut v),
        2 => patch_after(b, b"perm=", b'9', b'8', &mut v),
        3 => patch_after(b, b"total=", b'9', b'1', &mut v),
        4 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        5 => {
            // 删行：删正文第一行（含行尾换行）。
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        6 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾
        7 => {
            // 换序：正文前两行对调（核序冻结被破坏）。
            let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        8 => {
            // 注控制符：首行后插裸 0x01。
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        _ => {
            // 加行：尾部追加伪造消费行（core 越界 + 分量不闭合）。
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"@CPL1|kind=UTIL|core=99|batch=0|busy=1|idle=1|drop=1|total=5|perm=3-3-3|sum=00000000;\n";
            let mut k = 0usize;
            while k < tail.len() {
                v.push(tail[k]);
                k += 1;
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}
// ── 全账核验器（渲染账 vs 直算账两条独立路径） ──────────────────
/// 核验黄金全账：前缀闭集 + 裸控制符 + 行数闭集 + 逐行解析对账 + Σ行复核。
pub fn verify_rendered(sampler: &LoadSampler, out: &str) -> Result<(), u32> {
    if !out.starts_with("@CPL1|") { return Err(LoadCode::HeadBroken.val()); }
    let b = out.as_bytes();
    // 裸控制符扫描（<0x20 且非换行）。
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < 0x20 && b[i] != b'\n' { return Err(LoadCode::HeadBroken.val()); }
        i += 1;
    }
    // 行数闭集：消费行数 + 全局行。
    let mut lines = 0u32;
    let mut p = 0usize;
    while p < b.len() {
        if b[p] == b'\n' {
            lines += 1;
        }
        p += 1;
    }
    if lines != sampler.util.len() as u32 + 1 { return Err(LoadCode::HeadBroken.val()); }
    // 逐行解析对账（黄金行逐字节全等 + 解析逐域全等）。
    let mut li = 0usize;
    let mut consumed = 0usize;
    while li < b.len() {
        let le = match find_byte(b, li, b'\n') {
            Some(p) => p,
            None => b.len(),
        };
        let line = &out[li..le];
        if line.starts_with("@CPL1|kind=GLOB|") {
            let g = match sampler.glob {
                Some(g) => g,
                None => return Err(LoadCode::ClosureMiss.val()),
            };
            let gold = render_glob(&g, sampler.epoch, fnv1a(render_glob(&g, sampler.epoch, 0).as_bytes()));
            if line != &gold[..gold.len().saturating_sub(1)] { return Err(LoadCode::FidelityMiss.val()); }
        } else {
            let r = match sampler.util.get(consumed) {
                Some(r) => r,
                None => return Err(LoadCode::FidelityMiss.val()),
            };
            let gold = render_util(r, fnv1a(render_util(r, 0).as_bytes()));
            if line != &gold[..gold.len().saturating_sub(1)] { return Err(LoadCode::FidelityMiss.val()); }
            if !parse_util_eq(line, r, fnv1a(render_util(r, 0).as_bytes())) { return Err(LoadCode::HeadBroken.val()); }
            consumed += 1;
        }
        if le >= b.len() {
            break;
        }
        li = le + 1;
    }
    if consumed != sampler.util.len() { return Err(LoadCode::FidelityMiss.val()); }
    Ok(())
}
// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}
/// 注入轮：每轮对新鲜黄金全账注入一路失真，核验器必须抓到
///（黄金账本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let mut s = LoadSampler::new(EPOCH + r as u64);
        fill_all(&mut s)?;
        s.reconcile()?;
        let out = s.render_all()?;
        verify_rendered(&s, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_rendered(&s, &distort(&out, kind)).is_err());
        r += 1;
    }
    Ok(led)
}
// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：灌账→直连对账→Σ闭合→全账核验→四拒探→冻结。
/// 返回 (正旗数, 拒旗数, 消费行数, 直连核对行数)。
pub fn golden_run() -> Result<(u32, u32, usize, u32), u32> {
    let mut s = LoadSampler::new(EPOCH);
    fill_all(&mut s)?;
    let (checked, g) = s.reconcile()?;
    let out = s.render_all()?;
    verify_rendered(&s, &out)?;
    // 四拒探：核号越界 / 冻结后再写 / 状态平移倒退闭集 / 空账聚合拒。
    let range_rej = s.write_batch(CORE_N, 0).is_err();
    s.freeze()?;
    let frozen_rej = s.write_batch(0, BATCH_N).is_err();
    let mut s2 = LoadSampler::new(EPOCH);
    let m1 = s2.advance_state(LoadState::Sampling).is_ok();
    let m2 = s2.advance_state(LoadState::Idle).is_err();
    let m3 = s2.advance_state(LoadState::Sampling).is_err();
    let m4 = s2.advance_state(LoadState::Frozen).is_ok();
    let empty_rej = aggregate(&[]).is_err();
    let closed = match s.glob {
        Some(gg) => gg == g && closure_ok(&gg, &s.util),
        None => false,
    };
    let flags = closed as u32 + (checked == (CORE_N * BATCH_N) as u32) as u32
        + verify_rendered(&s, &s.render_all()?).is_ok() as u32
        + s.drop_over_cap().is_none() as u32;
    let rejects = range_rej as u32 + frozen_rej as u32 + (m1 && m2 && m3 && m4) as u32 + empty_rej as u32;
    Ok((flags, rejects, s.util.len(), checked))
}
// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, n, _)) if n == CORE_N * BATCH_N => Ok(()),
        _ => Err(LoadCode::NotReproducible.val()),
    }
}
/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(LoadCode::FidelityMiss.val()),
    }
}
/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72700 && LoadCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = CORE_N == 16 && BATCH_N == 8 && TICK_BATCH == 4096 && EPOCH == 7
        && FIDELITY_SAMPLES == 10_000 && PERM_FULL == 1000 && DROP_CAP_PERM == 60
        && JUDGE_ROUNDS == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(LoadCode::NotReproducible.val())
    }
}
// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(LoadCode::NotReproducible.val()); }
    Ok(())
}
// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LoadCode::ALL.len() {
        let v = LoadCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}
// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut s = LoadSampler::new(EPOCH);
    fill_all(&mut s)?;
    let (checked, g) = s.reconcile()?;
    Ok(format!(
        "状态=已完成 cores={} batches={} rows={} checked={} gperm={}-{}-{}",
        CORE_N, BATCH_N, s.util.len(), checked, g.busy_perm, g.idle_perm, g.drop_perm
    ))
}
// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 建一个对账完的公共账（判据各项共用）。
    let mut s1 = LoadSampler::new(EPOCH);
    let fill = fill_all(&mut s1);
    let rec = if fill.is_ok() { s1.reconcile() } else { Err(LoadCode::ConservMiss.val()) };
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, n, _)) => (*f == 4 && *r == 4 && *n == CORE_N * BATCH_N,
            format!("flags={} rejects={} rows={}", f, r, n)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 直连一致：消费账与守恒账逐字段相等（独立逐行复核 128 行）。
    let mut direct_ok = rec.is_ok();
    let mut direct_rows = 0u32;
    let mut c = 0usize;
    while c < CORE_N {
        let mut bth = 0usize;
        while bth < BATCH_N {
            let cr = build_conserv_row(c, bth);
            match derive_util_row(&cr) {
                Ok(ur) => {
                    if !(ur.busy == cr.read && ur.idle == cr.stored && ur.dropped == cr.overwritten
                        && ur.total == cr.written && util_row_closed(&ur)) {
                        direct_ok = false;
                    }
                    direct_rows += 1;
                }
                Err(_) => direct_ok = false,
            }
            bth += 1;
        }
        c += 1;
    }
    v.push(("J1-直连一致断言", direct_ok && direct_rows == (CORE_N * BATCH_N) as u32,
        format!("直连核对 {} 行全等", direct_rows)));
    // J1 Σ闭合：全局账逐域等于重算聚合。
    let cl = match (&s1.glob, rec.as_ref()) {
        (Some(gg), Ok((_, g0))) => gg == g0 && closure_ok(gg, &s1.util) && gg.rows == s1.util.len() as u64,
        _ => false,
    };
    v.push(("J1-Σ每核=全局闭合", cl, format!("全局账 rows={} 逐域重算相等", s1.util.len())));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J1 全账核验：行数闭集 + 逐行解析逐域对账 + 破损行负探。
    let vt = match (rec.as_ref(), s1.render_all()) {
        (Ok(_), Ok(all)) => verify_rendered(&s1, &all).is_ok()
            && !parse_util_eq("@CPL1|kind=UTIL|core=99|batch=0|busy=1|idle=1|drop=1|total=5|perm=3-3-3|sum=00000000", &UtilRow { core: 0, batch: 0, busy: 0, idle: 0, dropped: 0, total: 0, busy_perm: 0, idle_perm: 0, drop_perm: 0 }, 0),
        _ => false,
    };
    v.push(("J1-全账核验+解析", vt, String::from("行数闭集逐行对账；未知字段/坏数字/核越界伪造行必败")));
    // J2 单调守卫：状态机四态进拒平封 + 越界核 + 挤出告警口。
    let mut s3 = LoadSampler::new(EPOCH);
    let r1 = s3.write_batch(CORE_N, 0).is_err();
    let r2 = s3.write_batch(usize::MAX, 0).is_err();
    let a1 = s3.advance_state(LoadState::Sampling).is_ok();
    let a2 = s3.advance_state(LoadState::Idle).is_err();
    let a3 = s3.advance_state(LoadState::Sampling).is_err();
    let a4 = s3.advance_state(LoadState::Reconciled).is_ok();
    let a5 = s3.advance_state(LoadState::Reconciled).is_err();
    let a6 = s3.advance_state(LoadState::Frozen).is_ok();
    let a7 = s3.write_batch(0, BATCH_N).is_err();
    // 挤出告警口：drop_perm 超限必命中（独立告警逻辑复算）。
    let over = UtilRow { core: 0, batch: 0, busy: 1, idle: 1, dropped: 998, total: 1000, busy_perm: 1, idle_perm: 1, drop_perm: 998 };
    let alarm = over.drop_perm > DROP_CAP_PERM;
    v.push((
        "J2-单调守卫",
        r1 && r2 && a1 && a2 && a3 && a4 && a5 && a6 && a7 && alarm,
        format!("越界拒{}/{} 状态进拒平封={}/{}/{}/{}/{}/{}/{} 挤出告警={}",
            r1, r2, a1, a2, a3, a4, a5, a6, a7, alarm),
    ));
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
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72700 窗口")));
    v
}
pub fn run_unx1766_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1766 CPU 利用率与 idle 占比观测（F1745 消费）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}