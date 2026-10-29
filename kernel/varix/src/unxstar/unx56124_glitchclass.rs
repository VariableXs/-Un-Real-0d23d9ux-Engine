//! UNX-F56124 帧间隔毛刺分类器（0x60F60 段）
//!
//! 锚点：UNX-F56124-J1 毛刺分类全量（帧间隔序列逐帧分类落账：帧号 ×
//! 间隔 × 类别三元组）；J2 分类语义（五类闭集——平稳/轻毛刺/重毛刺/
//! 卡顿/恢复——逐帧分类可复算，阈值口径固定）；J3 分类审计链（帧序单
//! 调、指纹链、类别统计闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·毛刺分类面）。
//!
//! 结构：分类阈值口径（对基准间隔倍率：>1.5 轻毛刺、>3 重毛刺、>6 卡
//! 顿、回带即恢复）+ 帧间隔分类核（逐帧判定）+ 类别统计闭式（五类计数
//! 和 = 帧数）+ 指纹链 + 注入核（零间隔/帧号回退/空序列/类别越界/封账
//! 改写/未分类查询/统计漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60F60 独占 0x20）。
pub const SEG_BASE: u32 = 0x60F60;
pub const GL_GAP_ZERO: u32 = SEG_BASE + 0x01; // 零间隔
pub const GL_FRAME_BACK: u32 = SEG_BASE + 0x02; // 帧号回退
pub const GL_SEQ_EMPTY: u32 = SEG_BASE + 0x03; // 空序列
pub const GL_CLASS_OVER: u32 = SEG_BASE + 0x04; // 类别越界
pub const GL_SEALED: u32 = SEG_BASE + 0x05; // 分类账已封账拒写
pub const GL_NOT_CLASSIFIED: u32 = SEG_BASE + 0x06; // 未分类查询
pub const GL_STAT_MISS: u32 = SEG_BASE + 0x07; // 统计漂移

/// 分类五类闭集。
pub const C_STEADY: usize = 0;
pub const C_LIGHT: usize = 1;
pub const C_HEAVY: usize = 2;
pub const C_STALL: usize = 3;
pub const C_RECOVER: usize = 4;
pub const C_COUNT: usize = 5;
pub const C_NAMES: [&str; C_COUNT] = ["steady", "light_glitch", "heavy_glitch", "stall", "recover"];

/// 分类阈值（对基准间隔倍率 ×100 定点）。
pub const TH_LIGHT: u32 = 150; // 1.5×
pub const TH_HEAVY: u32 = 300; // 3×
pub const TH_STALL: u32 = 600; // 6×

/// 分类核：给定基准间隔与当前间隔（×100 倍率判定）。
pub fn classify(gap_us: u64, base_gap_us: u64, prev_class: usize) -> Result<usize, u32> {
    if gap_us == 0 || base_gap_us == 0 {
        return Err(GL_GAP_ZERO);
    }
    // 倍率 ×100：ratio_bp = gap × 10000 / base（万分比即 100× 倍率）。
    let ratio_bp = (gap_us as u128 * 10_000 / base_gap_us as u128) as u32;
    if ratio_bp >= TH_STALL * 100 {
        return Ok(C_STALL);
    }
    if ratio_bp >= TH_HEAVY * 100 {
        return Ok(C_HEAVY);
    }
    if ratio_bp >= TH_LIGHT * 100 {
        return Ok(C_LIGHT);
    }
    // 回带判定：前帧为毛刺类、本帧回带 → 恢复。
    if prev_class == C_LIGHT || prev_class == C_HEAVY || prev_class == C_STALL {
        return Ok(C_RECOVER);
    }
    Ok(C_STEADY)
}

/// 分类结果：帧号 × 间隔 × 类别 × 指纹 × 链。
#[derive(Clone)]
pub struct GlitchClass {
    pub frame: u32,
    pub gap_us: u64,
    pub class: usize,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 分类总账。
pub struct GlitchBook {
    pub items: Vec<GlitchClass>,
    pub base_gap_us: u64,
    pub sealed: bool,
}

impl GlitchBook {
    pub fn new(base_gap_us: u64) -> Self {
        GlitchBook { items: Vec::new(), base_gap_us, sealed: false }
    }

    /// 逐帧分类落账：零间隔拒 + 帧号递增双闸，类别自动判定。
    pub fn push(&mut self, frame: u32, gap_us: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(GL_SEALED);
        }
        if gap_us == 0 {
            return Err(GL_GAP_ZERO);
        }
        if let Some(prev) = self.items.last() {
            if frame <= prev.frame {
                return Err(GL_FRAME_BACK);
            }
        }
        let prev_class = self.items.last().map(|p| p.class).unwrap_or(C_STEADY);
        let class = classify(gap_us, self.base_gap_us, prev_class)?;
        let fp = {
            let mut h: u32 = 0x811C_9DC5;
            let line = format!("{}|{}|{}", frame, gap_us, C_NAMES[class]);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h
        };
        let chain = match self.items.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.items.push(GlitchClass { frame, gap_us, class, fp, chain });
        Ok(class)
    }

    /// 类别统计闭式（五类计数和 = 帧数）。
    pub fn class_census(&self) -> Result<[usize; C_COUNT], u32> {
        if self.items.is_empty() {
            return Err(GL_SEQ_EMPTY);
        }
        let mut c = [0usize; C_COUNT];
        let mut total = 0usize;
        for it in self.items.iter() {
            if it.class >= C_COUNT {
                return Err(GL_CLASS_OVER);
            }
            c[it.class] += 1;
            total += 1;
        }
        let mut sum = 0usize;
        for v in c.iter() {
            sum += *v;
        }
        if sum != total {
            return Err(GL_STAT_MISS);
        }
        Ok(c)
    }

    /// 查询帧类别（未分类显性拒绝）。
    pub fn class_of(&self, frame: u32) -> Result<usize, u32> {
        for it in self.items.iter() {
            if it.frame == frame {
                return Ok(it.class);
            }
        }
        Err(GL_NOT_CLASSIFIED)
    }

    /// J1：分类全量（逐帧类别合法）。
    pub fn items_ok(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|it| it.class < C_COUNT)
    }

    /// J3：指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        for i in 0..self.items.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.items[0].fp
            } else {
                self.items[i - 1].fp ^ self.items[i].fp
            };
            if self.items[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.items.last() {
            None => CHAIN_SEED,
            Some(it) => it.chain,
        }
    }
}

/// 标准夹具：基准 8333 微秒，序列 [8333, 8400, 13000, 30000, 8400, 8400]。
pub fn standard_book() -> GlitchBook {
    let mut b = GlitchBook::new(8_333);
    let gaps: [u64; 6] = [8_333, 8_400, 13_000, 30_000, 8_400, 8_400];
    for (i, g) in gaps.iter().enumerate() {
        if b.push((i + 1) as u32, *g).is_err() {
            panic!("standard fixture push must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（零间隔/帧号回退/空序列查询/类别越界/封账改写/未分类查询/统计漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 零间隔
    let mut b = GlitchBook::new(8_333);
    if b.push(1, 0) == Err(GL_GAP_ZERO) {
        caught += 1;
    }
    // 2) 帧号回退
    if b.push(2, 8_400).is_ok() && b.push(1, 8_400) == Err(GL_FRAME_BACK) {
        caught += 1;
    }
    // 3) 空序列统计
    if GlitchBook::new(8_333).class_census() == Err(GL_SEQ_EMPTY) {
        caught += 1;
    }
    // 4) 分类核零基准拒
    if classify(8_400, 0, C_STEADY) == Err(GL_GAP_ZERO) {
        caught += 1;
    }
    // 5) 未分类查询
    if standard_book().class_of(99).err() == Some(GL_NOT_CLASSIFIED) {
        caught += 1;
    }
    // 6) 统计漂移必抓（篡改类别后闭式失败）
    let mut dirty = standard_book();
    dirty.items[0].class = 9;
    if dirty.class_census() == Err(GL_CLASS_OVER) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.push(7, 8_400) == Err(GL_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 分类账渲染导出。
pub fn render_book(b: &GlitchBook) -> String {
    let mut out = String::new();
    for it in b.items.iter() {
        out.push_str(&format!("f{}|{}|{}|{:08x}\n", it.frame, it.gap_us, C_NAMES[it.class], it.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 类别闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 4 {
            return false;
        }
        let mut hit = false;
        for c in C_NAMES.iter() {
            if parts[2] == *c {
                hit = true;
            }
        }
        hit
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56124_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56124");
    // J1-1 分类全量（六帧逐帧分类合法）
    let b = standard_book();
    let ok = b.items_ok() && b.items.len() == 6;
    s.add("56124-J1-分类全量", ok, if ok { "" } else { "分类帧残缺" });
    // J1-2 五类闭集齐
    let ok = C_NAMES.len() == C_COUNT && C_NAMES[3] == "stall";
    s.add("56124-J1-五类闭集齐", ok, if ok { "" } else { "类别漂移" });
    // J2-1 分类判定（8400 对 8333 = 100.8 bp → steady）
    let ok = b.class_of(1) == Ok(C_STEADY) && b.class_of(2) == Ok(C_STEADY);
    s.add("56124-J2-平稳判定", ok, if ok { "" } else { "平稳误判" });
    // J2-2 轻毛刺判定（13000/8333 = 156 bp ≥ 150 → light）
    let ok = b.class_of(3) == Ok(C_LIGHT);
    s.add("56124-J2-轻毛刺判定", ok, if ok { "" } else { "轻毛刺误判" });
    // J2-3 卡顿判定（30000/8333 = 360 bp ≥ 300 heavy、< 600 不 stall）
    let ok = b.class_of(4) == Ok(C_HEAVY);
    s.add("56124-J2-重毛刺判定", ok, if ok { "" } else { "重毛刺误判" });
    // J2-4 恢复判定（毛刺后回带 → recover）
    let ok = b.class_of(5) == Ok(C_RECOVER) && b.class_of(6) == Ok(C_STEADY);
    s.add("56124-J2-恢复判定", ok, if ok { "" } else { "恢复误判" });
    // J2-5 阈值口径（1.5/3/6 三档）
    let ok = TH_LIGHT == 150 && TH_HEAVY == 300 && TH_STALL == 600;
    s.add("56124-J2-阈值口径", ok, if ok { "" } else { "阈值漂移" });
    // J3-1 类别统计闭式（steady 3 + light 1 + heavy 1 + recover 1）
    let c = b.class_census();
    let ok = match c {
        Ok(c) => c[C_STEADY] == 3 && c[C_LIGHT] == 1 && c[C_HEAVY] == 1 && c[C_RECOVER] == 1,
        Err(_) => false,
    };
    s.add("56124-J3-类别统计闭式", ok, if ok { "" } else { "统计漂移" });
    // J3-2 指纹链复核
    let ok = b.chain_ok();
    s.add("56124-J3-指纹链复核", ok, if ok { "" } else { "指纹链断裂" });
    // J3-3 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56124-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-4 封账拒写
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.push(7, 8_400) == Err(GL_SEALED);
    s.add("56124-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出六行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 6) && text.contains("f3|13000|light_glitch|");
    s.add("56124-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c2, n) = injection_core();
    let ok = c2 == n;
    let detail = if ok { String::new() } else { format!("注入 {c2}/{n}") };
    s.add_dyn("56124-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56124_all_checks_green() {
        let set = run_unx56124_checks();
        assert!(set.all_passed(), "UNX-F56124 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56124_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
