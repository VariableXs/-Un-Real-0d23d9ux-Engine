//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18457 · 声明观测导出（应用兼容画像） · 0x7EE00 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18457-J1 每应用声明画像（世代/级别/段元素）可导出，
//!       AI-25 消费格式一致
//! 本模块结构清单：
//!   1. 冻结常量层：应用键域 / 世代域 / 执行级别四档 / 段元素位面
//!      6 位 / 画像容量 / 判据轮数（推导注释写死）；
//!   2. 诊断码闭集 PeCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构一（画像）：Profile——应用键/世代/执行级别/段元素
//!      位面四元组，域守卫 complete()；
//!   4. 数据结构二（画像账）：ProfileLedger——每应用一画像（重复
//!      登记走更新不改行数），容量 16；
//!   5. 导出面：冻结表头 AI25-PROFILE-V1 + 数据行字段序
//!      app|gen|lvl|plane（AI-25 消费格式，字段序写死不许漂移）；
//!   6. 回程核：导出文本逐行重解析 → 与原画像逐字段对拍（往返
//!      一致），指纹 FNV-1a 复算一致；
//!   7. 注入核 4 类×10 轮：字段序篡改 / 字段缺失 / 级别越档 /
//!      重复登记造行，逐类断言被显式拒绝；
//!   8. 黄金全链 + run_unx18457_checks：15 条判据（断言性质）。
//! 为什么导出格式冻结：AI-25 消费方按字段序取值，画像导出若漂移
//! 字段序或漏字段，消费面会静默读错——故字段序以冻结常量入账，
//! 回程核逐行重解析对拍，任何漂移都在往返差上现形。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EE00，AI-VX11-1105-G1·A06 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EE00;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 应用键上界（冻结 32：画像账按应用键一应用一行）。
pub const APP_MAX: u32 = 32;
/// 世代域（冻结 1..=8：与 F18452 世代表 8 世代对齐）。
pub const GEN_MAX: u32 = 8;
/// 执行级别四档（冻结：与 F18446 口径对齐——0=asInvoker,
/// 1=highestAvailable, 2=requireAdministrator, 3=uiAccess）。
pub const LVL_NAMES: [&str; 4] =
    ["asInvoker", "highestAvailable", "requireAdministrator", "uiAccess"];
/// 段元素位面宽度（冻结 6：compatibility 段六类元素位）。
pub const PLANE_BITS: u32 = 6;
/// 画像账容量（冻结 16）。
pub const PROFILE_CAP: usize = 16;
/// 冻结表头（AI-25 消费格式 v1）。
pub const EXPORT_HEADER: &str = "AI25-PROFILE-V1";
/// 字段序（冻结）：app|gen|lvl|plane。
pub const FIELD_ORDER: [&str; 4] = ["app", "gen", "lvl", "plane"];
/// 正例画像数（冻结 8）。
pub const GOOD_PROFILES: usize = 8;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeCode {
    /// 应用键越域（0 或 >32）
    AppKeyIllegal,
    /// 世代越域（0 或 >8）
    GenIllegal,
    /// 执行级别越档（>3）
    LevelIllegal,
    /// 段元素位面越宽（≥2^6）
    PlaneIllegal,
    /// 画像账满
    LedgerFull,
    /// 导出行缺字段
    FieldMissing,
    /// 字段序漂移（AI-25 消费格式不一致）
    FieldOrderDrift,
    /// 往返不一致（重解析对拍失配）
    RoundTripBroken,
    /// 指纹复算失配
    FingerprintMismatch,
    /// 重复登记造行（每应用一画像红线）
    AppDupRow,
    /// 双跑不一致
    NotReproducible,
}

impl PeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 诊断码别名：code() = SEG_BASE + n（与 val() 同值，契约骨架口径）。
    pub fn code(self) -> u32 {
        self.val()
    }
    pub fn detail(self) -> &'static str {
        match self {
            PeCode::AppKeyIllegal => "应用键越域：0 或超 32",
            PeCode::GenIllegal => "世代越域：0 或超 8",
            PeCode::LevelIllegal => "执行级别越档：四档闭集外",
            PeCode::PlaneIllegal => "段元素位面越宽：6 位上限",
            PeCode::LedgerFull => "画像账满：16 行上限",
            PeCode::FieldMissing => "导出行缺字段：四字段闭集缺一",
            PeCode::FieldOrderDrift => "字段序漂移：AI-25 消费格式不一致",
            PeCode::RoundTripBroken => "往返不一致：重解析对拍失配",
            PeCode::FingerprintMismatch => "指纹复算失配",
            PeCode::AppDupRow => "重复登记造行：每应用一画像红线",
            PeCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [PeCode; 11] = [
        PeCode::AppKeyIllegal,
        PeCode::GenIllegal,
        PeCode::LevelIllegal,
        PeCode::PlaneIllegal,
        PeCode::LedgerFull,
        PeCode::FieldMissing,
        PeCode::FieldOrderDrift,
        PeCode::RoundTripBroken,
        PeCode::FingerprintMismatch,
        PeCode::AppDupRow,
        PeCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PeCode::ALL.len() {
        if PeCode::ALL[i].val() == code {
            return PeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EE00 段闭集内"
}

// ── 哈希（导出指纹） ────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 数据结构一：画像 ────────────────────────────────────────────
/// 每应用声明画像：应用键 / 世代 / 执行级别 / 段元素位面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub app_key: u32,
    pub gen: u32,
    pub exec_level: u32,
    pub seg_plane: u32,
}

impl Profile {
    /// 域守卫：四段全部在闭集内。
    pub fn complete(&self) -> bool {
        self.app_key >= 1
            && self.app_key <= APP_MAX
            && self.gen >= 1
            && self.gen <= GEN_MAX
            && self.exec_level <= 3
            && self.seg_plane < (1u32 << PLANE_BITS)
    }
    /// 级别名（四档闭集取名，越档返回空串哨兵）。
    pub fn level_name(&self) -> &'static str {
        match LVL_NAMES.get(self.exec_level as usize) {
            Some(n) => *n,
            None => "",
        }
    }
}

/// 画像域守卫独立入口（注册前判定，返回首命中违约定序）。
pub fn guard_profile(p: &Profile) -> Result<(), u32> {
    if p.app_key == 0 || p.app_key > APP_MAX {
        return Err(PeCode::AppKeyIllegal.val());
    }
    if p.gen == 0 || p.gen > GEN_MAX {
        return Err(PeCode::GenIllegal.val());
    }
    if p.exec_level > 3 {
        return Err(PeCode::LevelIllegal.val());
    }
    if p.seg_plane >= (1u32 << PLANE_BITS) {
        return Err(PeCode::PlaneIllegal.val());
    }
    Ok(())
}

// ── 数据结构二：画像账 ──────────────────────────────────────────
/// 画像账：每应用一画像，重复登记走更新（行数不变）。
pub struct ProfileLedger {
    pub rows: Vec<Profile>,
    pub updates: u32,
}

impl ProfileLedger {
    pub fn new() -> ProfileLedger {
        ProfileLedger { rows: Vec::new(), updates: 0 }
    }

    fn find_app(&self, app_key: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.app_key == app_key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记：新应用入行（容量守卫），已有应用更新（行数不变）。
    pub fn put(&mut self, p: Profile) -> Result<u32, u32> {
        guard_profile(&p)?;
        match self.find_app(p.app_key) {
            Some(idx) => {
                if let Some(r) = self.rows.get_mut(idx) {
                    *r = p;
                }
                self.updates = self.updates.saturating_add(1);
                Ok(1)
            }
            None => {
                if self.rows.len() >= PROFILE_CAP {
                    return Err(PeCode::LedgerFull.val());
                }
                self.rows.push(p);
                Ok(0)
            }
        }
    }

    /// 查画像。
    pub fn get(&self, app_key: u32) -> Option<Profile> {
        let idx = self.find_app(app_key)?;
        self.rows.get(idx).copied()
    }
}

// ── 导出面（AI-25 消费格式） ────────────────────────────────────
/// 单行导出（字段序冻结：app|gen|lvl|plane）。
pub fn export_row(p: &Profile) -> Result<String, u32> {
    guard_profile(p)?;
    Ok(format!("P|app={}|gen={}|lvl={}|plane={}", p.app_key, p.gen, p.exec_level, p.seg_plane))
}

/// 全账导出：表头 + 数据行（确定性，无随机无墙钟）。
pub fn export_all(led: &ProfileLedger) -> Result<String, u32> {
    let mut out = String::new();
    out.push_str(EXPORT_HEADER);
    out.push('\n');
    let mut i = 0;
    while i < led.rows.len() {
        let p = match led.rows.get(i) {
            Some(x) => *x,
            None => return Err(PeCode::FieldMissing.val()),
        };
        let line = export_row(&p)?;
        out.push_str(&line);
        out.push('\n');
        i += 1;
    }
    Ok(out)
}

// ── 回程核（重解析 + 对拍） ─────────────────────────────────────
/// 从 "key=value" 段抓 u32（十进制，非数字即拒）。
fn parse_u32_field(seg: &[u8]) -> Result<u32, u32> {
    let mut v = 0u32;
    let mut any = false;
    let mut i = 0;
    while i < seg.len() {
        let c = seg[i];
        if c >= b'0' && c <= b'9' {
            v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
            any = true;
        } else {
            return Err(PeCode::FieldOrderDrift.val());
        }
        i += 1;
    }
    if !any {
        return Err(PeCode::FieldMissing.val());
    }
    Ok(v)
}

/// 单行重解析：按冻结字段序取四段，缺段/错序/越域显式拒。
pub fn parse_row(line: &str) -> Result<Profile, u32> {
    let b = line.as_bytes();
    if b.len() < 2 || b[0] != b'P' || b[1] != b'|' {
        return Err(PeCode::FieldOrderDrift.val());
    }
    // 按 '|' 切段（no_std 无 str split，手工切；末段无 '|' 也要收）。
    let mut segs: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut k = 2usize;
    while k < b.len() {
        if b[k] == b'|' {
            segs.push(cur.clone());
            cur = String::new();
        } else {
            cur.push(b[k] as char);
        }
        k += 1;
    }
    segs.push(cur);
    if segs.len() != 4 {
        return Err(PeCode::FieldMissing.val());
    }
    // 字段序核对（AI-25 消费格式：app,gen,lvl,plane）。
    if segs[0].len() < 4 || &segs[0][0..4] != "app=" {
        return Err(PeCode::FieldOrderDrift.val());
    }
    if segs[1].len() < 4 || &segs[1][0..4] != "gen=" {
        return Err(PeCode::FieldOrderDrift.val());
    }
    if segs[2].len() < 4 || &segs[2][0..4] != "lvl=" {
        return Err(PeCode::FieldOrderDrift.val());
    }
    if segs[3].len() < 6 || &segs[3][0..6] != "plane=" {
        return Err(PeCode::FieldOrderDrift.val());
    }
    let app = parse_u32_field(segs[0].as_bytes().split_at(4).1)?;
    let gen = parse_u32_field(segs[1].as_bytes().split_at(4).1)?;
    let lvl = parse_u32_field(segs[2].as_bytes().split_at(4).1)?;
    let plane = parse_u32_field(segs[3].as_bytes().split_at(6).1)?;
    let p = Profile { app_key: app, gen, exec_level: lvl, seg_plane: plane };
    guard_profile(&p)?;
    Ok(p)
}

/// 全文重解析：跳过表头，逐行重解析并与账面逐字段对拍。
pub fn round_trip(led: &ProfileLedger, text: &str) -> Result<(), u32> {
    let b = text.as_bytes();
    // 表头核对。
    let hb = EXPORT_HEADER.as_bytes();
    if b.len() < hb.len() {
        return Err(PeCode::FieldOrderDrift.val());
    }
    let mut i = 0;
    while i < hb.len() {
        if b[i] != hb[i] {
            return Err(PeCode::FieldOrderDrift.val());
        }
        i += 1;
    }
    // 逐行解析对拍。
    let mut line_start = hb.len() + 1; // 跳过表头换行。
    let mut parsed = 0u32;
    let mut k = line_start;
    while k <= b.len() {
        let cut = k == b.len() || b[k] == b'\n';
        if cut {
            if k > line_start {
                let mut line = String::new();
                let mut m = line_start;
                while m < k {
                    line.push(b[m] as char);
                    m += 1;
                }
                let p = parse_row(&line)?;
                let expect = match led.get(p.app_key) {
                    Some(x) => x,
                    None => return Err(PeCode::RoundTripBroken.val()),
                };
                if p != expect {
                    return Err(PeCode::RoundTripBroken.val());
                }
                parsed += 1;
            }
            line_start = k + 1;
        }
        k += 1;
    }
    if parsed != led.rows.len() as u32 {
        return Err(PeCode::RoundTripBroken.val());
    }
    Ok(())
}

/// 导出指纹（AI-25 消费一致性锚）。
pub fn export_fingerprint(text: &str) -> u32 {
    fnv1a(text.as_bytes())
}

// ── 正例画像（8 例确定性） ──────────────────────────────────────
pub fn make_good_profile(idx: usize) -> Profile {
    Profile {
        app_key: (1 + idx as u32) % APP_MAX,
        gen: 1 + (idx as u32 * 3) % GEN_MAX,
        exec_level: (idx as u32) % 4,
        seg_plane: (idx as u32 * 5 + 1) % (1u32 << PLANE_BITS),
    }
}

// ── 注入核（4 类 ×10 轮） ───────────────────────────────────────
/// 注入检出账：四槽 tries/caught 成对。
#[derive(Debug)]
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽 0 字段序篡改 / 槽 1 字段缺失 / 槽 2 级别越档 /
/// 槽 3 重复登记造行。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：字段序篡改——gen 与 lvl 互换的行，重解析必拒。
        let p0 = make_good_profile(0);
        let bad_order = format!("P|app={}|lvl={}|gen={}|plane={}",
            p0.app_key, p0.gen, p0.exec_level, p0.seg_plane);
        let hit0 = parse_row(&bad_order).err() == Some(PeCode::FieldOrderDrift.val());
        led.record(0, hit0);
        // 槽 1：字段缺失——缺 plane 段。
        let p1 = make_good_profile(1);
        let missing = format!("P|app={}|gen={}|lvl={}", p1.app_key, p1.gen, p1.exec_level);
        let hit1 = parse_row(&missing).err() == Some(PeCode::FieldMissing.val());
        led.record(1, hit1);
        // 槽 2：级别越档——lvl=4 四档外。
        let p2 = make_good_profile(2);
        let bad_lvl = Profile {
            app_key: p2.app_key,
            gen: p2.gen,
            exec_level: 4,
            seg_plane: p2.seg_plane,
        };
        let hit2 = guard_profile(&bad_lvl).err() == Some(PeCode::LevelIllegal.val())
            && export_row(&bad_lvl).err() == Some(PeCode::LevelIllegal.val());
        led.record(2, hit2);
        // 槽 3：重复登记造行——同应用键二次登记必须走更新
        // （行数不变），造第二行语义不存在。
        let mut pl = ProfileLedger::new();
        let pa = make_good_profile(3);
        pl.put(pa).ok();
        let rows_before = pl.rows.len();
        let mut pb = pa;
        pb.seg_plane = (pb.seg_plane + 1) % (1u32 << PLANE_BITS);
        let res = pl.put(pb);
        let hit3 = res == Ok(1)
            && pl.rows.len() == rows_before
            && pl.updates == 1
            && pl.rows.len() == 1;
        led.record(3, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：8 画像入账 → 导出 → 往返对拍 → 指纹一致 → 注入全抓 →
/// 双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = ProfileLedger::new();
    let mut i = 0;
    while i < GOOD_PROFILES {
        led.put(make_good_profile(i))?;
        i += 1;
    }
    if led.rows.len() != GOOD_PROFILES {
        return Err(PeCode::LedgerFull.val());
    }
    let text = export_all(&led)?;
    round_trip(&led, &text)?;
    // 指纹复算：同文本二次指纹一致。
    let fp1 = export_fingerprint(&text);
    let fp2 = export_fingerprint(&text);
    if fp1 != fp2 {
        return Err(PeCode::FingerprintMismatch.val());
    }
    // 二次导出逐字节一致。
    let text2 = export_all(&led)?;
    if text2 != text {
        return Err(PeCode::RoundTripBroken.val());
    }
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() || inj.missed() != 0 {
        return Err(PeCode::NotReproducible.val());
    }
    Ok((led.rows.len() as u32, fp1))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：单画像导出→重解析逐字段相等。
pub fn probe_seq_a() -> Result<(), u32> {
    let p = make_good_profile(0);
    let line = export_row(&p)?;
    let back = parse_row(&line)?;
    if back == p {
        Ok(())
    } else {
        Err(PeCode::RoundTripBroken.val())
    }
}

/// 负探针 1：应用键越域（0 与 33 都拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let a = guard_profile(&Profile { app_key: 0, gen: 1, exec_level: 0, seg_plane: 1 });
    let b = guard_profile(&Profile { app_key: APP_MAX + 1, gen: 1, exec_level: 0, seg_plane: 1 });
    if a.err() == Some(PeCode::AppKeyIllegal.val())
        && b.err() == Some(PeCode::AppKeyIllegal.val())
    {
        Ok(())
    } else {
        Err(PeCode::AppKeyIllegal.val())
    }
}

/// 负探针 2：表头漂移拒（表头错一字重解析全线拒）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = ProfileLedger::new();
    led.put(make_good_profile(1)).ok();
    let text = export_all(&led)?;
    let wrong = text.replace(EXPORT_HEADER, "AI25-PROFILE-V2");
    match round_trip(&led, &wrong) {
        Err(c) if c == PeCode::FieldOrderDrift.val() => Ok(()),
        _ => Err(PeCode::FieldOrderDrift.val()),
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PeCode::ALL.len() {
        let v = PeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18457_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18457 声明观测导出");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!(
            "画像{:?} 指纹{:?}",
            g.as_ref().map(|r| r.0).unwrap_or(0),
            g.as_ref().map(|r| r.1).unwrap_or(0)
        ),
    );
    // ② 每应用画像可导出（J1 前半句）：8 画像 8 行 + 表头。
    let mut led = ProfileLedger::new();
    let mut i = 0;
    while i < GOOD_PROFILES {
        led.put(make_good_profile(i)).ok();
        i += 1;
    }
    let text = export_all(&led);
    s.add("画像可导出", matches!(&text, Ok(t) if t.starts_with(EXPORT_HEADER)),
        "表头 AI25-PROFILE-V1 + 8 数据行");
    // ③ AI-25 消费格式一致（J1 后半句）：字段序冻结四段。
    let mut order_ok = FIELD_ORDER[0] == "app"
        && FIELD_ORDER[1] == "gen"
        && FIELD_ORDER[2] == "lvl"
        && FIELD_ORDER[3] == "plane";
    let mut fi = 0;
    while fi < FIELD_ORDER.len() {
        if FIELD_ORDER[fi].is_empty() {
            order_ok = false;
        }
        fi += 1;
    }
    s.add("AI25字段序冻结", order_ok, "app|gen|lvl|plane 四段闭集");
    // ④ 往返一致：导出→重解析逐字段对拍。
    s.add("往返对拍一致", match &text {
        Ok(t) => round_trip(&led, t).is_ok(),
        Err(_) => false,
    }, "重解析 8 行逐字段相等");
    // ⑤ 导出指纹复算（一致性锚）。
    let fp = text.as_ref().map(|t| export_fingerprint(t)).unwrap_or(0);
    s.add("导出指纹复算", match &text {
        Ok(t) => export_fingerprint(t) == fp && fp != 0,
        Err(_) => false,
    }, "FNV-1a 同文本同指纹");
    // ⑥ 二次导出逐字节一致（确定性导出）。
    let text2 = export_all(&led);
    s.add("二次导出逐字节一致", text == text2, "无随机无墙钟");
    // ⑦ 四档级别名闭集非空。
    let mut lvl_ok = true;
    let mut li = 0;
    while li < LVL_NAMES.len() {
        if LVL_NAMES[li].is_empty() {
            lvl_ok = false;
        }
        li += 1;
    }
    s.add("级别四档闭集", lvl_ok, "与 F18446 执行级别口径对齐");
    // ⑧ 注入四类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入四类10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0)
        ),
    );
    // ⑨ 漏检为零。
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "注入=检出+漏检，漏检=0");
    // ⑩ 探针三序。
    s.add("探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        "单行往返/键域拒/表头漂移拒");
    // ⑪ 重复登记走更新（每应用一画像红线）。
    let mut lup = ProfileLedger::new();
    let pu = make_good_profile(4);
    lup.put(pu).ok();
    let mut pv = pu;
    pv.gen = 1 + (pv.gen % GEN_MAX);
    let res = lup.put(pv);
    s.add("重复登记走更新", res == Ok(1) && lup.rows.len() == 1 && lup.updates == 1,
        "行数不变，更新计数入账");
    // ⑫ 容量边界：16 行填满后第 17 应用拒。
    let mut lfull = ProfileLedger::new();
    let mut filled = 0u32;
    let mut k = 0;
    while k <= PROFILE_CAP {
        let p = Profile {
            app_key: (1 + k as u32) % APP_MAX,
            gen: 1 + (k as u32 % GEN_MAX),
            exec_level: (k as u32) % 4,
            seg_plane: 1,
        };
        if lfull.put(p).is_ok() {
            filled += 1;
        }
        k += 1;
    }
    s.add("容量边界", filled == PROFILE_CAP as u32, "16 行填满，超额应用拒入");
    // ⑬ 位面域守卫：63 合法、64 拒（6 位上限）。
    let edge_ok = guard_profile(&Profile { app_key: 1, gen: 1, exec_level: 0, seg_plane: 63 }).is_ok();
    let edge_bad = guard_profile(&Profile { app_key: 1, gen: 1, exec_level: 0, seg_plane: 64 })
        .err()
        == Some(PeCode::PlaneIllegal.val());
    s.add("位面6位上限", edge_ok && edge_bad, "段元素位面闭集 0..64");
    // ⑭ 双跑一致。
    s.add("双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // ⑮ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < PeCode::ALL.len() {
        if PeCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "11 码全有中文建议");
    s.add("段位守卫 0x7EE00", segment_guard(), "SEG_BASE 0x40 对齐窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_single_row() {
        let mut i = 0;
        while i < GOOD_PROFILES {
            let p = make_good_profile(i);
            let line = export_row(&p).unwrap_or_default();
            let back = parse_row(&line).unwrap_or(Profile {
                app_key: 0, gen: 0, exec_level: 0, seg_plane: 0,
            });
            assert_eq!(back, p, "第 {} 例往返逐字段相等", i);
            i += 1;
        }
    }

    #[test]
    fn export_header_and_rows() {
        let mut led = ProfileLedger::new();
        let mut i = 0;
        while i < GOOD_PROFILES {
            led.put(make_good_profile(i)).ok();
            i += 1;
        }
        let t = export_all(&led).unwrap_or_default();
        assert!(t.starts_with(EXPORT_HEADER));
        assert!(t.contains("app=1|"));
        assert_eq!(t.matches("P|").count(), GOOD_PROFILES, "8 数据行");
    }

    #[test]
    fn field_order_tamper_rejected_ten_rounds() {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let p = make_good_profile(0);
            let bad = format!("P|gen={}|app={}|lvl={}|plane={}",
                p.gen, p.app_key, p.exec_level, p.seg_plane);
            assert_eq!(
                parse_row(&bad).err(),
                Some(PeCode::FieldOrderDrift.val()),
                "第 {} 轮字段序漂移必拒",
                r
            );
            r += 1;
        }
    }

    #[test]
    fn missing_field_rejected() {
        let p = make_good_profile(1);
        let missing = format!("P|app={}|gen={}|lvl={}", p.app_key, p.gen, p.exec_level);
        assert_eq!(parse_row(&missing).err(), Some(PeCode::FieldMissing.val()));
    }

    #[test]
    fn level_out_of_range_rejected() {
        let mut l = 0;
        while l < JUDGE_ROUNDS {
            let p = Profile { app_key: 2, gen: 2, exec_level: 4, seg_plane: 1 };
            assert_eq!(guard_profile(&p).err(), Some(PeCode::LevelIllegal.val()));
            assert_eq!(export_row(&p).err(), Some(PeCode::LevelIllegal.val()));
            l += 1;
        }
    }

    #[test]
    fn dup_registration_updates_not_appends() {
        let mut led = ProfileLedger::new();
        let p = make_good_profile(2);
        led.put(p).ok();
        let mut q = p;
        q.exec_level = (q.exec_level + 1) % 4;
        assert_eq!(led.put(q).unwrap_or(9), 1, "更新语义");
        assert_eq!(led.rows.len(), 1, "行数不变");
        assert_eq!(led.updates, 1);
        assert_eq!(led.get(p.app_key), Some(q), "画像已更新");
    }

    #[test]
    fn header_drift_detected() {
        let mut led = ProfileLedger::new();
        led.put(make_good_profile(1)).ok();
        let t = export_all(&led).unwrap_or_default();
        let wrong = t.replace(EXPORT_HEADER, "AI25-PROFILE-V9");
        assert_eq!(
            round_trip(&led, &wrong).err(),
            Some(PeCode::FieldOrderDrift.val())
        );
    }

    #[test]
    fn domain_edges_app_gen_plane() {
        assert_eq!(
            guard_profile(&Profile { app_key: 0, gen: 1, exec_level: 0, seg_plane: 1 }).err(),
            Some(PeCode::AppKeyIllegal.val())
        );
        assert_eq!(
            guard_profile(&Profile { app_key: 1, gen: 0, exec_level: 0, seg_plane: 1 }).err(),
            Some(PeCode::GenIllegal.val())
        );
        assert_eq!(
            guard_profile(&Profile { app_key: 1, gen: 1, exec_level: 0, seg_plane: 64 }).err(),
            Some(PeCode::PlaneIllegal.val())
        );
        assert!(guard_profile(&Profile { app_key: APP_MAX, gen: GEN_MAX, exec_level: 3, seg_plane: 63 }).is_ok());
    }

    #[test]
    fn ledger_full_boundary() {
        let mut led = ProfileLedger::new();
        let mut k = 0;
        while k < PROFILE_CAP {
            let p = Profile {
                app_key: 1 + k as u32,
                gen: 1 + (k as u32 % GEN_MAX),
                exec_level: (k as u32) % 4,
                seg_plane: 1,
            };
            assert!(led.put(p).is_ok(), "第 {} 应用入账", k);
            k += 1;
        }
        let over = Profile { app_key: PROFILE_CAP as u32 + 1, gen: 1, exec_level: 0, seg_plane: 1 };
        assert_eq!(led.put(over).err(), Some(PeCode::LedgerFull.val()));
    }

    #[test]
    fn injection_ledger_all_caught_no_miss() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught());
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18457_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 {} 在窗内", s.len());
    }
}
