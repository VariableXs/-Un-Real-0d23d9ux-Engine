const fs = require("fs");

// 1) 全景图：F01251~F01875 行尾追加 ✅（root + docs 同步）
for (const p of ["AURORA-10000-功能全景图.md", "docs/AURORA-10000-功能全景图.md"]) {
  const lines = fs.readFileSync(p, "utf8").split("\n");
  let n = 0;
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^- F(\d{5}) /);
    if (!m) continue;
    const id = parseInt(m[1], 10);
    if (id >= 1251 && id <= 1875 && !lines[i].includes("✅")) {
      lines[i] = lines[i].replace(/\s*$/, "") + " ✅";
      n++;
    }
  }
  fs.writeFileSync(p, lines.join("\n"));
  console.log(p, "marked:", n);
}

// 2) 分工完成图：docs/ 落后于 root，同步为 root 内容（四副本纪律）
fs.copyFileSync("AURORA-10000-AI分工完成图.md", "docs/AURORA-10000-AI分工完成图.md");
console.log("分工完成图 docs copy synced");
