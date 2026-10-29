//! UNX-F56125 帧率对照行生成器（0x60F80 段）
//!
//! 锚点：UNX-F56125-J1 行账全量（对照行逐条落账：键 × A 侧帧率 ×
//! B 侧帧率 × 万分比差值）；J2 口径语义（metric=fps 闭集、帧率 ×100
//! 定点合法带、差值 i64 中转可复算）；J3 行账审计链（键序指纹、封
//! 账门、渲染完整）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·对照行面）。
//!
//! 结构：行登记簿（键闭集拼接、唯一键）+ 差值核（i64 中转、截断向
//! 零）+ 统计闭式（两侧均值复算对账）+ 键序指纹链 + 注入核（零帧率/
//! 帧率越界/重复键/封账拒写/未封导出/缺行查询/统计漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60F80 独占 0x20）。
pub const SEG_BASE: u32 = 0x60F80;
pub const FR_FPS_ZERO: u32 = SEG_BASE + 0x01; // 零帧率
pub const FR_FPS_RANGE: u32 = SEG_BASE + 0x02; // 帧率越界
pub const FR_DUP_KEY: u32 = SEG_BASE + 0x03; // 重复键
pub const FR_SEALED: u32 = SEG_BASE + 0x04; // 封账/账满拒写
pub const FR_NOT_SEALED: u32 = SEG_BASE + 0x05; // 未封账导出
pub const FR_NO_ROW: u32 = SEG_BASE + 0x06; // 缺行查询/键段缺失
pub const FR_STAT_MISS: u32 = SEG_BASE + 0x07; // 统计漂移

/// 口径闭集：metric 固定 fps（帧率 ×100 定点）。
pub const METRIC_FPS: &str = "fps";
pub const FPS_MIN: u64 = 100; // 1.00 fps
pub const FPS_MAX: u64 = 120_000; // 1200.00 fps

/// 键段闭集：分辨率 × 同步方式。
pub const RES_SET: [&str; 3] = ["1080p", "1440p", "2160p"];
pub const SYNC_SET: [&str; 2] = ["fixed", "vrr"];

fn res_ok(r: &str) -> bool {
    RES_SET.contains(&r)
}

fn sync_ok(s: &str) -> bool {
    SYNC_SET.contains(&s)
}

/// 帧率 ×100 定点合法带。
pub fn fps_ok(v: u64) -> bool {
    (FPS_MIN..=FPS_MAX).contains(&v)
}

/// 差值核：delta_bp = (b − a) × 10000 / a（i64 中转、截断向零）。
pub fn delta_bp(a: u64, b: u64) -> i64 {
    ((b as i64 - a as i64) * 10_000) / a as i64
}

/// 对照行。
pub struct FpsRow {
    pub no: usize,
    pub key: String,
    pub a_x100: u64,
    pub b_x100: u64,
    pub delta_bp: i64,
}

const CHAIN_SEED: u32 = 0x811C_9DC5;
const CHAIN_PRIME: u32 = 0x0100_0193;

fn fold_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h.wrapping_mul(CHAIN_PRIME)
}

fn fold_str(mut h: u32, s: &str) -> u32 {
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(CHAIN_PRIME);
    }
    h
}

fn fold_i64(h: u32, v: i64) -> u32 {
    fold_u32(fold_u32(h, v as u32), (v >> 32) as u32)
}

/// 帧率对照行登记簿。
pub struct FpsRowBook {
    cap: usize,
    rows: Vec<FpsRow>,
    mean_a: u64,
    mean_b: u64,
    chain: u32,
    sealed: bool,
}

impl FpsRowBook {
    pub fn new(cap: usize) -> Self {
        FpsRowBook {
            cap,
            rows: Vec::new(),
            mean_a: 0,
            mean_b: 0,
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 行切片只读访问。
    pub fn rows(&self) -> &[FpsRow] {
        &self.rows
    }

    pub fn mean_a(&self) -> u64 {
        self.mean_a
    }

    pub fn mean_b(&self) -> u64 {
        self.mean_b
    }

    /// 落一行：键段闭集校验 → 帧率带校验 → 唯一键 → 差值核。
    pub fn add_row(
        &mut self,
        scene: &str,
        res: &str,
        sync: &str,
        a: u64,
        b: u64,
    ) -> Result<usize, u32> {
        if self.sealed || self.rows.len() >= self.cap {
            return Err(FR_SEALED);
        }
        if scene.is_empty() || !res_ok(res) || !sync_ok(sync) {
            return Err(FR_NO_ROW);
        }
        if a == 0 || b == 0 {
            return Err(FR_FPS_ZERO);
        }
        if !fps_ok(a) || !fps_ok(b) {
            return Err(FR_FPS_RANGE);
        }
        let key = format!("{}|{}|{}|{}", METRIC_FPS, scene, res, sync);
        for r in &self.rows {
            if r.key == key {
                return Err(FR_DUP_KEY);
            }
        }
        let d = delta_bp(a, b);
        let no = self.rows.len();
        self.rows.push(FpsRow {
            no,
            key: key.clone(),
            a_x100: a,
            b_x100: b,
            delta_bp: d,
        });
        Ok(no)
    }

    /// 封账：空账拒绝；封账时定均值与键序指纹链。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FR_SEALED);
        }
        if self.rows.is_empty() {
            return Err(FR_NO_ROW);
        }
        let n = self.rows.len() as u64;
        let sa: u64 = self.rows.iter().map(|r| r.a_x100).sum();
        let sb: u64 = self.rows.iter().map(|r| r.b_x100).sum();
        self.mean_a = sa / n;
        self.mean_b = sb / n;
        self.chain = chain_sorted(&self.rows);
        self.sealed = true;
        Ok(())
    }

    /// 按键查询行三元组。
    pub fn query(&self, key: &str) -> Result<(u64, u64, i64), u32> {
        for r in &self.rows {
            if r.key == key {
                return Ok((r.a_x100, r.b_x100, r.delta_bp));
            }
        }
        Err(FR_NO_ROW)
    }

    /// 统计对账：均值复算 + 键序指纹复算，双不齐即漂移。
    pub fn verify_stats(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(FR_STAT_MISS);
        }
        let n = self.rows.len() as u64;
        let sa: u64 = self.rows.iter().map(|r| r.a_x100).sum();
        let sb: u64 = self.rows.iter().map(|r| r.b_x100).sum();
        if sa / n != self.mean_a || sb / n != self.mean_b {
            return Err(FR_STAT_MISS);
        }
        if self.chain != chain_sorted(&self.rows) {
            return Err(FR_STAT_MISS);
        }
        Ok(())
    }

    /// 渲染：每行 8 段（no|key(4)|a|b|delta）。
    pub fn export(&self) -> Result<Vec<String>, u32> {
        if !self.sealed {
            return Err(FR_NOT_SEALED);
        }
        let mut out = Vec::new();
        for r in &self.rows {
            out.push(format!(
                "{}|{}|{}|{}|{}",
                r.no, r.key, r.a_x100, r.b_x100, r.delta_bp
            ));
        }
        Ok(out)
    }

    /// 测试注入口：仅注入核使用。
    fn break_mean(&mut self) {
        self.mean_a += 1;
    }
}

/// 键序指纹链：按键名升序（插入排序、元组换位）独立重算。
pub fn chain_sorted(rows: &[FpsRow]) -> u32 {
    let mut idx: Vec<usize> = Vec::new();
    for i in 0..rows.len() {
        idx.push(i);
    }
    for i in 1..idx.len() {
        let mut j = i;
        while j > 0 && rows[idx[j]].key < rows[idx[j - 1]].key {
            let tmp = idx[j];
            idx[j] = idx[j - 1];
            idx[j - 1] = tmp;
            j -= 1;
        }
    }
    let mut h = CHAIN_SEED;
    for &i in &idx {
        let r = &rows[i];
        h = fold_str(h, &r.key);
        h = fold_u32(h, r.a_x100 as u32);
        h = fold_u32(h, r.b_x100 as u32);
        h = fold_i64(h, r.delta_bp);
    }
    h
}

/// 标准账：三行（scroll/drag/animation），均值 a=9000、b=8893。
fn fixture() -> FpsRowBook {
    let mut b = FpsRowBook::new(8);
    let _ = b.add_row("scroll", "1080p", "fixed", 12_000, 11_950);
    let _ = b.add_row("drag", "1440p", "vrr", 9_000, 9_030);
    let _ = b.add_row("animation", "2160p", "fixed", 6_000, 5_700);
    let _ = b.seal();
    b
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 零帧率必抓。
    {
        let mut b = FpsRowBook::new(8);
        caught += (b.add_row("scroll", "1080p", "fixed", 0, 11_950) == Err(FR_FPS_ZERO)) as usize;
    }
    // 2 帧率越界必抓。
    {
        let mut b = FpsRowBook::new(8);
        caught +=
            (b.add_row("scroll", "1080p", "fixed", 200_000, 11_950) == Err(FR_FPS_RANGE)) as usize;
    }
    // 3 重复键必抓。
    {
        let mut b = FpsRowBook::new(8);
        let _ = b.add_row("scroll", "1080p", "fixed", 12_000, 11_950);
        caught +=
            (b.add_row("scroll", "1080p", "fixed", 12_000, 11_950) == Err(FR_DUP_KEY)) as usize;
    }
    // 4 封账拒写必抓。
    {
        let mut b = fixture();
        caught += (b.add_row("web", "1080p", "fixed", 12_000, 11_950) == Err(FR_SEALED)) as usize;
    }
    // 5 未封账导出必抓。
    {
        let mut b = FpsRowBook::new(8);
        let _ = b.add_row("scroll", "1080p", "fixed", 12_000, 11_950);
        caught += matches!(b.export(), Err(FR_NOT_SEALED)) as usize;
    }
    // 6 缺行查询必抓。
    {
        let b = fixture();
        caught += (b.query("fps|web|1080p|fixed") == Err(FR_NO_ROW)) as usize;
    }
    // 7 统计漂移必抓。
    {
        let mut b = fixture();
        b.break_mean();
        caught += (b.verify_stats() == Err(FR_STAT_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56125 全量判据（15 项）。
pub fn run_unx56125_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56125");
    let b = fixture();

    // J1 行账全量。
    let ok = b.len() == 3;
    s.add("56125-J1-行账三条", ok, if ok { "" } else { "行数不符" });
    let ok = b.query("fps|scroll|1080p|fixed").is_ok()
        && b.query("fps|drag|1440p|vrr").is_ok()
        && b.query("fps|animation|2160p|fixed").is_ok();
    s.add("56125-J1-键格式", ok, if ok { "" } else { "键拼装不符" });
    let ok = METRIC_FPS == "fps" && RES_SET.len() == 3 && SYNC_SET.len() == 2;
    s.add("56125-J1-口径闭集", ok, if ok { "" } else { "闭集漂移" });

    // J2 口径语义。
    let (_, _, d1) = b.query("fps|scroll|1080p|fixed").unwrap_or((0, 0, 0));
    let ok = d1 == -41;
    s.add("56125-J2-差值行一", ok, if ok { "" } else { "scroll 差值非 -41" });
    let (_, _, d2) = b.query("fps|drag|1440p|vrr").unwrap_or((0, 0, 0));
    let ok = d2 == 33;
    s.add("56125-J2-差值行二", ok, if ok { "" } else { "drag 差值非 33" });
    let (_, _, d3) = b.query("fps|animation|2160p|fixed").unwrap_or((0, 0, 0));
    let ok = d3 == -500;
    s.add("56125-J2-差值行三", ok, if ok { "" } else { "animation 差值非 -500" });
    let ok = delta_bp(12_000, 11_950) == -41 && delta_bp(9_000, 9_030) == 33;
    s.add("56125-J2-差值核复算", ok, if ok { "" } else { "差值核漂移" });
    let ok = b.mean_a() == 9_000;
    s.add("56125-J2-均值A", ok, if ok { "" } else { "A 侧均值非 9000" });
    let ok = b.mean_b() == 8_893;
    s.add("56125-J2-均值B", ok, if ok { "" } else { "B 侧均值非 8893" });
    let ok = fps_ok(FPS_MIN) && fps_ok(FPS_MAX) && !fps_ok(99) && !fps_ok(FPS_MAX + 1);
    s.add("56125-J2-帧率带", ok, if ok { "" } else { "帧率带漂移" });

    // J3 行账审计链。
    let ok = b.sealed();
    s.add("56125-J3-封账门", ok, if ok { "" } else { "未封账" });
    let ok = b.chain() == chain_sorted(b.rows()) && b.chain() != CHAIN_SEED;
    s.add("56125-J3-指纹链", ok, if ok { "" } else { "指纹链漂移" });
    let ok = b.verify_stats().is_ok();
    s.add("56125-J3-统计对账", ok, if ok { "" } else { "统计对账失败" });
    let mut ok = matches!(b.export(), Ok(ref v) if v.len() == 3);
    if let Ok(lines) = b.export() {
        for l in &lines {
            if l.split('|').count() != 8 {
                ok = false;
            }
        }
    }
    s.add("56125-J3-渲染完整", ok, if ok { "" } else { "渲染段数漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56125-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56125_all_checks_green() {
        let set = run_unx56125_checks();
        assert!(set.all_passed(), "UNX-F56125 红项: {}", set.red_items_text());
    }
}
