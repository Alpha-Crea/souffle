#!/bin/bash
#
# Installe Souffle (version native) sur ce Mac. Double-cliquez sur ce fichier.
#
# Souffle est construit ici même, pour ce Mac précis. La première fois, le
# script installe ce qu'il faut pour cela (outils de développement d'Apple,
# Rust) : comptez 10 à 20 minutes. Les fois suivantes, deux ou trois minutes.
#
set -euo pipefail
cd "$(dirname "$0")"

titre() { printf '\n\033[1m%s\033[0m\n' "$1"; }
ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
souci() { printf '  \033[33m!\033[0m %s\n' "$1"; }
stop()  { printf '\n\033[31m✗ %s\033[0m\n\n' "$1"; printf 'Cette fenêtre peut être fermée.\n\n'; exit 1; }

clear
cat <<'BANNIERE'
┌──────────────────────────────────────────────┐
│                                              │
│   Souffle — installation sur ce Mac          │
│   version native                             │
│                                              │
│   Dictée vocale : un raccourci, vous parlez, │
│   le texte propre s'écrit là où vous tapez.  │
│                                              │
└──────────────────────────────────────────────┘
BANNIERE

titre "1. Vérifications"

[ "$(uname -s)" = "Darwin" ] || stop "Ce fichier est destiné à un Mac. Sur un PC, ouvrez le dossier « Windows »."
[ -f "application/src-tauri/Cargo.toml" ] || stop "Le dossier « application » est introuvable à côté de ce fichier. Décompressez l'archive entièrement avant de lancer l'installation."

MAJEURE="$(sw_vers -productVersion | cut -d. -f1)"
[ "$MAJEURE" -ge 11 ] || stop "Souffle demande macOS 11 (Big Sur) ou plus récent."
ok "macOS $(sw_vers -productVersion) · $(uname -m)"

# --- Outils de développement d'Apple (compilateur, signature) ---------------
if ! xcode-select -p >/dev/null 2>&1; then
  souci "les outils de développement d'Apple sont nécessaires"
  printf '\n  Une fenêtre d'"'"'Apple va s'"'"'ouvrir : cliquez sur « Installer », acceptez,\n'
  printf '  puis patientez (5 à 10 minutes). Ce script reprend tout seul ensuite.\n\n'
  xcode-select --install >/dev/null 2>&1 || true
  for _ in $(seq 1 360); do
    xcode-select -p >/dev/null 2>&1 && break
    sleep 5
  done
  xcode-select -p >/dev/null 2>&1 || stop "Les outils d'Apple ne sont pas installés. Relancez ce fichier une fois l'installation d'Apple terminée."
fi
ok "outils de développement d'Apple"

# --- Rust ---------------------------------------------------------------------
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null 2>&1; then
  souci "installation de Rust (une seule fois, environ 2 minutes)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null \
    || stop "Rust n'a pas pu être installé. Vérifiez la connexion Internet, puis relancez."
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi
ok "Rust $(rustc --version | awk '{print $2}')"

# --- Outil de construction Tauri ----------------------------------------------
cd application
if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
  ok "Node.js $(node -v)"
  npm install --no-audit --no-fund --silent >/dev/null 2>&1 || npm install --no-audit --no-fund
  TAURI=(npx tauri)
else
  if ! command -v cargo-tauri >/dev/null 2>&1; then
    souci "installation de l'outil Tauri (une seule fois, 5 à 8 minutes)"
    cargo install tauri-cli --version '^2' --locked
  fi
  TAURI=(cargo tauri)
fi

titre "2. Construction de Souffle"
printf '  La première fois : 5 à 10 minutes. Ensuite : deux ou trois.\n\n'
"${TAURI[@]}" build --bundles app

APP="src-tauri/target/release/bundle/macos/Souffle.app"
[ -d "$APP" ] || stop "L'application n'a pas été produite. Les messages ci-dessus disent pourquoi."
ok "Souffle construit"

titre "3. Installation dans le dossier Applications"

# Une seule instance à la fois : l'ancienne version (Electron ou native) est
# quittée d'abord, poliment puis fermement.
souffle_tourne() { pgrep -f "Souffle.app/Contents/MacOS" >/dev/null 2>&1; }
if souffle_tourne; then
  souci "Souffle est ouvert : fermeture de l'ancienne version"
  osascript -e 'tell application "Souffle" to quit' >/dev/null 2>&1 || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if ! souffle_tourne; then break; fi
    sleep 1
  done
  if souffle_tourne; then pkill -f "Souffle.app/Contents/MacOS" || true; sleep 1; fi
  if souffle_tourne; then stop "Impossible de fermer Souffle. Quittez-le depuis la barre de menus, puis relancez ce fichier."; fi
  ok "ancienne version fermée"
fi

if [ -e "/Applications/Souffle.app" ]; then
  if [ -d "/Applications/Souffle.app/Contents/Resources/app.asar" ] || [ -f "/Applications/Souffle.app/Contents/Resources/app.asar" ]; then
    souci "l'ancienne version (Electron) est remplacée ; vos réglages et votre historique sont repris"
  else
    souci "la version installée est remplacée"
  fi
  rm -rf "/Applications/Souffle.app"
fi
ditto "$APP" "/Applications/Souffle.app"
ok "Souffle installé dans Applications"

# La nouvelle application n'a pas la même signature que l'ancienne : les
# anciennes autorisations ne s'appliquent plus et resteraient cochées pour
# rien. On les efface, macOS les redemandera proprement.
tccutil reset Accessibility fr.alphagency.souffle >/dev/null 2>&1 || true
tccutil reset Microphone fr.alphagency.souffle >/dev/null 2>&1 || true

titre "4. Premier lancement"
open "/Applications/Souffle.app" || true

cat <<'SUITE'

  Souffle vit maintenant dans la barre de menus, en haut à droite
  (l'icône de la bulle avec l'onde).

  À faire une seule fois :

    • Collez votre clé API dans la fenêtre de réglages (Intelligence).
      L'ancienne était chiffrée par la version précédente et ne peut pas
      être relue. Vos autres réglages et votre historique sont repris.

    • Autorisez l'Accessibilité quand macOS le demande (Réglages Système →
      Confidentialité et sécurité → Accessibilité → Souffle). C'est ce qui
      permet de coller le texte là où vous écrivez.

    • Le micro sera demandé à la première dictée.

  Raccourci : ⌥ Espace (Option + Espace), inchangé.
  Pendant la dictée : clic sur la bulle = pause, clic droit = annuler,
  glisser = la déplacer.

SUITE
printf 'Cette fenêtre peut être fermée.\n\n'
