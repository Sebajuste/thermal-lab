# Camomile : l'enquête à l'origine du projet

Outbyte Camomile 2.0.4.51156, vendu comme un réducteur de température CPU, obtenait un
gain thermique réel par un moyen entièrement natif et trivialement reproductible — couper
le Turbo Boost dans un schéma d'alimentation Windows — tout en embarquant un socle
d'analyse système qui tenait la clef USB de la machine et empêchait Rufus d'écrire.

Ce document existe parce que tout ce que fait Thermal Lab vient de là : les deux réglages
manipulés, le choix de mesurer des deux côtés, et la méfiance envers les outils qui
promettent une optimisation sans dire ce qu'ils touchent.

Il sépare délibérément ce qui a été **prouvé** de ce qui a été **déduit**. Les deux
n'ont pas la même valeur, et confondre les deux est le défaut de la plupart des analyses
de ce genre.

---

## Ce qui est prouvé

### 1. Il tenait la clef USB ouverte en lecture/écriture

La preuve la plus directe ne vient pas de l'enquête mais de **Rufus**, qui énumère les
handles ouverts sur un périphérique et nomme les processus détenteurs. Extrait de son
journal :

```
WARNING: The following application(s) or service(s) are accessing the drive:
● [30608] "C:\Program Files (x86)\Outbyte\Camomile\Camomile.exe" /UseTray  /FromLogon /Schedule (rwx)
You should close these applications before retrying the operation.
Could not lock access to \\.\PhysicalDrive2: [0x00000005] Accès refusé.
```

`(rwx)` est la nature de l'accès détenu ; `0x5` l'échec de verrouillage qui en découle.
Ce n'est ni une corrélation ni une déduction : c'est une énumération de handles.

Symptôme vécu : Rufus refusait d'écrire avec « impossible d'accéder au média, il peut être
en cours d'utilisation par une autre application », et débrancher la clef n'y changeait
rien — Camomile s'y raccrochait à chaque insertion.

> **Cette preuve n'est plus consultable.** Rufus réécrit `rufus.log` à chaque session, et
> l'écriture Debian réussie l'a écrasée. Le fichier ne contient plus aucune occurrence de
> « Camomile ». La citation ci-dessus a été relevée avant l'écrasement.

### 2. Il coupait le Turbo Boost via un schéma d'alimentation

`powercfg /getactivescheme` renvoyait un schéma nommé **Camomile**, GUID
`4e2a2b94-6646-493e-9f10-64f712e088aa`, actif. Lecture du registre sous
`HKLM\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes`, comparée au schéma
Windows d'origine :

| Réglage | Camomile | Utilisation normale |
|---|---|---|
| `PERFBOOSTMODE` | **0** — turbo désactivé | non défini (défaut : 2) |
| `PROCTHROTTLEMAX` | **99 %** (secteur et batterie) | non défini (défaut : 100) |
| `PROCTHROTTLEMIN` | 5 % | non défini |

C'est tout le mécanisme thermique. Aucune technologie propriétaire : deux valeurs natives
de Windows, dont chacune suffit seule à interdire le turbo sur un Intel. Voir
[power-control.md](power-control.md) pour le détail du levier.

### 3. Ce qu'il embarquait réellement

Contenu de `C:\Program Files (x86)\Outbyte\Camomile` — un outil qui ne ferait que piloter
la fréquence du processeur n'a besoin d'aucun de ces modules :

| Module | Fonction apparente |
|---|---|
| `HWHelper.dll`, `HWHelper64.exe` | lecture des capteurs — la seule partie thermique |
| `SpywareCheckerHelper.dll` | analyse de fichiers |
| `UninstallManagerHelper.dll` | gestionnaire de désinstallation |
| `StartupManagerHelper.dll` | gestionnaire de démarrage |
| `RescueCenterHelper.dll` | « centre de secours » |
| `PopupManagerHelper.dll` | notifications |
| `GoogleAnalyticsHelperIV.dll` | télémétrie |
| `sqlite3.dll` | base locale |

C'est le socle commun des utilitaires Outbyte, avec la surveillance thermique posée
par-dessus.

### 4. C'était un binaire authentique

Signatures Authenticode **valides**, éditeur « Outbyte Computing Pty Ltd », version
2.0.4.51156, installé le 28/05/2026. Ce n'était pas un logiciel malveillant déguisé :
c'était le produit d'Outbyte, faisant ce qu'il fait.

### 5. Son nom de tâche planifiée contenait un homoglyphe cyrillique

Démarrage par tâche planifiée — ni service Windows, ni entrée de démarrage classique :

```
Start Camomile оn logon  [Ready]  ->  Camomile.exe /UseTray  /FromLogon /Schedule
```

Analyse caractère par caractère : le « о » de « on » est **U+043E** (cyrillique), pas
U+006F (latin). Effet de bord vérifiable : la tâche échappe à une recherche textuelle
sur « on ».

---

## Ce qui n'a été que déduit

**Que l'effet thermique venait de ce schéma.** Le mécanisme est documenté par Microsoft,
les valeurs sont sans ambiguïté, et l'ordre de grandeur colle à l'observation de
l'utilisateur (60 → 40 °C en jeu, perte d'images par seconde quasi nulle, ~3 °C de moins
dans la pièce). Mais **la machine n'a jamais été mesurée pendant que le schéma Camomile
était actif** : au moment des relevés par cœur, elle était déjà repassée sur « Utilisation
normale ». Les 178 % du nominal mesurés prouvent ce que fait le turbo *libre*, pas ce que
faisait Camomile.

> **Corroboration externe.** Un test indépendant publié en juillet 2024 décrit le même
> mécanisme, atteint par une autre méthode — l'observation des fréquences, là où cette
> enquête a lu le registre :
>
> > « when you enable "Cooling" mode, it basically locks CPU to its Base clock »
>
> Ses mesures : 4,45 GHz → 3,35 GHz, débit de compression 7-zip **inchangé** à 38-40 MB/s,
> aucune différence perceptible en jeu. Verrouiller le processeur à sa fréquence de base
> est très exactement l'effet de `PERFBOOSTMODE = 0` et `PROCTHROTTLEMAX = 99`.
>
> Deux méthodes indépendantes, même conclusion. Cela ne transforme pas la déduction en
> mesure faite ici, mais la sort du statut d'hypothèse.

Outbyte annonce d'ailleurs sur son propre blog un refroidissement de 100 °C à 59 °C. Pour
une fois dans cette catégorie de logiciels, **la promesse marketing est techniquement
honnête**.

**Qu'il restaurait le schéma précédent en se fermant.** Corrélation entre deux mesures
encadrant sa fermeture. Plausible, non démontré.

**Que l'accès disque venait d'un module d'analyse.** Rufus nomme le processus, pas la
bibliothèque. Le module responsable n'a pas été identifié.

---

## Ce qui n'a pas été établi

- **Pourquoi** il ouvrait un média amovible en écriture.
- **Comment** il lisait les températures : aucun `.sys` dans son dossier, donc soit un
  pilote installé ailleurs, soit déposé à l'exécution. Non cherché.
- Si le caractère cyrillique est une coquille ou délibéré. L'hypothèse d'un clavier non
  latin est une hypothèse, pas un résultat.

---

## Une affirmation retirée

Il a d'abord été écrit que **Camomile masquait les réglages processeur** dans `powercfg`,
au motif que `powercfg /query <guid> SUB_PROCESSOR` ne renvoyait qu'un en-tête vide.

Cette conclusion ne tient pas. Les attributs de visibilité n'ont jamais été lus pendant
que l'outil était installé, et le masquage de ces réglages est le comportement **par
défaut** de Windows. Ce qui a été attribué à Camomile était probablement l'état d'usine.

Le fait observé reste vrai — `powercfg` n'affichait rien, d'où la lecture par le registre,
qui reste la bonne méthode. Seule l'attribution de cause était infondée.

---

## État final

Camomile a été désinstallé. Son dossier n'existe plus, aucun processus Outbyte ne tourne,
et **son schéma d'alimentation a disparu avec lui** : `powercfg /list` ne montre plus que
les trois schémas Windows.

C'est le piège à connaître : un schéma créé par un outil tiers part avec l'outil. Pour le
conserver, il fallait le dupliquer avant la désinstallation :

```powershell
powercfg /duplicatescheme <guid>
```

Le bridage reste évidemment reproductible sans lui — c'est précisément ce que fait Thermal
Lab, et ce que `powercfg` fait à la main.

---

## L'éditeur, vérifié après coup

**OUTBYTE COMPUTING PTY LTD** — société privée **australienne**, ABN 87 615 979 765,
ACN 615 979 765, active depuis le **31 août 2018**, Nouvelle-Galles du Sud. Présence
commerciale américaine par ailleurs, à Santa Barbara (Californie). Entité réelle et
identifiable, ce qui n'a rien d'évident dans cette catégorie de logiciels — et cohérent
avec les signatures Authenticode valides relevées sur la machine.

Sa réputation est partagée, et les deux versants sont documentés.

**À charge.** Malwarebytes classe la famille en `PUP.Optional.Outbyte` — programme
potentiellement indésirable — au motif que ces logiciels font payer « des services sans
valeur ou déjà inclus dans Windows ». Une campagne de diffusion d'Outbyte PC Repair a
affiché des notifications **imitant la fenêtre de Windows Update** ; l'éditeur a reconnu
les faits et rompu avec l'affilié. Le Better Business Bureau lui attribue un **C+**, sans
accréditation, avec une plainte restée sans réponse.

**À décharge.** Certification AppEsteem — l'organisme qui vérifie précisément l'absence de
pratiques trompeuses dans cette catégorie — pour PC Repair, Driver Updater et AVarmor,
avec surveillance continue. Camomile est distribué sur le Microsoft Store. Le volume de
plaintes est faible : deux en trois ans.

**Ce qu'il faut en retenir.** La classification PUP vise le modèle commercial et l'histoire
marketing de la famille, pas un comportement malveillant du code. Camomile n'est pas un
logiciel malveillant : c'est un logiciel dont la valeur repose sur la méconnaissance de ce
que l'utilisateur possède déjà.

## Ce que le projet en a tiré

1. **Les deux réglages à manipuler**, et le fait qu'ils suffisent.
2. **Mesurer des deux côtés.** L'utilisateur croyait à une optimisation ; c'était un
   bridage. Les deux donnent le même résultat en jeu parce que la charge y est limitée par
   le GPU — mais ce n'est pas la même chose, et seule une mesure le montre.
3. **Dire ce qu'on touche.** Un outil qui modifie la configuration système de quelqu'un
   doit nommer le schéma affecté, rendre le changement réversible, et ne jamais l'appliquer
   sans demande explicite.
4. **Ne pas ouvrir ce qu'on n'a pas à ouvrir.** Un utilitaire thermique n'a aucune raison
   de tenir un handle sur un périphérique amovible.

---

## Sources externes

- [ABN Lookup — ABN 87 615 979 765](https://abr.business.gov.au/ABN/View?id=87615979765)
- [Malwarebytes — PUP.Optional.Outbyte](https://www.malwarebytes.com/blog/detections/pup-optional-outbyte)
- [Better Business Bureau — Outbyte](https://www.bbb.org/us/ca/santa-barbara/profile/computer-software/outbyte-1236-92089195)
- [Angry Sheep Blog — Outbyte Camomile software CPU cooling](https://rejzor.wordpress.com/2024/07/21/outbyte-camomile-software-cpu-cooling/) — le test indépendant cité plus haut
- [Outbyte — Camomile can cool down CPU](https://outbyte.com/blog/camomile-can-cool-down-cpu/)
- [Outbyte — Product line certified by AppEsteem](https://outbyte.com/blog/outbyte-product-line-certified-by-appesteem/)
- [2-spyware — Remove Outbyte PC Repair](https://www.2-spyware.com/remove-outbyte-pc-repair.html) — historique de la campagne imitant Windows Update

Consultées le 14 septembre 2026.
