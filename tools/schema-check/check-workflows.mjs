// Parses every .github/workflows/*.yml with a real YAML parser (Task 408 review: a bad block scalar
// would make GitHub reject the whole file). Run via scripts/check.ps1 and scripts/check.sh.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parseAllDocuments } from 'yaml';

const dir = join(import.meta.dirname, '..', '..', '.github', 'workflows');
let bad = false;
for (const f of readdirSync(dir).filter((n) => /\.ya?ml$/.test(n))) {
  const docs = parseAllDocuments(readFileSync(join(dir, f), 'utf8'));
  const errs = docs.flatMap((d) => d.errors);
  if (docs.length !== 1) errs.push(new Error(`expected 1 YAML document, found ${docs.length}`));
  const js = docs[0]?.toJS();
  if (!js || typeof js.jobs !== 'object') errs.push(new Error('no jobs mapping'));
  if (errs.length) {
    bad = true;
    for (const e of errs) console.error(`CHECK FAILED: ${f}: ${e.message}`);
  } else console.log(`ok   ${f}`);
}
process.exit(bad ? 1 : 0);
