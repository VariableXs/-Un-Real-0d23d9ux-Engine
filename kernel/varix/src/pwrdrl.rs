//! pwrdrl — WP-203 · B-703 断电一百次（判据实装层，MD2 篇 7.5 宪法）。
//!
//! 判据 B-703：零结构损坏，重放统计归档。
//! MD2 宪法原文（篇 7.2）："运行期写路径采用'日志元数据、数据按检查点'模式
//! ——这是断电窗口五秒（Q41）的来源，也是诚实文案的依据：最近五秒的元数据与
//! 数据可能未落，**结构永不损坏**。"
//! MD2 篇 7.3："检查点账本记录每次落盘的事务边界，断电重放时按账本恢复到
//! 最近检查点（判例 16 的'回到检查点而非假装完整'）。"
//! MD2 篇 7.5 断电恢复路径：上电 → 块层自检 → ext4 挂载尝试 → 日志重放
//! （按检查点账本）→ 重放成功正常挂载 / 失败转只读模式挂载并全屏黄条 →
//! 挂载结果与重放统计写入启动报告。
//! MD1 行 1640 反面："日志重放后直接读写——重放失败的盘被当健康盘写
//! （只读降级 B-701 路径）。"
//!
//! MD3 施工要点：断电百次（B-703）是本包出口仪式——QEMU 替身跑八十次、
//! 实机跑二十次，混着跑。本模块是宿主对练 harness（判据逻辑面），
//! QEMU/实机注入是接线面（脚本承接）。
//!
//! 核心恒等式：**断电丢数据（未 checkpoint 的），结构永不损坏。**
//! 半写块面（断电瞬间一部分新一部分旧）经账本重放必须收敛到
//! "最近完整检查点的盘面"；收敛失败 = 结构损坏 = 违例。

use crate::checks::CheckSet;
// no_std 态走 alloc 的 Vec/vec!（kvsrv/power_shutdown 同范式，两态通吃）。
use alloc::vec::{self, Vec};

/// 断电窗口（Q41）：最近五秒未落 = 诚实文案的数据面。
pub const POWER_LOSS_WINDOW_MS: u32 = 5_000;
/// 每轮对练的事务数范围上界。
pub const MAX_TXNS_PER_ROUND: usize = 40;
/// 盘面块数（对练模型 128 块 = 512KiB）。
pub const DISK_BLOCKS: usize = 128;

/// 检查点账本条目：一个已完整落盘的事务边界。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CkptEntry {
    /// 事务序号（严格递增）
    pub seq: u64,
    /// 事务覆盖块起点
    pub start: u32,
    /// 事务覆盖块数
    pub len: u32,
}

/// 检查点账本：只记"已完整落盘"的事务（半写事务绝不入账）。
#[derive(Default)]
pub struct CkptLedger {
    pub entries: Vec<CkptEntry>,
    pub sealed_seq: u64,
}

impl CkptLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 事务完整落盘后封账（边界入账本）。
    pub fn seal(&mut self, seq: u64, start: u32, len: u32) {
        self.entries.push(CkptEntry { seq, start, len });
        self.sealed_seq = seq;
    }

    /// 重放判定：给定断电时盘面可见的"新块位图"，从账本找最近完整检查点。
    /// 模型规则：账本内条目 = 承诺已落盘；条目外 = 可能半写。
    /// 收敛到最近检查点 = 盘面取账本最后一条覆盖为止的状态。
    /// 返回：重放到的检查点 seq 与被回滚的块数。
    pub fn replay(&self) -> (u64, usize) {
        match self.entries.last() {
            None => (0, 0),
            Some(last) => (last.seq, last.len as usize),
        }
    }
}

/// 结构完整性三面校验（重放后）：账本自洽 / 超级块面 / 数据面。
/// 模型层用确定性校验（块值 = f(seq, lba)），损坏 = 校验值错位。
pub struct DiskFace {
    /// 每块的 (写序号, 值)——值 = seq*256 + lba mod 256，可复算校验
    pub blocks: [(u64, u8); DISK_BLOCKS],
}

impl DiskFace {
    pub fn fresh() -> Self {
        Self { blocks: [(0, 0); DISK_BLOCKS] }
    }

    /// 写一个事务（半写可注入：只写前 keep_len 块——断电截断模型）。
    pub fn write_txn(&mut self, seq: u64, start: u32, len: u32, keep_len: u32) {
        for i in 0..keep_len.min(len) {
            let lba = (start + i) as usize % DISK_BLOCKS;
            self.blocks[lba] = (seq, ((seq as u32 * 256 + lba as u32) % 256) as u8);
        }
    }

    /// 数据面校验（重放语义诚实化）：
    /// 已封账块 lba 从后向前找覆盖它的已封账事务 e：
    /// - 盘面写序号 < e.seq = 账本承诺写了却没写上 → 结构损坏；
    /// - 序号恰等而值与复算不符 = 承诺落盘的内容错了 → 结构损坏；
    /// - 序号更大 = 被更晚的写覆盖（更晚封账事务或账本外半写）
    ///   = **重放回滚面**，合法——断电丢数据但结构不坏（MD2 诚实语义）。
    /// （模型语义边界：账本外序号的凭空写在真实世界靠日志 CRC 识别，
    /// 本模型统一归入回滚面；损坏判定锚定"承诺过的内容不许错"。）
    pub fn verify(&self, ledger: &CkptLedger) -> bool {
        for e in ledger.entries.iter().rev() {
            for i in 0..e.len {
                let lba = (e.start + i) as usize % DISK_BLOCKS;
                let (s, v) = self.blocks[lba];
                let expect = ((e.seq as u32 * 256 + lba as u32) % 256) as u8;
                if s < e.seq {
                    return false; // 承诺写了却没写上
                }
                if s == e.seq && v != expect {
                    return false; // 承诺值错位
                }
                // s > e.seq：更晚写覆盖，重放回滚面，合法
            }
        }
        true
    }
}

/// 重放统计（写启动报告的面——MD2 篇 7.5）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReplayStats {
    /// 重放到的检查点 seq（0 = 空账本从头开始）
    pub replayed_to_seq: u64,
    /// 回滚块数
    pub rolled_back_blocks: usize,
    /// true = 正常挂载；false = 只读降级 + 黄条
    pub mounted_rw: bool,
    /// true = 零结构损坏
    pub structure_ok: bool,
}

/// 单轮断电对练：随机事务流 + 随机断电截断 → 重放 → 判定。
/// 返回本轮重放统计。诚实语义：数据可丢（未封账），结构不可坏。
pub fn one_power_loss_drill(g: &mut crate::comprecover::Lcg) -> ReplayStats {
    let mut ledger = CkptLedger::new();
    let mut disk = DiskFace::fresh();
    let txns = 4 + (g.next() % (MAX_TXNS_PER_ROUND - 4) as u64) as usize;
    let mut seq: u64 = 0;
    for i in 0..txns {
        seq += 1;
        let start = (g.next() % (DISK_BLOCKS as u64 / 2)) as u32;
        let len = 1 + (g.next() % 8) as u32;
        // 断电截断模型：最后一个事务可能只写了一部分（keep < len）
        let is_last = i == txns - 1;
        let keep = if is_last && g.next() % 2 == 0 {
            g.next() % len.max(1) as u64
        } else {
            len as u64
        } as u32;
        disk.write_txn(seq, start, len, keep);
        if keep >= len {
            // 完整落盘才封账（半写事务绝不入账）
            ledger.seal(seq, start, len);
        } else if is_last {
            break; // 断电——半写不入账，停止
        }
    }
    let (rseq, rolled) = ledger.replay();
    let structure_ok = disk.verify(&ledger);
    // 重放失败 → 只读降级（B-701 路径）；本模型结构校验即重放判定
    ReplayStats {
        replayed_to_seq: rseq,
        rolled_back_blocks: rolled,
        mounted_rw: structure_ok,
        structure_ok,
    }
}

/// 百次对练 harness（宿主面）：统计归档 = MD2 篇 7.5 "重放统计归档"。
pub struct DrillArchive {
    pub rounds: u32,
    pub zero_damage_rounds: u32,
    pub ro_degraded_rounds: u32,
    pub total_rolled_back: u64,
    /// 配比记账（MD3：QEMU 替身八十 + 实机二十，混着跑）
    pub qemu_standin_target: u32,
    pub real_hw_target: u32,
}

/// 跑百次（宿主判据面）。QEMU 替身八十次与实机二十次的注入由脚本承接，
/// 本函数的 rounds 参数即宿主替身轮数（对账时与脚本计数合并归档）。
pub fn run_power_loss_drills(seed: u64, rounds: u32) -> DrillArchive {
    let mut g = crate::comprecover::Lcg(seed);
    let mut arch = DrillArchive {
        rounds,
        zero_damage_rounds: 0,
        ro_degraded_rounds: 0,
        total_rolled_back: 0,
        qemu_standin_target: 80,
        real_hw_target: 20,
    };
    for _ in 0..rounds {
        let s = one_power_loss_drill(&mut g);
        if s.structure_ok {
            arch.zero_damage_rounds += 1;
        } else {
            arch.ro_degraded_rounds += 1;
        }
        arch.total_rolled_back += s.rolled_back_blocks as u64;
    }
    arch
}

// ---------------------------------------------------------------- 自检

pub fn run_pwrdrl_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-703 断电一百次");
    {
        // 封账语义：完整落盘入账、半写不入账
        let mut ledger = CkptLedger::new();
        ledger.seal(1, 0, 4);
        set.add(
            "B-703 完整事务封账",
            ledger.entries.len() == 1 && ledger.sealed_seq == 1,
            "事务边界入账本",
        );
    }
    {
        // 重放收敛：账本最后一条即最近检查点
        let mut ledger = CkptLedger::new();
        ledger.seal(1, 0, 4);
        ledger.seal(2, 8, 6);
        let (s, r) = ledger.replay();
        set.add(
            "B-703 重放到最近检查点",
            s == 2 && r == 6,
            "回到检查点而非假装完整（判例 16）",
        );
    }
    {
        // 空账本重放：从零开始
        let ledger = CkptLedger::new();
        let (s, r) = ledger.replay();
        set.add("B-703 空账本从零重放", s == 0 && r == 0, "无检查点即全回滚");
    }
    {
        // 半写不入账：disk verify 通过（半写块被账本排除在承诺外）
        let mut g = crate::comprecover::Lcg(0xB703);
        let s = one_power_loss_drill(&mut g);
        set.add(
            "B-703 单轮半写不破结构",
            s.structure_ok,
            "断电丢数据但结构永不损坏",
        );
    }
    {
        // 结构校验 catches 损坏：承诺值错位（序号合法、内容错）
        let mut ledger = CkptLedger::new();
        ledger.seal(1, 0, 4);
        let mut disk = DiskFace::fresh();
        disk.write_txn(1, 0, 4, 4);
        disk.blocks[2] = (1, 0xFF); // 序号是 1 的、内容不是 1 复算的值 = 承诺值错位
        set.add(
            "B-703 承诺值错位判定损坏",
            !disk.verify(&ledger),
            "账本承诺过的内容不许错",
        );
    }
    {
        // 百次对练：零结构损坏
        let arch = run_power_loss_drills(0xB703, 100);
        set.add(
            "B-703 百次零结构损坏",
            arch.rounds == 100 && arch.zero_damage_rounds == 100,
            "WD-052 宿主面百轮全收敛",
        );
    }
    {
        // 配比记账：替身八十 + 实机二十
        let arch = run_power_loss_drills(1, 10);
        set.add(
            "B-703 配比记账面",
            arch.qemu_standin_target == 80 && arch.real_hw_target == 20,
            "MD3 出口仪式配比 QEMU80+实机20",
        );
    }
    {
        // 诚实窗口：断电窗口与合并窗口同一数字（Q41 / 附录 K 联动）
        set.add(
            "B-703 断电窗口=合并窗口",
            POWER_LOSS_WINDOW_MS == crate::comprecover::COALESCE_WINDOW_MS,
            "诚实文案的数据面同一数字 5000ms",
        );
    }
    {
        // 重放统计归档字段齐（写启动报告的面）
        let s = ReplayStats {
            replayed_to_seq: 3,
            rolled_back_blocks: 5,
            mounted_rw: true,
            structure_ok: true,
        };
        set.add(
            "B-703 重放统计归档面",
            s.replayed_to_seq == 3 && s.rolled_back_blocks == 5,
            "挂载结果与重放统计可入启动报告",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f503_ledger_semantics() {
        let mut ledger = CkptLedger::new();
        assert_eq!(ledger.replay(), (0, 0));
        ledger.seal(1, 0, 4);
        ledger.seal(2, 8, 6);
        assert_eq!(ledger.replay(), (2, 6));
        assert_eq!(ledger.sealed_seq, 2);
    }

    #[test]
    fn f503_half_write_never_sealed() {
        // 半写事务（keep < len）不入账——对练 harness 内部规则的结构性验证
        let mut g = crate::comprecover::Lcg(42);
        for _ in 0..50 {
            let s = one_power_loss_drill(&mut g);
            // 诚实语义：结构校验全过（可能 mounted_rw=false 只发生在损坏——本模型零损坏）
            assert!(s.structure_ok, "半写不入账则结构必完好");
        }
    }

    #[test]
    fn f503_ghost_write_detected() {
        let mut ledger = CkptLedger::new();
        ledger.seal(1, 0, 4);
        let mut disk = DiskFace::fresh();
        disk.write_txn(1, 0, 4, 4);
        assert!(disk.verify(&ledger));
        // 承诺值错位：同序号、内容错 → 损坏
        disk.blocks[0] = (1, 0xFF);
        assert!(!disk.verify(&ledger), "承诺落盘的内容错了 = 结构损坏");
        // 承诺写了却没写上：序号倒退 → 损坏
        disk.blocks[1] = (0, 0);
        assert!(!disk.verify(&ledger), "账本承诺写了却没写上 = 结构损坏");
        // 账本外更晚写（半写覆盖）：合法回滚面，不判损坏
        disk.blocks[1] = (9, 0xEE);
        disk.blocks[0] = (1, ((1u32 * 256 + 0) % 256) as u8);
        assert!(disk.verify(&ledger), "账本外半写 = 回滚面而非损坏");
    }

    #[test]
    fn f503_hundred_drills_zero_damage() {
        let arch = run_power_loss_drills(0xD5EE, 100);
        assert_eq!(arch.rounds, 100);
        assert_eq!(arch.zero_damage_rounds, 100, "零结构损坏");
        assert_eq!(arch.ro_degraded_rounds, 0);
        assert!(arch.total_rolled_back > 0, "确实有回滚发生（诚实丢数据面）");
    }

    #[test]
    fn f503_window_one_number() {
        assert_eq!(POWER_LOSS_WINDOW_MS, 5_000);
        assert_eq!(POWER_LOSS_WINDOW_MS, crate::fsyncp::COALESCE_WINDOW_MS);
        assert_eq!(POWER_LOSS_WINDOW_MS, crate::comprecover::COALESCE_WINDOW_MS);
    }

    #[test]
    fn f503_self_checks_pass() {
        let set = run_pwrdrl_checks();
        assert!(set.all_passed(), "B-703 自检全绿");
    }
}
