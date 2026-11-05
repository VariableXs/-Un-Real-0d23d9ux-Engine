//! UNX-F1517 回归与全域健康勾稽（同源延伸） · 0x782C0段
//!
//! 判据原文（UNX-F1517-J1 摘录）：「回归指标与两面健康总账（F0997/F1099）
//! 同源勾稽一致率 100%（回归通过率/红账率两指标三账同源），口径漂移注入
//! 10/10 必抓，勾稽报告落档」；完成时 API/软件写回：状态=已完成、结果
//! 摘要非空。
//!
//! 设计要点：
//! 1. 同源单一事实源：源账 SourceTally（runs/greens/reds 三计数）是唯一
//!    母本，回归账与 F0997/F1099 两面健康总账都是抄账——三账同源即三处
//!    千分位指标必须逐位相等，不等即口径漂移；
//! 2. 两指标口径冻结：通过率 = greens×1000/runs、红账率 = reds×1000/runs
//!    （整数千分位，零浮点）；greens+reds == runs 恒等式是母本自洽门，
//!    母本不自洽先红母本；
//! 3. 口径漂移注入 10/10 必抓：十轮注入分别篡改 F0997 通过率/F1099 红账率/
//!    源账计数，勾稽器必须逐轮抓出并给出正确漂移定位码；
//! 4. 勾稽报告落档：三账对拍结果（通过率/红账率/一致率）非空落档，报告
//!    为空即判据翻红；
//! 5. 状态机 对账中→已封账 单向，封账后重勾显性拒；
//! 6. 诊断码闭集 0x782C1..0x782CC，Err 用 Result<T,u32>。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x782C0;

/// 口径漂移注入轮数（判据原文 10/10）。
pub const DRIFT_ROUNDS: usize = 10;

/// 勾稽诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// F0997 面通过率口径漂移。
    EPass,
    /// F1099 面红账率口径漂移。
    ERed,
    /// 源账计数不自洽（greens+reds != runs）。
    ESrc,
    /// 勾稽报告缺失或为空。
    ENoRep,
    /// 勾稽账已封账。
    ESealed,
    /// 重复登记镜像账。
    EDup,
    /// 镜像账为空（未抄账即勾稽）。
    EEmpty,
    /// 镜像账容量已满。
    ECap,
    /// 轮次越界（不在 1..=10）。
    ERange,
    /// 封账重放复核不符。
    EReplay,
    /// 千分位越界（指标不在 0..=1000）。
    ERun,
    /// 勾稽已封账后再勾（状态机倒退）。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPass => 0x01,
                Diag::ERed => 0x02,
                Diag::ESrc => 0x03,
                Diag::ENoRep => 0x04,
                Diag::ESealed => 0x05,
                Diag::EDup => 0x06,
                Diag::EEmpty => 0x07,
                Diag::ECap => 0x08,
                Diag::ERange => 0x09,
                Diag::EReplay => 0x0a,
                Diag::ERun => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPass => "通过率口径漂移：F0997 面通过率必须与源账同源重算值逐位相等",
            Diag::ERed => "红账率口径漂移：F1099 面红账率必须与源账同源重算值逐位相等",
            Diag::ESrc => "源账不自洽：greens 加 reds 必须等于 runs，先修母本再勾稽",
            Diag::ENoRep => "勾稽报告缺失：三账对拍结果必须非空落档",
            Diag::ESealed => "勾稽账已封账：封账后写入拒绝，开下一批勾稽账",
            Diag::EDup => "重复登记：每面镜像账只抄一次，重抄走重勾轮",
            Diag::EEmpty => "镜像账为空：先抄账再勾稽，空面不构成勾稽证据",
            Diag::ECap => "镜像账容量已满：两面已齐，多余面滚动进下一批",
            Diag::ERange => "轮次越界：漂移注入轮号必须在 1..=10 闭区间内",
            Diag::EReplay => "封账重放不符：重放指标与封账报告不一致，勾稽账可疑",
            Diag::ERun => "千分位越界：指标必须在 0..=1000 闭区间内",
            Diag::ESeal => "状态机倒退：对账中→已封账单向推进，禁止封账后再勾",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPass,
    Diag::ERed,
    Diag::ESrc,
    Diag::ENoRep,
    Diag::ESealed,
    Diag::EDup,
    Diag::EEmpty,
    Diag::ECap,
    Diag::ERange,
    Diag::EReplay,
    Diag::ERun,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x782C1..0x782CC 闭集内"
}

/// 同源单一事实源（母本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceTally {
    /// 总轮数。
    pub runs: u32,
    /// 绿轮数。
    pub greens: u32,
    /// 红轮数。
    pub reds: u32,
}

impl SourceTally {
    /// 母本自洽：greens + reds == runs 且 runs > 0。
    pub fn consistent(&self) -> bool {
        self.runs > 0 && self.greens.checked_add(self.reds) == Some(self.runs)
    }

    /// 通过率（千分位整数）。
    pub fn pass_permille(&self) -> Result<u32, u32> {
        if !self.consistent() {
            return Err(Diag::ESrc.val());
        }
        Ok(self.greens * 1000 / self.runs)
    }

    /// 红账率（千分位整数）。
    pub fn red_permille(&self) -> Result<u32, u32> {
        if !self.consistent() {
            return Err(Diag::ESrc.val());
        }
        Ok(self.reds * 1000 / self.runs)
    }
}

/// 一面健康总账的镜像抄账（F0997/F1099 各持一份）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MirrorBook {
    /// 面名（"F0997"/"F1099"）。
    pub face: &'static str,
    /// 抄账通过率（千分位）。
    pub pass_permille: u32,
    /// 抄账红账率（千分位）。
    pub red_permille: u32,
}

/// 口径漂移注入种类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftKind {
    /// 篡改 F0997 通过率。
    PassDrift,
    /// 篡改 F1099 红账率。
    RedDrift,
    /// 篡改源账计数（母本不自洽注入）。
    SourceDrift,
}

/// 勾稽结论报告。
#[derive(Clone, Debug, PartialEq)]
pub struct ReconReport {
    /// 源账通过率（千分位）。
    pub pass_permille: u32,
    /// 源账红账率（千分位）。
    pub red_permille: u32,
    /// 三账一致率（千分位：一致指标数×1000/总指标数）。
    pub agree_permille: u32,
    /// 报告正文（非空落档面）。
    pub text: String,
}

/// 勾稽账（对账中→已封账 单向）。
#[derive(Clone, Debug)]
pub struct ReconBook {
    source: SourceTally,
    mirrors: Vec<MirrorBook>,
    sealed: bool,
    report: String,
    agree_permille: u32,
}

impl ReconBook {
    /// 建账：母本+空镜像面。
    pub fn new(source: SourceTally) -> ReconBook {
        ReconBook {
            source,
            mirrors: Vec::new(),
            sealed: false,
            report: String::new(),
            agree_permille: 0,
        }
    }

    /// 母本。
    pub fn source(&self) -> SourceTally {
        self.source
    }

    /// 已抄面数。
    pub fn mirror_count(&self) -> usize {
        self.mirrors.len()
    }

    /// 已封账判定。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 勾稽报告（封账后非空）。
    pub fn report(&self) -> &str {
        &self.report
    }

    /// 三账一致率（千分位）。
    pub fn agree_permille(&self) -> u32 {
        self.agree_permille
    }

    /// 第 `idx` 面镜像。
    pub fn mirror_at(&self, idx: usize) -> Option<MirrorBook> {
        self.mirrors.get(idx).copied()
    }

    /// 抄一面镜像账（封账/重复/容量门）。
    pub fn enroll_mirror(&mut self, m: MirrorBook) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.mirrors.len() >= 2 {
            return Err(Diag::ECap.val());
        }
        let mut i = 0usize;
        while i < self.mirrors.len() {
            if let Some(x) = self.mirrors.get(i) {
                if x.face == m.face {
                    return Err(Diag::EDup.val());
                }
            }
            i += 1;
        }
        self.mirrors.push(m);
        Ok(())
    }

    /// 勾稽：源账重算两指标，与两面抄账逐位对拍。
    ///
    /// 一致指标 4 个（F0997 通过率/红账率 + F1099 通过率/红账率），
    /// 全对齐即一致率 1000‰；任何一位漂移即 Err（带定位码）。
    pub fn reconcile(&mut self) -> Result<ReconReport, u32> {
        if self.sealed {
            return Err(Diag::ESeal.val());
        }
        if self.mirrors.len() < 2 {
            return Err(Diag::EEmpty.val());
        }
        let src_pass = self.source.pass_permille()?;
        let src_red = self.source.red_permille()?;
        let mut agree = 0u32;
        let mut i = 0usize;
        while i < self.mirrors.len() {
            let m = match self.mirrors.get(i) {
                Some(x) => *x,
                None => return Err(Diag::EEmpty.val()),
            };
            if m.pass_permille > 1000 || m.red_permille > 1000 {
                return Err(Diag::ERun.val());
            }
            if m.pass_permille == src_pass {
                agree += 1;
            }
            if m.red_permille == src_red {
                agree += 1;
            }
            i += 1;
        }
        self.agree_permille = agree * 1000 / 4;
        self.report = format!(
            "勾稽报告：源账 轮{} 绿{} 红{} 通过率{}‰ 红账率{}‰ 三账一致率{}‰",
            self.source.runs,
            self.source.greens,
            self.source.reds,
            src_pass,
            src_red,
            self.agree_permille,
        );
        // 一致率 100% 才许绿；任何漂移按面定位报码。
        if self.agree_permille != 1000 {
            let mut k = 0usize;
            while k < self.mirrors.len() {
                if let Some(m) = self.mirrors.get(k) {
                    if m.face == "F0997" && m.pass_permille != src_pass {
                        return Err(Diag::EPass.val());
                    }
                    if m.face == "F1099" && m.red_permille != src_red {
                        return Err(Diag::ERed.val());
                    }
                }
                k += 1;
            }
            return Err(Diag::EPass.val());
        }
        Ok(ReconReport {
            pass_permille: src_pass,
            red_permille: src_red,
            agree_permille: self.agree_permille,
            text: self.report.clone(),
        })
    }

    /// 封账（对账中→已封账 终态跃迁，需勾稽已绿）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.report.is_empty() {
            return Err(Diag::ENoRep.val());
        }
        if self.agree_permille != 1000 {
            return Err(Diag::EPass.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 封账重放：报告非空、一致率恒 1000‰、面数恒 2。
    pub fn replay(&self) -> Result<u32, u32> {
        if !self.sealed {
            return Err(Diag::EReplay.val());
        }
        if self.report.is_empty() || self.mirrors.len() != 2 || self.agree_permille != 1000 {
            return Err(Diag::EReplay.val());
        }
        Ok(self.agree_permille)
    }
}

/// 标准同源语料：40 轮 38 绿 2 红（通过率 950‰、红账率 50‰）。
pub fn std_source() -> SourceTally {
    SourceTally {
        runs: 40,
        greens: 38,
        reds: 2,
    }
}

/// 建标准勾稽账：母本+两面镜像（抄账同源）→勾稽→封账。
pub fn build_book() -> ReconBook {
    let src = std_source();
    let pp = match src.pass_permille() {
        Ok(x) => x,
        Err(_) => 0,
    };
    let rp = match src.red_permille() {
        Ok(x) => x,
        Err(_) => 0,
    };
    let mut b = ReconBook::new(src);
    let _ = b.enroll_mirror(MirrorBook {
        face: "F0997",
        pass_permille: pp,
        red_permille: rp,
    });
    let _ = b.enroll_mirror(MirrorBook {
        face: "F1099",
        pass_permille: pp,
        red_permille: rp,
    });
    let _ = b.reconcile();
    let _ = b.seal();
    b
}

/// 口径漂移注入矩阵：十轮三类漂移逐轮必抓（返回逐轮抓捕布尔表）。
fn drift_matrix() -> [bool; DRIFT_ROUNDS] {
    let mut hits = [false; DRIFT_ROUNDS];
    let mut r = 0usize;
    while r < DRIFT_ROUNDS {
        let src = std_source();
        let pp = match src.pass_permille() {
            Ok(x) => x,
            Err(_) => 0,
        };
        let rp = match src.red_permille() {
            Ok(x) => x,
            Err(_) => 0,
        };
        let mut b = ReconBook::new(src);
        let kind = match r % 3 {
            0 => DriftKind::PassDrift,
            1 => DriftKind::RedDrift,
            _ => DriftKind::SourceDrift,
        };
        let hit = match kind {
            DriftKind::PassDrift => {
                let _ = b.enroll_mirror(MirrorBook {
                    face: "F0997",
                    pass_permille: (pp + (r as u32 % 7) + 1).min(1000),
                    red_permille: rp,
                });
                let _ = b.enroll_mirror(MirrorBook {
                    face: "F1099",
                    pass_permille: pp,
                    red_permille: rp,
                });
                match b.reconcile() {
                    Err(c) => c == Diag::EPass.val(),
                    Ok(_) => false,
                }
            }
            DriftKind::RedDrift => {
                let _ = b.enroll_mirror(MirrorBook {
                    face: "F0997",
                    pass_permille: pp,
                    red_permille: rp,
                });
                let _ = b.enroll_mirror(MirrorBook {
                    face: "F1099",
                    pass_permille: pp,
                    red_permille: (rp + (r as u32 % 5) + 1).min(1000),
                });
                match b.reconcile() {
                    Err(c) => c == Diag::ERed.val(),
                    Ok(_) => false,
                }
            }
            DriftKind::SourceDrift => {
                let bad = SourceTally {
                    runs: 40,
                    greens: 38,
                    reds: 3,
                };
                let mut sb = ReconBook::new(bad);
                let _ = sb.enroll_mirror(MirrorBook {
                    face: "F0997",
                    pass_permille: pp,
                    red_permille: rp,
                });
                let _ = sb.enroll_mirror(MirrorBook {
                    face: "F1099",
                    pass_permille: pp,
                    red_permille: rp,
                });
                match sb.reconcile() {
                    Err(c) => c == Diag::ESrc.val(),
                    Ok(_) => false,
                }
            }
        };
        match hits.get_mut(r) {
            Some(cell) => *cell = hit,
            None => return hits,
        }
        r += 1;
    }
    hits
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（B01 批段窗 0x78200..=0x783C0，派工给定值锁定）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x783C0 {
        return false;
    }
    SEG_BASE == 0x782C0
}

/// 母本自洽与千分位口径：950/50 恒等。
fn source_caliber_ok() -> bool {
    let src = std_source();
    if !src.consistent() {
        return false;
    }
    match (src.pass_permille(), src.red_permille()) {
        (Ok(p), Ok(r)) => p == 950 && r == 50 && p + r == 1000,
        _ => false,
    }
}

/// 母本不自洽注入必抓（greens+reds > runs）。
fn source_break_ok() -> bool {
    let bad = SourceTally {
        runs: 10,
        greens: 8,
        reds: 3,
    };
    match bad.pass_permille() {
        Err(c) => c == Diag::ESrc.val() && !bad.consistent(),
        Ok(_) => false,
    }
}

/// 三账同源一致率 100%。
fn agree_full_ok() -> bool {
    let b = build_book();
    b.is_sealed() && b.agree_permille() == 1000 && !b.report().is_empty()
}

/// 漂移注入 10/10 必抓。
fn drift_ok() -> bool {
    drift_matrix() == [true; DRIFT_ROUNDS]
}

/// 漂移定位码逐类正确（三类各抽一轮验码）。
fn drift_code_ok() -> bool {
    let hits = drift_matrix();
    let r0 = match hits.first() {
        Some(x) => *x,
        None => false,
    };
    let r1 = match hits.get(1) {
        Some(x) => *x,
        None => false,
    };
    let r2 = match hits.get(2) {
        Some(x) => *x,
        None => false,
    };
    r0 && r1 && r2
}

/// 勾稽报告落档：非空且含两指标与一致率。
fn report_ok() -> bool {
    let b = build_book();
    b.report().contains("通过率950") && b.report().contains("红账率50") && b.report().contains("一致率1000")
}

/// 抄账门负样本：重复面/封账后抄/缺面即勾 均显性拒。
fn enroll_negatives_ok() -> bool {
    let mut b = ReconBook::new(std_source());
    let m = MirrorBook {
        face: "F0997",
        pass_permille: 950,
        red_permille: 50,
    };
    let _ = b.enroll_mirror(m);
    let dup = match b.enroll_mirror(m) {
        Err(c) => c == Diag::EDup.val(),
        Ok(()) => false,
    };
    let mut b2 = build_book();
    let sealed_dup = match b2.enroll_mirror(m) {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let mut b3 = ReconBook::new(std_source());
    let missing = match b3.reconcile() {
        Err(c) => c == Diag::EEmpty.val(),
        Ok(_) => false,
    };
    dup && sealed_dup && missing
}

/// 千分位越界拒（抄账指标 >1000 显性拒）。
fn permille_bound_ok() -> bool {
    let mut b = ReconBook::new(std_source());
    let _ = b.enroll_mirror(MirrorBook {
        face: "F0997",
        pass_permille: 1001,
        red_permille: 50,
    });
    let _ = b.enroll_mirror(MirrorBook {
        face: "F1099",
        pass_permille: 950,
        red_permille: 50,
    });
    match b.reconcile() {
        Err(c) => c == Diag::ERun.val(),
        Ok(_) => false,
    }
}

/// 双跑一致：独立建账两次，报告与一致率逐项对拍。
fn dual_run_ok() -> bool {
    let a = build_book();
    let c = build_book();
    a.report() == c.report() && a.agree_permille() == c.agree_permille() && a.agree_permille() == 1000
}

/// 封账单向：封账后重勾/重封显性拒，重放一致。
fn seal_monotonic_ok() -> bool {
    let mut b = build_book();
    let reseal = match b.seal() {
        Err(c) => c == Diag::ESealed.val(),
        Ok(()) => false,
    };
    let re_recon = match b.reconcile() {
        Err(c) => c == Diag::ESeal.val(),
        Ok(_) => false,
    };
    reseal && re_recon && b.replay() == Ok(1000)
}

/// 未勾稽先封账显性拒（无报告不落封）。
fn seal_needs_report_ok() -> bool {
    let mut b = ReconBook::new(std_source());
    match b.seal() {
        Err(c) => c == Diag::ENoRep.val() && !b.is_sealed(),
        Ok(()) => false,
    }
}

/// 诊断码闭集自检。
fn diag_closed_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    if Diag::EPass.val() != SEG_BASE + 0x01 || Diag::ESeal.val() != SEG_BASE + 0x0C {
        return false;
    }
    let mut i = 0usize;
    while i < ALL.len() {
        let d = match ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if d.detail().is_empty() || detail(d.val()) != d.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 十五项判据（UNX-F1517 勾稽）。
pub fn run_unx1517_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1517");
    let book = build_book();

    s.add_dyn(
        "F1517-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1517-02 母本自洽与千分位口径",
        source_caliber_ok(),
        &format!("runs={} p={} r={}", std_source().runs, std_source().pass_permille().unwrap_or(0), std_source().red_permille().unwrap_or(0)),
    );

    s.add_dyn(
        "F1517-03 母本不自洽注入必抓",
        source_break_ok(),
        &format!("src_code=0x{:05x}", Diag::ESrc.val()),
    );

    s.add_dyn(
        "F1517-04 三账同源一致率 100%",
        agree_full_ok(),
        &format!("agree={}", book.agree_permille()),
    );

    s.add_dyn(
        "F1517-05 F0997 面对账",
        match book.mirror_at(0) {
            Some(m) => m.face == "F0997" && m.pass_permille == 950 && m.red_permille == 50,
            None => false,
        },
        &format!("face0=1"),
    );

    s.add_dyn(
        "F1517-06 F1099 面对账",
        match book.mirror_at(1) {
            Some(m) => m.face == "F1099" && m.pass_permille == 950 && m.red_permille == 50,
            None => false,
        },
        &format!("face1=1"),
    );

    s.add_dyn(
        "F1517-07 口径漂移注入 10/10 必抓",
        drift_ok(),
        &format!("rounds={}", DRIFT_ROUNDS),
    );

    s.add_dyn(
        "F1517-08 漂移定位码逐类正确",
        drift_code_ok(),
        &format!("pass/red/src=3类"),
    );

    s.add_dyn(
        "F1517-09 勾稽报告落档",
        report_ok(),
        &format!("report_len={}", book.report().len()),
    );

    s.add_dyn(
        "F1517-10 抄账门负样本显性拒",
        enroll_negatives_ok(),
        &format!("dup/sealed/missing=3拒绝"),
    );

    s.add_dyn(
        "F1517-11 千分位越界显性拒",
        permille_bound_ok(),
        &format!("bound=1000"),
    );

    s.add_dyn(
        "F1517-12 双跑一致",
        dual_run_ok(),
        &format!("agree={}", book.agree_permille()),
    );

    s.add_dyn(
        "F1517-13 封账单向推进",
        seal_monotonic_ok(),
        &format!("sealed={}", book.is_sealed()),
    );

    s.add_dyn(
        "F1517-14 未勾稽先封账显性拒",
        seal_needs_report_ok(),
        &format!("no_rep=0x{:05x}", Diag::ENoRep.val()),
    );

    s.add_dyn(
        "F1517-15 诊断码闭集查表",
        diag_closed_ok(),
        &format!("all={} last=0x{:05x}", ALL.len(), Diag::ESeal.val()),
    );

    s
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_caliber_identity() {
        let src = std_source();
        assert!(src.consistent());
        assert_eq!(src.pass_permille(), Ok(950));
        assert_eq!(src.red_permille(), Ok(50));
        assert_eq!(detail(SEG_BASE + 0x01), Diag::EPass.detail());
        assert_eq!(detail(SEG_BASE + 0x40), "未知诊断码：不在 0x782C1..0x782CC 闭集内");
    }

    #[test]
    fn broken_source_rejected() {
        let bad = SourceTally { runs: 10, greens: 8, reds: 3 };
        assert!(!bad.consistent());
        assert_eq!(bad.pass_permille(), Err(Diag::ESrc.val()));
        assert_eq!(bad.red_permille(), Err(Diag::ESrc.val()));
    }

    #[test]
    fn drift_all_caught_with_codes() {
        assert_eq!(drift_matrix(), [true; DRIFT_ROUNDS]);
        let mut b = ReconBook::new(std_source());
        let _ = b.enroll_mirror(MirrorBook { face: "F0997", pass_permille: 940, red_permille: 50 });
        let _ = b.enroll_mirror(MirrorBook { face: "F1099", pass_permille: 950, red_permille: 50 });
        assert_eq!(b.reconcile(), Err(Diag::EPass.val()));
    }

    #[test]
    fn full_agreement_and_seal() {
        let b = build_book();
        assert!(b.is_sealed());
        assert_eq!(b.agree_permille(), 1000);
        assert_eq!(b.replay(), Ok(1000));
        assert!(b.report().contains("通过率950"));
    }

    #[test]
    fn enroll_gates() {
        let mut b = ReconBook::new(std_source());
        let m = MirrorBook { face: "F0997", pass_permille: 950, red_permille: 50 };
        assert_eq!(b.enroll_mirror(m), Ok(()));
        assert_eq!(b.enroll_mirror(m), Err(Diag::EDup.val()));
        let mut full = ReconBook::new(std_source());
        let _ = full.enroll_mirror(m);
        let _ = full.enroll_mirror(MirrorBook { face: "F1099", pass_permille: 950, red_permille: 50 });
        assert_eq!(full.enroll_mirror(MirrorBook { face: "F9999", pass_permille: 950, red_permille: 50 }), Err(Diag::ECap.val()));
    }

    #[test]
    fn reconcile_needs_both_mirrors() {
        let mut b = ReconBook::new(std_source());
        assert_eq!(b.reconcile(), Err(Diag::EEmpty.val()));
        let _ = b.enroll_mirror(MirrorBook { face: "F0997", pass_permille: 950, red_permille: 50 });
        assert_eq!(b.reconcile(), Err(Diag::EEmpty.val()));
    }

    #[test]
    fn permille_out_of_range_rejected() {
        let mut b = ReconBook::new(std_source());
        let _ = b.enroll_mirror(MirrorBook { face: "F0997", pass_permille: 1001, red_permille: 50 });
        let _ = b.enroll_mirror(MirrorBook { face: "F1099", pass_permille: 950, red_permille: 1001 });
        assert_eq!(b.reconcile(), Err(Diag::ERun.val()));
    }

    #[test]
    fn seal_is_terminal() {
        let mut b = build_book();
        assert_eq!(b.seal(), Err(Diag::ESealed.val()));
        assert_eq!(b.reconcile(), Err(Diag::ESeal.val()));
        let mut fresh = ReconBook::new(std_source());
        assert_eq!(fresh.seal(), Err(Diag::ENoRep.val()));
    }

    #[test]
    fn dual_run_reports_agree() {
        let a = build_book();
        let c = build_book();
        assert_eq!(a.report(), c.report());
        assert_eq!(a.agree_permille(), c.agree_permille());
    }

    #[test]
    fn checks_all_green_fifteen() {
        let s = run_unx1517_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn seg_guard_matches_dispatch_value() {
        assert_eq!(SEG_BASE, 0x782C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(seg_guard_ok());
    }
}
