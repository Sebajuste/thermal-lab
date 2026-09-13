// Pas de console au lancement en release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    thermal_lab_lib::run()
}
