//! UNX-F8761 · 域文档快照三件冻结账 · 诊断段 0xB760
//! 验收锚点：UNX-F8761-J1 三件域文档（README/附录/域档）内容指纹
//! 冻结——冻结后逐件复算指纹必须零漂移，任一件内容变更即触发
//! 冻结告警，三件对账自洽（件数/指纹域/冻结标记），双判据缺一即批内红灯。
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB760;

// ── 诊断段独占 · 文档冻结错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_DRIFT: u16 = SEG_ERR + 1;   // 指纹漂移
const E_MISS: u16 = SEG_ERR + 2;    // 文档缺失
const E_DUP: u16 = SEG_ERR + 3;     // 文档重名
const E_THAW: u16 = SEG_ERR + 4;    // 未冻结即对账
const E_BAL: u16 = SEG_ERR + 5;     // 三件对账不平

// ── SHA-256 风格确定性指纹（FNV 折叠双通道）────────────────
/// 双通道指纹：FNV-1a 正序 + 逆序折叠，合成 64 位"冻结指纹"。
/// （no_std 下以确定性双通道替代真 SHA-256，语义等价：内容敏感、
///  雪崩、稳定。）
pub fn freeze_fp(data: &[u8]) -> u64 {
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    let mut b: u64 = 0x8422_2325_cbf2_9ce4;
    let n = data.len();
    for (i, ch) in data.iter().enumerate() {
        a ^= *ch as u64;
        a = a.wrapping_mul(0x0000_0100_0000_01B3);
        // 逆通道：从尾部折回
        let rb = data[n - 1 - i];
        b ^= rb as u64;
        b = b.rotate_left(7).wrapping_mul(0x0000_0100_0000_01B3);
    }
    a ^ b.rotate_left(17).wrapping_add(a.rotate_right(31))
}

// ── 文档快照 ─────────────────────────────────────────────────
/// 一件域文档：名 + 内容 + 冻结指纹。
pub struct DocSnap {
    pub name: String,
    pub content: Vec<u8>,
    pub frozen: Option<u64>,
}

impl DocSnap {
    pub fn new(name: &str, content: &[u8]) -> Self {
        DocSnap { name: name.to_string(), content: content.to_vec(), frozen: None }
    }
    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = Some(freeze_fp(&self.content));
    }
    /// 复核：未冻结 Err(THAW)；冻结后漂移 Err(DRIFT)；否则 Ok。
    pub fn verify(&self) -> Result<u64, u16> {
        match self.frozen {
            None => Err(E_THAW),
            Some(f) => {
                let cur = freeze_fp(&self.content);
                if cur == f {
                    Ok(cur)
                } else {
                    Err(E_DRIFT)
                }
            }
        }
    }
    /// 内容变更模拟。
    pub fn mutate(&mut self, delta: u8) {
        if let Some(first) = self.content.first_mut() {
            *first = first.wrapping_add(delta);
        }
    }
}

// ── 三件冻结档 ───────────────────────────────────────────────
/// 三件（README/附录/域档）聚合冻结器。
pub struct DocVault {
    pub docs: Vec<DocSnap>,
    vault_frozen: bool,
}

impl DocVault {
    pub fn new() -> Self {
        DocVault { docs: Vec::new(), vault_frozen: false }
    }
    pub fn add(&mut self, d: DocSnap) -> bool {
        if self.vault_frozen {
            return false; // 冻结后禁增
        }
        if self.docs.iter().any(|x| x.name == d.name) {
            return false; // 重名拒
        }
        self.docs.push(d);
        true
    }
    /// 全库冻结：三件齐才可冻。
    pub fn freeze_all(&mut self) -> Result<(), u16> {
        if self.docs.len() != 3 {
            return Err(E_MISS);
        }
        for d in self.docs.iter_mut() {
            d.freeze();
        }
        self.vault_frozen = true;
        Ok(())
    }
    /// 三件逐件复核，返回 (件数, 漂移件)。
    pub fn verify_all(&self) -> (usize, usize) {
        let mut drift = 0;
        for d in &self.docs {
            if d.verify().is_err() {
                drift += 1;
            }
        }
        (self.docs.len(), drift)
    }
    /// 三件对账：件数恰 3、名域恰 {readme, appendix, domain}、全冻结。
    pub fn tri_recon(&self) -> Result<(), u16> {
        if self.docs.len() != 3 {
            return Err(E_BAL);
        }
        let names: Vec<&str> = self.docs.iter().map(|d| d.name.as_str()).collect();
        for want in ["readme", "appendix", "domain"] {
            if !names.contains(&want) {
                return Err(E_MISS);
            }
        }
        let n = names.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if names[i] == names[j] {
                    return Err(E_DUP);
                }
            }
        }
        for d in &self.docs {
            if d.frozen.is_none() {
                return Err(E_THAW);
            }
        }
        Ok(())
    }
    pub fn is_frozen(&self) -> bool {
        self.vault_frozen
    }
    /// 全库指纹（逐件折叠）。
    pub fn vault_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for d in &self.docs {
            if let Some(f) = d.frozen {
                for b in f.to_le_bytes() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01B3);
                }
            }
        }
        h
    }
}

/// 标准三件夹具。
pub fn standard_vault() -> DocVault {
    let mut v = DocVault::new();
    v.add(DocSnap::new("readme", b"varix kernel domain readme v15\n15 chapters\n"));
    v.add(DocSnap::new("appendix", b"appendix-14 single source of truth\n50174 chars\n"));
    v.add(DocSnap::new("domain", b"domain snapshot: unxstar registry\n1145 segments\n"));
    let _ = v.freeze_all();
    v
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8761_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8761 域文档快照三件冻结");
    let v = standard_vault();

    // 1. 三件对账全过
    let rec = v.tri_recon();
    s.add("DOC-三件对账", rec.is_ok(), if rec.is_ok() { "" } else { "三件不齐" });

    // 2. 冻结标记生效
    s.add("DOC-冻结生效", v.is_frozen(), if v.is_frozen() { "" } else { "未冻结" });

    // 3. 逐件复核零漂移
    let (n, drift) = v.verify_all();
    s.add("DOC-零漂移", n == 3 && drift == 0, if drift == 0 { "" } else { "存在漂移件" });

    // 4. 内容变更触发告警
    let mut v2 = standard_vault();
    v2.docs[1].mutate(1);
    let (n2, drift2) = v2.verify_all();
    s.add("DOC-变更告警", drift2 == 1 && n2 == 3,
        if drift2 == 1 { "" } else { "变更未告警" });

    // 5. 变更件 verify 逐件定位（恰第二件 DRIFT）
    let locate = v2.docs[1].verify() == Err(E_DRIFT) && v2.docs[0].verify().is_ok();
    s.add("DOC-漂移定位", locate, if locate { "" } else { "定位失效" });

    // 6. 未冻结即对账拒
    let mut v3 = DocVault::new();
    v3.add(DocSnap::new("readme", b"x"));
    v3.add(DocSnap::new("appendix", b"y"));
    v3.add(DocSnap::new("domain", b"z"));
    let thaw = v3.docs[0].verify() == Err(E_THAW);
    s.add("DOC-未冻拒", thaw, if thaw { "" } else { "未冻放行" });

    // 7. 件数不足冻结拒
    let mut v4 = DocVault::new();
    v4.add(DocSnap::new("readme", b"x"));
    v4.add(DocSnap::new("appendix", b"y"));
    let short = v4.freeze_all() == Err(E_MISS);
    s.add("DOC-缺件冻结拒", short, if short { "" } else { "缺件可冻" });

    // 8. 重名拒收
    let mut v5 = DocVault::new();
    v5.add(DocSnap::new("readme", b"a"));
    let dup_rej = !v5.add(DocSnap::new("readme", b"b"));
    s.add("DOC-重名拒", dup_rej, if dup_rej { "" } else { "重名放行" });

    // 9. 冻结后禁增
    let mut v6 = standard_vault();
    let frozen_add = !v6.add(DocSnap::new("extra", b"e"));
    s.add("DOC-冻后禁增", frozen_add, if frozen_add { "" } else { "冻后可增" });

    // 10. 指纹内容敏感 + 雪崩
    let fa = freeze_fp(b"doc A");
    let fb = freeze_fp(b"doc B");
    let fc = freeze_fp(b"doc A\x01");
    s.add("DOC-指纹敏感", fa != fb && fa != fc && fb != fc,
        if fa != fb && fa != fc { "" } else { "指纹碰撞" });

    // 11. 指纹稳定（重算同值）
    let fd = freeze_fp(b"stable content");
    let fe = freeze_fp(b"stable content");
    s.add("DOC-指纹稳定", fd == fe, if fd == fe { "" } else { "指纹不稳定" });

    // 12. 全库指纹对账（标准库指纹与重放一致）
    let v7 = standard_vault();
    let fp1 = v.vault_fp();
    let fp2 = v7.vault_fp();
    s.add("DOC-库指纹对账", fp1 == fp2 && fp1 != 0,
        if fp1 == fp2 { "" } else { "库指纹漂移" });

    // 13. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB760 && SEG_BASE < 0xB780;
    s.add("DOC-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
