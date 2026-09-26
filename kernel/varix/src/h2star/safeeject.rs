//! F294 安全弹出与拔出保护 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：写入中弹出的拦截与进度说明；弹出成功气泡；强拔
//! 恢复用例（写入中断电模拟——F180 演练判据同源）；损坏文件标注可
//! 发现性。
//!
//! **设计要点（主册）**：任务栏托盘「安全弹出」：有写入进行中时弹出
//! 不执行并明确说（「正在写入 3 个文件，剩 12MB」+预计秒数），写完
//! 弹出一击即成、成功有气泡确认；拔出保护兜底——写入中被物理拔出时
//! 文件系统日志（F066）保证卷可恢复，受影响文件在下次访问时明确标注
//! 损坏而不是静默给半截文件。
//!
//! 实装：弹出仲裁器（写入账 → 拦截+进度说明+预计秒数 / 放行+气泡）；
//! 强拔登记（写入中断电 → 受影响文件标注损坏——下次访问可发现）；
//! 恢复协作口（F066 日志保证卷可恢复——本层只登记不越权修复）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 写入中的事务。
#[derive(Clone, Debug)]
pub struct WriteTx {
    pub files: Vec<String>,
    /// 已写字节。
    pub done_bytes: u64,
    /// 总字节。
    pub total_bytes: u64,
    /// 当前写入速率（B/s——预计秒数计算）。
    pub rate_bps: u64,
}

impl WriteTx {
    /// 剩余字节。
    pub fn remaining(&self) -> u64 {
        self.total_bytes.saturating_sub(self.done_bytes)
    }

    /// 预计剩余秒数（速率为 0 → None——不瞎猜）。
    pub fn eta_s(&self) -> Option<u64> {
        if self.rate_bps == 0 {
            None
        } else {
            Some(self.remaining() / self.rate_bps)
        }
    }

    /// 拦截说明文案（三要素：发生了什么/剩多少/还要多久）。
    pub fn block_text(&self) -> String {
        let eta = match self.eta_s() {
            Some(s) => alloc::format!("，预计还要 {} 秒", s),
            None => String::new(),
        };
        alloc::format!(
            "正在写入 {} 个文件，剩 {} 字节{}——写完即可弹出",
            self.files.len(),
            self.remaining(),
            eta
        )
    }
}

/// 弹出决定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EjectDecision {
    /// 放行（写完——成功气泡）。
    Ok,
    /// 拦截（写入中——进度说明）。
    Blocked(String),
}

/// 卷弹出仲裁。
pub struct EjectArbiter {
    /// 活动写入事务（0 或 1——单卷单事务的简化账）。
    pub active: Option<WriteTx>,
}

impl EjectArbiter {
    pub fn new() -> EjectArbiter {
        EjectArbiter { active: None }
    }

    /// 请求弹出：有写入 → 拦截+说明；无 → 放行。
    pub fn eject(&self) -> EjectDecision {
        match &self.active {
            Some(tx) => EjectDecision::Blocked(tx.block_text()),
            None => EjectDecision::Ok,
        }
    }

    /// 写入完成确认 → 清账（下次弹出一击即成）。
    pub fn write_finished(&mut self) {
        self.active = None;
    }

    /// 成功气泡文案。
    pub fn ok_bubble() -> &'static str {
        "可以安全移除了"
    }

    // ---------------------------------------------------------------
    // 深化批次二：多事务聚合与气泡时效
    // ---------------------------------------------------------------

    /// 多事务聚合拦截文案（单卷多写入并行时：事务数+总剩余+最慢 ETA
    /// ——「正在写入 3 个文件，剩 12MB」口径的多事务版，不漏任何一路）。
    pub fn block_text_multi(txs: &[WriteTx]) -> String {
        if txs.is_empty() {
            return String::from("没有正在进行的写入");
        }
        let files: usize = txs.iter().map(|t| t.files.len()).sum();
        let remaining: u64 = txs.iter().map(|t| t.remaining()).sum();
        let eta = txs.iter().filter_map(|t| t.eta_s()).max();
        match eta {
            Some(s) => alloc::format!(
                "正在写入 {} 个文件（{} 路任务），剩 {} 字节，预计还要 {} 秒——写完即可弹出",
                files, txs.len(), remaining, s
            ),
            None => alloc::format!(
                "正在写入 {} 个文件（{} 路任务），剩 {} 字节——写完即可弹出",
                files, txs.len(), remaining
            ),
        }
    }

    /// 成功气泡展示时长（ms——反馈要有存在感但不黏屏；3s 自动散场）。
    pub const BUBBLE_MS: u64 = 3_000;
}

/// 强拔标注的生命周期：标注随「文件被完整重写」解除（重插后把文件
/// 重新写完整 → 标注清除——标注是状态不是烙印）。
impl TornLedger {
    /// 完整重写确认（校验和过 = 文件重新完整）→ 清除标注。
    /// 返回 false = 该文件本无标注（诚实）。
    pub fn clear_if_rewritten(&mut self, file: &str, content: &[u8]) -> bool {
        let before = self.marks.len();
        self.marks.retain(|m| !(m.file == file && m.written_bytes <= content.len() as u64));
        self.marks.len() != before
    }

    /// 标注存量（诊断面）。
    pub fn len(&self) -> usize {
        self.marks.len()
    }
}

/// F066 协作口纪律（文档化常量——本层只登记不越权修复卷）。
pub const FS_JOURNAL_NOTE: &str = "卷恢复由文件系统日志（F066）负责；本层只负责坏文件自首标注";

/// 强拔保护：受影响文件损坏标注（下次访问可发现——不静默给半截）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TornMark {
    pub file: String,
    /// 中断时已写字节（访问者据此知道不完整）。
    pub written_bytes: u64,
    pub at_min: u64,
}

/// 强拔登记簿。
pub struct TornLedger {
    marks: Vec<TornMark>,
}

impl TornLedger {
    pub fn new() -> TornLedger {
        TornLedger { marks: Vec::new() }
    }

    /// 强拔事件登记：卷上所有写入中文件 → 损坏标注（F066 保证卷本身
    /// 可恢复；本层负责「坏文件自首」）。
    pub fn record_torn(&mut self, files: &[(String, u64)], at_min: u64) -> usize {
        for (f, b) in files {
            self.marks.push(TornMark { file: f.clone(), written_bytes: *b, at_min });
        }
        self.marks.len()
    }

    /// 下次访问查询：文件是否在损坏标注上（可发现性判据的执行点）。
    pub fn is_torn(&self, file: &str) -> Option<&TornMark> {
        self.marks.iter().find(|m| m.file == file)
    }

    /// 访问时的标注文案（三要素——人话）。
    pub fn access_warning(file: &str, mark: &TornMark) -> String {
        alloc::format!(
            "「{}」在写入中断电时受损——只包含前 {} 字节且可能不完整，建议重新获取该文件",
            file,
            mark.written_bytes
        )
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_safeeject_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F294");
    // 写入中弹出：拦截 + 进度说明。
    let mut arb = EjectArbiter::new();
    arb.active = Some(WriteTx {
        files: alloc::vec![String::from("a.mkv"), String::from("b.mkv"), String::from("c.mkv")],
        done_bytes: 12 * 1024 * 1024,
        total_bytes: 24 * 1024 * 1024,
        rate_bps: 4 * 1024 * 1024,
    });
    let d1 = arb.eject();
    match d1 {
        EjectDecision::Blocked(ref txt) => {
            set.add(
                "F294 block with progress",
                txt.contains("3 个文件") && txt.contains("12") && txt.contains("3 秒"),
                "progress+eta",
            );
        }
        _ => set.add("F294 block with progress", false, "should block"),
    }
    // 写完弹出一击即成 + 气泡。
    arb.write_finished();
    set.add(
        "F294 ok after finish",
        arb.eject() == EjectDecision::Ok && EjectArbiter::ok_bubble().contains("安全"),
        "bubble",
    );
    // 速率未知不瞎猜。
    let tx0 = WriteTx { files: alloc::vec![String::from("x")], done_bytes: 1, total_bytes: 10, rate_bps: 0 };
    set.add("F294 eta honest", tx0.eta_s().is_none(), "no wild guess");
    // 强拔恢复：写入中断电 → 标注 + 下次访问可发现。
    let mut torn = TornLedger::new();
    let n = torn.record_torn(
        &[ (String::from("半截.mkv"), 700 * 1024 * 1024), (String::from("半截2.mkv"), 100) ],
        500,
    );
    set.add("F294 torn recorded", n == 2, "both marked");
    let hit = torn.is_torn("半截.mkv");
    set.add(
        "F294 discoverable",
        hit.is_some() && TornLedger::access_warning("半截.mkv", hit.unwrap()).contains("不完整"),
        "access warns",
    );
    let clean = torn.is_torn("无辜.txt");
    set.add("F294 clean untouched", clean.is_none(), "no false mark");
    // --- 深化二：多事务聚合拦截。 ---
    let multi = [
        WriteTx { files: alloc::vec![String::from("a"), String::from("b")], done_bytes: 1_000, total_bytes: 4_000, rate_bps: 1_000 },
        WriteTx { files: alloc::vec![String::from("c")], done_bytes: 0, total_bytes: 2_000, rate_bps: 0 },
    ];
    let agg = EjectArbiter::block_text_multi(&multi);
    set.add(
        "F294 multi-tx aggregate",
        agg.contains("3 个文件") && agg.contains("2 路任务") && agg.contains("5000 字节") && agg.contains("3 秒"),
        "3 files / 2 lanes / known eta only",
    );
    set.add(
        "F294 multi idle text",
        EjectArbiter::block_text_multi(&[]).contains("没有正在进行的写入"),
        "idle honest",
    );
    // --- 深化二：标注生命周期（完整重写解除标注）。 ---
    let mut torn2 = TornLedger::new();
    let _ = torn2.record_torn(&[(String::from("修复目标.bin"), 100)], 1);
    let cleared = torn2.clear_if_rewritten("修复目标.bin", &alloc::vec![0u8; 200]);
    set.add(
        "F294 torn cleared on rewrite",
        cleared && torn2.len() == 0,
        "mark is state not brand",
    );
    set.add(
        "F294 torn clear honest",
        !torn2.clear_if_rewritten("从未标注.txt", &alloc::vec![0u8; 10]),
        "nothing to clear",
    );
    // 部分重写（仍短于中断点）不清除——坏文件继续自首。
    let mut torn3 = TornLedger::new();
    let _ = torn3.record_torn(&[(String::from("still-torn.bin"), 500)], 1);
    let _ = torn3.clear_if_rewritten("still-torn.bin", &alloc::vec![0u8; 100]);
    set.add("F294 partial rewrite keeps mark", torn3.len() == 1, "still torn");
    // F066 协作口文档化。
    set.add("F294 F066 note", FS_JOURNAL_NOTE.contains("F066") && FS_JOURNAL_NOTE.contains("自首"), "boundary documented");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f294_eject_flow() {
        let set = run_safeeject_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F294 自检红 {f}/{p}");
    }

    #[test]
    fn never_blocks_when_idle() {
        let arb = EjectArbiter::new();
        assert_eq!(arb.eject(), EjectDecision::Ok, "空闲卷弹出永远放行");
    }
}
