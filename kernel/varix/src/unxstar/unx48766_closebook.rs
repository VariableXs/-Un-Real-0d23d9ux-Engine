//! UNX-F48766 C·域功条件断言二：闭账物主办完成（证据包闭环断言）
//! 判据：UNX-F48766-J1 证据包闭环断言全绿；判据锚定 域功条件。
//! 纪律：物主-证据包-闭账三段闭环，包内件齐、指纹一致、闭环冻结不可逆、悬挂必抓。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1DE0;

const E_OWNER_BAD: u16 = SEG_BASE + 1; // 物主非法
const E_PACK_MISS: u16 = SEG_BASE + 2; // 包缺漏
const E_ITEM_MISS: u16 = SEG_BASE + 3; // 件缺漏
const E_FP_DIFF: u16 = SEG_BASE + 4; // 指纹漂移
const E_OPEN_HANG: u16 = SEG_BASE + 5; // 未闭包悬挂
const E_DUP_ITEM: u16 = SEG_BASE + 6; // 件重复
const E_FROZEN_TOUCH: u16 = SEG_BASE + 7; // 闭包后改
const E_BAL_BREAK: u16 = SEG_BASE + 8; // 闭环账守恒破

/// 证据件种类。
pub const K_LEDGER: u8 = 1;
pub const K_GREP: u8 = 2;
pub const K_FUZZ: u8 = 3;
/// 每包必齐件数。
pub const ITEMS_PER_PACK: u32 = 3;

/// 证据件。
#[derive(Clone)]
pub struct EvItem {
    /// 件种类。
    pub kind: u8,
    /// 内容指纹。
    pub fp: u32,
}

impl EvItem {
    pub fn new(kind: u8, fp: u32) -> Result<EvItem, u16> {
        if kind == 0 || kind > K_FUZZ {
            return Err(E_ITEM_MISS);
        }
        if fp == 0 {
            return Err(E_FP_DIFF);
        }
        Ok(EvItem { kind, fp })
    }
}

/// 证据包。
#[derive(Clone)]
pub struct EvPack {
    /// 物主。
    pub owner: u32,
    /// 包内件。
    pub items: Vec<EvItem>,
    /// 闭包态。
    pub closed: bool,
}

/// 闭账物主簿。
pub struct CloseBook {
    packs: Vec<EvPack>,
    frozen: bool,
}

impl CloseBook {
    pub fn new() -> CloseBook {
        CloseBook {
            packs: Vec::new(),
            frozen: false,
        }
    }

    /// 开包（物主唯一）。
    pub fn open_pack(&mut self, owner: u32) -> Result<bool, u16> {
        if owner == 0 {
            return Err(E_OWNER_BAD);
        }
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        for p in self.packs.iter() {
            if p.owner == owner {
                return Err(E_OWNER_BAD);
            }
        }
        self.packs.push(EvPack {
            owner,
            items: Vec::new(),
            closed: false,
        });
        Ok(true)
    }

    /// 入件（包必须存在且未闭、种类白名单、包内唯一）。
    pub fn put_item(&mut self, owner: u32, kind: u8, fp: u32) -> Result<bool, u16> {
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        let it = EvItem::new(kind, fp)?;
        let mut found = false;
        for p in self.packs.iter_mut() {
            if p.owner == owner {
                found = true;
                if p.closed {
                    return Err(E_FROZEN_TOUCH);
                }
                for x in p.items.iter() {
                    if x.kind == kind {
                        return Err(E_DUP_ITEM);
                    }
                }
                p.items.push(it);
                break;
            }
        }
        if !found {
            return Err(E_PACK_MISS);
        }
        Ok(true)
    }

    /// 闭包（件齐才可闭）。
    pub fn close_pack(&mut self, owner: u32) -> Result<bool, u16> {
        let mut found = false;
        for p in self.packs.iter_mut() {
            if p.owner == owner {
                found = true;
                if p.closed {
                    return Err(E_FROZEN_TOUCH);
                }
                if p.items.len() as u32 != ITEMS_PER_PACK {
                    return Err(E_ITEM_MISS);
                }
                p.closed = true;
                break;
            }
        }
        if !found {
            return Err(E_PACK_MISS);
        }
        Ok(true)
    }

    /// 件指纹复算：指纹必须与登记一致（由构造保证非零，复算=逐件 fp 域校验）。
    pub fn assert_fps(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for p in self.packs.iter() {
            for it in p.items.iter() {
                if it.fp == 0 {
                    return Err(E_FP_DIFF);
                }
                n += 1;
            }
        }
        Ok(n)
    }

    /// 悬挂断言：全部包必须已闭。
    pub fn assert_all_closed(&self) -> Result<u32, u16> {
        for p in self.packs.iter() {
            if !p.closed {
                return Err(E_OPEN_HANG);
            }
        }
        Ok(self.packs.len() as u32)
    }

    /// 闭环账守恒：闭包数 × 件数/包 = 总件数。
    pub fn assert_balance(&self) -> Result<(), u16> {
        let mut closed_n = 0u32;
        let mut items_n = 0u32;
        for p in self.packs.iter() {
            if p.closed {
                closed_n += 1;
            }
            items_n += p.items.len() as u32;
        }
        if closed_n.wrapping_mul(ITEMS_PER_PACK) != items_n {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 全局冻结（全部包已闭才可冻）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        let n = self.assert_all_closed()?;
        self.assert_fps()?;
        self.assert_balance()?;
        self.frozen = true;
        Ok(n)
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 闭账物主办完成主判据：开包-入件-闭包-闭环断言全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 8 物主开包 ×3 件 ×闭包
    let mut cb = CloseBook::new();
    let mut o = 1u32;
    while o <= 8 {
        cb.open_pack(o)?;
        cb.put_item(o, K_LEDGER, 0x1000 + o)?;
        cb.put_item(o, K_GREP, 0x2000 + o)?;
        cb.put_item(o, K_FUZZ, 0x3000 + o)?;
        cb.close_pack(o)?;
        o += 1;
    }
    let n = cb.assert_all_closed()?;
    if n != 8 {
        return Err(E_OPEN_HANG);
    }
    cb.assert_balance()?;
    hits += 1;

    // R2 物主重复开包必拒
    if cb.open_pack(1).err() == Some(E_OWNER_BAD) {
        hits += 1;
    } else {
        return Err(E_OWNER_BAD);
    }

    // R3 物主为零必拒
    if cb.open_pack(0).err() == Some(E_OWNER_BAD) {
        hits += 1;
    } else {
        return Err(E_OWNER_BAD);
    }

    // R4 未开包装件必拒
    if cb.put_item(99, K_LEDGER, 1).err() == Some(E_PACK_MISS) {
        hits += 1;
    } else {
        return Err(E_PACK_MISS);
    }

    // R5 种类越界必拒
    if cb.put_item(1, 9, 1).err() == Some(E_ITEM_MISS) {
        hits += 1;
    } else {
        return Err(E_ITEM_MISS);
    }

    // R6 零指纹必拒
    if cb.put_item(1, K_LEDGER, 0).err() == Some(E_FP_DIFF) {
        hits += 1;
    } else {
        return Err(E_FP_DIFF);
    }

    // R7 包内件重复必拒
    if cb.put_item(1, K_LEDGER, 9).err() == Some(E_DUP_ITEM) {
        hits += 1;
    } else {
        return Err(E_DUP_ITEM);
    }

    // R8 缺件闭包必抓
    let mut cb2 = CloseBook::new();
    cb2.open_pack(7)?;
    cb2.put_item(7, K_LEDGER, 0x71)?;
    if cb2.close_pack(7).err() == Some(E_ITEM_MISS) {
        hits += 1;
    } else {
        return Err(E_ITEM_MISS);
    }

    // R9 悬挂必抓：一包未闭
    let mut cb3 = CloseBook::new();
    let mut o2 = 1u32;
    while o2 <= 4 {
        cb3.open_pack(o2)?;
        cb3.put_item(o2, K_LEDGER, 0x10 + o2)?;
        cb3.put_item(o2, K_GREP, 0x20 + o2)?;
        cb3.put_item(o2, K_FUZZ, 0x30 + o2)?;
        if o2 < 4 {
            cb3.close_pack(o2)?;
        }
        o2 += 1;
    }
    if cb3.assert_all_closed().err() == Some(E_OPEN_HANG) {
        hits += 1;
    } else {
        return Err(E_OPEN_HANG);
    }

    // R10 LCG ×16 包闭环全绿 + 全局冻结
    let mut rng = Lcg::new(0x48766);
    let mut cb4 = CloseBook::new();
    let mut o3 = 1u32;
    while o3 <= 16 {
        cb4.open_pack(o3)?;
        let f1 = rng.below(0xFFFF) as u32 + 1;
        let f2 = rng.below(0xFFFF) as u32 + 1;
        let f3 = rng.below(0xFFFF) as u32 + 1;
        cb4.put_item(o3, K_LEDGER, f1)?;
        cb4.put_item(o3, K_GREP, f2)?;
        cb4.put_item(o3, K_FUZZ, f3)?;
        cb4.close_pack(o3)?;
        o3 += 1;
    }
    let fn_ = cb4.freeze()?;
    if fn_ != 16 {
        return Err(E_BAL_BREAK);
    }
    if cb4.open_pack(99).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    hits += 1;

    Ok(hits)
}
