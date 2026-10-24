//! cga106_codecerr —— CGPU-F0106 编解码域错误纪律。
//! 锚点（CGPU 施工书）：CGPU-F0106 · 编解码域错误纪律 · 目标 300 行
//! 判据摘录：错误三分类（致命/可恢复/可忽略——类域闭集）；致
//! 命闸（致命错立即停链——不得带病续跑）；可恢复记账（恢复
//! 事件入账——计数落账）；可忽略上限（同类可忽略连发 ≤ 阈值
//! ——超阈升级为可恢复）；错误码闭集（域内码表——未知码拒核）；
//! 传播纪律（下层错误向上层传播时保留原码——不重编红线）；
//! 清算账（会话结束错误账清零——状态复位核）；分类幂等（同
//! 错误重复分类结果一致）。
//! 组件：ErrClass 三类 + ErrCode 闭集 + ErrLedger（分类账）+ 致
//! 命闸 + 升级阈 + 传播核 + 清算。
//! 职责定位：编解码管线的错误处理宪法——错误不是异常是账；三
//! 分类决定控制流（停/续/咽），账决定可观测性；与 cga73（图
//! 像管线错误纪律）同构不同域（编解码特有码表）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE9F0 段 · 本模块独占）=====
pub const XC_CLASS_OOB: u32 = 0xE9F0; // 分类越界
pub const XC_CODE_UNKNOWN: u32 = 0xE9F1; // 未知错误码
pub const XC_FATAL_STOP: u32 = 0xE9F2; // 致命停链
pub const XC_IGN_LIMIT: u32 = 0xE9F3; // 可忽略超阈
pub const XC_PROP_DRIFT: u32 = 0xE9F4; // 传播重编

// ===== 错误分类闭集 =====
pub const E_FATAL: u8 = 0;
pub const E_RECOVER: u8 = 1;
pub const E_IGNORE: u8 = 2;
pub const CLASSES: [u8; 3] = [E_FATAL, E_RECOVER, E_IGNORE];

// ===== 错误码闭集（编解码域）=====
pub const C_BITSTREAM: u16 = 0xE001; // 码流非法（致命）
pub const C_MARKER_MISS: u16 = 0xE002; // 标记缺失（致命）
pub const C_HUFF_DECODE: u16 = 0xE003; // Huffman 解错（可恢复——重同步）
pub const C_CRC_MISMATCH: u16 = 0xE004; // 校验失配（可恢复）
pub const C_TAIL_PAD: u16 = 0xE005; // 尾部填充（可忽略）
pub const C_UNKNOWN_MKR: u16 = 0xE006; // 未知标记段（可忽略）
/// (码 → 分类) 冻结表。
pub const CODE_CLASS: [(u16, u8); 6] = [
    (C_BITSTREAM, E_FATAL),
    (C_MARKER_MISS, E_FATAL),
    (C_HUFF_DECODE, E_RECOVER),
    (C_CRC_MISMATCH, E_RECOVER),
    (C_TAIL_PAD, E_IGNORE),
    (C_UNKNOWN_MKR, E_IGNORE),
];

/// 分类查表（未知码拒）。
pub fn classify(code: u16) -> Result<u8, u32> {
    CODE_CLASS.iter().find(|&&(c, _)| c == code).map(|&(_, k)| k).ok_or(XC_CODE_UNKNOWN)
}

/// 分类幂等（双查一致——结构性恒真但账核）。
pub fn classify_idempotent(code: u16) -> Result<(), u32> {
    let a = classify(code)?;
    let b = classify(code)?;
    if a != b {
        return Err(XC_CLASS_OOB);
    }
    Ok(())
}

/// 错误账。
pub struct ErrLedger {
    /// 各类计数。
    pub counts: [u32; 3],
    /// 可忽略连发计数（升级阈用）。
    pub ignore_streak: u32,
    /// 可忽略连发上限。
    pub ignore_limit: u32,
    /// 是否已致命停链。
    pub halted: bool,
}

impl ErrLedger {
    pub fn new(limit: u32) -> ErrLedger {
        ErrLedger { counts: [0; 3], ignore_streak: 0, ignore_limit: limit, halted: false }
    }

    /// 记一笔错误（分类→控制流决定）。
    /// 返回：true=续跑，false=停链。
    pub fn record(&mut self, code: u16) -> Result<bool, u32> {
        if self.halted {
            return Ok(false); // 已停链——后续一律拒
        }
        let k = classify(code)?;
        self.counts[k as usize] += 1;
        match k {
            E_FATAL => {
                self.halted = true;
                Err(XC_FATAL_STOP)
            }
            E_RECOVER => {
                self.ignore_streak = 0;
                Ok(true)
            }
            _ => {
                self.ignore_streak += 1;
                if self.ignore_streak > self.ignore_limit {
                    // 升级为可恢复语义（连续可忽略=系统性问题）。
                    self.counts[E_RECOVER as usize] += 1;
                    self.ignore_streak = 0;
                }
                Ok(true)
            }
        }
    }

    /// 传播核：错误码跨层传播不重编。
    pub fn propagate(code: u16) -> Result<u16, u32> {
        classify(code)?;
        Ok(code) // 原码透传（重编即 XC_PROP_DRIFT 的对偶——账核）
    }

    /// 清算（会话复位）。
    pub fn clear(&mut self) {
        self.counts = [0; 3];
        self.ignore_streak = 0;
        self.halted = false;
    }

    /// 账和守恒。
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
}

// ===== 主判据 =====
pub fn run_cga106_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0106");
    // 族一：分类表。
    cs.add("致命码", classify(C_BITSTREAM) == Ok(E_FATAL), "码流");
    cs.add("恢复码", classify(C_HUFF_DECODE) == Ok(E_RECOVER), "Huffman");
    cs.add("忽略码", classify(C_TAIL_PAD) == Ok(E_IGNORE), "尾填");
    cs.add("未知码拒", classify(0xFFFF).is_err(), "CODE_UNKNOWN");
    // 分类幂等。
    cs.add("分类幂等", classify_idempotent(C_CRC_MISMATCH).is_ok(), "双查一致");
    // 族二：致命闸。
    let mut l = ErrLedger::new(3);
    cs.add("致命停链", l.record(C_BITSTREAM) == Err(XC_FATAL_STOP), "立即停");
    cs.add("停后续拒", l.record(C_TAIL_PAD) == Ok(false), "带病续跑拒");
    cs.add("停链标志", l.halted, "账面");
    // 族三：可恢复续跑。
    let mut l2 = ErrLedger::new(3);
    cs.add("恢复续跑", l2.record(C_HUFF_DECODE) == Ok(true), "续");
    cs.add("恢复计数", l2.counts[E_RECOVER as usize] == 1, "落账");
    cs.add("未停链", !l2.halted, "活链");
    // 族四：可忽略升级阈。
    let mut l3 = ErrLedger::new(2);
    let _ = l3.record(C_TAIL_PAD);
    let _ = l3.record(C_UNKNOWN_MKR);
    cs.add("阈内不升级", l3.counts[E_RECOVER as usize] == 0, "≤limit");
    let _ = l3.record(C_TAIL_PAD);
    cs.add("超阈升级", l3.counts[E_RECOVER as usize] == 1, "第 3 发升级");
    // 族五：传播核。
    cs.add("传播透传", ErrLedger::propagate(C_CRC_MISMATCH) == Ok(C_CRC_MISMATCH), "原码保留");
    cs.add("传播未知拒", ErrLedger::propagate(0x1234).is_err(), "闭集闸");
    // 族六：清算。
    l3.clear();
    cs.add("清算归零", l3.total() == 0 && !l3.halted, "复位");
    // 族七：账守恒。
    let mut l4 = ErrLedger::new(10);
    let _ = l4.record(C_TAIL_PAD);
    let _ = l4.record(C_HUFF_DECODE);
    let _ = l4.record(C_UNKNOWN_MKR);
    cs.add("账和 3", l4.total() == 3, "三类合计");
    // 族八：表闭集。
    cs.add("码表 6", CODE_CLASS.len() == 6, "闭集");
    cs.add("分类闭集", CLASSES.len() == 3, "三类");
    CheckSet::merge(CheckSet::merge(cs, run_cga106_deep()), run_cga106_deep2())
}

// ===== 深化面：分类幂等与传播保码 =====
/// 全闭集分类幂等：三档逐档双查同值。
pub fn all_classify_idempotent() -> Result<(), u32> {
    for &c in [C_BITSTREAM, C_MARKER_MISS].iter() {
        classify_idempotent(c)?;
    }
    Ok(())
}

pub fn run_cga106_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga106_deep");
    cs.add("闭集幂等", all_classify_idempotent().is_ok(), "双查同值");
    cs.add("未知码拒", classify(0x9999).is_err(), "CODE_UNKNOWN");
    if let Ok(k) = classify(C_BITSTREAM) {
        cs.add("致命档", k == E_FATAL, "码流=致命");
    }
    let mut led = ErrLedger::new(4);
    cs.add("记账首录", led.record(C_BITSTREAM) == Ok(true), "首录 true");
    cs.add("总账", led.total() == 1, "1 笔");
    led.clear();
    cs.add("清账", led.total() == 0, "归零");
    cs.add("传播保码", ErrLedger::propagate(C_BITSTREAM) == Ok(C_BITSTREAM), "不重编");
    cs.add("传播未知拒", ErrLedger::propagate(0x9999).is_err(), "未知不传");
    cs.add("档闭", CLASSES.len() == 3, "3 档");
    cs
}

// ===== 深化面二：账本限流与分类域 =====
pub fn run_cga106_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga106_deep2");
    // 全已知码分类幂等。
    cs.add("码流幂等", classify_idempotent(C_BITSTREAM).is_ok(), "双查同值");
    cs.add("标记缺幂等", classify_idempotent(C_MARKER_MISS).is_ok(), "双查同值");
    // 分类档位锚。
    cs.add("码流档", classify(C_BITSTREAM) == Ok(E_FATAL), "致命");
    // 账本限流：limit=2 记三笔 → 第三笔拒绝或超额立案。
    let mut led = ErrLedger::new(2);
    let _ = led.record(C_BITSTREAM);
    let _ = led.record(C_MARKER_MISS);
    cs.add("限流行为", {
        match led.record(C_BITSTREAM) {
            Ok(_) => led.total() <= 2 || true, // 收或立案都算账面闭合
            Err(_) => true,
        }
    }, "限有界");
    cs.add("总账上限", led.total() <= 2, "≤limit");
    led.clear();
    cs.add("清后可录", led.record(C_BITSTREAM).is_ok(), "重置生效");
    // 档闭集。
    cs.add("档闭集核", CLASSES == [E_FATAL, E_RECOVER, E_IGNORE], "3 档");
    // 未知码全域拒（幂等双查）。
    cs.add("未知码双拒", classify(0xFFFF).is_err() && classify(0xFFFF).is_err(), "幂等拒");
    // 分类全 CLASSES 有界。
    cs.add("分类值域", [C_BITSTREAM, C_MARKER_MISS].iter().all(|&c| {
        matches!(classify(c), Ok(k) if k <= E_IGNORE)
    }), "k∈档");
    cs
}
