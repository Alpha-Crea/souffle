'use strict';
/**
 * Souffle — dictée vocale système.
 * Process principal : fenêtres, raccourci global, machine à états, orchestration du pipeline.
 */
const path = require('node:path');
const {
  app,
  BrowserWindow,
  Tray,
  Menu,
  ipcMain,
  globalShortcut,
  screen,
  shell,
  nativeImage,
  systemPreferences,
  dialog
} = require('electron');

const { Store } = require('./store');
const { History } = require('./history');
const { transcribe, format, testKey } = require('./ai');
const inject = require('./inject');

const isMac = process.platform === 'darwin';
const ICONS = path.join(__dirname, '..', '..', 'build');

let store;
let history;
let tray = null;
let overlayWin = null;
let recorderWin = null;
let settingsWin = null;

/** 'idle' | 'recording' | 'working' | 'done' | 'error' */
let state = 'idle';
let recordingStartedAt = 0;
let targetApp = '';
let abortCtl = null;
let uiohook = null;
let captureWatchdog = null;
let maxDurationTimer = null;

const DIAG = process.argv.includes('--diag');
const MAX_RECORDING_MS = 120000;
// Délai sans le moindre signe de vie de la fenêtre de capture. Ouvrir le micro
// peut demander plusieurs essais : c'est chaque essai qui réarme ce compte.
const CAPTURE_WATCHDOG_MS = 3500;

/** Journal horodaté : c'est ce qui rend une panne lisible depuis le terminal. */
function log(...args) {
  const t = new Date().toISOString().slice(11, 23);
  console.log(`[souffle ${t}]`, ...args);
}

/* ------------------------------------------------------------------ */
/* Fenêtres                                                            */
/* ------------------------------------------------------------------ */

function createRecorderWindow() {
  recorderWin = new BrowserWindow({
    width: 320,
    height: 200,
    show: false,
    skipTaskbar: true,
    webPreferences: {
      preload: path.join(__dirname, '..', 'preload', 'recorder.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false
    }
  });
  recorderWin.loadFile(path.join(__dirname, '..', 'renderer', 'recorder', 'index.html'));

  const ses = recorderWin.webContents.session;
  ses.setPermissionRequestHandler((_wc, permission, cb) =>
    cb(permission === 'media' || permission === 'audioCapture')
  );
  // getUserMedia passe aussi par le *check* synchrone : sans ce handler,
  // Electron peut refuser le micro sans jamais afficher de demande.
  ses.setPermissionCheckHandler((_wc, permission) =>
    permission === 'media' || permission === 'audioCapture'
  );

  // Tout ce que la fenêtre de capture logue remonte dans le terminal.
  // Electron ≥ 36 passe un objet unique ; les versions antérieures (event, level, message).
  recorderWin.webContents.on('console-message', (...args) => {
    const first = args[0];
    const modern = first && typeof first === 'object' && 'message' in first;
    const message = String(modern ? first.message : args[2] ?? '');
    const level = modern ? first.level : args[1];
    const serious = level === 'error' || level === 'warning' || level >= 2;
    if (message.startsWith('[rec]') || serious) log('capture ›', message);
  });
  recorderWin.webContents.on('render-process-gone', (_e, details) =>
    log('capture › process perdu :', details.reason)
  );

  // Cette fenêtre porte tout le moteur audio : la fermer casserait l'application.
  // On intercepte la fermeture pour la masquer au lieu de la détruire.
  recorderWin.on('close', (e) => {
    if (app.isQuitting) return;
    e.preventDefault();
    recorderWin.hide();
    log('fenêtre de capture masquée (fermeture interceptée)');
  });

  if (DIAG) {
    recorderWin.show();
    recorderWin.webContents.openDevTools({ mode: 'detach' });
  }
}

/** Recrée la fenêtre de capture si elle a disparu, et attend qu'elle soit prête. */
async function ensureRecorderWindow() {
  if (recorderWin && !recorderWin.isDestroyed()) return true;
  log('fenêtre de capture absente, recréation');
  createRecorderWindow();
  await new Promise((resolve) => {
    recorderWin.webContents.once('did-finish-load', resolve);
    setTimeout(resolve, 3000);
  });
  return recorderWin && !recorderWin.isDestroyed();
}

/** Envoi protégé : une fenêtre détruite ne doit jamais faire tomber le process. */
function sendToRecorder(channel, payload) {
  if (!recorderWin || recorderWin.isDestroyed()) {
    log(`envoi « ${channel} » impossible : fenêtre de capture détruite`);
    return false;
  }
  recorderWin.webContents.send(channel, payload);
  return true;
}

function overlayBounds() {
  const cursor = screen.getCursorScreenPoint();
  const display = screen.getDisplayNearestPoint(cursor);
  const w = 300;
  const h = 74;
  const { x, y, width, height } = display.workArea;
  return {
    x: Math.round(x + (width - w) / 2),
    y: Math.round(y + height - h - 48),
    width: w,
    height: h
  };
}

function createOverlayWindow() {
  overlayWin = new BrowserWindow({
    ...overlayBounds(),
    frame: false,
    transparent: true,
    resizable: false,
    movable: false,
    show: false,
    focusable: false,
    skipTaskbar: true,
    hasShadow: false,
    alwaysOnTop: true,
    fullscreenable: false,
    acceptFirstMouse: false,
    webPreferences: {
      preload: path.join(__dirname, '..', 'preload', 'overlay.js'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: false
    }
  });
  overlayWin.setAlwaysOnTop(true, 'screen-saver');
  overlayWin.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
  overlayWin.setIgnoreMouseEvents(true, { forward: false });
  overlayWin.loadFile(path.join(__dirname, '..', 'renderer', 'overlay', 'index.html'));
}

function showOverlay() {
  if (!overlayWin) return;
  overlayWin.setBounds(overlayBounds());
  overlayWin.showInactive(); // n'enlève jamais le focus à l'app cible
}

function hideOverlay(delay = 0) {
  if (!overlayWin) return;
  setTimeout(() => {
    if (state === 'idle' && overlayWin && !overlayWin.isDestroyed()) overlayWin.hide();
  }, delay);
}

function openSettings(tab) {
  if (settingsWin && !settingsWin.isDestroyed()) {
    settingsWin.show();
    settingsWin.focus();
    if (tab) settingsWin.webContents.send('settings:tab', tab);
    return;
  }
  settingsWin = new BrowserWindow({
    width: 880,
    height: 660,
    minWidth: 720,
    minHeight: 540,
    title: 'Souffle — Réglages',
    titleBarStyle: isMac ? 'hiddenInset' : 'default',
    backgroundColor: '#0f1115',
    show: false,
    webPreferences: {
      preload: path.join(__dirname, '..', 'preload', 'settings.js'),
      contextIsolation: true,
      nodeIntegration: false
    }
  });
  settingsWin.loadFile(path.join(__dirname, '..', 'renderer', 'settings', 'index.html'));
  settingsWin.once('ready-to-show', () => {
    settingsWin.show();
    if (tab) settingsWin.webContents.send('settings:tab', tab);
  });
  settingsWin.on('closed', () => {
    settingsWin = null;
  });
}

/* ------------------------------------------------------------------ */
/* Machine à états                                                     */
/* ------------------------------------------------------------------ */

function setState(next, payload = {}) {
  state = next;
  if (overlayWin && !overlayWin.isDestroyed()) {
    overlayWin.webContents.send('overlay:state', {
      state: next,
      sounds: store.get('playSounds'),
      ...payload
    });
  }
  updateTray();
}

async function startRecording() {
  if (state !== 'idle' && state !== 'done' && state !== 'error') {
    log(`démarrage ignoré (état = ${state})`);
    return;
  }

  if (isMac) {
    const mic = systemPreferences.getMediaAccessStatus('microphone');
    log('autorisation micro =', mic);
    if (mic === 'denied' || mic === 'restricted') {
      setState('error', { message: 'Micro bloqué — Réglages Système' });
      shell.openExternal('x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone');
      setTimeout(() => {
        setState('idle');
        hideOverlay(0);
      }, 3000);
      return;
    }
    if (mic !== 'granted') {
      const ok = await systemPreferences.askForMediaAccess('microphone');
      log('demande micro →', ok);
      if (!ok) {
        setState('error', { message: 'Micro refusé' });
        setTimeout(() => {
          setState('idle');
          hideOverlay(0);
        }, 2500);
        return;
      }
    }
  }

  if (!(await ensureRecorderWindow())) {
    setState('error', { message: 'Moteur audio indisponible' });
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 3000);
    return;
  }

  // On lance la capture d'abord : identifier l'app active coûte 100 à 300 ms,
  // et personne n'attend un feu vert avant de commencer sa phrase.
  recordingStartedAt = Date.now();
  segmentsInserted = 0;
  const live = Boolean(store.get('liveMode'));
  setState('recording', { live });
  showOverlay();
  log(`capture demandée${live ? ' (mode direct)' : ''}`);
  sendToRecorder('recorder:start', { live });
  startEscapeWatch();
  inject.frontmostApp().then((name) => {
    targetApp = name;
    log('application cible =', name || '(inconnue)');
  });

  // Filet de sécurité : si la capture ne confirme pas son démarrage, on ne
  // laisse pas la pilule tourner dans le vide indéfiniment.
  armCaptureWatchdog();

  clearTimeout(maxDurationTimer);
  maxDurationTimer = setTimeout(() => {
    if (state === 'recording') {
      log('durée maximale atteinte, arrêt automatique');
      stopRecording();
    }
  }, MAX_RECORDING_MS);

  // Pendant la dictée, la pilule devient cliquable : clic = terminer, clic droit = annuler.
  overlayWin?.setIgnoreMouseEvents(false);

  ensureMainShortcut();
}

function stopRecording() {
  if (state !== 'recording') {
    log(`arrêt ignoré (état = ${state})`);
    return;
  }
  log('arrêt demandé après', Math.round((Date.now() - recordingStartedAt) / 100) / 10, 's');
  clearTimeout(captureWatchdog);
  clearTimeout(maxDurationTimer);
  overlayWin?.setIgnoreMouseEvents(true);
  stopEscapeWatch();
  setState('working');
  if (!sendToRecorder('recorder:stop')) {
    setState('error', { message: 'Moteur audio perdu' });
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 2500);
  }
  ensureMainShortcut();
}

function cancelRecording() {
  log('annulation');
  clearTimeout(captureWatchdog);
  clearTimeout(maxDurationTimer);
  overlayWin?.setIgnoreMouseEvents(true);
  stopEscapeWatch();
  if (abortCtl) abortCtl.abort();
  sendToRecorder('recorder:cancel');
  setState('idle');
  hideOverlay(150);
  ensureMainShortcut();
}

/**
 * Le raccourci principal doit survivre à tout. On le vérifie après chaque
 * transition : une seule perte silencieuse rendrait l'application inutilisable.
 */
function ensureMainShortcut() {
  const accel = store.get('shortcut');
  if (globalShortcut.isRegistered(accel)) return;
  log(`raccourci « ${accel} » perdu, réenregistrement`);
  registerShortcut();
}

/* Arrêt et annulation depuis la pilule, sans dépendre du clavier. */
ipcMain.on('overlay:stop', stopRecording);
ipcMain.on('overlay:cancel', cancelRecording);

function toggleRecording() {
  log(`raccourci reçu (état = ${state})`);
  try {
    if (state === 'recording') stopRecording();
    else startRecording().catch(onFatal);
  } catch (err) {
    onFatal(err);
  }
}

/**
 * Une dictée qui échoue ne doit jamais tuer l'application ni ouvrir une boîte
 * de dialogue système : on logue, on remet à zéro, on continue.
 */
function onFatal(err) {
  log('ERREUR non rattrapée :', err && err.stack ? err.stack : err);
  try {
    setState('error', { message: 'Erreur interne — voir le terminal' });
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 3000);
    ensureMainShortcut();
  } catch {
    /* on ne peut plus rien faire de propre ici */
  }
}

/**
 * Chien de garde du démarrage.
 * Il ne compte pas le temps total d'ouverture du micro : la fenêtre de capture
 * peut légitimement essayer plusieurs périphériques quand une autre application
 * tient le micro. Chaque tentative réarme le délai ; seul un silence complet de
 * la fenêtre de capture déclenche l'erreur.
 */
function armCaptureWatchdog(ms = CAPTURE_WATCHDOG_MS) {
  clearTimeout(captureWatchdog);
  captureWatchdog = setTimeout(() => {
    if (state !== 'recording') return;
    log('ERREUR : la capture micro n’a jamais démarré');
    globalShortcut.unregister('Escape');
    setState('error', { message: 'Micro injoignable' });
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 3000);
  }, ms);
}

/** La fenêtre de capture essaie d'ouvrir une entrée audio : elle est vivante. */
ipcMain.on('recorder:acquiring', () => {
  if (state !== 'recording') return;
  armCaptureWatchdog();
});

/** La fenêtre de capture confirme que le flux tourne vraiment. */
ipcMain.on('recorder:started', () => {
  clearTimeout(captureWatchdog);
  log('capture active');
});

/* ------------------------------------------------------------------ */
/* Pipeline : audio -> texte -> insertion                              */
/* ------------------------------------------------------------------ */

/**
 * File d'attente d'insertion.
 * Les transcriptions tournent en parallèle — c'est ce qui rend le mode direct
 * fluide — mais les phrases doivent être collées dans l'ordre où elles ont été
 * prononcées, pas dans l'ordre où le réseau les rend. La place dans la file est
 * donc réservée dès l'arrivée de l'audio, avant tout appel distant.
 */
let insertQueue = Promise.resolve();
let segmentsInserted = 0;

ipcMain.on('recorder:audio', (_e, payload) => {
  const prepared = prepareText(payload).catch((err) => {
    onPipelineError(err, payload);
    return null;
  });

  insertQueue = insertQueue
    .then(() => prepared)
    .then((ready) => (ready ? deliver(ready, payload) : null))
    .catch((err) => onPipelineError(err, payload));
});

/** Étape parallélisable : audio -> texte prêt à écrire. */
async function prepareText({ buffer, mime, durationMs, segment = false }) {
  const audio = Buffer.from(buffer);
  log(`audio reçu (${segment ? 'segment' : 'dictée'}) : ${audio.length} octets, ${mime}, ${durationMs} ms`);

  if (!audio.length || durationMs < 350) {
    log('audio trop court, abandon');
    if (!segment) {
      setState('idle');
      hideOverlay(100);
    }
    return null;
  }

  abortCtl = new AbortController();
  const signal = abortCtl.signal;

  if (!segment) setState('working', { step: 'transcription' });
  const t0 = Date.now();
  const raw = await transcribe({ audio, mime, store, signal });
  log(`transcription (${Date.now() - t0} ms) :`, JSON.stringify(raw));

  if (!raw) {
    if (segment) return null; // un blanc au milieu d'une dictée continue n'est pas une erreur
    setState('error', { message: 'Rien entendu' });
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 1800);
    return null;
  }

  if (!segment) setState('working', { step: 'mise en forme' });
  const t1 = Date.now();
  const text = await format({ text: raw, store, appName: targetApp, signal });
  log(`mise en forme (${Date.now() - t1} ms) :`, JSON.stringify(text));

  return { raw, text };
}

/** Étape strictement ordonnée : écriture dans l'application cible. */
async function deliver({ raw, text }, { durationMs, segment = false, final = false }) {
  // En continu, chaque phrase s'ajoute à la précédente : il faut l'espace.
  const prefix = segmentsInserted > 0 ? ' ' : '';

  const result = await inject.insertText(prefix + text, {
    autoPaste: store.get('autoPaste'),
    targetApp,
    refocus: store.get('refocusTarget')
  });
  log('insertion :', JSON.stringify(result));
  if (result.refocused) log(`focus rendu à « ${result.refocused} »`);

  const words = text.split(/\s+/).filter(Boolean).length;
  store.bumpStats({ words, seconds: Math.round(durationMs / 1000) });

  if (!store.get('privacyMode')) {
    history.add({ raw, text, app: targetApp, words }, store.get('historyLimit'));
  }

  if (!result.pasted && result.reason === 'accessibility') {
    setState('error', { message: 'Copié — autorisez l\u2019Accessibilité' });
    inject.openAccessibilitySettings();
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 2600);
    return;
  }

  if (!result.pasted && result.reason === 'keystroke-failed') {
    // Le texte est dans le presse-papier : on le dit plutôt que d'afficher « inséré ».
    setState('error', { message: 'Copié — collez manuellement' });
    setTimeout(() => {
      setState('idle');
      hideOverlay(0);
    }, 2400);
    return;
  }

  segmentsInserted += 1;

  if (segment && !final) {
    // On reste en écoute : l'overlay affiche le décompte au lieu d'un état final.
    if (state === 'recording') setState('recording', { live: true, segments: segmentsInserted });
    return;
  }

  // Fin de dictée : le compteur repart à zéro, sinon la dictée suivante
  // commencerait par une espace héritée de celle-ci.
  segmentsInserted = 0;

  setState('done', { words, text: text.slice(0, 90), pasted: result.pasted });
  setTimeout(() => {
    setState('idle');
    hideOverlay(0);
  }, 1100);
}

function onPipelineError(err, { segment = false } = {}) {
  abortCtl = null;
  if (err && err.name === 'AbortError') {
    setState('idle');
    hideOverlay(0);
    return;
  }
  log('ERREUR pipeline :', err && err.message ? err.message : err);
  if (segment) return; // en direct, une phrase ratée ne doit pas couper la dictée
  setState('error', { message: humanError(err) });
  setTimeout(() => {
    setState('idle');
    hideOverlay(0);
  }, 3200);
}

function humanError(err) {
  const m = String(err.message || err);
  if (m.includes('401')) return 'Clé API refusée';
  if (m.includes('429')) return 'Quota dépassé';
  if (/fetch failed|ENOTFOUND|ECONNREFUSED/.test(m)) return 'Pas de réseau';
  if (m.includes('Aucune clé')) return 'Clé API manquante';
  return m.slice(0, 60);
}

ipcMain.on('recorder:error', (_e, message) => {
  clearTimeout(captureWatchdog);
  clearTimeout(maxDurationTimer);
  log('ERREUR capture :', message);
  setState('error', { message: String(message).slice(0, 60) });
  setTimeout(() => {
    setState('idle');
    hideOverlay(0);
  }, 3000);
});

ipcMain.on('recorder:level', (_e, level) => {
  if (overlayWin && !overlayWin.isDestroyed() && state === 'recording') {
    overlayWin.webContents.send('overlay:level', level);
  }
});

/* ------------------------------------------------------------------ */
/* IPC réglages                                                        */
/* ------------------------------------------------------------------ */

ipcMain.handle('settings:get', () => ({
  ...store.all(),
  secrets: store.secretStatus(),
  platform: process.platform,
  version: app.getVersion(),
  accessibility: inject.accessibilityGranted(),
  holdAvailable: Boolean(uiohook),
  escapeAvailable: escapeAvailable()
}));

ipcMain.handle('settings:set', (_e, patch) => {
  const oldShortcut = store.get('shortcut');
  store.set(patch);

  if (patch.shortcut && patch.shortcut !== oldShortcut) registerShortcut();
  if ('mode' in patch) setupHoldMode();
  if ('launchAtLogin' in patch) {
    try {
      app.setLoginItemSettings({ openAtLogin: Boolean(patch.launchAtLogin), openAsHidden: true });
    } catch (err) {
      log('lancement au démarrage indisponible :', err.message);
    }
  }
  updateTray();
  return true;
});

ipcMain.handle('secret:set', (_e, { key, value }) => {
  store.setSecret(key, value);
  return store.secretStatus();
});

ipcMain.handle('secret:test', async (_e, { provider, key, customUrl }) => {
  try {
    const n = await testKey({ provider, key: key || store.getSecret(`${provider}Key`), customUrl });
    return { ok: true, models: n };
  } catch (err) {
    return { ok: false, error: humanError(err) };
  }
});

ipcMain.handle('history:list', () => history.list());
ipcMain.handle('history:clear', () => {
  history.clear();
  return true;
});
ipcMain.handle('perm:accessibility', () => {
  inject.openAccessibilitySettings();
  return true;
});
ipcMain.handle('perm:microphone', async () => {
  if (!isMac) return true;
  return systemPreferences.askForMediaAccess('microphone');
});
ipcMain.handle('shortcut:validate', (_e, accel) => {
  try {
    const taken = globalShortcut.isRegistered(accel);
    return { ok: !taken || accel === store.get('shortcut'), taken };
  } catch {
    return { ok: false, taken: false };
  }
});
ipcMain.handle('app:openExternal', (_e, url) => shell.openExternal(url));
ipcMain.handle('app:dictate', () => toggleRecording());

/* ------------------------------------------------------------------ */
/* Raccourci global + mode « maintenir »                               */
/* ------------------------------------------------------------------ */

function registerShortcut() {
  globalShortcut.unregisterAll();
  const accel = store.get('shortcut');
  try {
    const ok = globalShortcut.register(accel, () => {
      if (store.get('mode') === 'hold' && uiohook) return; // géré par uiohook
      toggleRecording();
    });
    if (!ok) throw new Error('refusé par le système');
    log(`raccourci « ${accel} » enregistré`);
  } catch (err) {
    dialog.showMessageBox({
      type: 'warning',
      message: `Le raccourci « ${accel} » est déjà pris.`,
      detail: 'Choisissez-en un autre dans les réglages de Souffle.',
      buttons: ['Ouvrir les réglages', 'Plus tard']
    }).then(({ response }) => response === 0 && openSettings('general'));
  }
}

/* ------------------------------------------------------------------ */
/* Clavier bas niveau (uiohook) : maintien + Échap                     */
/* ------------------------------------------------------------------ */

/**
 * uiohook-napi observe le clavier sans rien intercepter. On l'utilise pour deux
 * choses qu'un raccourci global ne sait pas faire :
 *   - détecter le relâchement d'une touche (mode « maintenir pour parler ») ;
 *   - écouter Échap pendant une dictée sans l'enregistrer comme raccourci global,
 *     ce qui casserait le gestionnaire de hotkeys de macOS.
 * Un unique écouteur dispatche vers les deux usages.
 */
let hookStarted = false;
let escapeWatching = false;
let holdKeyDown = false;

function startHook() {
  if (!uiohook || hookStarted) return;
  const { uIOhook, UiohookKey } = uiohook;

  uIOhook.on('keydown', (e) => {
    if (escapeWatching && e.keycode === UiohookKey.Escape) {
      onEscapePressed();
      return;
    }
    if (store.get('mode') === 'hold' && !holdKeyDown && matchesShortcut(e)) {
      holdKeyDown = true;
      startRecording().catch(onFatal);
    }
  });

  uIOhook.on('keyup', (e) => {
    if (!holdKeyDown) return;
    if (e.keycode !== holdKeycode()) return;
    holdKeyDown = false;
    stopRecording();
  });

  try {
    uIOhook.start();
    hookStarted = true;
    log('écoute clavier bas niveau active');
  } catch (err) {
    log('uiohook indisponible :', err.message);
  }
}

function holdKeycode() {
  const { UiohookKey } = uiohook;
  const parts = store.get('shortcut').split('+').map((p) => p.trim().toLowerCase());
  const name = parts[parts.length - 1];
  const KEYMAP = {
    space: UiohookKey.Space,
    f1: UiohookKey.F1, f2: UiohookKey.F2, f3: UiohookKey.F3,
    f13: UiohookKey.F13, d: UiohookKey.D, v: UiohookKey.V
  };
  return KEYMAP[name] ?? UiohookKey.Space;
}

function matchesShortcut(e) {
  const parts = store.get('shortcut').split('+').map((p) => p.trim().toLowerCase());
  const want = {
    ctrl: parts.includes('control') || parts.includes('ctrl') || parts.includes('commandorcontrol'),
    alt: parts.includes('alt') || parts.includes('option'),
    shift: parts.includes('shift'),
    meta: parts.includes('command') || parts.includes('cmd') || parts.includes('super')
  };
  return (
    e.keycode === holdKeycode() &&
    Boolean(e.ctrlKey) === want.ctrl &&
    Boolean(e.altKey) === want.alt &&
    Boolean(e.shiftKey) === want.shift &&
    Boolean(e.metaKey) === want.meta
  );
}

/** Échap n'est écouté que pendant une dictée : le reste du temps, on ne regarde rien. */
function onEscapePressed() {
  const action = store.get('escapeAction');
  log(`Échap pendant la dictée → ${action}`);
  if (action === 'stop') stopRecording();
  else if (action === 'cancel') cancelRecording();
}

/** Échap est-il utilisable sur cette machine ? */
function escapeAvailable() {
  return Boolean(uiohook) || !isMac;
}

function startEscapeWatch() {
  if (store.get('escapeAction') === 'off') return;

  if (uiohook) {
    startHook();
    escapeWatching = true;
    return;
  }

  // Sans module natif, on retombe sur un raccourci global — mais uniquement
  // hors macOS. Sur macOS, enregistrer une touche sans modificateur casse le
  // gestionnaire de hotkeys du système et fait taire le raccourci principal.
  if (isMac) return;

  // Jamais depuis l'intérieur d'un callback de raccourci : on repousse d'un tour.
  setTimeout(() => {
    if (state !== 'recording') return;
    try {
      globalShortcut.register('Escape', onEscapePressed);
    } catch (err) {
      log('Échap indisponible :', err.message);
    }
    ensureMainShortcut();
  }, 0);
}

function stopEscapeWatch() {
  escapeWatching = false;
  if (uiohook || isMac) return;
  try {
    if (globalShortcut.isRegistered('Escape')) globalShortcut.unregister('Escape');
  } catch {
    /* rien à libérer */
  }
  ensureMainShortcut();
}

function setupHoldMode() {
  if (!uiohook) return;
  if (store.get('mode') === 'hold') startHook();
  holdKeyDown = false;
}

/* ------------------------------------------------------------------ */
/* Barre de menus / zone de notification                               */
/* ------------------------------------------------------------------ */

function trayImage() {
  const file = isMac ? 'trayTemplate.png' : 'tray.png';
  const img = nativeImage.createFromPath(path.join(ICONS, file));
  if (isMac) img.setTemplateImage(true);
  return img.isEmpty() ? nativeImage.createEmpty() : img;
}

/**
 * La fenêtre de réglages doit refléter ce que le menu vient de changer.
 * Un rechargement complet évite d'avoir deux sources de vérité à synchroniser.
 */
function refreshSettingsWindow() {
  if (settingsWin && !settingsWin.isDestroyed() && settingsWin.isVisible()) {
    settingsWin.reload();
  }
}

/** Ouvre le mode d'emploi dans le navigateur par défaut. */
function openGuide() {
  const guide = app.isPackaged
    ? path.join(process.resourcesPath, 'LISEZ-MOI.html')
    : path.join(__dirname, '..', '..', 'LISEZ-MOI.html');
  shell.openPath(guide).then((err) => {
    if (err) log('mode d’emploi introuvable :', err);
  });
}

const SPOKEN_LANGUAGES = [
  ['auto', 'Détection automatique'],
  ['fr', 'Français'],
  ['en', 'English'],
  ['de', 'Deutsch'],
  ['es', 'Español'],
  ['it', 'Italiano'],
  ['pt', 'Português']
];

function updateTray() {
  if (!tray) return;
  const labels = {
    idle: 'Souffle — prêt',
    recording: 'Souffle — écoute…',
    working: 'Souffle — transcription…',
    done: 'Souffle — inséré',
    error: 'Souffle — erreur'
  };
  tray.setToolTip(labels[state] || 'Souffle');

  const s = store.get('stats');
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: state === 'recording' ? 'Arrêter la dictée' : 'Dicter maintenant', click: toggleRecording },
      { type: 'separator' },
      { label: `Raccourci : ${store.get('shortcut')}`, enabled: false },
      {
        label: `${s.words.toLocaleString('fr-FR')} mots dictés · ~${Math.round(
          (s.words / 45 - s.seconds / 60) || 0
        )} min gagnées`,
        enabled: false
      },
      { type: 'separator' },
      {
        // Whisper ne détecte qu'une seule langue par enregistrement. Forcer la
        // bonne avant de parler vaut mieux que d'espérer qu'il devine ; ce menu
        // rend le changement instantané, sans ouvrir les réglages.
        label: 'Langue parlée',
        submenu: SPOKEN_LANGUAGES.map(([code, label]) => ({
          label,
          type: 'radio',
          checked: store.get('language') === code,
          click: () => {
            store.set({ language: code });
            log(`langue parlée → ${code}`);
            updateTray();
            refreshSettingsWindow();
          }
        }))
      },
      {
        label: 'Mode direct',
        type: 'checkbox',
        checked: store.get('liveMode'),
        click: (item) => {
          store.set({ liveMode: item.checked });
          updateTray();
          refreshSettingsWindow();
        }
      },
      {
        label: 'Mode confidentialité',
        type: 'checkbox',
        checked: store.get('privacyMode'),
        click: (item) => {
          store.set({ privacyMode: item.checked });
          updateTray();
          refreshSettingsWindow();
        }
      },
      { label: 'Réglages…', accelerator: 'CommandOrControl+,', click: () => openSettings() },
      { label: 'Historique…', click: () => openSettings('history') },
      { label: 'Mode d’emploi', click: openGuide },
      { type: 'separator' },
      { label: 'Quitter Souffle', click: () => app.quit() }
    ])
  );
}

/* ------------------------------------------------------------------ */
/* Cycle de vie                                                        */
/* ------------------------------------------------------------------ */

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => openSettings());

  app.whenReady().then(() => {
    store = new Store();
    history = new History();

    try {
      uiohook = require('uiohook-napi');
    } catch {
      uiohook = null; // dépendance optionnelle absente : mode bascule uniquement
    }

    if (isMac) app.dock?.hide();

    createRecorderWindow();
    createOverlayWindow();

    tray = new Tray(trayImage());
    updateTray();
    tray.on('click', () => (isMac ? null : openSettings()));

    registerShortcut();
    setupHoldMode();

    // macOS refuse cet appel à un binaire non signé : inutile de le tenter à vide.
    if (store.get('launchAtLogin')) {
      try {
        app.setLoginItemSettings({ openAtLogin: true, openAsHidden: true });
      } catch (err) {
        log('lancement au démarrage indisponible :', err.message);
      }
    }

    log(`prêt — ${app.getVersion()} · ${process.platform} · Electron ${process.versions.electron}`);
    log(`micro = ${isMac ? systemPreferences.getMediaAccessStatus('microphone') : 'n/a'} · accessibilité = ${inject.accessibilityGranted()}`);
    if (DIAG) log('MODE DIAGNOSTIC : fenêtre de capture visible, DevTools ouverts');

    if (!store.get('onboarded')) {
      openSettings('general');
      store.set({ onboarded: true });
    }

    app.on('activate', () => openSettings());
  });

  process.on('uncaughtException', onFatal);
  process.on('unhandledRejection', onFatal);

  // App de barre de menus : fermer la fenêtre de réglages ne doit pas quitter.
  app.on('window-all-closed', () => {});
  app.on('before-quit', () => {
    app.isQuitting = true;
  });
  app.on('will-quit', () => {
    globalShortcut.unregisterAll();
    try {
      uiohook?.uIOhook.stop();
    } catch {
      /* déjà arrêté */
    }
  });
}
