// Pas de console noire derrière l'application sous Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    souffle_lib::run();
}
