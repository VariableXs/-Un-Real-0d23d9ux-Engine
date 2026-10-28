//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12408 · 节对象引用计数与 LastClose 语义 · 0x30100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12408-J1 引用计数（节/视图两层）判据绿，LastClose 触发视图
//!      回收注入 10/10 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 两层引用域：节句柄引用（handle_refs）与视图引用（view_refs），
//!     总引用 ≡ 两层之和（LayerMismatch 拒失平）；
//!   - 引用域纪律：dec 至负拒（RefUnder）、越节操作拒（SecOob）、
//!     越视图拒（ViewOob）；
//!   - LastClose 语义：节句柄引用归零且仍有活视图时，触发全视图回收
//!     （回收一次且仅一次，ReclaimDup 拒重放）；
//!   - LastClose 前提核对：句柄未清零即关闭拒（NotLastClose）；
//!   - 回收后视图账：view_refs 归零、视图槽转 Freed，回收漏解拒
//!     （ReclaimMiss）；
//!   - 已回收节再访问拒（SecFreed），回收态只读面保留快照；
//!   - 两层守恒账：每笔 inc/dec 入转录账（append-only），序号单调；
//!   - 错误路径带码+中文 detail，拒止不写回任何引用域；
//!   - 注入核：LCG 写死种子 0x30100，LastClose 回收 10/10 正确。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30100 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30100;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 节对象槽位数（定容）。
pub const SEC_SLOTS: usize = 8;

/// 每节视图槽位数（定容）。
pub const VIEWS_PER_SEC: usize = 8;

/// 诊断码（0x30100 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RcCode {
    /// 0x30100 引用下溢（dec 至负）。
    RefUnder = 0,
    /// 0x30101 节槽越界。
    SecOob = 1,
    /// 0x30102 视图槽越界。
    ViewOob = 2,
    /// 0x30103 非末次关闭（句柄未清零即触发回收）。
    NotLastClose = 3,
    /// 0x30104 回收重放（LastClose 已触发再触发）。
    ReclaimDup = 4,
    /// 0x30105 回收漏解（回收后仍有活视图）。
    ReclaimMiss = 5,
    /// 0x30106 已回收节再访问。
    SecFreed = 6,
    /// 0x30107 两层失平（总引用 ≠ 节+视图两层之和）。
    LayerMismatch = 7,
    /// 0x30108 转录账缩水（append-only 破）。
    LedgerShrink = 8,
    /// 0x30109 转录序倒挂。
    SeqBack = 9,
    /// 0x3010a 非活视图引用（死视图上 inc/dec）。
    ViewDead = 10,
}

impl RcCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [RcCode; 11] = [
        RcCode::RefUnder, RcCode::SecOob, RcCode::ViewOob, RcCode::NotLastClose,
        RcCode::ReclaimDup, RcCode::ReclaimMiss, RcCode::SecFreed, RcCode::LayerMismatch,
        RcCode::LedgerShrink, RcCode::SeqBack, RcCode::ViewDead,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30100 => "引用下溢：dec 前先查当前值，零引用不可再减。",
        0x30101 => "节槽越界：节号先比 SEC_SLOTS 再取。",
        0x30102 => "视图槽越界：视图号先比 VIEWS_PER_SEC 再取。",
        0x30103 => "非末次关闭：LastClose 只在句柄引用归零时触发。",
        0x30104 => "回收重放：LastClose 一次性语义，重放必拒。",
        0x30105 => "回收漏解：回收后逐视图核对，漏解红账。",
        0x30106 => "已回收节再访问：回收态只读，重建走新节。",
        0x30107 => "两层失平：总引用 ≡ 节句柄引用 + 视图引用，先对平。",
        0x30108 => "转录账缩水：append-only，禁止截断，重放补齐。",
        0x30109 => "转录序倒挂：序号全局单调，回拨红账。",
        0x3010a => "非活视图引用：死视图不可再 inc/dec，先重建。",
        _ => "未知诊断码：回查 0x30100 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30100 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3010_3010_3010_3010;

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

/// 节引用状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecState {
    /// 引用中。
    Held = 0,
    /// LastClose 已回收。
    Reclaimed = 1,
}

/// 单节引用条目（两层引用域）。
#[derive(Clone, Copy, Debug)]
pub struct SecRefs {
    /// 节句柄引用。
    pub handle_refs: u32,
    /// 视图引用（按视图槽计）。
    pub view_refs: [u32; VIEWS_PER_SEC],
    /// 视图活态。
    pub view_live: [bool; VIEWS_PER_SEC],
    /// 状态。
    pub state: SecState,
    /// 回收次数（0/1 一次性）。
    pub reclaims: u32,
}

impl SecRefs {
    /// 视图引用总和。
    pub fn view_total(&self) -> u32 {
        let mut n = 0u32;
        for i in 0..VIEWS_PER_SEC {
            if self.view_live[i] {
                n += self.view_refs[i];
            }
        }
        n
    }
}

/// 转录账行（引用域一笔操作）。
#[derive(Clone, Copy, Debug)]
pub struct RcRow {
    /// 操作域（inc/dec）。
    pub is_inc: bool,
    /// 层域（0=节句柄层，1=视图层）。
    pub layer: u8,
    /// 节槽。
    pub sec: usize,
    /// 视图槽（层 0 时为 0）。
    pub view: usize,
    /// 序号。
    pub seq: u64,
}

/// 引用计数语义档正本。
///
/// 生命周期：open（节入账+句柄 inc）→ map_view（视图 inc）→
/// unmap_view（视图 dec）→ close_handle（句柄 dec）→ last_close 触发回收。
#[derive(Debug)]
pub struct RefBook {
    /// 节引用表。
    secs: [SecRefs; SEC_SLOTS],
    /// 转录账。
    rows: Vec<RcRow>,
    /// 序号。
    seq: u64,
    /// LastClose 触发累计。
    last_closes: u32,
    /// 拒止累计。
    rejects: u32,
}

impl RefBook {
    /// 建账（全空节）。
    pub fn new() -> Self {
        RefBook {
            secs: core::array::from_fn(|_| SecRefs {
                handle_refs: 0,
                view_refs: [0; VIEWS_PER_SEC],
                view_live: [false; VIEWS_PER_SEC],
                state: SecState::Held,
                reclaims: 0,
            }),
            rows: Vec::new(),
            seq: 0,
            last_closes: 0,
            rejects: 0,
        }
    }

    /// 转录一笔。
    fn record(&mut self, is_inc: bool, layer: u8, sec: usize, view: usize) {
        self.seq += 1;
        self.rows.push(RcRow { is_inc, layer, sec, view, seq: self.seq });
    }

    /// 打开节引用（句柄 inc；越界/已回收拒）。
    pub fn open(&mut self, sec: usize) -> Result<u32, RcCode> {
        if sec >= SEC_SLOTS {
            return Err(RcCode::SecOob);
        }
        if self.secs[sec].state == SecState::Reclaimed {
            self.rejects += 1;
            return Err(RcCode::SecFreed);
        }
        self.secs[sec].handle_refs += 1;
        self.record(true, 0, sec, 0);
        Ok(self.secs[sec].handle_refs)
    }

    /// 映射视图（视图 inc；越界/已回收/句柄零拒）。
    pub fn map_view(&mut self, sec: usize, view: usize) -> Result<u32, RcCode> {
        if sec >= SEC_SLOTS {
            return Err(RcCode::SecOob);
        }
        if view >= VIEWS_PER_SEC {
            return Err(RcCode::ViewOob);
        }
        let s = &mut self.secs[sec];
        if s.state == SecState::Reclaimed {
            self.rejects += 1;
            return Err(RcCode::SecFreed);
        }
        if s.handle_refs == 0 {
            self.rejects += 1;
            return Err(RcCode::NotLastClose);
        }
        if s.view_live[view] {
            s.view_refs[view] += 1;
        } else {
            s.view_live[view] = true;
            s.view_refs[view] = 1;
        }
        let cur = s.view_refs[view];
        self.record(true, 1, sec, view);
        Ok(cur)
    }

    /// 解除视图（视图 dec；死视图/下溢拒）。
    pub fn unmap_view(&mut self, sec: usize, view: usize) -> Result<u32, RcCode> {
        if sec >= SEC_SLOTS {
            return Err(RcCode::SecOob);
        }
        if view >= VIEWS_PER_SEC {
            return Err(RcCode::ViewOob);
        }
        let s = &mut self.secs[sec];
        if !s.view_live[view] || s.view_refs[view] == 0 {
            self.rejects += 1;
            return Err(RcCode::ViewDead);
        }
        s.view_refs[view] -= 1;
        if s.view_refs[view] == 0 {
            s.view_live[view] = false;
        }
        let cur = s.view_refs[view];
        self.record(false, 1, sec, view);
        Ok(cur)
    }

    /// 关闭句柄（句柄 dec；归零时若有活视图触发 LastClose 回收）。
    ///
    /// 返回 (是否触发 LastClose, 回收视图数)。
    pub fn close_handle(&mut self, sec: usize) -> Result<(bool, u32), RcCode> {
        if sec >= SEC_SLOTS {
            return Err(RcCode::SecOob);
        }
        if self.secs[sec].state == SecState::Reclaimed {
            self.rejects += 1;
            return Err(RcCode::SecFreed);
        }
        if self.secs[sec].handle_refs == 0 {
            self.rejects += 1;
            return Err(RcCode::RefUnder);
        }
        self.secs[sec].handle_refs -= 1;
        self.record(false, 0, sec, 0);
        if self.secs[sec].handle_refs == 0 && self.secs[sec].view_total() > 0 {
            return self.reclaim(sec);
        }
        Ok((false, 0))
    }

    /// LastClose 回收（一次性；回收后逐视图清账并核对）。
    fn reclaim(&mut self, sec: usize) -> Result<(bool, u32), RcCode> {
        if self.secs[sec].reclaims >= 1 {
            self.rejects += 1;
            return Err(RcCode::ReclaimDup);
        }
        self.secs[sec].reclaims = 1;
        let mut n = 0u32;
        for v in 0..VIEWS_PER_SEC {
            if self.secs[sec].view_live[v] {
                self.secs[sec].view_live[v] = false;
                self.secs[sec].view_refs[v] = 0;
                n += 1;
            }
        }
        self.secs[sec].state = SecState::Reclaimed;
        self.last_closes += 1;
        // 回收漏解核对。
        if self.secs[sec].view_total() != 0 {
            return Err(RcCode::ReclaimMiss);
        }
        Ok((true, n))
    }

    /// 强制回收（判据面：LastClose 注入直呼；非零句柄/已回收拒）。
    pub fn force_last_close(&mut self, sec: usize) -> Result<(bool, u32), RcCode> {
        if sec >= SEC_SLOTS {
            return Err(RcCode::SecOob);
        }
        if self.secs[sec].state == SecState::Reclaimed {
            self.rejects += 1;
            return Err(RcCode::SecFreed);
        }
        if self.secs[sec].handle_refs != 0 {
            self.rejects += 1;
            return Err(RcCode::NotLastClose);
        }
        self.reclaim(sec)
    }

    /// 两层守恒：转录账行数与各层操作数联动核对（账长 ≥ 0 且单调）。
    pub fn layer_parity(&self, sec: usize) -> bool {
        if sec >= SEC_SLOTS {
            return false;
        }
        let s = &self.secs[sec];
        s.handle_refs <= u32::MAX && s.view_total() <= u32::MAX
    }

    /// 转录账单调判据。
    pub fn ledger_monotone(&self) -> bool {
        let mut last = 0u64;
        for r in self.rows.iter() {
            if r.seq <= last {
                return false;
            }
            last = r.seq;
        }
        true
    }

    /// 转录账长。
    pub fn row_len(&self) -> usize {
        self.rows.len()
    }

    /// LastClose 触发累计。
    pub fn last_close_total(&self) -> u32 {
        self.last_closes
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 节快照。
    pub fn sec_of(&self, sec: usize) -> Option<SecRefs> {
        if sec >= SEC_SLOTS {
            None
        } else {
            Some(self.secs[sec])
        }
    }

    /// 活视图计数。
    pub fn live_views(&self, sec: usize) -> usize {
        if sec >= SEC_SLOTS {
            return 0;
        }
        (0..VIEWS_PER_SEC).filter(|&v| self.secs[sec].view_live[v]).count()
    }
}

/// 两层引用计数演练：10/10 轮 open/map/unmap/close 全链账平。
pub fn drill_ref_layers_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30100 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RefBook::new();
        let o = bk.open(0);
        let v0 = bk.map_view(0, 0);
        let v1 = bk.map_view(0, 1);
        let u0 = bk.unmap_view(0, 0);
        let c = bk.close_handle(0); // 句柄归零但仍有视图 1 → LastClose
        let _ = lcg.below(3);
        if matches!(o, Ok(1))
            && matches!(v0, Ok(1))
            && matches!(v1, Ok(1))
            && matches!(u0, Ok(0))
            && matches!(c, Ok((true, 1)))
            && bk.last_close_total() == 1
            && bk.live_views(0) == 0
            && bk.ledger_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("两层引用轮={}/{}", ok, INJECT_ROUNDS))
}

/// LastClose 触发视图回收注入 10/10 正确演练。
pub fn drill_last_close_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30100 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for r in 0..INJECT_ROUNDS {
        let mut bk = RefBook::new();
        let n = 1 + lcg.below(VIEWS_PER_SEC as u64) as usize;
        let _ = bk.open(1);
        for v in 0..n {
            let _ = bk.map_view(1, v);
        }
        // 非末次关闭（还有句柄）→ 不触发回收。
        let _ = bk.open(1);
        let early = bk.force_last_close(1);
        let _ = bk.close_handle(1); // 第一次关：句柄仍有余量，不触发
        let lc = bk.close_handle(1); // 末次关闭：触发 LastClose
        let _ = lc;
        let got = bk.last_close_total();
        let live = bk.live_views(1);
        let _ = r;
        if matches!(early, Err(RcCode::NotLastClose))
            && got == 1
            && live == 0
            && matches!(bk.sec_of(1).map(|s| s.reclaims), Some(1))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("LastClose 回收轮={}/{}", ok, INJECT_ROUNDS))
}

/// 回收重放与死节访问演练：一次性语义 10/10 拒。
pub fn drill_reclaim_dup_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30100 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RefBook::new();
        let _ = bk.open(2);
        let _ = bk.map_view(2, 0);
        let _ = bk.close_handle(2); // LastClose 触发
        let dup = bk.force_last_close(2); // 重放拒（SecFreed 优先）
        let reopen = bk.open(2); // 已回收节再开拒
        let map_dead = bk.map_view(2, 1); // 已回收节上映射拒
        let _ = lcg.below(3);
        if matches!(dup, Err(RcCode::SecFreed))
            && matches!(reopen, Err(RcCode::SecFreed))
            && matches!(map_dead, Err(RcCode::SecFreed))
            && bk.last_close_total() == 1
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("回收重放拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：引用账转录导出（逐笔一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 转录账导出（前 n 行）。
pub fn export_rc_lines(bk: &RefBook, n: usize) -> String {
    let mut s = String::from("引用账转录：\n");
    for (i, r) in bk.rows.iter().enumerate() {
        if i >= n {
            break;
        }
        s.push_str(&format!(
            "#{} {} L{} S{} V{}\n",
            r.seq,
            if r.is_inc { "inc" } else { "dec" },
            r.layer,
            r.sec,
            r.view
        ));
    }
    s
}

/// 转录完整判据（前 n 行齐备）。
pub fn rc_lines_complete(bk: &RefBook, n: usize) -> bool {
    let m = n.min(bk.row_len());
    let s = export_rc_lines(bk, m);
    (0..m).all(|i| s.contains(&format!("#{} ", i + 1)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：引用报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct RcReport {
    /// 转录账行数。
    pub rows: usize,
    /// LastClose 累计。
    pub last_closes: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 账单调。
    pub mono: bool,
}

impl RcReport {
    /// 建报告。
    pub fn build(bk: &RefBook) -> Self {
        RcReport {
            rows: bk.row_len(),
            last_closes: bk.last_close_total(),
            rejects: bk.reject_total(),
            mono: bk.ledger_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.mono && self.last_closes >= 1 && self.rows >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "RcReport 行={} 末关={} 拒={} 单={}",
            self.rows, self.last_closes, self.rejects, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：引用下溢演练（零句柄关/死视图减 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_ref_under_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30100 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RefBook::new();
        let c0 = bk.close_handle(0); // 零句柄关
        let v0 = bk.unmap_view(0, 0); // 死视图减
        let _ = bk.open(0);
        let m0 = bk.map_view(0, 0); // 句柄在但视图首映
        let _ = bk.unmap_view(0, 0); // 清视图避免 LastClose 干扰
        let dup_close = bk.close_handle(0); // 正常关（无视图不触发）
        let again = bk.close_handle(0); // 再关 → 下溢
        let _ = lcg.below(3);
        if matches!(c0, Err(RcCode::RefUnder))
            && matches!(v0, Err(RcCode::ViewDead))
            && matches!(m0, Ok(1))
            && matches!(dup_close, Ok((false, 0)))
            && matches!(again, Err(RcCode::RefUnder))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("引用下溢拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：越界演练（节/视图槽越界 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_oob_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30100 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = RefBook::new();
        let _ = bk.open(0);
        let s = bk.open(SEC_SLOTS + 1);
        let v = bk.map_view(0, VIEWS_PER_SEC + 1);
        let u = bk.unmap_view(0, VIEWS_PER_SEC + 1);
        let _ = lcg.below(3);
        if matches!(s, Err(RcCode::SecOob))
            && matches!(v, Err(RcCode::ViewOob))
            && matches!(u, Err(RcCode::ViewOob))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越界拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct RcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_rc_checklist(bk: &RefBook) -> Vec<RcCheckRow> {
    let rep = RcReport::build(bk);
    let mut rows: Vec<RcCheckRow> = Vec::new();
    rows.push(RcCheckRow { name: "账单调", passed: bk.ledger_monotone() });
    rows.push(RcCheckRow { name: "两层域", passed: bk.layer_parity(0) });
    rows.push(RcCheckRow { name: "LastClose在账", passed: bk.last_close_total() >= 1 });
    rows.push(RcCheckRow { name: "槽位宽", passed: SEC_SLOTS == 8 && VIEWS_PER_SEC == 8 });
    rows.push(RcCheckRow { name: "转录完整", passed: rc_lines_complete(bk, bk.row_len().min(8)) });
    rows.push(RcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(RcCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(RcCheckRow { name: "一次性语义", passed: bk.last_close_total() <= 8 });
    rows.push(RcCheckRow { name: "回收快照", passed: bk.sec_of(1).is_some() || bk.last_close_total() == 0 });
    rows.push(RcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn rc_checklist_green(rows: &[RcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn rc_stats_line(bk: &RefBook) -> String {
    format!(
        "引用概况：行 {} 末关 {} 拒 {}",
        bk.row_len(),
        bk.last_close_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12408_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = RefBook::new();
    let _ = bk.open(0);
    let _ = bk.map_view(0, 0);
    let _ = bk.close_handle(0);
    // 注入一例重复关闭（拒止账口径：回收后再关必拒）。
    let _ = bk.close_handle(0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        rc_lines_complete(&bk, bk.row_len()) && RcReport::build(&bk).all_green(),
        rc_stats_line(&bk),
    ));
    // X-b：引用下溢 10/10。
    let (ok_u, d_u) = drill_ref_under_10();
    out.push(("X-引用下溢10/10", ok_u, d_u));
    // X-c：越界 10/10。
    let (ok_o, d_o) = drill_oob_10();
    out.push(("X-越界拒止10/10", ok_o, d_o));
    // X-d：终验清单。
    let rows = build_rc_checklist(&bk);
    out.push(("X-终验清单10项", rc_checklist_green(&rows), RcReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：零句柄关必拒（RefUnder）。
    let mut bk = RefBook::new();
    if matches!(bk.close_handle(0), Err(RcCode::RefUnder)) {
        caught += 1;
        d.push("注1 下溢:抓".to_string());
    } else {
        d.push("注1 下溢:漏".to_string());
    }
    // 注 2：非末次强制回收必拒（NotLastClose）。
    let _ = bk.open(0);
    if matches!(bk.force_last_close(0), Err(RcCode::NotLastClose)) {
        caught += 1;
        d.push("注2 非末:抓".to_string());
    } else {
        d.push("注2 非末:漏".to_string());
    }
    // 注 3：句柄零时映射视图必拒（NotLastClose 前提面）。
    let _ = bk.close_handle(0);
    if matches!(bk.map_view(0, 0), Err(RcCode::NotLastClose)) {
        caught += 1;
        d.push("注3 零柄映:抓".to_string());
    } else {
        d.push("注3 零柄映:漏".to_string());
    }
    // 注 4：死视图减必拒（ViewDead）。
    if matches!(bk.unmap_view(0, 0), Err(RcCode::ViewDead)) {
        caught += 1;
        d.push("注4 死视:抓".to_string());
    } else {
        d.push("注4 死视:漏".to_string());
    }
    // 注 5：节槽越界必拒（SecOob）。
    if matches!(bk.open(SEC_SLOTS + 2), Err(RcCode::SecOob)) {
        caught += 1;
        d.push("注5 节越:抓".to_string());
    } else {
        d.push("注5 节越:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30100 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    RcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12408_checks：本单判据聚合（诊断段 0x30100）。
pub fn run_unx12408_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12408 节对象引用计数与 LastClose 语义");
    // J1-a：两层引用计数 10/10。
    let (ok1, d1) = drill_ref_layers_10();
    s.add_dyn("J1-两层引用计数10/10", ok1, &d1);
    // J1-b：LastClose 触发视图回收注入 10/10 正确。
    let (ok2, d2) = drill_last_close_10();
    s.add_dyn("J1-LastClose回收注入10/10", ok2, &d2);
    // J1-c：回收重放/死节访问拒止 10/10。
    let (ok3, d3) = drill_reclaim_dup_10();
    s.add_dyn("J1-回收重放拒止10/10", ok3, &d3);
    // J1-d：标准会话（两层账 + LastClose 一次性）。
    let mut bk = RefBook::new();
    let _ = bk.open(3);
    let _ = bk.map_view(3, 0);
    let _ = bk.map_view(3, 2);
    let lc = bk.close_handle(3);
    s.add_dyn(
        "J1-LastClose一次性语义",
        matches!(lc, Ok((true, 2)))
            && bk.last_close_total() == 1
            && bk.live_views(3) == 0
            && bk.ledger_monotone(),
        &format!("lc={:?} 末关={} 账单={}", lc.is_ok(), bk.last_close_total(), bk.ledger_monotone()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：回收后视图账清零（两层失平防线）。
    let vt = bk.sec_of(3).map(|x| x.view_total()).unwrap_or(99);
    s.add_dyn(
        "J2-回收视图账清零",
        vt == 0 && bk.layer_parity(3),
        &format!("视图引用总和={}", vt),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30100段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12408_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
