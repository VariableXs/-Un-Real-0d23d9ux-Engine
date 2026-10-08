//! CheckSet 自检框架（沿用 VARIX 内核同款范式：一域一文件、自检 ≥ 功能数）。

pub struct CheckSet {
    pub domain: String,
    pub items: Vec<(String, bool, String)>,
}

impl CheckSet {
    pub fn new(domain: &str) -> Self {
        CheckSet {
            domain: domain.into(),
            items: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, cond: bool, detail: &str) {
        self.items.push((name.into(), cond, detail.into()));
    }

    pub fn passed(&self) -> usize {
        self.items.iter().filter(|(_, c, _)| *c).count()
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    pub fn all_pass(&self) -> bool {
        self.items.iter().all(|(_, c, _)| *c)
    }

    pub fn render(&self) -> String {
        let mut s = format!("== {} ({}/{}）==\n", self.domain, self.passed(), self.total());
        for (name, ok, detail) in &self.items {
            s.push_str(if *ok { "  [OK] " } else { "  [FAIL] " });
            s.push_str(name);
            if !ok {
                s.push_str("  -- ");
                s.push_str(detail);
            }
            s.push('\n');
        }
        s
    }
}
