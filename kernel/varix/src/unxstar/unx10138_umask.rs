//! UNX-F10138 umask 继承正反双判据（掩码达成与位清零误用受控）· 0x93280 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10139-J1 时间戳达成（写后 mtime 更新、读后 atime 更新
//!       10/10 次）；UNX-F10139-J2 非属主改时间误用受控（预期 EPERM
//!       精确复现 10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93280（本卡独占 0x40 窗口，下一卡 0x932C0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_umask_call`："op|oct" 两成分（op ∈ {umask, create}），
//!      oct 十进制、允许越九位入判定层，逐档拒绝；
//!   ② 掩码面 `UmaskFace`：umask 置位（0..=0o777）→ create 按掩码清零
//!      （effective = mode & !mask），清零逐位可复算；
//!   ③ 状态机 `MaskState`：Open→Masked→Applied 单调（禁倒退）；
//!   ④ 注入核 `BadMask`：掩码越九位 / mode 越九位 / 坏调用行 / 清零失准
//!      四类坏样本逐类 10/10 次精确复现 EINVAL/审计档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10138 独占 0x40 窗口，下一卡 0x932C0）。
pub const SEG_BASE: u32 = 0x93280;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x932C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 九位闭集上限。
pub const NINE_MAX: u32 = 0o777;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UmCode {
    /// 掩码越九位（EINVAL 族）
    BadMask,
    /// mode 越九位（EINVAL 族）
    BadMode,
    /// 坏调用行（EINVAL 族）
    BadCall,
    /// 清零失准（审计档）
    ClearDrift,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 掩码账失平
    MaskDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl UmCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            UmCode::BadMask => "掩码越九位被拒：umask 必须 0..=0o777；EINVAL 精确归档",
            UmCode::BadMode => "mode 越九位被拒：create 的 mode 必须 0..=0o777；EINVAL 精确归档",
            UmCode::BadCall => "坏调用行被拒：op 闭集 {umask, create}；请核对调用行",
            UmCode::ClearDrift => "清零失准被拒：effective 必须逐位等于 mode & !mask",
            UmCode::BadOp => "非法 op 被拒：op 闭集 {umask, create}；请核对调用行",
            UmCode::BadNumber => "非数字成分被拒：oct 必须为十进制；请改写",
            UmCode::MissingPart => "成分缺失被拒：调用行必须两段 op|oct；请补成分",
            UmCode::StateRegress => "状态倒退被拒：Open→Masked→Applied 单调",
            UmCode::EmptySummary => "空成文被拒：掩码小结不得为空串；请先置掩码再成文",
            UmCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            UmCode::MaskDrift => "掩码账失平被拒：掩码变更必须逐次入账",
            UmCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [UmCode; 12] = [
        UmCode::BadMask,
        UmCode::BadMode,
        UmCode::BadCall,
        UmCode::ClearDrift,
        UmCode::BadOp,
        UmCode::BadNumber,
        UmCode::MissingPart,
        UmCode::StateRegress,
        UmCode::EmptySummary,
        UmCode::SegmentViolation,
        UmCode::MaskDrift,
        UmCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的掩码调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UmaskCall {
    /// 动词
    pub op: String,
    /// 参数（十进制，允许越九位）
    pub oct: u32,
}

/// 解析 "op|oct" 两段调用行。
pub fn parse_umask_call(line: &str) -> Result<UmaskCall, UmCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 2 {
        return Err(UmCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(UmCode::MissingPart),
    };
    if op != "umask" && op != "create" {
        return Err(UmCode::BadOp);
    }
    let os = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(UmCode::MissingPart),
    };
    if os.is_empty() {
        return Err(UmCode::BadNumber);
    }
    let mut oct: u32 = 0;
    for b in os.bytes() {
        if !b.is_ascii_digit() {
            return Err(UmCode::BadNumber);
        }
        oct = oct.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Ok(UmaskCall { op, oct })
}

// ── 掩码状态机 ────────────────────────────────────────────────────────
/// 掩码生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskState {
    /// 未置掩码
    Open,
    /// 已置掩码
    Masked,
    /// 已应用（create 过）
    Applied,
}

impl MaskState {
    /// 单调推进判定。
    pub fn can_advance(self, next: MaskState) -> bool {
        let rank = |s: MaskState| match s {
            MaskState::Open => 0u8,
            MaskState::Masked => 1,
            MaskState::Applied => 2,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            MaskState::Open => "未置掩码",
            MaskState::Masked => "已置掩码",
            MaskState::Applied => "已应用",
        }
    }
}

// ── 掩码面 ────────────────────────────────────────────────────────────
/// 掩码面：进程掩码 + 建档清零核。
#[derive(Clone, Debug)]
pub struct UmaskFace {
    /// 当前掩码（0..=0o777）
    pub mask: u32,
    /// 生命周期状态
    pub state: MaskState,
    /// 累计置掩码次数
    pub sets: u32,
    /// 累计建档次数
    pub creates: u32,
    /// 建档 mode 账（逐次记录 effective）
    pub effective_log: Vec<u32>,
}

impl UmaskFace {
    /// 初始面：掩码 0（不澄清）。
    pub fn new() -> UmaskFace {
        UmaskFace {
            mask: 0,
            state: MaskState::Open,
            sets: 0,
            creates: 0,
            effective_log: Vec::new(),
        }
    }

    /// umask 置位：越九位逐档拒绝。
    pub fn set_mask(&mut self, mask: u32) -> Result<u32, UmCode> {
        if mask > NINE_MAX {
            return Err(UmCode::BadMask);
        }
        let old = self.mask;
        self.mask = mask;
        self.sets = self.sets.saturating_add(1);
        if self.state.can_advance(MaskState::Masked) {
            self.state = MaskState::Masked;
        }
        Ok(old)
    }

    /// create：mode 越九位拒绝；effective = mode & !mask 逐位清零入账。
    pub fn create(&mut self, mode: u32) -> Result<u32, UmCode> {
        if mode > NINE_MAX {
            return Err(UmCode::BadMode);
        }
        let effective = mode & !self.mask;
        let _ = self.effective_log.push(effective);
        self.creates = self.creates.saturating_add(1);
        if self.state.can_advance(MaskState::Applied) {
            self.state = MaskState::Applied;
        }
        Ok(effective)
    }

    /// 清零审计：全账 effective == mode & !mask（重算可复算）。
    pub fn clear_consistent(&self, modes: &[u32]) -> bool {
        if self.effective_log.len() != modes.len() {
            return false;
        }
        let mut i = 0usize;
        while i < modes.len() {
            let m = match modes.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let e = match self.effective_log.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if e != m & !self.mask {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── J1 达成核：umask 置位→创建清零 10/10 轮 ───────────────────────────
/// 逐轮：置 0o022→create 0o666 得 0o644→create 0o777 得 0o755→清零账。
pub fn run_umask_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut f = UmaskFace::new();
        let mut all_ok = true;
        // 面1：置掩码 0o022。
        all_ok = all_ok && f.set_mask(0o022).is_ok();
        probes += 1;
        // 面2：create 0o666 → 0o644。
        all_ok = all_ok && f.create(0o666) == Ok(0o644);
        probes += 1;
        // 面3：create 0o777 → 0o755。
        all_ok = all_ok && f.create(0o777) == Ok(0o755);
        probes += 1;
        // 面4：清零账逐位一致。
        let modes = Vec::from(&[0o666u32, 0o777] as &[u32]);
        all_ok = all_ok && f.clear_consistent(&modes);
        probes += 1;
        // 面5：置掩码 0 后 create 全放行。
        let _ = f.set_mask(0);
        all_ok = all_ok && f.create(0o666) == Ok(0o666);
        probes += 1;
        // 面6：计数守恒。
        all_ok = all_ok && f.sets == 2 && f.creates == 3;
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏掩码矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMask {
    /// 掩码越九位（EINVAL 族）
    BadMask,
    /// mode 越九位（EINVAL 族）
    BadMode,
    /// 坏调用行（EINVAL 族）
    BadCall,
    /// 清零失准（审计档）
    ClearDrift,
}

impl BadMask {
    /// 闭集全量。
    pub const ALL: [BadMask; 4] = [
        BadMask::BadMask,
        BadMask::BadMode,
        BadMask::BadCall,
        BadMask::ClearDrift,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadMask::BadMask => "掩码越九位",
            BadMask::BadMode => "mode 越九位",
            BadMask::BadCall => "坏调用行",
            BadMask::ClearDrift => "清零失准注入",
        }
    }
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), UmCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(UmCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏掩码注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_mask_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadMask::ALL.len() {
        let case = match BadMask::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut f = UmaskFace::new();
            let caught = match case {
                BadMask::BadMask => {
                    f.set_mask(NINE_MAX + 1).err() == Some(UmCode::BadMask)
                }
                BadMask::BadMode => {
                    f.create(NINE_MAX + 1).err() == Some(UmCode::BadMode)
                }
                BadMask::BadCall => {
                    parse_umask_call("chmod|22").err() == Some(UmCode::BadOp)
                        && parse_umask_call("umask|x").err() == Some(UmCode::BadNumber)
                }
                BadMask::ClearDrift => {
                    // 注入：手工污染 effective_log 后审计必须抓到。
                    let _ = f.set_mask(0o022);
                    let _ = f.create(0o666);
                    if let Some(slot) = f.effective_log.get_mut(0) {
                        *slot = 0o666;
                    }
                    let modes = Vec::from(&[0o666u32] as &[u32]);
                    !f.clear_consistent(&modes)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 掩码小结成文（纯函数） ────────────────────────────────────────────
/// 八进制成文：「mask=<o> creates=<n>」。
pub fn compose_umask_summary(f: &UmaskFace) -> Result<String, UmCode> {
    let out = format!("mask=0o{:03o} creates={}", f.mask, f.creates);
    if out.is_empty() {
        return Err(UmCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越九位/坏行族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料指纹：置掩码→建档→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut f = UmaskFace::new();
    let _ = f.set_mask(0o027);
    let _ = f.create(0o666);
    let _ = f.create(0o775);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_umask_summary(&f) {
        bytes.extend_from_slice(s.as_bytes());
    }
    let mut i = 0usize;
    while i < f.effective_log.len() {
        if let Some(e) = f.effective_log.get(i) {
            bytes.extend_from_slice(&e.to_le_bytes());
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10138 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10138_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10138");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93280..=0x932BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < UmCode::ALL.len() {
        let mut j = i + 1;
        while j < UmCode::ALL.len() {
            if let (Some(a), Some(b)) = (UmCode::ALL.get(i), UmCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = UmCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_umask_call("umask|18").is_ok()
        && parse_umask_call("chmod|22").err() == Some(UmCode::BadOp)
        && parse_umask_call("umask|x").err() == Some(UmCode::BadNumber)
        && parse_umask_call("umask").err() == Some(UmCode::MissingPart);
    s.add("调用行解析器：op 双词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {umask, create}；oct 十进制（越九位判定层拒）");

    // 4. J1 达成核
    let (ok, probes) = run_umask_rounds();
    s.add("J1 掩码达成：umask 置位→创建文件 mode 按掩码清零 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（置掩码/清零×2/账平/归零放行/计数）全对");

    // 5. J2 不可设置位
    let mut f = UmaskFace::new();
    let bad_ok = f.set_mask(NINE_MAX + 1).err() == Some(UmCode::BadMask)
        && f.create(NINE_MAX + 1).err() == Some(UmCode::BadMode)
        && f.mask == 0;
    s.add("J2 不可设置位受控：EINVAL 双入口精确复现且掩码不动", bad_ok,
          "越九位双入口逐档拒；被拒置位不改掩码");

    // 6. 清零逐位可复算
    let _ = f.set_mask(0o022);
    let e1 = f.create(0o666);
    let e2 = f.create(0o777);
    let calc_ok = e1 == Ok(0o644) && e2 == Ok(0o755)
        && (0o666 & !0o022) == 0o644 && (0o777 & !0o022) == 0o755;
    s.add("清零复算：effective == mode & !mask 逐位成立", calc_ok,
          "清零是 umask 唯一语义；位运算必须逐位可复核");

    // 7. 清零失准注入
    let mut f2 = UmaskFace::new();
    let _ = f2.set_mask(0o022);
    let _ = f2.create(0o666);
    let inject_ok = {
        let mut caught = true;
        if let Some(slot) = f2.effective_log.get_mut(0) {
            *slot = 0o666;
        }
        let modes = Vec::from(&[0o666u32] as &[u32]);
        caught = !f2.clear_consistent(&modes);
        caught
    };
    s.add("清零失准注入：污染账必被抓", inject_ok,
          "审计必须能抓到任何清零账污染");

    // 8. 归零掩码放行
    let _ = f.set_mask(0);
    let zero_ok = f.create(0o666) == Ok(0o666);
    s.add("归零掩码：umask(0) 后 create 全放行", zero_ok,
          "0o000 掩码不清任何位；语义必须双向可验证");

    // 9. 注入核
    let (inj, per_class) = run_bad_mask_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "越位双入口/坏行/失准注入必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadMask::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = MaskState::Open.can_advance(MaskState::Applied)
        && MaskState::Masked.can_advance(MaskState::Applied)
        && !MaskState::Applied.can_advance(MaskState::Open)
        && MaskState::Applied.label() == "已应用";
    s.add("状态机：Open→Masked→Applied 单调", sm_ok,
          "掩码生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_umask_summary(&f);
    let text_ok = match &sum {
        Ok(x) => x.contains("mask=0o000"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结八进制可复算且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("掩码达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_umask_call("umask|18").is_ok());
        assert_eq!(parse_umask_call("chmod|22").err(), Some(UmCode::BadOp));
        assert_eq!(parse_umask_call("umask|x").err(), Some(UmCode::BadNumber));
        assert_eq!(parse_umask_call("umask").err(), Some(UmCode::MissingPart));
    }

    #[test]
    fn mask_clears_bits() {
        let mut f = UmaskFace::new();
        let _ = f.set_mask(0o022);
        assert_eq!(f.create(0o666), Ok(0o644));
        assert_eq!(f.create(0o777), Ok(0o755));
        assert_eq!(f.create(0o600), Ok(0o600));
    }

    #[test]
    fn bad_mask_blocked() {
        let mut f = UmaskFace::new();
        assert_eq!(f.set_mask(NINE_MAX + 1).err(), Some(UmCode::BadMask));
        assert_eq!(f.mask, 0);
    }

    #[test]
    fn bad_mode_blocked() {
        let mut f = UmaskFace::new();
        assert_eq!(f.create(NINE_MAX + 1).err(), Some(UmCode::BadMode));
        assert_eq!(f.creates, 0);
    }

    #[test]
    fn zero_mask_passes_all() {
        let mut f = UmaskFace::new();
        let _ = f.set_mask(0);
        assert_eq!(f.create(0o777), Ok(0o777));
    }

    #[test]
    fn clear_consistent_detects_pollution() {
        let mut f = UmaskFace::new();
        let _ = f.set_mask(0o022);
        let _ = f.create(0o666);
        if let Some(slot) = f.effective_log.get_mut(0) {
            *slot = 0o666;
        }
        let modes = Vec::from(&[0o666u32] as &[u32]);
        assert!(!f.clear_consistent(&modes));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_mask_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_umask_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(MaskState::Open.can_advance(MaskState::Masked));
        assert!(!MaskState::Applied.can_advance(MaskState::Masked));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10138_checks_green() {
        let set = run_unx10138_checks();
        assert!(set.all_passed(), "UNX-F10138 red: {}", set.red_items_text());
    }
}
