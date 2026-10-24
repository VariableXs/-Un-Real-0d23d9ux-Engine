//! UNX-F0230 · 外接盘/多盘引导冲突消解
//!
//! 锚点判据（施工书原文）：UNX-F0230-J1 多盘插入时引导介质消解
//! 规则（优先级/指纹锁定）生效，误选率 0。
//!
//! 设计：介质清单（内建 / 外接 USB / 网络盘，各带优先级 + 可引导
//! 指纹 + 拔插态）→ 消解规则（优先级序 → 同级内指纹锁定 → 在场
//! 过滤）→ 误选率红线：规则选出者必须同时满足「在场 + 可引导 +
//! 优先级最高」，任何一次违规则 0 容忍。
//!
//! 错误码段：0x2D60（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0230_VERSION: &str = "F0230-mediapick-v1";

/// 诊断码段基址（UNX-F0230 独占）。
pub const SEG_BASE: u16 = 0x2D60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 规则违例（选了不在场/不可引导/低优先）。
    MisPick = SEG_BASE + 1,
    /// 全部介质不可引导。
    NoBootable = SEG_BASE + 2,
    /// 指纹锁定到不存在的介质。
    FpLockDangling = SEG_BASE + 3,
    /// 介质重复登记。
    DupMedia = SEG_BASE + 4,
    /// 消解账不平。
    PickLedgerBroken = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 介质类别（优先级序：内建 > 外接 > 网络）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MediaClass {
    /// 内建盘（优先级最高）。
    Builtin,
    /// 外接 USB。
    External,
    /// 网络。
    Network,
}

/// 介质。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// 介质名。
    pub name: &'static str,
    /// 类别。
    pub class: MediaClass,
    /// 在场（未拔）。
    pub present: bool,
    /// 可引导（有合法 ESP）。
    pub bootable: bool,
    /// 指纹（内容哈希）。
    pub fp: u64,
}

/// 消解器（可持指纹锁定）。
#[derive(Debug, Default)]
pub struct MediaPicker {
    /// 介质清单。
    pub medias: Vec<Media>,
    /// 指纹锁定（Some = 只认该指纹）。
    fp_lock: Option<u64>,
}

impl MediaPicker {
    /// 登记介质（重复拒）。
    pub fn put(&mut self, m: Media) -> Result<(), u16> {
        if self.medias.iter().any(|x| x.name == m.name) {
            return Err(E::DupMedia.code());
        }
        self.medias.push(m);
        Ok(())
    }

    /// 设指纹锁定。
    pub fn lock_fp(&mut self, fp: u64) -> Result<(), u16> {
        if !self.medias.iter().any(|m| m.fp == fp) {
            return Err(E::FpLockDangling.code());
        }
        self.fp_lock = Some(fp);
        Ok(())
    }

    /// 消解：在场 + 可引导 + 指纹锁（若有）+ 类别最高者。
    pub fn pick(&self) -> Result<&Media, u16> {
        let mut best: Option<&Media> = None;
        for m in self.medias.iter() {
            if !m.present || !m.bootable {
                continue;
            }
            if let Some(lock) = self.fp_lock {
                if m.fp != lock {
                    continue;
                }
            }
            match best {
                Some(b) if b.class <= m.class => {}
                _ => best = Some(m),
            }
        }
        best.ok_or(E::NoBootable.code())
    }

    /// 误选率红线：picked 必须优于其他全部候选。
    /// （Builtin 枚举值最小 = 最高优先；存在更小值候选而未选它即违例。）
    pub fn pick_sanity(&self, picked: &Media) -> Result<(), u16> {
        for m in self.medias.iter() {
            if m.name == picked.name {
                continue;
            }
            if m.present && m.bootable && m.class < picked.class {
                return Err(E::MisPick.code());
            }
        }
        if !picked.present || !picked.bootable {
            return Err(E::MisPick.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：消解规则生效，误选率 0）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：内建在场可引导 + 外接在场可引导 + 网络在场不可引导。
    let mut p = MediaPicker::default();
    p.put(Media {
        name: "nvme0",
        class: MediaClass::Builtin,
        present: true,
        bootable: true,
        fp: 0x1111,
    })?;
    p.put(Media {
        name: "usb0",
        class: MediaClass::External,
        present: true,
        bootable: true,
        fp: 0x2222,
    })?;
    p.put(Media {
        name: "net0",
        class: MediaClass::Network,
        present: true,
        bootable: false,
        fp: 0x3333,
    })?;

    // 二、J1 主体：消解选内建（最高优先且在场可引导），误选 0。
    let picked = p.pick()?;
    if picked.name != "nvme0" {
        return Err(E::MisPick.code());
    }
    p.pick_sanity(picked)?;

    // 三、内建拔出 → 外接顶上（规则在缺位时递补）。
    p.medias[0].present = false;
    let picked2 = p.pick()?;
    if picked2.name != "usb0" {
        return Err(E::MisPick.code());
    }
    p.pick_sanity(picked2)?;

    // 四、指纹锁定：锁定外接指纹 → 即使内建在场也选外接。
    p.medias[0].present = true;
    p.lock_fp(0x2222)?;
    let picked3 = p.pick()?;
    if picked3.name != "usb0" {
        return Err(E::MisPick.code());
    }
    p.pick_sanity(picked3)?;
    // 锁定悬空指纹拒。
    if p.lock_fp(0xDEAD).is_ok() {
        return Err(E::FpLockDangling.code());
    }

    // 五、红线逐项：全不可引导 / 全拔出 / 违例检出。
    p.medias[1].bootable = false;
    if p.pick().is_ok() {
        return Err(E::NoBootable.code());
    }
    let mut q = MediaPicker::default();
    q.put(Media {
        name: "usb0",
        class: MediaClass::External,
        present: false,
        bootable: true,
        fp: 0x9999,
    })?;
    if q.pick().is_ok() {
        return Err(E::NoBootable.code());
    }
    // 违例注入：把低优介质标成 picked 校验红。
    let low = Media {
        name: "net0",
        class: MediaClass::Network,
        present: true,
        bootable: true,
        fp: 0x4444,
    };
    let mut r = MediaPicker::default();
    r.put(Media {
        name: "nvme0",
        class: MediaClass::Builtin,
        present: true,
        bootable: true,
        fp: 0x1111,
    })?;
    r.put(low)?;
    let bad_pick = match r.medias.last() {
        Some(m) => m,
        None => return Err(E::MisPick.code()),
    };
    if r.pick_sanity(bad_pick).is_ok() {
        return Err(E::MisPick.code());
    }

    // 六、判别式段位对拍。
    if E::MisPick.code() != SEG_BASE + 1 || E::PickLedgerBroken.code() != SEG_BASE + 5 {
        return Err(E::MisPick.code());
    }
    Ok(())
}
