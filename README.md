# Thermal Lab

POC Tauri (Rust + React/TypeScript) pour mesurer l'effet réel d'un bridage CPU sur les
températures et la consommation, et le basculer en direct pendant une session de jeu.

## Documentation

Le détail est dans [`docs/`](docs/README.md) : [architecture](docs/architecture.md),
[écrire un pilote](docs/adding-a-provider.md), [ce que valent les chiffres](docs/measurement.md),
[le bridage](docs/power-control.md), [distribution](docs/distribution.md),
[développement](docs/development.md).

## Le principe

Les « optimiseurs thermiques » du commerce n'emploient pas de magie : ils désactivent le
Turbo Boost via le schéma d'alimentation de Windows. Sur un i9-14900K, cela fait passer le
processeur de ~5,7 GHz / 250-350 W à sa fréquence de base de 3,2 GHz / ~80-100 W. En jeu,
où la charge est presque toujours limitée par le GPU, la perte d'images par seconde est
marginale alors que la chute de température et de dissipation est massive.

Cette application expose ce levier et, surtout, **le mesure des deux côtés** pour que le
gain soit constaté plutôt que supposé.

## Ce que fait l'application

- elle **réside dans la zone de notification** : clic gauche sur l'icône pour ouvrir le
  panneau, clic ailleurs pour le refermer, clic droit pour le menu. Seul « Quitter » dans
  ce menu ferme l'application — voir [docs/background-ux.md](docs/background-ux.md) ;
- affichage temps réel (1 Hz) de la fréquence effective du CPU, de sa charge, des
  températures et de la puissance GPU ;
- un interrupteur unique qui bascule le bridage sur le schéma d'alimentation actif,
  doublé dans le menu de l'icône ;
- une table de comparaison qui accumule séparément les moyennes de chaque phase — on
  bascule en cours de partie et on lit l'écart à charge identique. L'accumulation est
  tenue côté Rust : elle continue panneau fermé, ce qui est précisément l'usage visé.

## Les deux réglages manipulés

Sur le schéma d'alimentation **actif**, en secteur et en batterie :

| Réglage | Bridé | Libre |
|---|---|---|
| `PERFBOOSTMODE` | `0` (turbo désactivé) | `2` (agressif, défaut Windows) |
| `PROCTHROTTLEMAX` | `99` % | `100` % |

Sur un Intel, le plafond à 99 % suffit à interdire le turbo ; les deux sont posés pour
rester cohérent quel que soit le pilote de performance en usage.

La lecture passe par le registre plutôt que par `powercfg /query` : ces réglages sont
souvent marqués comme masqués, et `powercfg` ne les affiche alors pas. L'écriture passe
par `powercfg`, qui se charge de propager la valeur au système via `/setactive`.

## Mesure des températures : la contrainte

Lire la température de die d'un processeur Intel impose de passer par les registres MSR,
donc par un **pilote noyau**. Le pilote habituellement employé pour cela (`WinRing0`)
figure sur la liste des pilotes vulnérables bloqués par Microsoft. Il n'est pas embarqué
ici : on lit à la place un outil qui fournit déjà son propre pilote signé.

Sept sources, toutes optionnelles et dégradables :

| Source | Fournit | Disponibilité |
|---|---|---|
| Compteurs de perf. WMI | fréquence par cœur, charge CPU | toujours |
| `nvidia-smi` | température, puissance, horloge, charge GPU | pilote NVIDIA |
| Zone thermique ACPI | température carte mère (indicative) | selon la carte |
| **Core Temp** (mémoire partagée) | **température et puissance package CPU** | si Core Temp tourne |
| **HWiNFO** (mémoire partagée) | idem | si HWiNFO tourne, mémoire partagée activée |
| LibreHardwareMonitor (HTTP) | idem | si LHM tourne, serveur web activé |
| **GPU AMD** via LHM (HTTP) | **température, puissance, horloge, charge GPU** | si LHM tourne, carte Radeon |

Les trois derniers apportent leur propre pilote signé ; l'application n'en embarque aucun.
`providers/core_temp.rs` mappe la section nommée `CoreTempMappingObjectEx`,
`providers/hwinfo.rs` la section `HWiNFO_SENS_SM2` — dans les deux cas en lecture seule,
sans dépendance au-delà de l'API Win32. Ces outils doivent tourner **en administrateur**
pour que leur pilote soit chargé.

La source décisive n'est toutefois pas une sonde thermique mais
`PercentProcessorPerformance`, pris **au maximum sur les cœurs individuels** : exprimé en
pourcentage de la fréquence nominale, il vaut ~99 au plus quand le turbo est bridé et
monte à 178 (≈5,7 GHz) quand il est libre. La moyenne `_Total` ne convient pas — elle
mélange 32 processeurs logiques, reste vers 50 au repos et ne franchit 100 que par
à-coups, ce qui produit un indicateur clignotant.

## Lancer

L'écriture du schéma d'alimentation exige des droits administrateur. Le binaire de
release les réclame de lui-même — écusson sur l'icône, élévation au lancement. En
développement le manifeste n'est pas posé : lancer depuis un terminal élevé, sinon
l'application démarre en lecture seule et désactive l'interrupteur en le signalant.

```bash
npm install
npm run tauri:dev     # depuis un terminal élevé pour pouvoir basculer
npm run tauri:build   # binaire + installateur
cargo test --manifest-path src-tauri/Cargo.toml
```

## Architecture

Le pipeline de mesure est agnostique : des **fournisseurs** interchangeables alimentent
un relevé commun, et aucun ne connaît les autres.

```
src-tauri/src/
  lib.rs                        les commandes exposées au frontend
  capabilities.rs               ce que la machine permet, et pourquoi pas le reste
  power.rs                      lecture registre + écriture powercfg du schéma actif
  sensors/
    metric.rs                   les grandeurs mesurables, et le relevé agrégé
    provider.rs                 le contrat que respecte toute source
    registry.rs                 quels fournisseurs, dans quel ordre de priorité
    hub.rs                      la boucle d'échantillonnage
    wmi_context.rs              accès WMI partagé (COM, connexions, variants)
    shared_memory.rs            mappage de sections nommées, partagé par deux pilotes
    lhm.rs                      capteurs LibreHardwareMonitor (HTTP, repli WMI)
    providers/
      core_temp.rs  hwinfo.rs  libre_hw.rs  amd_gpu.rs
      perf_counters.rs  nvidia.rs  acpi.rs

src/
  api.ts                        types miroirs des structures Rust
  phases.ts                     accumulation des moyennes par phase
  App.tsx                       composition du tableau de bord
  components/                   MetricCard, Sparkline, PhaseTable, ProvidersPanel
```

### Ajouter un fournisseur

Implémenter `Provider` dans `sensors/providers/`, puis l'insérer au bon rang dans
`registry::build()`. Rien d'autre à modifier : ni le relevé, ni l'UI, ni les capacités.

Trois méthodes : `info()` déclare ce que la source sait mesurer, `probe()` tente de
l'établir, `sample()` alimente le relevé.

### Résolution des conflits

Quand plusieurs fournisseurs savent mesurer la même grandeur, **le premier servi
gagne** : `Reading::offer` ne remplace jamais une valeur déjà posée, et le registre est
parcouru par priorité décroissante. Toute la règle d'arbitrage tient dans cette ligne,
et l'ordre du registre est couvert par un test.

La provenance de chaque valeur remonte jusqu'à l'interface : on doit pouvoir savoir
d'où sort un chiffre.

### Modèle de capacités

`capabilities.rs` répond à une question : que peut faire cette machine, et pourquoi pas
le reste. Chaque fournisseur absent porte sa raison et sa marche à suivre ; chaque
grandeur non mesurée indique qui saurait la fournir. Rien n'est grisé sans explication —
un poste d'entreprise verrouillé, une machine sans GPU NVIDIA et un PC sans outil de
monitoring sont des situations normales, pas des erreurs.

## Limites connues

- Quitter l'application laisse le schéma d'alimentation dans l'état où il se trouve : le
  bridage est un réglage Windows, il ne s'annule pas tout seul. Le menu de l'icône affiche
  cet état en permanence.
- Le démarrage automatique passe par une **tâche planifiée** au logon, exécutée avec les
  autorisations maximales : l'application démarre élevée, sans invite. Cocher la case
  demande l'élévation une fois.
- Le bridage s'applique au **schéma actif** : changer de schéma dans Windows change la
  cible. Le nom du schéma courant est affiché en permanence pour éviter la confusion.
- La comparaison de phases n'a de sens qu'à charge comparable. Basculer pendant un écran
  de chargement ou un menu fausse les moyennes — basculer en jeu, scène stable.
- Pas de mesure d'images par seconde : la constater demande un overlay type RTSS, hors
  périmètre de ce POC.

## Licence

[MIT](LICENSE).
