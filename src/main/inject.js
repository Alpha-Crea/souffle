'use strict';
/**
 * Insertion du texte dans l'application qui a le focus.
 *
 * Stratégie volontairement sans module natif : on passe par le presse-papier
 * puis on simule Cmd+V / Ctrl+V au niveau OS. Le texte reste ensuite dans le
 * presse-papier — c'est le filet quand le collage rate ; le rendre à son ancien
 * contenu est une option, désactivée par défaut.
 *   - macOS   : osascript + System Events   (requiert Accessibilité)
 *   - Windows : PowerShell SendKeys
 *   - Linux   : xdotool si présent, sinon wtype (Wayland)
 */
const { clipboard, shell, systemPreferences } = require('electron');
const { execFile } = require('node:child_process');

/**
 * windowsHide est capital et non cosmétique : sans lui, chaque appel à
 * PowerShell ouvre une console qui passe au premier plan. Elle devient alors la
 * fenêtre active — et le Ctrl+V part dans cette console au lieu de l'application
 * visée, pendant que la détection d'application au premier plan croit que
 * l'utilisateur travaille dans « powershell ».
 */
const run = (cmd, args, opts = {}) =>
  new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout: 5000, windowsHide: true, ...opts }, (err, stdout) =>
      err ? reject(err) : resolve(String(stdout || '').trim())
    );
  });

/**
 * Littéral de chaîne PowerShell. Les guillemets simples n'interprètent ni $ ni
 * l'accent grave ; seul le guillemet simple lui-même doit être doublé.
 */
const psQuote = (value) => `'${String(value).replace(/'/g, "''")}'`;

/* ------------------------------------------------------------------ */
/* Application au premier plan                                         */
/* ------------------------------------------------------------------ */

async function frontmostApp() {
  try {
    if (process.platform === 'darwin') {
      return await run('osascript', [
        '-e',
        'tell application "System Events" to get name of first application process whose frontmost is true'
      ]);
    }
    if (process.platform === 'win32') {
      const ps = `
        Add-Type @"
          using System;using System.Runtime.InteropServices;
          public class W {
            [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
            [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int p);
          }
"@
        $p = 0; [void][W]::GetWindowThreadProcessId([W]::GetForegroundWindow(), [ref]$p)
        (Get-Process -Id $p).ProcessName`;
      // Add-Type compile du C# à la volée : le premier appel après un démarrage
      // à froid dépasse volontiers cinq secondes. Expirer ici ferait perdre
      // l'application cible, donc le retour au bon champ.
      return await run('powershell', ['-NoProfile', '-NonInteractive', '-Command', ps], {
        timeout: 15000
      });
    }
    return await run('xdotool', ['getactivewindow', 'getwindowclassname']);
  } catch {
    return '';
  }
}

/**
 * Redonne le focus à l'application visée. Entre le moment où l'on commence à
 * parler et celui où le texte est prêt, il s'écoule une à deux secondes : largement
 * de quoi cliquer ailleurs. Sans ce rappel, le texte atterrit dans la mauvaise fenêtre.
 */
async function focusApp(name) {
  if (!name) return false;
  try {
    if (process.platform === 'darwin') {
      await run('osascript', [
        '-e',
        `tell application "System Events" to set frontmost of (first application process whose name is ${JSON.stringify(
          name
        )}) to true`
      ]);
      return true;
    }
    if (process.platform === 'win32') {
      // AppActivate prend un identifiant de processus : on le retrouve par son nom.
      const ps = `
        $p = Get-Process -Name ${psQuote(name)} -ErrorAction SilentlyContinue |
             Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($p) { (New-Object -ComObject WScript.Shell).AppActivate($p.Id) | Out-Null; 'ok' }`;
      const out = await run('powershell', ['-NoProfile', '-NonInteractive', '-Command', ps]);
      return out.includes('ok');
    }
    if (process.platform === 'linux') {
      await run('xdotool', ['search', '--classname', name, 'windowactivate', '--sync']);
      return true;
    }
    return false;
  } catch {
    return false;
  }
}

/* ------------------------------------------------------------------ */
/* Collage                                                             */
/* ------------------------------------------------------------------ */

async function sendPasteKeystroke() {
  if (process.platform === 'darwin') {
    return run('osascript', [
      '-e',
      'tell application "System Events" to keystroke "v" using command down'
    ]);
  }
  if (process.platform === 'win32') {
    return run('powershell', [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      'Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait("^v")'
    ]);
  }
  try {
    return await run('xdotool', ['key', '--clearmodifiers', 'ctrl+v']);
  } catch {
    return run('wtype', ['-M', 'ctrl', 'v', '-m', 'ctrl']);
  }
}

/**
 * Primitives système regroupées pour pouvoir être remplacées dans les tests :
 * un conteneur d'intégration n'a ni osascript ni fenêtres réelles.
 */
const sys = { frontmostApp, focusApp, sendPasteKeystroke };

/**
 * @param {string} text
 * @param {{autoPaste?:boolean, targetApp?:string, refocus?:boolean, restoreClipboard?:boolean}} opts
 * @returns {Promise<{pasted:boolean, reason?:string, refocused?:string}>}
 */
async function insertText(
  text,
  { autoPaste = true, targetApp = '', refocus = true, restoreClipboard = false } = {}
) {
  if (!text) return { pasted: false, reason: 'empty' };

  const previous = clipboard.readText();
  clipboard.writeText(text);

  if (!autoPaste) return { pasted: false, reason: 'clipboard-only' };

  if (process.platform === 'darwin' && !systemPreferences.isTrustedAccessibilityClient(false)) {
    return { pasted: false, reason: 'accessibility' };
  }

  // Si l'utilisateur a changé de fenêtre pendant la transcription, on revient
  // sur celle où il a commencé à parler — c'est là qu'il attend son texte.
  let refocused = null;
  if (refocus && targetApp) {
    const current = await sys.frontmostApp();
    if (current && current !== targetApp) {
      const ok = await sys.focusApp(targetApp);
      if (ok) {
        refocused = targetApp;
        await new Promise((r) => setTimeout(r, 220));
      }
    }
  }

  try {
    // Laisse le focus se stabiliser avant d'envoyer la frappe.
    await new Promise((r) => setTimeout(r, 120));
    await sys.sendPasteKeystroke();
  } catch (err) {
    console.error('[souffle] collage impossible :', err.message);
    return { pasted: false, reason: 'keystroke-failed' };
  }

  // Une frappe ⌘V envoyée sans erreur ne prouve pas que l'application l'a
  // reçue : un Finder, une fenêtre sans champ de saisie, un focus repris par
  // une autre app, et le texte n'atterrit nulle part. Restaurer l'ancien
  // presse-papier là-dessus efface la dictée pour de bon. On ne le fait donc
  // que si l'utilisateur l'a explicitement demandé ; par défaut le texte reste
  // disponible et un ⌘V manuel rattrape le collage manqué.
  if (restoreClipboard) {
    setTimeout(() => {
      if (clipboard.readText() === text) clipboard.writeText(previous);
    }, 1200);
  }

  return { pasted: true, refocused };
}

/** Ouvre le volet Accessibilité des Réglages Système (macOS). */
function openAccessibilitySettings() {
  if (process.platform !== 'darwin') return;
  systemPreferences.isTrustedAccessibilityClient(true);
  shell.openExternal(
    'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility'
  );
}

function accessibilityGranted() {
  if (process.platform !== 'darwin') return true;
  return systemPreferences.isTrustedAccessibilityClient(false);
}

module.exports = {
  insertText,
  sys,
  focusApp,
  frontmostApp,
  openAccessibilitySettings,
  accessibilityGranted
};
