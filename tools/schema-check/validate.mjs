// Validates every docs/architecture/examples/*.json against its schema (chosen by filename prefix)
// and checks that every docs/architecture/examples/invalid/*.json is REJECTED.
// Exit 0 and print "SCHEMAS OK" when everything holds.
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const schemaDir = join(root, "docs", "architecture", "schemas");
const exampleDir = join(root, "docs", "architecture", "examples");
const ajv = new Ajv2020({ strict: true, strictRequired: false, allErrors: true, allowUnionTypes: true });
addFormats(ajv);
const ids = {};
for (const f of readdirSync(schemaDir).filter((f) => f.endsWith(".schema.json"))) {
  const s = JSON.parse(readFileSync(join(schemaDir, f), "utf8"));
  ajv.addSchema(s);
  ids[f.replace(".schema.json", "")] = s.$id;
}
// Longest matching prefix wins (clipboard-source before clip).
const names = Object.keys(ids).sort((a, b) => b.length - a.length);
const pick = (file) => names.find((n) => file.startsWith(n + "."));
let failed = false;
const run = (dir, expectValid) => {
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".json"))) {
    const n = pick(f);
    if (!n) { console.log(`FAIL no schema for ${f}`); failed = true; continue; }
    const validate = ajv.getSchema(ids[n]);
    const ok = validate(JSON.parse(readFileSync(join(dir, f), "utf8")));
    if (ok !== expectValid) {
      failed = true;
      console.log(`FAIL ${expectValid ? "" : "invalid/"}${f} (${n}): expected ${expectValid ? "valid" : "rejected"}`);
      if (validate.errors) console.log(JSON.stringify(validate.errors, null, 1));
    } else console.log(`ok   ${expectValid ? "" : "invalid/"}${f} -> ${n} ${expectValid ? "valid" : "rejected"}`);
  }
};
run(exampleDir, true);
run(join(exampleDir, "invalid"), false);
if (failed) { console.log("SCHEMAS FAILED"); process.exit(1); }
console.log("SCHEMAS OK");
