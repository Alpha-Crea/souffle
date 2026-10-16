'use strict';

const pill = document.getElementById('pill');
const label = document.getElementById('label');
const hint = document.getElementById('hint');
const timer = document.getElementById('timer');
const bars = [...document.querySelectorAll('#bars span')];

let tick = null;
let audioCtx = null;
let lastState = null;
let retryable = false;

/* Chrono de parole : il s'arrête pendant la pause et reprend où il en était. */
let elapsedMs = 0;
let runStart = 0;

/* ---------- sons générés (aucun fichier à embarquer) ---------- */

function beep(freq, duration = 0.09, gain = 0.045) {
  try {
    audioCtx = audioCtx || new AudioContext();
    const osc = audioCtx.createOscillator();
    const vol = audioCtx.createGain();
    osc.type = 'sine';
    osc.frequency.value = freq;
    vol.gain.setValueAtTime(0, audioCtx.currentTime);
    vol.gain.linearRampToValueAtTime(gain, audioCtx.currentTime + 0.012);
    vol.gain.exponentialRampToValueAtTime(0.0001, audioCtx.currentTime + duration);
    osc.connect(vol).connect(audioCtx.destination);
    osc.start();
    osc.stop(audioCtx.currentTime + duration + 0.02);
  } catch {
    /* pas de sortie audio : sans conséquence */
  }
}

/* ---------- gestes sur la pilule ----------
 * Clic : pause / reprise pendant la dictée, nouvel essai après un échec.
 * Clic droit : annuler. Glisser : déplacer la pilule, où qu'elle soit.
 * Un clic et un glisser se distinguent par la distance parcourue. */

const DRAG_THRESHOLD = 4;
let press = null;

pill.addEventListener('pointerdown', (e) => {
  if (e.button !== 0) return;
  pill.setPointerCapture(e.pointerId);
  press = { id: e.pointerId, x: e.screenX, y: e.screenY, dragging: false };
});

pill.addEventListener('pointermove', (e) => {
  if (!press || e.pointerId !== press.id) return;
  if (!press.dragging) {
    if (Math.abs(e.screenX - press.x) + Math.abs(e.screenY - press.y) < DRAG_THRESHOLD) return;
    press.dragging = true;
    pill.classList.add('dragging');
    window.souffle.dragStart({ screenX: press.x, screenY: press.y });
  }
  window.souffle.dragMove({ screenX: e.screenX, screenY: e.screenY });
});

function release(e, cancelled) {
  if (!press || e.pointerId !== press.id) return;
  const wasDrag = press.dragging;
  press = null;
  pill.classList.remove('dragging');
  if (wasDrag) {
    window.souffle.dragEnd();
    return;
  }
  if (cancelled) return;
  const state = pill.dataset.state;
  if (state === 'recording' || state === 'paused') window.souffle.togglePause();
  else if (state === 'error' && retryable) window.souffle.retry();
}
pill.addEventListener('pointerup', (e) => release(e, false));
pill.addEventListener('pointercancel', (e) => release(e, true));

pill.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const state = pill.dataset.state;
  if (state === 'recording' || state === 'paused') window.souffle.cancel();
});

/* ---------- rendu ---------- */

const COPY = {
  recording: ['Écoute…', 'Cliquez pour mettre en pause'],
  paused: ['En pause', 'Cliquez pour reprendre'],
  working: ['Transcription…', 'Un instant'],
  done: ['Inséré', ''],
  error: ['Erreur', '']
};

function showTime() {
  const ms = elapsedMs + (runStart ? Date.now() - runStart : 0);
  const s = Math.floor(ms / 1000);
  timer.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

function render({ state, message, step, words, sounds, pasted, live, segments, retry }) {
  pill.dataset.state = state;
  retryable = state === 'error' && Boolean(retry);
  pill.dataset.retry = retryable ? '1' : '0';

  const [l, h] = COPY[state] || ['', ''];
  label.textContent =
    state === 'error' ? message || l
    : state === 'done' && pasted === false ? 'Copié'
    : state === 'recording' && live ? 'Direct'
    : l;
  hint.textContent =
    state === 'working' && step ? step
    : state === 'done' && words ? `${words} mot${words > 1 ? 's' : ''}`
    : state === 'error' ? (retryable ? 'Cliquez pour réessayer' : 'Réglages de Souffle')
    : state === 'recording' && live && segments
      ? `${segments} phrase${segments > 1 ? 's' : ''} écrite${segments > 1 ? 's' : ''}`
      : h;

  // En mode direct, l'état « recording » est réémis à chaque phrase écrite :
  // le chrono et le bip ne repartent qu'au vrai début de la dictée.
  const entering = state !== lastState;
  const previous = lastState;
  lastState = state;
  if (!entering) return;

  if (state === 'recording') {
    if (previous !== 'paused') {
      elapsedMs = 0;
      if (sounds) beep(660);
    }
    runStart = Date.now();
    clearInterval(tick);
    tick = setInterval(showTime, 250);
    showTime();
    return;
  }

  if (state === 'paused') {
    if (runStart) elapsedMs += Date.now() - runStart;
    runStart = 0;
    clearInterval(tick);
    tick = null;
    showTime();
    bars.forEach((b) => (b.style.height = '6px'));
    return;
  }

  clearInterval(tick);
  tick = null;
  runStart = 0;
  bars.forEach((b) => (b.style.height = '6px'));
  if (state === 'done' && sounds) beep(880, 0.08);
  if (state === 'error' && sounds) beep(220, 0.16);
}

/* Chaque barre réagit avec un décalage : ça se lit comme une vraie onde. */
const WEIGHTS = [0.55, 0.85, 1, 0.85, 0.55];

function renderLevel(level) {
  if (pill.dataset.state !== 'recording') return;
  bars.forEach((bar, i) => {
    const h = 6 + level * WEIGHTS[i] * 20 + Math.random() * 2.5;
    bar.style.height = `${Math.min(26, h).toFixed(1)}px`;
  });
}

window.souffle.onState(render);
window.souffle.onLevel(renderLevel);
