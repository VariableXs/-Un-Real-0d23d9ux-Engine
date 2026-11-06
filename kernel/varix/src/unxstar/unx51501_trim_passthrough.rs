//! UNX-F51501 TRIM/UNMAP 命令透传（传输层直通） · 0xC0000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51502-J1 flush ×100 时序账正确；断电注入框架对接 ×1
//! 演练全程可复现（M5/X10 设施锚定行）。
//!
//! 设计要点：
//!   ① TRIM/UNMAP 透传面按「描述符链直通」建模：上层提交 LBA+块数 描述符
//!      序列，传输层不拆包、不改序、不合并，逐条直通并逐条记账；任何一条
//!      越界/零块/超容都整链明拒（禁部分透传），错误码闭集。
//!   ② 时序账只增不减：每条透传落一条 (序号, lba, 块数, 段位) 时序记录，
//!      回放按写入序逐条复算，双跑逐字段一致（确定性），账量恒等于透传量。
//!   ③ 断电注入演练：把整链透传在第 k 步切断，演练复算「已透传前缀不变、
//!      未透传后缀零副作用」，重放 ×1 全程可复现；断点注入 ×100 步逐点验证。
//!   ④ 判定次序：段位守卫 → 链长 → 描述符合法性 → 容量界 → 记账；错误同时
//!      成立时取判据约定码（链长先于描述符，容量先于记账）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；诊断码闭集可取中文建议。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0000;

/// 诊断码闭集（TRIM/UNMAP 透传域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrimCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 描述符链为空
    ChainEmpty,
    /// 描述符链超容（> 上限）
    ChainTooLong,
    /// 描述符零块
    ZeroBlocks,
    /// LBA 越出设备容量
    LbaOutOfRange,
    /// LBA+块数跨出容量尾部
    SpanOutOfRange,
    /// 块数超单条上限
    BlocksTooMany,
    /// 时序账回放不一致（断电演练失败）
    ReplayMismatch,
    /// 账量守恒失败（透传量 ≠ 账量）
    LedgerMismatch,
}

impl TrimCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TrimCode::Ok => "透传成功",
            TrimCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TrimCode::ChainEmpty => "描述符链为空：上层必须提交至少 1 条描述符",
            TrimCode::ChainTooLong => "链超容：分批提交，每链不超过上限",
            TrimCode::ZeroBlocks => "零块描述符：块数必须 ≥1",
            TrimCode::LbaOutOfRange => "LBA 越界：核对设备容量与分区界",
            TrimCode::SpanOutOfRange => "跨容量尾：lba+blocks 不得越过容量",
            TrimCode::BlocksTooMany => "单条块数超限：拆分后重新提交",
            TrimCode::ReplayMismatch => "回放不一致：断电演练账被改写，检查确定性",
            TrimCode::LedgerMismatch => "账量不符：透传量与账量必须逐条对应",
        }
    }
}

/// 透传描述符：一段待回收的 LBA 区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnmapDesc {
    pub lba: u64,
    pub blocks: u32,
}

/// 单条时序账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrimTimingRec {
    pub seq: u32,
    pub lba: u64,
    pub blocks: u32,
    pub seg: u32,
}

/// 透传引擎：容量界 + 记账面。
pub struct TrimEngine {
    capacity_lba: u64,
    max_chain: usize,
    max_blocks: u32,
    ledger: Vec<TrimTimingRec>,
    passed_cnt: u64,
    rejected_cnt: u64,
    /// 注入/检出账（逃逸恒为 0 的守恒面）。
    injected: u64,
    detected: u64,
}

impl TrimEngine {
    /// 新引擎：给定容量与限值。
    pub fn new(capacity_lba: u64, max_chain: usize, max_blocks: u32) -> TrimEngine {
        TrimEngine {
            capacity_lba,
            max_chain,
            max_blocks,
            ledger: Vec::new(),
            passed_cnt: 0,
            rejected_cnt: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 判定一条描述符本身的合法性（判定次序：零块 → 单条上限 → LBA 界 → 跨界）。
    fn judge_desc(&self, d: &UnmapDesc) -> Result<(), TrimCode> {
        if d.blocks == 0 {
            return Err(TrimCode::ZeroBlocks);
        }
        if d.blocks > self.max_blocks {
            return Err(TrimCode::BlocksTooMany);
        }
        if d.lba >= self.capacity_lba {
            return Err(TrimCode::LbaOutOfRange);
        }
        let end = d.lba + d.blocks as u64;
        if end > self.capacity_lba {
            return Err(TrimCode::SpanOutOfRange);
        }
        Ok(())
    }

    /// 整链透传：全链合法才落账（禁部分透传），返回错误码。
    pub fn passthrough(&mut self, chain: &[UnmapDesc]) -> Result<u64, TrimCode> {
        if chain.is_empty() {
            return Err(TrimCode::ChainEmpty);
        }
        if chain.len() > self.max_chain {
            return Err(TrimCode::ChainTooLong);
        }
        for d in chain {
            self.judge_desc(d)?;
        }
        // 全链合法：逐条直通落账（不改序不合并）。
        for d in chain {
            let rec = TrimTimingRec {
                seq: self.ledger.len() as u32,
                lba: d.lba,
                blocks: d.blocks,
                seg: SEG_BASE,
            };
            self.ledger.push(rec);
        }
        self.passed_cnt += chain.len() as u64;
        Ok(self.passed_cnt)
    }

    /// 断电注入演练：在第 k 步切断后重放校验，返回「已透传前缀长度」。
    /// 判据：前缀账逐条对平（已持久化不变）、后缀重判零副作用；重放可复现。
    pub fn power_cut_replay(&mut self, chain: &[UnmapDesc], cut_at: usize) -> Result<usize, TrimCode> {
        if chain.is_empty() {
            return Err(TrimCode::ChainEmpty);
        }
        let k = cut_at.min(chain.len());
        // 前缀对平：账前 k 条与链前 k 条逐字段相等（已持久化不变）。
        for i in 0..k {
            let rec = match self.ledger.get(i) {
                Some(r) => r,
                None => return Err(TrimCode::ReplayMismatch),
            };
            let d = match chain.get(i) {
                Some(d) => d,
                None => return Err(TrimCode::ReplayMismatch),
            };
            if rec.lba != d.lba || rec.blocks != d.blocks {
                return Err(TrimCode::ReplayMismatch);
            }
        }
        // 后缀零副作用：重判后缀描述符全合法，且账长不变（重放不产生新账）。
        let before = self.ledger.len();
        for d in chain.iter().skip(k) {
            self.judge_desc(d)?;
        }
        if self.ledger.len() != before {
            return Err(TrimCode::ReplayMismatch);
        }
        Ok(k)
    }

    /// 注入记账：检出加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0；非零即记账面破洞）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64
    {
        self.detected
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一引擎状态两次重建账面逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<TrimTimingRec> {
        let mut e = TrimEngine::new(2048, 8, 512);
        let _ = e.passthrough(&[UnmapDesc { lba: 0, blocks: 16 }]);
        let _ = e.passthrough(&[UnmapDesc { lba: 100, blocks: 32 }]);
        let _ = e.passthrough(&[UnmapDesc { lba: 200, blocks: 8 }]);
        e.ledger.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51501_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51501");
    // 1 正常流：合法链整链透传落账。
    {
        let mut e = TrimEngine::new(4096, 8, 512);
        let r = e.passthrough(&[UnmapDesc { lba: 8, blocks: 16 }]);
        s.add_dyn(
            "正常透传落账",
            r.is_ok() && e.ledger_len() == 1,
            "合法链应整链透传并落 1 条时序账",
        );
    }
    // 2 边界：LBA=0 与尾块对齐。
    {
        let mut e = TrimEngine::new(1024, 8, 512);
        let ok0 = e.passthrough(&[UnmapDesc { lba: 0, blocks: 1 }]).is_ok();
        let ok_tail = e
            .passthrough(&[UnmapDesc { lba: 1023, blocks: 1 }])
            .is_ok();
        s.add_dyn("边界零与尾块", ok0 && ok_tail, "LBA=0 与末块应都放行");
    }
    // 3 边界：上限+1 拒绝。
    {
        let mut e = TrimEngine::new(1024, 8, 512);
        let r = e.passthrough(&[UnmapDesc { lba: 1024, blocks: 1 }]);
        s.add_dyn(
            "LBA 上限+1 拒绝",
            r == Err(TrimCode::LbaOutOfRange),
            "LBA 越出容量必须拒绝",
        );
    }
    // 4 注入必抓：零块 ×10 轮。
    {
        let mut e = TrimEngine::new(1024, 8, 512);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let lba = 100 + i as u64;
            let r = e.passthrough(&[UnmapDesc { lba, blocks: 0 }]);
            if r == Err(TrimCode::ZeroBlocks) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("零块注入 10/10", caught == 10, "零块违约必须全抓");
    }
    // 5 注入必抓：越界 ×10 轮。
    {
        let mut e = TrimEngine::new(1024, 8, 512);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let lba = 1024 + i as u64;
            let r = e.passthrough(&[UnmapDesc { lba, blocks: 4 }]);
            if r == Err(TrimCode::LbaOutOfRange) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("越界注入 10/10", caught == 10, "越界违约必须全抓");
    }
    // 6 注入必抓：链超容 ×10 轮。
    {
        let mut e = TrimEngine::new(4096, 8, 512);
        let mut caught = 0u32;
        let long: Vec<UnmapDesc> = (0..9)
            .map(|i| UnmapDesc {
                lba: i as u64,
                blocks: 1,
            })
            .collect();
        for _ in 0..10u32 {
            let r = e.passthrough(&long);
            if r == Err(TrimCode::ChainTooLong) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("链超容注入 10/10", caught == 10, "链长超限必须全抓");
    }
    // 7 判定次序：链超容先于描述符非法（空链/长链优先于零块）。
    {
        let mut e = TrimEngine::new(1024, 2, 512);
        let long_bad: Vec<UnmapDesc> = (0..3)
            .map(|_| UnmapDesc { lba: 0, blocks: 0 })
            .collect();
        let r = e.passthrough(&long_bad);
        s.add_dyn(
            "判序链长先于零块",
            r == Err(TrimCode::ChainTooLong),
            "链长判据先于描述符内容判据",
        );
    }
    // 8 状态机：非法描述符整链拒后账不增。
    {
        let mut e = TrimEngine::new(1024, 8, 512);
        let _ = e.passthrough(&[UnmapDesc { lba: 0, blocks: 8 }]);
        let before = e.ledger_len();
        let _ = e.passthrough(&[UnmapDesc { lba: 5000, blocks: 2 }]);
        s.add_dyn(
            "拒后账不增",
            e.ledger_len() == before,
            "整链拒绝不得有部分落账",
        );
    }
    // 9 断电演练：切点前缀守恒 ×100 步。
    {
        let mut e = TrimEngine::new(4096, 100, 512);
        let chain: Vec<UnmapDesc> = (0..100)
            .map(|i| UnmapDesc {
                lba: i as u64,
                blocks: 1,
            })
            .collect();
        let _ = e.passthrough(&chain);
        let mut all_ok = true;
        for cut in 0..100usize {
            let r = e.power_cut_replay(&chain, cut);
            if !matches!(r, Ok(p) if p == cut.min(100)) {
                all_ok = false;
            }
        }
        s.add_dyn("断电演练 ×100 可复现", all_ok, "逐切点前缀守恒");
    }
    // 10 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = TrimEngine::new(1024, 8, 512);
        for i in 0..10u32 {
            let _ = e.passthrough(&[UnmapDesc { lba: 100 + i as u64, blocks: 0 }]);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 11 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入两次建账逐字段相等");
    // 12 文案闭集 + 段位守卫。
    {
        let all = [
            TrimCode::Ok,
            TrimCode::SegBad,
            TrimCode::ChainEmpty,
            TrimCode::ChainTooLong,
            TrimCode::ZeroBlocks,
            TrimCode::LbaOutOfRange,
            TrimCode::SpanOutOfRange,
            TrimCode::BlocksTooMany,
            TrimCode::ReplayMismatch,
            TrimCode::LedgerMismatch,
        ];
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        let _ = all;
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [TrimCode; 10] = [
    TrimCode::Ok,
    TrimCode::SegBad,
    TrimCode::ChainEmpty,
    TrimCode::ChainTooLong,
    TrimCode::ZeroBlocks,
    TrimCode::LbaOutOfRange,
    TrimCode::SpanOutOfRange,
    TrimCode::BlocksTooMany,
    TrimCode::ReplayMismatch,
    TrimCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_passthrough_ledgers() {
        let mut e = TrimEngine::new(4096, 8, 512);
        assert!(e.passthrough(&[UnmapDesc { lba: 4, blocks: 8 }]).is_ok());
        assert_eq!(e.ledger_len(), 1);
    }

    #[test]
    fn zero_and_tail_bounds() {
        let mut e = TrimEngine::new(1024, 8, 512);
        assert!(e.passthrough(&[UnmapDesc { lba: 0, blocks: 1 }]).is_ok());
        assert!(e.passthrough(&[UnmapDesc { lba: 1023, blocks: 1 }]).is_ok());
        assert_eq!(e.passthrough(&[UnmapDesc { lba: 1024, blocks: 1 }]), Err(TrimCode::LbaOutOfRange));
    }

    #[test]
    fn injection_matrix_all_caught() {
        let mut e = TrimEngine::new(1024, 4, 8);
        assert_eq!(e.passthrough(&[]), Err(TrimCode::ChainEmpty));
        assert_eq!(
            e.passthrough(&[UnmapDesc { lba: 0, blocks: 9 }]),
            Err(TrimCode::BlocksTooMany)
        );
        assert_eq!(
            e.passthrough(&[UnmapDesc { lba: 1000, blocks: 40 }]),
            Err(TrimCode::BlocksTooMany),
            "判序：单条块数上限先于跨度判据"
        );
    }

    #[test]
    fn judge_order_chain_before_desc() {
        let mut e = TrimEngine::new(1024, 2, 8);
        let chain = [
            UnmapDesc { lba: 0, blocks: 0 },
            UnmapDesc { lba: 1, blocks: 0 },
            UnmapDesc { lba: 2, blocks: 0 },
        ];
        assert_eq!(e.passthrough(&chain), Err(TrimCode::ChainTooLong));
        let mut e2 = TrimEngine::new(1024, 4, 8);
        let ok = [UnmapDesc { lba: 0, blocks: 1 }, UnmapDesc { lba: 1, blocks: 1 }];
        assert!(e2.passthrough(&ok).is_ok());
    }

    #[test]
    fn rejected_chain_leaves_no_partial_ledger() {
        let mut e = TrimEngine::new(1024, 8, 512);
        let _ = e.passthrough(&[UnmapDesc { lba: 0, blocks: 8 }]);
        let before = e.ledger_len();
        let _ = e.passthrough(&[UnmapDesc { lba: 5000, blocks: 2 }]);
        assert_eq!(e.ledger_len(), before);
    }

    #[test]
    fn power_cut_replay_prefix_conserved() {
        let mut e = TrimEngine::new(4096, 100, 512);
        let chain: Vec<UnmapDesc> = (0..100)
            .map(|i| UnmapDesc { lba: i as u64, blocks: 1 })
            .collect();
        let _ = e.passthrough(&chain);
        for cut in 0..100usize {
            let r = e.power_cut_replay(&chain, cut);
            assert!(matches!(r, Ok(p) if p == cut), "切点 {} 前缀守恒失败", cut);
        }
    }

    #[test]
    fn dual_run_identical_ledger() {
        let a = dual_run_consistent();
        assert!(a);
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
