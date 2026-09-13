# Distribuer l'outil

Ce document existe parce que ce qui marche sur la machine de développement ne marche pas
ailleurs, et que les raisons sont structurelles plutôt qu'accidentelles.

## Le mur : la température exige un pilote noyau

Les registres MSR sont en ring 0. **Aucune API utilisateur de Windows n'expose la
température de die** — ni WMI, ni les compteurs de performance, ni une bibliothèque
quelconque. Core Temp, HWiNFO, LibreHardwareMonitor, les utilitaires des fabricants :
tous embarquent le leur.

Trois options, aucune gratuite.

### Signer son propre pilote

Certificat EV, inscription au Partner Center, signature par attestation Microsoft. Au-delà
du coût, on devient responsable d'un pilote donnant accès aux MSR à qui sait lui parler —
la catégorie exacte que Microsoft finit par bloquer.

### Dépendre d'un outil tiers

Le choix retenu. Les pilotes `core_temp`, `hwinfo` et `libre_hw` détectent l'outil et
lisent son interface publique. L'utilisateur installe ce qu'il veut, ou rien.

### Se passer de la température

Moins absurde qu'il n'y paraît : voir [measurement.md](measurement.md). La fréquence par
cœur prouve le mécanisme sans aucun pilote.

## Pourquoi pas un composant libre embarqué

Le seul candidat réaliste est `WinRing0` ou l'un de ses dérivés — c'est ce qu'utilise
LibreHardwareMonitor. Il figure sur la liste des pilotes vulnérables bloquée par défaut par
Microsoft (`VulnerableDriverBlocklistEnable = 1`, constaté sur la machine de dev).

L'embarquer, ou le proposer dans l'installateur, revient à distribuer une élévation de
privilèges sous sa propre signature. Écarté.

## L'élévation : demandée par le binaire, en release

Le binaire de release porte un manifeste applicatif
([`windows-app-manifest.xml`](../src-tauri/windows-app-manifest.xml)) réclamant
`highestAvailable`. Windows affiche donc l'écusson sur son icône et l'élève au lancement,
sans que l'utilisateur ait à y penser — le bridage du schéma d'alimentation est la raison
d'être de l'application, et le découvrir grisé après coup est une mauvaise façon de
l'apprendre.

`highestAvailable` et non `requireAdministrator` : sur un compte administrateur les deux
élèvent à l'identique, mais sur un compte standard le second **refuserait de démarrer**.
Or c'est exactement le poste décrit plus bas, celui où l'application a encore quelque
chose à offrir — mesurer, et dire pourquoi elle ne peut rien basculer. Interdire le
lancement jetterait le modèle de capacités avec l'eau du bain.

Le manifeste n'est posé qu'en release : `tauri dev` relance le binaire à chaque
modification du Rust, et une invite UAC par redémarrage rendrait la boucle inutilisable.

**Conséquence sur la signature :** un binaire non signé qui réclame l'élévation affiche
une invite UAC jaune « Éditeur inconnu », plus alarmante que la bleue d'une application
signée. Le certificat n'est plus seulement une question de SmartScreen à l'installation,
il conditionne la tête de la fenêtre affichée à chaque lancement.

## Sur un poste d'entreprise : quatre obstacles indépendants

En lever un ne sert à rien si les autres tiennent.

1. **L'intégrité de la mémoire (HVCI)**, activée par défaut sur les installations
   Windows 11 récentes et quasi systématique en parc géré, bloque le chargement de ce type
   de pilote. Elle est désactivée sur la machine de dev, ce qui explique que tout y
   fonctionne sans effort.
2. **Installer un outil de monitoring demande l'admin.**
3. **Modifier le schéma d'alimentation demande l'admin.**
4. **Les stratégies de groupe verrouillent souvent les schémas.** Clés à vérifier :
   `HKLM\SYSTEM\CurrentControlSet\Control\StorageDevicePolicies` et
   `HKLM\SOFTWARE\Policies\Microsoft\Windows\RemovableStorageDevices` pour les médias,
   et les stratégies d'alimentation dédiées.

Attendu sur un poste managé : **lecture seule, sans température**. Ce n'est pas une limite
à contourner — c'est le comportement d'une machine correctement administrée, et le
contourner serait précisément ce qu'on reproche aux « optimiseurs » du commerce.

Le modèle de capacités est la réponse : chaque absence porte sa raison, aucune fonction
n'est grisée sans explication.

## Ce qu'il reste à faire pour distribuer

**Signer l'exécutable.** Sans quoi SmartScreen dissuadera les destinataires d'installer.

**Élévation ciblée.** Aujourd'hui l'application entière doit tourner élevée pour écrire le
schéma, donc une webview complète en administrateur. Préférable : lancer non élevé, et
n'élever qu'un petit assistant au moment de l'écriture. C'est le chantier structurant
restant côté distribution.

**Ne jamais appliquer sans demander.** Le bridage modifie la configuration système de
quelqu'un d'autre. Toujours explicite, toujours réversible, et le nom du schéma affecté
toujours affiché.
