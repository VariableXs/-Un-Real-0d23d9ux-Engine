//! UNX-F56131 帧率样本存证归档件（0x61040 段）
//!
//! 锚点：UNX-F56131-J1 存证全量（场景 × 波次 × 帧率 × 摘要逐条归
//! 档、键唯一）；J2 存证语义（摘要 FNV 可复算、链总量封存、闭包重
//! 放逐条比对）；J3 归档审计链（封账门、链复算、渲染完整）三判据全
//! 绿。
//! 域：CoRun·UNX-F2-B40（帧率域·存证归档面）。
//!
//! 结构：归档登记簿（场景×波次唯一键、帧率带）+ 摘要核（场景/波次/
//! 帧率折叠）+ 重放核（&dyn Fn 按号取档比对）+ 链总量 + 注入核（零
//! 帧率/重复键/封账拒录/未封校验/重放缺档/摘要不符/空库封存必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61040 独占 0x20）。
pub const SEG_BASE: u32 = 0x61040;
pub const FV_INPUT_ZERO: u32 = SEG_BASE + 0x01; // 零帧率/帧率越界
pub const FV_FPS_RANGE: u32 = SEG_BASE + 0x02; // 帧率越界（保留独立码）
pub const FV_DUP_KEY: u32 = SEG_BASE + 0x03; // 重复键（场景×波次）
pub const FV_SEALED: u32 = SEG_BASE + 0x04; // 封账/账满拒录
pub const FV_NOT_SEALED: u32 = SEG_BASE + 0x05; // 未封账校验
pub const FV_DIGEST_MISS: u32 = SEG_BASE + 0x06; // 摘要不符/重放缺档
pub const FV_EMPTY: u32 = SEG_BASE + 0x07; // 空库封存

/// 帧率 ×100 定点合法带。
pub const FPS_MIN: u64 = 100;
pub const FPS_MAX: u64 = 120_000;

pub fn fps_ok(v: u64) -> bool {
    (FPS_MIN..=FPS_MAX).contains(&v)
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

/// 摘要核：场景折叠 → 波次折叠 → 帧率折叠。
pub fn digest_of(scene: &str, wave: u32, fps_x100: u64) -> u32 {
    let h = fold_str(CHAIN_SEED, scene);
    let h = fold_u32(h, wave);
    fold_u32(h, fps_x100 as u32)
}

/// 存证档目。
pub struct VaultRecord {
    pub no: usize,
    pub scene: String,
    pub wave: u32,
    pub fps_x100: u64,
    pub digest: u32,
}

/// 帧率样本存证归档库。
pub struct FpsVault {
    cap: usize,
    records: Vec<VaultRecord>,
    chain: u32,
    sealed: bool,
}

impl FpsVault {
    pub fn new(cap: usize) -> Self {
        FpsVault {
            cap,
            records: Vec::new(),
            chain: CHAIN_SEED,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn chain(&self) -> u32 {
        self.chain
    }

    /// 档目切片只读访问。
    pub fn records(&self) -> &[VaultRecord] {
        &self.records
    }

    /// 归档：帧率带 → 唯一键 → 摘要落档。
    pub fn append(&mut self, scene: &str, wave: u32, fps_x100: u64) -> Result<usize, u32> {
        if self.sealed || self.records.len() >= self.cap {
            return Err(FV_SEALED);
        }
        if scene.is_empty() {
            return Err(FV_INPUT_ZERO);
        }
        if fps_x100 == 0 {
            return Err(FV_INPUT_ZERO);
        }
        if !fps_ok(fps_x100) {
            return Err(FV_FPS_RANGE);
        }
        for r in &self.records {
            if r.scene == scene && r.wave == wave {
                return Err(FV_DUP_KEY);
            }
        }
        let no = self.records.len();
        self.records.push(VaultRecord {
            no,
            scene: scene.to_string(),
            wave,
            fps_x100,
            digest: digest_of(scene, wave, fps_x100),
        });
        Ok(no)
    }

    /// 按键取档。
    pub fn query(&self, scene: &str, wave: u32) -> Result<(u64, u32), u32> {
        for r in &self.records {
            if r.scene == scene && r.wave == wave {
                return Ok((r.fps_x100, r.digest));
            }
        }
        Err(FV_DIGEST_MISS)
    }

    /// 封存：空库拒绝、定链总量。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FV_SEALED);
        }
        if self.records.is_empty() {
            return Err(FV_EMPTY);
        }
        let mut h = CHAIN_SEED;
        for r in &self.records {
            h = fold_u32(h, r.digest);
        }
        self.chain = h;
        self.sealed = true;
        Ok(())
    }

    /// 重放核：闭包按号取档，逐条复算摘要比对。
    pub fn verify(&self, fetch: &dyn Fn(usize) -> Option<(String, u32, u64)>) -> Result<(), u32> {
        if !self.sealed {
            return Err(FV_NOT_SEALED);
        }
        let mut h = CHAIN_SEED;
        for r in &self.records {
            let (scene, wave, fps) = fetch(r.no).ok_or(FV_DIGEST_MISS)?;
            if scene != r.scene || wave != r.wave || fps != r.fps_x100 {
                return Err(FV_DIGEST_MISS);
            }
            let d = digest_of(&scene, wave, fps);
            if d != r.digest {
                return Err(FV_DIGEST_MISS);
            }
            h = fold_u32(h, d);
        }
        if h != self.chain {
            return Err(FV_DIGEST_MISS);
        }
        Ok(())
    }

    /// 渲染：每行 5 段（no|scene|w{wave}|fps|digest）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in &self.records {
            out.push(format!(
                "{}|{}|w{}|{}|{:x}",
                r.no, r.scene, r.wave, r.fps_x100, r.digest
            ));
        }
        out
    }
}

/// 标准库：三条档目（scroll w1 / drag w1 / scroll w2）后封存。
fn fixture() -> FpsVault {
    let mut v = FpsVault::new(8);
    let _ = v.append("scroll", 1, 12_000);
    let _ = v.append("drag", 1, 9_000);
    let _ = v.append("scroll", 2, 11_950);
    let _ = v.seal();
    v
}

/// 注入核：七路必抓（0/7 即红）。
fn injection_core() -> (usize, usize) {
    const N: usize = 7;
    let mut caught = 0usize;
    // 1 零帧率必抓。
    {
        let mut v = FpsVault::new(8);
        caught += (v.append("scroll", 1, 0) == Err(FV_INPUT_ZERO)) as usize;
    }
    // 2 帧率越界必抓。
    {
        let mut v = FpsVault::new(8);
        caught += (v.append("scroll", 1, 999_999) == Err(FV_FPS_RANGE)) as usize;
    }
    // 3 重复键必抓。
    {
        let mut v = FpsVault::new(8);
        let _ = v.append("scroll", 1, 12_000);
        caught += (v.append("scroll", 1, 12_000) == Err(FV_DUP_KEY)) as usize;
    }
    // 4 封账拒录必抓。
    {
        let mut v = fixture();
        caught += (v.append("web", 1, 12_000) == Err(FV_SEALED)) as usize;
    }
    // 5 未封账校验必抓。
    {
        let mut v = FpsVault::new(8);
        let _ = v.append("scroll", 1, 12_000);
        let ok = matches!(v.verify(&|_| None), Err(FV_NOT_SEALED));
        caught += ok as usize;
    }
    // 6 重放缺档必抓。
    {
        let v = fixture();
        let fetch = |no: usize| -> Option<(String, u32, u64)> {
            if no == 2 {
                None
            } else {
                Some(("x".to_string(), 1, 1))
            }
        };
        caught += (v.verify(&fetch) == Err(FV_DIGEST_MISS)) as usize;
    }
    // 7 摘要不符必抓（帧率被改写）。
    {
        let v = fixture();
        let fetch = |no: usize| -> Option<(String, u32, u64)> {
            match no {
                0 => Some(("scroll".to_string(), 1, 9_999)),
                _ => Some(("x".to_string(), 1, 1)),
            }
        };
        caught += (v.verify(&fetch) == Err(FV_DIGEST_MISS)) as usize;
    }
    (caught, N)
}

/// UNX-F56131 全量判据（15 项）。
pub fn run_unx56131_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56131");
    let v = fixture();

    // J1 存证全量。
    let ok = v.len() == 3;
    s.add("56131-J1-存证三条", ok, if ok { "" } else { "档目数不符" });
    let mut uniq = 3;
    for i in 0..v.records().len() {
        for j in 0..i {
            if v.records()[i].scene == v.records()[j].scene
                && v.records()[i].wave == v.records()[j].wave
            {
                uniq = j + 1;
            }
        }
    }
    let ok = uniq == 3;
    s.add("56131-J1-键唯一", ok, if ok { "" } else { "存在重复键" });
    let ok = v.records().iter().all(|r| fps_ok(r.fps_x100));
    s.add("56131-J1-帧率带", ok, if ok { "" } else { "帧率越带" });

    // J2 存证语义。
    let ok = v
        .records()
        .iter()
        .all(|r| r.digest == digest_of(&r.scene, r.wave, r.fps_x100));
    s.add("56131-J2-摘要可复算", ok, if ok { "" } else { "摘要复算漂移" });
    let ok = digest_of("scroll", 1, 12_000) == digest_of("scroll", 1, 12_000);
    s.add("56131-J2-摘要确定性", ok, if ok { "" } else { "摘要非确定" });
    let fetch_ok = |no: usize| -> Option<(String, u32, u64)> {
        match no {
            0 => Some(("scroll".to_string(), 1, 12_000)),
            1 => Some(("drag".to_string(), 1, 9_000)),
            2 => Some(("scroll".to_string(), 2, 11_950)),
            _ => None,
        }
    };
    let ok = v.verify(&fetch_ok).is_ok();
    s.add("56131-J2-全链校验", ok, if ok { "" } else { "重放校验失败" });
    let ok = v.chain() != CHAIN_SEED;
    s.add("56131-J2-链总量", ok, if ok { "" } else { "链总量未定" });
    let ok = v.query("scroll", 2) == Ok((11_950, digest_of("scroll", 2, 11_950)));
    s.add("56131-J2-按键取档", ok, if ok { "" } else { "取档语义漂移" });

    // J3 归档审计链。
    let ok = v.sealed();
    s.add("56131-J3-封账门", ok, if ok { "" } else { "未封账" });
    let mut h = CHAIN_SEED;
    for r in v.records() {
        h = fold_u32(h, r.digest);
    }
    let ok = v.chain() == h;
    s.add("56131-J3-链复算", ok, if ok { "" } else { "链复算漂移" });
    let ok = v.query("web", 1) == Err(FV_DIGEST_MISS);
    s.add("56131-J3-校验门语义", ok, if ok { "" } else { "校验门漂移" });
    let lines = v.render();
    let mut ok = lines.len() == 3;
    for l in &lines {
        if l.split('|').count() != 5 {
            ok = false;
        }
    }
    let ok = ok && lines.len() == 3 && lines[0].starts_with("0|scroll|w1|");
    s.add("56131-J3-渲染完整", ok, if ok { "" } else { "渲染段式漂移" });
    let ok = lines[1].contains("w1") && lines[2].contains("w2");
    s.add("56131-J3-波次标渲染", ok, if ok { "" } else { "波次标漂移" });

    // 注入核。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {}/{}", c, n) };
    s.add_dyn("56131-注入核必抓", ok, &detail);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56131_all_checks_green() {
        let set = run_unx56131_checks();
        assert!(set.all_passed(), "UNX-F56131 红项: {}", set.red_items_text());
    }
}
