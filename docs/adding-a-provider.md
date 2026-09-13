# Écrire un pilote de capteur

Ajouter une source se fait en deux gestes : un fichier dans `sensors/providers/`, une
ligne dans `registry::build()`. Rien d'autre ne bouge — ni le relevé, ni l'UI, ni le
modèle de capacités.

## Squelette

```rust
//! Pilote <nom> — <mécanisme d'accès>.
//!
//! <ce que l'utilisateur doit avoir fait pour que ça marche>

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind};

const ID: &str = "mon-outil";
const PROVIDES: &[Metric] = &[Metric::CpuTempC];

#[derive(Default)]
pub struct MonProvider {
    /* l'état établi au probe : connexion, handle, nom de section… */
}

impl MonProvider {
    pub fn new() -> Self { Self::default() }
}

impl Provider for MonProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "Mon Outil",
            kind: ProviderKind::External,
            provides: PROVIDES,
            url: Some("https://…"),   // obligatoire si External — un test le vérifie
        }
    }

    fn probe(&mut self, ctx: &ProbeContext<'_>) -> ProbeState { /* … */ }

    fn sample(&mut self, out: &mut Reading) {
        out.offer(Metric::CpuTempC, valeur, ID);
    }
}
```

Puis dans `sensors/providers/mod.rs` et `registry::build()`, au rang correspondant à sa
fiabilité.

## Les règles qui comptent

**`provides` doit être exact.** C'est lui qui alimente le modèle de capacités : déclarer
une grandeur qu'on ne fournit pas fait mentir l'UI, qui dira à l'utilisateur d'installer
un outil qui ne l'aidera pas.

**Ne pas revendiquer ce qu'une meilleure source couvre déjà.** HWiNFO et
LibreHardwareMonitor savent lire le GPU, mais ne déclarent que les métriques CPU :
`nvidia-smi` est plus fiable et toujours présent avec le pilote. C'est un choix délibéré,
pas un oubli — sans quoi l'ordre global du registre devrait arbitrer CPU et GPU
simultanément, ce qu'un simple rang ne permet pas.

**Une source, plusieurs pilotes, quand les prérequis diffèrent.** `amd_gpu.rs` interroge
le même WMI que `libre_hw.rs` mais reste un fichier séparé : il ne mesure rien sans carte
Radeon, et `provides` doit dire exactement cela. Les fusionner ferait promettre à l'UI des
grandeurs GPU sur une machine qui n'en a pas, ou taire la température CPU sur une machine
sans Radeon.

**`sample()` ne se soucie pas des conflits.** Il propose, `offer` arbitre. Ne jamais
tester si une valeur est déjà présente.

**Distinguer `Unavailable` de `Failed`.** L'outil n'est pas lancé → `Unavailable` avec un
`hint` actionnable. La section existe mais la structure est illisible → `Failed`, qui
signale un vrai bug. Confondre les deux fait passer une régression pour une absence
banale.

**`probe()` est rappelé.** Toutes les 5 boucles tant que l'état n'est pas `Ready`. Il doit
donc être idempotent et bon marché ; ne pas y faire de travail lourd.

## Les deux mécanismes déjà outillés

### Mémoire partagée

`shared_memory::MappedView::open(nom)` mappe une section nommée en lecture seule et la
démappe à la destruction. `read_struct::<T>(offset)` copie une structure après vérification
des bornes, en lecture non alignée.

```rust
let view = MappedView::open("MaSectionNommee")?;
let header = view.read_struct::<Header>(0)?;
```

`T` doit décrire **exactement** la disposition publiée par l'outil. Deux garde-fous à
reprendre systématiquement :

- un test sur `size_of::<T>()` et `align_of::<T>()`, qui attrape une erreur de structure
  sans avoir l'outil installé (voir `hwinfo.rs`) ;
- une validation à l'exécution dans `probe()` : signature attendue, ou champ texte
  lisible. `core_temp.rs` rejette un nom de CPU vide — un décalage d'un seul octet suffit
  à produire du charabia et des températures aberrantes.

### WMI

`ProbeContext::wmi` porte le contexte COM du thread d'échantillonnage. Ne jamais appeler
`COMLibrary::new()` depuis un pilote : COM s'initialise une fois par thread, et c'est le
hub qui s'en charge.

```rust
let Some(wmi) = ctx.wmi else { return ProbeState::Failed { error: "COM indisponible".into() } };
let con = wmi.connect("root\\cimv2").map_err(…)?;
let rows = con.raw_query::<Row>(QUERY)?;
```

`variant_f64` accepte tout ce qui se ramène à un nombre : WMI type ses colonnes selon la
classe interrogée, et pas toujours comme la documentation le laisse croire.

## Ajouter une grandeur

Si la source mesure quelque chose de réellement nouveau, il faut toucher `metric.rs` :
ajouter la variante, l'inscrire dans `Metric::ALL`, et lui donner une unité dans `unit()`.
Le reste suit — les capacités et l'UI itèrent sur `ALL`.

Ne pas créer une métrique pour une variante de la même grandeur. `BoardTempC` existe
séparément de `CpuTempC` non par confort mais parce qu'une zone ACPI **ne mesure pas** le
die : les confondre laisserait une source approchée se substituer à une mesure exacte.
