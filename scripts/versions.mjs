import { readFileSync } from "node:fs";

/**
 * Les trois fichiers qui portent le numéro de version, et l'expression qui l'y trouve.
 *
 * Le premier fait foi : le codegen de Tauri n'utilise `CARGO_PKG_VERSION` que si le
 * champ y est absent. Les deux autres suivent — `Cargo.toml` parce qu'il reste le
 * numéro que voit `cargo`, `package.json` parce qu'un dépôt qui se contredit sur sa
 * propre version fait douter du reste.
 */
export const SOURCES = [
  { file: "src-tauri/tauri.conf.json", re: /"version":\s*"([^"]+)"/ },
  { file: "package.json", re: /"version":\s*"([^"]+)"/ },
  { file: "src-tauri/Cargo.toml", re: /^version\s*=\s*"([^"]+)"/m },
];

/** Lit les trois numéros. Échoue plutôt que de supposer, si l'un est introuvable. */
export function read() {
  return SOURCES.map(({ file, re }) => {
    const found = readFileSync(file, "utf8").match(re);
    if (!found) throw new Error(`aucun numéro de version trouvé dans ${file}`);
    return { file, re, version: found[1] };
  });
}

export const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;
