import { read } from "./versions.mjs";

// Bumper un fichier et oublier les deux autres ne casse rien et n'alerte personne :
// l'installateur porte un numéro, le dépôt en affiche un autre. D'où cette vérification,
// qui coûte quelques millisecondes et rend l'oubli impossible à pousser.
const found = read();
const versions = new Set(found.map((f) => f.version));

if (versions.size !== 1) {
  console.error("Les numéros de version divergent :\n");
  for (const { file, version } of found) console.error(`  ${version}  ${file}`);
  console.error("\n`npm run bump <version>` les réaligne.");
  process.exit(1);
}

console.log(
  `version ${[...versions][0]}, cohérente dans les ${found.length} fichiers`,
);
