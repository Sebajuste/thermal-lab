# Le mode résident

L'application vit dans la zone de notification. Le panneau n'est qu'une vue sur une
mesure qui, elle, ne s'arrête jamais tant que l'icône est là.

## Le contrat, en quatre règles

| Geste | Effet |
|---|---|
| Clic gauche sur l'icône | ouvre le panneau contre l'icône, ou le referme s'il est ouvert |
| Clic ailleurs, `Échap`, croix du panneau | masque le panneau — **la mesure continue** |
| Clic droit sur l'icône | menu : ouvrir, bascule du bridage, démarrage automatique, quitter |
| « Quitter » dans ce menu | seule sortie de l'application |

Rien d'autre ne ferme le processus. `WindowEvent::CloseRequested` est intercepté et
converti en masquage, ce qui rend la croix inoffensive — son libellé au survol le dit,
parce qu'aucune croix de fenêtre ne laisse deviner qu'elle ne ferme rien.

## Pourquoi un volet et pas une fenêtre

Le panneau se comporte comme les volets système de Windows 11 (volume, réseau) : sans
bordure, ancré sur l'icône, au premier plan, absent de la barre des tâches, refermé dès
qu'il perd le focus. C'est la forme attendue pour quelque chose qu'on ouvre dix secondes
pour lire une valeur et qu'on referme.

L'épingle de la barre de titre suspend le masquage automatique. Elle n'est pas un
ornement : l'usage central de cette application est de regarder les températures
**pendant** qu'un jeu a le focus, c'est-à-dire exactement quand la règle de masquage
jouerait contre elle.

Elle suspend aussi le **replacement**. Détaché, le panneau est un volet : il revient
contre son icône à chaque ouverture, et le déplacer n'aurait aucune suite. Épinglé, il
devient une fenêtre ordinaire : il garde la position où on le pose, ouverture après
ouverture. La même case gouverne les deux, parce que c'est le même changement de nature.

### Déplacer une fenêtre sans bordure

Sans décoration, la barre de titre est la seule prise. Elle appelle la commande
`start_drag`, et non l'attribut `data-tauri-drag-region` : **cet attribut exige la
permission `core:window:allow-start-dragging`, qui n'est pas dans le jeu accordé par
défaut, et son absence ne produit aucun message.** Le panneau était simplement
impossible à déplacer, en silence. Une commande, elle, remonte son erreur dans le
bandeau — c'est la seule raison de ce choix, et elle suffit.

### La forme du panneau

460 × 610, et rien de négociable dedans : c'est un volet qu'on ouvre dix secondes.

- **une seule ligne d'en-tête** — processeur, schéma d'alimentation, et l'interrupteur à
  bascule à droite. L'état se relit aussi sur la pastille de la barre de titre et sur la
  couleur de l'icône, ce qui dispense l'interrupteur de porter une étiquette ;
- **trois onglets en bas, sur icônes seules** — mesures en direct, comparaison, capacités
  et réglages. Le libellé reste au survol et pour le lecteur d'écran ;
- **du texte remplacé par des info-bulles** partout où il n'apprenait rien à la deuxième
  lecture : « maximum des cœurs », « occupation totale des cœurs », le type d'un
  fournisseur. Ce qui reste à l'écran est chiffré, ou actionnable — la marche à suivre
  d'un fournisseur absent, par exemple, ne bouge pas.

La provenance de chaque mesure, elle, reste visible sous la carte : c'est une décision
d'architecture, pas un ornement. Quand deux outils de monitoring tournent, il faut savoir
lequel parle.

La hauteur est calée sur l'onglet des mesures, le plus chargé des trois à ne pas devoir
défiler. Ajouter une carte veut donc dire remonter cette valeur dans `tauri.conf.json`,
ou accepter une barre de défilement sur la vue principale.

### Le piège du clic qui rouvre

Cliquer sur l'icône alors que le panneau est ouvert produit deux événements dans cet
ordre : perte de focus du panneau (il se masque), puis notre gestionnaire de clic (qui le
voit masqué et le rouvre). Sans précaution, l'icône n'arrive jamais à refermer le panneau.

`flyout.rs` pose donc un horodatage à chaque masquage par perte de focus et ignore toute
demande d'ouverture qui suit de moins de 350 ms. C'est le seul endroit du code où l'ordre
des événements Windows transparaît.

## Le placement

L'ancre est le rectangle de l'icône, fourni par `TrayIconEvent::Click`. Le panneau se
place centré sur elle, au-dessus si l'icône est dans la moitié basse de son écran, en
dessous sinon — ce qui couvre une barre des tâches déplacée en haut. Le résultat est
borné à l'écran qui porte l'icône, choisi parmi `available_monitors()` : sur un poste
multi-écrans, le volet ne s'ouvre pas sur le voisin.

Ouvrir par le menu avant tout clic sur l'icône (il n'y a alors pas d'ancre) replie sur le
coin bas-droit de l'écran principal.

## Le démarrage

| Lancement | Comportement |
|---|---|
| Manuel | icône posée **et** panneau ouvert — sans quoi un double-clic sur l'exécutable semblerait sans effet |
| Ouverture de session | icône seule, panneau masqué |

La distinction tient à l'argument `--silent`, que le démarrage automatique ajoute à la
ligne de commande.

### Pourquoi une tâche planifiée et pas la clé `Run`

La voie évidente — une valeur sous `HKCU\...\CurrentVersion\Run` — lance le programme avec
le **jeton filtré** de la session, donc sans élévation. Et Windows ne pose aucune question
UAC à l'ouverture de session : un programme qui réclamerait l'élévation depuis cette clé
serait bloqué, pas promu. L'application démarrerait en lecture seule, mesurant tout et ne
pouvant rien basculer — l'inverse de ce qu'on attend d'un démarrage automatique ici.

`autostart.rs` crée donc une tâche déclenchée au logon, avec « exécuter avec les
autorisations maximales » :

```
schtasks /Create /TN "Thermal Lab" /TR "\"<exe>\" --silent"
         /SC ONLOGON /RL HIGHEST /RU <domaine\utilisateur> /IT /F
```

C'est ce que font Core Temp et HWiNFO pour leur propre démarrage. Le prix est une invite
UAC **au moment où l'on coche la case**, une fois, et plus jamais ensuite.

La tâche et le manifeste de release (voir [distribution.md](distribution.md)) ne font pas
double emploi : le manifeste couvre les lancements manuels, la tâche couvre l'ouverture de
session, où aucune invite ne peut être affichée. `/RL HIGHEST` est ce qui permet à la
seconde de satisfaire le premier sans rien demander.

Deux détails qui ont leur raison d'être :

- `/RU` **avec** `/IT` — sans `/IT`, `schtasks` réclamerait un mot de passe. Avec, la
  tâche s'exécute sous le compte courant, seulement quand il est ouvert ;
- créer ou supprimer une tâche `HIGHEST` exige l'élévation. Quand l'application l'a déjà,
  elle appelle `schtasks` directement — synchrone, sortie lisible. Sinon elle passe par
  `ShellExecuteEx` avec le verbe `runas` et attend le code de sortie : `ShellExecute` seul
  ne rend aucune poignée sur le processus, donc aucun moyen de distinguer une tâche créée
  d'une commande refusée.

La lecture de l'état n'exige rien : `schtasks /Query` sur une tâche inexistante répond
« fichier introuvable », pas « accès refusé ».

### La case répond « cet exécutable-ci », pas « une tâche existe »

`is_enabled()` lit la tâche en XML et vérifie que son `<Command>` **est le binaire en
cours d'exécution**. Se contenter de l'existence de la tâche serait un piège : déplacer
l'exécutable la laisse pointer sur l'ancien emplacement, où plus rien ne répond. La case
resterait cochée en annonçant un démarrage automatique qui échouerait en silence à la
session suivante — et la réparer demanderait deux gestes, dont le premier détruit.

Posée ainsi, la case se décoche d'elle-même après un déplacement, et un seul clic la
réinscrit au bon endroit : `/F` remplace la tâche homonyme, sans doublon ni résidu.

Conséquence en développement : lancer le binaire de debug pendant qu'une tâche vise celui
de release montre la case décochée, et la cocher **repointerait** le démarrage automatique
sur le binaire de debug. C'est cohérent avec ce que la case affirme, mais mieux vaut le
savoir.

Une seule instance est admise (`tauri-plugin-single-instance`) : deux icônes et deux
boucles de mesure pour un seul état système n'auraient aucun sens. Relancer l'exécutable
revient donc à rappeler le panneau.

## Ce que porte l'icône

- **sa couleur** — teintée en vert quand le bridage est actif, d'origine sinon. La teinte
  est calculée au lancement à partir de l'icône du bundle : sa luminance, reteintée du
  vert `--good` de la feuille de style. Une seule ressource, deux états, et la pastille du
  panneau désigne la même chose que l'icône ;
- **son infobulle** — dernières températures et puissances, plus l'état du bridage. C'est
  la réponse à « il fait quoi, là ? » sans rien ouvrir. Réécrite seulement quand le texte
  change, pour ne pas appeler Win32 une fois par seconde sans raison ;
- **son menu** — la bascule du bridage y est dupliquée depuis le panneau, parce que
  l'ouvrir uniquement pour cliquer un interrupteur serait absurde. La coche est remise sur
  l'état réel si l'écriture échoue, et l'erreur remonte au panneau, qu'on ouvre pour elle.

## La mesure ne dépend pas de la fenêtre

C'est la contrainte qui a décidé de la répartition du code. WebView2 bride les minuteries
d'une fenêtre masquée — une seconde par tick, puis une minute au-delà de cinq minutes
d'occultation. Un accumulateur de phases côté frontend produirait donc des moyennes
calculées sur quelques échantillons épars, précisément dans l'usage visé : basculer le
bridage en cours de jeu, panneau fermé.

L'accumulation vit donc dans `phases.rs`, alimentée par le thread d'échantillonnage via le
point d'accroche `SensorHub::start(period, on_reading)`. Le frontend ne fait plus que lire
un instantané. Le même point d'accroche sert à rafraîchir l'infobulle.

Corollaire à garder en tête : **tout ce qui doit survivre au masquage du panneau
appartient au Rust.** L'historique des courbes, lui, est resté côté React — il n'a de sens
que sous les yeux de quelqu'un.
