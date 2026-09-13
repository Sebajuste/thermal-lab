import { readFileSync, writeFileSync } from "node:fs";
import { read, SEMVER } from "./versions.mjs";

const LEVELS = ["major", "minor", "patch"];

const arg = process.argv[2];
if (!arg) {
  console.error("usage : npm run bump <version|major|minor|patch>");
  process.exit(2);
}

const found = read();
const current = found[0].version;
const parts = current.match(SEMVER);
if (!parts) throw new Error(`version courante illisible : ${current}`);

let next;
if (SEMVER.test(arg)) {
  next = arg;
} else if (LEVELS.includes(arg)) {
  const [major, minor, patch] = parts.slice(1).map(Number);
  next = {
    major: `${major + 1}.0.0`,
    minor: `${major}.${minor + 1}.0`,
    patch: `${major}.${minor}.${patch + 1}`,
  }[arg];
} else {
  console.error(`argument incompris : ${arg}`);
  process.exit(2);
}

// L'updater compare les numéros : une version qui n'est pas strictement supérieure est
// invisible pour les installations existantes, et la release passerait inapercue.
if (next === current) {
  console.error(`la version est déjà ${current}`);
  process.exit(1);
}

// Remplacement sur la ligne trouvée, et non réécriture du JSON : reformater trois
// fichiers pour changer trois caractères produirait un diff illisible.
for (const { file, re, version } of found) {
  const before = readFileSync(file, "utf8");
  const line = before.match(re)[0];
  writeFileSync(file, before.replace(line, line.replace(version, next)));
  console.log(`  ${version} -> ${next}  ${file}`);
}

console.log(
  `\nRelire le diff, committer, ouvrir la PR vers main : le merge publie v${next}.`,
);
