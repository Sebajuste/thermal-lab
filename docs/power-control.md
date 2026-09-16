# Le bridage : mécanisme et pièges

## Ce qui est manipulé

Deux réglages du sous-groupe processeur, sur le schéma d'alimentation **actif**, en
secteur et en batterie.

| Réglage | GUID | Bridé | Libre |
|---|---|---|---|
| `PERFBOOSTMODE` | `be337238-0d82-4146-a960-4f3749d470c7` | `0` (turbo désactivé) | `2` (agressif, défaut) |
| `PROCTHROTTLEMAX` | `bc5038f7-23e0-4960-96da-33abaf5935ec` | `99` % | `100` % |

Sous-groupe processeur : `54533251-82be-4824-96c1-47b60b740d00`.

Sur un Intel, le plafond à 99 % suffit à interdire le turbo. Les deux sont posés pour
rester cohérent quel que soit le pilote de performance en usage — ancien modèle ou Intel
Speed Shift.

C'est exactement le levier des « optimiseurs » du commerce. Aucune technologie
propriétaire n'est en jeu : le produit qui a motivé ce POC créait simplement un schéma
d'alimentation nommé avec ces deux valeurs.

## Lecture par le registre, écriture par powercfg

**Lecture.** `powercfg /query` n'affiche rien pour ces réglages quand leur attribut est
masqué — et les outils tiers les masquent. On lit donc directement :

```
HKLM\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes
  \<guid du schéma>\54533251-…\<guid du réglage>\ACSettingIndex
```

Clé absente = valeur par défaut de Windows (`2` pour le boost, `100` pour le plafond).

**Écriture.** Par `powercfg`, qui passe par l'API power privilégiée et propage au système.

```
powercfg /setacvalueindex <guid> SUB_PROCESSOR PERFBOOSTMODE 0
powercfg /setdcvalueindex <guid> SUB_PROCESSOR PERFBOOSTMODE 0
powercfg /setacvalueindex <guid> SUB_PROCESSOR PROCTHROTTLEMAX 99
powercfg /setdcvalueindex <guid> SUB_PROCESSOR PROCTHROTTLEMAX 99
powercfg /setactive <guid>
```

**Le `/setactive` final n'est pas optionnel** : sans lui les valeurs sont écrites mais
jamais appliquées.

## La garantie d'arrêt

**L'arrêt de Thermal Lab, quelle qu'en soit la cause, rend la machine dans l'état où elle
était avant l'intervention.** Le bridage est un réglage de Windows, pas un mode que
l'application tiendrait ouvert : rien ne le défait tout seul.

Le mécanisme est un journal, `restore.json`, dans le dossier de configuration du compte,
à côté de `settings.json`. Il porte le GUID du schéma et ses deux valeurs **d'avant**.

```
armé  →  écrit avant la première modification, jamais après
purgé →  une fois la machine réellement rendue, jamais avant
```

| Cause de l'arrêt | Ce qui restaure |
|---|---|
| Menu de l'icône, commande du panneau | `RunEvent::Exit`, dans la boucle d'événements |
| Redémarrage d'une mise à jour | la nouvelle instance, qui trouve le journal |
| Fin de session Windows, arrêt de tâche, plantage | le lancement suivant, avant toute lecture |
| Échec de la restauration elle-même | le journal reste : la dette est retentée plus tard |

Trois conséquences à garder en tête :

- **La référence est l'état d'avant, pas les valeurs par défaut de Windows.** Une machine
  déjà bridée par un outil tiers retrouve *son* bridage. Les défauts (`2` / `100`) ne
  servent que si la référence est perdue — journal illisible, ou bridage qui ne vient pas
  de nous.
- **Le schéma restauré est celui qu'on a modifié**, pas l'actif du moment : son GUID est
  dans le journal, et Windows a pu en activer un autre entre-temps.
- **Sans journal possible, pas de bridage.** Si le dossier de configuration est
  introuvable, l'interrupteur renvoie une erreur plutôt que de promettre un retour en
  arrière qu'il ne pourrait pas tenir.

Une seconde pression sur l'interrupteur n'écrase pas la référence : elle date de la
première intervention, sinon l'état bridé s'enregistrerait comme état d'origine.

## Pièges rencontrés

### Le test d'élévation

Ne **pas** tester l'ouverture en écriture de `SCHEMES_PATH` : cette clé n'accorde le
contrôle total qu'à SYSTEM, les administrateurs y sont en lecture seule. Un tel test
répond « non élevé » dans un terminal élevé. L'élévation se lit sur le jeton du process
(`OpenProcessToken` + `TokenElevation`), et l'écriture passe de toute façon par `powercfg`.

### Le parsing du GUID

La sortie de `powercfg /getactivescheme` est **localisée**. Découper sur « GUID du mode de
gestion » casse sur un Windows anglais. `extract_guid` cherche la forme canonique
8-4-4-4-12, et `extract_name` le texte entre parenthèses en fin de ligne. Les deux langues
sont couvertes par des tests.

### Le schéma actif change sous les pieds

Le bridage s'applique au schéma **actif au moment de l'écriture**. Un outil tiers qui se
ferme peut restaurer le schéma précédent — c'est exactement ce qui s'est produit en cours
de développement, et l'application a été soupçonnée à tort de mal détecter le turbo.

Le nom du schéma courant est affiché en permanence dans l'en-tête pour cette raison.

### Un schéma reste après désinstallation de son créateur

Un schéma tiers survit à son outil et reste utilisable :

```powershell
powercfg /list                 # inventorier
powercfg /setactive <guid>     # basculer
powercfg /duplicatescheme <guid>   # s'en faire une copie pérenne
```

Dupliquer avant de désinstaller l'outil qui l'a créé, sinon le réglage part avec lui.

## Vérifier à la main

`scripts/check-restore.ps1` rend les deux valeurs du schéma actif et l'état du journal,
en lecture seule et sans élévation. Le lancer avant, pendant et après une session prouve
la garantie d'arrêt sur la vraie machine — y compris en tuant le processus.

Le détail, si on préfère le faire soi-même :

```powershell
$g = [regex]::Match((powercfg /getactivescheme), '[0-9a-f-]{36}').Value
$base = "HKLM:\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\$g\54533251-82be-4824-96c1-47b60b740d00"
Get-ItemProperty "$base\be337238-0d82-4146-a960-4f3749d470c7" | Select ACSettingIndex  # boost
Get-ItemProperty "$base\bc5038f7-23e0-4960-96da-33abaf5935ec" | Select ACSettingIndex  # plafond
```

Pour rendre ces réglages de nouveau visibles dans l'interface Windows :

```powershell
powercfg -attributes SUB_PROCESSOR bc5038f7-23e0-4960-96da-33abaf5935ec -ATTRIB_HIDE
powercfg -attributes SUB_PROCESSOR be337238-0d82-4146-a960-4f3749d470c7 -ATTRIB_HIDE
```

## Note matériel

Sur les Intel 13ᵉ et 14ᵉ générations, concernées par la dégradation par *Vmin shift*,
faire tourner sans turbo est protecteur autant que rafraîchissant. Indépendamment de cette
application, le correctif officiel est le microcode BIOS `0x12B` ou ultérieur, avec le
profil « Intel Default Settings ».
