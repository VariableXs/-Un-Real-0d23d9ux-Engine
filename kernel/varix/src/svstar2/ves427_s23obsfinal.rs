//! ves427_s23obsfinal — VE-F4187 · T 域体验日志与观测终审
//! 终审：T 域观测终审——体验日志埋点口径冻结（国际化事件四类埋点：语种切换/翻译缺失回退/地区格式变更/方向切换）
//! +观测面板项终版+留痕采样率复核；
//! 口径漂移→对版修正；面板缺项→补齐；采样失真→重校；
//! 埋点不含可识别用户内容（隐私红线复述）；采样脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x11A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObCode {
    Ok = 0,
    CaliberDrift = SEG_BASE + 0x01, // 0x11A1
    PanelMissing = SEG_BASE + 0x02, // 0x11A2
    SampleSkew = SEG_BASE + 0x03,   // 0x11A3
    UserContent = SEG_BASE + 0x04,  // 0x11A4 隐私红线
}

/// 四类埋点（域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind { LangSwitch = 0, MtFallback = 1, RegionFormat = 2, DirSwitch = 3 }

impl EventKind {
    pub const ALL: [EventKind; 4] = [
        EventKind::LangSwitch, EventKind::MtFallback, EventKind::RegionFormat, EventKind::DirSwitch];
    pub fn name(self) -> &'static str {
        match self {
            EventKind::LangSwitch => "lang-switch", EventKind::MtFallback => "mt-fallback",
            EventKind::RegionFormat => "region-format", EventKind::DirSwitch => "dir-switch",
        }
    }
}

/// 埋点口径条目：事件类 + 口径版本 + 是否含用户内容（必须否）+ 采样率（bp）。
#[derive(Clone, PartialEq)]
pub struct CaliberEntry { pub kind: EventKind, pub caliber_ver: u32, pub has_user_content: bool, pub sample_bp: u32 }

/// 观测终审册。
pub struct ObsFinalBook {
    pub calibers: Vec<CaliberEntry>,
    pub frozen_ver: u32,
    pub panel: Vec<EventKind>, // 面板项终版
}

impl ObsFinalBook {
    pub fn new() -> Self { ObsFinalBook { calibers: Vec::new(), frozen_ver: 0, panel: Vec::new() } }
    pub fn put(&mut self, kind: EventKind, ver: u32, uc: bool, bp: u32) {
        self.calibers.retain(|c| c.kind != kind);
        self.calibers.push(CaliberEntry { kind, caliber_ver: ver, has_user_content: uc, sample_bp: bp });
    }
    pub fn caliber_ver(&self, k: EventKind) -> u32 {
        self.calibers.iter().find(|c| c.kind == k).map(|c| c.caliber_ver).unwrap_or(0)
    }
    /// 口径冻结：四类齐+全部不含用户内容+口径版本一致才可冻。
    pub fn freeze(&mut self, ver: u32) -> Result<(), ObCode> {
        for k in EventKind::ALL {
            let c = match self.calibers.iter().find(|c| c.kind == k) {
                Some(c) => c, None => return Err(ObCode::CaliberDrift),
            };
            if c.has_user_content { return Err(ObCode::UserContent); }
            if c.caliber_ver != ver { return Err(ObCode::CaliberDrift); }
        }
        self.frozen_ver = ver;
        Ok(())
    }
    /// 面板缺项检查。
    pub fn panel_gaps(&self) -> Vec<EventKind> {
        EventKind::ALL.iter().filter(|k| !self.panel.contains(k)).map(|k| *k).collect()
    }
    /// 采样率复核：每类须在 [MIN_BP, MAX_BP] 内，失真→重校码。
    pub fn sample_recheck(&self) -> Result<(), ObCode> {
        for c in &self.calibers {
            if c.sample_bp < 100 || c.sample_bp > 10_000 { return Err(ObCode::SampleSkew); }
        }
        Ok(())
    }
}

// ───────────── 深化块 D427-A · 埋点事件流账本 ─────────────
// 口径冻结只管"登记"，缺事件流入账与量分布留痕。补结构化事件流账本。

/// 一条埋点事件：批次 + 事件类 + 步序号 + 载荷长度（脱敏后仅计长，禁内容）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EventRec {
    pub batch: u16,
    pub kind: EventKind,
    pub step: u32,
    pub payload_len: u16, // 仅长度：隐私红线（不含可识别用户内容）
}

/// 事件流账本：定容量 + 幂等闸（同批同步同类去重）+ 守恒 + 分位。
pub struct EventLedger {
    pub recs: Vec<EventRec>,
    pub cap: usize,
}

impl EventLedger {
    pub fn new(cap: usize) -> Self { EventLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；重复键→幂等返回 false。
    pub fn push(&mut self, r: EventRec) -> Result<bool, ObCode> {
        if self.recs.len() >= self.cap { return Err(ObCode::PanelMissing); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.step == r.step && x.kind == r.kind) {
            return Ok(false);
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑载荷长 ≡ ∑(按事件类分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.payload_len as u64).sum();
        let mut by_kind: u64 = 0;
        for k in EventKind::ALL {
            by_kind += self.recs.iter().filter(|r| r.kind == k)
                .map(|r| r.payload_len as u64).sum::<u64>();
        }
        total == by_kind
    }
    /// 事件类计数投影：四类各自条数。
    pub fn kind_counts(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs { out[r.kind as usize] += 1; }
        out
    }
    /// 类内步序分位（整数排序取点，禁浮点）。
    pub fn kind_step_quantile(&self, k: EventKind, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.kind == k)
            .map(|r| r.step).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D427-B · 边界探针 ─────────────

/// 探针 P1：载荷越界（超过采样留痕上限）→显性 Err（红线：驻留最小化）。
pub const PAYLOAD_CAP: u16 = 512;
pub fn probe_payload_cap(len: u16) -> Result<(), ObCode> {
    if len > PAYLOAD_CAP { return Err(ObCode::UserContent); }
    Ok(())
}

/// 探针 P2：空类步序分位/越界分位号→显性 Err。
pub fn probe_step_quantile(lg: &EventLedger, k: EventKind, q: u32) -> Result<u32, ObCode> {
    if q == 0 || q > 100 { return Err(ObCode::SampleSkew); }
    if lg.recs.iter().filter(|r| r.kind == k).count() == 0 { return Err(ObCode::CaliberDrift); }
    Ok(lg.kind_step_quantile(k, q))
}

// ───────────── 深化块 D427-C · LCG 注入回归 ─────────────

/// 8 行确定性 LCG。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn pick(&mut self, n: u32) -> u32 { ((self.next() >> 33) as u32) % n }
}

/// 注入回归：N 轮随机采样率注入，超带（<100 或 >10_000 bp）者
/// sample_recheck 必抓（100%）；带内者零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_sample_skew_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x27_27_27_27);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let bp = g.pick(12_000); // [0, 12_000) 覆盖带内带外
        let mut bk = ObsFinalBook::new();
        for k in EventKind::ALL {
            let v = if k == EventKind::LangSwitch { bp } else { 1_000 };
            bk.put(k, 3, false, v);
        }
        let expect_catch = bp < 100 || bp > 10_000;
        match bk.sample_recheck() {
            Err(ObCode::SampleSkew) if expect_catch => caught += 1,
            Ok(()) if !expect_catch => {}
            _ => false_pos += 1,
        }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D427-D · 序列化投影 + FNV 指纹 ─────────────

/// CaliberEntry 定长 10 字节编码：kind(1)+ver(4,LE)+uc(1)+bp(4,LE)。
pub fn encode_caliber(c: &CaliberEntry) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    out.push(c.kind as u8);
    out.extend_from_slice(&c.caliber_ver.to_le_bytes());
    out.push(if c.has_user_content { 1 } else { 0 });
    out.extend_from_slice(&c.sample_bp.to_le_bytes());
    out
}

/// 解码：错长/坏类号→显性 Err。
pub fn decode_caliber(b: &[u8]) -> Result<CaliberEntry, ObCode> {
    if b.len() != 10 { return Err(ObCode::CaliberDrift); }
    let kind = match EventKind::ALL.iter().find(|k| **k as u8 == b[0]) {
        Some(k) => *k,
        None => return Err(ObCode::CaliberDrift),
    };
    let mut ver = [0u8; 4];
    ver.copy_from_slice(&b[1..5]);
    let mut bp = [0u8; 4];
    bp.copy_from_slice(&b[6..10]);
    Ok(CaliberEntry { kind, caliber_ver: u32::from_le_bytes(ver),
        has_user_content: b[5] == 1, sample_bp: u32::from_le_bytes(bp) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 口径册指纹：冻结版本 + 四类口径条目序列化后 FNV（口径快照指纹）。
pub fn caliber_fingerprint(bk: &ObsFinalBook) -> u32 {
    let mut bytes = bk.frozen_ver.to_le_bytes().to_vec();
    for k in EventKind::ALL {
        match bk.calibers.iter().find(|c| c.kind == k) {
            Some(c) => bytes.extend_from_slice(&encode_caliber(c)),
            None => bytes.extend_from_slice(&[0xEEu8; 10]),
        }
    }
    fnv1a(&bytes)
}

/// 指纹十六进制投影。
pub fn fingerprint_hex(h: u32) -> String {
    let mut out = String::new();
    for i in 0..8 {
        let nib = ((h >> (28 - i * 4)) & 0xF) as u8;
        out.push(if nib < 10 { (b'0' + nib) as char } else { (b'a' + nib - 10) as char });
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves427_checks() -> CheckSet {
    let mut set = CheckSet::new("ves427");
    let mut bk = ObsFinalBook::new();

    // C427-01 四类齐前冻结拒（口径漂移码）。
    bk.put(EventKind::LangSwitch, 3, false, 1_000);
    chk(&mut set, "C427-01", bk.freeze(3) == Err(ObCode::CaliberDrift), "口径不齐拒冻");

    // C427-02 埋点含用户内容→隐私红线码（域本色红线复述）。
    bk.put(EventKind::MtFallback, 3, true, 1_000);
    chk(&mut set, "C427-02", bk.freeze(3) == Err(ObCode::UserContent), "用户内容红线");

    // C427-03 修正为不含用户内容。
    bk.put(EventKind::MtFallback, 3, false, 1_000);

    // C427-04 口径版本漂移拒。
    bk.put(EventKind::RegionFormat, 2, false, 500);
    chk(&mut set, "C427-04", bk.freeze(3) == Err(ObCode::CaliberDrift), "版本漂移拒");

    // C427-05 对版修正后四类齐冻。
    bk.put(EventKind::RegionFormat, 3, false, 500);
    bk.put(EventKind::DirSwitch, 3, false, 2_000);
    chk(&mut set, "C427-05", bk.freeze(3).is_ok() && bk.frozen_ver == 3, "四类齐冻结");

    // C427-06 面板缺项定位。
    bk.panel.push(EventKind::LangSwitch);
    let gaps = bk.panel_gaps();
    chk(&mut set, "C427-06", gaps.len() == 3 && gaps.contains(&EventKind::DirSwitch), "面板缺项定位");

    // C427-07 补齐面板项终版。
    bk.panel.push(EventKind::MtFallback);
    bk.panel.push(EventKind::RegionFormat);
    bk.panel.push(EventKind::DirSwitch);
    chk(&mut set, "C427-07", bk.panel_gaps().is_empty() && bk.panel.len() == 4, "面板终版齐");

    // C427-08 采样率失真（过低）→重校。
    bk.put(EventKind::LangSwitch, 3, false, 50);
    chk(&mut set, "C427-08", bk.sample_recheck() == Err(ObCode::SampleSkew), "采样过低重校");

    // C427-09 采样率失真（过高超 100%）→重校。
    bk.put(EventKind::LangSwitch, 3, false, 10_500);
    chk(&mut set, "C427-09", bk.sample_recheck() == Err(ObCode::SampleSkew), "采样超界重校");

    // C427-10 复核后采样带内通过。
    bk.put(EventKind::LangSwitch, 3, false, 1_000);
    chk(&mut set, "C427-10", bk.sample_recheck().is_ok(), "采样带内通过");

    // C427-11 四类埋点名单单源。
    chk(&mut set, "C427-11", EventKind::ALL.len() == 4
        && EventKind::ALL[0].name() == "lang-switch"
        && EventKind::ALL[3].name() == "dir-switch", "四类单源");

    // C427-12 埋点口径覆盖幂等（同事件替换）。
    chk(&mut set, "C427-12", bk.calibers.len() == 4, "口径覆盖幂等");

    // ── 深化块 D427 判据 ──
    // D427-01 事件流账本：两批四类入账 + 守恒恒等式。
    let mut lg = EventLedger::new(64);
    let _ = lg.push(EventRec { batch: 1, kind: EventKind::LangSwitch, step: 10, payload_len: 32 });
    let _ = lg.push(EventRec { batch: 1, kind: EventKind::MtFallback, step: 20, payload_len: 48 });
    let _ = lg.push(EventRec { batch: 2, kind: EventKind::RegionFormat, step: 5, payload_len: 16 });
    let _ = lg.push(EventRec { batch: 2, kind: EventKind::DirSwitch, step: 8, payload_len: 24 });
    chk(&mut set, "D427-01", lg.recs.len() == 4 && lg.conserves(), "事件流守恒");

    // D427-02 幂等闸：同批同步同类二次提交零副作用。
    chk(&mut set, "D427-02", lg.push(EventRec { batch: 1, kind: EventKind::LangSwitch, step: 10, payload_len: 999 }) == Ok(false)
        && lg.recs.len() == 4, "事件幂等闸");

    // D427-03 类计数投影 + 批次去重。
    chk(&mut set, "D427-03", lg.kind_counts() == [1, 1, 1, 1] && lg.batch_count() == 2, "类计数投影");

    // D427-04 步序分位（整数）。
    chk(&mut set, "D427-04", lg.kind_step_quantile(EventKind::LangSwitch, 50) == 10
        && lg.kind_step_quantile(EventKind::MtFallback, 100) == 20, "步序分位");

    // D427-05 边界探针：载荷超上限→红线码；带内放行。
    chk(&mut set, "D427-05", probe_payload_cap(513) == Err(ObCode::UserContent)
        && probe_payload_cap(PAYLOAD_CAP).is_ok(), "载荷上限探针");

    // D427-06 边界探针：越界分位号/空类→显性 Err。
    chk(&mut set, "D427-06", probe_step_quantile(&lg, EventKind::LangSwitch, 0) == Err(ObCode::SampleSkew)
        && probe_step_quantile(&lg, EventKind::LangSwitch, 101) == Err(ObCode::SampleSkew), "分位探针");

    // D427-07 LCG 采样失真注入回归：24 轮 100% 抓带外 + 零误报。
    let (n, c, fp) = lcg_sample_skew_regression(24);
    chk(&mut set, "D427-07", n == 24 && c + fp == n && fp == 0 && c > 0, "采样失真注入回归");

    // D427-08 口径条目序列化往返。
    let c0 = CaliberEntry { kind: EventKind::DirSwitch, caliber_ver: 3, has_user_content: false, sample_bp: 2_000 };
    chk(&mut set, "D427-08", decode_caliber(&encode_caliber(&c0)) == Ok(c0.clone()), "口径序列化往返");

    // D427-09 口径解码边界：错长/坏类号→显性 Err。
    chk(&mut set, "D427-09", decode_caliber(&[0u8; 9]) == Err(ObCode::CaliberDrift)
        && decode_caliber(&{ let mut v = encode_caliber(&c0); v[0] = 0x55; v }) == Err(ObCode::CaliberDrift), "口径解码边界");

    // D427-10 口径册指纹：冻结前/后指纹变 + 确定性 + hex 投影。
    let mut bkc = ObsFinalBook::new();
    for k in EventKind::ALL { bkc.put(k, 3, false, 1_000); }
    let f_pre = caliber_fingerprint(&bkc);
    let _ = bkc.freeze(3);
    let f_post = caliber_fingerprint(&bkc);
    let f_post2 = caliber_fingerprint(&bkc);
    chk(&mut set, "D427-10", f_pre != f_post && f_post == f_post2
        && fingerprint_hex(f_post).len() == 8, "口径册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves427_smoke() {
        let set = run_ves427_checks();
        assert!(set.failed_count() == 0, "ves427 red: {}", set.red_items_text());
    }
}
