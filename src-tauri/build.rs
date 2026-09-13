fn main() {
    let mut attributes = tauri_build::Attributes::new();

    // Le manifeste qui reclame l'elevation n'est pose qu'en release.
    //
    // En developpement, `tauri dev` relance le binaire a chaque modification du Rust :
    // une invite UAC par redemarrage rendrait la boucle inutilisable. Lancer la session
    // de developpement depuis un terminal deja eleve donne le meme resultat sans le
    // manifeste, puisque le processus herite du jeton de son parent.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        attributes = attributes.windows_attributes(
            tauri_build::WindowsAttributes::new()
                .app_manifest(include_str!("windows-app-manifest.xml")),
        );
    }

    tauri_build::try_build(attributes).expect("erreur de tauri-build");
}
