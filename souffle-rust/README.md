# Souffle — version native (Rust + Tauri)

La même dictée que la version Electron, réécrite pour être légère : quelques mégaoctets au lieu
de 250, le moteur web du système au lieu d'un Chromium embarqué, la capture micro en Rust au
lieu d'une fenêtre cachée, et le verre de la bulle dessiné par macOS.

```
src-tauri/src/
  engine.rs        machine à états : pause, file de sécurité, réessai, mode direct (testé de bout en bout)
  ai.rs            transcription + reformulation, garde-fou, bascule Groq ↔ OpenAI
  audio/           capture micro (cpal), découpage des phrases, WAV, sons
  pending.rs       file de sécurité : audio gardé jusqu'à l'historique, écrit au fil de l'eau
  history.rs       historique local
  config.rs        réglages (mêmes clés que la version Electron) + clés dans le trousseau
  inject.rs        presse-papier + ⌘V / Ctrl+V
  platform/        macOS (Accessibilité, verre, Échap) et Windows (focus, fenêtre sans focus)
  bubble.rs        la bulle : position, déplacement, tous les bureaux
  tray.rs          barre de menus / zone de notification
  shortcuts.rs     raccourci global, maintien, Échap
  commands.rs      commandes des pages
ui/                bulle et réglages (HTML/CSS/JS sans outil de construction)
distribution/      notice et installateur Mac
```

## Développer

```bash
npm install
npx tauri dev          # lance Souffle
cd src-tauri && cargo test   # 37 tests, dont les scénarios complets du moteur
```

## Construire

- **Mac** : `npx tauri build --bundles app` (ou double-clic sur `distribution/Installer sur ce Mac.command`
  depuis le paquet de distribution).
- **Windows**, depuis Linux : `npx tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`
  (nécessite `cargo install cargo-xwin`, `clang-cl`, `lld-link`, `llvm-rc` et `makensis`).
  Sur Windows : `npx tauri build`.

Au premier lancement, les réglages, l'historique et les dictées en attente de la version Electron
sont repris. Les clés API, chiffrées par Electron, sont à recoller.
