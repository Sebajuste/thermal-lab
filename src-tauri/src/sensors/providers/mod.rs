//! Les pilotes concrets. Chacun ignore l'existence des autres.

pub mod acpi;
pub mod amd_gpu;
pub mod core_temp;
pub mod hwinfo;
pub mod libre_hw;
pub mod nvidia;
pub mod perf_counters;
