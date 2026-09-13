'use strict';
/**
 * Fenêtre invisible dédiée à la capture micro.
 *
 * Deux régimes :
 *   - classique : un seul enregistrement, envoyé quand l'utilisateur arrête ;
 *   - direct    : on découpe sur les silences et on envoie chaque phrase dès
 *                 qu'elle est finie, pour que le texte s'écrive pendant qu'on parle.
 *
 * Le flux micro reste ouvert entre deux dictées : le rouvrir coûte 200 à 500 ms
 * et fait clignoter l'indicateur micro du système. Il est tout de même relâché
 * après une minute d'inactivité, pour ne pas confisquer le micro aux autres
 * applications — et pour ne pas hériter d'un flux périmé au réveil.
 *
 * Tout ce qui est logué en « [rec] » remonte dans le terminal.
 */

let stream = null;
let recorder = null;
let chunks = [];
let audioCtx = null;
let analyser = null;
let levelTimer = null;

let live = false;
let cancelled = false;
let stopping = false; // arrêt définitif demandé par l'utilisateur
let segmentStartedAt = 0;

/* Détection de fin de phrase */
const SILENCE_RMS = 0.012; // en dessous, on considère qu'il n'y a pas de voix
const SILENCE_TO_CUT_MS = 700; // durée de blanc qui clôt une phrase
const MIN_SPEECH_MS = 600; // en deçà, ce n'est pas une phrase mais un bruit
const MAX_SEGMENT_MS = 15000; // on coupe de force pour ne pas accumuler
let speechMs = 0;
let silenceMs = 0;

const log = (...a) => console.log('[rec]', ...a);

const MIME = [
  'audio/webm;codecs=opus',
  'audio/webm',
  'audio/ogg;codecs=opus',
  'audio/mp4'
].find((t) => window.MediaRecorder && MediaRecorder.isTypeSupported(t)) || '';

log('format retenu =', MIME || 'défaut du navigateur');

/* ------------------------------------------------------------------ */
/* Flux micro                                                          */
/* ------------------------------------------------------------------ */

/**
 * Ouvrir le micro n'est pas fiable dès qu'une autre application l'utilise
 * (ChatGPT, Teams, Zoom, un assistant vocal…). Trois choses peuvent arriver :
 *   - getUserMedia rejette (NotReadableError / AbortError) ;
 *   - getUserMedia ne répond JAMAIS — le pire cas, l'application reste figée ;
 *   - le périphérique d'entrée par défaut a changé et celui d'avant est mort.
 * On traite les trois : chaque tentative est bornée dans le temps, et on
 * descend une liste de replis jusqu'à obtenir un flux réellement vivant.
 */
const GUM_TIMEOUT_MS = 1800; // au-delà, le périphérique ne répond pas
const RETRY_PAUSE_MS = 150;
const IDLE_RELEASE_MS = 60000; // on rend le micro au système après une minute
const LAST_DEVICE_KEY = 'souffle.dernierMicro';

const PREFERRED = {
  channelCount: 1,
  echoCancellation: true,
  noiseSuppression: true,
  autoGainControl: true
};

let idleRelease = null;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function rememberDevice(id) {
  try {
    if (id) localStorage.setItem(LAST_DEVICE_KEY, id);
  } catch {
    /* stockage indisponible : sans conséquence */
  }
}

function lastDevice() {
  try {
    return localStorage.getItem(LAST_DEVICE_KEY) || '';
  } catch {
    return '';
  }
}

/** Un flux gardé ouvert peut être mort sans que `active` le dise. */
function streamUsable() {
  if (!stream || !stream.active) return false;
  const track = stream.getAudioTracks()[0];
  return Boolean(track && track.readyState === 'live' && !track.muted);
}

/**
 * Une dictée est-elle en cours ? En mode direct il y a un court instant entre
 * deux phrases où le MediaRecorder est arrêté : le compteur de niveau, lui,
 * tourne du début à la fin. C'est donc lui qui fait foi.
 */
function busy() {
  return levelTimer !== null || Boolean(recorder && recorder.state === 'recording');
}

/** Libère micro et contexte audio : indispensable avant de retenter autrement. */
function releaseStream(why) {
  if (stream) {
    for (const track of stream.getTracks()) {
      try {
        track.stop();
      } catch {
        /* déjà arrêtée */
      }
    }
    log('micro relâché', why ? `(${why})` : '');
  }
  stream = null;
  analyser = null;
  if (audioCtx) {
    const ctx = audioCtx;
    audioCtx = null;
    ctx.close().catch(() => {});
  }
}

function scheduleRelease() {
  clearTimeout(idleRelease);
  idleRelease = setTimeout(() => {
    if (busy()) return;
    // Garder le micro ouvert indéfiniment empêcherait les autres applications
    // de s'en servir — et nous vaudrait le même blocage en retour.
    releaseStream('inactivité');
  }, IDLE_RELEASE_MS);
}

/** getUserMedia borné : un périphérique muet ne doit pas figer la dictée. */
function askMedia(constraints) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      settled = true;
      const err = new Error('le périphérique ne répond pas');
      err.name = 'TimeoutError';
      reject(err);
    }, GUM_TIMEOUT_MS);

    navigator.mediaDevices.getUserMedia(constraints).then(
      (s) => {
        clearTimeout(timer);
        // Réponse après l'abandon : on ne garde surtout pas un micro fantôme ouvert.
        if (settled) {
          for (const t of s.getTracks()) t.stop();
          return;
        }
        settled = true;
        resolve(s);
      },
      (err) => {
        clearTimeout(timer);
        if (settled) return;
        settled = true;
        reject(err);
      }
    );
  });
}

/** Liste ordonnée des tentatives : du confort au dernier recours. */
async function attempts() {
  const list = [];
  const known = lastDevice();

  if (known) {
    list.push({
      label: 'micro mémorisé',
      constraints: { audio: { ...PREFERRED, deviceId: { exact: known } } }
    });
  }
  list.push({ label: 'micro par défaut', constraints: { audio: { ...PREFERRED } } });
  // Sans traitement : certains pilotes refusent le mode mono + réduction de bruit
  // quand une autre application tient déjà le périphérique.
  list.push({ label: 'micro par défaut sans traitement', constraints: { audio: true } });

  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    for (const d of devices) {
      if (d.kind !== 'audioinput' || !d.deviceId) continue;
      if (d.deviceId === 'default' || d.deviceId === known) continue;
      list.push({
        label: `entrée « ${d.label || d.deviceId.slice(0, 8)} »`,
        constraints: { audio: { deviceId: { exact: d.deviceId } } }
      });
    }
  } catch (err) {
    log('énumération des périphériques impossible :', err.name);
  }

  return list;
}

function micErrorMessage(err) {
  switch (err && err.name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return 'Micro refusé par le système';
    case 'NotFoundError':
    case 'OverconstrainedError':
      return 'Aucun micro détecté';
    case 'NotReadableError':
    case 'TrackStartError':
    case 'AbortError':
    case 'TimeoutError':
      return 'Micro pris par une autre app';
    default:
      return `Micro indisponible (${(err && err.name) || 'inconnu'})`;
  }
}

async function ensureStream() {
  clearTimeout(idleRelease);

  if (streamUsable()) {
    log('flux déjà ouvert');
    return stream;
  }
  if (stream) releaseStream('flux périmé');

  let lastErr = null;
  const plans = await attempts();

  for (const plan of plans) {
    // Chaque essai prévient le process principal : sans ça, son chien de garde
    // conclurait à une panne alors qu'on est simplement en train de réessayer.
    window.souffle.sendAcquiring();
    log(`appel getUserMedia — ${plan.label}`);
    try {
      const s = await askMedia(plan.constraints);
      const track = s.getAudioTracks()[0];
      if (!track || track.readyState !== 'live') {
        for (const t of s.getTracks()) t.stop();
        throw Object.assign(new Error('piste morte à l’ouverture'), { name: 'NotReadableError' });
      }
      stream = s;
      rememberDevice(track.getSettings().deviceId || '');
      log('flux obtenu :', track.label || '(micro sans nom)');

      track.addEventListener('ended', () => {
        log('la piste micro a été coupée par le système');
        if (!busy()) releaseStream('piste terminée');
        else stream = null;
      });
      track.addEventListener('mute', () => log('piste micro coupée (mute) par une autre application'));
      track.addEventListener('unmute', () => log('piste micro rétablie'));

      audioCtx = new AudioContext();
      const src = audioCtx.createMediaStreamSource(stream);
      analyser = audioCtx.createAnalyser();
      analyser.fftSize = 512;
      analyser.smoothingTimeConstant = 0.75;
      src.connect(analyser);
      return stream;
    } catch (err) {
      lastErr = err;
      log(`échec (${plan.label}) :`, err.name, '—', err.message);
      releaseStream();
      // Une autorisation refusée ne sera pas accordée par le périphérique suivant.
      if (err.name === 'NotAllowedError' || err.name === 'SecurityError') break;
      await sleep(RETRY_PAUSE_MS);
    }
  }

  throw lastErr || Object.assign(new Error('aucune entrée audio'), { name: 'NotFoundError' });
}

// Brancher un casque ou installer une application audio réordonne les entrées :
// le flux mémorisé peut alors pointer vers un périphérique qui n'existe plus.
navigator.mediaDevices?.addEventListener?.('devicechange', () => {
  log('liste des périphériques audio modifiée');
  if (!busy()) releaseStream('changement de périphérique');
});

/* ------------------------------------------------------------------ */
/* Niveau sonore + découpage sur les silences                          */
/* ------------------------------------------------------------------ */

const TICK_MS = 60;

function startLevelMeter() {
  const buf = new Uint8Array(analyser.frequencyBinCount);
  let quiet = 0;

  levelTimer = setInterval(() => {
    analyser.getByteTimeDomainData(buf);
    let sum = 0;
    for (let i = 0; i < buf.length; i++) {
      const v = (buf[i] - 128) / 128;
      sum += v * v;
    }
    const rms = Math.sqrt(sum / buf.length);

    if (rms < 0.002) quiet++;
    else quiet = 0;
    if (quiet === 50) log('attention : 3 s de silence absolu, vérifiez l’entrée micro');

    // Compression douce : la voix normale remplit la barre sans saturer.
    window.souffle.sendLevel(Math.min(1, Math.pow(rms * 4.2, 0.7)));

    if (!live || stopping || cancelled) return;

    if (rms >= SILENCE_RMS) {
      speechMs += TICK_MS;
      silenceMs = 0;
    } else if (speechMs > 0) {
      silenceMs += TICK_MS;
    }

    const elapsed = Date.now() - segmentStartedAt;
    const endOfSentence = speechMs >= MIN_SPEECH_MS && silenceMs >= SILENCE_TO_CUT_MS;
    const tooLong = elapsed >= MAX_SEGMENT_MS && speechMs >= MIN_SPEECH_MS;

    if (endOfSentence || tooLong) {
      log(`fin de phrase détectée (${speechMs} ms de voix, ${silenceMs} ms de blanc)`);
      cutSegment();
    }
  }, TICK_MS);
}

function stopLevelMeter() {
  clearInterval(levelTimer);
  levelTimer = null;
}

/* ------------------------------------------------------------------ */
/* Enregistrement                                                      */
/* ------------------------------------------------------------------ */

function beginSegment() {
  chunks = [];
  speechMs = 0;
  silenceMs = 0;
  segmentStartedAt = Date.now();

  recorder = new MediaRecorder(
    stream,
    MIME ? { mimeType: MIME, audioBitsPerSecond: 64000 } : undefined
  );
  recorder.ondataavailable = (e) => e.data.size && chunks.push(e.data);
  recorder.onstop = handleSegmentEnd;
  recorder.onerror = (e) => {
    log('MediaRecorder en erreur :', e.error?.name);
    window.souffle.sendError(`Enregistrement interrompu (${e.error?.name || 'inconnu'})`);
  };
  recorder.start();
}

/** Clôt la phrase en cours ; une nouvelle démarre aussitôt derrière. */
function cutSegment() {
  if (recorder && recorder.state === 'recording') recorder.stop();
}

async function handleSegmentEnd() {
  const durationMs = Date.now() - segmentStartedAt;
  const parts = chunks;
  // On lit le type MAINTENANT : beginSegment() va remplacer `recorder` juste après.
  const mime = recorder.mimeType || 'audio/webm';
  chunks = [];

  if (cancelled) {
    log('annulé, audio jeté');
    return;
  }

  const blob = new Blob(parts, { type: mime });
  const isFinal = !live || stopping;
  log(`${isFinal ? 'enregistrement terminé' : 'phrase envoyée'} : ${blob.size} octets en ${durationMs} ms`);

  // En mode direct, on relance la capture AVANT l'envoi réseau : le blanc
  // entre deux phrases doit être aussi court que possible.
  if (live && !stopping) beginSegment();

  const buffer = new Uint8Array(await blob.arrayBuffer());
  window.souffle.sendAudio({
    buffer,
    mime,
    durationMs,
    segment: live,
    final: isFinal
  });
}

async function start(opts = {}) {
  try {
    cancelled = false;
    stopping = false;
    live = Boolean(opts.live);
    chunks = [];

    await ensureStream();
    if (audioCtx.state === 'suspended') await audioCtx.resume();

    beginSegment();
    startLevelMeter();
    log(`enregistrement démarré${live ? ' (mode direct)' : ''}`);
    window.souffle.sendStarted();
  } catch (err) {
    log('échec du démarrage :', err.name, err.message);
    releaseStream('échec du démarrage');
    window.souffle.sendError(micErrorMessage(err));
  }
}

function stop() {
  stopping = true;
  stopLevelMeter();
  scheduleRelease();
  if (recorder && recorder.state === 'recording') {
    recorder.stop();
  } else {
    log('arrêt demandé mais aucun enregistrement en cours (état =', recorder?.state || 'néant', ')');
    window.souffle.sendError('La capture n’avait pas démarré');
  }
}

function cancel() {
  cancelled = true;
  stopping = true;
  stopLevelMeter();
  scheduleRelease();
  if (recorder && recorder.state === 'recording') recorder.stop();
  chunks = [];
}

window.souffle.onStart(start);
window.souffle.onStop(stop);
window.souffle.onCancel(cancel);
log('fenêtre de capture prête');
