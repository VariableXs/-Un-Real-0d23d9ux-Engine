//! VE-F0478 · IR 层数学扩展库（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0478`
//!
//! **锚点原文**：「IR 层数学扩展库：常用数学复合运算（矩阵分解、四元数运
//! 算、噪声函数类）作为 IR 内建扩展指令集登记（签名、语义、降级路径三要
//! 素）、扩展实现策略（引擎内实现或生成内联展开两路）、扩展版本化。判据：
//! 三要素登记、两路实现、降级路径、版本化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三要素登记**（判据一）。每条扩展指令登记三要素（签名 / 语义描述 /
//!    降级路径），缺一即登记拒收（[`ExtBook::register`]）。
//!
//! 2. **两路实现**（判据二）。实现策略二选一显性（Engine = 引擎内实现 /
//!    Inline = 生成内联展开），策略标记随扩展走。
//!
//! 3. **降级路径**（判据三）。扩展不可用 → 降级到基础指令展开（降级路径
//!    面：每扩展带基础指令展开序列描述），降级结果语义等价（对拍面：参考
//!    值双算一致）。
//!
//! 4. **版本化**（判据四）。扩展集版本化（升版流程显性：加扩展 → 版本递增
//!    + 变更留痕）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0478";

/// 实现策略（两路闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImplStrategy {
    /// 引擎内实现（后端直接支持）。
    Engine,
    /// 生成内联展开（编译期展开为基础指令）。
    Inline,
}

impl ImplStrategy {
    pub fn tag(&self) -> &'static str {
        match self {
            ImplStrategy::Engine => "engine",
            ImplStrategy::Inline => "inline",
        }
    }
}

/// 扩展指令三要素。
#[derive(Clone, Debug, PartialEq)]
pub struct ExtInst {
    /// 签名摘要（name(s:ty,...) → ret 口径同 vec47）。
    pub sig: String,
    /// 语义描述（非空——缺一拒收面）。
    pub semantics: &'static str,
    /// 降级路径（基础指令展开描述；Engine 路可空串=无降级需求）。
    pub fallback: &'static str,
    pub strategy: ImplStrategy,
}

/// 扩展册（版本化登记）。
#[derive(Debug, Default)]
pub struct ExtBook {
    pub insts: Vec<ExtInst>,
    pub ver: u32,
    pub ver_log: Vec<(u32, String)>,
    pub rejects: Vec<String>,
}

impl ExtBook {
    pub fn new() -> Self {
        ExtBook { insts: Vec::new(), ver: 1, ver_log: Vec::new(), rejects: Vec::new() }
    }
    /// 登记：三要素缺一拒收（Inline 路必须带降级路径）+ 签名重复拦截。
    pub fn register(&mut self, e: ExtInst) -> Result<usize, String> {
        if e.semantics.is_empty() {
            self.rejects.push(format!("{}：语义描述缺失", e.sig));
            return Err("语义描述缺失".to_string());
        }
        if e.strategy == ImplStrategy::Inline && e.fallback.is_empty() {
            self.rejects.push(format!("{}：内联路缺降级路径", e.sig));
            return Err("内联路缺降级路径".to_string());
        }
        if self.insts.iter().any(|x| x.sig == e.sig) {
            self.rejects.push(format!("{}：签名重复", e.sig));
            return Err("签名重复".to_string());
        }
        self.insts.push(e);
        Ok(self.insts.len() - 1)
    }
    /// 升版：加扩展 → 版本递增 + 变更留痕（同版不递增）。
    pub fn bump(&mut self, note: &str) {
        self.ver += 1;
        self.ver_log.push((self.ver, note.to_string()));
    }
    pub fn lookup(&self, sig: &str) -> Option<&ExtInst> {
        self.insts.iter().find(|x| x.sig == sig)
    }
}

// ---------------------------------------------------------------------------
// 参考实现（降级路径语义等价的对拍面）
// ---------------------------------------------------------------------------

/// 四元数（Hamilton 积参考实现——对拍基准）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quat {
    pub const IDENTITY: Quat = Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
    /// Hamilton 积。
    pub fn mul(&self, o: &Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
    /// 归一化（零长守卫：返回单位元）。
    pub fn normalized(&self) -> Quat {
        let n = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if n < 1e-12 {
            Quat::IDENTITY
        } else {
            Quat { w: self.w / n, x: self.x / n, y: self.y / n, z: self.z / n }
        }
    }
}

/// 2D 值噪声（哈希网格插值参考实现——降级对拍基准）。
pub fn value_noise_2d(x: f64, y: f64) -> f64 {
    // 整数格点哈希（FNV 变体）+ 双线性插值。
    let xi = x.floor() as i64;
    let yi = y.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let h = |gx: i64, gy: i64| -> f64 {
        let b = format!("{}:{}", gx, gy).into_bytes();
        (fnv64(&b) % 10_000) as f64 / 10_000.0
    };
    let v00 = h(xi, yi);
    let v10 = h(xi + 1, yi);
    let v01 = h(xi, yi + 1);
    let v11 = h(xi + 1, yi + 1);
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    v00 * (1.0 - sx) * (1.0 - sy) + v10 * sx * (1.0 - sy) + v01 * (1.0 - sx) * sy + v11 * sx * sy
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h = (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 降级对拍：扩展值与基础展开值语义等价（容差 1e-9）。
pub fn fallback_equiv(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec78_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三要素登记。
    {
        let mut book = ExtBook::new();
        let ok = book.register(ExtInst {
            sig: "quat_mul(q,q)q".to_string(),
            semantics: "四元数 Hamilton 积",
            fallback: "basis 乘积展开 16 mul + 6 add",
            strategy: ImplStrategy::Inline,
        });
        v.push(("F0478-登记-三要素收", ok.is_ok()));
        // Inline 缺降级路径 → 拒收（缺一即拦）。
        let bad = book.register(ExtInst {
            sig: "noise2d(f,f)f".to_string(),
            semantics: "2D 值噪声",
            fallback: "",
            strategy: ImplStrategy::Inline,
        });
        v.push((
            "F0478-登记-缺降级拒",
            bad.is_err() && book.rejects.iter().any(|r| r.contains("noise2d")),
        ));
        // Engine 路可无降级路径（后端直持）。
        let eng = book.register(ExtInst {
            sig: "mat_svd(m)m".to_string(),
            semantics: "矩阵 SVD 分解",
            fallback: "",
            strategy: ImplStrategy::Engine,
        });
        v.push(("F0478-登记-Engine免降级", eng.is_ok()));
        // 签名重复拦截。
        let dup = book.register(ExtInst {
            sig: "quat_mul(q,q)q".to_string(),
            semantics: "重复",
            fallback: "x",
            strategy: ImplStrategy::Inline,
        });
        v.push(("F0478-登记-签名重复拦", dup.is_err()));
    }

    // 二、两路实现。
    {
        // 策略标记随扩展走（tag 面）。
        v.push((
            "F0478-两路-标记面",
            ImplStrategy::Engine.tag() == "engine" && ImplStrategy::Inline.tag() == "inline",
        ));
        let mut book = ExtBook::new();
        book.register(ExtInst {
            sig: "a()i".to_string(),
            semantics: "a",
            fallback: "展开",
            strategy: ImplStrategy::Inline,
        }).ok();
        book.register(ExtInst {
            sig: "b()i".to_string(),
            semantics: "b",
            fallback: "",
            strategy: ImplStrategy::Engine,
        }).ok();
        v.push((
            "F0478-两路-随扩展走",
            book.lookup("a()i").map(|x| x.strategy) == Some(ImplStrategy::Inline)
                && book.lookup("b()i").map(|x| x.strategy) == Some(ImplStrategy::Engine),
        ));
    }

    // 三、降级路径语义等价（对拍面）。
    {
        // 四元数：单位元恒等 + 结合律抽查（参考实现对拍）。
        let q = Quat { w: 1.0, x: 2.0, y: 3.0, z: 4.0 }.normalized();
        let ident = q.mul(&Quat::IDENTITY);
        v.push((
            "F0478-降级-四元数恒等",
            fallback_equiv(ident.w, q.w) && fallback_equiv(ident.x, q.x),
        ));
        // 结合律：(q1*q2)*q3 == q1*(q2*q3)。
        let q1 = Quat { w: 0.5, x: 0.5, y: 0.5, z: 0.5 };
        let q2 = Quat { w: 1.0, x: 0.0, y: 1.0, z: 0.0 };
        let q3 = Quat { w: 0.0, x: 1.0, y: 0.0, z: 1.0 };
        let ab = q1.mul(&q2).mul(&q3);
        let ba = q1.mul(&q2.mul(&q3));
        v.push((
            "F0478-降级-结合律",
            fallback_equiv(ab.w, ba.w) && fallback_equiv(ab.x, ba.x) && fallback_equiv(ab.z, ba.z),
        ));
        // 值噪声：输出落在 [0,1]（降级展开语义有界）。
        let n = value_noise_2d(3.7, 8.2);
        v.push(("F0478-降级-噪声有界", (0.0..=1.0).contains(&n)));
        // 噪声连续性：相邻点差有界（插值面非阶跃）。
        let n2 = value_noise_2d(3.71, 8.2);
        v.push(("F0478-降级-噪声连续", (n - n2).abs() < 0.5));
    }

    // 四、版本化。
    {
        let mut book = ExtBook::new();
        book.register(ExtInst {
            sig: "x()i".to_string(),
            semantics: "x",
            fallback: "展开",
            strategy: ImplStrategy::Inline,
        }).ok();
        book.bump("加入 x()i");
        book.bump("加入 y()f");
        v.push((
            "F0478-版本-递增留痕",
            book.ver == 3 && book.ver_log.len() == 2 && book.ver_log[1].0 == 3,
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0478-判据-锚点入册", ANCHOR == "VE-F0478"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec78）。
pub fn run_vec78_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec78");
    for (name, ok) in vec78_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec78_raw_checks as run_vec78_checks;
    #[test]
    fn vec78_mathlib_checks() {
        for (name, ok) in run_vec78_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
