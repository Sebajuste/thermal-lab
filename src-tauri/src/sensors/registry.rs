//! Composition du pipeline : quels fournisseurs, et dans quel ordre.
//!
//! **L'ordre est la priorite.** `Reading::offer` retient la premiere valeur proposee
//! pour une grandeur, donc un fournisseur place plus haut ne peut pas etre ecrase par un
//! suivant. Ajouter une source revient a inserer une ligne ici, au bon rang.
//!
//! Le classement suit la fidelite de la mesure :
//!
//! 1. les outils a pilote noyau, seuls a lire les registres MSR (temperature de die,
//!    puissance package), Core Temp avant HWiNFO car il ne demande aucun reglage ;
//! 2. le GPU AMD via LibreHardwareMonitor, seule source de ses grandeurs ;
//! 3. les compteurs Windows et `nvidia-smi`, qui ne recouvrent pas les precedents ;
//! 4. la zone ACPI, qui ne mesure pas le CPU et n'alimente qu'une metrique distincte.
//!
//! `amd-gpu` passe avant `nvidia-smi` par contrainte de rang — les sources externes
//! precedent les integrees — et non parce qu'elle serait plus fidele. Les deux ne se
//! disputent les memes grandeurs que sur une machine melant iGPU Radeon et carte NVIDIA,
//! cas que ce rang unique ne sait pas arbitrer.

use super::provider::Provider;
use super::providers::{
    acpi::AcpiProvider, amd_gpu::AmdGpuProvider, core_temp::CoreTempProvider,
    hwinfo::HwInfoProvider, libre_hw::LibreHwProvider, nvidia::NvidiaProvider,
    perf_counters::PerfCountersProvider,
};

pub fn build() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(CoreTempProvider::new()),
        Box::new(HwInfoProvider::new()),
        Box::new(LibreHwProvider::new()),
        Box::new(AmdGpuProvider::new()),
        Box::new(PerfCountersProvider::new()),
        Box::new(NvidiaProvider::new()),
        Box::new(AcpiProvider::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::provider::ProviderKind;
    use std::collections::HashSet;

    #[test]
    fn identifiers_are_unique() {
        let ids: Vec<_> = build().iter().map(|p| p.info().id).collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "identifiants de fournisseurs dupliques");
    }

    #[test]
    fn every_provider_declares_what_it_measures() {
        for p in build() {
            let info = p.info();
            assert!(!info.provides.is_empty(), "{} ne declare aucune metrique", info.id);
            assert!(!info.name.is_empty());
        }
    }

    /// Un fournisseur externe sans lien laisse l'utilisateur sans recours quand il
    /// manque : c'est precisement ce que le modele de capacites doit eviter.
    #[test]
    fn external_providers_tell_where_to_get_them() {
        for p in build() {
            let info = p.info();
            if info.kind == ProviderKind::External {
                assert!(info.url.is_some(), "{} est externe sans URL", info.id);
            }
        }
    }

    /// Les sources a pilote noyau doivent preceder les autres, sans quoi une mesure
    /// approchee pourrait occuper la place d'une mesure exacte.
    #[test]
    fn kernel_backed_providers_come_first() {
        let kinds: Vec<_> = build().iter().map(|p| p.info().kind).collect();
        let last_external = kinds.iter().rposition(|k| *k == ProviderKind::External);
        let first_builtin = kinds.iter().position(|k| *k == ProviderKind::Builtin);
        assert!(last_external < first_builtin, "ordre de priorite incoherent");
    }
}
