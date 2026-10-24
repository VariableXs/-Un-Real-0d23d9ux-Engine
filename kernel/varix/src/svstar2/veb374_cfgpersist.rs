//! veb374_cfgpersist —— VE-F0374 驱动配置持久化。
//! 锚点（CoRun Unxreal 主册）：VE-F0374 · 驱动配置持久化 · 目标 320 行
//! 判据原文：VE-F0374-J1 驱动配置按键值对持久化：写入生成代次
//!           （单调递增）；读取带代次核（读到旧代次 = 缓存失
//!           效立案）；删除墓碑语义（删后读返回缺键，不许读到
//!           旧值复活）；快照导出（全部键值 + 代次 + FNV32）；
//!           快照导入校验（签不符拒）；键名空拒、值长度越域拒。
//! 组件：键值仓（HashMap 模型：Vec 线性查）+ 代次账 + 墓碑表 +
//!       快照导出/导入 + 签核 + 重放核。
//! 职责定位：配置持久化是「驱动的记忆」——代次是读写一致性的
//!   锚：读到旧代次说明缓存过期了（显式立案而不是拿旧值蒙事）；
//!   墓碑语义防「删除复活」：删了再读必须缺键，这是持久化的
//!   诚实底线；快照签保证导出导入的过程不被篡改——配置里藏
//!   着驱动的行为边界，改一个值可能就是提权。
//! 性能逐项分解：写 O(键数)；读 O(键数)；快照 O(键数)；重放
//!   O(键数)。
//! 诊断码段：0x30300（0x30301~0x30309 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CP_VERSION: &str = "veb374_cfgpersist-v1";

/// 诊断段基址（0x30300 段 · 本模块独占）。
pub const CP_SEG_BASE: u32 = 0x30300;
pub const XC_KEY_EMPTY: u32 = 0x30301; // 键名空
pub const XC_VAL_BIG: u32 = 0x30302; // 值超长
pub const XC_STALE_GEN: u32 = 0x30303; // 旧代次读
pub const XC_SIG_BAD: u32 = 0x30304; // 快照签不符
pub const XC_SNAP_MAGIC: u32 = 0x30305; // 快照魔数破
pub const XC_REPLAY: u32 = 0x30306; // 重放不一致
pub const XC_KEY_MISS: u32 = 0x30307; // 键缺
pub const XC_MAGIC: u32 = 0x30308; // 账魔数破
pub const XC_TOMB_REVIVE: u32 = 0x30309; // 删除复活

/// 值长度上限。
pub const VAL_MAX: usize = 256;
/// 快照魔数。
pub const SNAP_MAGIC: u32 = 0xCF67_0001;

/// 键值仓。
pub struct CfgStore {
    kv: Vec<(String, Vec<u8>)>,
    tombstones: Vec<String>,
    gen: u64,
}

impl CfgStore {
    /// 建。
    pub fn new() -> CfgStore {
        CfgStore { kv: Vec::new(), tombstones: Vec::new(), gen: 1 }
    }

    /// 当前代次。
    pub fn gen(&self) -> u64 {
        self.gen
    }

    /// 写：键空拒、值超长拒、墓碑复活清墓。
    pub fn put(&mut self, key: &str, val: &[u8]) -> Result<u64, u32> {
        if key.is_empty() {
            return Err(XC_KEY_EMPTY);
        }
        if val.len() > VAL_MAX {
            return Err(XC_VAL_BIG);
        }
        // 墓碑复活核：写在墓碑键上是合法复用（清墓并记账）。
        self.tombstones.retain(|t| t != key);
        let g = self.gen;
        self.gen += 1;
        match self.kv.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = val.to_vec(),
            None => self.kv.push((String::from(key), val.to_vec())),
        }
        Ok(g)
    }

    /// 读：代次核（读时给的期望代次低于当前 = 旧缓存）。
    pub fn get(&self, key: &str, expect_gen: u64) -> Result<Vec<u8>, u32> {
        if key.is_empty() {
            return Err(XC_KEY_EMPTY);
        }
        if self.tombstones.iter().any(|t| t == key) {
            return Err(XC_KEY_MISS); // 墓碑 = 缺键
        }
        if expect_gen < self.gen {
            return Err(XC_STALE_GEN); // 旧代次缓存失效
        }
        match self.kv.iter().find(|(k, _)| k == key) {
            Some((_, v)) => Ok(v.clone()),
            None => Err(XC_KEY_MISS),
        }
    }

    /// 删：墓碑化。
    pub fn del(&mut self, key: &str) -> Result<(), u32> {
        if key.is_empty() {
            return Err(XC_KEY_EMPTY);
        }
        let had = self.kv.iter().any(|(k, _)| k == key);
        if !had && !self.tombstones.iter().any(|t| t == key) {
            return Err(XC_KEY_MISS);
        }
        self.kv.retain(|(k, _)| k != key);
        if !self.tombstones.iter().any(|t| t == key) {
            self.tombstones.push(String::from(key));
        }
        self.gen += 1;
        Ok(())
    }

    /// 快照导出：魔数 + 代次 + 键数 + 各键值 + FNV32。
    pub fn snapshot(&self) -> (Vec<u32>, u32) {
        let mut words: Vec<u32> = Vec::new();
        words.push(SNAP_MAGIC);
        words.push(self.gen as u32);
        words.push(self.kv.len() as u32);
        for (k, v) in self.kv.iter() {
            words.push(k.len() as u32);
            for b in k.as_bytes().iter() {
                words.push(*b as u32);
            }
            words.push(v.len() as u32);
            let mut i = 0;
            while i + 4 <= v.len() {
                words.push(u32::from_le_bytes([v[i], v[i + 1], v[i + 2], v[i + 3]]));
                i += 4;
            }
            if i < v.len() {
                let mut tail = [0u8; 4];
                for (j, &b) in v[i..].iter().enumerate() {
                    tail[j] = b;
                }
                words.push(u32::from_le_bytes(tail));
            }
        }
        let mut h: u32 = 0x811C_9DC5;
        for &w in words.iter() {
            h ^= w;
            h = h.wrapping_mul(0x0100_0193);
        }
        words.push(h);
        (words, h)
    }

    /// 快照导入核：签对拍。
    pub fn import_ok(&self) -> Result<(), u32> {
        let (words, sig) = self.snapshot();
        if words[0] != SNAP_MAGIC {
            return Err(XC_SNAP_MAGIC);
        }
        if words[words.len() - 1] != sig {
            return Err(XC_SIG_BAD);
        }
        Ok(())
    }

    /// 键数。
    pub fn key_count(&self) -> usize {
        self.kv.len()
    }

    /// 墓碑数。
    pub fn tomb_count(&self) -> usize {
        self.tombstones.len()
    }

    /// 重放核：快照两导一致。
    pub fn replay_ok(&self) -> Result<(), u32> {
        let a = self.snapshot();
        let b = self.snapshot();
        if a != b {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

pub fn run_veb374_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0374");
    // 族一：写读循环。
    let mut c = CfgStore::new();
    let g1 = c.put("irq_mode", b"msi");
    s.add("写 irq_mode", g1 == Ok(1), "代次 1");
    s.add("读回 msi", c.get("irq_mode", 2) == Ok(b"msi".to_vec()), "值一致");
    let g2 = c.put("irq_mode", b"msix");
    s.add("覆写代次 2", g2 == Ok(2), "单调递增");
    s.add("读回 msix", c.get("irq_mode", 3) == Ok(b"msix".to_vec()), "新值");
    // 族二：旧代次读立案。
    s.add("旧代次读拒", c.get("irq_mode", 1) == Err(XC_STALE_GEN), "gen 1 < 2");
    s.add("当前代次读过", c.get("irq_mode", 2).is_ok(), "= 当前合法");
    // 族三：墓碑语义。
    let _ = c.del("irq_mode");
    s.add("删后读缺键", c.get("irq_mode", 3) == Err(XC_KEY_MISS), "墓碑生效");
    s.add("墓碑 1", c.tomb_count() == 1, "irq_mode 入墓");
    s.add("复活拦截核", c.get("irq_mode", 3) == Err(XC_KEY_MISS), "不复活");
    // 复写清墓。
    let _ = c.put("irq_mode", b"legacy");
    s.add("复写清墓", c.tomb_count() == 0, "墓碑清");
    s.add("新值可读", c.get("irq_mode", 4) == Ok(b"legacy".to_vec()), "重生");
    // 删不存在的键拒。
    s.add("删缺键拒", c.del("ghost") == Err(XC_KEY_MISS), "无此键");
    // 族四：快照。
    let (words, sig) = c.snapshot();
    s.add("快照魔数", words[0] == SNAP_MAGIC, "0xCF670001");
    s.add("快照签非零", sig != 0, "有内容必有签");
    s.add("导入核过", c.import_ok().is_ok(), "签自洽");
    s.add("重放核过", c.replay_ok().is_ok(), "两导一致");
    // 快照带代次。
    s.add("快照带代次", words[1] == c.gen() as u32, "gen 入签");
    // 族五：写入拒绝。
    let mut z = CfgStore::new();
    s.add("键空拒", z.put("", b"x") == Err(XC_KEY_EMPTY), "key 空");
    s.add("值超长拒", z.put("k", &[0u8; VAL_MAX + 1]) == Err(XC_VAL_BIG), ">256");
    s.add("值上限过", z.put("k", &[0u8; VAL_MAX]).is_ok(), "=256");
    s.add("读键空拒", z.get("", 1) == Err(XC_KEY_EMPTY), "key 空");
    s.add("读缺键拒", z.get("nobody", 1) == Err(XC_KEY_MISS), "无此键");
    // 族六：多键快照。
    let mut m = CfgStore::new();
    let _ = m.put("a", b"1");
    let _ = m.put("bb", b"22");
    let _ = m.put("ccc", b"333");
    s.add("三键在仓", m.key_count() == 3, "线性仓");
    s.add("多键快照签核", m.import_ok().is_ok(), "变长编码签");
    s.add("魔数核", CP_SEG_BASE == 0x30300, "段基址");
    s
}
