#!/usr/bin/env bash
#
# Assemble sur le Bureau un dossier « Souffle » prêt à donner à quelqu'un :
#
#   ~/Desktop/Souffle/
#     1 — À quoi sert Souffle.md
#     2 — Installation.md
#     macOS/Souffle — Mac Apple Silicon (M1 M2 M3 M4).dmg
#     macOS/Souffle — Mac Intel.dmg
#     Windows/Souffle — Windows.exe
#   ~/Desktop/Souffle.zip        (le même dossier, compressé)
#
# Les fichiers .dmg sont construits ici même (macOS obligatoire pour ça).
# L'installateur Windows est récupéré depuis les Releases du dépôt : il est
# construit automatiquement à chaque fusion sur la branche principale.
#
# Usage :  bash distribution/preparer-le-dossier.sh
#
set -euo pipefail

PROJET="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUREAU="$HOME/Desktop"
DEST="$BUREAU/Souffle"
ZIP="$BUREAU/Souffle.zip"
DEPOT="${SOUFFLE_DEPOT:-Alpha-Crea/souffle}"

gras()  { printf '\n\033[1m▸ %s\033[0m\n' "$1"; }
ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
souci() { printf '  \033[33m!\033[0m %s\n' "$1"; }

trap 'printf "\n\033[31m✗ Arrêt à l étape ci-dessus.\033[0m Rien de plus n a été écrit sur le Bureau.\n"' ERR

# ------------------------------------------------------------------ #
gras "Vérifications"

command -v node >/dev/null 2>&1 || {
  echo "Node.js est introuvable. Installez-le depuis nodejs.org, puis relancez."
  exit 1
}
[ -f "$PROJET/package.json" ] || {
  echo "Ce script doit rester dans le dossier « distribution » du projet Souffle."
  exit 1
}
[ -d "$BUREAU" ] || {
  echo "Bureau introuvable ($BUREAU). Définissez BUREAU dans le script si le vôtre est ailleurs."
  exit 1
}

VERSION="$(node -p "require('$PROJET/package.json').version")"
ok "projet  : $PROJET"
ok "version : $VERSION"

# ------------------------------------------------------------------ #
gras "Dossier de destination"

# On ne supprime jamais : un dossier ou une archive qui existe déjà est mis de
# côté sous un nom daté. Personne ne perd son travail à cause d'un script.
HORODATE="$(date '+%Y-%m-%d à %Hh%M')"
[ -e "$DEST" ] && { mv "$DEST" "$BUREAU/Souffle (remplacé le $HORODATE)"; souci "dossier précédent renommé en « Souffle (remplacé le $HORODATE) »"; }
[ -e "$ZIP" ]  && { mv "$ZIP"  "$BUREAU/Souffle (remplacé le $HORODATE).zip"; souci "archive précédente renommée"; }

mkdir -p "$DEST/macOS" "$DEST/Windows"
ok "$DEST"

# ------------------------------------------------------------------ #
gras "Application macOS"

if [ "$(uname -s)" = "Darwin" ]; then
  [ -d "$PROJET/node_modules" ] || {
    souci "dépendances absentes, installation (quelques minutes)…"
    (cd "$PROJET" && npm install --no-audit --no-fund)
  }
  souci "construction en cours, comptez deux à cinq minutes…"
  (cd "$PROJET" && npm run dist:mac)

  trouves=0
  for dmg in "$PROJET"/dist/*.dmg; do
    [ -e "$dmg" ] || continue
    case "$(basename "$dmg")" in
      *arm64*)        cible="$DEST/macOS/Souffle — Mac Apple Silicon (M1 M2 M3 M4).dmg" ;;
      *x64*|*intel*)  cible="$DEST/macOS/Souffle — Mac Intel.dmg" ;;
      *)              cible="$DEST/macOS/$(basename "$dmg")" ;;
    esac
    cp "$dmg" "$cible"
    ok "$(basename "$cible")"
    trouves=$((trouves + 1))
  done

  [ "$trouves" -gt 0 ] || souci "aucun .dmg trouvé dans dist/ — relisez les messages d'electron-builder ci-dessus"
else
  souci "vous n'êtes pas sur un Mac ($(uname -s)) : un .dmg ne peut être construit que sur macOS"
  cat > "$DEST/macOS/À LIRE.txt" <<'NOTE'
L'application macOS n'a pas pu être construite : elle exige un Mac.

Sur un Mac, dans le dossier du projet :

    npm install
    npm run dist:mac

Les fichiers .dmg apparaissent alors dans le sous-dossier « dist ».
NOTE
fi

# ------------------------------------------------------------------ #
gras "Installateur Windows"

EXE="$DEST/Windows/Souffle — Windows.exe"
recupere=0

if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  if gh release download --repo "$DEPOT" --pattern '*.exe' --dir "$DEST/Windows" --clobber >/dev/null 2>&1; then
    for f in "$DEST"/Windows/*.exe; do [ -e "$f" ] && mv "$f" "$EXE" && recupere=1; done
  fi
fi

if [ "$recupere" -eq 0 ]; then
  URL="$(curl -fsSL "https://api.github.com/repos/$DEPOT/releases/latest" 2>/dev/null \
        | node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>{try{const a=(JSON.parse(d).assets||[]).find(a=>a.name.endsWith(".exe"));process.stdout.write(a?a.browser_download_url:"")}catch{}})' || true)"
  if [ -n "${URL:-}" ] && curl -fsSL "$URL" -o "$EXE"; then
    recupere=1
  fi
fi

if [ "$recupere" -eq 1 ]; then
  ok "$(basename "$EXE")  ($(du -h "$EXE" | cut -f1))"
else
  souci "téléchargement automatique impossible (dépôt privé ou hors ligne)"
  cat > "$DEST/Windows/À LIRE — installateur à télécharger.txt" <<NOTE
L'installateur Windows n'a pas pu être téléchargé automatiquement.

Récupérez-le à la main, en étant connecté à GitHub :

    https://github.com/$DEPOT/releases

Prenez le fichier « Souffle-Installateur-….exe » de la dernière version,
et posez-le dans ce dossier en le renommant « Souffle — Windows.exe ».
NOTE
fi

# ------------------------------------------------------------------ #
gras "Documents"

cp "$PROJET/distribution/A-QUOI-SERT-SOUFFLE.md" "$DEST/1 — À quoi sert Souffle.md"
cp "$PROJET/distribution/INSTALLATION.md"        "$DEST/2 — Installation.md"
ok "1 — À quoi sert Souffle.md"
ok "2 — Installation.md"

# ------------------------------------------------------------------ #
gras "Archive"

if command -v ditto >/dev/null 2>&1; then
  (cd "$BUREAU" && ditto -c -k --sequesterRsrc --keepParent "Souffle" "Souffle.zip")
else
  (cd "$BUREAU" && zip -qr "Souffle.zip" "Souffle")
fi
ok "Souffle.zip  ($(du -h "$ZIP" | cut -f1))"

# ------------------------------------------------------------------ #
printf '\n\033[1m✓ Terminé.\033[0m Sur votre Bureau :\n\n'
printf '   Souffle/        le dossier, prêt à parcourir\n'
printf '   Souffle.zip     le même, prêt à envoyer\n\n'
printf 'Le fichier à ouvrir pour installer est indiqué dans « 2 — Installation.md ».\n\n'
