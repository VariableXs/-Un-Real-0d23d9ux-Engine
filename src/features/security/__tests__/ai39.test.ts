/**
 * UNREAL-X-15000 · AI-39 安全深水区与收官（领域10 · 族0381~0390 · X09501~X09750）门禁用例。
 * V/三方线五族 125 项（security/ai39Checks.ts）；C 线四族由 ca-core cargo test 覆盖。
 */
import { describe, expect, it } from 'vitest';
import { runAi39VChecks, checkF0382, checkF0385, checkF0386, checkF0387, checkF0390 } from '../ai39Checks';
import { BOUNTY_SEVERITY, KID_LEVELS, COMPLIANCE_DOMAINS, SEC_A11Y_CHANNELS, SEC_FINALE_GATES } from '../ai39Models';

describe('AI-39 安全深水区与收官 · V/三方线（族0382/0385/0386/0387/0390）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0382()).toHaveLength(25);
    expect(checkF0385()).toHaveLength(25);
    expect(checkF0386()).toHaveLength(25);
    expect(checkF0387()).toHaveLength(25);
    expect(checkF0390()).toHaveLength(25);
  });

  it('V 线聚合 125 项全绿', () => {
    const { entries, failed } = runAi39VChecks();
    expect(entries).toHaveLength(125);
    expect(failed).toEqual([]);
  });

  it('ID 连续覆盖 X09526~X09550 / X09601~X09750', () => {
    const { entries } = runAi39VChecks();
    const ids = entries.map((e) => e.id);
    expect(new Set(ids).size).toBe(125);
    for (let x = 9526; x <= 9550; x++) expect(ids).toContain(`X${String(x).padStart(5, '0')}`);
    for (let x = 9601; x <= 9675; x++) expect(ids).toContain(`X${String(x).padStart(5, '0')}`);
    for (let x = 9726; x <= 9750; x++) expect(ids).toContain(`X${String(x).padStart(5, '0')}`);
    expect(ids).not.toContain('X09551');
    expect(ids).not.toContain('X09676');
  });

  it('五族档位常量口径对齐', () => {
    expect(BOUNTY_SEVERITY).toHaveLength(4);
    expect(KID_LEVELS).toHaveLength(5);
    expect(COMPLIANCE_DOMAINS).toHaveLength(6);
    expect(SEC_A11Y_CHANNELS).toHaveLength(4);
    expect(SEC_FINALE_GATES).toHaveLength(5);
  });
});
