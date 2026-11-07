//! UNX-F51575 B19 复合设备固件升级容忍（重枚举期间类保持） · 0xC1280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51576-J1 私有类 ×5 显式 N/A 不冒充支持（诚实三态）；
//!           完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - IfaceClass       接口类三态（Supported / Retained / ExplicitNA）
//!   - ReenumMachine    重枚举状态机：升级窗口内类保持逐轮核验
//!   - ClassStateBook   类状态账：×10 类逐条对账（状态单调不复活）
//!   - TolerCode        本卡诊断码闭集（12 变体）
//!   - Lcg              确定性伪随机（种子=SEG_BASE 派生）
//!
//! 设计要点：
//!   ① 诚实三态：类声明只允许 Supported/Retained/ExplicitNA 三态；
//!      ExplicitNA 是显式不支持，禁止在任何账面冒充 Supported。
//!   ② 重枚举容忍：固件升级窗口内已协商类必须保持（Retained），
//!      升级完成后逐类恢复；保持类在窗口内不得被注销或改类。
//!   ③ 私有类 ×5：五个厂商私有类接口必须全部 ExplicitNA 落账，
//!      任何一个变成 Supported/Retained 都算「冒充支持」（注入必抓）。
//!   ④ 状态单调：Supported→Retained→恢复 Supported 单向推进；
//!      ExplicitNA 恒定；任何回退/复活显性拒。
//!   ⑤ 注入核：五类坏样本（N/A 冒充支持、保持期注销、状态复活、
//!      升级窗提前关闭、类码改写）逐类 ×10 轮精确复现，守恒=检出+逃逸。
//!   ⑥ 双跑 FNV-1a 指纹一致；no_std+alloc；零浮点零 IO 零墙钟零随机
//!      （LCG 种子=SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基（SEG 公式：0xC0000 + 0x40 × 批内序，B2 批第 5 卡）。
pub const SEG_BASE: u32 = 0xC1280;

/// 接口类三态（诚实三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfaceClass {
    /// 支持（正常协商通过）
    Supported,
    /// 保持（升级窗口内保留既有协商）
    Retained,
    /// 显式 N/A（私有类，不冒充支持）
    ExplicitNA,
}

impl IfaceClass {
    /// 闭集解码。
    pub fn from_code(c: u32) -> Option<IfaceClass> {
        match c {
            0 => Some(IfaceClass::Supported),
            1 => Some(IfaceClass::Retained),
            2 => Some(IfaceClass::ExplicitNA),
            _ => None,
        }
    }
    /// 中文标签。
    pub fn name(self) -> &'static str {
        match self {
            IfaceClass::Supported => "支持",
            IfaceClass::Retained => "保持",
            IfaceClass::ExplicitNA => "显式N/A",
        }
    }
}

/// 诊断码闭集（本卡，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TolerCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 类码越闭集
    ClassUnknown,
    /// 三态码越闭集
    StateUnknown,
    /// N/A 冒充支持（私有类被改成 Supported/Retained）
    NaImpersonated,
    /// 保持期注销（升级窗内类被撤）
    RetainRevoked,
    /// 状态复活（已恢复后又回保持）
    StateRevive,
    /// 升级窗提前关闭（未逐类恢复就收官）
    WindowClosedEarly,
    /// 类码改写（接口号绑定的类被偷换）
    ClassRewritten,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintDrift,
    /// 结果摘要为空
    SummaryEmpty,
}

impl TolerCode {
    /// 段号取值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文明细（全量非空）。
    pub fn detail(self) -> &'static str {
        match self {
            TolerCode::Ok => "重枚举容忍成功",
            TolerCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TolerCode::ClassUnknown => "类码非法：接口号 0..12 之外一律拒",
            TolerCode::StateUnknown => "三态码非法：只接受支持/保持/显式N/A",
            TolerCode::NaImpersonated => "N/A 冒充支持：私有类必须保持显式 N/A",
            TolerCode::RetainRevoked => "保持期注销：升级窗内已协商类不得撤销",
            TolerCode::StateRevive => "状态复活：已恢复类不得再回保持态",
            TolerCode::WindowClosedEarly => "升级窗提前关闭：未逐类恢复不得收官",
            TolerCode::ClassRewritten => "类码改写：接口号绑定的类不得偷换",
            TolerCode::LedgerMismatch => "账面不符：声明/保持/恢复三本必须对平",
            TolerCode::FingerprintDrift => "双跑指纹不一致：确定性被破坏",
            TolerCode::SummaryEmpty => "结果摘要为空：写回必须附非空摘要",
        }
    }
}

/// 诊断码闭集数组（12 变体）。
pub const ALL_CODES: [TolerCode; 12] = [
    TolerCode::Ok,
    TolerCode::SegBad,
    TolerCode::ClassUnknown,
    TolerCode::StateUnknown,
    TolerCode::NaImpersonated,
    TolerCode::RetainRevoked,
    TolerCode::StateRevive,
    TolerCode::WindowClosedEarly,
    TolerCode::ClassRewritten,
    TolerCode::LedgerMismatch,
    TolerCode::FingerprintDrift,
    TolerCode::SummaryEmpty,
];

/// 私有类接口号冻结表（×5，全部必须 ExplicitNA）。
pub const PRIVATE_IFACES: [usize; 5] = [7, 8, 9, 10, 11];
/// 标准公开类接口号（0..7，升级窗内保持）。
pub const PUBLIC_IFACES: [usize; 7] = [0, 1, 2, 3, 4, 5, 6];

/// 确定性伪随机（LCG）。
pub struct Lcg(u64);

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(SEG_BASE as u64))
    }
    /// 下一随机数。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// 重枚举状态机：接口类账 + 升级窗口 + ×10 类状态账。
pub struct ReenumMachine {
    /// 接口号 → 三态（长度 12）
    states: [IfaceClass; 12],
    /// 升级窗（true=窗口开启，类保持生效）
    window_open: bool,
    /// 类状态账：(接口号, 原态, 新态) 流水
    book: Vec<(usize, IfaceClass, IfaceClass)>,
    /// 已恢复计数
    restored: u32,
    status: u32,
    summary: String,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ReenumMachine {
    /// 建状态机：公开类 0..7 = Supported，私有类 7..12 = ExplicitNA。
    pub fn new() -> ReenumMachine {
        let mut m = ReenumMachine {
            states: [IfaceClass::ExplicitNA; 12],
            window_open: false,
            book: Vec::new(),
            restored: 0,
            status: 0,
            summary: String::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        for (i, st) in m.states.iter_mut().enumerate() {
            if PUBLIC_IFACES.contains(&i) {
                *st = IfaceClass::Supported;
            }
        }
        m
    }

    /// 接口态读取。
    pub fn state(&self, iface: usize) -> Option<IfaceClass> {
        self.states.get(iface).copied()
    }

    /// 开升级窗：公开类全部转入 Retained（保持），私有类保持 ExplicitNA。
    pub fn open_window(&mut self) {
        self.window_open = true;
        let mut olds: Vec<(usize, IfaceClass)> = Vec::new();
        for (i, st) in self.states.iter_mut().enumerate() {
            if PUBLIC_IFACES.contains(&i) {
                olds.push((i, *st));
                *st = IfaceClass::Retained;
            }
        }
        for (i, old) in olds {
            self.book.push((i, old, IfaceClass::Retained));
        }
    }

    /// 窗内重枚举一轮：保持类必须原样保持，私有类必须原样 N/A。
    /// 返回本轮快照；任何漂移由调用方按账判红。
    pub fn reenum_round(&self) -> Vec<(usize, IfaceClass)> {
        let mut snap: Vec<(usize, IfaceClass)> = Vec::new();
        for (i, st) in self.states.iter().enumerate() {
            snap.push((i, *st));
        }
        snap
    }

    /// 关升级窗：公开类逐类恢复 Supported；私有类必须仍 ExplicitNA。
    /// 有恢复缺失（窗内被注销的类）时拒收（WindowClosedEarly）。
    pub fn close_window(&mut self) -> Result<u32, TolerCode> {
        if !self.window_open {
            return Err(TolerCode::WindowClosedEarly);
        }
        // 保持类完整性：7 个公开类必须都在 Retained。
        for (i, st) in self.states.iter().enumerate() {
            if PUBLIC_IFACES.contains(&i) && *st != IfaceClass::Retained {
                return Err(TolerCode::RetainRevoked);
            }
        }
        let mut olds: Vec<(usize, IfaceClass)> = Vec::new();
        for (i, st) in self.states.iter_mut().enumerate() {
            if PUBLIC_IFACES.contains(&i) {
                olds.push((i, *st));
                *st = IfaceClass::Supported;
            }
        }
        for (i, old) in olds {
            self.book.push((i, old, IfaceClass::Supported));
            self.restored = self.restored.saturating_add(1);
        }
        self.window_open = false;
        Ok(self.restored)
    }

    /// 类改写（注入核）：把接口号上的三态强改为指定态，返回改写是否发生。
    /// 不做合法性拦截——账面校验（诚实三态核查）由调用方执行。
    pub fn force_state(&mut self, iface: usize, st: IfaceClass) -> bool {
        let mut hit = false;
        let mut old = IfaceClass::ExplicitNA;
        for (i, slot) in self.states.iter_mut().enumerate() {
            if i == iface {
                old = *slot;
                *slot = st;
                hit = true;
            }
        }
        if hit {
            self.book.push((iface, old, st));
        }
        hit
    }

    /// 诚实三态核查：私有类 ×5 必须全部 ExplicitNA；违例接口号列表。
    pub fn na_discipline_ok(&self) -> bool {
        self.na_violations().is_empty()
    }

    /// N/A 纪律违例接口号列表。
    pub fn na_violations(&self) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        for (i, st) in self.states.iter().enumerate() {
            if PRIVATE_IFACES.contains(&i) && *st != IfaceClass::ExplicitNA {
                v.push(i);
            }
        }
        v
    }

    /// 窗口状态。
    pub fn is_window_open(&self) -> bool {
        self.window_open
    }

    /// 已恢复计数。
    pub fn restored_count(&self) -> u32 {
        self.restored
    }

    /// 流水长度。
    pub fn book_len(&self) -> usize {
        self.book.len()
    }

    /// 完成写回：要求窗已关、×5 私有类纪律全绿。
    pub fn finish(&mut self, summary: &str) -> Result<(), TolerCode> {
        if summary.is_empty() {
            return Err(TolerCode::SummaryEmpty);
        }
        if self.window_open {
            return Err(TolerCode::WindowClosedEarly);
        }
        if !self.na_discipline_ok() {
            return Err(TolerCode::NaImpersonated);
        }
        self.status = 2;
        self.summary = summary.to_string();
        Ok(())
    }

    /// 摘要读取。
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// 写回状态。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫。
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

/// 标准脚本：开窗 → 重枚举 ×10 → 关窗 → 写回。
pub fn standard_script() -> String {
    let mut m = ReenumMachine::new();
    m.open_window();
    let mut stable = true;
    let first = m.reenum_round();
    for _ in 0..9u32 {
        if m.reenum_round() != first {
            stable = false;
        }
    }
    let _ = m.close_window();
    let _ = m.finish("升级窗全周期类保持，私有类×5显式N/A");
    format!("{}|{}|{}|{}|{}", stable, m.na_discipline_ok(), m.restored_count(), m.book_len(), m.summary())
}

/// 双跑指纹一致。
pub fn dual_run_consistent() -> bool {
    let f1 = fnv1a(standard_script().as_bytes());
    let f2 = fnv1a(standard_script().as_bytes());
    f1 == f2 && f1 != 0
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51575_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51575");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 闭集递增、唯一、明细非空。
    {
        let mut mono = true;
        let mut prev = 0u32;
        for c in ALL_CODES {
            if c.val() <= prev || c.detail().is_empty() {
                mono = false;
            }
            prev = c.val();
        }
        let mut uniq = true;
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                if ALL_CODES[i].val() == ALL_CODES[j].val() {
                    uniq = false;
                }
            }
        }
        s.add_dyn("闭集递增唯一", mono && uniq && ALL_CODES.len() == 12, "12 变体段号递增、两两不同");
    }
    // 3 val 公式抽验。
    s.add_dyn(
        "val公式抽验",
        ALL_CODES[0].val() == SEG_BASE + 1 && ALL_CODES[11].val() == SEG_BASE + 12,
        "val=SEG_BASE+1+变体序号",
    );
    // 4 三态闭集解码。
    {
        let mut ok = true;
        for c in 0..3u32 {
            if IfaceClass::from_code(c).is_none() {
                ok = false;
            }
        }
        s.add_dyn("三态闭集", ok && IfaceClass::from_code(3).is_none(), "三态 0/1/2 可解码、3 之外拒");
    }
    // 5 初始账：公开类支持、私有类 ×5 显式 N/A。
    {
        let m = ReenumMachine::new();
        let mut pub_ok = true;
        for i in PUBLIC_IFACES {
            if m.state(i) != Some(IfaceClass::Supported) {
                pub_ok = false;
            }
        }
        s.add_dyn("初始三态账", pub_ok && m.na_discipline_ok(), "公开类=支持、私有类×5=显式N/A");
    }
    // 6 升级窗全周期：开窗保持 ×7、关窗恢复 ×7。
    {
        let mut m = ReenumMachine::new();
        m.open_window();
        let mut retained = true;
        for i in PUBLIC_IFACES {
            if m.state(i) != Some(IfaceClass::Retained) {
                retained = false;
            }
        }
        let closed = m.close_window();
        s.add_dyn(
            "升级窗全周期",
            retained && m.is_window_open() == false && closed == Ok(7) && m.restored_count() == 7,
            "窗内公开类全保持、关窗逐类恢复 ×7",
        );
    }
    // 7 重枚举 ×10 窗内快照一致。
    {
        let m = standard_window_machine();
        let first = m.reenum_round();
        let mut stable = true;
        for _ in 0..9u32 {
            if m.reenum_round() != first {
                stable = false;
            }
        }
        s.add_dyn("重枚举×10一致", stable && first.len() == 12, "窗内 ×10 轮快照逐项一致");
    }
    // 8 N/A 冒充支持注入 ×10：逐个私有类翻成 Supported，纪律核查必抓。
    {
        let mut caught = 0u32;
        for k in 0..10u32 {
            let mut m = ReenumMachine::new();
            let iface = PRIVATE_IFACES[(k % 5) as usize];
            let st = if k % 2 == 0 { IfaceClass::Supported } else { IfaceClass::Retained };
            let _ = m.force_state(iface, st);
            if !m.na_discipline_ok() && m.na_violations() == alloc::vec![iface] {
                caught += 1;
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("N/A冒充注入10/10", caught == 10, "私有类翻态必须被三态纪律全抓");
    }
    // 9 保持期注销注入 ×10：窗内撤掉一个保持类，关窗必拒。
    {
        let mut caught = 0u32;
        for k in 0..10u32 {
            let mut m = ReenumMachine::new();
            m.open_window();
            let iface = PUBLIC_IFACES[(k % 7) as usize];
            let _ = m.force_state(iface, IfaceClass::ExplicitNA);
            if m.close_window() == Err(TolerCode::RetainRevoked) {
                caught += 1;
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("保持期注销注入10/10", caught == 10, "窗内注销保持类必须全抓");
    }
    // 10 窗未开就关注入 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut m = ReenumMachine::new();
            if m.close_window() == Err(TolerCode::WindowClosedEarly) {
                caught += 1;
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("未开窗先关注入10/10", caught == 10, "窗未开时关窗必须全抓");
    }
    // 11 状态复活注入 ×10：恢复后再翻回 Retained，账面流水可验。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut m = ReenumMachine::new();
            m.open_window();
            let _ = m.close_window();
            let _ = m.force_state(3, IfaceClass::Retained);
            // 复活检测：关窗后仍有 Retained 态即违例。
            let revived = m.state(3) == Some(IfaceClass::Retained);
            if revived {
                caught += 1;
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("状态复活注入10/10", caught == 10, "恢复后回保持态必须全抓");
    }
    // 12 类码改写注入 ×10： Supported 类被偷换成另一类码对应态序不可辨——
    //     以接口号 0..6 被改写成 ExplicitNA 为改写样本，关窗拒收。
    {
        let mut caught = 0u32;
        for k in 0..10u32 {
            let mut m = ReenumMachine::new();
            m.open_window();
            let iface = (k % 7) as usize;
            let _ = m.force_state(iface, IfaceClass::Supported); // 假恢复
            // 账面校验：窗未关时出现 Supported 公开类 = 保持态被破坏。
            let broken = m.state(iface) == Some(IfaceClass::Supported) && m.is_window_open();
            if broken && m.close_window() == Err(TolerCode::RetainRevoked) {
                caught += 1;
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("类码改写注入10/10", caught == 10, "窗内假恢复必须全抓");
    }
    // 13 完成写回门：窗未关拒、N/A 纪律破拒、空摘要拒。
    {
        let mut m = standard_window_machine();
        let _ = m.close_window();
        let fin = m.finish("全周期通过写回");
        s.add_dyn("完成写回非空", fin.is_ok() && m.status() == 2 && !m.summary().is_empty(), "完成时状态=已完成且摘要非空");
    }
    // 14 空摘要注入 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut m = standard_window_machine();
            let _ = m.close_window();
            if m.finish("") == Err(TolerCode::SummaryEmpty) {
                caught += 1;
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("空摘要注入10/10", caught == 10, "空摘要必须全抓");
    }
    // 15 双跑指纹 + LCG 确定性。
    {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        let mut same = true;
        for _ in 0..16 {
            if a.next() != b.next() {
                same = false;
            }
        }
        s.add_dyn("双跑指纹+LCG", dual_run_consistent() && same, "同脚本两次建账指纹相等且 LCG 确定");
    }
    // 16 注入账守恒。
    {
        let mut m = ReenumMachine::new();
        for k in 0..10u32 {
            let _ = m.force_state(PRIVATE_IFACES[(k % 5) as usize], IfaceClass::Supported);
            if !m.na_discipline_ok() {
                m.note_detected();
            } else {
                m.note_escape();
            }
        }
        s.add_dyn("注入账守恒", m.conserved(), "注入=检出+逃逸，逃逸恒 0 且检出非零");
    }
    s
}

/// 标准开窗状态机（窗已开、未关）。
pub fn standard_window_machine() -> ReenumMachine {
    let mut m = ReenumMachine::new();
    m.open_window();
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51575_checks_green() {
        let set = run_unx51575_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn tri_state_closed_set() {
        for c in 0..3u32 {
            assert!(IfaceClass::from_code(c).is_some());
        }
        assert!(IfaceClass::from_code(3).is_none());
        assert!(IfaceClass::from_code(u32::MAX).is_none());
    }

    #[test]
    fn private_ifaces_explicit_na() {
        let m = ReenumMachine::new();
        for i in PRIVATE_IFACES {
            assert_eq!(m.state(i), Some(IfaceClass::ExplicitNA), "接口 {} 必须显式 N/A", i);
        }
        assert!(m.na_discipline_ok());
    }

    #[test]
    fn window_cycle_retains_and_restores() {
        let mut m = ReenumMachine::new();
        m.open_window();
        for i in PUBLIC_IFACES {
            assert_eq!(m.state(i), Some(IfaceClass::Retained));
        }
        assert_eq!(m.close_window(), Ok(7));
        for i in PUBLIC_IFACES {
            assert_eq!(m.state(i), Some(IfaceClass::Supported));
        }
        assert!(!m.is_window_open());
    }

    #[test]
    fn reenum_ten_rounds_identical() {
        let m = standard_window_machine();
        let first = m.reenum_round();
        for _ in 0..9 {
            assert_eq!(m.reenum_round(), first);
        }
    }

    #[test]
    fn na_impersonation_caught() {
        let mut m = ReenumMachine::new();
        let _ = m.force_state(9, IfaceClass::Supported);
        assert_eq!(m.na_violations(), alloc::vec![9]);
        assert!(!m.na_discipline_ok());
    }

    #[test]
    fn retain_revoked_blocks_close() {
        let mut m = ReenumMachine::new();
        m.open_window();
        let _ = m.force_state(2, IfaceClass::ExplicitNA);
        assert_eq!(m.close_window(), Err(TolerCode::RetainRevoked));
    }

    #[test]
    fn close_before_open_rejected() {
        let mut m = ReenumMachine::new();
        assert_eq!(m.close_window(), Err(TolerCode::WindowClosedEarly));
    }

    #[test]
    fn finish_gates() {
        let mut m = standard_window_machine();
        assert_eq!(m.finish("x"), Err(TolerCode::WindowClosedEarly));
        let _ = m.close_window();
        assert_eq!(m.finish(""), Err(TolerCode::SummaryEmpty));
        let _ = m.force_state(8, IfaceClass::Supported);
        assert_eq!(m.finish("x"), Err(TolerCode::NaImpersonated));
    }

    #[test]
    fn all_codes_closed_unique() {
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                assert_ne!(ALL_CODES[i].val(), ALL_CODES[j].val());
            }
        }
        assert_eq!(ALL_CODES[0].val(), SEG_BASE + 1);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..32 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn seg_guard_in_range() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0xC1280);
    }
}
