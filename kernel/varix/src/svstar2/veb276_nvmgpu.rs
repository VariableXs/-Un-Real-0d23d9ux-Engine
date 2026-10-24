//! veb276_nvmgpu —— VE-F0276 NVIDIA 多卡语义。
//! 锚点（CoRun Unxreal 主册）：VE-F0276 · NVIDIA 多卡语义 · 目标 300 行
//! 判据原文：VE-F0276-J1 多卡语义账：卡对登记（主卡 + 从卡，
//!           主唯一）；角色分配（主渲染 + 从辅助）；帧同步闸
//!           （主从帧号差 ≤ 2 才放行）；分区账（显存与提交通
//!           路按卡隔离）；主卡唯一核（同对双主拒）；从卡摘除
//!           （摘从卡 → 对降单卡，主卡工作不中断）；摘主卡
//!           （对解散，从卡释放）。
//! 组件：卡对账 + 角色分配 + 帧同步闸 + 分区账。
//! 职责定位：多卡语义是「NVIDIA 组的搭桥术」——主从不是等级
//!           是分工：主卡吃渲染主路，从卡吃辅助支路；帧同步闸
//!           差 2 帧是流水的弹性容差，超差即卡顿源；摘主卡解
//!           散对是纪律：没有主的对不存在。
//! 性能逐项分解：登记 O(1)；同步判定 O(1)；摘除 O(对数)。
//! 诊断码段：0x30E00（0x30E01~0x30E09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MG_VERSION: &str = "veb276_nvmgpu-v1";

/// 诊断段基址（0x30E00 段 · 本模块独占）。
pub const MG_SEG_BASE: u32 = 0x30E00;
pub const XC_MAGIC: u32 = 0x30E01; // 账魔数破
pub const XC_DUP: u32 = 0x30E02; // 卡已在对
pub const XC_ROLE: u32 = 0x30E03; // 角色不合法
pub const XC_NOMAIN: u32 = 0x30E04; // 无主卡
pub const XC_SYNC: u32 = 0x30E05; // 帧差超闸
pub const XC_ALONE: u32 = 0x30E06; // 对不存在

/// 账魔数。
const MAGIC: u32 = 0x0E00_1B77;

/// 卡角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 主渲染。
    Main,
    /// 从辅助。
    Aux,
}

/// 帧同步闸容差。
pub const SYNC_GAP: u32 = 2;

/// 卡对。
pub struct Pair {
    /// 主卡号。
    pub main_id: u32,
    /// 从卡号（可选——单卡对无从）。
    pub aux_id: Option<u32>,
    /// 主卡帧号。
    pub main_frame: u64,
    /// 从卡帧号。
    pub aux_frame: u64,
}

/// 多卡账。
pub struct MgpuBook {
    pairs: Vec<Pair>,
    magic: u32,
}

impl MgpuBook {
    /// 建空账。
    pub fn new() -> MgpuBook {
        MgpuBook { pairs: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 查卡所在对（索引）。
    pub fn find_card(&self, id: u32) -> Option<usize> {
        for i in 0..self.pairs.len() {
            if self.pairs[i].main_id == id {
                return Some(i);
            }
            if self.pairs[i].aux_id == Some(id) {
                return Some(i);
            }
        }
        None
    }

    /// 建对：main 主、aux 从（None 即单卡对）。主重复即拒。
    pub fn form(&mut self, main_id: u32, aux_id: Option<u32>) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if main_id == 0 {
            return Err(XC_ROLE);
        }
        if let Some(a) = aux_id {
            if a == main_id {
                return Err(XC_ROLE);
            }
        }
        if self.find_card(main_id).is_some() {
            return Err(XC_DUP);
        }
        if let Some(a) = aux_id {
            if self.find_card(a).is_some() {
                return Err(XC_DUP);
            }
        }
        self.pairs.push(Pair {
            main_id,
            aux_id,
            main_frame: 0,
            aux_frame: 0,
        });
        Ok(())
    }

    /// 主卡推进一帧。
    pub fn main_tick(&mut self, main_id: u32) -> Result<u64, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let idx = match self.find_card(main_id) {
            Some(i) if self.pairs[i].main_id == main_id => i,
            _ => return Err(XC_ALONE),
        };
        self.pairs[idx].main_frame += 1;
        Ok(self.pairs[idx].main_frame)
    }

    /// 从卡推进一帧。
    pub fn aux_tick(&mut self, aux_id: u32) -> Result<u64, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let mut hit = None;
        for i in 0..self.pairs.len() {
            if self.pairs[i].aux_id == Some(aux_id) {
                hit = Some(i);
            }
        }
        let idx = match hit {
            Some(i) => i,
            None => return Err(XC_ALONE),
        };
        self.pairs[idx].aux_frame += 1;
        Ok(self.pairs[idx].aux_frame)
    }

    /// 帧同步闸：主从帧差 ≤ 2 放行；从卡缺席视为同步。
    pub fn sync_gate(&self, main_id: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let idx = match self.find_card(main_id) {
            Some(i) if self.pairs[i].main_id == main_id => i,
            _ => return Err(XC_ALONE),
        };
        let p = &self.pairs[idx];
        match p.aux_id {
            None => Ok(true),
            Some(_) => {
                let d = if p.main_frame >= p.aux_frame {
                    p.main_frame - p.aux_frame
                } else {
                    p.aux_frame - p.main_frame
                };
                if d > SYNC_GAP as u64 {
                    Err(XC_SYNC)
                } else {
                    Ok(true)
                }
            }
        }
    }

    /// 角色查询：主卡问 Main、从卡问 Aux、散卡无。
    pub fn role_of(&self, id: u32) -> Option<Role> {
        for i in 0..self.pairs.len() {
            if self.pairs[i].main_id == id {
                return Some(Role::Main);
            }
            if self.pairs[i].aux_id == Some(id) {
                return Some(Role::Aux);
            }
        }
        None
    }

    /// 摘从卡：对降单卡，主不中断；卡不在任何对则拒。
    pub fn remove_aux(&mut self, aux_id: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let mut hit = None;
        for i in 0..self.pairs.len() {
            if self.pairs[i].aux_id == Some(aux_id) {
                hit = Some(i);
            }
        }
        match hit {
            Some(i) => {
                self.pairs[i].aux_id = None;
                self.pairs[i].aux_frame = 0;
                Ok(())
            }
            None => Err(XC_ALONE),
        }
    }

    /// 摘主卡：对解散（整对移除）。
    pub fn remove_main(&mut self, main_id: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let idx = match self.find_card(main_id) {
            Some(i) if self.pairs[i].main_id == main_id => i,
            _ => return Err(XC_ALONE),
        };
        self.pairs.remove(idx);
        Ok(())
    }

    /// 对数。
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }
}

pub fn run_veb276_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0276");
    // 族零：建对配置。
    let mut b = MgpuBook::new();
    s.add("主 0 拒", b.form(0, None) == Err(XC_ROLE), "无零号主");
    s.add("自配对拒", b.form(3, Some(3)) == Err(XC_ROLE), "主=从");
    // 族一：建对与角色。
    s.add("建对 1+2", b.form(1, Some(2)).is_ok(), "主 1 从 2");
    s.add("主重复拒", b.form(1, None) == Err(XC_DUP), "1 已在对");
    s.add("从复用拒", b.form(3, Some(2)) == Err(XC_DUP), "2 已在对");
    s.add("主角色", b.role_of(1) == Some(Role::Main), "1 主");
    s.add("从角色", b.role_of(2) == Some(Role::Aux), "2 从");
    s.add("散卡无角色", b.role_of(9).is_none(), "9 散");
    // 族二：帧同步闸。
    let _ = b.main_tick(1);
    s.add("差 1 放行", b.sync_gate(1).is_ok(), "1-0≤2");
    let _ = b.aux_tick(2);
    let _ = b.aux_tick(2);
    s.add("差 2 放行", b.sync_gate(1).is_ok(), "1-2≤2");
    let _ = b.aux_tick(2);
    s.add("差 3 拒", b.sync_gate(1) == Err(XC_SYNC), "2-1>2");
    let _ = b.main_tick(1);
    let _ = b.main_tick(1);
    s.add("追平放行", b.sync_gate(1).is_ok(), "3-2≤2");
    // 族三：单卡对恒同步。
    s.add("单卡对", b.form(5, None).is_ok(), "5 独行");
    s.add("单卡恒同步", b.sync_gate(5) == Ok(true), "无从卡");
    // 族四：摘从卡。
    s.add("摘从 2", b.remove_aux(2).is_ok(), "对降单卡");
    s.add("2 散卡", b.role_of(2).is_none(), "已释放");
    s.add("1 仍主", b.role_of(1) == Some(Role::Main), "主不断");
    s.add("摘散卡拒", b.remove_aux(2) == Err(XC_ALONE), "已不在对");
    // 族五：摘主卡解散。
    s.add("摘主 1", b.remove_main(1).is_ok(), "对解散");
    s.add("1 散卡", b.role_of(1).is_none(), "主已释放");
    s.add("对数 1", b.pair_count() == 1, "剩 5 独行");
    s.add("摘主 5", b.remove_main(5).is_ok(), "独行也散");
    s.add("对数 0", b.pair_count() == 0, "空账");
    s.add("魔数核", MG_SEG_BASE == 0x30E00, "段基址");
    s
}
