import { en } from "../src/i18n/en.ts";
import { id } from "../src/i18n/id.ts";

const enKeys = Object.keys(en).sort();
const idKeys = Object.keys(id).sort();

console.log(`Checking i18n key parity between English (${enKeys.length} keys) and Indonesian (${idKeys.length} keys)...`);

const missingInId = enKeys.filter(k => !(k in id));
const extraInId = idKeys.filter(k => !(k in en));

if (missingInId.length > 0) {
  console.error("ERROR: Keys missing in Indonesian (id):", missingInId);
  process.exit(1);
}

if (extraInId.length > 0) {
  console.error("ERROR: Extra keys in Indonesian (id) not in English:", extraInId);
  process.exit(1);
}

// Check for empty values
for (const [k, v] of Object.entries(en)) {
  if (!v || v.trim().length === 0) {
    console.error(`ERROR: Key "${k}" in English is empty!`);
    process.exit(1);
  }
}

for (const [k, v] of Object.entries(id)) {
  if (!v || v.trim().length === 0) {
    console.error(`ERROR: Key "${k}" in Indonesian is empty!`);
    process.exit(1);
  }
}

console.log("✓ 100% key parity verified with non-empty translations for all keys.");
