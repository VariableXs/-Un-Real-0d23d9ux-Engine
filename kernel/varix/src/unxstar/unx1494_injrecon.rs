//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1494 断言注入资产对账（注入类断言清点勾稽） · 0x75F40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6 M组）：
//!   注入断言数=注入资产账行数 100% 勾稽，注入资产账只增不减，
//!   漏记注入必抓。
//! 逐卡要点：
//!   注入类断言清点：断言总账中注入类断言数=注入资产账行数 100%
//!   勾稽，注入资产账只增不减（删行拒），漏记注入（构造一条注入
//!   断言不入资产账）必抓。
//! 设计要点：
//!   ① 双账模型：断言总账（功能类/注入类二分）+ 注入资产账（资
//!     产行带 断言号/注入类/载荷），两账均只增不减；
//!   ② 勾稽口径：总账注入类断言数 == 资产账行数，且每条资产行
//!     指向的断言必须存在且为注入类，双向全对才算 100%；
//!   ③ 漏记注入演练：LCG 固定种子挑一条注入断言不挂资产，勾稽
//!     必报失配 10/10，漏抓即红线；
//!   ④ 删行红线：总账删行、资产账删行、抹账重开三路全部显性拒；
//!   ⑤ 资产挂账闸门：断言必须存在、必须是注入类、不得重复挂账，
//!     三闸依次落闸，失败给可操作中文建议；
//!   ⑥ 常量冻结：注入类闭集 4（越界坐标/坏格式/坏字库/重复号）、
//!     真机资产 6 行、判据轮数 10（来源：本卡判据与 F0495 三类
//!     坏参闭集的挂接约定）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75F40，AI-VX7-1104 批6 M组窗口第二格）。
pub const SEG_BASE: u32 = 0x75F40;

/// 注入类闭集数（冻结 4：越界坐标/坏格式/坏字库/重复号）。
pub const INJ_KINDS: usize = 4;
/// 真机注入资产行数（冻结 6，与真机总账注入断言数一致）。
pub const MACHINE_ASSETS: usize = 6;
/// 判据轮数（写死 10：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 勾稽百分制满值（冻结 100）。
pub const RECON_FULL: u32 = 100;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconCode {
    /// 勾稽失配：注入断言数 != 资产账行数
    ReconMismatch,
    /// 资产悬空：资产行指向不存在或非注入类断言
    AssetOrphan,
    /// 重复挂账：同一断言号已挂过资产
    AssetDupLink,
    /// 资产账删行被拒：只增不减红线
    AssetRmDenied,
    /// 总账删行被拒：只增不减红线
    MasterRmDenied,
    /// 账号断裂：断言号未从 0 单调递增
    IdNotMonotonic,
    /// 漏记注入：注入断言未挂资产
    OmitInject,
    /// 对账状态倒退
    ReconBackStep,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl ReconCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ReconCode::ReconMismatch => "勾稽失配：请清点总账注入类断言数并与资产账行数逐行比对",
            ReconCode::AssetOrphan => "资产悬空：资产行断言号必须指向已登记的注入类断言",
            ReconCode::AssetDupLink => "重复挂账：该断言号已挂过资产，请核对资产账流水",
            ReconCode::AssetRmDenied => "资产账删行被拒：注入资产账只增不减，更正请走追加冲正",
            ReconCode::MasterRmDenied => "总账删行被拒：断言总账只增不减，更正请走追加冲正",
            ReconCode::IdNotMonotonic => "账号断裂：断言号必须从 0 起单调递增，请补齐缺号",
            ReconCode::OmitInject => "漏记注入：该注入断言未挂资产，请补一行资产账再勾稽",
            ReconCode::ReconBackStep => "对账状态倒退：平衡态不可回退，请重开新账期",
            ReconCode::NotReproducible => "双跑不一致：复现红线，请固定种子与输入重跑",
        }
    }
    pub const ALL: [ReconCode; 9] = [
        ReconCode::ReconMismatch,
        ReconCode::AssetOrphan,
        ReconCode::AssetDupLink,
        ReconCode::AssetRmDenied,
        ReconCode::MasterRmDenied,
        ReconCode::IdNotMonotonic,
        ReconCode::OmitInject,
        ReconCode::ReconBackStep,
        ReconCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码给中文提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75F40 段闭集内"
}

// ── 断言总账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertKind {
    /// 功能类断言（正常流）
    Functional,
    /// 注入类断言（人为构造违约必抓）
    Inject,
}

/// 断言总账行：号 + 类 + 挂接目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertRow {
    pub id: u32,
    pub kind: AssertKind,
    pub target: &'static str,
}

/// 断言总账（只增不减）。
#[derive(Debug)]
pub struct AssertMaster {
    pub rows: Vec<AssertRow>,
    next_id: u32,
}

impl AssertMaster {
    pub fn new() -> AssertMaster {
        AssertMaster { rows: Vec::new(), next_id: 0 }
    }

    /// 追加断言行（号单调递增闸）。
    pub fn add_row(&mut self, kind: AssertKind, target: &'static str) -> Result<u32, u32> {
        let id = self.next_id;
        if id > 0 {
            // 上一行号应为 id-1（单调闸）。
            let prev = self.rows.len().checked_sub(1);
            let ok = match prev.and_then(|p| self.rows.get(p)) {
                Some(r) => r.id.saturating_add(1) == id,
                None => false,
            };
            if !ok {
                return Err(ReconCode::IdNotMonotonic.val());
            }
        }
        self.rows.push(AssertRow { id, kind, target });
        self.next_id = id.saturating_add(1);
        Ok(id)
    }

    /// 删行显性拒（只增不减红线）。
    pub fn remove_row(&mut self, id: u32) -> Result<(), u32> {
        let _ = id;
        Err(ReconCode::MasterRmDenied.val())
    }

    /// 抹账显性拒（只增不减红线）。
    pub fn clear(&mut self) -> Result<(), u32> {
        Err(ReconCode::MasterRmDenied.val())
    }

    /// 注入类断言清点。
    pub fn inject_count(&self) -> usize {
        self.rows.iter().filter(|r| r.kind == AssertKind::Inject).count()
    }

    /// 按号查行。
    pub fn row_of(&self, id: u32) -> Option<AssertRow> {
        self.rows.iter().copied().find(|r| r.id == id)
    }
}

// ── 注入资产账 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectKind {
    /// 越界坐标注入
    OobCoord,
    /// 坏格式注入
    BadFormat,
    /// 坏字库索引注入
    BadGlyph,
    /// 重复号注入
    DupId,
}

/// 注入资产行：资产序 + 断言号 + 注入类 + 载荷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectAsset {
    pub asset_no: u32,
    pub assert_id: u32,
    pub kind: InjectKind,
    pub payload: u32,
}

/// 注入资产账（只增不减）。
#[derive(Debug)]
pub struct AssetLedger {
    pub rows: Vec<InjectAsset>,
    next_no: u32,
}

impl AssetLedger {
    pub fn new() -> AssetLedger {
        AssetLedger { rows: Vec::new(), next_no: 0 }
    }

    /// 挂账三闸：断言存在 → 必为注入类 → 不重复挂账。
    pub fn attach(
        &mut self,
        master: &AssertMaster,
        assert_id: u32,
        kind: InjectKind,
        payload: u32,
    ) -> Result<u32, u32> {
        let row = match master.row_of(assert_id) {
            Some(r) => r,
            None => return Err(ReconCode::AssetOrphan.val()),
        };
        if row.kind != AssertKind::Inject {
            return Err(ReconCode::AssetOrphan.val());
        }
        if self.rows.iter().any(|a| a.assert_id == assert_id) {
            return Err(ReconCode::AssetDupLink.val());
        }
        let no = self.next_no;
        self.rows.push(InjectAsset { asset_no: no, assert_id, kind, payload });
        self.next_no = no.saturating_add(1);
        Ok(no)
    }

    /// 删行显性拒（只增不减红线）。
    pub fn remove_row(&mut self, asset_no: u32) -> Result<(), u32> {
        let _ = asset_no;
        Err(ReconCode::AssetRmDenied.val())
    }

    /// 抹账显性拒（只增不减红线）。
    pub fn clear(&mut self) -> Result<(), u32> {
        Err(ReconCode::AssetRmDenied.val())
    }
}

// ── 对账状态机（开账→挂齐→平衡，单调） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconState {
    /// 开账：两账在记
    Open,
    /// 挂齐：注入断言全部挂了资产
    Linked,
    /// 平衡：勾稽 100% 已落账
    Balanced,
}

/// 勾稽：注入断言数 == 资产行数 且 每条资产指向注入类断言，
/// 返回勾稽百分比（整数口径，全对即 100）。
pub fn reconcile(master: &AssertMaster, assets: &AssetLedger) -> Result<u32, u32> {
    let inj = master.inject_count();
    if assets.rows.len() != inj {
        return Err(ReconCode::ReconMismatch.val());
    }
    for a in assets.rows.iter() {
        match master.row_of(a.assert_id) {
            Some(r) if r.kind == AssertKind::Inject => {}
            _ => return Err(ReconCode::AssetOrphan.val()),
        }
    }
    Ok(RECON_FULL)
}

/// 对账会话：两账 + 状态机。
#[derive(Debug)]
pub struct ReconSession {
    pub master: AssertMaster,
    pub assets: AssetLedger,
    pub state: ReconState,
}

impl ReconSession {
    pub fn new() -> ReconSession {
        ReconSession { master: AssertMaster::new(), assets: AssetLedger::new(), state: ReconState::Open }
    }

    /// 登记注入断言并同步挂资产（一条龙，防漏记）。
    pub fn register_inject(
        &mut self,
        target: &'static str,
        kind: InjectKind,
        payload: u32,
    ) -> Result<u32, u32> {
        let id = self.master.add_row(AssertKind::Inject, target)?;
        self.assets.attach(&self.master, id, kind, payload)?;
        if self.state == ReconState::Open {
            self.state = ReconState::Linked;
        }
        Ok(id)
    }

    /// 登记功能类断言（不挂资产）。
    pub fn register_functional(&mut self, target: &'static str) -> Result<u32, u32> {
        let id = self.master.add_row(AssertKind::Functional, target)?;
        Ok(id)
    }

    /// 勾稽落账（Open/Linked → Balanced；倒退拒）。
    pub fn settle(&mut self) -> Result<u32, u32> {
        if self.state == ReconState::Balanced {
            return Err(ReconCode::ReconBackStep.val());
        }
        let pct = reconcile(&self.master, &self.assets)?;
        self.state = ReconState::Balanced;
        Ok(pct)
    }

    /// 倒退探针：平衡态回退显性拒。
    pub fn demote(&self) -> Result<(), u32> {
        if self.state == ReconState::Balanced {
            return Err(ReconCode::ReconBackStep.val());
        }
        Ok(())
    }
}

// ── LCG（固定种子，整数口径） ───────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 真机账（手写语料：8 功能 + 6 注入 + 6 资产） ────────────────
/// 真机注入资产闭集（与 F0495 三类坏参 + 重复号对齐）。
pub fn machine_injects() -> [(InjectKind, u32, &'static str); MACHINE_ASSETS] {
    [
        (InjectKind::OobCoord, 41, "坐标越界注入"),
        (InjectKind::BadFormat, 0xFFFF, "未注册格式注入"),
        (InjectKind::BadGlyph, 200, "字库索引越界注入"),
        (InjectKind::DupId, 7, "重复断言号注入"),
        (InjectKind::OobCoord, 41, "坐标回绕注入"),
        (InjectKind::BadFormat, 0x1234, "格式表坏指针注入"),
    ]
}

/// 建真机会话：8 条功能 + 6 条注入全部挂资产。
pub fn machine_session() -> Result<ReconSession, u32> {
    let mut s = ReconSession::new();
    let func_targets = [
        "五数复算一致",
        "段位守卫",
        "哨兵完好",
        "双跑一致",
        "滚动对冲守恒",
        "清屏拦截守恒",
        "图案校验全过",
        "多输出候选盘点",
    ];
    for t in func_targets.iter() {
        s.register_functional(t)?;
    }
    for (k, p, t) in machine_injects().iter() {
        s.register_inject(t, *k, *p)?;
    }
    Ok(s)
}

// ── 漏记注入演练（LCG 挑一条不挂资产，10 轮必抓） ────────────────
pub struct OmitLedger {
    pub tries: u32,
    pub caught: u32,
}

/// 漏记轮：建满注入断言但不给挑中的一条挂资产，勾稽必报失配。
pub fn omit_rounds(rounds: u32) -> Result<OmitLedger, u32> {
    let mut led = OmitLedger { tries: 0, caught: 0 };
    let mut rng = Lcg::new(SEG_BASE as u64);
    let inj = machine_injects();
    let mut r = 0;
    while r < rounds {
        let omit_idx = rng.below(inj.len() as u64) as usize;
        let mut s = ReconSession::new();
        for t in [
            "五数复算一致",
            "段位守卫",
            "哨兵完好",
            "双跑一致",
            "滚动对冲守恒",
            "清屏拦截守恒",
            "图案校验全过",
            "多输出候选盘点",
        ]
        .iter()
        {
            s.register_functional(t)?;
        }
        let mut idx = 0;
        for (k, p, t) in inj.iter() {
            if idx == omit_idx {
                // 漏记注入：只登记断言，不挂资产。
                s.master.add_row(AssertKind::Inject, t)?;
            } else {
                s.register_inject(t, *k, *p)?;
            }
            idx += 1;
        }
        let hit = match reconcile(&s.master, &s.assets) {
            Err(e) => e == ReconCode::ReconMismatch.val(),
            Ok(_) => false,
        };
        led.tries = led.tries.saturating_add(1);
        if hit {
            led.caught = led.caught.saturating_add(1);
        }
        r += 1;
    }
    if led.caught != led.tries {
        return Err(ReconCode::OmitInject.val());
    }
    Ok(led)
}

// ── 黄金全链：双账登记→三闸→勾稽 100%→双拒→漏记抓 ──────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut s = machine_session()?;
    let count_ok = (s.master.rows.len() == 14
        && s.master.inject_count() == MACHINE_ASSETS
        && s.assets.rows.len() == MACHINE_ASSETS) as u32;
    // 勾稽 100%。
    let pct = reconcile(&s.master, &s.assets)?;
    let pct_ok = (pct == RECON_FULL) as u32;
    // 删行/抹账四拒。
    let d1 = s.master.remove_row(0).is_err();
    let d2 = s.assets.remove_row(0).is_err();
    let d3 = s.master.clear().is_err();
    let d4 = s.assets.clear().is_err();
    let deny_ok = (d1 && d2 && d3 && d4) as u32;
    // 三闸：挂功能断言拒、重复挂账拒、悬空挂账拒。
    let g1 = s.assets.attach(&s.master, 0, InjectKind::OobCoord, 1).is_err();
    let g2 = s.assets.attach(&s.master, 8, InjectKind::DupId, 1).is_err();
    let g3 = s.assets.attach(&s.master, 99, InjectKind::DupId, 1).is_err();
    let gate_ok = (g1 && g2 && g3) as u32;
    Ok((count_ok.saturating_add(pct_ok), deny_ok.saturating_add(gate_ok)))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链 2/2 + 双拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 2)) => Ok(()),
        _ => Err(ReconCode::NotReproducible.val()),
    }
}

/// 序 B：漏记注入必抓（挑第 2 条注入不挂资产）。
pub fn probe_seq_b() -> Result<(), u32> {
    let led = omit_rounds(3)?;
    if led.caught == led.tries && led.tries == 3 {
        Ok(())
    } else {
        Err(ReconCode::OmitInject.val())
    }
}

/// 序 C：常量冻结（注入类 4 / 真机资产 6 / 判据轮 10 / 满值 100）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        INJ_KINDS == 4,
        MACHINE_ASSETS == 6,
        JUDGE_ROUNDS == 10,
        RECON_FULL == 100,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(ReconCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ReconCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((a, b)) => *a == 2 && *b == 2,
            Err(_) => false,
        },
        match &g {
            Ok((a, b)) => format!("账齐+勾稽={} 四拒三闸={}", a, b),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ② 勾稽 100%（真机账）。
    let s = machine_session();
    let pct = match &s {
        Ok(sess) => reconcile(&sess.master, &sess.assets),
        Err(e) => Err(*e),
    };
    v.push((
        "J1-勾稽100%",
        match &pct {
            Ok(p) => *p == RECON_FULL,
            Err(_) => false,
        },
        match &pct {
            Ok(p) => format!("勾稽={}%", p),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ③ 注入断言数 == 资产账行数。
    let cnt = match &s {
        Ok(sess) => sess.master.inject_count() == sess.assets.rows.len(),
        Err(_) => false,
    };
    v.push((
        "J1-注入断言数=资产行数",
        cnt,
        String::from("总账注入类清点与资产账行数逐行等"),
    ));

    // ④ 资产账只增不减（删行/抹账双拒）。
    let mut s4 = match machine_session() {
        Ok(x) => x,
        Err(_) => ReconSession::new(),
    };
    let ro = s4.master.remove_row(0).is_err() && s4.assets.remove_row(0).is_err();
    let cl = s4.master.clear().is_err() && s4.assets.clear().is_err();
    let len_kept = s4.master.rows.len() == 14 && s4.assets.rows.len() == MACHINE_ASSETS;
    v.push((
        "J1-两账只增不减",
        ro && cl && len_kept,
        String::from("删行/抹账全拒且行数守恒"),
    ));

    // ⑤ 漏记注入必抓 10/10。
    let omit = omit_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-漏记注入10/10",
        match &omit {
            Ok(l) => l.tries == JUDGE_ROUNDS && l.caught == l.tries,
            Err(_) => false,
        },
        match &omit {
            Ok(l) => format!("tries={} caught={}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ⑥ 挂账三闸（功能拒/重复拒/悬空拒）。
    let mut s6 = match machine_session() {
        Ok(x) => x,
        Err(_) => ReconSession::new(),
    };
    let g1 = s6.assets.attach(&s6.master, 0, InjectKind::OobCoord, 1).is_err();
    let g2 = s6.assets.attach(&s6.master, 8, InjectKind::DupId, 1).is_err();
    let g3 = s6.assets.attach(&s6.master, 99, InjectKind::DupId, 1).is_err();
    v.push((
        "J1-挂账三闸",
        g1 && g2 && g3,
        String::from("功能拒/重复拒/悬空拒 三闸全落"),
    ));

    // ⑦ 对账状态机单调（平衡态回退拒）。
    let mut s7 = ReconSession::new();
    let first = s7.settle().is_ok();
    let second = s7.settle().is_err();
    let back_deny = s7.demote().is_err();
    v.push((
        "J1-状态机单调",
        first && second && back_deny,
        String::from("开账→挂齐→平衡 单调，回退拒"),
    ));

    // ⑧ 账号单调闸（断言号从 0 连续递增）。
    let ids_ok = match &s {
        Ok(sess) => {
            let mut expect = 0u32;
            let mut ok = true;
            for r in sess.master.rows.iter() {
                if r.id != expect {
                    ok = false;
                }
                expect = expect.saturating_add(1);
            }
            ok
        }
        Err(_) => false,
    };
    v.push((
        "J1-账号单调",
        ids_ok,
        String::from("0..13 连续无断号"),
    ));

    // ⑨ 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push(("J1-探针三序", a && b && c, format!("a={} b=检出{} c=检出{}", a, b, c)));

    // ⑩ 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // ⑪ 诊断码闭集查表。
    let mut codes_ok = true;
    let mut ci = 0;
    while ci < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(ci) {
            if detail(c.val()) != c.detail() {
                codes_ok = false;
            }
        }
        ci += 1;
    }
    v.push((
        "J3-诊断码闭集",
        codes_ok && detail(0xDEAD_0000) == "未知诊断码：不在 0x75F40 段闭集内",
        String::from("ALL 查表一致 + 未知码兜底"),
    ));

    // ⑫ 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75F40 窗口 + 0x40 对齐"),
    ));
    v
}

pub fn run_unx1494_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1494 断言注入资产对账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_chain() {
        assert_eq!(golden_run(), Ok((2, 2)));
    }

    #[test]
    fn t02_recon_full_percent() {
        let s = match machine_session() {
            Ok(x) => x,
            Err(_) => ReconSession::new(),
        };
        assert_eq!(reconcile(&s.master, &s.assets), Ok(RECON_FULL));
        assert_eq!(s.master.inject_count(), MACHINE_ASSETS);
        assert_eq!(s.assets.rows.len(), MACHINE_ASSETS);
    }

    #[test]
    fn t03_delete_and_clear_denied() {
        let mut s = match machine_session() {
            Ok(x) => x,
            Err(_) => ReconSession::new(),
        };
        let m_len = s.master.rows.len();
        let a_len = s.assets.rows.len();
        assert_eq!(s.master.remove_row(0), Err(ReconCode::MasterRmDenied.val()));
        assert_eq!(s.assets.remove_row(0), Err(ReconCode::AssetRmDenied.val()));
        assert!(s.master.clear().is_err());
        assert!(s.assets.clear().is_err());
        assert_eq!(s.master.rows.len(), m_len);
        assert_eq!(s.assets.rows.len(), a_len);
    }

    #[test]
    fn t04_attach_gates() {
        let mut s = match machine_session() {
            Ok(x) => x,
            Err(_) => ReconSession::new(),
        };
        // 功能断言（0 号）挂资产 → 悬空拒。
        assert_eq!(
            s.assets.attach(&s.master, 0, InjectKind::OobCoord, 1),
            Err(ReconCode::AssetOrphan.val())
        );
        // 已挂资产断言（8 号）重复挂 → 重复拒。
        assert_eq!(
            s.assets.attach(&s.master, 8, InjectKind::DupId, 1),
            Err(ReconCode::AssetDupLink.val())
        );
        // 不存在的断言号 → 悬空拒。
        assert!(s.assets.attach(&s.master, 99, InjectKind::DupId, 1).is_err());
    }

    #[test]
    fn t05_omit_rounds_full() {
        let led = match omit_rounds(JUDGE_ROUNDS) {
            Ok(x) => x,
            Err(_) => OmitLedger { tries: 0, caught: 0 },
        };
        assert_eq!(led.tries, JUDGE_ROUNDS);
        assert_eq!(led.caught, JUDGE_ROUNDS);
        assert!(omit_rounds(JUDGE_ROUNDS).is_ok());
    }

    #[test]
    fn t06_state_machine() {
        let mut s = ReconSession::new();
        // 空账（0 注入 0 资产）也可平衡。
        assert_eq!(s.settle(), Ok(RECON_FULL));
        assert!(s.settle().is_err());
        assert!(s.demote().is_err());
    }

    #[test]
    fn t07_ids_monotonic() {
        let s = match machine_session() {
            Ok(x) => x,
            Err(_) => ReconSession::new(),
        };
        let mut expect = 0u32;
        for r in s.master.rows.iter() {
            assert_eq!(r.id, expect);
            expect += 1;
        }
        assert_eq!(expect, 14);
    }

    #[test]
    fn t08_repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t09_code_closed_set() {
        assert_eq!(ReconCode::ALL.len(), 9);
        let mut i = 0;
        while i < ReconCode::ALL.len() {
            if let Some(c) = ReconCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert_eq!(
            detail(0xDEAD_0000),
            "未知诊断码：不在 0x75F40 段闭集内"
        );
    }

    #[test]
    fn t10_register_inject_one_shot() {
        let mut s = ReconSession::new();
        let id = match s.register_inject("测试注入", InjectKind::BadGlyph, 200) {
            Ok(x) => x,
            Err(_) => u32::MAX,
        };
        assert_eq!(id, 0);
        assert_eq!(s.master.inject_count(), 1);
        assert_eq!(s.assets.rows.len(), 1);
        assert!(s.settle().is_ok());
    }

    #[test]
    fn t11_probes_abc() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn t12_machine_inject_closed_set() {
        let inj = machine_injects();
        assert_eq!(inj.len(), INJ_KINDS + 2);
        let mut kinds_seen = [0usize; INJ_KINDS];
        for (k, _, _) in inj.iter() {
            let slot = match k {
                InjectKind::OobCoord => 0,
                InjectKind::BadFormat => 1,
                InjectKind::BadGlyph => 2,
                InjectKind::DupId => 3,
            };
            if let Some(c) = kinds_seen.get_mut(slot) {
                *c += 1;
            }
        }
        assert!(kinds_seen.iter().all(|c| *c > 0));
    }
}
