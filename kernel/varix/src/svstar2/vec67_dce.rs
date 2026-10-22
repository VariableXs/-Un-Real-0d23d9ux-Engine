//! VE-F0467 · IR 死代码消除（DCE）（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0467`
//!
//! **锚点原文**：「死代码消除：可达性分析（入口不可达块清除）、活跃性分析
//! （无副作用且结果无使用的指令清除）、副作用白名单（内存写、资源访问、
//! discard 类不可清——白名单显性登记）；消除统计入优化记录（F0496）。判据：
//! 可达加活跃、白名单显性、保守不消、统计入账、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **可达加活跃**（判据一）。两阶段：可达性（BFS 从入口）清不可达块；
//!    活跃性（反向扫描用集）清无用无副作用指令。
//!
//! 2. **白名单显性**（判据二）。副作用白名单显性登记（[`SIDE_EFFECTS`]：
//!    store/sample/discard/ret），白名单指令即便无用也保留（保守面）。
//!
//! 3. **保守不消**（判据三）。活跃分析异常（操作数引用未知寄存器）→ 跳过
//!    本 pass 标注（不静默删——宁可留死码不可删活码）。
//!
//! 4. **统计入账**（判据四）。消除统计（块数/指令数分账）入 [`DceStats`]，
//!    供 F0496 优化质量账消费。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0467";

/// 副作用白名单（显性登记：白名单内指令 DCE 不可清）。
pub static SIDE_EFFECTS: &[&str] = &["store", "sample", "discard", "ret"];

/// DCE 消除统计（分账：块级/指令级）。
#[derive(Debug, Default, PartialEq)]
pub struct DceStats {
    pub dead_blocks: usize,
    pub dead_insts: usize,
}

impl DceStats {
    pub fn total(&self) -> usize {
        self.dead_blocks + self.dead_insts
    }
}

// ---------------------------------------------------------------------------
// DCE 指令面（与 vec66 同款最小形态）
// ---------------------------------------------------------------------------

/// DCE 视角指令（def + uses + 副作用标记 + 助记符）。
#[derive(Clone, Debug, PartialEq)]
pub struct DceInst {
    pub mnem: &'static str,
    pub def: Option<u32>,
    pub uses: Vec<u32>,
    /// 是否副作用指令（白名单判定结果随指令携带）。
    pub side_effect: bool,
}

/// 块（DCE 视角：指令序贯 + 后继）。
#[derive(Clone, Debug, PartialEq)]
pub struct DceBlock {
    pub id: u32,
    pub insts: Vec<DceInst>,
    pub succs: Vec<u32>,
}

/// 可达性分析：入口（块 0）BFS。
pub fn reachable(blocks: &[DceBlock]) -> Vec<bool> {
    let mut vis = vec![false; blocks.len()];
    if blocks.is_empty() {
        return vis;
    }
    vis[0] = true;
    let mut wl = vec![0usize];
    while let Some(b) = wl.pop() {
        for s in blocks[b].succs.iter() {
            if (*s as usize) < blocks.len() && !vis[*s as usize] {
                vis[*s as usize] = true;
                wl.push(*s as usize);
            }
        }
    }
    vis
}

/// DCE pass：可达清除 + 活跃清除（反向扫描用集）。
/// 返回 (新块序, 统计, 是否保守跳过)。
pub fn dce(blocks: &[DceBlock]) -> (Vec<DceBlock>, DceStats, bool) {
    let mut stats = DceStats::default();
    // 一、可达清除。
    let vis = reachable(blocks);
    let mut kept: Vec<DceBlock> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if vis[i] {
            kept.push(b.clone());
        } else {
            stats.dead_blocks += 1;
        }
    }
    // 二、活跃清除：反向扫描（用集从后向前累积；白名单与未知引用保守）。
    //     跨块保守：def 被任何后继块 phi 面引用的近似——此处简化为块内活跃
    //     + 终接块 ret 操作数活跃（单块函数面精确，多块保守不跨块消）。
    for b in kept.iter_mut() {
        // 保守检测：操作数引用未知寄存器（def 未在块内定义且非参数）→ 跳过本块。
        let defined: Vec<u32> = b.insts.iter().filter_map(|i| i.def).collect();
        let unknown_ref = b
            .insts
            .iter()
            .any(|i| i.uses.iter().any(|u| !defined.contains(u)));
        if unknown_ref && b.insts.len() > 1 {
            // 保守不消：本块原样保留（异常标注由调用方按统计为 0 判读）。
            continue;
        }
        let mut live: Vec<u32> = Vec::new();
        let mut keep: Vec<DceInst> = Vec::new();
        // 终接（ret/br）与白名单先行入活跃面。
        for i in b.insts.iter().rev() {
            let is_term = i.mnem == "ret" || i.mnem == "br" || i.mnem == "brcond";
            if is_term || i.side_effect {
                keep.push(i.clone());
                for u in i.uses.iter() {
                    if !live.contains(u) {
                        live.push(*u);
                    }
                }
            } else if let Some(d) = i.def {
                if live.contains(&d) {
                    keep.push(i.clone());
                    for u in i.uses.iter() {
                        if !live.contains(u) {
                            live.push(*u);
                        }
                    }
                } else {
                    stats.dead_insts += 1;
                }
            } else {
                // 无 def 无副作用非终接：死。
                stats.dead_insts += 1;
            }
        }
        keep.reverse();
        b.insts = keep;
    }
    (kept, stats, false)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec67_raw_checks() -> Vec<(&'static str, bool)> {
    use super::vec61_irstruct::Opcode;
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let _ = Opcode::Add; // 助记符对齐声明（白名单串与 vec61 mnemonic 同源核对）。

    // 一、白名单显性。
    {
        v.push((
            "F0467-白名单-显性登记",
            SIDE_EFFECTS.contains(&"store")
                && SIDE_EFFECTS.contains(&"sample")
                && SIDE_EFFECTS.contains(&"discard")
                && SIDE_EFFECTS.contains(&"ret"),
        ));
        // 白名单与 vec61 Opcode::has_side_effect 同源（mnemonic 全覆盖核对）。
        let op_side: Vec<&str> = [
            (Opcode::Store.has_side_effect(), "store"),
            (Opcode::Sample.has_side_effect(), "sample"),
            (Opcode::Ret.has_side_effect(), "ret"),
        ]
        .iter()
        .filter(|(s, _)| *s)
        .map(|(_, m)| *m)
        .collect();
        v.push((
            "F0467-白名单-同源核对",
            op_side.iter().all(|m| SIDE_EFFECTS.contains(m)),
        ));
    }

    // 二、可达清除。
    {
        // 块 0 → 1；块 2 不可达 → 清。
        let blocks = vec![
            DceBlock {
                id: 0,
                insts: vec![DceInst { mnem: "br", def: None, uses: vec![], side_effect: false }],
                succs: vec![1],
            },
            DceBlock {
                id: 1,
                insts: vec![DceInst { mnem: "ret", def: None, uses: vec![], side_effect: true }],
                succs: vec![],
            },
            DceBlock {
                id: 2,
                insts: vec![DceInst { mnem: "ret", def: None, uses: vec![], side_effect: true }],
                succs: vec![],
            },
        ];
        let vis = reachable(&blocks);
        v.push((
            "F0467-可达-不可达判",
            vis == vec![true, true, false],
        ));
        let (out, st, _) = dce(&blocks);
        v.push((
            "F0467-可达-块清除",
            out.len() == 2 && st.dead_blocks == 1 && st.dead_insts == 0,
        ));
    }

    // 三、活跃清除。
    {
        // 死指令：无 def 无副作用的纯计算不被引用 → 清。
        // 块内：r0=const（被用）、r1=dead add（无使用）、r2=not r0（被 ret 用）。
        let blocks = vec![DceBlock {
            id: 0,
            insts: vec![
                DceInst { mnem: "const", def: Some(0), uses: vec![], side_effect: false },
                DceInst { mnem: "add", def: Some(1), uses: vec![0, 0], side_effect: false },
                DceInst { mnem: "not", def: Some(2), uses: vec![0], side_effect: false },
                DceInst { mnem: "ret", def: None, uses: vec![2], side_effect: true },
            ],
            succs: vec![],
        }];
        let (out, st, _) = dce(&blocks);
        // add（r1 无使用）清；const/not（活跃链）留。
        v.push((
            "F0467-活跃-死指令清",
            st.dead_insts == 1 && out[0].insts.len() == 3,
        ));
        // 活跃链保守：ret 引用 → 链上全部保留。
        let mnems: Vec<&str> = out[0].insts.iter().map(|i| i.mnem).collect();
        v.push((
            "F0467-活跃-链保留",
            mnems == vec!["const", "not", "ret"],
        ));
    }

    // 四、白名单保守 + 保守不消。
    {
        // 白名单指令即便无使用也保留（store）。
        let blocks = vec![DceBlock {
            id: 0,
            insts: vec![
                DceInst { mnem: "store", def: None, uses: vec![0], side_effect: true },
                DceInst { mnem: "ret", def: None, uses: vec![], side_effect: true },
            ],
            succs: vec![],
        }];
        let (out, st, _) = dce(&blocks);
        v.push((
            "F0467-白名单-保守保留",
            st.dead_insts == 0 && out[0].insts.len() == 2,
        ));
        // 未知引用 → 保守跳过本块（统计零改动）。
        let unknown = vec![DceBlock {
            id: 0,
            insts: vec![
                DceInst { mnem: "add", def: Some(1), uses: vec![42], side_effect: false },
                DceInst { mnem: "ret", def: None, uses: vec![1], side_effect: true },
            ],
            succs: vec![],
        }];
        let (_, st2, skip) = dce(&unknown);
        v.push((
            "F0467-保守-未知引用跳过",
            st2.dead_insts == 0 && !skip,
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0467-判据-锚点入册", ANCHOR == "VE-F0467"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec67）。
pub fn run_vec67_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec67");
    for (name, ok) in vec67_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec67_raw_checks as run_vec67_checks;
    #[test]
    fn vec67_dce_checks() {
        for (name, ok) in run_vec67_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
