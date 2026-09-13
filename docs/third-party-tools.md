# Les outils tiers : état et lancement

Trois des grandeurs les plus intéressantes — température de die, puissance package —
n'existent que si un outil tiers tourne et a chargé son pilote noyau. Le modèle de
capacités doit donc raconter la vie de cet outil, pas seulement celle du fournisseur qui
le lit.

## Deux informations qui ne disent pas la même chose

| Source | Ce qu'elle sait |
|---|---|
| `ProbeState` du fournisseur | la mesure arrive, ou non |
| `tools::ToolStatus` | l'outil est installé, il tourne, on sait où il est |

« Core Temp ne tourne pas » recouvrait trois situations qui n'appellent pas la même
action. C'est leur croisement qui donne l'état affiché :

| Fournisseur | Outil | Affiché | Ce qu'on propose |
|---|---|---|---|
| établi | — | `actif` | rien |
| non établi | tourne | `lancé` | le réglage qui manque (mémoire partagée, serveur web, élévation) |
| non établi | installé, arrêté | `arrêté` | **le lancer**, en administrateur |
| non établi | absent, avec certitude | `absent` | le lien de téléchargement |
| non établi | introuvable, sans certitude | `introuvable` | le lien, sans affirmer qu'il manque |
| en échec | — | `erreur` | le message de la source |

L'état `lancé` est celui qui justifie tout le reste : un outil peut tourner sans rien
publier. HWiNFO démarre avec sa mémoire partagée désactivée ; Core Temp lancé sans
élévation ne charge pas son pilote ; LibreHardwareMonitor démarre sans son serveur web.
Sans la détection de processus, ces cas ressemblaient à « pas installé ».

## Le cas LibreHardwareMonitor : le WMI a disparu

LHM publiait ses capteurs dans l'espace de noms WMI `root\LibreHardwareMonitor`. La
version **0.9.5** (janvier 2026) a supprimé ce fournisseur — .NET 10 ne le supporte plus
([issue #2143]) — et les mainteneurs renvoient vers le serveur HTTP intégré. Un outil à
jour tournait donc en affichant `lancé` indéfiniment, et le conseil qui accompagnait cet
état, « lancer en administrateur », était sans effet.

`sensors/lhm.rs` établit donc deux voies, dans cet ordre :

1. **HTTP** — `http://localhost:8085/data.json`, la seule que propose un LHM à jour. Le
   port se règle dans les options de l'outil ; `THERMAL_LAB_LHM_PORT` permet de nous le
   dire quand il n'est pas celui par défaut ;
2. **WMI** — `root\LibreHardwareMonitor` puis `root\OpenHardwareMonitor`, pour les
   versions antérieures à 0.9.5 et pour OpenHardwareMonitor.

Les champs se correspondent un pour un, ce qui laisse les deux pilotes qui les lisent
(`libre_hw`, `amd_gpu`) ignorer par quelle voie la mesure est arrivée :

| WMI | `data.json` |
|---|---|
| `Identifier` | `SensorId` |
| `SensorType` | `Type` |
| `Name` | `Text` |
| `Value` | `RawValue` |

`RawValue` et non `Value` : ce dernier est une chaîne mise en forme et localisée
(`« 52,4 °C »`), inexploitable. Un capteur sans lecture vaut `"NaN"` et ne produit pas de
ligne — le laisser passer masquerait le repli d'un pilote par une valeur qui ne mesure
rien.

Le serveur web n'étant pas actif par défaut, c'est ce que dit désormais l'état `lancé`
pour cet outil : **Options → Remote Web Server → Run**, l'élévation restant nécessaire par
ailleurs pour le pilote noyau.

[issue #2143]: https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/2143

## Comment l'outil est localisé

Trois pistes, dans l'ordre, dans `tools.rs` :

1. **la base de désinstallation** — les trois emplacements (64 bits, 32 bits,
   utilisateur), en cherchant un fragment de `DisplayName` puis en lisant
   `InstallLocation` ou `DisplayIcon`. La recherche porte sur le nom affiché et non sur
   la clé, dont le suffixe dépend de l'installateur employé ;
2. **`App Paths`** — ce que Windows consulte quand on tape le nom d'un programme ;
3. **les emplacements usuels**, avec développement des `%VAR%`.

Le résultat est mis en cache trente secondes : parcourir quelques centaines de clés de
registre est imperceptible, le refaire toutes les dix secondes serait gratuit. L'état
« en cours d'exécution », lui, est toujours relu.

### Le cas portable

LibreHardwareMonitor est distribué en archive : il n'a **aucune entrée de
désinstallation**, et rien ne dit où l'utilisateur l'a déposé. Ne pas le trouver ne
prouve donc pas son absence — d'où le drapeau `certain`, et l'état `introuvable` plutôt
qu'`absent`. Affirmer qu'un outil manque alors qu'il est peut-être à deux dossiers de là
serait exactement le genre d'approximation que le modèle de capacités cherche à éviter.

## La détection d'exécution

`CreateToolhelp32Snapshot` sur les noms d'image, comparés à l'exécutable de l'outil et à
ses alias (`HWiNFO32.exe`, `OpenHardwareMonitor.exe`…). Le nom suffit : on ne cherche pas
où l'outil tourne, seulement s'il tourne. C'est aussi la seule information qu'un processus
élevé laisse lire à une application qui ne l'est pas.

## Le lancement

`ShellExecuteW` avec le verbe `runas`, sur un geste explicite de l'utilisateur — le bouton
de la ligne concernée. Les trois outils chargent un pilote noyau : sans élévation ils
démarrent et ne publient rien, ce qui serait pire que de ne pas les lancer. C'est donc
Windows qui pose la question, parce qu'il est le seul à pouvoir la poser.

Le code de retour est une pseudo-instance : au-delà de 32 c'est un succès, en deçà un code
d'erreur historique dont seul `5` — élévation refusée — mérite un message propre.

Un outil qui tourne déjà n'est pas relançable : `launchable` vaut `installé && !tourne`,
et la règle est calculée en Rust pour ne pas exister en double dans l'interface.

## Ajouter un outil

Une entrée dans `TOOLS`, une ligne dans `BY_PROVIDER` — le second permet à deux
fournisseurs de dépendre du même outil, ce qui est le cas de `libre-hw` et `amd-gpu`.
Rien d'autre ne bouge : l'interface se déduit des drapeaux.
