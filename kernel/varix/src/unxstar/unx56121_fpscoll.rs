//! UNX-F56121 合成器帧账对接采集器（族旗舰）（0x60F00 段）
//!
//! 锚点：UNX-F56121-J1 帧账采集全量（采集批逐批落账：批键 × 均值帧率
//! × 样本帧数三元组）；J2 帧账对接语义（帧率值域物理界、帧样本量守闸、
//! 对接口径与帧账同源——逐项可验）；J3 采集审计链（批指纹链、封账只
//! 读、均值闭式）三判据全绿。
//! 域：CoRun·UNX-F2-B40（帧率域·族旗舰采集面）。
//!
//! 结构：帧率采集批登记册（批键 → 均值帧率 × 样本帧数逐批落账）+ 值域
//! 闸（帧率万分比物理界）+ 样本量守闸（≥30 帧）+ 场景闭集（滚动/拖拽/
//! 动画三型）+ 指纹链 + 注入核（空批键/帧率越界/样本不足/未知场景/重
//! 复批/封账改写/空账查询必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60F00 独占 0x20）。
pub const SEG_BASE: u32 = 0x60F00;
pub const FC_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 批键为空
pub const FC_FPS_RANGE: u32 = SEG_BASE + 0x02; // 帧率越物理界
pub const FC_SAMPLE_SHORT: u32 = SEG_BASE + 0x03; // 帧样本量不足
pub const FC_SCENE_UNK: u32 = SEG_BASE + 0x04; // 未知场景（三闭集外）
pub const FC_DUP: u32 = SEG_BASE + 0x05; // 重复批键
pub const FC_SEALED: u32 = SEG_BASE + 0x06; // 采集账已封账拒批
pub const FC_EMPTY: u32 = SEG_BASE + 0x07; // 空账查询

/// 场景闭集。
pub const SC_ROLL: usize = 0;
pub const SC_DRAG: usize = 1;
pub const SC_ANIM: usize = 2;
pub const SC_COUNT: usize = 3;
pub const SC_NAMES: [&str; SC_COUNT] = ["scroll", "drag", "animation"];

/// 帧样本量纪律。
pub const FRAME_MIN: u32 = 30;

/// 帧率物理界（帧率 ×100 定点，即 0.01 fps 粒度）。
pub const FPS_MIN_X100: u32 = 100; // 1.00 fps
pub const FPS_MAX_X100: u32 = 100_000; // 1000.00 fps

/// 场景闭集核。
pub fn scene_ok(scene: usize) -> bool {
    scene < SC_COUNT
}

/// 帧率值域核。
pub fn fps_ok(fps_x100: u32) -> bool {
    fps_x100 >= FPS_MIN_X100 && fps_x100 <= FPS_MAX_X100
}

/// 批指纹：FNV-1a 32 位。
pub fn batch_fingerprint(key: &str, fps_x100: u32, frames: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, fps_x100, frames);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 采集批：批键 × 场景 × 均值帧率 × 样本帧数 × 指纹 × 链。
#[derive(Clone)]
pub struct FcBatch {
    pub key: String,
    pub scene: usize,
    pub fps_x100: u32,
    pub frames: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 采集总账。
pub struct FcBook {
    pub batches: Vec<FcBatch>,
    pub sealed: bool,
}

impl FcBook {
    pub fn new() -> Self {
        FcBook { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 采集落账：空键拒 + 场景闭集 + 帧率值域 + 样本闸 + 重复拒五闸。
    pub fn collect(&mut self, key: &str, scene: usize, fps_x100: u32, frames: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        if key.is_empty() {
            return Err(FC_KEY_EMPTY);
        }
        if !scene_ok(scene) {
            return Err(FC_SCENE_UNK);
        }
        if !fps_ok(fps_x100) {
            return Err(FC_FPS_RANGE);
        }
        if frames < FRAME_MIN {
            return Err(FC_SAMPLE_SHORT);
        }
        if self.find(key).is_some() {
            return Err(FC_DUP);
        }
        let fp = batch_fingerprint(key, fps_x100, frames);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(FcBatch {
            key: key.to_string(),
            scene,
            fps_x100,
            frames,
            fp,
            chain,
        });
        Ok(())
    }

    /// 查询批结果（未采集显性拒绝）。
    pub fn result_of(&self, key: &str) -> Result<(usize, u32, u32), u32> {
        match self.find(key) {
            Some(i) => {
                let b = &self.batches[i];
                Ok((b.scene, b.fps_x100, b.frames))
            }
            None => Err(FC_EMPTY),
        }
    }

    /// J1：采集账全量（逐批合法）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| {
                !b.key.is_empty() && scene_ok(b.scene) && fps_ok(b.fps_x100) && b.frames >= FRAME_MIN
            })
    }

    /// J3：帧率均值闭式（x100 定点均值）。
    pub fn mean_fps(&self) -> Result<u32, u32> {
        if self.batches.is_empty() {
            return Err(FC_EMPTY);
        }
        let mut sum: u64 = 0;
        for b in self.batches.iter() {
            sum += b.fps_x100 as u64;
        }
        Ok((sum / self.batches.len() as u64) as u32)
    }

    /// J3：批指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.batches.is_empty() {
            return false;
        }
        for i in 0..self.batches.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.batches[0].fp
            } else {
                self.batches[i - 1].fp ^ self.batches[i].fp
            };
            if self.batches[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.batches.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        }
    }
}

/// 标准夹具：3 批采集（三场景各一批）。
pub fn standard_book() -> FcBook {
    let mut b = FcBook::new();
    let plan: [(&str, usize, u32, u32); 3] =
        [("fps|scroll|wave1", SC_ROLL, 11_940, 600), ("fps|drag|wave1", SC_DRAG, 11_820, 600), ("fps|anim|wave1", SC_ANIM, 11_760, 600)];
    for p in plan.iter() {
        if b.collect(p.0, p.1, p.2, p.3).is_err() {
            panic!("standard fixture collect must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空批键/未知场景/帧率越界/样本不足/重复批/封账改写/空账查询）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空批键
    let mut b = FcBook::new();
    if b.collect("", SC_ROLL, 11_940, 600) == Err(FC_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 未知场景
    if b.collect("k", 9, 11_940, 600) == Err(FC_SCENE_UNK) {
        caught += 1;
    }
    // 3) 帧率越界
    if b.collect("k", SC_ROLL, FPS_MAX_X100 + 1, 600) == Err(FC_FPS_RANGE) {
        caught += 1;
    }
    // 4) 样本不足
    if b.collect("k", SC_ROLL, 11_940, 29) == Err(FC_SAMPLE_SHORT) {
        caught += 1;
    }
    // 5) 重复批
    if b.collect("k2", SC_ROLL, 11_940, 600).is_ok()
        && b.collect("k2", SC_ROLL, 12_000, 600) == Err(FC_DUP)
    {
        caught += 1;
    }
    // 6) 空账查询
    if FcBook::new().result_of("k").err() == Some(FC_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.collect("late", SC_ROLL, 11_940, 600) == Err(FC_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 采集账渲染导出。
pub fn render_book(b: &FcBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!(
            "{}|{}|{}|{}|{:08x}\n",
            x.key, SC_NAMES[x.scene], x.fps_x100, x.frames, x.fp
        ));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行五段 + 场景闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 5
            && (parts[1] == "scroll" || parts[1] == "drag" || parts[1] == "animation")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56121");
    // J1-1 帧账采集全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56121-J1-帧账采集全量", ok, if ok { "" } else { "采集批残缺" });
    // J1-2 场景闭集齐
    let ok = SC_NAMES.len() == SC_COUNT && SC_NAMES[2] == "animation";
    s.add("56121-J1-场景闭集齐", ok, if ok { "" } else { "场景漂移" });
    // J1-3 帧率值域边界
    let ok = fps_ok(FPS_MIN_X100) && fps_ok(FPS_MAX_X100) && !fps_ok(99) && !fps_ok(FPS_MAX_X100 + 1);
    s.add("56121-J1-帧率值域边界", ok, if ok { "" } else { "值域漂移" });
    // J2-1 批结果命中（三事件组齐）
    let ok = b.result_of("fps|scroll|wave1") == Ok((SC_ROLL, 11_940, 600));
    s.add("56121-J2-批结果命中", ok, if ok { "" } else { "结果漂移" });
    // J2-2 样本量守闸
    let ok = FRAME_MIN == 30;
    s.add("56121-J2-样本量守闸", ok, if ok { "" } else { "守闸漂移" });
    // J2-3 帧率均值闭式（(11940+11820+11760)/3 = 11840）
    let ok = b.mean_fps() == Ok(11_840);
    s.add("56121-J2-帧率均值闭式", ok, if ok { "" } else { "均值算术漂移" });
    // J2-4 空账查询显性拒绝
    let ok = b.result_of("ghost").err() == Some(FC_EMPTY);
    s.add("56121-J2-空账拒绝", ok, if ok { "" } else { "查询语义漂移" });
    // J3-1 批指纹链复核
    let ok = b.chain_ok();
    s.add("56121-J3-批指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56121-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（改帧率链尾变）
    let mut b3 = standard_book();
    b3.batches[0].fps_x100 += 1;
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56121-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒批
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.collect("late", SC_ROLL, 11_940, 600) == Err(FC_SEALED);
    s.add("56121-J3-封账拒批", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.starts_with("fps|scroll|wave1|scroll|11940|600|");
    s.add("56121-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56121-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56121_all_checks_green() {
        let set = run_unx56121_checks();
        assert!(set.all_passed(), "UNX-F56121 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56121_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
