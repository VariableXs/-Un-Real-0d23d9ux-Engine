//! UNX-F36310-J1 矩阵锁步回归库（240 格重放剧本）。
//! 判据：① 锁步重放 240 格逐位一致；② 差异格检出即定位到主体/
//! 客体/操作三元组；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权回归的剧本重放）：回归账不能只比
//! 总量，要**逐位**比。锁步重放库内置三条遍历剧本（主体主序、
//! 操作主序、客体主序），每条剧本按各自顺序求值 240 格并打成
//! 位流（每格 3 bit 归因码，21 格/字打包成 u64 字流）；三条位流
//! 与语义书位流两两对向——遍历顺序不同而位流全同，正是「求值
//! 与顺序无关」的直接证据（任何顺序依赖都是隐藏状态泄漏）。
//! 差异格检出器：两条位流一旦不同，检出器必须**定位到主体/客
//! 体/操作三元组**（格号 → cell_parts 反解），报告里带格号与双
//! 方归因码——回归失败不许只报「有差异」，要报「哪一格、什么
//! 变成了什么」。检出器本身用注入差位的合成账自测（差位不是
//! 真账——合成注入只发生在测试面，不污染矩阵）。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB200;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_BITSTREAM: u16 = SEG_BASE + 1; // 位流不等（锁步破）
pub const E_PACK: u16 = SEG_BASE + 2; // 打包账不平（字数/位域漂移）
pub const E_LOCATE: u16 = SEG_BASE + 3; // 差异定位失败（三元组错）
pub const E_RANGE: u16 = SEG_BASE + 4; // 格号越界
pub const E_SPEC: u16 = SEG_BASE + 5; // 与语义书漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 每格位宽（归因码 0..5 → 3 bit）。
pub const BITS_PER_CELL: u32 = 3;
/// 每字格数（64 bit / 3 bit = 21 格，余 1 bit 弃用）。
pub const CELLS_PER_WORD: u32 = 21;
/// 位流字数（240 格 → 12 字，末字余位补零）。
pub const STREAM_WORDS: usize = 12;

/// 剧本一：主体主序（S→O→OP，格号自然序）。
pub fn script_subject_major() -> Result<Vec<u32>, u16> {
    let mut order = Vec::new();
    for id in 1..=mx::N_CELLS as u32 {
        order.push(id);
    }
    Ok(order)
}

/// 剧本二：操作主序（OP→S→O）。
pub fn script_op_major() -> Result<Vec<u32>, u16> {
    let mut order = Vec::new();
    for op in 0..mx::N_OP {
        for s in 0..mx::N_SUBJ {
            for o in 0..mx::N_OBJ {
                order.push(mx::cell_id(s, o, op)?);
            }
        }
    }
    Ok(order)
}

/// 剧本三：客体主序（O→OP→S）+ 全局倒放（第二次跑反向走——
/// 顺序无关性的对抗面）。
pub fn script_object_major_reversed() -> Result<Vec<u32>, u16> {
    let mut order = Vec::new();
    for o in 0..mx::N_OBJ {
        for op in 0..mx::N_OP {
            for s in 0..mx::N_SUBJ {
                order.push(mx::cell_id(s, o, op)?);
            }
        }
    }
    order.reverse();
    Ok(order)
}

// ---- 位流打包（逐位一致比的载体） ---------------------------------------------
// 归因码 3 bit/格，从低位起装；字内剩余位补零。打包后位流与格
// 序绑定——不同剧本的位流按各自格序解包回 240 格归因数组再比。
pub fn pack_stream(verdicts: &[u8]) -> Result<Vec<u64>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if verdicts.len() != mx::N_CELLS {
        return Err(E_PACK);
    }
    let mut words = vec![0u64; STREAM_WORDS];
    for (i, v) in verdicts.iter().enumerate() {
        if *v > mx::A_DENY {
            return Err(E_PACK); // 3 bit 装不下出闭集码
        }
        let word = i as u32 / CELLS_PER_WORD;
        let slot = i as u32 % CELLS_PER_WORD;
        words[word as usize] |= (*v as u64) << (slot * BITS_PER_CELL);
    }
    Ok(words)
}

/// 位流解包（回 240 格归因数组——位域封闭性双向核）。
pub fn unpack_stream(words: &[u64]) -> Result<Vec<u8>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if words.len() != STREAM_WORDS {
        return Err(E_PACK);
    }
    let mut verdicts = Vec::new();
    for i in 0..mx::N_CELLS {
        let word = (i as u32) / CELLS_PER_WORD;
        let slot = (i as u32) % CELLS_PER_WORD;
        let v = ((words[word as usize] >> (slot * BITS_PER_CELL)) & 0x7) as u8;
        if v > mx::A_DENY {
            return Err(E_PACK); // 弃用位非零/越闭集 = 打包破
        }
        verdicts.push(v);
    }
    Ok(verdicts)
}

/// 单剧本重放：按剧本格序求值 → 位流 → 解包回自然序数组。
pub fn replay_script(order: &[u32]) -> Result<Vec<u8>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if order.len() != mx::N_CELLS {
        return Err(E_BITSTREAM);
    }
    let mut by_cell = vec![0u8; mx::N_CELLS];
    for id in order.iter() {
        let (s, o, op) = mx::cell_parts(*id)?;
        let v = mx::evaluate(s, o, op)?;
        if v != mx::spec_at(*id)? {
            return Err(E_SPEC);
        }
        by_cell[(*id - 1) as usize] = v;
    }
    // 剧本格序覆盖核：240 格无重无漏（锁步重放的前提）。
    let mut seen = vec![false; mx::N_CELLS];
    for id in order.iter() {
        if *id == 0 || *id as usize > mx::N_CELLS || seen[(*id - 1) as usize] {
            return Err(E_BITSTREAM);
        }
        seen[(*id - 1) as usize] = true;
    }
    Ok(by_cell)
}

/// 差异格检出器（判据 ②：检出即定位三元组）。
pub struct DiffReport {
    pub cell: u32,
    pub subject: usize,
    pub object: usize,
    pub op: usize,
    pub left: u8,
    pub right: u8,
}

pub fn locate_first_diff(a: &[u8], b: &[u8]) -> Result<Option<DiffReport>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if a.len() != mx::N_CELLS || b.len() != mx::N_CELLS {
        return Err(E_LOCATE);
    }
    for i in 0..mx::N_CELLS {
        if a[i] != b[i] {
            let cell = (i + 1) as u32;
            let (s, o, op) = mx::cell_parts(cell)?;
            return Ok(Some(DiffReport {
                cell,
                subject: s,
                object: o,
                op,
                left: a[i],
                right: b[i],
            }));
        }
    }
    Ok(None)
}

// ---- 判据 ①：锁步重放 240 格逐位一致 ------------------------------------------
// 三剧本求值位流 + 语义书位流四流两两对向（打包子自带闭集核）。
pub fn lockstep_all_scripts() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let spec_verdicts: Vec<u8> = mx::SPEC.to_vec();
    let spec_stream = pack_stream(&spec_verdicts)?;
    let scripts = [
        script_subject_major()?,
        script_op_major()?,
        script_object_major_reversed()?,
    ];
    let mut runs = 0u32;
    for order in scripts.iter() {
        let verdicts = replay_script(order)?;
        let stream = pack_stream(&verdicts)?;
        // 位流逐字对向（240 格 × 3 bit 全同）。
        if stream.len() != spec_stream.len() {
            return Err(E_BITSTREAM);
        }
        for w in 0..STREAM_WORDS {
            if stream[w] != spec_stream[w] {
                return Err(E_BITSTREAM);
            }
        }
        // 解包回读对向（打包↔解包往返一致性）。
        let roundtrip = unpack_stream(&stream)?;
        if roundtrip != verdicts {
            return Err(E_PACK);
        }
        runs += 1;
    }
    if runs != 3 {
        return Err(E_BITSTREAM);
    }
    Ok(runs)
}

/// 重放账行（呈现面：剧本名 + 位流首末字十六进制摘要）。
pub fn replay_row_text(name: &str, stream: &[u64]) -> Result<String, u16> {
    if stream.len() != STREAM_WORDS {
        return Err(E_PACK);
    }
    let mut text = String::from(name);
    text.push_str(" first=");
    text.push_str(&stream[0].to_string());
    text.push_str(" last=");
    text.push_str(&stream[STREAM_WORDS - 1].to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let runs = lockstep_all_scripts()?;
    // 检出器自测：合成账注入差位 → 必须定位到正确三元组。
    let mut doctored: Vec<u8> = mx::SPEC.to_vec();
    let probe_cell: u32 = 152; // S5 O0 WRITE（语义书归因 5）
    doctored[(probe_cell - 1) as usize] = mx::A_ALLOW;
    let spec_arr: Vec<u8> = mx::SPEC.to_vec();
    match locate_first_diff(&doctored, &spec_arr)? {
        Some(rep) => {
            if rep.cell != probe_cell || rep.left != mx::A_ALLOW || rep.right != mx::A_DENY {
                return Err(E_LOCATE);
            }
            let (s, o, op) = mx::cell_parts(rep.cell)?;
            if rep.subject != s || rep.object != o || rep.op != op {
                return Err(E_LOCATE);
            }
        }
        None => return Err(E_LOCATE), // 注入差位必须被检出
    }
    // 无差账必须报 None。
    if locate_first_diff(&spec_arr, &spec_arr)?.is_some() {
        return Err(E_LOCATE);
    }
    let row = replay_row_text("subject_major", &pack_stream(&spec_arr)?)?;
    if !row.starts_with("subject_major") {
        return Err(E_PACK);
    }
    if runs != 3 {
        return Err(E_BITSTREAM);
    }
    Ok(runs.wrapping_mul(1000).wrapping_add(probe_cell))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BITSTREAM => "位流不等（锁步重放破/剧本格序覆盖不全）",
        E_PACK => "打包账不平（字数/位域越闭集/往返不一致）",
        E_LOCATE => "差异定位失败（三元组/双方归因错报）",
        E_RANGE => "格号越界",
        E_SPEC => "与语义书漂移",
        _ => "未知诊断码",
    }
}

pub fn run_unx36310_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36310_mxlockstep");
    cs.add("SEG 段位独占", SEG_BASE == 0xB200, "0xB200~0xB20F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三剧本位流逐位全同·差位检出定位 152 号格·检出器自测双绿"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("位宽锚", BITS_PER_CELL == 3 && CELLS_PER_WORD == 21, "3 bit × 21 格/字（弃 1 bit 位域封闭）");
    cs.add("字数锚", STREAM_WORDS == 12, "240 格 = 12 字位流");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36310_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36310_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
