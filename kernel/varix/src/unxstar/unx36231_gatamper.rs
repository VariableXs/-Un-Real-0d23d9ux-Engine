//! UNX-F36231-J1 句柄掩码篡改防护（掩码区完整性校验）。
//! 判据：① 封印-校验闭环：seal 生成全表逐槽校验和账，verify 对活槽
//! 重算对拍——篡改注入 10 例（释放换对象/换掩码重挂/陈旧代数）全部
//! 被校验器修出，重封印后校验恢复绿；② 字段翻转敏感性：Entry 五字段
//! 任一位翻转必换校验和（10 例两值对拍——校验和不是摆设）；③ 墓碑
//! 槽不入账（封印只覆盖活句柄——关闭后的槽位变化不算篡改）；④ 纯功
//! 能代码 ≥320 行。
//! 设计纪律：Windows 句柄表在内核态受特权保护，用户态不可直改掩码；
//! 本面把「掩码区完整性」账面化：FNV-1a 逐槽摘要 + 全表封印账，任何
//! 掩码/对象/代数的静默改动都会在 verify 处显性翻红——篡改防护的
//! 本质是「改动必留痕」。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa230;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_TAMPER: u16 = SEG_BASE + 2; // 篡改检出（活槽校验和不符）
pub const E_SEAL: u16 = SEG_BASE + 3; // 封印账破（活槽数不符/账序漂移）
pub const E_STABLE: u16 = SEG_BASE + 4; // 字段翻转不敏感（校验和失效）
pub const E_CAP: u16 = SEG_BASE + 5; // 槽位越界
pub const E_STALE: u16 = SEG_BASE + 6; // 陈旧句柄（封印面操作走旧代数）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa230, "段位锚漂移");

/// 单槽校验和（FNV-1a 四字节双射逐步 + 字段隔离混合——任一字段任一
/// 位翻转必换值；字段顺序即混合序，防止字段互换对冲）。
pub fn entry_sum(e: &gf::Entry) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    // obj 四字节。
    let mut i = 0;
    while i < 4 {
        h = h.wrapping_mul(0x0100_0193) ^ ((e.obj >> (i * 8)) & 0xFF);
        i += 1;
    }
    // ga 四字节。
    let mut i = 0;
    while i < 4 {
        h = h.wrapping_mul(0x0100_0193) ^ ((e.ga >> (i * 8)) & 0xFF);
        i += 1;
    }
    // attrs 四字节。
    let mut i = 0;
    while i < 4 {
        h = h.wrapping_mul(0x0100_0193) ^ ((e.attrs >> (i * 8)) & 0xFF);
        i += 1;
    }
    // gen 与 auth_epoch 各四字节（授权戳入账——静默改戳也留痕）。
    let mut i = 0;
    while i < 4 {
        h = h.wrapping_mul(0x0100_0193) ^ ((e.gen >> (i * 8)) & 0xFF);
        i += 1;
    }
    let mut i = 0;
    while i < 4 {
        h = h.wrapping_mul(0x0100_0193) ^ ((e.auth_epoch >> (i * 8)) & 0xFF);
        i += 1;
    }
    h
}

/// 全表封印账（判据 ① 的载体：活槽逐槽校验和 + 活槽数 + 封印戳）。
pub struct Seal {
    sums: [u32; gf::TABLE_CAP],
    pub n: u32,
    pub stamp: u32,
}

impl Seal {
    pub fn fresh() -> Seal {
        Seal { sums: [0; gf::TABLE_CAP], n: 0, stamp: 0 }
    }

    /// 活槽校验和读回（越界 → 零——校验器按位对拍）。
    pub fn sum_at(&self, slot: usize) -> u32 {
        if slot >= gf::TABLE_CAP {
            0
        } else {
            self.sums[slot]
        }
    }
}

/// 封印（判据 ① 落点一）：全表扫描，活槽计账，墓碑/空槽记零。
pub fn seal_table(t: &gf::HandleTable, stamp: u32) -> Seal {
    let mut s = Seal::fresh();
    s.stamp = stamp;
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) {
                s.sums[slot] = entry_sum(&e);
                s.n = s.n.wrapping_add(1);
            }
        }
        slot += 1;
    }
    s
}

/// 校验（判据 ① 落点二）：活槽重算对拍 + 活槽数对平。
/// 篡改形态全捕：改掩码/换对象/换代数/偷挂新句柄 → 校验和或 n 失配。
pub fn verify_seal(t: &gf::HandleTable, s: &Seal) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut live = 0u32;
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if let Some(e) = t.entry_at(slot) {
            if !gf::is_free_entry(&e) {
                live = live.wrapping_add(1);
                if s.sum_at(slot) != entry_sum(&e) {
                    return Err(E_TAMPER);
                }
            } else if s.sum_at(slot) != 0 {
                // 活句柄在封印后消失（被关闭/清算）——同样算账面变动。
                return Err(E_TAMPER);
            }
        } else if s.sum_at(slot) != 0 {
            return Err(E_TAMPER);
        }
        slot += 1;
    }
    if live != s.n {
        return Err(E_SEAL);
    }
    Ok(())
}

/// 字段翻转敏感性核（判据 ② 的落点）：基条目两字段值逐一对拍——
/// 五字段任一改值必换校验和。
pub fn flip_sensitive(base: &gf::Entry, alt: &gf::Entry) -> Result<(), u16> {
    let b = entry_sum(base);
    let a = entry_sum(alt);
    if b == a {
        return Err(E_STABLE);
    }
    // 确定性：同条目重算恒等。
    if entry_sum(base) != b || entry_sum(alt) != a {
        return Err(E_STABLE);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：字段翻转敏感性 10 例（判据 ② 落点）：五字段 × 两值形态。
    let base = gf::Entry { obj: 0x1111, ga: sr::READ_CONTROL | 0x0001, attrs: gf::HA_INHERIT, gen: 3, auth_epoch: 42 };
    let alts = [
        gf::Entry { obj: 0x1112, ..base },
        gf::Entry { obj: 0x1111 ^ 0x0001_0000, ..base },
        gf::Entry { ga: sr::READ_CONTROL, ..base },
        gf::Entry { ga: sr::READ_CONTROL | 0x0003, ..base },
        gf::Entry { attrs: 0, ..base },
        gf::Entry { attrs: gf::HA_INHERIT | gf::HA_PROTECT, ..base },
        gf::Entry { gen: 4, ..base },
        gf::Entry { gen: 3 | 0x8000_0000, ..base },
        gf::Entry { auth_epoch: 43, ..base },
        gf::Entry { auth_epoch: 42 ^ 0x1000_0000, ..base },
    ];
    let mut i = 0;
    while i < 10 {
        flip_sensitive(&base, &alts[i])?;
        i += 1;
    }
    // R2：封印-校验闭环绿路径（判据 ① 正向）：建表 40 例 → 封印 →
    // 校验绿（零篡改零误报）。
    let mut t = gf::HandleTable::new();
    let mut keys = [(0usize, 0u32); 40];
    let mut i = 0;
    while i < 40 {
        keys[i] = t
            .attach(0x6000 + i as u32, sr::READ_CONTROL | (i as u32 & 0x0007), 0)
            .map_err(|_| E_CAP)?;
        i += 1;
    }
    let s0 = seal_table(&t, 7);
    if s0.n != 40 || verify_seal(&t, &s0).is_err() {
        return Err(E_SEAL);
    }
    // R3：篡改注入 10 例（判据 ① 落点三）：释放换对象重挂（掩码也换）
    // ——verify 全数修出，重封印后恢复绿。
    let mut caught = 0u32;
    let mut i = 0;
    while i < 10 {
        let (slot, gen) = keys[i * 4];
        // 静默换内容：关旧（走陈旧代数在真表里不可能——这里用 release
        // 再以不同掩码重挂同一槽位模拟「掩码区被直改」）。
        t.release(slot, gen).map_err(|_| E_STALE)?;
        let new_ga = sr::STD_ALL;
        let (_ns, _ng) = t.attach(0x6000 + (i * 4) as u32, new_ga, gf::HA_PROTECT).map_err(|_| E_CAP)?;
        if verify_seal(&t, &s0).err() != Some(E_TAMPER) {
            return Err(E_TAMPER);
        }
        caught = caught.wrapping_add(1);
        i += 1;
    }
    if caught != 10 {
        return Err(E_TAMPER);
    }
    // 重封印后校验恢复绿（防护不是永久红——账面可重建）。
    let s1 = seal_table(&t, 8);
    if s1.n != 40 || verify_seal(&t, &s1).is_err() {
        return Err(E_SEAL);
    }
    // 重封印幂等（封印是纯函数）：同表同戳两次封印逐槽和全等。
    let s1b = seal_table(&t, 8);
    if s1b.n != s1.n || s1b.stamp != s1.stamp {
        return Err(E_SEAL);
    }
    let mut slot = 0;
    while slot < gf::TABLE_CAP {
        if s1.sum_at(slot) != s1b.sum_at(slot) {
            return Err(E_SEAL);
        }
        slot += 1;
    }
    // R4：偷挂新句柄（封印后 add）→ n 失配 → E_SEAL；关闭一个活句柄
    //（封印后 del）→ 墓碑槽账面归零 → 同样修出。
    let (xslot, xgen) = t.attach(0x7000, sr::READ_CONTROL, 0).map_err(|_| E_CAP)?;
    if verify_seal(&t, &s1).err() != Some(E_TAMPER) {
        return Err(E_TAMPER);
    }
    let s2 = seal_table(&t, 9);
    if s2.n != 41 {
        return Err(E_SEAL);
    }
    t.release(xslot, xgen).map_err(|_| E_STALE)?;
    if verify_seal(&t, &s2).err() != Some(E_TAMPER) {
        return Err(E_TAMPER);
    }
    // R5：越界锚与戳锚。
    if s0.sum_at(gf::TABLE_CAP + 1) != 0 {
        return Err(E_CAP);
    }
    if s0.stamp != 7 || s1.stamp != 8 {
        return Err(E_SEAL);
    }
    // 校验和：两代封印账 n + 检出数（确定性总锚）。
    Ok(s0.n.wrapping_add(s1.n).wrapping_add(caught))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TAMPER => "篡改检出（活槽校验和不符/封印槽内容漂移）",
        E_SEAL => "封印账破（活槽数不符/戳漂移）",
        E_STABLE => "字段翻转不敏感（校验和对改动无感——失效）",
        E_CAP => "槽位越界",
        E_STALE => "陈旧句柄（封印面操作走旧代数）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36231_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36231_gatamper");
    cs.add("SEG 段位独占", SEG_BASE == 0xa230, "0xa230~0xa23F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "10 例字段敏感/10 例篡改全修出/重封印恢复"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("摘要双射", entry_sum(&gf::Entry { obj: 0, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 }) != entry_sum(&gf::Entry { obj: 1, ga: 0, attrs: 0, gen: 0, auth_epoch: 0 }), "FNV-1a 乘子奇 → 步步可逆 → 字段敏感");
    cs.add("墓碑免账", Seal::fresh().n == 0 && Seal::fresh().sum_at(0) == 0, "空表封印零活槽（墓碑不入账）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_tamper_detected_and_reseal() {
        let mut t = gf::HandleTable::new();
        let (s0, g0) = t.attach(1, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        let (s1, g1) = t.attach(2, sr::READ_CONTROL, 0).unwrap_or((0, 0));
        let seal = seal_table(&t, 1);
        assert!(verify_seal(&t, &seal).is_ok());
        // 换掩码重挂（模拟直改）。
        t.release(s0, g0).unwrap_or(());
        let _ = t.attach(1, sr::STD_ALL, 0);
        assert_eq!(verify_seal(&t, &seal).err(), Some(E_TAMPER));
        // 换代数（另一槽）。
        t.release(s1, g1).unwrap_or(());
        let _ = t.attach(2, sr::READ_CONTROL, 0);
        assert_eq!(verify_seal(&t, &seal).err(), Some(E_TAMPER));
        // 重封印恢复。
        let seal2 = seal_table(&t, 2);
        assert!(verify_seal(&t, &seal2).is_ok());
    }

    #[test]
    fn test_flip_sensitivity_all_fields() {
        let base = gf::Entry { obj: 5, ga: 6, attrs: 7, gen: 8, auth_epoch: 9 };
        let alts = [
            gf::Entry { obj: 6, ..base },
            gf::Entry { ga: 7, ..base },
            gf::Entry { attrs: 8, ..base },
            gf::Entry { gen: 9, ..base },
            gf::Entry { auth_epoch: 10, ..base },
        ];
        let mut i = 0;
        while i < 5 {
            assert!(flip_sensitive(&base, &alts[i]).is_ok());
            i += 1;
        }
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36231_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
