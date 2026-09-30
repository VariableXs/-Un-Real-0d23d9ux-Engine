/**
 * 真实验收 · V 线运行时收集器（临时用例，验收后可删）。
 * 动态导入所有导出 checkF#### 的检查文件，逐族调用并求值，
 * 产出 { id, pass } 全量清单到 docs/acceptance/acceptance-raw/vline-ids.json。
 */
import { describe, it } from 'vitest';
import * as fs from 'node:fs';
import * as path from 'node:path';

const ROOT = process.cwd();

function listCheckFiles(dir: string): string[] {
  const out: string[] = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (e.name === '__tests__' || e.name === 'node_modules') continue;
      out.push(...listCheckFiles(p));
    } else if (/\.tsx?$/.test(e.name)) {
      out.push(p);
    }
  }
  return out;
}

describe('V 线运行时 ID 收集', () => {
  it('collect', async () => {
    const files = listCheckFiles(path.join(ROOT, 'src')).filter((f) => {
      const src = fs.readFileSync(f, 'utf8');
      return /export function check[A-Za-z]*\d+/.test(src);
    });
    const entries: Array<{ id: string; pass: boolean; file: string }> = [];
    const errors: string[] = [];
    for (const f of files) {
      const mod = (await import(f)) as Record<string, unknown>;
      const fns = Object.entries(mod)
        .filter(([k, v]) => /^check[A-Za-z]*\d+$/.test(k) && typeof v === 'function')
        .sort(([a], [b]) => Number(a.match(/\d+/)![0]) - Number(b.match(/\d+/)![0]));
      for (const [, fn] of fns) {
        try {
          const list = (fn as () => Array<{ id: string; check: () => boolean }>)();
          for (const e of list) {
            let pass = false;
            try {
              pass = !!e.check();
            } catch {
              pass = false;
            }
            entries.push({ id: e.id, pass, file: path.relative(ROOT, f) });
          }
        } catch (err) {
          errors.push(`${f}: ${(err as Error).message}`);
        }
      }
    }
    // 非功能产物（快照 JSON）按仓库约定归档到 _attic，不混入 docs/。
    const outDir = path.join(ROOT, '_attic', 'acceptance', 'acceptance-raw');
    fs.mkdirSync(outDir, { recursive: true });
    fs.writeFileSync(
      path.join(outDir, 'vline-ids.json'),
      JSON.stringify({ total: entries.length, failing: entries.filter((e) => !e.pass).length, errors, entries }, null, 1),
    );
    console.log(`[collect-vline] entries=${entries.length} failing=${entries.filter((e) => !e.pass).length} errors=${errors.length}`);
  }, 600000);
});
