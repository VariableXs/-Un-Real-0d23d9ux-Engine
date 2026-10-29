//! VE-F4305 · 一致性×主题生态（U06 场景应用组）
//! 职责：主题上架基线三项（令牌完整覆盖/对比度达标/暗色等价）+ 生态巡检
//! 同步提醒（三次未响应降权）+ 劣化梯度（降权直至下架）+ 申诉复审改判
//! + 对比度基线即无障碍硬线（域色）+ 门禁复用同域第二用。
//! 段位：0x3060（续表 idx539）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3060;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmCode {
    /// 基线缺项（拒收）。
    BaseMissing = SEG_BASE,
    /// 对比度不达标（无障碍硬线——域色）。
    ContrastFail = SEG_BASE + 0x01,
    /// 下架。
    OffShelf = SEG_BASE + 0x02,
    /// 生态账漂移（重复上架同号/越界主题查询）。
    ThemeDrift = SEG_BASE + 0x03,
    /// 主题指纹失配（序列化损坏）。
    ThemeFpMismatch = SEG_BASE + 0x04,
}

/// 主题基线三项。
#[derive(PartialEq, Clone, Copy, Debug)]
pub struct ThemeBase {
    pub token_cover: u32,   // 令牌覆盖率 bp
    pub contrast_bp: u32,   // 对比度 bp（域色硬线）
    pub dark_equiv: bool,   // 暗色等价
}

impl ThemeBase {
    pub fn ok(&self, cover_cap: u32, contrast_cap: u32) -> bool {
        self.token_cover >= cover_cap && self.contrast_bp >= contrast_cap && self.dark_equiv
    }
}

/// 主题资产。
#[derive(PartialEq, Clone, Copy, Debug)]
pub struct Theme {
    pub id: u16,
    pub base: ThemeBase,
    pub listed: bool,
    pub weight: u8, // 权重（降权梯度 0-100）
    pub misses: u8, // 巡检未响应次数
    pub off: bool,
}

/// 主题生态器。
pub struct ThemeEco {
    pub themes: Vec<Theme>,
    /// 令牌全集数。
    pub token_total: u32,
    pub contrast_cap_bp: u32,
    pub cover_cap_bp: u32,
    pub appeals: Vec<(u16, bool)>,
}

impl ThemeEco {
    pub fn new(token_total: u32, cover_cap_bp: u32, contrast_cap_bp: u32) -> Self {
        ThemeEco { themes: Vec::new(), token_total, cover_cap_bp, contrast_cap_bp, appeals: Vec::new() }
    }

    /// 上架申请：三项基线。
    pub fn apply(&mut self, id: u16, covered_tokens: u32, contrast_bp: u32, dark: bool) -> Result<(), TmCode> {
        let cover = covered_tokens * 10_000 / self.token_total.max(1);
        let base = ThemeBase { token_cover: cover, contrast_bp, dark_equiv: dark };
        if contrast_bp < self.contrast_cap_bp {
            return Err(TmCode::ContrastFail); // 域色硬线
        }
        if !base.ok(self.cover_cap_bp, self.contrast_cap_bp) {
            return Err(TmCode::BaseMissing);
        }
        self.themes.push(Theme { id, base, listed: true, weight: 100, misses: 0, off: false });
        Ok(())
    }

    /// 巡检提醒：未响应计数。
    pub fn patrol_miss(&mut self, id: u16) -> Result<(), TmCode> {
        let t = self.themes.iter_mut().find(|t| t.id == id).ok_or(TmCode::OffShelf)?;
        t.misses += 1;
        if t.misses >= 3 {
            t.weight = t.weight.saturating_sub(30); // 降权梯度
            if t.weight == 0 {
                t.listed = false;
                t.off = true;
                return Err(TmCode::OffShelf);
            }
        }
        Ok(())
    }

    /// 申诉复审改判。
    pub fn appeal(&mut self, id: u16, ok: bool) -> Result<(), TmCode> {
        let t = self.themes.iter_mut().find(|t| t.id == id).ok_or(TmCode::OffShelf)?;
        self.appeals.push((id, ok));
        if ok {
            t.misses = 0;
            t.weight = 100;
            if t.off {
                t.off = false;
                t.listed = true;
            }
            Ok(())
        } else {
            Err(TmCode::OffShelf)
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C539-01..12。
pub fn run_ves539_checks() -> CheckSet {
    let mut set = CheckSet::new("ves539");

    let mut te = ThemeEco::new(100, 9_000, 4_500);
    chk(&mut set, "C539-01", te.apply(1, 95, 4_000, true) == Err(TmCode::ContrastFail), "对比度不达标=域色硬线拒收");
    chk(&mut set, "C539-02", te.apply(1, 50, 5_000, true) == Err(TmCode::BaseMissing), "令牌覆盖不足拒收");
    chk(&mut set, "C539-03", te.apply(1, 95, 5_000, false) == Err(TmCode::BaseMissing), "暗色不等价拒收");
    chk(&mut set, "C539-04", te.apply(1, 95, 5_000, true).is_ok(), "三项基线全过上架");
    chk(&mut set, "C539-05", te.themes[0].weight == 100, "初始权重满");

    // 巡检降权梯度
    let _ = te.patrol_miss(1);
    let _ = te.patrol_miss(1);
    chk(&mut set, "C539-06", te.themes[0].misses == 2 && te.themes[0].weight == 100, "两次未响应仍满权（3 次触发）");
    chk(&mut set, "C539-07", te.patrol_miss(1).is_ok(), "第三次触发降权 30");
    chk(&mut set, "C539-08", te.themes[0].weight == 70, "降权梯度在册");

    // 申诉复位
    chk(&mut set, "C539-09", te.appeal(1, true).is_ok() && te.themes[0].weight == 100, "申诉成立复位");
    chk(&mut set, "C539-10", te.appeals.len() == 1 && te.appeals[0].1, "改判留痕");

    // 极端降权至下架
    for _ in 0..9 {
        let _ = te.patrol_miss(1);
    }
    chk(&mut set, "C539-11", te.themes[0].off, "多次未响应权重归零下架");
    chk(&mut set, "C539-12", te.appeal(1, true).is_ok() && !te.themes[0].off, "申诉复探下架复位");

    ves539_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：权重梯度账本（主题×步归因）+ 守恒恒等式 + 边界探针
// + 生态权重整数分位投影 + LCG 巡检注入回归 + 幂等闸
// + 主题 6 字节序列化往返 + FNV-1a 生态指纹。

use alloc::string::{String, ToString};

/// 权重梯度账目：主题 × 步 × 降权后权重。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightLog {
    pub id: u16,
    pub step: u16,
    pub weight: u8,
}

/// 主题生态深化账本。
pub struct WeightLedger {
    pub logs: Vec<WeightLog>,
    pub step: u16,
}

impl WeightLedger {
    pub fn new() -> Self {
        WeightLedger { logs: Vec::new(), step: 0 }
    }

    /// 记一笔权重变化。
    pub fn log(&mut self, id: u16, weight: u8) {
        self.step = self.step.wrapping_add(1);
        self.logs.push(WeightLog { id, step: self.step, weight });
    }

    /// 守恒恒等式：同主题账内权重序列单调不升（降权梯度不可逆，
    /// 复位由申诉走独立通道不落此账）。
    pub fn conservation(&self) -> bool {
        for id in 0..=u16::MAX {
            let mut prev: Option<u8> = None;
            for l in self.logs.iter().filter(|l| l.id == id) {
                if let Some(p) = prev {
                    if l.weight > p {
                        return false;
                    }
                }
                prev = Some(l.weight);
            }
        }
        true
    }

    /// 投影：主题当前账面权重（最近一笔）。
    pub fn latest(&self, id: u16) -> Option<u8> {
        self.logs.iter().filter(|l| l.id == id).last().map(|l| l.weight)
    }

    /// 投影：全生态账面权重中位（排序取点，整数禁浮点）。
    pub fn weight_median(&self) -> u32 {
        let mut v: Vec<u8> = self.logs.iter().map(|l| l.weight).collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        u32::from(v[v.len() / 2])
    }

    /// 边界探针：空账查询 → None。
    pub fn empty_probe(&self) -> bool {
        self.logs.is_empty() && self.latest(1).is_none() && self.weight_median() == 0
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

/// LCG 巡检注入回归：随机主题随机次数未响应——权重序列单调不升、
/// 归零必下架（listed=false）；申诉成立必复位 100（异常必抓）。
fn patrol_inject(seed: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..32u32 {
        let mut te = ThemeEco::new(100, 9_000, 4_500);
        if te.apply(1, 95, 5_000, true).is_err() {
            return Err("巡检注入：合法主题上架失败".to_string());
        }
        let mut ledger = WeightLedger::new();
        let times = (rng.next() % 12) + 1;
        for _ in 0..times {
            let _ = te.patrol_miss(1);
            ledger.log(1, te.themes[0].weight);
        }
        if !ledger.conservation() {
            return Err("巡检注入：权重账出现回升（梯度被破坏）".to_string());
        }
        let w = te.themes[0].weight;
        if w == 0 && (te.themes[0].listed || !te.themes[0].off) {
            return Err("巡检注入：权重归零未触发下架".to_string());
        }
        if w > 0 && !te.themes[0].listed {
            return Err("巡检注入：未归零却被下架".to_string());
        }
        let _ = te.appeal(1, true);
        if te.themes[0].weight != 100 {
            return Err("巡检注入：申诉成立未复位满权".to_string());
        }
    }
    Ok(())
}

/// 幂等闸：申诉成立重复提交零副作用。
fn idempotence_probe() -> bool {
    let mut te = ThemeEco::new(100, 9_000, 4_500);
    if te.apply(1, 95, 5_000, true).is_err() {
        return false;
    }
    let _ = te.patrol_miss(1);
    let _ = te.patrol_miss(1);
    let _ = te.patrol_miss(1);
    let _ = te.appeal(1, true);
    let snap = (te.themes[0].weight, te.themes[0].misses, te.appeals.len());
    let _ = te.appeal(1, true);
    (te.themes[0].weight, te.themes[0].misses, te.appeals.len()) == snap
}

/// 主题定长 6 字节编码：[id lo, id hi, 权重, 覆盖率 lo, 覆盖率 hi, 态位]。
/// 态位：bit0=listed bit1=off bit2=暗色等价。
pub fn encode_theme(t: &Theme) -> [u8; 6] {
    let st = u8::from(t.listed) | (u8::from(t.off) << 1) | (u8::from(t.base.dark_equiv) << 2);
    [
        (t.id & 0xFF) as u8,
        (t.id >> 8) as u8,
        t.weight,
        (t.base.token_cover & 0xFF) as u8,
        ((t.base.token_cover >> 8) & 0xFF) as u8,
        st,
    ]
}

/// 解码：态位高位保留位损坏 → ThemeFpMismatch。
pub fn decode_theme(b: &[u8; 6]) -> Result<Theme, TmCode> {
    if b[5] & 0xF8 != 0 {
        return Err(TmCode::ThemeFpMismatch);
    }
    let id = b[0] as u16 | ((b[1] as u16) << 8);
    let cover = b[3] as u32 | ((b[4] as u32) << 8);
    Ok(Theme {
        id,
        base: ThemeBase { token_cover: cover, contrast_bp: 0, dark_equiv: b[5] & 0x04 != 0 },
        listed: b[5] & 0x01 != 0,
        weight: b[2],
        misses: 0,
        off: b[5] & 0x02 != 0,
    })
}

/// FNV-1a 32 位生态指纹。
pub fn fnv_themes(themes: &[Theme]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for t in themes {
        for byte in encode_theme(t) {
            h ^= byte as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// 深化判据并入主判据流（C539-13..23）。
pub fn ves539_deep_checks(set: &mut CheckSet) {
    let mut wl = WeightLedger::new();
    wl.log(1, 100);
    wl.log(1, 70);
    wl.log(1, 40);
    set.add("C539-13", wl.conservation(), "权重账守恒（同主题序列单调不升）");
    set.add("C539-14", wl.latest(1) == Some(40), "最近笔投影");
    set.add("C539-15", wl.weight_median() == 70, "权重中位整数取点");
    wl.log(1, 90); // 违例注入：权重回升
    set.add("C539-16", !wl.conservation(), "回升违例被守恒恒等式抓出");

    match patrol_inject(0x7E39_3901) {
        Ok(()) => set.add("C539-16", true, "LCG 巡检注入 32 轮梯度单调+归零必下架+申诉复位"),
        Err(msg) => set.add_dyn("C539-16", false, &msg),
    }

    set.add("C539-17", idempotence_probe(), "幂等闸：申诉重复提交零副作用");

    let t = Theme { id: 0x0BEE, base: ThemeBase { token_cover: 9_500, contrast_bp: 5_000, dark_equiv: true }, listed: true, weight: 70, misses: 0, off: false };
    match decode_theme(&encode_theme(&t)) {
        Ok(back) => set.add("C539-18", back.id == t.id && back.weight == 70 && back.listed && back.base.dark_equiv, "主题 6 字节编码解码往返"),
        Err(_) => set.add("C539-18", false, "主题解码异常失败"),
    }
    set.add("C539-19", decode_theme(&[1, 2, 3, 4, 5, 0xF8]) == Err(TmCode::ThemeFpMismatch), "主题态位损坏拒绝解码");

    let mut te2 = ThemeEco::new(100, 9_000, 4_500);
    let _ = te2.apply(1, 95, 5_000, true);
    let fp1 = fnv_themes(&te2.themes);
    let _ = te2.patrol_miss(1);
    let _ = te2.patrol_miss(1);
    let _ = te2.patrol_miss(1);
    set.add("C539-20", fnv_themes(&te2.themes) != fp1, "FNV 生态指纹随降权漂移");
    set.add("C539-21", WeightLedger::new().empty_probe(), "空权重账边界探针");
    set.add("C539-22", te2.appeal(9, true) == Err(TmCode::OffShelf), "越界主题申诉探针显性报错");
    set.add("C539-23", fnv_themes(&[]) == 0x811C_9DC5, "空生态指纹偏移基");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_eco() {
        let mut te = ThemeEco::new(10, 9_000, 4_500);
        assert!(te.apply(1, 10, 5_000, true).is_ok());
        assert_eq!(te.apply(2, 10, 4_000, true), Err(TmCode::ContrastFail));
        for _ in 0..3 {
            let _ = te.patrol_miss(1);
        }
        assert_eq!(te.themes[0].weight, 70);
    }
}
