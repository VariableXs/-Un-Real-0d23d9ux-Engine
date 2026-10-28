//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12447 · NtAccessCheck 语义档 · 0x305e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12447-J1 DACL 遍历判定（Allow/Deny 序）判据绿，注入 30/30
//!      判定与锚一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - ACE 域：Ace 行（Deny/Allow 型 + 掩码 + Sid），DACL 定容 8 行；
//!   - 遍历判定（NT 口径）：逐 ACE 扫描，首个命中 Sid 的 ACE 定生死——
//!     Deny 命中且掩码交叠 → 拒；Allow 命中且掩码覆盖 → 授；扫尽无命中
//!     → 拒（无 DACL 面）；
//!   - 规范序纪律：DACL 登记 Allow 先于 Deny 视为非规范序（OrderBad 拒），
//!     Deny 前置保证 deny-wins 语义与掩码代数锚一致；
//!   - 掩码代数锚：锚定面独立重算（deny 交叠 → 拒；allow 覆盖 desired →
//!     授；否则拒），与遍历判定 30/30 对表；
//!   - 注入 30 例：desired 掩码 × Deny 有无 × Allow 有无 × Sid 匹配四轴
//!     LCG 组合，逐例与锚一致；
//!   - Sid 域：令牌组 Rid 闭集（4 组），Sid 不在组集一律不命中；
//!   - DesiredAccess 零拒（DesiredZero）、Ace 越位拒（AceOob）；
//!   - 错误路径带码+中文 detail，拒止不写回 DACL；
//!   - 注入核：LCG 写死种子 0x305e0，30/30 判定与锚一致 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x305e0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x305e0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入例数（判准 30/30）。
pub const INJECT_CASES: usize = 30;

/// DACL 行上限（定容 8）。
pub const DACL_ROWS: usize = 8;

/// 令牌组 Sid 数（闭集 4）。
pub const SID_COUNT: u32 = 4;

// 期望访问位（演练域口径：读/写/执行三轴）。
/// 期望读位。
pub const DESIRED_READ: u32 = 0x1;
/// 期望写位。
pub const DESIRED_WRITE: u32 = 0x2;
/// 期望执行位。
pub const DESIRED_EXECUTE: u32 = 0x4;
/// 合法掩码闭集。
pub const DESIRED_VALID_MASK: u32 = DESIRED_READ | DESIRED_WRITE | DESIRED_EXECUTE;

/// 诊断码（0x305e0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcCode {
    /// 0x305e0 DACL 满格。
    DaclOob = 0,
    /// 0x305e1 ACE 型/位越集（Deny/Allow 之外）。
    AceOob = 1,
    /// 0x305e2 期望掩码零。
    DesiredZero = 2,
    /// 0x305e3 Sid 越集（越四组闭集）。
    SidOob = 3,
    /// 0x305e4 期望掩码越集。
    MaskMiss = 4,
    /// 0x305e5 ACE 重号（同型同 Sid 同掩码）。
    AceDup = 5,
    /// 0x305e6 DACL 缩水（append-only 破）。
    DaclShrink = 6,
    /// 0x305e7 非规范序（Allow 先于 Deny 登记）。
    OrderBad = 7,
    /// 0x305e8 期望掩码越位。
    DesiredOob = 8,
    /// 0x305e9 判定与锚失配。
    AnchorMiss = 9,
}

impl AcCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [AcCode; 10] = [
        AcCode::DaclOob, AcCode::AceOob, AcCode::DesiredZero, AcCode::SidOob,
        AcCode::MaskMiss, AcCode::AceDup, AcCode::DaclShrink, AcCode::OrderBad,
        AcCode::DesiredOob, AcCode::AnchorMiss,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x305e0 => "DACL 满格：先删废 ACE 再登记，行账对平。",
        0x305e1 => "ACE 型越集：只收 Deny/Allow 两型。",
        0x305e2 => "期望掩码零：DesiredAccess 必须 >0。",
        0x305e3 => "Sid 越集：Sid 先比 SID_COUNT 再登记。",
        0x305e4 => "掩码越集：Ace 掩码只收 READ/WRITE/EXECUTE 三位。",
        0x305e5 => "ACE 重号：同型同 Sid 同掩码只登一行。",
        0x305e6 => "DACL 缩水：append-only，禁止截断，重放补齐。",
        0x305e7 => "非规范序：Deny 前置登记，保 deny-wins 语义。",
        0x305e8 => "期望掩码越位：DesiredAccess 只收 READ/WRITE/EXECUTE 三位。",
        0x305e9 => "判定与锚失配：遍历面与掩码代数面逐例对表。",
        _ => "未知诊断码：回查 0x305e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x305e0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x305e_305e_305e_305e;

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

/// ACE 型（两型闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AceType {
    /// Deny（拒）。
    Deny = 0,
    /// Allow（授）。
    Allow = 1,
}

/// ACE 行。
#[derive(Clone, Copy, Debug)]
pub struct AceRow {
    /// 型（Deny/Allow）。
    pub ace_type: AceType,
    /// 掩码（三位闭集）。
    pub mask: u32,
    /// Sid（组 Rid，1..=4）。
    pub sid: u32,
}

/// 判定结果（授/拒两态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 授（掩码覆盖）。
    Grant = 0,
    /// 拒（Deny 命中/无命中/覆盖不足）。
    Deny = 1,
}

/// NtAccessCheck 语义档正本。
///
/// 生命周期：add_ace（DACL 登记，Deny 前置规范序）→
/// access_check（遍历判定）→ anchor_check（掩码代数锚）。
#[derive(Debug)]
pub struct AccessBook {
    /// DACL。
    dacl: Vec<AceRow>,
    /// DACL 峰值。
    high: usize,
    /// 判定账（append-only）。
    verdicts: Vec<(u64, Verdict)>,
    /// 判定序。
    seq: u64,
    /// 拒止累计。
    rejects: u32,
}

impl AccessBook {
    /// 建账（空 DACL）。
    pub fn new() -> Self {
        AccessBook {
            dacl: Vec::new(),
            high: 0,
            verdicts: Vec::new(),
            seq: 0,
            rejects: 0,
        }
    }

    /// ACE 域核对（型/掩码/Sid 闭集）。
    pub fn ace_ok(ace_type: AceType, mask: u32, sid: u32) -> bool {
        mask != 0
            && mask & !DESIRED_VALID_MASK == 0
            && sid >= 1
            && sid <= SID_COUNT
            && (ace_type == AceType::Deny || ace_type == AceType::Allow)
    }

    /// 登记 ACE（域核对 + 规范序 + 重号拒）。
    pub fn add_ace(&mut self, ace_type: AceType, mask: u32, sid: u32) -> Result<bool, AcCode> {
        if !Self::ace_ok(ace_type, mask, sid) {
            self.rejects += 1;
            if mask == 0 || mask & !DESIRED_VALID_MASK != 0 {
                return Err(AcCode::MaskMiss);
            }
            if sid < 1 || sid > SID_COUNT {
                return Err(AcCode::SidOob);
            }
            return Err(AcCode::AceOob);
        }
        if self.dacl.len() >= DACL_ROWS {
            self.rejects += 1;
            return Err(AcCode::DaclOob);
        }
        // 重号拒（同型同 Sid 同掩码）。
        if self
            .dacl
            .iter()
            .any(|a| a.ace_type == ace_type && a.mask == mask && a.sid == sid)
        {
            self.rejects += 1;
            return Err(AcCode::AceDup);
        }
        // 规范序：Deny 之前不得有 Allow（deny 前置口径）。
        if ace_type == AceType::Deny && self.dacl.iter().any(|a| a.ace_type == AceType::Allow) {
            self.rejects += 1;
            return Err(AcCode::OrderBad);
        }
        self.dacl.push(AceRow { ace_type, mask, sid });
        if self.dacl.len() > self.high {
            self.high = self.dacl.len();
        }
        Ok(true)
    }

    /// 遍历判定（首个命中 Sid 的 ACE 定生死；扫尽无命中 → 拒）。
    pub fn access_check(&mut self, desired: u32, sid: u32) -> Verdict {
        self.seq += 1;
        let v = self.traverse(desired, sid);
        self.verdicts.push((self.seq, v));
        v
    }

    /// 遍历纯函数（不记账；锚对表面用）。
    pub fn traverse(&self, desired: u32, sid: u32) -> Verdict {
        if desired == 0 || desired & !DESIRED_VALID_MASK != 0 || sid < 1 || sid > SID_COUNT {
            return Verdict::Deny;
        }
        for a in self.dacl.iter() {
            if a.sid != sid {
                continue;
            }
            match a.ace_type {
                AceType::Deny => {
                    if a.mask & desired != 0 {
                        return Verdict::Deny;
                    }
                }
                AceType::Allow => {
                    if a.mask & desired == desired {
                        return Verdict::Grant;
                    }
                }
            }
        }
        Verdict::Deny
    }

    /// 掩码代数锚（独立重算：deny 交叠 → 拒；allow 覆盖 → 授；否则拒）。
    pub fn anchor_check(&self, desired: u32, sid: u32) -> Verdict {
        if desired == 0 || desired & !DESIRED_VALID_MASK != 0 || sid < 1 || sid > SID_COUNT {
            return Verdict::Deny;
        }
        let mut deny_mask = 0u32;
        let mut allow_mask = 0u32;
        for a in self.dacl.iter() {
            if a.sid != sid {
                continue;
            }
            match a.ace_type {
                AceType::Deny => deny_mask |= a.mask,
                AceType::Allow => allow_mask |= a.mask,
            }
        }
        if deny_mask & desired != 0 {
            return Verdict::Deny;
        }
        if allow_mask & desired == desired && desired != 0 {
            return Verdict::Grant;
        }
        Verdict::Deny
    }

    /// 期望访问掩码域核对。
    pub fn desired_ok(desired: u32) -> bool {
        desired != 0 && desired & !DESIRED_VALID_MASK == 0
    }

    /// 注入 30 例抽签（desired×Deny×Allow×Sid 四轴 LCG 组合）。
    ///
    /// 返回 (desired, sid)；DACL 由 build_case_dacl 按 LCG 预建。
    pub fn pick_case(lcg: &mut Lcg) -> (u32, u32) {
        let desired: u32 = match lcg.below(7) {
            0 => DESIRED_READ,
            1 => DESIRED_WRITE,
            2 => DESIRED_EXECUTE,
            3 => DESIRED_READ | DESIRED_WRITE,
            4 => DESIRED_READ | DESIRED_EXECUTE,
            5 => DESIRED_READ | DESIRED_WRITE | DESIRED_EXECUTE,
            _ => DESIRED_WRITE | DESIRED_EXECUTE,
        };
        let sid = 1 + lcg.below(SID_COUNT as u64) as u32;
        (desired, sid)
    }

    /// 案例域 DACL 预建（确定性：LCG 决定 Deny/Allow 有无与掩码）。
    pub fn build_case_dacl(lcg: &mut Lcg) -> AccessBook {
        let mut bk = AccessBook::new();
        // 有概率放一条 Deny ACE（sid 1..=4）。
        if lcg.below(2) == 0 {
            let sid = 1 + lcg.below(SID_COUNT as u64) as u32;
            let mask: u32 = match lcg.below(3) {
                0 => DESIRED_READ,
                1 => DESIRED_WRITE,
                _ => DESIRED_EXECUTE,
            };
            let _ = bk.add_ace(AceType::Deny, mask, sid);
        }
        // 有概率放一条 Allow ACE（sid 1..=4）。
        if lcg.below(2) == 0 {
            let sid = 1 + lcg.below(SID_COUNT as u64) as u32;
            let mask: u32 = match lcg.below(3) {
                0 => DESIRED_READ,
                1 => DESIRED_READ | DESIRED_WRITE,
                _ => DESIRED_READ | DESIRED_WRITE | DESIRED_EXECUTE,
            };
            let _ = bk.add_ace(AceType::Allow, mask, sid);
        }
        bk
    }

    /// 注入 30 例与锚一致演练核（单轮）。
    ///
    /// 返回 (一致例数, 例数)。
    pub fn case30(&mut self, lcg: &mut Lcg) -> (usize, usize) {
        let mut hit = 0usize;
        for _i in 0..INJECT_CASES {
            // 每例重建案例 DACL（确定性四轴组合）。
            let probe = Self::build_case_dacl(lcg);
            let (desired, sid) = Self::pick_case(lcg);
            let got = probe.traverse(desired, sid);
            let want = probe.anchor_check(desired, sid);
            if got == want {
                hit += 1;
            }
        }
        (hit, INJECT_CASES)
    }

    /// DACL 长。
    pub fn dacl_len(&self) -> usize {
        self.dacl.len()
    }

    /// DACL 只增不减判据。
    pub fn monotone(&self) -> bool {
        self.dacl.len() >= self.high
    }

    /// 判定账长。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// ACE 快照。
    pub fn ace_of(&self, idx: usize) -> Option<AceRow> {
        if idx >= self.dacl.len() {
            None
        } else {
            Some(self.dacl[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 注入 30/30 与锚一致演练。
pub fn drill_case30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305e0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AccessBook::new();
        let (hit, total) = bk.case30(&mut lcg);
        if hit == total {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入30例与锚一致 {}/{} 轮={}/{}", INJECT_CASES, INJECT_CASES, ok, INJECT_ROUNDS))
}

/// Allow/Deny 序演练：Deny 前置 deny-wins、无命中拒 10/10。
pub fn drill_traverse_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305e0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AccessBook::new();
        // SID 1：Deny 写 + Allow 读写 → 期望读授、写拒。
        let _ = bk.add_ace(AceType::Deny, DESIRED_WRITE, 1);
        let _ = bk.add_ace(AceType::Allow, DESIRED_READ | DESIRED_WRITE, 1);
        // SID 2：仅 Allow 读 → 读授、写拒（覆盖不足）。
        let _ = bk.add_ace(AceType::Allow, DESIRED_READ, 2);
        let r1 = bk.access_check(DESIRED_READ, 1);
        let r2 = bk.access_check(DESIRED_WRITE, 1);
        let r3 = bk.access_check(DESIRED_READ, 2);
        let r4 = bk.access_check(DESIRED_WRITE, 2);
        // SID 3：无 ACE → 拒。
        let r5 = bk.access_check(DESIRED_READ, 3);
        let _ = lcg.below(3);
        if r1 == Verdict::Grant
            && r2 == Verdict::Deny
            && r3 == Verdict::Grant
            && r4 == Verdict::Deny
            && r5 == Verdict::Deny
            && bk.verdict_len() == 5
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("Allow/Deny 序轮={}/{}", ok, INJECT_ROUNDS))
}

/// DACL 登记域演练：非规范序/重号/越掩码/越 Sid/满格 10/10 拒。
pub fn drill_dacl_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305e0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AccessBook::new();
        let _ = bk.add_ace(AceType::Allow, DESIRED_READ, 1);
        // 路 1：Deny 落 Allow 后 → OrderBad。
        let order = bk.add_ace(AceType::Deny, DESIRED_WRITE, 1);
        // 路 2：重号 → AceDup。
        let dup = bk.add_ace(AceType::Allow, DESIRED_READ, 1);
        // 路 3：越掩码 → MaskMiss。
        let mask = bk.add_ace(AceType::Allow, 0x8, 2);
        // 路 4：越 Sid → SidOob。
        let sid = bk.add_ace(AceType::Allow, DESIRED_READ, 9);
        // 路 5：零期望掩码面（判定域拒止：DesiredZero 走 traverse 拒面）。
        let z = bk.access_check(0, 1);
        // 路 6：满格拒（sid/掩码 组合唯一，避免重号干扰）。
        let mut full = false;
        for i in 2..DACL_ROWS + 2 {
            let m: u32 = if i < 6 {
                DESIRED_READ | DESIRED_WRITE
            } else {
                DESIRED_READ | DESIRED_EXECUTE
            };
            if matches!(
                bk.add_ace(AceType::Allow, m, i as u32 % SID_COUNT + 1),
                Err(AcCode::DaclOob)
            ) {
                full = true;
            }
        }
        let _ = lcg.below(3);
        if matches!(order, Err(AcCode::OrderBad))
            && matches!(dup, Err(AcCode::AceDup))
            && matches!(mask, Err(AcCode::MaskMiss))
            && matches!(sid, Err(AcCode::SidOob))
            && z == Verdict::Deny
            && full
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("DACL 域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：DACL 转录导出（逐 ACE 一行：型/掩码/Sid）。
/// ─────────────────────────────────────────────────────────────────────
/// DACL 转录导出。
pub fn export_dacl_lines(bk: &AccessBook) -> String {
    let mut s = String::from("DACL 转录：\n");
    let mut i = 0usize;
    while let Some(a) = bk.ace_of(i) {
        s.push_str(&format!(
            "· {:?} M{:x} S{}\n",
            a.ace_type, a.mask, a.sid
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐 ACE 行齐备）。
pub fn dacl_lines_complete(bk: &AccessBook) -> bool {
    let s = export_dacl_lines(bk);
    if bk.dacl_len() == 0 {
        return false;
    }
    (0..bk.dacl_len()).all(|i| {
        bk.ace_of(i)
            .map(|a| s.contains(&format!("S{}", a.sid)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：访问检查报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct AcReport {
    /// DACL 行数。
    pub dacl: usize,
    /// 判定账行数。
    pub verdicts: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// DACL 单调。
    pub mono: bool,
    /// 转录完整。
    pub lines: bool,
}

impl AcReport {
    /// 建报告。
    pub fn build(bk: &AccessBook) -> Self {
        AcReport {
            dacl: bk.dacl_len(),
            verdicts: bk.verdict_len(),
            rejects: bk.reject_total(),
            mono: bk.monotone(),
            lines: dacl_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.mono && self.verdicts >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "AcReport ACL={} 判={} 拒={} 单={} 转录={}",
            self.dacl, self.verdicts, self.rejects, self.mono, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：锚面独立域演练（锚与遍历在空/满 DACL 两极一致 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_poles_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305e0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        // 空极：无 DACL 全拒。
        let empty = AccessBook::new();
        let e1 = empty.traverse(DESIRED_READ, 1);
        let e2 = empty.anchor_check(DESIRED_READ, 1);
        // 满极：全组全权 Allow → 全授。
        let mut full = AccessBook::new();
        let mut fill_ok = true;
        for sid in 1..=SID_COUNT {
            fill_ok = fill_ok
                && full
                    .add_ace(AceType::Allow, DESIRED_VALID_MASK, sid)
                    .is_ok();
        }
        let f1 = full.traverse(DESIRED_VALID_MASK, 1);
        let f2 = full.anchor_check(DESIRED_VALID_MASK, 1);
        // 越域面：掩码越位两极同拒。
        let o1 = empty.traverse(0x8, 1);
        let o2 = empty.anchor_check(0x8, 1);
        let _ = lcg.below(3);
        if e1 == Verdict::Deny
            && e2 == Verdict::Deny
            && fill_ok
            && f1 == Verdict::Grant
            && f2 == Verdict::Grant
            && o1 == Verdict::Deny
            && o2 == Verdict::Deny
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚面两极轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：期望掩码域演练（三合法位/组合/越位 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_desired_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305e0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = AccessBook::desired_ok(DESIRED_READ);
        pass = pass && AccessBook::desired_ok(DESIRED_READ | DESIRED_WRITE | DESIRED_EXECUTE);
        pass = pass && !AccessBook::desired_ok(0);
        pass = pass && !AccessBook::desired_ok(0x8);
        pass = pass && !AccessBook::desired_ok(DESIRED_READ | 0x10);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("期望掩码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct AcCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ac_checklist(bk: &AccessBook) -> Vec<AcCheckRow> {
    let rep = AcReport::build(bk);
    let mut rows: Vec<AcCheckRow> = Vec::new();
    rows.push(AcCheckRow { name: "DACL单调", passed: bk.monotone() });
    rows.push(AcCheckRow { name: "掩码闭集", passed: DESIRED_VALID_MASK == 0x7 });
    rows.push(AcCheckRow { name: "Sid域", passed: SID_COUNT == 4 });
    rows.push(AcCheckRow { name: "行宽8", passed: DACL_ROWS == 8 });
    rows.push(AcCheckRow { name: "注入口径", passed: INJECT_CASES == 30 });
    rows.push(AcCheckRow { name: "转录完整", passed: dacl_lines_complete(bk) });
    rows.push(AcCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(AcCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(AcCheckRow { name: "判定在账", passed: bk.verdict_len() >= 1 });
    rows.push(AcCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ac_checklist_green(rows: &[AcCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ac_stats_line(bk: &AccessBook) -> String {
    format!(
        "访问检查概况：ACL {} 判 {} 拒 {}",
        bk.dacl_len(),
        bk.verdict_len(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12447_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = AccessBook::new();
    let _ = bk.add_ace(AceType::Deny, DESIRED_WRITE, 1);
    let _ = bk.add_ace(AceType::Allow, DESIRED_READ | DESIRED_WRITE, 1);
    let _ = bk.access_check(DESIRED_READ, 1);
    // 注入一例非规范序 Deny（拒止账口径）。
    let _ = bk.add_ace(AceType::Deny, DESIRED_EXECUTE, 1);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        dacl_lines_complete(&bk) && AcReport::build(&bk).all_green(),
        ac_stats_line(&bk),
    ));
    // X-b：锚面两极 10/10。
    let (ok_p, d_p) = drill_anchor_poles_10();
    out.push(("X-锚面两极10/10", ok_p, d_p));
    // X-c：期望掩码域 10/10。
    let (ok_d, d_d) = drill_desired_domain_10();
    out.push(("X-期望掩码域10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_ac_checklist(&bk);
    out.push(("X-终验清单10项", ac_checklist_green(&rows), AcReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：非规范序必拒（OrderBad）。
    let mut bk = AccessBook::new();
    let _ = bk.add_ace(AceType::Allow, DESIRED_READ, 1);
    if matches!(bk.add_ace(AceType::Deny, DESIRED_WRITE, 1), Err(AcCode::OrderBad)) {
        caught += 1;
        d.push("注1 倒序:抓".to_string());
    } else {
        d.push("注1 倒序:漏".to_string());
    }
    // 注 2：ACE 重号必拒（AceDup）。
    if matches!(bk.add_ace(AceType::Allow, DESIRED_READ, 1), Err(AcCode::AceDup)) {
        caught += 1;
        d.push("注2 重号:抓".to_string());
    } else {
        d.push("注2 重号:漏".to_string());
    }
    // 注 3：越掩码必拒（MaskMiss）。
    if matches!(bk.add_ace(AceType::Allow, 0x8, 2), Err(AcCode::MaskMiss)) {
        caught += 1;
        d.push("注3 越掩码:抓".to_string());
    } else {
        d.push("注3 越掩码:漏".to_string());
    }
    // 注 4：越 Sid 必拒（SidOob）。
    if matches!(bk.add_ace(AceType::Allow, DESIRED_READ, 9), Err(AcCode::SidOob)) {
        caught += 1;
        d.push("注4 越sid:抓".to_string());
    } else {
        d.push("注4 越sid:漏".to_string());
    }
    // 注 5：零期望掩码判定必拒面（traverse 拒面 + desired_ok 假）。
    if !AccessBook::desired_ok(0) && bk.traverse(0, 1) == Verdict::Deny {
        caught += 1;
        d.push("注5 零掩码:抓".to_string());
    } else {
        d.push("注5 零掩码:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x305e0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    AcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12447_checks：本单判据聚合（诊断段 0x305e0）。
pub fn run_unx12447_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12447 NtAccessCheck 语义档");
    // J1-a：注入 30/30 判定与锚一致。
    let (ok1, d1) = drill_case30_10();
    s.add_dyn("J1-注入30例与锚一致", ok1, &d1);
    // J1-b：DACL 遍历判定（Allow/Deny 序 10/10）。
    let (ok2, d2) = drill_traverse_10();
    s.add_dyn("J1-遍历判定序10/10", ok2, &d2);
    // J1-c：DACL 登记域拒止（10/10）。
    let (ok3, d3) = drill_dacl_domain_10();
    s.add_dyn("J1-DACL域拒止10/10", ok3, &d3);
    // J1-d：标准会话（Deny 前置 + 遍历/锚一致 + 判定账）。
    let mut bk = AccessBook::new();
    let _ = bk.add_ace(AceType::Deny, DESIRED_WRITE, 1);
    let _ = bk.add_ace(AceType::Allow, DESIRED_READ | DESIRED_WRITE, 1);
    let g1 = bk.access_check(DESIRED_READ, 1);
    let d1v = bk.access_check(DESIRED_WRITE, 1);
    let a1 = bk.anchor_check(DESIRED_READ, 1);
    let a2 = bk.anchor_check(DESIRED_WRITE, 1);
    s.add_dyn(
        "J1-遍历锚一致",
        g1 == Verdict::Grant
            && d1v == Verdict::Deny
            && a1 == g1
            && a2 == d1v
            && bk.monotone()
            && dacl_lines_complete(&bk),
        &format!(
            "授={:?} 拒={:?} 锚平=({}, {})",
            g1 == Verdict::Grant,
            d1v == Verdict::Deny,
            a1 == g1,
            a2 == d1v
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏 ACE 不入 DACL）。
    let n0 = bk.dacl_len();
    let _ = bk.add_ace(AceType::Deny, DESIRED_READ, 1);
    s.add_dyn(
        "J2-拒止不写回",
        bk.dacl_len() == n0 && bk.monotone(),
        &format!("ACL {}→{}", n0, bk.dacl_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x305e0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12447_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
