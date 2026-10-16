@echo off
rem  Installe Souffle sur ce PC Windows. Double-cliquez sur ce fichier.
chcp 65001 >nul
setlocal
cd /d "%~dp0"
cls

echo.
echo  ┌──────────────────────────────────────────────┐
echo  │                                              │
echo  │   Souffle — installation sur ce PC           │
echo  │                                              │
echo  │   Dictee vocale : un raccourci, vous parlez, │
echo  │   le texte propre s'ecrit ou vous tapez.     │
echo  │                                              │
echo  └──────────────────────────────────────────────┘
echo.

echo  [1] Verifications
echo.

if not exist "application\package.json" (
  echo     Le dossier "application" est introuvable a cote de ce fichier.
  echo     Decompressez l'archive entierement avant de lancer l'installation.
  echo.
  pause
  exit /b 1
)

where node >nul 2>nul
if errorlevel 1 (
  echo     Node.js est necessaire pour construire l'application,
  echo     et il n'est pas installe.
  echo.
  echo       1. Rendez-vous sur   https://nodejs.org
  echo       2. Telechargez la version LTS et installez-la
  echo       3. Revenez ici et double-cliquez de nouveau sur ce fichier
  echo.
  pause
  exit /b 1
)

for /f "delims=" %%v in ('node -v') do echo     OK  Node.js %%v
echo.

echo  [2] Construction de l'installateur
echo.
echo     Comptez trois a huit minutes. La premiere fois, un composant
echo     de 150 Mo est telecharge : c'est normal, une seule fois.
echo.

cd application
call npm install --no-audit --no-fund
if errorlevel 1 goto echec

call npx electron-builder --win nsis --x64
if errorlevel 1 goto echec

set "INSTALLATEUR="
for %%f in ("dist\*.exe") do set "INSTALLATEUR=%%~ff"

if not defined INSTALLATEUR (
  echo.
  echo     Aucun installateur produit. Les messages ci-dessus disent pourquoi.
  echo.
  pause
  exit /b 1
)

echo.
echo     OK  %INSTALLATEUR%
echo.

echo  [3] Installation
echo.
echo     L'installateur va s'ouvrir. Windows affichera un ecran bleu
echo     "Windows a protege votre ordinateur" : c'est normal, l'application
echo     n'est pas signee par un certificat commercial.
echo.
echo     Cliquez sur "Informations complementaires", puis sur
echo     "Executer quand meme". Le reste se fait tout seul.
echo.
pause

start "" "%INSTALLATEUR%"

echo.
echo  [4] Ensuite
echo.
echo     Souffle vit dans la zone de notification, en bas a droite,
echo     pres de l'horloge. Cliquez sur la fleche ^^ si l'icone est masquee.
echo.
echo     Une seule chose a faire : coller une cle API dans la fenetre de
echo     reglages qui s'ouvre au premier lancement (onglet Intelligence).
echo     Sans elle, Souffle ne peut rien transcrire.
echo.
echo     Raccourci : Ctrl + Maj + Espace
echo     Appuyez, parlez, re-appuyez. Le texte s'ecrit ou etait le curseur.
echo.
echo     Tout est explique dans "2 - Installation.md" et
echo     "1 - A quoi sert Souffle.md", a cote de ce fichier.
echo.
echo  Termine. Cette fenetre peut etre fermee.
echo.
pause
exit /b 0

:echec
echo.
echo     La construction a echoue. Les messages ci-dessus disent pourquoi.
echo.
pause
exit /b 1
