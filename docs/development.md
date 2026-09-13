# Développement

## Prérequis

Rust 1.96, Node 22, et la CLI Tauri 2 (installée en dépendance de développement du
projet). Windows uniquement : tout le code capteur et alimentation est spécifique à la
plateforme.

## Commandes

```bash
npm install
npm run tauri:dev      # application en développement, rechargement à chaud
npm run tauri:build    # binaire + installateur
npm run build          # frontend seul : tsc puis vite build
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -- --nocapture   # voir les relevés réels
```

**Lancer depuis un terminal élevé** pour que l'interrupteur de bridage soit actif. Sans
élévation l'application démarre en lecture seule et l'explique.

Pour que les températures apparaissent, lancer **Core Temp** (ou HWiNFO avec « Shared
Memory Support », ou LibreHardwareMonitor avec son serveur web) — lui aussi en
administrateur, son pilote en dépend. Inutile de redémarrer l'application : `probe()` est retenté toutes les 5 secondes.

## Tests

Répartis selon ce qu'ils protègent — le nombre exact n'est pas recopié ici, il se
démode à chaque commit :

| Zone | Ce qui est couvert |
|---|---|
| `metric.rs` | la règle d'arbitrage : premier servi, rejet des non-finis, un échec laisse la place |
| `registry.rs` | identifiants uniques, tout externe a une URL, ordre de priorité |
| `hub.rs` | le cycle d'une source : retentée au bon moment, lue dès qu'elle apparaît, perdue dès qu'elle se tait |
| `power.rs` | parsing du GUID en français et en anglais, rejet des chaînes malformées |
| `shared_memory.rs` | décodage des chaînes C, absence de section non fatale |
| `hwinfo.rs` | dispositions mémoire — `size_of` 320 et 48, alignement du `__time64_t` |
| `core_temp.rs` | décodage validé contre la vraie section partagée |
| `lhm.rs` | aplatissement de `data.json`, capteurs sans lecture écartés |
| `libre_hw.rs`, `amd_gpu.rs` | le tri des capteurs : package avant cœurs, une seule carte, rien hors du CPU |
| `phases.rs` | aiguillage vers la bonne phase, secondes déduites de la période, remise à zéro |
| `tools.rs` | cohérence des états rapportés, développement des `%VAR%`, énumération des processus |
| `autostart.rs` | la tâche est élevée, silencieuse, liée au logon ; guillemets imbriqués de l'action ; une tâche restée sur un ancien emplacement ne compte pas |

Les tests de disposition mémoire attrapent une erreur de structure **sans avoir l'outil
installé** : c'est ce qui rend `hwinfo.rs` maintenable alors qu'il n'a jamais été exécuté
contre un HWiNFO réel sur cette machine.

Le test Core Temp se comporte différemment selon que l'outil tourne : il annonce n'avoir
rien à valider s'il est absent, échoue si la structure est illisible, et vérifie des
plages physiques sinon. Un `Failed` au probe y est traité comme un bug, pas comme une
absence.

## Ce que vérifie la CI

Sur les **pull requests seulement**, pas à chaque poussée : `develop` reçoit des commits
intermédiaires qu'on ne cherche pas à valider un par un. Quatre portes, dans cet ordre :

```
npm run check:versions   # les trois fichiers qui portent le numéro concordent
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
npm run build            # tsc puis vite
```

Les cinq se lancent en local, et c'est la façon la plus rapide de savoir si une pull
request passera. La protection de `main` exige ce statut, donc rien n'y entre sans.

## Constantes de réglage

| Constante | Fichier | Valeur | Justification |
|---|---|---|---|
| période d'échantillonnage | `lib.rs` | 1000 ms | — |
| `REPROBE_EVERY` | `sensors/hub.rs` | 5 cycles | reprise à chaud d'un outil lancé après coup |
| `TURBO_THRESHOLD_PCT` | `App.tsx` | 105 % | entre ~99 (bridé) et 178 (turbo) — voir measurement.md |
| `LOAD_FLOOR_PCT` | `App.tsx` | 15 % | en dessous, l'absence de turbo ne prouve rien |
| `HISTORY` | `App.tsx` | 120 | 2 minutes de courbes à 1 Hz |
| `CAPS_MS` | `App.tsx` | 10000 ms | les fournisseurs bougent rarement ; le hub re-sonde de son côté |
| `POWER_MS` | `App.tsx` | 5000 ms | rattrape un changement de schéma fait depuis Windows |
| `PHASES_MS` | `App.tsx` | 2000 ms | simple rafraîchissement d'affichage : l'accumulation est côté Rust |
| `REOPEN_GUARD` | `flyout.rs` | 350 ms | voir le piège du clic qui rouvre, dans background-ux.md |
| `GAP` | `flyout.rs` | 12 px | marge du panneau contre l'icône et les bords d'écran |
| `INSTALL_CACHE` | `tools.rs` | 30 s | la base de désinstallation ne change pas toutes les dix secondes |

`TURBO_THRESHOLD_PCT` et `LOAD_FLOOR_PCT` ne vivent que dans `App.tsx` : le Rust publie
`cpuMaxCorePct` brut, l'interprétation appartient à l'UI. Si un jour le seuil doit servir
côté Rust — une alerte, un journal — le remonter dans `metric.rs` et l'exposer, plutôt que
d'en tenir deux copies qui divergeront.

## Dépannage

**Les températures restent vides.** Aucun fournisseur externe établi. L'onglet Système
dit pour chacun s'il est absent, installé mais arrêté — avec un bouton pour le lancer — ou
lancé sans publier. Ce dernier cas veut dire élévation manquante, ou « Shared Memory
Support » décoché dans HWiNFO.

**L'interrupteur est grisé.** Pas d'élévation, ou schémas verrouillés par stratégie. La
raison exacte est affichée sous le tableau des fournisseurs.

**Le badge dit « repos » en permanence.** Charge CPU sous 15 % : normal au bureau, la
mesure ne conclut que sous charge.

**Le démarrage automatique ne prend pas.** C'est une tâche planifiée nommée
« Thermal Lab » : `schtasks /Query /TN "Thermal Lab"` pour la voir, le Planificateur de
tâches de Windows pour l'inspecter. La créer exige l'élévation — l'application la demande
par UAC si elle ne l'a pas déjà. Un poste dont la stratégie interdit les tâches planifiées
refusera la création, et le message de `schtasks` remonte dans le bandeau.

**Le panneau ne se déplace pas.** La barre de titre est la seule prise, et elle passe par
la commande `start_drag`. Si un jour l'attribut `data-tauri-drag-region` est réintroduit,
il faudra lui accorder `core:window:allow-start-dragging` dans un fichier de capacités :
sans cette permission il échoue sans rien dire. La position ne tient que si le panneau est
épinglé — sinon il se replace contre son icône à chaque ouverture.

**Le panneau se referme dès que je clique ailleurs.** C'est la règle du mode résident.
L'épingle de la barre de titre la suspend. En développement, `THERMAL_LAB_NO_AUTOHIDE=1`
la désactive complètement — sans quoi l'ouverture des devtools, qui vole le focus, referme
le panneau.

**Deux icônes dans la zone de notification.** Une instance précédente tourne encore : le
verrou d'instance unique ne vaut que pour les binaires qui l'embarquent. Quitter l'ancienne
par son menu. Ne jamais arrêter le processus par nom d'image.

**Le schéma affiché n'est pas celui attendu.** Un outil tiers a pu le changer en se
fermant. `powercfg /getactivescheme` pour trancher.

**`cargo check` trop rapide pour être honnête.** `cargo clean -p thermal-lab` force une
recompilation réelle du crate sans rebâtir les dépendances.

## Conventions

Un fichier, une responsabilité — y compris côté frontend, où `components/` isole les
éléments d'affichage. L'accumulation des phases, elle, a quitté le frontend : tout ce qui
doit survivre au masquage du panneau appartient au Rust.

Les commentaires expliquent **pourquoi**, pas quoi. Les commentaires de ce projet qui
méritent d'être lus documentent tous un piège mesuré : la moyenne `_Total` trompeuse, la
clé de registre en lecture seule pour les administrateurs, le `/setactive` obligatoire.

Les messages destinés à l'utilisateur — `reason`, `hint`, notes de cartes — sont en
français, avec accents. Les identifiants, noms de champs et commentaires de code restent
sans accents, par prudence d'encodage sur la chaîne d'outils Windows.
