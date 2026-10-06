#!/bin/bash
#
# Installe Souffle sur ce Mac. Double-cliquez sur ce fichier.
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
│                                              │
│   Dictée vocale : un raccourci, vous parlez, │
│   le texte propre s'écrit là où vous tapez.  │
│                                              │
└──────────────────────────────────────────────┘
BANNIERE

titre "1. Vérifications"

[ "$(uname -s)" = "Darwin" ] || stop "Ce fichier est destiné à un Mac. Sur un PC, utilisez « Installer sur ce PC.bat »."
[ -f "application/package.json" ] || stop "Le dossier « application » est introuvable à côté de ce fichier. Décompressez l'archive entièrement avant de lancer l'installation."

if ! command -v node >/dev/null 2>&1; then
  printf '\n'
  printf '  Node.js est nécessaire pour construire l'"'"'application, et il n'"'"'est pas installé.\n\n'
  printf '  1. Rendez-vous sur   \033[1mhttps://nodejs.org\033[0m\n'
  printf '  2. Téléchargez la version \033[1mLTS\033[0m, installez-la (suivant, suivant…)\n'
  printf '  3. Revenez ici et double-cliquez de nouveau sur ce fichier\n\n'
  stop "Installation interrompue : Node.js manquant."
fi

ARCHI="$(uname -m)"
case "$ARCHI" in
  arm64)  CIBLE="--arm64"; NOM="Apple Silicon" ;;
  x86_64) CIBLE="--x64";   NOM="Intel" ;;
  *)      CIBLE="";        NOM="$ARCHI" ;;
esac

ok "macOS $(sw_vers -productVersion 2>/dev/null || echo '?') · Mac $NOM"
ok "Node.js $(node -v)"

titre "2. Construction de l'application"
printf '  Comptez deux à cinq minutes. La première fois, un composant de 150 Mo\n'
printf '  est téléchargé — c'"'"'est normal, cela n'"'"'arrive qu'"'"'une seule fois.\n\n'

cd application
npm install --no-audit --no-fund
npx electron-builder --mac dir $CIBLE

# electron-builder range la version Intel dans dist/mac et l'Apple Silicon dans
# dist/mac-arm64 : on prend la première qui existe, sans dépendre de `find`.
APP=""
for candidat in dist/mac*/Souffle.app; do
  if [ -d "$candidat" ]; then APP="$candidat"; break; fi
done
[ -n "$APP" ] || stop "L'application n'a pas été produite. Les messages ci-dessus disent pourquoi."
ok "application construite"

titre "3. Installation dans le dossier Applications"

# Souffle refuse de tourner en double : si l'ancienne version tourne encore, la
# nouvelle se fermerait aussitôt au lancement et rouvrirait simplement les
# réglages de l'ancienne. On la quitte d'abord, poliment puis fermement.
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
  souci "une version de Souffle est déjà installée"
  printf '\n  La remplacer ? [o/N] '
  read -r reponse
  case "$reponse" in
    [oOyY]*) rm -rf "/Applications/Souffle.app" ;;
    *) stop "Installation annulée. Rien n'a été modifié." ;;
  esac
fi

ditto "$APP" "/Applications/Souffle.app"
ok "Souffle installé dans Applications"

titre "4. Premier lancement"
open "/Applications/Souffle.app" || true

cat <<'SUITE'

  Souffle vit maintenant dans la barre de menus, en haut à droite.

  Il reste deux choses à faire, une seule fois :

    • Coller une clé API dans la fenêtre de réglages qui vient de s'ouvrir
      (onglet Intelligence). Sans elle, Souffle ne peut rien transcrire.

    • Autoriser l'Accessibilité : Réglages Système → Confidentialité et
      sécurité → Accessibilité → cocher Souffle. C'est ce qui lui permet
      de coller le texte dans l'application où vous écrivez.

  Le micro, lui, sera demandé tout seul à la première dictée.

  Raccourci : ⌥ Espace  (Option + Espace)
  Appuyez, parlez, ré-appuyez. Le texte s'écrit là où était votre curseur.

  Tout est expliqué dans « 2 — Installation.md » et
  « 1 — À quoi sert Souffle.md », à côté de ce fichier.

SUITE

printf '\033[1m✓ Terminé.\033[0m Cette fenêtre peut être fermée.\n\n'
