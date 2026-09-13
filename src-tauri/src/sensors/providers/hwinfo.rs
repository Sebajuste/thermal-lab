//! Pilote HWiNFO — memoire partagee `HWiNFO_SENS_SM2`.
//!
//! HWiNFO publie un tableau plat de releves decrits par un en-tete : on y trouve les
//! decalages, la taille d'un element et leur nombre. On ne suppose donc pas la taille
//! des elements, on lit celle que l'en-tete annonce.
//!
//! Cote utilisateur, HWiNFO doit tourner **et** avoir « Shared Memory Support » actif
//! dans ses reglages : il est desactive par defaut.

use crate::sensors::metric::{Metric, Reading};
use crate::sensors::provider::{
    ProbeContext, ProbeState, Provider, ProviderInfo, ProviderKind, Sampled,
};
use crate::sensors::shared_memory::{c_string, MappedView};

const ID: &str = "hwinfo";
const SECTION: &str = "Global\\HWiNFO_SENS_SM2";
const PROVIDES: &[Metric] = &[Metric::CpuTempC, Metric::CpuPowerW];

/// Signature "SiWH" en little-endian.
const SIGNATURE: u32 = 0x4857_6953;

/// Garde-fou : au-dela, l'en-tete est incoherent et on refuse de boucler.
const MAX_ELEMENTS: u32 = 8192;

const READING_TYPE_TEMP: u32 = 1;
const READING_TYPE_POWER: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    signature: u32,
    version: u32,
    revision: u32,
    poll_time: i64,
    sensor_offset: u32,
    sensor_element_size: u32,
    sensor_count: u32,
    reading_offset: u32,
    reading_element_size: u32,
    reading_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ReadingElement {
    reading_type: u32,
    sensor_index: u32,
    reading_id: u32,
    label_orig: [u8; 128],
    label_user: [u8; 128],
    unit: [u8; 16],
    value: f64,
    value_min: f64,
    value_max: f64,
    value_avg: f64,
}

#[derive(Default)]
pub struct HwInfoProvider {
    available: bool,
}

impl HwInfoProvider {
    pub fn new() -> Self {
        Self::default()
    }

    fn header(view: &MappedView) -> Option<Header> {
        let h = view.read_struct::<Header>(0)?;
        (h.signature == SIGNATURE && h.reading_count <= MAX_ELEMENTS).then_some(h)
    }
}

impl Provider for HwInfoProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: "HWiNFO",
            kind: ProviderKind::External,
            provides: PROVIDES,
            url: Some("https://www.hwinfo.com/"),
        }
    }

    fn probe(&mut self, _ctx: &ProbeContext<'_>) -> ProbeState {
        self.available = false;

        let Some(view) = MappedView::open(SECTION) else {
            return ProbeState::unavailable(
                "HWiNFO ne tourne pas, ou sa memoire partagee est desactivee",
                "Dans HWiNFO : Settings > Main Settings > cocher « Shared Memory Support ».",
            );
        };

        match Self::header(&view) {
            None => ProbeState::Failed {
                error: "signature ou en-tete inattendu dans la section partagee".into(),
            },
            Some(_) => {
                self.available = true;
                ProbeState::Ready
            }
        }
    }

    fn sample(&mut self, out: &mut Reading) -> Sampled {
        if !self.available {
            return Sampled::Lost;
        }
        let Some(view) = MappedView::open(SECTION) else {
            return Sampled::Lost;
        };
        let Some(h) = Self::header(&view) else {
            return Sampled::Lost;
        };

        for i in 0..h.reading_count {
            let offset = h.reading_offset as usize + (i as usize) * h.reading_element_size as usize;
            let Some(e) = view.read_struct::<ReadingElement>(offset) else {
                break;
            };
            if !e.value.is_finite() {
                continue;
            }

            // Le libelle utilisateur est renommable ; l'original ne l'est pas.
            let label = c_string(&e.label_orig).to_lowercase();

            match e.reading_type {
                READING_TYPE_TEMP if label.contains("cpu package") => {
                    out.offer(Metric::CpuTempC, e.value, ID);
                }
                READING_TYPE_POWER if label.contains("cpu package power") => {
                    out.offer(Metric::CpuPowerW, e.value, ID);
                }
                _ => {}
            }
        }

        Sampled::Answered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La disposition doit correspondre a celle publiee par HWiNFO : 12 octets de
    /// champs, trois chaines fixes, puis quatre doubles alignes sur 8.
    #[test]
    fn reading_element_layout_matches_published_struct() {
        assert_eq!(std::mem::size_of::<ReadingElement>(), 320);
        assert_eq!(std::mem::align_of::<ReadingElement>(), 8);
    }

    #[test]
    fn header_layout_accounts_for_time64_alignment() {
        // 3 DWORD, 4 octets de bourrage, __time64_t, puis 6 DWORD.
        assert_eq!(std::mem::size_of::<Header>(), 48);
    }

    #[test]
    fn signature_decodes_as_siwh() {
        assert_eq!(&SIGNATURE.to_le_bytes(), b"SiWH");
    }
}
