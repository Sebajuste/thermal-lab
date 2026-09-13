# Publier et mettre à jour

L'application s'installe chez des gens qui ne suivront pas le dépôt. Sans canal de mise à
jour, la seule version qui compte est celle du jour de l'installation — et un correctif
n'atteint personne.

## Ce qui déclenche une publication

**Le numéro de version de [`tauri.conf.json`](../src-tauri/tauri.conf.json), pas le
commit.** Le workflow [`release.yml`](../.github/workflows/release.yml) se déclenche à
chaque poussée sur `main`, mais s'arrête aussitôt si le tag `v<version>` existe déjà.

Publier revient donc à un seul geste :

```
# bumper "version" dans src-tauri/tauri.conf.json, puis
git commit -am "v0.2.0" && git push
```

Le workflow compile sur `windows-latest`, exécute les tests Rust, construit l'installateur
NSIS, crée la release GitHub, y attache l'installateur et génère `latest.json`.

Publier à chaque commit aurait deux coûts : une compilation Rust complète pour une
correction de typo, et une notification de mise à jour à tout le monde pour la même.

## Le canal de mise à jour

L'application interroge au lancement :

```
https://github.com/Sebajuste/thermal-lab/releases/latest/download/latest.json
```

GitHub redirige `latest` vers la release la plus récente ; l'URL n'a donc pas à connaître
les numéros de version. **Cela suppose un dépôt public** : les assets d'un dépôt privé ne
sont pas téléchargeables sans jeton, et l'application n'en a pas.

La recherche au lancement échoue en silence — ne pas joindre GitHub n'est pas un événement
dont l'utilisateur a quelque chose à faire. Celle du bouton, dans l'onglet Système, rend
son erreur : elle a été demandée.

## La signature de mise à jour

Un paquet téléchargé est vérifié contre une clé publique inscrite dans la configuration.
Sans elle, n'importe qui pouvant servir un `latest.json` à la place de GitHub servirait
aussi un exécutable qui s'installerait tout seul, en administrateur.

À ne pas confondre avec la signature Authenticode discutée dans
[distribution.md](distribution.md) : celle-ci authentifie la **mise à jour** auprès des
installations existantes, l'autre authentifie l'**éditeur** auprès de Windows. Les deux
manquent aujourd'hui ; elles ne se remplacent pas.

### Générer la paire, une seule fois

```powershell
npx tauri signer generate -w "$env:USERPROFILE\.tauri\thermal-lab.key"
```

Par `npx` et non `npm run` : `npm run -- -w` ne transmet pas `-w` au script, npm
l'interceptant comme son propre `--workspace`.

La clé privée est **la** chose à ne pas perdre : la remplacer casserait la mise à jour de
toutes les installations existantes, qui refuseraient un paquet signé par une inconnue.
Elles devraient être réinstallées à la main.

Trois endroits à renseigner ensuite :

| Où | Quoi |
|---|---|
| `tauri.conf.json` → `plugins.updater.pubkey` | contenu de `thermal-lab.key.pub` |
| Secret GitHub `TAURI_SIGNING_PRIVATE_KEY` | contenu de `thermal-lab.key` |
| Secret GitHub `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | le mot de passe saisi |

Tant que `pubkey` est vide, l'application démarre et cherche normalement : la clé ne sert
qu'au moment de vérifier le paquet téléchargé. L'échec n'arriverait donc qu'à
l'installation, au pire moment. À renseigner avant la première release.

## Le mot de passe vide, et un message trompeur

La cle generee ici n'a pas de mot de passe. Le secret
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` doit donc porter une **chaine vide** — et non ne pas
exister : le workflow resout un secret absent en chaine vide et pose la variable malgre
tout, si bien que supprimer le secret ne change rien. Pour qu'elle n'existe pas, il
faudrait retirer la ligne du workflow.

La saisie interactive de `gh secret set` lit l'entree standard telle quelle. Y repondre
par une touche Entree seule peut y laisser un caractere, et un mot de passe d'un caractere
suffit a tout casser. Pour forcer le vide :

```
printf '' | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo Sebajuste/thermal-lab
```

Le message rendu alors par Tauri est **failed to decode secret key: incorrect updater
private key password**. Il designe le mot de passe, mais recouvre en realite tout echec de
dechiffrement de la cle — un fichier altere en transit donne le meme. Le depart se fait en
deux essais locaux, qui coutent une seconde :

```
npx tauri signer sign -f "$env:USERPROFILE\.tauri\thermal-lab.key" -p "" fichier
npx tauri signer sign -f "$env:USERPROFILE\.tauri\thermal-lab.key" -p "x" fichier
```

Si le premier passe et que le second rend le message du CI, la cle est saine et c'est le
mot de passe qui est en cause.

Enfin, depuis PowerShell, envoyer un fichier a `gh secret set` demande `-Raw` : sans lui,
`Get-Content` decoupe en lignes et le pipeline les recompose, ce qui suffit a casser le
checksum minisign.

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\thermal-lab.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo Sebajuste/thermal-lab
```

## Le piege : `createUpdaterArtifacts`

`bundle.createUpdaterArtifacts` vaut **`false`** par defaut. Sans lui, le bundler produit
l'installateur et s'arrete la : ni signatures, ni paquets de mise a jour. Le build
reussit, la release se cree, l'installateur s'y attache — et `latest.json` manque, sans
qu'aucune etape n'ait echoue. C'est ce qui est arrive a la v0.1.0, dont le seul indice
tenait a une ligne de log : « Signature not found for the updater JSON. Skipping
upload... ».

Le symptome a distance est muet aussi : les installations existantes interrogent un
`latest.json` absent, recoivent un 404, et concluent qu'elles sont a jour.

## L'installateur : `currentUser`

L'installateur NSIS écrit dans `%LOCALAPPDATA%`, pour l'utilisateur courant seulement.
Ce n'est pas un détail d'emplacement : un installateur `perMachine` écrit dans
`Program Files`, ce qui demande l'élévation **à chaque mise à jour**. Une invite UAC par
correctif suffirait à faire cliquer « plus tard » indéfiniment.

L'application continue de s'élever seule au lancement, par son manifeste — les deux
mécanismes sont indépendants. Voir [distribution.md](distribution.md).

## Ce qui reste désagréable

Le binaire n'est pas signé Authenticode. Chaque mise à jour rejoue donc l'invite UAC
jaune « Éditeur inconnu ». L'auto-mise à jour rend le certificat **plus** utile, pas
moins : on passe d'une alerte à l'installation à une alerte récurrente.
