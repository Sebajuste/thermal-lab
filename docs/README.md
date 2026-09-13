# Documentation — Thermal Lab

POC Tauri mesurant l'effet réel d'un bridage CPU sur les températures et la consommation,
avec bascule en direct pour comparer à charge identique.

## Par où entrer

| Document | Pour |
|---|---|
| [architecture.md](architecture.md) | comprendre le pipeline et le flux de données |
| [background-ux.md](background-ux.md) | le mode résident : icône, panneau, démarrage |
| [adding-a-provider.md](adding-a-provider.md) | écrire un nouveau pilote de capteur |
| [measurement.md](measurement.md) | savoir ce que valent les chiffres affichés |
| [third-party-tools.md](third-party-tools.md) | détecter, situer et lancer les outils tiers |
| [power-control.md](power-control.md) | le mécanisme de bridage et ses pièges |
| [distribution.md](distribution.md) | ce qui bloque hors de la machine de dev |
| [development.md](development.md) | commandes, tests, dépannage |

## Diagrammes

Les schémas sont en Mermaid, dans le corps des documents qu'ils illustrent — diffables
comme du texte, sans image à régénérer. GitHub les rend nativement ; dans VS Code, il faut
l'extension *Markdown Preview Mermaid Support*.

Seul le pipeline de mesure est diagrammé : c'est la partie stable. Le châssis résident
— icône, panneau, cycle de vie — attend la fin de sa refonte, un schéma faux étant pire
que pas de schéma.

## Le sujet en trois phrases

Les « optimiseurs thermiques » du commerce désactivent le Turbo Boost via le schéma
d'alimentation de Windows — rien de plus. Sur un i9-14900K, cela fait passer le processeur
de ~5,7 GHz / 250-350 W à 3,2 GHz / ~80-100 W, pour une perte d'images par seconde
marginale en jeu, où la charge est limitée par le GPU.

Cette application expose ce levier **et le mesure des deux côtés**, pour que le gain soit
constaté plutôt que supposé.

## État actuel

Fonctionnel : lecture des capteurs (6 fournisseurs), bascule du bridage, comparaison de
phases, modèle de capacités, mode résident dans la zone de notification, détection et
lancement des outils tiers. 37 tests, aucun avertissement de compilation.

Non couvert, par ordre d'intérêt :

1. **Mesure des images par seconde** — c'est la moitié manquante de la démonstration :
   on prouve la baisse de consommation, pas l'absence de coût. Demande un overlay
   type RTSS ou un hook de présentation.
2. **Signature du binaire et élévation ciblée** — voir [distribution.md](distribution.md).
3. **Multi-GPU** — `providers/nvidia.rs` et `providers/amd_gpu.rs` ne lisent chacun que
   la première carte, et le rang unique du registre ne sait pas arbitrer entre elles sur
   une machine mixte. Côté AMD, tout passe par LibreHardwareMonitor : il n'existe pas
   d'équivalent de `nvidia-smi` livré avec le pilote, seul l'ADLX natif s'en approcherait.
4. **Persistance des sessions de mesure** — les moyennes survivent au masquage du
   panneau, pas à la sortie de l'application.

## Convention

Un fichier, une responsabilité. Elle tient jusqu'ici et vaut pour les deux côtés de
l'application ; `sensors/providers/` en est l'illustration la plus stricte, chaque pilote
ignorant l'existence des autres.
