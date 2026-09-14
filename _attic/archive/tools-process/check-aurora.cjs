#!/usr/bin/env node
/**
 * AURORA-10000 ID 校验（实施总步骤图 §10）：
 * 校验全景图 F00001~F10000 唯一且连续。
 * 用法：node tools/check-aurora.cjs [全景图路径]（默认 docs/AURORA-10000-功能全景图.md）
 */
'use strict';
const fs = require('fs');
const path = require('path');
const file = process.argv[2] || path.join(__dirname, '..', 'docs', 'AURORA-10000-功能全景图.md');
const t = fs.readFileSync(file, 'utf8');
const seen = new Set();
const dup = [];
for (const m of t.matchAll(/^- F(\d{5}) /gm)) {
  const n = parseInt(m[1], 10);
  if (seen.has(n)) dup.push(n);
  else seen.add(n);
}
const miss = [];
for (let i = 1; i <= 10000; i++) if (!seen.has(i)) miss.push(i);
console.log(
  'unique=' + seen.size,
  'dups=' + (dup.join(',') || 'none'),
  'missing=' + (miss.length > 10 ? `${miss.length} items (first: ${miss.slice(0, 5).join(',')})` : miss.join(',') || 'none'),
);
if (seen.size !== 10000 || dup.length || miss.length) process.exit(1);
