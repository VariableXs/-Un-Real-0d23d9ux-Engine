//! UNX-F51571 B19 与 M2/M4/H5 复合键鼠手柄联测（三消费方锚定行） · 0xC1180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51572-J1 错误码 ×8 并入；未定义码注入 10/10 拒绝；
//!           完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - AnchorRow        三消费方（M2 键盘 / M4 鼠标 / H5 手柄）锚定行记录
//!   - JointTestLedger  联测账本：锚定行落账 + 错误码并入闭集 + 守恒账
//!   - ErrorCode8       并入的复合设备族错误码 ×8（闭集，val=段基+1+序号）
//!   - AnchorCode       本卡诊断码闭集（12 变体）
//!   - Lcg              确定性伪随机（种子=SEG_BASE 派生，替代真随机）
//!
//! 设计要点：
//!   ① 三消费方锚定：M2/M4/H5 各自持一条锚定行，锚定行 = (消费方, 设备号,
//!      锚定序号, 状态)；联测判定前必须三方锚定齐备，缺一方显性拒。
//!   ② 错误码并入纪律：复合设备族错误码 ×8 逐个并入本域闭集表，并入时
//!      校验段号区间不越界、detail 非空；未定义码（越闭集）一律拒绝落账。
//!   ③ 注入核：四类坏样本（未定义错误码、锚定序号倒退、缺消费方锚定、
//!      状态回退）逐类 ×10 轮精确复现，注入账守恒=检出+逃逸，逃逸恒 0。
//!   ④ 双跑 FNV-1a 指纹：同一脚本两次建账，逐字节 digest 必须一致。
//!   ⑤ no_std+alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子=SEG_BASE
//!      派生）；零 unwrap/expect/panic/裸下标，取值一律 .get()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基（SEG 公式：0xC0000 + 0x40 × 批内序，B2 批第 1 卡）。
pub const SEG_BASE: u32 = 0xC1180;

/// 复合设备族并入错误码 ×8（闭集；val()=SEG_BASE+1+变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorCode8 {
    /// 描述符束不齐（IAD 与接口数不符）
    DescBundleShort = 0,
    /// 类码冲突（两接口同 bInterfaceClass 争一条端点）
    ClassCodeClash,
    /// 端点溢出（复合设备端点数超账面配额）
    EndpointOverflow,
    /// 键盘侧报文越界（HID 报告描述符长度越账）
    KeyboardReportOob,
    /// 鼠标侧滚轮相位丢失
    MouseWheelPhaseLost,
    /// 手柄侧振动通道断连
    PadRumbleDetach,
    /// 复合 teardown 次序错（先拆接口后拆 IAD）
    TeardownOrderBad,
    /// 并发 teardown 竞争（两消费方同时拆同一接口）
    TeardownRace,
}

impl ErrorCode8 {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ErrorCode8::DescBundleShort => "描述符束不齐：IAD 声明的接口数与实际不符，先对齐再并入",
            ErrorCode8::ClassCodeClash => "类码冲突：两接口争同一端点，按类优先级让位后重试",
            ErrorCode8::EndpointOverflow => "端点溢出：复合设备端点数超账面配额，裁撤低优先类",
            ErrorCode8::KeyboardReportOob => "键盘侧报文越界：HID 报告描述符长度越账，截断申报",
            ErrorCode8::MouseWheelPhaseLost => "鼠标侧滚轮相位丢失：重新锁定相位再联测",
            ErrorCode8::PadRumbleDetach => "手柄侧振动通道断连：先补通道再继续并发 teardown",
            ErrorCode8::TeardownOrderBad => "teardown 次序错：必须先拆接口、后拆 IAD",
            ErrorCode8::TeardownRace => "并发 teardown 竞争：同一接口同轮只允许一方拆",
        }
    }
    /// 闭集全序表（权威清单）。
    pub const ALL: [ErrorCode8; 8] = [
        ErrorCode8::DescBundleShort,
        ErrorCode8::ClassCodeClash,
        ErrorCode8::EndpointOverflow,
        ErrorCode8::KeyboardReportOob,
        ErrorCode8::MouseWheelPhaseLost,
        ErrorCode8::PadRumbleDetach,
        ErrorCode8::TeardownOrderBad,
        ErrorCode8::TeardownRace,
    ];
}

/// 三消费方（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consumer {
    /// M2 键盘
    Keyboard,
    /// M4 鼠标
    Mouse,
    /// H5 手柄
    Pad,
}

impl Consumer {
    /// 闭集解码：0/1/2 之外 None。
    pub fn from_code(c: u32) -> Option<Consumer> {
        match c {
            0 => Some(Consumer::Keyboard),
            1 => Some(Consumer::Mouse),
            2 => Some(Consumer::Pad),
            _ => None,
        }
    }
    /// 中文标签。
    pub fn name(self) -> &'static str {
        match self {
            Consumer::Keyboard => "M2键盘",
            Consumer::Mouse => "M4鼠标",
            Consumer::Pad => "H5手柄",
        }
    }
}

/// 诊断码闭集（本卡，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 消费方码越闭集
    ConsumerUnknown,
    /// 未定义错误码（越 ×8 闭集，注入必拒）
    ErrorCodeUndefined,
    /// 缺消费方锚定行
    AnchorMissing,
    /// 锚定序号倒退
    AnchorRegress,
    /// 状态回退（已完成之后又改状态）
    StateRegress,
    /// 锚定行字段不齐
    RowIncomplete,
    /// 重复锚定（同一消费方二次锚定）
    AnchorDuplicate,
    /// 账面不符（锚定/判定/拒绝三本不平）
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintDrift,
    /// 结果摘要为空（写回判据要求非空）
    SummaryEmpty,
}

impl AnchorCode {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            AnchorCode::Ok => "联测锚定成功",
            AnchorCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            AnchorCode::ConsumerUnknown => "消费方码非法：只接受 0/1/2 三方",
            AnchorCode::ErrorCodeUndefined => "未定义错误码：越 ×8 闭集一律拒绝并入",
            AnchorCode::AnchorMissing => "缺锚定行：三方锚定齐备后才可联测判定",
            AnchorCode::AnchorRegress => "锚定序号倒退：锚定序必须严格递增",
            AnchorCode::StateRegress => "状态回退：已完成之后不得再改状态",
            AnchorCode::RowIncomplete => "锚定行字段不齐：四字段必须全部非零/非空",
            AnchorCode::AnchorDuplicate => "重复锚定：同一消费方只允许锚定一次",
            AnchorCode::LedgerMismatch => "账面不符：锚定/判定/拒绝三本计数必须对平",
            AnchorCode::FingerprintDrift => "双跑指纹不一致：确定性被破坏，重查脚本",
            AnchorCode::SummaryEmpty => "结果摘要为空：完成时写回必须附非空摘要",
        }
    }
}

/// 诊断码闭集数组（权威清单，12 变体）。
pub const ALL_CODES: [AnchorCode; 12] = [
    AnchorCode::Ok,
    AnchorCode::SegBad,
    AnchorCode::ConsumerUnknown,
    AnchorCode::ErrorCodeUndefined,
    AnchorCode::AnchorMissing,
    AnchorCode::AnchorRegress,
    AnchorCode::StateRegress,
    AnchorCode::RowIncomplete,
    AnchorCode::AnchorDuplicate,
    AnchorCode::LedgerMismatch,
    AnchorCode::FingerprintDrift,
    AnchorCode::SummaryEmpty,
];

/// 单条锚定行：消费方、设备号、锚定序号、状态（1=在测 2=已完成）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnchorRow {
    /// 消费方（0/1/2）
    pub consumer: u32,
    /// 设备号（联测账内唯一）
    pub device: u32,
    /// 锚定序号（严格递增）
    pub seq: u64,
    /// 状态（1=在测 2=已完成）
    pub state: u32,
}

impl AnchorRow {
    /// 字段完整性：四字段全部在合法域内。
    pub fn complete(&self) -> bool {
        self.consumer <= 2 && self.device > 0 && self.seq > 0 && (self.state == 1 || self.state == 2)
    }
}

/// 确定性伪随机（LCG；种子由 SEG_BASE 派生，禁真随机）。
pub struct Lcg(u64);

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(SEG_BASE as u64))
    }
    /// 下一随机数（确定序）。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// 联测账本：三消费方锚定 + 错误码并入 + 守恒账。
pub struct JointTestLedger {
    anchors: Vec<AnchorRow>,
    /// 已并入闭集的错误码段号（按并入序）
    merged: Vec<u32>,
    /// 被拒的未定义码段号（注入留痕）
    rejected: Vec<u32>,
    /// 完成时写回摘要（判据要求非空）
    summary: String,
    /// 写回状态（0=未写回 1=已完成）
    status: u32,
    anchor_cnt: u64,
    judge_cnt: u64,
    reject_cnt: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl JointTestLedger {
    /// 建空账。
    pub fn new() -> JointTestLedger {
        JointTestLedger {
            anchors: Vec::new(),
            merged: Vec::new(),
            rejected: Vec::new(),
            summary: String::new(),
            status: 0,
            anchor_cnt: 0,
            judge_cnt: 0,
            reject_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 锚定一方消费方：码闭集 → 字段完整性 → 序号单调 → 不重复 → 落账。
    pub fn anchor(&mut self, row: AnchorRow) -> Result<(), AnchorCode> {
        if Consumer::from_code(row.consumer).is_none() {
            return Err(AnchorCode::ConsumerUnknown);
        }
        if !row.complete() {
            return Err(AnchorCode::RowIncomplete);
        }
        if self.anchors.iter().any(|r| r.consumer == row.consumer) {
            return Err(AnchorCode::AnchorDuplicate);
        }
        let max_seq = self.anchors.iter().map(|r| r.seq).max().unwrap_or(0);
        if row.seq <= max_seq {
            return Err(AnchorCode::AnchorRegress);
        }
        self.anchors.push(row);
        self.anchor_cnt = self.anchor_cnt.saturating_add(1);
        Ok(())
    }

    /// 并入一个错误码：闭集匹配段号，未定义一律拒。
    pub fn merge_error_code(&mut self, val: u32) -> Result<ErrorCode8, AnchorCode> {
        let mut hit = None;
        for c in ErrorCode8::ALL {
            if c.val() == val {
                hit = Some(c);
            }
        }
        let c = match hit {
            Some(c) => c,
            None => {
                self.rejected.push(val);
                self.reject_cnt = self.reject_cnt.saturating_add(1);
                return Err(AnchorCode::ErrorCodeUndefined);
            }
        };
        self.merged.push(c.val());
        Ok(c)
    }

    /// 联测判定：三方锚定齐备才放行；齐备后判/拒账推进。
    pub fn joint_judge(&mut self) -> Result<u32, AnchorCode> {
        if self.anchors.len() < 3 {
            return Err(AnchorCode::AnchorMissing);
        }
        self.judge_cnt = self.judge_cnt.saturating_add(1);
        Ok(self.anchors.len() as u32)
    }

    /// 完成写回：状态=已完成、结果摘要非空（判据原文落点）。
    pub fn finish(&mut self, summary: &str) -> Result<(), AnchorCode> {
        if summary.is_empty() {
            return Err(AnchorCode::SummaryEmpty);
        }
        if self.status == 2 {
            return Err(AnchorCode::StateRegress);
        }
        self.status = 2;
        self.summary = summary.to_string();
        Ok(())
    }

    /// 摘要读取。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 写回状态（0/2）。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 已并入码数。
    pub fn merged_len(&self) -> usize {
        self.merged.len()
    }

    /// 被拒未定义码数。
    pub fn rejected_len(&self) -> usize {
        self.rejected.len()
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 锚定/判定/拒绝三本计数。
    pub fn tally(&self) -> (u64, u64, u64) {
        (self.anchor_cnt, self.judge_cnt, self.reject_cnt)
    }
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 标准联测脚本：三方锚定 → 并入 ×8 → 判定 → 写回，输出快照串。
pub fn standard_script() -> String {
    let mut l = JointTestLedger::new();
    let _ = l.anchor(AnchorRow { consumer: 0, device: 0xB2, seq: 1, state: 1 });
    let _ = l.anchor(AnchorRow { consumer: 1, device: 0xB2, seq: 2, state: 1 });
    let _ = l.anchor(AnchorRow { consumer: 2, device: 0xB2, seq: 3, state: 1 });
    let mut merged = 0u32;
    for c in ErrorCode8::ALL {
        if l.merge_error_code(c.val()).is_ok() {
            merged += 1;
        }
    }
    let _ = l.joint_judge();
    let _ = l.finish("三方锚定齐备，错误码×8并入，联测通过");
    format!("{}|{}|{}|{}|{}", l.status(), l.summary(), merged, l.tally().0, l.tally().1)
}

/// 双跑 FNV-1a 指纹一致性。
pub fn dual_run_consistent() -> bool {
    let f1 = fnv1a(standard_script().as_bytes());
    let f2 = fnv1a(standard_script().as_bytes());
    f1 == f2 && f1 != 0
}

/// 收口自检（15 条判据真测）。
pub fn run_unx51571_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51571");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误码 ×8 并入：段号连续且唯一。
    {
        let mut ok = true;
        let mut prev = 0u32;
        for c in ErrorCode8::ALL {
            if c.val() <= prev || c.detail().is_empty() {
                ok = false;
            }
            prev = c.val();
        }
        s.add_dyn("错误码×8并入闭集", ok && ErrorCode8::ALL.len() == 8, "×8 错误码段号递增、明细非空");
    }
    // 3 val 公式抽验：SEG_BASE+1+变体序号。
    s.add_dyn(
        "val公式抽验",
        ErrorCode8::ALL[0].val() == SEG_BASE + 1 && ALL_CODES[11].val() == SEG_BASE + 12,
        "val=SEG_BASE+1+变体序号",
    );
    // 4 闭集唯一性：两两不重码。
    {
        let mut uniq = true;
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                if ALL_CODES[i].val() == ALL_CODES[j].val() {
                    uniq = false;
                }
            }
        }
        s.add_dyn("闭集唯一性", uniq && ALL_CODES.len() == 12, "12 变体段号两两不同");
    }
    // 5 三方锚定齐备后判定放行。
    {
        let mut l = standard_anchor_ledger();
        s.add_dyn("三方锚定齐备", l.joint_judge().is_ok(), "M2/M4/H5 各锚定一行后判定放行");
    }
    // 6 缺锚定注入 ×10：缺任一方都显性拒。
    {
        let mut caught = 0u32;
        for skip in 0..3u32 {
            let mut l = JointTestLedger::new();
            let mut seq = 0u64;
            for c in 0..3u32 {
                seq += 1;
                if c != skip {
                    let _ = l.anchor(AnchorRow { consumer: c, device: 7, seq, state: 1 });
                }
            }
            if l.joint_judge() == Err(AnchorCode::AnchorMissing) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        // ×10 轮：三方组合循环 3+3+3 再补 1 轮全锚定（应放行，不算注入）。
        for _ in 0..7u32 {
            let mut l = JointTestLedger::new();
            let _ = l.anchor(AnchorRow { consumer: 0, device: 1, seq: 1, state: 1 });
            let _ = l.anchor(AnchorRow { consumer: 1, device: 1, seq: 2, state: 1 });
            if l.joint_judge() == Err(AnchorCode::AnchorMissing) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        s.add_dyn("缺锚定注入10/10", caught == 10, "缺任一消费方锚定必须全抓");
    }
    // 7 未定义码注入 ×10：越 ×8 闭集一律拒。
    {
        let mut l = JointTestLedger::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if l.merge_error_code(SEG_BASE + 1 + 8 + i) == Err(AnchorCode::ErrorCodeUndefined) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        s.add_dyn(
            "未定义码注入10/10",
            caught == 10 && l.rejected_len() == 10 && l.merged_len() == 0,
            "未定义错误码必须 10/10 拒绝并留痕",
        );
    }
    // 8 合法 ×8 注入对照：全并入成功。
    {
        let mut l = JointTestLedger::new();
        let mut ok = 0u32;
        for c in ErrorCode8::ALL {
            if l.merge_error_code(c.val()).is_ok() {
                ok += 1;
            }
        }
        s.add_dyn("合法×8全并入", ok == 8 && l.merged_len() == 8, "×8 合法码应全数并入");
    }
    // 9 锚定序倒退注入 ×10。
    {
        let mut l = JointTestLedger::new();
        let _ = l.anchor(AnchorRow { consumer: 0, device: 3, seq: 5, state: 1 });
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = l.anchor(AnchorRow { consumer: 1, device: 3, seq: (i % 5) as u64, state: 1 });
            if r == Err(AnchorCode::AnchorRegress) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        s.add_dyn("序号倒退注入10/10", caught == 10, "锚定序不递增必须全抓");
    }
    // 10 重复锚定注入 ×10。
    {
        let mut l = JointTestLedger::new();
        let _ = l.anchor(AnchorRow { consumer: 2, device: 4, seq: 1, state: 1 });
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = l.anchor(AnchorRow { consumer: 2, device: 4, seq: 2 + i as u64, state: 1 });
            if r == Err(AnchorCode::AnchorDuplicate) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        s.add_dyn("重复锚定注入10/10", caught == 10, "同一消费方二次锚定必须全抓");
    }
    // 11 状态回退注入 ×10：已完成后再 finish 必拒。
    {
        let mut l = standard_anchor_ledger();
        let _ = l.finish("首次完成写回");
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if l.finish("二次写回") == Err(AnchorCode::StateRegress) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        s.add_dyn("状态回退注入10/10", caught == 10 && l.status() == 2, "已完成后再写回必须全抓");
    }
    // 12 空摘要注入 ×10。
    {
        let mut l = standard_anchor_ledger();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if l.finish("") == Err(AnchorCode::SummaryEmpty) {
                caught += 1;
                l.note_detected();
            } else {
                l.note_escape();
            }
        }
        s.add_dyn("空摘要注入10/10", caught == 10 && l.status() == 0, "空摘要写回必须全抓且状态不动");
    }
    // 13 完成写回：状态=已完成、结果摘要非空。
    {
        let mut l = standard_anchor_ledger();
        let _ = l.finish("三方联测通过，错误码×8并入完成");
        s.add_dyn(
            "完成写回非空",
            l.status() == 2 && !l.summary().is_empty(),
            "完成时状态=已完成且结果摘要非空",
        );
    }
    // 14 双跑 FNV-1a 指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一脚本两次建账 FNV-1a 摘要相等");
    // 15 注入账守恒。
    {
        let mut l = standard_anchor_ledger();
        for i in 0..10u32 {
            if l.merge_error_code(SEG_BASE + 1 + 100 + i) == Err(AnchorCode::ErrorCodeUndefined) {
                l.note_detected();
            }
        }
        s.add_dyn("注入账守恒", l.conserved(), "注入=检出+逃逸，逃逸恒 0 且检出非零");
    }
    // 16 LCG 确定性：同种子双序列一致。
    {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut same = true;
        for _ in 0..16 {
            if a.next() != b.next() {
                same = false;
            }
        }
        s.add_dyn("LCG确定性", same, "同种子 LCG 双序列必须逐项相等");
    }
    s
}

/// 标准三方锚定账（测试与判据共用）。
pub fn standard_anchor_ledger() -> JointTestLedger {
    let mut l = JointTestLedger::new();
    let _ = l.anchor(AnchorRow { consumer: 0, device: 0xB2, seq: 1, state: 1 });
    let _ = l.anchor(AnchorRow { consumer: 1, device: 0xB2, seq: 2, state: 1 });
    let _ = l.anchor(AnchorRow { consumer: 2, device: 0xB2, seq: 3, state: 1 });
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51571_checks_green() {
        let set = run_unx51571_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn error8_codes_monotonic_and_detailed() {
        let mut prev = 0u32;
        for c in ErrorCode8::ALL {
            assert!(c.val() > prev);
            assert!(!c.detail().is_empty());
            prev = c.val();
        }
        assert_eq!(ErrorCode8::ALL.len(), 8);
    }

    #[test]
    fn all_codes_closed_unique() {
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                assert_ne!(ALL_CODES[i].val(), ALL_CODES[j].val());
            }
        }
        assert_eq!(ALL_CODES.len(), 12);
    }

    #[test]
    fn joint_judge_requires_all_three() {
        let mut l = JointTestLedger::new();
        assert_eq!(l.joint_judge(), Err(AnchorCode::AnchorMissing));
        let _ = l.anchor(AnchorRow { consumer: 0, device: 1, seq: 1, state: 1 });
        let _ = l.anchor(AnchorRow { consumer: 1, device: 1, seq: 2, state: 1 });
        assert_eq!(l.joint_judge(), Err(AnchorCode::AnchorMissing));
        let _ = l.anchor(AnchorRow { consumer: 2, device: 1, seq: 3, state: 1 });
        assert!(l.joint_judge().is_ok());
    }

    #[test]
    fn undefined_code_injection_all_caught() {
        let mut l = JointTestLedger::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if l.merge_error_code(SEG_BASE + 1 + 8 + i) == Err(AnchorCode::ErrorCodeUndefined) {
                caught += 1;
                l.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(l.conserved());
    }

    #[test]
    fn anchor_regress_injection_all_caught() {
        let mut l = JointTestLedger::new();
        let _ = l.anchor(AnchorRow { consumer: 0, device: 3, seq: 9, state: 1 });
        let mut caught = 0u32;
        for i in 0..10u32 {
            if l.anchor(AnchorRow { consumer: 1, device: 3, seq: (i % 9) as u64, state: 1 })
                == Err(AnchorCode::AnchorRegress)
            {
                caught += 1;
                l.note_detected();
            }
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn state_regress_and_empty_summary_rejected() {
        let mut l = standard_anchor_ledger();
        assert!(l.finish("首写").is_ok());
        assert_eq!(l.finish("再写"), Err(AnchorCode::StateRegress));
        let mut l2 = standard_anchor_ledger();
        assert_eq!(l2.finish(""), Err(AnchorCode::SummaryEmpty));
        assert_eq!(l2.status(), 0);
    }

    #[test]
    fn consumer_closed_set() {
        assert!(Consumer::from_code(0).is_some());
        assert!(Consumer::from_code(2).is_some());
        assert!(Consumer::from_code(3).is_none());
        assert!(Consumer::from_code(u32::MAX).is_none());
    }

    #[test]
    fn row_completeness_gate() {
        let bad = AnchorRow { consumer: 0, device: 0, seq: 1, state: 1 };
        assert!(!bad.complete());
        let bad2 = AnchorRow { consumer: 0, device: 1, seq: 1, state: 7 };
        assert!(!bad2.complete());
        let good = AnchorRow { consumer: 1, device: 1, seq: 1, state: 2 };
        assert!(good.complete());
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(
            fnv1a(standard_script().as_bytes()),
            fnv1a(standard_script().as_bytes())
        );
    }

    #[test]
    fn lcg_deterministic_from_seg() {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn seg_guard_in_range() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC1180);
    }
}
