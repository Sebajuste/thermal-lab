//! Mappage en lecture seule d'un objet de memoire partagee nomme.
//!
//! Plusieurs outils de monitoring publient leurs releves ainsi plutot que par WMI :
//! c'est leur interface d'integration documentee. Ce module ne connait aucun d'eux, il
//! ne sait qu'ouvrir une section nommee et en extraire des structures a un decalage.

use std::ffi::c_void;
use std::mem::{size_of, MaybeUninit};
use std::ptr;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ,
    MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS,
};

/// Une vue mappee, demappee a la destruction.
pub struct MappedView {
    handle: *mut c_void,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    len: usize,
}

// La vue est un bloc memoire en lecture seule sans etat partage cote Rust : elle peut
// traverser les threads avec sa structure proprietaire.
unsafe impl Send for MappedView {}

impl MappedView {
    /// Ouvre la section nommee et mappe l'integralite de son contenu.
    ///
    /// Renvoie `None` si la section n'existe pas — cas nominal quand l'outil qui la
    /// publie n'est pas lance.
    pub fn open(name: &str) -> Option<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY : le handle est verifie non nul avant usage, la vue avant lecture, et
        // les deux sont liberes sur chaque chemin de sortie (ici et dans Drop).
        unsafe {
            let handle = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
            if handle.is_null() {
                return None;
            }

            // Taille 0 : mappe toute la section, dont on ignore la taille a priori.
            let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }

            let mut mbi = MaybeUninit::<MEMORY_BASIC_INFORMATION>::zeroed();
            let written = VirtualQuery(
                view.Value,
                mbi.as_mut_ptr(),
                size_of::<MEMORY_BASIC_INFORMATION>(),
            );
            if written == 0 {
                UnmapViewOfFile(view);
                CloseHandle(handle);
                return None;
            }

            Some(Self {
                handle,
                view,
                len: mbi.assume_init().RegionSize,
            })
        }
    }

    /// Copie une structure situee au decalage donne, apres verification des bornes.
    ///
    /// `T` doit decrire exactement la disposition memoire publiee par l'outil : c'est la
    /// responsabilite de l'appelant, et la raison pour laquelle chaque pilote valide ses
    /// lectures (nom de CPU lisible, valeurs dans des plages physiques).
    pub fn read_struct<T: Copy>(&self, offset: usize) -> Option<T> {
        let end = offset.checked_add(size_of::<T>())?;
        if end > self.len {
            return None;
        }
        // SAFETY : bornes verifiees ci-dessus ; lecture non alignee, donc sans
        // hypothese sur l'alignement du decalage.
        unsafe { Some(ptr::read_unaligned(self.view.Value.add(offset) as *const T)) }
    }
}

impl Drop for MappedView {
    fn drop(&mut self) {
        // SAFETY : handle et vue proviennent d'un `open` reussi et ne sont liberes qu'ici.
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}

/// Decode une chaine C de longueur fixe, telle qu'on en trouve dans ces structures.
pub fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fixed_width_c_strings() {
        assert_eq!(c_string(b"Intel Core i9\0\0\0\0"), "Intel Core i9");
        assert_eq!(c_string(b"sans terminateur"), "sans terminateur");
        assert_eq!(c_string(b"\0"), "");
        assert_eq!(c_string(b"  espaces  \0"), "espaces");
    }

    #[test]
    fn missing_section_is_not_an_error() {
        assert!(MappedView::open("ClaudeSectionQuiNExistePas_0000").is_none());
    }
}
