const re = /import\s+(?:type\s+)?([^'";]*?)\s*from\s*["']([^"']+)["']/g;
for (const s of ['import type { Foo } from "./types";', 'import React, { useState as st } from "react";']) {
  console.log(s, "=>", JSON.stringify([...s.matchAll(re)].map((m) => [m[1], m[2]])));
}
const rust = /^use\s+([\w:]+)(?:::\{([^}]*)\})?/gm;
const r = "use crate::widget::{Box, Label};\nuse std::collections::HashMap;";
console.log("rust =>", JSON.stringify([...r.matchAll(rust)].map((m) => [m[1], m[2]])));
