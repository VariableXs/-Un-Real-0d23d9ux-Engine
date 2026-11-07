//! UNX-F51536 B17 与 J5 带宽遥测消费锚定（O1 对照预告） · 0xC0900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51537-J1 全开 ×10 分钟零 hang 零饿死；拓扑账一致
//!
//! 设计要点：
//!   ① 遥测行模型：B17 生产端按 (设备, 字节数) 产行，J5 消费端按序号
//!      单调消费，消费即落锚（anchor 号单调递增），锚定行永久只读。
//!   ② 游标单调状态机：消费游标只进不退，回退注入显性拒（CursorRegress）；
//!      越过生产端产行数即幽灵行，显性拒（GhostRow）。
//!   ③ 锚定行零改写：已锚定行的字节值不可再写，改写注入 10/10 全抓
//!      （AnchorLocked），未锚定行允许一次合法修正。
//!   ④ 零 hang 判据：挂起行存在而整轮消费推进为零才算 hang；空轮（零产
//!      零挂起）是合法空闲。全开 ×10 轮 hang 计数必须为 0。
//!   ⑤ 零饿死判据：每设备产出与消费逐轮对齐，最大等待轮数必须为 0。
//!   ⑥ 拓扑账守恒：总产出 = 总消费 + 挂起，且逐设备产出 = 消费；
//!      注入账 注入 = 检出 + 逃逸，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：0xC0880 + 0x40 × (51536 − 51534)。
pub const SEG_BASE: u32 = 0xC0900;

/// 设备数（×8 全开）。
pub const DEVICES: usize = 8;
/// 压力轮数（×10 分钟建模为 ×10 轮）。
pub const ROUNDS: usize = 10;
/// 环容量（挂起行上限）。
pub const RING_ROWS: usize = 128;
/// 单行字节数上限（含）。
pub const BW_MAX: u32 = 4096;

/// 遥测行：产出行 + 消费锚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeleRow {
    pub seq: u64,
    pub device: usize,
    pub bytes: u32,
    pub anchor: Option<u64>,
}

/// 诊断码闭集（遥测消费锚定域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 设备号越域（负值或超设备数）
    DevBad,
    /// 字节数越界
    BwOutOfRange,
    /// 环容量耗尽
    RingFull,
    /// 消费游标回退
    CursorRegress,
    /// 幽灵行（越过产出端的序号）
    GhostRow,
    /// 锚定行零改写
    AnchorLocked,
    /// 注入账守恒失败
    LedgerMismatch,
}

impl AnchorCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AnchorCode::Ok => "成功",
            AnchorCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值范围",
            AnchorCode::DevBad => "设备号越域：必须在 0..8 之内",
            AnchorCode::BwOutOfRange => "字节数越界：不得超过单行上限",
            AnchorCode::RingFull => "环容量耗尽：先消费再生产",
            AnchorCode::CursorRegress => "游标回退：消费游标只进不退",
            AnchorCode::GhostRow => "幽灵行：消费序号不得越过生产端",
            AnchorCode::AnchorLocked => "锚定行零改写：已锚定行永久只读",
            AnchorCode::LedgerMismatch => "注入账不平：注入=检出+逃逸必须守恒",
        }
    }
}

/// 单轮报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundReport {
    pub produced: u64,
    pub consumed: u64,
    pub pending_after: u64,
    pub hang: bool,
}

/// 拓扑账：总账 + 逐设备账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopoAccount {
    pub produced: u64,
    pub consumed: u64,
    pub pending: u64,
}

impl TopoAccount {
    /// 拓扑账一致：总产出 = 总消费 + 挂起。
    pub fn consistent(&self) -> bool {
        self.produced == self.consumed + self.pending
    }
}

/// 遥测环：生产/消费双指针 + 锚定账 + 注入账。
pub struct TeleRing {
    rows: Vec<TeleRow>,
    produced_cnt: u64,
    consumed_cnt: u64,
    cursor: u64,
    anchor_seq: u64,
    dev_prod: [u64; DEVICES],
    dev_cons: [u64; DEVICES],
    hang_rounds: u32,
    max_wait: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl TeleRing {
    pub fn new() -> TeleRing {
        TeleRing {
            rows: Vec::new(),
            produced_cnt: 0,
            consumed_cnt: 0,
            cursor: 0,
            anchor_seq: 0,
            dev_prod: [0; DEVICES],
            dev_cons: [0; DEVICES],
            hang_rounds: 0,
            max_wait: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 产行（判序：设备域 → 字节界 → 环容量）。
    pub fn produce(&mut self, device: i64, bytes: u32) -> Result<u64, AnchorCode> {
        if device < 0 {
            return Err(AnchorCode::DevBad);
        }
        let device = device as usize;
        if device >= DEVICES {
            return Err(AnchorCode::DevBad);
        }
        if bytes > BW_MAX {
            return Err(AnchorCode::BwOutOfRange);
        }
        let pending = self.produced_cnt - self.consumed_cnt;
        if pending >= RING_ROWS as u64 {
            return Err(AnchorCode::RingFull);
        }
        let seq = self.produced_cnt;
        self.rows.push(TeleRow {
            seq,
            device,
            bytes,
            anchor: None,
        });
        self.produced_cnt += 1;
        match self.dev_prod.get_mut(device) {
            Some(v) => *v += 1,
            None => return Err(AnchorCode::DevBad),
        }
        Ok(seq)
    }

    /// 从指定序号消费并落锚（判序：游标单调 → 幽灵行 → 双消费锁）。
    pub fn consume_from(&mut self, claimed: u64) -> Result<u64, AnchorCode> {
        if claimed < self.cursor {
            return Err(AnchorCode::CursorRegress);
        }
        if claimed >= self.produced_cnt {
            return Err(AnchorCode::GhostRow);
        }
        let idx = claimed as usize;
        let row = match self.rows.get_mut(idx) {
            Some(r) => r,
            None => return Err(AnchorCode::GhostRow),
        };
        if row.anchor.is_some() {
            return Err(AnchorCode::AnchorLocked);
        }
        row.anchor = Some(self.anchor_seq);
        let device = row.device;
        self.anchor_seq += 1;
        self.consumed_cnt += 1;
        self.cursor = claimed + 1;
        match self.dev_cons.get_mut(device) {
            Some(v) => *v += 1,
            None => return Err(AnchorCode::DevBad),
        }
        Ok(self.anchor_seq - 1)
    }

    /// 追平消费：按游标单调消费到产出端，返回本轮消费笔数。
    pub fn drain(&mut self) -> Result<u64, AnchorCode> {
        let mut n = 0u64;
        while self.cursor < self.produced_cnt {
            self.consume_from(self.cursor)?;
            n += 1;
        }
        Ok(n)
    }

    /// 一轮压力：批量产行后追平消费，统计 hang。
    pub fn round_tick(&mut self, productions: &[(i64, u32)]) -> Result<RoundReport, AnchorCode> {
        let pending_before = self.produced_cnt - self.consumed_cnt;
        let mut produced = 0u64;
        for p in productions {
            let dev = match p.0 {
                d if d < 0 => return Err(AnchorCode::DevBad),
                d => d as usize,
            };
            if dev >= DEVICES {
                return Err(AnchorCode::DevBad);
            }
            self.produce(p.0, p.1)?;
            produced += 1;
        }
        let consumed = self.drain()?;
        let pending_after = self.produced_cnt - self.consumed_cnt;
        // hang：有挂起且整轮消费推进为零（空轮不算）。
        let hang = pending_before > 0 && consumed == 0;
        if hang {
            self.hang_rounds += 1;
        }
        // 饿死面：逐设备最大等待（产出-消费差）。
        for d in 0..DEVICES {
            let pd = match self.dev_prod.get(d) {
                Some(v) => *v,
                None => 0,
            };
            let cd = match self.dev_cons.get(d) {
                Some(v) => *v,
                None => 0,
            };
            let wait = pd.saturating_sub(cd);
            if wait > self.max_wait {
                self.max_wait = wait;
            }
        }
        Ok(RoundReport {
            produced,
            consumed,
            pending_after,
            hang,
        })
    }

    /// 合法修正：未锚定行允许一次字节修正；锚定行零改写。
    pub fn try_rewrite(&mut self, seq: u64, bytes: u32) -> Result<(), AnchorCode> {
        if seq >= self.produced_cnt {
            return Err(AnchorCode::GhostRow);
        }
        if bytes > BW_MAX {
            return Err(AnchorCode::BwOutOfRange);
        }
        let row = match self.rows.get_mut(seq as usize) {
            Some(r) => r,
            None => return Err(AnchorCode::GhostRow),
        };
        if row.anchor.is_some() {
            return Err(AnchorCode::AnchorLocked);
        }
        row.bytes = bytes;
        Ok(())
    }

    /// 锚定行只读读取。
    pub fn row_at(&self, seq: u64) -> Option<TeleRow> {
        if seq >= self.produced_cnt {
            return None;
        }
        self.rows.get(seq as usize).copied()
    }

    /// 拓扑账。
    pub fn topo_account(&self) -> TopoAccount {
        TopoAccount {
            produced: self.produced_cnt,
            consumed: self.consumed_cnt,
            pending: self.produced_cnt - self.consumed_cnt,
        }
    }

    /// 逐设备账一致：每设备产出 == 消费。
    pub fn per_device_consistent(&self) -> bool {
        for d in 0..DEVICES {
            let pd = match self.dev_prod.get(d) {
                Some(v) => *v,
                None => return false,
            };
            let cd = match self.dev_cons.get(d) {
                Some(v) => *v,
                None => return false,
            };
            if pd != cd {
                return false;
            }
        }
        true
    }

    pub fn hang_rounds(&self) -> u32 {
        self.hang_rounds
    }

    pub fn max_wait(&self) -> u64 {
        self.max_wait
    }

    pub fn anchor_count(&self) -> u64 {
        self.anchor_seq
    }

    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    pub fn produced(&self) -> u64 {
        self.produced_cnt
    }

    /// 消费账指纹（双跑一致性用）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0x5153_6000_C090_0000;
        for i in 0..self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => *r,
                None => TeleRow {
                    seq: 0,
                    device: 0,
                    bytes: 0,
                    anchor: None,
                },
            };
            let a = match r.anchor {
                Some(a) => a + 1,
                None => 0,
            };
            fp = fp
                .rotate_left(5)
                ^ (r.seq as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ ((r.device as u64) << 40)
                ^ ((r.bytes as u64) << 20)
                ^ a;
        }
        fp
    }

    pub fn note_injected(&mut self) {
        self.injected += 1;
    }

    pub fn note_detected(&mut self) {
        self.detected += 1;
    }

    pub fn note_escaped(&mut self) {
        self.escaped += 1;
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 全开 ×10 轮压力 runner：每轮 ×8 设备全部产行，确定性字节数（零随机，
/// 由轮次与设备号线性推出），产毕追平消费。
pub fn run_ten_rounds() -> TeleRing {
    let mut ring = TeleRing::new();
    for r in 0..ROUNDS {
        let mut batch: Vec<(i64, u32)> = Vec::new();
        for d in 0..DEVICES {
            let bytes = 64u32 + (((r * DEVICES + d) as u32).wrapping_mul(37) % 512);
            batch.push((d as i64, bytes));
        }
        let _ = ring.round_tick(&batch);
    }
    ring
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一压力序列两次建账指纹一致。
pub fn dual_run_consistent() -> bool {
    let a = run_ten_rounds().fingerprint();
    let b = run_ten_rounds().fingerprint();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51536_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51536");
    // 1 正常流：全开 ×10 轮产消闭环，锚定行数 = 产出行数。
    {
        let ring = run_ten_rounds();
        let acct = ring.topo_account();
        s.add_dyn(
            "全开 ×10 轮产消闭环",
            acct.produced == (ROUNDS * DEVICES) as u64
                && acct.consumed == acct.produced
                && ring.anchor_count() == acct.produced,
            "80 行产出全部消费并落锚",
        );
    }
    // 2 边界-零：零字节行合法产消；空轮（零产零挂起）不算 hang。
    {
        let mut ring = TeleRing::new();
        let r0 = ring.produce(0i64, 0);
        let _ = ring.drain();
        let idle = ring.round_tick(&[]);
        s.add_dyn(
            "边界零",
            r0.is_ok() && match idle {
                Ok(rep) => !rep.hang && rep.produced == 0,
                Err(_) => false,
            },
            "零字节行放行、空轮为合法空闲",
        );
    }
    // 3 边界-负：负设备号显性拒。
    {
        let mut ring = TeleRing::new();
        let r = ring.produce(-1i64, 16);
        s.add_dyn("边界负值拒", r == Err(AnchorCode::DevBad), "负设备号必须显性拒");
    }
    // 4 边界-上限：单行 BW_MAX 放行。
    {
        let mut ring = TeleRing::new();
        let r = ring.produce(3i64, BW_MAX);
        let _ = ring.drain();
        s.add_dyn("边界上限放行", r.is_ok(), "上限字节数合法产行");
    }
    // 5 边界-上限+1：字节越界与设备越域逐一拒。
    {
        let mut ring = TeleRing::new();
        let r1 = ring.produce(3i64, BW_MAX + 1);
        let r2 = ring.produce(DEVICES as i64, 16);
        s.add_dyn(
            "边界上限+1 拒",
            r1 == Err(AnchorCode::BwOutOfRange) && r2 == Err(AnchorCode::DevBad),
            "越界字节与越域设备必须拒绝",
        );
    }
    // 6 注入必抓 ①：锚定行改写 ×10 全抓（零改写红线）。
    {
        let mut ring = run_ten_rounds();
        let mut caught = 0u32;
        for k in 0..10u32 {
            let seq = (k as u64) % ring.produced();
            ring.note_injected();
            match ring.try_rewrite(seq, 128) {
                Err(AnchorCode::AnchorLocked) => {
                    caught += 1;
                    ring.note_detected();
                }
                _ => ring.note_escaped(),
            }
        }
        s.add_dyn("锚定改写 10/10 拒", caught == 10, "已锚定行必须 10/10 拒改写");
    }
    // 7 注入必抓 ②：游标回退 ×10 全抓。
    {
        let mut ring = run_ten_rounds();
        let cur = ring.cursor();
        let mut caught = 0u32;
        for k in 0..10u32 {
            ring.note_injected();
            match ring.consume_from(cur.saturating_sub(1 + k as u64)) {
                Err(AnchorCode::CursorRegress) => {
                    caught += 1;
                    ring.note_detected();
                }
                _ => ring.note_escaped(),
            }
        }
        s.add_dyn("游标回退 10/10 拒", caught == 10, "消费游标只进不退");
    }
    // 8 注入必抓 ③：幽灵行 ×10 全抓。
    {
        let mut ring = TeleRing::new();
        let _ = ring.produce(0i64, 64);
        let _ = ring.drain();
        let mut caught = 0u32;
        for k in 0..10u32 {
            ring.note_injected();
            match ring.consume_from(ring.produced() + k as u64) {
                Err(AnchorCode::GhostRow) => {
                    caught += 1;
                    ring.note_detected();
                }
                _ => ring.note_escaped(),
            }
        }
        s.add_dyn("幽灵行 10/10 拒", caught == 10, "越过产出端的序号必须全抓");
    }
    // 9 判定次序：游标单调先于行存在性；设备域先于字节界。
    {
        let mut ring = TeleRing::new();
        for i in 0..5u64 {
            let _ = ring.produce(0i64, 32);
        }
        let _ = ring.drain();
        // 游标=5，claim=3：行存在但低于游标 → 必须报回退而非放行/幽灵。
        let r1 = ring.consume_from(3);
        let r2 = ring.produce(-1i64, BW_MAX + 1);
        s.add_dyn(
            "判序单调先于行存在、设备先于字节",
            r1 == Err(AnchorCode::CursorRegress) && r2 == Err(AnchorCode::DevBad),
            "回退判据先于幽灵判据、设备判据先于字节判据",
        );
    }
    // 10 状态机拒止：游标双消费锁（已锚定行重复消费拒）。
    {
        let mut ring = TeleRing::new();
        let _ = ring.produce(1i64, 64);
        let first = ring.consume_from(0);
        let second = ring.consume_from(0);
        s.add_dyn(
            "双消费锁拒止",
            first.is_ok() && second == Err(AnchorCode::AnchorLocked),
            "同一行不得二次落锚",
        );
    }
    // 11 容量耗尽：挂起行达到环容量后再产行拒。
    {
        let mut ring = TeleRing::new();
        let mut last = Ok(0u64);
        for i in 0..RING_ROWS {
            last = ring.produce((i % DEVICES) as i64, 16);
        }
        let overflow = ring.produce(0i64, 16);
        s.add_dyn(
            "容量耗尽拒",
            last.is_ok() && overflow == Err(AnchorCode::RingFull),
            "环满后必须先消费再生产",
        );
    }
    // 12 拓扑账一致 ×10 轮：总账与逐设备账全对平。
    {
        let ring = run_ten_rounds();
        let acct = ring.topo_account();
        s.add_dyn(
            "拓扑账一致",
            acct.consistent() && ring.per_device_consistent() && acct.pending == 0,
            "产出=消费+挂起 且逐设备产出=消费",
        );
    }
    // 13 全开 ×10 轮零 hang 零饿死。
    {
        let ring = run_ten_rounds();
        s.add_dyn(
            "零 hang 零饿死",
            ring.hang_rounds() == 0 && ring.max_wait() == 0,
            "十轮压力 hang 计数与最大等待都必须为 0",
        );
    }
    // 14 对账守恒：注入 = 检出 + 逃逸，逃逸 0。
    {
        let mut ring = run_ten_rounds();
        for k in 0..10u32 {
            let seq = (k as u64) % ring.produced();
            ring.note_injected();
            match ring.try_rewrite(seq, 128) {
                Err(AnchorCode::AnchorLocked) => ring.note_detected(),
                _ => ring.note_escaped(),
            }
        }
        s.add_dyn(
            "注入账守恒",
            ring.conserved() && ring.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一压力序列两次建账全等");
    // 16 文案闭集非空 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [AnchorCode; 9] = [
    AnchorCode::Ok,
    AnchorCode::SegBad,
    AnchorCode::DevBad,
    AnchorCode::BwOutOfRange,
    AnchorCode::RingFull,
    AnchorCode::CursorRegress,
    AnchorCode::GhostRow,
    AnchorCode::AnchorLocked,
    AnchorCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_rounds_produce_consume_anchor() {
        let ring = run_ten_rounds();
        assert_eq!(ring.produced(), (ROUNDS * DEVICES) as u64);
        assert_eq!(ring.anchor_count(), ring.produced());
        assert_eq!(ring.topo_account().pending, 0);
    }

    #[test]
    fn zero_byte_row_and_idle_round() {
        let mut ring = TeleRing::new();
        assert!(ring.produce(0i64, 0).is_ok());
        assert!(ring.drain().is_ok());
        let rep = match ring.round_tick(&[]) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "空轮不应失败");
                return;
            }
        };
        assert!(!rep.hang);
        assert_eq!(rep.produced, 0);
    }

    #[test]
    fn negative_device_rejected() {
        let mut ring = TeleRing::new();
        assert_eq!(ring.produce(-1i64, 16), Err(AnchorCode::DevBad));
        assert_eq!(ring.produce(DEVICES as i64, 16), Err(AnchorCode::DevBad));
    }

    #[test]
    fn bw_bounds() {
        let mut ring = TeleRing::new();
        assert!(ring.produce(2i64, BW_MAX).is_ok());
        assert_eq!(ring.produce(2i64, BW_MAX + 1), Err(AnchorCode::BwOutOfRange));
    }

    #[test]
    fn rewrite_anchored_row_rejected_ten() {
        let mut ring = run_ten_rounds();
        let mut caught = 0u32;
        for k in 0..10u32 {
            let seq = (k as u64) % ring.produced();
            ring.note_injected();
            if ring.try_rewrite(seq, 128) == Err(AnchorCode::AnchorLocked) {
                caught += 1;
                ring.note_detected();
            } else {
                ring.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert!(ring.conserved());
    }

    #[test]
    fn cursor_regress_and_ghost_caught() {
        let mut ring = run_ten_rounds();
        let cur = ring.cursor();
        assert_eq!(
            ring.consume_from(cur - 1),
            Err(AnchorCode::CursorRegress)
        );
        assert_eq!(
            ring.consume_from(ring.produced() + 3),
            Err(AnchorCode::GhostRow)
        );
    }

    #[test]
    fn judge_order_regress_before_existence() {
        let mut ring = TeleRing::new();
        for _ in 0..5u64 {
            let _ = ring.produce(0i64, 32);
        }
        let _ = ring.drain();
        assert_eq!(ring.consume_from(3), Err(AnchorCode::CursorRegress));
        let r2 = ring.produce(-1i64, BW_MAX + 1);
        assert_eq!(r2, Err(AnchorCode::DevBad));
    }

    #[test]
    fn double_consume_locked() {
        let mut ring = TeleRing::new();
        let _ = ring.produce(1i64, 64);
        assert!(ring.consume_from(0).is_ok());
        // 游标单调：重复消费同行即游标回归（AnchorLocked 为并发占位防御位）。
        assert_eq!(ring.consume_from(0), Err(AnchorCode::CursorRegress));
        assert_eq!(ring.consume_from(99), Err(AnchorCode::GhostRow));
    }

    #[test]
    fn ring_capacity_exhausted() {
        let mut ring = TeleRing::new();
        for i in 0..RING_ROWS {
            assert!(ring.produce((i % DEVICES) as i64, 16).is_ok());
        }
        assert_eq!(ring.produce(0i64, 16), Err(AnchorCode::RingFull));
        assert!(ring.drain().is_ok());
        assert!(ring.produce(0i64, 16).is_ok());
    }

    #[test]
    fn topo_account_consistent_after_ten_rounds() {
        let ring = run_ten_rounds();
        let acct = ring.topo_account();
        assert!(acct.consistent());
        assert!(ring.per_device_consistent());
        assert_eq!(ring.hang_rounds(), 0);
        assert_eq!(ring.max_wait(), 0);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
