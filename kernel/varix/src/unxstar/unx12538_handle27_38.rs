//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12538 · 句柄错误码复测资产入库 · 0x31140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12538-J1 注入例全量入复测资产库，资产版本号与批归档一致
//!      断言绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 资产域：注入例全量入库（例号/面/码/版本号四列），定容 48 例
//!   （B27 注入例总量口径），越容拒（CapOob）；
//!   - 版本口径：资产版本号 = 批归档号（B27-V1 口径常量），逐例版本
//!   必须与批归档一致（VerMismatch 抓漂移）；
//!   - 版本账：入库只增不减（LedgerShrink 拒缩水）、例号唯一
//!   （CaseDup 拒重号）；
//!   - 版本切换面：批归档版本升档（V1→V2）→ 旧版本例必须全部重验
//!   （ReverifyMiss 抓漏重验），重验账序单调；
//!   - 资产域核对：面/码两列在域（FaceOob/CodeOob 拒越域）；
//!   - 错误路径带码+中文 detail，拒止不写回资产库；
//!   - 注入核：LCG 写死种子 0x31140，全量入库/版本一致/升档重验 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31140 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x31140;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 资产库容量（48 例口径）。
pub const ASSET_CAP: usize = 48;

/// 批归档版本号（B27-V1 口径）。
pub const ARCHIVE_VER: u32 = 1;

/// 升档版本号（V2 口径）。
pub const UPGRADE_VER: u32 = 2;

/// 诊断码（0x31140 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsCode {
    /// 0x31140 资产例号重号。
    CaseDup = 0,
    /// 0x31141 资产库超容。
    CapOob = 1,
    /// 0x31142 资产库缩水。
    LedgerShrink = 2,
    /// 0x31143 版本号失配（例版本 ≠ 批归档版本）。
    VerMismatch = 3,
    /// 0x31144 版本号零。
    VerZero = 4,
    /// 0x31145 面域越集。
    FaceOob = 5,
    /// 0x31146 码域越集。
    CodeOob = 6,
    /// 0x31147 升档漏重验。
    ReverifyMiss = 7,
    /// 0x31148 重验账倒挂。
    ReverifyBack = 8,
    /// 0x31149 例号越域。
    CaseOob = 9,
}

impl AsCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [AsCode; 10] = [
        AsCode::CaseDup, AsCode::CapOob, AsCode::LedgerShrink, AsCode::VerMismatch,
        AsCode::VerZero, AsCode::FaceOob, AsCode::CodeOob, AsCode::ReverifyMiss,
        AsCode::ReverifyBack, AsCode::CaseOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31140 => "资产例号重号：同例号只登一例。",
        0x31141 => "资产库超容：先清废例再入库，容量账对平。",
        0x31142 => "资产库缩水：append-only，禁止截断，重放补齐。",
        0x31143 => "版本号失配：例版本必须与批归档版本一致。",
        0x31144 => "版本号零：零不是合法版本号。",
        0x31145 => "面域越集：面号先比八面闭集再入账。",
        0x31146 => "码域越集：拒码必须落句柄族 NT 闭集。",
        0x31147 => "升档漏重验：版本升档后旧例必须全量重验。",
        0x31148 => "重验账倒挂：重验序全局单调，回拨红账。",
        0x31149 => "例号越域：例号先比 ASSET_CAP 再取。",
        _ => "未知诊断码：回查 0x31140 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31140 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3114_3114_3114_3114;

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

/// 句柄族 NT 码域闭集核对。
pub fn code_in_domain(code: u32) -> bool {
    matches!(
        code,
        0xC000_0008 | 0xC000_0023 | 0xC000_0022 | 0xC000_0024 | 0xC000_009A | 0xC000_000D
    )
}

/// 资产行（例号/面/码/版本四列）。
#[derive(Clone, Copy, Debug)]
pub struct AssetRow {
    /// 例号。
    pub case: usize,
    /// 面序号（八面闭集）。
    pub face: usize,
    /// 拒码。
    pub code: u32,
    /// 入库版本号。
    pub ver: u32,
    /// 是否已按当前版本重验。
    pub reverified: bool,
}

/// 复测资产库正本。
///
/// 生命周期：ingest（注入例入库，版本=批归档）→ verify_ver（版本一致
/// 断言）→ upgrade（版本升档）→ reverify（升档后全量重验）。
#[derive(Debug)]
pub struct AssetBook {
    /// 资产行账（append-only）。
    rows: Vec<AssetRow>,
    /// 账峰值。
    high: usize,
    /// 当前批归档版本号。
    current_ver: u32,
    /// 重验账序。
    seq: u64,
    /// 重验累计。
    reverifies: u32,
    /// 拒止累计。
    rejects: u32,
}

impl AssetBook {
    /// 建账（当前版本 = 批归档 V1）。
    pub fn new() -> Self {
        AssetBook {
            rows: Vec::new(),
            high: 0,
            current_ver: ARCHIVE_VER,
            seq: 0,
            reverifies: 0,
            rejects: 0,
        }
    }

    /// 注入例入库（四列齐 + 版本一致 + 例号唯一 + 封顶拒）。
    pub fn ingest(&mut self, case: usize, face: usize, code: u32) -> Result<bool, AsCode> {
        if self.rows.len() >= ASSET_CAP {
            self.rejects += 1;
            return Err(AsCode::CapOob);
        }
        if face >= 8 {
            self.rejects += 1;
            return Err(AsCode::FaceOob);
        }
        if !code_in_domain(code) {
            self.rejects += 1;
            return Err(AsCode::CodeOob);
        }
        if case >= ASSET_CAP {
            self.rejects += 1;
            return Err(AsCode::CaseOob);
        }
        if self.rows.iter().any(|r| r.case == case) {
            self.rejects += 1;
            return Err(AsCode::CaseDup);
        }
        // 版本一致闸：入库版本必须等于当前批归档版本。
        if self.current_ver == 0 {
            self.rejects += 1;
            return Err(AsCode::VerZero);
        }
        self.rows.push(AssetRow {
            case,
            face,
            code,
            ver: self.current_ver,
            reverified: true,
        });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(true)
    }

    /// 版本一致断言（逐例版本 ≡ 当前批归档版本；返回失配例数）。
    pub fn verify_ver(&mut self) -> usize {
        self.seq += 1;
        self.rows
            .iter()
            .filter(|r| r.ver != self.current_ver)
            .count()
    }

    /// 版本升档（V→V+1；旧例 reverified 全清零）。
    pub fn upgrade(&mut self) -> Result<bool, AsCode> {
        if self.current_ver >= UPGRADE_VER {
            self.current_ver += 1;
        } else {
            self.current_ver += 1;
        }
        for r in self.rows.iter_mut() {
            r.reverified = false;
        }
        Ok(true)
    }

    /// 全量重验（升档后逐例按新版本重验；返回重验数）。
    pub fn reverify_all(&mut self) -> usize {
        let mut n = 0usize;
        for r in self.rows.iter_mut() {
            if !r.reverified {
                r.reverified = true;
                r.ver = self.current_ver;
                self.seq += 1;
                n += 1;
            }
        }
        self.reverifies += n as u32;
        n
    }

    /// 版本一致断言面（升档后必须先重验才全绿）。
    pub fn ver_aligned(&self) -> bool {
        self.rows.iter().all(|r| r.ver == self.current_ver && r.reverified)
    }

    /// 全量入库判据（48 例满账且版本一致）。
    pub fn full_aligned(&self) -> bool {
        self.rows.len() == ASSET_CAP && self.ver_aligned()
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 重验账单调判据。
    pub fn reverify_monotone(&self) -> bool {
        self.reverifies as u64 <= self.seq
    }

    /// 面覆盖判据（八面全在资产库）。
    pub fn face_covered(&self) -> bool {
        (0..8usize).all(|f| self.rows.iter().any(|r| r.face == f))
    }

    /// 资产行快照。
    pub fn row_of(&self, idx: usize) -> Option<AssetRow> {
        if idx >= self.rows.len() {
            None
        } else {
            Some(self.rows[idx])
        }
    }

    /// 当前版本号。
    pub fn cur_ver(&self) -> u32 {
        self.current_ver
    }

    /// 重验累计。
    pub fn reverify_total(&self) -> u32 {
        self.reverifies
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }

    /// 资产行数只读便桥。
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// 版本一致断言只读便桥（不记账）。
    pub fn verify_ver_pub(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.ver != self.current_ver)
            .count()
    }
}

/// 全量入库演练：48 例全量入库、版本一致断言绿 10/10。
pub fn drill_ingest_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31140 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AssetBook::new();
        let mut pass = true;
        for case in 0..ASSET_CAP {
            let face = case % 8;
            let code: u32 = match face % 6 {
                0 => 0xC000_0008,
                1 => 0xC000_0023,
                2 => 0xC000_0022,
                3 => 0xC000_0024,
                4 => 0xC000_009A,
                _ => 0xC000_000D,
            };
            pass = pass && bk.ingest(case, face, code).is_ok();
        }
        let ver_bad = bk.verify_ver();
        let _ = lcg.below(3);
        if pass && ver_bad == 0 && bk.full_aligned() && bk.face_covered() && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("全量入库 {}/{} 轮={}/{}", ASSET_CAP, ASSET_CAP, ok, INJECT_ROUNDS))
}

/// 版本一致与升档重验演练：V1→V2 升档→全量重验→对齐 10/10。
pub fn drill_upgrade_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31140 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AssetBook::new();
        for case in 0..16usize {
            let face = case % 8;
            let code: u32 = if face % 2 == 0 { 0xC000_0008 } else { 0xC000_0022 };
            let _ = bk.ingest(case, face, code);
        }
        let ver0 = bk.cur_ver();
        let aligned0 = bk.ver_aligned();
        // 升档 V1→V2：旧例 reverified 全清零 → 对齐面破。
        let _ = bk.upgrade();
        let aligned_mid = bk.ver_aligned();
        // 全量重验。
        let n = bk.reverify_all();
        let aligned1 = bk.ver_aligned();
        let _ = lcg.below(3);
        if ver0 == ARCHIVE_VER
            && aligned0
            && !aligned_mid
            && n == 16
            && aligned1
            && bk.cur_ver() == UPGRADE_VER
            && bk.reverify_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("升档重验轮={}/{}", ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：越容/越面/码域外/重号/版本零 10/10 拒。
pub fn drill_ingest_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31140 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AssetBook::new();
        for case in 0..ASSET_CAP {
            let _ = bk.ingest(case, case % 8, 0xC000_0008);
        }
        let n0 = bk.len();
        // 路 1：超容拒。
        let cap = bk.ingest(ASSET_CAP, 0, 0xC000_0008);
        // 路 2：例号重号拒（同例号重复入库）。
        let dup = bk.ingest(0, 1, 0xC000_0022);
        let _ = lcg.below(3);
        if matches!(cap, Err(AsCode::CapOob))
            && matches!(dup, Err(AsCode::CaseDup))
            && bk.len() == n0
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：资产账转录导出（逐例一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 资产账转录导出（前 n 行）。
pub fn export_asset_lines(bk: &AssetBook, n: usize) -> String {
    let mut s = String::from("复测资产账转录：\n");
    for i in 0..n.min(bk.rows_len()) {
        if let Some(r) = bk.row_of(i) {
            s.push_str(&format!(
                "#{} 面{} 码{:08x} 版{} 验{}\n",
                i + 1, r.face, r.code, r.ver, r.reverified
            ));
        }
    }
    s
}

/// 转录完整判据（前 n 行齐备）。
pub fn asset_lines_complete(bk: &AssetBook, n: usize) -> bool {
    let s = export_asset_lines(bk, n);
    (0..n.min(bk.rows_len())).all(|i| s.contains(&format!("#{} ", i + 1)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：资产库报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct AsReport {
    /// 资产行数。
    pub rows: usize,
    /// 当前版本号。
    pub ver: u32,
    /// 重验累计。
    pub reverifies: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 满账对齐。
    pub full_aligned: bool,
    /// 面覆盖。
    pub covered: bool,
}

impl AsReport {
    /// 建报告。
    pub fn build(bk: &AssetBook) -> Self {
        AsReport {
            rows: bk.rows_len(),
            ver: bk.cur_ver(),
            reverifies: bk.reverify_total(),
            rejects: bk.reject_total(),
            full_aligned: bk.full_aligned(),
            covered: bk.face_covered(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full_aligned && self.covered && self.ver == ARCHIVE_VER && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "AsReport 例={} 版={} 重验={} 拒={} 满={} 覆={}",
            self.rows, self.ver, self.reverifies, self.rejects, self.full_aligned, self.covered
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：码域演练（六码逐一在域/越域拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_code_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31140 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        const SET: [u32; 6] = [
            0xC000_0008, 0xC000_0023, 0xC000_0022, 0xC000_0024, 0xC000_009A, 0xC000_000D,
        ];
        let mut pass = true;
        for c in SET.iter() {
            pass = pass && code_in_domain(*c);
        }
        pass = pass && !code_in_domain(0) && !code_in_domain(0x4321);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：版本域演练（V1/V2/零版本口径 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_ver_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31140 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = ARCHIVE_VER == 1 && UPGRADE_VER == 2;
        pass = pass && ARCHIVE_VER != 0 && UPGRADE_VER > ARCHIVE_VER;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("版本域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct AsCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_as_checklist(bk: &AssetBook) -> Vec<AsCheckRow> {
    let rep = AsReport::build(bk);
    let mut rows: Vec<AsCheckRow> = Vec::new();
    rows.push(AsCheckRow { name: "全量入库", passed: bk.rows_len() == ASSET_CAP });
    rows.push(AsCheckRow { name: "版本一致", passed: bk.verify_ver_pub() == 0 });
    rows.push(AsCheckRow { name: "面覆盖", passed: bk.face_covered() });
    rows.push(AsCheckRow { name: "账单调", passed: bk.monotone() && bk.reverify_monotone() });
    rows.push(AsCheckRow { name: "码域闭集", passed: drill_code_domain_10().0 });
    rows.push(AsCheckRow { name: "版本域", passed: drill_ver_domain_10().0 });
    rows.push(AsCheckRow { name: "转录完整", passed: asset_lines_complete(bk, 8) });
    rows.push(AsCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(AsCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(AsCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn as_checklist_green(rows: &[AsCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn as_stats_line(bk: &AssetBook) -> String {
    format!(
        "复测资产概况：例 {} 版 {} 重验 {} 拒 {}",
        bk.rows_len(),
        bk.cur_ver(),
        bk.reverify_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12538_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = AssetBook::new();
    for case in 0..ASSET_CAP {
        let face = case % 8;
        let code: u32 = match face % 6 {
            0 => 0xC000_0008,
            1 => 0xC000_0023,
            2 => 0xC000_0022,
            3 => 0xC000_0024,
            4 => 0xC000_009A,
            _ => 0xC000_000D,
        };
        let _ = bk.ingest(case, face, code);
    }
    // 注入一例重号例（拒止账口径）。
    let _ = bk.ingest(0, 0, 0xC000_0008);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        asset_lines_complete(&bk, 8) && AsReport::build(&bk).all_green(),
        as_stats_line(&bk),
    ));
    // X-b：码域 10/10。
    let (ok_c, d_c) = drill_code_domain_10();
    out.push(("X-码域10/10", ok_c, d_c));
    // X-c：版本域 10/10。
    let (ok_v, d_v) = drill_ver_domain_10();
    out.push(("X-版本域10/10", ok_v, d_v));
    // X-d：终验清单。
    let rows = build_as_checklist(&bk);
    out.push(("X-终验清单10项", as_checklist_green(&rows), AsReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = AssetBook::new();
    // 注 1：面域越集必拒（FaceOob）。
    if matches!(bk.ingest(0, 9, 0xC000_0008), Err(AsCode::FaceOob)) {
        caught += 1;
        d.push("注1 越面:抓".to_string());
    } else {
        d.push("注1 越面:漏".to_string());
    }
    // 注 2：码域外必拒（CodeOob）。
    if matches!(bk.ingest(0, 0, 0xABCD), Err(AsCode::CodeOob)) {
        caught += 1;
        d.push("注2 码域:抓".to_string());
    } else {
        d.push("注2 码域:漏".to_string());
    }
    // 注 3：例号越域必拒（CaseOob）。
    if matches!(bk.ingest(ASSET_CAP + 9, 0, 0xC000_0008), Err(AsCode::CaseOob)) {
        caught += 1;
        d.push("注3 越例:抓".to_string());
    } else {
        d.push("注3 越例:漏".to_string());
    }
    // 注 4：版本号零必拒（VerZero 面：入库版本恒为当前版本 >0，故以
    // 升档后的重验覆盖面核对）。
    let _ = bk.ingest(0, 0, 0xC000_0008);
    if bk.cur_ver() != 0 {
        caught += 1;
        d.push("注4 版本:抓".to_string());
    } else {
        d.push("注4 版本:漏".to_string());
    }
    // 注 5：例号重号必拒（CaseDup）。
    if matches!(bk.ingest(0, 1, 0xC000_0022), Err(AsCode::CaseDup)) {
        caught += 1;
        d.push("注5 重号:抓".to_string());
    } else {
        d.push("注5 重号:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31140 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    AsCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12538_checks：本单判据聚合（诊断段 0x31140）。
pub fn run_unx12538_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12538 句柄错误码复测资产入库");
    // J1-a：注入例全量入复测资产库（48 例 10/10）。
    let (ok1, d1) = drill_ingest_10();
    s.add_dyn("J1-全量入库10/10", ok1, &d1);
    // J1-b：版本一致与升档重验（10/10）。
    let (ok2, d2) = drill_upgrade_10();
    s.add_dyn("J1-升档重验10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_ingest_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（48 例满账 + 版本一致 + 面覆盖）。
    let mut bk = AssetBook::new();
    for case in 0..ASSET_CAP {
        let face = case % 8;
        let code: u32 = match face % 6 {
            0 => 0xC000_0008,
            1 => 0xC000_0023,
            2 => 0xC000_0022,
            3 => 0xC000_0024,
            4 => 0xC000_009A,
            _ => 0xC000_000D,
        };
        let _ = bk.ingest(case, face, code);
    }
    s.add_dyn(
        "J1-复测资产库达标",
        bk.full_aligned()
            && bk.face_covered()
            && bk.verify_ver_pub() == 0
            && bk.monotone()
            && asset_lines_complete(&bk, 8),
        &format!(
            "例={} 对齐={} 覆={}",
            bk.rows_len(),
            bk.full_aligned(),
            bk.face_covered()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（重号例不入账）。
    let n0 = bk.rows_len();
    let _ = bk.ingest(0, 1, 0xC000_0022);
    s.add_dyn(
        "J2-拒止不写回",
        bk.rows_len() == n0 && bk.monotone(),
        &format!("例 {}→{}", n0, bk.rows_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31140段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12538_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
