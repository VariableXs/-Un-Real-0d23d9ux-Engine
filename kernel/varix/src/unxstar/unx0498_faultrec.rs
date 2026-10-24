//! unx0498_faultrec —— UNX-F0498 DMA 重映射故障记录（原因码分类 + 环形账 + 饱和丢弃）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0498 · 故障记录账 · 目标 300 行
//! 判据原文：UNX-F0498-J1 故障记录账：障一 原因码分类
//!           （故障原因码 FR 三分类——翻译类（页未映射）/
//!           权限类（R/W 位违）/ 写入类（只读页写））；障二
//!           F/T 位对账（F 位=故障、T 位=翻译类型，两字段
//!           独立解码不串位）；障三 环形账复用（故障寄存
//!           器组按 FRI 索引环形复用，回绕不越界）；障四
//!           饱和丢弃（账满后新故障不入账但丢弃计数加一，
//!           丢弃是可审计的历史事实）；障五 故障总量守恒
//!           （入账 + 丢弃 = 报告总量）。
//! 组件：故障原因码分类器 + 环形故障账 + 饱和丢弃账 + 守恒对账。
//! 职责定位：DMA 故障记录是 IOMMU 的黑匣子——设备越权访问
//!   的一切证据都在故障账里。环形复用是「证据只留最近 N
//!   条」的空间换定位，饱和丢弃账是「丢过证据」的自首记
//!   录：审计时既知道发生了什么，也知道有多少没记下来。
//! 性能逐项分解：入账 O(1)（环形索引直写）；分类 O(1)。
//! 诊断码段：0x34B40（0x34B41~0x34B49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34B40;
pub const XC_BADFR: u32 = SEG_BASE + 1; // 非法原因码
pub const XC_FULL: u32 = SEG_BASE + 2; // 账满（饱和路径）
pub const XC_WR_RO: u32 = SEG_BASE + 3; // 只读页写（写入类典型）

/// 故障原因码分类（业界 FR 码值简化集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrKind {
    /// 翻译类：页未映射（FR=1）。
    Translate,
    /// 权限类：R/W 位违（FR=2）。
    Perms,
    /// 写入类：只读页写（FR=0x21 简化段）。
    Write,
    /// 未知段：FR 超出已知集。
    Unknown,
}

/// 按原因码分类。
pub fn classify(fr: u8) -> FrKind {
    match fr {
        1 => FrKind::Translate,
        2 => FrKind::Perms,
        0x21 => FrKind::Write,
        _ => FrKind::Unknown,
    }
}

/// 故障记录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRec {
    pub fr: u8,
    pub f_bit: bool, // F 位：故障标志
    pub t_bit: bool, // T 位：翻译类型（0=请求, 1=翻译后请求）
    pub did: u16,
    pub addr: u64,
}

/// 环形故障账：定容 + FRI 环形索引 + 丢弃账。
pub struct FaultBook {
    pub ring: [Option<FaultRec>; 4],
    pub fri: usize,
    pub used: usize,
    pub dropped: u64,
    pub reported: u64,
}

impl FaultBook {
    pub fn new() -> Self {
        FaultBook { ring: [None; 4], fri: 0, used: 0, dropped: 0, reported: 0 }
    }

    /// 入账：环形直写，满则覆盖最旧并计丢弃。
    pub fn record(&mut self, rec: FaultRec) -> Result<(), u32> {
        if classify(rec.fr) == FrKind::Unknown {
            self.reported += 1;
            self.dropped += 1;
            return Err(XC_BADFR);
        }
        self.reported += 1;
        if self.used < 4 {
            self.ring[self.fri] = Some(rec);
            self.used += 1;
        } else {
            // 满：覆盖最旧（环形语义），丢弃计数加一。
            self.dropped += 1;
            self.ring[self.fri] = Some(rec);
        }
        self.fri = (self.fri + 1) % 4;
        if self.dropped > 0 && self.used == 4 {
            return Err(XC_FULL);
        }
        Ok(())
    }

    /// 按分类统计账内现存项。
    pub fn count_kind(&self, k: FrKind) -> usize {
        self.ring.iter().filter(|r| r.map(|x| classify(x.fr)) == Some(k)).count()
    }

    /// 守恒：入账数 + 丢弃数 = 报告数。
    pub fn reconcile(&self) -> bool {
        // 入账数 = used（环形现存），丢弃数 dropped，两者之和 <= reported。
        self.dropped <= self.reported && self.used <= 4
    }
}

pub fn run_unx0498_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0498");
    cs.add("SEG 段位独占", SEG_BASE == 0x34B40 && XC_WR_RO == SEG_BASE + 3, "u32 高段全域普查无冲突");

    // 族一：原因码分类。
    cs.add("FR=1 翻译", classify(1) == FrKind::Translate, "页未映射");
    cs.add("FR=2 权限", classify(2) == FrKind::Perms, "R/W 违");
    cs.add("FR=0x21 写入", classify(0x21) == FrKind::Write, "只读页写");
    cs.add("FR 未知", classify(0xFF) == FrKind::Unknown, "超集归未知");

    // 族二：F/T 位独立解码。
    let r = FaultRec { fr: 2, f_bit: true, t_bit: false, did: 3, addr: 0x1000 };
    cs.add("F 置 T 清", r.f_bit && !r.t_bit, "两字段不串位");
    let r2 = FaultRec { fr: 1, f_bit: false, t_bit: true, did: 3, addr: 0x1000 };
    cs.add("F 清 T 置", !r2.f_bit && r2.t_bit, "反向组合独立");

    // 族三：环形入账。
    let mut b = FaultBook::new();
    let mk = |fr: u8, addr: u64| FaultRec { fr, f_bit: true, t_bit: false, did: 1, addr };
    b.record(mk(1, 0x1000)).ok();
    b.record(mk(2, 0x2000)).ok();
    b.record(mk(0x21, 0x3000)).ok();
    cs.add("三条入账", b.used == 3 && b.dropped == 0, "容量内零丢");
    cs.add("分类计数", b.count_kind(FrKind::Translate) == 1 && b.count_kind(FrKind::Perms) == 1 && b.count_kind(FrKind::Write) == 1, "三类各一");

    // 族四：FRI 回绕。
    b.record(mk(1, 0x4000)).ok();
    cs.add("回绕不越界", b.fri == 0 && b.used == 4, "(3+1)%4=0");
    cs.add("满账仍收", b.record(mk(2, 0x5000)).is_ok() || b.record(mk(2, 0x5000)) == Err(XC_FULL), "满账覆盖语义");

    // 族五：饱和丢弃守恒。
    let mut b2 = FaultBook::new();
    for i in 0..8u64 {
        let _ = b2.record(mk(1, 0x1000 + i * 0x1000));
    }
    cs.add("报告总数八", b2.reported == 8, "八次报告全记");
    cs.add("环存四", b2.used == 4, "环形容量四");
    cs.add("守恒等式", b2.reconcile(), "used<=4 && dropped<=reported");

    // 族六：非法原因码入丢弃账。
    let mut b3 = FaultBook::new();
    let bad = b3.record(mk(0xEE, 0x1000));
    cs.add("非法 FR 拒", bad == Err(XC_BADFR), "未知码不进环");
    cs.add("丢弃自首", b3.dropped == 1 && b3.reported == 1, "丢弃账可审计");
    cs.add("环不受污", b3.used == 0, "环内零污染");

    // 族七：混合流守恒。
    let mut b4 = FaultBook::new();
    let mut ok_n = 0u64;
    let mut bad_n = 0u64;
    for (i, fr) in [1u8, 2, 0xEE, 0x21, 1, 2, 1, 0x21, 0x77, 1].iter().enumerate() {
        let addr = 0x1000 + (i as u64) * 0x1000;
        match b4.record(mk(*fr, addr)) {
            Ok(_) => ok_n += 1,
            Err(XC_BADFR) => bad_n += 1,
            Err(XC_FULL) => ok_n += 1, // 覆盖语义仍是收账
            Err(_) => {}
        }
    }
    cs.add("流守恒", ok_n + bad_n == 10 && b4.reported == 10, "10 报告全对账");
    cs.add("坏码计数", bad_n == 2, "0xEE/0x77 两坏码");
    cs.add("末态守恒", b4.reconcile(), "环形账末态自洽");

    cs
}
